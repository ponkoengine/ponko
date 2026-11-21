//! Seas from a wave spectrum through the C ABI: `PonkoWaterDesc` version 2
//! (`has_spectrum`, `PonkoWaveSpectrum`) draws the same sea as Rust's
//! `Waves::spectrum`, a version 1 descriptor keeps its Gerstner waves, and
//! bad spectra are refused.
mod common;
use common::{last_error, run_ctx};
use ponko::prelude::vec2;
use ponko::water::{WaveSpectrum, Waves};
use ponko_ffi::*;

use PonkoStatus::*;

fn v2(x: f32, y: f32) -> PonkoVec2 {
    PonkoVec2 { x, y }
}

fn sea(version: u32, size: usize) -> PonkoWaterDesc {
    // SAFETY: a plain C struct; zero is a valid bit pattern for every field.
    let mut d: PonkoWaterDesc = unsafe { std::mem::zeroed() };
    (d.struct_size, d.version) = (size as u32, version);
    // SAFETY: a live descriptor with its header set.
    assert_eq!(
        unsafe { ponko_water_desc_sea(&mut d, 0.5, v2(12.0, 4.0), 2.0) },
        PONKO_OK
    );
    d
}

unsafe fn height(ctx: *mut PonkoCtx, water: u64, x: f32, z: f32, t: f32) -> f32 {
    let mut s = PonkoWaterSample::default();
    assert_eq!(
        unsafe { ponko_ctx_water_sample(ctx, water, v2(x, z), t, &mut s) },
        PONKO_OK,
        "{}",
        last_error()
    );
    s.height
}

#[test]
fn a_spectrum_sea_from_c_is_the_same_sea_as_rusts() {
    let spectrum = ponko_wave_spectrum(v2(12.0, 4.0), 2.0);
    assert_eq!(
        (spectrum.tile, spectrum.resolution, spectrum.choppiness, spectrum.seed),
        (256.0, 128, 1.0, 1)
    );
    let rust = Waves::spectrum(WaveSpectrum::new(vec2(12.0, 4.0), 2.0).with_seed(7));
    run_ctx(|ctx| unsafe {
        let mut d = sea(PONKO_WATER_DESC_VERSION, size_of::<PonkoWaterDesc>());
        d.has_spectrum = true;
        d.spectrum = PonkoWaveSpectrum { seed: 7, ..spectrum };
        let mut water = 0;
        assert_eq!(ponko_ctx_spawn_water(ctx, &d, &mut water), PONKO_OK, "{}", last_error());
        let mut moved = false;
        for (i, (x, z)) in [(3.0, -2.0), (40.0, 11.0), (-90.0, 7.5)].into_iter().enumerate() {
            let t = 1.25 * i as f32;
            let c = height(ctx, water, x, z, t);
            let r = 0.5 + rust.height_at(vec2(x, z), t);
            assert!((c - r).abs() < 1e-3, "at ({x}, {z}) t {t}: C {c}, Rust {r}");
            moved |= (c - 0.5).abs() > 1e-3;
        }
        assert!(moved, "a 2 m sea is not flat");
    });
}

#[test]
fn a_version_1_desc_keeps_its_gerstner_waves() {
    run_ctx(|ctx| unsafe {
        let v1_len = std::mem::offset_of!(PonkoWaterDesc, has_spectrum);
        let mut old = sea(1, v1_len);
        // What a version 1 program never wrote: ignored.
        old.has_spectrum = true;
        old.spectrum = ponko_wave_spectrum(v2(12.0, 4.0), 2.0);
        let gerstner = sea(PONKO_WATER_DESC_VERSION, size_of::<PonkoWaterDesc>());
        let (mut a, mut b) = (0, 0);
        assert_eq!(ponko_ctx_spawn_water(ctx, &old, &mut a), PONKO_OK, "{}", last_error());
        assert_eq!(ponko_ctx_spawn_water(ctx, &gerstner, &mut b), PONKO_OK);
        for t in [0.0, 0.7, 2.2] {
            assert_eq!(height(ctx, a, 5.0, 9.0, t), height(ctx, b, 5.0, 9.0, t));
        }
    });
}

#[test]
fn bad_spectra_are_refused() {
    run_ctx(|ctx| unsafe {
        let good = ponko_wave_spectrum(v2(10.0, 0.0), 1.0);
        for bad in [
            PonkoWaveSpectrum { tile: 0.0, ..good },
            PonkoWaveSpectrum { height: -1.0, ..good },
            PonkoWaveSpectrum {
                choppiness: f32::NAN,
                ..good
            },
            PonkoWaveSpectrum {
                wind: v2(f32::INFINITY, 0.0),
                ..good
            },
        ] {
            let mut d = sea(PONKO_WATER_DESC_VERSION, size_of::<PonkoWaterDesc>());
            d.has_spectrum = true;
            d.spectrum = bad;
            let mut water = 0;
            assert_eq!(ponko_ctx_spawn_water(ctx, &d, &mut water), PONKO_INVALID_ARGUMENT);
        }
    });
}
