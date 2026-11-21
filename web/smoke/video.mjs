#!/usr/bin/env node
// Video in the browser (see README.md, "Video"): the fixture
// examples/web_video.rs plays the engine's test clip through
// `ponko::video::Video`, decoded by the browser's own AV1 decoder
// (WebCodecs), and reports every frame it showed; this runner opens it in a
// headless browser and checks the report (video-lib.mjs, checkVideo): the
// pixels of each frame, their order and lag behind the clock, the end, and
// two seeks while paused.
//
//   node web/smoke/video.mjs --build        # web/build.sh --features video web_video, then run
//   node web/smoke/video.mjs --dir target/web/web_video
//
// The page runs on WebGL2 (WebGPU hidden, as the CI's other Chromium runs).
// Exit codes as smoke.mjs: 0 passed, 1 failed, 2 unsupported (no browser,
// or a browser without WebCodecs AV1), 3 bad arguments or a missing build.
// report.json goes to --out.

import { spawnSync } from "node:child_process";
import { cpus, loadavg } from "node:os";
import { mkdir, stat, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { browserLog } from "./browser-log.mjs";
import { CALL_TIMEOUT_MS, EXIT, UsageError, launchConfig, launchFailureKind, startWatchdog, withDeadline } from "./lib.mjs";
import { LAUNCHERS, serve } from "./smoke.mjs";
import { USAGE, buildArgs, checkVideo, parseArgs, videoDone } from "./video-lib.mjs";

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

/** Whether the page's browser decodes AV1 with WebCodecs. */
async function av1Support(page) {
  return withDeadline(
    page.evaluate(async () => {
      if (typeof VideoDecoder === "undefined") return { supported: false, reason: "no VideoDecoder" };
      const { supported } = await VideoDecoder.isConfigSupported({ codec: "av01.0.04M.08", codedWidth: 64, codedHeight: 64 });
      return { supported, reason: supported ? null : "VideoDecoder turns AV1 down" };
    }),
    CALL_TIMEOUT_MS,
    "asking for WebCodecs AV1",
  );
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
  if (args.build) {
    console.log(`building: ${buildArgs(args.dir).join(" ")}`);
    if (spawnSync("bash", buildArgs(args.dir), { stdio: "inherit" }).status !== 0) process.exit(EXIT.usage);
  }
  const site = resolve(args.dir);
  if (!(await stat(join(site, "index.html")).catch(() => null))) {
    console.error(`${join(site, "index.html")} is missing: pass --build`);
    process.exit(EXIT.usage);
  }
  await mkdir(args.out, { recursive: true });
  const report = {
    fixture: "web_video",
    browser: args.browser,
    machine: { cpus: cpus().length, load_average: loadavg() },
    problems: [],
  };
  const finish = async (code, outcome) => {
    report.outcome = outcome;
    report.browserLog = browserLog.slice(-200);
    await writeFile(join(args.out, "report.json"), JSON.stringify(report, null, 2) + "\n");
    process.exit(code);
  };
  const server = await serve(site);
  const stopWatchdog = startWatchdog((args.timeout + 60) * 1000, () => {
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
    server.close();
    await finish(kind === "unsupported" ? EXIT.unsupported : EXIT.app, kind);
  }
  let code = EXIT.pass;
  let outcome = "passed";
  try {
    const context = await browser.newContext();
    // Hide WebGPU so the engine takes its WebGL2 fallback.
    await context.addInitScript(() => {
      Object.defineProperty(Navigator.prototype, "gpu", { get: () => undefined, configurable: true });
    });
    const page = await context.newPage();
    const lines = [];
    page.on("console", (m) => lines.push(`${m.type()}: ${m.text()}`));
    page.on("pageerror", (e) => lines.push(`pageerror: ${e.message}`));
    await withDeadline(page.goto(`http://127.0.0.1:${server.address().port}/`), CALL_TIMEOUT_MS, "opening the page");
    report.av1 = await av1Support(page);
    if (!report.av1.supported) {
      report.problems.push(`unsupported: ${report.av1.reason}`);
      code = EXIT.unsupported;
      outcome = "unsupported";
    } else {
      const deadline = Date.now() + args.timeout * 1000;
      let last = null;
      while (Date.now() < deadline) {
        last = await state(page);
        if (videoDone(last)) break;
        if (lines.some((l) => l.startsWith("pageerror:"))) break;
        await sleep(100);
      }
      report.state = last;
      console.log(`fixture: ${JSON.stringify(last?.video ?? null)}`);
      if (!videoDone(last)) {
        report.problems.push(`the fixture didn't finish within ${args.timeout}s; last state ${JSON.stringify(last)}\n  ${lines.slice(-10).join("\n  ")}`);
      } else {
        report.problems.push(...checkVideo(last.video));
      }
    }
    await context.close();
  } catch (e) {
    report.problems.push(e.message);
  } finally {
    await withDeadline(browser.close(), CALL_TIMEOUT_MS, "closing the browser").catch(() => {});
    server.close();
    stopWatchdog();
  }
  if (code === EXIT.pass && report.problems.length) {
    code = EXIT.app;
    outcome = "failed";
  }
  for (const p of report.problems) console.error(`${code === EXIT.unsupported ? "UNSUPPORTED" : "FAIL"}: ${p}`);
  if (code === EXIT.pass) console.log(`PASS (load average ${loadavg().map((l) => l.toFixed(0)).join(", ")})`);
  await finish(code, outcome);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
