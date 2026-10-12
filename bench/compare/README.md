# Side-by-side: TS prototype versus new compiler

Rerunnable comparison. Same sources through both compilers on an idle
machine (the script prints `uptime` at start and end), 5 runs each,
p50 (median of 5) and p95 (max of 5).

```sh
node compiler/bench/compare/run.mjs [WORK_DIR]
```

The TS prototype is `node bin/hd.js` from the repo (`src/`); the new one
is `compiler/target/release/hd` (build it first with
`cargo build --release -p hd_cli` from `compiler/`).

Inputs in `inputs/`: `tiny.hd` (the footprint tiny program), `mid.hd`
(the `bench/math` kernel fixed so both compilers accept it: `let`
reassignables instead of `:=`, `pub fn main() -> void $ Console`),
`test1.hd` / `test30.hd` / `test300.hd` (1, 30, 300 `assert_equal`
cases). `examples/dogfood/calc.hd` is read from the repo, not copied.
The checked-in `bench/tokenizer|math|records|traits` programs are
probed once each in Skips: they currently fail on both compilers
(stale `:=` reassignment / `pkg.*` single-file imports on TS,
missing `$ Console` on new), so `mid.hd` stands in for them.

Cold TS = `rm -rf build` before each run; warm TS = build dir reused
after one warmup. Cold new = fresh empty `HD_CACHE` per run; warm new =
shared `HD_CACHE` after one warmup. The new compiler has no `check`
and no `--version`: startup uses `hd --help` (new) versus `hd help`
(TS), and `check` is TS-only.
