// The parts of the threads runner (threads.mjs) that need no browser:
// arguments, what a run of examples/web_threads.rs must report on a
// cross-origin isolated page and on one that isn't, and the benchmark
// summary. threads.test.mjs tests this file (`node --test web/smoke`).

import { UsageError } from "./lib.mjs";

/** What makes a page cross-origin isolated, so the game may start workers. */
export const ISOLATION_HEADERS = {
  "cross-origin-opener-policy": "same-origin",
  "cross-origin-embedder-policy": "require-corp",
};

/** The fallback warning the engine logs on a page that isn't isolated. */
export const FALLBACK_WARNING = "it runs on the page's thread alone";

/** What the engine logs when a worker fails and it stops the game. */
export const STOP_MESSAGE = "the game stops";

/** The fixture's benchmark workloads (examples/web_threads.rs). */
export const WORKLOADS = ["par", "sweep", "interleaved", "jobs"];

/** Ticks a check waits for: past the checksum (tick 120) and a few jobs. */
export const CHECK_TICKS = 150;

export const USAGE =
  "usage: threads.mjs [--dir <site>] [--build] [--browser chromium|firefox|webkit]\n" +
  "                   [--check] [--case worker-panic]\n" +
  "                   [--bench [--workloads par,sweep,interleaved,jobs] [--threads 1,<n>] [--repeat <r>]]\n" +
  "                   [--out <dir>] [--timeout <s>]";

export function parseArgs(argv) {
  const args = {
    dir: "target/web/web_threads",
    build: false,
    browser: "chromium",
    mode: "check",
    case: null,
    workloads: [...WORKLOADS],
    threads: null,
    repeat: 3,
    out: "target/web-smoke/threads",
    timeout: 120,
  };
  const value = (i, flag) => {
    const v = argv[i + 1];
    if (v === undefined || v.startsWith("--")) throw new UsageError(`${flag} needs a value`);
    return v;
  };
  const number = (v, flag) => {
    const n = Number(v);
    if (!Number.isInteger(n) || n < 1) throw new UsageError(`${flag} takes a whole number, not ${v}`);
    return n;
  };
  for (let i = 0; i < argv.length; i++) {
    const flag = argv[i];
    switch (flag) {
      case "--dir": args.dir = value(i++, flag); break;
      case "--build": args.build = true; break;
      case "--browser": args.browser = value(i++, flag); break;
      case "--check": args.mode = "check"; break;
      case "--bench": args.mode = "bench"; break;
      case "--case": {
        args.case = value(i++, flag);
        if (args.case !== "worker-panic") throw new UsageError(`unknown case ${args.case} (worker-panic)`);
        break;
      }
      case "--workloads": {
        args.workloads = value(i++, flag).split(",");
        const bad = args.workloads.filter((w) => !WORKLOADS.includes(w));
        if (bad.length) throw new UsageError(`unknown workload ${bad.join(", ")} (${WORKLOADS.join(", ")})`);
        break;
      }
      case "--threads": args.threads = value(i++, flag).split(",").map((t) => number(t, flag)); break;
      case "--repeat": args.repeat = number(value(i++, flag), flag); break;
      case "--out": args.out = value(i++, flag); break;
      case "--timeout": args.timeout = number(value(i++, flag), flag); break;
      default: throw new UsageError(`unknown argument ${flag}`);
    }
  }
  if (args.case && args.mode !== "check") throw new UsageError("--case goes with --check");
  return args;
}

/** Whether a check run has reported everything it is checked on. */
export function checkReady(state) {
  return (
    state != null &&
    state.ticks >= CHECK_TICKS &&
    state.checksum != null &&
    state.wait_off_page != null &&
    state.job_off_page != null &&
    state.group != null
  );
}

/**
 * The problems with a check run on a cross-origin isolated page: the game
 * must have started workers and run query items, batch systems and jobs on
 * them, never on the page's thread.
 */
export function checkIsolated(state) {
  const problems = [];
  const expect = (ok, what) => ok || problems.push(what);
  expect(state.isolated === true, `the page isn't cross-origin isolated (isolated: ${state.isolated})`);
  expect(state.threads >= 2, `the engine started ${state.threads} worker threads, not at least 2`);
  expect(state.par_threads >= 2, `parallel query items ran on ${state.par_threads} worker threads, not at least 2`);
  expect(state.par_on_page === false, "parallel query items ran on the page's thread");
  expect(state.batch_threads >= 2, `batch systems ran on ${state.batch_threads} worker threads, not at least 2`);
  expect(state.batch_on_page === false, "batch systems ran on the page's thread");
  expect(state.batch_threaded_runs > 0, "the scheduler sent no batch to the workers");
  expect(state.schedule_threads === true, "the scheduler says it has no threads");
  expect(state.wait_off_page === true, "the job waited for in setup ran on the page's thread");
  expect(state.job_off_page === true, "a job ran on the page's thread");
  expect(state.group === "ready", `the asset group from assets/ didn't load (${state.group})`);
  // Where it was decoded only counts once it loaded.
  expect(state.group !== "ready" || state.group_off_page === true, "the asset group's file was decoded on the page's thread");
  return problems;
}

/**
 * The problems with a check run on a page that isn't isolated: the same
 * build must run, on the page's thread alone, and say why in the console.
 */
export function checkFallback(state, consoleText) {
  const problems = [];
  const expect = (ok, what) => ok || problems.push(what);
  expect(state.isolated === false, `the page is cross-origin isolated (isolated: ${state.isolated})`);
  expect(state.threads === 0, `the engine started ${state.threads} worker threads on a page that isn't isolated`);
  expect(state.par_threads === 0 && state.par_on_page === true, "parallel query items didn't all run on the page's thread");
  expect(state.batch_threads === 0 && state.batch_threaded_runs === 0, "batch systems went to other threads");
  expect(state.wait_off_page === false && state.job_off_page === false, "jobs didn't run on the page's thread");
  expect(state.group === "ready", `the asset group from assets/ didn't load (${state.group})`);
  expect(state.group !== "ready" || state.group_off_page === false, "the asset group's file wasn't decoded on the page's thread");
  expect(consoleText.includes(FALLBACK_WARNING), `the console doesn't say why the game runs on one thread ("${FALLBACK_WARNING}")`);
  return problems;
}

/** The two runs must have simulated the same world and read the same file. */
export function checkSameResult(isolated, fallback) {
  const problems = [];
  if (isolated.checksum !== fallback.checksum) {
    problems.push(`the checksums differ: ${isolated.checksum} with threads, ${fallback.checksum} without`);
  }
  if (isolated.group_bytes !== fallback.group_bytes) {
    problems.push(`the asset group read ${isolated.group_bytes} bytes with threads, ${fallback.group_bytes} without`);
  }
  return problems;
}

/** Nearest-rank percentile of sorted numbers. */
function rank(sorted, p) {
  return sorted[Math.min(sorted.length - 1, Math.max(0, Math.ceil((p / 100) * sorted.length) - 1))];
}

/** The median of some numbers. */
export function median(values) {
  const sorted = [...values].sort((a, b) => a - b);
  return rank(sorted, 50);
}

/**
 * One row per workload and thread count, from page loads `{workload,
 * threads, runs_ms}`: the median of each load's median, the spread of
 * those medians (`(max - min) / median`), and the speed-up against one
 * thread with whether it is beyond the noise (the two spreads together).
 */
export function summarize(loads) {
  const groups = new Map();
  for (const load of loads) {
    const key = `${load.workload}/${load.threads}`;
    if (!groups.has(key)) groups.set(key, { workload: load.workload, threads: load.threads, medians: [] });
    groups.get(key).medians.push(median(load.runs_ms));
  }
  const rows = [...groups.values()].map((g) => {
    const m = median(g.medians);
    const spread = g.medians.length > 1 ? (Math.max(...g.medians) - Math.min(...g.medians)) / m : null;
    return { ...g, median_ms: m, min_ms: Math.min(...g.medians), max_ms: Math.max(...g.medians), spread };
  });
  for (const row of rows) {
    const one = rows.find((r) => r.workload === row.workload && r.threads <= 1);
    if (!one || one === row) continue;
    row.speedup = one.median_ms / row.median_ms;
    const noise = (one.spread ?? Infinity) + (row.spread ?? Infinity);
    row.beyond_noise = Math.abs(row.speedup - 1) > noise;
  }
  return rows.sort((a, b) => a.workload.localeCompare(b.workload) || a.threads - b.threads);
}

/** The summary as a Markdown table. */
export function summaryTable(rows) {
  const ms = (v) => v.toFixed(2);
  const lines = [
    "| Workload | Threads | Median (ms) | Min–max of loads (ms) | Spread | Speed-up vs 1 |",
    "|---|---|---|---|---|---|",
  ];
  for (const r of rows) {
    const speedup = r.speedup == null ? "" : `${r.speedup.toFixed(2)}x${r.beyond_noise ? "" : " (within noise)"}`;
    const spread = r.spread == null ? "n/a" : `${(r.spread * 100).toFixed(0)}%`;
    lines.push(`| ${r.workload} | ${r.threads} | ${ms(r.median_ms)} | ${ms(r.min_ms)}–${ms(r.max_ms)} | ${spread} | ${speedup} |`);
  }
  return lines.join("\n");
}
