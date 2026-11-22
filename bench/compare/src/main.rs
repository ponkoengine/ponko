//! Three of the rendering baseline's scenes (`static-world`,
//! `dynamic-world`, `multi-material`) built again with Bevy, for a
//! side-by-side run with `PONKO_RENDER_MATCHED=1`. The meshes, colors,
//! textures, placement, camera path and sun are the same code as in
//! `src/render/gpu3d/scene_bench.rs`; `crates/ponko_bench/RENDER_SCENES.md`
//! lists what is matched and what is not.
//!
//! Draws offscreen, opens no window:
//!
//! ```sh
//! cd bench/bevy_scenes
//! PONKO_RENDER_CSV=bevy.csv PONKO_BENCH_JSON=bevy.json cargo run --release
//! ```
//!
//! Settings, as the ponko run reads them: `PONKO_RENDER_FRAMES` (600),
//! `PONKO_RENDER_WARMUP` (120), `PONKO_RENDER_SIZE` (1280x720),
//! `PONKO_RENDER_SCALE` (1), `PONKO_RENDER_PNG=<dir>`. An argument keeps
//! the scenes whose names contain it.

use std::fmt::Write as _;
use std::time::Instant;

use bevy::{
    anti_alias::fxaa::Fxaa,
    app::SubApps,
    asset::RenderAssetUsages,
    camera::{Exposure, RenderTarget},
    core_pipeline::tonemapping::Tonemapping,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap, GlobalAmbientLight},
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    render::{
        RenderPlugin,
        pipelined_rendering::PipelinedRenderingPlugin,
        render_resource::{Extent3d, PollType, TextureDimension, TextureFormat},
        renderer::{RenderAdapterInfo, RenderDevice},
        view::screenshot::{Screenshot, save_to_disk},
    },
    window::ExitCondition,
};
use serde_json::{Value, json};

/// The version `Cargo.toml` pins.
const BEVY_VERSION: &str = "0.20.1";

const SCENES: [&str; 3] = ["static-world", "dynamic-world", "multi-material"];

/// The bounds of the scenes' random angles: about half a turn and a turn.
/// Any bound would do; these are the ones the recorded baselines were placed
/// with, and changing them moves every object.
#[allow(clippy::approx_constant)]
const HALF_TURN: f32 = 3.14;
#[allow(clippy::approx_constant)]
const TURN: f32 = 6.28;

/// The fixed pseudo-random sequence the scenes are placed with: the same
/// as ponko's, so every object lands in the same place.
struct Sequence(u64);

impl Sequence {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }

    fn around(&mut self, half: f32) -> f32 {
        (self.next() * 2.0 - 1.0) * half
    }

    fn between(&mut self, low: f32, high: f32) -> f32 {
        low + self.next() * (high - low)
    }
}

// ------------------------------------------------------------------ meshes
// The same vertices and triangles as ponko's `MeshData::cube`, `plane` and
// `sphere`, so both engines draw the same triangle counts.

#[derive(Default)]
struct Shape {
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    uvs: Vec<Vec2>,
    indices: Vec<u32>,
}

impl Shape {
    fn cube(size: f32) -> Self {
        let h = size / 2.0;
        let mut shape = Self::default();
        let faces = [
            (Vec3::X, Vec3::NEG_Z, Vec3::Y),
            (Vec3::NEG_X, Vec3::Z, Vec3::Y),
            (Vec3::Y, Vec3::X, Vec3::NEG_Z),
            (Vec3::NEG_Y, Vec3::X, Vec3::Z),
            (Vec3::Z, Vec3::X, Vec3::Y),
            (Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y),
        ];
        for (normal, right, up) in faces {
            let base = shape.positions.len() as u32;
            for (u, v) in [(0.0, 1.0), (1.0, 1.0), (1.0, 0.0), (0.0, 0.0)] {
                shape
                    .positions
                    .push(normal * h + right * (u * 2.0 - 1.0) * h + up * (1.0 - v * 2.0) * h);
                shape.normals.push(normal);
                shape.uvs.push(Vec2::new(u, v));
            }
            shape
                .indices
                .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        shape
    }

    fn plane(size: f32) -> Self {
        let h = size / 2.0;
        Self {
            positions: vec![
                Vec3::new(-h, 0.0, h),
                Vec3::new(h, 0.0, h),
                Vec3::new(h, 0.0, -h),
                Vec3::new(-h, 0.0, -h),
            ],
            normals: vec![Vec3::Y; 4],
            uvs: vec![
                Vec2::new(0.0, 1.0),
                Vec2::new(1.0, 1.0),
                Vec2::new(1.0, 0.0),
                Vec2::new(0.0, 0.0),
            ],
            indices: vec![0, 1, 2, 0, 2, 3],
        }
    }

    fn sphere(radius: f32, segments: u32) -> Self {
        let around = segments.max(3);
        let rings = (around / 2).max(2);
        let mut shape = Self::default();
        for ring in 0..=rings {
            let v = ring as f32 / rings as f32;
            let polar = v * std::f32::consts::PI;
            for step in 0..=around {
                let u = step as f32 / around as f32;
                let azimuth = u * std::f32::consts::TAU;
                let normal = Vec3::new(polar.sin() * azimuth.cos(), polar.cos(), -polar.sin() * azimuth.sin());
                shape.positions.push(normal * radius);
                shape.normals.push(normal);
                shape.uvs.push(Vec2::new(u, v));
            }
        }
        let row = around + 1;
        for ring in 0..rings {
            for step in 0..around {
                let a = ring * row + step;
                let b = a + row;
                if ring > 0 {
                    shape.indices.extend([a, b, a + 1]);
                }
                if ring + 1 < rings {
                    shape.indices.extend([a + 1, b, b + 1]);
                }
            }
        }
        shape
    }

    fn mesh(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs)
            .with_inserted_indices(Indices::U32(self.indices))
    }
}

// ---------------------------------------------------------------- textures

fn to_linear(c: u8) -> f32 {
    let c = f32::from(c) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn to_srgb(c: f32) -> u8 {
    let c = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (c.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The 64 x 64 checker ponko's scene makes for `index`, with a full mip
/// chain (each level the average of four texels, in linear light).
fn texture(index: u32, holes: bool) -> Image {
    const SIDE: u32 = 64;
    let squares = 2 + index % 4 * 2;
    let (a, b) = (
        [
            200 - (index * 23 % 120) as u8,
            120 + (index * 41 % 120) as u8,
            90 + (index * 67 % 150) as u8,
        ],
        [
            70 + (index * 13 % 60) as u8,
            60 + (index * 29 % 80) as u8,
            50 + (index * 53 % 90) as u8,
        ],
    );
    let mut level = Vec::with_capacity((SIDE * SIDE * 4) as usize);
    for y in 0..SIDE {
        for x in 0..SIDE {
            let odd = (x * squares / SIDE + y * squares / SIDE) % 2 == 1;
            let [r, g, bl] = if odd { b } else { a };
            level.extend_from_slice(&[r, g, bl, if odd && holes { 0 } else { 255 }]);
        }
    }
    let (mut data, mut side, mut levels) = (level.clone(), SIDE as usize, 1);
    while side > 1 {
        let half = side / 2;
        let mut next = Vec::with_capacity(half * half * 4);
        for y in 0..half {
            for x in 0..half {
                let texel = |dx: usize, dy: usize, c: usize| level[((y * 2 + dy) * side + x * 2 + dx) * 4 + c];
                for c in 0..4 {
                    let four = [texel(0, 0, c), texel(1, 0, c), texel(0, 1, c), texel(1, 1, c)];
                    next.push(if c == 3 {
                        (four.iter().map(|v| u32::from(*v)).sum::<u32>() / 4) as u8
                    } else {
                        to_srgb(four.iter().map(|v| to_linear(*v)).sum::<f32>() / 4.0)
                    });
                }
            }
        }
        data.extend_from_slice(&next);
        (level, side, levels) = (next, half, levels + 1);
    }
    let top = data[..(SIDE * SIDE * 4) as usize].to_vec();
    let mut image = Image::new(
        Extent3d {
            width: SIDE,
            height: SIDE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        top,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    // Every level, largest first.
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = levels;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    });
    image
}

// ------------------------------------------------------------------ scenes

#[derive(Component)]
struct Mover {
    origin: Vec3,
    phase: f32,
    radius: f32,
    speed: f32,
}

/// What a scene holds and how its camera circles it.
#[derive(Default)]
struct Built {
    models: usize,
    moving_models: usize,
    meshes: usize,
    materials: usize,
    textures: usize,
    /// Center, distance and height of the camera's circle.
    orbit: (Vec3, f32, f32),
}

struct Builder<'a> {
    world: &'a mut World,
    scale: f32,
    built: Built,
}

impl Builder<'_> {
    fn count(&self, base: usize, least: usize) -> usize {
        ((base as f32 * self.scale).round() as usize).max(least)
    }

    fn mesh(&mut self, shape: Shape) -> Handle<Mesh> {
        self.built.meshes += 1;
        self.world.resource_mut::<Assets<Mesh>>().add(shape.mesh())
    }

    fn material(&mut self, material: StandardMaterial) -> Handle<StandardMaterial> {
        self.built.materials += 1;
        self.world.resource_mut::<Assets<StandardMaterial>>().add(material)
    }

    fn model(&mut self, mesh: &(Handle<Mesh>, Handle<StandardMaterial>), at: Transform) -> EntityWorldMut<'_> {
        self.built.models += 1;
        self.world
            .spawn((Mesh3d(mesh.0.clone()), MeshMaterial3d(mesh.1.clone()), at))
    }

    fn ground(&mut self, size: f32, color: Color) {
        let material = self.material(StandardMaterial {
            base_color: color,
            perceptual_roughness: 0.9,
            ..default()
        });
        let mesh = self.mesh(Shape::plane(size));
        self.model(&(mesh, material), Transform::IDENTITY);
    }

    fn simple_meshes(&mut self, kinds: usize, size: f32) -> Vec<(Handle<Mesh>, Handle<StandardMaterial>)> {
        (0..kinds)
            .map(|i| {
                let shade = i as f32 / kinds as f32;
                let material = self.material(StandardMaterial {
                    base_color: Color::srgb(
                        0.35 + 0.6 * shade,
                        0.75 - 0.4 * shade,
                        0.4 + 0.3 * ((i % 3) as f32 / 2.0),
                    ),
                    perceptual_roughness: 0.4 + 0.5 * shade,
                    ..default()
                });
                let shape = if i % 2 == 0 {
                    Shape::cube(size * (0.6 + 0.05 * (i % 5) as f32))
                } else {
                    Shape::sphere(size * (0.35 + 0.03 * (i % 4) as f32), 10)
                };
                (self.mesh(shape), material)
            })
            .collect()
    }

    fn build(&mut self, scene: &str) {
        let mut seq = Sequence(0x9E37_79B9_7F4A_7C15);
        match scene {
            "static-world" => {
                let half = 200.0;
                self.ground(half * 2.0, Color::srgb(0.32, 0.4, 0.3));
                let meshes = self.simple_meshes(8, 1.0);
                let each = self.count(25_000, 16);
                for mesh in meshes {
                    for _ in 0..each {
                        let scale = Vec3::new(seq.between(0.6, 2.0), seq.between(0.6, 4.0), seq.between(0.6, 2.0));
                        let rotation = Quat::from_rotation_y(seq.around(HALF_TURN));
                        let translation = Vec3::new(seq.around(half), scale.y * 0.3, seq.around(half));
                        self.model(
                            &mesh,
                            Transform {
                                translation,
                                rotation,
                                scale,
                            },
                        );
                    }
                }
                self.built.orbit = (Vec3::ZERO, 60.0, 25.0);
            }
            "dynamic-world" => {
                self.ground(200.0, Color::srgb(0.3, 0.33, 0.4));
                let meshes = self.simple_meshes(16, 1.0);
                let models = self.count(20_000, 16);
                let side = (models as f32).sqrt().ceil() as usize;
                for i in 0..models {
                    let origin = Vec3::new(
                        (i % side) as f32 * 1.2 - side as f32 * 0.6,
                        1.0,
                        (i / side) as f32 * 1.2 - side as f32 * 0.6,
                    );
                    let mover = Mover {
                        origin,
                        phase: seq.next() * TURN,
                        radius: seq.between(0.2, 0.8),
                        speed: seq.between(0.5, 2.0),
                    };
                    self.model(&meshes[i % meshes.len()], Transform::from_translation(origin))
                        .insert(mover);
                    self.built.moving_models += 1;
                }
                let reach = side as f32 * 0.6;
                self.built.orbit = (Vec3::ZERO, reach * 0.9 + 6.0, reach * 0.5 + 4.0);
            }
            "multi-material" => {
                self.ground(200.0, Color::srgb(0.4, 0.38, 0.34));
                let textures: Vec<(Handle<Image>, Handle<Image>)> = (0..16)
                    .map(|i| {
                        let mut images = self.world.resource_mut::<Assets<Image>>();
                        (images.add(texture(i, false)), images.add(texture(i, true)))
                    })
                    .collect();
                self.built.textures = textures.len() * 2;
                let kinds = self.count(512, 8);
                let meshes: Vec<_> = (0..kinds)
                    .map(|i| {
                        let cutout = i % 5 == 4;
                        let (plain, holed) = &textures[i % textures.len()];
                        let double_sided = i % 10 == 9;
                        let material = self.material(StandardMaterial {
                            base_color: Color::srgb(
                                0.5 + 0.5 * (i % 7) as f32 / 6.0,
                                0.5 + 0.5 * (i % 11) as f32 / 10.0,
                                0.5 + 0.5 * (i % 13) as f32 / 12.0,
                            ),
                            base_color_texture: (i % 4 != 3)
                                .then(|| if cutout { holed.clone() } else { plain.clone() }),
                            perceptual_roughness: 0.15 + 0.8 * (i % 9) as f32 / 8.0,
                            metallic: if i % 6 == 0 { 1.0 } else { 0.0 },
                            emissive: if i % 17 == 0 {
                                Color::srgb(0.6, 0.3, 0.1).to_linear()
                            } else {
                                LinearRgba::BLACK
                            },
                            emissive_exposure_weight: 1.0,
                            alpha_mode: if cutout {
                                AlphaMode::Mask(0.5)
                            } else {
                                AlphaMode::Opaque
                            },
                            double_sided,
                            cull_mode: (!double_sided).then_some(bevy::render::render_resource::Face::Back),
                            ..default()
                        });
                        let shape = match i % 3 {
                            0 => Shape::cube(0.5 + 0.02 * (i % 8) as f32),
                            1 => Shape::sphere(0.3 + 0.01 * (i % 8) as f32, 12),
                            _ => Shape::plane(0.7),
                        };
                        (self.mesh(shape), material)
                    })
                    .collect();
                let models = self.count(6_000, 16);
                let side = (models as f32).sqrt().ceil() as usize;
                for i in 0..models {
                    let mesh = &meshes[(i as u32).wrapping_mul(2_654_435_761) as usize % meshes.len()];
                    let at = Vec3::new(
                        (i % side) as f32 * 1.1 - side as f32 * 0.55,
                        0.5 + (i % 3) as f32 * 0.4,
                        (i / side) as f32 * 1.1 - side as f32 * 0.55,
                    );
                    self.model(
                        mesh,
                        Transform::from_translation(at).with_rotation(Quat::from_rotation_y(seq.around(HALF_TURN))),
                    );
                }
                let reach = side as f32 * 0.55;
                self.built.orbit = (Vec3::ZERO, reach * 0.9 + 5.0, reach * 0.5 + 4.0);
            }
            other => unreachable!("no scene named {other}"),
        }
    }
}

// ----------------------------------------------------------------- running

struct Settings {
    frames: usize,
    warmup: usize,
    size: (u32, u32),
    scale: f32,
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn settings() -> Result<Settings, String> {
    let number = |name: &str, default: usize| match env(name) {
        None => Ok(default),
        Some(v) => v
            .trim()
            .parse::<usize>()
            .map_err(|_| format!("{name}={v} is not a whole number")),
    };
    let size = match env("PONKO_RENDER_SIZE") {
        None => (1280, 720),
        Some(size) => size
            .trim()
            .split_once(['x', 'X'])
            .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
            .ok_or_else(|| format!("PONKO_RENDER_SIZE={size} is not WIDTHxHEIGHT, as in 1280x720"))?,
    };
    let scale = match env("PONKO_RENDER_SCALE") {
        None => 1.0,
        Some(scale) => scale
            .trim()
            .parse::<f32>()
            .ok()
            .filter(|s| s.is_finite() && *s > 0.0)
            .ok_or_else(|| format!("PONKO_RENDER_SCALE={scale} is not a positive number"))?,
    };
    let frames = number("PONKO_RENDER_FRAMES", 600)?;
    if frames == 0 {
        return Err("PONKO_RENDER_FRAMES=0 measures nothing".to_owned());
    }
    Ok(Settings {
        frames,
        warmup: number("PONKO_RENDER_WARMUP", 120)?,
        size,
        scale,
    })
}

/// Nearest-rank percentiles, as the ponko report computes them.
#[derive(Clone, Copy)]
struct Dist {
    samples: usize,
    min: f64,
    p50: f64,
    p95: f64,
    p99: f64,
    max: f64,
    mean: f64,
}

impl Dist {
    fn of(values: &mut [f64]) -> Dist {
        values.sort_by(f64::total_cmp);
        let rank = |pct: usize| values[(pct * values.len()).div_ceil(100).max(1) - 1];
        Dist {
            samples: values.len(),
            min: values[0],
            p50: rank(50),
            p95: rank(95),
            p99: rank(99),
            max: values[values.len() - 1],
            mean: values.iter().sum::<f64>() / values.len() as f64,
        }
    }

    fn json(&self) -> Value {
        let round = |v: f64| (v * 1e6).round() / 1e6;
        json!({
            "samples": self.samples,
            "min": round(self.min),
            "p50": round(self.p50),
            "p95": round(self.p95),
            "p99": round(self.p99),
            "max": round(self.max),
            "mean": round(self.mean),
        })
    }
}

struct Measured {
    name: &'static str,
    built: Built,
    adapter: Value,
    update: Dist,
    cpu: Dist,
    frame: Dist,
}

/// Why this program reports no GPU pass times.
const NO_GPU_TIMES: &str =
    "this program reads no GPU timestamps from Bevy (its render diagnostics record them on Vulkan and DX12 only)";

fn measure(name: &'static str, settings: &Settings, png: Option<&std::path::Path>) -> Measured {
    // The log can be set up once in a process: the first scene does it.
    static LOGGING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let mut plugins = DefaultPlugins.build();
    if LOGGING.swap(true, std::sync::atomic::Ordering::Relaxed) {
        plugins = plugins.disable::<bevy::log::LogPlugin>();
    }
    let mut app = App::new();
    app.add_plugins(
        plugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .set(RenderPlugin {
                synchronous_pipeline_compilation: true,
                ..default()
            })
            // One thread simulates and renders, as in the ponko run.
            .disable::<PipelinedRenderingPlugin>(),
    );
    // The sun and the ambient light of ponko's default `Lighting`, in
    // ponko's units (the camera's exposure stands in for the tone curve).
    let rgb = |hex: u32| Color::srgb_u8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
    let (sky, ground) = (rgb(0x86_9B_B0).to_linear(), rgb(0x4D_41_35).to_linear());
    // Black behind the scene, as ponko's plain sky is in the matched run.
    app.insert_resource(ClearColor(Color::BLACK))
        .insert_resource(GlobalAmbientLight {
            // Ponko's ambient runs from the ground color to the sky color
            // with the normal; Bevy's is one color: the middle of the two.
            color: Color::LinearRgba((sky + ground) * 0.5),
            brightness: 1.0,
            ..default()
        })
        .insert_resource(DirectionalLightShadowMap { size: 2048 });
    app.finish();
    app.cleanup();
    let mut apps: SubApps = std::mem::take(app.sub_apps_mut());

    let world = apps.main.world_mut();
    let mut builder = Builder {
        world,
        scale: settings.scale,
        built: Built::default(),
    };
    builder.build(name);
    let built = builder.built;
    world.spawn((
        DirectionalLight {
            color: rgb(0xFF_F4_E0),
            illuminance: 3.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::IDENTITY.looking_to(Vec3::new(-0.4, -1.0, -0.3), Vec3::Y),
        CascadeShadowConfigBuilder {
            num_cascades: 4,
            maximum_distance: 80.0,
            ..default()
        }
        .build(),
    ));
    let mut target = Image::new_target_texture(settings.size.0, settings.size.1, TextureFormat::Rgba8UnormSrgb, None);
    target.asset_usage = RenderAssetUsages::RENDER_WORLD;
    let target = world.resource_mut::<Assets<Image>>().add(target);
    let (center, distance, height) = built.orbit;
    let camera = world
        .spawn((
            Camera3d::default(),
            RenderTarget::from(target.clone()),
            Projection::Perspective(PerspectiveProjection {
                fov: 60f32.to_radians(),
                near: 0.1,
                far: (distance * 6.0).max(300.0),
                ..default()
            }),
            Msaa::Sample4,
            Tonemapping::AcesFitted,
            Fxaa::default(),
            // An exposure of 1 / 0.6: ponko's ACES curve (Narkowicz's fit)
            // has that factor built in, Bevy's fitted ACES does not.
            Exposure { ev100: -1.0 },
            Transform::IDENTITY,
        ))
        .id();
    let mut movers = world.query::<(&mut Transform, &Mover)>();

    let (mut update, mut cpu, mut frame) = (Vec::new(), Vec::new(), Vec::new());
    for index in 0..settings.warmup + settings.frames {
        let started = Instant::now();
        let world = apps.main.world_mut();
        let angle = 0.6 + index as f32 * 0.004;
        let eye = center + Vec3::new(angle.cos() * distance, height, angle.sin() * distance);
        *world.get_mut::<Transform>(camera).expect("the camera") =
            Transform::from_translation(eye).looking_at(center, Vec3::Y);
        let time = index as f32 / 60.0;
        for (mut transform, mover) in movers.iter_mut(world) {
            let at = time * mover.speed + mover.phase;
            transform.translation = mover.origin + Vec3::Y * at.sin() * mover.radius;
            transform.rotation = Quat::from_rotation_y(at);
        }
        let updated = Instant::now();
        // Every schedule of the main world, then extraction and the render
        // world up to the submit.
        apps.update();
        let submitted = Instant::now();
        apps.main
            .world()
            .resource::<RenderDevice>()
            .wgpu_device()
            .poll(PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .expect("the GPU finishes the frame");
        let done = Instant::now();
        if index >= settings.warmup {
            update.push((updated - started).as_secs_f64() * 1e3);
            cpu.push((submitted - updated).as_secs_f64() * 1e3);
            frame.push((done - updated).as_secs_f64() * 1e3);
        }
    }
    if let Some(dir) = png {
        let path = dir.join(format!("{name}.png"));
        apps.main
            .world_mut()
            .spawn(Screenshot::image(target))
            .observe(save_to_disk(path.clone()));
        // The copy comes back a frame or two later.
        for _ in 0..4 {
            apps.update();
            let _ = apps
                .main
                .world()
                .resource::<RenderDevice>()
                .wgpu_device()
                .poll(PollType::Wait {
                    submission_index: None,
                    timeout: None,
                });
        }
        println!("    picture written to {}", path.display());
    }
    let info = apps.main.world().resource::<RenderAdapterInfo>();
    let adapter = json!({
        "name": info.name,
        "backend": format!("{:?}", info.backend).to_lowercase(),
        "device_type": format!("{:?}", info.device_type).to_lowercase(),
        "driver": info.driver,
        "driver_info": info.driver_info,
    });
    Measured {
        name,
        built,
        adapter,
        update: Dist::of(&mut update),
        cpu: Dist::of(&mut cpu),
        frame: Dist::of(&mut frame),
    }
}

/// First line of a command's output, or `"unknown"`.
fn command_line(cmd: &str, args: &[&str]) -> String {
    std::process::Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.lines().next().map(|l| l.trim().to_owned()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

/// The 1, 5 and 15 minute load averages (how busy the machine is), as the
/// ponko run records them, or `"unknown"`.
fn load_average() -> String {
    if cfg!(target_os = "macos") {
        let s = command_line("sysctl", &["-n", "vm.loadavg"]);
        return s.trim_matches(|c| c == '{' || c == '}' || c == ' ').to_owned();
    }
    std::fs::read_to_string("/proc/loadavg")
        .ok()
        .map(|s| s.split_whitespace().take(3).collect::<Vec<_>>().join(" "))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn write(path: &std::ffi::OsStr, what: &str, text: String) {
    match std::fs::write(path, text) {
        Ok(()) => println!("{what} written to {}", path.to_string_lossy()),
        Err(e) => {
            eprintln!("could not write {}: {e}", path.to_string_lossy());
            std::process::exit(1);
        }
    }
}

fn main() {
    let filter = std::env::args().nth(1).unwrap_or_default();
    let settings = match settings() {
        Ok(settings) => settings,
        Err(problem) => {
            eprintln!("{problem}");
            std::process::exit(2);
        }
    };
    let png = std::env::var_os("PONKO_RENDER_PNG").map(std::path::PathBuf::from);
    if let Some(dir) = &png
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        eprintln!("could not make {}: {e}", dir.display());
        std::process::exit(1);
    }
    println!(
        "bevy scenes: {} x {}, 4x MSAA, scale {}, {} warm-up + {} measured frames",
        settings.size.0, settings.size.1, settings.scale, settings.warmup, settings.frames
    );
    let env = json!({
        "rustc": command_line("rustc", &["--version"]),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "debug_assertions": cfg!(debug_assertions),
        "load_average": load_average(),
    });
    let mut measured = Vec::new();
    for name in SCENES.into_iter().filter(|name| name.contains(&filter)) {
        let scene = measure(name, &settings, png.as_deref());
        println!(
            "{:<32} cpu p50 {:>8.3} ms  p95 {:>8.3} ms   frame p50 {:>8.3} ms  p95 {:>8.3} ms  p99 {:>8.3} ms",
            format!("bevy/scene {name}"),
            scene.cpu.p50,
            scene.cpu.p95,
            scene.frame.p50,
            scene.frame.p95,
            scene.frame.p99
        );
        measured.push(scene);
    }
    let Some(first) = measured.first() else {
        eprintln!("no scene's name contains {filter:?}; there are {SCENES:?}");
        std::process::exit(2);
    };
    if let Some(path) = std::env::var_os("PONKO_BENCH_JSON") {
        let document = json!({
            "engine": "bevy",
            "engine_version": BEVY_VERSION,
            "env": env,
            "gpu": { "adapter": first.adapter },
            "frames": settings.frames,
            "warmup": settings.warmup,
            "width": settings.size.0,
            "height": settings.size.1,
            "samples": 4,
            "scale": settings.scale,
            "scenes": measured.iter().map(|m| json!({
                "name": m.name,
                "content": {
                    "models": m.built.models,
                    "moving_models": m.built.moving_models,
                    "meshes": m.built.meshes,
                    "materials": m.built.materials,
                    "textures": m.built.textures,
                },
                "timings_ms": {
                    "update": m.update.json(),
                    "cpu": m.cpu.json(),
                    "frame": m.frame.json(),
                },
                "gpu": { "timed_frames": 0, "unavailable": NO_GPU_TIMES },
            })).collect::<Vec<_>>(),
        });
        write(&path, "JSON", format!("{document:#}\n"));
    }
    if let Some(path) = std::env::var_os("PONKO_RENDER_CSV") {
        // The columns of the ponko run's summary CSV.
        let mut out = String::from("scene,group,metric,unit,samples,min,p50,p95,p99,max,mean,note\n");
        for m in &measured {
            for (metric, d) in [("update", m.update), ("cpu", m.cpu), ("frame", m.frame)] {
                let _ = writeln!(
                    out,
                    "{},cpu,{metric},ms,{},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},",
                    m.name, d.samples, d.min, d.p50, d.p95, d.p99, d.max, d.mean
                );
            }
            let _ = writeln!(out, "{},gpu,total,ms,0,,,,,,,\"{NO_GPU_TIMES}\"", m.name);
        }
        write(&path, "CSV", out);
    }
}
