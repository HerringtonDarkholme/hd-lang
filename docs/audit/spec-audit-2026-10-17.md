# Spec Audit, 2026-10-17

This pass sorts every row of
[diagnostic-notes.md](diagnostic-notes.md) (192 rows, read at `ebcb9a06d`)
and the condensed list in issue #183. Each row was checked against the
current compiler (`target/debug/hd`, dev build of `ebcb9a06d`). A row whose
"Today" might be stale got a probe under `target/spec-audit/` (not
committed). Rows are named by their Date and Where columns.

| Group | Meaning | Rows |
| --- | --- | --- |
| D | Needs an owner ruling | 23 rows, as 22 questions in 6 batches |
| B | Code right, spec wording lags | 36: 33 applied, 1 moved to D, 2 to C |
| C | Message, code or host quality only; no spec rule involved | 60 |
| A | Resolved; deleted from diagnostic-notes.md in this commit | 76 |

The probes also found one new compiler bug, listed first under C: a
`while ... else` loop whose `break` values are functions with different
rows crashes the checker.

## Owner Answers (2026-10-17)

All 22 questions are answered. Each becomes a spec edit plus, where marked, a compiler change; issue numbers are added as they are filed.

| Q | Answer |
| --- | --- |
| D1 | Always an error: a generic call cycle whose type arguments grow is rejected; layout never decides. Checker change. |
| D2 | `private-type-leak` for an associated type binding of an impl whose trait and target are public. Checker change. |
| D3 | A copy-update snapshots `src` when the spread is evaluated, before any explicit field. Compiler change. |
| D4 | Iteration panics when growth compacts the entries during a live iteration. Runtime change. |
| D5 | Infer `never` for a parameter only `never` values meet. Spec sentence. |
| D6 | One copy of shared data per variant; a shared field whose type names a type parameter is a named error (replacing `unsupported`). Holds while payload stays out of scope in the constructor expression. |
| D7 | One requirement key: a binding the supertrait list fixes counts in key identity, as for trait values. Checker change. |
| D8 | A trait default is checked once, against the trait. Spec sentence. |
| D9 | An `@error` message is a plain string literal only; a prefixed string is `invalid-error-marker`. Checker change. |
| D10 | Repeated fact types are checked in list literals only; the spec says so. Spec sentence. |
| D11 | `println` at the top level of a non-entry module reports both `missing-requirement` and `suspension-forbidden-context`; the fixture is split. Checker change. |
| D12 | `hd test --update` rewrites a failing in-code snapshot's `expect` literal in place, as for doc tests. CLI change. |
| D13 | `duplicate-associated-binding` also inside one bound. Checker change. |
| D14 | `duplicate-data-pattern-field` covers variant payload patterns. Checker change. |
| D15 | Repeated parameter names in an enum header or a variant are `duplicate-field`. Checker change. |
| D16 | A tuple selection past the end is the missing-field code. The owner also renames `unknown-data-field` to `unknown-field` everywhere. |
| D17 | One general manifest code for any `hd.toml` error without a specific code, with file and line. CLI change. |
| D18 | Relax the row-printing rules: each key once, in a stable order; an alias expansion may be part of the message. Function types in messages still gain their rows (compiler work). |
| D19 | Delete `module.repo.private-pattern` until hd uses a checksum log or proxy. |
| D20 | Add a CLI session fixture format for the REPL (inputs file plus expected transcript). Conformance README and runner change. |
| D21 | Detect a fact that reads itself and fail at once with `fact-evaluation-failed`. Runtime change. |
| D22 | `impl Suspend[T] for X` reports `unknown-trait` (Suspend is a built-in data type), with a message that says so. Checker change. |

## D. Questions For The Owner

### Batch 1: Language Semantics

**D1. Polymorphic recursion that the compiler can build.**
The spec says a generic function that calls itself with ever larger type
arguments is always an error. The compiler builds and runs
`fn wrap[T](value: T, n: i32) -> i32` calling `wrap([value], n - 1)`: it
shares one body for every reference-sized `T`, so the set of instances
stays finite. The spec also says a program cannot tell shared bodies from
specialized ones. Today, whether this program compiles depends on that
layout choice.

1. Always an error (Recommended). The checker reports a generic call cycle
   whose type arguments grow, as Go reports an instantiation cycle; layout
   never decides. Consequence: a checker change, and `wrap([value], n - 1)`
   is rejected on every engine.
2. An error only when the instance set is infinite, as today. Consequence:
   the spec rule is reworded, and the shared-layout rule gets an exception,
   since validity then depends on how the compiler lays out `T`.

Refs: `types.generic.polymorphic-recursion`, `types.generic.sharing`,
`types.generic.instantiation-depth.error`; probe `p/p83.hd` checks with 0
errors.

**D2. A private type behind a public trait impl's associated type.**
`impl Make for Maker: type Out = Hidden` compiles when `Make` and `Maker`
are public and `Hidden` is module-private. Code in other modules then gets
`Hidden` values from `maker.make()`, a type it cannot name. The leak check
lists parameters, results, fields, bounds and the rest, but no associated
type binding. A `pub` method of a private type needs no check, since no
one outside can reach it.

1. Make it `private-type-leak` (Recommended). Add associated type bindings
   of an impl whose trait and target are both public to the leak check.
   Rust still rejects this case as a hard error (E0446). Consequence: a
   checker change; such impls must make the bound type `pub`.
2. Allow it, as today. Consequence: other modules hold values of a type
   they cannot name. They can call its public methods through the trait.

Refs: `module.vis.signature.coverage`; probe `p27/` checks with 0 errors.

**D3. When a copy-update reads the fields it copies.**
In `D { ...src, f: e }` the spread is evaluated first, but the spec does not
say when the copied fields are read. If `e` changes `src` through another
reference, the result shows it. Today the compiler reads the copied fields
at construction, after the explicit field expressions.

1. Read them when the spread is evaluated (Recommended). The copy is a
   snapshot of `src` before any explicit field runs, which matches "the
   spread is evaluated first". Consequence: a compiler change that reads
   the copied fields before the explicit expressions.
2. Read them at construction, as today. Consequence: one spec sentence;
   a change made by an explicit field expression shows in the copy.

Refs: `expr.update.spread-first`; Muse R4, #122.

**D4. A map that compacts while it is iterated.**
Iteration panics when the map's length differs from the length it started
with. A removal and an insertion between two `next` calls keep the length.
If that insertion fills the entry arrays, growth compacts and renumbers
the entries, and the iterator may skip live entries or yield the new one.
It stays memory-safe.

1. Panic when growth compacts the entries during a live iteration
   (Recommended). Consequence: one check on the growth path; no live entry
   is ever skipped silently, and the length rule stays as it is.
2. Count every insertion and removal and panic on any change, as Java's
   `modCount` does, for every collection. Consequence: the simplest rule,
   but one more counter in every list and map.
3. Document the edge: in that case some entries may be skipped. Consequence:
   no code change; programs may silently miss entries.

Refs: `flow.for.invalidate`, `flow.for.invalidate.panic`; Muse R7, #124.

### Batch 2: Language Semantics

**D5. Inferring a type parameter that only `never` values meet.**
`id(panic("x"))` with `fn id[T](x: T) -> T` checks today with `T` as
`never`. The spec does not say whether such a parameter is `never` or
cannot be inferred.

1. Infer `never`, as today (Recommended). It matches "`never` is assignable
   to every type" and the least-common-type rule for all-`never` values.
   Consequence: one spec sentence.
2. Report `cannot-infer-type`. Consequence: a checker change; the user
   writes `id::[i32](panic("x"))`.

Refs: `types.never.assignable`, `types.lct.never.all`, `trait.inspectable.not.never`; probe `p/p155.hd`.

**D6. Shared data of a generic enum.**
The spec says shared constructor data belongs to the variant and is
evaluated once, but names no instantiations. Today `Box[string]` and
`Box[i32]` read one copy per variant, set up at module initialization. A
shared field whose type names a type parameter, as `items: List[T]`, has
no single type, and the checker reports `unsupported`.

1. One copy per variant of the declaration, as today, and a shared field
   whose type names a type parameter is an error (Recommended).
   Consequence: one rule and a named error code in place of `unsupported`.
2. One copy per instantiation. Consequence: `List[T]` shared data works,
   but each instantiation evaluates its own copy at initialization.

Refs: `data.shared.per-variant`, `data.enum.generic`; #285d.

**D7. Requirement keys that differ only by a binding a supertrait fixes.**
Under `trait PriceFeed < Feed[Item = i32]`, the trait values `dyn PriceFeed`
and `dyn PriceFeed[Item = i32]` are one type. The requirement keys
`$ PriceFeed` and `$ PriceFeed[Item = i32]` stay two keys today, so a
provider given for one does not cover a function that names the other.

1. One key (Recommended). A binding that the supertrait list fixes counts
   when keys are compared, as it does for trait values. Consequence: one
   rule and a checker change in key comparison.
2. Two keys, as today. Consequence: one sentence saying so; users must
   write the key the same way everywhere.

Refs: `req.key.binding.identity`, `trait.dyn.binding.identity`; #239.

**D8. Checking the default `rest` of a walker.**
`Walker`'s default `rest` calls `self.member(h, items)` with a `List[T]`.
A walker whose `member` adds a bound, as `member[F < Eq]`, cannot meet it
with the default's unbounded `T`. Fixtures define such walkers without
`rest` and expect them to be valid. The Rust compiler checks a trait
default once, against the trait. The old TS compiler checked it per
implementation and rejected those fixtures.

1. Check a trait default once, against the trait, as today (Recommended).
   Consequence: one sentence; the fixtures stay valid.
2. Check it per implementation. Consequence: such walkers must write
   `rest` (std walkers already do); fixtures such as
   `typed-derivation-walk.hd` change.

Refs: `annot.walk.rest.default`; #173c, #182.

### Batch 3: Smaller Semantic Choices

**D9. A prefixed string as an `@error` message.**
The spec says a message is an ordinary interpolated string. Today
`@error(raw"bad")`, with a user `@str_prefix` function `raw`, is accepted,
and the error prints `bad`. (The notes said it was rejected; that changed.)

1. Only a plain string literal (Recommended). A prefixed string runs a
   user function each time the error is shown. Consequence: the checker
   reports `invalid-error-marker` for it again.
2. Accept any string literal, as today. Consequence: the message rule says
   so.

Refs: `annot.error.message.string`, `annot.error.form.argument`; probe
`p/p44.hd`.

**D10. A repeated fact type that a list expression brings.**
A member line may name a list, as `x += shared_list`. The checker sees only
a literal's own elements, so a repeated fact type inside a named list is
not reported, at check time or at run time, and `facts.find` returns the
first one. The spec makes any repeat an error.

1. Check literal elements only, and say so (Recommended). Consequence: one
   sentence; a named list with a repeat stays unreported.
2. Also check at run time: building the facts panics on a repeat.
   Consequence: a new panic site in fact building.

Refs: `annot.line.duplicate`, `annot.line.right-typed`; #285c.

**D11. `println` at the top level of a non-entry module.**
That code must be requirement-free, so `println` there is
`missing-requirement`. The spec also says `println` follows every rule of
`block_on`, which makes a direct call in non-entry initialization
`suspension-forbidden-context`. The compiler and the fixture give the
first error only.

1. One error, `missing-requirement` (Recommended). Consequence: the
   `println` rule gets an exception for that context; no code change.
2. Both errors. Consequence: a checker change, and the fixture must be
   rewritten, since a case may hold one error only.

Refs: `module.console.println-block-on`, `req.drive.block-on.direct`,
`req.drive.block-on.forbidden-contexts`; fixture
`non-entry-top-level-println.hd`.

**D12. What `hd test --update` does to an in-code `snapshot`.**
The spec's Why says a literal `expect` lets an update run record a new or
changed expectation in place. No rule says so, and today an update run
fails the snapshot and leaves the source as it was. `hd help test` already
says `--update` rewrites each failing snapshot. Doc test snapshots are
rewritten by their own rule.

1. An update run rewrites a failing snapshot's `expect` literal in place,
   as it does for doc tests (Recommended). Consequence: a CLI change.
2. Compare only. Consequence: the Why sentence goes, and `hd help test`
   is corrected.

Refs: `module.testing.snapshot.literal`, `cli.test.doc.update`; probe
`p89/`.

### Batch 4: Error Codes

**D13. A bound whose binding contradicts its trait's supertrait list.**
`T < Prices[Item = string]` under `trait Prices < Supplier[Item = i32]`
can never be met. It is reported only when the two bindings sit in two
bounds of one parameter (`duplicate-associated-binding`), not inside one
bound.

1. Report `duplicate-associated-binding` here too (Recommended).
   Consequence: one rule and a checker change.
2. Report nothing at the bound, as today. Consequence: each use of the
   function fails with an unrelated bound error.

Refs: `trait.binding.once.elaborated`, `trait.binding.super.merge`,
`trait.binding.super.conflict`; probe `p/p113.hd`.

**D14. A variant payload field matched twice in one pattern.**
`.Rect(w, w=1)` names one payload field twice. The rule for data patterns
gives `duplicate-data-pattern-field`; no rule covers variants, and the
checker reports `unsupported`.

1. Extend the data-pattern rule and its code to variant payload patterns
   (Recommended). Consequence: one rule and a checker change.
2. A new code for variants. Consequence: one more code to learn.

Refs: `flow.match.data.fields.duplicate`; probe `p/p138.hd`; #256.

**D15. Two parameters of one name in an enum header or a variant.**
`enum E(a: i32, a: i32)` and `Two(x: i32, x: i32)` check with no error
today. Function parameters that share a name report `duplicate-binding`
since #337; data fields report `duplicate-field`.

1. Both are `duplicate-field`, as a payload name that repeats a shared
   parameter already is (Recommended). Consequence: one rule and a checker
   change.
2. Both are `duplicate-binding`, as function parameters are. Consequence:
   the same, with the parameter code.

Refs: `data.field.unique`, `data.shared.payload-names`; probe
`p/p145.hd`.

**D16. Selecting a tuple element past the end.**
`value._1` on an `(i32,)` reports `unknown-method` today, and the fixture
expects it. A selection with no argument clause is a field read, and every
other missing field is `unknown-data-field`.

1. `unknown-data-field` (Recommended). Consequence: a checker change and a
   fixture edit.
2. `unknown-method`, as today. Consequence: a rule names that code for
   tuple selection.

Refs: `expr.member.field-read`, `expr.tuple.select-underscore`; fixture
`tuple-index-out-of-range.hd`; #187.

### Batch 5: Tooling And Messages

**D17. `hd.toml` errors that have no code.**
Bad `[capabilities]` keys or values, and a package that a workspace
manifest neither lists nor excludes, report with `"code":null` in JSON. The
unlisted-member error also has no file or line.

1. One new general code for any `hd.toml` error that no rule names a code
   for, with the manifest's file and line (Recommended). Consequence: a
   README code, rule edits, and a CLI change; agents can match on it.
2. Keep them code-less, as usage errors are. Consequence: no change; the
   unlisted-member error still gets a position and its two fix-its as
   compiler work.

Refs: `cli.cap.table.keys`, `cli.cap.value.unscoped`,
`cli.mode.member.unlisted`; probe `ws/`.

**D18. How diagnostics print requirement rows.**
The spec asks that an inferred union row print in element order, and that
an alias's expansion show as a label under the alias. Today a function type
in a message prints with no row at all (`found fn() -> i32`). Union rows
print sorted. An alias expansion is appended to the message text, and only
for the function's own row.

1. Relax the rules: rows print with each key once, in a stable order, and
   an expansion may be part of the message (Recommended). Consequence:
   two rule edits; function types in messages still need their rows,
   as compiler work.
2. Keep the rules. Consequence: the compiler records element order at each
   join and prints labels.

Refs: `req.row.union.literal.diagnostics`, `req.row.alias.diagnostics.expanded`;
#48, #175.

**D19. The private-path pattern rule.**
The spec says a private-path pattern keeps host paths away from a checksum
log or proxy, but gives the pattern no form. `hd` talks to no checksum log
or proxy at all, so nothing needs to stay private yet.

1. Delete the rule until `hd` uses a log or proxy (Recommended).
   Consequence: one rule fewer; nothing changes in behavior.
2. Define a form now, such as an environment variable like Go's
   `GOPRIVATE`. Consequence: a spec design and CLI work with no effect yet.

Refs: `module.repo.private-pattern`, `module.sum.only`; #250.

**D20. A fixture format for REPL sessions.**
The spec owns fixture formats and has none for a session, so the REPL
rules are tested only by Rust tests named by rule ID.

1. Add a CLI session fixture: an inputs file and the expected transcript
   (Recommended). Consequence: a conformance README section and a runner
   change; REPL rules get portable cases.
2. Keep the Rust tests. Consequence: no change; another implementation has
   no REPL cases.

Refs: `cli.repl.*`; #17.

### Batch 6: Found While Applying Group B

Group B was re-checked at `86d03d467` with probes under
`target/spec-audit-b/` (not committed). These two items did not hold as
written.

**D21. A fact that reads itself.**
A fact whose evaluation reads the same fact again recurses until the stack
runs out, and the panic is a plain `stack-exhausted`. Any other panic in a
fact is `fact-evaluation-failed`, naming the fact and the original
category, as `annot.fact.eval.panic-runtime` requires. Stack exhaustion
escapes that rule today.

1. Detect the re-entry and panic at once with `fact-evaluation-failed`,
   naming the fact (Recommended). Rust's `OnceCell` panics on a reentrant
   init the same way. Consequence: a runtime change, one flag per fact.
2. Recurse, as today, and report `stack-exhausted` unwrapped.
   Consequence: `annot.fact.eval.panic-runtime` gets an exception.

Refs: `annot.fact.eval.panic-runtime`, `annot.fact.eval.lazy`; #285a;
probes `i32.hd`, `i32b.hd`.

**D22. The code for implementing `Suspend[T]`.**
Owner decision #14 made `Suspend[T]` a built-in type, applied as
`req.protocol.builtin` (B35). `impl Suspend[i32] for X` still reports
`sealed-trait-implementation`, a code named for sealed traits.
`impl Other for X` with another data type `Other` reports `unknown-trait`,
"not a trait", and so does `dyn Suspend[i32]`. The new rule keeps today's
code.

1. Keep `sealed-trait-implementation` (Recommended). It tells the writer
   that only the standard library implements it. Consequence: none.
2. Report `unknown-trait`, as for any data type after `impl`.
   Consequence: a checker change; the fixture
   `user-suspend-implementation.hd` changes its code.

Refs: `req.protocol.builtin`, `trait.sealed.user-impl`; probes `i35.hd`,
`i35b.hd`.

## B. Spec Edits (Code Right, Wording Lags)

Each item names the file, the rule, and the edit. "New item after X" means a
new numbered item with the given ID; renumber the list.

1. **`..` after a missing directory** (2026-10-15). Applied in c914df255. `spec/cli/command-line.md`,
   new item after `cli.cap.scope.path.resolved`:
   `r[cli.cap.scope.path.access] Once the grant check passes, the access
   itself resolves the path as the operating system does, on every engine.
   So reading data/missing/../a.txt fails with NotFound when data/missing
   does not exist.`
2. **Child that does not read its input** (2026-10-15). Moved to C (Hosts): the Node host returns `.Err` on `EPIPE`. `spec/std/process.md`,
   new item after `std-process.helper.stdin-default`:
   `r[std-process.stdin.unread] When the child exits or closes its standard
   input before reading all of stdin, the rest is dropped. The call still
   returns .Ok with the child's output and status.` (Go's `exec` does the
   same.)
3. **REPL member lines for an earlier type** (2026-10-13). Moved to C (Compiler Bug): a later derivation does not see the block.
   `spec/cli/command-line.md`, Redefinition, new Note after
   `cli.repl.redefine.impl`: "A derivation built for an earlier input keeps
   the facts it saw. A later input's block adds facts that later
   derivations see, and `find` returns the first; a repeated fact type
   across inputs is `duplicate-fact`."
4. **REPL `Debug` added late** (2026-10-13). Applied in c914df255. Same place, one more Note
   sentence: "An implementation of `Debug` added after a value of its type
   was shown does not change how later values of that type show; a type
   not yet shown uses it." (Owner accepted, live-execution.md.)
5. **REPL example** (2026-10-12). Applied in c914df255, with `use std.path.{Path}`: `Path` lives in `std.path`, not `std.fs`. `spec/cli/command-line.md`, example
   after `cli.repl.panic.declarations`. Before: `hd> read_text!("notes.txt")   # reads ./notes.txt`.
   After: `hd> read_text!(Path("notes.txt"))   # reads ./notes.txt`, and
   the `use` line above it becomes `hd> use std.fs.{Path, read_text}`.
6. **REPL top-level bang calls** (2026-10-12, owner decision). Applied in c914df255. New item
   after `cli.repl.host.default-profile`: `r[cli.repl.input.driver] An
   input's top level is a suspension driver context, as main!'s body is, so
   an input may make bang calls. Its row is inferred under the default
   profile.` `spec/lang/10-modules.md` `module.init.script-not-driver`,
   append: "A REPL input is a driver context, by cli.repl.input.driver."
7. **REPL value line** (2026-10-12). Applied in c914df255. `cli.repl.value`, append: "An
   expression input is an input whose only top-level item is an expression
   statement that binds nothing. The REPL shows no value line when that
   expression's type is `void` or `never`."
8. **Field missing several derived traits** (2026-10-08). Applied in c914df255.
   `spec/lang/09-traits.md` `trait.derive.field-missing-trait`, append: "A
   field that lacks several of the derived traits gets one error, naming
   the first of them in the `@derive` list."
9. **Workspace diagnostic paths** (2026-10-08). Applied in c914df255. `cli.json.diagnostic.file`,
   append: "At a workspace root, `file` is relative to the workspace root,
   as `lib/src/lib.hd`, so two members' files stay apart."
10. **`hd new --pages`** (2026-10-09). Applied in c914df255; nothing linked to the old heading. `spec/cli/command-line.md`. Delete
    the Note under "Planned: GitHub Pages Workflow" and rename the heading
    to "GitHub Pages Workflow". The compiler writes the workflow (probe
    `p65/`), and the fixtures `new-pages`, `new-pages-existing` pass.
11. **Loop value joins** (2026-10-09). Table row done in 86d03d467 (#380); the `flow.loop.else.type` wording applied in c914df255. `spec/lang/04-type-system.md`, the
    `types.lct.sites` table, new row: "Value-producing `for` or `while` |
    every reachable `break` value, and the `else` suite's value | [Control
    Flow](06-control-flow.md#r-flow.loop.else.type)". `spec/lang/06-control-flow.md`
    `flow.loop.else.type`. Before: "must produce one compatible loop result
    type". After: "join by the least common type". The compiler crashes on
    this site today (C, first item).
12. **Unsolved parameter as an expected type** (2026-10-09). Applied in c914df255. `types.lct.sites`
    lead sentence, append: "An expected type that is a type parameter the
    enclosing call has not solved yet counts as none: the site joins first,
    and the call then solves the parameter from the result."
13. **Trait value without the impl** (2026-10-09). Applied in c914df255. `spec/lang/09-traits.md`
    `trait.dyn.convert`, append: "A conversion without one is an error.
    Error: `type-mismatch`."
14. **`pub` on a local declaration** (2026-10-09). Applied in c914df255; the fixture `parse/invalid/pub-local-function.hd` already existed. `spec/lang/03-names-and-scopes.md`
    `names.local.no-pub`, append: "Error: `syntax-error`." Add a fixture;
    the parser reports "'pub' is not accepted on local declarations" (probe
    `p/p84.hd`).
15. **`dbg` order with spread** (2026-10-09). Applied in c914df255. `spec/lang/10-modules.md`
    `module.dbg.body.print`, append: "A call with a spread or `values=`
    evaluates every argument first, then prints each element. A call with
    positional arguments prints each one as it is evaluated."
16. **`**` on a type parameter base** (2026-10-09). Applied in c914df255. `spec/lang/05-expressions.md`,
    new item after `expr.power.mixed`: `r[expr.power.type-parameter] For a
    base of a type parameter, its bound picks the rule: T < Integer takes
    the integer rules and T < Float the float rules. Any other base type
    parameter is an error. Error: type-mismatch.`
17. **`()` against a rest-only tuple type** (2026-10-09). Applied in c914df255.
    `expr.tuple.rest.collect`, append: "So `()` against `(List[i32]...,)`
    is a tuple whose rest is empty." Add a fixture.
18. **Shortest float display ties** (2026-10-09). Applied in c914df255. `spec/lang/04-type-system.md`
    `types.display.float`, append: "When two shortest strings are equally
    close to the value, it writes the one with an even last digit." Both
    engines do (a66c3c1c8; Node probe prints `2.9802322387695312e-8`).
19. **`Fn[Args, O, $ R]` called with separate arguments** (2026-10-09). Applied in c914df255.
    `spec/lang/05-expressions.md`, new item after the spread-inputs table:
    `r[expr.call.spread.inputs.only] A value of type Fn[Args, O, $ R] takes
    its inputs only as one spread. A call with separate arguments, a
    trailing block, or more positional values is an error. Error:
    type-mismatch.`
20. **`TypeId` name of `void`** (2026-10-09). Applied in c914df255. `spec/lang/09-traits.md`,
    `trait.typeid.name.tuple` row. Before: "`(A, B)`". After: "`(A, B)`; the
    empty tuple, `void` included, is `()`".
21. **`dyn Sub` and supertrait bindings** (2026-10-09). Applied in c914df255. `trait.dyn.binding.complete`,
    append: "A binding that a supertrait list fixes counts as bound, so under
    `trait Sub < Super[Item = T]` the types `dyn Sub` and `dyn Sub[Item = T]`
    are one type."
22. **Projection that no bound reaches** (2026-10-09). Applied in c914df255. `spec/lang/09-traits.md`,
    new item after `trait.assoc.ambiguous-type`: `r[trait.assoc.no-type] A
    projection whose bounds select no associated type declaration is an
    error. Error: unknown-associated-type.` (probe `p/p112.hd`).
23. **`dyn` over a `Self`-fixed binding** (2026-10-10). Applied in c914df255. New item after
    `trait.dyn.binding.complete`: `r[trait.dyn.binding.self-fixed] A
    binding to Self that a supertrait list fixes, as in trait Summable <
    Add[Out = Self], counts as bound. The trait value does not carry it, so
    Out stays a projection.`
24. **`facts_of` and valueless decorators** (2026-10-10). Applied in c914df255.
    `spec/lang/14-annotations.md` `annot.facts-of.declared`, append: "Its
    signature is `pub fn facts_of[F](f: F) -> Facts`." `annot.facts-of.result`,
    append: "Compiler decorators attach no value, so the result leaves out
    `@derive`, `@error`, `@from`, `@source`, `@intrinsic`, `@annotate`,
    `@num_suffix` and `@str_prefix` lines." (Typed facts missing from the
    result is C work.)
25. **A requirement `hd add` is adding** (2026-10-10). Applied in c914df255. `cli.dep.invalid`,
    append: "A requirement that the command is adding has no line yet, so
    its error has no file or line."
26. **SSH fallback** (2026-10-10, owner approved, #280). Applied in c914df255. `cli.dep.repo-url`.
    Append: "When the HTTPS fetch fails for any reason, `hd` tries
    `ssh://git@HOST/REPO` without prompts. Within one run it remembers,
    per host, the scheme that worked. When both fail, one `fetch-failed`
    names both URLs. `insteadOf` rewrites apply first."
27. **`...=` into a readonly target** (2026-10-10). Applied in c914df255. `spec/lang/04-type-system.md`
    `types.path.store-embedded`. Before: "which must have mutable access.
    Otherwise the store is an error. Error: `mutable-upgrade`." After:
    "A readonly target is `readonly-root`, as for any store. When the copy
    itself has only readonly access, as from a readonly source with a `mut`
    composite field, the store is an error. Error: `mutable-upgrade`."
    (probe `p/p139.hd`).
28. **Embedding depth beside a cycle** (2026-10-10). Applied in c914df255. `spec/lang/08-data-and-enums.md`,
    `data.embed.depth` row, append to its text: "A type that reaches a cycle
    without being part of it gets no `embedding-cycle`; its depth counts
    along chains that never repeat a type."
29. **One trait twice as a supertrait** (2026-10-10). Applied in c914df255. `trait.super.unique`.
    Before: "must not list one trait twice". After: "must not list one
    trait twice with the same arguments; `Convert[i32] & Convert[string]`
    lists two bounds".
30. **Tuples at joins** (2026-10-11). Applied in c914df255. `types.lct.conversions`, append:
    "Permission weakening applies inside tuple elements too, as assignment
    allows, so `(mut User, i32)` and `(User, i32)` join to `(User, i32)`."
31. **Pseudo-version base** (2026-10-11). Applied in c914df255. `spec/lang/10-modules.md`
    `module.version.pseudo`, append: "The closest earlier tag is the
    greatest version tag, with the package's tag prefix, among the commit's
    ancestors, as Go chooses it."
32. **A fact that reads itself** (2026-10-11). Moved to D as D21: the panic escapes `fact-evaluation-failed`. `spec/lang/14-annotations.md`,
    new item after `annot.fact.eval.panic-runtime`: `r[annot.fact.eval.reentrant]
    A fact whose evaluation reads the same fact again recurses until the
    stack runs out.`
33. **Constructing `PollContext`** (2026-10-11). Applied in c914df255. `req.poll-context.construction`,
    append: "Its one field is private, so a construction in user code is an
    error. Error: `private-member`."
34. **Property registration rows** (2026-10-11). Applied in c914df255. `spec/lang/10-modules.md`,
    the `it_prop` and `it_prop_with` signatures above
    `module.testing.reg.calls`: add `$Q` to the generics, write the body as
    `prop: fn!(T) -> R $ Q`, and the result as `-> void $ Q`, as
    `lib/std/testing.hd` declares them.
35. **`Suspend[T]` is a built-in type** (2026-10-12, owner decision #14). Applied in c914df255 as `req.protocol.builtin`; `req.protocol.sealed`, `req.protocol.implementers` and `trait.sealed.suspend` retired. Its error code is D22.
    `spec/lang/09-traits.md`: delete the `trait.sealed.suspend` row.
    `spec/lang/11-requirements-and-suspension.md`: `req.protocol.sealed`
    becomes "`Suspend[T]` is a built-in type that no package implements; code
    writes it without `dyn`, as `mut Suspend[i32]`." Merge
    `req.protocol.implementers` into it. In `req.poll-context.user` and
    `req.combinator.user`, replace "the sealed `Suspend` trait" and
    "`Suspend[T]` is sealed" with "a built-in type". Read the whole
    `Suspend[T]` Protocol section first; its declaration block may change
    too.
36. **Module-qualified type names** (2026-10-17). Applied in c914df255. `spec/lang/03-names-and-scopes.md`,
    new rule beside the type-name lookup rules: `r[names.type.qualified] A
    type may be named through a module in scope, as json.Json after use
    std.json, by the rules of expr.name.qualified. A missing declaration is
    unknown-import, and a private one private-import.` Today
    `json.Json` checks, `json.Nope` is `unknown-import`, and
    `std.json.Json` is `unknown-name` for `std`, as in expressions (probes
    `p/p201b.hd`, `p/p201c.hd`).

## C. Compiler, Diagnostic And Host Work

Filed as GitHub issues under #183 group C, each item re-probed at e7f630651
(2026-10-17). "Dropped" means it no longer reproduces or has nothing left to
do; "moved to D" means a spec decision is needed first.

### Compiler Bug Found By This Pass

- `while ... else` with `break` values of different rows: dropped, fixed by
  86d03d467 (#380); probe `p69.hd` checks clean.
- REPL facts after a type's first derivation: #381.

### Checker Gaps

- Long `else if` chain: #382.
- Header goal out of fuel: #383.
- `f32` literal rounds twice: #384.
- Case folding for module paths: #385.
- Host panics during a fact: #386.
- Same-name items in one message: #387 (it is worse than untested: the
  names print identically, unqualified).
- Typed facts missing from `facts_of` results: dropped, no failing input
  could be built (a typed decorator on a function reads back through
  `find::[T]()`; `facts-of-read.hd` passes).

### Messages And Hints

- Unimported or misspelled `it_prop`, `itt` in `tests:`: #388.
- `(1: i64)`: #389.
- `collect` into a list of the wrong element type: #390.
- Mutation after `let m = f()` where `f` returns `mut T`: #391.
- Std names used without `use`, `StringBuilder`, `parse_f64`,
  `@arbitrary.with`: #388.
- `mixed-derived-law`: #390.
- `ambiguous-method` hint: #390.
- Row parameters print as `owner#index`: dropped, a function type prints
  without its row, and no probe printed `owner#index`.
- Test and task module paths: #393.
- Span of `syntax-error` for an unknown `$.name`: #389.
- Near names, `append`, `Map.new`, `copy`, `into`, `print`: #388.
- `mutable-receiver-required`: #391.
- Wrong spellings, `loop:`, `/// doc`, `use` inside a function: #389;
  `eprintln` or `Error` without `use`: #388.
- Suspension hints: #392.
- Map key panic: #393.
- `List + List`: #390.
- Stdlib-audit hints: `Display` of an optional #390; `unwrap` and
  `Map::new()` #388.
- `unsatisfied-trait-bound` prints a `mut` the value lacks: #390.
- `mixed-signedness`: #391.
- Bound mismatch with defaulted literals: #390.
- Syntax errors from other languages: #389.
- `T < Add` with `acc + item`: #390.
- Doc test in an executable package: #393.
- One-argument `IndexSet`: #390.
- Mid-character string slice: #393.

### Cascades

- `?` in `main!() -> void`: #394.
- Index key of the wrong type: #395.
- Missing `use std.convert.From`: #396.

### Fix-Its

- `unused-derivation-fact`: #397.
- Test or task file beside its directory: moved to D. The JSON fix-it
  shape (`cli.json.fix.edit`) replaces bytes in a file and cannot move one.
- Fix-its in text output: moved to D. The spec fixes no text layout for a
  diagnostic, so the form of a text fix-it is a decision. The row stays in
  diagnostic-notes.md.

### CLI And Std

- `hd test` case that cannot run yet: dropped, no action (the note itself
  says keep it rare, no spec change). The row stays in diagnostic-notes.md.
- `dbg-in-dependency`: #398.
- Declared defaults in derivations: already open as #44.

### Hosts

- Node file grants: `..` after a link: #399 (reproduced, node reads the
  file, wasmtime refuses). Link swapped between check and open: #400.
- Process input a child does not read: #401.
- External process spawning without a shebang: dropped, the test exists
  (`no_shebang_executable_uses_rust_standard_spawning_behavior`; its flake
  is #375).
- Pipe-worker thread creation failure: dropped, the process provider no
  longer uses scoped threads (it runs on the async runtime).
- Native resolver family hints: #406.
- Node Http: TLS alerts classified as `Connect`: #402.
- Node Http: response headers cut at 1000 pairs: #403 (reproduced).
- Node Net: high-bit read maxima: #404.
- Native DNS cancellation: #405.
- Non-Unix output-cap cancellation: dropped, the process provider uses
  async pipes on every platform.
- Windows directory grants on Node: dropped, `createGrant` takes the path
  separator (host/core.mjs).
- Linux nested cgroup CPU quotas: #408.
- Native syscall cancellation limits: dropped, an OS limit; the row stays in
  diagnostic-notes.md.
- Darwin descriptor exhaustion edge: dropped, owner accepted it as a
  documented limit; the row stays in diagnostic-notes.md.
- `std-process.cancel` on Windows, the job-assignment window: #407.

## A. Resolved (Deleted From diagnostic-notes.md)

"Fixture passes" means `KNOWN_FAILURES.tsv` is empty, so every conformance
case passes; where useful the probe or commit is named too.

- 2026-10-12 CLI fixtures `doc-out`, `doc-main-page`: both write `let sum = +0` now.
- 2026-10-10 Completed test suspension root: fixed with emitter regressions (row's own record).
- 2026-10-10 Closed UDP handle with a denied destination: owner-approved fix landed on both engines.
- 2026-10-08 `@derive(Eq, Ord)` without `PartialOrd`: one error now (probe `p/p40.hd`).
- 2026-10-08 Derivation block walker failure at the header: `annot.walker.obligation.error` reports at the opt-in, which is the block header.
- 2026-10-08 `trailing-block-position` message: now "a trailing callback block is not valid inside brackets" (probe `p/p49.hd`).
- 2026-10-08 `unknown-trait` on an impl head: one error, no cascade (probe `p/p50.hd`).
- 2026-10-08 `x is x`: one error (probe `p/p51.hd`).
- 2026-10-08 Command-line errors in JSON: usage errors stay `code:null` as the Exit Status note says; no not-implemented manifest section remains.
- 2026-10-08 `hd.toml` diagnostics at column 1, toolchain 0.0.0: matches "an error at its line"; no change (M7).
- 2026-10-08 `join` single primitive: `concat_all` landed (#317, #179).
- 2026-10-08 `unlisted-entry` fix-it at end of file: the spec names no line; implementation choice.
- 2026-10-08 Fixture `cli/ambiguous-import`: now uses the specified fields only (probe `p61/`).
- 2026-10-08 Library use of a dev dependency: fixed in O14.
- 2026-10-08 Fixture `cli/command-help`: now lists the Commands table.
- 2026-10-09 `crlf-line-endings.hd`: fixture passes.
- 2026-10-09 Promoted methods at one depth: `ambiguous-promoted-member` at the declaration by `names.conflict.error` (probe `p/p70.hd`).
- 2026-10-09 `nonfinal-spread.hd` against `positional-spread-duplicates-vararg.hd`: both pass.
- 2026-10-09 `positional-after-spread.hd`: fixture passes.
- 2026-10-09 `error-find-concrete-receiver.hd`: fixture passes.
- 2026-10-09 `@error(transparent)` on a variant without one payload member: `decorator-target-kind` now (probe `p/p74.hd`).
- 2026-10-09 `private-member` from an integration or doc view: reported (probes `p77/` doc test passes, `tests/sum.hd` gets `private-member`).
- 2026-10-09 `literal-var-method-missing.hd`: fixture passes.
- 2026-10-09 Shared compiled cache: `HD_BUILD_ID` is in `toolchain_key`, LRU eviction in 9b60ea1b.
- 2026-10-09 `denied-capability` from `hd test`: no position, as `hd run`'s refusal; matches "in the same way".
- 2026-10-09 `instantiation-too-deep` truncation: a design-doc wish, not a spec rule.
- 2026-10-09 `trailing-block-ineligible` added against the freeze: the owner kept it, and the compiler reports it (fixture passes).
- 2026-10-09 Fixture `nonlocal-impl.hd`: one error now (probe `p86/`).
- 2026-10-09 Fixture `resource-disposed-result.hd`: `pub message` now; cfbd9f1e4 added the host-trait case.
- 2026-10-09 Property body rows: std declares `$Q` and the fixture passes; the spec text is B34.
- 2026-10-09 REPL value display through `dbg_text`: the compiler lowers it, and the REPL exists.
- 2026-10-09 Hidden `dbg_show` helper: nothing in the spec changes.
- 2026-10-09 `dbg` of generic values: implementation-defined by `module.dbg.value.generic`; per-instance printing is allowed.
- 2026-10-09 Float `**` rounding: tested against about 35,000 correctly rounded references; keep (M7).
- 2026-10-09 `power-mixed-numeric-types.hd`: fixture passes.
- 2026-10-09 `callable-value-short-binding-store.hd`: fixture passes.
- 2026-10-09 `facts_of(f).find`: `facts-of-read.hd` passes.
- 2026-10-09 Entry-run panics with an empty message: no std site passes an empty message now.
- 2026-10-09 CLI fixture `test-outcomes` text step: amended to the `FAIL`/`PANIC` form.
- 2026-10-09 Messages that name `void`: no rule on message spelling; `()` is kept.
- 2026-10-10 Fixtures `memory-fs-directories.hd`, `std-types-debug.hd`: both pass.
- 2026-10-10 Fixture `suspension-forbidden-context.hd`: recategorized as a runtime panic (b5fdeaf74).
- 2026-10-10 Fixture `folder-cycle-leaf-folder.hd` (M4): returns `Error` now; fixture passes.
- 2026-10-10 Fixture `range-inclusive-no-end.hd`: the error is at 4:11, on the marker line.
- 2026-10-10 Drift `script-test-init-requirement`: fixture passes.
- 2026-10-10 Drift `enum-default-requires-provider`: fixture passes.
- 2026-10-10 Drift `missing-requirement-cold-construction.hd`: fixture passes.
- 2026-10-10 `cli.cache.hash` with an edited entry: the rule compares the record; Go's trust model.
- 2026-10-10 Fetch progress line: implementation choice; no rule needed.
- 2026-10-10 `fetch-failed` message: implementation choice beyond the rule's host path.
- 2026-10-10 Text form of a dependency command's error: the fixture fixes it, and it passes.
- 2026-10-10 Output of `hd add` and `hd update`: superseded by `cli.dep.add.report`.
- 2026-10-10 Tidy that leaves no entry: implementation choice.
- 2026-10-10 Parser messages with no TS text: wording is the implementation's.
- 2026-10-10 A tab inside brackets: both texts state the fault.
- 2026-10-10 `annot.line.right.not-list`: typed (#285c); fixture passes.
- 2026-10-10 `data.embed.depth.self` order across modules: which type carries the error is detail the spec need not fix.
- 2026-10-11 `-(2 + 2)` defaulting to `usize`: follows the text; the user writes `-4`, so no rule for this case.
- 2026-10-11 `types.generic.infer.join`: the spec now names `no-common-type` for a trait-value conflict.
- 2026-10-11 `T?` and `T??` join to `T??`: covered by "injection adds one layer".
- 2026-10-11 Unbounded recursion on Node: fixed in `compiler/host/core.mjs`.
- 2026-10-11 Easier `hd add`: spec and implementation landed (#307a, #307b).
- 2026-10-11 Releases and SemVer: tracked as issue #28.
- 2026-10-11 `trait.own.module.error` and inherent impls: `trait.own.module.inherent-target` covers them.
- 2026-10-11 `cli.dep.add.report.pseudo`: the rule speaks of the printed line only.
- 2026-10-11 `cli.dep.update.query` output wording: implementation's.
- 2026-10-11 `cli.dep.query.order` all-digit prefix: the order's stated consequence.
- 2026-10-11 Same-name packages' `TypeId` names: printed name apart from identity (6408a056c).
- 2026-10-11 `Inspectable` at Collect: the checker stays strict; nothing observable.
- 2026-10-11 Member and variant facts evaluate at handle build: `annot.fact.eval.lazy` now says so.
- 2026-10-11 `dyn Any` carries identity in codegen: not observable; no spec change.
- 2026-10-11 Member-line elements checked once at check time: fixed (#312).
- 2026-10-11 `expected-i32-found-usize.hd`: fixture passes.
- 2026-10-12 Fixture `folder-cycle-leaf-folder.hd` (#8): fixture passes.
- 2026-10-12 Fixture `resource-disposed-result.hd` (#9): fixture passes.
- 2026-10-16 Poisoned match result into a generic call: one error now (probe `p/p200.hd`).
