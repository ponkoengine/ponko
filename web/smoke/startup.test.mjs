// Exercise the shipped page's startup UI even on machines without a browser.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { runInNewContext } from "node:vm";

const template = readFileSync(new URL("../index.html", import.meta.url), "utf8");
const script = template.split('<script type="module">')[1].split("</script>")[0];

async function startPage(init = async () => {}) {
  const status = { textContent: "Loading…", remove() { this.removed = true; } };
  const canvas = { focus() { this.focused = true; } };
  let callback;
  await runInNewContext(`(async () => {${script.replace(/^\s*import .*;$/m, "")}})()`, {
    init,
    game: { onPonkoStartup: (handler) => { callback = handler; } },
    document: { getElementById: (id) => ({ status, ponko: canvas })[id] },
    window: { addEventListener() {} },
    console: { error() {} },
  });
  return { status, canvas, report: callback };
}

test("wasm initialization leaves the page loading until engine setup succeeds", async () => {
  const page = await startPage(async () => { throw new Error("Using exceptions for control flow"); });
  assert.equal(page.status.textContent, "Loading…");
  assert.equal(page.status.removed, undefined);
  assert.equal(page.canvas.focused, undefined);
  page.report(null);
  assert.equal(page.status.removed, true);
  assert.equal(page.canvas.focused, true);
});

for (const [kind, message] of [
  ["Unsupported", "No usable WebGPU or WebGL2 context"],
  ["Unsupported", "The WebGL2 context was lost during startup"],
  ["NotFound", "crab.png was not fetched"],
]) {
  test(`the template displays the startup error: ${message}`, async () => {
    const page = await startPage();
    page.report({ kind, message });
    assert.equal(page.status.className, "error");
    assert.equal(page.status.textContent, `Could not start the game: ${message}`);
    assert.equal(page.canvas.focused, undefined);
  });
}
