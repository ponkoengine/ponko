// The parts of the spectrum runner (spectrum.mjs) that need no browser:
// arguments, the builds it compares, what a run of
// examples/web_spectrum.rs must report, and the summary.
// spectrum.test.mjs tests this file (`node --test web/smoke`).

import { UsageError } from "./lib.mjs";
import { median } from "./threads-lib.mjs";

/** The grid sizes the fixture times, and the two kinds of moment. */
export const CELLS = [64, 128, 256];
export const KINDS = ["height", "sway"];

/**
 * The builds compared: the ordinary one; `simd128` for the whole module
 * (the compiler's own vectorising, rustfft's scalar code); and that with
 * rustfft's WASM SIMD FFT (the `wasm-simd` feature). The feature without
 * the flag (three times slower when it was measured) no longer compiles.
 */
export const VARIANTS = {
  scalar: { features: "3d", simd128: false, dir: "target/web/web_spectrum" },
  simd128: { features: "3d", simd128: true, dir: "target/web/web_spectrum_simd128" },
  simd: { features: "3d,wasm-simd", simd128: true, dir: "target/web/web_spectrum_simd" },
};

export const USAGE =
  "usage: spectrum.mjs [--build] [--variants scalar,simd128,simd] [--repeat <r>]\n" +
  "                    [--browser chromium|firefox|webkit] [--out <dir>] [--timeout <s>]";

export function parseArgs(argv) {
  const args = {
    build: false,
    variants: Object.keys(VARIANTS),
    repeat: 3,
    browser: "chromium",
    out: "target/web-smoke/spectrum",
    timeout: 180,
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
      case "--build": args.build = true; break;
      case "--variants": {
        args.variants = value(i++, flag).split(",");
        const bad = args.variants.filter((v) => !(v in VARIANTS));
        if (bad.length) throw new UsageError(`unknown variant ${bad.join(", ")} (${Object.keys(VARIANTS).join(", ")})`);
        break;
      }
      case "--repeat": args.repeat = number(value(i++, flag), flag); break;
      case "--browser": args.browser = value(i++, flag); break;
      case "--out": args.out = value(i++, flag); break;
      case "--timeout": args.timeout = number(value(i++, flag), flag); break;
      default: throw new UsageError(`unknown argument ${flag}`);
    }
  }
  return args;
}

/**
 * The environment a variant builds in: `simd128` added to the rust flags
 * cargo reads (the first of CARGO_ENCODED_RUSTFLAGS, RUSTFLAGS and the
 * wasm target's variable that is set), keeping what they hold.
 */
export function variantEnv(variant, env) {
  const out = { ...env };
  if (!VARIANTS[variant].simd128) return out;
  const flag = "-C target-feature=+simd128";
  const set = (name) => (out[name] ?? "") !== "";
  if (set("CARGO_ENCODED_RUSTFLAGS")) {
    out.CARGO_ENCODED_RUSTFLAGS = `${out.CARGO_ENCODED_RUSTFLAGS}\x1f${flag.replace(" ", "\x1f")}`;
  } else {
    const target = "CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS";
    const name = set("RUSTFLAGS") ? "RUSTFLAGS" : target;
    out[name] = `${out[name] ?? ""} ${flag}`.trim();
  }
  return out;
}

/** The web/build.sh arguments for a variant. */
export function buildArgs(variant) {
  const v = VARIANTS[variant];
  return ["web/build.sh", "--features", v.features, "web_spectrum", v.dir];
}

/** Whether a page's state has every size and kind timed. */
export function benchDone(state) {
  return state?.done === true && Array.isArray(state.bench);
}

/**
 * The problems with a finished run of `variant`: it must say it was built
 * as asked (simd128, rustfft's SIMD) and have timed every size and kind,
 * every time positive and finite.
 */
export function checkRun(variant, state) {
  const v = VARIANTS[variant];
  const problems = [];
  const expect = (ok, what) => ok || problems.push(what);
  expect(state.simd === v.simd128, `the ${variant} build reports simd128 ${state.simd}, not ${v.simd128}`);
  const fft = v.features.includes("wasm-simd");
  expect(state.rustfft_simd === fft, `the ${variant} build reports rustfft's SIMD ${state.rustfft_simd}, not ${fft}`);
  for (const cells of CELLS) {
    for (const kind of KINDS) {
      const row = state.bench.find((r) => r.cells === cells && r.kind === kind);
      if (!row) {
        problems.push(`no ${kind} timing at ${cells} cells`);
        continue;
      }
      expect(
        row.moment_ms.length > 0 && row.moment_ms.every((ms) => Number.isFinite(ms) && ms > 0),
        `${cells} cells, ${kind}: times ${JSON.stringify(row.moment_ms)}`,
      );
    }
  }
  return problems;
}

/**
 * One row per size, kind and variant from page loads `{variant, bench}`:
 * the median of each load's median, the range of those medians, and for
 * a SIMD variant the speed-up over the ordinary build and whether it is
 * beyond the noise (the two ranges don't overlap).
 */
export function summarize(loads) {
  const groups = new Map();
  for (const load of loads) {
    for (const row of load.bench) {
      const key = `${row.cells}/${row.kind}/${load.variant}`;
      if (!groups.has(key)) groups.set(key, { cells: row.cells, kind: row.kind, variant: load.variant, medians: [] });
      groups.get(key).medians.push(median(row.moment_ms));
    }
  }
  const rows = [...groups.values()].map((g) => ({
    ...g,
    median_ms: median(g.medians),
    min_ms: Math.min(...g.medians),
    max_ms: Math.max(...g.medians),
  }));
  for (const row of rows) {
    const base = rows.find((r) => r.cells === row.cells && r.kind === row.kind && r.variant === "scalar");
    if (!base || base === row) continue;
    row.speedup = base.median_ms / row.median_ms;
    row.beyond_noise = row.max_ms < base.min_ms || row.min_ms > base.max_ms;
  }
  const order = Object.keys(VARIANTS);
  return rows.sort(
    (a, b) => a.cells - b.cells || KINDS.indexOf(a.kind) - KINDS.indexOf(b.kind) || order.indexOf(a.variant) - order.indexOf(b.variant),
  );
}

/** The summary as a Markdown table. */
export function summaryTable(rows) {
  const ms = (v) => v.toFixed(2);
  const lines = ["| Cells | Moment | Build | Median ms | Range ms | Speed-up |", "|---|---|---|---|---|---|"];
  for (const r of rows) {
    const speedup = r.speedup == null ? "" : `${r.speedup.toFixed(2)}x${r.beyond_noise ? "" : " (within noise)"}`;
    lines.push(`| ${r.cells} | ${r.kind} | ${r.variant} | ${ms(r.median_ms)} | ${ms(r.min_ms)} to ${ms(r.max_ms)} | ${speedup} |`);
  }
  return lines.join("\n");
}
