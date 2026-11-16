//! Level-of-detail groups drawn offscreen together with what else decides
//! whether they show: skinned levels posed by the group's skeleton (each
//! level, and both through a crossfade), occlusion culling against an
//! earlier frame's depth, and a camera moving fast. Every frame must draw
//! the level `LodGroup::pick` chooses and nothing less than a frame drawn
//! without occlusion culling. Groups made into static batches
//! (`LodGroup::instance_levels`) must draw the same levels as the groups,
//! on the GPU-driven path and on a device without compute culling. Skips
//! without a GPU.
use super::gpu3d::*;
use super::render_test_support as support;
use super::static_batch_render_test::{BatchRig, RigSetup};
use crate::anim::{Joint, Skeleton};
use crate::d3::Transform as Transform3D;
use crate::prelude::*;
use crate::render::{
    BatchCulling, GpuFeature, InstanceBatchOptions, InstanceLevel, LodGroup, LodHysteresis, LodLevel, LodPick,
    LodRenderer, LodSettings, LodShadow, Material, MaterialDesc, Mesh, MeshData, Model, ModelMaterials,
};
use crate::three::{Frame3D, GpuMaterialDesc, Scene3D, SkyLight};

const SIZE: u32 = 128;

/// An offscreen renderer, a `Ctx` to fill and the frame kept between draws
/// (it holds the occlusion grid and the hysteresis), drawing skinned models.
struct Rig {
    device: wgpu::Device,
    queue: wgpu::Queue,
    output: wgpu::Texture,
    target: wgpu::TextureView,
    textures: Vec<super::gpu::GpuTexture>,
    renderer: Renderer3D,
    ctx: Ctx,
    frame: Frame3D,
}

impl Rig {
    /// `None` without a GPU.
    fn new() -> Option<Self> {
        let gpu = support::gpu()?;
        let (device, queue) = (gpu.device, gpu.queue);
        let (layout, white, textures) = support::white_textures(&device, &queue);
        let output = support::target(&device, (SIZE, SIZE), wgpu::TextureFormat::Rgba8UnormSrgb);
        let target = output.create_view(&Default::default());
        let renderer = Renderer3D::new(&device, &layout, output.format(), 0, &white, 4, true);
        let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(SIZE as f32, SIZE as f32));
        ctx.scene3d.sky.atmosphere = false;
        ctx.scene3d.lighting.shadows = false;
        Some(Self {
            device,
            queue,
            output,
            target,
            textures,
            renderer,
            ctx,
            frame: Frame3D::default(),
        })
    }

    /// A glowing mesh of `data` in `color`, on the CPU and the GPU.
    fn mesh(&mut self, data: &MeshData, color: Color) -> Mesh {
        let desc = glowing(color);
        let material = self.ctx.gfx.create_material(desc);
        let mesh = self.ctx.gfx.create_mesh(data, material);
        self.renderer
            .create_material(&self.device, &GpuMaterialDesc::new(&desc), &self.textures);
        self.renderer
            .create_mesh(&self.device, data, &[(0..data.indices.len() as u32, material)], true);
        mesh
    }

    /// A glowing bar 2 tall standing on the origin, in `color`, its upper
    /// half bound to joint 1 of [`arm`].
    fn bar(&mut self, color: Color) -> Mesh {
        let mut bar = MeshData::cube(1.0);
        for p in &mut bar.positions {
            *p = vec3(p.x * 0.3, (p.y + 0.5) * 2.0, p.z * 0.3);
        }
        bar.joints = bar.positions.iter().map(|p| [u16::from(p.y > 1.0), 0, 0, 0]).collect();
        bar.weights = vec![[1.0, 0.0, 0.0, 0.0]; bar.positions.len()];
        let desc = glowing(color);
        let material = self.ctx.gfx.create_material(desc);
        let mesh = self.ctx.gfx.create_skinned_mesh(&bar, material, &arm());
        self.renderer
            .create_material(&self.device, &GpuMaterialDesc::new(&desc), &self.textures);
        let mut weighted = bar.clone();
        for w in &mut weighted.weights {
            *w = crate::three::normalized_weights(*w);
        }
        self.renderer.create_mesh(
            &self.device,
            &weighted,
            &[(0..bar.indices.len() as u32, material)],
            true,
        );
        mesh
    }

    /// Draws the world as a game's frame does (skeletons first, occlusion
    /// against the depth the renderer handed back) and reads the pixels.
    fn draw(&mut self) -> Vec<u8> {
        let ctx = &mut self.ctx;
        let mut skins = std::mem::take(&mut self.frame.skins);
        skins.collect(&mut ctx.world, &ctx.gfx.assets3d);
        let frame = &mut self.frame;
        frame.lod = ctx.scene3d.lod;
        if !ctx.scene3d.camera.occlusion_culling {
            frame.occlusion = None;
        } else if let Some(grid) = self.renderer.take_occlusion() {
            frame.occlusion = Some(grid);
        }
        frame.collect(
            &mut ctx.world,
            &ctx.gfx.assets3d,
            &skins,
            &ctx.scene3d.camera,
            &ctx.scene3d.lighting,
        );
        frame.skins = skins;
        let scene = Scene3D {
            frame,
            lighting: &ctx.scene3d.lighting,
            sky: &ctx.scene3d.sky,
            light: SkyLight {
                sun: Vec3::ZERO,
                sky: Vec3::ZERO,
                ground: Vec3::ZERO,
            },
            camera: ctx.scene3d.camera.position,
            post: &ctx.scene3d.post,
            time: (0.0, 1.0 / 60.0),
            occlusion: ctx.scene3d.camera.occlusion_culling,
        };
        self.renderer.prepare(
            &self.device,
            &self.queue,
            &Prepare3D {
                scene: &scene,
                lut: None,
                size: (SIZE, SIZE),
            },
        );
        let buffers = self
            .renderer
            .draw(&self.device, &self.target, wgpu::Color::BLACK, &self.textures, None);
        self.queue.submit(buffers);
        self.renderer.submitted();
        support::read_texture(&self.device, &self.queue, &self.output)
    }

    /// Whether the camera sees `at` lit: most of the 3 x 3 pixels around
    /// where it lands, so one pixel of rasterisation difference between
    /// devices (a software one's edges) doesn't decide it.
    fn lit(&self, pixels: &[u8], at: Vec3) -> bool {
        let clip = self.ctx.scene3d.camera.view_proj() * at.extend(1.0);
        let ndc = clip.truncate() / clip.w;
        let last = SIZE as i32 - 1;
        let x = ((ndc.x * 0.5 + 0.5) * SIZE as f32) as i32;
        let y = ((0.5 - ndc.y * 0.5) * SIZE as f32) as i32;
        let mut lit = 0;
        for (dx, dy) in (-1..=1).flat_map(|dx| (-1..=1).map(move |dy| (dx, dy))) {
            let (px, py) = ((x + dx).clamp(0, last) as usize, (y + dy).clamp(0, last) as usize);
            let p = &pixels[(py * SIZE as usize + px) * 4..][..3];
            lit += usize::from(p.iter().any(|&c| c > 100));
        }
        lit >= 5
    }

    /// A glowing material in `color`, on the CPU and the GPU.
    fn material(&mut self, color: Color) -> Material {
        let desc = glowing(color);
        let material = self.ctx.gfx.create_material(desc);
        self.renderer
            .create_material(&self.device, &GpuMaterialDesc::new(&desc), &self.textures);
        material
    }
}

/// A material that shows as a flat `color`, whatever the light.
fn glowing(color: Color) -> MaterialDesc {
    MaterialDesc {
        color: Color::rgb(0.0, 0.0, 0.0),
        emissive: color,
        roughness: 1.0,
        ..Default::default()
    }
}

/// Two joints: a base, and a top 1 up.
fn arm() -> Skeleton {
    Skeleton::new(vec![
        Joint::new("base", None),
        Joint::new("top", 0).with_rest(Transform3D::at(vec3(0.0, 1.0, 0.0))),
    ])
}

/// The arm with its top turned a quarter to the left: a bar bound to it
/// points left from 1 up instead of standing.
fn bent() -> Skeleton {
    let mut s = arm();
    s.pose[1].rotation = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
    s
}

/// Pixels counted by colour: red, green and blue ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Count {
    red: usize,
    green: usize,
    blue: usize,
}

fn count(pixels: &[u8]) -> Count {
    let mut c = Count::default();
    for p in pixels.as_chunks::<4>().0 {
        match (p[0] > 100, p[1] > 100, p[2] > 100) {
            (true, false, false) => c.red += 1,
            (false, true, false) => c.green += 1,
            (false, false, true) => c.blue += 1,
            _ => {}
        }
    }
    c
}

/// A group of size 2 with a red and a green level, each `meshes` (one
/// renderer each), switching where it is 40% of the screen tall, gone below
/// 2%; `fade` crossfades the first into the second.
fn red_green(red: Mesh, green: Mesh, fade: f32) -> LodGroup {
    LodGroup::new(
        2.0,
        vec![
            LodLevel::new(0.4, vec![LodRenderer::new(red)]).with_fade(fade),
            LodLevel::new(0.02, vec![LodRenderer::new(green)]),
        ],
    )
}

/// Where the bent bar is (its top face turned to stand at x -1, the bar
/// leaning from the origin up to it), and where its unposed top half would be.
const BESIDE: Vec3 = vec3(-0.8, 0.85, 0.0);
const ABOVE: Vec3 = vec3(0.0, 1.75, 0.0);

/// Looks at the bar from `distance` in front.
fn look(ctx: &mut Ctx, distance: f32) {
    ctx.scene3d.camera.position = vec3(0.0, 1.0, distance);
    ctx.scene3d.camera.look_at(vec3(0.0, 1.0, 0.0));
}

#[test]
fn a_skinned_lod_group_is_drawn_posed_at_every_level() {
    let Some(mut rig) = Rig::new() else {
        return;
    };
    let (red, green) = (rig.bar(Color::rgb(1.0, 0.0, 0.0)), rig.bar(Color::rgb(0.0, 1.0, 0.0)));
    let group = rig
        .ctx
        .world
        .spawn((Transform3D::default(), red_green(red, green, 0.0), bent()));
    for (distance, level) in [(2.5, 0), (8.0, 1)] {
        look(&mut rig.ctx, distance);
        let g = rig.ctx.world.get::<LodGroup>(group).unwrap();
        assert_eq!(
            g.pick_for(&Transform3D::default(), &rig.ctx.scene3d.camera, &rig.ctx.scene3d.lod),
            LodPick::Level(level),
            "the scene asks for level {level} at {distance} m"
        );
        let pixels = rig.draw();
        let c = count(&pixels);
        assert!(rig.lit(&pixels, BESIDE), "{distance} m: the posed top half points left");
        assert!(
            !rig.lit(&pixels, ABOVE),
            "{distance} m: nothing stands where the rest pose would"
        );
        let (shown, hidden) = if level == 0 { (c.red, c.green) } else { (c.green, c.red) };
        assert!(
            shown > 20 && hidden == 0,
            "{distance} m: only level {level} is drawn: {c:?}"
        );
    }
    // Negative control: the same group without a skeleton has nothing to
    // pose it, and draws its meshes as modelled (standing).
    rig.ctx.world.remove::<Skeleton>(group);
    look(&mut rig.ctx, 2.5);
    let pixels = rig.draw();
    assert!(rig.lit(&pixels, ABOVE), "without a skeleton the bar stands");
    assert!(!rig.lit(&pixels, BESIDE));
}

#[test]
fn a_skinned_lod_crossfade_draws_both_levels_posed() {
    let Some(mut rig) = Rig::new() else {
        return;
    };
    let (red, green) = (rig.bar(Color::rgb(1.0, 0.0, 0.0)), rig.bar(Color::rgb(0.0, 1.0, 0.0)));
    rig.ctx
        .world
        .spawn((Transform3D::default(), red_green(red, green, 1.0), bent()));
    // Half way through the fade band, between 40% and a full screen.
    let fov = rig.ctx.scene3d.camera.fov_y;
    look(&mut rig.ctx, LodGroup::distance_at(2.0, 0.7, fov));
    let g = rig.ctx.world.query::<&LodGroup>().iter().next().unwrap().clone();
    assert!(matches!(
        g.pick_for(&Transform3D::default(), &rig.ctx.scene3d.camera, &rig.ctx.scene3d.lod),
        LodPick::Fade { .. }
    ));
    let pixels = rig.draw();
    let c = count(&pixels);
    assert!(c.red > 10 && c.green > 10, "both levels are drawn, dithered: {c:?}");
    assert!(rig.lit(&pixels, BESIDE), "the dither leaves no hole in the posed bar");
    assert!(!rig.lit(&pixels, ABOVE), "neither level is drawn unposed");
}

#[test]
fn a_skinned_lod_group_keeps_its_pose_and_level_under_fast_camera_motion() {
    let Some(mut rig) = Rig::new() else {
        return;
    };
    let (red, green) = (rig.bar(Color::rgb(1.0, 0.0, 0.0)), rig.bar(Color::rgb(0.0, 1.0, 0.0)));
    let group = red_green(red, green, 0.3);
    rig.ctx.world.spawn((Transform3D::default(), group.clone(), bent()));
    // Big jumps in and out, swinging from side to side: every frame a
    // very different view, through both levels and the fade between
    // (2.9 to 4.2 m away).
    let distances = [
        2.2, 3.4, 12.0, 3.6, 18.0, 2.6, 7.0, 3.3, 15.0, 2.3, 9.5, 3.9, 2.4, 20.0, 3.5, 5.0,
    ];
    let mut seen = [false; 3];
    for (k, &distance) in distances.iter().enumerate() {
        let side = if k % 2 == 0 { 1.0 } else { -1.0 } * distance * 0.3;
        rig.ctx.scene3d.camera.position = vec3(side, 1.0, distance);
        rig.ctx.scene3d.camera.look_at(vec3(0.0, 1.0, 0.0));
        let pick = group.pick_for(&Transform3D::default(), &rig.ctx.scene3d.camera, &rig.ctx.scene3d.lod);
        let pixels = rig.draw();
        let c = count(&pixels);
        assert!(!rig.lit(&pixels, ABOVE), "frame {k}: drawn unposed");
        match pick {
            LodPick::Level(0) => {
                seen[0] = true;
                assert!(c.red > 0 && c.green == 0, "frame {k}: level 0 only: {c:?}");
            }
            LodPick::Level(1) => {
                seen[1] = true;
                assert!(c.green > 0 && c.red == 0, "frame {k}: level 1 only: {c:?}");
            }
            LodPick::Fade { .. } => {
                seen[2] = true;
                assert!(c.red + c.green > 0, "frame {k}: the fade draws the group: {c:?}");
            }
            other => panic!("frame {k}: the path stays where the group is drawn: {other:?}"),
        }
    }
    assert_eq!(seen, [true; 3], "the path crosses both levels and the fade");
}

/// A blue wall at z 4, and behind it (and beside it) a row of red/green
/// groups of cubes, the middle one a skinned bar.
fn wall_scene(rig: &mut Rig) {
    let wall = rig.mesh(&MeshData::cube(1.0), Color::rgb(0.0, 0.0, 1.0));
    rig.ctx.world.spawn((
        Transform3D {
            position: vec3(0.0, 1.0, 4.0),
            scale: vec3(5.0, 4.0, 0.5),
            ..Default::default()
        },
        Model::new(wall),
    ));
    let red = rig.mesh(&MeshData::cube(1.0), Color::rgb(1.0, 0.0, 0.0));
    let green = rig.mesh(&MeshData::cube(1.0), Color::rgb(0.0, 1.0, 0.0));
    for x in [-9.0, -6.0, -3.0, 3.0, 6.0, 9.0] {
        rig.ctx.world.spawn((
            Transform3D::at(vec3(x, 1.0, 0.0)),
            LodGroup::new(
                1.0,
                vec![
                    LodLevel::new(0.12, vec![LodRenderer::new(red)]).with_fade(0.3),
                    LodLevel::new(0.01, vec![LodRenderer::new(green)]),
                ],
            ),
        ));
    }
    let (red_bar, green_bar) = (rig.bar(Color::rgb(1.0, 0.0, 0.0)), rig.bar(Color::rgb(0.0, 1.0, 0.0)));
    rig.ctx.world.spawn((
        Transform3D::default(),
        LodGroup::new(
            2.0,
            vec![
                LodLevel::new(0.2, vec![LodRenderer::new(red_bar)]).with_fade(0.3),
                LodLevel::new(0.01, vec![LodRenderer::new(green_bar)]),
            ],
        ),
        bent(),
    ));
}

/// Where the camera is in frame `k` of [`a_fast_camera_past_a_wall_draws_every_group_occlusion_culling_keeps`]:
/// at rest behind the wall (where the depth comes back and culls), then
/// strafing 1.6 m a frame from far left to far right, then back closer in,
/// looking straight ahead.
fn strafe(ctx: &mut Ctx, k: usize) {
    let (x, z) = match k {
        0..6 => (0.0, 13.0),
        6..22 => (-12.0 + (k - 6) as f32 * 1.6, 13.0),
        _ => (12.0 - (k - 22) as f32 * 1.6, 8.0),
    };
    ctx.scene3d.camera.position = vec3(x, 1.0, z);
    ctx.scene3d.camera.look_at(vec3(x * 0.5, 1.0, 0.0));
}

#[test]
fn a_fast_camera_past_a_wall_draws_every_group_occlusion_culling_keeps() {
    let (Some(mut culled), Some(mut all)) = (Rig::new(), Rig::new()) else {
        return;
    };
    if !support::needs(&culled.device, GpuFeature::OcclusionCulling) {
        return;
    }
    wall_scene(&mut culled);
    wall_scene(&mut all);
    culled.ctx.scene3d.camera.occlusion_culling = true;
    all.ctx.scene3d.camera.occlusion_culling = false;
    let (mut hidden, mut visible) = (0, 0);
    for k in 0..38 {
        strafe(&mut culled.ctx, k);
        strafe(&mut all.ctx, k);
        let (a, b) = (count(&culled.draw()), count(&all.draw()));
        hidden += culled.frame.occluded;
        assert_eq!(all.frame.occluded, 0);
        // What occlusion culling leaves out was hidden: the same pixels of
        // each level (a pixel of slack for the edges of the dither).
        let close = |x: usize, y: usize| x.abs_diff(y) <= 1 + y / 100;
        assert!(
            close(a.red, b.red) && close(a.green, b.green) && close(a.blue, b.blue),
            "frame {k}: occlusion culling changed the picture: {a:?} against {b:?}"
        );
        visible += usize::from(b.red + b.green > 0);
    }
    assert!(visible > 20, "groups are in view in most frames: {visible}");
    // Negative control: the wall did hide groups, which were culled.
    assert!(hidden > 0, "nothing was ever culled: the test checks nothing");
}

#[test]
fn a_hidden_lod_renderer_whose_shadow_is_drawn_counts_as_occluded_once() {
    let Some(mut rig) = Rig::new() else {
        return;
    };
    if !support::needs(&rig.device, GpuFeature::OcclusionCulling) {
        return;
    }
    let wall = rig.mesh(&MeshData::cube(1.0), Color::rgb(0.0, 0.0, 1.0));
    rig.ctx.world.spawn((
        Transform3D {
            position: vec3(0.0, 1.0, 4.0),
            scale: vec3(6.0, 6.0, 0.5),
            ..Default::default()
        },
        Model::new(wall),
    ));
    // Behind the wall: a group of one renderer, which still casts into the
    // sun's shadow maps, and a shadow-only caster the camera never draws.
    let red = rig.mesh(&MeshData::cube(1.0), Color::rgb(1.0, 0.0, 0.0));
    rig.ctx.world.spawn((
        Transform3D::at(vec3(0.0, 1.0, 0.0)),
        LodGroup::new(1.0, vec![LodLevel::new(0.01, vec![LodRenderer::new(red)])]),
    ));
    let caster = rig.mesh(&MeshData::cube(1.0), Color::rgb(0.0, 1.0, 0.0));
    rig.ctx.world.spawn((
        Transform3D::at(vec3(0.5, 1.0, -1.0)),
        crate::render::ShadowMesh(Some(caster)),
    ));
    rig.ctx.scene3d.lighting.shadows = true;
    rig.ctx.scene3d.lighting.sun_direction = vec3(0.3, -1.0, -0.2);
    rig.ctx.scene3d.camera.occlusion_culling = true;
    look(&mut rig.ctx, 12.0);
    // The depth comes back a frame or two later; then the group is hidden.
    let mut culled = false;
    for _ in 0..6 {
        let pixels = rig.draw();
        if rig.frame.occluded > 0 {
            culled = true;
            assert_eq!(count(&pixels).red, 0, "the group is behind the wall");
            break;
        }
    }
    assert!(culled, "the group was never culled");
    // Its renderer still casts: the shadow maps draw it.
    let shadow_drawn =
        (0..rig.frame.cascades).any(|v| rig.frame.shadow_runs[v].iter().any(|&(mesh, _, _)| mesh == red.slot()));
    assert!(
        shadow_drawn,
        "the hidden group still casts its shadow: the test needs it seen that way"
    );
    assert_eq!(
        rig.frame.occluded, 1,
        "one renderer hidden from the camera: counted once, and the shadow-only caster not at all"
    );
}

/// A wall between the camera (12 m in front, see [`look`]) and whatever
/// stands near the origin, with sun shadows: what stands there is hidden
/// from the camera and still casts.
fn behind_a_wall(rig: &mut Rig) {
    let wall = rig.mesh(&MeshData::cube(1.0), Color::rgb(0.0, 0.0, 1.0));
    rig.ctx.world.spawn((
        Transform3D {
            position: vec3(0.0, 1.0, 4.0),
            scale: vec3(6.0, 6.0, 0.5),
            ..Default::default()
        },
        Model::new(wall),
    ));
    rig.ctx.scene3d.lighting.shadows = true;
    rig.ctx.scene3d.lighting.sun_direction = vec3(0.3, -1.0, -0.2);
    rig.ctx.scene3d.camera.occlusion_culling = true;
    look(&mut rig.ctx, 12.0);
}

/// Draws until the occlusion grid hides something (the depth comes back a
/// frame or two late), then twice more: `occluded` on those two frames.
fn occluded_counts(rig: &mut Rig, what: &str) -> [usize; 2] {
    for _ in 0..6 {
        let pixels = rig.draw();
        if rig.frame.occluded > 0 {
            let c = count(&pixels);
            assert_eq!(c.red + c.green, 0, "{what}: the group is behind the wall: {c:?}");
            return [(), ()].map(|()| {
                rig.draw();
                rig.frame.occluded
            });
        }
    }
    panic!("{what}: the group was never culled");
}

/// Whether the shadow maps draw `mesh` this frame (dithered or not: a
/// fading mesh's runs carry the top bit).
fn casts(rig: &Rig, mesh: Mesh) -> bool {
    (0..rig.frame.cascades).any(|v| {
        rig.frame.shadow_runs[v]
            .iter()
            .any(|&(m, _, _)| m & !(1 << 31) == mesh.slot())
    })
}

#[test]
fn a_hidden_lod_crossfade_counts_each_of_its_two_renderers_once() {
    let Some(mut rig) = Rig::new() else {
        return;
    };
    if !support::needs(&rig.device, GpuFeature::OcclusionCulling) {
        return;
    }
    behind_a_wall(&mut rig);
    let red = rig.mesh(&MeshData::cube(1.0), Color::rgb(1.0, 0.0, 0.0));
    let green = rig.mesh(&MeshData::cube(1.0), Color::rgb(0.0, 1.0, 0.0));
    // Level 0 down to half of what the group covers from the camera,
    // fading over all above that: the camera is inside the fade.
    let h = LodGroup::screen_height_at(1.0, 12.0, rig.ctx.scene3d.camera.fov_y);
    let group = LodGroup::new(
        1.0,
        vec![
            LodLevel::new(h * 0.5, vec![LodRenderer::new(red)]).with_fade(1.0),
            LodLevel::new(0.001, vec![LodRenderer::new(green)]),
        ],
    );
    let at = Transform3D::at(vec3(0.0, 1.0, 0.0));
    assert!(matches!(
        group.pick_for(&at, &rig.ctx.scene3d.camera, &rig.ctx.scene3d.lod),
        LodPick::Fade { .. }
    ));
    rig.ctx.world.spawn((at, group));
    assert_eq!(
        occluded_counts(&mut rig, "crossfade"),
        [2, 2],
        "both renderers of the fade are hidden: each counted once"
    );
    assert!(casts(&rig, red) && casts(&rig, green), "both still cast");
}

#[test]
fn a_hidden_lod_group_held_by_hysteresis_counts_once() {
    let Some(mut rig) = Rig::new() else {
        return;
    };
    if !support::needs(&rig.device, GpuFeature::OcclusionCulling) {
        return;
    }
    behind_a_wall(&mut rig);
    rig.ctx.scene3d.lod.crossfade = false;
    let red = rig.mesh(&MeshData::cube(1.0), Color::rgb(1.0, 0.0, 0.0));
    let green = rig.mesh(&MeshData::cube(1.0), Color::rgb(0.0, 1.0, 0.0));
    // Level 0 while the group covers what it does from 11.8 m or more.
    let fov = rig.ctx.scene3d.camera.fov_y;
    let group = LodGroup::new(
        1.0,
        vec![
            LodLevel::new(LodGroup::screen_height_at(1.0, 11.8, fov), vec![LodRenderer::new(red)]),
            LodLevel::new(0.001, vec![LodRenderer::new(green)]),
        ],
    );
    let at = Transform3D::at(vec3(0.0, 1.0, 0.0));
    rig.ctx.world.spawn((at, group.clone(), LodHysteresis(0.2)));
    // From 11.5 m: level 0. Then from 12 m the plain pick is level 1, and
    // hysteresis holds level 0: the group is picked again every frame.
    look(&mut rig.ctx, 11.5);
    rig.draw();
    look(&mut rig.ctx, 12.0);
    assert_eq!(
        group.pick_for(&at, &rig.ctx.scene3d.camera, &rig.ctx.scene3d.lod),
        LodPick::Level(1)
    );
    let counts = occluded_counts(&mut rig, "hysteresis");
    assert!(
        casts(&rig, red) && !casts(&rig, green),
        "hysteresis holds level 0 (its shadow shows which)"
    );
    assert_eq!(counts, [1, 1], "the held level's one renderer, counted once");
}

#[test]
fn a_hidden_skinned_lod_group_counts_each_drawn_renderer_once() {
    for what in ["plain", "a skinned shadow-only caster", "a material override"] {
        let Some(mut rig) = Rig::new() else {
            return;
        };
        if !support::needs(&rig.device, GpuFeature::OcclusionCulling) {
            return;
        }
        behind_a_wall(&mut rig);
        let red = rig.bar(Color::rgb(1.0, 0.0, 0.0));
        let mut level = LodLevel::new(0.01, vec![LodRenderer::new(red)]);
        if what == "a skinned shadow-only caster" {
            let caster = rig.bar(Color::rgb(0.0, 1.0, 0.0));
            level = level.with_shadow(LodShadow::Casters(vec![LodRenderer::new(caster)]));
        }
        let group = rig
            .ctx
            .world
            .spawn((Transform3D::default(), LodGroup::new(2.0, vec![level]), bent()));
        if what == "a material override" {
            let white = rig.material(Color::rgb(1.0, 1.0, 1.0));
            rig.ctx.world.insert(group, ModelMaterials::new(&[(0, white)]));
        }
        assert_eq!(
            occluded_counts(&mut rig, what),
            [1, 1],
            "{what}: the one renderer the camera would draw, counted once"
        );
    }
}

// ---------------------------------------------------------------- batches

/// The three levels of the batch tests: a red cube near, a green sphere,
/// a blue box far.
fn three_levels(rig: &mut BatchRig) -> LodGroup {
    let red = rig.mesh(&MeshData::cube(1.0), glowing(Color::rgb(1.0, 0.0, 0.0)));
    let green = rig.mesh(&MeshData::sphere(0.5, 8), glowing(Color::rgb(0.0, 1.0, 0.0)));
    let blue = rig.mesh(&MeshData::cube(0.6), glowing(Color::rgb(0.0, 0.0, 1.0)));
    LodGroup::new(
        1.0,
        vec![
            LodLevel::new(0.25, vec![LodRenderer::new(red)]),
            LodLevel::new(0.08, vec![LodRenderer::new(green)]),
            LodLevel::new(0.015, vec![LodRenderer::new(blue)]),
        ],
    )
}

/// How the field is drawn: as entities, or as one batch of the group's
/// instance levels made with a bias.
#[derive(Clone, Copy)]
enum Field {
    Entities,
    Batch { culling: BatchCulling, bias: f32 },
}

/// A field of 8 x 8 groups 3 m apart, drawn as `how` says.
fn field(rig: &mut BatchRig, how: Field) -> LodGroup {
    let group = three_levels(rig);
    let transforms: Vec<Mat4> = (0..64)
        .map(|i| Mat4::from_translation(vec3((i % 8) as f32 * 3.0 - 10.5, 0.0, -((i / 8) as f32) * 3.0)))
        .collect();
    rig.ctx.scene3d.lod.crossfade = false;
    match how {
        Field::Entities => {
            for m in &transforms {
                rig.ctx
                    .world
                    .spawn((Transform3D::at(m.w_axis.truncate()), group.clone()));
            }
        }
        Field::Batch { culling, bias } => {
            let settings = LodSettings {
                bias,
                ..rig.ctx.scene3d.lod
            };
            let levels = group
                .instance_levels(1.0, rig.ctx.scene3d.camera.fov_y, &settings)
                .expect("a plain group");
            rig.batch_levels(
                &levels,
                &transforms,
                &[],
                InstanceBatchOptions {
                    culling,
                    cell_size: 6.0,
                    ..Default::default()
                },
            );
        }
    }
    group
}

/// Frame `k` of a fast flight low over the field and away from it.
fn fly(ctx: &mut Ctx, k: usize) {
    let z = 1.5 + k as f32 * 1.2;
    let x = (k as f32 * 0.9).sin() * 3.7;
    ctx.scene3d.camera.position = vec3(x, 1.5 + k as f32 * 0.2, z);
    ctx.scene3d.camera.look_at(vec3(0.0, 0.0, -10.0));
}

/// Draws `entities` and `batch` (made by [`field`]) along the flight and
/// compares the pixels of each level: the first frame whose levels differ,
/// or `None` when every frame shows the same.
fn level_mismatch(entities: &mut BatchRig, batch: &mut BatchRig, group: &LodGroup, what: &str) -> Option<String> {
    let fov = entities.ctx.scene3d.camera.fov_y;
    let levels = group.instance_levels(1.0, fov, &entities.ctx.scene3d.lod).unwrap();
    let mut shown = Count::default();
    for k in 0..20 {
        fly(&mut entities.ctx, k);
        fly(&mut batch.ctx, k);
        // No copy sits on a switch distance, where rounding picks either.
        let eye = entities.ctx.scene3d.camera.position;
        for i in 0..64 {
            let at = vec3((i % 8) as f32 * 3.0 - 10.5, 0.0, -((i / 8) as f32) * 3.0);
            let d = at.distance(eye);
            for l in &levels {
                assert!(
                    (d - l.max_distance).abs() > 1e-3,
                    "{what}, frame {k}: copy {i} at a switch: move the flight"
                );
            }
        }
        let (a, b) = (count(&entities.image().rgba), count(&batch.image().rgba));
        assert!(a.red + a.green + a.blue > 100, "frame {k}: the field is in view");
        let close = |x: usize, y: usize| x.abs_diff(y) <= 2 + y / 50;
        if !(close(a.red, b.red) && close(a.green, b.green) && close(a.blue, b.blue)) {
            return Some(format!(
                "{what}, frame {k}: the batch draws other levels than the groups: {b:?} against {a:?}"
            ));
        }
        shown.red += a.red;
        shown.green += a.green;
        shown.blue += a.blue;
    }
    assert!(
        shown.red > 0 && shown.green > 0 && shown.blue > 0,
        "{what}: the flight shows every level: {shown:?}"
    );
    None
}

/// The batch draws the levels the entities do along the flight, and a
/// batch made with another bias does not: the comparison sees levels.
fn same_levels(setup: RigSetup, culling: BatchCulling, route: Route, what: &str) {
    let (Some(mut entities), Some(mut batch), Some(mut wrong)) =
        (BatchRig::new(setup), BatchRig::new(setup), BatchRig::new(setup))
    else {
        return;
    };
    if route == Route::Gpu && !support::needs(&batch.gpu.device, GpuFeature::ComputeCulling) {
        return;
    }
    let group = field(&mut entities, Field::Entities);
    field(&mut batch, Field::Batch { culling, bias: 1.0 });
    // The control: bands for a bias of 0.5, so levels switch at half the
    // distances the entities switch at.
    field(&mut wrong, Field::Batch { culling, bias: 0.5 });
    assert_eq!(batch.renderer.batch_route(0), Some(route), "{what}");
    assert_eq!(wrong.renderer.batch_route(0), Some(route), "{what}, the control");
    if let Some(mismatch) = level_mismatch(&mut entities, &mut batch, &group, what) {
        panic!("{mismatch}");
    }
    let control = level_mismatch(&mut entities, &mut wrong, &group, &format!("{what}, bias 0.5"));
    assert!(
        control.is_some(),
        "{what}: a batch made with another bias draws the same pixels: the test can't see levels"
    );
}

#[test]
fn lod_groups_as_gpu_batch_levels_draw_the_levels_the_groups_draw() {
    // The levels are picked on the GPU.
    same_levels(RigSetup::default(), BatchCulling::PreferGpu, Route::Gpu, "GPU-driven");
}

#[test]
fn lod_groups_as_batch_levels_fall_back_to_cpu_picking_without_compute_culling() {
    let webgl2 = RigSetup {
        compute: false,
        ..Default::default()
    };
    // Asking for the GPU on a device that can't: the CPU path all the same.
    same_levels(
        webgl2,
        BatchCulling::PreferGpu,
        Route::CpuNoCompute,
        "no compute culling",
    );
}

#[test]
fn auto_keeps_a_batch_of_lod_levels_on_the_cpu_at_any_size() {
    let Some(mut rig) = BatchRig::new(RigSetup::default()) else {
        return;
    };
    if !support::needs(&rig.gpu.device, GpuFeature::ComputeCulling) {
        return;
    }
    let group = three_levels(&mut rig);
    let levels = group
        .instance_levels(1.0, rig.ctx.scene3d.camera.fov_y, &rig.ctx.scene3d.lod)
        .unwrap();
    assert_eq!(levels.len(), 3);
    // Past the crossover that sends a one-level batch to the GPU.
    let n = super::capabilities::GPU_BATCH_MIN as usize;
    let transforms: Vec<Mat4> = (0..n)
        .map(|i| Mat4::from_translation(vec3((i % 512) as f32 * 0.5 - 128.0, 0.0, -((i / 512) as f32) * 0.5)))
        .collect();
    let route = |rig: &mut BatchRig, levels: &[InstanceLevel], culling| {
        let options = InstanceBatchOptions {
            culling,
            ..Default::default()
        };
        let batch = rig.batch_levels(levels, &transforms, &[], options);
        let route = rig.renderer.batch_route(batch.id).unwrap();
        rig.free(batch);
        route
    };
    assert_eq!(
        route(&mut rig, &levels, BatchCulling::Auto),
        Route::CpuSmall,
        "three levels: the CPU path"
    );
    // The controls: one of the levels alone goes to the GPU, and asking
    // for the GPU still sends the level batch there.
    assert_eq!(
        route(&mut rig, &levels[..1], BatchCulling::Auto),
        Route::Gpu,
        "one level: the crossover"
    );
    assert_eq!(
        route(&mut rig, &levels, BatchCulling::PreferGpu),
        Route::Gpu,
        "the game's wish"
    );
}
