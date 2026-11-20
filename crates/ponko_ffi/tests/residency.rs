//! Residency budgets and asset streaming through the C API: textures and
//! meshes streamed without waiting, their states and errors, budgets,
//! upload stats, and a LOD group whose level streams in.
use std::ffi::{CString, c_void};
use std::ptr::null_mut;

mod common;

use ponko_ffi::*;

use PonkoAssetState::*;
use PonkoResidencyKind::*;
use PonkoStatus::*;

const HOUSE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/models/house.glb");
const CRAB: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/crab.png");

unsafe fn state(ctx: *mut PonkoCtx, kind: PonkoResidencyKind, name: &CString) -> u32 {
    let mut s = 99;
    assert_eq!(
        unsafe { ponko_ctx_asset_state(ctx, kind as u32, name.as_ptr(), &mut s) },
        PONKO_OK
    );
    s
}

#[derive(Default)]
struct Seen {
    entity: u64,
    tick: u32,
    house_resident_at: Option<u32>,
}

unsafe extern "C" fn setup(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let seen = &mut *user.cast::<Seen>();
        let (house, crab) = (CString::new(HOUSE).unwrap(), CString::new(CRAB).unwrap());
        assert_eq!(ponko_ctx_set_deterministic_uploads(ctx, true), PONKO_OK);
        assert_eq!(ponko_ctx_set_upload_budget(ctx, true, 1 << 20), PONKO_OK);

        // A texture: not ready at once, then resident under one handle.
        let mut texture = std::mem::zeroed::<PonkoTexture>();
        let mut ready = true;
        assert_eq!(
            ponko_ctx_stream_texture(ctx, crab.as_ptr(), &mut texture, &mut ready),
            PONKO_OK
        );
        assert!(!ready);
        assert_eq!(state(ctx, PONKO_RESIDENCY_TEXTURE, &crab), PONKO_ASSET_LOADING as u32);
        assert_eq!(ponko_ctx_finish_uploads(ctx), PONKO_OK);
        assert_eq!(
            ponko_ctx_stream_texture(ctx, crab.as_ptr(), &mut texture, &mut ready),
            PONKO_OK
        );
        assert!(ready && texture.width == 32);
        assert_eq!(state(ctx, PONKO_RESIDENCY_TEXTURE, &crab), PONKO_ASSET_RESIDENT as u32);
        let mut again = std::mem::zeroed::<PonkoTexture>();
        assert_eq!(
            ponko_ctx_resident_texture(ctx, crab.as_ptr(), true, &mut again),
            PONKO_OK
        );
        assert_eq!(again.id, texture.id);
        let mut stats = PonkoResidencyStats::default();
        assert_eq!(
            ponko_ctx_residency(ctx, PONKO_RESIDENCY_TEXTURE as u32, &mut stats),
            PONKO_OK
        );
        assert_eq!(
            (stats.resident, stats.pinned, stats.resident_bytes),
            (1, 1, 32 * 32 * 4)
        );
        assert_eq!(
            ponko_ctx_unpin_resident(ctx, PONKO_RESIDENCY_TEXTURE as u32, crab.as_ptr()),
            PONKO_OK
        );

        // Budgets.
        assert_eq!(
            ponko_ctx_set_residency_budget(ctx, PONKO_RESIDENCY_MESH as u32, true, 1 << 30),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_residency(ctx, PONKO_RESIDENCY_MESH as u32, &mut stats),
            PONKO_OK
        );
        assert!(stats.has_budget && stats.budget == 1 << 30 && stats.resident == 0);
        assert_eq!(ponko_ctx_set_residency_budget(ctx, 9, true, 0), PONKO_INVALID_ARGUMENT);

        // A missing file: failed, with its message.
        let nope = c"nope.png";
        assert_eq!(
            ponko_ctx_stream_texture(ctx, nope.as_ptr(), &mut texture, &mut ready),
            PONKO_OK
        );
        assert_eq!(ponko_ctx_finish_uploads(ctx), PONKO_OK);
        assert_eq!(
            state(ctx, PONKO_RESIDENCY_TEXTURE, &CString::from(nope)),
            PONKO_ASSET_FAILED as u32
        );
        let mut buffer = [0 as std::ffi::c_char; 8];
        let mut len = 0;
        assert_eq!(
            ponko_ctx_asset_error(
                ctx,
                PONKO_RESIDENCY_TEXTURE as u32,
                nope.as_ptr(),
                buffer.as_mut_ptr(),
                buffer.len(),
                &mut len
            ),
            PONKO_OK
        );
        assert!(len > buffer.len(), "the full length, cut short in the buffer");
        assert_eq!(buffer[7], 0);
        let mut uploads = PonkoUploadStats::default();
        assert_eq!(ponko_ctx_upload_stats(ctx, &mut uploads), PONKO_OK);
        assert_eq!((uploads.failures, uploads.pending), (1, 0));
        assert!(uploads.has_budget && uploads.budget == 1 << 20);

        // A LOD group whose one level streams in from a file.
        let world = ponko_ctx_world(ctx);
        let mut transform = 0;
        assert_eq!(ponko_transform3d_component(world, &mut transform), PONKO_OK);
        let at = ponko_transform3d_at(PonkoVec3 { x: 0.0, y: 0.0, z: 0.0 });
        let value = PonkoValue {
            component: transform,
            data: (&raw const at).cast(),
            size: size_of::<PonkoTransform3D>(),
        };
        assert_eq!(ponko_spawn(world, &value, 1, &mut seen.entity), PONKO_OK);
        let renderer = ponko_lod_renderer(PonkoMesh { id: PONKO_MESH_NONE });
        let level = PonkoLodLevel {
            screen_height: 0.0,
            fade_width: 0.0,
            renderers: &renderer,
            renderer_count: 1,
            custom_shadow: false,
            shadow_casters: std::ptr::null(),
            shadow_caster_count: 0,
        };
        assert_eq!(
            ponko_ctx_set_lod_group(ctx, seen.entity, 4.0, PonkoVec3 { x: 0.0, y: 0.0, z: 0.0 }, &level, 1),
            PONKO_OK
        );
        let files = [house.as_ptr()];
        assert_eq!(
            ponko_ctx_set_streamed_lod(ctx, seen.entity, files.as_ptr(), 1),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_set_streamed_lod(ctx, seen.entity, std::ptr::null(), 1),
            PONKO_INVALID_ARGUMENT
        );
    }
}

unsafe extern "C" fn watch(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let seen = &mut *user.cast::<Seen>();
        seen.tick += 1;
        let house = CString::new(HOUSE).unwrap();
        if seen.house_resident_at.is_none() && state(ctx, PONKO_RESIDENCY_MESH, &house) == PONKO_ASSET_RESIDENT as u32 {
            seen.house_resident_at = Some(seen.tick);
            let mut mesh = PonkoMesh::default();
            let mut ready = false;
            assert_eq!(
                ponko_ctx_stream_mesh(ctx, house.as_ptr(), &mut mesh, &mut ready),
                PONKO_OK
            );
            assert!(ready);
            let mut resident = false;
            assert_eq!(
                ponko_ctx_is_resident(ctx, PONKO_RESIDENCY_MESH as u32, house.as_ptr(), &mut resident),
                PONKO_OK
            );
            assert!(resident);
        }
    }
}

#[test]
fn games_stream_assets_and_lod_levels_through_the_c_api() {
    let mut seen = Seen::default();
    let user = (&raw mut seen).cast();
    unsafe {
        let g = ponko_game_new(c"streaming".as_ptr(), 64, 64);
        assert_eq!(
            ponko_game_sim_system(g, c"watch".as_ptr(), 1, Some(watch), user),
            PONKO_OK
        );
        let mut world = null_mut();
        assert_eq!(ponko_game_run_headless(g, 4, Some(setup), user, &mut world), PONKO_OK);
        assert_eq!(ponko_world_free(world), PONKO_OK);
    }
    // Asked for by the group in the first frame's streaming step and, with
    // deterministic uploads, uploaded in the same step.
    assert_eq!(seen.house_resident_at, Some(1));
}

#[test]
fn caller_buffers_get_the_sorted_streamed_asset_prefix_and_its_names() {
    common::run_ctx(|ctx| unsafe {
        let mut texture = std::mem::zeroed::<PonkoTexture>();
        let mut ready = false;
        for path in [c"missing-z.png", c"missing-a.png"] {
            assert_eq!(
                ponko_ctx_stream_texture(ctx, path.as_ptr(), &mut texture, &mut ready),
                PONKO_OK
            );
        }
        assert_eq!(ponko_ctx_finish_uploads(ctx), PONKO_OK);

        let mut len = 99;
        let mut bytes = 99;
        assert_eq!(
            ponko_ctx_streamed_assets(ctx, null_mut(), 0, null_mut(), 0, &mut len, &mut bytes),
            PONKO_OK
        );
        assert_eq!((len, bytes), (2, 0));

        let mut records = [PonkoStreamedAsset::default(); 2];
        let mut names = [1; 40];
        assert_eq!(
            ponko_ctx_streamed_assets(
                ctx,
                records.as_mut_ptr(),
                1,
                names.as_mut_ptr(),
                2,
                &mut len,
                &mut bytes
            ),
            PONKO_OK
        );
        assert_eq!((len, bytes), (2, c"missing-a.png".count_bytes() + 1));
        assert_eq!(records[1], PonkoStreamedAsset::default());
        assert_eq!(names, [1; 40], "no partial name");

        assert_eq!(
            ponko_ctx_streamed_assets(
                ctx,
                records.as_mut_ptr(),
                records.len(),
                names.as_mut_ptr(),
                names.len(),
                &mut len,
                &mut bytes
            ),
            PONKO_OK
        );
        assert_eq!(len, 2);
        assert_eq!(bytes, 2 * (c"missing-a.png".count_bytes() + 1));
        for (record, name) in records.iter().zip([c"missing-a.png", c"missing-z.png"]) {
            assert_eq!(record.kind, PONKO_RESIDENCY_TEXTURE as u32);
            assert_eq!(record.state, PONKO_ASSET_FAILED as u32);
            assert!(!record.has_handle);
            assert_eq!((record.width, record.height), (0, 0));
            assert_eq!(record.name_len, name.count_bytes());
            assert_eq!(std::ffi::CStr::from_ptr(names.as_ptr().add(record.name_offset)), name);
        }

        assert_eq!(
            ponko_ctx_streamed_assets(ctx, records.as_mut_ptr(), 1, null_mut(), 1, &mut len, &mut bytes),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_streamed_assets(ctx, records.as_mut_ptr(), 1, null_mut(), 0, &mut len, null_mut()),
            PONKO_INVALID_ARGUMENT
        );
    });
}
