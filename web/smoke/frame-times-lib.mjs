// The frame-times runner's arguments and arithmetic (frame-times.mjs), kept
// apart so node --test can check them without a browser.

import { UsageError } from "./lib.mjs";

export const BACKENDS = ["webgpu", "webgl2"];
export const PACINGS = ["uncapped", "display"];

/**
 * Chromium flags for each pacing. "uncapped" switches off the display's
 * vsync and the frame-rate limit, so a frame follows the last as soon as it
 * is done and the interval is the frame's cost; "display" leaves the
 * browser's own pacing (60 Hz in headless Chromium).
 */
export const PACING_FLAGS = {
  uncapped: ["--disable-gpu-vsync", "--disable-frame-rate-limit"],
  display: [],
};

export const USAGE =
  "usage: frame-times.mjs [--dir <site>] [--build] [--backends webgpu,webgl2] [--pacing uncapped|display]\n" +
  "                       [--frames <n>] [--warmup <n>] [--repeat <n>] [--out <dir>] [--timeout <s>]";

export function parseArgs(argv) {
  const args = {
    dir: "target/web/web_smoke_3d",
    build: false,
    backends: [...BACKENDS],
    pacing: "uncapped",
    frames: 1000,
    warmup: 120,
    repeat: 3,
    out: null,
    timeout: 60,
  };
  const value = (i, flag) => {
    const v = argv[i + 1];
    if (v === undefined || v.startsWith("--")) throw new UsageError(`${flag} needs a value`);
    return v;
  };
  const whole = (v, flag) => {
    const n = Number(v);
    if (!Number.isInteger(n) || n < 1) throw new UsageError(`${flag} takes a whole number, not ${v}`);
    return n;
  };
  for (let i = 0; i < argv.length; i++) {
    const flag = argv[i];
    switch (flag) {
      case "--dir": args.dir = value(i++, flag); break;
      case "--build": args.build = true; break;
      case "--backends": {
        args.backends = value(i++, flag).split(",");
        const bad = args.backends.filter((b) => !BACKENDS.includes(b));
        if (bad.length) throw new UsageError(`unknown backend ${bad.join(", ")} (${BACKENDS.join(", ")})`);
        break;
      }
      case "--pacing": {
        args.pacing = value(i++, flag);
        if (!PACINGS.includes(args.pacing)) throw new UsageError(`--pacing must be one of ${PACINGS.join(", ")}`);
        break;
      }
      case "--frames": args.frames = whole(value(i++, flag), flag); break;
      case "--warmup": args.warmup = whole(value(i++, flag), flag); break;
      case "--repeat": args.repeat = whole(value(i++, flag), flag); break;
      case "--out": args.out = value(i++, flag); break;
      case "--timeout": args.timeout = whole(value(i++, flag), flag); break;
      default: throw new UsageError(`unknown argument ${flag}`);
    }
  }
  if (args.frames < 20) throw new UsageError("--frames must be at least 20 for a 95th percentile");
  args.out ??= `target/web-smoke/frame-times-${args.pacing}`;
  return args;
}

/** The `p`th percentile (0 to 100) of sorted values, nearest rank. */
export function percentile(sorted, p) {
  if (!sorted.length) return NaN;
  const rank = Math.ceil((p / 100) * sorted.length);
  return sorted[Math.min(sorted.length, Math.max(1, rank)) - 1];
}

/**
 * The frame times between consecutive frame stamps (ms, in order), and
 * their median, 95th and 99th percentiles, mean and maximum.
 */
export function frameStats(stamps) {
  const times = stamps.slice(1).map((t, i) => t - stamps[i]);
  const sorted = [...times].sort((a, b) => a - b);
  const mean = times.reduce((a, b) => a + b, 0) / times.length;
  return {
    frames: times.length,
    median_ms: percentile(sorted, 50),
    p95_ms: percentile(sorted, 95),
    p99_ms: percentile(sorted, 99),
    mean_ms: mean,
    max_ms: sorted[sorted.length - 1],
  };
}

/**
 * One row per backend from its page loads: the median of the loads'
 * medians and of their 95th percentiles, and the lowest and highest of each.
 */
export function summarize(loads) {
  const rows = [];
  for (const backend of BACKENDS) {
    const mine = loads.filter((l) => l.backend === backend);
    if (!mine.length) continue;
    const of = (key) => mine.map((l) => l.stats[key]).sort((a, b) => a - b);
    const medians = of("median_ms");
    const p95s = of("p95_ms");
    rows.push({
      backend,
      loads: mine.length,
      median_ms: percentile(medians, 50),
      median_range: [medians[0], medians[medians.length - 1]],
      p95_ms: percentile(p95s, 50),
      p95_range: [p95s[0], p95s[p95s.length - 1]],
    });
  }
  return rows;
}

/** The summary as a Markdown table. */
export function summaryTable(rows) {
  const ms = (v) => v.toFixed(2);
  const lines = [
    "| Backend | Loads | Median frame (ms) | Range of medians | p95 frame (ms) | Range of p95s |",
    "|---|---|---|---|---|---|",
  ];
  for (const r of rows) {
    lines.push(`| ${r.backend} | ${r.loads} | ${ms(r.median_ms)} | ${ms(r.median_range[0])}–${ms(r.median_range[1])} | ${ms(r.p95_ms)} | ${ms(r.p95_range[0])}–${ms(r.p95_range[1])} |`);
  }
  return lines.join("\n");
}
