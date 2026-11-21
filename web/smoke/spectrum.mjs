#!/usr/bin/env node
// The sea's CPU wave spectrum in the browser (see README.md, "Spectrum"):
// the fixture examples/web_spectrum.rs, built with and without WASM SIMD,
// timed in a headless browser.
//
//   node web/smoke/spectrum.mjs --build                 # build all three, time each 3 times
//   node web/smoke/spectrum.mjs --variants scalar,simd --repeat 5
//
// The builds (spectrum-lib.mjs, VARIANTS): `scalar`, the ordinary web
// build; `fft-simd`, with the `wasm-simd` feature (rustfft's SIMD FFT,
// no compiler flag); `simd`, that and `-C target-feature=+simd128` for the
// whole module. Each page load times one new moment of the field (the
// WebGL2 upload, `height_at`) and one with the sway's speed (`sample`,
// buoyancy) at 64, 128 and 256 cells; the runner checks every build says
// it was built as asked and timed everything, then prints the median of
// the loads' medians, their range, and each SIMD build's speed-up over
// the ordinary one with whether it is beyond the noise. Timings are never
// a pass or a fail. Exit codes as smoke.mjs: 0 passed, 1 failed, 2
// unsupported (no browser), 3 bad arguments or a missing build.
// report.json goes to --out.

import { spawnSync } from "node:child_process";
import { cpus, loadavg } from "node:os";
import { mkdir, stat, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { browserLog } from "./browser-log.mjs";
import { CALL_TIMEOUT_MS, EXIT, UsageError, launchConfig, launchFailureKind, startWatchdog, withDeadline } from "./lib.mjs";
import { LAUNCHERS, serve } from "./smoke.mjs";
import { USAGE, VARIANTS, benchDone, buildArgs, checkRun, parseArgs, summarize, summaryTable, variantEnv } from "./spectrum-lib.mjs";

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

/** One page load of a build: waits for every timing, or fails after `seconds`. */
async function load(browser, url, seconds) {
  const context = await browser.newContext();
  const page = await context.newPage();
  const lines = [];
  page.on("console", (m) => lines.push(`${m.type()}: ${m.text()}`));
  page.on("pageerror", (e) => lines.push(`pageerror: ${e.message}`));
  try {
    await withDeadline(page.goto(url), CALL_TIMEOUT_MS, `opening ${url}`);
    const deadline = Date.now() + seconds * 1000;
    let last = null;
    while (Date.now() < deadline) {
      last = await state(page);
      if (benchDone(last)) return last;
      if (lines.some((l) => l.startsWith("pageerror:"))) break;
      await sleep(250);
    }
    throw new Error(`the timings didn't finish within ${seconds}s; last state ${JSON.stringify(last)}\n  ${lines.slice(-10).join("\n  ")}`);
  } finally {
    await context.close();
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
  if (args.build) {
    for (const variant of args.variants) {
      console.log(`building ${variant}: ${buildArgs(variant).join(" ")}${VARIANTS[variant].simd128 ? " with simd128" : ""}`);
      if (spawnSync("bash", buildArgs(variant), { stdio: "inherit", env: variantEnv(variant, process.env) }).status !== 0) {
        process.exit(EXIT.usage);
      }
    }
  }
  for (const variant of args.variants) {
    const index = join(resolve(VARIANTS[variant].dir), "index.html");
    if (!(await stat(index).catch(() => null))) {
      console.error(`${index} is missing: pass --build`);
      process.exit(EXIT.usage);
    }
  }
  await mkdir(args.out, { recursive: true });
  const report = {
    fixture: "web_spectrum",
    browser: args.browser,
    machine: { cpus: cpus().length, cpu: cpus()[0]?.model ?? null, load_average: loadavg() },
    problems: [],
  };
  const finish = async (code, outcome) => {
    report.outcome = outcome;
    report.browserLog = browserLog.slice(-200);
    await writeFile(join(args.out, "report.json"), JSON.stringify(report, null, 2) + "\n");
    process.exit(code);
  };
  const servers = Object.fromEntries(
    await Promise.all(args.variants.map(async (v) => [v, await serve(resolve(VARIANTS[v].dir))])),
  );
  const stopWatchdog = startWatchdog((args.timeout * args.repeat * args.variants.length + 60) * 1000, () => {
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
  const loads = [];
  try {
    // Variants take turns, so a busy moment of the machine doesn't land on one alone.
    for (let r = 0; r < args.repeat; r++) {
      for (const variant of args.variants) {
        const done = await load(browser, `http://127.0.0.1:${servers[variant].address().port}/`, args.timeout);
        report.problems.push(...checkRun(variant, done).map((p) => `${variant}: ${p}`));
        loads.push({ variant, bench: done.bench });
        const at = (cells, kind) => {
          const row = done.bench.find((b) => b.cells === cells && b.kind === kind);
          return row ? [...row.moment_ms].sort((a, b) => a - b)[row.moment_ms.length >> 1].toFixed(2) : "?";
        };
        console.log(`${variant}, load ${r + 1}: one moment ${at(64, "height")} / ${at(128, "height")} / ${at(256, "height")} ms at 64 / 128 / 256 cells; with the sway's speed ${at(64, "sway")} / ${at(128, "sway")} / ${at(256, "sway")} ms`);
      }
    }
    const rows = summarize(loads);
    report.loads = loads;
    report.summary = rows;
    console.log(`\n${summaryTable(rows)}\n\nload average ${loadavg().map((l) => l.toFixed(0)).join(", ")} on ${cpus().length} CPUs`);
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
