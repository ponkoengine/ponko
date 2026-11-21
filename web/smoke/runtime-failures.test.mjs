// The shipped page shows failures that stop a running game (a panic, a
// lost WebGL2 context or WebGPU device, a stopped worker thread), on
// machines without a browser: the template's script runs against a fake page and a fake
// game.js whose callbacks the tests call as the engine would.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { runInNewContext } from "node:vm";

const template = readFileSync(new URL("../index.html", import.meta.url), "utf8");
const script = template.split('<script type="module">')[1].split("</script>")[0];

/**
 * Runs the template. `failure` is what the engine already reported before
 * the page subscribed (a panic before the event loop started).
 */
async function startPage({ init = async () => {}, failure = null } = {}) {
  const body = { appended: [], append(node) { this.appended.push(node); } };
  const status = { textContent: "Loading…", className: "", remove() { this.removed = true; } };
  const canvas = { focus() { this.focused = true; } };
  const listeners = {};
  let startup;
  let failed;
  await runInNewContext(`(async () => {${script.replace(/^\s*import .*;$/m, "")}})()`, {
    init,
    game: {
      onPonkoStartup: (handler) => { startup = handler; },
      onPonkoFailure: (handler) => {
        failed = handler;
        if (failure) handler(failure);
      },
    },
    document: { body, getElementById: (id) => ({ status, ponko: canvas })[id] },
    window: { addEventListener: (name, handler) => { listeners[name] = handler; } },
    console: { error() {} },
  });
  return {
    status,
    body,
    started: (error = null) => startup(error),
    fail: (failure) => failed(failure),
    windowError: (message) => listeners.error({ message }),
  };
}

const panic = { kind: "Panic", message: "index out of bounds: the len is 3 but the index is 7", location: "src/main.rs:12:5", text: null };

test("a panic after startup brings the status back with the panic message", async () => {
  const page = await startPage();
  page.started();
  assert.equal(page.status.removed, true);
  page.fail(panic);
  assert.deepEqual(page.body.appended, [page.status]);
  assert.equal(page.status.className, "error");
  assert.equal(page.status.textContent, `The game stopped: ${panic.message}`);
});

test("the game's own failure text replaces the page's", async () => {
  const page = await startPage();
  page.started();
  page.fail({ ...panic, text: "Sorry, the game crashed. Reload the page to play again." });
  assert.equal(page.status.textContent, "Sorry, the game crashed. Reload the page to play again.");
});

test("the trap that follows a panic does not hide the panic's message", async () => {
  const page = await startPage();
  page.started();
  page.fail(panic);
  page.windowError("RuntimeError: unreachable");
  assert.equal(page.status.textContent, `The game stopped: ${panic.message}`);
  assert.equal(page.body.appended.length, 1);
});

test("a WebGL2 context lost while playing is shown", async () => {
  const page = await startPage();
  page.started();
  const message = "The WebGL2 context was lost while the game was running. Reload the page to try again.";
  page.fail({ kind: "ContextLost", message, location: null, text: null });
  assert.equal(page.status.textContent, `The game stopped: ${message}`);
});

test("a WebGPU device lost while playing is shown with the browser's reason", async () => {
  const page = await startPage();
  page.started();
  const message = "The WebGPU device was lost while the game was running: Device was destroyed. Reload the page to try again.";
  page.fail({ kind: "DeviceLost", message, location: null, text: null });
  assert.deepEqual(page.body.appended, [page.status]);
  assert.equal(page.status.className, "error");
  assert.equal(page.status.textContent, `The game stopped: ${message}`);
});

test("a game's own text replaces the page's for a lost WebGPU device too", async () => {
  const page = await startPage();
  page.started();
  const message = "The WebGPU device was lost while the game was running. Reload the page to try again.";
  page.fail({ kind: "DeviceLost", message, location: null, text: "The graphics card stopped answering. Reload to keep playing." });
  assert.equal(page.status.textContent, "The graphics card stopped answering. Reload to keep playing.");
});

test("a worker thread that stopped is shown with its panic", async () => {
  const page = await startPage();
  page.started();
  const message = "the worker thread ponko-frame-0 stopped: RuntimeError: unreachable (it panicked: a parallel query failed)";
  page.fail({ kind: "ThreadStopped", message, location: null, text: null });
  assert.equal(page.status.textContent, `The game stopped: ${message}`);
});

test("a script error the engine did not report is still shown after startup", async () => {
  const page = await startPage();
  page.started();
  page.windowError("TypeError: x is undefined");
  assert.deepEqual(page.body.appended, [page.status]);
  assert.equal(page.status.textContent, "The game stopped: TypeError: x is undefined (see the browser console)");
});

test("a panic during setup replaces the loading message in place", async () => {
  const page = await startPage();
  page.fail({ ...panic, message: "the level file is empty" });
  assert.deepEqual(page.body.appended, [], "the status line never left the page");
  assert.equal(page.status.textContent, "The game stopped: the level file is empty");
});

test("a panic before the event loop started is shown instead of the trap it caused", async () => {
  const page = await startPage({
    init: async () => { throw new Error("RuntimeError: unreachable"); },
    failure: { ...panic, message: "the game's main panicked" },
  });
  assert.equal(page.status.className, "error");
  assert.equal(page.status.textContent, "The game stopped: the game's main panicked");
});

test("a startup error stays on the page when a script error follows it", async () => {
  const page = await startPage();
  page.started({ kind: "Unsupported", message: "No usable WebGPU or WebGL2 context" });
  page.windowError("RuntimeError: unreachable");
  assert.equal(page.status.textContent, "Could not start the game: No usable WebGPU or WebGL2 context");
});

// The browser runner's own checks (runtime-failures.mjs), on recorded runs.
const { checkReport, checkRun, deviceLostMessage, expected, runOutcome } = await import("./runtime-failures.mjs");

/** The crash report the engine hands the page for a run that wanted `want`. */
const reportFor = (want) => ({
  name: "crash-reports/crash-report.txt",
  kind: want.report,
  message: want.message,
  text: `Ponko: web smoke crash report\nkind: ${want.report}\nmessage: ${want.message}\nengine version: 0.1.0\n`,
});

test("the browser runner passes a run that shows the panic it reported", () => {
  const want = expected("runtime-panic");
  const failure = { kind: "Panic", message: want.message, location: "examples/common/web_smoke.rs:80:13", text: null };
  assert.deepEqual(checkRun("runtime-panic", { ticks: 21, status: want.status, failure, report: reportFor(want) }), []);
});

test("the browser runner fails a run whose page shows the trap instead of the panic", () => {
  const want = expected("runtime-panic");
  const failure = { kind: "Panic", message: want.message, location: "examples/common/web_smoke.rs:80:13", text: null };
  const problems = checkRun("runtime-panic", { ticks: 21, status: "The game stopped: RuntimeError: unreachable (see the browser console)", failure, report: reportFor(want) });
  assert.equal(problems.length, 1);
  assert.match(problems[0], /the page shows/);
});

test("the browser runner fails a run without the game's own failure text", () => {
  const want = expected("failure-text");
  const failure = { kind: "Panic", message: want.message, location: "examples/common/web_smoke.rs:80:13", text: null };
  const problems = checkRun("failure-text", { ticks: 21, status: `The game stopped: ${want.message}`, failure, report: reportFor(want) });
  assert.equal(problems.length, 2);
});

test("the browser runner fails a run where no failure reached onPonkoFailure", () => {
  const want = expected("context-lost");
  assert.deepEqual(checkRun("context-lost", { ticks: 30, status: want.status, failure: null }), ["onPonkoFailure never called its callback"]);
});

// A lost WebGPU device: the engine's message carries the browser's reason.
const destroyed = { reason: "destroyed", message: "Device was destroyed.", by: "runner" };
const lostFailure = (message) => ({ kind: "DeviceLost", message, location: null, text: null });

test("the engine's lost-device sentence carries the browser's reason without a doubled stop", () => {
  assert.equal(
    deviceLostMessage("Device was destroyed."),
    "The WebGPU device was lost while the game was running: Device was destroyed. Reload the page to try again.",
  );
  assert.equal(
    deviceLostMessage(""),
    "The WebGPU device was lost while the game was running. Reload the page to try again.",
  );
  assert.equal(deviceLostMessage("  \n"), deviceLostMessage(""));
});

test("the browser runner passes a run where the lost device reached the page with its reason", () => {
  const want = expected("device-lost", destroyed);
  assert.equal(want.kind, "DeviceLost");
  const seen = { ticks: 40, status: want.status, failure: lostFailure(want.message), lost: destroyed, report: reportFor(want) };
  assert.deepEqual(checkRun("device-lost", seen), []);
});

test("the browser runner fails a lost device that wrote no crash report", () => {
  const want = expected("device-lost", destroyed);
  const seen = { ticks: 40, status: want.status, failure: lostFailure(want.message), lost: destroyed, report: null };
  assert.deepEqual(checkRun("device-lost", seen), ["no crash report reached the page (window.ponkoCrashReport)"]);
});

test("a lost device's crash report is an error with the page's message", () => {
  const want = expected("device-lost", destroyed);
  assert.equal(want.report, "error");
  assert.deepEqual(checkReport(want, reportFor(want)), []);
  const asPanic = { ...reportFor(want), kind: "panic" };
  assert.ok(checkReport(want, asPanic).some((p) => p.startsWith("the crash report's kind")));
  const other = reportFor(expected("context-lost"));
  const problems = checkReport(want, other);
  assert.ok(problems.some((p) => p.startsWith("the crash report's message")), problems.join("; "));
  assert.ok(problems.some((p) => p.startsWith("the crash report's text lacks")), problems.join("; "));
});

test("the browser runner fails a lost device reported as a lost WebGL2 context", () => {
  const message = expected("context-lost").message;
  const seen = { ticks: 40, status: `The game stopped: ${message}`, failure: { ...lostFailure(message), kind: "ContextLost" }, lost: destroyed };
  const problems = checkRun("device-lost", seen);
  assert.ok(problems.some((p) => p.startsWith("the failure's kind is \"ContextLost\"")), problems.join("; "));
  assert.ok(problems.some((p) => p.startsWith("the page shows")), problems.join("; "));
});

test("the browser runner fails a lost device whose message drops the browser's reason", () => {
  const generic = deviceLostMessage("");
  const seen = { ticks: 40, status: `The game stopped: ${generic}`, failure: lostFailure(generic), lost: destroyed };
  const problems = checkRun("device-lost", seen);
  assert.ok(problems.some((p) => p.startsWith("the failure's message")), problems.join("; "));
});

test("the browser runner fails a device-lost run where the page never heard of it", () => {
  const seen = { ticks: 40, status: null, failure: null, lost: destroyed };
  const problems = checkRun("device-lost", seen);
  assert.ok(problems.includes("onPonkoFailure never called its callback"));
});

test("the browser runner fails a device-lost run where the device was never lost", () => {
  const want = expected("device-lost");
  const problems = checkRun("device-lost", { ticks: 40, status: want.status, failure: lostFailure(want.message), lost: null, report: reportFor(want) });
  assert.deepEqual(problems, ["the browser never resolved the game's device.lost"]);
});

test("a runner run without WebGPU is unsupported only when no case could run", () => {
  const report = (runs, unsupported, problems = []) => ({ runs: Object.fromEntries(runs.map((r) => [r, {}])), unsupported, problems });
  assert.deepEqual(runOutcome(report(["device-lost"], ["device-lost"]), {}), [2, "unsupported"]);
  assert.deepEqual(runOutcome(report(["device-lost"], ["device-lost"]), { allowUnsupported: true }), [0, "unsupported"]);
  assert.deepEqual(runOutcome(report(["runtime-panic", "device-lost"], ["device-lost"]), {}), [0, "passed, some unsupported"]);
  assert.deepEqual(runOutcome(report(["runtime-panic"], [], ["runtime-panic: x"]), {}), [1, "failed"]);
});
