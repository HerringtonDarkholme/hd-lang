---
name: compiler-tests
description: Test the hd compiler with stage fixtures and benchmarks - write a focused .hd case, pick the view that shows one topic (tokens, tree, tir, own, wat, counts, layout), bless and review generated baselines, record bad emitted code with an expected note, and measure performance (perf jobs: attribute, then a ceiling experiment, then build), run the gates for a compiler change (conformance in three modes, stage tests), and report perf and size in the standard table (every bench program against JS, the three V8 tier columns, sizes, compile time). Use when adding or changing a compiler test, checking what a stage emits for some code, gating a compiler change, or measuring a perf change.
---

# Test The hd Compiler

Two tools: **stage fixtures** show what one compiler stage produces for a
small piece of hd, and **bench/study** measures speed. Runtime behaviour
(what a program prints, exit codes, panics) is tested in
`spec/conformance/` (see its README); a fixture never replaces it.
Design record: issues #657 and #675,
[docs/compiler/testing-the-compiler.md](../../../docs/compiler/testing-the-compiler.md) §8.4.

## Principles

1. **A case is easy to add.** Write a small `.hd` file and bless. No Rust,
   no registration.
2. **Output reads by eye.** Names, not indices; no hashes; nothing
   unrelated to the topic.
3. **A case is about one topic** (struct layout, a field write, count
   changes on a return). Put context in the file only as far as the
   subject needs it.
4. **Written code and generated output never share a file.** People edit
   cases; only blessing writes baselines.

## Layout

| Path | Holds |
| --- | --- |
| `crates/<crate>/tests/cases/<topic>/<name>.hd` | a case: plain hd, hand-written |
| `crates/<crate>/tests/baselines/<topic>/<name>.<view>` | its generated output, one file per view |

The crate is the stage whose view the case uses: `hd_syntax` (tokens,
tree), `hd_check` (tir), `hd_wasm_linear` (wat), `hd_driver` (own,
counts, layout). The harness is `crates/hd_testkit/src/stages.rs`; every crate
runs its cases with `cargo test -p <crate> --test stages` (`hd_syntax`:
`cargo test -p hd_syntax --test all stages::`). Cases are discovered
recursively; no generated output belongs in a source case. A baseline
starts with a generated/do-not-edit banner.

## Writing A Case

```hd
# @dump: wat counts
# expected: one f64.store at offset 8; no retain or release
struct Body:
    x: f64
    vx: f64

fn place(bodies: &mut List[Body], i: usize, x: f64) -> void:
    bodies[i].x = x
    #^^^^^^^^^^^^^^
```

- **Header comments** at the top:
  - `# @dump: VIEW...` names the views to generate;
  - `# @build: wasmtime release` picks the engine target and profile
    (defaults `v8`, `dev`);
  - `# @file: other.hd` starts an additional virtual module; following
    lines belong to it until the next `@file`. The first module defaults
    to `main.hd`; dumps and subject markers select that module only.
    The harness checks multi-module cases as virtual packages.
- **The subject** (optional): a caret line `#^^^` under the code it marks,
  or a `# subject` line before a statement (it covers the lines indented
  deeper after it). Without a marker the whole file is dumped, each source
  line followed by the output it produced.
- **The file must pass `hd check` and `hd fmt --check`** (use the write-hd
  and hd-cli skills). No `main` is needed: fixture builds emit every
  function the case defines.
- **`# expected:`** states what good output would be when today's output is
  bad. A blessed baseline records current behaviour, not correct
  behaviour; the note keeps the difference visible.

## Views

| View | Shows |
| --- | --- |
| `tokens` | each token's kind and text |
| `tree` | the largest syntax nodes inside the subject |
| `tir` | the subject's TIR instructions, locals by name, type and flags (`[param read]`), fields by name |
| `own` | the subject's TIR after the ownership passes: read modes (`Read copy [$1]`), `Drop`, `IsUnique` and `Pin` statements, and the body's exit set |
| `wat` | the Wasm instructions that came from the subject: calls by name (`rt:retain_known`, `identity[i32]`, `println`), locals by source name or field path (`bodies`, `b.vx`), temporaries `t0`, `t1`... |
| `counts` | only the retains, releases and frees inside the subject, with what they count |
| `layout` | a type's fields, offsets, sizes, counted parts and cycle-capable bit |

Pick the narrowest view for the topic: `counts` for reference counting,
`layout` for representation, `wat` only when the instructions themselves
are the point. Never snapshot a whole module.

## Running And Blessing

- Run: `cargo test -p <crate> --test stages` (`hd_syntax`: use
  `cargo test -p hd_syntax --test all stages::`). A mismatch prints a line
  diff (`-` baseline, `+` actual); a missing baseline gives a blessing hint.
- Bless: `HD_BLESS=1 cargo test -p <crate> --test stages` overwrites the
  baselines and deletes those whose case or dump is gone (a plain run
  fails on them), leaving source cases untouched. A separate case file selects
  a different build, so one baseline has one build. **Read the baseline
  diff before committing**: it is the review of what the compiler now emits. A blessed regression is a bug
  you just recorded as correct.
- To explore a snippet through every stage, temporarily add a case under
  a crate's `tests/cases/` with `# @dump: tokens tree tir wat counts`,
  bless it with that crate's command, and remove the scratch case and its
  baselines afterward.

## Recording Bad Emitted Code

When a view shows code a hand-written version would not have (a
whole-value copy, a retain and release that cancel, a re-derived base, a
dead branch), add a case for it under a topic such as `counts/` or
`element-access/`, with a `# expected:` note, and link the issue that will
fix it. The fix then shows up as a reviewed baseline diff. Do not fix it by
special-casing the pattern in the emitter; fix the cause.

## Gates For A Compiler Change

Run these in the background, and grep each whole log for
`FAILED|panicked`; the count must be 0. Use `nice` and `-j 4`, with the
dev profile for tests.

- `cargo fmt --check`, and `cargo clippy --all-targets -- -D warnings`.
- Driver conformance in three modes: `cargo test -p hd_driver --test
  conformance`, again with `HD_CONFORMANCE_PIPELINE=optimized`, and again
  with `HD_CONFORMANCE_CHECKED=1`.
- `cargo test -p hd_driver`, the touched crates' tests, `cargo test -p
  hd_check --test stages`, and `bash spec/check.sh`.
- Add the rest of shared-rules.md "Git And Safety" for other touched
  paths: `hd_cli` tests, the web compiler, UI.
- Bless a baseline only for an intended change, and list each blessed
  file in the report.

## Perf Work: Attribute, Ceiling, Build

Every perf job runs these three steps in order. A step that shows the
job can't pay ends it: close the issue with the numbers, as a measured
miss. Don't build first and measure after.

1. **Attribute.** Count where the cost actually is before changing
   anything: a call counter keyed by call site or type, a profile, or
   TurboFan node counts per function. Check the ticket's guess against it,
   because tickets are often wrong. #909 guessed trait values, and the
   cost was one `List[string]` field. #920 guessed the inlined callees, and
   the cause was caller size. Post the breakdown in the report.
2. **Ceiling.** Build the cheapest hack that removes the attributed cost
   entirely. It may be unsafe, behind an environment variable, or only
   valid for the bench. Measure it at all three V8 tiers, interleaved.
   That is the most the real change can win. If it's under about 3% on the
   target program, or it only helps one tier while hurting another, stop.
   Examples: #704's count-only hack showed −14% on binary_trees before it
   was built; #901's add-or-remove-every-lookup run bounded it at 4–9%;
   #876's shadow-stack run showed returns into a slot only grew code.
3. **Build.** Only now write the real change, with fixtures and the
   gates, one cause per commit. Then report it with the table below.

Known measured misses not to retry without new evidence: inlining count
fast paths on V8 (#644, #910), removing inline targets to shrink TurboFan
graphs (#884, #920), a flag bit or fat views for strings (#901), returns
into a caller slot (#876), removing local shuffles for TurboFan time
(#926: `local.get/set/tee` build no TurboFan nodes, so they cost bytes
only).

## Perf And Size Report

Every perf change reports the same table, so results compare across jobs.

- **Every program, hd only, base and head:** the 11 study programs plus
  every one in `bench/runtime/progs` (20 in all today; count the directory). Use
  `node bench/study/run.mjs --no-refs` in release mode with `--engines node`,
  or interleaved runs of the two compilers' modules. Give each program's
  ms before and after, the geometric mean of head/base, and the machine
  load.
- **Three V8 columns** for any program whose Wasm changed: default tier,
  `--no-liftoff` and `--liftoff-only`. Wasm has no on-stack replacement,
  so one long call stays in Liftoff (#724; the owner leaves tiering
  alone). Name any default-tier change that comes only from tiering.
- **Interleaved base/head runs,** median of 15 or more, for each program
  whose Wasm changed. Programs whose Wasm is byte-identical (check md5)
  need no timing.
- **Sizes:**
  - hello.wasm in dev and release; unexplained growth blocks the commit;
  - each program's Wasm bytes;
  - the web compiler's gzip size (`node scripts/build-web-compiler.js`;
    2 MB is a hard cap).
- **Compile time:** json release compile, cold cache, interleaved. The
  compiler must not get slower.
- **Reference languages** (hand-written JS, MoonBit linear and wasm-gc,
  Rust, Go, Python, any added later) are not part of a per-commit report.
  The orchestrator runs them once after a round of merges (owner,
  2026-10-26): `node bench/study/run.mjs` with references,
  `node bench/runtime/run.mjs --release`, and
  `node bench/study/moonbit/compare.mjs`.
- **Goal:** beat hand-written JS on every program except
  `progs/string-build` (the deliberately naive `out + s` loop). Then beat
  MoonBit's linear backend clearly on every program at every tier (#866).
  Then MoonBit's wasm-gc backend (#913).
- Report integer milliseconds. Release builds wrap integer overflow, as
  Rust does.
