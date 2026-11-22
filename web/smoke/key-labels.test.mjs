// Tests for the browser-free parts of key-labels.mjs: `node --test web/smoke`.
import assert from "node:assert/strict";
import { test } from "node:test";
import { UsageError } from "./lib.mjs";
import { AZERTY, KEY_LABEL_CASES, expectedLabels } from "./key-labels.mjs";

test("a layout map labels the keys at once, without a press", () => {
  assert.deepEqual(expectedLabels("layout-map", true), { first: "Z/Q/S/D", afterPress: null });
});

test("without a layout map the labels are US until a press, which only Chromium can send", () => {
  assert.deepEqual(expectedLabels("no-layout-map", true), { first: "W/A/S/D", afterPress: "Z/A/S/D" });
  assert.deepEqual(expectedLabels("no-layout-map", false), { first: "W/A/S/D", afterPress: null });
  assert.deepEqual(expectedLabels("refused", true), { first: "W/A/S/D", afterPress: null });
});

test("every case has its labels and an unknown case is a usage error", () => {
  for (const c of KEY_LABEL_CASES) assert.ok(expectedLabels(c, true).first);
  assert.throws(() => expectedLabels("qwertz", true), UsageError);
});

test("the French map sends W's place to z and A's to q", () => {
  assert.equal(AZERTY.KeyW, "z");
  assert.equal(AZERTY.KeyA, "q");
});
