// Tests for first-load.mjs's server: the range requests an asset pack is
// fetched with (`node --test web/smoke`).
import assert from "node:assert/strict";
import { test } from "node:test";
import { range } from "./first-load.mjs";

test("a range within the file is served as asked", () => {
  assert.deepEqual(range("bytes=0-65535", 200000), { start: 0, end: 65535 });
  assert.deepEqual(range("bytes=65536-199999", 200000), { start: 65536, end: 199999 });
});

test("a range past the end is clipped to the file, as a small pack's first request is", () => {
  assert.deepEqual(range("bytes=0-65535", 1000), { start: 0, end: 999 });
  assert.deepEqual(range("bytes=10-", 1000), { start: 10, end: 999 });
  assert.deepEqual(range("bytes=-100", 1000), { start: 900, end: 999 });
});

test("no range, or one that can't be read, is the whole file or unsatisfiable", () => {
  assert.equal(range(undefined, 10), null);
  assert.equal(range("items=0-1", 10), null);
  assert.equal(range("bytes=20-30", 10), "unsatisfiable");
  assert.equal(range("bytes=5-2", 10), "unsatisfiable");
});
