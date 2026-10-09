# Muse Task Queue

> **Active again, 2026-10-08 ~16:00 (owner: "muse is back").** Pull
> origin/main into your worktree first; S14, S15 and S16 were done by
> the orchestrator while you were away.

> **Owner, 2026-10-07 night:** Codex died; its open jobs moved here
> ("try offload work to muse"). Same rules as Codex had: small, checkable
> jobs; no conformance-fixture coverage jobs; never edit
> `compiler/crates/` product code (test files and generated files only
> where a job says so). The orchestrator reviews every commit.


This file is Muse's work queue. The orchestrating session adds jobs here.
Muse does them top to bottom, one commit per job, and deletes a job's
section in the same commit that finishes it. Git history keeps the record.
When the queue is empty, report that and wait.

## How To Work

- Work in **one** long-lived git worktree of your own, for example
  `git worktree add /private/tmp/muse-work -b muse/work origin/main`
  the first time, and reuse it for every job. Keep it; don't delete it.
- **Never write in the shared main checkout**
  `/Users/hd/code/test/hd-lang`: no edits, commits, or checkouts there.
  Your commits reach `main` only by `git push origin HEAD:main` from your
  worktree.
- Start each job from current main: `git fetch origin && git reset --hard
  origin/main` in your worktree, but only when it holds no unpushed work
  (check `git status` and `git log origin/main..HEAD` first).
- **Any change under `compiler/` (samples and bench inputs included) or
  to a conformance fixture runs the whole Rust suite before you push:**
  `cargo test -q --release --workspace` from `compiler/`. Crate tests
  read samples by line number (`hd_cli/tests/test_cmd.rs` asserts
  `compiler/samples/testing/cart.hd` lines), so the conformance gate alone
  is not enough; U1 turned CI red that way. If a crate test needs an
  update you can't make (`compiler/crates/` is not yours), stop and say
  so in the commit message instead of pushing.
- Install dependencies in your worktree with `pnpm install
  --frozen-lockfile`; never symlink or modify the shared `node_modules`.
- To finish each job:

  ```sh
  git fetch origin && git rebase origin/main
  pnpm run check
  git push origin HEAD:main
  ```

  If the push is rejected because main moved, fetch, rebase, rerun
  `pnpm run check`, and push again. Never force-push.
- A job is done only when its commit is on `origin/main`. A local commit
  is not done: push it.
- **No more fixture jobs** (owner, 2026-10-07: "kill all codex's fixture
  jobs"). Don't write conformance fixtures unless a job explicitly asks.
- **Fixture jobs also run the Rust parse tests:** after adding or
  changing any `spec/conformance/parse/` case, run `cd compiler && cargo
  test -p hd_syntax --test corpus`. The old Rust parser is being replaced,
  so a case it gets wrong goes into `compiler/KNOWN_FAILURES.tsv` (path,
  tab, expected code or `accept`) in the same commit. Never push with
  `cargo test` red (S1, f1a266a6, turned main red this way).
- While working, run only scoped checks: `node --experimental-strip-types
  test/run-portable.ts --changed` (or `--phase parse|type|runtime`) and
  `node --test --experimental-strip-types <the test files you touch>`. Run
  the full `pnpm run check` and `pnpm run test:ui` once, right before the
  push.
- Don't start a full `pnpm run check` or a full conformance run while
  another agent's full run is going (check the load average with
  `uptime`; above ~30, wait for it to drop). Two full runs at once push
  the load past 300 and make both time out.
- Never wait with an `until` or `while … sleep` loop: run a check in the
  foreground with a timeout. Use `gh run watch` if you ever need to wait for
  CI.
- Use pnpm only. Add files by name, never `git add -A`.
- Put the size-guard numbers in each commit message: `compileToWat` of the
  tiny program and the one-test program (today 16,350 B / 23 functions and
  3,143 B / 6 functions). Explain any growth.
- Print the CI run links once after a push and move on. Don't wait for CI.
- Never edit a fixture to make it pass. If the spec isn't clear-cut, skip
  the item, write the question under "Questions" at the end of this file,
  and go on.
- hd code style: an `i32` literal is `+N` (`total := +0`), not
  `let total: i32 = 0`, except in a group of annotated declarations,
  where the `i32` stays annotated so the widths read side by side.

- **Spec examples.** Adding, removing or moving a ```text block in a spec
  chapter renumbers every later block, so realign that chapter's rows in
  `spec/conformance/examples.tsv`, not only the new row. An earlier job missed
  this and broke `bash spec/check.sh` on main (fixed in c8c35a7e). After
  any rebase, rerun `bash spec/check.sh` before pushing; never push with
  it red.

## Don't Touch

- **`compiler/crates/`:** the orchestrator's lane. You may add
  `compiler/bench/` (Q4, P1).
- `src/`: the frozen prototype. Bug fixes only through a KNOWN_FAILURES
  row, never an edit.
- `lib/std/`, `guide/`, `website/`: read only, unless a job says so.
- `future-work/compiler/*.md`: only where a job says so.
- `spec/lang/*.md` rule text: only where a job says so. Fixtures under
  `spec/conformance/` and `test/portable/` are yours in the fixture jobs.

## Jobs

### L2. Std Gaps, Part 1: Backoff, retry_with!, Rng::from_seed, default()

Your L1 inventory found plain-hd items the spec declares and `lib/std`
lacks. Add them to `lib/std` (this job lifts the read-only rule for
these files only), with the exact signatures the spec states:

- `std.task`: `Backoff` (rule `std-task.backoff.decl-usize`) and
  `retry_with!` (`std-task.retry-with.decl`), a loop over `Clock.sleep!`
  written like the existing `retry!`.
- `std.random`: `Rng::from_seed`, as `std-random.rng.from-random` uses it.
- `std.ops`: the free `default()` that makes the `@default` fact
  (`std-ops.default.derive.marker`).

Where the spec leaves a detail open, take the simplest body that meets
the rule text and list it in your commit message; never add API the
spec does not name. Checks: `pnpm run check` (lib/std feeds the
prototype too), the full `cargo test -q --release --workspace` in
`compiler/`, and `HD_UPDATE_CONFORMANCE=1` for the driver conformance
test (the CONFORMANCE.md diff may only add cases; say which, e.g.
`runtime/valid/retry-with-backoff.hd`). Report hello-world size before
and after (it should not change). One commit; push. Timebox 45 minutes.

### L3. Std Gaps, Part 2: std.sys And std.net Declarations

Add `lib/std/sys.hd` with what the spec declares as plain hd: the `Sys`
trait, `SysError`, and the map-backed `MapSys` provider. Then
`lib/std/net.hd`: the `Net` trait, `NetError`, and the data types, but
only where each item is plain hd. A type that would need a host handle
(a live socket) is out of scope: list it in the commit message and
stop there; no host hooks, no `compiler/crates/` edits. Same checks and
size report as L2; fixtures to watch: `runtime/valid/map-sys.hd`,
`runtime/valid/net-own-provider.hd`. One commit; push. Timebox 45
minutes.

### T2. Test: Every KnownItems Field Names A Real Std Item (#69)

`compiler/crates/hd_resolve/src/known.rs` resolves `KnownItems` once per
run: the std items the compiler recognises (`eq`, `hash`, `walker`, …).
A std rename leaves a field at `DefId::NONE` and silently turns its
check off. Add one test file, `compiler/crates/hd_resolve/tests/known_items.rs`
(this job lifts the `compiler/crates/` rule for that new file only; no
product-code edits): build std through the public API the existing
tests use, then assert every field is set, naming the field on failure.
If a field is optional by design (say why in a comment you find in
`known.rs`), list it explicitly as exempt. Checks: `cargo fmt --check`,
clippy `-D warnings`, the full `cargo test -q --release --workspace`.
If the test finds an unset field today, don't fix `known.rs`: mark it
exempt with a `// BUG:` line and report it in the commit message. One
commit; push. Timebox 40 minutes.

### D2n. Design Text For The Derivation Instance Check (#17a, #17d)

Two commits landed: `2f8cf104` (the instantiated-template check at
each derivation opt-in: `member-not-derivable`) and the
`derive-field-missing-trait` commit after it (comparison traits report
at the field; derived newtypes get an implementation head; the base
type is checked through the solver; derived implementations repeat
their type's anchor slots). Read both diffs and write what they do into
the design text where derivation checking belongs (find it in
`future-work/compiler/checking-and-tir.md` or
`resolution-and-interfaces.md`): the walker/describer/source record, the
finish-time obligation pass, the driver's re-check of template methods
with a scratch buffer, the cache-key widening for template modules, the
one-error-per-field rule, and the open gaps (derivation blocks report at
the block header; derived newtype methods have no bodies yet, #117).
Mark any matching `reconciliation.md` row. Docs only; one commit; push.
Timebox 40 minutes.

### Y2. Proposal: The Intrinsic A Single-Pass `join` Needs (#112)

Owner decision: string appends get a builder now, a rope in phase 3.
`lib/std/text.hd` `join` is O(n log n) by halves, and `StringBuilder`
stores parts and calls `join`. Single-pass needs one primitive that
hd cannot write today. Propose the smallest one, with its signature as
a plain hd declaration (only the body is special; see
`@intrinsic("bytes_len")` in `text.hd`), e.g. a total-length
allocation plus a byte copy, or one `concat` over a list. For each
candidate: the hd code of `join` and `StringBuilder.build` on top of
it, the Wasm the emitter would produce (GC arrays, `array.copy`), its
cost (allocations, copies per byte), and what the spec must name
(intrinsics are spec-named). Recommend one. Add it as a section to
`future-work/compiler/representation-runtime.md` beside the owner's
decision. Research only: no lib, spec or compiler edits. One commit;
push. Timebox 40 minutes.

### Y3. Design Note: Derived Newtype Methods In Codegen (#117)

`@derive` on a newtype now has an implementation head, and no method
bodies: five `runtime/valid` fixtures stop at the Collect stage
(`derived-newtype`, `derived-debug-newtype`, `serde-std-writes`,
`json-typed-members`, `derive-members-of-data-and-enums`). Spec 09
`trait.derive.newtype.*` says the method applies the base type's method
to the wrapped values and rewraps the allowed `Self` positions. Read
`future-work/compiler/codegen.md` and the newtype representation (is a
newtype erased to its base at runtime?) and write a short design note:
where the bodies come from (a generated adapter per instance, or the
base implementation reused directly when the representation is the
same), which crate makes them (mono or emit), how `Self?`,
`Result[Self, E]` and `List[Self]` positions are rewrapped, and the
code-size cost per derived newtype. Add it to `codegen.md`. Research
only; one commit; push. Timebox 40 minutes.

### D2o. Design Text For Derive Codegen (#119)

Commit "Codegen: derived implementations instantiate their template (#119)"
landed. Read its diff and write it into the design text: in
`future-work/compiler/checking-and-tir.md` §4.13.9, how an opt-in's checked
template methods become the derived implementation's methods
(`hd_tir::wire::map_ids`, Structure calls choosing the derivation); in
`future-work/compiler/codegen.md` §12.3, the generated `Structure` bodies
(`hd_check/src/structure.rs`: facts, name, walk, describe, build, one hidden
method per handle) and what is not carried yet (facts, doc comments, shared
constructor data); and in §13.2 the refined A1 rule: an own type parameter
stays exact when a data type, tuple, function type, trait value or projection
in the item's signature or body holds it. Docs only; one commit; push.
Timebox 40 minutes.

### P1. Profile The New Compiler (After S4; Standing Job)

Owner, 2026-10-07: "you write the code, codex do the profiling. move
fast". The orchestrator writes `compiler/crates/*`; you measure it.

- Each time a new compiler commit lands on main (`git log -- compiler/`),
  profile it: `hd run` / `hd build` on the samples and on the generated
  bench (`cargo run --release -p hd_driver --example bench N`) at 3,000 and
  30,000 lines, cold, warm, body edit, signature edit, comment edit.
  Use `samply` or `cargo flamegraph` if installed, else `perf`-style
  timers already in the driver's counters.
- Write `audit/compiler/profile-<date>-<short hash>.md`: per-stage time,
  the top 10 hot functions with their share, allocations if measurable,
  and for each hotspot one line: **implementation slip** (name the fix)
  or **architecture issue** (name the design section). Compare with the
  previous report.
- Only flag what is atrociously bad (order-of-magnitude, superlinear,
  or a stage that dominates for no design reason). Perf is eyeballed,
  not gated, and micro-tuning is out of scope.
- You may add benchmark inputs or a harness under `compiler/bench/`
  (new directory). Never edit `compiler/crates/`; the orchestrator
  applies fixes from your report.
- Push each report within 30 minutes of starting it.

## Questions

(none)
