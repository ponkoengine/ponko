#!/usr/bin/env node
// Failures while a web game runs (see README.md, "Runtime failures"): the
// 2D fixture (web/build.sh web_smoke) in a headless browser, stopped four
// ways after it started. Each must reach the page as the engine reported
// it, through the generated template and through `onPonkoFailure`.
//
//   node web/smoke/runtime-failures.mjs --build
//   node web/smoke/runtime-failures.mjs --dir target/web/web_smoke --browser chromium
//   node web/smoke/runtime-failures.mjs --case device-lost --chromium-mode new \
//     --browser-arg --use-webgpu-adapter=swiftshader --allow-unsupported
//
// - runtime-panic: the fixture panics in a frame after 20 ticks. The page
//   shows "The game stopped: <the panic's message>"; the callback gets
//   kind "Panic", the message and the panic's source position.
// - failure-text: the same panic, with the game's own text registered
//   through `ponko::web::on_failure`: the page shows that text instead.
// - context-lost: on WebGL2, the runner loses the canvas's context while
//   the game runs: kind "ContextLost" and its message on the page.
// - device-lost: on WebGPU, the runner destroys the game's GPUDevice while
//   the game runs: kind "DeviceLost", reason "Destroyed" (or "Unknown" when
//   the browser lost it first), the engine's sentence for that reason with
//   the browser's words in it. Where the browser has no WebGPU (the game
//   runs on WebGL2) the case is unsupported, not failed.
//
// Exit codes as smoke.mjs: 0 passed, 1 failed, 2 unsupported (no browser,
// or every case asked for was unsupported; --allow-unsupported makes that
// a pass), 3 bad arguments or a missing build. report.json goes to --out.

import { spawnSync } from "node:child_process";
import { mkdir, stat, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { browserLog } from "./browser-log.mjs";
import { CALL_TIMEOUT_MS, EXIT, UsageError, launchConfig, launchFailureKind, startWatchdog, withDeadline } from "./lib.mjs";
import { LAUNCHERS, serve } from "./smoke.mjs";

const USAGE =
  "usage: runtime-failures.mjs [--dir <site>] [--build] [--browser chromium|firefox|webkit]\n" +
  "                            [--case runtime-panic|failure-text|context-lost|device-lost] [--out <dir>] [--timeout <s>]\n" +
  "                            [--chromium-mode shell|new] [--browser-arg <flag>]... [--allow-unsupported]";

export const RUNTIME_CASES = ["runtime-panic", "failure-text", "context-lost", "device-lost"];

/**
 * The failure's `reason` for a lost WebGPU device (`DeviceLostReason` in
 * src/web/failure.rs) from the browser's `GPUDeviceLostInfo.reason`:
 * "Destroyed" for "destroyed", "Unknown" for anything else.
 */
export function deviceLostReason(browserReason) {
  return browserReason === "destroyed" ? "Destroyed" : "Unknown";
}

/**
 * The engine's message for a lost WebGPU device (`FailureKind::DeviceLost`,
 * `device_lost_message` in src/web/failure.rs): its sentence for the
 * browser's reason (`GPUDeviceLostInfo.reason`: destroyed, or lost), with
 * the browser's words (`GPUDeviceLostInfo.message`) in it when there are
 * some.
 */
export function deviceLostMessage(browserReason, browserMessage) {
  const what = deviceLostReason(browserReason) === "Destroyed" ? "destroyed" : "lost";
  const words = (browserMessage ?? "").trim().replace(/\.+$/, "").trimEnd();
  return words
    ? `The WebGPU device was ${what} while the game was running: ${words}. Reload the page to try again.`
    : `The WebGPU device was ${what} while the game was running. Reload the page to try again.`;
}

/**
 * The page's text, the callback's failure and the crash report each case
 * must produce. `lost` is the page's own `GPUDeviceLostInfo` (device-lost
 * only): the failure's `reason` and the engine's message follow the
 * browser's reason, and the message carries its words. The report
 * (`window.ponkoCrashReport`, see the guide's "Crash reports") is a
 * `panic` for a panic and an `error` for a failure that stops the game,
 * with the same message.
 */
export function expected(runtimeCase, lost = null) {
  const panic = `web_smoke: panicking while running, as ?smoke_case=${runtimeCase} asked`;
  switch (runtimeCase) {
    case "runtime-panic":
      return { kind: "Panic", message: panic, text: null, reason: null, status: `The game stopped: ${panic}`, report: "panic" };
    case "failure-text": {
      const text = `web_smoke stopped, in its own words: ${panic}`;
      return { kind: "Panic", message: panic, text, reason: null, status: text, report: "panic" };
    }
    case "context-lost": {
      const message = "The WebGL2 context was lost while the game was running. Reload the page to try again.";
      return { kind: "ContextLost", message, text: null, reason: null, status: `The game stopped: ${message}`, report: "error" };
    }
    case "device-lost": {
      const message = deviceLostMessage(lost?.reason, lost?.message);
      const reason = deviceLostReason(lost?.reason);
      return { kind: "DeviceLost", message, text: null, reason, status: `The game stopped: ${message}`, report: "error" };
    }
    default:
      throw new UsageError(`unknown case ${runtimeCase} (${RUNTIME_CASES.join(", ")})`);
  }
}

/**
 * What is wrong with the crash report the page got (`window.ponkoCrashReport`,
 * `{ name, text, kind, message }`), for a run that wanted `want`.
 */
export function checkReport(want, report) {
  if (!report) return ["no crash report reached the page (window.ponkoCrashReport)"];
  const problems = [];
  if (report.kind !== want.report) {
    problems.push(`the crash report's kind is ${JSON.stringify(report.kind)}, wanted ${JSON.stringify(want.report)}`);
  }
  if (report.message !== want.message) {
    problems.push(`the crash report's message is ${JSON.stringify(report.message)}, wanted ${JSON.stringify(want.message)}`);
  }
  const text = String(report.text ?? "");
  for (const line of [`kind: ${want.report}`, `message: ${want.message}`, "engine version: "]) {
    if (!text.includes(line)) problems.push(`the crash report's text lacks ${JSON.stringify(line)}`);
  }
  if (!String(report.name ?? "").startsWith("crash-reports/")) {
    problems.push(`the crash report's name ${JSON.stringify(report.name)} is not in crash-reports/`);
  }
  return problems;
}

/**
 * What is wrong with one run: `seen` holds the state the fixture published
 * before it stopped, the page's status line, the failure a late
 * `onPonkoFailure` subscriber got, (device-lost) the `GPUDeviceLostInfo`
 * the page's own `device.lost` got, and the crash report the page got. An
 * empty list is a pass.
 */
export function checkRun(runtimeCase, seen) {
  const problems = [];
  if (runtimeCase === "device-lost" && !seen.lost) problems.push("the browser never resolved the game's device.lost");
  const want = expected(runtimeCase, seen.lost);
  if (!(seen.ticks > 0)) problems.push(`the game never ran a tick before it stopped (ticks ${seen.ticks})`);
  if (seen.status !== want.status) problems.push(`the page shows ${JSON.stringify(seen.status)}, wanted ${JSON.stringify(want.status)}`);
  const failure = seen.failure;
  if (!failure) {
    problems.push("onPonkoFailure never called its callback");
    return problems;
  }
  for (const field of ["kind", "message", "text", "reason"]) {
    if (failure[field] !== want[field]) {
      problems.push(`the failure's ${field} is ${JSON.stringify(failure[field])}, wanted ${JSON.stringify(want[field])}`);
    }
  }
  if (want.kind === "Panic" && !failure.location?.includes("web_smoke.rs")) {
    problems.push(`a panic's location should name its source file, got ${JSON.stringify(failure.location)}`);
  }
  if (want.kind !== "Panic" && failure.location !== null) {
    problems.push(`only a panic has a location, got ${JSON.stringify(failure.location)}`);
  }
  problems.push(...checkReport(want, seen.report));
  return problems;
}

/**
 * The exit code and outcome of a run: a problem fails it; a run whose
 * every case was unsupported is unsupported (a pass with
 * --allow-unsupported); else it passed, the unsupported cases named.
 */
export function runOutcome(report, args) {
  if (report.problems.length) return [EXIT.app, "failed"];
  const ran = Object.keys(report.runs).length;
  if (ran > 0 && report.unsupported.length === ran) {
    return args.allowUnsupported ? [EXIT.pass, "unsupported"] : [EXIT.unsupported, "unsupported"];
  }
  return [EXIT.pass, report.unsupported.length ? "passed, some unsupported" : "passed"];
}

function parseArgs(argv) {
  const args = {
    dir: "target/web/web_smoke",
    build: false,
    browser: "chromium",
    cases: RUNTIME_CASES,
    out: "target/web-smoke/runtime-failures",
    timeout: 60,
    chromiumMode: "shell",
    browserArgs: [],
    allowUnsupported: false,
  };
  for (let i = 0; i < argv.length; i++) {
    const flag = argv[i];
    const value = () => {
      if (i + 1 >= argv.length) throw new UsageError(`${flag} needs a value`);
      return argv[++i];
    };
    switch (flag) {
      case "--dir": args.dir = value(); break;
      case "--build": args.build = true; break;
      case "--browser":
        args.browser = value();
        if (!LAUNCHERS[args.browser]) throw new UsageError(`unknown browser ${args.browser}`);
        break;
      case "--case": {
        const one = value();
        expected(one);
        args.cases = [one];
        break;
      }
      case "--out": args.out = value(); break;
      case "--timeout": args.timeout = Number(value()); break;
      case "--chromium-mode":
        args.chromiumMode = value();
        if (!["shell", "new"].includes(args.chromiumMode)) throw new UsageError(`unknown chromium mode ${args.chromiumMode}`);
        break;
      case "--browser-arg": args.browserArgs.push(value()); break;
      case "--allow-unsupported": args.allowUnsupported = true; break;
      default: throw new UsageError(`unknown argument ${flag}`);
    }
  }
  return args;
}

const sleep = (ms) => new Promise((ok) => setTimeout(ok, ms));
const call = (promise, what) => withDeadline(promise, CALL_TIMEOUT_MS, what);

/** The fixture's ticks, the page's error line (or null) and a late subscriber's failure. */
async function look(page) {
  return call(page.evaluate(async () => {
    const raw = document.documentElement.getAttribute("data-smoke");
    const status = document.getElementById("status");
    const { onPonkoFailure } = await import(new URL("./game.js", location.href));
    let failure = null;
    // After the failure, a late subscriber gets it at once. Before the
    // module is initialized there is nothing to subscribe to yet.
    try {
      onPonkoFailure((error) => {
        failure = { kind: error.kind, message: error.message, location: error.location, text: error.text, reason: error.reason };
      });
    } catch {
      // Still loading.
    }
    return {
      ticks: raw ? JSON.parse(raw).ticks : 0,
      status: status && status.className === "error" ? status.textContent : null,
      failure,
      lost: globalThis.__smokeLost ?? null,
      report: window.ponkoCrashReport
        ? (({ name, text, kind, message }) => ({ name, text, kind, message }))(window.ponkoCrashReport)
        : null,
    };
  }), "reading the page");
}

async function runCase(browser, server, runtimeCase, args) {
  const context = await browser.newContext();
  const lines = [];
  try {
    if (runtimeCase !== "device-lost") {
      // WebGL2 only: a WebGPU canvas has no context to lose this way, and a
      // panic is the same on either. (Chromium's headless shell can't
      // present a WebGPU canvas and loses its device at once, which would
      // stop the game before it panics.)
      await context.addInitScript(() => {
        Object.defineProperty(Navigator.prototype, "gpu", { get: () => undefined, configurable: true });
      });
    }
    if (runtimeCase === "device-lost") {
      // Keep the devices the page opens, so the runner can destroy the
      // game's: no browser lets a page crash its GPU process, and a
      // destroyed device is lost the same way to the game. The page's own
      // `device.lost` says why, and whether the runner did it.
      await context.addInitScript(() => {
        if (!globalThis.GPUAdapter) return;
        const requestDevice = GPUAdapter.prototype.requestDevice;
        GPUAdapter.prototype.requestDevice = async function (...args) {
          const device = await requestDevice.apply(this, args);
          (globalThis.__smokeDevices ??= []).push(device);
          device.lost.then((info) => {
            globalThis.__smokeLost ??= {
              reason: info.reason,
              message: info.message,
              by: globalThis.__smokeDestroying ? "runner" : "browser",
            };
          });
          return device;
        };
      });
    }
    const page = await context.newPage();
    page.on("console", (m) => lines.push(`${m.type()}: ${m.text()}`));
    page.on("pageerror", (e) => lines.push(`pageerror: ${e.message}`));
    const query = runtimeCase === "context-lost" || runtimeCase === "device-lost" ? "" : `?smoke_case=${runtimeCase}`;
    await call(page.goto(`http://127.0.0.1:${server.address().port}/${query}`), "opening the page");
    const deadline = Date.now() + args.timeout * 1000;
    let seen = await look(page);
    let ticks = seen.ticks;
    let lost = false;
    while (Date.now() < deadline && !seen.status) {
      if (runtimeCase === "context-lost" && !lost && seen.ticks > 0) {
        lost = await call(page.evaluate(() => {
          const gl = document.getElementById("ponko").getContext("webgl2");
          const extension = gl?.getExtension("WEBGL_lose_context");
          extension?.loseContext();
          return Boolean(extension);
        }), "losing the WebGL2 context");
        if (!lost) return { problems: ["the canvas has no WebGL2 context to lose"], lines };
      }
      if (runtimeCase === "device-lost" && !lost && seen.ticks > 0) {
        lost = await call(page.evaluate(() => {
          const device = globalThis.__smokeDevices?.at(-1);
          globalThis.__smokeDestroying = true;
          device?.destroy();
          return Boolean(device);
        }), "destroying the WebGPU device");
        if (!lost) return { unsupported: "the game opened no WebGPU device here (it runs on WebGL2)", problems: [], lines };
      }
      await sleep(200);
      seen = await look(page);
      ticks = Math.max(ticks, seen.ticks);
    }
    // The failure that follows (a panic's trap) must not replace it.
    await sleep(500);
    seen = await look(page);
    seen.ticks = Math.max(ticks, seen.ticks);
    return { seen, problems: checkRun(runtimeCase, seen), lines };

  } finally {
    await call(context.close(), "closing the page").catch(() => {});
  }
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
  if (args.build && spawnSync("bash", ["web/build.sh", "web_smoke"], { stdio: "inherit" }).status !== 0) {
    process.exit(EXIT.usage);
  }
  const root = resolve(args.dir);
  if (!(await stat(join(root, "index.html")).catch(() => null))) {
    console.error(`${join(root, "index.html")} is missing: pass --build, or build it with web/build.sh web_smoke`);
    process.exit(EXIT.usage);
  }
  await mkdir(args.out, { recursive: true });
  const report = { fixture: "web_smoke", browser: args.browser, runs: {}, problems: [], unsupported: [] };
  const finish = async (code, outcome) => {
    report.outcome = outcome;
    report.browserLog = browserLog.slice(-200);
    await writeFile(join(args.out, "report.json"), JSON.stringify(report, null, 2) + "\n");
    process.exit(code);
  };
  const server = await serve(root);
  const stopWatchdog = startWatchdog((args.timeout * args.cases.length + 60) * 1000, () => {
    report.problems.push("the run passed its deadline: the browser stopped answering");
    finish(EXIT.app, "browser-hang");
  });
  let browser;
  try {
    browser = await LAUNCHERS[args.browser].launch(launchConfig(args.browser, process.platform, args).options);
  } catch (e) {
    const kind = launchFailureKind(e.message, process.env);
    report.problems.push(e.message);
    stopWatchdog();
    server.close();
    await finish(kind === "unsupported" ? EXIT.unsupported : EXIT.app, kind);
  }
  try {
    for (const runtimeCase of args.cases) {
      const run = await runCase(browser, server, runtimeCase, args);
      report.runs[runtimeCase] = run;
      report.problems.push(...run.problems.map((p) => `${runtimeCase}: ${p}`));
      if (run.unsupported) {
        report.unsupported.push(runtimeCase);
        console.log(`${runtimeCase}: unsupported: ${run.unsupported}`);
      } else {
        // Chromium's headless shell loses a WebGPU canvas's device by itself
        // before the runner gets to: still a lost device reaching the page.
        const by = run.seen?.lost ? ` (device lost by the ${run.seen.lost.by})` : "";
        console.log(`${runtimeCase}: ${run.problems.length ? "FAIL" : "ok"}${by}; page shows ${JSON.stringify(run.seen?.status ?? null)}`);
      }
    }
  } catch (e) {
    report.problems.push(e.message);
  } finally {
    await call(browser.close(), "closing the browser").catch(() => {});
    server.close();
    stopWatchdog();
  }
  for (const p of report.problems) console.error(`FAIL: ${p}`);
  const [code, outcome] = runOutcome(report, args);
  if (code === EXIT.pass) console.log(outcome === "passed" ? "PASS" : `PASS (unsupported here: ${report.unsupported.join(", ")})`);
  else if (code === EXIT.unsupported) console.log(`UNSUPPORTED: ${report.unsupported.join(", ")}`);
  await finish(code, outcome);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
