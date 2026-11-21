use super::*;
#[cfg(feature = "physics3d")]
use glam::{Vec3Swizzles, vec3};

fn draw() -> DebugDraw {
    DebugDraw::new(true)
}

fn visible_lines(d: &DebugDraw) -> Vec<DebugLine> {
    d.visible().iter().flat_map(|b| b.lines.iter().copied()).collect()
}

#[test]
fn a_box_is_twelve_edges_and_a_rectangle_four() {
    let mut d = draw();
    d.aabb(Vec3::ZERO, Vec3::ONE, Color::GREEN);
    d.aabb(Vec2::ZERO, Vec2::ONE, Color::GREEN);
    let s = d.stats();
    assert_eq!((s.lines_3d, s.lines_2d), (12, 4));
    // Every edge of the unit box is one unit long.
    for l in visible_lines(&d) {
        assert!(((l.b - l.a).length() - 1.0).abs() < 1e-5, "{l:?}");
    }
}

#[test]
fn shapes_have_their_line_counts() {
    let count = |f: &dyn Fn(&mut DebugDraw)| {
        let mut d = draw();
        f(&mut d);
        let s = d.stats();
        s.lines_2d + s.lines_3d
    };
    assert_eq!(
        count(&|d| {
            d.line(Vec3::ZERO, Vec3::X, Color::RED);
        }),
        1
    );
    assert_eq!(
        count(&|d| {
            d.ray(Vec2::ZERO, Vec2::X, Color::RED);
        }),
        1
    );
    assert_eq!(
        count(&|d| {
            d.arrow(Vec2::ZERO, Vec2::X, Color::RED);
        }),
        3
    );
    assert_eq!(
        count(&|d| {
            d.arrow(Vec3::ZERO, Vec3::X, Color::RED);
        }),
        5
    );
    assert_eq!(
        count(&|d| {
            d.circle(Vec2::ZERO, 1.0, Color::RED);
        }),
        CIRCLE_SEGMENTS
    );
    assert_eq!(
        count(&|d| {
            d.sphere(Vec3::ZERO, 1.0, Color::RED);
        }),
        3 * CIRCLE_SEGMENTS
    );
    // Two sides and two half circles in 2D.
    assert_eq!(
        count(&|d| {
            d.capsule(Vec2::ZERO, Vec2::Y, 0.5, Color::RED);
        }),
        2 + CIRCLE_SEGMENTS
    );
    // Two rings, four sides, four half circles in 3D.
    assert_eq!(
        count(&|d| {
            d.capsule(Vec3::ZERO, Vec3::Y, 0.5, Color::RED);
        }),
        2 * CIRCLE_SEGMENTS + 4 + 2 * CIRCLE_SEGMENTS
    );
    // 4 cells: 5 lines each way.
    assert_eq!(
        count(&|d| {
            d.grid(Vec3::ZERO, 1.0, 4, Color::RED);
        }),
        10
    );
    assert_eq!(
        count(&|d| {
            d.grid(Vec3::ZERO, 1.0, 0, Color::RED);
        }),
        0
    );
    assert_eq!(
        count(&|d| {
            d.polyline(&[Vec2::ZERO, Vec2::X, Vec2::Y], true, Color::RED);
        }),
        3
    );
    assert_eq!(
        count(&|d| {
            d.polyline(&[Vec2::ZERO, Vec2::X, Vec2::Y], false, Color::RED);
        }),
        2
    );
    assert_eq!(
        count(&|d| {
            d.axes(Vec3::ZERO, Quat::IDENTITY, 1.0);
        }),
        3
    );
}

#[test]
fn a_capsule_reaches_radius_past_its_ends() {
    let mut d = draw();
    d.capsule(Vec3::ZERO, Vec3::Y * 2.0, 0.5, Color::RED);
    let lines = visible_lines(&d);
    let top = lines.iter().flat_map(|l| [l.a.y, l.b.y]).fold(f32::MIN, f32::max);
    let bottom = lines.iter().flat_map(|l| [l.a.y, l.b.y]).fold(f32::MAX, f32::min);
    assert!(
        (top - 2.5).abs() < 1e-4 && (bottom + 0.5).abs() < 1e-4,
        "{bottom}..{top}"
    );
}

#[cfg(feature = "3d")]
#[test]
fn a_frustum_is_twelve_edges_even_with_an_infinite_far_plane() {
    let mut camera = crate::d3::Camera::new(Vec2::new(800.0, 600.0));
    camera.far = f32::INFINITY;
    let mut d = draw();
    d.frustum(&camera, Color::WHITE);
    let lines = visible_lines(&d);
    assert_eq!(lines.len(), 12);
    let farthest = lines
        .iter()
        .flat_map(|l| [l.a, l.b])
        .map(|p| p.distance(camera.position))
        .fold(0.0, f32::max);
    assert!(farthest.is_finite() && farthest > INFINITE_FAR * 0.9, "{farthest}");
}

#[test]
fn drawing_while_disabled_records_nothing() {
    let mut d = DebugDraw::new(false);
    d.line(Vec3::ZERO, Vec3::X, Color::RED).seconds(5.0).on_top().width(3.0);
    d.text(Vec2::ZERO, "hi", Color::WHITE).size(20.0);
    assert_eq!(d.stats(), DebugStats::default());
}

#[test]
fn turning_drawing_off_drops_what_was_drawn_but_keeps_the_views() {
    let mut d = draw();
    d.show(DebugView::Bounds, true);
    d.sphere(Vec3::ZERO, 1.0, Color::RED).seconds(10.0);
    d.set_enabled(false);
    assert_eq!(d.stats(), DebugStats::default());
    d.set_enabled(true);
    assert_eq!(d.stats(), DebugStats::default());
    assert!(d.showing(DebugView::Bounds));
}

#[test]
fn frame_shapes_last_one_frame() {
    let mut d = draw();
    d.begin_frame(0.0);
    d.line(Vec2::ZERO, Vec2::X, Color::RED);
    assert_eq!(d.stats().lines_2d, 1);
    d.begin_frame(0.016);
    assert_eq!(d.stats().lines_2d, 0);
}

#[test]
fn tick_shapes_stay_until_the_next_tick_ends() {
    let mut d = draw();
    d.begin_frame(0.0);
    d.set_in_sim(true);
    d.line(Vec3::ZERO, Vec3::X, Color::RED);
    d.end_tick();
    d.set_in_sim(false);
    assert_eq!(d.stats().lines_3d, 1, "drawn after its tick");
    // Frames without a tick still show it.
    d.begin_frame(0.016);
    d.begin_frame(0.032);
    assert_eq!(d.stats().lines_3d, 1);
    // A tick that draws something else replaces it.
    d.set_in_sim(true);
    d.circle(Vec3::ZERO, 1.0, Color::RED);
    d.end_tick();
    d.set_in_sim(false);
    assert_eq!(d.stats().lines_3d, CIRCLE_SEGMENTS);
    // A tick that draws nothing clears it.
    d.set_in_sim(true);
    d.end_tick();
    d.set_in_sim(false);
    assert_eq!(d.stats().lines_3d, 0);
}

#[test]
fn timed_shapes_go_after_their_seconds_whoever_drew_them() {
    let mut d = draw();
    d.begin_frame(1.0);
    d.line(Vec2::ZERO, Vec2::X, Color::RED).seconds(0.5);
    d.set_in_sim(true);
    d.line(Vec3::ZERO, Vec3::X, Color::RED).seconds(2.0);
    d.end_tick();
    d.end_tick(); // further ticks don't take it
    d.set_in_sim(false);
    d.begin_frame(1.4);
    assert_eq!((d.stats().lines_2d, d.stats().lines_3d), (1, 1));
    d.begin_frame(1.6);
    assert_eq!((d.stats().lines_2d, d.stats().lines_3d), (0, 1));
    d.begin_frame(3.1);
    assert_eq!(d.stats(), DebugStats::default());
}

#[test]
fn seconds_of_zero_or_nan_keep_the_usual_lifetime() {
    let mut d = draw();
    d.begin_frame(0.0);
    d.line(Vec2::ZERO, Vec2::X, Color::RED).seconds(0.0);
    d.line(Vec2::ZERO, Vec2::X, Color::RED).seconds(f32::NAN);
    d.begin_frame(0.016);
    assert_eq!(d.stats().lines_2d, 0);
}

#[test]
fn timed_labels_keep_their_text() {
    let mut d = draw();
    d.begin_frame(0.0);
    d.text(Vec2::ZERO, "frame", Color::WHITE);
    d.text(Vec2::ONE, "timed", Color::WHITE).seconds(1.0).size(30.0);
    d.text(Vec2::ONE, "after", Color::WHITE);
    let timed = &d.buf.timed;
    assert_eq!(&timed.text[timed.labels[0].text.clone()], "timed");
    assert_eq!(timed.labels[0].size, 30.0);
    let frame = &d.buf.frame;
    let texts: Vec<_> = frame.labels.iter().map(|l| &frame.text[l.text.clone()]).collect();
    assert_eq!(texts, ["frame", "after"]);
    d.begin_frame(0.5);
    assert_eq!(d.stats().labels, 1);
}

#[test]
fn shape_settings_change_only_the_shape_just_drawn() {
    let mut d = draw();
    d.line(Vec3::ZERO, Vec3::X, Color::RED);
    d.sphere(Vec3::ZERO, 1.0, Color::RED).on_top().width(3.0);
    let lines = visible_lines(&d);
    assert_eq!(lines[0].flags & ON_TOP, 0);
    assert_eq!(lines[0].width, 1.0);
    assert!(lines[1..].iter().all(|l| l.flags & ON_TOP != 0 && l.width == 3.0));
}

#[test]
fn screen_space_applies_to_2d_shapes_only() {
    let mut d = draw();
    d.line(Vec2::ZERO, Vec2::X, Color::RED).screen();
    d.line(Vec3::ZERO, Vec3::X, Color::RED).screen();
    d.line(Vec2::ZERO, Vec2::X, Color::RED).screen().space(DrawSpace::World);
    let lines = visible_lines(&d);
    assert_ne!(lines[0].flags & SCREEN, 0);
    assert_eq!(lines[1].flags & SCREEN, 0);
    assert_eq!(lines[2].flags & SCREEN, 0);
}

#[test]
fn draw_space_is_one_type_for_sprites_and_debug_drawing() {
    // The prelude's and the debug path name the same type: a value of
    // one is a value of the other, with no conversion.
    let space: crate::prelude::DrawSpace = crate::debug::DrawSpace::Screen;
    let same: crate::prelude::DrawSpace = space;
    assert_eq!(
        std::any::TypeId::of::<crate::prelude::DrawSpace>(),
        std::any::TypeId::of::<crate::debug::DrawSpace>()
    );
    let params = crate::texture::DrawParams::default().with_space(same);
    assert!(params.screen);
    let mut d = draw();
    d.line(Vec2::ZERO, Vec2::X, Color::RED).space(space);
    assert_ne!(visible_lines(&d)[0].flags & SCREEN, 0);
}

fn labels(d: &DebugDraw) -> Vec<DebugLabel> {
    d.visible().iter().flat_map(|b| b.labels.iter().cloned()).collect()
}

#[test]
fn depth_tested_applies_to_3d_labels_and_the_last_of_it_and_on_top_wins() {
    let mut d = draw();
    d.text(Vec3::ZERO, "a", Color::WHITE).depth_tested();
    d.text(Vec3::ZERO, "b", Color::WHITE).depth_tested().on_top();
    d.text(Vec3::ZERO, "c", Color::WHITE).on_top().depth_tested();
    d.text(Vec2::ZERO, "d", Color::WHITE).depth_tested();
    d.text(Vec3::ZERO, "e", Color::WHITE);
    d.line(Vec3::ZERO, Vec3::X, Color::RED).depth_tested();
    let l = labels(&d);
    let tested: Vec<bool> = l.iter().map(|l| l.flags & TESTED != 0).collect();
    assert_eq!(tested, [true, false, true, false, false]);
    assert_ne!(l[1].flags & ON_TOP, 0);
    assert_eq!(l[2].flags & ON_TOP, 0);
    assert_eq!(visible_lines(&d)[0].flags & (ON_TOP | TESTED), 0, "lines don't change");
}

#[test]
fn the_collider_distance_defaults_to_fifty_and_is_never_negative() {
    let mut d = draw();
    assert_eq!(d.collider_distance(), 50.0);
    d.set_collider_distance(12.0);
    assert_eq!(d.collider_distance(), 12.0);
    d.set_collider_distance(-1.0);
    assert_eq!(d.collider_distance(), 0.0);
    d.set_collider_distance(f32::NAN);
    assert_eq!(d.collider_distance(), 0.0);
}

#[test]
fn a_view_systems_buffer_draws_like_the_main_one_and_is_added_to_it() {
    let mut main = draw();
    main.begin_frame(2.0);
    main.set_in_sim(true);
    let mut own = DebugDraw::new(false);
    own.follow(&main);
    own.line(Vec3::ZERO, Vec3::X, Color::RED);
    own.text(Vec3::ZERO, "own", Color::RED).seconds(1.0);
    main.absorb(&mut own);
    assert_eq!(own.stats(), DebugStats::default(), "the system's buffer is emptied");
    assert_eq!(
        main.buf.tick_live.lines.len(),
        1,
        "drawn in a tick: lasts like the tick's"
    );
    assert_eq!(main.buf.timed.labels.len(), 1);
    assert_eq!(&main.buf.timed.text[main.buf.timed.labels[0].text.clone()], "own");
    assert_eq!(main.buf.timed.labels[0].until, 3.0);
}

#[test]
fn a_disabled_main_buffer_drops_what_view_systems_drew() {
    let mut main = DebugDraw::new(false);
    let mut own = DebugDraw::new(true);
    own.line(Vec3::ZERO, Vec3::X, Color::RED);
    main.absorb(&mut own);
    assert_eq!(main.stats(), DebugStats::default());
    assert_eq!(own.stats(), DebugStats::default());
}

#[test]
fn redrawing_each_frame_reuses_the_buffers() {
    let mut d = draw();
    let frame = |d: &mut DebugDraw, t: f64| {
        d.begin_frame(t);
        for i in 0..1000 {
            d.line(Vec3::ZERO, Vec3::X * i as f32, Color::RED);
        }
        d.text(Vec2::ZERO, "label", Color::WHITE);
    };
    frame(&mut d, 0.0);
    let (lines, text) = (d.buf.frame.lines.as_ptr(), d.buf.frame.text.as_ptr());
    for t in 1..10 {
        frame(&mut d, t as f64);
    }
    assert_eq!(d.buf.frame.lines.as_ptr(), lines, "the same allocation");
    assert_eq!(d.buf.frame.text.as_ptr(), text);
}

#[test]
fn views_are_named_for_the_control_endpoint() {
    for v in DebugView::ALL {
        assert_eq!(DebugView::from_name(v.name()), Some(v));
    }
    assert_eq!(DebugView::from_name("nope"), None);
    let mut views = builtin::Views::default();
    views.set(DebugView::Lights, true);
    views.set(DebugView::Hierarchy, true);
    views.set(DebugView::Lights, false);
    assert!(!views.get(DebugView::Lights) && views.get(DebugView::Hierarchy));
}

/// What the built-in views draw from `ctx`'s world, as (2D lines, 3D lines).
fn view_lines(ctx: &mut crate::ctx::Ctx, view: DebugView) -> (usize, usize) {
    ctx.debug = DebugDraw::new(true);
    ctx.debug.show(view, true);
    builtin::draw(ctx);
    let s = ctx.debug.stats();
    (s.lines_2d, s.lines_3d)
}

fn ctx() -> crate::ctx::Ctx {
    crate::ctx::Ctx::new(1.0 / 60.0, Vec2::new(800.0, 600.0))
}

#[test]
fn no_view_on_draws_nothing() {
    let mut c = ctx();
    c.world.spawn((crate::d2::Transform::default(),));
    c.debug = DebugDraw::new(true);
    builtin::draw(&mut c);
    assert_eq!(c.debug.stats(), DebugStats::default());
}

#[cfg(feature = "physics")]
#[test]
fn the_collider_view_draws_2d_colliders() {
    let mut c = ctx();
    let t = crate::d2::Transform::default();
    c.world.spawn((t, crate::d2::Collider::circle(10.0)));
    c.world.spawn((t, crate::d2::Collider::rect(Vec2::splat(4.0)).sensor()));
    c.world
        .spawn((t, crate::d2::Collider::polygon(&[Vec2::ZERO, Vec2::X, Vec2::Y])));
    c.world.spawn((t,)); // no collider
    assert_eq!(view_lines(&mut c, DebugView::Colliders).0, CIRCLE_SEGMENTS + 4 + 3);
}

#[cfg(feature = "physics3d")]
#[test]
fn the_collider_view_draws_3d_colliders_and_hull_edges() {
    let mut c = ctx();
    let t = crate::d3::Transform::default();
    c.world.spawn((t, crate::d3::Collider::cuboid(Vec3::ONE)));
    c.world.spawn((t, crate::d3::Collider::sphere(1.0)));
    let hull = c
        .physics3d
        .convex_hull(&[Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z])
        .expect("a hull");
    let at = crate::d3::Transform::at(vec3(5.0, 0.0, 0.0));
    c.world.spawn((at, crate::d3::Collider::mesh(hull)));
    // A tetrahedron is six edges, not its bounding box's twelve.
    assert_eq!(view_lines(&mut c, DebugView::Colliders).1, 12 + 3 * CIRCLE_SEGMENTS + 6);
    let lines = visible_lines(&c.debug);
    let edges = &lines[12 + 3 * CIRCLE_SEGMENTS..];
    let ends = || edges.iter().flat_map(|l| [l.a, l.b]);
    let max = ends().fold(Vec3::MIN, Vec3::max);
    let min = ends().fold(Vec3::MAX, Vec3::min);
    assert!(
        max.abs_diff_eq(vec3(6.0, 1.0, 1.0), 1e-4) && min.abs_diff_eq(vec3(5.0, 0.0, 0.0), 1e-4),
        "the hull's corners, at its entity: {min} {max}"
    );
}

/// A flat square triangle mesh `n` cells of 1 a side, from (0, 0, 0) along +X and +Z.
#[cfg(feature = "physics3d")]
fn flat_grid(c: &mut crate::ctx::Ctx, n: u32) -> crate::d3::CollisionMesh {
    let (mut vertices, mut indices) = (Vec::new(), Vec::new());
    for z in 0..=n {
        for x in 0..=n {
            vertices.push(vec3(x as f32, 0.0, z as f32));
        }
    }
    let row = n + 1;
    for z in 0..n {
        for x in 0..n {
            let i = z * row + x;
            indices.push([i, i + row, i + 1]);
            indices.push([i + 1, i + row, i + row + 1]);
        }
    }
    c.physics3d.trimesh(&vertices, &indices).expect("a mesh")
}

/// The 3D lines the colliders view draws, with the camera at `eye`.
#[cfg(feature = "physics3d")]
fn collider_lines(c: &mut crate::ctx::Ctx, eye: Vec3, distance: f32) -> Vec<DebugLine> {
    c.scene3d.camera.position = eye;
    c.debug = DebugDraw::new(true);
    c.debug.set_collider_distance(distance);
    c.debug.show(DebugView::Colliders, true);
    builtin::draw(c);
    visible_lines(&c.debug)
}

#[cfg(feature = "physics3d")]
#[test]
fn the_collider_view_draws_mesh_triangles_near_the_camera_only() {
    let mut c = ctx();
    let mesh = flat_grid(&mut c, 100);
    // Turned a quarter around Y and moved: the triangles follow the entity.
    let at =
        crate::d3::Transform::at(vec3(-50.0, 0.0, 50.0)).rotated(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2));
    c.world.spawn((at, crate::d3::Collider::mesh(mesh)));
    let eye = vec3(0.0, 2.0, 0.0);
    let near = collider_lines(&mut c, eye, 5.0);
    // 20,000 triangles in all; around the camera, within 5 of it (2 up):
    // a disc of radius about 4.6 on the ground, about 66 cells.
    assert!(near.len() > 3 * 100 && near.len() < 3 * 400, "{}", near.len());
    for l in &near {
        assert!(l.a.y.abs() < 1e-4 && l.b.y.abs() < 1e-4, "on the ground: {l:?}");
        // Each line is an edge of a triangle that comes within 5 of the eye.
        assert!(l.a.distance(eye) < 5.0 + 1.5 && l.b.distance(eye) < 5.0 + 1.5, "{l:?}");
        assert_eq!(l.flags & THREE_D, THREE_D);
    }
    // Twice as far, about four times the triangles.
    let far = collider_lines(&mut c, eye, 10.0);
    assert!(far.len() > 3 * near.len(), "{} vs {}", far.len(), near.len());
    // Far from the mesh, nothing.
    assert!(collider_lines(&mut c, vec3(0.0, 100.0, 0.0), 10.0).is_empty());
    // On the mesh, inside one triangle, at a distance of 0.01: that triangle alone.
    let under = collider_lines(&mut c, vec3(0.2, 0.0, 0.3), 0.01);
    assert_eq!(under.len(), 3);
}

#[cfg(feature = "physics3d")]
#[test]
fn the_collider_view_draws_terrain_triangles_scaled_with_its_tile() {
    let mut c = ctx();
    // 33 x 33 heights over 64 m: cells 2 m across, sloping up along +X.
    let heights: Vec<f32> = (0..33 * 33).map(|i| (i % 33) as f32 * 0.05).collect();
    let ground = c.physics3d.heightfield(&heights, 64.0);
    let corner = vec3(-32.0, -1.0, -32.0);
    let tile = crate::d3::Transform::at(corner);
    c.world.spawn((tile, crate::d3::TerrainCollider::new(ground)));
    let eye = vec3(0.0, 0.0, 0.0);
    let lines = collider_lines(&mut c, eye, 6.0);
    assert!(!lines.is_empty());
    for l in &lines {
        for p in [l.a, l.b] {
            // On the heightfield: height 0.5 per column of 2 m from the corner.
            let column = (p.x - corner.x) / 2.0;
            assert!((p.y - (corner.y + column * 0.05)).abs() < 1e-3, "{p}");
            assert!(p.xz().length() < 6.0 + 3.0, "near the camera: {p}");
        }
    }
    // Scaled twice as wide: the same view reaches the same distance, in
    // half as many (bigger) cells.
    c.world.clear();
    let wide = crate::d3::Transform {
        scale: vec3(2.0, 1.0, 2.0),
        ..crate::d3::Transform::at(vec3(-64.0, -1.0, -64.0))
    };
    c.world.spawn((wide, crate::d3::TerrainCollider::new(ground)));
    let scaled = collider_lines(&mut c, eye, 6.0);
    assert!(
        !scaled.is_empty() && scaled.len() < lines.len(),
        "{} vs {}",
        scaled.len(),
        lines.len()
    );
    let reach = scaled.iter().flat_map(|l| [l.a.x, l.b.x]).fold(f32::MIN, f32::max);
    assert!(reach > 5.0 && reach < 6.0 + 5.0, "{reach}");
    // Mirrored along X from a corner at x = 32: drawn over x from -32 to
    // 32, sloping up towards -X, as its collider is.
    c.world.clear();
    let mirrored = crate::d3::Transform {
        scale: vec3(-1.0, 1.0, 1.0),
        ..crate::d3::Transform::at(vec3(32.0, -1.0, -32.0))
    };
    c.world.spawn((mirrored, crate::d3::TerrainCollider::new(ground)));
    let lines = collider_lines(&mut c, vec3(4.0, 0.0, 0.0), 6.0);
    assert!(!lines.is_empty());
    for l in &lines {
        for p in [l.a, l.b] {
            let column = (32.0 - p.x) / 2.0;
            assert!(p.x <= 32.0 + 1e-3 && (p.y - (-1.0 + column * 0.05)).abs() < 1e-3, "{p}");
        }
    }
}

#[cfg(feature = "navmesh")]
#[test]
fn the_navmesh_view_draws_polygon_edges() {
    let mut c = ctx();
    assert_eq!(view_lines(&mut c, DebugView::NavMesh), (0, 0), "no navmesh: nothing");
    let floor =
        crate::nav::NavSource::from_mesh(&crate::render::MeshData::plane(10.0), crate::d3::Transform::default());
    let nav = crate::d3::NavMesh::bake(&[floor], crate::nav::NavBakeSettings::default()).expect("baked");
    c.world.insert_resource(nav);
    let (_, lines) = view_lines(&mut c, DebugView::NavMesh);
    assert!(lines >= 3, "at least one polygon: {lines}");
    for l in visible_lines(&c.debug) {
        assert!(l.a.x.abs() <= 5.01 && l.a.z.abs() <= 5.01, "on the floor: {l:?}");
    }
}

#[test]
fn the_bounds_view_draws_sprite_rectangles() {
    let mut c = ctx();
    let texture = crate::texture::Texture::new(1, 32, 16);
    let t = crate::d2::Transform {
        scale: Vec2::splat(2.0),
        ..Default::default()
    };
    c.world.spawn((t, crate::d2::Sprite::new(texture)));
    assert_eq!(view_lines(&mut c, DebugView::Bounds).0, 4);
    let lines = visible_lines(&c.debug);
    let max = lines.iter().flat_map(|l| [l.a, l.b]).fold(Vec3::MIN, Vec3::max);
    assert!(max.abs_diff_eq(Vec3::new(32.0, 16.0, 0.0), 1e-4), "{max}");
}

#[cfg(feature = "3d")]
#[test]
fn the_bounds_view_draws_model_spheres() {
    let mut c = ctx();
    let material = c.gfx.default_material();
    let mesh = c.gfx.create_mesh(&crate::render::MeshData::cube(2.0), material);
    c.world
        .spawn((crate::d3::Transform::default(), crate::render::Model::new(mesh)));
    c.world.spawn((
        crate::d3::Transform::default(),
        crate::render::Model::new(crate::render::Mesh::NONE),
    ));
    assert_eq!(view_lines(&mut c, DebugView::Bounds).1, 3 * CIRCLE_SEGMENTS);
}

#[test]
fn the_light_view_draws_2d_light_reach() {
    let mut c = ctx();
    let t = crate::d2::Transform::default();
    c.world.spawn((t, crate::d2::PointLight::default()));
    c.world.spawn((t, crate::d2::SpotLight::default()));
    assert_eq!(
        view_lines(&mut c, DebugView::Lights).0,
        CIRCLE_SEGMENTS + 2 + CIRCLE_SEGMENTS / 2
    );
}

#[cfg(feature = "3d")]
#[test]
fn the_light_view_draws_3d_light_volumes() {
    let mut c = ctx();
    let t = crate::d3::Transform::default();
    c.world.spawn((t, crate::d3::PointLight::default()));
    c.world.spawn((t, crate::d3::SpotLight::default()));
    assert_eq!(
        view_lines(&mut c, DebugView::Lights).1,
        3 * CIRCLE_SEGMENTS + 4 + CIRCLE_SEGMENTS
    );
}

#[test]
fn the_hierarchy_view_draws_axes_and_links_to_parents() {
    let mut c = ctx();
    let parent = c.world.spawn((crate::d2::Transform::default(),));
    c.world.spawn((
        crate::d2::Transform::at(Vec2::new(50.0, 0.0)),
        crate::attach::Attached::to(parent),
    ));
    // Two entities' axes (two lines each) and one link.
    assert_eq!(view_lines(&mut c, DebugView::Hierarchy).0, 2 * 2 + 1);
    #[cfg(feature = "3d")]
    {
        let mut c = ctx();
        let parent = c.world.spawn((crate::d3::Transform::default(),));
        c.world
            .spawn((crate::d3::Transform::default(), crate::attach::Attached3D::to(parent)));
        assert_eq!(view_lines(&mut c, DebugView::Hierarchy).1, 2 * 3 + 1);
    }
}

#[test]
fn zz_print_sizes() {
    println!(
        "SIZES ctx {} debug {} batch {}",
        size_of::<crate::ctx::Ctx>(),
        size_of::<super::DebugDraw>(),
        size_of::<super::Batch>()
    );
}

#[test]
fn debug_lines_draw_in_every_2d_view_on_the_default_layer_and_screen_lines_once() {
    use crate::camera::{RenderLayers, Viewport};
    crate::prelude::Game::new("debug views", 800, 600).run_headless(0, |c| {
        c.camera.rect = Viewport::split(2, 0);
        let mut right = c.camera;
        right.rect = Viewport::split(2, 1);
        right.position = Vec2::new(2000.0, 0.0);
        c.views2d.push(crate::d2::View::new(right));
        let mut map = c.camera;
        map.rect = Viewport::new(0.75, 0.0, 0.25, 0.25);
        map.layers = RenderLayers::layer(1); // the negative control: off layer 0
        c.views2d.push(crate::d2::View::new(map));
        c.place_cameras();
        // What the game left set is put back afterwards.
        c.gfx.set_layers(RenderLayers::layer(3));
        let clip = crate::camera::Rect::new(1.0, 2.0, 3.0, 4.0);
        c.gfx.ui_list.set_clip(Some(clip));
        c.debug = DebugDraw::new(true);
        c.debug.line(Vec2::ZERO, Vec2::X * 10.0, Color::RED);
        c.debug.line(Vec2::ZERO, Vec2::X * 10.0, Color::GREEN).on_top();
        c.debug.line(Vec2::ZERO, Vec2::X * 10.0, Color::BLUE).screen();
        c.hand_over_debug_draw();
        assert_eq!(c.gfx.draw_list.layers(), RenderLayers::layer(3).0);
        assert_eq!(c.gfx.ui_list.clip(), Some(clip));

        // The world line: one sprite on the default layer, for every view
        // that shows it.
        let (sprites, batches) = c.gfx.draw_list.finish();
        assert_eq!((sprites.len(), batches.len()), (1, 1));
        assert_eq!(batches[0].layers, RenderLayers::DEFAULT.0);
        // The line on top: once per camera on layer 0, where that camera sees
        // it, clipped to its rectangle; the screen line once, unclipped.
        let (sprites, batches) = c.gfx.ui_list.finish();
        let green = [0, 255, 0, 255];
        let on_top: Vec<_> = batches
            .iter()
            .filter(|b| sprites[b.start as usize].color == green)
            .map(|b| (b.clip, b.end - b.start, sprites[b.start as usize].pos))
            .collect();
        let (left, right) = (c.camera, c.views2d[0].camera);
        assert_eq!(
            on_top,
            [
                (Some(left.screen_rect()), 1, left.world_to_screen(Vec2::ZERO).to_array()),
                (
                    Some(right.screen_rect()),
                    1,
                    right.world_to_screen(Vec2::ZERO).to_array()
                ),
            ]
        );
        let screen: Vec<_> = batches
            .iter()
            .filter(|b| sprites[b.start as usize].color == [0, 0, 255, 255])
            .map(|b| (b.clip, b.end - b.start))
            .collect();
        assert_eq!(screen, [(None, 1)]);
    });
}
