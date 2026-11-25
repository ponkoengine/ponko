//! 2D skeletal animation: a puppet with rigid parts and a skinned tail,
//! walking and running through a 1D blend, jumping from a trigger,
//! looking at the mouse and reaching for it with two-bone IK.
//!
//!   `cargo run --example skeleton2d`
//!
//! - Left / Right: slower / faster (the walk-run blend's `speed`).
//! - Space: jump (a trigger; the jump plays once, then the graph returns).
//! - Hold the left mouse button: the front arm reaches for the mouse.
//! - Escape: quit.
//!
//! The puppet is built in code with generated textures; a game would
//! usually load the same thing from a file with `ctx.load_skeleton2d`
//! (the guide's "2D skeletons" chapter has the format).

use std::sync::Arc;

use ponko::anim::{AnimationCondition, AnimationParameter, AnimationTransition, ParamId, StateId};
use ponko::d2::Transform;
use ponko::prelude::*;
use ponko::skeleton2d::{
    BonePose, BoneTrack, Clip, IkConstraint, Key as Frame, MeshVertex, Skeleton, SkeletonData, SkeletonGraph,
    SkeletonMotion, SkeletonState, SkinnedMesh, SpritePart,
};

/// Where the puppet stands.
const HOME: Vec2 = vec2(400.0, 380.0);

/// The puppet and what the screen shows about it.
struct Puppet {
    entity: Entity,
    /// The state machine's parameters, resolved once in setup.
    speed_parameter: ParamId,
    jump_trigger: ParamId,
    speed: f32,
    last_event: String,
}

fn main() {
    Game::new("Ponko: 2D skeletons", 800, 600)
        .setup(setup)
        .system(controls)
        .run();
}

/// A `w` x `h` texture: a rounded box of `color` with a darker rim.
fn texture(ctx: &mut Ctx, w: u32, h: u32, color: Color) -> Texture {
    let mut pixels = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let edge = x.min(y).min(w - 1 - x).min(h - 1 - y);
            let shade = if edge == 0 { 0.6 } else { 1.0 };
            let byte = |c: f32| (c * shade * 255.0) as u8;
            pixels.extend_from_slice(&[byte(color.r), byte(color.g), byte(color.b), 255]);
        }
    }
    ctx.gfx.create_texture(w, h, &pixels)
}

/// Rotation keys for `bone`, in degrees, evenly over `duration`.
fn swing(bone: usize, duration: f32, degrees: &[f32]) -> BoneTrack {
    let mut track = BoneTrack::new(bone);
    let step = duration / (degrees.len() - 1) as f32;
    track.rotate = degrees
        .iter()
        .enumerate()
        .map(|(i, d)| Frame::new(i as f32 * step, d.to_radians()))
        .collect();
    track
}

// ANCHOR: skeleton2d
fn puppet(ctx: &mut Ctx) -> SkeletonData {
    let body = texture(ctx, 16, 16, Color::rgb(0.95, 0.55, 0.25));
    let limb = texture(ctx, 8, 8, Color::rgb(0.85, 0.40, 0.20));
    let head = texture(ctx, 16, 16, Color::rgb(1.0, 0.75, 0.45));
    let tail = texture(ctx, 16, 4, Color::rgb(0.9, 0.5, 0.3));

    let mut data = SkeletonData::new();
    // Bones: y is down, rotations are clockwise radians, children follow
    // their parents.
    let root = data.add_bone("root", None, BonePose::IDENTITY, 0.0);
    let hip = data.add_bone("hip", Some(root), BonePose::at(vec2(0.0, -40.0)), 0.0);
    let torso = data.add_bone("torso", Some(hip), BonePose::IDENTITY.with_rotation(-1.57), 40.0);
    let neck = data.add_bone("neck", Some(torso), BonePose::at(vec2(40.0, 0.0)), 20.0);
    let arm = data.add_bone(
        "arm",
        Some(torso),
        BonePose::at(vec2(34.0, 0.0)).with_rotation(2.6),
        22.0,
    );
    let hand = data.add_bone(
        "hand",
        Some(arm),
        BonePose::at(vec2(22.0, 0.0)).with_rotation(-0.6),
        20.0,
    );
    let mut legs = Vec::new();
    for (name, x) in [("leg-back", -6.0), ("leg-front", 6.0)] {
        let thigh = data.add_bone(name, Some(hip), BonePose::at(vec2(x, 0.0)).with_rotation(1.57), 20.0);
        data.add_bone(
            &format!("{name}-shin"),
            Some(thigh),
            BonePose::at(vec2(20.0, 0.0)),
            20.0,
        );
        legs.push(thigh);
    }
    let tail_base = data.add_bone(
        "tail",
        Some(hip),
        BonePose::at(vec2(-8.0, 0.0)).with_rotation(2.8),
        24.0,
    );
    let tail_tip = data.add_bone("tail-tip", Some(tail_base), BonePose::at(vec2(24.0, 0.0)), 24.0);
    let gaze = data.add_bone("gaze", Some(root), BonePose::at(vec2(80.0, -90.0)), 0.0);
    let reach = data.add_bone("reach", Some(root), BonePose::at(vec2(40.0, -40.0)), 0.0);

    // Slots draw in the order they are added: the tail behind, then the
    // back leg, the body, the front leg, the arm and the head.
    let part = |texture: Texture, size: Vec2| SpritePart::new(texture).with_size(size).with_pivot(vec2(0.0, 0.5));
    let slot = data.add_slot("tail", tail_base);
    let mut vertices = Vec::new();
    for i in 0..=4 {
        let t = i as f32 / 4.0;
        for y in [-5.0 * (1.0 - t) - 1.0, 5.0 * (1.0 - t) + 1.0] {
            let at = Vec2::new(48.0 * t, y);
            let uv = vec2(t, if y < 0.0 { 0.0 } else { 1.0 });
            // Each vertex follows the base and the tip by how far along it is.
            vertices.push(MeshVertex::new(at, uv, &[(tail_base, 1.0 - t), (tail_tip, t)]));
        }
    }
    let triangles = (0..4u16).flat_map(|i| [2 * i, 2 * i + 2, 2 * i + 1, 2 * i + 1, 2 * i + 2, 2 * i + 3]);
    data.add_attachment(slot, "tail", SkinnedMesh::new(tail, vertices, triangles.collect()));
    for (i, &thigh) in legs.iter().enumerate() {
        let back = data.add_slot(&format!("thigh-{i}"), thigh);
        data.add_attachment(back, "thigh", part(limb, vec2(22.0, 8.0)));
        let shin = data.add_slot(&format!("shin-{i}"), thigh + 1);
        data.add_attachment(shin, "shin", part(limb, vec2(22.0, 7.0)));
        if i == 0 {
            let slot = data.add_slot("body", torso);
            data.add_attachment(slot, "body", part(body, vec2(44.0, 28.0)));
        }
    }
    let slot = data.add_slot("arm", arm);
    data.add_attachment(slot, "arm", part(limb, vec2(24.0, 7.0)));
    let slot = data.add_slot("hand", hand);
    data.add_attachment(slot, "hand", part(limb, vec2(20.0, 6.0)));
    let slot = data.add_slot("head", neck);
    data.add_attachment(slot, "head", part(head, vec2(26.0, 24.0)));
    // A sprite swap: the jump closes the eyes (a darker head).
    let blink = texture(ctx, 16, 16, Color::rgb(0.8, 0.55, 0.35));
    data.add_attachment(slot, "head-blink", part(blink, vec2(26.0, 24.0)));

    // IK: the head looks at `gaze`, the arm reaches for `reach`. The game
    // moves their targets with `set_ik_target`.
    data.add_ik(IkConstraint::new("look", &[neck], gaze).with_mix(0.8));
    data.add_ik(
        IkConstraint::new("reach", &[arm, hand], reach)
            .bend_negative()
            .with_mix(0.0),
    );

    // Clips: offsets from the rest pose.
    let legs_cycle = |clip: Clip, amount: f32, duration: f32| {
        clip.with_bone_track(swing(legs[0], duration, &[amount, -amount, amount]))
            .with_bone_track(swing(legs[1], duration, &[-amount, amount, -amount]))
            .with_bone_track(swing(legs[0] + 1, duration, &[0.0, amount, 0.0]))
            .with_bone_track(swing(legs[1] + 1, duration, &[amount, 0.0, amount]))
            .with_bone_track(swing(tail_tip, duration, &[-15.0, 15.0, -15.0]))
            .with_event(0.0, "step")
            .with_event(duration / 2.0, "step")
    };
    let walk = data.add_clip(legs_cycle(Clip::new("walk", 1.0), 25.0, 1.0));
    let mut run = legs_cycle(Clip::new("run", 0.5), 45.0, 0.5).with_bone_track(swing(torso, 0.5, &[10.0, 15.0, 10.0]));
    let mut bob = BoneTrack::new(hip);
    bob.translate = vec![
        Frame::new(0.0, Vec2::ZERO),
        Frame::new(0.125, vec2(0.0, -6.0)),
        Frame::new(0.25, Vec2::ZERO),
        Frame::new(0.375, vec2(0.0, -6.0)),
        Frame::new(0.5, Vec2::ZERO),
    ];
    run = run.with_bone_track(bob);
    let run = data.add_clip(run);
    let mut jump = Clip::new("jump", 0.6)
        .with_bone_track(swing(legs[0], 0.6, &[0.0, -50.0, 0.0]))
        .with_bone_track(swing(legs[1], 0.6, &[0.0, -40.0, 0.0]))
        .with_event(0.3, "land");
    let mut up = BoneTrack::new(root);
    up.translate = vec![
        Frame::new(0.0, Vec2::ZERO),
        Frame::new(0.3, vec2(0.0, -60.0)),
        Frame::new(0.6, Vec2::ZERO),
    ];
    let mut eyes = ponko::skeleton2d::SlotTrack::new(slot);
    eyes.attachment = vec![
        Frame::new(0.1, Some("head-blink".into())),
        Frame::new(0.5, Some("head".into())),
    ];
    jump = jump.with_bone_track(up).with_slot_track(eyes);
    let jump = data.add_clip(jump);

    // The state machine: the shared parameters, conditions and transitions.
    let graph = SkeletonGraph::new(
        vec![
            ("speed", AnimationParameter::Float(0.0)),
            ("jump", AnimationParameter::Trigger(false)),
        ],
        vec![
            SkeletonState::new(
                "move",
                SkeletonMotion::Blend1D {
                    parameter: ParamId(0),
                    children: vec![(0.0, walk), (1.0, run)],
                },
            ),
            SkeletonState::new("jump", SkeletonMotion::Clip(jump)).once(),
        ],
        vec![
            AnimationTransition::new(StateId(0), StateId(1))
                .with_condition(AnimationCondition::Trigger(ParamId(1)))
                .with_duration(0.1),
            AnimationTransition::new(StateId(1), StateId(0))
                .with_exit_time(1.0)
                .with_duration(0.2),
        ],
        StateId(0),
    )
    .expect("a valid graph");
    data.set_graph(graph).expect("its clips exist");
    data
}
// ANCHOR_END: skeleton2d

fn setup(ctx: &mut Ctx) {
    ctx.gfx.clear(Color::rgb(0.16, 0.18, 0.22));
    let data = Arc::new(puppet(ctx));
    let skeleton = Skeleton::new(data);
    // A misspelt name stops here, listing the parameters the graph has.
    let resolve = |name: &str| skeleton.try_parameter_id(name).unwrap_or_else(|e| panic!("{e}"));
    let (speed_parameter, jump_trigger) = (resolve("speed"), resolve("jump"));
    let entity = ctx.world.spawn((Transform::at(HOME), skeleton));
    ctx.world.insert_resource(Puppet {
        entity,
        speed_parameter,
        jump_trigger,
        speed: 0.0,
        last_event: String::new(),
    });
}

fn controls(ctx: &mut Ctx) {
    if ctx.input.key_pressed(Key::Escape) {
        ctx.quit();
    }
    let mouse = ctx.camera.screen_to_world(ctx.input.mouse_pos());
    let dt = ctx.time.dt();
    let mut puppet = ctx.world.remove_resource::<Puppet>().expect("made in setup");
    if ctx.input.key_down(Key::Right) {
        puppet.speed = (puppet.speed + dt).min(1.0);
    }
    if ctx.input.key_down(Key::Left) {
        puppet.speed = (puppet.speed - dt).max(0.0);
    }
    let reaching = ctx.input.mouse_down(MouseButton::Left);
    let jump = ctx.input.key_pressed(Key::Space);
    if let Some(s) = ctx.world.get_mut::<Skeleton>(puppet.entity) {
        s.set_float(puppet.speed_parameter, puppet.speed);
        if jump {
            s.set_trigger(puppet.jump_trigger, true);
        }
        // IK targets are in the skeleton's space: the entity's position is
        // its origin here (no rotation or scale).
        s.set_ik_target(0, Some(mouse - HOME));
        s.set_ik_target(1, Some(mouse - HOME));
        s.set_ik_mix(1, if reaching { 1.0 } else { 0.0 });
        if let Some(event) = s.drain_events().last() {
            puppet.last_event = event.to_string();
        }
    }
    ctx.gfx.draw_text(
        &format!(
            "speed {:.2} (Left/Right)   Space: jump   hold mouse: reach   last event: {}",
            puppet.speed, puppet.last_event
        ),
        vec2(16.0, 24.0),
        16.0,
        Color::WHITE,
    );
    ctx.world.insert_resource(puppet);
}
