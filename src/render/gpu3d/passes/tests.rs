use super::*;

fn pass(name: &'static str, after: &'static [&'static str]) -> Pass {
    Pass {
        name,
        after,
        when: When::ALWAYS,
        steps: &[],
    }
}

/// A pass with `steps`, in every frame.
fn with(name: &'static str, after: &'static [&'static str], steps: &'static [Step]) -> Pass {
    Pass {
        steps,
        ..pass(name, after)
    }
}

/// The parts `draw` builds for a frame with `features`, with `shadows`
/// shadow parts when it has any.
fn order_for(features: Features, shadows: usize) -> Vec<usize> {
    let mut order = vec![PRELUDE];
    if features.contains(F::SHADOWS) {
        order.extend(std::iter::repeat_n(SHADOW, shadows));
    }
    if features.contains(F::VIEWS) {
        order.push(VIEWS);
    }
    if features.contains(F::GI) {
        order.push(GI);
    }
    order.extend([PREPASS, OPAQUE]);
    if features.contains(F::SPLIT) {
        order.push(TRANSPARENT);
    }
    order.push(POST);
    order
}

fn check(table: &[Pass], order: &[usize], features: Features) -> Result<(), PassError> {
    check_frame(table, order.iter().copied(), features)
}

#[test]
fn frame_passes_are_consistent() {
    check_passes(&FRAME_PASSES).unwrap();
}

#[test]
fn frame_pass_indices_match_their_names() {
    for (index, name) in [
        (PRELUDE, "3d prelude"),
        (SHADOW, "3d shadows"),
        (VIEWS, "3d target cameras"),
        (GI, "3d global illumination"),
        (PREPASS, "3d depth prepass"),
        (OPAQUE, "3d opaque"),
        (TRANSPARENT, "3d transparent"),
        (POST, "3d post"),
    ] {
        assert_eq!(FRAME_PASSES[index].name, name);
    }
}

#[test]
fn resources_and_features_are_listed_once() {
    for (i, resource) in Resource::ALL.into_iter().enumerate() {
        assert_eq!(resource as usize, i, "{resource:?} is out of place in Resource::ALL");
        assert_eq!(
            Resource::ALL.iter().filter(|r| r.name() == resource.name()).count(),
            1,
            "two resources are named {}",
            resource.name()
        );
    }
    let mut all = Features::NONE;
    for &(flag, name) in Features::NAMES {
        assert!(!all.intersects(flag), "{name} shares its bit with another feature");
        all = all.with(flag);
    }
    assert_eq!(all.flags().count(), Features::NAMES.len());
    // A pass's recorded steps are a bit each.
    assert!(FRAME_PASSES.iter().all(|p| p.steps.len() <= 64));
}

#[test]
fn every_frame_order_draw_builds_is_valid() {
    // Every feature on its own and every pair of them, with what they
    // imply: `draw` records the prelude, prepass, opaque and post, and the
    // other passes when the frame's features call for them.
    let flags: Vec<Features> = Features::NAMES.iter().map(|&(flag, _)| flag).collect();
    let mut frames = 0;
    for (i, &a) in flags.iter().enumerate() {
        for &b in &flags[i..] {
            let features = a.with(b).closed();
            if features.check().is_err() {
                continue; // two kinds of antialiasing
            }
            for shadows in [1, 3] {
                let order = order_for(features, shadows);
                if let Err(error) = check(&FRAME_PASSES, &order, features) {
                    panic!("a frame with {features}: {error}");
                }
            }
            frames += 1;
        }
    }
    assert!(frames > 400);
    check(&FRAME_PASSES, &order_for(Features::NONE, 0), Features::NONE).unwrap();
}

#[test]
fn frames_have_the_passes_their_features_call_for() {
    let split = F::SPLIT;
    let error = check(&FRAME_PASSES, &[PRELUDE, PREPASS, OPAQUE, POST], split).unwrap_err();
    assert_eq!(
        error,
        PassError::MissingPass {
            pass: "3d transparent",
            features: split,
        }
    );
    assert_eq!(
        error.to_string(),
        "pass \"3d transparent\" is missing from a frame that calls for it \
         (the frame has scene-reading transparents)"
    );
    let error = check(&FRAME_PASSES, &[PRELUDE, GI, PREPASS, OPAQUE, POST], Features::NONE).unwrap_err();
    assert_eq!(
        error.to_string(),
        "pass \"3d global illumination\" is in a frame that doesn't call for it (the frame has nothing)"
    );
}

#[test]
fn frame_features_must_be_complete_and_compatible() {
    let error = check(&FRAME_PASSES, &order_for(F::TAA, 0), F::TAA).unwrap_err();
    assert_eq!(
        error,
        PassError::MissingFeature {
            flag: F::TAA,
            needs: F::VELOCITY,
        }
    );
    assert_eq!(
        error.to_string(),
        "a frame with temporal antialiasing must also have motion vectors"
    );
    let both = F::FXAA.with(F::SMAA);
    assert_eq!(
        both.check().unwrap_err().to_string(),
        "a frame can't have both fxaa and smaa"
    );
    // Closing adds what a feature implies, through two levels.
    let closed = F::TARGET_CUTOUTS.closed();
    assert!(closed.contains(F::TARGET_CAMERAS.with(F::VIEWS).with(F::MSAA)));
    closed.check().unwrap();
    assert!(F::AO_TEMPORAL.closed().contains(F::AO.with(F::VELOCITY)));
    assert!(F::SSR_LEVELS.closed().contains(F::SSR));
    assert!(F::GI_UPDATE.closed().contains(F::GI));
}

#[test]
fn frame_passes_are_described() {
    let text = describe(&FRAME_PASSES);
    let steps: usize = FRAME_PASSES.iter().map(|p| p.steps.len()).sum();
    assert_eq!(text.lines().count(), FRAME_PASSES.len() + steps);
    assert!(text.contains("3d transparent: after [3d opaque] in a frame with scene-reading transparents\n"));
    assert!(text.contains(
        "  scene copy in any frame: taa input as copy source in a frame with temporal antialiasing; \
         lit frame as copy source in a frame without temporal antialiasing; scene copy as copy destination\n"
    ));
}

#[test]
fn a_new_pass_slots_in_with_declared_dependencies() {
    // A pass that reads the opaque scene and writes its own target, for
    // the post pass to read.
    const WRITES_CAMERAS: &[Step] = &[step(
        "custom",
        When::ALWAYS,
        &[
            u(Hdr, A::Sample).off(F::TAA),
            u(SceneDepth, A::SampleMs),
            u(TargetImage, A::Color),
        ],
    )];
    const WRITES_OUTPUT: &[Step] = &[step(
        "custom",
        When::ALWAYS,
        &[
            u(Hdr, A::Sample).off(F::TAA),
            u(SceneDepth, A::SampleMs),
            u(Output, A::Color),
        ],
    )];
    let mut table = FRAME_PASSES.to_vec();
    table.push(with("3d custom", &["3d opaque", "3d transparent"], WRITES_CAMERAS));
    let custom = table.len() - 1;
    // Writing target cameras after a pass that reads them (the opaque
    // pass) is caught. The probe captures read what their own pass drew
    // just before, in every frame: they read this frame's images.
    let error = check_passes(&table).unwrap_err();
    assert_eq!(
        error,
        PassError::UndeclaredRead {
            pass: "3d opaque",
            resource: TargetImage,
            writer: "3d custom",
        }
    );
    assert_eq!(
        error.to_string(),
        "pass \"3d opaque\" reads target camera image but doesn't run after \"3d custom\", \
         which writes it"
    );
    table[custom].steps = WRITES_OUTPUT;
    // `Output` also has the post pass as a writer: unordered, they clash.
    assert_eq!(
        check_passes(&table),
        Err(PassError::UnorderedWrites {
            resource: Output,
            first: "3d post",
            second: "3d custom",
        })
    );
    table[POST].after = &["3d opaque", "3d transparent", "3d custom"];
    check_passes(&table).unwrap();
    let features = F::MSAA;
    check(&table, &[PRELUDE, PREPASS, OPAQUE, custom, POST], features).unwrap();
    assert_eq!(
        check(&table, &[PRELUDE, PREPASS, custom, OPAQUE, POST], features),
        Err(PassError::OutOfOrder {
            pass: "3d custom",
            after: "3d opaque",
        })
    );
    // The frame's features call for it: leaving it out is caught too.
    assert_eq!(
        check(&table, &[PRELUDE, PREPASS, OPAQUE, POST], features),
        Err(PassError::MissingPass {
            pass: "3d custom",
            features,
        })
    );
}

#[test]
fn cycles_are_rejected_with_their_path() {
    let table = [pass("a", &["c"]), pass("b", &["a"]), pass("c", &["b"])];
    let error = check_passes(&table).unwrap_err();
    assert_eq!(error, PassError::Cycle(vec!["a", "c", "b"]));
    assert_eq!(
        error.to_string(),
        "pass dependencies form a cycle: \"a\" -> \"c\" -> \"b\" -> \"a\""
    );
    let table = [pass("a", &["a"])];
    assert_eq!(check_passes(&table), Err(PassError::Cycle(vec!["a"])));
}

#[test]
fn unknown_and_duplicate_passes_and_steps_are_rejected() {
    assert_eq!(
        check_passes(&[pass("a", &["missing"])]),
        Err(PassError::UnknownPass {
            pass: "a",
            after: "missing",
        })
    );
    assert_eq!(
        check_passes(&[pass("a", &[]), pass("a", &[])]),
        Err(PassError::DuplicateName("a"))
    );
    const TWICE: &[Step] = &[step("draw", When::ALWAYS, &[]), step("draw", When::ALWAYS, &[])];
    let error = check_passes(&[with("a", &[], TWICE)]).unwrap_err();
    assert_eq!(error.to_string(), "pass \"a\" has two steps named \"draw\"");
}

const WRITES_DEPTH: &[Step] = &[step("draw", When::ALWAYS, &[u(SceneDepth, A::Depth)])];
const READS_DEPTH: &[Step] = &[step("light", When::ALWAYS, &[u(SceneDepth, A::SampleMs)])];

#[test]
fn reads_must_be_ordered_against_their_writers() {
    let table = [with("depth", &[], WRITES_DEPTH), with("lighting", &[], READS_DEPTH)];
    let error = check_passes(&table).unwrap_err();
    assert_eq!(
        error.to_string(),
        "pass \"lighting\" reads scene depth but doesn't run after \"depth\", which writes it"
    );
    // What lasts a frame may be overwritten by a later pass once it was
    // read; what is kept across frames may not (the read would see the
    // frame before).
    const WRITES_SHADOWS: &[Step] = &[step("draw", When::ALWAYS, &[u(SunShadowMap, A::Depth)])];
    const READS_SHADOWS: &[Step] = &[step("light", When::ALWAYS, &[u(SunShadowMap, A::Sample)])];
    let table = [
        with("first", &[], WRITES_DEPTH),
        with("lighting", &["first"], READS_DEPTH),
        with("again", &["lighting"], WRITES_DEPTH),
    ];
    check_passes(&table).unwrap();
    let table = [
        with("lighting", &[], READS_SHADOWS),
        with("shadows", &["lighting"], WRITES_SHADOWS),
    ];
    assert_eq!(
        check_passes(&table),
        Err(PassError::UndeclaredRead {
            pass: "lighting",
            resource: SunShadowMap,
            writer: "shadows",
        })
    );
}

#[test]
fn writers_of_one_resource_must_be_ordered() {
    const WRITES: &[Step] = &[step("draw", When::ALWAYS, &[u(Hdr, A::Color)])];
    let table = [with("a", &[], WRITES), with("b", &[], WRITES)];
    assert_eq!(
        check_passes(&table),
        Err(PassError::UnorderedWrites {
            resource: Hdr,
            first: "a",
            second: "b",
        })
    );
    // Two writers that no frame has together don't clash.
    const WITH_TAA: &[Step] = &[step("draw", on(F::TAA), &[u(Hdr, A::Color)])];
    const WITH_FXAA: &[Step] = &[step("draw", on(F::FXAA), &[u(Hdr, A::Color)])];
    check_passes(&[with("a", &[], WITH_TAA), with("b", &[], WITH_FXAA)]).unwrap();
}

#[test]
fn history_must_be_read_before_it_is_overwritten() {
    const RESOLVES: &[Step] = &[step("resolve", When::ALWAYS, &[u(TaaHistory, A::Color)])];
    const REPROJECTS: &[Step] = &[step("reproject", When::ALWAYS, &[u(TaaHistory, A::SamplePrevious)])];
    let table = [
        with("resolve", &[], RESOLVES),
        with("reproject", &["resolve"], REPROJECTS),
    ];
    assert_eq!(
        check_passes(&table),
        Err(PassError::HistoryOverwritten {
            pass: "reproject",
            resource: TaaHistory,
            writer: "resolve",
        })
    );
    // Inside one pass too: a step can't read the history a step before it
    // overwrote. The step that overwrites it may read it (a pair of images).
    const WRONG: &[Step] = &[RESOLVES[0], REPROJECTS[0]];
    let error = check_passes(&[with("post", &[], WRONG)]).unwrap_err();
    assert_eq!(
        error.to_string(),
        "pass \"post\" reads last frame's taa history but \"post\", which overwrites it, doesn't run after it"
    );
    const PAIR: &[Step] = &[step(
        "resolve",
        When::ALWAYS,
        &[u(TaaHistory, A::SamplePrevious), u(TaaHistory, A::Color)],
    )];
    check_passes(&[with("post", &[], PAIR)]).unwrap();
}

/// The error of a table with one pass of one step using `uses`.
fn step_error(uses: &[Use]) -> String {
    let uses: &'static [Use] = Box::leak(uses.to_vec().into_boxed_slice());
    let steps: &'static [Step] = Box::leak(Box::new([step("draw", When::ALWAYS, uses)]));
    check_passes(&[with("pass", &[], steps)]).unwrap_err().to_string()
}

#[test]
fn a_step_must_use_resources_as_they_are_described() {
    assert_eq!(
        step_error(&[u(SceneDepth, A::Sample)]),
        "step \"draw\" of pass \"pass\" uses scene depth as a sampled texture, but it is multisampled"
    );
    assert_eq!(
        step_error(&[u(Hdr, A::SampleMs)]),
        "step \"draw\" of pass \"pass\" uses lit frame as a multisampled texture, but it has one sample"
    );
    assert_eq!(
        step_error(&[u(Hdr, A::Depth)]),
        "step \"draw\" of pass \"pass\" uses lit frame as a depth attachment, but it isn't a depth texture"
    );
    assert_eq!(
        step_error(&[u(SceneDepth, A::Color)]),
        "step \"draw\" of pass \"pass\" uses scene depth as a color attachment, but it is a depth texture"
    );
    assert_eq!(
        step_error(&[u(VisibleDraws, A::Sample)]),
        "step \"draw\" of pass \"pass\" uses visible static draws as a sampled texture, but it is a buffer"
    );
    assert_eq!(
        step_error(&[u(Hdr, A::SamplePrevious)]),
        "step \"draw\" of pass \"pass\" uses lit frame as a last frame's texture, but it isn't kept across frames"
    );
    // Without multisampling the frame's depth has one sample, and may be
    // sampled as such.
    const SINGLE: &[Step] = &[
        WRITES_DEPTH[0],
        step("light", When::ALWAYS, &[u(SceneDepth, A::Sample).off(F::MSAA)]),
    ];
    check_passes(&[with("pass", &[], SINGLE)]).unwrap();
}

#[test]
fn attachments_of_a_step_must_agree_on_the_sample_count() {
    assert_eq!(
        step_error(&[u(Hdr, A::Color), u(SceneDepth, A::DepthLoad)]),
        "step \"draw\" of pass \"pass\" uses lit frame (one sample) together with scene depth (multisampled)"
    );
    // The same pair, in frames without multisampling only.
    const SINGLE: &[Step] = &[step(
        "draw",
        When::ALWAYS,
        &[u(Hdr, A::Color).off(F::MSAA), u(SceneDepth, A::Depth)],
    )];
    check_passes(&[with("pass", &[], SINGLE)]).unwrap();
}

#[test]
fn a_resolve_needs_a_multisampled_source_of_its_format() {
    assert_eq!(
        step_error(&[u(Hdr, A::Resolve)]),
        "step \"draw\" of pass \"pass\" uses lit frame as a resolve target, \
         but the step has no multisampled color attachment"
    );
    assert_eq!(
        step_error(&[u(SceneColor, A::Resolve)]),
        "step \"draw\" of pass \"pass\" uses scene color as a resolve target, but it is multisampled"
    );
    assert_eq!(
        step_error(&[u(SceneColor, A::Color), u(Velocity, A::Resolve)]),
        "step \"draw\" of pass \"pass\" uses scene color (Hdr format) together with motion vectors (Velocity format)"
    );
    const RESOLVES: &[Step] = &[step(
        "draw",
        When::ALWAYS,
        &[u(SceneColor, A::Color), u(Hdr, A::Resolve)],
    )];
    check_passes(&[with("pass", &[], RESOLVES)]).unwrap();
}

#[test]
fn a_copy_needs_matching_sides() {
    assert_eq!(
        step_error(&[u(Velocity, A::CopyFrom), u(SceneCopy, A::CopyTo)]),
        "step \"draw\" of pass \"pass\" uses motion vectors (Velocity format) together with scene copy (Hdr format)"
    );
    assert_eq!(
        step_error(&[u(SceneColor, A::CopyFrom), u(SceneCopy, A::CopyTo)]),
        "step \"draw\" of pass \"pass\" uses scene color (multisampled) together with scene copy (one sample)"
    );
}

#[test]
fn a_step_cannot_sample_what_it_renders_to() {
    assert_eq!(
        step_error(&[u(Hdr, A::ColorLoad), u(Hdr, A::Sample)]),
        "step \"draw\" of pass \"pass\" renders to lit frame and samples it at the same time"
    );
    // Reading the depth while it is attached read-only is fine (the
    // transparent pass does).
    const READ_ONLY: &[Step] = &[
        WRITES_DEPTH[0],
        step(
            "blend",
            When::ALWAYS,
            &[u(SceneDepth, A::DepthRead), u(SceneDepth, A::SampleMs)],
        ),
    ];
    check_passes(&[with("pass", &[], READ_ONLY)]).unwrap();
}

#[test]
fn conditional_reads_need_a_writer_in_every_frame_they_happen_in() {
    // The copy is only made with a transparent surface that reads it, but
    // the effect reads it in every frame.
    const COPIES: &[Step] = &[
        step("draw", When::ALWAYS, &[u(Hdr, A::Color)]),
        step("copy", on(F::SPLIT), &[u(Hdr, A::CopyFrom), u(SceneCopy, A::CopyTo)]),
        step("effect", When::ALWAYS, &[u(SceneCopy, A::Sample)]),
    ];
    let error = check_passes(&[with("pass", &[], COPIES)]).unwrap_err();
    assert_eq!(
        error,
        PassError::Uncovered {
            pass: "pass",
            step: "effect",
            resource: SceneCopy,
            frames: When {
                all: Features::NONE,
                none: F::SPLIT,
            },
        }
    );
    assert_eq!(
        error.to_string(),
        "step \"effect\" of pass \"pass\" reads scene copy, which no earlier step writes in \
         a frame without scene-reading transparents"
    );
    // Writers that cover the frames between them are enough: one with
    // temporal antialiasing, one without.
    const EITHER: &[Step] = &[
        step("resolve", on(F::TAA), &[u(Hdr, A::Color)]),
        step("draw", When::ALWAYS, &[u(Hdr, A::Color).off(F::TAA)]),
        step("tone map", When::ALWAYS, &[u(Hdr, A::Sample)]),
    ];
    check_passes(&[with("pass", &[], EITHER)]).unwrap();
    // A feature's implications count: temporal antialiasing has motion vectors.
    const IMPLIED: &[Step] = &[
        step("motion", on(F::VELOCITY), &[u(Velocity, A::Color)]),
        step("resolve", on(F::TAA), &[u(Velocity, A::Sample)]),
    ];
    check_passes(&[with("pass", &[], IMPLIED)]).unwrap();
    // A writer in a pass that isn't in every frame doesn't cover a read
    // that is.
    const READS: &[Step] = &[step("light", When::ALWAYS, &[u(SceneDepth, A::SampleMs)])];
    let table = [
        Pass {
            when: on(F::GI),
            ..with("sometimes", &[], WRITES_DEPTH)
        },
        with("always", &["sometimes"], READS),
    ];
    assert_eq!(
        check_passes(&table).unwrap_err().to_string(),
        "step \"light\" of pass \"always\" reads scene depth, which no earlier step writes in \
         a frame without global illumination"
    );
}

#[test]
fn frames_reject_reads_of_unwritten_frame_resources() {
    // A frame's check sees the same, for the frame's own features.
    const STEPS: &[Step] = &[
        step("draw", on(F::SPLIT), &[u(SceneDepth, A::Depth)]),
        step("light", When::ALWAYS, &[u(SceneDepth, A::SampleMs)]),
    ];
    let table = [with("pass", &[], STEPS)];
    let error = check(&table, &[0], Features::NONE).unwrap_err();
    assert_eq!(
        error,
        PassError::ReadBeforeWrite {
            pass: "pass",
            step: "light",
            resource: SceneDepth,
        }
    );
    assert_eq!(
        error.to_string(),
        "step \"light\" of pass \"pass\" reads scene depth, which nothing earlier wrote this frame"
    );
    check(&table, &[0], F::SPLIT).unwrap();
    // Persistent resources may be read without a writer this frame: shadow
    // cascades kept from an earlier frame.
    check(&FRAME_PASSES, &[PRELUDE, PREPASS, OPAQUE, POST], Features::NONE).unwrap();
}

#[test]
fn frame_orders_reject_passes_before_their_dependencies() {
    let order = [PRELUDE, PREPASS, OPAQUE, POST, TRANSPARENT];
    assert_eq!(
        check(&FRAME_PASSES, &order, F::SPLIT),
        Err(PassError::OutOfOrder {
            pass: "3d post",
            after: "3d transparent",
        })
    );
}

#[test]
fn resources_that_share_memory_are_never_alive_together() {
    // The scene copy's texture is the screen effects' scratch: free once
    // the transparent surfaces are drawn. Every frame `draw` builds keeps
    // them apart (see `every_frame_order_draw_builds_is_valid`); a pass
    // that used the scratch before the transparent pass read the copy
    // would not.
    assert_eq!(EffectScratch.desc().shares, Some(SceneCopy));
    const EARLY: &[Step] = &[
        step(
            "scene copy",
            When::ALWAYS,
            &[u(Hdr, A::CopyFrom), u(SceneCopy, A::CopyTo)],
        ),
        step("blur", When::ALWAYS, &[u(Hdr, A::Sample), u(EffectScratch, A::Color)]),
        step("color", When::ALWAYS, &[u(SceneCopy, A::Sample)]),
    ];
    const DRAWS: &[Step] = &[step("draw", When::ALWAYS, &[u(Hdr, A::Color)])];
    let table = [with("opaque", &[], DRAWS), with("transparent", &["opaque"], EARLY)];
    check_passes(&table).unwrap();
    let error = check(&table, &[0, 1], Features::NONE).unwrap_err();
    assert_eq!(
        error,
        PassError::SharedOverlap {
            resource: EffectScratch,
            with: SceneCopy,
        }
    );
    assert_eq!(
        error.to_string(),
        "effect scratch and scene copy share memory but are both alive at one point of the frame"
    );
}

#[test]
fn transient_lifetimes_bound_the_textures_a_frame_needs() {
    // The full-size, single-sampled HDR images of a frame, and how many of
    // them hold something a later step still reads, at most.
    let images = [Hdr, TaaInput, SceneCopy, EffectScratch];
    let alive = |features: Features| {
        let features = features.closed();
        let order = order_for(features, 1);
        check(&FRAME_PASSES, &order, features).unwrap();
        alive_at_once(&FRAME_PASSES, &order, features, &images)
    };
    // The lit frame alone; with a copy for transparents or a scratch for
    // effects, two: the renderer's two textures (lit frame, scene copy).
    assert_eq!(alive(F::MSAA), 1);
    assert_eq!(alive(F::MSAA.with(F::SPLIT)), 2);
    assert_eq!(alive(F::MSAA.with(F::EFFECTS).with(F::EFFECTS_ODD)), 2);
    assert_eq!(alive(F::MSAA.with(F::SPLIT).with(F::EFFECTS).with(F::EFFECTS_ODD)), 2);
    // With temporal antialiasing its input is a third texture, but never
    // more than two are alive: the input's last reader (the resolve) comes
    // before the effects' scratch is first written.
    let taa = F::MSAA
        .with(F::TAA)
        .with(F::SPLIT)
        .with(F::EFFECTS)
        .with(F::EFFECTS_ODD);
    assert_eq!(alive(taa), 2);
    let order = order_for(taa.closed(), 1);
    let span = |r| span(&FRAME_PASSES, &order.iter().copied(), taa.closed(), r).unwrap();
    let place = |pass: usize, step: &str| {
        let part = order.iter().position(|&p| p == pass).unwrap();
        (
            part,
            FRAME_PASSES[pass].steps.iter().position(|s| s.name == step).unwrap(),
        )
    };
    assert_eq!(span(TaaInput), (place(OPAQUE, "color"), place(POST, "taa")));
    assert_eq!(
        span(SceneCopy),
        (place(TRANSPARENT, "scene copy"), place(TRANSPARENT, "color"))
    );
    assert_eq!(
        span(EffectScratch),
        (place(POST, "screen effect"), place(POST, "tone map"))
    );
    assert_eq!(span(Hdr), (place(POST, "taa"), place(POST, "screen effect")));
}

#[test]
fn a_part_must_record_the_steps_its_pass_declares() {
    let bit = |pass: usize, step: &str| 1u64 << FRAME_PASSES[pass].steps.iter().position(|s| s.name == step).unwrap();
    let depth = bit(PREPASS, "depth");
    check_recorded(&FRAME_PASSES, PREPASS, Features::NONE, depth).unwrap();
    let error = check_recorded(&FRAME_PASSES, PREPASS, F::AO, depth).unwrap_err();
    assert_eq!(
        error.to_string(),
        "pass \"3d depth prepass\" didn't record step \"ambient occlusion\", which it declares for this frame \
         (the frame has ambient occlusion)"
    );
    let error = check_recorded(
        &FRAME_PASSES,
        PREPASS,
        Features::NONE,
        depth | bit(PREPASS, "motion vectors"),
    )
    .unwrap_err();
    assert_eq!(
        error,
        PassError::UnexpectedStep {
            pass: "3d depth prepass",
            step: "motion vectors",
            features: Features::NONE,
        }
    );
    assert_eq!(
        error.to_string(),
        "pass \"3d depth prepass\" recorded step \"motion vectors\", which it doesn't declare for this frame \
         (the frame has nothing)"
    );
    // The occlusion grid is reduced after the prepass or after the opaque
    // pass, never both.
    let occlusion = F::OCCLUSION;
    let reduced = bit(OPAQUE, "color") | bit(OPAQUE, "occlusion depth");
    check_recorded(&FRAME_PASSES, OPAQUE, occlusion, reduced).unwrap();
    assert!(check_recorded(&FRAME_PASSES, OPAQUE, occlusion.with(F::SOLID_PREPASS), reduced).is_err());
}

#[test]
fn marks_are_noted_for_the_part_being_recorded() {
    // Outside a part, a mark does nothing.
    mark("depth");
    let outer = recording(PREPASS);
    mark("depth");
    // A part recorded while this one waits doesn't disturb it.
    let inner = recording(OPAQUE);
    mark("color");
    let opaque = recorded(inner);
    mark("motion vectors");
    let prepass = recorded(outer);
    check_recorded(&FRAME_PASSES, OPAQUE, Features::NONE, opaque).unwrap();
    check_recorded(&FRAME_PASSES, PREPASS, F::VELOCITY, prepass).unwrap();
    assert_eq!(recorded((usize::MAX, 0)), 0);
}

#[test]
#[should_panic(expected = "pass \"3d depth prepass\" recorded step \"bloom\", which it doesn't declare")]
fn a_mark_of_a_step_its_pass_does_not_have_panics() {
    let outer = recording(PREPASS);
    let _restore = Restore(outer);
    mark("bloom");
}

/// Puts this thread's recording state back, even on a panic.
struct Restore((usize, u64));
impl Drop for Restore {
    fn drop(&mut self) {
        recorded(self.0);
    }
}

#[test]
fn discarding_what_a_later_step_reads_is_caught() {
    let later = |resource, features: Features| read_later(&FRAME_PASSES, OPAQUE, "color", resource, features);
    // Nothing reads the depth after a plain frame's opaque pass, or the
    // multisampled color once it is resolved.
    assert_eq!(later(SceneDepth, F::MSAA), None);
    assert_eq!(later(SceneColor, F::MSAA), None);
    // The transparent pass draws over both.
    assert_eq!(later(SceneDepth, F::SPLIT), Some(("3d transparent", "color")));
    assert_eq!(
        later(SceneColor, F::MSAA.with(F::SPLIT)),
        Some(("3d transparent", "color"))
    );
    // Effects and temporal antialiasing read the depth.
    assert_eq!(later(SceneDepth, F::EFFECTS), Some(("3d post", "screen effect")));
    assert_eq!(later(SceneDepth, F::TAA.with(F::VELOCITY)), Some(("3d post", "taa")));
    // Without a solid prepass the occlusion grid is reduced after the
    // opaque pass, whatever else the frame does.
    let occlusion = F::OCCLUSION.with(F::AO);
    assert_eq!(later(SceneDepth, occlusion), Some(("3d opaque", "occlusion depth")));
    assert_eq!(later(SceneDepth, occlusion.with(F::SOLID_PREPASS)), None);
    // Without multisampling, the lit frame itself is the color attachment.
    assert_eq!(later(Hdr, Features::NONE), Some(("3d post", "tone map")));
    let error = check_discard(&FRAME_PASSES, OPAQUE, "color", SceneDepth, occlusion).unwrap_err();
    assert_eq!(
        error.to_string(),
        "step \"color\" of pass \"3d opaque\" discards scene depth, which step \"occlusion depth\" of pass \
         \"3d opaque\" reads later"
    );
    check_discard(&FRAME_PASSES, OPAQUE, "color", SceneDepth, F::MSAA).unwrap();
    // A step that overwrites it first ends its life: the opaque pass may
    // discard the cut-out prepass's color.
    assert_eq!(
        read_later(&FRAME_PASSES, PREPASS, "cut-outs", SceneColor, F::MSAA.with(F::CUTOUTS)),
        None
    );
}
