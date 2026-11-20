// Tests for threads-lib.mjs: no browser and no build (`node --test web/smoke`).
import { test } from "node:test";
import assert from "node:assert/strict";
import { UsageError } from "./lib.mjs";
import {
  CHECK_TICKS,
  FALLBACK_WARNING,
  ISOLATION_HEADERS,
  checkFallback,
  checkIsolated,
  checkReady,
  checkSameResult,
  median,
  parseArgs,
  summarize,
  summaryTable,
} from "./threads-lib.mjs";

const isolatedRun = {
  ticks: 160,
  isolated: true,
  threads: 7,
  par_threads: 7,
  par_on_page: false,
  batch_threads: 4,
  batch_on_page: false,
  batch_threaded_runs: 160,
  schedule_threads: true,
  wait_off_page: true,
  job_off_page: true,
  group: "ready",
  group_off_page: true,
  group_bytes: 966,
  checksum: 81016914.26837707,
};

const plainRun = {
  ticks: 160,
  isolated: false,
  threads: 0,
  par_threads: 0,
  par_on_page: true,
  batch_threads: 0,
  batch_on_page: true,
  batch_threaded_runs: 0,
  schedule_threads: false,
  wait_off_page: false,
  job_off_page: false,
  group: "ready",
  group_off_page: false,
  group_bytes: 966,
  checksum: 81016914.26837707,
};

test("the isolation headers are the two that make a page cross-origin isolated", () => {
  assert.deepEqual(ISOLATION_HEADERS, {
    "cross-origin-opener-policy": "same-origin",
    "cross-origin-embedder-policy": "require-corp",
  });
});

test("arguments default to the check of the built fixture and refuse what they don't know", () => {
  const args = parseArgs([]);
  assert.equal(args.mode, "check");
  assert.equal(args.dir, "target/web/web_threads");
  assert.deepEqual(parseArgs(["--bench", "--workloads", "par,jobs", "--threads", "1,4", "--repeat", "5"]).threads, [1, 4]);
  assert.throws(() => parseArgs(["--workloads", "nope"]), UsageError);
  assert.throws(() => parseArgs(["--repeat", "0"]), UsageError);
  assert.throws(() => parseArgs(["--case", "stall"]), UsageError);
  assert.throws(() => parseArgs(["--bench", "--case", "worker-panic"]), UsageError);
});

test("a check waits for the checksum, both jobs and the asset group", () => {
  assert.equal(checkReady(null), false);
  assert.equal(checkReady({ ...isolatedRun, ticks: CHECK_TICKS - 1 }), false);
  assert.equal(checkReady({ ...isolatedRun, job_off_page: null }), false);
  assert.equal(checkReady({ ...isolatedRun, group: null }), false);
  assert.equal(checkReady(isolatedRun), true);
});

test("an isolated run passes only when workers ran the queries, batches and jobs", () => {
  assert.deepEqual(checkIsolated(isolatedRun), []);
  // The fallback run on an isolated page: every expectation names itself.
  const problems = checkIsolated({ ...plainRun, isolated: true });
  assert.ok(problems.some((p) => p.includes("0 worker threads")), problems.join("\n"));
  assert.ok(problems.some((p) => p.includes("query items ran on the page's thread")));
  assert.ok(problems.some((p) => p.includes("no batch")));
  assert.ok(problems.some((p) => p.includes("a job ran on the page's thread")));
  assert.ok(problems.some((p) => p.includes("decoded on the page's thread")));
  // A group that couldn't read its file on a worker fails the check by name.
  const failed = checkIsolated({ ...isolatedRun, group: 'failed: "crab.png" not found in assets/' });
  assert.deepEqual(failed, [`the asset group from assets/ didn't load (failed: "crab.png" not found in assets/)`]);
  // One worker is not "more than one".
  assert.equal(checkIsolated({ ...isolatedRun, par_threads: 1 }).length, 1);
});

test("a run that isn't isolated must stay on the page's thread and say why", () => {
  assert.deepEqual(checkFallback(plainRun, `warning: ponko: ... ${FALLBACK_WARNING}`), []);
  assert.ok(checkFallback(plainRun, "").some((p) => p.includes("doesn't say why")));
  assert.ok(checkFallback({ ...isolatedRun, isolated: false }, FALLBACK_WARNING).length >= 3);
});

test("both runs must simulate the same world", () => {
  assert.deepEqual(checkSameResult(isolatedRun, plainRun), []);
  assert.equal(checkSameResult(isolatedRun, { ...plainRun, checksum: 1 }).length, 1);
  assert.equal(checkSameResult(isolatedRun, { ...plainRun, group_bytes: 0 }).length, 1);
});

test("the summary compares medians and keeps a speed-up inside the noise from counting", () => {
  assert.equal(median([3, 1, 2]), 2);
  const loads = [
    { workload: "par", threads: 1, runs_ms: [10, 10, 11] },
    { workload: "par", threads: 1, runs_ms: [12, 12, 12] },
    { workload: "par", threads: 7, runs_ms: [2, 2, 3] },
    { workload: "par", threads: 7, runs_ms: [2.2, 2.2, 2.2] },
    // Noisy: medians 10 and 20 with one thread, 9 and 19 with seven.
    { workload: "jobs", threads: 1, runs_ms: [10] },
    { workload: "jobs", threads: 1, runs_ms: [20] },
    { workload: "jobs", threads: 7, runs_ms: [9] },
    { workload: "jobs", threads: 7, runs_ms: [19] },
  ];
  const rows = summarize(loads);
  const par = rows.find((r) => r.workload === "par" && r.threads === 7);
  assert.equal(par.median_ms, 2);
  assert.ok(par.speedup > 4 && par.beyond_noise, JSON.stringify(par));
  const jobs = rows.find((r) => r.workload === "jobs" && r.threads === 7);
  assert.equal(jobs.beyond_noise, false, JSON.stringify(jobs));
  const table = summaryTable(rows);
  assert.match(table, /\| par \| 7 \| 2\.00 \|/);
  assert.match(table, /within noise/);
});
