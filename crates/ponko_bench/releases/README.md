# Release baselines

Wall-clock baselines of `ponko_bench`, one folder per release and machine:
`<version>/<machine>/run-<n>.json`, five runs on one idle machine, recorded
for the release pull request (CONTRIBUTING.md, "Releasing") and linked from
the release notes. The machine folder is the name `bench_track` gives the
machine (`<os>-<arch>-<cpu>`, for example `macos-aarch64-apple-m4-max`, or
`PONKO_BENCH_MACHINE`).

Compare two releases on the same machine:

```sh
cargo run --release -p ponko_bench --bin bench_track -- \
  compare crates/ponko_bench/releases/<old>/<machine> crates/ponko_bench/releases/<new>/<machine>
```

0.1.0 has no folder: its record is `baselines/ecs.json` as the `v0.1.0` tag has
it (`git show v0.1.0:crates/ponko_bench/baselines/ecs.json`; the file in the
working tree has been recorded again since; see the crate README, "Tracking
results over time"). The instruction counts of every
release recorded before the project dropped CI are on the `bench-history`
branch, under `releases/<tag>/`.
