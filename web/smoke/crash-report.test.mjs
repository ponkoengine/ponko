// The shipped page offers a game's crash report (`ponko::crash`) as a file
// to save, even on machines without a browser.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { runInNewContext } from "node:vm";

const template = readFileSync(new URL("../index.html", import.meta.url), "utf8");
const script = template.split('<script id="crash-report-script">')[1].split("</script>")[0];

function page(already) {
  const link = { hidden: true };
  const listeners = {};
  const window = {
    ponkoCrashReport: already,
    addEventListener(name, f) { listeners[name] = f; },
  };
  const blobs = [];
  runInNewContext(script, {
    window,
    document: { getElementById: (id) => (id === "crash-report" ? link : null) },
    URL: { createObjectURL: (blob) => { blobs.push(blob); return `blob:${blobs.length}`; } },
    Blob: class { constructor(parts, options) { this.text = parts.join(""); this.type = options.type; } },
    String,
  });
  return { link, blobs, send: (detail) => listeners.ponkocrashreport({ detail }) };
}

const report = {
  name: "crash-reports/2026-11-21_14-03-07.txt",
  text: "Space Crab crash report\nkind: panic\n",
  kind: "panic",
  message: "boom",
};

test("the link stays hidden until the game sends a crash report", () => {
  const p = page(undefined);
  assert.equal(p.link.hidden, true);
  assert.equal(p.blobs.length, 0);
});

test("a crash report becomes a text file the player can save", () => {
  const p = page(undefined);
  p.send(report);
  assert.equal(p.link.hidden, false);
  assert.equal(p.link.download, "2026-11-21_14-03-07.txt");
  assert.equal(p.link.href, "blob:1");
  assert.equal(p.blobs[0].text, report.text);
  assert.equal(p.blobs[0].type, "text/plain");
});

test("a report sent before the page listened is offered too", () => {
  const p = page(report);
  assert.equal(p.link.hidden, false);
  assert.equal(p.blobs[0].text, report.text);
});

test("an event without a report changes nothing", () => {
  const p = page(undefined);
  p.send(null);
  p.send({ name: "x.txt" });
  assert.equal(p.link.hidden, true);
});
