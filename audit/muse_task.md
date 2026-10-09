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
- `spec/`: **frozen (owner, 2026-10-08 night: "freeze all spec", "no
  change, log them all")**. No edits to rule text, std chapters,
  conformance fixtures or indexes, whatever a job or an earlier answer
  says. Log every spec or fixture issue you find as a row in
  `audit/compiler/diagnostic-notes.md` and go on.

## Jobs

### R23. Design Note: Local Declarations In Bodies (#123, Design Text)

18 programs stop at Body on a local `data` declaration, 4 on a local
named `fn`, 2 on a local `impl` (your R22 plan). Read the spec rules for
items inside bodies (`spec/lang/02-grammar.md` statements,
`03-names-and-scopes.md` scopes, `07-functions.md`, `08-data-and-enums.md`)
and add to `checking-and-tir.md` how a local item is named and lowered
(a hidden module-level item with a mangled path? captures forbidden?),
how its methods and impls are found, and what Emit sees. Timebox 45
minutes.

### R24. Design Note: Small Body Gaps (#123, Design Text)

From your R22 plan: `**` (14), open range patterns (9), full range `..`
(6), compound assignment (4), tuple patterns with a rest (4),
assignment targets (7), mixed positional and named variant fields (4),
`RestType` in a body (5). For each, the spec rule IDs and the TIR it
lowers to (a desugar into existing nodes where the design allows), one
row each in a table in `checking-and-tir.md`. Timebox 45 minutes.

### R25. Design Note: Type Names Used As Values (Design Text)

7 programs use a type name as a value (`Mailer`, `Priced`, `list`,
`Set`, `T`), 2 call a type name (R22). Read the spec rules (names,
`types.*` for type-as-value, constructors, `type-used-as-value`) and say
which of these are valid (and what they mean) and which must be
errors, citing rule IDs; then the lowering for the valid ones. If the
spec does not settle a case, log it in
`audit/compiler/diagnostic-notes.md` instead of deciding. Add to
`checking-and-tir.md`. Timebox 30 minutes.

### R26. Turn Your R5 Report Into Task Rows (Research)

`audit/compiler/triage-missing-errors.md` groups 250 accepted invalid
programs into 21 causes. For each cause not already matching an
orchestrator task number, write one proposed task line (title in the
form "[check] what is missing (N programs)", the spec rule IDs, the
stage) at the end of that file, biggest first. Read only; timebox 20
minutes.

### R27. Profile The Runtime Of The Samples (Research)

P1 measures compile time. Measure run time: build each sample under
`compiler/samples` and the six programs in `compiler/bench/runtime/progs/`
with the current compiler, run them (`hd FILE.wasm` or the bench's
`run.mjs`), and compare with the last runtime report in git history
(`runtime-vs-node-*.md`, `runtime-hotspots-*.md`). Flag only
order-of-magnitude problems, with the hot function if you can find it.
Write `audit/compiler/runtime-<date>-<short hash>.md`. Timebox 45
minutes.

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

### Answers (orchestrator, 2026-10-08 evening)

- **Q-L3: option 1.** The language rules (`trait.dyn.keyword`,
  `types.trait.value.dyn-required`) stand. Add the missing `dyn` in
  `spec/std/net.md` (this answer lifts the spec rule for those three
  signatures only) and in `runtime/valid/net-own-provider.hd`, keep
  `lib/std/net.hd` with `mut dyn`, and land it with L3.
- **Prototype regressions:** `src/` stays frozen, and TS checks no longer
  gate lib/std changes (only `src/` or website changes run `pnpm run
  check`). Do not edit `src/` (no auto-declare fix, no whitelist lines).
  Record each prototype regression or blind spot (the two Inspectable
  fixtures, `map-sys.hd`, `net-own-provider.hd`) as a row in
  `test/portable/KNOWN_FAILURES.tsv` with a finding tag `PROTO-STD`.
- **`hd_driver/tests/checker.rs`:** update its std module count (38 to
  40) in the same commit; this answer lifts the `compiler/crates/` rule
  for that test file only.
- **`retry_with!` and `Backoff`: the owner lifted the batch-73 hold
  (2026-10-08 evening).** Push them with L2, and delete the
  `runtime/valid/retry-with-backoff.hd` row (finding `RETRY-WITH`) from
  `test/portable/KNOWN_FAILURES.tsv` in the same commit. Report hello and
  the 3,000-line bench build before and after, so the std.task to
  std.time cost is on record.

- **Q-F1 (owner, 2026-10-08 23:50): leave all 122 fixtures as
  `fn main() -> i32`; F1 is closed.** A non-pub `main` is an ordinary
  function (`module.entry.private-main`), so the fixtures are valid; the
  partial commit 3b3a9181 was reverted because it broke
  `fn.body.void-final`. The compiler bugs it exposed are the
  orchestrator's. Lesson for fixture jobs: when a job's recipe conflicts
  with a spec rule, stop and ask instead of changing the recipe.

### Q-R3: bound-receiver shape and the bare-path prerequisite (R3, 2026-10-09)

1. **Bound references: `Closure`-wrap (recommended).** §13.11 builds a
   bound reference as a `Closure` node wrapping the adapter with the
   receiver captured, so the TIR schema (`ItemRef`: item + type args,
   no env slot) does not change. The alternative is extending `ItemRef`
   with an env operand, which touches the schema, the wire format and
   every scan. Recommend the wrap.
2. **Bare paths need a Check task first (recommended order).** The 14
   "a path used as a value" Body stops never reach `item_value`;
   routing `PathExpr` through it with expected-type instantiation
   (`fn.type.generic.*`) is a Check change the adapter task depends on.
   Recommend queuing it before or with the Emit adapter work.

### Q-R4: two spread semantics edges (R4, 2026-10-09)
1. **Copy-update reads happen at construction (recommended).** The
   spread expression is evaluated first, then the explicit field
   expressions in source order, then the new value is built — so a
   field copy reads the source as it is after the explicit expressions
   ran. If an explicit expression mutates the source through another
   reference, the copy sees the mutated field. The alternative
   (snapshot every field when the spread is evaluated) costs a full
   copy even when a later explicit expression overwrites, and the spec
   pins only the evaluation order, not the read time. Recommend
   construction-time reads.
2. **A spread vararg is passed through, not copied (recommended).**
   `f(xs...)` at a vararg fills it with the `xs` value itself, so the
   callee observes the caller's list (separate arguments still collect
   a fresh list). The alternative (copy on spread) makes the two forms
   indistinguishable but costs a copy the table's "as its collected
   value" does not ask for. Recommend pass-through.

### Answers (orchestrator, 2026-10-09 00:50)

- **Q-R3.1:** `Closure`-wrap a bound reference; `ItemRef` keeps its
  schema.
- **Q-R3.2:** agreed; the orchestrator splits #46 into the Check task
  (bare paths through `item_value` with expected-type instantiation)
  and the Emit adapter task, in that order.
- **Q-R4.1:** construction-time reads, as recommended. The spec fixes
  only the order (`expr.update.spread-first`), not when copied fields
  are read; that gap is logged in `audit/compiler/diagnostic-notes.md`
  (the spec is frozen).
- **Q-R4.2:** pass-through, as recommended.
### Q-R6: ineligible trailing-block callee (R6, 2026-10-09)

A trailing block applies only when resolution finds a callable with
an eligible final parameter (`grammar.call.trailing-block.eligible`),
but no diagnostic code names the failure (e.g. `total := len(items):`
where the callee takes no callback). The spec is frozen, so a new
code needs an owner decision; the fallback is reusing `type-mismatch`
on the block. Recommend asking the owner for a code
(`trailing-block-ineligible` or reuse) before the Check task lands.

### Q-R7: iteration-during-mutation and impl resolution (R7, 2026-10-09)

1. **Snapshot `used` at iterator creation (recommended).** Entries
   appended while an iteration runs stay invisible; entries removed
   before their yield are skipped via the live tombstone check. The
   alternative (a live end-cursor that picks up appends) makes the
   yield set depend on interleaving the spec does not describe.
   Recommend snapshot + tombstone checks.
2. **Emit re-selects the key impls (recommended).** The intrinsic node
   carries only the key type; Emit asks the solver for the `Hash`/`Eq`
   impls at the instance's substituted types instead of Check
   recording them on the node, so the TIR schema does not change.
   Recommend Emit-side selection.

### Q-R8: default inlining policy and derived-fact globals (R8, 2026-10-09)

1. **No special-casing for default bodies in the inliner
   (recommended).** Trivial constants inline; anything larger goes
   through the default thunk unless bounded inlining (§12.6) takes it
   under its normal budgets. The alternative (always inline small
   defaults, or never inline them) is a size/speed knob for measured
   tuning, not for this design. Recommend the neutral rule.
2. **One lazy global per derived-fact instantiation (recommended).**
   A derived fact's value can differ per type arguments, so sharing
   one global across instantiations risks wrong reads; per-(fact,
   args) globals with a shared getter are always correct, at one
   global plus flag each. Recommend per-instantiation globals.

### Q-R9: generated-impl bounds and transparent @from (R9, 2026-10-09)

1. **Mirror @derive derived-bounds (recommended).** A generic error
   type whose message or members use T needs bounds like
   impl[T < Display] Display for E[T]; the compiler should walk the
   message/member types exactly as @derive derived-bounds does.
   Recommend the same rule, not unbounded impls (which would fail at
   each use) and not T < Error-everywhere.
2. **Transparent @from is allowed and orthogonal (recommended).** A
   transparent type's one member may also carry @from: the From
   conversion is generated and the message/cause still forward to the
   member. The alternative (reject the combination) has no rule
   behind it - cause.one limits @from/@source only.
   Recommend allowing it.

### Q-R10: property case counts and row failures (R10, 2026-10-09)

1. **Default cases = 100 (recommended).** No rule states a default
   count; the only example passes cases=8 explicitly. Recommend
   cases = 100 by default on both it_prop and it_prop_with
   (Hypothesis's example count), overridable per call.
2. **Run all rows (recommended).** Each it_each element is its own
   case name[i]; a failing row should not hide later rows.
   Recommend running every row and reporting each, rather than
   stopping at the first failure.

### Answers (orchestrator, 2026-10-09 01:20)

- **Q-R6:** a new code is a spec change, and the spec is frozen. Logged
  in `audit/compiler/diagnostic-notes.md` and queued for the owner; until
  then the compiler keeps an existing code for an ineligible trailing
  block. Nothing for you to do.
- **Q-R7.1:** snapshot at iterator creation plus the live tombstone check,
  as recommended; the spec gap (mutation during iteration) is logged.
- **Q-R7.2:** Emit-side key-impl selection; the TIR schema stays.
- **Q-R8.1 and Q-R8.2:** the neutral inlining rule; one lazy global per
  derived-fact instantiation. As recommended.
- **Q-R9.1:** mirror `@derive`'s derived bounds. **Q-R9.2:** allowed; no
  rule rejects transparent `@from`.
- **Q-R10.1:** the spec states no default case count; logged and queued
  for the owner (recommendation 100). Not decided here.
- **Q-R10.2:** run every row; each row is its own case, so one failure
  must not hide the others.

### Q-R13: sub-word canonical form and profile keying (R13, 2026-10-09)

1. **Producers keep sub-word values canonical (recommended).** Every
   narrowing sequence ends with mask plus sign/zero-extend, so an i8
   in a local is always a sign-extended i32 and consumers use native
   ops. The alternative (extend at each use) saves two instructions
   per narrowing op and pays per consumer instead, while risking
   exactly the class of bug in R2 section D. Recommend canonical.
2. **Code entries are per profile (recommended).** Debug-checked and
   release-wrapped sequences for the same source must never share a
   cache entry; the pipeline/code key carries the profile.
   Recommend making that keying explicit if it is not already.

### Q-R14: unavailable slots and widening form (R14, 2026-10-09)

1. **Trap stub for dynamically unavailable slots (recommended).** A
   supertrait method that cannot work dynamically
   (trait.dyn.member.unavailable) still shapes its vtable slot, but
   Check rejects every call through it (trait.dyn.bound.available),
   so the slot fills with an explicit-panic stub. The alternative
   (shrinking the vtable) couples the shape to availability.
   Recommend the stub: unreachable by construction, loud if reached.
2. **Widening is free (recommended).** The widened value reuses the
   (payload, child vtable) pair as-is instead of materializing a
   parent-vtable pair. The alternative costs a struct build per
   widening for no observable difference. Recommend free.

### Q-R15: default pipeline and test name section (R15, 2026-10-09)

1. **Fast-build pipeline by default everywhere (recommended).** Only
   --release asks for the optimized pipeline; debug, test, and REPL
   builds use the fast one. The alternative (optimized by default
   with an opt-out) trades build latency every run. Recommend
   fast-by-default.
2. **Test builds keep the name section (recommended).** Only release
   omits the standard name section; test output and failure stacks
   stay readable. Recommend keeping it everywhere else.

### Q-R17: of declaration and targets wording (R17, 2026-10-09)

1. **Declare TypeId.of in lib/std (recommended).** The spec shows
   impl TypeId with pub fn of[T < Inspectable]() but lib/std/inspect.hd
   has no of; all eight programs wait on that declaration as well as
   on the call path. Recommend adding the plain-hd declaration
   (owner/orchestrator lane; this job could not touch lib/std).
2. **Read targets as covering associated calls (recommended).**
   expr.call.generic.targets names only module functions, while
   fn.ref.generic covers Type::member::[T] references; nothing names
   the call form Type::member::[T](args). Recommend treating the
   combination as covering it (no frozen-spec change), rather than
   ruling the eight programs out of scope.

### Answers (orchestrator, 2026-10-09 02:20)

- **Q-R13:** canonical sub-word values (producers mask and extend);
  code entries keyed per profile, made explicit with #109.
- **Q-R14:** a trap stub for dynamically unavailable slots; widening
  reuses the (payload, child vtable) pair.
- **Q-R15:** the fast pipeline everywhere except `--release`; only
  release omits the name section.
- **Q-R17.1:** the spec already shows `TypeId::of`; declaring it in
  `lib/std` belongs to the compiler task #140 (orchestrator's), not to a
  research job.
- **Q-R17.2:** covered, with no spec change: `fn.ref.generic` gives
  `Type::member::[T]` as a reference and `fn.ref.call` makes a reference
  followed by an argument clause an ordinary call.

### Q-R19: unmatched keys and diagnostic spelling (R19, 2026-10-09)

1. **Unmatched full keys stay internal errors (recommended).** Past
   Check, every ProviderGet key is covered by construction, so a miss
   means an internal invariant broke, as today. The alternative
   (a user-facing diagnostic) would report Check-approved code as
   broken. Recommend keeping the internal error.
2. **Spell the full key in diagnostics (recommended).** Wherever a
   provider is named (missing-key and collision reports), print
   Repo[User], not Repo. The alternative (trait-only names) repeats
   the confusion this design removes. Recommend full spelling.

### Q-R20: loop value joins take the union too (R20, 2026-10-09)

**Recommend yes.** A loop-else value joining with break-carried
function values is a least-common-type site like any other, so
break values with rows should union exactly as if/match arms do.
The alternative (first-break-wins row) repeats the too-few-providers
bug this design removes. The spec lists branches, arms, and
closure/fn results but not loop joins; recommend the same rule
rather than a carve-out.

### Q-R21: fix-it cost and promotion ties (R21, 2026-10-09)

1. **Compute unavailable-trait matches diagnostics-only with a fuel
   cap (recommended).** Listing traits that would match but are not
   available means solving in traits the module never imported; do it
   only for the error path, capped, never memoized. The alternative
   (always computing it) taxes every successful multi-trait lookup.
   Recommend diagnostics-only.
2. **Same-depth promotion ties are ambiguous (recommended).** The
   promoted candidate wins at the smallest depth, but nothing breaks
   a tie between two promotion paths at that depth. Recommend
   ambiguous-method, consistent with trait-vs-trait ties.

### Answers (orchestrator, 2026-10-09 03:10)

- **Q-R19:** both as recommended (an unmatched full key stays an internal
  error; diagnostics spell the full key, `Repo[User]`).
- **Q-R20:** not now. The least-common-type table
  (`types.lct.uses`) does not list loop-`else`/`break` joins, so the
  compiler adds no union there; the gap is logged in
  `audit/compiler/diagnostic-notes.md` (the spec is frozen).
- **Q-R21.1:** diagnostics-only, fuel-capped, never memoized.
- **Q-R21.2:** the spec does not say how two promoted methods at the same
  smallest depth tie (`names.method-lookup.promoted-candidate`); logged.
  The compiler reports `ambiguous-method` there, the conservative error.
