// The shipped page shows failures that stop a running game (a panic, a
// lost WebGL2 context, a stopped worker thread), on machines without a
// browser: the template's script runs against a fake page and a fake
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
const { checkRun, expected } = await import("./runtime-failures.mjs");

test("the browser runner passes a run that shows the panic it reported", () => {
  const want = expected("runtime-panic");
  const failure = { kind: "Panic", message: want.message, location: "examples/common/web_smoke.rs:80:13", text: null };
  assert.deepEqual(checkRun("runtime-panic", { ticks: 21, status: want.status, failure }), []);
});

test("the browser runner fails a run whose page shows the trap instead of the panic", () => {
  const want = expected("runtime-panic");
  const failure = { kind: "Panic", message: want.message, location: "examples/common/web_smoke.rs:80:13", text: null };
  const problems = checkRun("runtime-panic", { ticks: 21, status: "The game stopped: RuntimeError: unreachable (see the browser console)", failure });
  assert.equal(problems.length, 1);
  assert.match(problems[0], /the page shows/);
});

test("the browser runner fails a run without the game's own failure text", () => {
  const want = expected("failure-text");
  const failure = { kind: "Panic", message: want.message, location: "examples/common/web_smoke.rs:80:13", text: null };
  const problems = checkRun("failure-text", { ticks: 21, status: `The game stopped: ${want.message}`, failure });
  assert.equal(problems.length, 2);
});

test("the browser runner fails a run where no failure reached onPonkoFailure", () => {
  const want = expected("context-lost");
  assert.deepEqual(checkRun("context-lost", { ticks: 30, status: want.status, failure: null }), ["onPonkoFailure never called its callback"]);
});
