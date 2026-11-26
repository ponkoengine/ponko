#!/usr/bin/env node
// WebGPU first, WebGL2 as the fallback (README.md, "WebGPU first"): a
// fixture built by web/build.sh, in a headless browser, once on the
// browser's own WebGPU and once per way WebGPU can fail before the game
// draws. Each failure is faked in the page before the game's wasm starts.
//
//   web/build.sh --features 3d web_smoke_3d
//   node web/smoke/webgpu-fallback.mjs --dir target/web/web_smoke_3d --chromium-mode new
//   node web/smoke/webgpu-fallback.mjs --case device-fails --chromium-mode new
//
// - webgpu: nothing faked. The game draws with WebGPU: the canvas has a
//   WebGPU context, the engine reports backend "webgpu", the console says
//   "ponko: drawing with WebGPU" and nothing about a fallback.
// - no-webgpu: the page has no navigator.gpu.
// - no-adapter: navigator.gpu.requestAdapter() resolves to null.
// - device-fails: GPUAdapter.requestDevice() rejects.
// - device-lost: GPUAdapter.requestDevice() gives a device the browser
//   has already lost (destroyed before the game gets it).
// - no-present: the adapter and its device work, but a canvas never shows
//   a frame: every frame texture a WebGPU canvas hands out is invalid, as
//   with the software fallback adapter of Chromium's headless shell on
//   macOS. The engine's test frame (on a canvas of its own) must catch it.
// Each failure case must end on WebGL2, not on a blank page: the game
// ticks, the canvas has a WebGL2 context and shows something, the engine
// reports backend "gl", there is no startup error, page error or panic,
// and the console has exactly one "ponko: falling back to WebGL2: <why>"
// line naming that failure. device-fails, device-lost and no-present need
// a real WebGPU adapter to fake on: without one they are unsupported, not
// failed.
//
// --repeat <n> loads the webgpu and no-webgpu pages n times each (default
// 3) and reports the median time to the first frame (the fixture's first
// published state, from the page's navigation start).
//
// Exit codes as smoke.mjs: 0 passed, 1 failed, 2 unsupported (no browser,
// or every case asked for was unsupported; --allow-unsupported makes that
// a pass), 3 bad arguments or a missing build. report.json goes to --out.

import { mkdir, stat, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { CALL_TIMEOUT_MS, EXIT, UsageError, canvasBackendOf, launchConfig, launchFailureKind, startWatchdog, withDeadline } from "./lib.mjs";
import { GAME_CANVAS, LAUNCHERS, canvasBackend, canvasStats, serve } from "./smoke.mjs";

const USAGE =
  "usage: webgpu-fallback.mjs [--dir <site>] [--browser chromium|firefox|webkit]\n" +
  "                           [--case webgpu|no-webgpu|no-adapter|device-fails|device-lost|no-present] [--repeat <n>]\n" +
  "                           [--out <dir>] [--timeout <s>] [--chromium-mode shell|new] [--browser-arg <flag>]...\n" +
  "                           [--allow-unsupported]";

export const FALLBACK_CASES = ["webgpu", "no-webgpu", "no-adapter", "device-fails", "device-lost", "no-present"];

/** The message a faked requestDevice() rejects with (device-fails). */
export const REFUSAL = "webgpu-fallback: requestDevice refused by the runner";

/** The console line prefix of a fallback (src/render/browser_gpu.rs). */
export const FALLBACK_LINE = "ponko: falling back to WebGL2: ";
export const WEBGPU_LINE = "ponko: drawing with WebGPU";

/** The words the fallback line must hold for each failure case. */
export function expectedReason(fallbackCase) {
  switch (fallbackCase) {
    case "no-webgpu": return "navigator.gpu is missing";
    case "no-adapter": return "requestAdapter() found no adapter";
    case "device-fails": return REFUSAL;
    case "device-lost": return "the WebGPU device was lost as it opened";
    case "no-present": return "a canvas can't show WebGPU frames: a test frame failed";
    case "webgpu": return null;
    default: throw new UsageError(`unknown case ${fallbackCase} (${FALLBACK_CASES.join(", ")})`);
  }
}

/** Whether a case needs a real WebGPU adapter to fake its failure on. */
export function needsAdapter(fallbackCase) {
  return ["webgpu", "device-fails", "device-lost", "no-present"].includes(fallbackCase);
}

/**
 * What is wrong with one page load: `seen` holds the fixture's ticks, the
 * canvas's context, the engine's backend, the page's status line, the
 * console lines, page errors and the canvas's colors. Empty: it passed.
 */
export function checkCase(fallbackCase, seen) {
  const problems = [];
  const want = fallbackCase === "webgpu" ? "webgpu" : "webgl2";
  if (!(seen.ticks > 0)) problems.push(`the game never ticked (status ${JSON.stringify(seen.status)})`);
  if (seen.status) problems.push(`the page shows ${JSON.stringify(seen.status)}`);
  if (seen.canvas !== want) problems.push(`the canvas has a ${seen.canvas} context, wanted ${want}`);
  if (canvasBackendOf(seen.engineBackend) !== want) {
    problems.push(`the engine reports backend ${JSON.stringify(seen.engineBackend)}, wanted ${want === "webgpu" ? "webgpu" : "gl"}`);
  }
  for (const error of seen.pageErrors) problems.push(`page error: ${error}`);
  const panic = seen.console.find((line) => line.includes("panicked at"));
  if (panic) problems.push(`panic: ${panic}`);
  const fallbacks = seen.console.filter((line) => line.includes(FALLBACK_LINE));
  const reason = expectedReason(fallbackCase);
  if (reason === null) {
    if (fallbacks.length) problems.push(`WebGPU worked, yet the console says ${JSON.stringify(fallbacks[0])}`);
    if (!seen.console.some((line) => line.includes(WEBGPU_LINE))) problems.push(`no "${WEBGPU_LINE}" line in the console`);
  } else if (fallbacks.length !== 1) {
    problems.push(`wanted one "${FALLBACK_LINE}..." line, got ${fallbacks.length}: ${JSON.stringify(fallbacks)}`);
  } else if (!fallbacks[0].includes(reason)) {
    problems.push(`the fallback line ${JSON.stringify(fallbacks[0])} should say ${JSON.stringify(reason)}`);
  }
  if (seen.ticks > 0 && (!seen.pixels || blank(seen.pixels))) problems.push(`the canvas looks blank: ${JSON.stringify(seen.pixels)}`);
  return problems;
}

/** A canvas of fewer than 3 colors, or almost all one color, as smoke.mjs judges it. */
export function blank(stats) {
  return stats.colors < 3 || stats.foreground < 0.001;
}

/** The middle value (the lower of the two middle ones for an even count), or null. */
export function median(values) {
  const sorted = values.filter((v) => typeof v === "number").sort((a, b) => a - b);
  return sorted.length ? sorted[Math.floor((sorted.length - 1) / 2)] : null;
}

/** Exit code and outcome, as runtime-failures.mjs: a problem fails; all unsupported is unsupported. */
export function runOutcome(report, args) {
  if (report.problems.length) return [EXIT.app, "failed"];
  const ran = Object.keys(report.runs).length;
  if (ran > 0 && report.unsupported.length === ran) {
    return args.allowUnsupported ? [EXIT.pass, "unsupported"] : [EXIT.unsupported, "unsupported"];
  }
  return [EXIT.pass, report.unsupported.length ? "passed, some unsupported" : "passed"];
}

export function parseArgs(argv) {
  const args = {
    dir: "target/web/web_smoke_3d",
    browser: "chromium",
    cases: FALLBACK_CASES,
    repeat: 3,
    out: "target/web-smoke/webgpu-fallback",
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
      case "--browser":
        args.browser = value();
        if (!LAUNCHERS[args.browser]) throw new UsageError(`unknown browser ${args.browser}`);
        break;
      case "--case": {
        const one = value();
        expectedReason(one);
        args.cases = [one];
        break;
      }
      case "--repeat":
        args.repeat = Number(value());
        if (!Number.isInteger(args.repeat) || args.repeat < 1) throw new UsageError("--repeat needs a whole number of at least 1");
        break;
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

/** Installed before the page's scripts: the time of the fixture's first published state. */
function watchFirstFrame() {
  const observer = new MutationObserver(() => {
    if (document.documentElement?.hasAttribute("data-smoke")) {
      globalThis.__firstFrameMs ??= performance.now();
      observer.disconnect();
    }
  });
  observer.observe(document, { attributes: true, subtree: true, attributeFilter: ["data-smoke"] });
}

/** Installed before the page's scripts: the failure `fallbackCase` fakes. */
function fake([fallbackCase, refusal]) {
  if (fallbackCase === "no-webgpu") {
    Object.defineProperty(Navigator.prototype, "gpu", { get: () => undefined, configurable: true });
  } else if (fallbackCase === "no-adapter") {
    if (globalThis.GPU) {
      GPU.prototype.requestAdapter = async () => null;
    } else {
      // A browser without WebGPU: a navigator.gpu that finds no adapter.
      const gpu = { requestAdapter: async () => null, getPreferredCanvasFormat: () => "bgra8unorm" };
      Object.defineProperty(Navigator.prototype, "gpu", { get: () => gpu, configurable: true });
    }
  } else if (fallbackCase === "device-fails" && globalThis.GPUAdapter) {
    GPUAdapter.prototype.requestDevice = async () => {
      throw new DOMException(refusal, "OperationError");
    };
  } else if (fallbackCase === "device-lost" && globalThis.GPUAdapter) {
    const requestDevice = GPUAdapter.prototype.requestDevice;
    GPUAdapter.prototype.requestDevice = async function (...args) {
      const device = await requestDevice.apply(this, args);
      device.destroy();
      return device;
    };
  } else if (fallbackCase === "no-present" && globalThis.GPUCanvasContext) {
    // Every frame texture is invalid (a 1D texture can't be drawn to), on
    // every WebGPU canvas, the page's and any other.
    const configure = GPUCanvasContext.prototype.configure;
    const getCurrentTexture = GPUCanvasContext.prototype.getCurrentTexture;
    const configs = new WeakMap();
    GPUCanvasContext.prototype.configure = function (config) {
      configs.set(this, config);
      return configure.call(this, config);
    };
    GPUCanvasContext.prototype.getCurrentTexture = function () {
      const config = configs.get(this);
      if (!config) return getCurrentTexture.call(this);
      return config.device.createTexture({ size: [1], dimension: "1d", format: config.format, usage: GPUTextureUsage.RENDER_ATTACHMENT });
    };
  }
}

/** Whether the browser has a WebGPU adapter, asked on a blank page. */
async function hasAdapter(browser, url) {
  const context = await browser.newContext();
  try {
    const page = await context.newPage();
    await call(page.goto(`${url}missing-probe-page`), "opening a blank page");
    return await call(page.evaluate(async () => Boolean(navigator.gpu && (await navigator.gpu.requestAdapter()))), "asking for an adapter");
  } finally {
    await call(context.close(), "closing the probe").catch(() => {});
  }
}

async function load(browser, url, fallbackCase, args) {
  const context = await browser.newContext({ viewport: { width: 800, height: 600 } });
  const consoleLines = [];
  const pageErrors = [];
  try {
    await context.addInitScript(watchFirstFrame);
    // An init script takes one argument: the case and the refusal together.
    await context.addInitScript(fake, [fallbackCase, REFUSAL]);
    const page = await context.newPage();
    page.on("console", (m) => consoleLines.push(`${m.type()}: ${m.text()}`));
    page.on("pageerror", (e) => pageErrors.push(String(e.message)));
    await call(page.goto(url), "opening the game");
    const deadline = Date.now() + args.timeout * 1000;
    let state;
    for (;;) {
      state = await call(page.evaluate(() => {
        const raw = document.documentElement.getAttribute("data-smoke");
        const gpu = document.documentElement.getAttribute("data-smoke-gpu");
        const status = document.getElementById("status");
        return {
          ticks: raw ? JSON.parse(raw).ticks : 0,
          engineBackend: gpu ? JSON.parse(gpu).adapter?.backend ?? null : null,
          status: status && status.className === "error" ? status.textContent : null,
          firstFrameMs: globalThis.__firstFrameMs ?? null,
        };
      }), "reading the page");
      if ((state.ticks > 0 && state.engineBackend) || state.status || pageErrors.length || Date.now() > deadline) break;
      await sleep(100);
    }
    // Let it run a moment, so a late failure still shows.
    await sleep(500);
    const canvas = await call(canvasBackend(page), "reading the canvas's context");
    // A screenshot first, as smoke.mjs does; a copy of the canvas where the
    // screenshot misses a WebGPU canvas.
    let pixels = null;
    if (state.ticks > 0) {
      const png = await call(page.locator(GAME_CANVAS).screenshot(), "taking the canvas screenshot");
      await writeFile(join(args.out, `${fallbackCase}.png`), png);
      pixels = await call(canvasStats(page, png), "reading the screenshot");
      if (blank(pixels)) pixels = await call(canvasStats(page, null), "reading the canvas").catch(() => pixels);
    }
    return { ...state, canvas, pixels, console: consoleLines, pageErrors };
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
  const root = resolve(args.dir);
  if (!(await stat(join(root, "index.html")).catch(() => null))) {
    console.error(`${join(root, "index.html")} is missing: build it with web/build.sh (see the usage at the top of this file)`);
    process.exit(EXIT.usage);
  }
  await mkdir(args.out, { recursive: true });
  const report = { site: args.dir, browser: args.browser, chromiumMode: args.chromiumMode, runs: {}, firstFrameMs: {}, problems: [], unsupported: [] };
  const finish = async (code, outcome) => {
    report.outcome = outcome;
    await writeFile(join(args.out, "report.json"), JSON.stringify(report, null, 2) + "\n");
    process.exit(code);
  };
  const server = await serve(root);
  const url = `http://127.0.0.1:${server.address().port}/`;
  const loads = args.cases.reduce((n, c) => n + (c === "webgpu" || c === "no-webgpu" ? args.repeat : 1), 0);
  const stopWatchdog = startWatchdog((args.timeout * loads + 60) * 1000, () => {
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
    report.browserVersion = `${args.browser} ${browser.version()}`;
    report.webgpuAdapter = await hasAdapter(browser, url);
    console.log(`${report.browserVersion} (${args.chromiumMode}); WebGPU adapter: ${report.webgpuAdapter ? "yes" : "no"}`);
    for (const fallbackCase of args.cases) {
      if (needsAdapter(fallbackCase) && !report.webgpuAdapter) {
        report.unsupported.push(fallbackCase);
        report.runs[fallbackCase] = { unsupported: "this browser gives no WebGPU adapter here" };
        console.log(`${fallbackCase}: unsupported: this browser gives no WebGPU adapter here`);
        continue;
      }
      const times = [];
      const loadsOfCase = fallbackCase === "webgpu" || fallbackCase === "no-webgpu" ? args.repeat : 1;
      let last;
      for (let n = 0; n < loadsOfCase; n++) {
        last = await load(browser, url, fallbackCase, args);
        const problems = checkCase(fallbackCase, last);
        times.push(last.firstFrameMs);
        report.problems.push(...problems.map((p) => `${fallbackCase}: ${p}`));
        if (problems.length) break;
      }
      const fallback = last.console.find((line) => line.includes(FALLBACK_LINE)) ?? null;
      report.runs[fallbackCase] = { ...last, firstFrameMsEach: times };
      if (loadsOfCase > 1) report.firstFrameMs[fallbackCase] = median(times);
      const failed = report.problems.some((p) => p.startsWith(`${fallbackCase}: `));
      console.log(
        `${fallbackCase}: ${failed ? "FAIL" : "ok"} on ${last.canvas} (engine: ${last.engineBackend}); ` +
          `first frame ${times.map((t) => (t === null ? "-" : `${Math.round(t)} ms`)).join(", ")}` +
          (fallback ? `; console: ${fallback}` : ""),
      );
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
