//! A plaza of 10,000 animated people in one crowd:
//! `cargo run --example crowd --features 3d`. Escape quits.
//!
//! Every person is a skinned character built in code (five boxes on four
//! joints) playing a baked clip: most idle, and every two seconds another
//! section of the plaza cheers. Near people draw a detailed mesh, far ones
//! a plain one; all of them cast sun shadows. The whole crowd is a handful
//! of draws, and its clips advance on the GPU: the game only updates the
//! crowd when a section changes what it plays.
use ponko::anim::{AnimationClip, AnimationTrack, Interpolation, Joint, JointAnimation, Skeleton};
use ponko::d3::Transform;
use ponko::prelude::*;
use ponko::render::{
    Crowd, CrowdAnimation, CrowdClip, CrowdInstance, CrowdLevel, CrowdOptions, MaterialDesc, MeshData, Model,
};

/// People along each side of the plaza.
pub const SIDE: usize = 100;
/// People in the crowd.
pub const PEOPLE: usize = SIDE * SIDE;
/// Metres between neighbours.
const SPACING: f32 = 1.5;
/// Rows of people in a section that cheers together.
const SECTION_ROWS: usize = 20;
/// The crowd's clips, in the order they are baked.
const IDLE: u16 = 0;
const CHEER: u16 = 1;

/// The crowd and what each person plays.
pub struct Plaza {
    pub crowd: Crowd,
    pub people: Vec<CrowdInstance>,
    /// The section cheering now.
    pub section: usize,
    /// Updates the crowd took.
    pub updates: u32,
}

// ANCHOR: crowd_figure
/// A person 1.85 m tall: legs on the hips, a torso and head on the spine,
/// an arm on each shoulder. Every face is cut into `cuts` x `cuts` quads.
fn person(cuts: u32) -> (Skeleton, MeshData) {
    let skeleton = Skeleton::new(vec![
        Joint::new("hips", None),
        Joint::new("spine", 0).with_rest(Transform::at(vec3(0.0, 0.9, 0.0))),
        Joint::new("arm_l", 1).with_rest(Transform::at(vec3(-0.31, 0.55, 0.0))),
        Joint::new("arm_r", 1).with_rest(Transform::at(vec3(0.31, 0.55, 0.0))),
    ]);
    let mut mesh = MeshData::default();
    let parts = [
        (vec3(-0.2, 0.0, -0.12), vec3(0.2, 0.9, 0.12), 0),
        (vec3(-0.25, 0.9, -0.15), vec3(0.25, 1.5, 0.15), 1),
        (vec3(-0.13, 1.55, -0.13), vec3(0.13, 1.85, 0.13), 1),
        (vec3(-0.37, 0.85, -0.07), vec3(-0.25, 1.48, 0.07), 2),
        (vec3(0.25, 0.85, -0.07), vec3(0.37, 1.48, 0.07), 3),
    ];
    for (min, max, joint) in parts {
        add_box(&mut mesh, min, max, joint, cuts);
    }
    (skeleton, mesh)
}
// ANCHOR_END: crowd_figure

/// A box from `min` to `max` moved by `joint` alone.
fn add_box(mesh: &mut MeshData, min: Vec3, max: Vec3, joint: u16, cuts: u32) {
    let faces = [
        (Vec3::X, Vec3::Y, Vec3::Z),
        (Vec3::NEG_X, Vec3::Y, Vec3::NEG_Z),
        (Vec3::Y, Vec3::Z, Vec3::X),
        (Vec3::NEG_Y, Vec3::NEG_Z, Vec3::X),
        (Vec3::Z, Vec3::NEG_Y, Vec3::NEG_X),
        (Vec3::NEG_Z, Vec3::Y, Vec3::NEG_X),
    ];
    let (center, half, row) = ((min + max) * 0.5, (max - min) * 0.5, cuts + 1);
    for (normal, u, v) in faces {
        let base = mesh.positions.len() as u32;
        for j in 0..row {
            for i in 0..row {
                let a = i as f32 / cuts as f32 * 2.0 - 1.0;
                let b = j as f32 / cuts as f32 * 2.0 - 1.0;
                mesh.positions.push(center + (normal + u * a + v * b) * half);
                mesh.normals.push(normal);
                mesh.joints.push([joint, 0, 0, 0]);
                mesh.weights.push([1.0, 0.0, 0.0, 0.0]);
            }
        }
        let quad = if u.cross(v).dot(normal) > 0.0 {
            [0, 1, 2, 0, 2, 3]
        } else {
            [0, 2, 1, 0, 3, 2]
        };
        for j in 0..cuts {
            for i in 0..cuts {
                let at = base + j * row + i;
                let corners = [at, at + 1, at + row + 1, at + row];
                mesh.indices.extend(quad.map(|k| corners[k]));
            }
        }
    }
}

/// A clip turning each `(joint, axis)` through `angles` at `times`.
fn clip(skeleton: &Skeleton, name: &str, times: &[f32], joints: &[(usize, Vec3, &[f32])]) -> AnimationClip {
    let tracks = joints
        .iter()
        .map(|&(joint, axis, angles)| {
            let rotations = angles
                .iter()
                .map(|&a| Vec4::from_array(Quat::from_axis_angle(axis, a).to_array()))
                .collect();
            JointAnimation::new(joint, skeleton.joints()[joint].name.clone()).with_rotation(AnimationTrack::new(
                times.to_vec(),
                rotations,
                Interpolation::Linear,
            ))
        })
        .collect();
    AnimationClip::new(name, skeleton, tracks)
}

// ANCHOR: crowd_bake
/// The clips the whole crowd plays, baked once: idling (a slow sway, arms
/// swinging) and cheering (arms up, bouncing).
fn bake(skeleton: &Skeleton) -> CrowdAnimation {
    let idle = clip(
        skeleton,
        "idle",
        &[0.0, 1.0, 2.0],
        &[
            (1, Vec3::Z, &[-0.06, 0.06, -0.06]),
            (2, Vec3::X, &[0.15, -0.15, 0.15]),
            (3, Vec3::X, &[-0.15, 0.15, -0.15]),
        ],
    );
    let cheer = clip(
        skeleton,
        "cheer",
        &[0.0, 0.3, 0.6],
        &[
            (1, Vec3::X, &[0.0, -0.12, 0.0]),
            (2, Vec3::Z, &[-2.5, -2.9, -2.5]),
            (3, Vec3::Z, &[2.5, 2.9, 2.5]),
        ],
    );
    CrowdAnimation::bake(skeleton, &[CrowdClip::looping(idle), CrowdClip::looping(cheer)], 30.0)
}
// ANCHOR_END: crowd_bake

/// What person `i` plays while section `section` cheers.
fn plays(i: usize, section: usize) -> CrowdInstance {
    let (x, z) = ((i % SIDE) as f32, (i / SIDE) as f32);
    let at = Mat4::from_translation(vec3(
        (x - SIDE as f32 / 2.0) * SPACING,
        0.0,
        (z - SIDE as f32 / 2.0) * SPACING,
    ));
    let tints = [0xD9734E, 0x4E8FD9, 0xE8C547, 0x6BBF59, 0xB565C9, 0xE0E0E0];
    let tint = Color::hex(tints[(i * 7 + i / SIDE) % tints.len()]);
    // A start of its own, so neighbours don't move as one.
    let start = (i * 37 % 100) as f32 * 0.02;
    let person = CrowdInstance::new(at).with_color(tint);
    if i / SIDE / SECTION_ROWS == section {
        // Cheering, with a quarter of the idle mixed in.
        person.playing(CHEER, start).blending(IDLE, start, 0.25)
    } else {
        person.playing(IDLE, start)
    }
}

// ANCHOR: crowd_create
fn setup(ctx: &mut Ctx) {
    ctx.scene3d.camera.position = vec3(0.0, 22.0, 95.0);
    ctx.scene3d.camera.look_at(vec3(0.0, 0.0, 30.0));
    ctx.scene3d.camera.far = 400.0;
    ctx.scene3d.lighting.shadows = true;
    let stone = ctx.gfx.create_material(MaterialDesc::color(Color::hex(0x9A9488)));
    let ground = ctx.gfx.create_mesh(&MeshData::plane(400.0), stone);
    ctx.world.spawn((Transform::default(), Model::new(ground)));

    let cloth = ctx.gfx.create_material(MaterialDesc::color(Color::WHITE));
    let (skeleton, near) = person(4);
    let near = ctx.gfx.create_skinned_mesh(&near, cloth, &skeleton);
    let far = ctx.gfx.create_skinned_mesh(&person(1).1, cloth, &skeleton);
    let animation = bake(&skeleton);
    let levels = [
        // Up to 40 m: 960 triangles a person, crossfading into the far
        // mesh over its last 4 m.
        CrowdLevel::new(near, 0.0, 40.0).with_fade(4.0),
        // Beyond: 60 triangles, and two joints a corner are plenty.
        CrowdLevel::new(far, 40.0, f32::INFINITY).with_influences(2),
    ];
    let people: Vec<CrowdInstance> = (0..PEOPLE).map(|i| plays(i, 0)).collect();
    let crowd = ctx
        .gfx
        .create_crowd(&animation, &levels, &people, CrowdOptions::default());
    ctx.world.insert_resource(Plaza {
        crowd,
        people,
        section: 0,
        updates: 0,
    });
}
// ANCHOR_END: crowd_create

// ANCHOR: crowd_update
/// Every two seconds the next section cheers: one update of the crowd.
/// Between updates nothing is sent; the clips play on by themselves.
fn sections(ctx: &mut Ctx) {
    if ctx.time.sim_tick() % 120 != 119 {
        return;
    }
    let plaza = ctx.world.resource_mut::<Plaza>();
    plaza.section = (plaza.section + 1) % (SIDE / SECTION_ROWS);
    for (i, person) in plaza.people.iter_mut().enumerate() {
        *person = plays(i, plaza.section);
    }
    if ctx.gfx.update_crowd(plaza.crowd, &plaza.people) {
        plaza.updates += 1;
    }
}
// ANCHOR_END: crowd_update

/// Slowly circles the plaza.
fn orbit(ctx: &mut Ctx) {
    let angle = ctx.time.elapsed() as f32 * 0.05;
    ctx.scene3d.camera.position = vec3(angle.sin() * 95.0, 22.0, angle.cos() * 95.0);
    let ahead = vec3(angle.sin() * 30.0, 0.0, angle.cos() * 30.0);
    ctx.scene3d.camera.look_at(ahead);
    if ctx.input.key_pressed(Key::Escape) {
        ctx.quit();
    }
}

pub fn game() -> ConfiguredGame {
    Game::new("Ponko: crowd", 1280, 720)
        .sim_system(sections)
        .system(orbit)
        .setup(setup)
}

fn main() {
    game().run();
}
