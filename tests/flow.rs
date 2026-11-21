//! Flow fields: shortest ways to goals over a cost grid.
#[allow(unused_imports)]
use ponko::d2::Transform as Transform2D;
#[allow(unused_imports)]
use ponko::flow::{FlowField, NavGrid};
use ponko::prelude::*;

/// Follows the field from `start` one cell at a time; returns the cells visited.
fn walk(nav: &NavGrid, flow: &FlowField, start: Vec2, max_steps: usize) -> Vec<UVec2> {
    let mut at = start;
    let mut path = vec![nav.cell_at(at).unwrap()];
    for _ in 0..max_steps {
        let d = flow.direction_at(at);
        if d == Vec2::ZERO {
            break;
        }
        // Step to the center of the neighbor cell the direction points at.
        let step = vec2(
            d.x.signum() * (d.x.abs() > 0.1) as i32 as f32,
            d.y.signum() * (d.y.abs() > 0.1) as i32 as f32,
        );
        at = nav.center_of(nav.cell_at(at).unwrap()) + step * nav.cell_size();
        path.push(nav.cell_at(at).unwrap());
    }
    path
}

#[test]
fn agents_follow_the_field_around_walls_to_the_goal() {
    // A wall across the middle with one gap at the far end.
    let mut nav = NavGrid::new(20, 20, 1.0);
    for x in 0..19 {
        nav.set_cost(x, 10, NavGrid::WALL);
    }
    let mut flow = FlowField::new();
    let goal = vec2(2.5, 17.5);
    flow.compute(&nav, &[goal]);

    let path = walk(&nav, &flow, vec2(2.5, 2.5), 100);
    assert_eq!(*path.last().unwrap(), uvec2(2, 17), "reached the goal");
    assert!(
        path.iter().all(|c| nav.cost(c.x, c.y) != Some(NavGrid::WALL)),
        "never stood on a wall"
    );
    assert!(path.contains(&uvec2(19, 10)), "went through the gap");
    assert_eq!(flow.direction_at(goal), Vec2::ZERO);
    assert_eq!(flow.distance_at(goal), Some(0.0));
}

#[test]
fn diagonal_moves_never_squeeze_between_walls() {
    // # .
    // . #   the diagonal from (0, 1) to (1, 0) is blocked by both corners.
    let mut nav = NavGrid::new(2, 2, 1.0);
    nav.set_cost(0, 0, NavGrid::WALL);
    nav.set_cost(1, 1, NavGrid::WALL);
    let mut flow = FlowField::new();
    flow.compute(&nav, &[vec2(1.5, 0.5)]);
    assert_eq!(flow.direction_at(vec2(0.5, 1.5)), Vec2::ZERO);
    assert_eq!(flow.distance_at(vec2(0.5, 1.5)), None);
}

#[test]
fn unreachable_cells_and_walls_have_no_direction() {
    let mut nav = NavGrid::new(10, 10, 1.0);
    // Box in the top-left 3 × 3 corner.
    for i in 0..4 {
        nav.set_cost(3, i, NavGrid::WALL);
        nav.set_cost(i, 3, NavGrid::WALL);
    }
    let mut flow = FlowField::new();
    flow.compute(&nav, &[vec2(8.5, 8.5)]);
    assert_eq!(flow.direction_at(vec2(1.5, 1.5)), Vec2::ZERO);
    assert_eq!(flow.distance_at(vec2(1.5, 1.5)), None);
    assert_eq!(flow.direction_at(vec2(3.5, 0.5)), Vec2::ZERO, "a wall");
    assert!(flow.distance_at(vec2(5.5, 5.5)).is_some());
}

#[test]
fn the_nearest_of_several_goals_wins() {
    let nav = NavGrid::new(30, 1, 1.0);
    let mut flow = FlowField::new();
    flow.compute(&nav, &[vec2(0.5, 0.5), vec2(29.5, 0.5)]);
    assert_eq!(flow.direction_at(vec2(5.5, 0.5)), vec2(-1.0, 0.0));
    assert_eq!(flow.direction_at(vec2(20.5, 0.5)), vec2(1.0, 0.0));
    assert_eq!(flow.distance_at(vec2(5.5, 0.5)), Some(5.0));
}

#[test]
fn slow_ground_is_avoided_when_going_around_is_shorter() {
    // A strip of mud (cost 50) between start and goal, with a detour
    // around it that is longer in cells but much cheaper.
    let mut nav = NavGrid::new(9, 9, 1.0);
    for x in 0..8 {
        nav.set_cost(x, 4, 50);
    }
    let mut flow = FlowField::new();
    flow.compute(&nav, &[vec2(1.5, 7.5)]);
    let path = walk(&nav, &flow, vec2(1.5, 1.5), 100);
    assert!(
        path.iter().all(|c| nav.cost(c.x, c.y) != Some(50)),
        "never stepped in the mud: {path:?}"
    );
    // Crossing the mud straight would cost 5 + 50 + ... ; the detour is cheaper.
    assert!(flow.distance_at(vec2(1.5, 1.5)).unwrap() < 50.0);

    // Make the detour impossible and the mud is the only way.
    nav.set_cost(8, 4, NavGrid::WALL);
    flow.compute(&nav, &[vec2(1.5, 7.5)]);
    assert!(flow.distance_at(vec2(1.5, 1.5)).unwrap() > 50.0);
}

#[test]
fn origin_and_cell_size_map_world_points_to_cells() {
    let mut nav = NavGrid::new(10, 10, 8.0);
    nav.origin = vec2(-40.0, 100.0);
    assert_eq!(nav.cell_at(vec2(-40.0, 100.0)), Some(uvec2(0, 0)));
    assert_eq!(nav.cell_at(vec2(39.9, 179.9)), Some(uvec2(9, 9)));
    assert_eq!(nav.cell_at(vec2(-40.1, 100.0)), None);
    assert_eq!(nav.center_of(uvec2(1, 0)), vec2(-28.0, 104.0));

    let mut flow = FlowField::new();
    flow.compute(&nav, &[nav.center_of(uvec2(9, 0))]);
    assert_eq!(flow.direction_at(nav.center_of(uvec2(0, 0))), vec2(1.0, 0.0));
    assert_eq!(
        flow.distance_at(nav.center_of(uvec2(0, 0))),
        Some(72.0),
        "9 cells of 8 units"
    );
    assert_eq!(flow.direction_at(vec2(1000.0, 1000.0)), Vec2::ZERO, "outside");
    assert_eq!(flow.distance_at(vec2(1000.0, 1000.0)), None);
}

#[test]
fn goals_on_walls_or_outside_are_skipped() {
    let mut nav = NavGrid::new(5, 5, 1.0);
    nav.set_cost(2, 2, NavGrid::WALL);
    let mut flow = FlowField::new();
    flow.compute(&nav, &[vec2(2.5, 2.5), vec2(-3.0, 0.0)]);
    assert_eq!(
        flow.distance_at(vec2(0.5, 0.5)),
        None,
        "no usable goal: nothing is reachable"
    );
    assert_eq!(flow.direction_at(vec2(0.5, 0.5)), Vec2::ZERO);
}

#[test]
fn an_uncomputed_field_points_nowhere() {
    let flow = FlowField::new();
    assert_eq!(flow.direction_at(vec2(1.0, 1.0)), Vec2::ZERO);
    assert_eq!(flow.distance_at(vec2(1.0, 1.0)), None);
}

#[test]
fn fields_are_queried_from_parallel_queries() {
    let nav = NavGrid::new(100, 100, 1.0);
    let mut flow = FlowField::new();
    flow.compute(&nav, &[vec2(50.5, 50.5)]);
    let mut world = World::new();
    for i in 0..10_000 {
        world.spawn((Transform2D::at(vec2((i % 100) as f32 + 0.5, (i / 100) as f32 + 0.5)),));
    }
    let total = |world: &mut World| -> f32 {
        world
            .query::<&Transform2D>()
            .iter()
            .map(|t| flow.distance_at(t.position).unwrap())
            .sum()
    };
    let before = total(&mut world);
    let flow_ref = &flow;
    world.query::<&mut Transform2D>().par_for_each(|t| {
        t.position += flow_ref.direction_at(t.position) * 1.0;
    });
    // Everyone but the one at the goal got one cell closer, straight or diagonal.
    let after = total(&mut world);
    assert!(before - after >= 9_999.0 * 0.99, "{before} -> {after}");
}

#[test]
#[should_panic(expected = "NavGrid::new: 0 x 4 cells")]
fn empty_grids_panic() {
    let _ = NavGrid::new(0, 4, 1.0);
}

#[test]
#[should_panic(expected = "NavGrid::new: cell_size must be positive")]
fn cell_size_must_be_positive() {
    let _ = NavGrid::new(4, 4, 0.0);
}

#[test]
fn a_field_computed_on_the_frame_pool_matches_one_computed_on_one_thread() {
    // Every row's directions go to the frame pool's runner and are cut up
    // there; one thread runs the same pass in order.
    let mut nav = NavGrid::new(300, 200, 2.0);
    for y in 10..190 {
        nav.set_cost(100, y, NavGrid::WALL);
        nav.set_cost(200, 199 - y, NavGrid::WALL);
    }
    for x in 120..180 {
        nav.set_cost(x, 50, 5);
    }
    let goals = [vec2(590.0, 10.0), vec2(4.0, 390.0)];
    let compute = || {
        let mut flow = FlowField::new();
        flow.compute(&nav, &goals);
        flow
    };
    let one = rayon::ThreadPoolBuilder::new().num_threads(1).build().unwrap();
    let alone = one.install(compute);
    let shared = compute();
    assert_eq!(alone.raw_view().dir, shared.raw_view().dir);
    for y in 0..200 {
        for x in 0..300 {
            let at = nav.center_of(uvec2(x, y));
            assert_eq!(alone.distance_at(at), shared.distance_at(at));
        }
    }
    let path = walk(&nav, &shared, vec2(1.0, 1.0), 2_000);
    assert!(path.len() > 100, "it walks around both walls: {} cells", path.len());
}
