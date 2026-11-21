//! Asset groups: assets declared together and loaded in the background,
//! with progress, failure and cancel, behind a loading state. Headless,
//! with slow fake sources standing in for a slow disk or network.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use ponko::assets::{AssetGroup, AssetSource, GroupState, MemorySource, PackSource};
use ponko::prelude::*;
use ponko::streaming::ResidencyKind;
use serde::{Deserialize, Serialize};

fn game() -> Game {
    Game::new("asset groups", 1, 1).hot_reload(false)
}

/// How long a test waits for work on other threads before calling it hung.
/// Only a hang uses it up (each wait ends as soon as the work is done), so
/// it can be long enough for a machine that stalls threads for seconds.
const PATIENCE: Duration = Duration::from_secs(60);

/// A source that takes `delay` to read each file, counting reads; it may
/// be read on any thread.
#[derive(Clone)]
struct Slow {
    id: &'static str,
    files: MemorySource,
    delay: Duration,
    /// Reads begun.
    started: Arc<AtomicU32>,
    /// Reads done.
    reads: Arc<AtomicU32>,
    /// Reads made on the thread that made the source (the game thread).
    here: Arc<AtomicU32>,
    made_on: ThreadId,
    /// When set, each read waits until it is opened (`Slow::open`).
    gate: Option<Arc<(Mutex<bool>, Condvar)>>,
}

impl Slow {
    fn new(id: &'static str, delay: Duration, files: &[(&str, Vec<u8>)]) -> Self {
        let mut memory = MemorySource::new(id);
        for (name, bytes) in files {
            memory.insert(*name, bytes.clone());
        }
        Self {
            id,
            files: memory,
            delay,
            started: Arc::new(AtomicU32::new(0)),
            reads: Arc::new(AtomicU32::new(0)),
            here: Arc::new(AtomicU32::new(0)),
            made_on: std::thread::current().id(),
            gate: None,
        }
    }

    /// Reads wait for [`open`](Self::open) (for at most 10 s: a read on the
    /// game thread would otherwise wait for itself).
    fn gated(mut self) -> Self {
        self.gate = Some(Arc::new((Mutex::new(false), Condvar::new())));
        self
    }

    /// Lets every read through, waiting or to come.
    fn open(&self) {
        let (open, wake) = &**self.gate.as_ref().expect("a gated source");
        *open.lock().unwrap() = true;
        wake.notify_all();
    }
}

impl AssetSource for Slow {
    fn id(&self) -> &str {
        self.id
    }

    fn contains(&self, name: &str) -> bool {
        self.files.contains(name)
    }

    fn size(&self, name: &str) -> Option<u64> {
        self.files.size(name)
    }

    fn read(&self, name: &str) -> std::io::Result<Vec<u8>> {
        if self.files.contains(name) {
            self.started.fetch_add(1, Ordering::SeqCst);
            if std::thread::current().id() == self.made_on {
                self.here.fetch_add(1, Ordering::SeqCst);
            }
            if let Some(gate) = &self.gate {
                let (open, wake) = &**gate;
                let open = open.lock().unwrap();
                drop(
                    wake.wait_timeout_while(open, Duration::from_secs(10), |open| !*open)
                        .unwrap(),
                );
            }
            std::thread::sleep(self.delay);
            self.reads.fetch_add(1, Ordering::SeqCst);
        }
        self.files.read(name)
    }
}

fn png(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::from_pixel(width, height, image::Rgba(rgba))
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    bytes
}

/// A mono 16-bit WAV of `samples` samples at 8 kHz.
fn wav(samples: u32) -> Vec<u8> {
    let data = samples * 2;
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    for field in [
        16u32.to_le_bytes(),
        [1, 0, 1, 0],
        8000u32.to_le_bytes(),
        16_000u32.to_le_bytes(),
        [2, 0, 16, 0],
    ] {
        out.extend_from_slice(&field);
    }
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for i in 0..samples {
        out.extend_from_slice(&(((i % 40) as i16 - 20) * 400).to_le_bytes());
    }
    out
}

/// Runs the engine's per-frame group step until `id` stops loading; the
/// frames it took. Values decode on workers, so this waits for them (for
/// at most [`PATIENCE`]) rather than for a number of frames.
fn frames_until_done(ctx: &mut Ctx, id: ponko::assets::GroupId) -> u32 {
    let deadline = Instant::now() + PATIENCE;
    for frame in 1.. {
        ctx.pump_asset_groups();
        if ctx.assets.group_state(id) != GroupState::Loading {
            return frame;
        }
        assert!(
            Instant::now() < deadline,
            "the group was still loading after {frame} frames"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    unreachable!()
}

#[test]
fn a_slow_background_source_never_holds_up_a_frame() {
    let files: Vec<(String, Vec<u8>)> = (0..8).map(|i| (format!("chunk{i}.bin"), vec![i as u8; 1000])).collect();
    let refs: Vec<(&str, Vec<u8>)> = files.iter().map(|(n, b)| (n.as_str(), b.clone())).collect();
    // Every read waits until the test lets it through: a frame step that
    // waited for one would never end, so steps that return while all of
    // them wait show that no frame waits for a read, whatever the machine.
    let slow = Slow::new("network", Duration::ZERO, &refs).gated();
    let (gate, reads, here) = (slow.clone(), slow.reads.clone(), slow.here.clone());
    game().run_headless(0, move |ctx| {
        ctx.assets.add_background_source(slow);
        let mut group = AssetGroup::new("chunks");
        for (name, _) in &files {
            group = group.bytes(name);
        }
        let id = ctx.assets.start_group(group);
        for _ in 0..3 {
            ctx.pump_asset_groups();
        }
        assert_eq!(ctx.assets.group_state(id), GroupState::Loading);
        assert_eq!(reads.load(Ordering::SeqCst), 0, "every read is still held");
        gate.open();
        let frames = frames_until_done(ctx, id);
        assert_eq!(ctx.assets.group_state(id), GroupState::Ready);
        assert_eq!(reads.load(Ordering::SeqCst), 8);
        assert_eq!(here.load(Ordering::SeqCst), 0, "no read ran on the game thread");
        assert!(frames > 1, "the values were taken a frame after their reads");
        for (name, bytes) in &files {
            assert_eq!(ctx.assets.take_group_value::<Vec<u8>>(id, name).as_ref(), Some(bytes));
        }
    });
}

#[test]
fn a_slow_game_thread_source_is_read_within_the_frame_budget() {
    let files: Vec<(String, Vec<u8>)> = (0..6).map(|i| (format!("f{i}.bin"), vec![1; 10])).collect();
    let refs: Vec<(&str, Vec<u8>)> = files.iter().map(|(n, b)| (n.as_str(), b.clone())).collect();
    // A source only the game thread may read: 5 ms a file.
    let slow = Slow::new("disk", Duration::from_millis(5), &refs);
    let reads = slow.reads.clone();
    game().run_headless(0, move |ctx| {
        ctx.assets.add_source(slow);
        ctx.assets.set_group_frame_budget(1.0);
        let mut group = AssetGroup::new("files");
        for (name, _) in &files {
            group = group.bytes(name);
        }
        let id = ctx.assets.start_group(group);
        ctx.pump_asset_groups();
        // One read, then the 1 ms budget is spent: the rest wait.
        assert_eq!(reads.load(Ordering::SeqCst), 1, "one file per frame on the game thread");
        let frames = frames_until_done(ctx, id);
        assert!(frames >= 5, "six reads, one a frame: {frames}");
        assert_eq!(ctx.assets.group_progress(id).loaded, 6);
    });
}

#[test]
fn progress_counts_assets_and_bytes_and_never_goes_back() {
    let sizes = [100usize, 300, 600];
    game().run_headless(0, move |ctx| {
        let mut source = MemorySource::new("mem");
        for (i, size) in sizes.iter().enumerate() {
            source.insert(format!("{i}.bin"), vec![0; *size]);
        }
        ctx.assets.add_background_source(source);
        let id = ctx
            .assets
            .start_group(AssetGroup::new("three").bytes("0.bin").bytes("1.bin").bytes("2.bin"));
        let start = ctx.assets.group_progress(id);
        assert_eq!((start.loaded, start.total, start.bytes_loaded), (0, 3, 0));
        assert_eq!(start.fraction(), 0.0);
        let mut last = 0.0;
        loop {
            ctx.pump_asset_groups();
            let progress = ctx.assets.group_progress(id);
            assert!(progress.fraction() >= last, "progress went back");
            last = progress.fraction();
            if progress.state == GroupState::Ready {
                break;
            }
            std::thread::yield_now();
        }
        let done = ctx.assets.group_progress(id);
        assert!(done.bytes_known, "sizes of memory files are known");
        assert_eq!(
            (done.loaded, done.bytes_read, done.bytes_total, done.bytes_loaded),
            (3, 1000, 1000, 1000)
        );
        assert_eq!(done.fraction(), 1.0);
    });
}

#[test]
fn sizes_are_known_before_reading_when_every_source_is_a_background_one() {
    let slow = Slow::new(
        "slow",
        Duration::from_millis(40),
        &[("a.bin", vec![0; 64]), ("b.bin", vec![0; 36])],
    );
    game().run_headless(0, move |ctx| {
        ctx.assets.add_background_source(slow);
        let id = ctx
            .assets
            .start_group(AssetGroup::new("two").bytes("a.bin").bytes("b.bin"));
        // The sizes come from a worker; the reads take longer.
        let deadline = Instant::now() + PATIENCE;
        while !ctx.assets.group_progress(id).bytes_known {
            assert!(Instant::now() < deadline, "the sizes never came");
            ctx.pump_asset_groups();
            std::thread::yield_now();
        }
        let progress = ctx.assets.group_progress(id);
        assert_eq!(progress.bytes_total, 100);
        ctx.finish_uploads();
        assert_eq!(ctx.assets.group_progress(id).bytes_loaded, 100);
    });
}

#[test]
fn a_missing_asset_fails_the_group_naming_its_path() {
    game().run_headless(0, |ctx| {
        ctx.assets
            .add_background_source(MemorySource::new("mem").with("there.bin", "x"));
        let id = ctx
            .assets
            .start_group(AssetGroup::new("level").bytes("there.bin").bytes("sprites/missing.png"));
        ctx.finish_uploads();
        assert_eq!(ctx.assets.group_state(id), GroupState::Failed);
        let error = ctx.assets.group_error(id).expect("failed groups say why");
        assert_eq!(error.kind(), ErrorKind::NotFound);
        assert_eq!(error.path(), Some("sprites/missing.png"));
        assert!(error.message().contains("sprites/missing.png"), "{error}");
        // Cancelling a failed group drops what it kept for a restart.
        ctx.assets.cancel_group(id);
        assert_eq!(ctx.assets.group_state(id), GroupState::Cancelled);
        assert!(ctx.assets.group_error(id).is_none(), "only failed groups have an error");
    });
}

#[test]
fn a_decoder_error_fails_the_group_with_its_kind_and_the_asset_path() {
    game().run_headless(0, |ctx| {
        ctx.assets
            .add_background_source(MemorySource::new("mem").with("rules.json", "{ not json"));
        let group = AssetGroup::new("rules").with("rules.json", |bytes| {
            serde_json::from_slice::<serde_json::Value>(bytes).map_err(|e| Error::new(ErrorKind::Json, e.to_string()))
        });
        let id = ctx.assets.start_group(group);
        ctx.finish_uploads();
        let error = ctx.assets.group_error(id).unwrap();
        assert_eq!((error.kind(), error.path()), (ErrorKind::Json, Some("rules.json")));
        assert!(error.message().contains("could not be decoded"), "{error}");
    });
}

#[test]
fn a_broken_image_fails_the_group_with_a_decode_error() {
    game().run_headless(0, |ctx| {
        ctx.assets
            .add_background_source(MemorySource::new("mem").with("bad.png", "not an image"));
        let id = ctx.assets.start_group(AssetGroup::new("bad").texture("bad.png"));
        ctx.finish_uploads();
        let error = ctx.assets.group_error(id).unwrap();
        assert_eq!((error.kind(), error.path()), (ErrorKind::Decode, Some("bad.png")));
    });
}

#[test]
fn cancel_stops_reading_the_rest_of_the_group() {
    let files: Vec<(String, Vec<u8>)> = (0..40).map(|i| (format!("{i}.bin"), vec![0; 10])).collect();
    let refs: Vec<(&str, Vec<u8>)> = files.iter().map(|(n, b)| (n.as_str(), b.clone())).collect();
    let slow = Slow::new("slow", Duration::from_millis(20), &refs);
    let (started, reads) = (slow.started.clone(), slow.reads.clone());
    // The frame before the cancel starts at most one read per core (and at
    // least four): every read that ever happens is one of those.
    let first_frame = std::thread::available_parallelism().map_or(4, |n| n.get().max(4)) as u32;
    game().run_headless(0, move |ctx| {
        ctx.assets.add_background_source(slow);
        let mut group = AssetGroup::new("many");
        for (name, _) in &files {
            group = group.bytes(name);
        }
        let id = ctx.assets.start_group(group);
        ctx.pump_asset_groups();
        ctx.assets.cancel_group(id);
        assert_eq!(ctx.assets.group_state(id), GroupState::Cancelled);
        // Frames go on; a group that kept loading would start the rest here.
        for _ in 0..50 {
            ctx.pump_asset_groups();
            std::thread::sleep(Duration::from_millis(2));
        }
        // Reads already running finish (waited for, not timed); nothing new
        // starts.
        let deadline = Instant::now() + PATIENCE;
        while started.load(Ordering::SeqCst) != reads.load(Ordering::SeqCst) {
            assert!(Instant::now() < deadline, "the running reads never finished");
            std::thread::sleep(Duration::from_millis(2));
        }
        let after = reads.load(Ordering::SeqCst);
        assert!(after < 40, "{after} of 40 files read after cancelling");
        assert!(after <= first_frame, "{after} reads, more than the first frame started");
        assert_eq!(ctx.assets.take_group_value::<Vec<u8>>(id, "39.bin"), None);
        for _ in 0..5 {
            ctx.pump_asset_groups();
        }
        assert_eq!(ctx.assets.group_state(id), GroupState::Cancelled, "it stays cancelled");
        assert!(
            reads.load(Ordering::SeqCst) <= first_frame,
            "nothing was started after the cancel"
        );
    });
}

#[test]
fn a_group_texture_is_what_load_texture_returns_without_reading_again() {
    let slow = Slow::new("art", Duration::ZERO, &[("crab.png", png(8, 4, [200, 50, 50, 255]))]);
    let reads = slow.reads.clone();
    game().run_headless(0, move |ctx| {
        ctx.assets.add_background_source(slow);
        let id = ctx.assets.start_group(AssetGroup::new("art").texture("crab.png"));
        ctx.finish_uploads();
        assert_eq!(ctx.assets.group_state(id), GroupState::Ready);
        let crab = ctx.gfx.load_texture("crab.png");
        assert_eq!((crab.width(), crab.height()), (8, 4));
        assert_eq!(ctx.gfx.load_texture("crab.png"), crab);
        assert_eq!(reads.load(Ordering::SeqCst), 1, "load_texture found it loaded");
    });
}

#[test]
fn group_uploads_stay_within_the_upload_budget_each_frame() {
    // Eight 128 x 128 textures, 64 KiB each, under a 128 KiB budget, in
    // the game's own frames.
    let files: Vec<(String, Vec<u8>)> = (0..8)
        .map(|i| (format!("t{i}.png"), png(128, 128, [i, 0, 0, 255])))
        .collect();
    let names: Vec<String> = files.iter().map(|(n, _)| n.clone()).collect();
    let seen = std::rc::Rc::new(std::cell::RefCell::new((0u64, 0u32, None::<u64>, 0u64)));
    let log = seen.clone();
    game()
        .sim_system(move |ctx: &mut Ctx| {
            let id = ctx.assets.group("textures").unwrap();
            let mut seen = log.borrow_mut();
            let frame = ctx.gfx.upload_stats().frame_bytes;
            seen.0 = seen.0.max(frame);
            if frame > 0 {
                seen.1 += 1;
            }
            if seen.2.is_none() && ctx.assets.group_state(id) == GroupState::Ready {
                seen.2 = Some(ctx.time.sim_tick());
                seen.3 = ctx.gfx.upload_stats().total_bytes;
            }
            drop(seen);
            std::thread::sleep(Duration::from_millis(2));
        })
        .run_headless(400, move |ctx| {
            let mut source = MemorySource::new("mem");
            for (name, bytes) in &files {
                source.insert(name.clone(), bytes.clone());
            }
            ctx.assets.add_background_source(source);
            ctx.gfx.set_upload_budget(Some(128 << 10));
            let group = names.iter().fold(AssetGroup::new("textures"), |g, n| g.texture(n));
            ctx.assets.start_group(group);
        });
    let (most, frames_uploading, ready_at, total) = *seen.borrow();
    assert!(ready_at.is_some(), "the group loaded");
    assert!(most <= 128 << 10, "a frame uploaded {most} bytes");
    assert!(
        frames_uploading >= 4,
        "eight uploads, two a frame: {frames_uploading} frames"
    );
    assert_eq!(total, 8 * (64 << 10), "counted with the other uploads");
}

#[test]
fn streamed_textures_load_under_the_residency_budget() {
    // Four 64 x 64 textures (16 KiB each at full size) under a 40 KiB budget.
    let files: Vec<(String, Vec<u8>)> = (0..4)
        .map(|i| (format!("s{i}.png"), png(64, 64, [0, i, 0, 255])))
        .collect();
    game().run_headless(0, move |ctx| {
        let mut source = MemorySource::new("mem");
        for (name, bytes) in &files {
            source.insert(name.clone(), bytes.clone());
        }
        ctx.assets.add_background_source(source);
        ctx.gfx.set_residency_budget(ResidencyKind::Texture, Some(40 << 10));
        let mut group = AssetGroup::new("streamed");
        for (name, _) in &files {
            group = group.streamed_texture(name);
        }
        let id = ctx.assets.start_group(group);
        ctx.finish_uploads();
        assert_eq!(ctx.assets.group_state(id), GroupState::Ready);
        let stats = ctx.gfx.residency(ResidencyKind::Texture);
        assert!(stats.resident_bytes <= 40 << 10, "{stats:?}");
        for (name, _) in &files {
            let texture = ctx.gfx.stream_texture(name).expect("each has its handle");
            assert_eq!((texture.width(), texture.height()), (64, 64));
        }
    });
}

#[test]
fn sounds_decode_on_workers_and_load_returns_them() {
    let slow = Slow::new("sfx", Duration::ZERO, &[("hit.wav", wav(800))]);
    let reads = slow.reads.clone();
    game().run_headless(0, move |ctx| {
        ctx.assets.add_background_source(slow);
        let id = ctx.assets.start_group(AssetGroup::new("sfx").sound("hit.wav"));
        ctx.finish_uploads();
        assert_eq!(ctx.assets.group_state(id), GroupState::Ready);
        let hit = ctx.audio.load("hit.wav");
        assert_eq!(ctx.audio.load("hit.wav"), hit);
        assert_eq!(reads.load(Ordering::SeqCst), 1, "Audio::load found it loaded");
    });
}

#[test]
fn groups_read_packs_and_count_their_sizes() {
    game().run_headless(0, |ctx| {
        let pack = PackSource::from_bytes("sample", include_bytes!("fixtures/packs/sample.pack").as_slice()).unwrap();
        ctx.assets.add_background_source(pack);
        let id = ctx.assets.start_group(
            AssetGroup::new("from a pack")
                .texture("sprites/dot.png")
                .bytes("notes/long.txt")
                .bytes("rules.json"),
        );
        ctx.finish_uploads();
        assert_eq!(ctx.assets.group_state(id), GroupState::Ready);
        let long: Vec<u8> = ctx.assets.take_group_value(id, "notes/long.txt").unwrap();
        assert_eq!(
            String::from_utf8(long).unwrap().lines().count(),
            40,
            "deflated files unpack"
        );
        let progress = ctx.assets.group_progress(id);
        assert!(progress.bytes_known && progress.bytes_total > 0);
        assert_eq!(progress.bytes_total, progress.bytes_read);
    });
}

#[test]
fn sources_keep_their_order_whether_read_now_or_on_a_worker() {
    game().run_headless(0, |ctx| {
        // Added first, read on the game thread: it wins.
        ctx.assets
            .add_source(MemorySource::new("patch").with("a.txt", "patched"));
        ctx.assets
            .add_background_source(MemorySource::new("base").with("a.txt", "base").with("b.txt", "base"));
        let id = ctx
            .assets
            .start_group(AssetGroup::new("order").bytes("a.txt").bytes("b.txt"));
        ctx.finish_uploads();
        assert_eq!(ctx.assets.take_group_value::<Vec<u8>>(id, "a.txt").unwrap(), b"patched");
        assert_eq!(ctx.assets.take_group_value::<Vec<u8>>(id, "b.txt").unwrap(), b"base");
    });
    game().run_headless(0, |ctx| {
        // A game-thread source after a background one: still asked second.
        ctx.assets
            .add_background_source(MemorySource::new("base").with("a.txt", "base"));
        ctx.assets
            .add_source(MemorySource::new("patch").with("a.txt", "patched").with("c.txt", "c"));
        let id = ctx
            .assets
            .start_group(AssetGroup::new("order").bytes("a.txt").bytes("c.txt"));
        ctx.finish_uploads();
        assert_eq!(ctx.assets.take_group_value::<Vec<u8>>(id, "a.txt").unwrap(), b"base");
        assert_eq!(ctx.assets.take_group_value::<Vec<u8>>(id, "c.txt").unwrap(), b"c");
    });
}

#[test]
fn an_empty_group_is_ready_at_once_and_values_are_taken_once_by_type() {
    game().run_headless(0, |ctx| {
        let empty = ctx.assets.start_group(AssetGroup::new("nothing"));
        assert_eq!(ctx.assets.group_state(empty), GroupState::Ready);
        ctx.assets
            .add_background_source(MemorySource::new("mem").with("n.bin", "1"));
        let id = ctx.assets.start_group(AssetGroup::new("one").bytes("n.bin"));
        ctx.finish_uploads();
        assert_eq!(ctx.assets.take_group_value::<String>(id, "n.bin"), None, "not a String");
        assert_eq!(ctx.assets.take_group_value::<Vec<u8>>(id, "n.bin"), Some(b"1".to_vec()));
        assert_eq!(ctx.assets.take_group_value::<Vec<u8>>(id, "n.bin"), None, "taken");
        assert_eq!(ctx.assets.group("one"), Some(id));
    });
}

#[test]
#[should_panic(expected = "was not started in this game")]
fn a_group_id_from_another_game_is_refused() {
    let id = {
        let mut id = None;
        game().run_headless(0, |ctx| id = Some(ctx.assets.start_group(AssetGroup::new("a"))));
        id.unwrap()
    };
    game().run_headless(0, move |ctx| {
        let _ = ctx.assets.group_state(id);
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
enum Screen {
    Title,
    Loading,
    Playing,
}

fn level_source(delay: Duration) -> Slow {
    Slow::new(
        "level",
        delay,
        &[("level.bin", vec![7; 32]), ("tiles.png", png(4, 4, [9, 9, 9, 255]))],
    )
}

#[test]
fn a_loading_state_leaves_when_its_group_is_ready() {
    let source = level_source(Duration::from_millis(30));
    let world = game()
        .state(Screen::Loading)
        .loading_state(
            Screen::Loading,
            |_| AssetGroup::new("level").bytes("level.bin").texture("tiles.png"),
            Screen::Playing,
        )
        .on_enter(Screen::Playing, |ctx: &mut Ctx| {
            let id = ctx.assets.group("level").unwrap();
            assert_eq!(ctx.assets.group_state(id), GroupState::Ready, "entered once ready");
            let level: Vec<u8> = ctx.assets.take_group_value(id, "level.bin").unwrap();
            ctx.world.insert_resource(level);
        })
        .sim_system(|_ctx: &mut Ctx| std::thread::sleep(Duration::from_millis(5)))
        .run_headless(200, move |ctx| ctx.assets.add_background_source(source));
    assert_eq!(world.resource::<ponko::state::State<Screen>>().get(), &Screen::Playing);
    assert_eq!(world.resource::<Vec<u8>>(), &vec![7; 32]);
}

#[test]
fn leaving_a_loading_state_early_cancels_its_group() {
    let source = level_source(Duration::from_millis(200));
    let world = game()
        .state(Screen::Loading)
        .loading_state(
            Screen::Loading,
            |_| AssetGroup::new("level").bytes("level.bin").texture("tiles.png"),
            Screen::Playing,
        )
        .sim_system(|ctx: &mut Ctx| {
            if ctx.time.sim_tick() == 2 {
                ctx.set_state(Screen::Title); // the player backs out
            }
        })
        .run_headless(5, move |ctx| ctx.assets.add_background_source(source));
    assert_eq!(world.resource::<ponko::state::State<Screen>>().get(), &Screen::Title);
    // The world has no `Assets`; check the cancel through a fresh game.
    let source = level_source(Duration::from_millis(200));
    game()
        .state(Screen::Loading)
        .loading_state(
            Screen::Loading,
            |_| AssetGroup::new("level").bytes("level.bin"),
            Screen::Playing,
        )
        .sim_system(|ctx: &mut Ctx| match ctx.time.sim_tick() {
            1 => ctx.set_state(Screen::Title),
            3 => {
                let id = ctx.assets.group("level").unwrap();
                assert_eq!(ctx.assets.group_state(id), GroupState::Cancelled);
            }
            _ => {}
        })
        .run_headless(4, move |ctx| ctx.assets.add_background_source(source));
}

#[test]
fn a_failed_group_keeps_the_game_in_its_loading_state() {
    let world = game()
        .state(Screen::Loading)
        .loading_state(
            Screen::Loading,
            |_| AssetGroup::new("level").bytes("missing.bin"),
            Screen::Playing,
        )
        .run_headless(20, |_| {});
    assert_eq!(world.resource::<ponko::state::State<Screen>>().get(), &Screen::Loading);
}

/// A source that counts the reads of each file and fails the first
/// `fail_first` reads of the files named in `flaky`, as a dropped
/// connection would.
#[derive(Clone)]
struct Flaky {
    files: MemorySource,
    flaky: Vec<&'static str>,
    fail_first: u32,
    reads: Arc<Mutex<HashMap<String, u32>>>,
    delay: Duration,
}

impl Flaky {
    fn new(files: &[(&str, Vec<u8>)], flaky: &[&'static str]) -> Self {
        let mut memory = MemorySource::new("flaky");
        for (name, bytes) in files {
            memory.insert(*name, bytes.clone());
        }
        Self {
            files: memory,
            flaky: flaky.to_vec(),
            fail_first: 1,
            reads: Arc::default(),
            delay: Duration::ZERO,
        }
    }

    fn reads(&self) -> Arc<Mutex<HashMap<String, u32>>> {
        self.reads.clone()
    }
}

impl AssetSource for Flaky {
    fn id(&self) -> &str {
        "flaky"
    }

    fn contains(&self, name: &str) -> bool {
        self.files.contains(name)
    }

    fn size(&self, name: &str) -> Option<u64> {
        self.files.size(name)
    }

    fn read(&self, name: &str) -> std::io::Result<Vec<u8>> {
        std::thread::sleep(self.delay);
        let n = {
            let mut reads = self.reads.lock().unwrap();
            let n = reads.entry(name.to_string()).or_insert(0);
            *n += 1;
            *n
        };
        if self.flaky.contains(&name) && n <= self.fail_first {
            return Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                "connection reset",
            ));
        }
        self.files.read(name)
    }
}

fn bins(n: usize) -> Vec<(String, Vec<u8>)> {
    (0..n).map(|i| (format!("{i}.bin"), vec![i as u8; 100 + i])).collect()
}

fn refs(files: &[(String, Vec<u8>)]) -> Vec<(&str, Vec<u8>)> {
    files.iter().map(|(n, b)| (n.as_str(), b.clone())).collect()
}

#[test]
fn restarting_a_failed_group_reads_again_only_what_failed() {
    let files = bins(12);
    let mut all = refs(&files);
    all.push(("art.png", png(16, 8, [10, 20, 30, 255])));
    all.push(("beep.wav", wav(400)));
    let flaky = Flaky::new(&all, &["7.bin"]);
    let reads = flaky.reads();
    game().run_headless(0, move |ctx| {
        ctx.assets.add_background_source(flaky);
        let mut group = AssetGroup::new("level").texture("art.png").sound("beep.wav");
        for (name, _) in &files {
            group = group.bytes(name);
        }
        let id = ctx.assets.start_group(group);
        ctx.finish_uploads();
        assert_eq!(ctx.assets.group_state(id), GroupState::Failed);
        let error = ctx.assets.group_error(id).unwrap();
        assert_eq!(error.path(), Some("7.bin"), "{error}");
        let art = ctx.gfx.load_texture("art.png");

        ctx.assets.restart_group(id);
        assert_eq!(ctx.assets.group_state(id), GroupState::Loading);
        assert!(ctx.assets.group_error(id).is_none());
        ctx.finish_uploads();
        assert_eq!(ctx.assets.group_state(id), GroupState::Ready);
        let progress = ctx.assets.group_progress(id);
        assert_eq!((progress.loaded, progress.total), (14, 14));
        assert_eq!(progress.fraction(), 1.0);
        for (name, bytes) in &files {
            assert_eq!(
                ctx.assets.take_group_value::<Vec<u8>>(id, name).as_ref(),
                Some(bytes),
                "{name}"
            );
        }
        assert_eq!(ctx.gfx.load_texture("art.png"), art, "the texture kept its handle");
        let reads = reads.lock().unwrap();
        assert_eq!(reads.get("7.bin"), Some(&2), "the failed file is read again");
        for (name, n) in reads.iter().filter(|(name, _)| name.as_str() != "7.bin") {
            assert_eq!(
                *n, 1,
                "{name} loaded (or was being read) before the failure: never read again"
            );
        }
        assert_eq!(reads.len(), 14);
    });
}

#[test]
fn a_restarted_group_that_fails_again_can_be_restarted_again() {
    let files = bins(3);
    let mut flaky = Flaky::new(&refs(&files), &["1.bin"]);
    flaky.fail_first = 2;
    let reads = flaky.reads();
    game().run_headless(0, move |ctx| {
        ctx.assets.add_background_source(flaky);
        let id = ctx
            .assets
            .start_group(AssetGroup::new("g").bytes("0.bin").bytes("1.bin").bytes("2.bin"));
        for expected in [GroupState::Failed, GroupState::Failed, GroupState::Ready] {
            ctx.finish_uploads();
            assert_eq!(ctx.assets.group_state(id), expected);
            ctx.assets.restart_group(id);
        }
        // Restarting a ready group does nothing.
        assert_eq!(ctx.assets.group_state(id), GroupState::Ready);
        let reads = reads.lock().unwrap();
        assert_eq!((reads["0.bin"], reads["1.bin"], reads["2.bin"]), (1, 3, 1));
    });
}

#[test]
fn restarting_a_cancelled_group_loads_only_what_had_not_loaded() {
    let files = bins(24);
    let mut flaky = Flaky::new(&refs(&files), &[]);
    flaky.delay = Duration::from_millis(15);
    let reads = flaky.reads();
    game().run_headless(0, move |ctx| {
        ctx.assets.add_background_source(flaky);
        let mut group = AssetGroup::new("many");
        for (name, _) in &files {
            group = group.bytes(name);
        }
        let id = ctx.assets.start_group(group);
        while ctx.assets.group_progress(id).loaded < 2 {
            ctx.pump_asset_groups();
            std::thread::sleep(Duration::from_millis(2));
        }
        ctx.assets.cancel_group(id);
        ctx.pump_asset_groups(); // the step that stops its work
        let loaded: Vec<String> = files
            .iter()
            .map(|(name, _)| name.clone())
            .filter(|name| ctx.assets.take_group_value::<Vec<u8>>(id, name).is_some())
            .collect();
        assert!(
            loaded.len() >= 2 && loaded.len() < 24,
            "{} loaded before the cancel",
            loaded.len()
        );
        ctx.assets.restart_group(id);
        ctx.finish_uploads();
        assert_eq!(ctx.assets.group_state(id), GroupState::Ready);
        assert_eq!(ctx.assets.group_progress(id).loaded, 24);
        let reads = reads.lock().unwrap();
        for name in &loaded {
            assert_eq!(reads[name], 1, "{name} loaded before the cancel: not read again");
        }
        for (name, _) in &files {
            if !loaded.contains(name) {
                assert!(ctx.assets.take_group_value::<Vec<u8>>(id, name).is_some(), "{name}");
            }
        }
    });
}

#[test]
fn a_group_restarted_before_its_cancel_was_stopped_still_loads() {
    let files = bins(16);
    let mut flaky = Flaky::new(&refs(&files), &[]);
    flaky.delay = Duration::from_millis(10);
    game().run_headless(0, move |ctx| {
        ctx.assets.add_background_source(flaky);
        let mut group = AssetGroup::new("many");
        for (name, _) in &files {
            group = group.bytes(name);
        }
        let id = ctx.assets.start_group(group);
        ctx.pump_asset_groups(); // reads start on workers
        ctx.assets.cancel_group(id);
        ctx.assets.restart_group(id); // a "back" and "again" in one frame
        ctx.finish_uploads();
        assert_eq!(
            ctx.assets.group_state(id),
            GroupState::Ready,
            "{:?}",
            ctx.assets.group_error(id)
        );
        assert_eq!(ctx.assets.group_progress(id).loaded, 16);
    });
}

#[test]
fn restart_does_nothing_to_a_loading_group() {
    let slow = Slow::new("slow", Duration::from_millis(20), &[("a.bin", vec![1])]);
    game().run_headless(0, move |ctx| {
        ctx.assets.add_background_source(slow);
        let id = ctx.assets.start_group(AssetGroup::new("one").bytes("a.bin"));
        ctx.assets.restart_group(id);
        assert_eq!(ctx.assets.group_state(id), GroupState::Loading);
        ctx.finish_uploads();
        assert_eq!(ctx.assets.group_state(id), GroupState::Ready);
    });
}

#[test]
fn a_loading_screen_retries_a_failed_group_and_moves_on() {
    let flaky = Flaky::new(
        &[("level.bin", vec![5; 8]), ("tiles.png", png(4, 4, [1, 2, 3, 255]))],
        &["level.bin"],
    );
    let reads = flaky.reads();
    let world = game()
        .state(Screen::Loading)
        .loading_state(
            Screen::Loading,
            |_| AssetGroup::new("level").bytes("level.bin").texture("tiles.png"),
            Screen::Playing,
        )
        // The loading screen's "try again": restart the group once it failed.
        .sim_system(|ctx: &mut Ctx| {
            if let Some(id) = ctx.assets.group("level")
                && ctx.assets.group_state(id) == GroupState::Failed
            {
                ctx.assets.restart_group(id);
            }
            std::thread::sleep(Duration::from_millis(2));
        })
        .run_headless(200, move |ctx| ctx.assets.add_background_source(flaky));
    assert_eq!(world.resource::<ponko::state::State<Screen>>().get(), &Screen::Playing);
    let reads = reads.lock().unwrap();
    assert_eq!((reads["level.bin"], reads["tiles.png"]), (2, 1));
}

#[test]
fn a_group_counts_the_bytes_and_time_of_its_texture_uploads() {
    let files: Vec<(String, Vec<u8>)> = (0..3)
        .map(|i| (format!("t{i}.png"), png(32, 16, [i as u8, 0, 0, 255])))
        .collect();
    let slow = Slow::new("art", Duration::ZERO, &refs(&files));
    game().run_headless(0, move |ctx| {
        ctx.assets.add_background_source(slow);
        let mut group = AssetGroup::new("art").bytes("t0.png");
        for (name, _) in &files {
            group = group.texture(name);
        }
        let id = ctx.assets.start_group(group);
        ctx.finish_uploads();
        assert_eq!(ctx.assets.group_state(id), GroupState::Ready);
        let progress = ctx.assets.group_progress(id);
        // Three 32 x 16 RGBA8 textures; the bytes asset uploads nothing.
        assert_eq!(progress.bytes_uploaded, 3 * 32 * 16 * 4);
        assert!(progress.upload_ms >= 0.0);
        let stats = ctx.assets.group_stats();
        assert_eq!(stats.total_upload_bytes, 3 * 32 * 16 * 4);
        assert!(stats.max_upload_ms >= stats.upload_ms);
        assert!(
            stats.total_upload_ms + 1e-3 >= f64::from(stats.max_upload_ms),
            "{stats:?}"
        );
        assert!(stats.max_step_ms >= stats.max_upload_ms, "uploads are part of the step");
    });
}

#[cfg(feature = "3d")]
mod meshes {
    use super::*;

    const HOUSE: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/models/house.glb"));

    #[test]
    fn a_group_mesh_is_parsed_on_a_worker_and_load_mesh_returns_it() {
        let slow = Slow::new(
            "models",
            Duration::from_millis(20),
            &[("models/home.glb", HOUSE.to_vec())],
        );
        let reads = slow.reads.clone();
        game().run_headless(0, move |ctx| {
            ctx.assets.add_background_source(slow);
            let id = ctx
                .assets
                .start_group(AssetGroup::new("models").mesh("models/home.glb"));
            ctx.finish_uploads();
            assert_eq!(ctx.assets.group_state(id), GroupState::Ready);
            assert!(ctx.gfx.is_resident(ResidencyKind::Mesh, "models/home.glb"));
            let home = ctx.gfx.load_mesh("models/home.glb");
            assert!(ctx.gfx.is_live(home));
            assert_eq!(reads.load(Ordering::SeqCst), 1, "load_mesh found it loaded");
        });
    }

    #[test]
    fn a_group_mesh_from_a_game_thread_source_loads_too() {
        game().run_headless(0, |ctx| {
            ctx.assets
                .add_source(MemorySource::new("models").with("models/home.glb", HOUSE));
            let id = ctx
                .assets
                .start_group(AssetGroup::new("models").mesh("models/home.glb"));
            ctx.finish_uploads();
            assert_eq!(ctx.assets.group_state(id), GroupState::Ready);
            assert!(ctx.gfx.is_resident(ResidencyKind::Mesh, "models/home.glb"));
        });
    }

    #[test]
    fn a_missing_group_mesh_fails_the_group_naming_it() {
        game().run_headless(0, |ctx| {
            let id = ctx
                .assets
                .start_group(AssetGroup::new("models").mesh("models/none.glb"));
            ctx.finish_uploads();
            let error = ctx.assets.group_error(id).unwrap();
            assert_eq!(
                (error.kind(), error.path()),
                (ErrorKind::NotFound, Some("models/none.glb"))
            );
        });
    }
}

// The guide's example ("Loading screens", book/src/loading.md).
mod guide {
    use super::*;

    // ANCHOR: loading_screen
    #[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
    enum Stage {
        Loading,
        Playing,
    }

    #[derive(Deserialize)]
    struct Rules {
        speed: f32,
    }

    /// Everything the first level needs, read and decoded on worker threads.
    fn first_level(_ctx: &mut Ctx) -> AssetGroup {
        AssetGroup::new("level 1")
            .texture("crab.png") // load_texture returns it afterwards
            .sound("tutorial_2d/coin.wav") // and audio.load this
            .with("rules.json", |bytes| {
                // Any loader of your own: bytes in, a value or an Error out.
                serde_json::from_slice::<Rules>(bytes).map_err(|e| Error::new(ErrorKind::Json, e.to_string()))
            })
    }

    /// The loading screen: a bar that fills as the files arrive, and why
    /// it stopped if one is missing or broken.
    fn loading_bar(ctx: &mut Ctx) {
        let Some(id) = ctx.assets.group("level 1") else { return };
        let fraction = ctx.assets.group_progress(id).fraction();
        ctx.gfx.draw_rect(Rect::new(100.0, 280.0, 600.0, 40.0), Color::GRAY);
        ctx.gfx
            .draw_rect(Rect::new(100.0, 280.0, 600.0 * fraction, 40.0), Color::WHITE);
        if let Some(error) = ctx.assets.group_error(id) {
            let text = error.to_string();
            ctx.gfx.draw_text(&text, vec2(100.0, 340.0), 20.0, Color::RED);
        }
    }

    fn start_playing(ctx: &mut Ctx) {
        let id = ctx.assets.group("level 1").unwrap();
        let rules: Rules = ctx.assets.take_group_value(id, "rules.json").unwrap();
        ctx.world.insert_resource(rules.speed);
        // Loaded: these return at once, without reading the files again.
        let crab = ctx.gfx.load_texture("crab.png");
        let coin = ctx.audio.load("tutorial_2d/coin.wav");
        ctx.world.insert_resource((crab, coin));
    }

    fn level_game() -> Game {
        Game::new("Loading", 800, 600)
            .state(Stage::Loading)
            // Entering Loading starts the group; it moves to Playing once ready.
            .loading_state(Stage::Loading, first_level, Stage::Playing)
            .add(Phase::Frame, loading_bar.run_if(in_state(Stage::Loading)))
            .on_enter(Stage::Playing, start_playing)
    }
    // ANCHOR_END: loading_screen

    #[test]
    fn the_guide_loading_screen_runs() {
        let files = Slow::new(
            "level",
            Duration::from_millis(5),
            &[
                ("crab.png", png(8, 8, [40, 120, 40, 255])),
                ("tutorial_2d/coin.wav", wav(800)),
                ("rules.json", br#"{ "speed": 3.5 }"#.to_vec()),
            ],
        );
        let world = level_game()
            .hot_reload(false)
            .sim_system(|_: &mut Ctx| std::thread::sleep(Duration::from_millis(2)))
            .run_headless(200, move |ctx| ctx.assets.add_background_source(files));
        assert_eq!(world.resource::<ponko::state::State<Stage>>().get(), &Stage::Playing);
        assert_eq!(*world.resource::<f32>(), 3.5);
    }
}

// ANCHOR: sliced_loader
/// Parses lines of numbers, two lines a call.
struct Numbers {
    text: String,
    at: usize,
    values: Vec<i32>,
}

impl ponko::assets::SliceLoader for Numbers {
    type Value = Vec<i32>;
    fn step(&mut self) -> Result<bool, Error> {
        for _ in 0..2 {
            let Some(line) = self.text[self.at..].lines().next() else {
                return Ok(true);
            };
            self.at += line.len() + 1;
            let value = line
                .parse()
                .map_err(|_| Error::new(ErrorKind::Decode, format!("\"{line}\" is not a number")))?;
            self.values.push(value);
        }
        Ok(self.at >= self.text.len())
    }
    fn finish(self) -> Vec<i32> {
        self.values
    }
}

/// `start` for `with_sliced`: `AssetGroup::new("level").with_sliced("level.txt", numbers)`.
fn numbers(bytes: Vec<u8>) -> Result<Numbers, Error> {
    let text = String::from_utf8(bytes).map_err(|e| Error::new(ErrorKind::Decode, e.to_string()))?;
    Ok(Numbers {
        text,
        at: 0,
        values: Vec::new(),
    })
}
// ANCHOR_END: sliced_loader

#[test]
fn a_sliced_loader_makes_its_value_and_its_errors_name_the_asset() {
    game().run_headless(0, |ctx| {
        ctx.assets.add_background_source(
            MemorySource::new("mem")
                .with("good.txt", "1\n2\n3\n4\n5\n")
                .with("bad.txt", "1\n2\nthree\n"),
        );
        let id = ctx
            .assets
            .start_group(AssetGroup::new("good").with_sliced("good.txt", numbers));
        frames_until_done(ctx, id);
        assert_eq!(ctx.assets.group_state(id), GroupState::Ready);
        assert_eq!(
            ctx.assets.take_group_value::<Vec<i32>>(id, "good.txt"),
            Some(vec![1, 2, 3, 4, 5])
        );
        let id = ctx
            .assets
            .start_group(AssetGroup::new("bad").with_sliced("bad.txt", numbers));
        frames_until_done(ctx, id);
        let error = ctx.assets.group_error(id).expect("it failed");
        assert_eq!((error.kind(), error.path()), (ErrorKind::Decode, Some("bad.txt")));
        assert!(error.to_string().contains("\"three\" is not a number"), "{error}");
    });
}

#[test]
fn headless_upload_stats_say_the_gpu_does_not_time_uploads() {
    game().run_headless(0, |ctx| {
        ctx.assets
            .add_background_source(MemorySource::new("mem").with("a.png", png(16, 16, [1, 2, 3, 255])));
        let id = ctx.assets.start_group(AssetGroup::new("a").texture("a.png"));
        frames_until_done(ctx, id);
        let (groups, streams) = (ctx.assets.group_stats(), ctx.gfx.upload_stats());
        assert!(!groups.gpu_upload_timing && !streams.gpu_upload_timing);
        assert_eq!((groups.gpu_upload_ms, streams.gpu_upload_ms), (None, None));
        assert_eq!(groups.total_gpu_upload_ms, 0.0);
    });
}
