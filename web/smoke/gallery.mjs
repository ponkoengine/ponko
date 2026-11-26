#!/usr/bin/env node
// Checks a built web gallery (web/gallery/README.md) in a headless browser:
//
//   node web/gallery/build.mjs
//   node web/smoke/gallery.mjs --backend webgl2
//   node web/smoke/gallery.mjs --only battle,3d --browser webkit
//
// The index must link every example. Each example's page must then start its
// game and keep drawing (draw calls or WebGPU submissions go on), with no
// panic, page error, WebGL error or failed asset request; survive a click; and show a
// canvas that isn't blank. A demo that needs WebGPU, on a page without it
// (`--backend webgl2` hides it), must instead say it needs WebGPU and not
// start. Unlike smoke.mjs's fixtures, examples publish no state, so these
// checks are what any page can be held to.
//
// Exit codes: 0 every page passed, 1 a page failed, 2 the browser can't run
// the backend asked for (or isn't installed), 3 bad arguments or no build.
// Each page leaves report.json (the format summary.mjs reads), canvas.png and
// console.log in <out>/<example>/.

import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { EXIT, UsageError, launchConfig, launchFailureKind, summaryMarkdown, summaryRow, unsupportedReason } from "./lib.mjs";
import {
  GALLERY_USAGE,
  MIN_PROGRESS_DRAWS,
  PROGRESS_SECONDS,
  checkWebgpuMessage,
  expectedBackend,
  expectedPage,
  parseGalleryArgs,
  selectExamples,
  webglErrorOf,
} from "./gallery-lib.mjs";
import { GAME_CANVAS, LAUNCHERS, canvasBackend, canvasStats, probe, serve } from "./smoke.mjs";

const sleep = (ms) => new Promise((ok) => setTimeout(ok, ms));

class Failure extends Error {
  constructor(kind, message) {
    super(message);
    this.kind = kind;
  }
}

function usage(message) {
  console.error(`gallery: ${message}`);
  console.error(GALLERY_USAGE);
  process.exit(EXIT.usage);
}

/** Counts the page's draw calls and WebGPU submissions; installed before its scripts run. */
function countDraws() {
  const seen = { draws: 0 };
  Object.defineProperty(window, "__galleryDraws", { value: seen });
  const wrap = (proto, names) => {
    if (!proto) return;
    for (const name of names) {
      const original = proto[name];
      if (typeof original !== "function") continue;
      proto[name] = function (...args) {
        seen.draws++;
        return original.apply(this, args);
      };
    }
  };
  wrap(window.WebGL2RenderingContext?.prototype, [
    "clear", "drawArrays", "drawElements", "drawArraysInstanced", "drawElementsInstanced", "drawRangeElements",
  ]);
  wrap(window.GPUQueue?.prototype, ["submit"]);
  // A headless browser refuses pointer lock (the first-person demos ask for
  // it on a click), rejecting with an empty error; a real one grants it.
  Element.prototype.requestPointerLock = function () {
    return Promise.resolve();
  };
}

async function pageState(page) {
  return page.evaluate(() => ({
    gallery: document.documentElement.dataset.gallery ?? null,
    backend: document.documentElement.dataset.backend ?? null,
    status: document.getElementById("status")?.textContent ?? null,
    note: document.getElementById("backend-note")?.hidden === false ? document.getElementById("backend-note").textContent : null,
    draws: window.__galleryDraws?.draws ?? 0,
  }));
}

/** The index page links every example in the gallery. */
async function checkIndex(browser, url, examples) {
  const context = await browser.newContext();
  try {
    const page = await context.newPage();
    const response = await page.goto(url);
    if (!response?.ok()) return `the index answered ${response?.status()}`;
    const links = await page.$$eval(".cards a", (as) => as.map((a) => a.getAttribute("href")));
    const missing = examples.filter((e) => !links.includes(`${e.example}/`)).map((e) => e.example);
    return missing.length ? `the index doesn't link ${missing.join(", ")}` : null;
  } finally {
    await context.close();
  }
}

/** Opens one example's page and checks it; fills `report` and returns its outcome. */
async function checkExample(browser, url, entry, args, caps, report) {
  const context = await browser.newContext({ viewport: { width: 1000, height: 800 } });
  try {
    if (args.backend === "webgl2") {
      await context.addInitScript(() => {
        Object.defineProperty(Navigator.prototype, "gpu", { get: () => undefined, configurable: true });
      });
    }
    await context.addInitScript(countDraws);
    const page = await context.newPage();
    const started = Date.now();
    const elapsed = () => (Date.now() - started) / 1000;
    page.on("console", (m) => report.console.push({ type: m.type(), text: m.text(), at: elapsed() }));
    page.on("pageerror", (e) => report.pageErrors.push({ message: String(e.stack ?? e), at: elapsed() }));
    page.on("response", (r) => report.requests.push({ path: new URL(r.url()).pathname, status: r.status() }));
    page.on("requestfailed", (r) =>
      report.requests.push({ path: new URL(r.url()).pathname, status: "failed", error: r.failure()?.errorText }),
    );
    const appFailure = (state) => {
      const bad = report.requests.find((r) => r.path.startsWith(`/${entry.example}/`) && r.status !== 200);
      if (bad) return new Failure("asset", `${bad.path} answered ${bad.status}`);
      const panic = report.console.find((m) => m.text.includes("panicked at"));
      if (panic) return new Failure("panic", panic.text.split("\n").slice(0, 3).join(" | "));
      if (report.pageErrors.length) return new Failure("page-error", report.pageErrors[0].message.split("\n")[0]);
      const glError = report.console.map((m) => webglErrorOf(m.text)).find(Boolean);
      if (glError) return new Failure("webgl-error", `the browser refused a WebGL call: ${glError}`);
      if (state?.gallery === "error") return new Failure("page-error", state.status);
      return null;
    };

    await page.goto(`${url}${entry.example}/`);
    const expected = expectedPage(entry, args.backend, caps);
    report.expected = expected;
    const deadline = Date.now() + args.timeout * 1000;
    let state;
    for (;;) {
      state = await pageState(page);
      const failure = appFailure(state);
      if (failure) throw failure;
      // Started: the page says so and the game has drawn (only then does the
      // canvas have its context; asking for one earlier would take it).
      if ((state.gallery === "running" && state.draws > 0) || state.gallery === "unsupported") break;
      if (Date.now() > deadline) throw new Failure("startup-timeout", `the page never started within ${args.timeout}s (${state.gallery})`);
      await sleep(100);
    }
    report.startupSeconds = elapsed();

    // A demo that needs WebGPU, without it: the page explains, and nothing starts.
    if (expected === "webgpu-message") {
      if (state.gallery !== "unsupported") throw new Failure("fallback", `the demo needs WebGPU but the page started it on ${state.backend}`);
      const wrong = checkWebgpuMessage(state.status);
      if (wrong) throw new Failure("fallback", wrong);
      if (report.requests.some((r) => r.path.endsWith("/game_bg.wasm"))) throw new Failure("fallback", "the page showed its message but still loaded the game");
      report.message = state.status;
      return { status: EXIT.pass, kind: "webgpu-message", message: `showed its WebGPU message: ${state.status}` };
    }
    if (state.gallery !== "running") throw new Failure("unsupported-page", `the page didn't start the demo: ${state.status}`);
    report.backend = await canvasBackend(page);
    report.backendNote = state.note;
    const wanted = expectedBackend(args.backend, caps);
    if (report.backend !== "unknown" && report.backend !== wanted) {
      throw new Failure("backend", `the canvas uses ${report.backend}, but the page should have picked ${wanted}`);
    }

    // Progress: the game keeps drawing. A heavy scene compiles pipelines
    // in its first frames, so this waits for the draws rather than timing them.
    const before = state.draws;
    const progressDeadline = Date.now() + PROGRESS_SECONDS * 1000;
    let failure;
    for (;;) {
      await sleep(250);
      state = await pageState(page);
      failure = appFailure(state);
      if (failure) throw failure;
      if (state.draws - before >= MIN_PROGRESS_DRAWS) break;
      if (Date.now() > progressDeadline) {
        throw new Failure("stall", `the page stopped drawing: ${before} -> ${state.draws} draws in ${PROGRESS_SECONDS}s`);
      }
    }
    report.progress = { before, after: state.draws };

    // A click on the game doesn't break it.
    const box = await page.locator(GAME_CANVAS).boundingBox();
    await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
    await sleep(1000);
    state = await pageState(page);
    failure = appFailure(state);
    if (failure) throw failure;

    // Rendering: the canvas isn't blank.
    // The page's pixels in the canvas's box: an element screenshot waits for
    // the element to be "stable", which a heavy demo on software GL can
    // keep it from being for longer than the timeout.
    const png = await page.screenshot({ clip: box, timeout: 60_000 });
    await writeFile(join(report.dir, "canvas.png"), png);
    const blank = (stats) => stats.colors < 3 || stats.foreground < 0.001;
    report.canvas = { screenshot: await canvasStats(page, png) };
    if (blank(report.canvas.screenshot)) report.canvas.readback = await canvasStats(page, null);
    if (Object.values(report.canvas).every(blank)) {
      if (report.backend === "webgpu" && caps.webgpu?.canvas !== true) {
        throw new Failure("unsupported", `blank WebGPU canvas, but a plain WebGPU canvas fails here too: ${caps.webgpu?.canvasProblem ?? "no adapter"}`);
      }
      throw new Failure("blank", `the canvas looks blank: ${JSON.stringify(report.canvas)}`);
    }
    return { status: EXIT.pass, kind: "pass", message: `ran on ${report.backend} (${state.draws} draws)` };
  } catch (e) {
    const screenshot = context.pages()[0];
    await screenshot?.screenshot({ path: join(report.dir, "page.png") }).catch(() => {});
    if (e instanceof Failure) {
      return { status: e.kind === "unsupported" ? (args.allowUnsupported ? EXIT.pass : EXIT.unsupported) : EXIT.app, kind: e.kind, message: e.message };
    }
    return { status: EXIT.app, kind: "harness", message: String(e.stack ?? e) };
  } finally {
    await context.close().catch(() => {});
  }
}

async function main() {
  let args;
  let examples;
  let gallery;
  try {
    args = parseGalleryArgs(process.argv.slice(2));
    try {
      gallery = JSON.parse(await readFile(join(args.dir, "gallery.json"), "utf8"));
    } catch {
      usage(`${resolve(args.dir)}/gallery.json not found: build the gallery first (node web/gallery/build.mjs)`);
    }
    examples = selectExamples(gallery, args.only);
  } catch (e) {
    if (e instanceof UsageError) usage(e.message);
    throw e;
  }
  await mkdir(args.out, { recursive: true });
  const server = await serve(resolve(args.dir));
  const url = `http://127.0.0.1:${server.address().port}/`;
  const rows = [];
  let worst = EXIT.pass;
  let browser;
  try {
    try {
      browser = await LAUNCHERS[args.browser].launch(launchConfig(args.browser, process.platform, args).options);
    } catch (e) {
      const kind = launchFailureKind(e.message, process.env);
      console.log(`${kind === "unsupported" ? "UNSUPPORTED" : "FAIL"}: could not launch headless ${args.browser}: ${e.message.split("\n")[0]}`);
      process.exitCode = kind === "unsupported" && !args.allowUnsupported ? EXIT.unsupported : kind === "unsupported" ? EXIT.pass : EXIT.app;
      return;
    }
    const name = `${args.browser} ${browser.version()}`;
    const probeContext = await browser.newContext();
    const caps = await probe(probeContext, url, args.backend);
    await probeContext.close();
    console.log(`${name}, ${process.platform}-${process.arch}; WebGPU adapter: ${JSON.stringify(caps.webgpu)}; WebGL2: ${JSON.stringify(caps.webgl2)}`);
    const reason = unsupportedReason(caps, args.backend, args.browser);
    if (reason) {
      console.log(`UNSUPPORTED: ${reason}`);
      process.exitCode = args.allowUnsupported ? EXIT.pass : EXIT.unsupported;
      return;
    }
    const indexProblem = await checkIndex(browser, url, examples);
    console.log(indexProblem ? `FAIL index: ${indexProblem}` : `pass index: links all ${examples.length} example(s)`);
    if (indexProblem) worst = EXIT.app;

    for (const entry of examples) {
      const dir = join(args.out, entry.example);
      await mkdir(dir, { recursive: true });
      for (const file of ["report.json", "console.log", "canvas.png", "page.png"]) await rm(join(dir, file), { force: true });
      const started = Date.now();
      const report = {
        fixture: `gallery/${entry.example}`,
        case: "ok",
        browser: name,
        requestedBrowser: args.browser,
        requestedBackend: args.backend,
        platform: `${process.platform}-${process.arch}`,
        ref: gallery.ref,
        webgpu: entry.webgpu,
        capabilities: caps,
        console: [],
        pageErrors: [],
        requests: [],
        dir,
      };
      report.outcome = await checkExample(browser, url, entry, args, caps, report);
      report.elapsedSeconds = (Date.now() - started) / 1000;
      delete report.dir;
      await writeFile(join(dir, "report.json"), JSON.stringify(report, null, 2) + "\n");
      await writeFile(join(dir, "console.log"), report.console.map((m) => `[${m.type}] ${m.text}`).join("\n") + "\n");
      const { status, kind, message } = report.outcome;
      const label = kind === "unsupported" ? "UNSUPPORTED" : status === EXIT.pass ? "PASS" : "FAIL";
      console.log(`${label} ${entry.example} (${kind}): ${message.replace(/\s*\n\s*/g, " ")} [${report.elapsedSeconds.toFixed(1)}s]`);
      if (status !== EXIT.pass && (worst === EXIT.pass || status === EXIT.app)) worst = status;
      rows.push(summaryRow(report));
    }
    console.log(`\n${summaryMarkdown(rows)}`);
    process.exitCode = worst;
  } finally {
    await browser?.close().catch(() => {});
    server.close();
  }
}

main();
