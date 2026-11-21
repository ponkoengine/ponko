// Tests for threads-lib.mjs: no browser and no build (`node --test web/smoke`).
import { test } from "node:test";
import assert from "node:assert/strict";
import { UsageError } from "./lib.mjs";
import {
  CHECK_TICKS,
  FALLBACK_WARNING,
  ISOLATION_HEADERS,
  checkFallback,
  checkIsolated,
  checkReady,
  checkGamepadStep,
  checkRumbleCalls,
  checkSameResult,
  checkWarningBuild,
  denyWarnings,
  EFFECT_CAP_MS,
  fakeGamepad,
  FAKE_PAD_ID,
  GAMEPAD_STEPS,
  GILRS_AXIS_TOLERANCE,
  installFakeGamepads,
  WARNING_ITEM,
  median,
  parseArgs,
  PLAIN_DIR,
  summarize,
  summaryTable,
} from "./threads-lib.mjs";

const isolatedRun = {
  ticks: 160,
  isolated: true,
  threads: 7,
  par_threads: 7,
  par_on_page: false,
  batch_threads: 4,
  batch_on_page: false,
  batch_threaded_runs: 160,
  schedule_threads: true,
  wait_off_page: true,
  job_off_page: true,
  group: "ready",
  group_off_page: true,
  group_bytes: 966,
  checksum: 81016914.26837707,
};

const plainRun = {
  ticks: 160,
  isolated: false,
  threads: 0,
  par_threads: 0,
  par_on_page: true,
  batch_threads: 0,
  batch_on_page: true,
  batch_threaded_runs: 0,
  schedule_threads: false,
  wait_off_page: false,
  job_off_page: false,
  group: "ready",
  group_off_page: false,
  group_bytes: 966,
  checksum: 81016914.26837707,
};

test("the isolation headers are the two that make a page cross-origin isolated", () => {
  assert.deepEqual(ISOLATION_HEADERS, {
    "cross-origin-opener-policy": "same-origin",
    "cross-origin-embedder-policy": "require-corp",
  });
});

test("arguments default to the check of the built fixture and refuse what they don't know", () => {
  const args = parseArgs([]);
  assert.equal(args.mode, "check");
  assert.equal(args.dir, "target/web/web_threads");
  assert.deepEqual(parseArgs(["--bench", "--workloads", "par,jobs", "--threads", "1,4", "--repeat", "5"]).threads, [1, 4]);
  assert.throws(() => parseArgs(["--workloads", "nope"]), UsageError);
  assert.throws(() => parseArgs(["--repeat", "0"]), UsageError);
  assert.throws(() => parseArgs(["--case", "stall"]), UsageError);
  assert.equal(parseArgs(["--case", "gamepads"]).case, "gamepads");
  assert.equal(parseArgs(["--case", "warning"]).case, "warning");
  assert.equal(parseArgs(["--case", "gamepads-plain"]).dir, PLAIN_DIR, "the ordinary build is its own folder");
  assert.equal(parseArgs(["--case", "gamepads-plain", "--dir", "x"]).dir, "x");
  assert.equal(parseArgs(["--case", "gamepads"]).dir, "target/web/web_threads");
  assert.throws(() => parseArgs(["--bench", "--case", "worker-panic"]), UsageError);
});

test("a check waits for the checksum, both jobs and the asset group", () => {
  assert.equal(checkReady(null), false);
  assert.equal(checkReady({ ...isolatedRun, ticks: CHECK_TICKS - 1 }), false);
  assert.equal(checkReady({ ...isolatedRun, job_off_page: null }), false);
  assert.equal(checkReady({ ...isolatedRun, group: null }), false);
  assert.equal(checkReady(isolatedRun), true);
});

test("an isolated run passes only when workers ran the queries, batches and jobs", () => {
  assert.deepEqual(checkIsolated(isolatedRun), []);
  // The fallback run on an isolated page: every expectation names itself.
  const problems = checkIsolated({ ...plainRun, isolated: true });
  assert.ok(problems.some((p) => p.includes("0 worker threads")), problems.join("\n"));
  assert.ok(problems.some((p) => p.includes("query items ran on the page's thread")));
  assert.ok(problems.some((p) => p.includes("no batch")));
  assert.ok(problems.some((p) => p.includes("a job ran on the page's thread")));
  assert.ok(problems.some((p) => p.includes("decoded on the page's thread")));
  // A group that couldn't read its file on a worker fails the check by name.
  const failed = checkIsolated({ ...isolatedRun, group: 'failed: "crab.png" not found in assets/' });
  assert.deepEqual(failed, [`the asset group from assets/ didn't load (failed: "crab.png" not found in assets/)`]);
  // One worker is not "more than one".
  assert.equal(checkIsolated({ ...isolatedRun, par_threads: 1 }).length, 1);
});

test("a run that isn't isolated must stay on the page's thread and say why", () => {
  assert.deepEqual(checkFallback(plainRun, `warning: ponko: ... ${FALLBACK_WARNING}`), []);
  assert.ok(checkFallback(plainRun, "").some((p) => p.includes("doesn't say why")));
  assert.ok(checkFallback({ ...isolatedRun, isolated: false }, FALLBACK_WARNING).length >= 3);
});

test("both runs must simulate the same world", () => {
  assert.deepEqual(checkSameResult(isolatedRun, plainRun), []);
  assert.equal(checkSameResult(isolatedRun, { ...plainRun, checksum: 1 }).length, 1);
  assert.equal(checkSameResult(isolatedRun, { ...plainRun, group_bytes: 0 }).length, 1);
});

test("the summary compares medians and keeps a speed-up inside the noise from counting", () => {
  assert.equal(median([3, 1, 2]), 2);
  const loads = [
    { workload: "par", threads: 1, runs_ms: [10, 10, 11] },
    { workload: "par", threads: 1, runs_ms: [12, 12, 12] },
    { workload: "par", threads: 7, runs_ms: [2, 2, 3] },
    { workload: "par", threads: 7, runs_ms: [2.2, 2.2, 2.2] },
    // Noisy: medians 10 and 20 with one thread, 9 and 19 with seven.
    { workload: "jobs", threads: 1, runs_ms: [10] },
    { workload: "jobs", threads: 1, runs_ms: [20] },
    { workload: "jobs", threads: 7, runs_ms: [9] },
    { workload: "jobs", threads: 7, runs_ms: [19] },
  ];
  const rows = summarize(loads);
  const par = rows.find((r) => r.workload === "par" && r.threads === 7);
  assert.equal(par.median_ms, 2);
  assert.ok(par.speedup > 4 && par.beyond_noise, JSON.stringify(par));
  const jobs = rows.find((r) => r.workload === "jobs" && r.threads === 7);
  assert.equal(jobs.beyond_noise, false, JSON.stringify(jobs));
  const table = summaryTable(rows);
  assert.match(table, /\| par \| 7 \| 2\.00 \|/);
  assert.match(table, /within noise/);
});

test("the build denies warnings in the variable cargo reads, keeping the caller's flags", () => {
  const target = "CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS";
  assert.equal(denyWarnings({}).RUSTFLAGS, "-D warnings");
  assert.equal(denyWarnings({ RUSTFLAGS: "--cfg x" }).RUSTFLAGS, "--cfg x -D warnings");
  assert.equal(denyWarnings({ RUSTFLAGS: "-D warnings" }).RUSTFLAGS, "-D warnings", "not twice");
  // The target's own flags are what cargo reads when RUSTFLAGS isn't set.
  const own = denyWarnings({ [target]: "--cfg x" });
  assert.equal(own[target], "--cfg x -D warnings");
  assert.equal(own.RUSTFLAGS, undefined);
  // The encoded flags win over RUSTFLAGS in cargo.
  const encoded = denyWarnings({ CARGO_ENCODED_RUSTFLAGS: "--cfg\x1fx", RUSTFLAGS: "--cfg y" });
  assert.equal(encoded.CARGO_ENCODED_RUSTFLAGS, "--cfg\x1fx\x1f-D\x1fwarnings");
  assert.equal(encoded.RUSTFLAGS, "--cfg y");
});

test("the warning self-test passes only when the build fails on the fixture's warning", () => {
  const failedOnIt = [
    `error: function \`${WARNING_ITEM}\` is never used`,
    "  |",
    "  = note: \`-D dead-code\` implied by \`-D warnings\`",
  ].join("\n");
  assert.deepEqual(checkWarningBuild(101, failedOnIt), []);
  // Passed: the warning didn't fail it.
  assert.match(checkWarningBuild(0, "warning: function is never used")[0], /passed/);
  // Cargo compiled it without atomics (the warning's code left out), and
  // wasm-bindgen failed after: what the old build.sh did under RUSTFLAGS.
  const lost = "    Finished `release` profile [optimized] target(s) in 3m 06s\nerror: failed to find intrinsics";
  assert.match(checkWarningBuild(1, lost)[0], /without failing on its warning/);
  // Failed on something else (no nightly, say): not proof.
  assert.match(checkWarningBuild(1, "error: toolchain 'nightly' is not installed")[0], /not on/);
  // Reported, but only as a warning, while something else failed.
  assert.match(checkWarningBuild(1, `warning: function \`${WARNING_ITEM}\` is never used`)[0], /not as an error/);
});

test("fed gamepads come back from getGamepads at their index, shaped like the browser's", () => {
  const scope = { navigator: {} };
  installFakeGamepads(scope);
  assert.deepEqual(scope.navigator.getGamepads(), [null, null, null, null]);
  const pad = fakeGamepad({ index: 1, buttons: { 0: 1, 6: 0.25 }, axes: [0.5, -1, 0, 0] });
  scope.__ponkoFeedGamepads([pad]);
  const list = scope.navigator.getGamepads();
  assert.equal(list[0], null);
  assert.equal(list[1].id, FAKE_PAD_ID);
  assert.equal(list[1].mapping, "standard");
  assert.equal(list[1].buttons.length, 17);
  assert.deepEqual(list[1].buttons[0], { pressed: true, touched: true, value: 1 });
  assert.deepEqual(list[1].buttons[6], { pressed: false, touched: true, value: 0.25 }, "a quarter-pulled trigger isn't pressed");
  assert.deepEqual(list[1].axes, [0.5, -1, 0, 0]);
  scope.__ponkoFeedGamepads([]);
  assert.deepEqual(scope.navigator.getGamepads(), [null, null, null, null]);
});

test("each gamepad step passes only on what the engine must report", () => {
  const pad = (down, axes, extra = {}) => ({
    slot: 0,
    name: FAKE_PAD_ID,
    family: "Xbox",
    vendor: 0x045e,
    product: 0x0b13,
    rumble: true,
    down,
    axes: { lx: 0, ly: 0, rx: 0, ry: 0, lt: 0, rt: 0, ...axes },
    ...extra,
  });
  const [none, connect, press, release, unplug] = GAMEPAD_STEPS;
  assert.deepEqual(checkGamepadStep(none, { pads: [] }), []);
  assert.equal(checkGamepadStep(none, { pads: [pad([], {})] }).length, 1);
  assert.deepEqual(checkGamepadStep(connect, { pads: [pad([], {})] }), []);
  assert.match(checkGamepadStep(connect, { pads: [] })[0], /one pad/);
  assert.match(checkGamepadStep(connect, { pads: [{ ...pad([], {}), family: "Generic" }] })[0], /family/);
  assert.match(checkGamepadStep(connect, { pads: [{ ...pad([], {}), rumble: false }] })[0], /rumble capability/);
  const pressed = { pads: [pad(["DPadRight", "South"], { lx: 0.5, ly: -1, ry: 0.75, lt: 0.25 })], pad_presses: 1 };
  assert.deepEqual(checkGamepadStep(press, pressed), []);
  // Y up must stay negative, and a quarter-pulled trigger isn't a press.
  assert.match(checkGamepadStep(press, { ...pressed, pads: [{ ...pressed.pads[0], axes: { ...pressed.pads[0].axes, ly: 1 } }] })[0], /ly/);
  assert.match(checkGamepadStep(press, { ...pressed, pads: [{ ...pressed.pads[0], down: ["South", "DPadRight", "LeftTrigger"] }] })[0], /buttons down/);
  assert.match(checkGamepadStep(press, { ...pressed, pad_presses: 0 })[0], /pressed 0 times/);
  // gilrs's dead zone moves a half to 0.44: fine on the ordinary build, not for the exact backend.
  const scaled = { ...pressed, pads: [{ ...pressed.pads[0], axes: { ...pressed.pads[0].axes, lx: 0.444 } }] };
  assert.match(checkGamepadStep(press, scaled)[0], /lx/);
  assert.deepEqual(checkGamepadStep(press, scaled, GILRS_AXIS_TOLERANCE), []);
  assert.match(checkGamepadStep(press, { ...scaled, pads: [{ ...scaled.pads[0], axes: { ...scaled.pads[0].axes, lx: -0.5 } }] }, GILRS_AXIS_TOLERANCE)[0], /lx/);
  assert.deepEqual(checkGamepadStep(release, { pads: [pad([], {})], pad_presses: 1 }), []);
  assert.deepEqual(checkGamepadStep(unplug, { pads: [] }), []);
});

/** A scope as a page has it: `performance.now()` from a clock the test moves. */
function fakeScope() {
  const clock = { now: 0 };
  return { scope: { navigator: {}, performance: { now: () => clock.now } }, clock };
}

test("a pad fed with rumble has a fake vibrationActuator that records its calls, and one without has none", () => {
  const { scope, clock } = fakeScope();
  installFakeGamepads(scope);
  scope.__ponkoFeedGamepads([fakeGamepad({ index: 0 }), fakeGamepad({ index: 1, rumble: false })]);
  const [first, second] = scope.navigator.getGamepads();
  assert.equal(second.vibrationActuator, undefined, "Firefox's pads have none");
  assert.equal(first.rumble, undefined, "the flag isn't a property of the browser's Gamepad");
  const actuator = first.vibrationActuator;
  assert.equal(actuator, scope.navigator.getGamepads()[0].vibrationActuator, "the same object every call");
  clock.now = 1000;
  actuator.playEffect("dual-rumble", { startDelay: 0, duration: 2000, weakMagnitude: 0.3, strongMagnitude: 0.8 });
  clock.now = 3000;
  actuator.reset();
  assert.deepEqual(scope.__ponkoRumbleCalls, [
    { kind: "play", index: 0, type: "dual-rumble", duration: 2000, strong: 0.8, weak: 0.3, delay: 0, at: 1000 },
    { kind: "reset", index: 0, at: 3000 },
  ]);
});

test("the fake actuator refuses what a browser refuses: an effect over its cap", async () => {
  const { scope } = fakeScope();
  installFakeGamepads(scope);
  scope.__ponkoFeedGamepads([fakeGamepad()]);
  const { vibrationActuator } = scope.navigator.getGamepads()[0];
  const over = { startDelay: 0, duration: EFFECT_CAP_MS + 1, weakMagnitude: 1, strongMagnitude: 1 };
  await assert.rejects(vibrationActuator.playEffect("dual-rumble", over), TypeError);
  assert.equal(scope.__ponkoRumbleCalls[0].kind, "rejected");
  assert.equal(await vibrationActuator.playEffect("dual-rumble", { ...over, duration: EFFECT_CAP_MS }), "complete");
});

const play = (at, extra = {}) => ({
  kind: "play", index: 0, type: "dual-rumble", duration: 2000, strong: 0.8, weak: 0.3, delay: 0, at, ...extra,
});
const stop = (at) => ({ kind: "reset", index: 0, at });

test("rumble passes when it is played again before the cap, as asked, and stopped at its end", () => {
  assert.deepEqual(checkRumbleCalls([play(0), play(1000), play(2000), stop(2500)]), []);
});

test("rumble fails when it is not played again before the browser's cap ends it", () => {
  // One effect, then the stop: the game gave up, or never re-sent.
  assert.match(checkRumbleCalls([play(0), stop(2500)])[0], /not again/);
  // A gap as long as the effect: the pad went quiet in between.
  assert.ok(checkRumbleCalls([play(0), play(2100), stop(2500)]).some((p) => /went quiet/.test(p)));
  // An effect over the cap, which a browser refuses.
  const refused = { ...play(0, { duration: 6000 }), kind: "rejected" };
  assert.ok(checkRumbleCalls([refused, play(1000), stop(2500)]).some((p) => /refused/.test(p)));
  assert.ok(checkRumbleCalls([play(0, { duration: 6000 }), play(1000), stop(2500)]).some((p) => /over the 5000 ms cap/.test(p)));
});

test("rumble fails on wrong levels, no stop, or a stop at the wrong time", () => {
  assert.ok(checkRumbleCalls([play(0, { strong: 1 }), play(1000), stop(2500)]).some((p) => /levels/.test(p)));
  assert.ok(checkRumbleCalls([play(0), play(1000), play(2000)]).some((p) => /not the stop/.test(p)));
  assert.ok(checkRumbleCalls([play(0), play(1000), stop(1200)]).some((p) => /it stopped after/.test(p)));
});
