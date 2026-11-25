// The parts of the threads runner (threads.mjs) that need no browser:
// arguments, what a run of examples/web_threads.rs must report on a
// cross-origin isolated page and on one that isn't, and the benchmark
// summary. threads.test.mjs tests this file (`node --test web/smoke`).

import { UsageError } from "./lib.mjs";

/** What makes a page cross-origin isolated, so the game may start workers. */
export const ISOLATION_HEADERS = {
  "cross-origin-opener-policy": "same-origin",
  "cross-origin-embedder-policy": "require-corp",
};

/** The fallback warning the engine logs on a page that isn't isolated. */
export const FALLBACK_WARNING = "it runs on the page's thread alone";

/** What the engine logs when a worker fails and it stops the game. */
export const STOP_MESSAGE = "the game stops";

/** The fixture's benchmark workloads (examples/web_threads.rs). */
export const WORKLOADS = ["par", "sweep", "interleaved", "jobs", "boids"];

/** Ticks a check waits for: past the checksum (tick 120) and a few jobs. */
export const CHECK_TICKS = 150;

/**
 * The `--case`s: the browser ones, and `warning`, a build with no browser.
 * `gamepads-plain` is `gamepads` on the ordinary web build (no threads),
 * whose pads come from the same backend, built without shared memory.
 */
export const CASES = ["worker-panic", "gamepads", "gamepads-plain", "service-worker", "warning", "thresholds", "allocator"];

/**
 * The fixture's threaded build has these features: its 3D culling and
 * light bake need the 3D renderer, its characters 3D physics, and the
 * culling and skeleton drawing their headless handles.
 */
export const FIXTURE_FEATURES = "3d,physics3d,bench-internals";

/** Where the ordinary build of the fixture goes for `--case gamepads-plain`. */
export const PLAIN_DIR = "target/web/web_threads_plain";

/**
 * Where the threaded build with `ponko-isolate.js` goes for `--case
 * service-worker` (`web/build.sh --threads --isolation-worker`).
 */
export const SW_DIR = "target/web/web_threads_sw";

/**
 * Where `--case allocator` builds the fixture a second time with one heap
 * per thread (`THREAD_HEAPS_CFG` added to its flags), to compare with the
 * standard allocator of `target/web/web_threads`.
 */
export const THREAD_HEAPS_DIR = "target/web/web_threads_heaps";
export const THREAD_HEAPS_CFG = "--cfg ponko_web_thread_heaps";

/** The passes `--case thresholds` sweeps (the fixture's `?sweep=thresholds`). */
export const SWEPT = ["hierarchy/shared_level", "skeleton2d/shared_draw", "physics3d/shared_characters"];

/** What `ponko-isolate.js` logs when it reloads the page through its worker. */
export const SW_RELOAD_MESSAGE = "ponko-isolate.js: reloading once";

/**
 * The parallel passes the fixture times (`pass_ms`), by the scope each opens
 * on the frame pool: the hierarchy's update and the engine's own passes.
 */
export const PASSES = [
  "hierarchy/shared_level",
  "spatial/shared_rebuild",
  "flow/shared_directions",
  "three/shared_cull",
  "reflect/save",
  "skeleton2d/shared_draw",
  "physics3d/shared_characters",
  "three/shared_bake",
];

export const USAGE =
  "usage: threads.mjs [--dir <site>] [--build] [--browser chromium|firefox|webkit]\n" +
  "                   [--check] [--case worker-panic|gamepads|gamepads-plain|service-worker|warning|thresholds|allocator]\n" +
  "                   [--bench [--workloads par,sweep,interleaved,jobs,boids] [--threads 1,<n>] [--repeat <r>]]\n" +
  "                   [--out <dir>] [--timeout <s>]";

export function parseArgs(argv) {
  const args = {
    dir: null,
    build: false,
    browser: "chromium",
    mode: "check",
    case: null,
    workloads: [...WORKLOADS],
    threads: null,
    repeat: 3,
    out: "target/web-smoke/threads",
    timeout: 120,
  };
  const value = (i, flag) => {
    const v = argv[i + 1];
    if (v === undefined || v.startsWith("--")) throw new UsageError(`${flag} needs a value`);
    return v;
  };
  const number = (v, flag) => {
    const n = Number(v);
    if (!Number.isInteger(n) || n < 1) throw new UsageError(`${flag} takes a whole number, not ${v}`);
    return n;
  };
  for (let i = 0; i < argv.length; i++) {
    const flag = argv[i];
    switch (flag) {
      case "--dir": args.dir = value(i++, flag); break;
      case "--build": args.build = true; break;
      case "--browser": args.browser = value(i++, flag); break;
      case "--check": args.mode = "check"; break;
      case "--bench": args.mode = "bench"; break;
      case "--case": {
        args.case = value(i++, flag);
        if (!CASES.includes(args.case)) throw new UsageError(`unknown case ${args.case} (${CASES.join(", ")})`);
        break;
      }
      case "--workloads": {
        args.workloads = value(i++, flag).split(",");
        const bad = args.workloads.filter((w) => !WORKLOADS.includes(w));
        if (bad.length) throw new UsageError(`unknown workload ${bad.join(", ")} (${WORKLOADS.join(", ")})`);
        break;
      }
      case "--threads": args.threads = value(i++, flag).split(",").map((t) => number(t, flag)); break;
      case "--repeat": args.repeat = number(value(i++, flag), flag); break;
      case "--out": args.out = value(i++, flag); break;
      case "--timeout": args.timeout = number(value(i++, flag), flag); break;
      default: throw new UsageError(`unknown argument ${flag}`);
    }
  }
  if (args.case && args.mode !== "check") throw new UsageError("--case goes with --check");
  args.dir ??= args.case === "gamepads-plain" ? PLAIN_DIR : args.case === "service-worker" ? SW_DIR : "target/web/web_threads";
  return args;
}

/** Whether a check run has reported everything it is checked on. */
export function checkReady(state) {
  return (
    state != null &&
    state.ticks >= CHECK_TICKS &&
    state.checksum != null &&
    state.wait_off_page != null &&
    state.job_off_page != null &&
    state.group != null
  );
}

/**
 * The problems with a check run on a cross-origin isolated page: the game
 * must have started workers and run query items, batch systems, a shared
 * hierarchy level, the engine's own parallel passes (the retained 3D
 * culling, the 2D skeleton drawing, the 3D characters and the light bake
 * among them; the JSON world save never) and jobs on them, never on the
 * page's thread.
 */
export function checkIsolated(state) {
  const problems = [];
  const expect = (ok, what) => ok || problems.push(what);
  expect(state.isolated === true, `the page isn't cross-origin isolated (isolated: ${state.isolated})`);
  expect(state.threads >= 2, `the engine started ${state.threads} worker threads, not at least 2`);
  expect(state.par_threads >= 2, `parallel query items ran on ${state.par_threads} worker threads, not at least 2`);
  expect(state.par_on_page === false, "parallel query items ran on the page's thread");
  expect(state.batch_threads >= 2, `batch systems ran on ${state.batch_threads} worker threads, not at least 2`);
  expect(state.batch_on_page === false, "batch systems ran on the page's thread");
  expect(state.batch_threaded_runs > 0, "the scheduler sent no batch to the workers");
  expect(state.schedule_threads === true, "the scheduler says it has no threads");
  expect(state.hier_shared === true, "the hierarchy shared no level out to the workers");
  const passes = state.passes_shared ?? [];
  expect(
    passes.length >= 2,
    `the engine's own passes that went through the frame pool's runner: ${passes.length ? passes.join(", ") : "none"}, not at least 2`,
  );
  // Sized by the frame pool's threads, not rayon's (1 on the page).
  expect(passes.includes("three/shared_cull"), "the 3D culling didn't go through the frame pool's runner (three/shared_cull)");
  // The JSON save stays on the page's thread: shared out, the browser's
  // one allocator lock made it about 10 times slower.
  expect(!passes.includes("reflect/shared_save"), "the JSON world save went to the frame pool's workers (reflect/shared_save), where it is slower");
  for (const pass of ["skeleton2d/shared_draw", "physics3d/shared_characters", "three/shared_bake"]) {
    expect(passes.includes(pass), `${pass} didn't go through the frame pool's runner`);
  }
  problems.push(...checkPassTimes(state));
  expect(Array.isArray(state.frame3d) && state.frame3d[0] > 0, `the 3D frame found no models to draw (frame3d: ${JSON.stringify(state.frame3d)})`);
  expect(state.wait_off_page === true, "the job waited for in setup ran on the page's thread");
  expect(state.job_off_page === true, "a job ran on the page's thread");
  expect(state.group === "ready", `the asset group from assets/ didn't load (${state.group})`);
  // Where it was decoded only counts once it loaded.
  expect(state.group !== "ready" || state.group_off_page === true, "the asset group's file was decoded on the page's thread");
  return problems;
}

/**
 * The problems with a check run on a page that isn't isolated: the same
 * build must run, on the page's thread alone, and say why in the console.
 */
export function checkFallback(state, consoleText) {
  const problems = [];
  const expect = (ok, what) => ok || problems.push(what);
  expect(state.isolated === false, `the page is cross-origin isolated (isolated: ${state.isolated})`);
  expect(state.threads === 0, `the engine started ${state.threads} worker threads on a page that isn't isolated`);
  expect(state.par_threads === 0 && state.par_on_page === true, "parallel query items didn't all run on the page's thread");
  expect(state.batch_threads === 0 && state.batch_threaded_runs === 0, "batch systems went to other threads");
  expect(state.wait_off_page === false && state.job_off_page === false, "jobs didn't run on the page's thread");
  expect(state.group === "ready", `the asset group from assets/ didn't load (${state.group})`);
  expect(state.group !== "ready" || state.group_off_page === false, "the asset group's file wasn't decoded on the page's thread");
  expect(consoleText.includes(FALLBACK_WARNING), `the console doesn't say why the game runs on one thread ("${FALLBACK_WARNING}")`);
  return problems;
}

/** Every pass in `PASSES` must have been timed (`pass_ms`, in milliseconds). */
export function checkPassTimes(state) {
  const times = state.pass_ms ?? {};
  const missing = PASSES.filter((p) => !(Number.isFinite(times[p]) && times[p] >= 0));
  return missing.length ? [`no time for ${missing.join(", ")} (pass_ms: ${JSON.stringify(times)})`] : [];
}

/**
 * One row per pass in `PASSES`: whether the isolated run shared it out to
 * the workers (its scope opened there; the hierarchy's `hier_shared`), and
 * its time with threads and on the page's thread alone, from the two runs.
 */
export function passRows(isolated, plain) {
  const shared = new Set(isolated.passes_shared ?? []);
  return PASSES.map((pass) => {
    const threaded = isolated.pass_ms?.[pass] ?? null;
    const alone = plain.pass_ms?.[pass] ?? null;
    return {
      pass,
      shared: pass === "hierarchy/shared_level" ? isolated.hier_shared === true : shared.has(pass),
      threaded_ms: threaded,
      alone_ms: alone,
      speedup: threaded > 0 && alone != null ? alone / threaded : null,
    };
  });
}

/** `passRows` as a Markdown table. */
export function passTable(rows, threads) {
  const ms = (v) => (v == null ? "n/a" : v.toFixed(2));
  const lines = [
    `| Pass (scope) | Shared out | ${threads} threads (ms) | Page's thread alone (ms) | Speed-up |`,
    "|---|---|---|---|---|",
  ];
  for (const r of rows) {
    const speedup = r.speedup == null ? "n/a" : `${r.speedup.toFixed(1)}x`;
    lines.push(`| \`${r.pass}\` | ${r.shared ? "yes" : "no"} | ${ms(r.threaded_ms)} | ${ms(r.alone_ms)} | ${speedup} |`);
  }
  return lines.join("\n");
}

/**
 * The problems with a check run served without the headers, with
 * `ponko-isolate.js`: the page must have reloaded once through the service
 * worker and then be isolated, with the game's work on the workers.
 */
export function checkServiceWorker(state, consoleText) {
  const problems = [];
  if (!consoleText.includes(SW_RELOAD_MESSAGE)) {
    problems.push(`the console doesn't say the page reloaded through the service worker ("${SW_RELOAD_MESSAGE}")`);
  }
  if (state.isolated !== true) {
    problems.push(`the page isn't cross-origin isolated after the reload (isolated: ${state.isolated})`);
    return problems;
  }
  problems.push(...checkIsolated(state));
  return problems;
}

/** The two runs must have simulated the same world and read the same file. */
export function checkSameResult(isolated, fallback) {
  const problems = [];
  if (isolated.checksum !== fallback.checksum) {
    problems.push(`the checksums differ: ${isolated.checksum} with threads, ${fallback.checksum} without`);
  }
  if (JSON.stringify(isolated.frame3d) !== JSON.stringify(fallback.frame3d)) {
    problems.push(`the 3D frames differ: ${JSON.stringify(isolated.frame3d)} with threads, ${JSON.stringify(fallback.frame3d)} without`);
  }
  if (isolated.save_bytes !== fallback.save_bytes) {
    problems.push(`the JSON saves differ in length: ${isolated.save_bytes} bytes with threads, ${fallback.save_bytes} without`);
  }
  if (isolated.group_bytes !== fallback.group_bytes) {
    problems.push(`the asset group read ${isolated.group_bytes} bytes with threads, ${fallback.group_bytes} without`);
  }
  return problems;
}

/** Nearest-rank percentile of sorted numbers. */
function rank(sorted, p) {
  return sorted[Math.min(sorted.length - 1, Math.max(0, Math.ceil((p / 100) * sorted.length) - 1))];
}

/** The median of some numbers. */
export function median(values) {
  const sorted = [...values].sort((a, b) => a - b);
  return rank(sorted, 50);
}

/**
 * One row per workload and thread count, from page loads `{workload,
 * threads, runs_ms}`: the median of each load's median, the spread of
 * those medians (`(max - min) / median`), and the speed-up against one
 * thread with whether it is beyond the noise (the two spreads together).
 */
export function summarize(loads) {
  const groups = new Map();
  for (const load of loads) {
    const key = `${load.workload}/${load.threads}`;
    if (!groups.has(key)) groups.set(key, { workload: load.workload, threads: load.threads, medians: [] });
    groups.get(key).medians.push(median(load.runs_ms));
  }
  const rows = [...groups.values()].map((g) => {
    const m = median(g.medians);
    const spread = g.medians.length > 1 ? (Math.max(...g.medians) - Math.min(...g.medians)) / m : null;
    return { ...g, median_ms: m, min_ms: Math.min(...g.medians), max_ms: Math.max(...g.medians), spread };
  });
  for (const row of rows) {
    const one = rows.find((r) => r.workload === row.workload && r.threads <= 1);
    if (!one || one === row) continue;
    row.speedup = one.median_ms / row.median_ms;
    const noise = (one.spread ?? Infinity) + (row.spread ?? Infinity);
    row.beyond_noise = Math.abs(row.speedup - 1) > noise;
  }
  return rows.sort((a, b) => a.workload.localeCompare(b.workload) || a.threads - b.threads);
}

/** The summary as a Markdown table. */
export function summaryTable(rows) {
  const ms = (v) => v.toFixed(2);
  const lines = [
    "| Workload | Threads | Median (ms) | Min–max of loads (ms) | Spread | Speed-up vs 1 |",
    "|---|---|---|---|---|---|",
  ];
  for (const r of rows) {
    const speedup = r.speedup == null ? "" : `${r.speedup.toFixed(2)}x${r.beyond_noise ? "" : " (within noise)"}`;
    const spread = r.spread == null ? "n/a" : `${(r.spread * 100).toFixed(0)}%`;
    lines.push(`| ${r.workload} | ${r.threads} | ${ms(r.median_ms)} | ${ms(r.min_ms)}–${ms(r.max_ms)} | ${spread} | ${speedup} |`);
  }
  return lines.join("\n");
}

/**
 * `env` with `-D warnings` added where cargo reads it: the first of
 * CARGO_ENCODED_RUSTFLAGS, RUSTFLAGS and the wasm target's flags that is
 * set, else RUSTFLAGS. `web/build.sh --threads` then adds its own flags to
 * the same variable, so a warning fails the threaded build.
 */
export function denyWarnings(env) {
  const out = { ...env };
  const set = (name) => (out[name] ?? "") !== "";
  const target = "CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS";
  if (set("CARGO_ENCODED_RUSTFLAGS")) {
    const words = out.CARGO_ENCODED_RUSTFLAGS.split("\x1f");
    if (!/-D\x1fwarnings|-Dwarnings/.test(words.join("\x1f"))) out.CARGO_ENCODED_RUSTFLAGS = [...words, "-D", "warnings"].join("\x1f");
  } else {
    const name = !set("RUSTFLAGS") && set(target) ? target : "RUSTFLAGS";
    const now = out[name] ?? "";
    if (!/-D\s*warnings/.test(now)) out[name] = `${now} -D warnings`.trim();
  }
  return out;
}

/** The warning self-test's example and the unused function it must fail on. */
export const WARNING_FIXTURE = "web_threads_warning";
export const WARNING_ITEM = "seen_only_by_a_threaded_build";

/**
 * The problems with the warning self-test's build (`status`, its whole
 * output): it must fail, on the fixture's unused function, because
 * `-D warnings` turned that warning into an error.
 */
export function checkWarningBuild(status, output) {
  const problems = [];
  if (status === 0) {
    problems.push(`the threaded build of ${WARNING_FIXTURE} passed: its warning didn't fail it (-D warnings or the atomics were dropped)`);
    return problems;
  }
  if (!output.includes(WARNING_ITEM) && /Finished `release` profile/.test(output)) {
    problems.push(`cargo compiled ${WARNING_FIXTURE} without failing on its warning (-D warnings or the atomics were dropped); a later step failed`);
  } else if (!output.includes(WARNING_ITEM)) {
    problems.push(`the threaded build failed, but not on ${WARNING_ITEM} (a build that compiles it has atomics); see the output above`);
  } else if (!output.includes("implied by `-D warnings`")) {
    problems.push(`${WARNING_ITEM} was reported, but not as an error from -D warnings`);
  }
  return problems;
}

/** The browser `id` of the fed pad: Chromium's form, with USB ids. */
export const FAKE_PAD_ID = "Xbox Wireless Controller (STANDARD GAMEPAD Vendor: 045e Product: 0b13)";

/**
 * A plain object shaped like the browser's `Gamepad` (standard layout: 17
 * buttons, 4 axes). `buttons` maps a button's index to its value; above
 * half is pressed, as browsers report triggers.
 */
export function fakeGamepad({
  index = 0,
  id = FAKE_PAD_ID,
  buttons = {},
  axes = [0, 0, 0, 0],
  rumble = true,
  triggerRumble = false,
} = {}) {
  return {
    id,
    index,
    // Not properties of the browser's Gamepad: installFakeGamepads turns
    // them into a `vibrationActuator` (Chromium has one on every pad, Firefox
    // none) whose `effects` list `trigger-rumble` too with `triggerRumble`
    // (Chromium, for pads whose driver has trigger motors).
    rumble,
    triggerRumble,
    connected: true,
    mapping: "standard",
    timestamp: 0,
    buttons: Array.from({ length: 17 }, (_, i) => {
      const value = buttons[i] ?? 0;
      return { pressed: value > 0.5, touched: value > 0, value };
    }),
    axes,
  };
}

/** The longest effect Chromium accepts from `playEffect`, in milliseconds. */
export const EFFECT_CAP_MS = 5000;

/**
 * Replaces `navigator.getGamepads()` in `scope` (the page, before its
 * scripts run) with fed pads: `scope.__ponkoFeedGamepads(pads)` sets them,
 * and the call returns four places, each pad at its `index`. A pad fed with
 * `rumble: true` has a fake `vibrationActuator` that records every call in
 * `scope.__ponkoRumbleCalls` (`{kind: "play", index, type, duration,
 * strong, weak, at}`, with `left` and `right` for a `trigger-rumble`;
 * `{kind: "reset", index, at}`; or `kind: "rejected"` for an effect over
 * the browser's cap or one the actuator doesn't list in its `effects`, which
 * a real one refuses). With `triggerRumble: true` its `effects` are
 * `["dual-rumble", "trigger-rumble"]`, else `["dual-rumble"]`. Self-contained,
 * so the runner can pass it to the page as it is.
 */
export function installFakeGamepads(scope = globalThis) {
  let pads = [];
  const calls = (scope.__ponkoRumbleCalls = []);
  const now = () => (scope.performance ? scope.performance.now() : Date.now());
  const actuators = new Map();
  const actuator = (index, triggers) => {
    const key = `${index}:${triggers}`;
    if (!actuators.has(key)) {
      const effects = triggers ? ["dual-rumble", "trigger-rumble"] : ["dual-rumble"];
      actuators.set(key, {
        type: "dual-rumble",
        effects,
        playEffect(type, params) {
          const call = {
            index,
            type,
            duration: params.duration,
            strong: params.strongMagnitude,
            weak: params.weakMagnitude,
            ...(type === "trigger-rumble" ? { left: params.leftTrigger, right: params.rightTrigger } : {}),
            delay: params.startDelay,
            at: now(),
          };
          if (!effects.includes(type) || !(params.duration <= 5000)) {
            calls.push({ ...call, kind: "rejected" });
            return Promise.reject(new TypeError("effect over the cap, or not one of the actuator's effects"));
          }
          calls.push({ ...call, kind: "play" });
          return Promise.resolve("complete");
        },
        reset() {
          calls.push({ kind: "reset", index, at: now() });
          return Promise.resolve("complete");
        },
      });
    }
    return actuators.get(key);
  };
  scope.__ponkoFeedGamepads = (next) => {
    pads = next;
  };
  const list = () => {
    const places = [null, null, null, null];
    for (const { rumble, triggerRumble, ...pad } of pads) {
      places[pad.index] = rumble ? { ...pad, vibrationActuator: actuator(pad.index, triggerRumble === true) } : pad;
    }
    return places;
  };
  Object.defineProperty(scope.navigator, "getGamepads", { configurable: true, value: list });
}

/**
 * The gamepad case's steps: the pads fed to the page, and what the engine
 * must then report in the fixture's `pads` (null: no pads).
 */
export const GAMEPAD_STEPS = [
  { name: "no pad", pads: [], expect: null },
  {
    name: "a pad connects",
    pads: [fakeGamepad()],
    expect: { down: [], axes: { lx: 0, ly: 0, rx: 0, ry: 0, lt: 0, rt: 0 } },
  },
  {
    name: "buttons, sticks and a trigger",
    // South, d-pad right, the left trigger a quarter in; left stick right and
    // up (past the edge: as fed), right stick three quarters down. The dead
    // zone (0.1, as gilrs's) rescales the right stick and the trigger.
    pads: [fakeGamepad({ buttons: { 0: 1, 15: 1, 6: 0.25 }, axes: [0.5, -1, 0, 0.75] })],
    expect: {
      down: ["South", "DPadRight"],
      axes: { lx: 0.5, ly: -1, rx: 0, ry: deadZone(0.75), lt: deadZone(0.25), rt: 0 },
      presses: 1,
    },
  },
  {
    name: "everything let go",
    pads: [fakeGamepad()],
    expect: { down: [], axes: { lx: 0, ly: 0, rx: 0, ry: 0, lt: 0, rt: 0 }, presses: 1 },
  },
  { name: "the pad unplugged", pads: [], expect: null },
];

/**
 * A value along one axis (the other at rest) as the engine reads it: the
 * dead zone of `src/gamepad_web.rs` (0.1, gilrs's) taken out and the rest
 * rescaled so full still reads 1. Hoisted, so GAMEPAD_STEPS can use it.
 */
export function deadZone(value, zone = 0.1) {
  const magnitude = Math.min(Math.abs(value), 1);
  return magnitude <= zone ? 0 : (Math.sign(value) * (magnitude - zone)) / (1 - zone);
}

/**
 * The problems with the fixture's state after a gamepad step; axes must be
 * within `tolerance` of the expected values (the backend's f32 against these doubles).
 */
export function checkGamepadStep(step, state, tolerance = 1e-6) {
  const pads = state?.pads ?? [];
  if (step.expect == null) return pads.length === 0 ? [] : [`expected no pads, the engine sees ${JSON.stringify(pads)}`];
  if (pads.length !== 1) return [`expected one pad, the engine sees ${pads.length}: ${JSON.stringify(pads)}`];
  const [pad] = pads;
  const problems = [];
  const expect = (ok, what) => ok || problems.push(what);
  expect(pad.slot === 0, `the pad is in slot ${pad.slot}, not 0`);
  expect(pad.name === FAKE_PAD_ID, `the pad's name is ${JSON.stringify(pad.name)}`);
  expect(pad.family === "Xbox", `the pad's family is ${pad.family}, not Xbox (from its vendor id)`);
  expect(pad.vendor === 0x045e && pad.product === 0x0b13, `the pad's ids are ${pad.vendor}:${pad.product}, not 0x045e:0x0b13`);
  const down = [...pad.down].sort().join(",");
  expect(pad.rumble === true, `the pad's rumble capability is ${pad.rumble}, not true (it has a vibrationActuator)`);
  expect(pad.trigger_rumble === false, `the pad's trigger rumble capability is ${pad.trigger_rumble}, not false (its actuator has no trigger-rumble)`);
  expect(down === [...step.expect.down].sort().join(","), `buttons down: [${pad.down}], not [${step.expect.down}]`);
  for (const [axis, want] of Object.entries(step.expect.axes)) {
    expect(Math.abs((pad.axes?.[axis] ?? NaN) - want) < tolerance, `axis ${axis} is ${pad.axes?.[axis]}, not ${want}`);
  }
  if (step.expect.presses != null) {
    expect(state.pad_presses === step.expect.presses, `South was pressed ${state.pad_presses} times, not ${step.expect.presses}`);
  }
  return problems;
}

/**
 * The rumble scenario: West pressed makes the fixture rumble the pad at
 * 0.8 / 0.3 for 2.5 s. The problems with the calls the fake actuator saw
 * by the time the game stopped it: it played the levels, it played them
 * again before the browser's cap (never an effect over it, never a gap of
 * the cap), and the last call was the stop.
 */
export const RUMBLE_PRESS = { 2: 1 };
export const RUMBLE_SECONDS = 2.5;

export function checkRumbleCalls(calls) {
  const problems = [];
  const expect = (ok, what) => ok || problems.push(what);
  const plays = calls.filter((c) => c.kind === "play");
  expect(!calls.some((c) => c.kind === "rejected"), `the browser refused an effect (over its ${EFFECT_CAP_MS} ms cap, or not dual-rumble)`);
  expect(plays.length >= 2, `the rumble was played ${plays.length} time(s), not again before its end (at least 2)`);
  for (const c of plays) {
    expect(c.type === "dual-rumble", `an effect of type ${c.type}`);
    expect(Math.abs(c.strong - 0.8) < 1e-6 && Math.abs(c.weak - 0.3) < 1e-6, `levels ${c.strong}/${c.weak}, not 0.8/0.3`);
    expect(c.duration > 0 && c.duration <= EFFECT_CAP_MS, `an effect of ${c.duration} ms, over the ${EFFECT_CAP_MS} ms cap`);
    expect(c.delay === 0, `an effect that starts after ${c.delay} ms`);
  }
  for (let i = 1; i < plays.length; i++) {
    const gap = plays[i].at - plays[i - 1].at;
    expect(gap < plays[i - 1].duration, `${gap.toFixed(0)} ms between two effects of ${plays[i - 1].duration} ms: it went quiet in between`);
    expect(gap < EFFECT_CAP_MS, `${gap.toFixed(0)} ms between two effects, past the browser's cap`);
  }
  const last = calls[calls.length - 1];
  expect(last?.kind === "reset", `the last call was ${last?.kind ?? "none"}, not the stop (reset) after ${RUMBLE_SECONDS} s`);
  if (last?.kind === "reset" && plays.length) {
    const lasted = (last.at - plays[0].at) / 1000;
    expect(lasted >= RUMBLE_SECONDS - 0.3 && lasted < RUMBLE_SECONDS + 3, `it stopped after ${lasted.toFixed(2)} s, not about ${RUMBLE_SECONDS} s`);
  }
  return problems;
}

/**
 * The trigger rumble scenario: two pads, the first with an actuator that
 * plays `trigger-rumble`, the second with one that plays `dual-rumble`
 * only. North pressed on both makes the fixture ask each for
 * `Rumble::new(0.0, 0.2).triggers(0.4, 0.9).seconds(1.5)`. The first must
 * get `trigger-rumble` effects with all four levels, the second
 * `dual-rumble` ones with the body motors alone (the trigger levels
 * dropped, as on the desktop), each stopped at the end.
 */
export const TRIGGER_PAD_ID = "Wireless Controller (STANDARD GAMEPAD Vendor: 054c Product: 09cc)";
export const TRIGGER_PADS = (buttons = {}) => [
  fakeGamepad({ index: 0, triggerRumble: true, buttons }),
  fakeGamepad({ index: 1, id: TRIGGER_PAD_ID, buttons }),
];
export const TRIGGER_PRESS = { 3: 1 };
export const TRIGGER_RUMBLE = { strong: 0, weak: 0.2, left: 0.4, right: 0.9, seconds: 1.5 };

/** The problems with the pads' trigger capabilities as the fixture reports them. */
export function checkTriggerCapabilities(state) {
  const pads = state?.pads ?? [];
  if (pads.length !== 2) return [`expected two pads, the engine sees ${pads.length}: ${JSON.stringify(pads)}`];
  const problems = [];
  const [first, second] = [...pads].sort((a, b) => a.slot - b.slot);
  if (first.trigger_rumble !== true) problems.push(`the pad whose actuator plays trigger-rumble reports trigger_rumble ${first.trigger_rumble}`);
  if (second.trigger_rumble !== false) problems.push(`the pad whose actuator doesn't reports trigger_rumble ${second.trigger_rumble}`);
  if (first.rumble !== true || second.rumble !== true) problems.push(`both pads have actuators, rumble is ${first.rumble} and ${second.rumble}`);
  return problems;
}

/** Whether both pads' trigger rumble has been stopped (the scenario is over). */
export function triggerRumbleDone(calls) {
  return [0, 1].every((index) => calls.some((c) => c.index === index && c.kind === "reset"));
}

export function checkTriggerRumbleCalls(calls) {
  const problems = [];
  const expect = (ok, what) => ok || problems.push(what);
  const near = (a, b) => Math.abs(a - b) < 1e-6;
  const want = TRIGGER_RUMBLE;
  expect(!calls.some((c) => c.kind === "rejected"), `an actuator refused an effect: ${JSON.stringify(calls.filter((c) => c.kind === "rejected"))}`);
  for (const [index, type] of [
    [0, "trigger-rumble"],
    [1, "dual-rumble"],
  ]) {
    const mine = calls.filter((c) => c.index === index);
    const plays = mine.filter((c) => c.kind === "play");
    expect(plays.length >= 1, `pad ${index} was never played`);
    for (const c of plays) {
      expect(c.type === type, `pad ${index} got a ${c.type} effect, not ${type}`);
      expect(near(c.strong, want.strong) && near(c.weak, want.weak), `pad ${index}: body levels ${c.strong}/${c.weak}, not ${want.strong}/${want.weak}`);
      if (type === "trigger-rumble") {
        expect(near(c.left, want.left) && near(c.right, want.right), `pad ${index}: trigger levels ${c.left}/${c.right}, not ${want.left}/${want.right}`);
      }
      expect(c.duration > 0 && c.duration <= EFFECT_CAP_MS, `pad ${index}: an effect of ${c.duration} ms`);
    }
    const last = mine[mine.length - 1];
    expect(last?.kind === "reset", `pad ${index}'s last call was ${last?.kind ?? "none"}, not the stop`);
    if (last?.kind === "reset" && plays.length) {
      const lasted = (last.at - plays[0].at) / 1000;
      expect(lasted >= want.seconds - 0.3 && lasted < want.seconds + 3, `pad ${index} stopped after ${lasted.toFixed(2)} s, not about ${want.seconds} s`);
    }
  }
  return problems;
}

/**
 * `env` with `flag` added to the variable cargo reads (as `denyWarnings`):
 * the build of `--case allocator` with one heap per thread.
 */
export function addRustFlag(env, flag) {
  const out = { ...env };
  const set = (name) => (out[name] ?? "") !== "";
  const target = "CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS";
  if (set("CARGO_ENCODED_RUSTFLAGS")) {
    out.CARGO_ENCODED_RUSTFLAGS = [...out.CARGO_ENCODED_RUSTFLAGS.split("\x1f"), ...flag.split(" ")].join("\x1f");
  } else {
    const name = !set("RUSTFLAGS") && set(target) ? target : "RUSTFLAGS";
    out[name] = `${out[name] ?? ""} ${flag}`.trim();
  }
  return out;
}

/**
 * Run in the page before its scripts (`addInitScript`): hides WebGPU, so the
 * engine draws with WebGL2. The sweeps time passes on the CPU only; on a
 * busy machine the headless shell's WebGPU canvas has lost its device at
 * the first frame, and the next GPU buffer stopped the page.
 */
export function hideWebGpu() {
  Object.defineProperty(Navigator.prototype, "gpu", { get: () => undefined, configurable: true });
}

/** Whether a sweep page (`?sweep=...`) has published everything. */
export function sweepDone(state) {
  return state != null && state.done === true;
}

/**
 * The problems with a `?sweep=thresholds` run: every swept pass timed at
 * each size, and run on both sides of its threshold, where it must stay on
 * the page's thread below and be shared out from it, ending on the same
 * hash as the page's thread alone on both sides.
 */
export function checkSides(state) {
  const problems = [];
  if (state.isolated !== true || !(state.threads >= 2)) {
    problems.push(`the sweep needs an isolated page with workers (isolated: ${state.isolated}, threads: ${state.threads})`);
  }
  for (const pass of SWEPT) {
    const rows = (state.sweep ?? []).filter((r) => r.pass === pass);
    if (rows.length === 0) problems.push(`${pass} wasn't timed`);
    const sides = (state.sides ?? []).filter((r) => r.pass === pass);
    const below = sides.find((r) => r.side === "below");
    const above = sides.find((r) => r.side === "above");
    if (!below || !above) {
      problems.push(`${pass} wasn't run on both sides of its threshold`);
      continue;
    }
    if (below.shared !== false) problems.push(`${pass} was shared out below its threshold (${below.size})`);
    if (above.shared !== true) problems.push(`${pass} stayed on the page's thread at its threshold (${above.size})`);
    for (const r of [below, above]) {
      if (r.hash !== r.serial_hash) {
        problems.push(`${pass} at ${r.size} (${r.side}) ended on ${r.hash}, but ${r.serial_hash} on the page's thread alone`);
      }
    }
  }
  return problems;
}

/**
 * The sweep's rows as a Markdown table: each size's median on the page's
 * thread and shared out, and which is faster (`same` within `noiseMs`).
 */
export function sweepTable(rows, threads, noiseMs = 0.05) {
  const lines = [`| Pass | Size | Page's thread (ms) | ${threads} threads (ms) | Faster |`, "|---|---|---|---|---|"];
  for (const r of rows) {
    const d = r.serial_ms - r.shared_ms;
    const faster = Math.abs(d) < noiseMs ? "same" : d > 0 ? "shared" : "page's thread";
    lines.push(`| \`${r.pass}\` | ${r.size} | ${r.serial_ms.toFixed(3)} | ${r.shared_ms.toFixed(3)} | ${faster} |`);
  }
  return lines.join("\n");
}

/**
 * The problems with `--case allocator`'s two runs: the default build must
 * have the standard allocator and the one built with `THREAD_HEAPS_CFG` one
 * heap per thread, and each must have saved the same JSON on the page's
 * thread and the workers.
 */
export function checkAllocator(perThread, system) {
  const problems = [];
  if (system.per_thread_allocator !== false) problems.push("the default threaded build has the per-thread allocator");
  if (perThread.per_thread_allocator !== true) problems.push(`the build with ${THREAD_HEAPS_CFG} doesn't have the per-thread allocator`);
  for (const [name, s] of [["per-thread", perThread], ["standard", system]]) {
    if (s.isolated !== true || !(s.threads >= 2)) problems.push(`${name}: the page isn't isolated with workers (threads: ${s.threads})`);
    if (s.alloc?.same_save !== true) problems.push(`${name}: the save on the workers differs from the page's thread's`);
  }
  return problems;
}

/** `--case allocator`'s numbers as a Markdown table, with each build's wasm size. */
export function allocatorTable(perThread, system, sizes) {
  const ms = (v) => (Number.isFinite(v) ? v.toFixed(2) : "n/a");
  const rows = [
    ["JSON save, page's thread, before the workers saved", "save_page_first_ms"],
    ["JSON save, page's thread, taking turns with the workers", "save_page_ms"],
    ["JSON save, workers", "save_workers_ms"],
    ["The save's JSON values built, page's thread", "save_build_ms"],
    ["The save's text written, page's thread", "save_write_ms"],
    ["The save's JSON values dropped, page's thread", "save_drop_ms"],
    ["Parsing that JSON, page's thread", "parse_ms"],
    ["100,000 blocks of 32 bytes allocated and freed, page's thread", "small_blocks_ms"],
    ["A vector grown to 1,000,000 numbers by pushes, page's thread", "vec_grow_ms"],
    ["Parallel query allocating per item", "par_alloc_ms"],
  ];
  const lines = ["| Pass | Standard allocator (ms) | One heap per thread (ms) |", "|---|---|---|"];
  for (const [label, key] of rows) lines.push(`| ${label} | ${ms(system.alloc?.[key])} | ${ms(perThread.alloc?.[key])} |`);
  const kb = (b) => (b == null ? "n/a" : `${Math.round(b / 1024)} KiB`);
  lines.push(`| \`game_bg.wasm\` | ${kb(sizes.system)} | ${kb(sizes.perThread)} |`);
  return lines.join("\n");
}
