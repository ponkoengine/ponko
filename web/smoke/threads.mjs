#!/usr/bin/env node
// Threads on the web (see README.md, "Threads"): the fixture
// examples/web_threads.rs, built with threads (web/build.sh --threads
// web_threads), in a headless browser.
//
//   node web/smoke/threads.mjs --build                  # the checks
//   node web/smoke/threads.mjs --case worker-panic      # a worker's panic stops the game
//   node web/smoke/threads.mjs --case gamepads          # fed browser gamepads reach the game, and rumble the pad
//   node web/smoke/threads.mjs --build --case gamepads-plain   # the same on the ordinary build (gilrs)
//   node web/smoke/threads.mjs --case warning           # a warning fails the threaded build
//   node web/smoke/threads.mjs --bench --repeat 3       # 1 vs N threads
//
// The checks open the same build twice: served with the cross-origin
// isolation headers, the game must start workers and run parallel query
// items, scheduler batches and jobs on them and never on the page's thread;
// served without, it must run on the page's thread alone and say why in the
// console. Both runs must simulate the same world (a checksum at tick 120).
// `--build` builds with RUSTFLAGS="-D warnings" added, so a warning in the
// threaded build fails the check; `--case warning` proves that it does, by
// building an example whose only code is an unused function that only a
// threaded build compiles (no browser). `--case gamepads` replaces the
// isolated page's `navigator.getGamepads()` with fed pads and checks that
// the engine sees them connect, press, move and leave; the pads have a fake
// `vibrationActuator` that records its calls, and the game's rumble must
// reach it (played again before the browser's 5 s cap, stopped at its end).
// Real rumble can't be felt in a headless browser: the recorded calls are
// the proof. `--case gamepads-plain` runs it on the ordinary web build of
// the same fixture, whose pads come from gilrs, not the threaded backend.
// The bench runs the fixture's workloads on the isolated page with one
// thread (`?threads=1`) and with the engine's default, and prints the
// medians, their spread, and the speed-up with whether it is beyond the
// noise. Exit codes as smoke.mjs: 0 passed, 1 failed, 2 unsupported (no
// browser), 3 bad arguments or a missing build. report.json goes to --out.

import { spawnSync } from "node:child_process";
import { cpus, loadavg } from "node:os";
import { mkdir, stat, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { browserLog } from "./browser-log.mjs";
import { CALL_TIMEOUT_MS, EXIT, UsageError, launchConfig, launchFailureKind, startWatchdog, withDeadline } from "./lib.mjs";
import { LAUNCHERS, serve } from "./smoke.mjs";
import {
  FALLBACK_WARNING,
  GAMEPAD_STEPS,
  GILRS_AXIS_TOLERANCE,
  ISOLATION_HEADERS,
  RUMBLE_PRESS,
  RUMBLE_SECONDS,
  STOP_MESSAGE,
  USAGE,
  checkFallback,
  checkIsolated,
  checkReady,
  checkGamepadStep,
  checkRumbleCalls,
  checkSameResult,
  checkWarningBuild,
  denyWarnings,
  fakeGamepad,
  installFakeGamepads,
  WARNING_FIXTURE,
  parseArgs,
  summarize,
  summaryTable,
} from "./threads-lib.mjs";

const sleep = (ms) => new Promise((ok) => setTimeout(ok, ms));

/** The fixture's state (`<html data-smoke>`), or null before its first frame. */
async function state(page) {
  const text = await withDeadline(
    page.evaluate(() => document.documentElement.dataset.smoke ?? null),
    CALL_TIMEOUT_MS,
    "reading the fixture's state",
  );
  return text ? JSON.parse(text) : null;
}

/** Opens `url` in a new page of `context`, keeping its console and errors. */
async function open(context, url) {
  const page = await context.newPage();
  const lines = [];
  page.on("console", (m) => lines.push(`${m.type()}: ${m.text()}`));
  page.on("pageerror", (e) => lines.push(`pageerror: ${e.message}`));
  await withDeadline(page.goto(url), CALL_TIMEOUT_MS, `opening ${url}`);
  return { page, lines };
}

/** Polls the state until `ready(state)`, or fails after `seconds`. */
async function until(page, lines, ready, seconds, what) {
  const deadline = Date.now() + seconds * 1000;
  let last = null;
  while (Date.now() < deadline) {
    last = await state(page);
    if (ready(last)) return last;
    if (lines.some((l) => l.startsWith("pageerror:"))) break;
    await sleep(250);
  }
  const tail = lines.slice(-10).join("\n  ");
  throw new Error(`${what} didn't happen within ${seconds}s; last state ${JSON.stringify(last)}\n  ${tail}`);
}

async function check(browser, servers, args, report) {
  const runs = {};
  for (const [kind, server] of Object.entries(servers)) {
    const context = await browser.newContext();
    const { page, lines } = await open(context, `http://127.0.0.1:${server.address().port}/`);
    const result = await until(page, lines, checkReady, args.timeout, `the ${kind} run reaching tick 150 with its jobs`);
    runs[kind] = { state: result, console: lines };
    await context.close();
  }
  report.runs = runs;
  const problems = [
    ...checkIsolated(runs.isolated.state).map((p) => `isolated: ${p}`),
    ...checkFallback(runs.plain.state, runs.plain.console.join("\n")).map((p) => `not isolated: ${p}`),
    ...checkSameResult(runs.isolated.state, runs.plain.state),
  ];
  const s = runs.isolated.state;
  console.log(
    `isolated: ${s.threads} worker threads; query items on ${s.par_threads}, batch systems on ${s.batch_threads}, ` +
      `${s.batch_threaded_runs} batches threaded; jobs off the page: ${s.wait_off_page && s.job_off_page}; ` +
      `asset group ${s.group}, decoded off the page: ${s.group_off_page}`,
  );
  console.log(`not isolated: ${runs.plain.state.threads} worker threads ("${FALLBACK_WARNING}"); checksums ${s.checksum} and ${runs.plain.state.checksum}`);
  return problems;
}

/** A panic inside the parallel query, on a worker: the game must stop, not hang the page. */
async function workerPanic(browser, servers, args, report) {
  const context = await browser.newContext();
  const { page, lines } = await open(context, `http://127.0.0.1:${servers.isolated.address().port}/?smoke_case=worker-panic`);
  const problems = [];
  const deadline = Date.now() + args.timeout * 1000;
  let stopped = false;
  while (Date.now() < deadline && !stopped) {
    stopped = lines.some((l) => l.includes(STOP_MESSAGE));
    if (!stopped) await sleep(250);
  }
  // The page's thread must still answer (it stopped instead of spinning).
  const status = await withDeadline(
    page.evaluate(() => document.getElementById("status")?.textContent ?? ""),
    10_000,
    "reading the page after the panic",
  ).catch((e) => `no answer: ${e.message}`);
  const before = (await state(page).catch(() => null))?.ticks;
  await sleep(1500);
  const after = (await state(page).catch(() => null))?.ticks;
  report.runs = { worker_panic: { console: lines, status, ticks: [before, after] } };
  if (!stopped) problems.push(`the console never said "${STOP_MESSAGE}"`);
  if (!lines.some((l) => l.includes("parallel query, as ?smoke_case=worker-panic asked"))) {
    problems.push("the panic's own message isn't in the console");
  }
  if (status.startsWith("no answer")) problems.push(`the page stopped answering: ${status}`);
  if (before !== after) problems.push(`the simulation kept running after the panic (tick ${before}, then ${after})`);
  console.log(`worker panic: ${stopped ? "the game stopped" : "the game did not stop"}; page status "${status}"; ticks ${before} → ${after}`);
  await context.close();
  return problems;
}

/**
 * Fed browser gamepads: each step must reach the game, and its rumble the
 * fake actuator. On the isolated page of the threaded build, or (`plain`)
 * the ordinary build.
 */
async function gamepads(browser, servers, args, report, plain = false) {
  const context = await browser.newContext();
  await context.addInitScript(installFakeGamepads);
  const server = plain ? servers.plain : servers.isolated;
  const { page, lines } = await open(context, `http://127.0.0.1:${server.address().port}/`);
  await until(page, lines, (s) => s != null && s.ticks > 0, args.timeout, "the first frame");
  const problems = [];
  const steps = [];
  for (const step of GAMEPAD_STEPS) {
    await withDeadline(page.evaluate((pads) => window.__ponkoFeedGamepads(pads), step.pads), CALL_TIMEOUT_MS, "feeding gamepads");
    let last = null;
    const deadline = Date.now() + 10_000;
    let left = ["not checked"];
    while (Date.now() < deadline) {
      last = await state(page);
      left = checkGamepadStep(step, last, plain ? GILRS_AXIS_TOLERANCE : undefined);
      if (left.length === 0) break;
      await sleep(100);
    }
    steps.push({ step: step.name, pads: last?.pads ?? null, pad_presses: last?.pad_presses ?? null, problems: left });
    console.log(`gamepads, ${step.name}: ${left.length ? left.join("; ") : "seen"}`);
    problems.push(...left.map((p) => `${step.name}: ${p}`));
  }
  // Rumble: West makes the game rumble the pad for a few seconds.
  const feed = (pads) => withDeadline(page.evaluate((p) => window.__ponkoFeedGamepads(p), pads), CALL_TIMEOUT_MS, "feeding gamepads");
  await feed([fakeGamepad()]);
  await sleep(500);
  await feed([fakeGamepad({ buttons: RUMBLE_PRESS })]);
  const calls = () => withDeadline(page.evaluate(() => window.__ponkoRumbleCalls), CALL_TIMEOUT_MS, "reading the rumble calls");
  const rumbleEnd = Date.now() + (RUMBLE_SECONDS + 8) * 1000;
  let seen = [];
  while (Date.now() < rumbleEnd) {
    seen = await calls();
    if (seen.some((c) => c.kind === "reset")) break;
    await sleep(100);
  }
  const rumbleProblems = checkRumbleCalls(seen);
  const plays = seen.filter((c) => c.kind === "play");
  console.log(
    `gamepads, rumble: ${plays.length} effects${plays.length ? ` of ${plays[0].duration} ms` : ""}, ` +
      `${seen.filter((c) => c.kind === "reset").length} stop(s)${rumbleProblems.length ? `: ${rumbleProblems.join("; ")}` : ", as the game asked"}`,
  );
  problems.push(...rumbleProblems.map((p) => `rumble: ${p}`));
  await feed([]);
  const final = await state(page);
  report.runs = { gamepads: { steps, rumble_calls: seen, threads: final?.threads, console: lines } };
  const threads = final?.threads;
  if (plain) {
    if (threads !== 0) problems.push(`the ordinary build ran ${threads} worker threads, not 0`);
  } else if (!(threads >= 2)) {
    problems.push(`the page ran on ${threads} worker threads: the case is about a build with threads, served isolated`);
  }
  await context.close();
  return problems;
}

/** A warning in the threaded build must fail it (no browser): see the file's comment. */
function warningBuild(report) {
  const env = denyWarnings(process.env);
  console.log(`building ${WARNING_FIXTURE} with threads and -D warnings; it must fail on its unused function`);
  const run = spawnSync("bash", ["web/build.sh", "--threads", WARNING_FIXTURE], { env, encoding: "utf8", maxBuffer: 64 << 20 });
  const output = `${run.stdout ?? ""}${run.stderr ?? ""}`;
  process.stdout.write(output.split("\n").slice(-30).join("\n"));
  const problems = checkWarningBuild(run.status, output);
  report.runs = { warning: { status: run.status, output_tail: output.split("\n").slice(-60) } };
  console.log(problems.length ? "" : `\nthe build failed on the warning, as it must`);
  return problems;
}

async function bench(browser, servers, args, report) {
  const port = servers.isolated.address().port;
  const settings = args.threads ?? [1, null];
  const loads = [];
  for (let r = 0; r < args.repeat; r++) {
    for (const workload of args.workloads) {
      for (const threads of settings) {
        const query = `?bench=${workload}${threads == null ? "" : `&threads=${threads}`}`;
        const context = await browser.newContext();
        const { page, lines } = await open(context, `http://127.0.0.1:${port}/${query}`);
        const done = await until(page, lines, (s) => s?.done === true, args.timeout, `the ${workload} benchmark (${query})`);
        await context.close();
        const load = { workload, threads: Math.max(1, done.threads), runs_ms: done.bench.runs_ms };
        loads.push(load);
        const sorted = [...load.runs_ms].sort((a, b) => a - b);
        console.log(`${workload} on ${load.threads} thread(s), run ${r + 1}: median ${sorted[sorted.length >> 1].toFixed(2)} ms`);
      }
    }
  }
  const rows = summarize(loads);
  report.loads = loads;
  report.summary = rows;
  console.log(`\n${summaryTable(rows)}`);
  return [];
}

async function main() {
  let args;
  try {
    args = parseArgs(process.argv.slice(2));
  } catch (e) {
    if (!(e instanceof UsageError)) throw e;
    console.error(`${e.message}\n${USAGE}`);
    process.exit(EXIT.usage);
  }
  if (args.case === "warning") {
    await mkdir(args.out, { recursive: true });
    const report = { fixture: WARNING_FIXTURE, mode: "warning", problems: [] };
    report.problems.push(...warningBuild(report));
    report.outcome = report.problems.length ? "failed" : "passed";
    await writeFile(join(args.out, "report.json"), JSON.stringify(report, null, 2) + "\n");
    for (const p of report.problems) console.error(`FAIL: ${p}`);
    if (report.problems.length === 0) console.log("PASS");
    process.exit(report.problems.length ? EXIT.app : EXIT.pass);
  }
  // With -D warnings: a warning in the threaded build fails the check.
  const buildEnv = denyWarnings(process.env);
  // The ordinary build (no --threads) of the same fixture, for gamepads-plain.
  const buildArgs = args.case === "gamepads-plain" ? ["web/build.sh", "web_threads", args.dir] : ["web/build.sh", "--threads", "web_threads"];
  if (args.build && spawnSync("bash", buildArgs, { stdio: "inherit", env: buildEnv }).status !== 0) {
    process.exit(EXIT.usage);
  }
  const root = resolve(args.dir);
  // A bench may time a build without threads (`--threads 1`), for what the
  // fallback costs; the checks need the threaded build.
  const needed = args.mode === "bench" || args.case === "gamepads-plain" ? ["index.html"] : ["index.html", "ponko-worker.js"];
  for (const file of needed) {
    if (!(await stat(join(root, file)).catch(() => null))) {
      console.error(`${join(root, file)} is missing: pass --build, or build it with web/build.sh --threads web_threads`);
      process.exit(EXIT.usage);
    }
  }
  await mkdir(args.out, { recursive: true });
  const report = {
    fixture: "web_threads",
    browser: args.browser,
    mode: args.case ?? args.mode,
    machine: { cpus: cpus().length, cpu: cpus()[0]?.model ?? null, load_average: loadavg() },
    problems: [],
  };
  const finish = async (code, outcome) => {
    report.outcome = outcome;
    report.browserLog = browserLog.slice(-200);
    await writeFile(join(args.out, "report.json"), JSON.stringify(report, null, 2) + "\n");
    process.exit(code);
  };
  const servers = { isolated: await serve(root, ISOLATION_HEADERS), plain: await serve(root) };
  // The whole run's deadline: a hung browser can't keep it going.
  const loadsPerRun = args.mode === "bench" ? args.repeat * args.workloads.length * (args.threads?.length ?? 2) : 2;
  const stopWatchdog = startWatchdog((args.timeout * loadsPerRun + 60) * 1000, () => {
    report.problems.push("the run passed its deadline: the browser stopped answering");
    finish(EXIT.app, "browser-hang");
  });
  let browser;
  try {
    browser = await LAUNCHERS[args.browser].launch(launchConfig(args.browser, process.platform).options);
  } catch (e) {
    const kind = launchFailureKind(e.message, process.env);
    report.problems.push(e.message);
    stopWatchdog();
    await finish(kind === "unsupported" ? EXIT.unsupported : EXIT.app, kind);
  }
  try {
    const run =
      args.case === "worker-panic"
        ? workerPanic
        : args.case === "gamepads"
          ? gamepads
          : args.case === "gamepads-plain"
            ? (...a) => gamepads(...a, true)
            : args.mode === "bench"
              ? bench
              : check;
    report.problems.push(...(await run(browser, servers, args, report)));
  } catch (e) {
    report.problems.push(e.message);
  } finally {
    await withDeadline(browser.close(), CALL_TIMEOUT_MS, "closing the browser").catch(() => {});
    for (const s of Object.values(servers)) s.close();
    stopWatchdog();
  }
  for (const p of report.problems) console.error(`FAIL: ${p}`);
  if (report.problems.length === 0) console.log("PASS");
  await finish(report.problems.length ? EXIT.app : EXIT.pass, report.problems.length ? "failed" : "passed");
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
