//! The entity hierarchy from C: parents, ordered children, recursive
//! despawn and the update that places changed subtrees.
use ponko_ffi::*;
use std::ptr::null_mut;

use PonkoStatus::*;

fn spawn_at(w: *mut PonkoWorld, transform: u32, x: f32) -> u64 {
    let t = PonkoTransform2D {
        position: PonkoVec2 { x, y: 0.0 },
        ..Default::default()
    };
    let value = PonkoValue {
        component: transform,
        data: (&raw const t).cast(),
        size: size_of::<PonkoTransform2D>(),
    };
    let mut e = 0;
    assert_eq!(unsafe { ponko_spawn(w, &value, 1, &mut e) }, PONKO_OK);
    e
}

fn position(w: *mut PonkoWorld, transform: u32, e: u64) -> (f32, f32) {
    let mut t = PonkoTransform2D::default();
    let size = size_of::<PonkoTransform2D>();
    assert_eq!(
        unsafe { ponko_get(w, e, transform, (&raw mut t).cast(), size) },
        PONKO_OK
    );
    (t.position.x, t.position.y)
}

type Lister = unsafe extern "C" fn(*mut PonkoWorld, u64, *mut u64, usize, *mut usize) -> PonkoStatus;

fn listed(w: *mut PonkoWorld, e: u64, f: Lister) -> Vec<u64> {
    let mut n = 0;
    assert_eq!(unsafe { f(w, e, null_mut(), 0, &mut n) }, PONKO_OK);
    let mut out = vec![0; n];
    assert_eq!(unsafe { f(w, e, out.as_mut_ptr(), out.len(), &mut n) }, PONKO_OK);
    out
}

#[test]
fn c_hierarchies_keep_order_follow_their_root_and_despawn_whole() {
    unsafe {
        let w = ponko_world_new();
        let mut transform = 0;
        assert_eq!(ponko_transform_component(w, &mut transform), PONKO_OK);
        let root = spawn_at(w, transform, 10.0);
        let [a, b, c] = [11.0, 12.0, 13.0].map(|x| spawn_at(w, transform, x));
        for k in [a, b, c] {
            assert_eq!(ponko_set_parent(w, k, root), PONKO_OK);
        }
        assert_eq!(ponko_set_parent(w, root, a), PONKO_INVALID_ARGUMENT, "a loop");
        assert_eq!(listed(w, root, ponko_children), [a, b, c]);
        assert_eq!(ponko_set_child_index(w, c, 0), PONKO_OK);
        assert_eq!(ponko_set_child_index(w, root, 0), PONKO_NOT_FOUND);
        assert_eq!(listed(w, root, ponko_children), [c, a, b]);
        assert_eq!(ponko_set_parent(w, b, a), PONKO_OK);
        assert_eq!(listed(w, root, ponko_descendants), [c, a, b]);
        assert_eq!(listed(w, b, ponko_ancestors), [a, root]);
        let mut parent = 0;
        assert_eq!(ponko_parent(w, b, &mut parent), PONKO_OK);
        assert_eq!(parent, a);
        assert_eq!(ponko_parent(w, root, &mut parent), PONKO_NOT_FOUND);

        // Move the root: the update places the whole tree.
        let mut t = PonkoTransform2D::default();
        let size = size_of::<PonkoTransform2D>();
        assert_eq!(ponko_get(w, root, transform, (&raw mut t).cast(), size), PONKO_OK);
        t.position.y = 4.0;
        assert_eq!(ponko_set(w, root, transform, (&raw const t).cast(), size), PONKO_OK);
        assert_eq!(ponko_update_hierarchy(w), PONKO_OK);
        assert_eq!(position(w, transform, b), (12.0, 4.0));

        let mut had = false;
        assert_eq!(ponko_remove_parent(w, c, &mut had), PONKO_OK);
        assert!(had);
        assert_eq!(listed(w, root, ponko_children), [a]);
        let mut n = 0;
        assert_eq!(ponko_despawn_recursive(w, root, &mut n), PONKO_OK);
        assert_eq!(n, 3, "root, a and b; c is a root now");
        let mut alive = true;
        assert_eq!(ponko_is_alive(w, c, &mut alive), PONKO_OK);
        assert!(alive);
        ponko_world_free(w);
    }
}

/// What [`despawn_subtree_later`] works on: the tree, and what it saw.
#[derive(Default)]
struct Tree {
    root: u64,
    kids: [u64; 3],
    joined: u64,
    status: Option<PonkoStatus>,
}

unsafe extern "C" fn build_tree(ctx: *mut PonkoCtx, user: *mut std::ffi::c_void) {
    unsafe {
        let t = &mut *user.cast::<Tree>();
        let w = ponko_ctx_world(ctx);
        let mut transform = 0;
        assert_eq!(ponko_transform_component(w, &mut transform), PONKO_OK);
        t.root = spawn_at(w, transform, 10.0);
        t.kids = [11.0, 12.0, 13.0].map(|x| spawn_at(w, transform, x));
        t.joined = spawn_at(w, transform, 14.0);
        for k in t.kids {
            assert_eq!(ponko_set_parent(w, k, t.root), PONKO_OK);
        }
    }
}

/// Records the recursive despawn, then changes the tree before the
/// system's commands are applied.
unsafe extern "C" fn despawn_subtree_later(ctx: *mut PonkoCtx, user: *mut std::ffi::c_void) {
    unsafe {
        let t = &mut *user.cast::<Tree>();
        if t.status.is_some() {
            return;
        }
        let w = ponko_ctx_world(ctx);
        t.status = Some(ponko_ctx_despawn_recursive_later(ctx, t.root));
        assert_eq!(ponko_set_parent(w, t.joined, t.root), PONKO_OK);
        let mut had = false;
        assert_eq!(ponko_remove_parent(w, t.kids[2], &mut had), PONKO_OK);
    }
}

#[test]
fn c_commands_despawn_the_subtree_as_it_is_when_applied() {
    let mut tree = Tree::default();
    unsafe {
        assert_eq!(ponko_ctx_despawn_recursive_later(null_mut(), 1), PONKO_INVALID_ARGUMENT);
        let g = ponko_game_new(c"tree".as_ptr(), 1, 1);
        let user = (&raw mut tree).cast();
        ponko_game_sim_system(g, std::ptr::null(), 1, Some(despawn_subtree_later), user);
        let mut w = null_mut();
        assert_eq!(ponko_game_run_headless(g, 2, Some(build_tree), user, &mut w), PONKO_OK);
        assert_eq!(tree.status, Some(PONKO_OK));
        let alive = |e: u64| {
            let mut alive = false;
            assert_eq!(ponko_is_alive(w, e, &mut alive), PONKO_OK);
            alive
        };
        for gone in [tree.root, tree.kids[0], tree.kids[1], tree.joined] {
            assert!(!alive(gone), "{gone} is in the subtree when applied");
        }
        assert!(alive(tree.kids[2]), "it left the subtree before the apply");
        ponko_world_free(w);
    }
}
