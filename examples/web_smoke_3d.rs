//! The 3D browser smoke-test fixture: `web_smoke` for the 3D renderer
//! (`web/smoke/fixtures.json`, built with `--features 3d`).
//!
//! It loads one glTF model (`models/house.glb`, fetched over HTTP on the web
//! and parsed in the browser), draws it with a sun with shadows, a few
//! spheres with different materials and a ground plane, and orbits the camera
//! from the fixed-step simulation. The state it publishes (see
//! `common/web_smoke.rs`) adds the triangles and models drawn last frame, so a
//! scene that loaded but draws nothing is caught:
//!
//! `{"fixture":"web_smoke_3d","ticks":120,"frames":118,"clicks":1,"size":[800,600],"triangles":1234,"models_drawn":8,"frame_stats":{...}}`
//!
//! `half_float_readback` is the outcome of reading a half-float render target
//! (`Rgba16Float`, which a target camera drew an emissive box into) back on the
//! CPU with `Graphics::read_render_target`: `"pending"`, then `"ok"` when the
//! texels are the ones written, `"unsupported: <why>"` when the browser's GPU
//! can't read it back (the engine's `ErrorKind::Unsupported` answer, which the
//! runner logs as a skip) or `"wrong: <what>"` when it read other values
//! (a failure; see `web/smoke/fixtures.json`, `readback`).
//!
//! `frame_stats` is the engine's own account of the last few seconds
//! (`ctx.perf.frame_stats()`: median and 95th-percentile frame time, CPU
//! time in systems and in drawing, display wait, and GPU time where the
//! browser times GPU passes), which `web/smoke/frame-times.mjs` reports
//! beside the frame times it measures in the page.
//!
//! The failure cases are `web_smoke`'s. On the desktop it opens a window
//! (`cargo run --example web_smoke_3d --features 3d`).

#[allow(unused_imports)]
#[cfg(feature = "3d")]
use ponko::d3::*;
use ponko::prelude::*;
use ponko::render::{MaterialDesc, MeshData, ModelMaterial, Readback, RenderTarget, TargetCamera};
#[allow(unused_imports)]
#[cfg(feature = "3d")]
use serde_json::json;
use std::cell::Cell;
use std::rc::Rc;

#[path = "common/web_smoke.rs"]
mod smoke;

fn main() {
    let ticks = Rc::new(Cell::new(0u64));
    let sim_ticks = ticks.clone();
    Game::new("Ponko: 3D web smoke", 800, 600)
        .fixed_hz(60)
        .sim_system(move |_ctx| sim_ticks.set(sim_ticks.get() + 1))
        .system(frame(ticks))
        .setup(|ctx| {
            let smoke = smoke::Smoke::new("web_smoke_3d");
            ctx.scene3d.lighting.sun_direction = vec3(-0.8, -0.6, -0.4);
            let ground_material = ctx
                .gfx
                .create_material(MaterialDesc::color(Color::hex(0x5E8C4A)).with_roughness(0.95));
            let ground = ctx.gfx.create_mesh(&MeshData::plane(60.0), ground_material);
            ctx.world.spawn((Transform::default(), Model::new(ground)));

            // The fetched asset: a model with its own materials and textures.
            let house = ctx.gfx.load_mesh("models/house.glb");
            ctx.world
                .spawn((Transform::at(vec3(0.0, 0.0, -3.0)), Model::new(house)));

            let white = ctx.gfx.default_material();
            let ball = ctx.gfx.create_mesh(&MeshData::sphere(0.45, 24), white);
            for i in 0..6 {
                let material = ctx.gfx.create_material(
                    MaterialDesc::color(Color::hex(0xD8453B))
                        .with_roughness(0.1 + i as f32 * 0.17)
                        .with_metallic((i % 2) as f32),
                );
                ctx.world.spawn((
                    Transform::at(vec3(i as f32 * 1.1 - 2.75, 0.45, 2.0)),
                    Model::new(ball),
                    ModelMaterial(material),
                ));
            }

            // A sphere with a normal map and an ORM map, made in memory: data
            // textures, which a browser with only WebGL2 must read as stored
            // (not sRGB-decoded) to light the bumps.
            let (bumps, orm) = data_maps();
            let normal_map = ctx.gfx.create_texture(16, 16, &bumps);
            let orm_map = ctx.gfx.create_texture(16, 16, &orm);
            let bumpy = ctx.gfx.create_material(
                MaterialDesc::color(Color::hex(0xD8D8D8))
                    .with_normal_map(normal_map)
                    .with_orm_map(orm_map),
            );
            let bumpy_ball = ctx.gfx.create_mesh(&MeshData::sphere(0.9, 32), bumpy);
            ctx.world
                .spawn((Transform::at(vec3(3.0, 0.9, -1.0)), Model::new(bumpy_ball)));
            let half_floats = HalfFloats::start(ctx);
            ctx.world.insert_resource(Fixture { smoke, half_floats });
        })
        .run();
}

/// The fixture's counters, made in setup for the frame system.
struct Fixture {
    smoke: smoke::Smoke,
    half_floats: HalfFloats,
}

/// Width and height of the half-float target the read-back check uses.
const TARGET_SIZE: u32 = 16;

/// The colour of the check's emissive box, as colours are written (sRGB).
const EMISSIVE: [f32; 3] = [0.8, 0.5, 0.2];

/// How far the centre texels may be from the box's linear light: the box is
/// also lit by the scene's light (a sheen of about 0.01 on every channel);
/// the half float's own error is far smaller.
const TOLERANCE: f32 = 0.03;

/// An sRGB-encoded channel as linear light, which is what the render target
/// holds.
fn linear(srgb: f32) -> f32 {
    if srgb <= 0.04045 {
        srgb / 12.92
    } else {
        ((srgb + 0.055) / 1.055).powf(2.4)
    }
}

/// The read-back check: a target camera draws an emissive box, far from the
/// scene, into a half-float (`Rgba16Float`) render target, which the game
/// reads back on the CPU and compares with the light the box gives off.
/// Target cameras draw on WebGL2, which has no compute passes.
struct HalfFloats {
    target: RenderTarget,
    pending: Option<Readback>,
    /// What the state publishes: see the module's `half_float_readback`.
    outcome: String,
}

impl HalfFloats {
    /// Setup: the target, the box and the camera that draws it.
    fn start(ctx: &mut Ctx) -> Self {
        let target = ctx.gfx.create_render_target(TARGET_SIZE, TARGET_SIZE);
        let [r, g, b] = EMISSIVE;
        let glow = ctx
            .gfx
            .create_material(MaterialDesc::color(Color::BLACK).with_emissive(Color::rgb(r, g, b)));
        let glowing = ctx.gfx.create_mesh(&MeshData::cube(8.0), glow);
        // Far from everything the main camera looks at.
        let at = vec3(400.0, 200.0, 400.0);
        ctx.world.spawn((Transform::at(at), Model::new(glowing)));
        let mut camera = ctx.scene3d.camera;
        camera.position = at + vec3(0.0, 0.0, 6.0);
        camera.look_at(at);
        ctx.scene3d
            .passes
            .target_cameras
            .push(TargetCamera::new(camera, target));
        Self {
            target,
            pending: None,
            outcome: "pending".into(),
        }
    }

    /// One frame: asks for the target's centre once the box has been drawn
    /// a few times, then waits for the answer.
    fn frame(&mut self, ctx: &mut Ctx, ticks: u64) {
        if self.outcome != "pending" || ticks < 10 {
            return;
        }
        let target = self.target;
        let middle = TARGET_SIZE / 2 - 1;
        let pending = *self
            .pending
            .get_or_insert_with(|| ctx.gfx.read_render_target(target, uvec2(middle, middle), uvec2(2, 2)));
        let Some(answer) = ctx.gfx.readback(pending) else {
            return;
        };
        self.outcome = match answer {
            Ok(texels) => compare(texels),
            Err(e) if e.kind() == ErrorKind::Unsupported => format!("unsupported: {}", e.message()),
            Err(e) => format!("wrong: the read failed: {e}"),
        };
    }
}

/// `"ok"` when the `texels` hold the box's light, else what differs.
fn compare(texels: &[Vec4]) -> String {
    let [r, g, b] = EMISSIVE;
    let want = vec3(linear(r), linear(g), linear(b));
    if texels.len() != 4 {
        return format!("wrong: read {} texels, expected 4", texels.len());
    }
    for (i, got) in texels.iter().enumerate() {
        if !got.truncate().abs_diff_eq(want, TOLERANCE) {
            return format!("wrong: texel {i} is {got:?}, expected about {want:?}");
        }
    }
    "ok".into()
}

/// The frame system: moves the camera and publishes the state.
fn frame(ticks: Rc<Cell<u64>>) -> impl FnMut(&mut Ctx) {
    move |ctx: &mut Ctx| {
        // The camera orbits on the simulation clock, so a stalled
        // simulation also freezes the picture.
        let angle = ticks.get() as f32 / 240.0;
        ctx.scene3d.camera.position = vec3(angle.sin() * 12.0, 6.0, angle.cos() * 12.0);
        ctx.scene3d.camera.look_at(vec3(0.0, 1.0, 0.0));
        // Taken out while it runs, as `Smoke::frame` needs all of `ctx`.
        let mut fixture = ctx.world.remove_resource::<Fixture>().expect("made in setup");
        fixture.half_floats.frame(ctx, ticks.get());
        fixture.smoke.frame(
            ctx,
            ticks.get(),
            json!({
                "half_float_readback": fixture.half_floats.outcome,
                "triangles": ctx.perf.render_stats().triangles,
                "models_drawn": ctx.gfx.models_drawn(),
                "frame_stats": frame_stats(ctx),
            }),
        );
        ctx.world.insert_resource(fixture);
    }
}

/// The engine's frame statistics, for the frame-times runner.
#[cfg(feature = "3d")]
fn frame_stats(ctx: &Ctx) -> serde_json::Value {
    let s = ctx.perf.frame_stats();
    json!({
        "frames": s.frames,
        "p50_ms": s.p50_ms,
        "p95_ms": s.p95_ms,
        "systems_ms": s.systems_ms,
        "render_cpu_ms": s.render_cpu_ms,
        "display_wait_ms": s.display_wait_ms,
        "gpu_ms": s.gpu_ms,
    })
}

/// A 16x16 tangent-space normal map of a grid of round bumps, and an ORM map
/// (occlusion, roughness, metallic) that varies across it.
fn data_maps() -> (Vec<u8>, Vec<u8>) {
    let mut normals = Vec::with_capacity(16 * 16 * 4);
    let mut orm = Vec::with_capacity(16 * 16 * 4);
    for y in 0..16 {
        for x in 0..16 {
            // The slope of a bump 8 texels wide, as a unit normal in 0..255.
            let (u, v) = ((x % 8) as f32 - 3.5, (y % 8) as f32 - 3.5);
            let n = vec3(-u * 0.12, -v * 0.12, 1.0).normalize();
            normals.extend([
                (n.x * 127.5 + 127.5) as u8,
                (n.y * 127.5 + 127.5) as u8,
                (n.z * 127.5 + 127.5) as u8,
                255,
            ]);
            orm.extend([255, (60 + x * 12) as u8, if y < 8 { 0 } else { 255 }, 255]);
        }
    }
    (normals, orm)
}
