# New Compiler Design: Body Checking And The TIR

Part of the [compiler design](README.md).

The detailed type-checking design is [type-checking.md](type-checking.md)
(frontend lane); this file keeps the D1 summary and the TIR definition.

**Changes in this pass** (backend lane, 2026-10-07). TIR's encoding is
rewritten as columns with exact sizes, constants as `Ref`s with no
column, a capture table, a sub-body table, side tables and a wire form
with ID remapping (§4.13.11, and
[data-structures.md §3.18](data-structures.md#318-tir)). The items of
[type-checking.md §17](type-checking.md#17-changes-needed-in-compilerdesignmd)
that touch this file are applied: item 2 in §4.13.1; item 3 in §4.13.2;
item 4 in §4.13.3 and §4.13.10; item 5 in §4.13.4; item 6 in §4.13.6;
items 7, 8 and 9 in §4.13.11; item 10 in §4.15. Item 1 is in
[data-structures.md §3.4](data-structures.md#34-types).

### 4.13 Body Checking

#### 4.13.1 Task Structure Per Module

| Phase | Task | Serial or parallel | Work |
| --- | --- | --- | --- |
| M1 | `ModulePrep(m)` | serial within the module | full parse if needed; module scope; lower private headers; the module's local impl tables; infer the results of private callables with omitted result types |
| M2 | `Body(m, i)` | parallel across all bodies of all modules | check one body against frozen tables |
| M3 | `ModuleFinish(m)` | serial within the module | solve inferred rows, run deferred row checks, write the init summary, sort diagnostics, build `ModuleResult` |

**Each body is checked exactly once per run** (type-checking.md rule
TC-4). A body that M1 checks for its result type is final; M2 does not
check it again. M1 checks the module's top-level statements first,
because unannotated top-level bindings get their types there
([type-checking.md §1.7](type-checking.md#17-body-tasks-and-the-exactly-once-rule)).

**Omitted result types (M1).** A private function without a result type
must be checked before its callers. M1 checks these functions in source
order. When one calls another that is not done, M1 checks the callee
first, depth first. A callee already in progress closes a cycle:
`recursive-function-needs-result-type`, reported at the cycle's first
function in source order, whichever function was entered first. The walk
is serial and bounded by the module's item count.

**Omitted rows (M3; mine).** Rows do not order checking. A call to a
private callable with an omitted row gives the call a row variable
`RowVar(callee)`. Each body records constraints: `RowVar(f) ⊇ keys` for the
keys `f`'s body uses outside `$.with` blocks, `RowVar(f) ⊇ RowVar(g)` for a
call from `f` to `g`, and a deferred check `available(h) ⊇ RowVar(g)` for a
call from an annotated `h`. M3 solves the variables as a least fixpoint
over the module's constraint graph, one SCC at a time, with monotone union
and at most one pass per key per SCC
([`req.row.omitted.cycle`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.omitted.cycle)).
Then it runs the deferred checks and fills the pending provider lists in
TIR (§3.9.5).

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
  [type-checking.md §2.4 and §3.4](type-checking.md#24-calls-and-use-site-type-arguments).
- **Tables.** Union-find over `InferVar` with path halving and rank, a
  binding per root, and the first span that decided it, for blame. Literal
  widths follow [Literal Inference](goals.md#literal-inference): literal
  classes in union-find, the family rule for methods on open literals,
  and no statement retry
  ([type-checking.md §3.6](type-checking.md#36-literal-widths)).
- **Speculation without copies.** Trying a candidate pushes bindings on a
  trail, and failure pops back to a mark. No checker state is cloned (the
  prototype's F-626).
- **No overloading, no disjunctions.** Every call resolves to one callee by
  name and receiver type, so there is no search over alternatives (the
  Swift lesson).

#### 4.13.3 Expressions And Statements

The checker follows the spec chapters in order. Points that matter for the
design:

- **Coercions** (implicit `Option` wrapping, readonly views, function row
  subsumption) become explicit TIR instructions, so D2 never re-derives
  them.
- **Mutability and access** checks run during checking, on the
  checker's own access types and local flags, never over TIR
  ([Mutable Paths](../../spec/lang/04-type-system.md#mutable-paths);
  [type-checking.md §8](type-checking.md#8-mutability-access-and-initialization)).
- **Typed holes.** `_` and `todo()` record the expected type and the
  in-scope names whose types fit, for the hole diagnostic.
- **Definite initialization** of locals and the `let-else` rules are
  computed while checking, syntax-directed over structured control flow,
  with the bits on the trail. They read no TIR, so checking does not
  depend on emission (type-checking.md rule TC-3).

#### 4.13.4 Rows

- A row is one pool item: sorted keys, declared row parameters, and, in
  the body-local pool only, pending private rows `RowVar(f)` minus a key
  set
  ([data-structures.md §3.4](data-structures.md#34-types)). Keys are kept
  sorted by `Ty` value for in-run set operations, so union and membership
  are linear merges ([Row Sets](../../spec/lang/11-requirements-and-suspension.md#row-sets)).
- Bodies record `Uses`, `Includes` (with its `minus` keys) and `Entails`
  facts, which M3 solves
  ([type-checking.md §5.5](type-checking.md#55-private-rows-and-the-m3-fixpoint)).
  A row pattern whose keys mention a type parameter cannot be matched
  against a pending row in M2; that is `cannot-infer-type` with a fix-it
  that writes the callee's `$` clause (type-checking.md §5.6).
- Printing and hashing re-sort keys by stable content, never by `Ty` value
  (the tsgo lesson).
- Entailment is membership after alias expansion
  ([Entailment](../../spec/lang/11-requirements-and-suspension.md#entailment)).
- A pattern with one unknown row parameter takes the least solution
  ([Least Row Solutions](../../spec/lang/11-requirements-and-suspension.md#least-row-solutions)).
- `$.with` blocks push lexical keys onto the body's `available` set.

#### 4.13.5 Suspension

- `!` is part of the name, so suspension needs no inference.
- A bang call outside a driver context is `bang-call-outside-suspension`
  ([`req.bang.driver-contexts`](../../spec/lang/11-requirements-and-suspension.md#r-req.bang.driver-contexts)).
- **The `block_on` ban is direct-only** (answer 13). In `defer` suites,
  default expressions, fact expressions and module initialization, a call
  whose resolved callee is `block_on` or `println` is an error. The check
  reads resolved callees in those contexts and no other body. An indirect
  call panics at run time; D2 emits that check.
- `Suspend[T]` values, `all!` and `race!` are typed as the spec says;
  their lowering is D2's.

#### 4.13.6 GADT Refinement

- Per `match` arm, unify the variant's result type with the scrutinee's
  type, first-order and nominal
  ([Refinement Algorithm](../../spec/lang/13-gadts.md#refinement-algorithm)).
- The equalities this yields on the scrutinee's type parameters are pushed
  on the trail and popped at the arm's end, so they never escape the arm.
  The pop is **scoped**: it undoes the arm's equalities and keeps
  ordinary bindings, and the escape check scans the bindings above the
  arm's mark
  ([type-checking.md §6.1](type-checking.md#61-arm-local-equalities)).
- A variant whose result cannot unify is impossible. Exhaustiveness skips
  it.
- Existential parameters get fresh rigid variables per arm.

#### 4.13.7 Tuples, Varargs And Arity

There are no variadic generics ([chapter 12](../../spec/lang/12-variadic-generics.md)).
`Args < Tuple` bounds an ordinary type parameter, a vararg's type is a
tuple, and a spread `f(args...)` unifies a tuple with a parameter list.
Tuple rest elements are the `rest` field of the tuple type. Tuple `Eq`,
`Ord` and `Hash` come from compiler-derived candidates at every arity.

#### 4.13.8 Exhaustiveness

Maranget's usefulness algorithm over the pattern matrix, with GADT
impossibility (§4.13.6), literal ranges, `Option` and tuples.

- Each matrix cell visited costs one fuel step, so a pathological match
  stops with the match limit diagnostic instead of hanging (§4.15).
- The missing-case witness becomes the diagnostic's example, and later a
  code-generating fix-it.

#### 4.13.9 Derive Instances, Templates, Facts And Test Overlays

- **Derive instances.** Each `@derive(X)` on a type whose trait has a
  template is a body task in the type's module. It instantiates `X`'s
  template from the trait's blob for the target, then checks it with the
  template's names taken from its resolution table and the target's
  members known ([Templates](../../spec/lang/14-annotations.md#templates)).
  The result is part of the target module's `check` entry. A template edit
  changes the trait folder's deep hash, which rechecks exactly the modules
  that derive from it.
- **Facts and defaults.** A fact or default expression is checked once, in
  its declaring module, as a requirement-free expression. Its value is
  D2's.
- **Test overlay.** The `tests:` block and the doc tests of module `m` are
  checked by a `TestOverlay(m)` task under `hd check --tests` and
  `hd test`. Their uses make no folder edge, so the task waits for the
  interfaces they use, and is cached as `check-test`. Doc tests are
  extracted from `##` blocks into synthetic files whose spans map back to
  their lines.

#### 4.13.10 Module Initialization

Definite initialization of top-level bindings follows transitive read sets
through function bodies
([`module.init.definite`](../../spec/lang/10-modules.md#r-module.init.definite)).
Inside one module, M3 computes it from that module's TIR. But an
initialization group that spans several modules of a folder, through a use
loop, needs the bodies of all of them. That is a body-derived fact across
modules, which the research did not list.

**Design (mine).** M3 writes an **init summary** per module, from the
`InitFacts` the checker records while it checks (never from TIR): for
each function and top-level statement, the top-level bindings it reads
directly, the same-folder functions it calls, and the trait methods it
dispatches ([`module.init.definite.dispatch`](../../spec/lang/10-modules.md#r-module.init.definite.dispatch)).
An `InitOrder(F)` task runs only for a folder whose use graph has a
multi-module loop with top-level statements. It reads the summaries,
orders statements by [Order Inside A Group](../../spec/lang/10-modules.md#order-inside-a-group),
and reports `top-level-read-before-initialization`. Its key is the sorted
summary hashes, so a body edit that does not change what a function reads
reruns nothing. Groups never span folders, so the fact stays inside one
folder.

#### 4.13.11 The Typed IR (TIR)

TIR is the one typed IR of a body. The checker emits it directly while it
checks, as Zig's Sema emits AIR and Carbon's checker emits SemIR. D2
consumes it: collection reads it, and emission walks it under a type
substitution to write Wasm (§13). There is no other tree between the
syntax tree and Wasm (§3.9.1). This section is the contract between the
checker's design and D2: the checker builds TIR only through the builder
API below, and D2 relies only on the invariants below. Its contract is
§3.10.1's TIR row.

A **body** is a function or method, an init group's statements, a test
case, a fact or a default expression. A closure is a **sub-body** of the
body that contains it, in the same columns.

##### Encoding

The structure's design card (lifetime, growth, memory, accessors) is
[data-structures.md §3.18](data-structures.md#318-tir). This is the
encoding.

```rust
pub struct TirBody {                    // one body in the module result; the worker's TirColumns has the same fields as Vecs
    pub item: DefId,
    pub kind: BodyKind,                 // u8: Fn | Init | TestCase | Fact | Default | DeriveInstance
    // instructions (§3.9.3), by Inst
    pub tags:  Col<TirTag>,             // 1 B
    pub data:  Col<[u32; 2]>,           // 8 B; operands, meaning set by the tag
    pub ty:    Col<Ty>,                 // 4 B; result type; `void` or `never` for statements
    pub syn:   Col<NodeIdx>,            // 4 B; the syntax node, for spans and sites
    pub extra: Col<u32>,                // operand lists and records, each `{start, len}` or a fixed record
    // locals: parameters, user bindings, pattern bindings, by LocalId (shared with the checker, data-structures.md §3.19)
    pub local_ty: Col<Ty>, pub local_name: Col<Symbol>, pub local_syn: Col<NodeIdx>,
    pub local_flags: Col<LocalFlags>,   // u8: READ | ASSIGNED | MUTATED | CAPTURED | CAPTURED_ASSIGNED | PARAM
    // sub-bodies, by SubId: 0 is the body itself, then one per closure in creation order
    pub sub_root:   Col<Inst>,          // the root Block
    pub sub_params: Col<Range32>,       // a range of `extra` holding parameter LocalIds
    pub sub_parent: Col<SubId>,         // NONE for 0
    pub sub_flags:  Col<u8>,            // SUSPENDS | HAS_ROW
    // captures, by CaptureId; one closure's captures are contiguous
    pub cap_local: Col<LocalId>,        // the outer local
    pub cap_mode:  Col<CaptureMode>,    // u8: Copy | Move | Shared; written once, at finish
    // side tables, each sorted by Inst
    pub susp:   Col<SuspRow>,           // 16 B: inst, scope chain Range32 in extra, hook site
    pub origin: Col<(Inst, u32)>,       // generated code -> origin record in extra
    pub hole:   Col<(Inst, Range32)>,   // typed-hole candidates (DefIds and LocalIds) in extra
}
#[derive(Copy, Clone)] pub struct Inst(u32);
#[derive(Copy, Clone)] pub struct Ref(u32);   // bit 31 clear: an Inst's value; set: a global pool constant (bits 0..30)
pub struct LocalId(u32); pub struct SubId(u32); pub struct CaptureId(u32);
#[repr(u8)] pub enum TirTag { /* generated from tir.ir */ }

const _: () = assert!(core::mem::size_of::<TirTag>() == 1);
const _: () = assert!(core::mem::size_of::<[u32; 2]>() == 8);
const _: () = assert!(core::mem::size_of::<SuspRow>() == 16);
```

- **Values are instructions.** An instruction's result is its value, used
  by later instructions through a `Ref`. Mutable user bindings are locals
  read with `LocalGet` and written with `LocalSet`.
- **Constants are `Ref`s, not instructions (mine).** A global constant's
  pool `Index` uses at most 31 bits (data-structures.md §3.9.2), so a
  `Ref` with bit 31 set names it directly. D1's `consts` column and the
  `Const` tag are gone: one form for constants, no instruction per
  literal. A literal's span, which only diagnostics need, is known to
  the checker when it reports.
- **Blocks list instructions.** A `Block` holds a `{start, len}` range in
  `extra` of the instructions it runs, in order. Every instruction is in
  exactly one block. The order of a block's list is evaluation order.
- **Types** are pool indices (data-structures.md §3.9.2). While a body is
  checked they may be body-local; the final sweep makes them global.
- **Records in `extra`** have fixed word counts, generated and asserted
  from `tir.ir`: a callee record is 2 to 5 words plus its type-argument
  list, a coercion record 2 words, a provider list `{start, len}`.
- **Capture modes** depend on statements after the closure, so they are
  a column written once at `finish`, from the checker's `Solution`
  (type-checking.md §17 item 8; mine in this form). The checker decides
  the modes (rule TC-1); the builder only stores them. No instruction
  word is patched.
- **Sizes.** 17 bytes per instruction plus about 7 bytes of `extra`;
  13 per local; 17 per sub-body; 5 per capture. About 180 bytes per
  source line.

##### Instruction Catalog

Notation: `a` and `b` are the two data words; `[...]` is a record in
`extra`. "Join" is the arms' common type by the spec's join rules.

**Values, locals and globals**

| Tag | Operands | Type rule |
| --- | --- | --- |
| `LocalGet` | a: local | the local's type, as a readonly view when the binding is readonly |
| `LocalSet` | a: local, b: value | `void`; the value's type equals the local's |
| `GlobalGet` | a: `DefId` of a top-level binding | the binding's type |
| `GlobalSet` | a: `DefId`, b: value | `void`; only in init bodies and for mutable top-level bindings |
| `ItemRef` | a: `DefId`, b: `[type arguments]` | the item's function type, instantiated (functions, variant constructors, method references) |
| `ProviderGet` | a: key type | the key's provider type (`$.use(K)`) |
| `Hole` | a: expected type | the expected type; only in a module with errors |
| `Poison` | none | poison; only in a module with errors |

**Operators and calls**

| Tag | Operands | Type rule |
| --- | --- | --- |
| `Prim` | a: `PrimOp`, b: `[operands]` (one or two) | arithmetic, bitwise and shifts: the operand type; comparisons: `bool`; conversions: the target. Only on primitive types |
| `And`, `Or` | a: left, b: right `Block` | `bool`; the right block runs only when needed |
| `Call` | a: `[callee]`, b: `[arguments, providers]` | the callee's result, substituted; `mut Suspend[T]` when the callee suspends and the call is plain (a cold call) |
| `CallValue` | a: function value, b: `[arguments, context]` | the function type's result |
| `CallDyn` | a: trait value, b: `[method, arguments]` | the method's result |
| `CallHost` | a: host method, b: `[arguments]` | the method's result; only in std's provider bodies (§17.1) |
| `Intrinsic` | a: intrinsic, b: `[type arguments, arguments]` | the intrinsic's declared result |
| `Default` | a: `DefId` of the parameter or field, b: `[type arguments]` | the parameter's or field's type |
| `Interp` | b: `[parts]`, each a literal constant or a value with its `Display` callee | `string` |
| `Coerce` | a: value, b: `[kind, evidence or NONE]` | the target type in `ty`; the kinds are in the table below |

**Coercion kinds** (type-checking.md §4.2 and §17 item 7). Each changes
the type, so each is an explicit instruction and invariant 4 holds.

| Kind | From → to | Evidence word | Run-time meaning for D2 |
| --- | --- | --- | --- |
| `Never` | `never` → any | none | unreachable |
| `Weaken` | `mut T` → `T` | none | none: a static view change |
| `Variance` | `C[A]` → `C[B]` by declared variance, readonly outer type | none | none ([Representation-Preserving Variance](../../spec/lang/04-type-system.md#representation-preserving-variance)) |
| `WrapSome` | `T` → `T?`, one layer | none | build `.Some` |
| `RowSubsume` | `fn ... $ R1` → `fn ... $ R2` | none | an adapter that passes only `R1`'s providers |
| `ToTraitValue` | `S` → `Tr`, `mut S` → `mut Tr` | the impl choice | box with its dispatch table |
| `ToAny` | `S` → `Any` | none | box with its type id |
| `Supertrait` | child trait value → parent trait value | none: the target type names the parent | re-table: load the parent's vtable from the child's |
| `SuspendFnToCtor` | `fn!` type → constructor type | none | none, or a thin adapter |

D1's "readonly view" kind is `Weaken`, since the spec's marked form is
`mut T` (data-structures.md §3.4).

A **callee record** is one of `Item(DefId, type arguments)`,
`TraitMethod(trait, method, self type, type arguments, choice)`, and
`Evidence(value, bound, method)` for a call through a GADT existential's
stored evidence. The **choice** is one word, a 2-bit kind and a 30-bit
value:

| Choice | Value | Meaning |
| --- | --- | --- |
| `Impl` | the impl's `DefId` | a written, derived or template impl |
| `Bound` | the bound's index in the parameter environment | dispatch through a bound in scope; static in every instance |
| `Builtin` | a `BuiltinImpl` number | a compiler-supplied impl: tuple `Eq`, `Ord`, `Hash` and `Debug` at every arity, the `Tuple` marker, and numeric-family members (type-checking.md §17 item 7) |

A `Builtin` callee has no TIR body. Collection maps `(BuiltinImpl,
concrete self type)` to a body that D2 generates per arity or per
primitive (§13.6), named by the stable path `std.builtin.<trait>` plus
the canonical self type in its instance key. This is the solver's
`Evidence::Builtin` (type-checking.md §1.6) written into TIR. **Arguments** are listed in parameter order; their
instructions were emitted earlier in source order, which is how named
arguments keep their evaluation order. **Providers** are one `Ref` per
key of the callee's row in key order, one context `Ref` for a
row-polymorphic callee, or `Pending(row variable)` until M3 fills it
(§3.9.5).

**Suspension and hooks**

| Tag | Operands | Type rule |
| --- | --- | --- |
| `Await` | a: `[callee]`, b: `[arguments, providers]` | the callee's result `T`. A bang call: a suspension point |
| `AwaitValue` | a: a `mut Suspend[T]` value | `T`. `s!()` on a stored suspension: a suspension point |
| `AwaitAll` | b: `[children]`, each `mut Suspend[X_i]` | `(X_1, ..., X_n)`. `all!`: one suspension point |
| `AwaitRace` | a: a `List[mut Suspend[T]]` | `T`. `race!`: one suspension point |
| `Hook` | a: `HookKind`, b: the instruction it observes | `void`; normal emission drops it (§14.7) |

Every suspension point has a side record: the `Scope`s enclosing it from
the innermost out, which are the scopes whose suites cancellation must
run there, and its hook site. Liveness is not recorded; emission computes
it (§14.2).

**Data and closures**

| Tag | Operands | Type rule |
| --- | --- | --- |
| `NewData` | b: `[field values]` in declaration order | the data type in `ty` |
| `CopyData` | a: source, b: `[(field, value)]` replacements | the source's type. Copy-update literals and part copies; every part not replaced is copied too ([`data.part.copy-update`](../../spec/lang/08-data-and-enums.md#r-data.part.copy-update)) |
| `NewVariant` | a: variant `DefId`, b: `[payload values, evidence choices]` | the enum type in `ty`; one evidence choice per bound of each existential parameter |
| `NewTuple` | b: `[elements]` | the tuple type |
| `NewList` | b: `[elements, spread bits]` | `List[T]` |
| `NewMap` | b: `[key, value pairs]` | `Map[K, V]` |
| `Field` | a: base, b: field index (parts included) | the field's type, with the base's access |
| `FieldSet` | a: base, b: `[field, value]` | `void`; the base is mutable |
| `TupleGet` | a: base, b: index | the element's type |
| `Closure` | a: sub-body, b: captures as a `CaptureId` range | the closure's function type. Each capture's mode, in the `cap_mode` column, is `Copy` (no side writes it after the capture), `Move` (only the closure uses it afterward) or `Shared` (both may) |

**Providers**

| Tag | Operands | Type rule |
| --- | --- | --- |
| `With` | a: `[(key, provider value)]`, b: body `Block` | the block's type (`$.with`) |
| `ContextNew` | a: `[(key, provider value)]` | the reusable context's type (`$.context`) |
| `ContextFor` | a: a row | the context passed to a row-polymorphic callee, built from the providers in scope |

**Control flow and cleanup**

| Tag | Operands | Type rule |
| --- | --- | --- |
| `Block` | a: `[instructions]`, b: tail value or `NONE` | the tail's type, or `void` |
| `Scope` | a: body `Block`, b: `[defer suites]`, each a `Block` | the body's type. A cleanup scope ([`flow.defer.scopes`](../../spec/lang/06-control-flow.md#r-flow.defer.scopes)) |
| `Defer` | a: suite number in the enclosing `Scope` | `void`; registers that suite here |
| `If` | a: condition, b: `[then Block, else Block]` | the join of both |
| `Loop` | a: body `Block`, b: `[else Block or NONE]` | the join of its `Break` values, or `void` |
| `ForRange` | a: `[start, end, kind, loop local]`, b: `[body, else]` | as `Loop`; `kind` is half-open, inclusive or from |
| `ForList`, `ForMap` | a: `[collection, item locals]`, b: `[body, else]` | as `Loop` |
| `Break` | a: target `Loop` or `Block`, b: value or `NONE` | `never` |
| `Continue` | a: target `Loop` | `never` |
| `Return` | a: value | `never` |
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
| `SwitchTag` | a: an enum or `Option` value, b: `[(variant, Block) cases, default Block or NONE]` | `never`: inside a decision tree, every path ends in `ToArm` or `Unreachable` |
| `SwitchInt`, `SwitchChar` | a: value, b: `[(constant or range, Block) cases, default]` | as `SwitchTag` |
| `SwitchStr` | a: value, b: `[(constant, Block) cases, default]` | as `SwitchTag` |
| `Payload` | a: a value switched to a variant, b: `[variant, field]` | the payload field's type, refined by GADT matching |
| `Unwrap` | a: an `Option` value switched to `.Some` | the inner type |
| `Guard` | a: condition `Block`, b: `[arm, fail Block]` | `never`; the fail block continues with the remaining rows |
| `ToArm` | a: arm number | `never`; the leaf has already bound the arm's locals with `LocalSet` |

The checker emits each arm's `Block` while it checks the arm, then builds
the decision tree after exhaustiveness and emits it as these switch
instructions. Shared arms appear once.

##### What The Checker Desugars

| Source | TIR |
| --- | --- |
| `a \|> f(_, b)` | `a`'s instructions, then `Call` with `a` in the placeholder's slot |
| `"x $y"` | `Interp` |
| string prefixes, literal suffixes (`10ms`) | `Call` of the resolved prefix or suffix function |
| `x[i]`, `x[i] = v` | `Call` of the resolved index or set method |
| `x += y`, and the other compound assignments | the place's operands evaluated once (their `Ref`s used twice), `Prim` or a trait `Call`, then `LocalSet`, `FieldSet` or a set `Call` |
| `x.E ...= e` | `CopyData`, then `FieldSet` |
| `[for x in xs if c => e]`, map comprehensions | a `Block`: an empty `NewList` or `NewMap`, a loop, a push `Call` |
| `e?` | `SwitchTag` on the `Result` or `Option`; the failing leaf converts the error with the resolved `From` call and `Return`s |
| `while c: body` | `Loop` whose body is `If c then body else Break` |
| `for` over a std range, list or map | `ForRange`, `ForList`, `ForMap`, chosen when the iterable's impl resolves to std's |
| any other `for` | a `Block`: the `iter` `Call`, then a `Loop` around the `next` `Call` and a `Match` on its `Option` |
| `let p = e else: ...`, `x is p` | `Match` with two arms |
| `x := e`, `let p = e` | a new local and `LocalSet`, or a `Match` for a pattern |
| `a ** b` | `Prim` on primitives, else the trait `Call` |
| `$.with`, `$.use`, `$.context` | `With`, `ProviderGet`, `ContextNew` |
| `all!(a(x), b(y))` | two cold `Call`s, then `AwaitAll` |
| `race!(...)` | a `NewList` of cold suspensions, then `AwaitRace` |
| test registrations | `Call`s in a `TestCase` body |

##### The Builder API

```rust
/// What the checker calls. `TirBuilder` implements it; a discarding sink
/// implements it too if the no-emit mode is accepted (type-checking.md §16 question 5).
pub trait TirSink {
    // values (each returns the new instruction's value)
    fn konst(&mut self, c: Index) -> Ref;                          // no instruction: a constant Ref
    fn local(&mut self, ty: Ty, name: Symbol, flags: LocalFlags, syn: NodeIdx) -> LocalId;
    fn get(&mut self, l: LocalId, syn: NodeIdx) -> Ref;
    fn set(&mut self, l: LocalId, v: Ref, syn: NodeIdx);
    fn prim(&mut self, op: PrimOp, args: &[Ref], ty: Ty, syn: NodeIdx) -> Ref;
    fn call(&mut self, callee: Callee, args: &[Ref], prov: Providers, ty: Ty, syn: NodeIdx) -> Ref;
    fn await_(&mut self, target: AwaitTarget, ty: Ty, syn: NodeIdx) -> Ref;  // records the scope chain
    fn coerce(&mut self, kind: Coercion, v: Ref, to: Ty, syn: NodeIdx) -> Ref;  // Coercion carries its evidence
    fn emit(&mut self, op: TirOp<'_>, ty: Ty, syn: NodeIdx) -> Ref;  // TirOp: generated, one variant per tag

    // structure: each open_* takes a scratch checkpoint, each close_* flushes it (§3.9.5)
    fn open_block(&mut self) -> BlockMark;
    fn close_block(&mut self, m: BlockMark, tail: Option<Ref>, ty: Ty, syn: NodeIdx) -> Ref;
    fn open_scope(&mut self) -> ScopeMark;
    fn defer(&mut self, suite: Ref /* a closed Block */, syn: NodeIdx);
    fn close_scope(&mut self, m: ScopeMark, body: Ref, syn: NodeIdx) -> Ref;
    fn open_loop(&mut self) -> LoopMark;       // Break and Continue name its LoopMark
    fn open_sub(&mut self, params: &[LocalId]) -> SubMark;    // a closure's body
    fn capture(&mut self, sub: SubMark, outer: LocalId) -> CaptureId;  // first use of an outer local inside it
    fn close_sub(&mut self, m: SubMark, root: Ref, fn_ty: Ty, syn: NodeIdx) -> Ref;  // emits the Closure

    // speculation (§3.9.5)
    fn checkpoint(&self) -> TirCheckpoint;
    fn rollback(&mut self, c: TirCheckpoint);
}

impl<'w> TirBuilder<'w> {
    pub fn new(item: DefId, kind: BodyKind, cols: &'w mut TirColumns) -> Self;  // the worker's reused columns
    /// End of the body: the final type sweep, the capture modes, then the verifier in debug builds.
    pub fn finish(self, solution: &Solution) -> TirBody;
}
pub struct Solution<'a> {
    pub infer: &'a dyn InferRead,               // resolves every `ty` to a global type
    pub capture_modes: &'a [CaptureMode],       // by CaptureId, decided by the checker
}
```

- The builder is the only writer of TIR. `emit` and the per-tag helpers
  are generated from `tir.ir` (data-structures.md §3.25), so every
  encoding goes through the schema.
- **The checker is generic over `TirSink`** (type-checking.md §17 item
  9), as `Checker<B: TirSink>`. This costs one generic parameter and
  lets the no-emit mode exist without a second code path, if the owner
  accepts it.
- **Captures.** A closure's captures are collected on the scratch stack
  while its body is checked, since nested closures interleave, and
  flushed contiguously into the capture columns at `close_sub`.
- Types may be inference variables while a body is built. `coerce` is
  called where bidirectional checking finds a coercion, so coercions are
  never re-derived later.
- `finish` resolves every `ty` to a global type, rejects leftovers,
  writes `cap_mode` from the solution, copies the body's ranges into one
  exact-size `TirBody`, truncates the worker columns, and in debug builds
  runs the verifier.

##### Invariants

The verifier checks each of these:

1. Every operand precedes its user, and is visible where it is used: in
   the same block earlier, or in an enclosing block.
2. Every instruction is in exactly one block list.
3. After `finish`, every type is global. `Hole` and `Poison` occur only
   in a module with errors, which D2 does not lower.
4. **Every operand's type equals the type its position expects**: call
   arguments equal the substituted parameter types, a `LocalSet` value
   equals the local's type, a `Return` value equals the body's result, a
   `Break` value equals its target's type. Coercions are explicit, so any
   mismatch is a checker bug.
5. `Break` and `Continue` targets enclose them. `ToArm` is inside its
   `Match`'s decision block. `Defer` is inside its `Scope`.
6. `Await`, `AwaitValue`, `AwaitAll` and `AwaitRace` occur only in a
   suspending body or closure. No suspension point, `Return`, `Break`,
   `Continue` or failing `?` leaf is inside a `defer` suite.
7. Every suspension point's side record lists exactly the `Scope`s that
   enclose it.
8. Every call to a callee with a row has providers for exactly its keys,
   and none is `Pending` after M3.
9. `CallHost` occurs only in std's provider bodies; an `Intrinsic` only
   where it is declared.
10. A `Closure`'s captures name locals of its enclosing sub-bodies, and
    after `finish` every capture has a mode.
11. Every switch in a decision tree has a case for each value of its
    type, or a default.
12. A constant `Ref` names a global pool constant whose type equals the
    type its position expects; no constant is body-local.
13. A `Builtin` choice names a `BuiltinImpl` whose trait is the callee's
    trait.

##### Lifetime And The `tir` Entry

- A body's TIR lives in its worker's columns until the body finishes, then
  moves into the module result. `ModuleFinish` fills pending providers,
  serializes every body of the module into the `tir` entry, and frees
  them. A check never keeps TIR, and a later
  build reads the entry instead of rechecking (§3.10.2).
- The entry's key is `H("tir", check_key(m))`. It also stores, per item,
  a **TIR hash** (the hash of that item's serialized columns, closures
  included) and a **dependency list**: the stable paths of the items its
  TIR names, each with its per-item interface hash. Instance keys use both
  (§13.8).
- Types stay generic (`Param`). D2 never materializes an instance as TIR.

**The wire form** (the `tir` entry's sections,
[data-structures.md §3.20.4](data-structures.md#3204-entry-sections-by-kind)):

| Section | Content |
| --- | --- |
| `strings`, `paths`, `types` | the entry's own tables; every `Symbol`, `DefId` and `Ty` in the module's TIR is a row here (data-structures.md §3.20.2) |
| `bodies` | per body, 48 bytes: item path row, kind, the start of its range in each column below, TIR hash, dependency range |
| `tags`, `data`, `extra` | the instruction columns of every body, concatenated in body order; ID words remapped to entry rows by the generated codec, other words copied |
| `ty` | type rows |
| `span_lo`, `span_hi` | byte offsets in the module's file, from `syn`, so emission never needs the syntax tree |
| `local_*`, `sub_*`, `cap_*`, `susp`, `origin`, `hole` | the other columns, concatenated, IDs remapped |
| `deps` | per body: (path row, item interface hash), sorted by path bytes |

- **Remap, not copy.** Pool indices, `DefId`s and `Symbol`s are run IDs
  and cannot be written (data-structures.md §3.20.2). The schema marks
  every ID-typed operand, and the generated writer remaps exactly those
  words. D1's "column copies" holds for the rest.
- **The TIR hash** of an item is computed over its remapped rows, with
  referenced types and paths hashed by content. It is therefore the same
  on every run and thread count.
- **Reading.** Emission maps the entry and casts each section. It interns
  a type row into the pool on first use, as an interface reader does.

### 4.14 Diagnostics

**One per root cause.**

1. Poison silences follow-ups (§3.6).
2. Each diagnostic carries a `RootKey`: the item, plus the name or span
   that caused it. `ModuleFinish` keeps the first diagnostic per root key
   in content order and drops the rest.
3. An unknown name reports once per body, a broken header once per item, a
   failed import once per use.
4. A header error in module `m` is reported in `m`, never again in its
   dependents, which see poison.

**Order.** Every diagnostic is sorted by package order, file stable path,
start offset, end offset, code and rendered message. The key is total, so
no tie falls back to an ID (tsgo #64589). Exact duplicates are dropped.

**Fix-its.**

- A fix-it is a list of exact edits on byte ranges.
- **Re-parse check at emission.** Before a fix-it is marked `Exact`, its
  edits are applied to a copy of the file text, which is re-lexed and
  re-parsed. If the parse gains an error, the fix-it is demoted to
  `Suggestion`: shown, but never applied. Only the first 20 fix-its per
  file are verified this way; the rest stay `Suggestion`.
- **`hd fix`** adds a type check of the result (§7.6).
- First-release kinds: did-you-mean names, missing `use` lines, `let mut`,
  the folder-cycle move, and the fix-its the spec names. Code-generating
  fix-its come later.

**Rendering.**

- **Compact text** (default): one line `file:line:col: code: message`, and
  at most one `hint:` line naming the fix. Notes and secondary labels
  appear with `--verbose`. The target is at most 60 tokens per diagnostic
  (the `mistakes` metric).
- **JSON lines** with the fields of
  [Machine Output](../../spec/cli/command-line.md#machine-output), plus
  `fixes` (edits with file, byte range and text) and `notes`. Adding these
  fields is CLI spec work for the spec pass.
- **Capping.** `--max-errors N` stops printing after N errors and still
  prints the summary with full counts. A summary mode prints counts per
  code and file.
- **`modules_checked`** in the JSON summary is the number of module
  `check` entries computed in this run, not read from the cache (answer 6).

### 4.15 Limits

Each limit counts language-level units, so a program passes or fails the
same way on every run, thread count and cache state (lesson 3). Code names
marked "proposed" are for the spec pass (answer 5).

| Limit | Counts | Default | Diagnostic |
| --- | --- | --- | --- |
| file size | bytes | 16 MiB | `file-too-large` (proposed) |
| nesting depth | brackets, blocks, closures and interpolations open at once | 256 | `nesting-too-deep` (proposed) |
| trait resolution depth | nested subgoals | 64 | `trait-resolution-depth` (spec) |
| body fuel | candidates, subgoals, unification steps, matrix cells, trials, joins, obligation retries; memo hits at their stored cost ([type-checking.md §11.1](type-checking.md#111-what-counts)) | 2,000,000 steps per body | `item-too-complex` (proposed), naming the item; on exhaustion the body's TIR rolls back to one `Poison` ([type-checking.md §11.2](type-checking.md#112-running-out)) |
| type size | nodes in one type | 10,000 | `type-too-large` (proposed) |
| exhaustiveness | matrix cells, within the body's fuel | shares body fuel | `match-too-complex` (proposed) |
| embedding depth | nested `data` embedding | the spec's | `embedding-too-deep` (spec) |
| instantiation depth | nesting depth of an instance's type arguments; length of the request chain from a root | 32; 256 | `instantiation-too-deep` (proposed; §13.4) |
| memory | process bytes | **none by default** (owner, 2026-10-07); opt in with `--max-memory` or `HD_MAX_MEMORY` | `memory-limit` (proposed), naming the stage |

**The opt-in memory cap.** By the owner's decision there is no default
cap. When one is set, a counting wrapper around the global allocator keeps
the process total in one atomic counter. The scheduler checks it before it
starts each task, and fuel checks it every 4,096 steps. Over the cap, no
new task starts, running bodies stop at their next check, and `hd` reports
one hard-limit diagnostic. It names the stage, such as "folder interfaces"
or "body checking", and the items in flight in content order. Then `hd`
exits with status 101. When a cap is hit depends on scheduling, so this is
the one limit that is not deterministic, and it is off unless asked for.
