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

- **Coercions** (implicit `Option` wrapping, readonly views, function row
  subsumption) become explicit TIR instructions, so D2 never re-derives
  them.
- **Mutability and access** checks run during checking, on the
  checker's own access types and local flags, never over TIR
  ([Mutable Paths](../../spec/lang/04-type-system.md#mutable-paths);
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
`mutable-upgrade` test of `req.mut.install-mutable`, and then coerces
`e` to the key's trait value type `K` (`ToTraitValue`, or `Supertrait`
from a subtrait value, or nothing from a `K` value), so a value whose
type lacks the impl is `type-mismatch`. An entry `ctx...` needs a value
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
  ([`req.bang.driver-contexts`](../../spec/lang/11-requirements-and-suspension.md#r-req.bang.driver-contexts)).
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
`TupleGet`, as for any tuple. Otherwise the checker emits `CallValue f,
[args]`: one argument, the inputs tuple, under a callee whose TIR type
keeps the third form. Instantiation substitutes a tuple for every
`Args`, so the callee's instance type has one parameter per element. The
emitter sees the third form on the callee's unsubstituted type and
passes the tuple's elements as the arguments, read as `TupleGet` reads
them, boxed tuple or not. There is no runtime machinery and no new TIR
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
  never on its own standard error (`module.testing.err-print`). A `?`
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
    pub moves: Vec<u32>,                // the LocalGet and Field instructions that move an owned place, sorted
    pub drop_mode: Vec<DropMode>,       // Always | Flag | Never, one per DropLocal in instruction order
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
  a record in `extra`. Def-before-use and visibility (invariant 1) apply
  to value edges only. A label is allocated when its loop or block opens,
  before the body, so `Break` and `Continue` never point forward; the
  label row is written once when the target closes.
- **Values are instructions.** An instruction's result is its value, used
  by later instructions through a `Ref`. Mutable user bindings are locals
  read with `LocalGet` and written with `LocalSet`.
- **Constants are `Ref`s, not instructions.** A `Ref` with bit 31 set
  names a row in that body's `consts` table: the constant's type and 64
  value bits; a `string` constant's bits index `strings`. There is no
  `Const` instruction.
- **Blocks list instructions.** A `Block` holds a record in `extra` of
  the instructions it runs, in evaluation order. Every ordinary
  instruction is in exactly one block.
- **Owned blocks are not siblings.** A `Block` named by an `If`, `Match`
  or `Loop` child-block operand is owned by that instruction and does not
  also occur in the enclosing block's list. The owner instruction
  occupies the enclosing list position.
- **Types** are pool indices. While a body is checked they may be
  inference variables; the checker's final pass resolves them.
- **Capture modes** depend on statements after the closure, so they are
  a column written once at `finish`. The checker decides the modes
  (rule TC-1); the builder only stores them. No instruction word is
  patched.
- **Sizes.** 17 bytes per instruction plus its `extra` records; 13 per
  local.

Planned, not built: an `origin` side table (generated code to its origin)
and a `hole` side table (typed-hole candidates).

##### Instruction Catalog

Notation: `a` and `b` are the two data words; `[...]` is a record in
`extra`. "Join" is the arms' common type by the spec's join rules.

**Values, locals and globals**

| Tag | Operands | Type rule |
| --- | --- | --- |
| `LocalGet` | a: local | the local's type, as a readonly view when the binding is readonly |
| `LocalSet` | a: local, b: value | `void`; the value's type equals the local's |
| `GlobalGet` | a: `[binding DefId]` | the binding's type |
| `GlobalSet` | a: `[binding DefId]`, b: value | `void`; only in init bodies and for mutable top-level bindings |
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
| `Is` | a: left, b: right | `bool`. Only on two operands whose types implement `AnyRef` (data, `List`, `Map`, an `AnyRef` type parameter), or on trait values and `Any`. `is` on an enum, optional, `Result`, tuple, primitive, string or function value is `identity-requires-references` before TIR ([`expr.is.value-operand`](../../spec/lang/05-expressions.md#r-expr.is.value-operand)); codegen lowers it by layout (crates/hd_wasm/DESIGN.md §15.2) |
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
| `ToTraitValue` | `S` → `Tr`, `mut S` → `mut Tr` | the impl choice | box with its dispatch table |
| `ToAny` | `S` → `Any` | none | box with its type id |
| `Supertrait` | child trait value → parent trait value | none: the target type names the parent | re-table: load the parent's vtable from the child's |
| `SuspendFnToCtor` | `fn!` type → constructor type | none | none, or a thin adapter |
| `Identity` | `T` → `T` | none | none: a least-common-type slot whose value needed no conversion |
| `Lend` | a place's value → `^T` ([`own.body.lend`](../../spec/lang/13-ownership.md#r-own.body.lend)) | none | none: the borrow is the same reference |
| `LendMut` | a place's value → `^mut T` | none | none: the borrow is the same reference |

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
  an indirect `block_on` or `println` panic (crates/hd_wasm/DESIGN.md
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
| `NewData` | b: `[field values]` in declaration order | the data type in `ty` |
| `CopyData` | a: source, b: `[(field, value)]` replacements | the source's type. Copy-update literals and part copies; every part not replaced is copied too ([`data.part.copy-update`](../../spec/lang/08-data-and-enums.md#r-data.part.copy-update)) |
| `NewVariant` | a: variant declaration index, b: `[payload values]` | the enum type in `ty` |
| `NewTuple` | a: rest-list operand or `NONE` (tuple spread), b: `[elements]` | the tuple type; with a rest operand, its rest element |
| `NewList` | a: `[spread positions]` or `NONE`, b: `[elements]` | `List[T]` |
| `NewMap` | b: `[key, value pairs]` | `Map[K, V]` |
| `Field` | a: base, b: field index (parts included) | the field's type, with the base's access |
| `FieldSet` | a: base, b: `[field, value]` | `void`; the base is mutable |
| `TupleGet` | a: base, b: index | the element's type |
| `Closure` | a: sub-body, b: captures as a `CaptureId` range | the closure's function type. Each capture's mode, in the `cap_mode` column, is `Copy` (no side writes it after the capture), `Move` (only the closure uses it afterward) or `Shared` (both may) |

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
| `Scope` | a: body `Block`, b: `[defer suites and drops]`: a suite `Block`, or `DROP_ENTRY` (bit 31) plus a `DropLocal` the body block lists, in instruction order | the body's type. A cleanup scope ([`flow.defer.scopes`](../../spec/lang/06-control-flow.md#r-flow.defer.scopes)) |
| `Defer` | a: suite number in the enclosing `Scope` | `void`; registers that suite here |
| `If` | a: condition, b: `[then Block, else Block]` | the join of both |
| `Loop` | a: body `Block`, b: else `Block` or `NONE` | the join of its `Break` values, or `void` |
| `ForRange` | a: `[start, end, kind, loop local]`, b: `[body, else]` | as `Loop`; `kind` is half-open, inclusive or from |
| `ForList`, `ForMap` | a: `[collection, item locals]`, b: `[body, else]` | as `Loop` |
| `Break` | a: target label, b: value or `NONE` | `never` |
| `Continue` | a: target label of a `Loop` | `never` |
| `Return` | a: value, or `NONE` for a bare `return` in a `void` body | `never` |
| `Unreachable` | none | `never`; the default of an exhaustive switch |

**Ownership** ([Ownership and Borrows](../../spec/lang/13-ownership.md)).
The checker decides every ownership fact and the builder records it, so
collection and emission never derive one again.

| Tag | Operands | Type rule |
| --- | --- | --- |
| `DropLocal` | a: an owned local | `void`; registers the local's drop in the enclosing `Scope`, where the local is bound |
| `DropTemp` | a: an owned temporary | `void`; drops it at the end of its statement |

A block that binds an owned local, and a function with an owned
parameter, is a `Scope` whose suites include those `DropLocal`s
(`TirSink::drop_mark` and `close_drops` wrap the statements after the
fact, so a block without one is built as before). Bindings and `defer`
suites thus drop in one last-in, first-out order, and the exit ladder
and the cancellation record that serve `defer` serve drops too. A move is
a `LocalGet` or `Field` of an owned place that no `Lend` wraps; the
ownership pass lists them in the `moves` column and decides each
`DropLocal`'s `drop_mode` (always, by flag, or never), both written at
`finish`; until it runs, every mode is `Always`. Emission runs nothing
for these forms yet.

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
| `Payload` | a: a value switched to a variant, b: `[variant declaration index, field]` | the payload field's type |
| `Unwrap` | a: an `Option` value switched to `.Some` | the inner type |
| `Guard` | a: condition `Block`, b: `[arm, fail Block]` | `never`; the fail block continues with the remaining rows |
| `ToArm` | a: arm number | `never`; the leaf has already bound the arm's locals with `LocalSet` |

The checker emits each arm's `Block` while it checks the arm, then builds
the decision tree after exhaustiveness and emits it as these switch
instructions. Shared arms appear once.

**Wire-visible identities.** `NewVariant`, `SwitchTag` and `Payload`
identify a variant by its declaration index within the enum, not by
`DefId`; the enclosing value or result type identifies the enum. The run
IDs used by `ProviderGet`, `ItemRef`, `GlobalGet`, `GlobalSet`,
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
| `x += y`, and the other compound assignments | the place's operands evaluated once (their `Ref`s used twice), `Prim` or a trait `Call`, then `LocalSet`, `FieldSet` or a set `Call` |
| `a ** b` on integers | a `Prim` integer-power operation: checked multiplication in the base's result type (`expr.power.checked`, `expr.power.int.*`); float `**` is the IEEE `pow` hook (`expr.power.float.pow`) |
| `a ** b` otherwise | the trait `Call` |
| `a..`, `..b` range patterns | bound comparisons in the decision tree (`n >= a`, `n < b`), no range value built (`flow.match.range.from`, `flow.match.range.to`, `flow.match.range.no-bind`) |
| `..` full-range expression | `RangeFull {}`: a `NewData` of the fieldless struct (`expr.range.form.full`) |
| `(a, rest...)` tuple pattern | the `Match` decision tree, with the rest subpattern binding the trailing elements as a `List` (`flow.match.spread.rest`, `flow.match.spread.cover`) |
| `p = e` through a call (`count() = 1`) | the callee evaluated to a place, then the store, in `expr.assign.order.call` order, with the evaluate-once machinery of compound assignment (`expr.assign.target`, `expr.assign.order`) |
| `Variant(pos, named=…)` mixed construction | checked by call conventions (positionals first), then `NewVariant` with payloads in order (`data.enum.construct.call`) |
| `(1, 2, 3, 4)` against `(usize, usize, List[i32]...)` | `NewTuple` with fixed elements plus the rest list as its rest operand, checked against the rest type (`types.tuple.rest.*`) |
| `x.E ...= e` | `CopyData`, then `FieldSet` |
| `T { ...base, f: e, ... }` | `base` evaluated first, then a `Field` read of each copied field when an explicit field expression may run code (`expr.update.copy-time`), then the explicit field expressions in source order, then `CopyData(base, [(f, e), ...])` with those reads among the replacements (copy-update; one spread max, first) |
| `[a, xs..., b]` | `NewList` whose first operand records the spread positions; each element evaluated once, in order |
| `(a, xs...)` | `NewTuple` with fixed elements `[a]` and the `xs` operand (a `List[T]`) as rest |
| `f(xs...)` at a vararg | the `xs` value itself as the vararg's collected value |
| `f(a, b, xs...)` after a tuple vararg's arguments | `a`, `b` as ordinary arguments, `xs` as the rest-element list |
| `f(xs...)` before fixed parameters | `xs` evaluated once into a temp, then ordinary arguments: `TupleGet`s for the fixed inputs, the rest list for a `List[T]` vararg |
| `[for x in xs if c => e]`, map comprehensions | a `Block`: an empty `NewList` or `NewMap`, a loop, a push `Call` |
| `e?` | `SwitchTag` on the `Result` or `Option`; the failing leaf converts the error with the resolved `From` call and `Return`s |
| `while c: body` | `Loop` whose body is `If c then body else Break` |
| `for` over a std range, list or map | `ForRange`, `ForList`, `ForMap`, chosen when the iterable's impl resolves to std's |
| any other `for` | a `Block`: the `iter` `Call`, then a `Loop` around the `next` `Call` and a `Match` on its `Option` |
| `let p = e else: ...`, `x is p` | `Match` with two arms |
| `x := e`, `let p = e` | a new local and `LocalSet`, or a `Match` for a pattern |
| `Order(x)` newtype call | nothing: the newtype is erased, so construction and unwrapping are permission-carrying views (`types.newtype.construct`, `types.newtype.construct-ref`, `types.newtype.construct-permission`, `types.newtype.unwrap-permission`); a permission upgrade is `mutable-upgrade`, not a conversion |
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
converts with permissions.

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
2. Every non-root instruction has exactly one structural owner: either
   one block list or one `If`, `Match` or `Loop` child-block operand,
   never both. The verifier builds this ownership map and rejects
   missing, duplicate and cyclic ownership.
3. After `finish`, every type is global. `Hole` and `Poison` occur only
   in a module with errors, which D2 does not lower.
4. **Every operand's type equals the type its position expects**: call
   arguments equal the substituted parameter types, a `LocalSet` value
   equals the local's type, a `Return` value equals the body's result, a
   `Break` value equals its target's type. Coercions are explicit, so any
   mismatch is a checker bug.
5. `Break` and `Continue` labels name a `Loop` or `Block` that encloses
   them, and each label row is written. `ToArm` is inside its `Match`'s
   decision block. `Defer` is inside its `Scope`.
6. `Await`, `AwaitValue`, `AwaitAll` and `AwaitRace` occur only in a
   suspending body or closure. No suspension point, `Return`, `Break`,
   `Continue` or failing `?` leaf is inside a `defer` suite.
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
18. A `DropLocal` names a local whose type may need a drop, and exactly
    one `Scope` lists it; `drop_mode` has one row per `DropLocal`. A
    `DropTemp`'s operand may need a drop.
19. Each row of `moves` names an owned place, a `LocalGet` or `Field`,
    and the rows are sorted.

Implementation differs: `verify` checks invariants 1 to 15 and 17 to 19
(`CHECKED_INVARIANTS`), several in part. The checker's facts come in
through `VerifyCx`: `verify_in` and `TirBuilder::finish_in` take them,
and the checker's own `finish` passes them for each body it checks. A
body built elsewhere, such as a generated structure method, gets
`verify` alone.
- **Partial checks.**
  - 18 and 19: the types (may need a drop, owned) only with the
    checker's facts (`VerifyCx::owns`).
  - 1: only def-before-use.
  - 2: only block lists.
  - 3: that no type is body-local, and no `Hole` or `Poison` in a module
    without errors. The second half runs in the driver's
    `ModuleFinish` (`poison_sites`), not in `finish`: an import's error
    is reported at the folder, outside the body that reads its poisoned
    name. It fires when neither the module nor the run so far reported
    an error.
  - 4: only a `LocalSet` value against its local and a `Break` value
    against its target, and not in a module with errors
    (`finish_tolerating`).
  - 5: only that labels exist and are written.
  - 6: only the `defer` suite half (`defer_suite_violations`, which the
    checker also uses to poison what it rejected).
  - 7: only the rows that exist; no body writes one yet.
  - 12: only that the row exists.
  - 15: only the operands, for a `DefaultCall` passed to a `Call` or
    `Await`.
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
| ownership | `moves`, `drop_mode`: only when either is not empty, so a body without marks encodes as before; the reader finds them before the hashed length ends |
| locations | `syn`, `local_syn`, `fallback`: after the hashed prefix |

- **Remap, not copy.** Pool indices, `DefId`s and `Symbol`s are run IDs
  and cannot be written. `id_words` knows the ID-carrying words of each
  tag, and a tag whose words are not mapped is refused with a structured
  error, never written with run IDs. `map_ids` rewrites a decoded body
  into another run's IDs.
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
