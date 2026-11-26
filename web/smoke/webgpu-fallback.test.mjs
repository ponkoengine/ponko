// The WebGPU-first runner's checks (webgpu-fallback.mjs), without a browser.
import assert from "node:assert/strict";
import { test } from "node:test";
import { EXIT, UsageError } from "./lib.mjs";
import { FALLBACK_CASES, FALLBACK_LINE, REFUSAL, WEBGPU_LINE, checkCase, expectedReason, median, needsAdapter, parseArgs, runOutcome } from "./webgpu-fallback.mjs";

/** A page that fell back to WebGL2 the way `fallbackCase` should. */
function fellBack(fallbackCase, change = {}) {
  return {
    ticks: 30,
    status: null,
    canvas: "webgl2",
    engineBackend: "gl",
    console: [`warning: ${FALLBACK_LINE}${expectedReason(fallbackCase)}`],
    pageErrors: [],
    pixels: { colors: 40, foreground: 0.3 },
    ...change,
  };
}

const onWebgpu = { ticks: 30, status: null, canvas: "webgpu", engineBackend: "webgpu", console: [`info: ${WEBGPU_LINE}`], pageErrors: [], pixels: { colors: 40, foreground: 0.3 } };

test("every failure case passes when it ends on WebGL2 with its one line", () => {
  for (const fallbackCase of FALLBACK_CASES.filter((c) => c !== "webgpu")) {
    assert.deepEqual(checkCase(fallbackCase, fellBack(fallbackCase)), [], fallbackCase);
  }
  assert.deepEqual(checkCase("webgpu", onWebgpu), []);
});

test("a blank page, a startup error or no ticks fail a fallback", () => {
  assert.match(checkCase("no-webgpu", fellBack("no-webgpu", { pixels: { colors: 1, foreground: 0 } })).join(), /blank/);
  assert.match(checkCase("no-webgpu", fellBack("no-webgpu", { pixels: null })).join(), /blank/);
  const stopped = checkCase("device-lost", fellBack("device-lost", { ticks: 0, status: "Could not start the game: ..." })).join();
  assert.match(stopped, /never ticked/);
  assert.match(stopped, /the page shows/);
});

test("a fallback must say why, once", () => {
  assert.match(checkCase("device-fails", fellBack("device-fails", { console: [] })).join(), /wanted one/);
  const twice = fellBack("no-adapter");
  twice.console = [...twice.console, ...twice.console];
  assert.match(checkCase("no-adapter", twice).join(), /got 2/);
  // The line for another failure doesn't count.
  assert.match(checkCase("device-lost", fellBack("no-adapter")).join(), /should say/);
  assert.ok(expectedReason("device-fails").includes(REFUSAL));
  // A canvas that never presents names the engine's test frame.
  assert.match(expectedReason("no-present"), /test frame failed/);
  assert.match(checkCase("no-present", fellBack("device-lost")).join(), /should say/);
});

test("WebGPU failing silently or the engine disagreeing with the canvas fails", () => {
  assert.match(checkCase("no-webgpu", fellBack("no-webgpu", { canvas: "webgpu" })).join(), /wanted webgl2/);
  assert.match(checkCase("no-webgpu", fellBack("no-webgpu", { engineBackend: "webgpu" })).join(), /wanted gl/);
  assert.match(checkCase("webgpu", { ...onWebgpu, console: [`warning: ${FALLBACK_LINE}x`] }).join(), /yet the console says/);
  assert.match(checkCase("webgpu", { ...onWebgpu, canvas: "webgl2", engineBackend: "gl" }).join(), /wanted webgpu/);
  assert.match(checkCase("no-webgpu", fellBack("no-webgpu", { console: ["error: panicked at src/x.rs:1:1", `warning: ${FALLBACK_LINE}navigator.gpu is missing`] })).join(), /panic/);
});

test("only the cases that fake on a real adapter need one", () => {
  assert.deepEqual(FALLBACK_CASES.filter(needsAdapter), ["webgpu", "device-fails", "device-lost", "no-present"]);
});

test("arguments and outcomes", () => {
  assert.deepEqual(parseArgs(["--case", "no-adapter", "--repeat", "5"]).cases, ["no-adapter"]);
  assert.equal(parseArgs(["--repeat", "5"]).repeat, 5);
  assert.throws(() => parseArgs(["--case", "nope"]), UsageError);
  assert.throws(() => parseArgs(["--repeat", "0"]), UsageError);
  assert.equal(median([300, null, 100, 200]), 200);
  assert.equal(median([4, 1, 3, 2]), 2);
  assert.equal(median([]), null);
  const runs = { a: {}, b: {} };
  assert.deepEqual(runOutcome({ problems: ["x"], runs, unsupported: [] }, {}), [EXIT.app, "failed"]);
  assert.deepEqual(runOutcome({ problems: [], runs, unsupported: ["a", "b"] }, {}), [EXIT.unsupported, "unsupported"]);
  assert.deepEqual(runOutcome({ problems: [], runs, unsupported: ["a", "b"] }, { allowUnsupported: true }), [EXIT.pass, "unsupported"]);
  assert.deepEqual(runOutcome({ problems: [], runs, unsupported: ["a"] }, {}), [EXIT.pass, "passed, some unsupported"]);
});
