#!/usr/bin/env node
// Browser replication smoke test (see README.md, "Replication"): a web build
// of the replication reference game (examples/net_arena) joins a native
// dedicated server over WebSocket and must see replicated entities move.
//
//   node web/smoke/replication.mjs --build
//   node web/smoke/replication.mjs --dir target/web/net_arena --native target/debug/examples/net_arena
//
// A native dedicated server (`net_arena server 127.0.0.1:0 --web 127.0.0.1:0`,
// no window) owns the world and moves its orbs. A headless-browser tab of the
// web build joins it (`?server=ws://...`) and publishes what it holds in
// `<html data-smoke>`. The run passes once the tab has joined, holds the
// server's orbs and its own player (spawned for it by the server and
// replicated back, marked as its own), and has seen an orb move. Exit codes
// as smoke.mjs: 0 passed, 1 failed, 2 unsupported (no browser), 3 bad
// arguments or a missing build. report.json and the server's output go to
// --out.

import { spawn, spawnSync } from "node:child_process";
import { mkdir, stat, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { browserLog } from "./browser-log.mjs";
import { CALL_TIMEOUT_MS, EXIT, UsageError, launchConfig, launchFailureKind, startWatchdog, withDeadline } from "./lib.mjs";
import { LAUNCHERS, serve } from "./smoke.mjs";

/** How far (in arena units) an orb must have moved, as the tab saw it. */
export const MIN_MOVE = 5;

export const USAGE =
  "usage: replication.mjs [--build] [--dir <built net_arena site>] [--native <net_arena binary>]\n" +
  "                       [--browser chromium|firefox|webkit] [--out <dir>] [--run-timeout <s>]";

export function parseArgs(argv) {
  const args = {
    build: false,
    dir: null,
    native: null,
    browser: "chromium",
    out: "target/web-smoke/replication",
    runTimeout: 120,
  };
  for (let i = 0; i < argv.length; i++) {
    const value = () => {
      const v = argv[++i];
      if (v === undefined) throw new UsageError(`${argv[i - 1]} needs a value`);
      return v;
    };
    switch (argv[i]) {
      case "--build": args.build = true; break;
      case "--dir": args.dir = value(); break;
      case "--native": args.native = value(); break;
      case "--browser": args.browser = value(); break;
      case "--out": args.out = value(); break;
      case "--run-timeout": args.runTimeout = Number(value()); break;
      default: throw new UsageError(`unknown argument ${argv[i]}`);
    }
  }
  if (!(args.browser in LAUNCHERS)) throw new UsageError(`unknown browser ${args.browser}`);
  // Room for the browser to start, the wasm to load and the tab to join.
  if (!(args.runTimeout >= 30)) throw new UsageError("--run-timeout must be at least 30");
  return args;
}

/** The server's line: `serving on 127.0.0.1:1234, browsers on ws://127.0.0.1:5678`. */
export function parseServing(lines) {
  for (const line of lines ?? []) {
    const m = /serving on (\S+), browsers on (ws:\/\/\S+)/.exec(line);
    if (m) return { udp: m[1], web: m[2] };
  }
  return null;
}

/**
 * What the tab published (`{joined, error, replicas, orbs, moved, farthest,
 * mine, ticks}`, see examples/net_arena/main.rs), checked. Returns the list
 * of problems; empty means it joined and saw replicated entities move.
 */
export function checkReplication(state) {
  if (!state) return ["the tab reported nothing (did the game start?)"];
  const problems = [];
  if (state.error) problems.push(`the tab's replication failed: ${state.error}`);
  if (!state.joined) problems.push("the tab did not join the server");
  if (!(state.orbs > 0)) problems.push(`the tab holds no replicated orbs (${state.replicas ?? 0} replicas)`);
  if (!state.mine) problems.push("the tab holds no player of its own (the server spawns one for each client)");
  if (!(state.moved > 0 && state.farthest >= MIN_MOVE)) {
    problems.push(`no replicated orb moved ${MIN_MOVE} units on the tab (farthest ${Number(state.farthest ?? 0).toFixed(2)})`);
  }
  return problems;
}

const sleep = (ms) => new Promise((ok) => setTimeout(ok, ms));

async function main() {
  let args;
  try {
    args = parseArgs(process.argv.slice(2));
  } catch (e) {
    console.error(`${e.message}\n${USAGE}`);
    process.exit(EXIT.usage);
  }
  if (args.build) {
    const steps = [
      ["bash", ["web/build.sh", "net_arena"]],
      ["cargo", ["build", "--example", "net_arena"]],
    ];
    for (const [cmd, cmdArgs] of steps) {
      if (spawnSync(cmd, cmdArgs, { stdio: "inherit" }).status !== 0) process.exit(EXIT.usage);
    }
  }
  const target = JSON.parse(spawnSync("cargo", ["metadata", "--format-version", "1", "--no-deps"]).stdout).target_directory;
  const root = resolve(args.dir ?? join(target, "web", "net_arena"));
  const native = resolve(args.native ?? join(target, "debug", "examples", process.platform === "win32" ? "net_arena.exe" : "net_arena"));
  for (const [what, path] of [["the web build", join(root, "index.html")], ["the native example", native]]) {
    if (!(await stat(path).catch(() => null))) {
      console.error(`${what} is missing (${path}): pass --build, or build it with web/build.sh net_arena and cargo build --example net_arena`);
      process.exit(EXIT.usage);
    }
  }
  await mkdir(args.out, { recursive: true });

  const started = Date.now();
  const log = (m) => console.log(`[${((Date.now() - started) / 1000).toFixed(1)}s] ${m}`);
  // The fields summary.mjs lists (fixture, browser, backend, outcome), then this run's own.
  const report = {
    fixture: "net_arena (replication)",
    browser: args.browser,
    requestedBackend: "webgl2",
    state: null,
    console: [],
    pageErrors: [],
    problems: [],
  };
  let browser;
  let site;
  let outcome = EXIT.app;
  const server = { lines: [], stderr: "", exit: null };

  const finish = async (code, problems) => {
    outcome = code;
    report.problems = problems;
    const kind = code === EXIT.pass ? "pass" : code === EXIT.unsupported ? "unsupported" : "replication";
    report.outcome = { status: code, kind, message: problems.join("; ") || "joined and saw replicated entities move" };
    report.seconds = (Date.now() - started) / 1000;
    report.server = { exit: server.exit, stdout: server.lines.slice(-200), stderr: server.stderr.slice(-20_000) };
    await writeFile(join(args.out, "report.json"), JSON.stringify(report, null, 2) + "\n");
    await writeFile(join(args.out, "browser.log"), browserLog.join("\n") + "\n");
    const verdict = code === EXIT.pass ? "PASS" : code === EXIT.unsupported ? "UNSUPPORTED" : "FAIL";
    for (const p of problems) log(`problem: ${p}`);
    log(`${verdict}: browser replication, ${args.browser} (report in ${args.out}/report.json)`);
  };

  const stopWatchdog = startWatchdog(args.runTimeout * 1000, async () => {
    await finish(EXIT.app, [`the run did not finish within ${args.runTimeout}s`]);
    server.child?.kill();
    process.exit(outcome);
  });

  try {
    // 1. The native dedicated server, on free ports.
    server.child = spawn(native, ["server", "127.0.0.1:0", "--web", "127.0.0.1:0"], { stdio: ["ignore", "pipe", "pipe"] });
    let partial = "";
    server.child.stdout.on("data", (d) => {
      const parts = (partial + d).split("\n");
      partial = parts.pop();
      server.lines.push(...parts);
    });
    server.child.stderr.on("data", (d) => (server.stderr += d));
    server.child.on("exit", (code, signal) => (server.exit = { code, signal }));
    let serving = null;
    for (let i = 0; i < 300 && !serving && server.exit === null; i++) {
      serving = parseServing(server.lines);
      if (!serving) await sleep(100);
    }
    if (!serving) throw new Error(`the server did not start: ${server.stderr.slice(-2000)}`);
    log(`server: UDP ${serving.udp}, browsers ${serving.web}`);

    // 2. A tab of the web build, joining over WebSocket.
    site = await serve(root);
    const url = `http://127.0.0.1:${site.address().port}/?server=${serving.web}`;
    const { options } = launchConfig(args.browser, process.platform, {});
    try {
      browser = await LAUNCHERS[args.browser].launch(options);
    } catch (e) {
      const kind = launchFailureKind(e.message, process.env);
      await finish(kind === "unsupported" ? EXIT.unsupported : EXIT.app, [`could not launch headless ${args.browser}: ${e.message.split("\n")[0]}`]);
      return;
    }
    report.browser = `${args.browser} ${browser.version()}`;
    report.backend = "webgl2";
    log(`${report.browser}; loading ${url}`);
    const context = await browser.newContext({ viewport: { width: 800, height: 600 } });
    // WebGL2: the test is about the network, not the GPU.
    await context.addInitScript(() => {
      Object.defineProperty(Navigator.prototype, "gpu", { get: () => undefined, configurable: true });
    });
    const page = await context.newPage();
    page.on("console", (m) => report.console.push({ type: m.type(), text: m.text() }));
    page.on("pageerror", (e) => report.pageErrors.push({ message: String(e.stack ?? e) }));
    await withDeadline(page.goto(url), CALL_TIMEOUT_MS, "loading the page");

    // 3. Read the tab until it has seen replication work (or something breaks).
    const deadline = Date.now() + (args.runTimeout - 15) * 1000;
    let problems = ["the tab reported nothing (did the game start?)"];
    let shown = "";
    for (;;) {
      const raw = await withDeadline(
        page.evaluate(() => document.documentElement.getAttribute("data-smoke")),
        CALL_TIMEOUT_MS,
        "reading the tab",
      );
      if (raw) report.state = JSON.parse(raw);
      problems = checkReplication(report.state);
      const summary = report.state ? `joined ${report.state.joined}, ${report.state.replicas} replicas, ${report.state.moved} orbs moved` : "";
      if (summary && summary !== shown) {
        log(`tab: ${summary}`);
        shown = summary;
      }
      const panic = report.console.find((m) => m.text.includes("panicked at"));
      if (panic) throw new Error(`the tab panicked: ${panic.text.split("\n")[0]}`);
      if (report.pageErrors.length) throw new Error(`the tab: ${report.pageErrors[0].message.split("\n")[0]}`);
      if (server.exit !== null) throw new Error(`the server stopped (${server.exit.code ?? server.exit.signal}): ${server.stderr.slice(-2000)}`);
      if (problems.length === 0 || Date.now() > deadline) break;
      await sleep(200);
    }
    log(`tab: ${JSON.stringify(report.state)}`);
    await finish(problems.length ? EXIT.app : EXIT.pass, problems);
  } catch (e) {
    await finish(EXIT.app, [e.message]);
  } finally {
    stopWatchdog();
    await withDeadline(browser?.close() ?? Promise.resolve(), CALL_TIMEOUT_MS, "closing the browser").catch(() => {});
    site?.close();
    if (server.exit === null) server.child?.kill();
  }
  process.exit(outcome);
}

// Run as a program; replication.test.mjs imports the checks above.
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
