#!/usr/bin/env node
// Frame times of the 3D smoke scene on a real GPU (README.md, "Frame
// times"; the web guide's "Frame times"): the 3D fixture built by
// web/build.sh, in headless Chromium's new headless mode, on WebGPU and on
// WebGL2.
//
//   web/build.sh --features 3d web_smoke_3d
//   node web/smoke/frame-times.mjs                       # uncapped, 3 loads per backend
//   node web/smoke/frame-times.mjs --pacing display      # at the browser's own frame rate
//   node web/smoke/frame-times.mjs --repeat 1 --frames 300
//
// Each page load waits for --warmup frames, then stamps the next --frames
// frames: a page script notes performance.now() each time the fixture
// publishes a new frame count (its <html data-smoke>, set once per engine
// frame), so a frame time is the time from one engine frame to the next.
// The page is served cross-origin isolated, which gives performance.now()
// its fine resolution (5 µs instead of 100 µs). With --pacing uncapped (the
// default) Chromium's vsync and frame-rate limit are off, so frames follow
// each other as fast as the engine, the browser and the GPU allow; with
// --pacing display it keeps its own pacing (60 Hz), which shows whether the
// scene holds the display's rate. Each load reports the median, 95th and
// 99th percentile, mean and slowest frame, and the engine's own account
// (`frame_stats` in the fixture's state: CPU time in systems and drawing,
// and GPU time where the browser times GPU passes). The summary gives the
// median over the loads, with the lowest and highest; the machine's load
// average is noted before and after each load.
//
// The numbers only mean something on a real GPU, so this is also a gate:
// each backend must run on a hardware adapter (no SwiftShader, no fallback
// adapter, no software WebGL2), or the run fails. Exit codes as smoke.mjs:
// 0 passed, 1 failed (no real GPU among them), 3 bad arguments or a
// missing build. report.json goes to --out.

import { spawnSync } from "node:child_process";
import { cpus, loadavg } from "node:os";
import { mkdir, stat, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { browserLog } from "./browser-log.mjs";
import { CALL_TIMEOUT_MS, EXIT, UsageError, launchConfig, softwareGpuReason, startWatchdog, withDeadline } from "./lib.mjs";
import { LAUNCHERS, canvasBackend, probe, serve } from "./smoke.mjs";
import { ISOLATION_HEADERS } from "./threads-lib.mjs";
import { PACING_FLAGS, USAGE, frameStats, parseArgs, summarize, summaryTable } from "./frame-times-lib.mjs";

/** Notes when each new engine frame is published (<html data-smoke>'s `frames`). */
function stampFrames() {
  window.__frameStamps = [];
  let last = null;
  new MutationObserver(() => {
    const raw = document.documentElement?.getAttribute("data-smoke");
    if (!raw) return;
    const now = performance.now();
    try {
      const frames = JSON.parse(raw).frames;
      if (frames !== last) {
        last = frames;
        window.__frameStamps.push(now);
      }
    } catch {
      // A state that isn't JSON yet: not a frame.
    }
  }).observe(document, { subtree: true, attributes: true, attributeFilter: ["data-smoke"] });
}

async function measure(browser, url, backend, args, caps) {
  const context = await browser.newContext({ viewport: { width: 800, height: 600 } });
  try {
    if (backend === "webgl2") {
      // Hide WebGPU so the engine takes its WebGL2 path.
      await context.addInitScript(() => {
        Object.defineProperty(Navigator.prototype, "gpu", { get: () => undefined, configurable: true });
      });
    }
    await context.addInitScript(stampFrames);
    const page = await context.newPage();
    const errors = [];
    page.on("pageerror", (e) => errors.push(String(e.message ?? e)));
    const loadBefore = loadavg()[0];
    await withDeadline(page.goto(url), CALL_TIMEOUT_MS, "opening the page");
    const needed = args.warmup + args.frames + 1;
    await withDeadline(
      page.waitForFunction((n) => window.__frameStamps.length >= n, needed, { timeout: args.timeout * 1000, polling: 100 }),
      args.timeout * 1000 + CALL_TIMEOUT_MS,
      `${needed} frames on ${backend}`,
    ).catch((e) => {
      throw new Error(`${backend}: fewer than ${needed} frames within ${args.timeout}s (${e.message.split("\n")[0]})${errors.length ? `; page errors: ${errors.join(" | ")}` : ""}`);
    });
    // The engine refreshes its own statistics four times a second, over its
    // last frames: wait for one refresh after the measured frames.
    await new Promise((ok) => setTimeout(ok, 500));
    const { stamps, state, gpu, isolated } = await withDeadline(
      page.evaluate(() => ({
        stamps: window.__frameStamps.slice(),
        state: JSON.parse(document.documentElement.getAttribute("data-smoke")),
        gpu: JSON.parse(document.documentElement.getAttribute("data-smoke-gpu") ?? "null"),
        isolated: window.crossOriginIsolated,
      })),
      CALL_TIMEOUT_MS,
      "reading the frame stamps",
    );
    const used = await withDeadline(canvasBackend(page), CALL_TIMEOUT_MS, "reading the canvas backend");
    if (used !== backend) throw new Error(`asked for ${backend} but the canvas uses ${used}`);
    const software = softwareGpuReason(caps, backend, gpu);
    if (software) throw new Error(`${backend}: a real GPU is required, but ${software}`);
    if (errors.length) throw new Error(`${backend}: page errors: ${errors.join(" | ")}`);
    const measured = stamps.slice(args.warmup, args.warmup + args.frames + 1);
    return {
      backend,
      stats: frameStats(measured),
      engine: state.frame_stats ?? null,
      triangles: state.triangles,
      adapter: backend === "webgpu" ? caps.webgpu : caps.webgl2,
      isolated,
      load_average: [loadBefore, loadavg()[0]],
    };
  } finally {
    await withDeadline(context.close(), CALL_TIMEOUT_MS, "closing the page").catch(() => {});
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
  if (args.build && spawnSync("bash", ["web/build.sh", "--features", "3d", "web_smoke_3d", args.dir], { stdio: "inherit" }).status !== 0) {
    process.exit(EXIT.usage);
  }
  const root = resolve(args.dir);
  if (!(await stat(join(root, "index.html")).catch(() => null))) {
    console.error(`${join(root, "index.html")} is missing: pass --build, or build it with web/build.sh --features 3d web_smoke_3d`);
    process.exit(EXIT.usage);
  }
  await mkdir(args.out, { recursive: true });
  const report = {
    fixture: "web_smoke_3d",
    pacing: args.pacing,
    frames: args.frames,
    warmup: args.warmup,
    viewport: [800, 600],
    machine: { platform: `${process.platform}-${process.arch}`, cpus: cpus().length, cpu: cpus()[0]?.model ?? null, load_average: loadavg() },
    loads: [],
    problems: [],
  };
  const finish = async (code) => {
    report.outcome = code === EXIT.pass ? "passed" : "failed";
    report.browserLog = browserLog.slice(-200);
    await writeFile(join(args.out, "report.json"), JSON.stringify(report, null, 2) + "\n");
    process.exit(code);
  };
  const loads = args.repeat * args.backends.length;
  const stopWatchdog = startWatchdog((args.timeout * (loads + 1) + 60) * 1000, () => {
    report.problems.push("the run passed its deadline: the browser stopped answering");
    finish(EXIT.app);
  });
  const server = await serve(root, ISOLATION_HEADERS);
  const url = `http://127.0.0.1:${server.address().port}/`;
  let browser;
  try {
    const { options } = launchConfig("chromium", process.platform, {
      chromiumMode: "new",
      requireGpu: true,
      browserArgs: PACING_FLAGS[args.pacing],
    });
    browser = await LAUNCHERS.chromium.launch(options);
    report.browser = `chromium ${browser.version()}`;
    console.log(`${report.browser}, ${report.machine.platform}, ${report.machine.cpu}, load average ${report.machine.load_average.map((l) => l.toFixed(1)).join(" ")}`);
    const caps = {};
    for (const backend of args.backends) {
      const context = await browser.newContext();
      caps[backend] = await withDeadline(probe(context, url, backend), CALL_TIMEOUT_MS * 2, `probing ${backend}`);
      await context.close();
    }
    report.capabilities = caps;
    for (let r = 0; r < args.repeat; r++) {
      for (const backend of args.backends) {
        const load = await measure(browser, url, backend, args, caps[backend]);
        report.loads.push(load);
        const s = load.stats;
        const e = load.engine;
        const engine = e
          ? `; engine: systems ${e.systems_ms.toFixed(2)}, drawing ${e.render_cpu_ms.toFixed(2)}, GPU ${e.gpu_ms == null ? "not timed" : e.gpu_ms.toFixed(2)} ms`
          : "";
        console.log(
          `${backend}, load ${r + 1}: median ${s.median_ms.toFixed(2)} ms, p95 ${s.p95_ms.toFixed(2)}, p99 ${s.p99_ms.toFixed(2)}, ` +
            `max ${s.max_ms.toFixed(2)} over ${s.frames} frames${engine} (load average ${load.load_average.map((l) => l.toFixed(1)).join(" -> ")})`,
        );
      }
    }
    report.summary = summarize(report.loads);
    console.log(`\n${args.pacing}, ${args.frames} frames after ${args.warmup}, 800x600:\n${summaryTable(report.summary)}`);
  } catch (e) {
    report.problems.push(e.message);
  } finally {
    await withDeadline(browser?.close() ?? Promise.resolve(), CALL_TIMEOUT_MS, "closing the browser").catch(() => {});
    server.close();
    stopWatchdog();
  }
  for (const p of report.problems) console.error(`FAIL: ${p}`);
  if (!report.problems.length) console.log("PASS");
  await finish(report.problems.length ? EXIT.app : EXIT.pass);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
