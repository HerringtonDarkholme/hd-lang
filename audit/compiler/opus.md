# Compiler Review: Bugs And Hacks (Task #230)

Status: open findings of a bounded, read-only review of `src/` at `5ba7393e`. Nothing here is accepted behavior or an owner decision.

## Summary

Open findings: O-03 (high: written type applications skip their declarations' generic bounds; task #280) and O-06 (whole-state speculation copies; perf F1). The others were fixed and deleted: O-05 and O-12 by 524f6816, O-08, O-09 and O-11 by bb5937bd.

Each repro ran with `timeout 60 node bin/hd.js check|run|test FILE` from a scratch folder `scratch-230/` in the worktree, so output paths keep that prefix.

## Findings

### O-03: type-argument bounds of written types are not checked, except a top-level `Map` key

- Severity: high. Kind: bug (with a name-based special case).
- Where: `src/checker/shared.ts:1309` and `src/checker/context.ts:1260` (both test `nominal?.name === "Map"` at the top level only).
- A written type such as `Box[P]` never checks `P` against `Box`'s declared bound. The only bound check on a written type is a hard-coded `Map` key test, and it skips nested types such as `List[Map[P, i32]]`.

Repro (`bound.hd`):

```text
data P:
    x: i32

data Box[T < Display]:
    value: T

fn nested(bs: List[Box[P]]) -> i32:
    bs.len()

fn count(ms: List[Map[P, i32]]) -> i32:
    ms.len()

pub fn main() -> void $ Console:
    println("${nested([])} ${count([])}")
```

`node bin/hd.js check` reports `ok`. A top-level `fn top(b: Box[P])` is also accepted.

- Expected: `unsatisfied-trait-bound` at `Box[P]` and at `Map[P, i32]` (`trait.bound.unsatisfied`, `types.map-key.declared-bound`).
- Fix: check every type application's arguments against its declaration's bounds in the shared written-type validator; then `Map` needs no special case.

### O-06: overloaded trait calls copy the whole checker state per candidate

- Severity: medium. Kind: resource.
- Where: `src/checker/call-speculation.ts:24` (`speculate`), called from `src/checker/expression-calls.ts:829` and `:1449`.
- Each trial snapshots the whole reachable object graph of the checker, including all locals and HIR checked so far. A call with several candidate instantiations, such as `C::from(x)` with `From[i32]` and `From[string]`, therefore costs time in proportion to the function body so far.

Repro: a generated `main` with N lines `let cI = C::from(I)` and `total = total + cI.v`, where `C` implements `From[i32]` and `From[string]`. `hd check` wall time:

| N | two `From` impls | one `From` impl (control) |
| ---: | ---: | ---: |
| 400 | 1.3 s | 0.4 s |
| 800 | 4.4 s | 0.8 s |
| 1600 | 16.6 s | 2.4 s |

Doubling N quadruples the time with two impls.

- Expected: checking time near linear in program size. No rule states a bound; this is a cost problem.
- Fix: make the trial a pure check that returns its HIR and diagnostics, or journal only the state a trial changes. Do not deep-snapshot the checker.

## Suspected, not reproduced

- `src/repl.ts:517` (`substitution`) replaces type parameters one at a time with a regex. If an argument's text is another parameter's name, as `E[B, i32]` for `enum E[A, B]` with a user type `B`, a later pass rewrites it again. Not run through the REPL.
- `src/checker/map-keys.ts:93` keeps the program's `Hash`/`Eq` implementations in a module-level `let`, set per `checkProgram`. A second check that started inside the first would read the wrong table. No such re-entry exists today, and an in-process run of 9 programs twice in reversed order showed no output differences.
- `src/checker/expression-calls.ts:259` still intercepts every `List` method call named `append`. A user trait method `append` implemented for `List[T]` would be shadowed by it. TODO item 20 already removes `append`, so this is not reported separately.
- `src/checker/member-lookup.ts:303` lets any std function see any std private member, whatever its module. No std code misuses this now.

## Not re-reported

These were seen and are already tracked: std diagnostic locations (TODO 5), `List.append` (TODO 20), the two `SCALAR_BOUNDARY` sets in `src/compiler.ts:206` and `src/emitter/host-providers.ts:28` (A07), package joining and privacy (A01, P2), the busy-polling `hd run` (F-555), and nested-closure check time (F-604).
