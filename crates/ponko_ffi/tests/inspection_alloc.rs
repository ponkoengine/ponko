//! Isolate process-wide CountingAllocator measurements in a single-test binary.
mod common;

use ponko::perf::{alloc_counts, thread_alloc_counts};
use ponko_ffi::PonkoStatus::*;
use ponko_ffi::*;
use std::ffi::CString;
use std::hint::black_box;
use std::ptr::null_mut;

#[test]
fn asset_handle_probes_and_fills_allocate_nothing_through_ffi() {
    common::run_ctx(|ctx| unsafe {
        let path = CString::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/crab.png")).unwrap();
        let mut texture = std::mem::zeroed::<PonkoTexture>();
        assert_eq!(ponko_ctx_load_texture(ctx, path.as_ptr(), &mut texture), PONKO_OK);
        for _ in 0..4 {
            let mut code = std::mem::zeroed::<PonkoTexture>();
            assert_eq!(
                ponko_ctx_create_texture(ctx, 1, 1, [255; 4].as_ptr(), 4, &mut code),
                PONKO_OK
            );
        }
        let mut sound = std::mem::zeroed::<PonkoSound>();
        let silent_samples = [0.0; 80];
        assert_eq!(
            ponko_ctx_create_sound(ctx, 8000, silent_samples.as_ptr(), silent_samples.len(), &mut sound),
            PONKO_OK
        );
        let class = PonkoAssetClass::PONKO_ASSET_CLASS_TEXTURE as u32;
        let mut full = [PonkoAssetRecord::default(); 16];
        let mut short = [PonkoAssetRecord::default(); 3];
        let mut len = 0;
        let mut sound_record = PonkoAssetRecord::default();
        let mut sounds = 0;

        assert_eq!(ponko_ctx_set_profiling(ctx, true), PONKO_OK);
        let before = alloc_counts().count;
        drop(black_box(Box::new(1u32)));
        assert!(alloc_counts().count > before, "positive control for the allocator");

        for (class, ptr, capacity, out_len) in [
            (class, null_mut(), 0, &raw mut len),
            (class, full.as_mut_ptr(), full.len(), &raw mut len),
            (class, short.as_mut_ptr(), short.len(), &raw mut len),
            (
                PonkoAssetClass::PONKO_ASSET_CLASS_SOUND as u32,
                &raw mut sound_record,
                1,
                &raw mut sounds,
            ),
        ] {
            for _ in 0..8 {
                let before = thread_alloc_counts().count;
                let status = ponko_ctx_asset_handles(ctx, class, ptr, capacity, out_len);
                let count = thread_alloc_counts().count - before;
                assert_eq!(status, PONKO_OK);
                assert_eq!(count, 0, "capacity {capacity}");
            }
        }
        assert_eq!(ponko_ctx_set_profiling(ctx, false), PONKO_OK);

        assert!((5..=full.len()).contains(&len));
        assert_eq!(short, full[..3]);
        assert_eq!((sounds, sound_record.id), (1, sound.id));
        let file = full[..len].iter().find(|r| r.id == texture.id).expect("loaded texture");
        assert_eq!(file.owner, PonkoAssetOwner::PONKO_ASSET_OWNER_FILE as u32);
        assert!(
            full[..len]
                .windows(2)
                .all(|r| (r[0].made_frame, r[0].id) < (r[1].made_frame, r[1].id))
        );
    });
}
