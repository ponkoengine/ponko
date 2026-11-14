#!/usr/bin/env node
// Browser multiplayer smoke test (see README.md, "Multiplayer"): the lockstep
// example with two browser tabs and two native players in one session.
//
//   node web/smoke/multiplayer.mjs --build
//   node web/smoke/multiplayer.mjs --dir target/web/lockstep --native target/debug/examples/lockstep
//
// A native host (`lockstep host --players 4 --web ...  --headless`, a bot at
// its keys) takes a native client over UDP and two headless-browser tabs of
// the web build over WebSocket. Each tab holds an arrow key. Every peer
// reports a fingerprint of the world at the same tick and how many ticks
// each player steered by then; the run passes when all four agree, every
// player's steering reached every peer and nobody saw a desync. Exit codes
// as smoke.mjs: 0 passed, 1 failed, 2 unsupported (no browser), 3 bad
// arguments or a missing build. report.json and the processes' output go to
// --out.

import { spawn, spawnSync } from "node:child_process";
import { mkdir, stat, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { browserLog } from "./browser-log.mjs";
import { CALL_TIMEOUT_MS, EXIT, UsageError, launchConfig, launchFailureKind, startWatchdog, withDeadline } from "./lib.mjs";
import { GAME_CANVAS, LAUNCHERS, serve } from "./smoke.mjs";

export const PLAYERS = 4;
export const TABS = 2;
/** Keys the tabs hold, one each (Playwright key names). */
export const TAB_KEYS = ["ArrowRight", "ArrowUp"];

export const USAGE =
  "usage: multiplayer.mjs [--build] [--dir <built lockstep site>] [--native <lockstep binary>]\n" +
  "                       [--browser chromium|firefox|webkit] [--ticks <n>] [--out <dir>] [--run-timeout <s>]";

export function parseArgs(argv) {
  const args = {
    build: false,
    dir: null,
    native: null,
    browser: "chromium",
    ticks: 720,
    out: "target/web-smoke/multiplayer",
    runTimeout: 240,
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
      case "--ticks": args.ticks = Number(value()); break;
      case "--out": args.out = value(); break;
      case "--run-timeout": args.runTimeout = Number(value()); break;
      default: throw new UsageError(`unknown argument ${argv[i]}`);
    }
  }
  if (!(args.browser in LAUNCHERS)) throw new UsageError(`unknown browser ${args.browser}`);
  // The fingerprint is taken at tick 300 (examples/lockstep.rs): the run
  // must last past it, with room for the slowest peer.
  if (!Number.isInteger(args.ticks) || args.ticks < 400) throw new UsageError("--ticks must be at least 400");
  if (!(args.runTimeout > 0)) throw new UsageError("--run-timeout must be positive");
  return args;
}

/** The host's first line: `hosting 4 players on 127.0.0.1:1234, browsers on ws://127.0.0.1:5678`. */
export function parseHosting(line) {
  const m = /hosting (\d+) players on (\S+), browsers on (ws:\/\/\S+)/.exec(line ?? "");
  return m ? { players: Number(m[1]), udp: m[2], web: m[3] } : null;
}

/**
 * What the four peers reported (each the lockstep example's state:
 * `{player, players, state, fingerprint, steered, desync, error}`), checked.
 * Returns the list of problems; empty means they played one game together.
 */
export function checkSession(peers, players = PLAYERS) {
  const problems = [];
  const names = peers.map((p) => p.name);
  for (const { name, state } of peers) {
    if (!state) {
      problems.push(`${name} reported nothing`);
      continue;
    }
    if (state.players !== players) problems.push(`${name} plays with ${state.players} players, not ${players}`);
    if (state.desync) problems.push(`${name} saw a desync: ${state.desync}`);
    if (state.error) problems.push(`${name} lost the session: ${state.error}`);
    if (!state.fingerprint) problems.push(`${name} never reached the fingerprint tick (state ${state.state}, tick ${state.ticks})`);
  }
  if (problems.length) return problems;
  const numbers = peers.map((p) => p.state.player).sort((a, b) => a - b);
  if (JSON.stringify(numbers) !== JSON.stringify([...Array(players).keys()])) {
    problems.push(`the peers are players ${numbers.join(", ")}: each should have its own number`);
  }
  const first = peers[0].state;
  for (const { name, state } of peers.slice(1)) {
    if (state.fingerprint !== first.fingerprint) {
      problems.push(`${name}'s world (${state.fingerprint}) differs from ${names[0]}'s (${first.fingerprint})`);
    }
    if (JSON.stringify(state.steered) !== JSON.stringify(first.steered)) {
      problems.push(`${name} saw steering ${JSON.stringify(state.steered)}, ${names[0]} ${JSON.stringify(first.steered)}`);
    }
  }
  // Every player, tabs and bots alike, steered: their inputs crossed to everyone.
  (first.steered ?? []).forEach((ticks, player) => {
    if (!(ticks > 0)) problems.push(`player ${player}'s input never reached the others`);
  });
  return problems;
}

const sleep = (ms) => new Promise((ok) => setTimeout(ok, ms));

/** A native peer: the process, its output lines, and how it ended. */
function startPeer(binary, args, name) {
  const child = spawn(binary, args, { stdio: ["ignore", "pipe", "pipe"] });
  const peer = { name, child, lines: [], stderr: "", exit: null };
  let partial = "";
  child.stdout.on("data", (d) => {
    const parts = (partial + d).split("\n");
    partial = parts.pop();
    peer.lines.push(...parts);
  });
  child.stderr.on("data", (d) => (peer.stderr += d));
  peer.done = new Promise((ok) =>
    child.on("exit", (code, signal) => {
      if (partial) peer.lines.push(partial);
      peer.exit = { code, signal };
      ok(peer.exit);
    }),
  );
  return peer;
}

/** The last JSON line a native peer printed: its final state. */
function lastState(peer) {
  for (const line of [...peer.lines].reverse()) {
    if (line.startsWith("{")) {
      try {
        return JSON.parse(line);
      } catch {
        return null;
      }
    }
  }
  return null;
}

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
      ["bash", ["web/build.sh", "lockstep"]],
      ["cargo", ["build", "--example", "lockstep"]],
    ];
    for (const [cmd, cmdArgs] of steps) {
      if (spawnSync(cmd, cmdArgs, { stdio: "inherit" }).status !== 0) process.exit(EXIT.usage);
    }
  }
  const target = JSON.parse(spawnSync("cargo", ["metadata", "--format-version", "1", "--no-deps"]).stdout).target_directory;
  const root = resolve(args.dir ?? join(target, "web", "lockstep"));
  const native = resolve(args.native ?? join(target, "debug", "examples", process.platform === "win32" ? "lockstep.exe" : "lockstep"));
  for (const [what, path] of [["the web build", join(root, "index.html")], ["the native example", native]]) {
    if (!(await stat(path).catch(() => null))) {
      console.error(`${what} is missing (${path}): pass --build, or build it with web/build.sh lockstep and cargo build --example lockstep`);
      process.exit(EXIT.usage);
    }
  }
  await mkdir(args.out, { recursive: true });

  const started = Date.now();
  const log = (m) => console.log(`[${((Date.now() - started) / 1000).toFixed(1)}s] ${m}`);
  // The fields summary.mjs lists (fixture, browser, backend, outcome), then this run's own.
  const report = {
    fixture: "lockstep (multiplayer)",
    browser: args.browser,
    requestedBackend: "webgl2",
    ticks: args.ticks,
    peers: {},
    console: [],
    pageErrors: [],
    problems: [],
  };
  const natives = [];
  let browser;
  let server;
  let outcome = EXIT.app;

  const finish = async (code, problems) => {
    outcome = code;
    report.problems = problems;
    const kind = code === EXIT.pass ? "pass" : code === EXIT.unsupported ? "unsupported" : "multiplayer";
    report.outcome = { status: code, kind, message: problems.join("; ") || "four peers, one world" };
    report.seconds = (Date.now() - started) / 1000;
    for (const peer of natives) {
      if (peer.exit === null) peer.child.kill();
      report.peers[peer.name] = { ...report.peers[peer.name], exit: peer.exit, stdout: peer.lines, stderr: peer.stderr };
    }
    await writeFile(join(args.out, "report.json"), JSON.stringify(report, null, 2) + "\n");
    await writeFile(join(args.out, "browser.log"), browserLog.join("\n") + "\n");
    const verdict = code === EXIT.pass ? "PASS" : code === EXIT.unsupported ? "UNSUPPORTED" : "FAIL";
    for (const p of problems) log(`problem: ${p}`);
    log(`${verdict}: browser multiplayer, ${args.browser} (report in ${args.out}/report.json)`);
  };

  const stopWatchdog = startWatchdog(args.runTimeout * 1000, async () => {
    await finish(EXIT.app, [`the run did not finish within ${args.runTimeout}s`]);
    process.exit(outcome);
  });

  try {
    // 1. The native host, then a native client over UDP.
    const ticks = String(args.ticks);
    const host = startPeer(native, ["host", "127.0.0.1:0", "--players", String(PLAYERS), "--web", "127.0.0.1:0", "--headless", ticks], "native host");
    natives.push(host);
    let hosting = null;
    for (let i = 0; i < 300 && !hosting && host.exit === null; i++) {
      hosting = parseHosting(host.lines[0]);
      await sleep(100);
    }
    if (!hosting) throw new Error(`the native host did not start: ${host.stderr.slice(-2000)}`);
    log(`native host: UDP ${hosting.udp}, browsers ${hosting.web}`);
    natives.push(startPeer(native, ["join", hosting.udp, "--headless", ticks], "native client"));

    // 2. Two tabs of the web build, joining over WebSocket.
    server = await serve(root);
    const url = `http://127.0.0.1:${server.address().port}/?server=${hosting.web}`;
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
    log(`${report.browser}; loading ${url} in ${TABS} tabs`);
    const context = await browser.newContext({ viewport: { width: 800, height: 600 } });
    // WebGL2 everywhere: the test is about the network, not the GPU.
    await context.addInitScript(() => {
      Object.defineProperty(Navigator.prototype, "gpu", { get: () => undefined, configurable: true });
    });
    const tabs = [];
    for (let i = 0; i < TABS; i++) {
      const page = await context.newPage();
      const name = `tab ${i + 1}`;
      page.on("console", (m) => report.console.push({ tab: name, type: m.type(), text: m.text() }));
      page.on("pageerror", (e) => report.pageErrors.push({ tab: name, message: String(e.stack ?? e) }));
      await withDeadline(page.goto(url), CALL_TIMEOUT_MS, `loading ${name}`);
      tabs.push({ name, page, key: TAB_KEYS[i], holding: false, state: null });
    }

    // 3. Each tab holds its key once its canvas is up, and is read until it
    //    reports the fingerprint (or something breaks).
    const deadline = Date.now() + (args.runTimeout - 30) * 1000;
    for (;;) {
      for (const tab of tabs) {
        const raw = await withDeadline(
          tab.page.evaluate(() => document.documentElement.getAttribute("data-smoke")),
          CALL_TIMEOUT_MS,
          `reading ${tab.name}`,
        );
        if (raw) tab.state = JSON.parse(raw);
        if (raw && !tab.holding) {
          await withDeadline(tab.page.click(GAME_CANVAS), CALL_TIMEOUT_MS, `clicking ${tab.name}`);
          await withDeadline(tab.page.keyboard.down(tab.key), CALL_TIMEOUT_MS, `pressing ${tab.key} in ${tab.name}`);
          tab.holding = true;
          log(`${tab.name} is up (${tab.state.state}); holding ${tab.key}`);
        }
      }
      const panic = report.console.find((m) => m.text.includes("panicked at"));
      if (panic) throw new Error(`${panic.tab} panicked: ${panic.text.split("\n")[0]}`);
      if (report.pageErrors.length) throw new Error(`${report.pageErrors[0].tab}: ${report.pageErrors[0].message.split("\n")[0]}`);
      if (tabs.every((t) => t.state?.fingerprint || t.state?.state === "Lost")) break;
      if (Date.now() > deadline) break;
      await sleep(200);
    }
    for (const tab of tabs) {
      log(`${tab.name}: ${JSON.stringify(tab.state)}`);
      report.peers[tab.name] = { state: tab.state };
    }

    // 4. The native peers finish their ticks and print their final state.
    await withDeadline(Promise.all(natives.map((p) => p.done)), Math.max(10_000, deadline - Date.now()), "waiting for the native peers to finish");
    for (const peer of natives) {
      const state = lastState(peer);
      log(`${peer.name}: ${JSON.stringify(state)} (exit ${peer.exit.code})`);
      report.peers[peer.name] = { state };
    }
    const peers = [...natives.map((p) => ({ name: p.name, state: lastState(p) })), ...tabs.map((t) => ({ name: t.name, state: t.state }))];
    const problems = checkSession(peers);
    for (const peer of natives) {
      if (peer.exit.code !== 0) problems.push(`${peer.name} exited with ${peer.exit.code ?? peer.exit.signal}`);
    }
    await finish(problems.length ? EXIT.app : EXIT.pass, problems);
  } catch (e) {
    await finish(EXIT.app, [e.message]);
  } finally {
    stopWatchdog();
    await withDeadline(browser?.close() ?? Promise.resolve(), CALL_TIMEOUT_MS, "closing the browser").catch(() => {});
    server?.close();
    for (const peer of natives) if (peer.exit === null) peer.child.kill();
  }
  process.exit(outcome);
}

// Run as a program; multiplayer.test.mjs imports the checks above.
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
