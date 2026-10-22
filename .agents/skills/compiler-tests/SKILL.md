---
name: compiler-tests
description: Test the hd compiler with stage fixtures and benchmarks - write a focused .hd case, pick the view that shows one topic (tokens, tree, tir, own, wat, counts, layout), bless and review generated baselines, record bad emitted code with an expected note, and measure performance with bench/study (about 300 ms per program, --quick, hd-only before/after). Use when adding or changing a compiler test, checking what a stage emits for some code, or measuring a perf change.
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

## Benchmarks

`bench/study/` has each program in hd, JavaScript, Go and Python, tuned
so hd on V8 takes about 300 ms. Results and the Wasm GC-era column are in
[bench/README.md](../../../bench/README.md).

- `node bench/study/run.mjs --quick`: hd on V8 only, 3 runs, about a
  minute. Use it before and after every perf change.
- `node bench/study/run.mjs`: hd on V8 and wasmtime plus the references,
  median of 5, about 7 minutes. Run it once after a batch of perf work.
- Report integer milliseconds. For a perf issue, measure **hd only, before
  and after**, on the programs it targets, on the same machine; note the
  load if it is high.
- Owner target: every program at most 1.2x its Wasm GC-era time, then
  Node parity.
- Release builds wrap integer overflow (`--release`), as Rust does.
