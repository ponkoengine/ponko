//! Vertex colours above 1 and a model's colour masked by vertex colour,
//! through a game's own frame (`Ctx::render_frame` with the offscreen
//! renderer). Each test has its negative control. Skips without a GPU.
use super::render_test_support::{Image, keep_image, offscreen_ctx};
use crate::anim::{Joint, Skeleton};
use crate::prelude::*;
use crate::render::{MaterialDesc, Mesh, MeshData, Model, RenderTarget, TargetCamera};

/// The window and the render target.
const SIZE: (u32, u32) = (64, 64);

/// A game with a black background, no sky and no shadows, the sun shining
/// straight at the front of what is drawn, and a target camera (the main
/// camera too) looking at the origin from the front, into an HDR target.
struct Scene {
    ctx: crate::ctx::Ctx,
    target: RenderTarget,
    skeleton: Skeleton,
}

impl Scene {
    fn new() -> Option<Self> {
        let mut ctx = offscreen_ctx(SIZE)?;
        ctx.scene3d.sky.atmosphere = false;
        ctx.scene3d.lighting.shadows = false;
        ctx.scene3d.lighting.sun_direction = vec3(0.0, 0.0, -1.0);
        ctx.gfx.clear_color = Color::BLACK;
        let target = ctx.gfx.create_render_target(SIZE.0, SIZE.1);
        let mut camera = ctx.scene3d.camera;
        camera.position = vec3(0.0, 0.0, 5.0);
        camera.look_at(Vec3::ZERO);
        ctx.scene3d.camera = camera;
        ctx.scene3d
            .passes
            .target_cameras
            .push(TargetCamera::new(camera, target));
        Some(Self {
            ctx,
            target,
            skeleton: Skeleton::new(vec![Joint::new("root", None)]),
        })
    }

    /// A mesh of `data` in a material of `desc`; `skinned`: bound to one
    /// joint, drawn through skinning.
    fn mesh(&mut self, data: &MeshData, desc: MaterialDesc, skinned: bool) -> Mesh {
        let material = self.ctx.gfx.create_material(desc);
        if skinned {
            let mut data = data.clone();
            data.joints = vec![[0; 4]; data.positions.len()];
            data.weights = vec![[1.0, 0.0, 0.0, 0.0]; data.positions.len()];
            self.ctx.gfx.create_skinned_mesh(&data, material, &self.skeleton)
        } else {
            self.ctx.gfx.create_mesh(&data, material)
        }
    }

    /// Draws `mesh` alone at the origin in `color` (twice: the first frame
    /// a mesh is drawn in is the one that puts it on the GPU) and reads the
    /// target: linear RGBA per texel, row by row.
    fn draw(&mut self, mesh: Mesh, color: Color, skinned: bool) -> Vec<[f32; 4]> {
        self.ctx.world.clear();
        let model = (crate::d3::Transform::default(), Model::new(mesh).with_color(color));
        if skinned {
            self.ctx.world.spawn((model.0, model.1, self.skeleton.clone()));
        } else {
            self.ctx.world.spawn(model);
        }
        self.ctx.render_frame();
        self.ctx.render_frame();
        let renderer = self.ctx.gfx.renderer.as_ref().expect("an offscreen renderer");
        let (bytes, width, height) = renderer
            .read_texture_level(self.target.texture().id, 0)
            .expect("the render target");
        let texels: Vec<[f32; 4]> = bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|p| std::array::from_fn(|i| half::f16::from_le_bytes([p[2 * i], p[2 * i + 1]]).to_f32()))
            .collect();
        let encode = |v: f32| (v.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
        keep_image(&Image {
            width,
            height,
            rgba: texels
                .iter()
                .flat_map(|c| [encode(c[0]), encode(c[1]), encode(c[2]), 255])
                .collect(),
        });
        texels
    }
}

/// The texel at world `x` on the middle row (the camera sees about 12
/// texels per meter at the origin).
fn at(texels: &[[f32; 4]], x: f32) -> [f32; 4] {
    let column = (SIZE.0 as f32 / 2.0 + x * 12.0) as u32;
    texels[(SIZE.1 / 2 * SIZE.0 + column) as usize]
}

/// A box of `color` on every corner.
fn colored_box(color: Color) -> MeshData {
    let mut data = MeshData::cube(1.0);
    data.colors = vec![color; data.positions.len()];
    data
}

/// Two boxes side by side in one mesh: the left one's corners have vertex
/// colour alpha 0, the right one's alpha 1 (white otherwise).
fn two_boxes() -> MeshData {
    let mut data = MeshData::default();
    for (x, alpha) in [(-0.6, 0.0), (0.6, 1.0)] {
        let cube = MeshData::cube(0.8);
        let first = data.positions.len() as u32;
        data.positions
            .extend(cube.positions.iter().map(|p| *p + vec3(x, 0.0, 0.0)));
        data.normals.extend(&cube.normals);
        data.uvs.extend(&cube.uvs);
        data.colors.extend(std::iter::repeat_n(
            Color::rgba(1.0, 1.0, 1.0, alpha),
            cube.positions.len(),
        ));
        data.indices.extend(cube.indices.iter().map(|i| i + first));
    }
    data
}

fn close(a: [f32; 4], b: [f32; 4], tolerance: f32) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() <= tolerance * b[i].max(1e-3))
}

#[test]
fn vertex_colours_above_one_light_a_corner_brighter_than_white() {
    let Some(mut scene) = Scene::new() else { return };
    let white = MaterialDesc::color(Color::WHITE).with_roughness(1.0);
    // Control: a vertex colour of 1, and a material colour of 2.
    let one = scene.mesh(&colored_box(Color::WHITE), white, false);
    let one = at(&scene.draw(one, Color::WHITE, false), 0.0);
    let by_material = scene.mesh(&MeshData::cube(1.0), white.with_color(Color::rgb(2.0, 2.0, 2.0)), false);
    let by_material = at(&scene.draw(by_material, Color::WHITE, false), 0.0);
    assert!(one[0] > 0.05, "the front is lit: {one:?}");
    assert!(
        by_material[0] > one[0] * 3.0,
        "a material colour of 2: {by_material:?} vs {one:?}"
    );
    // A vertex colour of 2 is as bright as a material colour of 2, not
    // clipped to 1; through skinning too.
    for skinned in [false, true] {
        let two = scene.mesh(&colored_box(Color::rgb(2.0, 2.0, 2.0)), white, skinned);
        let two = at(&scene.draw(two, Color::WHITE, skinned), 0.0);
        assert!(
            close(two, by_material, 0.02),
            "skinned {skinned}: {two:?} vs a material of 2 {by_material:?}"
        );
    }
    // One channel above 1 keeps the others: a blue of 3 on red 0.5.
    let blue = scene.mesh(&colored_box(Color::rgb(0.5, 0.5, 3.0)), white, false);
    let blue = at(&scene.draw(blue, Color::WHITE, false), 0.0);
    assert!(
        blue[2] > one[2] * 8.0 && close([blue[0], 0.0, 0.0, 0.0], [one[0] * 0.214, 0.0, 0.0, 0.0], 0.05),
        "{blue:?} vs {one:?}"
    );
}

#[test]
fn a_masked_tint_colours_only_where_the_vertex_alpha_says() {
    let Some(mut scene) = Scene::new() else { return };
    let red = Color::rgb(1.0, 0.0, 0.0);
    let white = MaterialDesc::color(Color::WHITE).with_roughness(1.0);
    let lit = |c: [f32; 4]| c[0] > 0.05;
    let reddish = |c: [f32; 4]| lit(c) && c[1] < c[0] * 0.02 && c[2] < c[0] * 0.02;
    // Control: without the mask, the model's colour tints both boxes.
    let plain = scene.mesh(&two_boxes(), white, false);
    let texels = scene.draw(plain, red, false);
    assert!(
        reddish(at(&texels, -0.6)) && reddish(at(&texels, 0.6)),
        "all red: {:?} {:?}",
        at(&texels, -0.6),
        at(&texels, 0.6)
    );
    for skinned in [false, true] {
        let masked = scene.mesh(&two_boxes(), white.with_masked_tint(true), skinned);
        // Masked: the left box (alpha 0) stays as it is untinted (white
        // lit by the warm sun), the right one turns red; alpha 0 isn't
        // see-through either.
        let untinted = scene.draw(masked, Color::WHITE, skinned);
        let texels = scene.draw(masked, red, skinned);
        let (left, right) = (at(&texels, -0.6), at(&texels, 0.6));
        assert!(
            lit(left) && close(left, at(&untinted, -0.6), 0.02) && reddish(right),
            "skinned {skinned}: {left:?} {right:?}"
        );
        assert!(
            close(at(&untinted, 0.6), at(&untinted, -0.6), 0.02),
            "skinned {skinned}: alpha 0 is not see-through"
        );
        // The same mesh and material in another model's colour.
        let blue = at(&scene.draw(masked, Color::rgb(0.0, 0.0, 1.0), skinned), 0.6);
        assert!(
            lit([blue[2], 0.0, 0.0, 0.0]) && blue[0] < blue[2] * 0.02,
            "skinned {skinned}: {blue:?}"
        );
    }
}

/// What the vertex colour's size costs, offscreen at 1920x1080: 400
/// dense spheres (about 6.6 million corners a frame), small on screen so
/// the vertices, not the pixels, take the time, with shadows on (each
/// cascade reads the vertices again). The GPU time of every pass and the
/// whole frame's, and the bytes of one corner. For numbers, not a check:
/// `cargo test --release -p ponko --lib --features 3d vertex_color_costs -- --ignored --nocapture`.
#[test]
#[ignore = "offscreen GPU timestamps at 1080p; run in release with --ignored --nocapture"]
fn vertex_color_costs() {
    const FRAMES: usize = 240;
    let run = || -> Option<(Vec<(&'static str, f32)>, f64)> {
        let gpu = super::render_test_support::gpu_with(|adapter| {
            Ok(wgpu::DeviceDescriptor {
                required_features: adapter.features() & wgpu::Features::TIMESTAMP_QUERY,
                ..Default::default()
            })
        })?;
        let size = (1920, 1080);
        let mut ctx = crate::ctx::Ctx::new(1.0 / 60.0, vec2(size.0 as f32, size.1 as f32));
        ctx.audio.headless = true;
        ctx.gfx.renderer = Some(super::gpu::Renderer::offscreen(gpu, size));
        ctx.scene3d.lighting.shadows = true;
        let mesh = {
            let mut data = MeshData::sphere(0.2, 128);
            data.colors = (0..data.positions.len())
                .map(|i| Color::rgb((i % 7) as f32 / 7.0, 0.5, 1.0))
                .collect();
            let material = ctx.gfx.create_material(MaterialDesc::color(Color::rgb(0.8, 0.5, 0.3)));
            ctx.gfx.create_mesh(&data, material)
        };
        for i in 0..400 {
            let at = vec3((i % 20) as f32 - 9.5, (i / 20) as f32 * 0.6 - 6.0, -12.0);
            ctx.world.spawn((crate::d3::Transform::at(at), Model::new(mesh)));
        }
        ctx.gfx.renderer.as_mut()?.set_gpu_timing_every(1);
        for _ in 0..30 {
            ctx.render_frame();
        }
        let mut sums: Vec<(&'static str, f32)> = Vec::new();
        let started = std::time::Instant::now();
        for _ in 0..FRAMES {
            ctx.render_frame();
            for p in ctx.gfx.renderer.as_ref()?.gpu_passes() {
                match sums.iter_mut().find(|(n, _)| *n == p.name) {
                    Some((_, ms)) => *ms += p.ms,
                    None => sums.push((p.name, p.ms)),
                }
            }
        }
        let cpu = started.elapsed().as_secs_f64() * 1000.0 / FRAMES as f64;
        for (_, ms) in &mut sums {
            *ms /= FRAMES as f32;
        }
        Some((sums, cpu))
    };
    if run().is_none() {
        return;
    }
    eprintln!("one corner: {} bytes", super::gpu3d::VERTEX_SIZE);
    for round in 1..=3 {
        let (passes, cpu) = run().expect("a GPU, as above");
        let total: f32 = passes.iter().map(|p| p.1).sum();
        let list: Vec<String> = passes
            .iter()
            .filter(|p| p.1 >= 0.05)
            .map(|(n, ms)| format!("{n} {ms:.3}"))
            .collect();
        eprintln!(
            "run {round}: GPU {total:.3} ms ({}), frame on the CPU {cpu:.2} ms",
            list.join(", ")
        );
    }
}
