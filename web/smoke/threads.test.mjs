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
  checkPassTimes,
  checkServiceWorker,
  PASSES,
  passRows,
  passTable,
  SW_DIR,
  SW_RELOAD_MESSAGE,
  checkReady,
  checkGamepadStep,
  checkRumbleCalls,
  checkSameResult,
  checkTriggerCapabilities,
  checkTriggerRumbleCalls,
  checkWarningBuild,
  denyWarnings,
  EFFECT_CAP_MS,
  fakeGamepad,
  FAKE_PAD_ID,
  GAMEPAD_STEPS,
  deadZone,
  installFakeGamepads,
  WARNING_ITEM,
  median,
  parseArgs,
  PLAIN_DIR,
  summarize,
  summaryTable,
  TRIGGER_PADS,
  TRIGGER_RUMBLE,
  triggerRumbleDone,
  addRustFlag,
  allocatorTable,
  checkAllocator,
  checkSides,
  SWEPT,
  THREAD_HEAPS_CFG,
  THREAD_HEAPS_DIR,
  sweepDone,
  sweepTable,
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
  hier_shared: true,
  passes_shared: [
    "spatial/shared_rebuild",
    "flow/shared_directions",
    "three/shared_cull",
    "skeleton2d/shared_draw",
    "physics3d/shared_characters",
    "three/shared_bake",
  ],
  pass_ms: Object.fromEntries(PASSES.map((p, i) => [p, 1 + i])),
  frame3d: [5210, 9870, 64],
  save_bytes: 437010,
  schedule_threads: true,
  wait_off_page: true,
  job_off_page: true,
  group: "ready",
  group_off_page: true,
  group_bytes: 966,
  checksum: 88451852.95592952,
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
  hier_shared: false,
  passes_shared: [],
  pass_ms: Object.fromEntries(PASSES.map((p, i) => [p, 4 * (1 + i)])),
  frame3d: [5210, 9870, 64],
  save_bytes: 437010,
  schedule_threads: false,
  wait_off_page: false,
  job_off_page: false,
  group: "ready",
  group_off_page: false,
  group_bytes: 966,
  checksum: 88451852.95592952,
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
  assert.deepEqual(parseArgs(["--bench", "--workloads", "boids"]).workloads, ["boids"], "the boids example's tick");
  assert.ok(parseArgs(["--bench"]).workloads.includes("boids"));
  assert.throws(() => parseArgs(["--repeat", "0"]), UsageError);
  assert.throws(() => parseArgs(["--case", "stall"]), UsageError);
  assert.equal(parseArgs(["--case", "gamepads"]).case, "gamepads");
  assert.equal(parseArgs(["--case", "warning"]).case, "warning");
  assert.equal(parseArgs(["--case", "gamepads-plain"]).dir, PLAIN_DIR, "the ordinary build is its own folder");
  assert.equal(parseArgs(["--case", "gamepads-plain", "--dir", "x"]).dir, "x");
  assert.equal(parseArgs(["--case", "gamepads"]).dir, "target/web/web_threads");
  assert.equal(parseArgs(["--case", "service-worker"]).dir, SW_DIR, "the build with the service worker is its own folder");
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
  assert.ok(problems.some((p) => p.includes("hierarchy shared no level")));
  assert.ok(problems.some((p) => p.includes("own passes that went through the frame pool's runner: none")));
  assert.ok(problems.some((p) => p.includes("a job ran on the page's thread")));
  assert.ok(problems.some((p) => p.includes("decoded on the page's thread")));
  // A group that couldn't read its file on a worker fails the check by name.
  const failed = checkIsolated({ ...isolatedRun, group: 'failed: "crab.png" not found in assets/' });
  assert.deepEqual(failed, [`the asset group from assets/ didn't load (failed: "crab.png" not found in assets/)`]);
  const without = (name) => isolatedRun.passes_shared.filter((p) => p !== name);
  // One of the engine's passes is not "at least two".
  assert.deepEqual(checkIsolated({ ...isolatedRun, passes_shared: ["three/shared_cull"] }), [
    "the engine's own passes that went through the frame pool's runner: three/shared_cull, not at least 2",
    "skeleton2d/shared_draw didn't go through the frame pool's runner",
    "physics3d/shared_characters didn't go through the frame pool's runner",
    "three/shared_bake didn't go through the frame pool's runner",
  ]);
  // The bake, the skeletons and the characters each by name.
  for (const pass of ["skeleton2d/shared_draw", "physics3d/shared_characters", "three/shared_bake"]) {
    assert.deepEqual(checkIsolated({ ...isolatedRun, passes_shared: without(pass) }), [
      `${pass} didn't go through the frame pool's runner`,
    ]);
  }
  // The 3D culling and the save must each go there, by name.
  assert.deepEqual(checkIsolated({ ...isolatedRun, passes_shared: without("three/shared_cull") }), [
    "the 3D culling didn't go through the frame pool's runner (three/shared_cull)",
  ]);
  // The JSON save must stay on the page's thread, where it is faster.
  assert.deepEqual(checkIsolated({ ...isolatedRun, passes_shared: [...isolatedRun.passes_shared, "reflect/shared_save"] }), [
    "the JSON world save went to the frame pool's workers (reflect/shared_save), where it is slower",
  ]);
  // A build without the 3D features collects no 3D frame.
  assert.deepEqual(checkIsolated({ ...isolatedRun, frame3d: null }), ["the 3D frame found no models to draw (frame3d: null)"]);
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
  assert.equal(checkSameResult(isolatedRun, { ...plainRun, frame3d: [5210, 9871, 64] }).length, 1);
  assert.equal(checkSameResult(isolatedRun, { ...plainRun, save_bytes: 1 }).length, 1);
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
    trigger_rumble: false,
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
  assert.match(checkGamepadStep(connect, { pads: [{ ...pad([], {}), trigger_rumble: true }] })[0], /trigger rumble capability/);
  const pressed = { pads: [pad(["DPadRight", "South"], { lx: 0.5, ly: -1, ry: 0.65 / 0.9, lt: 0.15 / 0.9 })], pad_presses: 1 };
  assert.deepEqual(checkGamepadStep(press, pressed), []);
  // Y up must stay negative, and a quarter-pulled trigger isn't a press.
  assert.match(checkGamepadStep(press, { ...pressed, pads: [{ ...pressed.pads[0], axes: { ...pressed.pads[0].axes, ly: 1 } }] })[0], /ly/);
  assert.match(checkGamepadStep(press, { ...pressed, pads: [{ ...pressed.pads[0], down: ["South", "DPadRight", "LeftTrigger"] }] })[0], /buttons down/);
  assert.match(checkGamepadStep(press, { ...pressed, pad_presses: 0 })[0], /pressed 0 times/);
  // The fed values without the dead zone are wrong: the engine reads 0.7222 and 0.1667.
  const raw = { ...pressed, pads: [{ ...pressed.pads[0], axes: { ...pressed.pads[0].axes, ry: 0.75, lt: 0.25 } }] };
  assert.equal(checkGamepadStep(press, raw).length, 2);
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

test("a pad fed with trigger rumble lists trigger-rumble in its actuator's effects and records the trigger levels", async () => {
  const { scope, clock } = fakeScope();
  installFakeGamepads(scope);
  scope.__ponkoFeedGamepads(TRIGGER_PADS());
  const [triggers, body] = scope.navigator.getGamepads();
  assert.deepEqual(triggers.vibrationActuator.effects, ["dual-rumble", "trigger-rumble"]);
  assert.deepEqual(body.vibrationActuator.effects, ["dual-rumble"]);
  assert.equal(triggers.triggerRumble, undefined, "the flag isn't a property of the browser's Gamepad");
  clock.now = 500;
  const params = { startDelay: 0, duration: 2000, strongMagnitude: 0, weakMagnitude: 0.2, leftTrigger: 0.4, rightTrigger: 0.9 };
  assert.equal(await triggers.vibrationActuator.playEffect("trigger-rumble", params), "complete");
  await assert.rejects(body.vibrationActuator.playEffect("trigger-rumble", params), TypeError, "not one of its effects");
  assert.deepEqual(scope.__ponkoRumbleCalls, [
    { kind: "play", index: 0, type: "trigger-rumble", duration: 2000, strong: 0, weak: 0.2, left: 0.4, right: 0.9, delay: 0, at: 500 },
    { kind: "rejected", index: 1, type: "trigger-rumble", duration: 2000, strong: 0, weak: 0.2, left: 0.4, right: 0.9, delay: 0, at: 500 },
  ]);
});

test("both pads must report their trigger capability", () => {
  const pad = (slot, trigger_rumble) => ({ slot, rumble: true, trigger_rumble });
  assert.deepEqual(checkTriggerCapabilities({ pads: [pad(1, false), pad(0, true)] }), []);
  assert.match(checkTriggerCapabilities({ pads: [pad(0, false), pad(1, false)] })[0], /plays trigger-rumble reports/);
  assert.match(checkTriggerCapabilities({ pads: [pad(0, true), pad(1, true)] })[0], /doesn't reports/);
  assert.match(checkTriggerCapabilities({ pads: [pad(0, true)] })[0], /two pads/);
});

const want = TRIGGER_RUMBLE;
const triggerPlay = (at, extra = {}) => ({
  kind: "play", index: 0, type: "trigger-rumble", duration: 2000, strong: want.strong, weak: want.weak, left: want.left, right: want.right, delay: 0, at, ...extra,
});
const bodyPlay = (at, extra = {}) => ({
  kind: "play", index: 1, type: "dual-rumble", duration: 2000, strong: want.strong, weak: want.weak, delay: 0, at, ...extra,
});
const reset = (index, at) => ({ kind: "reset", index, at });
const ms = want.seconds * 1000;

test("trigger rumble passes when the pad with trigger motors gets trigger-rumble and the other dual-rumble, both stopped", () => {
  const calls = [triggerPlay(0), bodyPlay(1), triggerPlay(1000), bodyPlay(1001), reset(0, ms), reset(1, ms + 1)];
  assert.deepEqual(checkTriggerRumbleCalls(calls), []);
  assert.ok(triggerRumbleDone(calls));
  assert.ok(!triggerRumbleDone(calls.slice(0, -1)), "the second pad isn't stopped yet");
});

test("trigger rumble fails when the fallback differs from the desktop's, or levels or the stop are wrong", () => {
  // The pad without trigger motors got trigger-rumble (refused by a browser) or nothing.
  const refused = { ...triggerPlay(1, { index: 1 }), kind: "rejected" };
  assert.ok(checkTriggerRumbleCalls([triggerPlay(0), refused, reset(0, ms), reset(1, ms)]).some((p) => /refused/.test(p)));
  assert.ok(checkTriggerRumbleCalls([triggerPlay(0), reset(0, ms), reset(1, ms)]).some((p) => /pad 1 was never played/.test(p)));
  // The pad with trigger motors got only dual-rumble: its triggers were dropped.
  assert.ok(checkTriggerRumbleCalls([bodyPlay(0, { index: 0 }), bodyPlay(1), reset(0, ms), reset(1, ms)]).some((p) => /pad 0 got a dual-rumble/.test(p)));
  assert.ok(checkTriggerRumbleCalls([triggerPlay(0, { right: 0.1 }), bodyPlay(1), reset(0, ms), reset(1, ms)]).some((p) => /trigger levels/.test(p)));
  assert.ok(checkTriggerRumbleCalls([triggerPlay(0), bodyPlay(1, { weak: 0 }), reset(0, ms), reset(1, ms)]).some((p) => /pad 1: body levels/.test(p)));
  assert.ok(checkTriggerRumbleCalls([triggerPlay(0), bodyPlay(1), reset(1, ms)]).some((p) => /pad 0's last call/.test(p)));
  assert.ok(checkTriggerRumbleCalls([triggerPlay(0), bodyPlay(1), reset(0, 200), reset(1, ms)]).some((p) => /pad 0 stopped after/.test(p)));
});

test("the expected axes take out the engine's dead zone as gilrs does", () => {
  assert.equal(deadZone(0.05), 0);
  assert.equal(deadZone(-0.1), 0);
  assert.ok(Math.abs(deadZone(0.55) - 0.5) < 1e-12);
  assert.ok(Math.abs(deadZone(-0.55) + 0.5) < 1e-12);
  assert.equal(deadZone(1), 1);
  assert.equal(deadZone(-1.5), -1, "past the edge reads full");
});

test("every timed pass needs a time, in both runs", () => {
  assert.deepEqual(checkPassTimes(isolatedRun), []);
  assert.deepEqual(checkPassTimes(plainRun), []);
  const { ["three/shared_bake"]: _, ...rest } = isolatedRun.pass_ms;
  assert.deepEqual(checkPassTimes({ ...isolatedRun, pass_ms: rest }), [`no time for three/shared_bake (pass_ms: ${JSON.stringify(rest)})`]);
  assert.equal(checkPassTimes({ ...isolatedRun, pass_ms: undefined }).length, 1);
  assert.ok(checkIsolated({ ...isolatedRun, pass_ms: rest }).some((p) => p.includes("no time for three/shared_bake")));
});

test("the pass table says what was shared out and compares its times on both pages", () => {
  const rows = passRows({ ...isolatedRun, passes_shared: isolatedRun.passes_shared.slice(1) }, plainRun);
  assert.deepEqual(rows.map((r) => r.pass), PASSES);
  assert.equal(rows[0].shared, true, "the hierarchy by hier_shared");
  assert.equal(rows[1].shared, false, "spatial/shared_rebuild's scope didn't open");
  assert.equal(rows[2].shared, true);
  assert.equal(rows[2].speedup, 4);
  const table = passTable(rows, 13);
  assert.match(table, /\| Pass \(scope\) \| Shared out \| 13 threads \(ms\) \| Page's thread alone \(ms\) \| Speed-up \|/);
  assert.match(table, /\| `flow\/shared_directions` \| yes \| 3\.00 \| 12\.00 \| 4\.0x \|/);
  assert.match(passTable(passRows(isolatedRun, { ...plainRun, pass_ms: {} }), 13), /\| n\/a \| n\/a \|/);
});

test("the service worker case passes only after the reload, isolated, with the work on the workers", () => {
  const reloaded = `info: ${SW_RELOAD_MESSAGE}, through the service worker, for cross-origin isolation`;
  assert.deepEqual(checkServiceWorker(isolatedRun, reloaded), []);
  assert.deepEqual(checkServiceWorker(isolatedRun, ""), [
    `the console doesn't say the page reloaded through the service worker ("${SW_RELOAD_MESSAGE}")`,
  ]);
  assert.deepEqual(checkServiceWorker(plainRun, reloaded), ["the page isn't cross-origin isolated after the reload (isolated: false)"]);
  assert.ok(checkServiceWorker({ ...isolatedRun, par_threads: 0 }, reloaded).length > 0, "the isolated run's checks apply");
});

/** A threshold sweep where every pass stays below and is shared from its threshold, on the same hash. */
function sweptRun() {
  const sides = SWEPT.flatMap((pass) => [
    { pass, size: 15, side: "below", shared: false, hash: "1", serial_hash: "1" },
    { pass, size: 16, side: "above", shared: true, hash: "2", serial_hash: "2" },
  ]);
  const sweep = SWEPT.map((pass) => ({ pass, size: 16, serial_ms: 0.3, shared_ms: 0.1 }));
  return { isolated: true, threads: 13, done: true, sweep, sides };
}

test("the threshold cases parse, and the allocator's second build has its own folder and flag", () => {
  assert.equal(parseArgs(["--case", "thresholds"]).case, "thresholds");
  assert.equal(parseArgs(["--case", "allocator"]).dir, "target/web/web_threads", "the standard allocator's build is the default one");
  assert.notEqual(THREAD_HEAPS_DIR, "target/web/web_threads");
  assert.equal(addRustFlag({}, THREAD_HEAPS_CFG).RUSTFLAGS, "--cfg ponko_web_thread_heaps");
  assert.equal(addRustFlag({ RUSTFLAGS: "-D warnings" }, THREAD_HEAPS_CFG).RUSTFLAGS, "-D warnings --cfg ponko_web_thread_heaps");
  const encoded = addRustFlag({ CARGO_ENCODED_RUSTFLAGS: "-D\x1fwarnings" }, THREAD_HEAPS_CFG);
  assert.equal(encoded.CARGO_ENCODED_RUSTFLAGS, "-D\x1fwarnings\x1f--cfg\x1fponko_web_thread_heaps");
  assert.equal(sweepDone(null), false);
  assert.equal(sweepDone({ done: false }), false);
  assert.equal(sweepDone({ done: true }), true);
});

test("a threshold sweep passes only when each pass is shared from its threshold and ends on the page's hash", () => {
  assert.deepEqual(checkSides(sweptRun()), []);
  const sharedBelow = sweptRun();
  sharedBelow.sides[0].shared = true;
  assert.match(checkSides(sharedBelow).join(), /shared out below its threshold/);
  const notShared = sweptRun();
  notShared.sides[3].shared = false;
  assert.match(checkSides(notShared).join(), /stayed on the page's thread at its threshold/);
  const otherHash = sweptRun();
  otherHash.sides[5].hash = "3";
  assert.match(checkSides(otherHash).join(), /ended on 3, but 2 on the page's thread alone/);
  const oneSide = sweptRun();
  oneSide.sides = oneSide.sides.filter((r) => !(r.pass === SWEPT[1] && r.side === "below"));
  assert.match(checkSides(oneSide).join(), /wasn't run on both sides/);
  assert.match(checkSides({ ...sweptRun(), isolated: false }).join(), /needs an isolated page/);
  assert.match(checkSides({ ...sweptRun(), sweep: [] }).join(), /wasn't timed/);
});

test("the sweep table says which side is faster, and calls differences inside the noise the same", () => {
  const table = sweepTable([
    { pass: "a", size: 1, serial_ms: 0.1, shared_ms: 0.3 },
    { pass: "a", size: 2, serial_ms: 0.5, shared_ms: 0.49 },
    { pass: "a", size: 3, serial_ms: 2, shared_ms: 0.5 },
  ], 13);
  assert.match(table, /13 threads/);
  assert.match(table, /\| 1 \| 0\.100 \| 0\.300 \| page's thread \|/);
  assert.match(table, /\| 2 \| .* \| same \|/);
  assert.match(table, /\| 3 \| .* \| shared \|/);
});

test("the allocator case needs the standard allocator in the default build, heaps in the other, and the same saves", () => {
  const run = (perThread) => ({
    isolated: true,
    threads: 13,
    per_thread_allocator: perThread,
    alloc: { same_save: true, save_page_ms: 20, save_workers_ms: perThread ? 5 : 200, parse_ms: 9, par_alloc_ms: 1 },
  });
  assert.deepEqual(checkAllocator(run(true), run(false)), []);
  assert.match(checkAllocator(run(false), run(false)).join(), /ponko_web_thread_heaps doesn't have the per-thread allocator/);
  assert.match(checkAllocator(run(true), run(true)).join(), /the default threaded build has the per-thread allocator/);
  const differs = run(true);
  differs.alloc.same_save = false;
  assert.match(checkAllocator(differs, run(false)).join(), /per-thread: the save on the workers differs/);
  const table = allocatorTable(run(true), run(false), { perThread: 2048, system: 1024 });
  assert.match(table, /\| JSON save, workers \| 200\.00 \| 5\.00 \|/);
  assert.match(table, /\| `game_bg.wasm` \| 1 KiB \| 2 KiB \|/);
});
