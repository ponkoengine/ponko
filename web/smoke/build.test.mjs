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
