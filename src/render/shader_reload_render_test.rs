//! On a real GPU (offscreen): a material shader reload that fails, in the
//! compiler or when the GPU builds its pipelines, keeps the last good
//! pipelines, and leaves the other shaders alone.
use super::*;

const RED: &str = "fn surface(s: Surface) -> Surface {\n    var out = s;\n    out.albedo = vec3<f32>(1.0, 0.0, 0.0);\n    return out;\n}\n";
const BLUE: &str = "fn surface(s: Surface) -> Surface {\n    var out = s;\n    out.albedo = vec3<f32>(0.0, 0.0, 1.0);\n    return out;\n}\n";
/// A minimal invalid shader: line 3 returns a number, not a `Surface`.
const BROKEN: &str = "// tints everything red\nfn surface(s: Surface) -> Surface {\n    return 1.0;\n}\n";
/// Valid WGSL the GPU cannot build pipelines for: an unfilterable light-record texture
/// sampled with a filtering sampler. Binding declarations now fail the contract
/// earlier, so they cannot exercise asynchronous GPU rejection anymore.
const BAD_SAMPLING: &str = "fn vertex(v: Vertex) -> Vertex { var out = v; out.position.x += textureSampleLevel(punctual_records, material_sampler, v.uv, 0.0).x; return out; }";

fn rig() -> Option<(wgpu::Device, Renderer3D)> {
    let gpu = crate::render::render_test_support::gpu_with(|_| Ok(Default::default()))?;
    let (device, _queue) = (gpu.device, gpu.queue);
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let white = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("shader reload test white"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let renderer = Renderer3D::new(&device, &layout, wgpu::TextureFormat::Rgba8Unorm, 0, &white, 4, true);
    Some((device, renderer))
}

/// The pipelines shader `id` draws with now.
fn pipelines(renderer: &Renderer3D, id: u32) -> Vec<wgpu::RenderPipeline> {
    renderer.shaders.sets[id as usize]
        .sides
        .iter()
        .flat_map(|p| {
            [
                p.opaque.clone(),
                p.prepass.clone(),
                p.shadow.clone(),
                p.transparent.clone(),
            ]
        })
        .collect()
}

#[test]
fn failed_reloads_keep_the_last_good_pipelines_and_spare_other_shaders() {
    let Some((device, mut renderer)) = rig() else {
        return;
    };
    let red = renderer.create_shader(&device, "red.wgsl", RED).unwrap();
    let blue = renderer.create_shader(&device, "blue.wgsl", BLUE).unwrap();
    let (red_before, blue_before) = (pipelines(&renderer, red), pipelines(&renderer, blue));

    // The compiler rejects it: where, why, and on which variant.
    let e = renderer.replace_shader(&device, blue, "blue.wgsl", BROKEN).unwrap_err();
    assert_eq!((e.line, e.column, e.in_engine_code), (Some(3), Some(12), false), "{e}");
    assert!(e.variant.starts_with("material shader on this GPU"), "{e}");
    assert_eq!(pipelines(&renderer, blue), blue_before, "the last good version stays");
    assert_eq!(pipelines(&renderer, red), red_before, "other shaders are untouched");

    // Contract failures use the same located diagnostic and keep last-good
    // pipelines, before any GPU scope or deferred verdict is needed.
    for source in [
        "fn vertex() -> f32 { return 0.0; }",
        "@group(0) @binding(0) var<uniform> frame: vec4<f32>;",
    ] {
        let e = renderer.replace_shader(&device, blue, "blue.wgsl", source).unwrap_err();
        assert_eq!((e.line, e.in_engine_code), (Some(1), false), "{e}");
        assert!(
            e.message.contains("signature") || e.message.contains("engine-owned"),
            "{e}"
        );
        assert_eq!(pipelines(&renderer, blue), blue_before);
        assert_eq!(pipelines(&renderer, red), red_before);
    }

    // The GPU rejects its pipelines: the error comes back, the device lives on.
    let e = renderer
        .replace_shader(&device, blue, "blue.wgsl", BAD_SAMPLING)
        .unwrap_err();
    assert!(e.message.contains("rejected"), "{e}");
    assert_eq!(pipelines(&renderer, blue), blue_before);
    assert_eq!(pipelines(&renderer, red), red_before);

    // A fix takes effect, and new shaders still build.
    renderer.replace_shader(&device, blue, "blue.wgsl", RED).unwrap();
    assert_ne!(pipelines(&renderer, blue), blue_before, "the fix replaced them");
    renderer.create_shader(&device, "third.wgsl", BLUE).unwrap();
    assert_eq!(pipelines(&renderer, red), red_before);
}

/// Material pipeline keys built so far whose module is `source`.
fn keys_of(renderer: &Renderer3D, source: &str) -> Vec<ShaderVariant> {
    let mut variants: Vec<_> = renderer
        .built_pipeline_keys()
        .into_iter()
        .filter(|k| &*k.set.module.source == source)
        .map(|k| k.variant)
        .collect();
    variants.sort_by_key(|v| ShaderVariant::ALL.iter().position(|a| a == v));
    variants
}

#[test]
fn identical_shaders_share_one_compile_and_one_pipeline_set() {
    let Some((device, mut renderer)) = rig() else {
        return;
    };
    let before = renderer.pipeline_stats();
    let a = renderer.create_shader(&device, "a.wgsl", RED).unwrap();
    let b = renderer.create_shader(&device, "b.wgsl", RED).unwrap();
    assert_ne!(a, b, "each shader keeps its own id");
    assert_eq!(pipelines(&renderer, a), pipelines(&renderer, b), "built once, shared");
    let after = renderer.pipeline_stats();
    assert_eq!(after.compiles - before.compiles, 1, "the second compiled nothing");
    assert_eq!(after.shared - before.shared, 1);
    assert_eq!(after.material_shaders - before.material_shaders, 1);
    // The plain one- and double-sided sets, with 4x multisampling's two
    // alpha-to-coverage pipelines each.
    assert_eq!(after.variant_sets - before.variant_sets, 2);
    assert_eq!(after.pipelines - before.pipelines, 20);
    assert_eq!(after.key_bytes - before.key_bytes, RED.len());
}

#[test]
fn a_reload_invalidates_only_the_reloaded_shaders_variants() {
    let Some((device, mut renderer)) = rig() else {
        return;
    };
    let red = renderer.create_shader(&device, "red.wgsl", RED).unwrap();
    let blue = renderer.create_shader(&device, "blue.wgsl", BLUE).unwrap();
    // Each gets a crossfade variant built (as a frame drawing one would).
    let fade = ShaderVariant::new(Sides::One, Instancing::Crossfade);
    let red_fade = renderer.shader_pipeline(red, fade, PipelineRole::Opaque).cloned();
    renderer.shader_pipeline(blue, fade, PipelineRole::Opaque);
    let (red_before, before) = (pipelines(&renderer, red), renderer.pipeline_stats());
    assert_eq!(keys_of(&renderer, BLUE).len(), 3);

    renderer
        .replace_shader(
            &device,
            blue,
            "blue.wgsl",
            RED.replace("1.0, 0.0, 0.0", "0.0, 1.0, 0.0").as_str(),
        )
        .unwrap();
    let after = renderer.pipeline_stats();
    assert_eq!(
        after.invalidated - before.invalidated,
        3,
        "blue's three built sets, nothing else"
    );
    assert!(keys_of(&renderer, BLUE).is_empty(), "the old code's keys are gone");
    assert_eq!(keys_of(&renderer, RED).len(), 3, "red keeps its sets, crossfade too");
    assert_eq!(pipelines(&renderer, red), red_before);
    assert_eq!(
        renderer.shader_pipeline(red, fade, PipelineRole::Opaque).cloned(),
        red_fade,
        "red's lazily built variant survives"
    );
    // The reloaded shader starts with only its plain sets again.
    assert!(!renderer.shader_warm(blue));
    assert_builds_add_up(&after);
}

#[test]
fn reloading_one_of_two_sharing_shaders_leaves_the_other_its_pipelines() {
    let Some((device, mut renderer)) = rig() else {
        return;
    };
    let a = renderer.create_shader(&device, "a.wgsl", RED).unwrap();
    let b = renderer.create_shader(&device, "b.wgsl", RED).unwrap();
    let shared = pipelines(&renderer, b);
    let before = renderer.pipeline_stats();
    renderer.replace_shader(&device, a, "a.wgsl", BLUE).unwrap();
    assert_eq!(pipelines(&renderer, b), shared, "b still draws with the shared set");
    assert_ne!(pipelines(&renderer, a), shared);
    let after = renderer.pipeline_stats();
    assert_eq!(
        after.invalidated, before.invalidated,
        "b holds the set: nothing dropped"
    );
    // Reloading back to code another shader uses compiles nothing.
    renderer.replace_shader(&device, a, "a.wgsl", RED).unwrap();
    assert_eq!(pipelines(&renderer, a), shared);
    let back = renderer.pipeline_stats();
    assert_eq!(back.compiles, after.compiles);
    assert_eq!(back.shared, after.shared + 1);
    assert_eq!(back.invalidated, after.invalidated + 2, "BLUE's plain sets went");
}

#[test]
fn failed_reloads_invalidate_nothing_and_are_timed() {
    let Some((device, mut renderer)) = rig() else {
        return;
    };
    let blue = renderer.create_shader(&device, "blue.wgsl", BLUE).unwrap();
    let before = renderer.pipeline_stats();
    renderer.replace_shader(&device, blue, "blue.wgsl", BROKEN).unwrap_err();
    renderer
        .replace_shader(&device, blue, "blue.wgsl", BAD_SAMPLING)
        .unwrap_err();
    let after = renderer.pipeline_stats();
    assert_eq!(after.invalidated, before.invalidated);
    assert_eq!(after.compiles, before.compiles + 2, "failed compiles count");
    assert_eq!(
        after.material_shaders, before.material_shaders,
        "rejected code isn't kept"
    );
    assert_eq!(
        after.builds, before.builds,
        "rejected code's sets aren't counted as builds"
    );
    assert_eq!(after.build_ms, before.build_ms);
    assert_builds_add_up(&after);
    // Warming then builds only sets in use, and the identity still holds.
    renderer.warm_pipelines();
    let warm = renderer.pipeline_stats();
    assert!(warm.builds > after.builds);
    assert_builds_add_up(&warm);
    assert!(after.last_compile_ms > 0.0 && after.slowest_compile_ms >= after.last_compile_ms);
}

#[test]
fn freeing_a_shader_lets_go_of_its_pipelines() {
    let Some((device, mut renderer)) = rig() else {
        return;
    };
    let blue = renderer.create_shader(&device, "blue.wgsl", BLUE).unwrap();
    let before = renderer.pipeline_stats();
    renderer.free_shader(blue);
    let after = renderer.pipeline_stats();
    assert_eq!(after.material_shaders, before.material_shaders - 1);
    assert_eq!(after.invalidated, before.invalidated + 2);
}

/// Creating, sharing and reloading material shaders, and building a
/// variant on first use, timed on this adapter (`--nocapture` prints them).
#[test]
fn pipeline_creation_and_reload_stalls_are_measured() {
    let Some((device, mut renderer)) = rig() else {
        return;
    };
    let before = renderer.pipeline_stats();
    let start = std::time::Instant::now();
    let id = renderer.create_shader(&device, "red.wgsl", RED).unwrap();
    let create = start.elapsed();
    let created = renderer.pipeline_stats();
    let start = std::time::Instant::now();
    renderer.create_shader(&device, "red again.wgsl", RED).unwrap();
    let shared = start.elapsed();
    let again = renderer.pipeline_stats();
    let start = std::time::Instant::now();
    renderer.replace_shader(&device, id, "red.wgsl", BLUE).unwrap();
    let reload = start.elapsed();
    let start = std::time::Instant::now();
    renderer.shader_pipeline(
        id,
        ShaderVariant::new(Sides::Both, Instancing::Shared),
        PipelineRole::Opaque,
    );
    let lazy = start.elapsed();
    let s = renderer.pipeline_stats();
    eprintln!(
        "create {:.2} ms, identical create {:.3} ms, reload {:.2} ms, variant on first use {:.2} ms; \
         {} shaders, {} sets, {} pipelines, {} key bytes; builds {} in {:.2} ms (slowest {:.2} ms)",
        create.as_secs_f64() * 1e3,
        shared.as_secs_f64() * 1e3,
        reload.as_secs_f64() * 1e3,
        lazy.as_secs_f64() * 1e3,
        s.material_shaders,
        s.variant_sets,
        s.pipelines,
        s.key_bytes,
        s.builds,
        s.build_ms,
        s.slowest_build_ms,
    );
    assert!(s.builds >= 7 && s.slowest_build_ms > 0.0);
    // Sharing skips the compile: counted, not timed (two wall-clock times
    // compare unreliably on a busy machine).
    assert_eq!(created.compiles, before.compiles + 1, "a new shader is compiled");
    assert_eq!(again.compiles, created.compiles, "sharing skips the compile");
    assert_eq!(
        again.shared,
        created.shared + 1,
        "the identical shader shares the pipelines"
    );
    assert_eq!(again.builds, created.builds, "and builds nothing");
}

/// Every set built is either still in use or was invalidated.
fn assert_builds_add_up(s: &crate::render::PipelineStats) {
    assert_eq!(s.builds, s.variant_sets as u64 + s.invalidated, "{s:?}");
}

#[test]
fn reloading_unchanged_code_keeps_its_set_and_shares_nothing() {
    let Some((device, mut renderer)) = rig() else {
        return;
    };
    let red = renderer.create_shader(&device, "red.wgsl", RED).unwrap();
    let (kept, before) = (pipelines(&renderer, red), renderer.pipeline_stats());
    renderer.replace_shader(&device, red, "red.wgsl", RED).unwrap();
    let after = renderer.pipeline_stats();
    assert_eq!(pipelines(&renderer, red), kept, "its own set, untouched");
    assert_eq!(after.shared, before.shared, "its own set is not shared");
    assert_eq!(
        (after.compiles, after.builds, after.invalidated),
        (before.compiles, before.builds, before.invalidated)
    );
    assert_builds_add_up(&after);
    // A second shader with that code does share it.
    renderer.create_shader(&device, "red again.wgsl", RED).unwrap();
    assert_eq!(renderer.pipeline_stats().shared, before.shared + 1);
}

const FX_GREY: &str = "fn effect(p: Pixel) -> vec3<f32> {\n    return vec3<f32>(dot(p.color, vec3<f32>(0.3)));\n}\n";
/// Line 2 names something that doesn't exist.
const FX_BROKEN: &str = "fn effect(p: Pixel) -> vec3<f32> {\n    return p.color * nope;\n}\n";
const FX_UNBOUND: &str = "@group(3) @binding(7) var<uniform> extra: vec4<f32>;\nfn effect(p: Pixel) -> vec3<f32> { return p.color * extra.xyz; }\n";
const CS_FADE: &str = "fn compute(c: Cell) -> vec4<f32> {\n    return c.value * 0.5;\n}\n";
/// Line 3 returns a number, not a `vec4<f32>`.
const CS_BROKEN: &str = "\nfn compute(c: Cell) -> vec4<f32> {\n    return 1.0;\n}\n";
const CS_UNBOUND: &str = "@group(3) @binding(7) var<uniform> extra: vec4<f32>;\nfn compute(c: Cell) -> vec4<f32> { return c.value * extra; }\n";

#[test]
fn effect_and_compute_errors_have_a_place_and_failed_reloads_keep_the_last_good_pipeline() {
    let Some((device, mut renderer)) = rig() else {
        return;
    };
    // Both reload a compute pass too.
    if !crate::render::render_test_support::needs(&device, crate::render::GpuFeature::Compute) {
        return;
    }
    let fx = renderer.create_effect(&device, "grey.wgsl", FX_GREY).unwrap();
    let cs = renderer.create_compute(&device, "fade.wgsl", CS_FADE).unwrap();
    let (fx_before, cs_before) = (
        renderer.effects.pipelines[fx as usize].clone(),
        renderer.compute_pipelines[cs as usize].clone(),
    );

    let e = renderer
        .replace_effect(&device, fx, "grey.wgsl", FX_BROKEN)
        .unwrap_err();
    assert_eq!((e.shader.as_str(), e.kind), ("grey.wgsl", ShaderKind::Effect));
    assert_eq!((e.line, e.in_engine_code), (Some(2), false), "{e}");
    assert_eq!(e.variant, EFFECT_VARIANT);
    let e = renderer
        .replace_compute(&device, cs, "fade.wgsl", CS_BROKEN)
        .unwrap_err();
    assert_eq!(
        (e.line, e.kind, e.in_engine_code),
        (Some(3), ShaderKind::Compute, false),
        "{e}"
    );

    // Valid WGSL the GPU rejects: an error, not a broken device.
    let e = renderer
        .replace_effect(&device, fx, "grey.wgsl", FX_UNBOUND)
        .unwrap_err();
    assert!(e.message.contains("the GPU rejected its pipeline"), "{e}");
    let e = renderer
        .replace_compute(&device, cs, "fade.wgsl", CS_UNBOUND)
        .unwrap_err();
    assert!(e.message.contains("the GPU rejected its pipeline"), "{e}");
    let e = renderer.create_effect(&device, "new.wgsl", FX_UNBOUND).unwrap_err();
    assert_eq!(e.kind, ShaderKind::Effect);
    assert_eq!(renderer.effects.pipelines.len(), 1, "a rejected new effect takes no id");

    assert_eq!(
        renderer.effects.pipelines[fx as usize], fx_before,
        "the last good effect stays"
    );
    assert_eq!(
        renderer.compute_pipelines[cs as usize], cs_before,
        "the last good pass stays"
    );

    renderer.replace_effect(&device, fx, "grey.wgsl", FX_GREY).unwrap();
    renderer.replace_compute(&device, cs, "fade.wgsl", CS_FADE).unwrap();
    assert_ne!(renderer.effects.pipelines[fx as usize], fx_before, "a fix takes effect");
    assert_ne!(renderer.compute_pipelines[cs as usize], cs_before);
    assert_eq!(renderer.pending_shader_checks(), 0, "native GPUs answer at once");
}

/// A browser's WebGPU answers error scopes later: the new pipelines are in
/// use meanwhile, and a rejection puts the previous ones back.
#[test]
fn late_gpu_rejections_put_the_previous_pipelines_back_and_are_reported() {
    let Some((device, mut renderer)) = rig() else {
        return;
    };
    // Both reload a compute pass too.
    if !crate::render::render_test_support::needs(&device, crate::render::GpuFeature::Compute) {
        return;
    }
    let red = renderer.create_shader(&device, "red.wgsl", RED).unwrap();
    let other = renderer.create_shader(&device, "blue.wgsl", BLUE).unwrap();
    let fx = renderer.create_effect(&device, "grey.wgsl", FX_GREY).unwrap();
    let cs = renderer.create_compute(&device, "fade.wgsl", CS_FADE).unwrap();
    let (red_before, other_before) = (pipelines(&renderer, red), pipelines(&renderer, other));
    let fx_before = renderer.effects.pipelines[fx as usize].clone();
    let cs_before = renderer.compute_pipelines[cs as usize].clone();
    renderer.defer_checks = true;

    // Each is put in use before the verdict comes back.
    renderer.replace_shader(&device, red, "red.wgsl", BAD_SAMPLING).unwrap();
    renderer.replace_effect(&device, fx, "grey.wgsl", FX_UNBOUND).unwrap();
    renderer.replace_compute(&device, cs, "fade.wgsl", CS_UNBOUND).unwrap();
    let new_effect = renderer.create_effect(&device, "new fx.wgsl", FX_UNBOUND).unwrap();
    let new_shader = renderer
        .create_shader(&device, "new.wgsl", &(BAD_SAMPLING.to_string() + "\n// another module"))
        .unwrap();
    // A fine reload (of new code, so it compiles) waits too, and passes.
    let green = RED.replace("1.0, 0.0, 0.0", "0.0, 1.0, 0.0");
    renderer.replace_shader(&device, other, "blue.wgsl", &green).unwrap();
    assert_ne!(pipelines(&renderer, red), red_before, "in use while waiting");
    assert_eq!(renderer.pending_shader_checks(), 6);

    let mut late = Vec::new();
    renderer.poll_shader_checks(&device, &mut late);
    assert_eq!(renderer.pending_shader_checks(), 0);
    let mut got: Vec<(String, ShaderKind, bool)> = late
        .iter()
        .map(|e| (e.diagnostic.shader.clone(), e.diagnostic.kind, e.reload))
        .collect();
    got.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        got,
        [
            ("fade.wgsl".into(), ShaderKind::Compute, true),
            ("grey.wgsl".into(), ShaderKind::Effect, true),
            ("new fx.wgsl".into(), ShaderKind::Effect, false),
            ("new.wgsl".into(), ShaderKind::Material, false),
            ("red.wgsl".into(), ShaderKind::Material, true),
        ]
    );
    assert!(late.iter().all(|e| e.diagnostic.message.contains("the GPU rejected")));

    assert_eq!(pipelines(&renderer, red), red_before, "the previous version is back");
    assert_eq!(renderer.effects.pipelines[fx as usize], fx_before);
    assert_eq!(renderer.compute_pipelines[cs as usize], cs_before);
    assert_eq!(
        pipelines(&renderer, new_shader),
        pipelines(&renderer, 0),
        "a new one draws as the built-in"
    );
    assert_ne!(
        renderer.effects.pipelines[new_effect as usize], fx_before,
        "a new effect gets a stand-in"
    );
    assert_ne!(pipelines(&renderer, other), other_before, "the good reload stays");
}

#[test]
fn a_late_rejection_behind_a_newer_reload_falls_back_to_the_last_good_version() {
    let Some((device, mut renderer)) = rig() else {
        return;
    };
    let red = renderer.create_shader(&device, "red.wgsl", RED).unwrap();
    let good = pipelines(&renderer, red);
    renderer.defer_checks = true;
    renderer.replace_shader(&device, red, "red.wgsl", BAD_SAMPLING).unwrap();
    let unbound2 = BAD_SAMPLING.to_string() + "\n// another module";
    renderer.replace_shader(&device, red, "red.wgsl", &unbound2).unwrap();
    let mut late = Vec::new();
    renderer.poll_shader_checks(&device, &mut late);
    assert_eq!(late.len(), 2);
    assert_eq!(
        pipelines(&renderer, red),
        good,
        "both rejected: the first good version is back"
    );
    let flags: Vec<(bool, bool)> = late.iter().map(|e| (e.reload, e.superseded)).collect();
    assert_eq!(
        flags,
        [(true, true), (true, false)],
        "the older one was replaced before its verdict"
    );
    // What `shader_status` makes of it: both reloads undone, one failure.
    let status = statuses_after("red.wgsl", 2, late);
    assert_eq!(status.outcome, crate::render::ShaderOutcome::ReloadFailed);
    assert_eq!(status.reloads, 0);
    assert_eq!(status.diagnostics.len(), 1, "{:?}", status.diagnostics);
}

/// The status of material shader `name` made, then reloaded `reloads`
/// times (each recorded as it is put in use), after the late rejections
/// `late` from the renderer.
fn statuses_after(name: &str, reloads: u32, late: Vec<LateShaderError>) -> crate::render::ShaderStatus {
    let mut out = None;
    crate::game::Game::new("t", 1, 1).run_headless(0, |ctx| {
        ctx.gfx.create_shader(name, RED);
        for _ in 0..reloads {
            ctx.gfx.record_shader(name, ShaderKind::Material, true, Ok(())).unwrap();
        }
        for e in late {
            ctx.gfx.record_rejected(e.reload, e.superseded, *e.diagnostic);
        }
        out = ctx.gfx.shader_status().first().cloned();
    });
    out.unwrap()
}

#[test]
fn a_late_rejection_of_an_older_reload_leaves_the_newer_one_that_passed() {
    let Some((device, mut renderer)) = rig() else {
        return;
    };
    let red = renderer.create_shader(&device, "red.wgsl", RED).unwrap();
    renderer.defer_checks = true;
    renderer.replace_shader(&device, red, "red.wgsl", BAD_SAMPLING).unwrap();
    renderer.replace_shader(&device, red, "red.wgsl", BLUE).unwrap();
    let blue = pipelines(&renderer, red);
    let mut late = Vec::new();
    renderer.poll_shader_checks(&device, &mut late);
    assert_eq!(late.len(), 1);
    assert!(late[0].superseded);
    assert_eq!(pipelines(&renderer, red), blue, "the newer code stays");
    let status = statuses_after("red.wgsl", 2, late);
    assert_eq!(
        status.outcome,
        crate::render::ShaderOutcome::Reloaded,
        "{:?}",
        status.diagnostics
    );
    assert_eq!(status.reloads, 1, "only the reload that took effect counts");
    assert!(status.diagnostics.is_empty());
}

#[test]
fn a_late_rejection_handed_down_also_frees_shaders_sharing_the_rejected_code() {
    let Some((device, mut renderer)) = rig() else {
        return;
    };
    let red = renderer.create_shader(&device, "red.wgsl", RED).unwrap();
    renderer.defer_checks = true;
    renderer.replace_shader(&device, red, "red.wgsl", BAD_SAMPLING).unwrap();
    // Made from the same code: shares the waiting pipelines, no check of its own.
    let twin = renderer.create_shader(&device, "twin.wgsl", BAD_SAMPLING).unwrap();
    assert_eq!(pipelines(&renderer, twin), pipelines(&renderer, red));
    // A newer reload, also waiting when the older verdict comes (it passes).
    renderer.replace_shader(&device, red, "red.wgsl", BLUE).unwrap();
    assert_eq!(renderer.pending_shader_checks(), 2);
    let blue = pipelines(&renderer, red);
    let mut late = Vec::new();
    renderer.poll_shader_checks(&device, &mut late);
    assert_eq!(late.len(), 1);
    assert_eq!(
        pipelines(&renderer, twin),
        pipelines(&renderer, 0),
        "the twin draws as the built-in"
    );
    assert_eq!(pipelines(&renderer, red), blue, "the newer code stays");
}

#[test]
fn late_rejection_after_slot_reuse_still_reaches_shaders_sharing_that_code() {
    let Some((device, mut renderer)) = rig() else {
        return;
    };
    renderer.defer_checks = true;
    let old = renderer.create_shader(&device, "old.wgsl", BAD_SAMPLING).unwrap();
    let twin = renderer.create_shader(&device, "twin.wgsl", BAD_SAMPLING).unwrap();
    assert_eq!(pipelines(&renderer, twin), pipelines(&renderer, old));
    assert_eq!(renderer.pending_shader_checks(), 1);

    renderer.free_shader(old);
    renderer
        .create_shader_at(&device, old, "replacement.wgsl", BLUE)
        .unwrap();
    let replacement = pipelines(&renderer, old);
    renderer.poll_shader_checks(&device, &mut Vec::new());
    assert_eq!(pipelines(&renderer, twin), pipelines(&renderer, 0));
    assert_eq!(pipelines(&renderer, old), replacement);
}

/// What `Ctx` does when a shader file fails at first load (a stand-in,
/// and for a compute pass, its pipeline freed) and a saved fix reloads it.
#[test]
fn first_load_stand_ins_are_replaced_by_the_fixed_reload() {
    let Some((device, mut renderer)) = rig() else {
        return;
    };
    let shader = renderer.create_shader(&device, "m.wgsl", "").unwrap();
    assert_eq!(
        pipelines(&renderer, shader),
        pipelines(&renderer, 0),
        "the built-in look"
    );
    let fx = renderer
        .create_effect(&device, "fx.wgsl", crate::three::EFFECT_STAND_IN)
        .unwrap();
    let fx_stand_in = renderer.effects.pipelines[fx as usize].clone();

    renderer.replace_shader(&device, shader, "m.wgsl", RED).unwrap();
    renderer.replace_effect(&device, fx, "fx.wgsl", FX_GREY).unwrap();
    assert_ne!(pipelines(&renderer, shader), pipelines(&renderer, 0));
    assert_ne!(renderer.effects.pipelines[fx as usize], fx_stand_in);

    // The compute pass, where the device has compute shaders.
    if !crate::render::render_test_support::needs(&device, crate::render::GpuFeature::Compute) {
        return;
    }
    let cs = renderer
        .create_compute(&device, "cs.wgsl", crate::three::COMPUTE_STAND_IN)
        .unwrap();
    renderer.free_compute(cs);
    assert!(renderer.compute_pipelines[cs as usize].is_none(), "skipped until fixed");
    renderer.replace_compute(&device, cs, "cs.wgsl", CS_FADE).unwrap();
    assert!(
        renderer.compute_pipelines[cs as usize].is_some(),
        "the fix fills the slot"
    );
}

#[test]
fn sample_changes_wait_for_pending_shader_reloads_to_settle() {
    let Some((device, mut renderer)) = rig() else {
        return;
    };
    let id = renderer.create_shader(&device, "pending.wgsl", RED).unwrap();
    renderer.defer_checks = true;
    renderer
        .replace_shader(&device, id, "pending.wgsl", BAD_SAMPLING)
        .unwrap();
    assert!(
        !renderer.set_samples(&device, 2),
        "a late verdict could restore old-count pipelines"
    );
    assert_eq!(renderer.samples, 4);
    let mut errors = Vec::new();
    renderer.poll_shader_checks(&device, &mut errors);
    assert_eq!(errors.len(), 1);
    // A no-op remains valid after the rejected reload; changing counts is
    // covered by the attachment/pixel test on adapters that support 2x.
    assert!(renderer.set_samples(&device, 4));
}
