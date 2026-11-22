// Tests for web/build.sh's flags (`node --test web/smoke`): no build. Cargo
// reads only the first of CARGO_ENCODED_RUSTFLAGS, RUSTFLAGS and the
// target's flags that is set, so `--threads` must put its flags into that
// one and keep the caller's: a CI's RUSTFLAGS="-D warnings" must still fail
// a threaded build on a warning (threads.mjs --case warning builds one).
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const BUILD = join(dirname(fileURLToPath(import.meta.url)), "..", "build.sh");
const TARGET = "CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS";
const FLAG_VARS = ["CARGO_ENCODED_RUSTFLAGS", "RUSTFLAGS", TARGET, "PONKO_WEB_TOOLCHAIN"];

/** What `build.sh --print-env` prints, as `{ name: value }`, with `set` in the environment. */
function printEnv(set, args = ["--threads"]) {
  const env = { ...process.env };
  for (const name of FLAG_VARS) delete env[name];
  Object.assign(env, set);
  const run = spawnSync("bash", [BUILD, ...args, "--print-env"], { env, encoding: "utf8" });
  assert.equal(run.status, 0, run.stderr);
  return Object.fromEntries(
    run.stdout
      .trimEnd()
      .split("\n")
      .map((line) => [line.slice(0, line.indexOf("=")), line.slice(line.indexOf("=") + 1)]),
  );
}

const THREAD_FLAGS = ["target-feature=+atomics,+bulk-memory,+mutable-globals", "link-arg=--shared-memory", "link-arg=--import-memory"];

function assertThreadFlags(words) {
  for (const flag of THREAD_FLAGS) assert.ok(words.includes(flag), `${flag} missing from ${words.join(" ")}`);
}

test("threaded_flags_alone_go_into_the_targets_variable", () => {
  const env = printEnv({});
  assert.deepEqual(Object.keys(env), ["toolchain", TARGET]);
  assert.equal(env.toolchain, "nightly");
  assertThreadFlags(env[TARGET].split(" "));
});

test("rustflags_deny_warnings_is_kept_in_a_threaded_build", () => {
  const env = printEnv({ RUSTFLAGS: "-D warnings" });
  assert.deepEqual(Object.keys(env), ["toolchain", "RUSTFLAGS"], "cargo ignores the target's flags once RUSTFLAGS is set");
  const words = env.RUSTFLAGS.split(" ");
  assertThreadFlags(words);
  assert.deepEqual(words.slice(-2), ["-D", "warnings"], "the caller's flags come last, as given");
});

test("encoded_rustflags_are_kept_in_a_threaded_build", () => {
  const env = printEnv({ CARGO_ENCODED_RUSTFLAGS: "-D\x1fwarnings", RUSTFLAGS: "--cfg ignored" });
  assert.deepEqual(Object.keys(env), ["toolchain", "CARGO_ENCODED_RUSTFLAGS"]);
  const words = env.CARGO_ENCODED_RUSTFLAGS.split("\x1f");
  assertThreadFlags(words);
  assert.deepEqual(words.slice(-2), ["-D", "warnings"]);
  assert.ok(!words.includes("ignored"), "RUSTFLAGS is ignored by cargo when the encoded flags are set");
});

test("the_targets_own_flags_are_kept_in_a_threaded_build", () => {
  const env = printEnv({ [TARGET]: "-D warnings" });
  const words = env[TARGET].split(" ");
  assertThreadFlags(words);
  assert.deepEqual(words.slice(-2), ["-D", "warnings"]);
});

test("the_web_toolchain_can_be_chosen", () => {
  assert.equal(printEnv({ PONKO_WEB_TOOLCHAIN: "nightly-2026-11-01" }).toolchain, "nightly-2026-11-01");
});

test("a_build_without_threads_changes_no_flags", () => {
  assert.deepEqual(printEnv({ RUSTFLAGS: "-D warnings" }, []), { toolchain: "default" });
});

test("clippy_runs_nightly_with_the_threaded_flags_and_denies_warnings", () => {
  const env = printEnv({ RUSTFLAGS: "--cfg x" }, ["--threads", "--clippy"]);
  assert.deepEqual(Object.keys(env), ["toolchain", "RUSTFLAGS", "clippy"]);
  assertThreadFlags(env.RUSTFLAGS.split(" "));
  const words = env.clippy.split(" ");
  assert.deepEqual(words.slice(0, 3), ["cargo", "+nightly", "clippy"]);
  assert.ok(words.includes("-Zbuild-std=panic_abort,std"), "std rebuilt with atomics, as the build does");
  assert.ok(words.includes("--lib") && words.includes("web_threads"), "the engine and the threads fixture");
  assert.deepEqual(words.slice(-3), ["--", "-D", "warnings"]);
});

test("simd_sets_simd128_and_the_wasm_simd_feature_together", () => {
  const env = printEnv({}, ["--simd", "--features", "3d"]);
  assert.deepEqual(env, { toolchain: "default", [TARGET]: "-C target-feature=+simd128", features: "3d,ponko/wasm-simd" });
  assert.equal(printEnv({}, ["--simd"]).features, "ponko/wasm-simd", "with no other features");
});

test("simd_keeps_the_callers_flags", () => {
  const rust = printEnv({ RUSTFLAGS: "-D warnings" }, ["--simd"]);
  assert.deepEqual(Object.keys(rust), ["toolchain", "RUSTFLAGS", "features"], "cargo ignores the target's flags once RUSTFLAGS is set");
  assert.equal(rust.RUSTFLAGS, "-C target-feature=+simd128 -D warnings");
  const target = printEnv({ [TARGET]: "-C opt-level=s" }, ["--simd"]);
  assert.equal(target[TARGET], "-C target-feature=+simd128 -C opt-level=s");
  const encoded = printEnv({ CARGO_ENCODED_RUSTFLAGS: "-D\x1fwarnings" }, ["--simd"]);
  assert.deepEqual(encoded.CARGO_ENCODED_RUSTFLAGS.split("\x1f"), ["-C", "target-feature=+simd128", "-D", "warnings"]);
});

test("simd_goes_with_threads", () => {
  const env = printEnv({}, ["--threads", "--simd"]);
  assert.equal(env.toolchain, "nightly");
  const words = env[TARGET].split(" ");
  assertThreadFlags(words);
  assert.ok(words.includes("target-feature=+simd128"), words.join(" "));
  assert.equal(env.features, "ponko/wasm-simd");
});

test("clippy_is_only_for_threaded_builds", () => {
  const run = spawnSync("bash", [BUILD, "--clippy", "--print-env"], { encoding: "utf8" });
  assert.equal(run.status, 2);
  assert.match(run.stderr, /--clippy is for --threads/);
});

test("the_isolation_worker_is_only_for_threaded_builds", () => {
  const run = spawnSync("bash", [BUILD, "--isolation-worker", "--print-env"], { encoding: "utf8" });
  assert.equal(run.status, 2);
  assert.match(run.stderr, /--isolation-worker is for --threads/);
});

test("a_cargo_profile_replaces_release_only_when_given", () => {
  assert.deepEqual(printEnv({}, []), { toolchain: "default" }, "release is the default and prints nothing");
  assert.deepEqual(printEnv({}, ["--profile", "dist-web"]), { toolchain: "default", profile: "dist-web" });
});

test("a_profile_flag_needs_a_name", () => {
  const run = spawnSync("bash", [BUILD, "--profile"], { encoding: "utf8" });
  assert.notEqual(run.status, 0);
  assert.match(run.stderr, /--profile needs a name/);
});
