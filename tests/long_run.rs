//! Long runs stay flat: a game that keeps spawning and despawning (directly,
//! through deferred commands and as parent-child families), making and
//! freeing handles, loading scenes, sending events, pushing and popping UI
//! screens and game states, starting and stopping sounds, drawing debug
//! shapes and moving a camera rig for 200k ticks holds no more memory at
//! the end than after its warm-up, keeps the same archetypes and query
//! cache, and reuses entity, UI, texture, sound and font slots instead of
//! counting them up ("Very long sessions" in `book/src/sim-clock.md`).
//!
//! The heap is measured with [`CountingAllocator`]'s net bytes, which count
//! the whole process, so this file holds a single test.

use std::collections::{HashSet, VecDeque};

use ponko::camera::CameraShake;
use ponko::d2::{FollowCamera, Transform as Transform2D};
use ponko::ecs_core::QueryCachePolicy;
use ponko::hierarchy::WorldHierarchy;
use ponko::perf::{CountingAllocator, net_alloc_bytes};
use ponko::prelude::*;
use serde::{Deserialize, Serialize};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// Ticks of churn. A scene is loaded every `SCENE_EVERY` ticks, replacing
/// the world; readings are taken on the tick before a load, so each one
/// sees the world at the same point of its cycle. The first reading that
/// counts comes after five scene cycles of warm-up.
const TICKS: u64 = 200_000;
const SCENE_EVERY: u64 = 5_000;
const WARM_UP: u64 = 5 * SCENE_EVERY - 1;
/// Entities alive at once from direct spawns, deferred spawns and families.
const LIVE: usize = 256;
const DEFERRED: usize = 64;
const FAMILIES: usize = 4;
/// A font is loaded (and the last one freed) this often.
const FONT_EVERY: u64 = 500;

/// What the run may add after warm-up: a hash table that happened to grow,
/// and retired slots. A slot is given out 4095 times and then retired, so
/// the 175 000 textures, sounds and (with `3d`) meshes made after warm-up
/// retire 43 slots of each kind, a few hundred bytes each across the
/// kind's tables as they double (measured: 4.7 KB headless, 12.8 KB with
/// `3d`). A leak of one byte per make would be 175 KB.
const SLACK: i64 = 32 << 10;

#[derive(Component, Clone, Copy, Serialize, Deserialize)]
struct Pos(f32, f32);
#[derive(Component, Clone, Copy)]
struct Vel(f32, f32);
#[derive(Component, Clone, Copy)]
struct Hp(#[allow(dead_code)] u32);
#[derive(Component, Clone, Copy)]
struct Deferred;
#[derive(Component, Clone, Copy)]
struct Hot;

#[derive(Clone, Copy)]
struct Ping(#[allow(dead_code)] u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Mode {
    Playing,
    Paused,
}

/// A file under `assets/` that is deleted when the test ends, pass or fail.
struct TempAsset(&'static str);
impl Drop for TempAsset {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets")
                .join(self.0),
        );
    }
}

/// The built-in font written to a temporary file `load_font` can read,
/// removed when the test ends.
struct FontFile(std::path::PathBuf);
impl FontFile {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("ponko-long-run-{}.ttf", std::process::id()));
        std::fs::write(&path, epaint_default_fonts::UBUNTU_LIGHT).unwrap();
        Self(path)
    }
}
impl Drop for FontFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// What the run looked like at one moment.
#[derive(Debug, Clone, PartialEq)]
struct Reading {
    tick: u64,
    heap: i64,
    entities: usize,
    archetypes: usize,
    query_signatures: usize,
    query_bytes: usize,
    live: ponko::handle::LiveCounts,
    /// Texture, sound and font ids made so far.
    made: [i64; 3],
}

/// Everything the churn keeps between ticks.
struct Churn {
    font: String,
    policy: QueryCachePolicy,
    spawned: VecDeque<Entity>,
    deferred: VecDeque<Entity>,
    families: VecDeque<Entity>,
    texture: Option<Texture>,
    sound: Option<Sound>,
    playing: Option<Playing>,
    font_handle: Option<Font>,
    #[cfg(feature = "3d")]
    three: three::Churn3D,
    menu: Option<UiId>,
    paused: bool,
    follow: FollowCamera,
    shake: CameraShake,
    max_index: u32,
    ui_slots: HashSet<u64>,
    made: [i64; 3],
    pings_read: u64,
    readings: Vec<Reading>,
}

impl Churn {
    fn new(font: String) -> Self {
        let mut policy = QueryCachePolicy::new();
        // Index every query from the first run, so the cache is in play.
        policy.min_archetypes = 0;
        policy.promote_after = 1;
        Self {
            font,
            policy,
            spawned: VecDeque::new(),
            deferred: VecDeque::new(),
            families: VecDeque::new(),
            texture: None,
            sound: None,
            playing: None,
            font_handle: None,
            #[cfg(feature = "3d")]
            three: three::Churn3D::default(),
            menu: None,
            paused: false,
            follow: FollowCamera::default(),
            shake: CameraShake::default(),
            max_index: 0,
            ui_slots: HashSet::new(),
            made: [0; 3],
            pings_read: 0,
            readings: Vec::with_capacity(64),
        }
    }

    fn read(&mut self, ctx: &mut Ctx) {
        let d = ctx.ecs_diagnostics();
        let reading = Reading {
            tick: ctx.time.sim_tick(),
            heap: net_alloc_bytes(),
            entities: d.entities,
            archetypes: d.archetypes.iter().filter(|a| a.entities > 0).count(),
            query_signatures: d.query_cache.signatures,
            query_bytes: d.query_cache.bytes,
            live: ctx.live_counts(),
            made: self.made,
        };
        self.readings.push(reading);
    }

    fn tick(&mut self, ctx: &mut Ctx) {
        let t = ctx.time.sim_tick();
        if t > 0 && t.is_multiple_of(SCENE_EVERY) {
            // The scene replaces the world at the end of this tick: what the
            // churn remembers of the old one goes with it.
            ctx.load_scene("test-long-run.json").unwrap();
            self.spawned.clear();
            self.deferred.clear();
            self.families.clear();
            return;
        }
        if ctx.world.query_cache_policy() != self.policy {
            ctx.world.set_query_cache_policy(self.policy);
        }
        self.entities(ctx, t);
        self.handles(ctx, t);
        #[cfg(feature = "3d")]
        self.three.tick(ctx);
        self.screens(ctx, t);
        self.camera(ctx, t);
        if t % SCENE_EVERY == SCENE_EVERY - 1 {
            self.read(ctx);
        }
    }

    /// Spawns and despawns, directly, deferred and as families; moves
    /// entities between archetypes; sends events.
    fn entities(&mut self, ctx: &mut Ctx, t: u64) {
        // Entities in four shapes, despawned oldest first.
        for i in 0..4u64 {
            let p = Pos(t as f32, i as f32);
            let e = match (t + i) % 4 {
                0 => ctx.world.spawn((p,)),
                1 => ctx.world.spawn((p, Vel(1.0, 0.0))),
                2 => ctx.world.spawn((p, Vel(0.0, 1.0), Hp(3))),
                _ => ctx.world.spawn((p, Hp(1))),
            };
            self.max_index = self.max_index.max(e.index());
            self.spawned.push_back(e);
        }
        while self.spawned.len() > LIVE {
            let e = self.spawned.pop_front().unwrap();
            ctx.world.despawn(e);
        }
        // A component coming and going: an entity changes archetype.
        let e = self.spawned[(t % LIVE as u64) as usize % self.spawned.len()];
        if ctx.world.get::<Hot>(e).is_some() {
            ctx.world.remove::<Hot>(e);
        } else {
            ctx.world.insert(e, Hot);
        }
        for (pos, vel) in ctx.world.query::<(&mut Pos, &Vel)>().iter() {
            pos.0 += vel.0;
            pos.1 += vel.1;
        }

        // Deferred spawns take a reserved id now and land at the end of the
        // tick; deferred despawns give ids back.
        let e = ctx.commands.spawn((Pos(0.0, 0.0), Deferred));
        self.max_index = self.max_index.max(e.index());
        self.deferred.push_back(e);
        if self.deferred.len() > DEFERRED {
            let e = self.deferred.pop_front().unwrap();
            ctx.commands.despawn(e);
        }
        if let Some(&e) = self.deferred.front() {
            ctx.commands.insert(e, Hp(t as u32));
        }

        // A parent and three children, despawned together later.
        if t.is_multiple_of(8) {
            let parent = ctx.world.spawn((Transform2D::at(vec2(t as f32, 0.0)),));
            for k in 0..3 {
                let child = ctx.world.spawn((Transform2D::at(vec2(k as f32, 0.0)), Hp(k)));
                ctx.world.set_parent(child, parent).unwrap();
            }
            self.families.push_back(parent);
            if self.families.len() > FAMILIES {
                let old = self.families.pop_front().unwrap();
                assert_eq!(ctx.world.despawn_recursive(old), 4, "a whole family goes");
            }
        }

        // Events, read by the system after this one.
        for k in 0..3 {
            ctx.events.send(Ping(t * 3 + k));
        }
    }

    /// A texture and a sound made, played and freed every tick; a font
    /// now and then.
    fn handles(&mut self, ctx: &mut Ctx, t: u64) {
        if let Some(old) = self.texture.take() {
            ctx.gfx.free(old);
        }
        self.texture = Some(ctx.gfx.create_texture(4, 4, &[200; 64]));
        if let Some(old) = self.playing.take() {
            ctx.audio.stop(old);
        }
        if let Some(old) = self.sound.take() {
            ctx.audio.free(old);
        }
        let sound = ctx.audio.create(8_000, &[0.0; 80]);
        self.playing = Some(ctx.audio.play(sound));
        self.sound = Some(sound);
        self.made[0] += 1;
        self.made[1] += 1;
        if t.is_multiple_of(FONT_EVERY) {
            if let Some(old) = self.font_handle.take() {
                ctx.gfx.free(old);
            }
            self.font_handle = Some(ctx.gfx.load_font(&self.font));
            self.made[2] += 1;
        }
    }

    /// A menu screen built, pushed, popped and removed; a pause state
    /// pushed and popped.
    fn screens(&mut self, ctx: &mut Ctx, t: u64) {
        match (t % 10, self.menu) {
            (0, None) => {
                let root = ctx.ui.root().layout(UiLayout::fill()).id();
                let resume = ctx.ui.button(root, "Resume").layout(UiLayout::row(40.0)).focused().id();
                let quit = ctx.ui.button(root, "Quit").layout(UiLayout::row(40.0)).id();
                ctx.ui.push_modal(root);
                for id in [root, resume, quit] {
                    self.ui_slots.insert(id.bits() & 0xF_FFFF);
                }
                self.menu = Some(root);
            }
            (5, Some(root)) => {
                assert_eq!(ctx.ui.pop_screen(), Some(root));
                ctx.ui.remove(root);
                self.menu = None;
            }
            _ => {}
        }
        match (t % 20, self.paused) {
            (0, false) => {
                ctx.push_state(Mode::Paused);
                self.paused = true;
            }
            (10, true) => {
                ctx.pop_state::<Mode>();
                self.paused = false;
            }
            _ => {}
        }
    }

    /// Debug shapes, and a follow camera with shake.
    fn camera(&mut self, ctx: &mut Ctx, t: u64) {
        let hero = vec2((t % 1000) as f32, (t % 300) as f32);
        ctx.debug.line(hero, hero + vec2(10.0, 0.0), Color::RED);
        ctx.debug.circle(hero, 4.0, Color::GREEN);
        ctx.debug.text(hero, "hero", Color::WHITE);
        if t.is_multiple_of(120) {
            self.shake.add_trauma(0.5);
        }
        self.shake.update(ctx.time.dt());
        self.follow.update(&mut ctx.camera, hero, ctx.time.dt());
        self.shake.apply_2d(&mut ctx.camera);
    }
}

/// Meshes and instance cells, which reuse their slots, made and freed
/// every tick.
#[cfg(feature = "3d")]
mod three {
    use ponko::inventory::AssetClass;
    use ponko::prelude::*;
    use ponko::render::{Compute, Effect, InstanceBatch, InstanceCell, Material, MaterialDesc, Mesh, MeshData, Shader};
    use ponko::terrain::{TextureArray, TextureKind};

    #[derive(Default)]
    pub struct Churn3D {
        batch: Option<InstanceBatch>,
        mesh: Option<Mesh>,
        cell: Option<InstanceCell>,
        resources: Option<(Material, Shader, Compute, Effect, TextureArray)>,
        first_slots: Option<[u32; 5]>,
    }

    impl Churn3D {
        pub fn tick(&mut self, ctx: &mut Ctx) {
            if ctx.time.sim_tick().is_multiple_of(200) {
                self.resources(ctx);
            }
            let material = ctx.gfx.default_material();
            if let Some(old) = self.mesh.take() {
                ctx.gfx.free(old);
            }
            self.mesh = Some(ctx.gfx.create_mesh(&MeshData::cube(1.0), material));
            let batch = *self.batch.get_or_insert_with(|| {
                let cube = ctx.gfx.create_mesh(&MeshData::cube(0.5), material);
                ctx.gfx.create_instance_batch(cube, &[], Default::default())
            });
            if let Some(old) = self.cell.take() {
                ctx.gfx.free(old);
            }
            self.cell = Some(
                ctx.gfx
                    .add_instance_cell(batch, &[Mat4::IDENTITY; 8], &[Color::WHITE; 8]),
            );
        }
        fn resources(&mut self, ctx: &mut Ctx) {
            let old = self.resources.take();
            if let Some((material, shader, compute, effect, array)) = old {
                ctx.free(material);
                ctx.free(shader);
                ctx.free(compute);
                ctx.free(effect);
                ctx.free(array);
            }
            let name = format!("churn {}", ctx.time.sim_tick());
            let shader = ctx.gfx.create_shader(&name, "");
            let compute = ctx.gfx.create_compute(&name, "fn update(i: u32) {}");
            let effect = ctx
                .gfx
                .create_effect(&name, "fn effect(p: Pixel) -> vec3<f32> { return p.color; }");
            let pixel: &[u8] = &[255; 4];
            let array = ctx.gfx.create_texture_array(1, 1, &[pixel], TextureKind::Color);
            let material = ctx.gfx.create_material(MaterialDesc::default().with_shader(shader));
            if let Some((m, s, c, e, a)) = old {
                assert_ne!((m, s, c, e, a), (material, shader, compute, effect, array));
                assert!(!ctx.is_live(m) && !ctx.is_live(s) && !ctx.is_live(c) && !ctx.is_live(e) && !ctx.is_live(a));
                ctx.free(m);
                ctx.free(s);
                ctx.free(c);
                ctx.free(e);
                ctx.free(a);
                assert!(
                    ctx.is_live(material)
                        && ctx.is_live(shader)
                        && ctx.is_live(compute)
                        && ctx.is_live(effect)
                        && ctx.is_live(array)
                );
            }
            let slots = [
                AssetClass::Material,
                AssetClass::Shader,
                AssetClass::Compute,
                AssetClass::Effect,
                AssetClass::TextureArray,
            ]
            .map(|class| {
                ctx.gfx
                    .asset_handles(class)
                    .iter()
                    .map(|record| record.id & 0xF_FFFF)
                    .max()
                    .unwrap()
            });
            let first = self.first_slots.get_or_insert(slots);
            for (slot, first) in slots.into_iter().zip(*first) {
                // 1000 makes fit one generation table slot. The default
                // material is also made after the first reading.
                assert!(slot <= first + 1, "resource slot grew from {first} to {slot}");
            }
            self.resources = Some((material, shader, compute, effect, array));
        }
    }
}

#[test]
fn two_hundred_thousand_ticks_of_churn_hold_memory_archetypes_and_ids_flat() {
    let scene = TempAsset("test-long-run.json");
    let font = FontFile::new();
    let churn = std::rc::Rc::new(std::cell::RefCell::new(Churn::new(
        font.0.to_str().unwrap().to_string(),
    )));
    let (driver, reader, out) = (churn.clone(), churn.clone(), churn.clone());
    let started = std::time::Instant::now();
    Game::new("long run", 320, 180)
        .state(Mode::Playing)
        .sim_system(move |ctx: &mut Ctx| driver.borrow_mut().tick(ctx))
        .sim_system(move |ctx: &mut Ctx| {
            let n = ctx.events.read::<Ping>().len() as u64;
            reader.borrow_mut().pings_read += n;
        })
        .run_headless(TICKS, |ctx| {
            ctx.debug.set_enabled(true);
            // Allocations are counted while profiling is on.
            ctx.perf.set_profiling(true);
            ctx.world.reflect::<Pos>();
            for i in 0..8 {
                ctx.world.spawn((Pos(i as f32, 0.0),));
            }
            ctx.assets.save_scene(&ctx.world, scene.0).unwrap();
        });
    let churn = out.borrow();
    eprintln!("{TICKS} ticks in {:.1?}", started.elapsed());
    for r in &churn.readings {
        eprintln!(
            "tick {:>6}: heap {:>9} B, {} entities in {} archetypes, query cache {} signatures / {} B",
            r.tick, r.heap, r.entities, r.archetypes, r.query_signatures, r.query_bytes
        );
    }
    let warm = churn
        .readings
        .iter()
        .find(|r| r.tick == WARM_UP)
        .expect("a reading after warm-up");
    let end = churn.readings.last().unwrap();
    assert_eq!(end.tick, TICKS - 1);

    // The world: the same entities, archetypes and query cache.
    assert_eq!(end.entities, warm.entities);
    assert_eq!(end.archetypes, warm.archetypes);
    assert!(warm.query_signatures > 0, "the query cache is in play");
    assert_eq!(
        (end.query_signatures, end.query_bytes),
        (warm.query_signatures, warm.query_bytes)
    );
    assert_eq!(end.live, warm.live, "every handle made is freed again");

    // Ids: hundreds of thousands of entities and UI nodes made, a few
    // hundred ids used.
    let peak = LIVE + DEFERRED + 4 * FAMILIES + 8;
    assert!(
        (churn.max_index as usize) < 2 * peak,
        "entity ids are reused: the highest index is {}",
        churn.max_index
    );
    assert!(
        churn.ui_slots.len() <= 3,
        "UI slots are reused: {}",
        churn.ui_slots.len()
    );

    // The heap: flat, though 175 000 textures and sounds and 315 fonts
    // were made and freed after warm-up.
    let made = |i: usize| end.made[i] - warm.made[i];
    let grown = end.heap - warm.heap;
    eprintln!(
        "heap grew {grown} B after warm-up, for {} textures, {} sounds and {} fonts made and freed (allowed {SLACK})",
        made(0),
        made(1),
        made(2)
    );
    assert!(grown <= SLACK, "the heap grew {grown} B, more than {SLACK}");

    // Every event sent was read (none are sent on the ticks that load a
    // scene: 5000, 10000, ... 195000).
    let loads = (TICKS - 1) / SCENE_EVERY;
    assert_eq!(churn.pings_read, 3 * (TICKS - loads));
}
