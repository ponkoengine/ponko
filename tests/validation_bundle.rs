//! The validation bundle example: a deliberate failure, the corrected run,
//! bounded step failures, secret and path redaction, and run comparison.
//!
//! The example's modules are compiled into this test, so the tests call the
//! same code `cargo run --example validation_bundle` runs.

#[path = "../examples/validation_bundle/bundle.rs"]
mod bundle;
#[path = "../examples/validation_bundle/identity.rs"]
mod identity;
#[path = "../examples/validation_bundle/launched.rs"]
mod launched;
#[path = "../examples/validation_bundle/orchestrate.rs"]
mod orchestrate;
#[path = "../examples/validation_bundle/reference.rs"]
mod reference;

use bundle::{CommandSpec, Redactor, compare_setups, sha256_hex};
use orchestrate::Options;
use reference::Variant;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A fresh, missing directory under the target directory.
fn out_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("validation-bundle-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn read(dir: &Path, rel: &str) -> String {
    std::fs::read_to_string(dir.join(rel)).unwrap()
}

/// Every file in the bundle, relative to `dir`, with `/` separators.
fn files(dir: &Path) -> Vec<String> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(&p, base, out);
            } else {
                out.push(p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

/// Every artifact the manifest lists exists and has the listed size and hash,
/// and the manifest lists every file but itself.
fn assert_artifacts_match(dir: &Path, manifest: &Value) {
    let listed: Vec<&str> = manifest["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["path"].as_str().unwrap())
        .collect();
    let mut on_disk = files(dir);
    on_disk.retain(|f| f != "manifest.json");
    assert_eq!(listed, on_disk);
    for a in manifest["artifacts"].as_array().unwrap() {
        let bytes = std::fs::read(dir.join(a["path"].as_str().unwrap())).unwrap();
        assert_eq!(a["bytes"], bytes.len());
        assert_eq!(a["sha256"], sha256_hex(&bytes), "{}", a["path"]);
    }
    let on_disk_manifest: Value = serde_json::from_str(&read(dir, "manifest.json")).unwrap();
    assert_eq!(&on_disk_manifest, manifest);
}

#[test]
fn sha256_matches_the_published_vectors() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    // 56 bytes: the padding spills into a second block.
    assert_eq!(
        sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
}

#[test]
fn the_buggy_run_writes_a_failing_bundle_that_points_at_the_tick() {
    let dir = out_dir("buggy");
    let report = orchestrate::run(&Options::new(&dir, Variant::Buggy)).unwrap();
    let m = &report.manifest;
    assert!(!report.passed);
    assert_eq!(
        (m["schema"].as_str(), m["schema_version"].as_u64()),
        (Some("ponko-validation-bundle"), Some(1))
    );
    assert_eq!(m["result"]["outcome"], "failed");
    assert_eq!(m["result"]["failure"]["kind"], "assertion");
    assert_eq!(m["result"]["failure"]["assertion"], "one-jump-per-press");
    // The second jump starts when the body lands again, 71 ticks in.
    assert_eq!(m["result"]["failure"]["tick"], 71);
    let failed: Vec<&str> = m["assertions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["outcome"] == "failed")
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    assert_eq!(failed, ["one-jump-per-press", "lands-by-the-end"]);

    // Identity, scenario and the unavailable pieces are all said out loud.
    assert_eq!(m["build"]["engine_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(m["build"]["commit"]["status"], "unavailable");
    assert_eq!(
        (m["scenario"]["id"].as_str(), m["scenario"]["seed"].as_u64()),
        (Some("hold-jump"), Some(7))
    );
    assert_eq!(m["scenario"]["steps"].as_array().unwrap().len(), 2);
    assert_eq!(m["metrics"]["screenshots"]["status"], "unavailable");
    assert_eq!(m["metrics"]["performance"]["status"], "unavailable");
    assert_eq!(m["commands"][0]["role"], "reproduce");

    // The log names the second jump; the trace has a row per tick.
    assert!(read(&dir, "logs/run.log").contains("tick 71: jump #2 started"));
    assert_eq!(read(&dir, "trace.csv").lines().count(), 1 + 120);
    assert_artifacts_match(&dir, m);
}

#[test]
fn the_corrected_run_passes_and_the_comparison_names_what_changed() {
    let (bad_dir, good_dir) = (out_dir("cmp-buggy"), out_dir("cmp-fixed"));
    let mut bad = Options::new(&bad_dir, Variant::Buggy);
    bad.commit = Some("1111111".into());
    let mut good = Options::new(&good_dir, Variant::Fixed);
    good.commit = Some("2222222".into());
    let (bad, good) = (orchestrate::run(&bad).unwrap(), orchestrate::run(&good).unwrap());

    assert!(good.passed);
    assert_eq!(good.manifest["result"]["outcome"], "passed");
    assert_eq!(good.manifest["result"]["failure"], Value::Null);
    assert!(
        good.manifest["assertions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["outcome"] == "passed")
    );
    assert_artifacts_match(&good_dir, &good.manifest);

    let diff = compare_setups(&bad.manifest, &good.manifest);
    let paths: Vec<&str> = diff.iter().map(|(p, ..)| p.as_str()).collect();
    assert_eq!(paths, ["build.commit", "commands.0.argv", "settings.variant"]);
    assert_eq!(diff[2].1, "buggy");
    assert_eq!(diff[2].2, "fixed");

    // Seed and feature differences show up too; identical setups show none.
    let mut other = Options::new(out_dir("cmp-seed"), Variant::Fixed);
    other.commit = Some("2222222".into());
    other.seed = 9;
    other.features = Some(vec!["audio".into()]);
    let other = orchestrate::run(&other).unwrap();
    let paths: Vec<String> = compare_setups(&good.manifest, &other.manifest)
        .into_iter()
        .map(|(p, ..)| p)
        .collect();
    assert_eq!(
        paths,
        [
            "build.features",
            "build.features_source",
            "commands.0.argv",
            "scenario.seed"
        ]
    );
    assert!(compare_setups(&good.manifest, &good.manifest).is_empty());
}

#[test]
fn the_same_inputs_give_the_same_bundle_bytes() {
    let (a, b) = (out_dir("same-a"), out_dir("same-b"));
    orchestrate::run(&Options::new(&a, Variant::Buggy)).unwrap();
    orchestrate::run(&Options::new(&b, Variant::Buggy)).unwrap();
    assert_eq!(files(&a), files(&b));
    for f in files(&a) {
        assert_eq!(
            std::fs::read(a.join(&f)).unwrap(),
            std::fs::read(b.join(&f)).unwrap(),
            "{f}"
        );
    }
}

#[test]
fn supplied_build_identity_is_recorded_and_repeats_in_the_reproduce_command() {
    let mut o = Options::new(out_dir("identity"), Variant::Fixed);
    o.commit = Some("abc1234".into());
    o.rustc = Some("rustc 1.95.0".into());
    o.features = Some(vec!["physics".into(), "3d".into()]);
    let m = orchestrate::run(&o).unwrap().manifest;
    assert_eq!(m["build"]["commit"], "abc1234");
    assert_eq!(m["build"]["rustc"], "rustc 1.95.0");
    assert_eq!(m["build"]["features"], json!(["physics", "3d"]));
    assert_eq!(m["build"]["features_source"], "supplied");
    let argv = m["commands"][0]["argv"].to_string();
    assert!(argv.contains("abc1234") && argv.contains("physics,3d"), "{argv}");
}

#[test]
fn a_bundle_directory_is_never_reused() {
    let dir = out_dir("reuse");
    orchestrate::run(&Options::new(&dir, Variant::Fixed)).unwrap();
    let err = orchestrate::run(&Options::new(&dir, Variant::Buggy))
        .err()
        .expect("second write is refused");
    assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
    // The first bundle is untouched.
    assert_eq!(
        serde_json::from_str::<Value>(&read(&dir, "manifest.json")).unwrap()["settings"]["variant"],
        "fixed"
    );
}

// ------------------------------------------------------------ step helpers

/// Run with `--exact helper_step` and a mode in the environment, this test
/// binary acts as the command the orchestrator runs: it fails, hangs or
/// floods its output. Without the variable it does nothing.
#[test]
fn helper_step() {
    let Ok(mode) = std::env::var("PONKO_BUNDLE_TEST_STEP") else {
        return;
    };
    match mode.as_str() {
        "fail" => {
            println!("   Compiling demo v0.1.0");
            eprintln!("error[E0425]: cannot find value `nope` in this scope");
            eprintln!("note: sent with control token=deadbeefcafe0123 from /home/alice/project/src/main.rs");
            std::process::exit(101);
        }
        "hang" => {
            println!("launching, waiting for the control endpoint");
            loop {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        "flood" => {
            for i in 0..20_000 {
                println!("line {i} of a very chatty build");
            }
            eprintln!("final error: the end");
            std::process::exit(1);
        }
        _ => {}
    }
}

fn step(mode: &str) -> CommandSpec {
    let exe = std::env::current_exe().unwrap().to_string_lossy().into_owned();
    let mut spec = CommandSpec::new(&[&exe, "--exact", "helper_step", "--nocapture", "--test-threads=1"]);
    spec.env.push(("PONKO_BUNDLE_TEST_STEP".into(), mode.into()));
    spec
}

#[test]
fn a_failed_step_ends_the_run_with_its_bounded_error_output() {
    let dir = out_dir("step-fail");
    let mut o = Options::new(&dir, Variant::Fixed);
    o.steps = vec![step("fail")];
    let report = orchestrate::run(&o).unwrap();
    let m = &report.manifest;
    assert!(!report.passed);
    assert_eq!(m["result"]["failure"]["kind"], "step_failed");
    assert_eq!(m["commands"][1]["status"], "failed");
    assert_eq!(m["commands"][1]["exit_code"], 101);
    assert_eq!(m["commands"][1]["env_names"], json!(["PONKO_BUNDLE_TEST_STEP"]));
    // No interaction ran after a failed build; that is stated, not implied.
    assert_eq!(m["assertions"]["status"], "unavailable");
    assert_eq!(m["result"]["ticks_run"], 0);
    let log = read(&dir, "logs/step-1.log");
    assert!(log.contains("cannot find value `nope`"), "{log}");
    // The control token in the output never reaches the bundle.
    assert!(!log.contains("deadbeefcafe0123"), "{log}");
    assert!(log.contains("token=<redacted>"), "{log}");
    assert!(!dir.join("logs/run.log").exists());
    assert_artifacts_match(&dir, m);
}

#[test]
fn a_step_that_hangs_is_killed_at_its_timeout() {
    let dir = out_dir("step-hang");
    let mut o = Options::new(&dir, Variant::Fixed);
    o.steps = vec![step("hang")];
    o.step_timeout = Duration::from_millis(600);
    let started = std::time::Instant::now();
    let report = orchestrate::run(&o).unwrap();
    // A hang detector: a step that isn't killed never ends.
    assert!(started.elapsed() < Duration::from_secs(60));
    let m = &report.manifest;
    assert_eq!(m["result"]["failure"]["kind"], "step_timeout");
    assert_eq!(m["commands"][1]["status"], "timeout");
    assert_eq!(m["commands"][1]["timeout_secs"], 0);
    assert!(read(&dir, "logs/step-1.log").contains("waiting for the control endpoint"));
}

#[test]
fn a_missing_program_is_a_launch_failure_not_a_crash() {
    let dir = out_dir("step-missing");
    let mut o = Options::new(&dir, Variant::Fixed);
    o.steps = vec![CommandSpec::new(&["definitely-not-a-program-ponko"])];
    let m = orchestrate::run(&o).unwrap().manifest;
    assert_eq!(m["commands"][1]["status"], "launch_failed");
    assert_eq!(m["result"]["outcome"], "failed");
    assert!(read(&dir, "logs/step-1.log").contains("could not start"));
}

#[test]
fn step_output_is_bounded_and_keeps_the_start_and_the_end() {
    let dir = out_dir("step-flood");
    let mut o = Options::new(&dir, Variant::Fixed);
    o.steps = vec![step("flood")];
    o.step_output_limit = 2_000;
    let m = orchestrate::run(&o).unwrap().manifest;
    let log = read(&dir, "logs/step-1.log");
    assert!(log.len() < 6_000, "{} bytes", log.len());
    assert!(log.contains("line 0 of a very chatty build"));
    assert!(log.contains("bytes omitted"));
    assert!(log.contains("final error: the end"));
    assert!(m["commands"][1]["omitted_output_bytes"].as_u64().unwrap() > 100_000);
}

// --------------------------------------------------------------- redaction

#[test]
fn secrets_are_removed_from_text_and_json_without_touching_prose() {
    let r = Redactor::default();
    assert_eq!(r.text("connect token=abc123 now\n"), "connect token=<redacted> now\n");
    assert_eq!(
        r.text(r#"{"id": 1, "token": "abc123", "port": 5}"#),
        r#"{"id": 1, "token": "<redacted>", "port": 5}"#
    );
    assert_eq!(
        r.text("Authorization: Bearer abc.def.ghi\nnext"),
        "Authorization: <redacted>\nnext"
    );
    assert_eq!(r.text("PASSWORD: hunter2, ok"), "PASSWORD: <redacted>, ok");
    assert_eq!(r.text("the token was rejected\n"), "the token was rejected\n");
    assert_eq!(
        r.text("two token=a secret=b\n"),
        "two token=<redacted> secret=<redacted>\n"
    );

    // A registry entry as the control endpoint writes it: the token goes, the rest stays.
    let registry = json!({"name": "demo", "pid": 42, "port": 5000, "token": "0123456789abcdef", "launch_id": "x"});
    let clean = r.value(&registry);
    assert_eq!(
        clean,
        json!({"name": "demo", "pid": 42, "port": 5000, "token": "<redacted>", "launch_id": "x"})
    );
}

#[test]
fn user_paths_become_placeholders_and_the_rest_of_the_path_survives() {
    let r = Redactor::default()
        .path("/home/alice", "<HOME>")
        .path("/home/alice/project", "<CWD>")
        .path(r"C:\Users\alice", "<HOME>");
    // The longest prefix wins, and both slash styles are covered.
    assert_eq!(
        r.text("/home/alice/project/target/debug/demo"),
        "<CWD>/target/debug/demo"
    );
    assert_eq!(r.text("/home/alice/.cache/x"), "<HOME>/.cache/x");
    assert_eq!(r.text(r"C:\Users\alice\game\run.log"), r"<HOME>\game\run.log");
    assert_eq!(r.text("C:/Users/alice/game"), "<HOME>/game");
    assert_eq!(r.placeholders(), ["<CWD>", "<HOME>"]);
    assert!(!r.placeholders().iter().any(|p| p.contains("alice")));
}

#[test]
fn redacted_bundles_keep_their_reproduction_metadata_and_stay_consistent() {
    let dir = out_dir("redact");
    let mut o = Options::new(&dir, Variant::Buggy);
    o.redact_user_paths = true;
    o.commit = Some("abc1234".into());
    // A failing step whose command line holds this test binary's own path,
    // normally inside the working directory.
    o.steps = vec![step("fail")];
    let report = orchestrate::run(&o).unwrap();
    let m = &report.manifest;
    assert_eq!(m["build"]["commit"], "abc1234");
    assert_eq!(m["redaction"]["path_placeholders"].as_array().unwrap().len(), 3);
    let exe = m["commands"][1]["argv"][0].as_str().unwrap();
    // The separator after the placeholder is the platform's own (`\` on Windows).
    assert!(
        ["<CWD>/", "<CWD>\\", "<HOME>/", "<HOME>\\"]
            .iter()
            .any(|p| exe.starts_with(p)),
        "{exe}"
    );
    assert!(
        exe.contains("validation_bundle"),
        "the rest of the path survives: {exe}"
    );
    for f in files(&dir) {
        let text = read(&dir, &f);
        for secret in [
            std::env::var("HOME").unwrap_or_default(),
            std::env::current_dir().unwrap().to_string_lossy().into_owned(),
        ] {
            if secret.len() > 1 {
                assert!(!text.contains(&secret), "{f} still holds {secret}");
            }
        }
    }
    // Hashes cover the redacted files, so the bundle still verifies.
    assert_artifacts_match(&dir, m);
}

// ------------------------------------------------------------ launched runs

use launched::Launch;

/// Set by the launched-run tests for the copy of this test binary they
/// start as the game: `<variant>:<seed>`.
const SERVE_ENV: &str = "VALIDATION_BUNDLE_TEST_SERVE";
/// Seconds the copy sleeps without opening a control endpoint.
const IDLE_ENV: &str = "VALIDATION_BUNDLE_TEST_IDLE";

/// Not a test of its own: when a launched-run test starts this binary as
/// the game, this is the game. It serves the reference game (or idles)
/// until the tool that launched it asks it to quit. Run as an ordinary
/// test it does nothing.
#[test]
fn launched_copy_of_the_reference_game() {
    if let Ok(secs) = std::env::var(IDLE_ENV) {
        std::thread::sleep(Duration::from_secs(secs.parse().unwrap()));
        return;
    }
    let Ok(spec) = std::env::var(SERVE_ENV) else {
        return;
    };
    let (variant, seed) = spec.split_once(':').unwrap();
    let variant = if variant == "buggy" {
        Variant::Buggy
    } else {
        Variant::Fixed
    };
    reference::serve(variant, seed.parse().unwrap(), reference::SERVE_GIVE_UP);
}

/// This test binary as a prebuilt game: started with only the entry above
/// selected, and told what to do through the environment.
fn this_binary(env: &str, value: &str) -> Launch {
    let mut launch = Launch::program(std::env::current_exe().unwrap().to_string_lossy());
    launch.args = Some(
        [
            "launched_copy_of_the_reference_game",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ]
        .map(String::from)
        .to_vec(),
    );
    launch.env = vec![(env.to_string(), value.to_string())];
    launch
}

fn launched(name: &str, variant: Variant) -> (PathBuf, orchestrate::Report) {
    let dir = out_dir(name);
    let mut o = Options::new(&dir, variant);
    o.launch = Some(this_binary(SERVE_ENV, &format!("{}:{}", variant.name(), o.seed)));
    let report = orchestrate::run(&o).unwrap();
    (dir, report)
}

fn kinds(manifest: &Value) -> Vec<(&str, &str)> {
    manifest["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| (a["path"].as_str().unwrap(), a["kind"].as_str().unwrap()))
        .collect()
}

#[test]
fn a_launched_game_driven_through_the_mcp_tools_fails_and_passes_like_the_in_process_run() {
    let (bad_dir, bad) = launched("launched-buggy", Variant::Buggy);
    let m = &bad.manifest;
    assert!(!bad.passed, "{}", m["result"]);
    assert_eq!(m["settings"]["mode"], "launched");
    assert_eq!(m["result"]["failure"]["kind"], "assertion");
    assert_eq!(m["result"]["failure"]["assertion"], "one-jump-per-press");
    assert_eq!(m["result"]["ticks_run"], reference::TICKS);
    assert_eq!(m["instance"]["name"], "validation-reference");
    assert_eq!(m["instance"]["headless"], true);
    assert_eq!(m["build"]["features_source"], "reported by the launched game");

    // The game was driven with the tools an agent has, in this order.
    let tools: Vec<&str> = m["commands"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["role"] == "mcp")
        .map(|c| c["tool"].as_str().unwrap())
        .collect();
    assert_eq!(
        tools,
        [
            "launch",
            "instance_capabilities",
            "run_input_scenario",
            "instance_info",
            "run_ticks",
            "get_resource",
            "read_logs",
            "system_timings",
            "perf_report",
            "screenshot",
            "quit_instance"
        ]
    );
    let launch = m["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["role"] == "launch")
        .unwrap();
    assert_eq!(launch["env_names"], json!([SERVE_ENV]), "names, never values");

    // Its stdout and stderr, the game's own log, the scenario's report and
    // the performance the tools reported are in the bundle.
    assert_eq!(
        kinds(m),
        [
            ("logs/game.log", "game-log"),
            ("logs/process-output.log", "process-output"),
            ("logs/run.log", "run-log"),
            ("performance/report.json", "performance"),
            ("performance/system-timings.json", "performance"),
            ("scenario-report.json", "scenario-report"),
            ("trace.csv", "trace"),
        ]
    );
    let output = read(&bad_dir, "logs/process-output.log");
    assert!(output.contains("scenario hold-jump: variant=buggy seed=7"), "{output}");
    assert!(output.contains("jump #2 started"), "{output}");
    assert!(read(&bad_dir, "logs/game.log").contains("jump #2 started"));
    assert_eq!(m["metrics"]["performance"]["system_timings"]["status"], "collected");
    let timings: Value = serde_json::from_str(&read(&bad_dir, "performance/system-timings.json")).unwrap();
    assert!(timings.as_array().is_some_and(|t| !t.is_empty()), "{timings}");
    // No window: the screenshot is marked unavailable, with the game's reason.
    assert_eq!(m["metrics"]["screenshots"]["status"], "unavailable");
    assert!(
        m["metrics"]["screenshots"]["reason"]
            .as_str()
            .unwrap()
            .contains("headless"),
        "{}",
        m["metrics"]["screenshots"]
    );
    assert_artifacts_match(&bad_dir, m);

    // Nothing of the control connection or of this run's scratch directory
    // is kept.
    let scratch = format!("ponko-validation-{}-", std::process::id());
    for f in files(&bad_dir) {
        let text = read(&bad_dir, &f);
        assert!(!text.contains(&scratch), "{f} names the scratch directory");
        if f != "manifest.json" {
            assert!(!text.to_lowercase().contains("token"), "{f} mentions a token");
        }
    }

    // The launched run observed what the in-process run observes: the same
    // trace and run log, byte for byte.
    let in_process_dir = out_dir("launched-vs-in-process");
    let in_process = orchestrate::run(&Options::new(&in_process_dir, Variant::Buggy)).unwrap();
    for file in ["trace.csv", "logs/run.log"] {
        assert_eq!(read(&bad_dir, file), read(&in_process_dir, file), "{file}");
    }
    assert_eq!(m["assertions"], in_process.manifest["assertions"]);

    // The corrected game passes, and the comparison says what changed.
    let (good_dir, good) = launched("launched-fixed", Variant::Fixed);
    assert!(good.passed, "{}", good.manifest["result"]);
    let report = bundle::compare_bundles(&bad_dir, &good_dir).unwrap();
    assert_eq!(report["integrity"], json!({ "a": [], "b": [] }));
    assert_eq!(report["same_setup"], false);
    assert_eq!(report["same_result"], false);
    let setup: Vec<&str> = report["setup_differences"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["path"].as_str().unwrap())
        .collect();
    assert!(setup.contains(&"settings.variant"), "{setup:?}");
    assert_eq!(report["result"]["a"]["failed_assertion"], "one-jump-per-press");
    assert_eq!(report["result"]["b"]["outcome"], "passed");
    let not_compared: Vec<&str> = report["not_compared"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        not_compared,
        [
            "logs/game.log",
            "logs/process-output.log",
            "performance/report.json",
            "performance/system-timings.json"
        ]
    );
    let differing: Vec<&str> = report["artifact_differences"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["path"].as_str().unwrap())
        .collect();
    assert_eq!(differing, ["logs/run.log", "trace.csv"]);
    let text = bundle::comparison_text(&report);
    assert!(text.contains("settings.variant: \"buggy\" -> \"fixed\""), "{text}");
    assert!(
        text.contains("result a: failed (assertion: one-jump-per-press at tick 71)"),
        "{text}"
    );
    assert!(text.contains("result b: passed"), "{text}");
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// The place of the recorded call of `tool` in a manifest's commands.
fn call_of(manifest: &Value, tool: &str) -> usize {
    manifest["commands"]
        .as_array()
        .unwrap()
        .iter()
        .position(|c| c["tool"] == tool)
        .unwrap_or_else(|| panic!("no {tool} call in {}", manifest["commands"]))
}

#[test]
fn a_call_that_went_wrong_for_a_moment_is_named_but_is_not_a_setup_difference() {
    let (dir, run) = launched("outcome-base", Variant::Fixed);
    let base = run.manifest;
    assert!(bundle::compare_command_outcomes(&base, &base).is_empty());

    // The same run, but the game was gone before it answered the last
    // call, the log read raced its exit, and more output was cut.
    let mut shaky = base.clone();
    let quit = call_of(&base, "quit_instance");
    let logs = call_of(&base, "read_logs");
    let launch = base["commands"]
        .as_array()
        .unwrap()
        .iter()
        .position(|c| c["role"] == "launch")
        .unwrap();
    shaky["commands"][quit]["ok"] = json!(false);
    shaky["commands"][logs]["ok"] = json!(false);
    shaky["commands"][launch]["omitted_output_bytes"] = json!(12);
    assert_eq!(compare_setups(&base, &shaky), [], "how a call went is not setup");
    let outcomes = bundle::compare_command_outcomes(&base, &shaky);
    let named: Vec<(&str, &str)> = outcomes.iter().map(|(p, c, ..)| (p.as_str(), c.as_str())).collect();
    let mut expected = [
        (format!("commands.{launch}.omitted_output_bytes"), "launch"),
        (format!("commands.{logs}.ok"), "mcp read_logs"),
        (format!("commands.{quit}.ok"), "mcp quit_instance"),
    ];
    expected.sort();
    assert_eq!(
        named,
        expected.iter().map(|(p, c)| (p.as_str(), *c)).collect::<Vec<_>>()
    );

    // Through the bundles on disk: still the same setup and result, with
    // the calls that went differently listed by name.
    let shaky_dir = out_dir("outcome-shaky");
    copy_dir(&dir, &shaky_dir);
    std::fs::write(shaky_dir.join("manifest.json"), shaky.to_string()).unwrap();
    let report = bundle::compare_bundles(&dir, &shaky_dir).unwrap();
    assert_eq!(report["verdict"], "same setup, same result", "{report:#}");
    assert_eq!(report["setup_differences"], json!([]));
    assert_eq!(report["command_outcome_differences"].as_array().unwrap().len(), 3);
    let text = bundle::comparison_text(&report);
    assert!(
        text.contains("commands that went differently (not a setup difference)"),
        "{text}"
    );
    assert!(
        text.contains(&format!("commands.{quit}.ok (mcp quit_instance): true -> false")),
        "{text}"
    );

    // What was asked still counts: another argument to the same call, or a
    // call that is missing, is a setup difference.
    let mut asked = base.clone();
    asked["commands"][logs]["arguments"]["lines"] = json!(5);
    let paths: Vec<String> = compare_setups(&base, &asked).into_iter().map(|(p, ..)| p).collect();
    assert_eq!(paths, [format!("commands.{logs}.arguments.lines")]);
    let mut shorter = base.clone();
    shorter["commands"].as_array_mut().unwrap().truncate(quit);
    let paths: Vec<String> = compare_setups(&base, &shorter).into_iter().map(|(p, ..)| p).collect();
    assert!(paths.contains(&format!("commands.{quit}.tool")), "{paths:?}");
    assert!(!paths.contains(&format!("commands.{quit}.ok")), "{paths:?}");
}

#[test]
fn two_launched_runs_of_one_setup_compare_as_the_same() {
    let (a, _) = launched("launched-same-a", Variant::Fixed);
    let (b, _) = launched("launched-same-b", Variant::Fixed);
    let report = bundle::compare_bundles(&a, &b).unwrap();
    assert_eq!(report["verdict"], "same setup, same result", "{report:#}");
    assert_eq!(report["setup_differences"], json!([]));
    assert_eq!(report["artifact_differences"], json!([]));
    // Timings and process output differ between the two and are set aside.
    assert_eq!(report["not_compared"].as_array().unwrap().len(), 4);
}

#[test]
fn a_game_that_never_opens_its_control_endpoint_is_a_bounded_launch_timeout() {
    let dir = out_dir("launch-timeout");
    let mut o = Options::new(&dir, Variant::Fixed);
    // It would idle ten minutes: only a game that wasn't killed reaches the
    // bound below, however slow the machine is.
    let mut launch = this_binary(IDLE_ENV, "600");
    launch.launch_timeout = Duration::from_secs(2);
    o.launch = Some(launch);
    let started = std::time::Instant::now();
    let report = orchestrate::run(&o).unwrap();
    assert!(started.elapsed() < Duration::from_secs(60), "the idle game was killed");
    let m = &report.manifest;
    assert!(!report.passed);
    let failure = &m["result"]["failure"];
    assert_eq!(failure["kind"], "launch_timeout");
    assert_eq!(failure["timeout_secs"], 2);
    assert_eq!(failure["log"], "logs/process-output.log");
    assert!(failure["message"].as_str().unwrap().len() <= 300);
    assert_eq!(m["assertions"]["status"], "unavailable");
    assert_eq!(m["instance"]["status"], "unavailable");
    assert_eq!(kinds(m), [("logs/process-output.log", "process-output")]);
    assert_artifacts_match(&dir, m);
}

#[test]
fn a_game_that_exits_before_its_control_endpoint_opens_is_a_launch_failure_with_its_output() {
    let dir = out_dir("launch-failed");
    let mut o = Options::new(&dir, Variant::Fixed);
    // No test has this name: the harness prints its summary and exits.
    let mut launch = this_binary(SERVE_ENV, "fixed:7");
    launch.args = Some(vec!["no_such_entry".into(), "--exact".into()]);
    launch.output_limit = 64;
    o.launch = Some(launch);
    let report = orchestrate::run(&o).unwrap();
    let m = &report.manifest;
    assert_eq!(m["result"]["failure"]["kind"], "launch_failed");
    assert!(
        m["result"]["failure"]["message"]
            .as_str()
            .unwrap()
            .contains("before its control endpoint opened"),
        "{}",
        m["result"]["failure"]
    );
    let output = read(&dir, "logs/process-output.log");
    assert!(output.contains("running 0 tests"), "{output}");
    // Bounded: its start and end, with the middle counted.
    assert!(output.contains("bytes omitted"), "{output}");
    let launch = m["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["role"] == "launch")
        .unwrap();
    assert!(launch["omitted_output_bytes"].as_u64().unwrap() > 0);
    assert_artifacts_match(&dir, m);
}

/// A cargo project of one file, outside this workspace.
fn project(name: &str, main: &str) -> PathBuf {
    let dir = out_dir(name);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"bundle-fixture\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[workspace]\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/main.rs"), main).unwrap();
    dir
}

#[test]
fn a_game_that_does_not_compile_is_a_build_failure_with_the_compiler_output() {
    let project = project("build-failed-project", "fn main() { let n: u32 = \"not a number\"; }\n");
    let dir = out_dir("build-failed");
    let mut o = Options::new(&dir, Variant::Fixed);
    let mut launch = Launch::cargo();
    launch.cwd = Some(project.to_string_lossy().into_owned());
    launch.example = None;
    o.launch = Some(launch);
    let report = orchestrate::run(&o).unwrap();
    let m = &report.manifest;
    assert!(!report.passed);
    assert_eq!(m["result"]["failure"]["kind"], "build_failed");
    assert_eq!(m["result"]["failure"]["log"], "logs/build.log");
    let build = m["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["role"] == "build")
        .unwrap();
    assert_eq!(build["argv"], json!(["cargo", "build"]));
    assert_eq!(build["status"], "failed");
    let log = read(&dir, "logs/build.log");
    assert!(log.contains("mismatched types"), "{log}");
    // The game was never started: no launch, no tool calls.
    assert!(m["commands"].as_array().unwrap().iter().all(|c| c["role"] != "mcp"));
    assert_eq!(kinds(m), [("logs/build.log", "step-log")]);
    assert_eq!(m["assertions"]["status"], "unavailable");
    assert_artifacts_match(&dir, m);
    let _ = std::fs::remove_dir_all(project);
}

#[test]
fn a_built_game_that_never_reports_in_is_told_apart_from_a_build_failure() {
    // It compiles, runs and exits at once, with no engine in it.
    let project = project(
        "launch-after-build-project",
        "fn main() { println!(\"no engine here\"); }\n",
    );
    let dir = out_dir("launch-after-build");
    let mut o = Options::new(&dir, Variant::Fixed);
    let mut launch = Launch::cargo();
    launch.cwd = Some(project.to_string_lossy().into_owned());
    launch.example = None;
    launch.args = Some(Vec::new());
    o.launch = Some(launch);
    let report = orchestrate::run(&o).unwrap();
    let m = &report.manifest;
    let build = m["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["role"] == "build")
        .unwrap();
    assert_eq!(build["status"], "passed");
    assert_eq!(m["result"]["failure"]["kind"], "launch_failed");
    assert!(read(&dir, "logs/process-output.log").contains("no engine here"));
    assert_artifacts_match(&dir, m);
    let _ = std::fs::remove_dir_all(project);
}

// ------------------------------------------- identity, performance, images

#[test]
fn the_commit_and_compiler_are_found_when_they_are_not_supplied() {
    let dir = out_dir("identity-found");
    let mut o = Options::new(&dir, Variant::Fixed);
    o.capture_identity = true;
    let m = orchestrate::run(&o).unwrap().manifest;
    let commit = &m["build"]["commit"];
    // A checkout has a commit; a source archive without git says why not.
    match commit["sha"].as_str() {
        Some(sha) => {
            assert!(sha.len() >= 40 && sha.chars().all(|c| c.is_ascii_hexdigit()), "{sha}");
            assert!(commit["dirty"].is_boolean(), "{commit}");
            assert_eq!(commit["source"], "git rev-parse HEAD");
        }
        None => {
            assert_eq!(commit["status"], "unavailable", "{commit}");
            assert!(commit["reason"].as_str().unwrap().contains("git"), "{commit}");
        }
    }
    let rustc = &m["build"]["rustc"];
    assert!(rustc["version"].as_str().unwrap().starts_with("rustc "), "{rustc}");
    assert!(!m["build"]["features"].as_array().unwrap().is_empty() || !cfg!(feature = "window"));

    // Supplied values win, and are kept as they were given.
    let mut o = Options::new(out_dir("identity-supplied"), Variant::Fixed);
    o.capture_identity = true;
    o.commit = Some("abc1234".into());
    o.rustc = Some("rustc 9.9.9".into());
    let m = orchestrate::run(&o).unwrap().manifest;
    assert_eq!(m["build"]["commit"], "abc1234");
    assert_eq!(m["build"]["rustc"], "rustc 9.9.9");
}

#[test]
fn a_profiled_run_keeps_its_performance_report_and_observes_the_same_ticks() {
    let (plain_dir, dir) = (out_dir("perf-plain"), out_dir("perf"));
    let plain = orchestrate::run(&Options::new(&plain_dir, Variant::Fixed))
        .unwrap()
        .manifest;
    assert_eq!(plain["metrics"]["performance"]["status"], "unavailable");
    let mut o = Options::new(&dir, Variant::Fixed);
    o.profile = true;
    let m = orchestrate::run(&o).unwrap().manifest;
    assert_eq!(
        m["metrics"]["performance"]["profile_headless"],
        json!({ "status": "collected", "artifact": "performance/report.json" })
    );
    let report: Value = serde_json::from_str(&read(&dir, "performance/report.json")).unwrap();
    assert!(report.is_object(), "{report}");
    assert_eq!(read(&dir, "trace.csv"), read(&plain_dir, "trace.csv"));
    assert_artifacts_match(&dir, &m);
    // The report is set aside when runs are compared; `--perf` is a setup
    // difference of its own.
    let cmp = bundle::compare_bundles(&plain_dir, &dir).unwrap();
    assert_eq!(cmp["artifact_differences"], json!([]));
    assert_eq!(cmp["not_compared"][0]["path"], "performance/report.json");
    assert_eq!(cmp["setup_differences"][0]["path"], "commands.0.argv");
}

/// A bundle with one image, as a windowed launched run writes it.
fn bundle_with_image(name: &str, pixels: &[u8]) -> PathBuf {
    let dir = out_dir(name);
    let mut b = bundle::Bundle::new(Redactor::default());
    b.section("build", json!({ "engine_version": "0.0.1" }));
    b.section(
        "settings",
        json!({ "variant": "fixed", "headless": false, "mode": "launched" }),
    );
    b.section(
        "result",
        json!({ "outcome": "passed", "failure": null, "ticks_run": 120 }),
    );
    b.section("assertions", json!([]));
    b.bytes("screenshots/final.png", "screenshot", pixels.to_vec());
    b.write(&dir).unwrap();
    dir
}

#[test]
fn an_image_is_evidence_only_once_inspected_and_is_never_compared_by_its_pixels() {
    // Two runs of one setup whose frames differ in a few bytes, as frames
    // from two GPUs do.
    let (a, b) = (
        bundle_with_image("image-a", b"\x89PNG frame from one adapter"),
        bundle_with_image("image-b", b"\x89PNG frame from another one"),
    );
    let report = bundle::compare_bundles(&a, &b).unwrap();
    assert_eq!(report["verdict"], "same setup, same result");
    assert_eq!(
        report["artifact_differences"],
        json!([]),
        "pixel differences are not a failure"
    );
    assert_eq!(report["not_compared"][0]["kind"], "screenshot");
    for side in ["a", "b"] {
        assert_eq!(
            report["visual_evidence"][side][0]["inspection"]["status"],
            "not_inspected"
        );
    }
    assert!(bundle::comparison_text(&report).contains("screenshots/final.png: NOT INSPECTED"));

    // Someone looks at one of them and says what it shows.
    let image = "screenshots/final.png";
    assert!(
        bundle::record_inspection(&a, image, "fine", "x", "me").is_err(),
        "verdicts are ok, wrong or unclear"
    );
    assert!(
        bundle::record_inspection(&a, image, "ok", " ", "me").is_err(),
        "a note is required"
    );
    assert!(
        bundle::record_inspection(&a, "trace.csv", "ok", "x", "me").is_err(),
        "only screenshots"
    );
    bundle::record_inspection(
        &a,
        image,
        "ok",
        "the body is on the ground, right of center",
        "reviewer",
    )
    .unwrap();
    let report = bundle::compare_bundles(&a, &b).unwrap();
    let seen = &report["visual_evidence"]["a"][0]["inspection"];
    assert_eq!(seen["status"], "inspected");
    assert_eq!(seen["verdict"], "ok");
    assert_eq!(seen["by"], "reviewer");
    assert_eq!(
        report["visual_evidence"]["b"][0]["inspection"]["status"],
        "not_inspected"
    );
    // The note is beside the manifest, which still verifies.
    assert_eq!(report["integrity"]["a"], json!([]));
    assert!(bundle::comparison_text(&report).contains("inspected by reviewer, ok"));

    // A note is about the image it was written for: change the image and
    // the bundle is damaged, and the note no longer applies.
    std::fs::write(a.join(image), b"\x89PNG something else").unwrap();
    assert!(bundle::record_inspection(&a, image, "ok", "still fine", "reviewer").is_err());
    let manifest = bundle::read_manifest(&a).unwrap();
    let problems = bundle::verify(&a, &manifest);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].starts_with("screenshots/final.png"), "{problems:?}");
}

#[test]
fn verification_names_missing_changed_and_unlisted_files() {
    let dir = out_dir("verify");
    let m = orchestrate::run(&Options::new(&dir, Variant::Fixed)).unwrap().manifest;
    assert_eq!(bundle::verify(&dir, &m), Vec::<String>::new());
    std::fs::write(dir.join("trace.csv"), "tick\n").unwrap();
    std::fs::remove_file(dir.join("logs/run.log")).unwrap();
    std::fs::write(dir.join("extra.txt"), "x").unwrap();
    let problems = bundle::verify(&dir, &m);
    assert_eq!(problems.len(), 3, "{problems:?}");
    assert!(
        problems
            .iter()
            .any(|p| p.starts_with("logs/run.log: listed, but missing"))
    );
    assert!(problems.iter().any(|p| p.starts_with("trace.csv:")));
    assert!(problems.iter().any(|p| p.starts_with("extra.txt: in the directory")));
    // A damaged bundle is said to be damaged when it is compared.
    let other = out_dir("verify-other");
    orchestrate::run(&Options::new(&other, Variant::Fixed)).unwrap();
    let report = bundle::compare_bundles(&dir, &other).unwrap();
    assert_eq!(report["integrity"]["a"].as_array().unwrap().len(), 3);
    assert!(bundle::comparison_text(&report).starts_with("bundle a: DAMAGED"));
    // Not a bundle at all.
    assert!(bundle::read_manifest(&out_dir("verify-missing")).is_err());
}

// ------------------------------------------------- a game of your own

/// A scenario file for the reference game: press jump, and check the
/// action and the game's own trace on given ticks.
fn scenario_file(jumps_at_80: u32) -> Value {
    json!({
        "version": 1,
        "id": "hold-jump-file",
        "duration": 100,
        "steps": [
            { "tick": 10, "type": "key", "key": "Space", "down": true },
            { "tick": 10, "type": "expect_action", "action": "jump", "pressed": true },
            { "tick": 80, "type": "expect_resource", "resource": "Trace", "pointer": "/rows/80/jumps", "equals": jumps_at_80 },
        ],
    })
}

fn launched_with_file(name: &str, variant: Variant, file: Value) -> (PathBuf, orchestrate::Report) {
    let dir = out_dir(name);
    let mut o = Options::new(&dir, variant);
    o.launch = Some(this_binary(SERVE_ENV, &format!("{}:{}", variant.name(), o.seed)));
    o.scenario = Some(file);
    let report = orchestrate::run(&o).unwrap();
    (dir, report)
}

#[test]
fn a_scenario_file_drives_a_launched_game_and_its_checks_are_the_assertions() {
    // Against the corrected game every check holds.
    let (dir, good) = launched_with_file("file-fixed", Variant::Fixed, scenario_file(1));
    let m = &good.manifest;
    assert!(good.passed, "{}", m["result"]);
    assert_eq!(m["scenario"]["id"], "hold-jump-file");
    assert_eq!(m["scenario"]["ticks"], 100);
    assert_eq!(m["scenario"]["steps"], 3);
    assert_eq!(m["scenario"]["seed"]["status"], "unavailable");
    assert_eq!(m["result"]["ticks_run"], 100);
    assert_eq!(m["assertions"][0]["id"], "scenario-checks");
    assert_eq!(m["assertions"][0]["outcome"], "passed");
    // The file itself is in the bundle, so the run can be repeated from it.
    let kept: Value = serde_json::from_str(&read(&dir, "scenario.json")).unwrap();
    assert_eq!(kept, scenario_file(1));
    assert!(kinds(m).contains(&("scenario.json", "scenario")));
    assert!(
        !kinds(m).iter().any(|k| k.0 == "trace.csv"),
        "the trace is the reference game's"
    );
    assert_artifacts_match(&dir, m);

    // Against the broken one the bundle names the failed step, its tick and
    // what the game had instead.
    let (dir, bad) = launched_with_file("file-buggy", Variant::Buggy, scenario_file(1));
    let m = &bad.manifest;
    assert!(!bad.passed);
    let failure = &m["result"]["failure"];
    assert_eq!(failure["kind"], "assertion");
    assert_eq!(failure["assertion"], "scenario-checks");
    assert_eq!(
        (failure["step"].as_u64(), failure["tick"].as_u64()),
        (Some(2), Some(80))
    );
    assert_eq!(failure["report"], "scenario-report.json");
    let checks = &m["assertions"][0];
    assert_eq!(checks["outcome"], "failed");
    assert!(checks["expected"].as_str().unwrap().contains("step 2"), "{checks}");
    assert_eq!(checks["actual"], "2", "the second jump");
    let report: Value = serde_json::from_str(&read(&dir, "scenario-report.json")).unwrap();
    assert_eq!(report["status"], "failed");
    assert_eq!(report["checks_passed"], 1);
    assert!(read(&dir, "logs/run.log").contains("assertion scenario-checks: FAILED at tick 80"));
    assert_artifacts_match(&dir, m);
}

#[test]
fn a_game_of_your_own_gets_only_the_arguments_you_gave_it() {
    // No arguments given: the reference game's `--serve --variant ...` must
    // not reach a game that isn't the reference game. The program doesn't
    // exist, so the launch fails; what was tried is recorded.
    let dir = out_dir("file-own-args");
    let mut o = Options::new(&dir, Variant::Fixed);
    let mut launch = Launch::program("definitely-not-a-game-ponko");
    launch.launch_timeout = Duration::from_secs(5);
    o.launch = Some(launch);
    o.scenario = Some(scenario_file(1));
    let report = orchestrate::run(&o).unwrap();
    assert!(!report.passed);
    let commands = report.manifest["commands"].as_array().unwrap();
    let reproduce = commands[0]["argv"].to_string();
    assert!(
        !reproduce.contains("--variant") && !reproduce.contains("--seed"),
        "{reproduce}"
    );
    let started = commands.iter().find(|c| c["role"] == "launch").unwrap();
    assert_eq!(started["argv"], json!(["definitely-not-a-game-ponko"]));
    let asked = commands.iter().find(|c| c["tool"] == "launch").unwrap();
    assert_eq!(asked["arguments"]["args"], json!([]));
}

#[test]
fn a_scenario_file_is_checked_before_anything_runs() {
    // A typo in the file is an error, not a weaker test.
    let dir = out_dir("file-typo");
    let mut o = Options::new(&dir, Variant::Fixed);
    o.launch = Some(this_binary(SERVE_ENV, "fixed:7"));
    o.scenario = Some(json!({ "version": 1, "steps": [{ "tick": 1, "type": "key", "kee": "Space", "down": true }] }));
    let error = orchestrate::run(&o).err().expect("refused");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(!dir.exists(), "nothing was written");
    // It needs a launched game: the in-process run is the reference interaction.
    let mut o = Options::new(&dir, Variant::Fixed);
    o.scenario = Some(scenario_file(1));
    assert_eq!(
        orchestrate::run(&o).err().unwrap().kind(),
        std::io::ErrorKind::InvalidInput
    );
}
