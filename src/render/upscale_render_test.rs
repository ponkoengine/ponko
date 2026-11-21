//! Offscreen GPU tests of the temporal upscaler (`Upscaler::Temporal`):
//! it rebuilds detail closer to the native picture than the stretch, leaves
//! no trail behind what moves, and runs only with TAA and a render scale
//! below 1. They skip without a GPU adapter.
//!
//! The same scenes drive the evaluation behind the guide's numbers (ignored
//! tests, run by hand): `upscaling_evaluation` compares the stretch and the
//! temporal upscaler at 50% and 67% with a supersampled native picture on
//! moving scenes and saves the images; `upscaling_gpu_time` measures the
//! frame's GPU time at 1440p and 4K.
//!
//! ```sh
//! PONKO_UPSCALE_EVAL_OUT=/some/dir cargo test --release -p ponko --features 3d --lib \
//!     upscale_render_test -- --ignored --nocapture --test-threads 1
//! ```
use super::render_test_support::{self as support, Image};
use super::static_batch_render_test::{BatchRig, RigSetup, glow};
use crate::d3::Transform as Transform3D;
use crate::prelude::*;
use crate::render::{AntiAliasing, GpuFeature, MaterialDesc, Mesh, MeshData, Model, Upscaler};
use crate::three::Blend;

/// The moving scenes the upscalers are compared on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Scene {
    /// Nothing moves: how close the picture converges.
    Still,
    /// The camera slides sideways past striped walls, thin poles and a floor.
    Pan,
    /// Thin poles sweep across and a thin bar turns, in front of the stripes.
    ThinMotion,
    /// A box crosses the stripes fast: what it uncovers has no history.
    Disocclusion,
    /// Cut-out leaves (alpha tested in the shader) sway while the camera drifts.
    Foliage,
}

impl Scene {
    const ALL: [Scene; 5] = [
        Scene::Still,
        Scene::Pan,
        Scene::ThinMotion,
        Scene::Disocclusion,
        Scene::Foliage,
    ];

    fn name(self) -> &'static str {
        match self {
            Scene::Still => "still",
            Scene::Pan => "camera_pan",
            Scene::ThinMotion => "thin_motion",
            Scene::Disocclusion => "disocclusion",
            Scene::Foliage => "foliage",
        }
    }
}

/// How a rig brings its picture to the window.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Method {
    /// The truth: drawn at twice the window's size and averaged down.
    Reference,
    /// The window's size with TAA.
    NativeTaa,
    /// Today's render scale: drawn smaller (4x multisampling, no TAA),
    /// stretched and sharpened.
    Stretch(f32),
    /// The same with TAA at the render size before the stretch.
    TaaStretch(f32),
    /// The temporal upscaler.
    Temporal(f32),
}

impl Method {
    fn name(self) -> String {
        match self {
            Method::Reference => "reference".into(),
            Method::NativeTaa => "native_taa".into(),
            Method::Stretch(s) => format!("stretch_{:.0}", s * 100.0),
            Method::TaaStretch(s) => format!("taa_stretch_{:.0}", s * 100.0),
            Method::Temporal(s) => format!("temporal_{:.0}", s * 100.0),
        }
    }

    /// Sets the rig's post settings for this method.
    fn apply(self, post: &mut crate::render::Post) {
        post.bloom = 0.0;
        post.anti_aliasing = AntiAliasing::None;
        post.upscaler = Upscaler::Bilinear;
        post.sharpen = 0.0;
        match self {
            Method::Reference => post.render_scale = 2.0,
            Method::NativeTaa => {
                post.render_scale = 1.0;
                post.anti_aliasing = AntiAliasing::Taa;
            }
            Method::Stretch(s) => {
                post.render_scale = s;
                post.sharpen = SHARPEN;
            }
            Method::TaaStretch(s) => {
                post.render_scale = s;
                post.sharpen = SHARPEN;
                post.anti_aliasing = AntiAliasing::Taa;
            }
            Method::Temporal(s) => {
                post.render_scale = s;
                post.anti_aliasing = AntiAliasing::Taa;
                post.upscaler = Upscaler::Temporal;
            }
        }
    }
}

/// The stretch's sharpening in the comparison.
const SHARPEN: f32 = 0.5;

/// What a scene animates: the entities that move, by role.
#[derive(Default)]
struct Movers {
    poles: Vec<(Entity, Vec3)>,
    bar: Option<Entity>,
    blocker: Option<Entity>,
    leaves: Vec<(Entity, Vec3)>,
}

/// A mesh whose material's `surface` is `source` (unlit, so only the
/// pattern shows).
fn patterned(rig: &mut BatchRig, name: &str, source: &str, data: &MeshData, cutout: bool) -> Mesh {
    let shader = rig.ctx.gfx.create_shader(name, source);
    rig.renderer.create_shader(&rig.gpu.device, name, source).unwrap();
    rig.mesh(
        data,
        MaterialDesc {
            color: Color::BLACK,
            roughness: 1.0,
            blend: if cutout { Blend::Cutout } else { Blend::Opaque },
            shader: Some(shader),
            double_sided: true,
            ..Default::default()
        },
    )
}

/// Fine stripes and rings, sharp-edged in the shader: detail smaller than
/// a drawn pixel that multisampling can't smooth.
const STRIPES: &str = "fn surface(s: Surface) -> Surface {
    var o = s;
    let a = step(0.5, fract(s.world.x * 3.1 + s.world.y * 1.3));
    let b = step(0.5, fract(length(s.world.xy - vec2<f32>(1.5, 1.2)) * 5.0));
    o.emissive = mix(vec3<f32>(0.08, 0.1, 0.15), vec3<f32>(0.95, 0.8, 0.55), a) * (0.55 + 0.45 * b);
    o.unlit = true;
    return o;
}";

/// A checkered floor: ever finer toward the horizon.
const CHECKER: &str = "fn surface(s: Surface) -> Surface {
    var o = s;
    let c = floor(s.world.xz * 2.0);
    let k = fract((c.x + c.y) * 0.5) * 2.0;
    o.emissive = mix(vec3<f32>(0.1, 0.12, 0.1), vec3<f32>(0.6, 0.7, 0.6), k);
    o.unlit = true;
    return o;
}";

/// Leaves: lobes cut out by alpha in the shader, with veins.
const LEAVES: &str = "fn surface(s: Surface) -> Surface {
    var o = s;
    let q = s.uv * 14.0;
    let cell = abs(fract(q + vec2<f32>(sin(q.y * 1.7), sin(q.x * 1.3)) * 0.35) - 0.5);
    o.alpha = 0.72 - (cell.x + cell.y);
    let vein = step(0.06, abs(cell.x - cell.y));
    o.emissive = vec3<f32>(0.12, 0.45, 0.1) * (0.5 + 0.5 * vein);
    o.unlit = true;
    return o;
}";

/// Builds `scene` in `rig`'s world (the same in every rig).
fn build(rig: &mut BatchRig, scene: Scene) -> Movers {
    let mut movers = Movers::default();
    let stripes = patterned(rig, "stripes", STRIPES, &MeshData::plane(1.0), false);
    let floor = patterned(rig, "checker", CHECKER, &MeshData::plane(1.0), false);
    rig.ctx.world.spawn((
        Transform3D {
            position: vec3(0.0, 2.0, -3.0),
            rotation: Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
            scale: vec3(14.0, 1.0, 6.0),
        },
        Model::new(stripes),
    ));
    rig.ctx.world.spawn((
        Transform3D {
            position: vec3(0.0, 0.0, -0.5),
            scale: vec3(14.0, 1.0, 9.0),
            ..Default::default()
        },
        Model::new(floor),
    ));
    let pole = rig.mesh(&MeshData::cube(1.0), glow(Color::rgb(0.9, 0.95, 1.0)));
    let dark = rig.mesh(&MeshData::cube(1.0), glow(Color::rgb(0.25, 0.05, 0.05)));
    let moving_poles = scene == Scene::ThinMotion;
    for i in 0..9 {
        let at = vec3(-2.4 + i as f32 * 0.6, 1.2, -1.2 + (i % 3) as f32 * 0.4);
        let e = rig.ctx.world.spawn((
            Transform3D {
                position: at,
                scale: vec3(0.025, 2.4, 0.025),
                ..Default::default()
            },
            Model::new(pole),
        ));
        if moving_poles {
            movers.poles.push((e, at));
        }
    }
    // A slab with a long diagonal edge.
    rig.ctx.world.spawn((
        Transform3D {
            position: vec3(1.6, 0.9, -1.8),
            rotation: Quat::from_rotation_z(0.37),
            scale: vec3(1.6, 0.5, 0.05),
        },
        Model::new(dark),
    ));
    match scene {
        Scene::ThinMotion => {
            movers.bar = Some(rig.ctx.world.spawn((
                Transform3D {
                    position: vec3(-0.8, 1.6, 0.2),
                    scale: vec3(2.2, 0.03, 0.03),
                    ..Default::default()
                },
                Model::new(pole),
            )));
        }
        Scene::Disocclusion => {
            movers.blocker = Some(rig.ctx.world.spawn((
                Transform3D {
                    position: vec3(-3.0, 1.2, 0.6),
                    scale: vec3(0.9, 1.4, 0.3),
                    ..Default::default()
                },
                Model::new(dark),
            )));
        }
        Scene::Foliage => {
            let leaves = patterned(rig, "leaves", LEAVES, &MeshData::plane(1.0), true);
            for i in 0..6 {
                let at = vec3(
                    -2.0 + i as f32 * 0.8,
                    1.3 + (i % 2) as f32 * 0.3,
                    0.3 - (i % 3) as f32 * 0.3,
                );
                let e = rig.ctx.world.spawn((
                    Transform3D {
                        position: at,
                        rotation: Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                        scale: Vec3::splat(1.1),
                    },
                    Model::new(leaves),
                ));
                movers.leaves.push((e, at));
            }
        }
        _ => {}
    }
    rig.ctx.scene3d.camera.far = 50.0;
    movers
}

/// Poses the camera and the movers for frame `n` (the same in every rig).
fn pose(rig: &mut BatchRig, scene: Scene, movers: &Movers, n: usize) {
    let t = n as f32;
    let drift = match scene {
        Scene::Pan => -0.6 + t * 0.012,
        Scene::Foliage => -0.2 + t * 0.003,
        _ => 0.0,
    };
    let camera = &mut rig.ctx.scene3d.camera;
    camera.position = vec3(drift, 1.5, 4.0);
    camera.look_at(vec3(drift, 1.2, 0.0));
    let world = &mut rig.ctx.world;
    for &(e, at) in &movers.poles {
        if let Some(tr) = world.get_mut::<Transform3D>(e) {
            tr.position = at + vec3((t * 0.04).sin() * 0.5, 0.0, 0.0);
        }
    }
    if let Some(e) = movers.bar
        && let Some(tr) = world.get_mut::<Transform3D>(e)
    {
        tr.rotation = Quat::from_rotation_z(t * 0.035);
    }
    if let Some(e) = movers.blocker
        && let Some(tr) = world.get_mut::<Transform3D>(e)
    {
        // Across and back, a sixteenth of the view a frame at most.
        tr.position.x = -3.0 + 6.0 * (0.5 - 0.5 * (t * 0.05).cos());
    }
    for (i, &(e, at)) in movers.leaves.iter().enumerate() {
        if let Some(tr) = world.get_mut::<Transform3D>(e) {
            let sway = (t * 0.08 + i as f32).sin() * 0.12;
            tr.position = at + vec3(sway * 0.3, 0.0, 0.0);
            tr.rotation = Quat::from_rotation_z(sway) * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
        }
    }
}

/// A rig drawing `scene` at `size` with `method`. `None` without a GPU or
/// without TAA where the method needs it.
fn rig(scene: Scene, method: Method, size: (u32, u32), timestamps: bool) -> Option<(BatchRig, Movers)> {
    let mut rig = BatchRig::new(RigSetup {
        size,
        timestamps,
        ..Default::default()
    })?;
    if !matches!(method, Method::Reference | Method::Stretch(_))
        && !support::needs(&rig.gpu.device, GpuFeature::TemporalAntialiasing)
    {
        return None;
    }
    method.apply(&mut rig.ctx.scene3d.post);
    let movers = build(&mut rig, scene);
    Some((rig, movers))
}

/// Per pixel, the mean of the three channels' absolute difference in 0..1
/// (sRGB).
fn errors(image: &Image, reference: &Image) -> Vec<f32> {
    image
        .rgba
        .chunks_exact(4)
        .zip(reference.rgba.chunks_exact(4))
        .map(|(a, b)| (0..3).map(|c| (f32::from(a[c]) - f32::from(b[c])).abs()).sum::<f32>() / (3.0 * 255.0))
        .collect()
}

/// The errors of a run: the mean over the measured frames of each frame's
/// mean error, and of the share of its pixels off by more than a fifth
/// (trails, lost thin things).
#[derive(Clone, Copy, Default, Debug)]
struct Score {
    mean: f32,
    large: f32,
}

/// How far each frame of a run is from the reference, summed.
#[derive(Default)]
struct Tally {
    mean: f64,
    large: f64,
    frames: u32,
}

impl Tally {
    fn add(&mut self, image: &Image, reference: &Image) {
        let e = errors(image, reference);
        let n = e.len() as f64;
        self.mean += e.iter().map(|&v| f64::from(v)).sum::<f64>() / n;
        self.large += e.iter().filter(|&&v| v > 0.2).count() as f64 / n;
        self.frames += 1;
    }

    fn score(&self) -> Score {
        let n = f64::from(self.frames.max(1));
        Score {
            mean: (self.mean / n) as f32,
            large: (self.large / n) as f32,
        }
    }
}

/// Runs `scene` with each of `methods` in lockstep for `frames` frames,
/// scoring frames from `measure` on against the first method's (the
/// reference). Returns the scores and each method's last image. `None`
/// without a GPU (or TAA).
fn compare(
    scene: Scene,
    methods: &[Method],
    size: (u32, u32),
    frames: usize,
    measure: usize,
) -> Option<(Vec<Score>, Vec<Image>)> {
    let mut rigs = Vec::new();
    for &m in methods {
        rigs.push(rig(scene, m, size, false)?);
    }
    let mut tallies: Vec<Tally> = methods.iter().map(|_| Tally::default()).collect();
    let mut last = Vec::new();
    for n in 0..frames {
        let mut images = Vec::new();
        for (rig, movers) in &mut rigs {
            pose(rig, scene, movers, n);
            if n >= measure || n + 1 == frames {
                images.push(rig.image());
            } else {
                rig.frame();
            }
        }
        if n >= measure {
            for (tally, image) in tallies.iter_mut().zip(&images) {
                tally.add(image, &images[0]);
            }
        }
        if n + 1 == frames {
            last = images;
        }
    }
    Some((tallies.iter().map(Tally::score).collect(), last))
}

const SIZE: (u32, u32) = (160, 96);

#[test]
fn a_still_scene_upscaled_over_time_comes_closer_to_native_than_the_stretch() {
    let methods = [
        Method::Reference,
        Method::Stretch(0.5),
        Method::TaaStretch(0.5),
        Method::Temporal(0.5),
    ];
    let Some((scores, images)) = compare(Scene::Still, &methods, SIZE, 40, 32) else {
        return;
    };
    for image in &images {
        support::keep_image(image);
    }
    let [_, stretch, taa_stretch, temporal] = scores[..] else {
        unreachable!()
    };
    assert!(
        temporal.mean < 0.8 * stretch.mean.min(taa_stretch.mean),
        "the converged picture has detail the stretch lacks: {temporal:?} against {stretch:?} and {taa_stretch:?}"
    );
}

#[test]
fn a_box_crossing_fine_stripes_leaves_no_trail_behind_it() {
    let methods = [Method::Reference, Method::TaaStretch(0.5), Method::Temporal(0.5)];
    let Some((scores, images)) = compare(Scene::Disocclusion, &methods, SIZE, 48, 24) else {
        return;
    };
    for image in &images {
        support::keep_image(image);
    }
    let [_, taa_stretch, temporal] = scores[..] else {
        unreachable!()
    };
    // A trail behind the box would be a band of pixels far off the truth.
    assert!(
        temporal.large <= taa_stretch.large * 1.1 + 0.002,
        "no more pixels far off than the stretch has: {temporal:?} against {taa_stretch:?}"
    );
    assert!(temporal.mean < taa_stretch.mean, "{temporal:?} against {taa_stretch:?}");
}

#[test]
fn the_temporal_upscaler_runs_only_with_taa_and_a_scale_below_one() {
    let draw = |method: Method, change: &dyn Fn(&mut crate::render::Post)| {
        let (mut rig, movers) = rig(Scene::Pan, method, (96, 64), false)?;
        change(&mut rig.ctx.scene3d.post);
        let mut image = None;
        for n in 0..4 {
            pose(&mut rig, Scene::Pan, &movers, n);
            image = Some(rig.image());
        }
        Some((image.unwrap(), rig.renderer.upscales_temporally()))
    };
    // Without TAA the setting changes nothing: the stretch, byte for byte.
    let Some((stretch, false)) = draw(Method::Stretch(0.5), &|_| {}) else {
        return;
    };
    let Some((asked, upscaled)) = draw(Method::Stretch(0.5), &|p| p.upscaler = Upscaler::Temporal) else {
        return;
    };
    assert!(!upscaled);
    assert_eq!(asked.rgba, stretch.rgba, "without TAA it stays the stretch");
    // At the window's size it is plain TAA, byte for byte.
    let Some((taa, false)) = draw(Method::NativeTaa, &|_| {}) else {
        return;
    };
    let (native, upscaled) = draw(Method::NativeTaa, &|p| p.upscaler = Upscaler::Temporal).unwrap();
    assert!(!upscaled);
    assert_eq!(native.rgba, taa.rgba, "at scale 1 it is TAA's resolve");
    // With both it runs, and the picture differs from TAA's stretch.
    let (temporal, upscaled) = draw(Method::Temporal(0.5), &|_| {}).unwrap();
    assert!(upscaled, "TAA, a scale of 0.5 and the setting: it runs");
    let (taa_stretch, _) = draw(Method::TaaStretch(0.5), &|_| {}).unwrap();
    assert_ne!(temporal.rgba, taa_stretch.rgba);
}

#[test]
fn changing_the_scale_or_the_upscaler_at_runtime_remakes_its_images() {
    let Some((mut rig, movers)) = rig(Scene::Still, Method::Temporal(0.5), (96, 64), false) else {
        return;
    };
    let step = |rig: &mut BatchRig, n| {
        pose(rig, Scene::Still, &movers, n);
        rig.image()
    };
    step(&mut rig, 0);
    assert!(rig.renderer.upscales_temporally());
    assert_eq!(rig.renderer.scene_size(), Some((48, 32)));
    rig.ctx.scene3d.post.render_scale = 0.75;
    let image = step(&mut rig, 1);
    assert!(rig.renderer.upscales_temporally());
    assert_eq!(rig.renderer.scene_size(), Some((72, 48)));
    assert_eq!((image.width, image.height), (96, 64));
    rig.ctx.scene3d.post.upscaler = Upscaler::Bilinear;
    step(&mut rig, 2);
    assert!(!rig.renderer.upscales_temporally(), "back to TAA and the stretch");
    assert!(rig.renderer.scales_output());
    rig.ctx.scene3d.post.upscaler = Upscaler::Temporal;
    rig.ctx.scene3d.post.render_scale = 1.0;
    step(&mut rig, 3);
    assert!(!rig.renderer.upscales_temporally() && !rig.renderer.scales_output());
}

/// Where the evaluation writes its images and table.
fn evaluation_dir() -> std::path::PathBuf {
    std::env::var_os("PONKO_UPSCALE_EVAL_OUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| support::output_dir().join("upscale-eval"))
}

fn save(dir: &std::path::Path, name: &str, image: &Image) {
    image::save_buffer(
        dir.join(format!("{name}.png")),
        &image.rgba,
        image.width,
        image.height,
        image::ColorType::Rgba8,
    )
    .unwrap();
}

/// The difference from the reference, amplified four times, as an image.
fn difference(image: &Image, reference: &Image) -> Image {
    let rgba = errors(image, reference)
        .into_iter()
        .flat_map(|e| {
            let v = (e * 4.0 * 255.0).min(255.0) as u8;
            [v, v, v, 255]
        })
        .collect();
    Image {
        width: image.width,
        height: image.height,
        rgba,
    }
}

#[test]
#[ignore = "an evaluation: run by hand, in release, for the guide's numbers"]
fn upscaling_evaluation() {
    const EVAL: (u32, u32) = (480, 272);
    let methods = [
        Method::Reference,
        Method::NativeTaa,
        Method::Stretch(0.5),
        Method::TaaStretch(0.5),
        Method::Temporal(0.5),
        Method::Stretch(0.67),
        Method::TaaStretch(0.67),
        Method::Temporal(0.67),
    ];
    let dir = evaluation_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let mut table = String::from("| scene |");
    for m in &methods[1..] {
        table += &format!(" {} |", m.name());
    }
    table += "\n|---|";
    table += &"---:|".repeat(methods.len() - 1);
    table += "\n";
    for scene in Scene::ALL {
        // 60 frames, the last 30 scored (the still scene: the last 10).
        let measure = if scene == Scene::Still { 50 } else { 30 };
        let Some((scores, images)) = compare(scene, &methods, EVAL, 60, measure) else {
            return;
        };
        table += &format!("| {} |", scene.name());
        for (m, (score, image)) in methods.iter().zip(scores.iter().zip(&images)).skip(1) {
            table += &format!(" {:.2} / {:.2}% |", score.mean * 255.0, score.large * 100.0);
            save(&dir, &format!("{}.{}", scene.name(), m.name()), image);
            save(
                &dir,
                &format!("{}.{}.diff", scene.name(), m.name()),
                &difference(image, &images[0]),
            );
        }
        save(&dir, &format!("{}.reference", scene.name()), &images[0]);
        table += "\n";
        println!("{} done", scene.name());
    }
    let probe = BatchRig::new(RigSetup::default()).unwrap();
    let header = format!(
        "Upscaling evaluation, {}x{} window, adapter {}.\n\
         Each cell: mean error in 8-bit sRGB levels against a picture drawn at twice the size and averaged \
         down / share of pixels off by more than a fifth, over the scored frames. Stretch: 4x MSAA, \
         sharpen {SHARPEN}; taa_stretch: TAA at the render size, then the stretch; temporal: the temporal \
         upscaler (taa_sharpness 0.25).\n\n",
        EVAL.0,
        EVAL.1,
        support::describe_adapter(&probe.gpu.info),
    );
    let report = header + &table;
    println!("{report}");
    std::fs::write(dir.join("errors.md"), report).unwrap();
}

/// A heavy frame for timing: four thousand models with sun shadows, TAA,
/// ambient occlusion, reflections and bloom (about the High preset), and
/// the stripes over everything far.
fn timing_scene(rig: &mut BatchRig) {
    const MODELS: usize = 4000;
    let meshes: Vec<Mesh> = (0..20)
        .map(|i| {
            let data = if i % 2 == 0 {
                MeshData::cube(0.4)
            } else {
                MeshData::sphere(0.3, 16)
            };
            let shade = 0.3 + 0.035 * i as f32;
            rig.mesh(
                &data,
                MaterialDesc {
                    color: Color::rgb(shade, 1.0 - shade * 0.5, 0.6),
                    ..Default::default()
                },
            )
        })
        .collect();
    let side = (MODELS as f32).sqrt().ceil() as usize;
    for i in 0..MODELS {
        let at = vec3(
            (i % side) as f32 * 1.5 - side as f32 * 0.75,
            ((i * 7) % 5) as f32 * 0.4,
            -((i / side) as f32) * 1.5,
        );
        rig.ctx
            .world
            .spawn((Transform3D::at(at), Model::new(meshes[i % meshes.len()])));
    }
    let stripes = patterned(rig, "stripes", STRIPES, &MeshData::plane(1.0), false);
    rig.ctx.world.spawn((
        Transform3D {
            position: vec3(0.0, -0.3, -(side as f32) * 0.75),
            scale: vec3(side as f32 * 2.0, 1.0, side as f32 * 2.0),
            ..Default::default()
        },
        Model::new(stripes),
    ));
    rig.ctx.scene3d.camera.far = side as f32 * 3.0;
    rig.ctx.scene3d.lighting.shadows = true;
    rig.ctx.scene3d.lighting.shadow_distance = side as f32 * 0.75;
    let post = &mut rig.ctx.scene3d.post;
    post.bloom = 0.05;
    post.ambient_occlusion.enabled = true;
    post.ssr = 1.0;
}

#[test]
#[ignore = "a benchmark: run by hand, in release, for the guide's numbers"]
fn upscaling_gpu_time() {
    const WARM_UP: usize = 30;
    const FRAMES: usize = 60;
    let methods = [
        Method::NativeTaa,
        Method::TaaStretch(0.67),
        Method::Temporal(0.67),
        Method::TaaStretch(0.5),
        Method::Temporal(0.5),
    ];
    let mut report = String::from(
        "| window | method | frame GPU ms (first pass start to last pass end) | TAA resolve ms | scaling ms |\n|---|---|---:|---:|---:|\n",
    );
    let mut adapter = String::new();
    for size in [(2560, 1440), (3840, 2160)] {
        for method in methods {
            let Some(mut rig) = BatchRig::new(RigSetup {
                size,
                timestamps: true,
                ..Default::default()
            }) else {
                return;
            };
            if rig.timer.is_none() || !support::needs(&rig.gpu.device, GpuFeature::TemporalAntialiasing) {
                support::skip("no timestamp queries or TAA");
                return;
            }
            adapter = support::describe_adapter(&rig.gpu.info);
            timing_scene(&mut rig);
            method.apply(&mut rig.ctx.scene3d.post);
            rig.ctx.scene3d.post.bloom = 0.05;
            rig.ctx.scene3d.post.ssr = 1.0;
            let start = vec3(-20.0, 6.0, 4.0);
            let ahead = vec3(0.4, -0.15, -1.0).normalize();
            let (mut total, mut resolve, mut scaling) = (Vec::new(), Vec::new(), Vec::new());
            let mut read = 0;
            for n in 0..WARM_UP + FRAMES {
                rig.ctx.scene3d.camera.position = start + vec3(n as f32 * 0.01, 0.0, 0.0);
                rig.ctx.scene3d.camera.look_at(rig.ctx.scene3d.camera.position + ahead);
                rig.frame();
                let timer = rig.timer.as_ref().unwrap();
                if n >= WARM_UP && timer.frames_read != read {
                    read = timer.frames_read;
                    let pass = |name: &str| {
                        timer
                            .last_frame
                            .iter()
                            .filter(|p| p.name == name)
                            .fold(0.0, |sum, p| sum + f64::from(p.ms))
                    };
                    // From the first pass's start to the last one's end:
                    // passes overlap on some GPUs, so their sum would
                    // count some time twice.
                    let spans = &timer.last_frame;
                    let end = spans.iter().map(|p| p.start_ms + p.ms).fold(0.0, f32::max);
                    let first = spans.iter().map(|p| p.start_ms).fold(f32::MAX, f32::min);
                    total.push(f64::from(end - first));
                    resolve.push(pass("taa"));
                    scaling.push(pass("upscale") + pass("temporal upscale"));
                }
            }
            let median = |v: &mut Vec<f64>| {
                v.sort_by(f64::total_cmp);
                v.get(v.len() / 2).copied().unwrap_or(f64::NAN)
            };
            report += &format!(
                "| {}x{} | {} | {:.2} | {:.2} | {:.2} |\n",
                size.0,
                size.1,
                method.name(),
                median(&mut total),
                median(&mut resolve),
                median(&mut scaling),
            );
        }
    }
    let report = format!(
        "Upscaling GPU time, adapter {adapter}; {WARM_UP} warm-up and {FRAMES} timed frames, medians. \
         4000 models, sun shadows, TAA, ambient occlusion, reflections, bloom.\n\n{report}"
    );
    println!("{report}");
    let dir = evaluation_dir();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("gpu-time.md"), report).unwrap();
}
