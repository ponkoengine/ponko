//! Shared helpers and conventions for the offscreen GPU suites
//! (`src/render/*_render_test.rs`).
//!
//! # Conventions
//!
//! - **Adapters.** Get the device with [`gpu`] or [`gpu_with`], never by
//!   calling `request_adapter` directly (GPU tests inside other modules too). When no adapter or device exists the
//!   test returns early and logs `render test SKIPPED [<test>]: <reason>`;
//!   when it runs it logs `render test ran [<test>] on <adapter> (<backend>,
//!   ...)`. A test that needs a capability the adapter lacks calls [`skip`]
//!   with the reason. A skip is not a pass: it proves nothing about the
//!   picture. To see what ran, use `--nocapture` and look for those lines:
//!
//!   ```sh
//!   cargo test -p ponko --lib --features 3d render_test -- --nocapture 2>&1 | grep "render test"
//!   ```
//!
//!   Set `PONKO_REQUIRE_GPU=1` on a machine (or CI runner) that has a GPU to
//!   turn a missing adapter or device into a failure instead of a skip.
//!   Without it, a missing adapter or device still fails once another test
//!   of the process opened a device (it was lost, see [`no_gpu`]). Capability
//!   skips stay skips: some adapters really lack a feature. Every skip is
//!   also appended to `skips.log` in [`output_dir`], for a run to count.
//! - **Devices at once.** Tests hold at most `PONKO_GPU_TESTS_AT_ONCE`
//!   devices at once (default one; see [`at_once`]): more lose the device
//!   on some drivers.
//! - **GPU tiers.** Set `PONKO_GPU_TIER` to `webgl2` or `baseline` to run
//!   every suite on a device opened with that tier's limits and without
//!   timestamp queries ([`forced`]; `desktop` runs as usual), as CI does
//!   one tier at a time:
//!
//!   ```sh
//!   PONKO_GPU_TIER=webgl2 cargo test -p ponko --lib --features 3d render_test
//!   ```
//!
//!   A test that needs a feature the forced tier lacks calls [`skip`] with
//!   the reason ([`needs`] does both); every other test must pass as on the
//!   real device. Only the capability suite opens its own forced devices.
//! - **Assertions.** Prefer semantic checks that say what must hold ("the box
//!   is lit", "the background behind it stays black", "this mode costs more
//!   than that one") with a negative control showing the check can fail.
//!   Compare images with [`diff`] and a [`Tolerance`] (per-channel slack plus
//!   a fraction of pixels allowed beyond it), not byte equality, unless the
//!   same device must produce identical output (a fallback drawing exactly
//!   like the mode it falls back to). Print the measured numbers with
//!   `eprintln!` so a failure on other hardware can be read.
//! - **Reference images.** A whole-image baseline goes through
//!   [`check_reference`]: a small PNG in `tests/fixtures/render/<name>.png`
//!   and its metadata in `<name>.json` (size, format, tolerance, the scene,
//!   the adapter it was made on, the PNG's checksum and why it was last
//!   changed). A missing or mismatching reference fails, and the actual and
//!   difference images are written to `target/render-test-output/` for
//!   inspection. Nothing is ever written to the fixtures unless asked:
//!
//!   ```sh
//!   PONKO_UPDATE_REFERENCES=1 PONKO_REFERENCE_REASON="why the picture changes" \
//!     cargo test -p ponko --lib --features 3d <test>
//!   ```
//!
//!   rewrites the references that no longer match (one that still matches
//!   is left alone), records the reason, and saves `<name>.before.png`,
//!   `<name>.after.png` and `<name>.diff.png` in `target/render-test-output/`
//!   for the pull request. It is refused when `CI` is set and without a
//!   reason. Changing a tolerance or a scene description in code without
//!   regenerating fails too, and so does a PNG swapped by hand (its
//!   checksum), so a different baseline always shows up in review; CI's
//!   reference job holds a pull request that changes one until a maintainer
//!   has looked at the before/after images. Keep them small (128x128 is
//!   plenty). `PONKO_RENDER_TEST_IMAGES=1` keeps every image a test read
//!   back, for looking at a failure.
//! - **Readback.** [`read_texture`] copies any texture back with row padding
//!   handled; [`target`] makes an offscreen colour target to draw into, and
//!   [`white_textures`] the one-white-texture set most rigs draw with.
//!
//! Every suite opens its device through these helpers (a test checks it).
//! The older suites still carry their own readback and texture set-up and
//! move over as they are touched.
use super::gpu::GpuTexture;
use std::path::PathBuf;

/// A device for one test, and what it runs on.
pub(crate) struct TestGpu {
    pub info: wgpu::AdapterInfo,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

/// Direct3D 12 devices of a test that panicked are never dropped. wgpu's
/// queue waits for the GPU to finish with no time limit when it is dropped,
/// so a test that panics while its GPU work is stuck (after a failed
/// pipeline, say) would wait in its unwinding forever and the test binary
/// would never exit. A clone of each Direct3D 12 device and queue a test
/// opens is kept here; once the test's thread has ended, the next test to
/// open one drops them, or leaves them alive if the thread panicked (a
/// leak in a run that already failed). Other backends are left alone:
/// Metal's timestamp query sets are given back when their queue goes (see
/// `query_sets`), so keeping queues there could run out.
mod kept {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, PoisonError, Weak};

    /// A test thread: alive while `alive` upgrades, and whether it panicked.
    struct Owner {
        alive: Weak<()>,
        panicked: Arc<AtomicBool>,
    }

    thread_local! {
        static OWNER: (Arc<()>, Arc<AtomicBool>) = (Arc::new(()), Arc::new(AtomicBool::new(false)));
    }

    static KEPT: Mutex<Vec<(Owner, wgpu::Device, wgpu::Queue)>> = Mutex::new(Vec::new());

    /// Installs, once, the panic hook that marks a thread that panicked
    /// (then runs the hook that was there).
    pub(super) fn watch() {
        static HOOK: std::sync::Once = std::sync::Once::new();
        HOOK.call_once(|| {
            let previous = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                let _ = OWNER.try_with(|o| o.1.store(true, Ordering::Relaxed));
                previous(info);
            }));
        });
    }

    /// Keeps `device` and `queue` past this test's thread, and lets go of
    /// those of threads that have ended.
    pub(super) fn keep(device: &wgpu::Device, queue: &wgpu::Queue) {
        watch();
        let Ok(owner) = OWNER.try_with(|o| Owner {
            alive: Arc::downgrade(&o.0),
            panicked: o.1.clone(),
        }) else {
            return;
        };
        let ended = {
            let mut kept = KEPT.lock().unwrap_or_else(PoisonError::into_inner);
            let (ended, running) = std::mem::take(&mut *kept)
                .into_iter()
                .partition::<Vec<_>, _>(|(o, ..)| o.alive.strong_count() == 0);
            *kept = running;
            kept.push((owner, device.clone(), queue.clone()));
            ended
        };
        // Not under the lock: dropping a queue waits for its GPU work.
        for entry in ended {
            if entry.0.panicked.load(Ordering::Relaxed) {
                std::mem::forget(entry);
            }
        }
    }

    /// Whether this thread panicked, so its devices are left alive.
    pub(super) fn panicked() -> bool {
        OWNER.with(|o| o.1.load(Ordering::Relaxed))
    }
}

/// The running test's name (libtest names each test's thread after it).
fn test_name() -> String {
    std::thread::current().name().unwrap_or("unnamed test").to_owned()
}

fn env_set(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| !v.is_empty() && v != "0")
}

/// Logs that the running test skips because the adapter can't do something
/// it needs, and appends the line to `skips.log` in [`output_dir`], so a run
/// can count its skips whatever the runner shows. The caller returns early.
pub(super) fn skip(reason: impl std::fmt::Display) {
    let line = format!("render test SKIPPED [{}]: {reason}", test_name());
    eprintln!("{line}");
    append(&output_dir(), "skips.log", &line);
}

/// Appends `line` to `file` in `output`, or says why it couldn't.
fn append(output: &std::path::Path, file: &str, line: &str) {
    use std::io::Write;
    let written = std::fs::create_dir_all(output).and_then(|()| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(output.join(file))
            .and_then(|mut log| writeln!(log, "{line}"))
    });
    if let Err(e) = written {
        eprintln!("(not logged in {}: {e})", output.display());
    }
}

/// Set once a test in this process has opened a device: from then on the
/// GPU is known to be there, so a test that finds none lost it (a removed
/// or reset device can take the adapter with it for a while) and fails.
static OPENED_A_DEVICE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// What a test that gets no adapter or device does.
#[derive(Debug, PartialEq, Eq)]
enum NoGpu {
    Skip,
    /// Fails, with why the GPU was expected.
    Fail(&'static str),
}

fn no_gpu_outcome(required: bool, opened_before: bool) -> NoGpu {
    if required {
        NoGpu::Fail("PONKO_REQUIRE_GPU is set")
    } else if opened_before {
        NoGpu::Fail(
            "an earlier test of this run opened one, so the device was lost on the way (see \"How many at once\" in \
             book/src/render-tests.md about PONKO_GPU_TESTS_AT_ONCE)",
        )
    } else {
        NoGpu::Skip
    }
}

/// A missing adapter or device: a skip on a machine without a GPU; a
/// failure under `PONKO_REQUIRE_GPU`, or once another test of this run has
/// opened a device (the device was lost, and a skip would pass silently).
/// Every suite reports a failed adapter or device request through this.
pub(super) fn no_gpu(reason: impl std::fmt::Display) {
    let opened = OPENED_A_DEVICE.load(std::sync::atomic::Ordering::Relaxed);
    match no_gpu_outcome(env_set("PONKO_REQUIRE_GPU"), opened) {
        NoGpu::Fail(why) => panic!("{reason}, and {why}"),
        NoGpu::Skip => skip(reason),
    }
}

/// How many tests may hold a GPU device at once in this process.
///
/// One device per test thread exhausts some drivers: on a Radeon RX Vega
/// the Vulkan and Direct3D 12 drivers lost the device, and every test on it
/// then failed with errors that looked like unrelated bugs. A test takes a
/// place when it first opens a device and gives it back when its thread
/// ends, so a test that opens several devices, or hands one to a renderer,
/// counts once. `PONKO_GPU_TESTS_AT_ONCE=<n>` sets the number; the default
/// is [`DEFAULT_AT_ONCE`]. Tests that need no GPU still run beside them on
/// the runner's other threads.
mod at_once {
    use std::sync::{Condvar, Mutex, PoisonError};
    use std::time::Duration;

    /// A counting semaphore.
    pub(super) struct Limiter {
        held: Mutex<usize>,
        freed: Condvar,
    }

    /// A place taken from a [`Limiter`], given back on drop.
    pub(super) struct Place(&'static Limiter);

    impl Limiter {
        pub(super) const fn new() -> Self {
            Self {
                held: Mutex::new(0),
                freed: Condvar::new(),
            }
        }

        /// Waits for one of `limit` places. After `patience` it takes one
        /// anyway and says so: a test that waits on itself (a device opened
        /// on a thread it spawned) runs late instead of hanging.
        pub(super) fn take(&'static self, limit: usize, patience: Duration) -> Place {
            let held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
            let (mut held, waited) = self
                .freed
                .wait_timeout_while(held, patience, |held| *held >= limit.max(1))
                .unwrap_or_else(PoisonError::into_inner);
            if waited.timed_out() {
                eprintln!(
                    "render test [{}] waited {patience:?} for one of {limit} GPU places; opening a device anyway",
                    super::test_name()
                );
            }
            *held += 1;
            Place(self)
        }

        pub(super) fn held(&self) -> usize {
            *self.held.lock().unwrap_or_else(PoisonError::into_inner)
        }
    }

    impl Drop for Place {
        fn drop(&mut self) {
            *self.0.held.lock().unwrap_or_else(PoisonError::into_inner) -= 1;
            self.0.freed.notify_one();
        }
    }

    /// One at a time. Two at once already lost the device on a Radeon RX
    /// Vega under Vulkan (`crowd_render_test`), and the suites spend most of
    /// their time waiting for the GPU, so more buy little.
    pub(super) const DEFAULT_AT_ONCE: usize = 1;

    pub(super) fn limit() -> usize {
        std::env::var("PONKO_GPU_TESTS_AT_ONCE")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .filter(|&n| n > 0)
            .unwrap_or(DEFAULT_AT_ONCE)
    }

    pub(super) static PROCESS: Limiter = Limiter::new();

    thread_local! {
        static HELD: std::cell::RefCell<Option<Place>> = const { std::cell::RefCell::new(None) };
    }

    /// Takes this test thread's place in `limiter`, unless it holds one
    /// already. The place is given back when the thread ends.
    pub(super) fn enter(limiter: &'static Limiter, limit: usize, patience: Duration) {
        let _ = HELD.try_with(|held| {
            let mut held = held.borrow_mut();
            if held.is_none() {
                *held = Some(limiter.take(limit, patience));
            }
        });
    }
}

/// The tier `PONKO_GPU_TIER` forces on the suites, if any.
pub(super) fn forced_tier() -> Option<crate::render::GpuTier> {
    crate::render::GpuTier::forced()
}

/// `descriptor` as the forced tier opens it (unchanged without one): that
/// tier's limits, and no timestamp queries below `desktop`. Every suite
/// opens its device through this.
pub(super) fn forced<'a>(descriptor: &wgpu::DeviceDescriptor<'a>) -> wgpu::DeviceDescriptor<'a> {
    let mut d = descriptor.clone();
    if let Some(tier) = forced_tier() {
        // CI checks for these lines: a run where nothing opened a device on
        // the tier must not pass.
        eprintln!("render test opens a device [{}] as the {tier} tier", test_name());
        d.required_limits = tier.device_limits(d.required_limits);
        d.required_features = tier.device_features(d.required_features);
    }
    d
}

/// Whether `device` has `feature`, as the renderer decides it.
pub(super) fn available(device: &wgpu::Device, feature: crate::render::GpuFeature) -> bool {
    super::capabilities::report(
        Default::default(),
        super::capabilities::GpuLimits::of(device),
        device.features().contains(wgpu::Features::TIMESTAMP_QUERY),
        (4, 4),
        true,
    )
    .available(feature)
}

/// Whether `device` has `feature`; if not, logs a skip naming the forced
/// tier (the caller returns early).
pub(super) fn needs(device: &wgpu::Device, feature: crate::render::GpuFeature) -> bool {
    if available(device, feature) {
        return true;
    }
    let tier = forced_tier().map_or_else(|| "this device".to_owned(), |t| format!("the forced {t} tier"));
    skip(format_args!("needs {feature:?}, which {tier} lacks"));
    false
}

/// Takes the running test's place among the tests that hold a GPU device
/// at once (see [`at_once`]), until the test ends. [`gpu`] and the other
/// helpers take it themselves; code that opens its own device (the files
/// allowed to, and the engine's own devices in a test) calls this first.
pub(crate) fn take_gpu_place() {
    at_once::enter(&at_once::PROCESS, at_once::limit(), std::time::Duration::from_secs(300));
}

/// A device with default limits and no optional features, or `None` (the
/// test returns early; see the module docs).
pub(crate) fn gpu() -> Option<TestGpu> {
    gpu_with(|_| Ok(wgpu::DeviceDescriptor::default()))
}

/// A device made from the descriptor `describe` picks for the adapter.
/// `describe` returns `Err(reason)` when the adapter can't run the test (a
/// capability skip, logged with that reason).
pub(super) fn gpu_with(
    describe: impl FnOnce(&wgpu::Adapter) -> Result<wgpu::DeviceDescriptor<'static>, String>,
) -> Option<TestGpu> {
    gpu_adjusted(describe, |_| {})
}

/// [`gpu_with`], with `adjust` changing the descriptor after the forced
/// tier was applied to it: for a test that takes a limit below (or sets it
/// apart from) whatever tier runs.
pub(super) fn gpu_adjusted(
    describe: impl FnOnce(&wgpu::Adapter) -> Result<wgpu::DeviceDescriptor<'static>, String>,
    adjust: impl FnOnce(&mut wgpu::DeviceDescriptor<'static>),
) -> Option<TestGpu> {
    take_gpu_place();
    // Like the engine's own instance, it follows wgpu's environment
    // variables: `WGPU_BACKEND=dx12` (or `vulkan`, `metal`, `gl`) picks the
    // backend, `WGPU_VALIDATION_INDIRECT_CALL=0` and the like set its flags.
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) {
        Ok(adapter) => adapter,
        Err(e) => {
            no_gpu(format_args!("no GPU adapter ({e})"));
            return None;
        }
    };
    let info = adapter.get_info();
    let descriptor = match describe(&adapter) {
        Ok(descriptor) => descriptor,
        Err(reason) => {
            skip(format_args!("{reason} (on {})", info.name));
            return None;
        }
    };
    let mut descriptor = forced(&descriptor);
    adjust(&mut descriptor);
    let (device, queue) = match pollster::block_on(adapter.request_device(&descriptor)) {
        Ok(pair) => pair,
        Err(e) => {
            no_gpu(format_args!("no GPU device on {} ({e})", info.name));
            return None;
        }
    };
    if info.backend == wgpu::Backend::Dx12 {
        kept::keep(&device, &queue);
    }
    OPENED_A_DEVICE.store(true, std::sync::atomic::Ordering::Relaxed);
    match forced_tier() {
        Some(tier) => eprintln!(
            "render test ran [{}] on {} as the {tier} tier",
            test_name(),
            describe_adapter(&info)
        ),
        None => eprintln!("render test ran [{}] on {}", test_name(), describe_adapter(&info)),
    }
    Some(TestGpu { info, device, queue })
}

/// Game context with the production renderer drawing to a texture, no window.
#[cfg(feature = "3d")]
pub(super) fn offscreen_ctx(size: (u32, u32)) -> Option<crate::ctx::Ctx> {
    let gpu = gpu()?;
    let mut ctx = crate::ctx::Ctx::new(1.0 / 60.0, glam::Vec2::new(size.0 as f32, size.1 as f32));
    ctx.audio.headless = true;
    ctx.gfx.renderer = Some(super::gpu::Renderer::offscreen(gpu, size));
    Some(ctx)
}

/// An adapter's name, backend, type and driver, for logs and metadata.
pub(super) fn describe_adapter(info: &wgpu::AdapterInfo) -> String {
    format!(
        "{} ({:?}, {:?}, driver {})",
        info.name,
        info.backend,
        info.device_type,
        driver(info)
    )
}

/// The adapter's driver and its version, or "unknown" (Metal reports none).
fn driver(info: &wgpu::AdapterInfo) -> String {
    let driver = format!("{} {}", info.driver, info.driver_info);
    match driver.trim() {
        "" => "unknown".to_owned(),
        d => d.to_owned(),
    }
}

/// The texture set most suites draw with: a bind group layout for one
/// filtered texture, a 1x1 white sRGB texture (also the renderer's default),
/// and that texture as texture 0.
pub(super) fn white_textures(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> (wgpu::BindGroupLayout, wgpu::Texture, Vec<GpuTexture>) {
    let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty,
        count: None,
    };
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            entry(
                0,
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
            ),
            entry(1, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
        ],
    });
    let white = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
    });
    queue.write_texture(
        white.as_image_copy(),
        &[255; 4],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: None,
        },
        white.size(),
    );
    let sampler = device.create_sampler(&Default::default());
    let view = white.create_view(&Default::default());
    let textures = vec![GpuTexture {
        texture: white.clone(),
        nearest: false,
        mips: None,
        bind_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        }),
    }];
    (layout, white, textures)
}

/// An offscreen colour target of `size` that can be drawn into and read back.
pub(super) fn target(device: &wgpu::Device, size: (u32, u32), format: wgpu::TextureFormat) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

/// The first mip and layer of `texture` (made with `COPY_SRC`), row by row
/// from the top with no padding. Waits for the GPU.
pub(crate) fn read_texture(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture) -> Vec<u8> {
    let (width, height) = (texture.width(), texture.height());
    let texel = texture
        .format()
        .block_copy_size(None)
        .expect("a format with one copyable aspect");
    let row = width * texel;
    let padded = row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(padded * height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: None,
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    readback.map_async(wgpu::MapMode::Read, .., |r| r.unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let mapped = readback.get_mapped_range(..).unwrap();
    if padded == row {
        return mapped.to_vec();
    }
    mapped
        .chunks_exact(padded as usize)
        .flat_map(|r| &r[..row as usize])
        .copied()
        .collect()
}

/// An RGBA8 image read back from the GPU, row by row from the top.
#[derive(Clone, PartialEq)]
pub(super) struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Image({}x{})", self.width, self.height)
    }
}

impl Image {
    /// Reads back an RGBA8 (or BGRA8, swizzled to RGBA) texture.
    pub fn read(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture) -> Self {
        let mut rgba = read_texture(device, queue, texture);
        match texture.format() {
            wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb => {}
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => {
                rgba.as_chunks_mut::<4>().0.iter_mut().for_each(|p| p.swap(0, 2));
            }
            other => panic!("not an 8-bit RGBA format: {other:?}"),
        }
        Self {
            width: texture.width(),
            height: texture.height(),
            rgba,
        }
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * self.width + x) * 4) as usize;
        self.rgba[at..at + 4].try_into().unwrap()
    }

    /// The fraction of the pixels in the rectangle `x` by `y` for which
    /// `test` holds.
    pub fn fraction(&self, x: std::ops::Range<u32>, y: std::ops::Range<u32>, test: impl Fn([u8; 4]) -> bool) -> f32 {
        let (mut hits, mut total) = (0usize, 0usize);
        for py in y {
            for px in x.clone() {
                total += 1;
                hits += usize::from(test(self.pixel(px, py)));
            }
        }
        assert!(total > 0, "an empty region");
        hits as f32 / total as f32
    }
}

/// Asserts that `test` holds for at least `at_least` (0..=1) of the pixels
/// in the rectangle `x` by `y`: a semantic check ("the box is lit").
pub(super) fn assert_region(
    image: &Image,
    x: std::ops::Range<u32>,
    y: std::ops::Range<u32>,
    at_least: f32,
    what: &str,
    test: impl Fn([u8; 4]) -> bool,
) {
    let (xs, ys) = (x.clone(), y.clone());
    let got = image.fraction(x, y, test);
    assert!(
        got >= at_least,
        "{what}: holds for {:.1}% of pixels {xs:?} x {ys:?}, wanted at least {:.1}%",
        got * 100.0,
        at_least * 100.0
    );
}

pub(super) use ponko_image_diff::{Diff, Tolerance};

fn view(image: &Image) -> ponko_image_diff::Image<'_> {
    ponko_image_diff::Image {
        width: image.width,
        height: image.height,
        rgba: &image.rgba,
    }
}

pub(super) fn diff(a: &Image, b: &Image, channel: u8) -> Diff {
    diff_within(
        a,
        b,
        Tolerance {
            channel,
            outliers: 0.0,
            edge: 0,
        },
    )
}

pub(super) fn diff_within(a: &Image, b: &Image, tolerance: Tolerance) -> Diff {
    ponko_image_diff::compare(&view(a), &view(b), tolerance).expect("valid render test images of equal size")
}

/// Prints `line` and appends it to `comparisons.log` in `output`: test
/// runners hide what a passing test prints, and the margins a comparison
/// passed with are what a first run on other hardware is read for.
pub(super) fn record(output: &std::path::Path, line: &str) {
    eprintln!("{line}");
    append(output, "comparisons.log", &format!("[{}] {line}", test_name()));
}

/// A reference image's metadata, stored next to it as `<name>.json`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(super) struct ReferenceMeta {
    pub name: String,
    /// What the picture shows and how it's set up, for reviewers.
    pub scene: String,
    pub width: u32,
    pub height: u32,
    /// Always 8-bit RGBA, sRGB-encoded.
    pub format: String,
    pub tolerance: Tolerance,
    /// The adapter the reference was made on.
    pub adapter: String,
    pub backend: String,
    pub driver: String,
    /// FNV-1a (64 bits, hex) of the PNG file: an image replaced without
    /// going through the update fails the check.
    pub checksum: String,
    /// Why the image was last made or changed (`PONKO_REFERENCE_REASON`).
    pub reason: String,
}

/// FNV-1a over `bytes`, as 16 hex digits (`scripts/check-reference-images.py`
/// computes the same).
pub(super) fn checksum(bytes: &[u8]) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &b in bytes {
        hash = (hash ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// The checkout's root: where the test runs from when the runner says so
/// (an archived test binary run on another machine), else where it was
/// built.
fn manifest_dir() -> PathBuf {
    std::env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .filter(|dir| dir.join("tests/fixtures/render").is_dir())
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
}

fn fixtures() -> PathBuf {
    manifest_dir().join("tests/fixtures/render")
}

/// Where failing comparisons leave their images.
pub(super) fn output_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest_dir().join("target"))
        .join("render-test-output")
}

fn save_png(path: &std::path::Path, image: &Image) {
    image::save_buffer(path, &image.rgba, image.width, image.height, image::ColorType::Rgba8)
        .unwrap_or_else(|e| panic!("writing {}: {e}", path.display()));
}

/// With `PONKO_RENDER_TEST_IMAGES=1`, saves `image` as
/// `target/render-test-output/<test>.<n>.png` (`n` counts the test's images
/// in the order it read them): what a test saw, for looking at a failure
/// or showing a change. Does nothing otherwise.
pub(super) fn keep_image(image: &Image) {
    use std::cell::Cell;
    thread_local!(static COUNT: Cell<u32> = const { Cell::new(0) });
    if !env_set("PONKO_RENDER_TEST_IMAGES") {
        return;
    }
    let n = COUNT.with(|c| c.replace(c.get() + 1));
    let name = test_name().replace("::", ".");
    let out = output_dir();
    std::fs::create_dir_all(&out).unwrap();
    save_png(&out.join(format!("{name}.{n:02}.png")), image);
}

/// Whether this run may rewrite reference images, and why it does:
/// `Ok(None)` for a normal run, `Ok(Some(reason))` for an update, `Err` for
/// an update that is not allowed (in CI, or without a reason).
pub(super) fn update_request(update: bool, reason: Option<&str>, ci: bool) -> Result<Option<String>, String> {
    if !update {
        return Ok(None);
    }
    if ci {
        return Err(
            "PONKO_UPDATE_REFERENCES is refused in CI: regenerate references locally and commit them for review"
                .to_owned(),
        );
    }
    match reason.map(str::trim) {
        Some(reason) if !reason.is_empty() => Ok(Some(reason.to_owned())),
        _ => Err(
            "PONKO_UPDATE_REFERENCES needs PONKO_REFERENCE_REASON=\"why the picture changes\": it is recorded \
             in the reference's metadata for the reviewer"
                .to_owned(),
        ),
    }
}

/// Who drew an image: the adapter's name, backend and driver.
#[derive(Clone, Debug)]
pub(super) struct Drawn {
    pub adapter: String,
    pub backend: String,
    pub driver: String,
}

impl Drawn {
    pub fn by(gpu: &wgpu::AdapterInfo) -> Self {
        Self {
            adapter: gpu.name.clone(),
            backend: format!("{:?}", gpu.backend),
            driver: driver(gpu),
        }
    }
}

/// Where the references are and where evidence and failure images go.
pub(super) struct ReferenceDirs {
    pub fixtures: PathBuf,
    pub output: PathBuf,
}

/// What a reference check did.
#[derive(Debug, PartialEq)]
pub(super) enum Checked {
    /// The image is within the tolerance of its reference.
    Matches,
    /// An update run: the image already matched, nothing was written.
    LeftAlone,
    /// An update run: the reference was made or replaced.
    Written,
}

/// `image` with the pixels beyond `tolerance` of `reference` in red and the
/// rest as dim grey.
fn difference_image(image: &Image, reference: &Image, tolerance: Tolerance) -> Image {
    Image {
        width: image.width,
        height: image.height,
        rgba: ponko_image_diff::heatmap(&view(image), &view(reference), tolerance).expect("valid reference images"),
    }
}

/// A negative control for a reference: `image`, a frame of the same scene
/// with what the reference is there to protect taken away (`what`: the
/// effect off, the object moved), must be beyond `tolerance` of the stored
/// reference, by at least twice its budget of outliers. A tolerance wide
/// enough to let it through compares nothing.
pub(super) fn reference_rejects(name: &str, image: &Image, tolerance: Tolerance, what: &str) {
    let bytes = std::fs::read(fixtures().join(format!("{name}.png")))
        .unwrap_or_else(|e| panic!("no reference image {name}.png ({e})"));
    let decoded = image::load_from_memory(&bytes).unwrap().into_rgba8();
    let reference = Image {
        width: decoded.width(),
        height: decoded.height(),
        rgba: decoded.into_raw(),
    };
    let d = diff_within(image, &reference, tolerance);
    record(
        &output_dir(),
        &format!("reference {name} against {what} (must differ): {d}; allowed {tolerance:?}"),
    );
    // Twice the budget: a control that only just fails says the budget is
    // as wide as the regression it is there to catch.
    assert!(
        d.outliers as f32 >= 2.0 * tolerance.outliers * d.pixels as f32 && !d.within(tolerance),
        "{name}: {what} is not at least twice beyond the tolerance of the reference ({d}, allowed {tolerance:?}): \
         the comparison would let it through or nearly so"
    );
}

/// Compares `image` with the reference `tests/fixtures/render/<name>.png`
/// within `tolerance` (see the module docs). `scene` describes the setup for
/// the metadata; `gpu` is the adapter that drew `image`.
pub(super) fn check_reference(name: &str, image: &Image, tolerance: Tolerance, scene: &str, gpu: &wgpu::AdapterInfo) {
    let reason = std::env::var("PONKO_REFERENCE_REASON").ok();
    let update = update_request(env_set("PONKO_UPDATE_REFERENCES"), reason.as_deref(), env_set("CI"))
        .unwrap_or_else(|e| panic!("{e}"));
    let dirs = ReferenceDirs {
        fixtures: fixtures(),
        output: output_dir(),
    };
    if let Err(e) = check_reference_in(&dirs, name, image, tolerance, scene, &Drawn::by(gpu), update.as_deref()) {
        panic!("{e}");
    }
}

/// [`check_reference`] in `dirs`, as an update with `update`'s reason or
/// as a check; `Err` is the failure's message.
pub(super) fn check_reference_in(
    dirs: &ReferenceDirs,
    name: &str,
    image: &Image,
    tolerance: Tolerance,
    scene: &str,
    drawn: &Drawn,
    update: Option<&str>,
) -> Result<Checked, String> {
    let (png, json) = (
        dirs.fixtures.join(format!("{name}.png")),
        dirs.fixtures.join(format!("{name}.json")),
    );
    let how = "regenerate locally with PONKO_UPDATE_REFERENCES=1 PONKO_REFERENCE_REASON=\"...\" and review the \
               new image (see the book's \"GPU regression tests\")";
    // What is there now, if anything.
    let existing = || -> Result<(ReferenceMeta, Image), String> {
        let text =
            std::fs::read_to_string(&json).map_err(|e| format!("no reference metadata {} ({e})", json.display()))?;
        let meta: ReferenceMeta = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", json.display()))?;
        let bytes = std::fs::read(&png).map_err(|e| format!("no reference image {} ({e})", png.display()))?;
        if checksum(&bytes) != meta.checksum {
            return Err(format!(
                "{} is not the image its metadata was made with (checksum {}, metadata says {})",
                png.display(),
                checksum(&bytes),
                meta.checksum
            ));
        }
        let decoded = image::load_from_memory(&bytes)
            .map_err(|e| format!("{}: {e}", png.display()))?
            .into_rgba8();
        let reference = Image {
            width: decoded.width(),
            height: decoded.height(),
            rgba: decoded.into_raw(),
        };
        if (meta.width, meta.height) != (reference.width, reference.height) {
            return Err(format!(
                "{}: the metadata's size doesn't match the image",
                json.display()
            ));
        }
        Ok((meta, reference))
    };
    // Whether `image` is what the reference holds, as this code describes it.
    let compare = |meta: &ReferenceMeta, reference: &Image| -> Result<Diff, String> {
        if meta.tolerance != tolerance {
            return Err(format!(
                "{name}: the tolerance in code ({tolerance:?}) differs from the reference's metadata ({:?})",
                meta.tolerance
            ));
        }
        if meta.scene != scene {
            return Err(format!(
                "{name}: the scene described in code differs from the reference's metadata"
            ));
        }
        if (image.width, image.height) != (reference.width, reference.height) {
            return Err(format!(
                "{name}: drawn at {}x{}, its reference is {}x{}",
                image.width, image.height, reference.width, reference.height
            ));
        }
        Ok(diff_within(image, reference, tolerance))
    };

    if let Some(reason) = update {
        let before = existing();
        if let Ok((meta, reference)) = &before
            && compare(meta, reference).is_ok_and(|d| d.within(tolerance))
        {
            eprintln!("reference {name}: still matches, left alone");
            return Ok(Checked::LeftAlone);
        }
        std::fs::create_dir_all(&dirs.fixtures).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&dirs.output).map_err(|e| e.to_string())?;
        // The evidence a reviewer needs: what it was, what it is, where
        // they differ.
        let after = dirs.output.join(format!("{name}.after.png"));
        save_png(&after, image);
        let mut evidence = format!("after {}", after.display());
        if let Ok((meta, reference)) = &before {
            let old = dirs.output.join(format!("{name}.before.png"));
            save_png(&old, reference);
            evidence = format!("before {}, {evidence}", old.display());
            if (image.width, image.height) == (reference.width, reference.height) {
                let changed = dirs.output.join(format!("{name}.diff.png"));
                save_png(&changed, &difference_image(image, reference, tolerance));
                evidence = format!(
                    "{evidence}, difference {} ({}; made on {}, {})",
                    changed.display(),
                    diff_within(image, reference, tolerance),
                    meta.adapter,
                    meta.backend
                );
            }
        }
        // A reference whose picture still matches but whose description or
        // tolerance changed keeps its image: only the metadata is rewritten.
        let same_picture = before.as_ref().is_ok_and(|(_, reference)| {
            (image.width, image.height) == (reference.width, reference.height)
                && diff_within(image, reference, tolerance).within(tolerance)
        });
        if same_picture {
            evidence = format!("the image is unchanged, only its metadata; {evidence}");
        } else {
            save_png(&png, image);
        }
        let bytes = std::fs::read(&png).map_err(|e| e.to_string())?;
        let meta = ReferenceMeta {
            name: name.to_owned(),
            scene: scene.to_owned(),
            width: image.width,
            height: image.height,
            format: "rgba8-srgb".to_owned(),
            tolerance,
            adapter: drawn.adapter.clone(),
            backend: drawn.backend.clone(),
            driver: drawn.driver.clone(),
            checksum: checksum(&bytes),
            reason: reason.to_owned(),
        };
        std::fs::write(&json, serde_json::to_string_pretty(&meta).unwrap() + "\n").map_err(|e| e.to_string())?;
        eprintln!(
            "updated reference {}: look at it, and put the images in the pull request ({evidence})",
            png.display()
        );
        return Ok(Checked::Written);
    }

    let (meta, reference) = existing().map_err(|e| format!("{e}: {how}"))?;
    let d = compare(&meta, &reference).map_err(|e| format!("{e}; {how}"))?;
    record(
        &dirs.output,
        &format!(
            "reference {name}: {d}; allowed {tolerance:?} (made on {}, {}; drawn on {}, {})",
            meta.adapter, meta.backend, drawn.adapter, drawn.backend
        ),
    );
    if d.within(tolerance) {
        return Ok(Checked::Matches);
    }
    std::fs::create_dir_all(&dirs.output).map_err(|e| e.to_string())?;
    let actual = dirs.output.join(format!("{name}.actual.png"));
    let difference = dirs.output.join(format!("{name}.diff.png"));
    save_png(&actual, image);
    save_png(&difference, &difference_image(image, &reference, tolerance));
    Err(format!(
        "{name} differs from its reference: {d}, allowed {tolerance:?}. Reference made on {} ({}, driver {}), \
         drawn on {} ({}, driver {}). See {} and {}; if the change is intended, {how}",
        meta.adapter,
        meta.backend,
        meta.driver,
        drawn.adapter,
        drawn.backend,
        drawn.driver,
        actual.display(),
        difference.display()
    ))
}

#[test]
fn diff_counts_pixels_beyond_the_channel_tolerance() {
    let a = Image {
        width: 2,
        height: 2,
        rgba: vec![0, 0, 0, 255, 10, 10, 10, 255, 100, 100, 100, 255, 255, 255, 255, 255],
    };
    let mut b = a.clone();
    b.rgba[4] = 13; // within 3
    b.rgba[8] = 120; // beyond
    let d = diff(&a, &b, 3);
    assert_eq!((d.max_channel, d.outliers, d.pixels), (20, 1, 4));
    assert!(d.within(Tolerance {
        channel: 3,
        outliers: 0.25,
        edge: 0,
    }));
    assert!(!d.within(Tolerance {
        channel: 3,
        outliers: 0.2,
        edge: 0,
    }));
    assert!((d.mean - 23.0 / 16.0).abs() < 1e-6);
}

/// The allowance for edges: an edge a pixel apart is the same picture, one
/// two pixels apart is not, and neither a changed shade nor a thin line
/// missing from either image hides in it.
#[test]
fn an_edge_a_pixel_apart_is_within_the_tolerance_and_a_changed_shade_is_not() {
    // 16x16, black left of column `edge` and white from it on.
    let split = |edge: u32| Image {
        width: 16,
        height: 16,
        rgba: (0..16 * 16)
            .flat_map(|i| if i % 16 < edge { [0, 0, 0, 255] } else { [255; 4] })
            .collect(),
    };
    let (in_place, near) = (
        Tolerance {
            channel: 8,
            outliers: 0.0,
            edge: 0,
        },
        Tolerance {
            channel: 8,
            outliers: 0.0,
            edge: 1,
        },
    );
    let (reference, moved_one, moved_two) = (split(8), split(9), split(10));
    assert_eq!(diff_within(&moved_one, &reference, in_place).outliers, 16);
    let d = diff_within(&moved_one, &reference, near);
    assert_eq!((d.outliers, d.beyond, d.max_channel), (0, 0, 255), "{d}");
    // Two columns of the reference touch its edge.
    assert_eq!(d.edges, 32, "{d}");
    assert!(d.within(near));
    // Two pixels apart: the column that is neither image's neighbour.
    let d = diff_within(&moved_two, &reference, near);
    assert_eq!(d.outliers, 32, "{d}");
    assert!(!d.within(near));
    // An antialiased edge: a shade between the two sides is within.
    let mut soft = split(8);
    for y in 0..16 {
        soft.rgba[(y * 16 + 8) * 4..][..3].fill(120);
    }
    assert_eq!(diff_within(&soft, &reference, near).outliers, 0);
    // A shade changed away from any edge is beyond it as before.
    let mut shaded = split(8);
    for y in 0..16 {
        shaded.rgba[(y * 16 + 2) * 4..][..3].fill(40);
    }
    let d = diff_within(&shaded, &reference, near);
    assert_eq!((d.outliers, d.beyond), (16, 40), "{d}");
    // A one-pixel line the reference has and the frame lacks, and the
    // other way round, both count.
    let mut line = split(8);
    for x in 0..5 {
        line.rgba[(12 * 16 + x) * 4..][..3].fill(200);
    }
    assert_eq!(diff_within(&reference, &line, near).outliers, 5);
    assert_eq!(diff_within(&line, &reference, near).outliers, 5);
    // The difference image marks the same pixels.
    let marked = difference_image(&line, &reference, near);
    let red = marked
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| **p == [255, 0, 0, 255])
        .count();
    assert_eq!(red, 5);
}

#[test]
fn semantic_regions_report_the_fraction_that_holds() {
    let image = Image {
        width: 2,
        height: 1,
        rgba: vec![255, 0, 0, 255, 0, 0, 0, 255],
    };
    assert_eq!(image.fraction(0..2, 0..1, |p| p[0] > 128), 0.5);
    assert_region(&image, 0..1, 0..1, 1.0, "the left pixel is red", |p| p[0] > 128);
}

#[test]
fn a_test_thread_that_panics_is_marked_so_its_devices_are_left_alive() {
    kept::watch();
    let marked = std::thread::spawn(|| {
        let before = kept::panicked();
        let _ = std::panic::catch_unwind(|| panic!("a failing test (expected in this test's output)"));
        (before, kept::panicked())
    })
    .join()
    .unwrap();
    assert_eq!(marked, (false, true), "unmarked before, marked after the panic");
    assert!(!kept::panicked(), "other threads stay unmarked");
}

#[test]
fn a_test_without_a_device_fails_once_the_run_has_opened_one_or_one_is_required() {
    assert_eq!(
        no_gpu_outcome(false, false),
        NoGpu::Skip,
        "a machine without a GPU skips"
    );
    assert!(
        matches!(no_gpu_outcome(false, true), NoGpu::Fail(why) if why.contains("lost")),
        "a device lost mid-run fails instead of passing silently"
    );
    assert!(matches!(no_gpu_outcome(true, false), NoGpu::Fail(why) if why.contains("PONKO_REQUIRE_GPU")));
    assert!(matches!(no_gpu_outcome(true, true), NoGpu::Fail(_)));
}

#[test]
fn every_skip_is_appended_to_the_log_a_run_counts() {
    let output = std::env::temp_dir().join(format!("ponko-skips-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&output);
    append(&output, "skips.log", "render test SKIPPED [a]: no GPU adapter");
    append(&output, "skips.log", "render test SKIPPED [b]: needs Compute");
    let log = std::fs::read_to_string(output.join("skips.log")).unwrap();
    assert_eq!(
        log.lines().filter(|l| l.starts_with("render test SKIPPED [")).count(),
        2,
        "{log}"
    );
    let _ = std::fs::remove_dir_all(&output);
}

#[test]
fn no_more_tests_than_the_limit_hold_a_device_at_once() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    static LIMITER: at_once::Limiter = at_once::Limiter::new();
    static INSIDE: AtomicUsize = AtomicUsize::new(0);
    static MOST: AtomicUsize = AtomicUsize::new(0);
    let threads: Vec<_> = (0..8)
        .map(|_| {
            std::thread::spawn(|| {
                for _ in 0..5 {
                    let _place = LIMITER.take(2, Duration::from_secs(60));
                    let now = INSIDE.fetch_add(1, Ordering::SeqCst) + 1;
                    MOST.fetch_max(now, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(2));
                    INSIDE.fetch_sub(1, Ordering::SeqCst);
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    assert!(
        MOST.load(Ordering::SeqCst) <= 2,
        "eight threads, two places: at most two at once"
    );
    assert_eq!(LIMITER.held(), 0, "every place is given back");
}

#[test]
fn a_test_threads_place_is_given_back_when_its_thread_ends() {
    use std::time::Duration;
    static LIMITER: at_once::Limiter = at_once::Limiter::new();
    std::thread::spawn(|| {
        at_once::enter(&LIMITER, 1, Duration::from_secs(60));
        at_once::enter(&LIMITER, 1, Duration::from_secs(60)); // a second device: no second place
        assert_eq!(LIMITER.held(), 1, "one place per test");
    })
    .join()
    .unwrap();
    assert_eq!(LIMITER.held(), 0, "the ended thread gave its place back");
}

#[test]
fn a_test_that_waits_on_itself_opens_late_instead_of_hanging() {
    use std::time::{Duration, Instant};
    static LIMITER: at_once::Limiter = at_once::Limiter::new();
    let first = LIMITER.take(1, Duration::from_secs(60));
    let started = Instant::now();
    let second = LIMITER.take(1, Duration::from_millis(50));
    assert!(
        started.elapsed() >= Duration::from_millis(50),
        "it waited its patience out first"
    );
    assert_eq!(LIMITER.held(), 2);
    drop((first, second));
    assert_eq!(LIMITER.held(), 0);
}

/// A 4x4 grey image, and what a workflow test checks references with.
#[cfg(test)]
mod reference_workflow {
    use super::*;

    const TOLERANCE: Tolerance = Tolerance {
        channel: 4,
        outliers: 0.0,
        edge: 0,
    };

    fn grey(level: u8) -> Image {
        Image {
            width: 4,
            height: 4,
            rgba: [level, level, level, 255].repeat(16),
        }
    }

    fn drawn() -> Drawn {
        Drawn {
            adapter: "Test adapter".to_owned(),
            backend: "None".to_owned(),
            driver: "unknown".to_owned(),
        }
    }

    /// Empty fixture and output folders of this test's own.
    fn dirs() -> ReferenceDirs {
        let root = output_dir().join("workflow-tests").join(test_name().replace("::", "."));
        let _ = std::fs::remove_dir_all(&root);
        ReferenceDirs {
            fixtures: root.join("fixtures"),
            output: root.join("output"),
        }
    }

    fn check(dirs: &ReferenceDirs, image: &Image, update: Option<&str>) -> Result<Checked, String> {
        check_reference_in(dirs, "sample", image, TOLERANCE, "A grey square.", &drawn(), update)
    }

    #[test]
    fn a_missing_reference_fails_and_says_how_to_make_one() {
        let dirs = dirs();
        let e = check(&dirs, &grey(100), None).unwrap_err();
        assert!(
            e.contains("no reference metadata") && e.contains("PONKO_UPDATE_REFERENCES=1"),
            "{e}"
        );
        assert!(!dirs.fixtures.exists(), "a check writes nothing");
    }

    #[test]
    fn an_update_is_refused_in_ci_and_without_a_reason() {
        assert_eq!(update_request(false, None, true), Ok(None));
        assert_eq!(update_request(false, Some("ignored"), false), Ok(None));
        assert_eq!(
            update_request(true, Some(" new shadow filter "), false),
            Ok(Some("new shadow filter".to_owned()))
        );
        let in_ci = update_request(true, Some("new shadow filter"), true).unwrap_err();
        assert!(in_ci.contains("refused in CI"), "{in_ci}");
        for reason in [None, Some(""), Some("  ")] {
            let e = update_request(true, reason, false).unwrap_err();
            assert!(e.contains("PONKO_REFERENCE_REASON"), "{e}");
        }
    }

    #[test]
    fn an_update_records_its_reason_and_leaves_before_after_and_difference_images() {
        let dirs = dirs();
        assert_eq!(check(&dirs, &grey(100), Some("first picture")), Ok(Checked::Written));
        assert_eq!(check(&dirs, &grey(100), None), Ok(Checked::Matches));
        // A first reference has nothing before it.
        assert!(dirs.output.join("sample.after.png").exists());
        assert!(!dirs.output.join("sample.before.png").exists());

        assert_eq!(check(&dirs, &grey(180), Some("brighter light")), Ok(Checked::Written));
        for evidence in ["sample.before.png", "sample.after.png", "sample.diff.png"] {
            assert!(dirs.output.join(evidence).exists(), "{evidence} is saved");
        }
        let before = image::open(dirs.output.join("sample.before.png")).unwrap().into_rgba8();
        assert_eq!(before.get_pixel(0, 0).0, [100, 100, 100, 255], "the old picture");
        let meta: ReferenceMeta =
            serde_json::from_str(&std::fs::read_to_string(dirs.fixtures.join("sample.json")).unwrap()).unwrap();
        assert_eq!(meta.reason, "brighter light");
        assert_eq!((meta.adapter.as_str(), meta.width, meta.height), ("Test adapter", 4, 4));
        assert_eq!(
            meta.checksum,
            checksum(&std::fs::read(dirs.fixtures.join("sample.png")).unwrap())
        );
        assert_eq!(check(&dirs, &grey(180), None), Ok(Checked::Matches));
    }

    #[test]
    fn an_update_leaves_a_reference_that_still_matches_alone() {
        let dirs = dirs();
        check(&dirs, &grey(100), Some("first picture")).unwrap();
        let written = std::fs::read(dirs.fixtures.join("sample.json")).unwrap();
        // Within the tolerance: no churn, and the reason on record stays.
        assert_eq!(
            check(&dirs, &grey(102), Some("an unrelated update run")),
            Ok(Checked::LeftAlone)
        );
        assert_eq!(std::fs::read(dirs.fixtures.join("sample.json")).unwrap(), written);
    }

    #[test]
    fn a_frame_beyond_the_tolerance_fails_naming_both_adapters_and_leaves_the_images() {
        let dirs = dirs();
        check(&dirs, &grey(100), Some("first picture")).unwrap();
        assert_eq!(
            check(&dirs, &grey(104), None),
            Ok(Checked::Matches),
            "within the tolerance"
        );
        let e = check(&dirs, &grey(105), None).unwrap_err();
        assert!(
            e.contains("differs from its reference") && e.contains("max channel difference 5"),
            "{e}"
        );
        assert!(
            e.contains("Reference made on Test adapter") && e.contains("drawn on Test adapter"),
            "{e}"
        );
        assert!(dirs.output.join("sample.actual.png").exists() && dirs.output.join("sample.diff.png").exists());
        // The reference itself is untouched by a failing check.
        assert_eq!(check(&dirs, &grey(100), None), Ok(Checked::Matches));
    }

    #[test]
    fn a_changed_tolerance_scene_or_size_fails_until_regenerated() {
        let dirs = dirs();
        check(&dirs, &grey(100), Some("first picture")).unwrap();
        let looser = Tolerance {
            channel: 40,
            outliers: 0.5,
            edge: 0,
        };
        let e = check_reference_in(&dirs, "sample", &grey(100), looser, "A grey square.", &drawn(), None).unwrap_err();
        assert!(e.contains("tolerance in code"), "{e}");
        let e =
            check_reference_in(&dirs, "sample", &grey(100), TOLERANCE, "Another scene.", &drawn(), None).unwrap_err();
        assert!(e.contains("scene described in code"), "{e}");
        let wide = Image {
            width: 8,
            height: 2,
            rgba: [100, 100, 100, 255].repeat(16),
        };
        let e = check(&dirs, &wide, None).unwrap_err();
        assert!(e.contains("drawn at 8x2"), "{e}");
        // Regenerating with the looser tolerance records it.
        assert_eq!(
            check_reference_in(
                &dirs,
                "sample",
                &grey(100),
                looser,
                "A grey square.",
                &drawn(),
                Some("looser")
            ),
            Ok(Checked::Written)
        );
    }

    #[test]
    fn an_image_swapped_without_its_metadata_fails() {
        let dirs = dirs();
        check(&dirs, &grey(100), Some("first picture")).unwrap();
        save_png(&dirs.fixtures.join("sample.png"), &grey(101));
        let e = check(&dirs, &grey(101), None).unwrap_err();
        assert!(e.contains("not the image its metadata was made with"), "{e}");
    }

    #[test]
    fn the_checksum_is_fnv1a_64() {
        // The published test vectors, which the CI script's copy shares.
        assert_eq!(checksum(b""), "cbf29ce484222325");
        assert_eq!(checksum(b"a"), "af63dc4c8601ec8c");
        assert_eq!(checksum(b"foobar"), "85944171f73967e8");
    }

    /// Every `*_render_test.rs` here, with its text.
    fn suites() -> Vec<(String, String)> {
        let dir = manifest_dir().join("src/render");
        let mut suites: Vec<(String, String)> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| {
                let path = entry.unwrap().path();
                let name = path.file_name()?.to_str()?.to_owned();
                name.ends_with("_render_test.rs")
                    .then(|| (name, std::fs::read_to_string(&path).unwrap()))
            })
            .collect();
        suites.sort();
        suites
    }

    /// Every Rust file under `folder` (relative to the package), with its text.
    fn sources(folder: &str) -> Vec<(String, String)> {
        fn walk(dir: &std::path::Path, root: &std::path::Path, found: &mut Vec<(String, String)>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(&path, root, found);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let name = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
                    found.push((name, std::fs::read_to_string(&path).unwrap()));
                }
            }
        }
        let root = manifest_dir();
        let mut found = Vec::new();
        walk(&root.join(folder), &root, &mut found);
        found.sort();
        found
    }

    /// The files that may open a GPU device themselves, and why.
    const OWN_DEVICE: [(&str, &str); 11] = [
        ("src/render/gpu.rs", "the engine's own device"),
        (
            "src/render/hdr_metadata.rs",
            "surfaces on a bare CAMetalLayer and a hidden DX12 window need adapters compatible with them; \
             no adapter goes through no_gpu",
        ),
        ("src/render/render_test_support.rs", "the shared helpers"),
        (
            "src/render/capabilities_render_test.rs",
            "forces its own tiers on the adapter, and reports a missing one through no_gpu",
        ),
        (
            "src/render/browser_gpu.rs",
            "the engine's own device in a browser: WebGPU first, WebGL2 as the fallback",
        ),
        (
            "src/three/bake/gpu.rs",
            "the GPU light baker's own device: a game calls it without a window, and no GPU is its `Unsupported` error",
        ),
        ("src/render/lights_bench.rs", "an ignored benchmark, run by hand"),
        ("src/render/gpu3d/record_bench.rs", "benchmark support, not a test"),
        (
            "src/render/gpu3d/scene_bench.rs",
            "benchmark support: `SceneBench::new` opens the high-performance adapter the benchmark reports; \
             its tests open theirs with gpu_with",
        ),
        (
            "src/render/gpu3d/lod_bench.rs",
            "benchmark support: `LodBench::new` opens the high-performance adapter the benchmark reports \
             and applies PONKO_GPU_TIER; its test reports a missing one through no_gpu",
        ),
        (
            "src/render/lights2d_bench.rs",
            "benchmark support: `Lights2DBench::new` opens the high-performance adapter the benchmark \
             reports and applies PONKO_GPU_TIER; its test reports a missing one through no_gpu",
        ),
    ];

    /// Not only the `*_render_test.rs` suites: a GPU test inside any module
    /// (a `mod tests` next to the code it checks) follows the same rule.
    #[test]
    fn every_suite_opens_its_device_through_the_shared_helpers() {
        let suites = suites();
        assert!(suites.len() >= 30, "the suites are found: {}", suites.len());
        let mut files = sources("src");
        files.extend(sources("tests"));
        assert!(files.len() > suites.len(), "the sources are found: {}", files.len());
        for (name, why) in OWN_DEVICE {
            let (_, text) = files
                .iter()
                .find(|(file, _)| file == name)
                .unwrap_or_else(|| panic!("{name} ({why}) is gone: take it off the list"));
            assert!(
                text.contains("request_adapter"),
                "{name} ({why}) opens no device any more: take it off the list"
            );
        }
        for (name, why) in OWN_DEVICE {
            if name == "src/render/browser_gpu.rs" || name == "src/render/render_test_support.rs" {
                continue; // the browser's device (no native tests), and the helpers
            }
            let text = &files.iter().find(|(file, _)| file == name).unwrap().1;
            assert_eq!(
                text.matches("wgpu::Instance::new(").count(),
                text.matches("render_test_support::take_gpu_place();").count(),
                "{name} ({why}) opens a device without taking a place among the GPU tests first:                  call render_test_support::take_gpu_place() before each wgpu::Instance::new"
            );
        }
        let capabilities = &files.iter().find(|(file, _)| file == OWN_DEVICE[2].0).unwrap().1;
        assert!(
            capabilities.contains("render_test_support::no_gpu"),
            "the capability suite reports a missing adapter"
        );
        for (name, text) in &files {
            if OWN_DEVICE.iter().any(|(file, _)| file == name) {
                continue;
            }
            for direct in ["request_adapter", "request_device", "wgpu::Instance::new"] {
                assert!(
                    !text.contains(direct),
                    "{name} calls {direct} itself: open the device with render_test_support::gpu or gpu_with, \
                     so a missing adapter is logged, PONKO_REQUIRE_GPU fails it and PONKO_GPU_TIER applies"
                );
            }
            assert!(
                !text.contains(": skipped\""),
                "{name} prints its own skip: call render_test_support::skip with the reason"
            );
        }
    }

    #[test]
    fn every_reference_image_is_checked_by_a_test_and_described() {
        let all: String = suites().into_iter().map(|(_, text)| text).collect();
        let mut names = Vec::new();
        for entry in std::fs::read_dir(fixtures()).unwrap() {
            let path = entry.unwrap().path();
            let stem = path.file_stem().unwrap().to_str().unwrap().to_owned();
            match path.extension().and_then(|e| e.to_str()) {
                Some("json") => {
                    let meta: ReferenceMeta = serde_json::from_str(&std::fs::read_to_string(&path).unwrap())
                        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                    assert_eq!(meta.name, stem, "{}", path.display());
                    assert!(meta.scene.len() > 40, "{stem}: the scene is described for reviewers");
                    assert!(!meta.reason.trim().is_empty(), "{stem}: why it was made is on record");
                    assert!(meta.width <= 256 && meta.height <= 256, "{stem}: references stay small");
                    assert!(path.with_extension("png").exists(), "{stem}.json has its image");
                    names.push(stem);
                }
                Some("png") => assert!(path.with_extension("json").exists(), "{stem}.png has its metadata"),
                _ => panic!(
                    "{}: only reference images and their metadata belong here",
                    path.display()
                ),
            }
        }
        assert!(names.len() >= 10, "the references are found: {names:?}");
        for name in names {
            assert!(
                all.contains(&format!("\"{name}\"")),
                "{name} is checked by no test: remove it, or check it with check_reference"
            );
        }
    }
}
