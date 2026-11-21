// Tests for spectrum-lib.mjs: no browser and no build (`node --test web/smoke`).
import { test } from "node:test";
import assert from "node:assert/strict";
import { UsageError } from "./lib.mjs";
import { CELLS, KINDS, VARIANTS, benchDone, buildArgs, checkRun, parseArgs, summarize, summaryTable, variantEnv } from "./spectrum-lib.mjs";

test("arguments default to all three builds, three loads each, and refuse what they don't know", () => {
  const args = parseArgs([]);
  assert.deepEqual(args.variants, ["scalar", "simd128", "simd"]);
  assert.equal(args.repeat, 3);
  assert.equal(args.build, false);
  assert.deepEqual(parseArgs(["--variants", "scalar,simd", "--repeat", "5", "--build"]).variants, ["scalar", "simd"]);
  assert.throws(() => parseArgs(["--variants", "avx"]), UsageError);
  assert.throws(() => parseArgs(["--repeat", "0"]), UsageError);
  assert.throws(() => parseArgs(["--repeat"]), UsageError);
  assert.throws(() => parseArgs(["--fast"]), UsageError);
});

test("both SIMD builds add simd128, to the flags cargo reads, keeping theirs", () => {
  const target = "CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS";
  assert.deepEqual(variantEnv("scalar", { PATH: "p" }), { PATH: "p" });
  assert.equal(variantEnv("simd128", {})[target], "-C target-feature=+simd128");
  assert.equal(variantEnv("simd", {})[target], "-C target-feature=+simd128");
  assert.equal(variantEnv("simd", { [target]: "-C opt-level=s" })[target], "-C opt-level=s -C target-feature=+simd128");
  const rust = variantEnv("simd", { RUSTFLAGS: "-D warnings" });
  assert.equal(rust.RUSTFLAGS, "-D warnings -C target-feature=+simd128", "cargo ignores the target variable when RUSTFLAGS is set");
  assert.equal(rust[target], undefined);
  assert.equal(variantEnv("simd", { CARGO_ENCODED_RUSTFLAGS: "-Dwarnings" }).CARGO_ENCODED_RUSTFLAGS, "-Dwarnings\x1f-C\x1ftarget-feature=+simd128");
});

test("each build goes to its own folder with its features", () => {
  assert.deepEqual(buildArgs("scalar"), ["web/build.sh", "--features", "3d", "web_spectrum", "target/web/web_spectrum"]);
  assert.deepEqual(buildArgs("simd").slice(1, 3), ["--features", "3d,wasm-simd"]);
  const dirs = Object.keys(VARIANTS).map((v) => buildArgs(v)[4]);
  assert.equal(new Set(dirs).size, dirs.length, "no build overwrites another");
});

/** A finished run of `variant`: every size and kind, `ms` per moment. */
function run(variant, ms = (cells) => cells / 100) {
  const v = VARIANTS[variant];
  return {
    simd: v.simd128,
    rustfft_simd: v.features.includes("wasm-simd"),
    done: true,
    bench: CELLS.flatMap((cells) => KINDS.map((kind) => ({ cells, kind, moment_ms: [ms(cells, kind), ms(cells, kind) * 1.1, ms(cells, kind) * 0.9] }))),
  };
}

test("a run passes when it was built as asked and timed every size and kind", () => {
  assert.ok(benchDone(run("scalar")));
  assert.ok(!benchDone({ done: false, bench: [] }));
  assert.ok(!benchDone(null));
  for (const variant of Object.keys(VARIANTS)) assert.deepEqual(checkRun(variant, run(variant)), []);
  assert.match(checkRun("simd", run("scalar"))[0], /simd128 false/);
  assert.match(checkRun("simd", { ...run("simd"), rustfft_simd: false })[0], /rustfft's SIMD false/);
  const missing = { ...run("scalar"), bench: run("scalar").bench.filter((r) => r.cells !== 256 || r.kind !== "sway") };
  assert.match(checkRun("scalar", missing)[0], /no sway timing at 256 cells/);
  const broken = { ...run("scalar"), bench: run("scalar").bench.map((r) => ({ ...r, moment_ms: r.cells === 64 ? [0] : r.moment_ms })) };
  assert.equal(checkRun("scalar", broken).length, 2, "a zero time at 64 cells, for both kinds");
});

test("the summary gives medians, ranges and speed-ups beyond the noise only when the ranges don't overlap", () => {
  const loads = [
    { variant: "scalar", bench: run("scalar", (c) => c / 100).bench },
    { variant: "scalar", bench: run("scalar", (c) => c / 80).bench },
    { variant: "simd", bench: run("simd", (c) => c / 400).bench },
    { variant: "simd", bench: run("simd", (c) => c / 300).bench },
    { variant: "simd128", bench: run("simd128", (c) => c / 95).bench },
    { variant: "simd128", bench: run("simd128", (c) => c / 90).bench },
  ];
  const rows = summarize(loads);
  assert.equal(rows.length, CELLS.length * KINDS.length * 3);
  const at = (cells, kind, variant) => rows.find((r) => r.cells === cells && r.kind === kind && r.variant === variant);
  const base = at(256, "height", "scalar");
  assert.equal(base.speedup, undefined);
  assert.equal(base.min_ms, 2.56);
  assert.equal(base.max_ms, 3.2);
  const simd = at(256, "height", "simd");
  assert.ok(simd.speedup > 3 && simd.beyond_noise, `a clear win: ${simd.speedup}`);
  const close = at(256, "height", "simd128");
  assert.ok(Number.isFinite(close.speedup) && !close.beyond_noise, "inside the scalar build's range: noise");
  assert.deepEqual(rows.slice(0, 3).map((r) => r.variant), ["scalar", "simd128", "simd"]);
  const table = summaryTable(rows);
  assert.match(table, /\| 256 \| height \| simd \| 0\.64 \| 0\.64 to 0\.85 \| 4\.00x \|/);
  assert.match(table, /within noise/);
});
