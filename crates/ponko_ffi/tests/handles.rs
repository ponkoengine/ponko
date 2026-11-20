//! One handle lifecycle through the C API: `ponko_ctx_handle_is_live`,
//! `ponko_ctx_handle_free` and `ponko_ctx_live_counts` for every kind.
mod common;

use ponko_ffi::*;
use std::ffi::c_void;
use std::ptr::null_mut;

use PonkoHandleKind::*;
use PonkoStatus::*;

unsafe fn is_live(ctx: *mut PonkoCtx, kind: PonkoHandleKind, id: u32) -> bool {
    let mut live = false;
    assert_eq!(
        unsafe { ponko_ctx_handle_is_live(ctx, kind as u32, id, &mut live) },
        PONKO_OK
    );
    live
}

unsafe fn counts(ctx: *mut PonkoCtx) -> PonkoLiveCounts {
    let mut c = PonkoLiveCounts::default();
    assert_eq!(unsafe { ponko_ctx_live_counts(ctx, &mut c) }, PONKO_OK);
    c
}

unsafe extern "C" fn lifecycle(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let errors = &mut *user.cast::<Vec<PonkoStatus>>();
        let before = counts(ctx);
        let white = PonkoColor {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 1.0,
        };
        let mut texture = std::mem::zeroed::<PonkoTexture>();
        assert_eq!(
            ponko_ctx_create_texture(ctx, 1, 1, [255u8; 4].as_ptr(), 4, &mut texture),
            PONKO_OK
        );
        let mut material = PonkoMaterial::default();
        assert_eq!(
            ponko_ctx_create_material(ctx, &ponko_material_desc(white), &mut material),
            PONKO_OK
        );
        let mut sound = std::mem::zeroed::<PonkoSound>();
        assert_eq!(
            ponko_ctx_create_sound(ctx, 8000, [0.0f32; 8].as_ptr(), 8, &mut sound),
            PONKO_OK
        );
        let made = [
            (PONKO_HANDLE_TEXTURE, texture.id),
            (PONKO_HANDLE_MATERIAL, material.id),
            (PONKO_HANDLE_SOUND, sound.id),
        ];
        let now = counts(ctx);
        assert_eq!(now.textures, before.textures + 1);
        assert_eq!(now.materials, before.materials + 1);
        assert_eq!(now.sounds, before.sounds + 1);
        for (kind, id) in made {
            assert!(is_live(ctx, kind, id), "{kind:?} live");
            assert_eq!(ponko_ctx_handle_free(ctx, kind as u32, id), PONKO_OK);
            assert_eq!(ponko_ctx_handle_free(ctx, kind as u32, id), PONKO_OK, "again: nothing");
            assert!(!is_live(ctx, kind, id), "{kind:?} dead");
        }
        assert_eq!(counts(ctx), before);
        // The C API's own checks see a freed material as gone.
        let data = ponko_mesh_data_cube(1.0);
        let mut mesh = PonkoMesh::default();
        errors.push(ponko_ctx_create_mesh(ctx, data, material, &mut mesh));
        ponko_mesh_data_free(data);
        // No handle of any kind has the NONE id.
        for kind in 0..=13 {
            let mut live = true;
            assert_eq!(
                ponko_ctx_handle_is_live(ctx, kind, PONKO_HANDLE_NONE, &mut live),
                PONKO_OK
            );
            assert!(!live);
            assert_eq!(ponko_ctx_handle_free(ctx, kind, PONKO_HANDLE_NONE), PONKO_OK);
        }
        // The built-in font is live and is never freed.
        assert!(is_live(ctx, PONKO_HANDLE_FONT, 0));
        assert_eq!(ponko_ctx_handle_free(ctx, PONKO_HANDLE_FONT as u32, 0), PONKO_OK);
        assert!(is_live(ctx, PONKO_HANDLE_FONT, 0));
        // Errors.
        errors.push(ponko_ctx_handle_free(ctx, 14, 0));
        errors.push(ponko_ctx_handle_is_live(ctx, 0, 0, null_mut()));
        errors.push(ponko_ctx_live_counts(ctx, null_mut()));
    }
}

#[test]
fn every_kind_frees_through_one_pair_of_calls() {
    let mut errors: Vec<PonkoStatus> = Vec::new();
    unsafe {
        let g = ponko_game_new(c"handles".as_ptr(), 64, 64);
        let mut world = null_mut();
        let user = (&raw mut errors).cast();
        assert_eq!(
            ponko_game_run_headless(g, 1, Some(lifecycle), user, &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
    }
    assert_eq!(errors, vec![PONKO_INVALID_ARGUMENT; 4]);
}

/// C passes render targets as plain `PonkoTexture`s; the kind Rust's
/// `RenderTarget` type checks when compiling is checked at run time.
unsafe extern "C" fn render_target_kind(ctx: *mut PonkoCtx, _: *mut c_void) {
    unsafe {
        let mut target = std::mem::zeroed::<PonkoTexture>();
        assert_eq!(ponko_ctx_create_render_target(ctx, 8, 8, &mut target), PONKO_OK);
        let mut plain = std::mem::zeroed::<PonkoTexture>();
        assert_eq!(
            ponko_ctx_create_texture(ctx, 1, 1, [255u8; 4].as_ptr(), 4, &mut plain),
            PONKO_OK
        );
        // A render target is a texture, and a render target.
        assert!(is_live(ctx, PONKO_HANDLE_TEXTURE, target.id));
        assert!(is_live(ctx, PONKO_HANDLE_RENDER_TARGET, target.id));
        // A texture is not a render target.
        assert!(is_live(ctx, PONKO_HANDLE_TEXTURE, plain.id));
        assert!(!is_live(ctx, PONKO_HANDLE_RENDER_TARGET, plain.id));
        assert_eq!(
            ponko_ctx_handle_free(ctx, PONKO_HANDLE_RENDER_TARGET as u32, plain.id),
            PONKO_INVALID_ARGUMENT
        );
        assert!(is_live(ctx, PONKO_HANDLE_TEXTURE, plain.id), "refused: still live");
        // Nor is it one where a pass writes.
        let camera = {
            let mut c = std::mem::zeroed::<PonkoCamera3D>();
            assert_eq!(ponko_ctx_camera3d(ctx, &mut c), PONKO_OK);
            c
        };
        let mut reflected = std::mem::zeroed::<PonkoTargetCamera>();
        assert_eq!(
            ponko_ctx_reflection_camera(ctx, &camera, plain, 0.0, &mut reflected),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_reflection_camera(ctx, &camera, target, 0.0, &mut reflected),
            PONKO_OK
        );
        assert_eq!(reflected.target.id, target.id);
        // Freed as a render target, it is gone under both kinds.
        assert_eq!(
            ponko_ctx_handle_free(ctx, PONKO_HANDLE_RENDER_TARGET as u32, target.id),
            PONKO_OK
        );
        assert!(!is_live(ctx, PONKO_HANDLE_TEXTURE, target.id));
        assert!(!is_live(ctx, PONKO_HANDLE_RENDER_TARGET, target.id));
        assert_eq!(
            ponko_ctx_handle_free(ctx, PONKO_HANDLE_RENDER_TARGET as u32, target.id),
            PONKO_OK,
            "again: nothing"
        );
    }
}

#[test]
fn render_targets_have_a_kind_checked_at_run_time() {
    unsafe {
        let g = ponko_game_new(c"render target kind".as_ptr(), 64, 64);
        let mut world = null_mut();
        assert_eq!(
            ponko_game_run_headless(g, 0, Some(render_target_kind), null_mut(), &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
    }
}

unsafe extern "C" fn accounting(ctx: *mut PonkoCtx, user: *mut c_void) {
    use PonkoAssetClass::*;
    unsafe {
        let errors = &mut *user.cast::<Vec<PonkoStatus>>();
        let count = |class: PonkoAssetClass| {
            let mut c = PonkoAssetCount::default();
            assert_eq!(ponko_ctx_asset_count(ctx, class as u32, &mut c), PONKO_OK);
            c
        };
        let before = count(PONKO_ASSET_CLASS_TEXTURE);
        let mut texture = std::mem::zeroed::<PonkoTexture>();
        let pixels = [0u8; 2 * 2 * 4];
        assert_eq!(
            ponko_ctx_create_texture(ctx, 2, 2, pixels.as_ptr(), pixels.len(), &mut texture),
            PONKO_OK
        );
        let after = count(PONKO_ASSET_CLASS_TEXTURE);
        assert_eq!((after.live, after.bytes), (before.live + 1, before.bytes + 16));
        let mut n = 0;
        let class = PONKO_ASSET_CLASS_TEXTURE as u32;
        assert_eq!(ponko_ctx_asset_handles(ctx, class, null_mut(), 0, &mut n), PONKO_OK);
        assert_eq!(n, after.live);
        let mut records = vec![PonkoAssetRecord::default(); n];
        let status = ponko_ctx_asset_handles(ctx, class, records.as_mut_ptr(), n, &mut n);
        assert_eq!(status, PONKO_OK);
        let mut oldest = PonkoAssetRecord::default();
        let status = ponko_ctx_asset_handles(ctx, class, &mut oldest, 1, &mut n);
        assert_eq!(status, PONKO_OK);
        assert_eq!(n, after.live);
        assert_eq!(oldest, records[0]);
        let made = records.iter().find(|r| r.id == texture.id).expect("listed");
        assert_eq!(made.owner, PonkoAssetOwner::PONKO_ASSET_OWNER_CODE as u32);
        assert_eq!((made.bytes, made.age_frames), (16, 0));
        // Made in code: no file name. (Loading and unloading a model file
        // is covered by the native tests, which have the asset folder.)
        let mut name = [1 as std::ffi::c_char; 8];
        let mut len = 9;
        assert_eq!(
            ponko_ctx_asset_owner(ctx, class, texture.id, name.as_mut_ptr(), name.len(), &mut len),
            PONKO_OK
        );
        assert_eq!((len, name[0]), (0, 0));
        let mut unloaded = true;
        assert_eq!(
            ponko_ctx_unload_mesh(ctx, c"models/none.glb".as_ptr(), &mut unloaded),
            PONKO_OK
        );
        assert!(!unloaded);
        // Errors.
        let mut c = PonkoAssetCount::default();
        errors.push(ponko_ctx_asset_count(ctx, 10, &mut c));
        errors.push(ponko_ctx_asset_handles(ctx, class, null_mut(), 1, &mut n));
        errors.push(ponko_ctx_asset_owner(ctx, class, 0, null_mut(), 1, &mut len));
        errors.push(ponko_ctx_unload_mesh(ctx, std::ptr::null(), &mut unloaded));
    }
}

#[test]
fn asset_counts_records_owners_and_unloading_work_from_c() {
    let mut errors: Vec<PonkoStatus> = Vec::new();
    unsafe {
        let g = ponko_game_new(c"accounting".as_ptr(), 64, 64);
        let mut world = null_mut();
        let user = (&raw mut errors).cast();
        assert_eq!(
            ponko_game_run_headless(g, 1, Some(accounting), user, &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
    }
    assert_eq!(errors, vec![PONKO_INVALID_ARGUMENT; 4]);
}

#[test]
fn native_mesh_cost_listing_is_empty_headless() {
    common::run_ctx(|ctx| unsafe {
        let mut record = PonkoMeshCost {
            models: 77,
            ..Default::default()
        };
        let mut names = [1; 8];
        let mut len = 9;
        let mut bytes = 9;
        assert_eq!(
            ponko_ctx_mesh_costs(
                ctx,
                &mut record,
                1,
                names.as_mut_ptr(),
                names.len(),
                &mut len,
                &mut bytes
            ),
            PONKO_OK
        );
        assert_eq!((len, bytes), (0, 0));
        assert_eq!(record.models, 77, "empty listing leaves caller storage alone");
        assert_eq!(names, [1; 8]);
        assert_eq!(
            ponko_ctx_mesh_costs(ctx, null_mut(), 1, null_mut(), 0, &mut len, &mut bytes),
            PONKO_INVALID_ARGUMENT
        );
    });
}
