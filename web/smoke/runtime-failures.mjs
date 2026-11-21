#!/usr/bin/env node
// Failures while a web game runs (see README.md, "Runtime failures"): the
// 2D fixture (web/build.sh web_smoke) in a headless browser, stopped three
// ways after it started. Each must reach the page as the engine reported
// it, through the generated template and through `onPonkoFailure`.
//
//   node web/smoke/runtime-failures.mjs --build
//   node web/smoke/runtime-failures.mjs --dir target/web/web_smoke --browser chromium
//
// - runtime-panic: the fixture panics in a frame after 20 ticks. The page
//   shows "The game stopped: <the panic's message>"; the callback gets
//   kind "Panic", the message and the panic's source position.
// - failure-text: the same panic, with the game's own text registered
//   through `ponko::web::on_failure`: the page shows that text instead.
// - context-lost: on WebGL2, the runner loses the canvas's context while
//   the game runs: kind "ContextLost" and its message on the page.
//
// Exit codes as smoke.mjs: 0 passed, 1 failed, 2 unsupported (no browser),
// 3 bad arguments or a missing build. report.json goes to --out.

import { spawnSync } from "node:child_process";
import { mkdir, stat, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { browserLog } from "./browser-log.mjs";
import { CALL_TIMEOUT_MS, EXIT, UsageError, launchConfig, launchFailureKind, startWatchdog, withDeadline } from "./lib.mjs";
import { LAUNCHERS, serve } from "./smoke.mjs";

const USAGE =
  "usage: runtime-failures.mjs [--dir <site>] [--build] [--browser chromium|firefox|webkit]\n" +
  "                            [--case runtime-panic|failure-text|context-lost] [--out <dir>] [--timeout <s>]";

export const RUNTIME_CASES = ["runtime-panic", "failure-text", "context-lost"];

/** The page's text and the callback's failure each case must produce. */
export function expected(runtimeCase) {
  const panic = `web_smoke: panicking while running, as ?smoke_case=${runtimeCase} asked`;
  switch (runtimeCase) {
    case "runtime-panic":
      return { kind: "Panic", message: panic, text: null, status: `The game stopped: ${panic}` };
    case "failure-text": {
      const text = `web_smoke stopped, in its own words: ${panic}`;
      return { kind: "Panic", message: panic, text, status: text };
    }
    case "context-lost": {
      const message = "The WebGL2 context was lost while the game was running. Reload the page to try again.";
      return { kind: "ContextLost", message, text: null, status: `The game stopped: ${message}` };
    }
    default:
      throw new UsageError(`unknown case ${runtimeCase} (${RUNTIME_CASES.join(", ")})`);
  }
}

/**
 * What is wrong with one run: `seen` holds the state the fixture published
 * before it stopped, the page's status line and the failure a late
 * `onPonkoFailure` subscriber got. An empty list is a pass.
 */
export function checkRun(runtimeCase, seen) {
  const want = expected(runtimeCase);
  const problems = [];
  if (!(seen.ticks > 0)) problems.push(`the game never ran a tick before it stopped (ticks ${seen.ticks})`);
  if (seen.status !== want.status) problems.push(`the page shows ${JSON.stringify(seen.status)}, wanted ${JSON.stringify(want.status)}`);
  const failure = seen.failure;
  if (!failure) {
    problems.push("onPonkoFailure never called its callback");
    return problems;
  }
  for (const field of ["kind", "message", "text"]) {
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
  return problems;
}

function parseArgs(argv) {
  const args = {
    dir: "target/web/web_smoke",
    build: false,
    browser: "chromium",
    cases: RUNTIME_CASES,
    out: "target/web-smoke/runtime-failures",
    timeout: 60,
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
        failure = { kind: error.kind, message: error.message, location: error.location, text: error.text };
      });
    } catch {
      // Still loading.
    }
    return {
      ticks: raw ? JSON.parse(raw).ticks : 0,
      status: status && status.className === "error" ? status.textContent : null,
      failure,
    };
  }), "reading the page");
}

async function runCase(browser, server, runtimeCase, args) {
  const context = await browser.newContext();
  const lines = [];
  try {
    if (runtimeCase === "context-lost") {
      // WebGL2 only: a WebGPU canvas has no context to lose this way.
      await context.addInitScript(() => {
        Object.defineProperty(Navigator.prototype, "gpu", { get: () => undefined, configurable: true });
      });
    }
    const page = await context.newPage();
    page.on("console", (m) => lines.push(`${m.type()}: ${m.text()}`));
    page.on("pageerror", (e) => lines.push(`pageerror: ${e.message}`));
    const query = runtimeCase === "context-lost" ? "" : `?smoke_case=${runtimeCase}`;
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
  const report = { fixture: "web_smoke", browser: args.browser, runs: {}, problems: [] };
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
    browser = await LAUNCHERS[args.browser].launch(launchConfig(args.browser, process.platform).options);
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
      console.log(`${runtimeCase}: ${run.problems.length ? "FAIL" : "ok"}; page shows ${JSON.stringify(run.seen?.status ?? null)}`);
    }
  } catch (e) {
    report.problems.push(e.message);
  } finally {
    await call(browser.close(), "closing the browser").catch(() => {});
    server.close();
    stopWatchdog();
  }
  for (const p of report.problems) console.error(`FAIL: ${p}`);
  if (report.problems.length === 0) console.log("PASS");
  await finish(report.problems.length ? EXIT.app : EXIT.pass, report.problems.length ? "failed" : "passed");
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
