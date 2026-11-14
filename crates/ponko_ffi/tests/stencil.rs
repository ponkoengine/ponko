//! Stencil state on materials from C: the constructors mirror Rust's, and
//! materials take one when made or changed, with bad values refused.
use ponko_ffi::*;
use std::ffi::c_void;
use std::ptr::{null, null_mut};

use PonkoStatus::*;

#[derive(Default)]
struct Seen {
    statuses: Vec<PonkoStatus>,
    done: bool,
}

const WHITE: PonkoColor = PonkoColor {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 1.0,
};

unsafe extern "C" fn setup(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let seen = &mut *user.cast::<Seen>();
        let desc = ponko_material_desc(WHITE);
        let once = ponko_stencil_once(1);
        let mut glass = PonkoMaterial::default();
        seen.statuses
            .push(ponko_ctx_create_material_with_stencil(ctx, &desc, &once, &mut glass));
        let mut plain = PonkoMaterial::default();
        seen.statuses
            .push(ponko_ctx_create_material_with_stencil(ctx, &desc, null(), &mut plain));
        let outline = PonkoStencil {
            order: 1,
            ..ponko_stencil_outside(1)
        };
        seen.statuses
            .push(ponko_ctx_update_material_with_stencil(ctx, plain, &desc, &outline));
        seen.statuses
            .push(ponko_ctx_update_material_with_stencil(ctx, glass, &desc, null()));
        // Refused: a compare or an operation out of range, no output.
        let mut bad = ponko_stencil_mark(2);
        bad.front.compare = 8;
        let mut unused = PonkoMaterial::default();
        seen.statuses
            .push(ponko_ctx_create_material_with_stencil(ctx, &desc, &bad, &mut unused));
        let mut bad = ponko_stencil_mark(2);
        bad.back.depth_fail = 9;
        seen.statuses
            .push(ponko_ctx_update_material_with_stencil(ctx, glass, &desc, &bad));
        seen.statuses
            .push(ponko_ctx_create_material_with_stencil(ctx, &desc, &once, null_mut()));
        seen.done = true;
    }
}

#[test]
fn materials_take_a_stencil_from_c() {
    let mut seen = Seen::default();
    unsafe {
        let g = ponko_game_new(c"stencil".as_ptr(), 64, 64);
        let mut world = null_mut();
        assert_eq!(
            ponko_game_run_headless(g, 1, Some(setup), (&raw mut seen).cast(), &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
    }
    assert!(seen.done);
    assert_eq!(
        seen.statuses,
        [
            PONKO_OK,
            PONKO_OK,
            PONKO_OK,
            PONKO_OK,
            PONKO_INVALID_ARGUMENT,
            PONKO_INVALID_ARGUMENT,
            PONKO_INVALID_ARGUMENT
        ]
    );
}

#[test]
fn the_stencil_constructors_mirror_rust() {
    use PonkoStencilCompare::*;
    use PonkoStencilOp::*;
    let face = |compare: PonkoStencilCompare, pass: PonkoStencilOp| PonkoStencilFace {
        compare: compare as u32,
        fail: PONKO_STENCIL_KEEP as u32,
        depth_fail: PONKO_STENCIL_KEEP as u32,
        pass: pass as u32,
    };
    let both = |f: PonkoStencilFace, reference: u8| PonkoStencil {
        front: f,
        back: f,
        order: 0,
        reference,
        read_mask: 0xFF,
        write_mask: 0xFF,
    };
    assert_eq!(
        ponko_stencil_default(),
        both(face(PONKO_STENCIL_ALWAYS, PONKO_STENCIL_KEEP), 0)
    );
    assert_eq!(
        ponko_stencil_mark(3),
        both(face(PONKO_STENCIL_ALWAYS, PONKO_STENCIL_REPLACE), 3)
    );
    assert_eq!(
        ponko_stencil_inside(4),
        both(face(PONKO_STENCIL_EQUAL, PONKO_STENCIL_KEEP), 4)
    );
    assert_eq!(
        ponko_stencil_outside(5),
        both(face(PONKO_STENCIL_NOT_EQUAL, PONKO_STENCIL_KEEP), 5)
    );
    assert_eq!(
        ponko_stencil_once(6),
        both(face(PONKO_STENCIL_NOT_EQUAL, PONKO_STENCIL_REPLACE), 6)
    );
    // The enums' values are Rust's.
    assert_eq!(
        PONKO_STENCIL_DECREMENT_WRAP as u32,
        ponko::render::StencilOp::DecrementWrap as u32
    );
    assert_eq!(
        PONKO_STENCIL_GREATER_EQUAL as u32,
        ponko::render::StencilCompare::GreaterEqual as u32
    );
}
