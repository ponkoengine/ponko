#!/usr/bin/env node
// Key labels in a browser (see README.md, "Key labels"): the 2D fixture
// (web/build.sh web_smoke) reports WASD as labelled on the player's
// keyboard (`key_labels` in its state). The runner gives the page a French
// layout three ways and checks what the game shows:
//
//   node web/smoke/key-labels.mjs --build
//   node web/smoke/key-labels.mjs --dir target/web/web_smoke --browser firefox
//
// - layout-map: `navigator.keyboard.getLayoutMap()` answers with a French
//   (AZERTY) map: the labels follow it without a key pressed ("Z/Q/S/D").
// - no-layout-map: the browser has no `navigator.keyboard` (Firefox,
//   Safari, a page outside a secure context): US labels ("W/A/S/D") until a
//   key is pressed; then (Chromium, which can send a key whose character
//   differs from its place) the key at W's place typing "z" relabels it
//   ("Z/A/S/D").
// - refused: `getLayoutMap()` rejects (a page in a frame that may not ask):
//   US labels, and the game keeps running.
//
// Exit codes as smoke.mjs: 0 passed, 1 failed, 2 unsupported (no browser),
// 3 bad arguments or a missing build. report.json goes to --out.

import { spawnSync } from "node:child_process";
import { mkdir, stat, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { CALL_TIMEOUT_MS, EXIT, UsageError, launchConfig, launchFailureKind, startWatchdog, withDeadline } from "./lib.mjs";
import { LAUNCHERS, serve } from "./smoke.mjs";

const USAGE =
  "usage: key-labels.mjs [--dir <site>] [--build] [--browser chromium|firefox|webkit]\n" +
  "                      [--case layout-map|no-layout-map|refused] [--out <dir>] [--timeout <s>]";

export const KEY_LABEL_CASES = ["layout-map", "no-layout-map", "refused"];

/** A French keyboard's unshifted characters by DOM `code`, as Chromium's layout map holds them. */
export const AZERTY = { KeyQ: "a", KeyW: "z", KeyA: "q", KeyZ: "w", KeyS: "s", KeyD: "d", Semicolon: "m", Digit2: "é" };

/**
 * What the fixture's `key_labels` must read in a case: `first` once the
 * page runs, and `afterPress` after the key at W's place typed "z" (null
 * where the runner presses nothing).
 */
export function expectedLabels(keyCase, canPress) {
  switch (keyCase) {
    case "layout-map": return { first: "Z/Q/S/D", afterPress: null };
    case "no-layout-map": return { first: "W/A/S/D", afterPress: canPress ? "Z/A/S/D" : null };
    case "refused": return { first: "W/A/S/D", afterPress: null };
    default: throw new UsageError(`unknown case ${keyCase} (${KEY_LABEL_CASES.join(", ")})`);
  }
}

/** The page's `navigator.keyboard` for a case, installed before the game starts. */
function initScript(keyCase) {
  // Runs in the page: no closures over the runner's variables.
  return [
    ({ keyCase, map }) => {
      const get = () => {
        if (keyCase === "no-layout-map") return undefined;
        return {
          getLayoutMap: () => keyCase === "refused"
            ? Promise.reject(new DOMException("not allowed here", "SecurityError"))
            : Promise.resolve(new Map(Object.entries(map))),
        };
      };
      Object.defineProperty(Navigator.prototype, "keyboard", { get, configurable: true });
      // WebGL2 only: Chromium's headless shell can't present a WebGPU canvas.
      Object.defineProperty(Navigator.prototype, "gpu", { get: () => undefined, configurable: true });
    },
    { keyCase, map: AZERTY },
  ];
}

function parseArgs(argv) {
  const args = {
    dir: "target/web/web_smoke",
    build: false,
    browser: "chromium",
    cases: KEY_LABEL_CASES,
    out: "target/web-smoke/key-labels",
    timeout: 60,
    chromiumMode: "shell",
    browserArgs: [],
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
        expectedLabels(one, false);
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

/** The fixture's state (null before its first frame). */
async function state(page) {
  return call(page.evaluate(() => {
    const raw = document.documentElement.getAttribute("data-smoke");
    return raw ? JSON.parse(raw) : null;
  }), "reading the state");
}

/** Waits until `key_labels` reads `want` (after the game ran a few ticks); the last state seen. */
async function waitFor(page, want, deadline) {
  let seen = await state(page);
  while (Date.now() < deadline && !(seen?.ticks > 10 && seen?.key_labels === want)) {
    await sleep(200);
    seen = await state(page);
  }
  return seen;
}

/** Presses the key at W's place with the character "z", as on a French keyboard (Chromium only). */
async function pressZAtW(page, context) {
  const session = await context.newCDPSession(page);
  const key = { code: "KeyW", key: "z", text: "z", unmodifiedText: "z", windowsVirtualKeyCode: 90, nativeVirtualKeyCode: 90 };
  await session.send("Input.dispatchKeyEvent", { type: "keyDown", ...key });
  await session.send("Input.dispatchKeyEvent", { type: "keyUp", ...key });
}

async function runCase(browser, server, keyCase, args) {
  const context = await browser.newContext();
  const lines = [];
  try {
    await context.addInitScript(...initScript(keyCase));
    const page = await context.newPage();
    page.on("console", (m) => lines.push(`${m.type()}: ${m.text()}`));
    page.on("pageerror", (e) => lines.push(`pageerror: ${e.message}`));
    await call(page.goto(`http://127.0.0.1:${server.address().port}/`), "opening the page");
    const want = expectedLabels(keyCase, args.browser === "chromium");
    const deadline = Date.now() + args.timeout * 1000;
    const problems = [];
    let seen = await waitFor(page, want.first, deadline);
    const first = seen?.key_labels ?? null;
    if (!(seen?.ticks > 10)) problems.push(`the game never ran (ticks ${seen?.ticks ?? 0})`);
    if (first !== want.first) problems.push(`key_labels is ${JSON.stringify(first)}, wanted ${JSON.stringify(want.first)}`);
    let afterPress = null;
    if (want.afterPress && !problems.length) {
      // The canvas takes the keys once clicked.
      const box = await call(page.locator("canvas").first().boundingBox(), "finding the canvas");
      await call(page.mouse.click(box.x + box.width / 2, box.y + box.height / 2), "clicking the canvas");
      await call(pressZAtW(page, context), "pressing a key");
      seen = await waitFor(page, want.afterPress, Date.now() + 10_000);
      afterPress = seen?.key_labels ?? null;
      if (afterPress !== want.afterPress) {
        problems.push(`after the key at W's place typed "z", key_labels is ${JSON.stringify(afterPress)}, wanted ${JSON.stringify(want.afterPress)}`);
      }
    }
    if (lines.some((l) => l.startsWith("pageerror"))) problems.push("the page threw: see lines");
    return { first, afterPress, ticks: seen?.ticks ?? 0, problems, lines };
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
    console.error(`FAIL: the browser didn't start: ${e.message.split("\n")[0]}`);
    stopWatchdog();
    server.close();
    await finish(kind === "unsupported" ? EXIT.unsupported : EXIT.app, kind);
  }
  try {
    for (const keyCase of args.cases) {
      const run = await runCase(browser, server, keyCase, args);
      report.runs[keyCase] = run;
      report.problems.push(...run.problems.map((p) => `${keyCase}: ${p}`));
      const pressed = run.afterPress ? `, after a press ${JSON.stringify(run.afterPress)}` : "";
      console.log(`${keyCase}: ${run.problems.length ? "FAIL" : "ok"}; key_labels ${JSON.stringify(run.first)}${pressed}`);
    }
  } catch (e) {
    report.problems.push(e.message);
  } finally {
    await call(browser.close(), "closing the browser").catch(() => {});
    server.close();
    stopWatchdog();
  }
  for (const p of report.problems) console.error(`FAIL: ${p}`);
  if (report.problems.length) return finish(EXIT.app, "failed");
  console.log("PASS");
  await finish(EXIT.pass, "passed");
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
