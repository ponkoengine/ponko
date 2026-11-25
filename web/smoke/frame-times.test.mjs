// node --test web/smoke/frame-times.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { UsageError } from "./lib.mjs";
import { PACING_FLAGS, frameStats, parseArgs, percentile, summarize, summaryTable } from "./frame-times-lib.mjs";

test("defaults: both backends, uncapped, 1000 frames after 120, three loads", () => {
  const args = parseArgs([]);
  assert.deepEqual(args.backends, ["webgpu", "webgl2"]);
  assert.equal(args.pacing, "uncapped");
  assert.equal(args.frames, 1000);
  assert.equal(args.warmup, 120);
  assert.equal(args.repeat, 3);
  assert.equal(args.dir, "target/web/web_smoke_3d");
  assert.equal(args.out, "target/web-smoke/frame-times-uncapped");
});

test("arguments are checked", () => {
  assert.deepEqual(parseArgs(["--backends", "webgl2"]).backends, ["webgl2"]);
  assert.equal(parseArgs(["--pacing", "display"]).out, "target/web-smoke/frame-times-display");
  assert.throws(() => parseArgs(["--backends", "vulkan"]), UsageError);
  assert.throws(() => parseArgs(["--pacing", "fast"]), UsageError);
  assert.throws(() => parseArgs(["--frames", "10"]), UsageError);
  assert.throws(() => parseArgs(["--repeat", "0"]), UsageError);
  assert.throws(() => parseArgs(["--frames"]), UsageError);
  assert.throws(() => parseArgs(["--nope"]), UsageError);
});

test("uncapped pacing switches off vsync and the frame-rate limit; display pacing changes nothing", () => {
  assert.deepEqual(PACING_FLAGS.uncapped, ["--disable-gpu-vsync", "--disable-frame-rate-limit"]);
  assert.deepEqual(PACING_FLAGS.display, []);
});

test("percentiles are nearest-rank", () => {
  const sorted = Array.from({ length: 100 }, (_, i) => i + 1);
  assert.equal(percentile(sorted, 50), 50);
  assert.equal(percentile(sorted, 95), 95);
  assert.equal(percentile(sorted, 100), 100);
  assert.equal(percentile([7], 95), 7);
  assert.ok(Number.isNaN(percentile([], 50)));
});

test("frame times are the gaps between stamps", () => {
  // 20 frames of 2 ms and one of 10 ms.
  const stamps = [0];
  for (let i = 0; i < 20; i++) stamps.push(stamps.at(-1) + 2);
  stamps.push(stamps.at(-1) + 10);
  const s = frameStats(stamps);
  assert.equal(s.frames, 21);
  assert.equal(s.median_ms, 2);
  assert.equal(s.p95_ms, 2);
  assert.equal(s.p99_ms, 10);
  assert.equal(s.max_ms, 10);
  assert.ok(Math.abs(s.mean_ms - 50 / 21) < 1e-9);
});

test("the summary takes the median over loads, with the range, per backend", () => {
  const load = (backend, median, p95) => ({ backend, stats: { median_ms: median, p95_ms: p95 } });
  const rows = summarize([load("webgl2", 1.2, 3), load("webgpu", 0.2, 0.6), load("webgl2", 1.0, 2.8), load("webgl2", 1.4, 3.3)]);
  assert.deepEqual(rows.map((r) => r.backend), ["webgpu", "webgl2"]);
  const gl = rows[1];
  assert.equal(gl.loads, 3);
  assert.equal(gl.median_ms, 1.2);
  assert.deepEqual(gl.median_range, [1.0, 1.4]);
  assert.equal(gl.p95_ms, 3);
  assert.deepEqual(gl.p95_range, [2.8, 3.3]);
  assert.match(summaryTable(rows), /\| webgl2 \| 3 \| 1\.20 \| 1\.00–1\.40 \| 3\.00 \| 2\.80–3\.30 \|/);
});
