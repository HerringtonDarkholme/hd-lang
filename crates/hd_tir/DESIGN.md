# New Compiler Design: Body Checking And The TIR

Status: design record, checked against the code at 3209d690. "Implementation
differs" notes mark where the code falls short of the design. The code and
the spec win.

Part of the [compiler design](../../docs/compiler/ARCHITECTURE.md).

The detailed type-checking design is
[crates/hd_check/DESIGN.md (type checking)](../hd_check/DESIGN.md); this
file keeps the summary of body checking and the TIR definition.

### 4.13 Body Checking

#### 4.13.1 Task Structure Per Module

| Phase | Task | Serial or parallel | Work |
| --- | --- | --- | --- |
| M1 | `ModulePrep(m)` | serial within the module | full parse if needed; module scope; lower private headers; the module's local impl tables; infer the results and rows of private callables that omit them |
| M2 | `Body(m)` | parallel across modules; inside a module, bodies run as one batched parallel iterator ([crates/hd_sched/DESIGN.md](../hd_sched/DESIGN.md#62-executors) granularity rule) | check each body against frozen tables |
| M3 | `ModuleFinish(m)` | serial within the module | write the init summary, sort diagnostics, write the `check` entry |

Implementation differs: result and row inference runs at the start of
`Body(m)`, serially, and it checks every private callable that omits its
result type or row. On a pool thread, `Body(m)` checks the other function
bodies in batches (`bodies_in_parallel` in `crates/hd_driver/src/lib.rs`).
Each batch has a context forked from the module's (`hd_check::fork`),
and what each body adds folds back in source order.

A body's reports depend on whether the module already holds an error
(`cannot-infer-type`, an error-recovery `never`). So the batches run only
up to the module's first error, and the bodies after it are checked in
order as before. This is worse: most bodies of ordinary code omit their
row, so M1 checks them on one core.

**Each body is checked exactly once per run** (crates/hd_check/DESIGN.md
(type checking) rule TC-4). A body checked early for its result type or
row is final; `Body(m)` takes it at its source position instead of
checking it again. The module's top-level statements are checked first,
because unannotated top-level bindings get their types there
([crates/hd_check/DESIGN.md (type checking) §1.7](../hd_check/DESIGN.md#17-body-tasks-and-the-exactly-once-rule)).

**What a one-function edit costs.** An edit rechecks the whole edited
module: M2 over every body dominates. Diagnostics print before any entry
is published, and the TIR is written only for an error-free module.

Planned, not built: per-body reuse inside the module's entry. A body
whose token hash, module scope, private headers, inferred results and
recorded reads are unchanged would take its TIR and diagnostics from the
previous entry, so a one-function edit costs one body.

**Omitted result types.** A private function or inherent method without
`-> T` is registered (`hd_check::results`). Every read of a callee's
signature goes through `Ck::with_result`, which checks a pending callee
right there, depth first, with its own `DiagBuf`. The result is a join
variable (`join_target`), so the final value and the `return` operands
take the least common type and the row union. A callee already in
progress closes a cycle: `recursive-function-needs-result-type`,
reported once at the cycle's first function in source order; the call is
poison. `ModuleFinish` writes the inferred results into the module's
items. A top-level binding's literal class closes at the end of its
statement (`types.literal.local.statement`), so a body that a top-level
statement reaches sees the binding's type.

**M3: inferred rows.** A row is final before the module's items are
written: codegen picks a call's providers from the callee's signature
row. A private function or inherent method without a `$` clause joins
the same registry (local `fn`s are closures, §4.13.4). Its body's bottom
frame is `Inferred`: it takes each key and row parameter the body uses
outside its own `$.with` blocks. Solving rows before any caller reads
them keeps pending rows out of types and the TIR, so no row tier and no
provider patch is needed.

- **Order.** Rows never order checking. A body is checked early only for
  an omitted result, or when a closure, a test case or a function value
  needs the row now (then also the unchecked bodies its row waits on).
  A body checked during the top-level statements sees the annotated
  bindings of later statements, declared ahead, so its read of one is
  `top-level-read-before-initialization`, not an unknown name. A call of
  a callee whose row is open records an edge (`RowEdge`): the callee, the
  call's type arguments by owner, and the keys of the `$.with` blocks
  around the call. Under a written row, the call records a deferred
  entailment check instead. A closure call or function value of a group
  member still being checked is a structured "not implemented".
- **Strongly connected groups.** When a body finishes, its row waits if
  an edge reaches a callable not yet checked, directly or through a
  waiting row. Otherwise it is solved with every other ready waiting
  row: a monotone pass over the edges in source order, `row(f) =
  uses(f) ∪ subst(row(g)) − minus` per edge, until nothing grows. That
  is the least solution
  ([`req.row.omitted.cycle`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.omitted.cycle)),
  and solving a ready set together needs no explicit SCCs. Then the
  deferred checks whose callees are solved run, `missing-requirement` at
  the call. The sweep checks every registered body, so every row is
  solved before `Body(m)` reads one.
- **Termination.** The solve only adds keys, and keys come from a finite
  set unless a generic member calls itself at a growing argument
  (`g[T]` calling `g[List[T]]`). A key nested past depth 32 is not added
  and is `instantiation-too-deep` at the call. A body that stops with
  "not implemented" has the empty row.
- **Cache.** `ModuleFinish` writes each solved row into the module's
  items next to the inferred results. A row never enters a folder
  interface: only private callables infer, so no other module's
  `check_key` sees it, and a row changes only with the module's source,
  which `check_key(m)` covers. `row_keys` feeds the callable's own code
  key and each caller's, so an edit that changes an inferred row misses
  the code entries of the callable and of its callers.

#### 4.13.2 The Inference Engine

- **Bidirectional and local to the body.** `check(expr, expected)` and
  `infer(expr)`. An expected type flows into literals, closures, `match`
  arms, and data and collection literals.
- **Declarations are never inferred.** Generic parameters, signatures,
  public rows and bounds are what the source writes. Inference solves only
  use-site type arguments, local bindings, closure parameters from an
  expected function type, literal widths, and private results and rows as
  above.
- **Use-site type arguments.** Instantiate the callee's generics with
  fresh variables, unify parameters with arguments in source order, apply
  defaults, and solve bounds through the trait solver
  ([Inference From Several Arguments](../../spec/lang/04-type-system.md#inference-from-several-arguments),
  [Inference Through A Bound](../../spec/lang/04-type-system.md#inference-through-a-bound)).
  A variable still unsolved at the end is `cannot-infer-type` when no
  solution exists and `ambiguous-type` when several do. The permission
  join, the instantiation choice and its trial bound are in
  [crates/hd_check/DESIGN.md (type checking) §2.4 and §3.4](../hd_check/DESIGN.md#24-calls-and-use-site-type-arguments).
- **Tables.** `InferTable` (`crates/hd_types/src/unify.rs`) is union-find
  with union by rank and no path compression, so a lookup never writes
  and a rollback undoes each link exactly. Literal widths follow
  [Literal Inference](../../docs/compiler/goals.md#literal-inference):
  literal classes in union-find, the family rule for methods on open
  literals, and no statement retry
  ([crates/hd_check/DESIGN.md (type checking) §3.6](../hd_check/DESIGN.md#36-literal-widths)).
- **Speculation without copies.** Trying a candidate pushes bindings on a
  trail, and failure pops back to a mark. No checker state is cloned.
- **No overloading, no disjunctions.** Every call resolves to one callee by
  name and receiver type, so there is no search over alternatives (the
  Swift lesson).
- **Local items lift to hidden module items.** A `data`, `enum`,
  `trait`, `type` or `fn` declared in a body becomes a module-level
  item with a mangled path (enclosing item, kind, name, statement
  index), visible only from its declaration point to its suite's end.
  Outer generic parameters become its parameters, so one declaration
  keeps one identity per instantiation
  (`names.local-type.identity`, `names.local-type.refs`). Local types
  capture nothing and execute nothing (`names.local-type.static`). A
  local `fn` lowers as a named `Closure` value with closure captures and
  a self-visible name (`fn.local.capture-rules`). A generic local `fn` is
  checked once over its own type parameters, which continue the body
  owner's parameter numbers; its sub-body is generic
  (`sub_flags::GENERIC`), its defining `Closure` makes no value, and each
  use is a `Closure` that names its type arguments and takes the
  declaration's captures, which collection instantiates like any generic
  item. A local `impl` emits no item and captures nothing
  (`names.local-impl.static`, `names.local-impl.no-capture`). It extends
  the lexical impl-table overlay to the suite's end, so method and trait
  lookup inside find its methods (which must involve a visible local
  nominal or trait, `names.local-impl.involve`). `pub` and decorators
  stay rejected (`names.local.no-pub`, `names.local.no-metadata`).

#### 4.13.3 Expressions And Statements

The checker follows the spec chapters in order. Points that matter for the
design:

- **Coercions** (implicit `Option` wrapping, permission weakening, function row
  subsumption) become explicit TIR instructions, so D2 never re-derives
  them.
- **Mutability and access** checks run during checking, on the
  checker's own access types and local flags, never over TIR
  ([Writable Places](../../spec/lang/04-type-system.md#writable-places);
  [crates/hd_check/DESIGN.md (type checking) §8](../hd_check/DESIGN.md#8-mutability-access-and-initialization)).
- **Typed holes.** `_` and `todo()` record the expected type and the
  in-scope names whose types fit, for the hole diagnostic.
- **Definite initialization** of locals and the `let-else` rules are
  computed while checking, syntax-directed over structured control flow,
  with the bits on the trail. They read no TIR, so checking does not
  depend on emission (crates/hd_check/DESIGN.md (type checking) rule TC-3).

#### 4.13.4 Rows

- A row is one pool item: sorted keys and declared row parameters. Keys
  are kept sorted by `Ty` value for in-run set operations, so union and
  membership are linear merges
  ([Row Sets](../../spec/lang/11-requirements-and-suspension.md#row-sets)).
- A closure's row belongs to its function type. Creating, returning or
  storing the closure adds nothing to its creator's row; calling it does.
- Printing and hashing re-sort keys by stable content, never by `Ty` value
  (the tsgo lesson).
- Entailment is membership after alias expansion
  ([Entailment](../../spec/lang/11-requirements-and-suspension.md#entailment)).
- A pattern with one unknown row parameter takes the least solution
  ([Least Row Solutions](../../spec/lang/11-requirements-and-suspension.md#least-row-solutions)).
- `$.with` blocks push lexical keys onto the body's `available` set.
- **Row unions form at four join sites.** A list or map literal with no
  expected type whose elements are function values, the branches of a
  value-producing `if`, the arms of a value-producing `match`, and the
  final value plus `return` operands of a closure or result-inferred
  function (`req.row.union.literal`, `req.row.union.sites`) all take the
  union of their rows. Each site joins into a *join variable*
  (`Ck::join_target`). A function value coerced into a join variable
  already solved to a function type widens that type's row to the union,
  by an undoable `rebind` (`Ck::join_row`, `req.row.union.sites.type`).
  Nested sites that share the variable (an `else if` chain, an `if`
  inside a list) widen the same one. Anything holding function values
  (rather than being one) keeps its own type
  (`req.row.union.literal.direct`, `req.row.union.sites.direct`). With
  an expected type there is no union: each member checks against it
  directly, and a key outside it is `type-mismatch`
  (`req.row.union.literal.expected`, `req.row.subsume.missing`). An
  unsolved type variable, such as a generic argument's, is no expected
  type: the site joins on its own and the caller solves the variable
  from the result. No TIR node records the widening: the callable ABI
  passes the key ids with the providers, so subsumption is no
  instruction (crates/hd_mono/DESIGN.md §12.4), and a call through the
  joined value passes the union's providers. Loop `break`/`else` joins
  are not a `types.lct.sites` site and take no union.

**Context values.** `$.Context[$ R]` is `TyData::Context(row)` with a
concrete row. `$.with(...)` and `$.context(...)` check their entries
alike, in source order. An entry `K=e` checks `e`, keeps the
`mutable-upgrade` test of `req.mut.install-writable-ref`, and then coerces
`e` to the key's trait value type `K` (`ToTraitValue`, or `Supertrait`
from a subtrait value, or nothing from a `K` value), so a value whose
type lacks the impl is `type-mismatch`. A `&S` or `&mut S` value becomes
`K` by `RefToTraitValue`, sharing the place (`req.ref.share.reference-values`). When `K`'s
trait has a `&self` or `&mut self` method (`req.ref.key.reference-values`), `e` converts
to `&dyn K` or `&mut dyn K` as any value does (only a reference does,
`req.ref.install-ref`), and `RefAsProvider` keeps
that reference as the provider; `$.use(K)` reads it back with
`ProviderAsRef`, so every value in a pair or a provider slot still has
the key's type `K`. An entry `ctx...` needs a value
of a context type, else `type-mismatch`. Both forms record
`(key, value)` pairs, where a spread's key word is the spread value's
own context type. `ContextNew` has the type `$.Context[$ keys]` over
every key the entries bind, duplicates merged
(`req.context.one-per-key`). `With` pushes the same key set as its
lexical frame. An expected context type is matched by type equality, so
`$.Context[$ Backup]` where `$.Context[$ Clock]` is expected is
`type-mismatch`. The pairs keep source order, which is all that
`req.context.order` needs: codegen applies them left to right.

#### 4.13.5 Suspension

- `!` is part of the name, so suspension needs no inference.
- A bang call outside a driver context is `bang-call-outside-suspension`
  ([`req.bang.driver-forms`](../../spec/lang/11-requirements-and-suspension.md#r-req.bang.driver-forms)).
- **The `block_on` ban is direct-only.** In `defer` suites, default
  expressions, fact expressions and module initialization, a call whose
  resolved callee is `block_on` or `println` is an error. The check reads
  resolved callees in those contexts and no other body. An indirect call
  panics at run time; D2 emits that check.
- `Suspend[T]` values, `all!` and `race!` are typed as the spec says;
  their lowering is D2's.

#### 4.13.6 GADT Refinement

There are no GADTs.

#### 4.13.7 Tuples, Varargs And Arity

There are no variadic generics ([chapter 12](../../spec/lang/12-variadic-generics.md)).
`Args < Tuple` bounds an ordinary type parameter, a vararg's type is a
tuple, and a spread `f(args...)` unifies a tuple with a parameter list.
Tuple rest elements are the `rest` field of the tuple type. Tuple `Eq`,
`Ord`, `Hash` and `Debug` come from std's tuple templates: `Impl`
evidence of the template at the tuple type, like any impl
([crates/hd_check/DESIGN.md (trait solver) §3.9](../hd_check/DESIGN.md#39-compiler-supplied-impls)).

**Arity-generic function types.** A function type's `params` spell its
inputs tuple in one of three forms (`Inputs`): one parameter per element,
the same with the last a rest element's `List` (`fn.type.vararg-rest`), or
one type that is the whole inputs tuple of an arity not known there.
`Fn[Args, O, $ R]` with `Args < Tuple` takes the third form, as does its
inference variable at a call. A written inputs argument that is neither a
tuple type nor a `Tuple`-bounded parameter is `generic-kind-mismatch`
(`fn.type.ctor.inputs-tuple`). Interning a function type whose one input
is a tuple type gives the first or second form. So substitution,
resolution and canonicalization all land on one type, and
`Fn[(A, B), O, $()]` stays `fn(A, B) -> O`. Unifying, matching an impl
head, or fitting an expected type, a whole input against the other
side's inputs tuple unifies the two tuples. So `?Args` meeting `add2`
becomes `(i32, i32)`, meeting `g` becomes `(usize, usize, List[i32]...)`,
and meeting `zero` becomes `()` (`fn.vararg.tuple-param.expected`). A
call of `f: Fn[Args, O, $ R]` takes one positional spread whose operand
has the inputs' type (`expr.call.spread.inputs`). Separate arguments
there are `type-mismatch`, since the arity is unknown. A spread of a
value of unknown arity before fixed parameters is `type-mismatch` too,
since `Args` is not the type of those inputs. When unifying the operand
solves the inputs as a tuple, the spread fills each parameter with one
read of its element (`Read [%t .k]`), as for any tuple. Otherwise the checker emits `CallValue f,
[args]`: one argument, the inputs tuple, under a callee whose TIR type
keeps the third form. Instantiation substitutes a tuple for every
`Args`, so the callee's instance type has one parameter per element. The
emitter sees the third form on the callee's unsubstituted type and
passes the tuple's elements as the arguments, read as an element read
reads them, boxed tuple or not. There is no runtime machinery and no new TIR
instruction.

#### 4.13.8 Exhaustiveness

Maranget's usefulness algorithm over the pattern matrix, with literal
ranges, `Option` and tuples (`crates/hd_check/src/pat.rs`).

- Each matrix cell visited costs one fuel step, so a pathological match
  stops instead of hanging (§4.15).
- The missing-case witness becomes the diagnostic's example.

#### 4.13.9 Derive Instances, Templates, Facts And Test Overlays

- **Derive instances.** Each `@derive(X)` on a type whose trait has a
  template is checked in the type's module. It instantiates `X`'s
  template from the trait's blob for the target, then checks it with the
  template's names taken from its resolution table and the target's
  members known ([Templates](../../spec/lang/14-annotations.md#templates)).
  The result is part of the target module's `check` entry. A template edit
  changes the trait folder's deep hash, which rechecks exactly the modules
  that derive from it.
  Implementation differs: no interface carries template bodies. The
  driver lowers each foreign template module from source once per run
  (`template_mods`) and checks it under a fresh body context built from
  its scope, the opt-in module's items, and both folders' closures. An
  opt-in module's check key takes the template text hash of each
  non-std template module in its closure whose trait the module's text
  names (`template_hashes`, `crates/hd_driver/src/lib.rs:4169`); a
  module whose text names neither `derive` nor `Structure` takes none.
  The hash covers the module's API text, the template's method bodies,
  and every body whose signature a caller infers
  ([crates/hd_resolve/DESIGN.md §4.10](../hd_resolve/DESIGN.md#410-folder-interface-construction)).
  This is worse: a header edit in a template's module rechecks every
  module that derives one of its templates.
- **The instantiated-template check at each opt-in.** Every `@derive(X)`
  and every derivation block re-checks `X`'s template methods once more,
  in the target's `Body` task after ordinary checking passes, in the
  template module's scope with the opt-in's parameters and bounds in the
  environment
  ([`annot.template.checked`](../../spec/lang/14-annotations.md#r-annot.template.checked)).
  Ordinary checking infers the walker, describer or source type at each
  `walk`, `describe` or `build` call, however the template obtains it,
  and records each call's Structure method and walker/describer/source
  type (`structure_calls`); a finish-time obligation pass then holds
  each member's type to the bounds of that implementation's `member[F]`
  (`annot.walker.obligation`). A failing member is one
  `member-not-derivable` at the opt-in naming it; every other diagnostic
  of the instance check is dropped, since the template's own module
  reports its own mistakes. Members with a declared default
  (`name = pass`) contribute no obligation, as a derived `Default`
  exempts them.
- **One error per field.** Comparison derivations (`Eq`, `PartialOrd`,
  `Ord`, `Hash`) run the same instance check and report
  `derive-field-missing-trait` at the field: the driver dedups by
  (type, slot) and reports a field that misses several traits once, for
  the first trait. Other derivations report `member-not-derivable` at
  the opt-in. A derived newtype gets an implementation head, and its
  base type is checked against the derived trait through the solver in
  header checks (`trait.derive.newtype.requires`, slot 1 is the base
  type). Anchor slots repeat the type's field positions (and a newtype's
  base slot) onto the derived implementation, so field errors point at
  the field even when the private type has no interface item of its own.
  Not built yet: field anchors for derivation blocks
  (`impl X for D by Structure`), so their member failures report at the
  block header.
- **No coinductive assumption.** The derived head is already in the
  target module's impl table, so a member goal such as `List[Tree[T]]:
  Eq` reaches it as an ordinary `Impl` and the search meets no cycle
  ([crates/hd_check/DESIGN.md (trait solver) §3.10](../hd_check/DESIGN.md#310-derives-delegation-and-error)).
- **Opt-in bodies become the derived methods.** Each checked template
  method becomes a method of the derived implementation: the opt-in body
  is kept, the template's `T` replaced by the target and the methods'
  parameters by the instance's (`hd_tir::wire::map_ids` rewrites every
  run ID), and each `Structure` call chooses the derivation itself
  (`Impl` choice), so collection calls these bodies directly. The bodies
  and the derived methods' items join the module's check entry.
- **Header goals are not body work.** Supertraits of each impl,
  supertrait bindings, a derived newtype's base impl and a delegation
  target's impl are checked by the folder's `HeaderCheck(F)` task
  ([crates/hd_resolve/DESIGN.md §4.10.1](../hd_resolve/DESIGN.md#4101-header-validation-stages),
  stage B; [crates/hd_sched/DESIGN.md §6.1](../hd_sched/DESIGN.md#61-tasks)), never by a
  body task.
- **Facts and defaults.** A fact or default expression is checked once, in
  its declaring module, as a requirement-free body of its own (body kind
  `Fact` or `Default`). A call that omits an argument emits a
  `DefaultCall` of that body per call (§4.13.11). A fact's value is
  computed at run time, on first read, by a lazily initialized global
  ([crates/hd_mono/DESIGN.md §12.3](../hd_mono/DESIGN.md#123-facts-defaults-derives-and-tests)).
- **Test overlay.** The `tests:` block and the doc tests of module `m` are
  checked under `hd check --tests` and `hd test` by the module's
  test-role `ModulePrep(m)`, `Body(m)` and `ModuleFinish(m)` tasks
  ([crates/hd_sched/DESIGN.md §6.1](../hd_sched/DESIGN.md#61-tasks)).
  Their uses make no folder edge, so the test role's `ModulePrep(m)`
  waits for the interfaces they use; the result is cached as
  `check-test`. Doc tests are
  extracted from `##` blocks into synthetic files whose spans map back to
  their lines.
- **Registration declarations are plain hd.** `lib/std/testing.hd`
  declares `it`, `it_each`, `it_prop`, `it_prop_with` and `snapshot`
  with their documented signatures; the language tier checks their
  names, options and positions at each registration call. The bodies
  never run, since test position intercepts the call, so each body is an
  unreachable stub.
- **Rows and properties expand at run, not at plan.** The plan lists one
  case per registration. `it_each` evaluates `rows` once in its program
  instance when the case runs, then runs the body per element and
  reports each as `name[i]`. `it_prop`/`it_prop_with` runs `examples`
  first, replays the saved `__regressions__` stream, then generates
  `cases` inputs from consecutive seeds (so the first four cover the
  low/high/zero/none edges); discards do not count toward `cases` (past
  10× `cases` the property fails). A failure shrinks the choice stream
  (draws are reported per `std-testing.choices.from-case`), prints the
  shrunk input with `Debug`, and saves the regression file.
- **Test bodies.** Each `it` case is a synthesized suspending item
  (`$testN`, body kind `TestCase`) with no parameters, whose row frame is
  the profile the runner binds: `TestRunner` alone for a unit test, the
  test profile's host traits too for an integration test. Its row is the
  profile keys the body uses, so a body that names `$ TestRunner` (or
  calls a function that does) checks in a `tests:` block, and any other
  key is `missing-requirement` (`module.testing.unit-row.test-runner`).
  A `timeout` argument is checked as `Duration?` and evaluated first, in
  the case's instance; a duration goes to std's `case_timeout`, which
  calls `TestRunner.report_timeout(ms)`, so a timed case's row also holds
  `TestRunner`. The host supervisor stops a body past its deadline and
  `hd test` reports it as a `time-limit` panic. A trailing block is the
  item's own body, with the fixed result `void`, or
  `Result[void, dyn Error]` when it holds a `?` outside nested closures
  (`expr.try.test.*`). An explicit closure (`body=fn!() -> T: ...`, or
  the second positional argument) is checked with no expected type, so
  it keeps its written or inferred result `T`; `T < Termination` is
  required at the closure (`unsatisfied-trait-bound`), and the item's
  body calls it, bang for a suspending one, so its row joins the case's
  row and the item's result is `T`. An `it_each`, `it_prop` or
  `it_prop_with` body gets the fixed result through
  `registration_body_result` and runs under the std case body, which
  takes the registration's generics and parameters, so the case passes
  the call's arguments, defaults included, unchanged. For a
  result other than `void`, the driver roots the `std.rt` status
  function that `report_fn` picks, as for `main`: an `.Err` writes its
  report and cause chain through `entry_write` into the case's captured
  standard error, and `hd test` prints that text as the case's `FAIL`
  message on standard output (the JSON `message` in `--format json`),
  never on its own standard error (`module.testing.err-runner-report`). A `?`
  whose error neither is assignable nor converts by `From` is
  `invalid-result-propagation` (`expr.try.convert.none`).
- **Snapshots are host work inside ordinary cases.** `snapshot_file`
  calls `TestRunner.snapshot_check` (file IO, `--update` recording and
  the missing-file failure live in the host); the planner only runs the
  case body, keeping the per-case call counter that names
  `<test-slug>-<n>.snap`. A mismatch fails as `assertion-failed`.
- **Per-case reporting.** `hd test` keeps its line per case: pass and
  fail counts, `PANIC` lines with `--filter` repros, rows as `name[i]`
  lines, property failures with the shrunk `Debug` input.

#### 4.13.10 Module Initialization

Definite initialization of top-level bindings follows transitive read sets
through function bodies
([`module.init.definite`](../../spec/lang/10-modules.md#r-module.init.definite)).
Inside one module, the checker computes it while it checks. But a module
cycle that spans several modules of a folder, through a use loop, needs
the bodies of all of them: a body-derived fact across modules.

So `ModuleFinish` writes an **init summary** per module
(`hd_check::init::InitSummary`), from the facts the checker records
while it checks (never from TIR): for each function and top-level
statement, the top-level bindings it reads directly, the same-folder
functions it calls, and the trait methods it dispatches
([`module.init.definite.dispatch`](../../spec/lang/10-modules.md#r-module.init.definite.dispatch)).
`InitOrder(F)` reads the summaries of each module cycle of the folder,
orders statements by
[Order Inside A Cycle](../../spec/lang/10-modules.md#order-inside-a-cycle),
and reports `top-level-read-before-initialization`. Its key (`init_key`)
is over the summaries, so a body edit that does not change what a
function reads reruns nothing. Groups never span folders, so the fact
stays inside one folder. The driver adds one `InitOrder` task per built
folder; a folder with no module cycle does no work. A module with errors
writes an empty summary, since the build stops before collection.

#### 4.13.11 The Typed IR (TIR)

TIR is the one typed IR of a body. The checker emits it directly while it
checks, as Zig's Sema emits AIR and Carbon's checker emits SemIR. D2
consumes it: collection reads it, and emission walks it under a type
substitution to write Wasm. There is no other tree between the syntax
tree and Wasm. The checker builds TIR only through the builder API below,
and D2 relies only on the invariants below.

A **body** is a function or method, an init group's statements, a test
case, a fact, a default expression, a derive instance or a structure
handle. A closure is a **sub-body** of the body that contains it, in the
same columns.

##### Encoding

The storage card is
[docs/compiler/data-structures.md §3.18](../../docs/compiler/data-structures.md#318-tir).
The encoding is `hd_tir::ir::Body` (`src/ir.rs`):

```rust
pub struct Body {
    pub item: DefId,
    pub kind: BodyKind,                 // u8: Fn | Init | TestCase | Fact | Default | DeriveInstance | Handle
    // instructions, by Inst
    pub tags: Vec<Tag>,                 // 1 B
    pub data: Vec<[u32; 2]>,            // 8 B; operands, meaning set by the tag's schema
    pub ty: Vec<Ty>,                    // 4 B; result type; `void` or `never` for statements
    pub syn: Vec<NodeIdx>,              // 4 B; the syntax node, for spans and sites
    pub extra: Vec<u32>,                // records: a length word, then that many words
    // locals: parameters, user bindings, pattern bindings, by LocalId
    pub local_ty: Vec<Ty>, pub local_name: Vec<Symbol>, pub local_syn: Vec<NodeIdx>,
    pub local_flags: Vec<u8>,           // READ | ASSIGNED | MUTATED | CAPTURED | CAPTURED_ASSIGNED | PARAM | SHORT | NOT_YET
    // sub-bodies, by SubId: 0 is the body itself, then one per closure in creation order
    pub sub_root: Vec<u32>, pub sub_params: Vec<Range32>, pub sub_parent: Vec<SubId>,
    pub sub_flags: Vec<u8>,             // GENERIC
    pub label_inst: Vec<u32>,           // by LabelId: the Loop or Block a Break or Continue targets
    pub cap_local: Vec<LocalId>,        // by CaptureId; one closure's captures are contiguous
    pub cap_mode: Vec<CaptureMode>,     // Copy | Move | Shared; written once, at finish
    pub susp: Vec<SuspRow>,             // 16 B: inst, scope chain, hook site
    pub consts: Vec<(Ty, u64)>,         // body-local constants, by constant Ref
    pub strings: Vec<Box<str>>,         // string constants' text
    pub label: Box<str>,                // what a Fact body is attached to, for panics
    pub fallback: Vec<[u32; 2]>,        // overflow checks on a literal's usize default; outside the hash
    pub exit_drops: Vec<(SubId, LocalId)>, // the ownership passes' exit set; never on the wire
    pub unwind_drops: Vec<(u32, u32)>,  // the ownership passes' unwind set: (suspension point, value); never on the wire
    pub stable_lists: Vec<(u32, u32)>,  // hoisting's stable list places: (loop, place word); never on the wire
    pub regions: Vec<(u32, u32)>,       // tier 2's lifetime regions (reserved): empty
}
pub struct Inst(pub u32);
pub struct Ref(pub u32);   // bit 31 clear: an Inst's value; set: a row in this body's consts
```

The `Tag` enum and each tag's operand schema come from one `tags!` table
in `src/ir.rs`, so every encoding goes through it.

- **Four kinds of reference.** The schema (`Op`) types every operand word
  as one of: a **value** (`Ref`), a **child block** (an `Inst` of a
  `Block` that this instruction owns), a **label** (`LabelId`), or a
  **local** (`LocalId`). Metadata words (`DefId`, types, indices) are a
  fifth, non-reference kind, and `Values`, `Blocks` and `Record` point at
  a record in `extra`. A **place** word (`Op::Place`) names where a value
  lives, below. Def-before-use and visibility (invariant 1) apply
  to value edges only. A label is allocated when its loop or block opens,
  before the body, so `Break` and `Continue` never point forward; the
  label row is written once when the target closes.
- **Values are instructions.** An instruction's result is its value, used
  by later instructions through a `Ref`. Mutable user bindings are locals
  read with a `Read` and written with a `Store` of their place.
- **Constants are `Ref`s, not instructions.** A `Ref` with bit 31 set
  names a row in that body's `consts` table: the constant's type and 64
  value bits; a `string` constant's bits index `strings`. There is no
  `Const` instruction.
- **Blocks list instructions.** A `Block` holds a record in `extra` of
  the instructions it runs, in evaluation order. Every ordinary
  instruction is in exactly one block.
- **Owned blocks have their owner as parent.** A `Block` named by an
  `If`, `Match` or `Loop` child-block operand is owned by that
  instruction. It is also listed in the enclosing block (invariant 2),
  and walks skip it there. The structure passes (`ir::topo`) take the
  owner as its parent, so a loop's body is inside its `Loop`.
- **Types** are pool indices. While a body is checked they may be
  inference variables; the checker's final pass resolves them.
- **Capture modes** depend on statements after the closure, so they are
  a column written once at `finish`. The checker decides the modes
  (rule TC-1); the builder only stores them. No instruction word is
  patched.
- **Places** (`ir::place`, `Place`; [docs/compiler/tir-v2.md](../../docs/compiler/tir-v2.md#places)).
  A place is a root (a local, a top-level binding and its type, a value,
  a tier-2 frame slot, or a pointer's target) and projections (dereference, field by
  declaration index, list element at an index value, enum variant). A
  place word with bit 31 set is a bare local `$n` (bits 0 to 30) with no
  record. Otherwise it is the offset of a place record
  `[len, root kind, root word, (root word 2), projection word ...]`: root
  kinds 0 local (`LocalId`), 1 global (`DefId`, `Ty`), 2 value (`Ref`), 3
  frame (reserved), 4 pointer (`Ref` to a `std.rt.memory.Ptr[T]`: the `T`
  at its address, written `[ptr %n]`, which the checker makes of `read`
  and `write`); a projection word's top two bits are its kind, 00
  field (bits 0 to 29 the index), 01 dereference, 10 element (the next
  word is the index `Ref`), 11 variant (bits 0 to 29 the index). A record
  is immutable, so instructions may share one; the wire form remaps a
  shared record's IDs once. `TirBuilder::place` encodes a place,
  `Body::decode_place` decodes one, and its text form is
  `[*$0 .1 [%5] as .2]`. `Read`, `Store` and `Ref` take a place
  (below).
- **Sizes.** 17 bytes per instruction plus its `extra` records; 13 per
  local.

Planned, not built: an `origin` side table (generated code to its origin)
and a `hole` side table (typed-hole candidates).

##### Instruction Catalog

Notation: `a` and `b` are the two data words; `[...]` is a record in
`extra`. "Join" is the arms' common type by the spec's join rules.

**Values.** A local and a top-level binding are read with `Read`
(below): `Read [$l]`, whose place word is the bare local and has no
record, has the local's type, without its outer `mut` when the binding
is readonly; `Read [@g]` has the binding's type.

| Tag | Operands | Type rule |
| --- | --- | --- |
| `ItemRef` | a: `[item DefId]`, b: `[type arguments]` | the item's function type, instantiated (functions, variant constructors, unbound method references, receiver first). The type arguments are the owner's, then the item's own: an inherent member's impl arguments; a trait member's `Self`, then the trait's arguments. A bound reference `value::name` is no `ItemRef`: it is a `Closure` whose body calls the method on a captured local holding the receiver (`hd_check/src/fnref.rs`) |
| `ProviderGet` | a: `[key type]` | the key's provider type (`$.use(K)`) |
| `Hole` | a: expected type | the expected type; only in a module with errors |
| `Poison` | none | poison; only in a module with errors |

**Operators and calls**

| Tag | Operands | Type rule |
| --- | --- | --- |
| `Prim` | a: `PrimOp`, b: `[operands]` (one or two) | arithmetic, bitwise and shifts: the operand type; comparisons: `bool`; conversions: the target. Only on primitive types |
| `And`, `Or` | a: left, b: right `Block` | `bool`; the right block runs only when needed |
| `Call` | a: `[callee]`, b: `[arguments, providers]` | the callee's result, substituted; `mut Suspend[T]` when the callee suspends and the call is plain (a cold call) |
| `CallValue` | a: function value, b: `[arguments, context]` | the function type's result |
| `CallDyn` | a: trait value, b: `[method, type arguments, evidence, arguments, providers]` | the method's result, substituted. The type arguments are the method's own. `evidence` holds one `Evidence` value per method-level bound, in the method's bound order, chosen by the checker at the call. Codegen does not read it: a generic method gets a type witness built from the type arguments (crates/hd_mono/DESIGN.md §13.5.1), so dropping the operand is a frontend cleanup |
| `Is` | a: left, b: right | `bool`. Only on two operands whose types implement `AnyRef` (`&T`, `&mut T`, an `AnyRef` type parameter), or on trait values and `Any`. `is` on a struct, list, map, enum, optional, `Result`, tuple, primitive, string or function value is `identity-requires-references` before TIR ([`expr.is.value-operand`](../../spec/lang/05-expressions.md#r-expr.is.value-operand)); codegen lowers it by layout (crates/hd_wasm_linear/DESIGN.md §15.2) |
| `CallHost` | a: host method, b: `[arguments]` | the method's result; only in std's provider bodies |
| `Intrinsic` | a: intrinsic, b: `[type arguments, arguments]` | the intrinsic's declared result |
| `DefaultCall` | a: `[default body DefId, type arguments]`, b: `[earlier argument values]` | the parameter's or field's type, substituted. One per omitted argument, per call |
| `Interp` | b: `[parts]`, each a literal constant or a value with its `Display` callee | `string` |
| `Coerce` | a: value, b: `[kind, evidence or NONE]` | the target type in `ty`; the kinds are in the table below |

**Coercion kinds** (crates/hd_check/DESIGN.md (type checking) §4.2). Each
changes the type, so each is an explicit instruction and invariant 4
holds.

| Kind | From → to | Evidence word | Run-time meaning for D2 |
| --- | --- | --- | --- |
| `Never` | `never` → any | none | unreachable |
| `Weaken` | `mut T` → `T` | none | none: a static view change |
| `Variance` | `C[A]` → `C[B]` by declared variance, readonly outer type | none | none ([Representation-Preserving Variance](../../spec/lang/04-type-system.md#representation-preserving-variance)) |
| `WrapSome` | `T` → `T?`, one layer | none | build `.Some` |
| `RowSubsume` | `fn ... $ R1` → `fn ... $ R2` | none | none: every function value takes a context and looks its keys up by id, so a context holding `R2`'s keys serves `R1` (crates/hd_mono/DESIGN.md §12.4) |
| `ToTraitValue` | `S` → `Tr` | the impl choice | box with its dispatch table |
| `ToAny` | `S` → `Any` | none | box with its type id |
| `Supertrait` | child trait value → parent trait value | none: the target type names the parent | re-table: load the parent's vtable from the child's |
| `SuspendFnToCtor` | `fn!` type → constructor type | none | none, or a thin adapter |
| `Identity` | `T` → `T` | none | none: a least-common-type slot whose value needed no conversion |
| `RefToTraitValue` | `&S` or `&mut S` → `Tr`, inside a trait value reference's `get`, or as the provider of a key whose trait has no `&self` or `&mut self` method | none | box the reference itself, with a dispatch table at `S`'s impl that reads through it or passes it per slot (crates/hd_wasm_linear/DESIGN.md §15.2, "Trait value references") |
| `RefAsProvider` | `&dyn K` or `&mut dyn K` → `K`, the provider of a key whose trait has a `&self` or `&mut self` method | none | the reference is the payload; the vtable is the one its `get` returns, which no call reads (crates/hd_wasm_linear/DESIGN.md §15.2, "Providers of reference traits") |
| `ProviderAsRef` | `K` → `&dyn K` or `&mut dyn K`, at `$.use(K)` of such a key | none | cast the payload back to the reference |

Today every `Coerce` writes `NONE` as its evidence word; the wire writer
refuses any other value until it is remapped.

**Default calls.** A default is evaluated at each call that omits it,
after every explicit argument, in parameter declaration order
([`fn.default.eval`](../../spec/lang/07-functions.md#r-fn.default.eval)),
and a data field default at each construction that omits it
([`data.default.eval`](../../spec/lang/08-data-and-enums.md#r-data.default.eval)).

- Each default is a body of its own, kind `Default`, checked once in its
  declaring module (crates/hd_check/DESIGN.md (type checking) §1.7). Its
  parameters are the callee's earlier parameters; a field default has
  none. Its `DefId`'s stable path is the declaration's path plus the
  parameter or field name.
- At a call, the checker emits one `DefaultCall` per omitted argument,
  after the explicit arguments' instructions, in declaration order. Its
  operands are the callee's type arguments and the `Ref`s of the earlier
  parameters' values, explicit or defaulted. Those are the same `Ref`s
  the call passes, so nothing is evaluated twice.
- It is a separate tag, not a `Call`, for two reasons. The verifier can
  check that its operands are exactly the call's earlier argument `Ref`s.
  And emission brackets it with the forbidden-context counter that makes
  an indirect `block_on` or `println` panic (crates/hd_wasm_linear/DESIGN.md
  §14.9), unless the default body calls nothing.
- Lowering is a direct call of the default body's instance. A default
  that is a constant, such as `= 10`, passes the trivial-inlining test
  and costs nothing.

**Qualified associated calls (`Type::f::[T]()`).** A call whose callee
spells explicit type arguments resolves in one order: the owner first
(a type, trait, or type parameter, as `fn.ref.lookup` resolves it for
references), then the member (methods and associated functions, never
fields), then the written type arguments: the owner's, then the member's
own, with trailing slots inferred or defaulted, over-long lists rejected
(`fn.generic.explicit.*`, `argument-count`), and row slots after `$`.
That is `hd_check/src/fnref.rs` (`member_of`, vars, substitution) with
the value path's expected-type fit replaced by ordinary call checking.
The result is an ordinary `Call`: an `Item` callee, or a `TraitMethod`
with the solver's choice for trait members. No new TIR node.

A **callee record** (`Callee`) is `Item(DefId, type arguments)`, three
words with its kind word, or `TraitMethod(trait, method, self type, type
arguments, choice)`, seven words. The `TraitMethod` record has one
`targs` list: the trait declaration's arguments first, in declaration
order, then the method's own type arguments; the interface supplies the
split point from the trait's generic count. The **choice** is two words:
a kind and a full 32-bit value.

| Choice | Value | Meaning |
| --- | --- | --- |
| `Impl` | the impl's `DefId` | a written, derived, delegated, generated, numeric-family or tuple-template impl |
| `Bound` | the bound's index in the parameter environment | dispatch through a bound in scope; static in every instance |
| `TraitValue` | the trait's `DefId` | the self type is a trait value of this trait or a subtrait: a vtable call, as `CallDyn` |
| `Builtin` | a `BuiltinImpl` number | a sealed trait answered from the type's form: `Any`, `AnyVal`, `AnyRef`, `Inspectable`, `Tuple`, `Num`, `Integer`, `Float`, `Suspend` |

These are the solver's `Evidence` values
([crates/hd_check/DESIGN.md (trait solver) §8.1](../hd_check/DESIGN.md#81-what-a-holds-answer-carries))
written into TIR. A `CallDyn` evidence operand uses the same encoding.
Tuple `Eq`, `Ord`, `Hash` and `Debug` are `Impl` choices of std's tuple
templates, not `Builtin`. A `Builtin` callee has no TIR body: collection
maps `(BuiltinImpl, concrete self type)` to a body that D2 generates per
primitive or per shape, named by the stable path `std.builtin.<trait>`
plus the canonical self type in its instance key. **Arguments** are
listed in parameter order; their instructions appear earlier in the
enclosing block's list in source order, which is how named arguments
keep their evaluation order. **Providers** (`Providers`, 3 words) are
`None`, `(key, provider Ref)` pairs, or one context `Ref` for a
row-polymorphic callee. Since rows are solved before callers read them
(§4.13.1), every call today writes `Providers::None` and codegen picks
providers from the callee's signature row; the `Pending` form is unused.

**Suspension and hooks**

| Tag | Operands | Type rule |
| --- | --- | --- |
| `Await` | a: `[callee]`, b: `[arguments, providers]` | the callee's result `T`. A bang call: a suspension point |
| `AwaitValue` | a: a `mut Suspend[T]` value | `T`. `s!()` on a stored suspension: a suspension point |
| `AwaitAll` | b: `[children]`, each `mut Suspend[X_i]` | `(X_1, ..., X_n)`. `all!`: one suspension point |
| `AwaitRace` | a: a `List[mut Suspend[T]]` | `T`. `race!`: one suspension point |
| `Hook` | a: `HookKind`, b: the instruction it observes | `void`; normal emission drops it |

Every suspension point has a side record (`SuspRow`): the `Scope`s
enclosing it from the innermost out, which are the scopes whose suites
cancellation must run there, and its hook site. Liveness is not
recorded; emission computes it. Not built yet: the wire form refuses a
body with suspension rows (`src/wire.rs`), so no body writes them today.

**Data and closures**

| Tag | Operands | Type rule |
| --- | --- | --- |
| `NewStruct` | b: `[field values]` in declaration order | the struct type in `ty`, or a compiler-supplied data object's type |
| `CopyStruct` | a: source, b: `[(field, value)]` replacements | the source's type. Copy-update literals only: every field not replaced is read from the source ([`expr.update.copy-fields`](../../spec/lang/05-expressions.md#r-expr.update.copy-fields)); a field write is a `Store` |
| `NewVariant` | a: variant declaration index, b: `[payload values]` | the enum type in `ty` |
| `NewTuple` | a: rest-list operand or `NONE` (tuple spread), b: `[elements]` | the tuple type; with a rest operand, its rest element |
| `NewList` | a: `[spread positions]` or `NONE`, b: `[elements]` | `List[T]` |
| `NewMap` | b: `[key, value pairs]` | `Map[K, V]` |
| `Closure` | a: sub-body, b: captures as a `CaptureId` range | the closure's function type. Each capture's mode, in the `cap_mode` column, is `Copy` (no side writes it after the capture), `Move` (only the closure uses it afterward) or `Shared` (both may) |

**References**

A reference names its place, so the RC backend lowers it to the
place's address (`crates/hd_wasm_linear/DESIGN.md` §23.20). The checker
makes references with `Ref`, reads through them with `Read` and writes
through them with `Store` (Places, below). Two reference instructions
remain:

| Tag | Operands | Type rule |
| --- | --- | --- |
| `RefDyn` | a: address-form `Block`, the trait value the source reference converts to | the trait value reference (`trait.dyn.ref.share`, `trait.dyn.ref.widen`) |
| `RefIdentity` | a: reference | std's `RefIdentity` (`identity_of`) |

**Places** ([docs/compiler/tir-v2.md](../../docs/compiler/tir-v2.md#reads-stores-and-references)).
After stage 1 of TIR v2 every reference is a `Ref`, every read through
a reference a `Read`, and every write a `Store`; stage 2 makes every
plain read of a local or a top-level binding a `Read` too (`[$l]`,
`[@g]`, mode `use`), and every read of a field: `[%v .k]` of a struct,
newtype or tuple value (embedded fields included; the base's access
carries to the field's type), `[*%o .k]` of a compiler-supplied data
object, and `[%v as .V .k]` of a payload. `Body::read_step` names the
step of such a read. A list element is `Read [%xs [%i]]`, a slot of the
buffer of the list header `%xs` (`Body::element_read`); there is no
element intrinsic. A read a change then goes through stays a `Read`
too: insertion follows each change back
through the reads it goes through (`own/insert.rs`, rule 5). A `mut
self` receiver of a collection is the callee's own copy, a value
argument like any `mut` parameter's. A `Store`
writes a whole local
(`[$l]`), a top-level binding (`[@g]`), a struct field of either, a
data object's field (`[*%o .k]`), a place through a reference
(`[*%r .x]`) or a list element (`[%xs [%i] .x]`).

**Places rooted at the local** (docs/compiler/tir-v2.md, stage 2 commit
12a). The checker builds a place one step at a time, a read of the base
and a projection rooted at that read's value, since its own walkers (the
writable check, `mark_write_path`) step back through those values. It
then roots every such chain at its local when it finishes a body
(`ir::root_places`, `src/ir/root.rs`): `bodies[i].x` through `&mut List[Body]` is one
`Read [*$0 [%5] .x]`, `xs[i] = v` is `Store [$0 [%1]] v`, `self.x` is
`Read [*$0 .x]`, and a top-level binding's chain is rooted at the
binding, `bags[i].items` as `[@bags [%i] .items]`. A base read
`%v = Read P` of a local's or a top-level binding's place folds into
its uses when every use is the root of a `Read`, `Store` or `Ref` place
(a `Store` or `Ref` one going into memory: a dereference or an element),
the uses are statements of `%v`'s block with only inert statements
between (reads, primitives, constructions, coercions, lengths; with an
element step in `P`, only reads that check nothing), and the local's
type names no type parameter of the body's own item and no opaque type
(`ir::erasable`: the erased body of a generic method called through a
trait value outlines an open read by its values; its impl's and trait's
parameters are concrete there; a binding's type is never open). The folded read goes, and the body
is rebuilt once (`own::splice`). A base used as a value too (a match's
scrutinee, an intrinsic's operand) keeps its read. A place rooted at a
local lies in the local itself only when every step is a field or a
variant (`Body::local_in_place`); a `Store` or `Ref` through a
dereference or an element writes or addresses a referent or a buffer,
not the local, and the passes and the verifier read it so. The old
path's TIR keeps the base reads.

| Tag | Operands | Type rule |
| --- | --- | --- |
| `Read` | a: place, b: `ReadMode` (`Use` from the checker; `Copy`, `Move` or `Borrow` after the ownership passes) | the place's type; the value at the place |
| `Store` | a: place, b: value | `void`; replaces the value at the place. The place is writable (invariant 21) |
| `Ref` | a: place, b: `0` shared or `1` mutable (`ref_word::MUT`), plus `8` (`ref_word::LOCAL`) for a local reference, `&T` or `&mut T`, which the checker sets as it erases the kind, and `2` (`ref_word::TAKES`) after the move pass | `&T` or `&mut T`: the place's address. A mutable one's place is writable (invariant 21). A taking one, of a whole local in a cell at its last access, takes the local's count of the cell and leaves the local empty |

`value_operands` of these, and of `IsUnique` and `Pin`, is the place's
value root and element indices (then a `Store`'s value); a local or
global root is no value.

**Ownership** ([docs/compiler/tir-v2.md](../../docs/compiler/tir-v2.md#ownership-statements)).
The ownership passes write these after decoding; the checker never does
(invariant 22), and the wire refuses them. Each has a place in a and
nothing in b but `IsUnique`, whose b is 0, `unique_word::HOISTED` (it
stands before the loop that follows it, which runs it once under its
entry test) or `unique_word::COVERED` (a hoisted one before an
enclosing loop makes it unneeded where no reference view is
reachable); the type is `void`.

| Tag | Meaning |
| --- | --- |
| `Drop` | releases the value at the place and leaves it empty; a drop of an empty place does nothing |
| `IsUnique` | makes the buffer of the list or map at the place its own, copying it when another holder shares it |
| `Pin` | after an `IsUnique` of the same list place, marks its header as its buffer's one owner while element references hold counts of the buffer |
| `FlagSet`, `FlagClear` | tier 2 only (reserved; invariant 23 rejects them): set and clear a borrow flag next to a pin |

A body's one exit per sub-body drops the locals of its exit set,
`exit_drops`: rows of a sub-body and a local, sorted, which the passes
write. A suspension point's cancellation, a way out no TIR jump spells,
releases the values of its unwind set, `unwind_drops`: rows of a
suspension point and a value, sorted, which the known-null pass
writes; it then leaves through the exit, so the exit set holds every
local that may hold a value at a suspension point. `stable_lists`, which
the hoisting pass writes, holds rows of a loop and a place of a list
whose header and buffer nothing in the loop replaces, sorted; the
backend keeps the buffer's address and length in locals for that loop.
`regions` is tier 2's (reserved) and stays empty.

**Providers**

| Tag | Operands | Type rule |
| --- | --- | --- |
| `With` | a: `[(key, provider value)]`, b: body `Block` | the block's type (`$.with`). Each value is a trait value of its key; a pair whose key is a context type spreads that context value |
| `ContextNew` | a: `[(key, provider value)]`, as for `With` | the reusable context's type (`$.context`) |
| `ContextFor` | a: a row | the context passed to a row-polymorphic callee, built from the providers in scope |

**Control flow and cleanup**

| Tag | Operands | Type rule |
| --- | --- | --- |
| `Block` | a: `[instructions]`, b: tail value or `NONE` | the tail's type, or `void` |
| `Scope` | a: body `Block`, b: `[defer suite Blocks]`, in instruction order | the body's type. A cleanup scope ([`flow.defer.scopes`](../../spec/lang/06-control-flow.md#r-flow.defer.scopes)) |
| `Defer` | a: suite number in the enclosing `Scope` | `void`; registers that suite here |
| `If` | a: condition, b: `[then Block, else Block]` | the join of both |
| `Loop` | a: body `Block`, b: always `NONE` | the join of its `Break` values, or `void` |
| `Break` | a: target label, b: value or `NONE` | `never` |
| `Continue` | a: target label of a `Loop` | `never` |
| `Return` | a: value, or `NONE` for a bare `return` in a `void` body | `never` |
| `Unreachable` | none | `never`; the default of an exhaustive switch |

An exit (`Break`, `Continue`, `Return`, or falling off a scope) runs the
registered suites of every `Scope` it leaves, innermost first. Which
scopes those are is explicit in the nesting, and cancellation at a
suspension point runs the same suites of the same scopes (its side
record). Ordinary cleanup and cancellation therefore share one source.

**Matching**

| Tag | Operands | Type rule |
| --- | --- | --- |
| `Match` | a: scrutinee, b: `[decision Block, arm Blocks]` | the join of the arms |
| `SwitchTag` | a: an enum or `Option` value, b: `[(variant declaration index, Block) cases, default Block or NONE]` | `never`: inside a decision tree, every path ends in `ToArm` or `Unreachable` |
| `SwitchInt`, `SwitchChar` | a: value, b: `[(constant or range, Block) cases, default]` | as `SwitchTag` |
| `SwitchStr` | a: value, b: `[(constant, Block) cases, default]` | as `SwitchTag` |
| `Guard` | a: condition `Block`, b: `[arm, fail Block]` | `never`; the fail block continues with the remaining rows |
| `ToArm` | a: arm number | `never`; the leaf has already bound the arm's locals with `Store`s |

The checker emits each arm's `Block` while it checks the arm, then builds
the decision tree after exhaustiveness and emits it as these switch
instructions. Shared arms appear once. A payload field of a value
switched to a variant is a `Read` of the variant projection,
`[%v as .V .k]`, of the payload field's type; an `Option`'s `.Some`
value is `[%v as .1 .0]`.

**Wire-visible identities.** `NewVariant`, `SwitchTag` and a variant
projection identify a variant by its declaration index within the enum, not by
`DefId`; the enclosing value or result type identifies the enum. The run
IDs used by `ProviderGet`, `ItemRef`, a global place root,
`DefaultCall`, `With`, `ContextNew` and calls live in their `extra`
records. The two data words hold only record offsets, values or
declaration indices, because the wire writer remaps typed words in
`extra` (`id_words`, `src/wire.rs`) and never guesses whether a data word
is a run ID.

##### What The Checker Desugars

| Source | TIR |
| --- | --- |
| `a \|> f(_, b)` | `a`'s instructions, then `Call` with `a` in the placeholder's slot |
| `"x $y"` | `Interp` |
| string prefixes, literal suffixes (`10ms`) | `Call` of the resolved prefix or suffix function |
| `x[i]`, `x[i] = v` | `Call` of the resolved index or set method |
| `x += y`, and the other compound assignments | the place's operands evaluated once (their `Ref`s used twice), `Prim` or a trait `Call`, then a `Store` of the place or a set `Call` |
| `a ** b` on integers | a `Prim` integer-power operation: checked multiplication in the base's result type (`expr.power.checked`, `expr.power.int.*`); float `**` is the IEEE `pow` hook (`expr.power.float.pow`) |
| `a ** b` otherwise | the trait `Call` |
| `a..`, `..b` range patterns | bound comparisons in the decision tree (`n >= a`, `n < b`), no range value built (`flow.match.range.from`, `flow.match.range.to`, `flow.match.range.no-bind`) |
| `..` full-range expression | `RangeFull {}`: a `NewStruct` of the fieldless struct (`expr.range.form.full`) |
| `(a, rest...)` tuple pattern | the `Match` decision tree, with the rest subpattern binding the trailing elements as a `List` (`flow.match.spread.rest`, `flow.match.spread.cover`) |
| `p = e` through a call (`count() = 1`) | the callee evaluated to a place, then the store, in `expr.assign.order.call` order, with the evaluate-once machinery of compound assignment (`expr.assign.target`, `expr.assign.order`) |
| `Variant(pos, named=…)` mixed construction | checked by call conventions (positionals first), then `NewVariant` with payloads in order (`data.enum.construct.call`) |
| `(1, 2, 3, 4)` against `(usize, usize, List[i32]...)` | `NewTuple` with fixed elements plus the rest list as its rest operand, checked against the rest type (`types.tuple.rest.*`) |
| `T { ...base, f: e, ... }` | `base` evaluated first, then a read of each copied field when an explicit field expression may run code (`expr.update.copy-time`), then the explicit field expressions in source order, then `CopyStruct(base, [(f, e), ...])` with those reads among the replacements (copy-update; one spread max, first) |
| `[a, xs..., b]` | `NewList` whose first operand records the spread positions; each element evaluated once, in order |
| `(a, xs...)` | `NewTuple` with fixed elements `[a]` and the `xs` operand (a `List[T]`) as rest |
| `f(xs...)` at a vararg | the `xs` value itself as the vararg's collected value |
| `f(a, b, xs...)` after a tuple vararg's arguments | `a`, `b` as ordinary arguments, `xs` as the rest-element list |
| `f(xs...)` before fixed parameters | `xs` evaluated once into a temp, then ordinary arguments: element reads for the fixed inputs, the rest list for a `List[T]` vararg |
| `[for x in xs if c => e]`, map comprehensions | a `Block`: an empty `NewList` or `NewMap`, a loop, a push `Call`; a list comprehension of one `for` and no `if` over a range or a `List` reserves the source's length once before the loop (`ListReserve`) |
| `e?` | `SwitchTag` on the `Result` or `Option`; the failing leaf converts the error with the resolved `From` call and `Return`s |
| `while c: body` | `Loop` whose body is `If c then body else Break` |
| `for` over a std numeric range | a counting `Loop` with cursor/end/inclusive locals and ordinary tests and assignments; the cursor advances before the body, and the inclusive endpoint clears its flag instead of overflowing |
| `for` over a built-in list or map | a counted `Loop` over element reads (`Read [%s [%p]]`) or the live-entry intrinsic, of the source value stored in `$coll`: a copy of a place's collection is the snapshot, which the ownership passes lend when nothing in the loop changes the place |
| any other `for` | a `Block`: the `iter` `Call`, then a `Loop` around the `next` `Call` and a `Match` on its `Option` |
| `let p = e else: ...`, `x is p` | `Match` with two arms |
| `x := e`, `let p = e` | a new local and a `Store` of it, or a `Match` for a pattern |
| `Order(x)` newtype call | nothing: the newtype is erased, so construction and unwrapping change no value (`types.newtype.construct`, `types.newtype.construct-ref`, `types.newtype.construct-value`) |
| a type parameter in a type (`T?`) | nothing: the name resolves to the in-scope parameter (`names.type-param.local`) and types erase |
| `$.with`, `$.use`, `$.context` | `With`, `ProviderGet`, `ContextNew` |
| `all!(a(x), b(y))` | two cold `Call`s, then `AwaitAll` |
| `race!(...)` | a `NewList` of cold suspensions, then `AwaitRace` |
| test registrations | `Call`s in a `TestCase` body |
| `f(a): block` (trailing block call) | the ordinary arguments, then `Call` with one more final argument: a `Closure` built from the block, contextually typed by the callee's final zero-argument function parameter |
| top-level `it("name"):` in a test module | test registration, exactly as in a `tests:` block (a `TestCase` body), not an ordinary statement |

A trailing block is a closure in call position. Check resolves the
callee first: its final parameter must have a zero-argument function
type (`fn.trailing.form`), else the form does not apply. The ordinary
arguments check in order, then the block checks as a zero-argument
closure whose result and behavior come from that parameter
(`fn.trailing.closure`); when the parameter is suspending (`fn!`), the
block is a suspending body and may make bang calls
(`fn.trailing.suspending`). `return` inside the block returns from the
callback (`fn.trailing.return`), as in any closure body. At run time the
closure value is created when the call's arguments evaluate, and the
block runs only when the callee invokes it.

In a test module (including one integration test file), a top-level
`it` / `it_each` / `it_prop` / `it_prop_with` trailing call goes through
the same registration as `tests:` blocks (`check_tests`): name and
options from the argument list, the block as a `TestCase` body. Any
other top-level trailing call lowers as an ordinary call.

**Type names in bodies resolve before anything else.** A bare type or
trait name in a body is resolved through scope, prelude and imports
first. An unresolvable name is a rejection, not a value: `list` and `Set`
are `unknown-type` (`names.module.other-module.type`), and `Mailer` or
`Priced` out of scope are `unknown-trait`
(`names.module.other-module.trait`). Only resolvable names reach value
checking: a type parameter in a type erases, and a newtype-name call
unwraps the newtype.

##### The Builder API

```rust
/// What the checker calls (src/ir.rs). `TirBuilder` implements it.
pub trait TirSink {
    fn konst(&mut self, index: u32) -> Ref;
    fn local(&mut self, ty: Ty, name: Symbol, flags: u8, syn: NodeIdx) -> LocalId;
    fn get(&mut self, l: LocalId, ty: Ty, syn: NodeIdx) -> Ref;
    fn set(&mut self, l: LocalId, v: Ref, syn: NodeIdx);
    fn prim(&mut self, op: u32, args: &[Ref], ty: Ty, syn: NodeIdx) -> Ref;
    fn call(&mut self, callee: &Callee, args: &[Ref], prov: Providers, ty: Ty, syn: NodeIdx) -> Ref;
    fn coerce(&mut self, kind: Coercion, evidence: u32, v: Ref, to: Ty, syn: NodeIdx) -> Ref;
    fn emit(&mut self, tag: Tag, a: u32, b: u32, ty: Ty, syn: NodeIdx) -> Ref;
    fn open_block(&mut self) -> BlockMark;
    fn close_block(&mut self, m: BlockMark, tail: Option<Ref>, ty: Ty, syn: NodeIdx) -> Ref;
    fn open_scope(&mut self) -> ScopeMark;
    fn defer(&mut self, suite: Ref, syn: NodeIdx);
    fn close_scope(&mut self, m: ScopeMark, body: Ref, ty: Ty, syn: NodeIdx) -> Ref;
    fn open_loop(&mut self) -> LoopMark;       // allocates the LabelId that Break and Continue name
    fn close_loop(&mut self, m: LoopMark, body: Ref, ty: Ty, syn: NodeIdx) -> Ref;
    fn reserve(&mut self) -> Slot;             // a position in the open block's list, filled later
    fn fill(&mut self, slot: Slot, tag: Tag, a: u32, b: u32, ty: Ty, syn: NodeIdx) -> Ref;
    fn open_sub(&mut self, params: &[LocalId]) -> SubMark;    // a closure's body
    fn capture(&mut self, sub: SubMark, outer: LocalId) -> CaptureId;
    fn close_sub(&mut self, m: SubMark, root: Ref, fn_ty: Ty, syn: NodeIdx) -> Ref;  // emits the Closure
    fn checkpoint(&self) -> TirCheckpoint;
    fn rollback(&mut self, c: TirCheckpoint);
}

impl TirBuilder {
    pub fn new(item: DefId, kind: BodyKind) -> Self;
    /// The root block, the capture modes, then the verifier.
    pub fn finish(self, root: Ref, capture_modes: &[CaptureMode]) -> Result<Body, Vec<VerifyError>>;
}
```

- The builder is the only writer of TIR. An open block's instruction list
  lives on a scratch stack until the block closes.
- **The checker is generic over `TirSink`**, as `Checker<B: TirSink>`, so
  a discarding sink gives a no-emit mode (for the playground and
  `hd fix`) without a second code path.
  Implementation differs: the checker holds a concrete `TirBuilder`
  (`Ck::b`, `crates/hd_check/src/body.rs:165`), and there is no
  discarding sink. Not judged.
- **Open literals.** Literal width is decided per connected literal
  class, so a literal may be emitted before its class binds. The checker
  pushes the constant with its class variable as its type
  (`TirBuilder::const_value`). Before `finish` the checker's final pass
  resolves every `consts` type, as it resolves instruction types
  (`finish_with`, `crates/hd_check/src/body.rs`). No placeholder bits and
  no operand rewrite are needed, since the `Ref` names the row whatever
  its type turns out to be.
- **Captures.** A closure's captures are collected while its body is
  checked, since nested closures interleave, and flushed contiguously
  into the capture columns at `close_sub`.
- Types may be inference variables while a body is built. `coerce` is
  called where bidirectional checking finds a coercion, so coercions are
  never re-derived later.
- **Slots.** An arm's tail coercion is decided at the join, after the arm
  closed. The arm reserves a slot for its tail, and closing the `If` or
  `Match` fills each arm's slot. The verifier rejects an unfilled slot.
  [crates/hd_check/DESIGN.md (type checking)](../hd_check/DESIGN.md) says when the checker reserves.
- **Speculation.** `checkpoint` and `rollback` truncate every column,
  `extra` and the scratch stack together.

##### Invariants

The verifier checks each of these:

1. Every **value** operand precedes its user, and is visible where it is
   used: in the same block earlier, or in an enclosing block. Child
   blocks, labels and locals follow their own rules (5 and 10).
2. Every non-root instruction is listed in exactly one block. A child
   block is listed too, in the block around the instruction that runs
   it, and is also named by that instruction's operand (an `If`,
   `Match`, `Loop` or `Scope`, for example); a closure's root is listed
   in its parent's block the same way. The verifier checks only the listing:
   it rejects a listed instruction that does not exist and one listed
   in two blocks.
3. After `finish`, every type is global. `Hole` and `Poison` occur only
   in a module with errors, which D2 does not lower.
4. **Every operand's type equals the type its position expects**: call
   arguments equal the substituted parameter types, a value stored into
   a bare local equals the local's type, a `Return` value equals the body's result, a
   `Break` value equals its target's type. Coercions are explicit, so any
   mismatch is a checker bug.
5. `Break` and `Continue` labels name a `Loop` or `Block` that encloses
   them, and each label row is written. `ToArm` is inside its `Match`'s
   decision block. `Defer` is inside its `Scope`.
6. `Await`, `AwaitValue`, `AwaitAll` and `AwaitRace` occur only in a
   suspending body or closure. No `Return`, `Break`, `Continue` or
   failing `?` leaf is inside a `defer` suite. A suspension point is
   only where a `using` binding's `dispose!` put it; the checker rejects
   one that source writes in a `defer` suite.
7. Every suspension point's side record lists exactly the `Scope`s that
   enclose it.
8. Every call to a callee with a row has providers for exactly its keys.
   A `NONE` provider names a key that the enclosing sub-body's own row
   holds.
9. `CallHost` occurs only in std's provider bodies; an `Intrinsic` only
   where it is declared.
10. A `Closure`'s captures name locals of its enclosing sub-bodies, and
    after `finish` every capture has a mode.
11. Every switch in a decision tree has a case for each value of its
    type, or a default.
12. A constant `Ref` names a row in this body's constant table whose
    type equals the type its position expects.
13. A `Builtin` choice names a `BuiltinImpl` whose trait is the callee's
    trait.
14. No reserved slot is empty after `finish`.
15. A `DefaultCall`'s operands are the `Ref`s its call passes for the
    earlier parameters, in order, and it precedes the call in the block.
16. (Unused.)
17. A `CallDyn` has one evidence operand per method-level bound.
18. (Unused.)
19. (Unused.)
20. Every place operand is well formed: it decodes, a local root names a
    local of the body, a value root and each element index precede the
    user and are visible there, a frame root occurs only in tier 2, and a
    pointer root is the whole place of a `Read` or a `Store`.
21. A `Store` or a mutable `Ref` reaches a writable place: a local,
    global or pointer root, or a dereference or an element among its
    projections (an element is a slot of the buffer a list header names).
    A value root is never written in place.
22. **The checker's TIR has no ownership**: every `Read` has mode `use`,
    no `Ref` takes a count, and there is no `Drop`, `IsUnique`, `Pin`,
    `FlagSet` or `FlagClear`, no `exit_drops`, `unwind_drops`,
    `stable_lists` or `regions` row.
23. **After the ownership passes, ownership balances**: every owned
    value has exactly one consumer or one `Drop` on each path; no `Read`
    of a place follows its `move` on any path without a `Store` in
    between; no `borrow` value is live across a write to an overlapping
    place or a suspension point. Every `Read` has a decided mode, the exit
    set names locals of the body, sorted, and nothing tier 2 reserves
    occurs.

Implementation differs: `verify` checks invariants 1 to 15, 17 and 20
to 22 (`CHECKED_INVARIANTS`), several in part, and `verify_owned` the
same with 23 in place of 22, for a body after the ownership passes. The checker's facts come in
through `VerifyCx`: `verify_in` and `TirBuilder::finish_in` take them,
and the checker's own `finish` passes them for each body it checks. A
body built elsewhere, such as a generated structure method, gets
`verify` alone.
- **Partial checks.**
  - 1 and 20: only def-before-use.
  - 2: only block lists.
  - 3: that no type is body-local, and no `Hole` or `Poison` in a module
    without errors. The second half runs in the driver's
    `ModuleFinish` (`poison_sites`), not in `finish`: an import's error
    is reported at the folder, outside the body that reads its poisoned
    name. It fires when neither the module nor the run so far reported
    an error.
  - 4: only a `Store` value into a bare local against its local, and a
    `Break` value
    against its target, and not in a module with errors
    (`finish_tolerating`).
  - 5: only that labels exist and are written.
  - 6: only the `defer` suite half (`defer_suite_violations`, which the
    checker also uses to poison what it rejected).
  - 7: only the rows that exist; no body writes one yet.
  - 12: only that the row exists.
  - 15: only the operands, for a `DefaultCall` passed to a `Call` or
    `Await`.
  - 23 (`src/ir/owned.rs`): a `Read` or `IsUnique` or `Pin` after a
    `move` read or a `Drop` of an overlapping place, joined over every
    path and to a fixed point around loops, with a `Store` that covers
    the place refilling it; a `borrow` value live (a backward liveness
    walk over the structured control flow, one bit per borrow, loops to a
    fixed point) at a write of an overlapping place or a suspension
    point, where a use of a value is a use of each borrow it lives on (a
    `borrow` read rooted at a borrow, a read of a binding local), a
    control instruction reads its operands before its blocks run, a
    borrow's own `Drop` is no use, and a write through the borrow's own
    value (into its list's buffer, behind its reference) keeps it; a
    binding local (a `Store` of a borrow) stored with an owned value,
    dropped, moved out of, addressed or in the exit set, and a counted
    parameter outside the exit set (one the body does not own) stored
    into, dropped, moved out of or addressed. Places overlap as
    docs/compiler/tir-v2.md's "Places" says, with no types: two roots
    overlap only when one goes behind a reference or into a buffer and
    the other is where a reference can reach; a `Drop` of a whole local
    empties only the local itself (a reference reaches its cell, which
    the reference keeps); a value's new round refills its places; a
    cleanup scope's suites run from the states at the body's exits. The
    one-consumer clause is a forward walk with three bit sets (owned
    values that may hold a count, values that may have met their
    consumer or `Drop`, owned locals that may hold a value), joined by
    union: a use or a `Drop` after a value's consumer or drop on some
    path, a value still owned where its sub-body ends or where a loop
    that computes it goes round, a local that may hold a value at the
    exit outside the exit set, and at a suspension point an owned value
    outside its unwind set or such a local are errors. A `move` read of
    a whole value consumes it; a `move` read or a `Drop` of a part of a
    value or a local takes that part, which a fourth bit set records
    (one bit per distinct part place): more parts, or a `Drop` of what
    is left, may follow, and nothing else, and once the parts taken on
    the path cover every counted leaf (`OwnCx::parts`) the value or the
    local is empty. A `Ref` that takes its local's count
    empties the local, as a `move` read of it does. Which uses consume is
    `own::operands` and `own::consumes`, with `OwnCx`'s parameter modes
    and counted types; `verify_owned` takes the same `OwnCx` as the
    passes. No `Ref` takes its local's count in a sub-body with a
    suspension point. A hoisted `IsUnique` stands before a loop (among
    other hoisted ones), names a place of an unaddressed local with one
    leading dereference at most and fields, and in that loop nothing
    copies it (a read of it, or of a place holding it, whose value a use
    keeps), references, writes, pins or suspends over it, and no call
    may write unless it is a whole local no closure captures; a covered
    one names a hoisted one's place inside that one's loop; the stable
    list rows name loops and such places, in order. A header that may
    change in place is held by one place (insertion's rule 7): with a
    local changing when an `IsUnique` or a `Pin` names a place rooted
    at it, or a value read from one, or a `&mut` reference takes its
    storage or a place in its list, a value that may hold a list or a
    map stored into a whole changing local is fresh (a copy included) or
    a map value read for a change; and a changing parameter (not a `mut
    self` view of a collection) is first read to be copied and stored
    back.
- **8.** Every call writes `Providers::None` today, and codegen reads
  providers from the callee's signature. So the check rejects a
  `Pending` form after `finish`, and pairs whose count is not the
  callee's closed row's key count. The `NONE`-provider half is not
  checked.
- **9.** Std's provider bodies are generated in Wasm (`hd_wasm`), never
  TIR, so a `CallHost` in any TIR body is rejected. An `Intrinsic` must
  name a known `IntrinsicOp`.
- **11.** A `SwitchTag` with no default must have a case for every
  variant of its scrutinee's enum or optional. A `SwitchInt`,
  `SwitchChar` or `SwitchStr` must have a default. Its cases are
  `(constant Ref, Block)` pairs, and a range is not encoded; no checker
  code emits these three switches.
- **13.** A `Builtin` choice's value is the `BuiltinImpl` plus one
  (`choice_of`), 0 when the solver answered with none, or `u32::MAX` or
  `u32::MAX - 1` while the checker still holds it. A named impl must
  answer the callee's trait.
- **17.** A `CallDyn` record is `[method, type arguments, evidence count,
  evidence..., arguments, providers]`. The evidence count must be the
  method's number of method-level bounds. No checker code emits
  `CallDyn`.
- **Cost.** It runs on every `finish`, not only in debug builds. The
  context checks read one item per call, switch or dynamic call.

##### Lifetime And The `tir` Entry

- A body's TIR lives in its builder until the body finishes, then moves
  into the module result. `ModuleFinish` serializes every body of the
  module into the TIR section of the module's `check` entry. A later
  build reads the entry instead of rechecking.
- **One entry, under `check_key(m)`.** The TIR is a function of the same
  inputs as the diagnostics, so it is a section of the `check` entry,
  not a file of its own. It is written only when the module has no
  error: a module with errors cannot be built, so its TIR would never be
  read.
- Each body is stored under its item's path hash, so `Collect` decodes
  only the bodies it reaches. The module's **TIR content hash** combines
  each body's item path hash and TIR hash with the init summary's hash;
  `prog_key` reads it.
- Types stay generic (`Param`). D2 never materializes an instance as TIR.
- **The ownership passes** (`src/own.rs`, docs/compiler/tir-v2.md "The
  ownership passes") run on every decoded body, once per body, before
  collection reads it; their output never reaches the entry. They read
  places the checker roots at their local (above). Built so far:
  insertion (`own/insert.rs`): copies, drops and
  the exit set, an `IsUnique` before each change of a collection place
  (an element store, a push, a map store or removal, a write through a
  map value) and before each read such a change
  goes through, of every list on that read's path, and an `IsUnique` and
  a `Pin` of each list on an element reference's path (the checker
  writes no pin of its own). A copy of a list or a map is a count of its
  buffer, which the backend takes through the copy glue (stage 3, 3g:
  the former header copies, `own/copies.rs`, went with their
  `ValueCopy`); the passes keep a parameter that a change reaches in
  place owned, and a local a `Pin` names unmoved. Before 3g the header
  copies went where a
  header that may change in place would otherwise be held by two places:
  a store into a whole local that a change reaches in place (a changing
  local), or of a value whose source is not a read of a local no change
  reaches (a changing local's, a global's, a dereference's, a block's),
  but a header that moves out of a dying local that held it alone, and a
  binding stored once and read, in its store's block, only before
  anything can change its source in place (a read of it then stands for
  its source); the function's result, but its own local's that holds no
  shared header; a store through a dereference and a struct update's
  source; a value argument whose source may change while the callee runs (an
  aliased local, a dereference, a global, a local a later argument
  changes), copied right after it; a changing parameter on entry; a
  local a closure captures by value that may share its header;
  then borrow (`own/borrow.rs`): a `copy` read whose value only lends,
  over a span no write of what it reads, call that may reach it or
  suspension point crosses, becomes a `borrow` and loses its `Drop`s
  (one an instance may make fresh keeps them); a local stored once with
  such a read holds a borrow and owns nothing, and so does a lent
  parameter the body never stores (both leave the exit set); then move
  (`own/moves.rs`): a backward liveness walk turns each
  `copy` that no path reads after into a `move` (a `Ref` of a whole
  celled local into a taking one), deletes the `Drop` that follows it
  in its block, and splits a base's drop into its other parts' when a
  part moves; a use of a borrow keeps what it lives on live; then pair
  cancellation (`own/pairs.rs`): a `copy` of a part whose base's `Drop`
  follows in a straight line moves and the drop goes or splits; then
  known-null (`own/known.rs`): a forward walk of which owned locals and
  values may hold a count deletes the `Drop`s no path reaches with one,
  removes the exit rows of locals empty at every way out (a `Return`,
  the root's end, a suspension point; never a parameter's or a
  capture's), and writes each suspension point's unwind set; then
  hoisting (`own/hoist.rs`): an `IsUnique` in a loop's iteration block,
  behind an entry test whose prefix is pure, moves before the loop
  (marked hoisted) when nothing in the loop copies, references,
  replaces, pins or suspends over its place and no call may write unless
  the place is a private local, every other `IsUnique` of the place in
  the loop goes (or stays covered behind a dereference), and the loops'
  stable list places are listed in `stable_lists`. Insertion
  rebuilds the body once, and the other passes record their edits for
  one more rebuild at the end, renumbering it (`own/splice.rs`). They
  read the body and `OwnCx` (counted types, parameter modes, how a body
  receives its own parameters, `may_write`, the in-place parts of a
  type, which types hold a collection in place, and receivers)
  only.
  `own::operands` names each value operand of an instruction with its
  role (lent, consumed, a call argument, a block tail, a drop), and
  `own::consumers` each value's consumer by insertion's rule; the
  verifier and the backend read the same two.

Planned, not built: per item, an **inline summary** (whether the item
passes the trivial inlining test, crates/hd_mono/DESIGN.md §13.8) and a
**dependency list** (the stable paths of the items its TIR names, each
with its per-item interface hash), for code keys.

**The wire form** (`write_body`, `src/wire.rs`): one blob per body.

| Part | Content |
| --- | --- |
| hashed length | `u32`: the byte length of the hashed prefix |
| tables | the body's own entry-local string, path and type tables; every `Symbol`, `DefId` and `Ty` in that body is a row in them |
| item, kind | the item's path row, the body kind |
| columns | `tags`, `data`, `ty`, `extra`, the local, sub-body, label and capture columns, `consts`, `strings`, `label`; ID words of `extra` remapped by `id_words`, other words copied |
| locations | `syn`, `local_syn`, `fallback`: after the hashed prefix |

- **Remap, not copy.** Pool indices, `DefId`s and `Symbol`s are run IDs
  and cannot be written. `id_words` knows the ID-carrying words of each
  tag, and a tag whose words are not mapped is refused with a structured
  error, never written with run IDs. `map_ids` rewrites a decoded body
  into another run's IDs.
- **Checker output only.** The ownership passes run after decoding, so
  the wire refuses their output: an ownership statement, and an
  `exit_drops`, `unwind_drops`, `stable_lists` or `regions` row, are structured errors. A taking `Ref`
  comes only with its local's exit row.
- **The TIR hash** (`tir_hash`) is the hash of the hashed prefix. Because
  tables are per body, it is a function of that body alone, the same on
  every run and thread count. A module pays for this stability by
  repeating strings, paths and types shared by several bodies once per
  body.
- **Positions are not hashed.** The location columns come after the
  hashed prefix, so moving a body, or changing a header so later
  syntax-node indices shift, keeps the hash and reuses its emitted code.
- **Reading.** `read_body` decodes a body and interns its tables into
  the current run's pool.

### 4.14 Diagnostics

**One per root cause.**

1. Poison silences follow-ups.
2. Each diagnostic may carry a `RootKey` (`crates/hd_diag/src/buf.rs`):
   the item, plus the name or span that caused it. Only the first
   diagnostic per root key is kept.
3. An unknown name reports once per body, a broken header once per item, a
   failed import once per use.
4. A header error in module `m` is reported in `m`, never again in its
   dependents, which see poison.

**A real primary span.** Every diagnostic carries a primary span into
source text; a sentinel file or zero-width synthetic placeholder is not a
publishable diagnostic. Not built yet for `folder-cycle`; see #171.

**Order.** Every diagnostic is sorted by package order, file stable path,
start offset, end offset, code and rendered message. The key is total, so
no tie falls back to an ID (tsgo #64589). Exact duplicates are dropped.

**Fix-its.**

- A fix-it is a list of exact edits on byte ranges, marked `Exact` or
  `Suggestion` by the code that makes it.
- **`hd fix`** applies the first exact fix-it of each diagnostic, checks
  the package again, and keeps a round only if no diagnostic occurs more
  often after it; otherwise it tries the fix-its one at a time. At most
  3 rounds. A full check after the edit is a stronger test than
  re-parsing each fix-it at emission, and only `hd fix` applies them.
- Kinds today include did-you-mean names, missing `use` lines, `let mut`,
  inserting `dyn`, and the fix-its the spec names. Code-generating
  fix-its come later.

**Rendering.**

- **Compact text** (default): one line `file:line:col: code: message`, and
  at most one `hint:` line naming the fix. Notes and secondary labels
  appear with `--verbose`. The target is at most 60 tokens per diagnostic
  (the `mistakes` metric).
- **JSON lines** with the fields of
  [Machine Output](../../spec/cli/command-line.md#machine-output).
- **Capping.** `--max-errors N` stops printing after N errors and still
  prints the summary with full counts.
- **`modules_checked`** in the JSON summary is the number of module
  `check` entries computed in this run, not read from the cache. It is an
  operational field: it differs between a cold and a warm run by design.

### 4.15 Limits

Each limit counts language-level units, so a program passes or fails the
same way on every run, thread count and cache state.

| Limit | Counts | Default | Diagnostic |
| --- | --- | --- | --- |
| nesting depth | brackets, blocks, closures and interpolations open at once | 160 (`crates/hd_syntax/src/parser/mod.rs`) | `nesting-too-deep` |
| trait resolution depth | nested subgoals | 64 | `trait-resolution-depth` |
| body fuel | candidates, subgoals, unification steps, matrix cells, trials, joins, obligation retries; memo hits at their stored cost ([crates/hd_check/DESIGN.md (type checking) §11.1](../hd_check/DESIGN.md#111-what-counts)) | `Fuel::BODY_DEFAULT`, 2^24 steps per body (`crates/hd_base/src/lib.rs`) | see below |
| embedding depth | nested `data` embedding | the spec's | `embedding-too-deep` |
| instantiation depth | nesting depth of an instance's type arguments; length of the request chain from a root; also row keys grown by polymorphic recursion | 32; 256 | `instantiation-too-deep`: a build error, and for rows a check error |

A body that runs out of fuel is `item-too-complex` naming the item, with
its TIR rolled back to one `Poison`
([crates/hd_check/DESIGN.md (type checking) §11.2](../hd_check/DESIGN.md#112-running-out)).

Planned, not built: `file-too-large` (16 MiB), `type-too-large` (10,000
nodes in one type) and `match-too-complex` (exhaustiveness out of body
fuel). The codes exist in `hd_diag`; nothing emits them.
