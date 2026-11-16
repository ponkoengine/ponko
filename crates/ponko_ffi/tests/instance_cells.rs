//! Instance cells from C: added to a batch, replaced and freed on their own,
//! freed with their batch, and every misuse reported.
mod common;

use PonkoStatus::*;
use common::run_ctx;
use ponko_ffi::*;
use std::ptr::{null, null_mut};

const WHITE: PonkoColor = PonkoColor {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 1.0,
};

fn at(x: f32) -> [f32; 16] {
    ponko::prelude::Mat4::from_translation(ponko::prelude::Vec3::new(x, 0.0, 0.0)).to_cols_array()
}

#[test]
fn cells_are_added_replaced_and_freed_on_their_own() {
    run_ctx(|ctx| unsafe {
        let mut material = PonkoMaterial::default();
        let desc = ponko_material_desc(WHITE);
        assert_eq!(ponko_ctx_create_material(ctx, &desc, &mut material), PONKO_OK);
        let mut mesh = PonkoMesh::default();
        assert_eq!(
            ponko_ctx_create_mesh(ctx, ponko_mesh_data_cube(1.0), material, &mut mesh),
            PONKO_OK
        );
        let options = PonkoInstanceBatchOptions {
            cell_size: 32.0,
            max_distance: f32::INFINITY,
            cast_shadows: true,
            tint: WHITE,
        };
        let mut batch = PonkoInstanceBatch::default();
        assert_eq!(
            ponko_ctx_create_instance_batch(ctx, mesh, null(), 0, options, &mut batch),
            PONKO_OK
        );

        let three = [at(0.0), at(1.0), at(2.0)];
        let colors = [WHITE; 3];
        let (mut a, mut b) = (PonkoInstanceCell::default(), PonkoInstanceCell::default());
        assert_eq!(
            ponko_ctx_add_instance_cell(ctx, batch, three.as_ptr(), 3, colors.as_ptr(), 3, &mut a),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_add_instance_cell(ctx, batch, three.as_ptr(), 1, null(), 0, &mut b),
            PONKO_OK
        );
        let len = |cell| {
            let mut n = 0;
            (ponko_ctx_instance_cell_len(ctx, cell, &mut n), n)
        };
        assert_eq!(len(a), (PONKO_OK, 3));
        assert_eq!(len(b), (PONKO_OK, 1));

        // Replacing one leaves the other.
        assert_eq!(
            ponko_ctx_replace_instance_cell(ctx, a, three.as_ptr(), 2, null(), 0),
            PONKO_OK
        );
        assert_eq!((len(a), len(b)), ((PONKO_OK, 2), (PONKO_OK, 1)));

        // Misuse: counts, nulls, a bad matrix.
        assert_eq!(
            ponko_ctx_add_instance_cell(ctx, batch, three.as_ptr(), 3, colors.as_ptr(), 2, &mut b),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_add_instance_cell(ctx, batch, null(), 3, null(), 0, &mut b),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_add_instance_cell(ctx, batch, three.as_ptr(), 3, null(), 0, null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        let mut bad = three;
        bad[1][3] = 1.0;
        assert_eq!(
            ponko_ctx_replace_instance_cell(ctx, a, bad.as_ptr(), 3, null(), 0),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_add_instance_cell(null_mut(), batch, three.as_ptr(), 3, null(), 0, &mut b),
            PONKO_INVALID_ARGUMENT
        );

        // Freed: dead, and freeing again does nothing.
        assert_eq!(ponko_ctx_free_instance_cell(ctx, a), PONKO_OK);
        assert_eq!(ponko_ctx_free_instance_cell(ctx, a), PONKO_OK);
        assert_eq!(len(a).0, PONKO_INVALID_ARGUMENT);
        assert_eq!(
            ponko_ctx_replace_instance_cell(ctx, a, three.as_ptr(), 3, null(), 0),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(len(b), (PONKO_OK, 1));

        // The batch's cells go with it; a freed batch takes no cells.
        assert_eq!(ponko_ctx_free_instance_batch(ctx, batch), PONKO_OK);
        assert_eq!(len(b).0, PONKO_INVALID_ARGUMENT);
        assert_eq!(
            ponko_ctx_add_instance_cell(ctx, batch, three.as_ptr(), 3, null(), 0, &mut b),
            PONKO_INVALID_ARGUMENT
        );
    });
}
