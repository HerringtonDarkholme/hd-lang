# New Compiler: The Trait Solver

Status: Design, not decided. Frontend lane, 2026-10-07.

This document details the trait solver of the new compiler. It implements
the interface that [type-checking.md §1.6](type-checking.md#16-the-trait-solver-interface)
fixes, and it refines these sections of the D1 and D2 design:

- [§4.12 Traits, Impls And Coherence](resolution-and-interfaces.md#412-traits-impls-and-coherence):
  owner-module impl tables, solving and coherence;
- [§4.10 Folder Interface Construction](resolution-and-interfaces.md#410-folder-interface-construction):
  what the interface records about traits and impls;
- [§4.13.11 The Typed IR](checking-and-tir.md#41311-the-typed-ir-tir):
  how a choice is recorded;
- [§13.2 Collection](codegen.md#132-collection) and
  [§13.5 Dictionaries](codegen.md#135-dictionaries-trait-values-and-gadt-evidence):
  how codegen consumes the choice.

Where this design needs a change in another file, section 16.4 lists it.
It does not edit those files.

## How To Read This

Conventions, as in [type-checking.md](type-checking.md):

- **Mine** marks an idea of this document: not shipped practice, and not
  in the research or in D1 and D2.
- **Rule TS-n** marks a design rule that a reviewer can check against the
  code. Section 15 maps prototype failures to these rules.
- Rust code sketches shape and ownership. It is not final code.
- "First release" is the first shipped compiler. "Later" is after it.
- The spec decides semantics. Where this design reads the spec in a way
  that needs confirming, section 16 says so.

**The short version.** hd's trait rules are small on purpose. Overlap is
decided from heads alone
([`trait.overlap.heads-only`](../../spec/lang/09-traits.md#r-trait.overlap.heads-only)),
there are no blanket impls
([`trait.target.no-blanket`](../../spec/lang/09-traits.md#r-trait.target.no-blanket)),
no specialization and no negative impls, and an impl may live only in one
of a few modules. Three consequences carry the whole design:

1. **At most one impl head matches a goal whose types are known.** So the
   solver commits to that impl and never backtracks (rule TS-2). The proof
   is an AND tree, not an AND-OR search.
2. **No search fixpoint.** The only coinduction the spec asks for is a
   derived impl assuming itself while its members are checked. Its head is
   already in the impl table, and uses never re-check members, so the
   search never meets that cycle. Every cycle it does meet is an overflow
   (rule TS-7).

Section 16.5 answers the Codex review of the earlier design files, finding
by finding.
3. **Codegen never searches.** At a concrete instance, the one matching
   head is the answer (rule TS-6).

What remains is real work: associated types and their normalization,
inference variables, the instantiation choice of generic traits, budgets
that do not depend on the schedule, and messages that name the root cause.

## Prior Art In One Table

Each row was checked against its source; the design keeps the column on the
right.

| System | What it does | What hd takes |
| --- | --- | --- |
| rustc's next solver ([dev guide: caching](https://rustc-dev-guide.rust-lang.org/solve/caching.html), [coinduction](https://rustc-dev-guide.rust-lang.org/solve/coinduction.html)) | canonical goals as cache keys; a search graph with cycle heads; provisional results and fixpoint iteration for coinductive cycles (auto traits, `Sized`, well-formedness); inductive cycles answer overflow; global cache entries store the depth they reached, and overflow is cached per remaining depth | canonical goals; depth stored with each entry (as an intrinsic height, section 7.3); inductive cycles answer overflow. Not taken: the provisional cache and fixpoints, which hd's rules make unnecessary |
| chalk | a separate logic-programming solver (SLG tabling) shared by rust-analyzer; it drifted from rustc and was abandoned for the next solver ([Rust blog, 2026-08-21](https://blog.rust-lang.org/2026/08/21/enabling-next-solver-on-nightly/)) | one solver, used by the checker, the impl checks and codegen alike; no general logic engine |
| Swift's Requirement Machine ([forums](https://forums.swift.org/t/the-requirement-machine-a-new-generics-implementation-based-on-term-rewriting/55601)) | generic signatures as a rewrite system, completed by Knuth-Bendix, after the old builder went exponential | not needed: hd has no same-type requirements between parameters except associated-type bindings, which are oriented (projection to type). Elaboration is a linear walk (section 4.2) |
| Lean 4 tabled resolution ([paper](https://arxiv.org/pdf/2001.04301)) | tabling removes the exponential cost of diamond hierarchies and handles cycles | the lesson that memoization must remove diamonds; hd gets it from the memo alone, since there is no OR search |
| Haskell (GHC) | an instance is selected by its head alone; the context is checked after the commit, and there is no backtracking | the commit rule TS-2. hd's head-only overlap makes it sound without any overlapping-instance pragmas |
| Scala 3 givens | search with priorities and a divergence check | nothing: hd has no priorities and no implicit search |
| Go | interface satisfaction by method sets; constraint satisfaction is memoized ("T satisfies B", Go #66699) | memoize "type satisfies bound" globally |
| MoonBit ([methods and traits](https://docs.moonbitlang.com/en/stable/language/methods.html)) | explicit `impl Trait for Type`; an impl lives in the package of the trait or of the type; the stable docs show no trait parameters and no associated types; monomorphization after linking. Its checker is known to be fast | the fast path of section 3.3: a goal on a non-generic trait with a known self type is one table probe. hd pays for more only where its rules are richer than MoonBit's: generic traits, associated types and impls owned through a trait argument |

## 1. Scope

### 1.1 Who Owns What

| Component | Owns | Does not own |
| --- | --- | --- |
| Resolution (`hd_resolve`) | impl heads, bound plans and derived heads in the interface; every header check that needs no solver: orphan, module ownership, targets, unconstrained parameters, sealed traits, dynamic safety, template placement | goals |
| **Trait solver (`hd_types::solve`, this document)** | the four goals; parameter-environment elaboration; normalization of projections; canonical goals and the memo; impl selection for codegen; `FailInfo` | inference variables (it reads them, never writes them), diagnostics text, spans |
| Impl checks (body tasks in M2) | each impl's supertrait and binding checks, derive member obligations, delegation checks: goals under the impl's environment, asked through the solver | the answers |
| Type checker (`hd_check`) | when to ask, applying learned bindings, obligations, the instantiation choice by trial, every diagnostic | impl search |
| Coherence (`Coherence(trait)` task) | overlap across the program graph, per trait, from heads | goals |
| Codegen collection (D2) | instances and vtables | anything but a head match at concrete types (rule TS-6) |

**Rule TS-1. The solver is a pure function of its memo key.** An answer
depends only on the canonical goal, the parameter environment, the local
impls visible at the asking point, the availability key (for `Methods`),
and the run's frozen interfaces. Section 7.1 makes each of these part of
the key. The solver holds no mutable state except memo tables. It never
reports a diagnostic, never sees a span, and never writes an inference
variable.

### 1.2 The Four Goals

The interface is the one in
[type-checking.md §1.6](type-checking.md#16-the-trait-solver-interface),
with a `mut` flag and a `Normalized` answer added (section 16.4,
changes 1 and 4).

| Goal | Asked for | hd example | Typical answer |
| --- | --- | --- | --- |
| `Implements` | each bound of an instantiated callee; `==`, `<`, `?` conversions; trait-value conversions; impl checks | `fn show[T < Display](v: T)` called with a `User` asks `User: Display` | `Holds { Impl(impl Display for User) }` |
| `Project` | a projection meets another type in unification; an operator's `Out`; `T::Item` in a signature | `price + price` with `impl Add for Money { type Out = Money }` asks `<Money as Add[Money]>::Out` | `Normalized { ty: Money, evidence }` |
| `Instantiations` | bound-only parameters; the instantiation choice of a method or operator; `X::from(v)` | `AppError::from(code)` asks `Instantiations { AppError, From }` | `Many([From[i32], From[string]])` in content order |
| `Methods` | `value.name(...)`, `Type::name(...)` | `w.show()` on a `Wrapper` with an inherent `show` and an available `Show` | `Many([Inherent(show), Trait(Show::show)])`; the checker takes the inherent one |

The answer shapes:

| Answer | Meaning | Checker action |
| --- | --- | --- |
| `Holds { evidence, learned }` | proven; `learned` binds inference variables that the proof forced (section 3.8) | apply `learned` on its own trail; record `evidence` in TIR |
| `Normalized { ty, evidence, learned }` | `Project` only: the projection's normal form, over the caller's variables | unify `ty` with the other side |
| `Many(candidates)` | several candidates, in content order (`Instantiations`, `Methods` only) | the instantiation choice or method rules |
| `Fails(FailInfo)` | no proof exists, whatever the open variables become | report, once per root cause (section 10) |
| `Stalled { on }` | the answer depends on these variables | keep an obligation watching them |
| `Overflow` | the proof needs a bound deeper than 64, or a cycle | `trait-resolution-depth` |
| `OutOfFuel` | the body's fuel ran out inside the solver | `item-too-complex` |

### 1.3 Entry Points

```rust
pub trait Solver: Sync {
    /// The checker's entry: one goal in one body.
    fn solve(&self, cx: &mut SolveCx<'_>, goal: Goal, fuel: &mut Fuel) -> Answer;

    /// Builds the elaborated environment of an item from its declared bounds
    /// (section 4.2). Called once per item signature by the checker or an impl check.
    fn elaborate(&self, bounds: &[DeclaredBound], out: &mut ParamEnvBuilder) -> EnvKey;

    /// Codegen's entry: the impl whose head matches a concrete trait reference.
    /// Head matching only; no subgoal, no depth limit, no fuel (rule TS-6).
    fn select(&self, tref: ConcreteTraitRef) -> Selection;

    /// Codegen's entry for associated types at an instance.
    fn normalize_concrete(&self, base: Ty, trait_: DefId, args: TyList, name: Symbol) -> Ty;
}
```

`elaborate`, `select` and `normalize_concrete` are additions to the §1.6
sketch. They need no new goal kind. Not in scope: implied bounds
([Declared Bounds Are Not Implied](../../spec/lang/09-traits.md#declared-bounds-are-not-implied)),
structural conformance, and any goal at run time.

## 2. Goal Representation

### 2.1 Trait References

A trait reference is a trait, a self type and the trait's arguments. Its
associated-type bindings travel beside it.

```rust
pub struct TraitRef { pub trait_: DefId, pub self_ty: Ty, pub args: TyList }

pub enum Goal {
    Implements { tref: TraitRef, bindings: AssocList, mut_: bool },  // mut_: a `T < mut Tr` bound
    Project { assoc: DefId, tref: TraitRef },  // assoc: the associated type's own item
    Instantiations { self_ty: Ty, trait_: DefId, mut_: bool },
    Methods { receiver: Ty, name: Symbol },
}

/// A projection type in the pool (replaces D1's `Assoc { base, trait_, name }`).
/// `tref` is the declaring trait's instantiated reference, so `<S as Add[i32]>::Out`
/// and `<S as Add[i64]>::Out` are different types.
Assoc { assoc: DefId, tref: TraitRef }
```

- **Projections name the declaring item.** `I::Item` under
  `I < NamedSupplier` names `Supplier`'s `Item` item and the instantiated
  `Supplier` reference that elaboration found
  ([`trait.binding.name-reach`](../../spec/lang/09-traits.md#r-trait.binding.name-reach)).
  The name alone would lose the supertrait path and the trait arguments.
- **An `AssocList` binding** is keyed by that item's `DefId`, not by name.

- `self_ty` keeps its outer `Mut` only for the `mut_` check. Impl matching
  strips it, since one impl serves both views
  ([`trait.target.both-views`](../../spec/lang/09-traits.md#r-trait.target.both-views)).
- An inner `mut` stays: `Store[mut User]` and `Store[User]` are different
  trait references
  ([`trait.target.inner-mut`](../../spec/lang/09-traits.md#r-trait.target.inner-mut)).
- Types reach the solver with aliases expanded. Newtypes stay nominal.
- Trait argument defaults are filled before the goal is formed, so
  `impl Add for Money` and the goal `Money: Add[Money]` agree
  ([`expr.op.trait.rhs-self`](../../spec/lang/05-expressions.md#r-expr.op.trait.rhs-self)).

### 2.2 Canonical Goals

A goal is canonicalized before any memo lookup:

1. **Resolve shallowly.** Each inference variable is replaced by its
   binding, recursively, through the checker's `InferRead`.
2. **Apply arm equalities (mine).** Inside a GADT arm, a rigid parameter
   with an equality `T ≡ R` is replaced by `R`. So `T: Display` in an arm
   where `T ≡ i32` becomes the global goal `i32: Display`, not a body-local
   one.
3. **Normalize concrete projections.** A projection whose base is known is
   replaced by its normal form (section 4.3). Projections on parameters
   stay as rigid types.
4. **Number the variables.** The remaining inference variables are
   replaced by canonical placeholders `Canon(0)`, `Canon(1)`, ... in order
   of first occurrence in a fixed pre-order walk: self type first, then
   trait arguments left to right, then bindings in name order. Each
   placeholder keeps its variable's kind: general, integer literal or float
   literal.
5. **Pick the key's scope.** See the table below.

```rust
pub struct CanonGoal {
    pub kind: GoalKind,     // u8
    pub mut_: bool,
    pub n_vars: u8,          // canonical placeholders; more than 255 is a Stalled answer
    pub trait_: DefId,       // or NONE for Methods
    pub self_ty: Ty,         // global pool when the scope is Global or Env
    pub args: TyList,
    pub extra: u32,          // AssocList for Implements, assoc DefId for Project, Symbol for Methods
}
pub struct CanonVars { pub map: SmallVec<[InferVar; 4]>, pub kinds: SmallVec<[VarKind; 4]> }
```

`Canon(i)` is a global pool tag, so a canonical goal holds no body-local
type. Two bodies asking `List[?3]: Eq` and `List[?9]: Eq` produce the same
key. `CanonVars` maps placeholders back to the asking body's variables
when the answer has learned bindings or stalls.

| Scope | When | Memo | Key (section 7.1) |
| --- | --- | --- | --- |
| `Global` | no placeholder, no parameter, no local type, no existential | the run's global memo, shared by every body and by codegen | the canonical goal |
| `Env` (mine) | parameters, but no placeholder, no local type, no existential | the run's global memo | the canonical goal and the item's `EnvKey` |
| `Body` | any placeholder, local type or existential | the body's memo | the canonical goal and the visible local impls |

A `Methods` goal adds the module's `AvailKey` in every scope.

**Why parameters may go global.** With the `EnvKey` in the key, a goal
that mentions `T` is still a pure function of its key, and the members of
a generic impl, which share the impl's bounds, share their goals.
type-checking.md §1.6 keeps such goals per body (change 2).

**Why the body memo has its own key arena.** A trial truncates the
body-local pool on rollback
([§3.9.5](data-structures.md#395-building-scratch-buffer-checkpoints-truncation)),
so a key stored there would dangle. Canonical keys hold only global types
and placeholders, in an arena no rollback truncates, and an entry stays
valid after a rollback because it names no particular variable.

### 2.3 The Parameter Environment

```rust
pub struct ParamEnv {            // SoA, built by `elaborate`, frozen per item
    pub key: EnvKey,             // interned: equal environments get equal keys
    clause_self: Box<[Ty]>,      // a Param, or an existential's placeholder
    clause_trait: Box<[DefId]>,
    clause_args: Box<[TyList]>,
    clause_bindings: Box<[AssocList]>,
    clause_mut: BitBox,          // the bound was written `mut`
    clause_origin: Box<[u16]>,   // declared bound index, or the supertrait path's first step
    by_self: Box<[(Ty, u32, u32)]>,  // sorted index: self type -> clause range
}
pub struct ArmEnv<'a> { pub base: &'a ParamEnv, pub eqs: &'a [(ParamRef, Ty)], pub existentials: &'a ParamEnv }
```

A clause's index is its position in the elaborated list. TIR's
`Bound { param, index }` choice uses it (section 8.1). The order is
deterministic: declared bounds in source order, each followed by its
supertraits, depth first, in declared order, each trait reference once.

## 3. Impl Selection

### 3.1 Where Candidates Come From

For a goal `S: Tr[A1..An]`, candidates come from these sources, tried in
this order:

| Order | Source | When it applies |
| --- | --- | --- |
| 1 | the parameter environment (section 4.2) | `S` is a parameter or an existential, after arm equalities |
| 2 | a trait value as self (section 9.3) | `S` is a trait value type of `Tr` or of a subtrait |
| 3 | compiler-supplied impls (section 3.9) | `Tr` is sealed: `Any`, `AnyVal`, `AnyRef`, `Inspectable`, `Tuple`, `Num`, `Integer`, `Float`, `Suspend`, `Structure` |
| 4 | impl tables of the owner modules (section 3.2) | written impls, derived impls, delegations, numeric families, tuple templates |
| 5 | the body's local impls | the goal names a local trait or a local type |

**Rule TS-2. Commit on the head; never backtrack among impls (mine, after
GHC).** Overlap is decided from heads, and no two heads of one trait
unify. So when the goal's types are known, at most one impl head matches.
The solver commits to it. If one of its bounds then fails, the goal fails:
no other impl could have answered it. Sources 1 to 3 come first, and the
first source that holds answers. A parameter can match only
parameter-headed impls (numeric families), which are std's and agree with
the environment.

Order between sources matters only for the evidence recorded, never for
whether a goal holds. A goal `T: Add[T]` under `T < Num` holds through the
environment (the supertrait binding of `Num`) and through std's
`impl[N < Num] Add for N`. The environment wins, so the evidence is
`Bound`. Codegen re-selects at the concrete type anyway (rule TS-6).

### 3.2 Owner Modules

An impl `impl Tr[Args] for Target` lives in a module that declares `Tr`,
the outer constructor of `Target`, or the outer constructor of one of
`Args`
([Implementation Modules](../../spec/lang/09-traits.md#implementation-modules)).
D1 turns this into lookup in at most `2 + len(Args)` modules
([§4.12.1](resolution-and-interfaces.md#4121-owner-module-impl-tables-mine)).

```rust
fn owner_modules(g: &TraitRef, out: &mut SmallVec<[ModuleId; 4]>) {
    out.push(module_of(g.trait_));
    for t in once(g.self_ty).chain(g.args.iter()) {
        match head_ctor(t) {
            Head::Decl(def) => out.push(module_of(def)),
            Head::Builtin(_) => out.push(STD_BUILTIN_TABLE),  // one synthetic table: every std module's impls for built-ins
            Head::Param | Head::TraitValue => {}              // owns nothing
            Head::Infer => {}                                  // section 3.4
        }
    }
    out.sort_by_key(content_rank); out.dedup();
}
```

**Built-in targets.** std may hold impls and inherent impls for
primitives, `List`, `Map`, tuples, `Fn`, `Option` and `Result` in any of
its modules
([`trait.own.module.inherent.std`](../../spec/lang/09-traits.md#r-trait.own.module.inherent.std)).
So std's interface gathers every impl and inherent impl whose target is a
built-in constructor into one synthetic table, `STD_BUILTIN_TABLE`, and
lookup reads that table instead of one "owner" module.

**The gap: impls owned through a trait argument.** When an argument is
still an inference variable, its owner module is unknown. `Money:
Add[?0]` may be answered by `impl Add[Cents] for Money` in the module that
declares `Cents`
([`trait.own.argument`](../../spec/lang/09-traits.md#r-trait.own.argument)).
`Instantiations` and the trait part of `Methods` are the same case with
every argument open. D1's `2 + len(Args)` bound covers only goals whose
arguments are known. The Codex review found the same gap (blocker 3).

The fix is a **candidate directory**, after the review's proposal:

1. Resolution flags each impl that its module owns only through a trait
   argument. The interface lists those heads in an `arg_impls` section,
   sorted by `(trait, target head key)`, with its own section hash.
2. Once per run, the driver merges the `arg_impls` sections of every
   folder in the program graph into a **directory**: per trait, rows of
   `(target head key, folder, impl row)`, in content order. It is frozen
   before any body task starts that needs it.
3. A goal with an open argument reads the trait's directory rows for its
   target head key, and keeps only rows whose folder is in the asking
   module's **dependency closure** (a bit set per module, built in M1).
   Section 16 question 1 asks the owner to confirm the closure rule.
4. **Cache key (incremental soundness).** A module's `check` key gains
   `arg_impls_closure_hash`: a Merkle hash over the `arg_impls` section
   hashes of every folder in the module's dependency closure, computed
   bottom-up with the deep hashes. Adding `impl Pick[Product] for Receiver`
   in a third folder changes that folder's section hash, so every module
   whose closure holds it is rechecked. The hash is coarse, but such impls
   are rare (`From` written by the source error's owner, `Add[Money] for
   i32`), and the section hash changes only when such a head changes.
   Section 16.4 change 21 states the change for cache.md.

Why the closure, not the whole graph: a library's check result must not
depend on which downstream packages a program adds. A downstream package
may write `impl Pick[Product] for Receiver` for a library's `Receiver`,
and the library, checked alone or inside that program, must see the same
instantiations.

### 3.3 The Head Index

```rust
pub struct ImplTable {                     // per module, SoA, frozen with its folder
    // D1's columns
    pub trait_: Box<[DefId]>, pub def: Box<[DefId]>, pub head_key: Box<[HeadKey]>,
    // added by this design
    pub arg_key: Box<[[HeadKey; 2]]>,      // head keys of the first two trait arguments; ANY when generic
    pub n_params: Box<[u8]>,
    pub head: Box<[HeadRef]>,              // target and trait arguments, as interface type records
    pub plan: Box<[PlanRange]>,            // the bound plan (section 3.6)
    pub assoc: Box<[AssocRange]>,          // associated-type bindings, by associated item DefId
    pub origin: Box<[ImplOrigin]>,         // Written | Derived { template } | Delegated { field } | Error | NumericFamily | TupleTemplate
    pub rank: Box<[u64]>,                  // content rank: (module path rank, source position); the sort key
    pub by_trait: HashMap<DefId, (u32, u32)>,  // an index into the sorted rows
}
pub enum HeadKey { Ctor(DefId), Prim(Prim), Tuple(u16), TupleAny, Fn, SuspendFn, Param, Any }
```

Rows are sorted by `(trait rank, head key, arg key, rank)`. A probe for
`S: Tr[A..]` in one module is:

1. `by_trait[Tr]` gives the trait's rows.
2. Binary search for `head_key(S)`; add the `Param` rows (numeric
   families) and, for a tuple, the `TupleAny` rows (tuple templates).
3. Within the bucket, skip rows whose `arg_key` disagrees with the goal's
   first two arguments. This is rustc's fast reject (`DeepRejectCtxt`)
   cut to two keys.
4. Match the survivors' heads (section 3.4).

**Fast path (mine, after MoonBit).** A non-generic trait (`Display`, `Eq`,
`Hash`, `Debug`) with a known self constructor has at most one row per
head key in all owner modules together. The probe is one binary search per
owner module, and the global memo makes the second probe free. In
practice most goals of a body take this path.

### 3.4 Matching A Head

Matching is one-way unification of the impl head against the goal. Impl
parameters are pattern variables. Goal types are rigid, except canonical
placeholders, which the solver may not bind.

```rust
enum MatchResult { Yes(Subst), No, Maybe(SmallVec<[CanonVar; 2]>) }
```

| Goal position holds | Head position holds | Result |
| --- | --- | --- |
| a type | an impl parameter | bind it; a second occurrence must be equal |
| a constructor | the same constructor | match the arguments pairwise |
| a constructor | another constructor | `No` |
| a placeholder | a constructor or a bound parameter | `Maybe(placeholder)` |
| a parameter of the asking item | a constructor | `No` |
| a row | a row parameter | bind it; a row parameter matches every row |
| a row | a concrete row | equal key sets, or `No` |
| a rigid projection | anything but an impl parameter | `No`: a projection on a parameter is opaque |

- **Rows.** A row argument in a head is a row parameter or a concrete row
  ([`trait.target.row-argument`](../../spec/lang/09-traits.md#r-trait.target.row-argument)),
  so matching never needs row unification with a tail.
- **Tuples.** A tuple head matches a tuple of the same arity and the same
  rest shape. `TupleAny` matches every tuple.
- **Several `Maybe`s.** The goal stalls on the union of their
  placeholders, in canonical order.
- **Cost.** Linear in the head's size. Heads are small, so matching is a
  few dozen steps at most.

### 3.5 Committing

After the probe:

| Matches | Answer |
| --- | --- |
| no `Yes`, no `Maybe` | `Fails`: no impl |
| exactly one `Yes`, no `Maybe` | commit: solve its bound plan |
| any `Maybe` | `Stalled` on the placeholders, unless every `Maybe` row's arguments are already ruled out by the fast reject |
| two `Yes` | only possible with an overlap error elsewhere: take the first in content order and continue. Coherence reports the overlap (section 5.4); the solver reports nothing |

### 3.6 Bounds As Subgoals

An impl's bounds become subgoals after its head matched. Resolution stores
each impl's **bound plan**, an ordered list of steps computed once at
interface time (mine):

```rust
pub enum PlanStep {
    Bound { param: u8, trait_: DefId, args: TyListTemplate, mut_: bool },
    Bind  { param: u8, trait_: DefId, args: TyListTemplate, name: Symbol, target: u8 }, // solve target from a projection
}
```

- **Order.** A parameter that only a binding constrains, as `T` in
  `impl[T < Display, I < Store[Item = T]] Summary for Feed[I]`
  ([`trait.overlap.constrained-binding`](../../spec/lang/09-traits.md#r-trait.overlap.constrained-binding)),
  is solved by its `Bind` step before any step that reads it. Resolution
  rejects an impl with no such order (`unconstrained-impl-parameter`), so
  a plan always exists. It is a topological sort of the binding graph,
  with source order breaking ties.
- **Execution.** Steps run in plan order, each at depth `d + 1` where `d`
  is the goal's depth
  ([`trait.bound.depth`](../../spec/lang/09-traits.md#r-trait.bound.depth)).
  The first step that fails decides the answer, so a plan's answer does
  not depend on anything but its order.
- **Self types shrink.** A step's self type is an impl parameter, and every
  parameter is bound to a part of the goal or fixed by a binding. So a
  subgoal's self type is a subterm of the goal's types or a projection's
  normal form. Only trait arguments and projections can make subgoals
  larger. This is why the depth limit, the type size limit and fuel
  together suffice (section 13).

### 3.7 Supertraits

Supertraits act in two directions.

| Direction | What happens | Where |
| --- | --- | --- |
| using a bound | `T < Child` also proves `T: Parent`, with supertrait bindings substituted (`Summable < Add[Out = Self]` gives `T: Add[T, Out = T]`) | elaboration (section 4.2), once per item |
| proving a bound | `X: Child` holds through `impl Child for X`; `X: Parent` is not re-proved | nowhere at use sites |

The second row is sound because the impl check proves every supertrait
under the impl's own bounds
([`trait.super.impl-bounds`](../../spec/lang/09-traits.md#r-trait.super.impl-bounds)).
That check is an impl-check body task in M2. It asks `Implements` for each
supertrait under the impl's environment, and `Project` for each
supertrait binding
([`trait.binding.super.mismatch`](../../spec/lang/09-traits.md#r-trait.binding.super.mismatch)).

### 3.8 What A Goal May Teach The Checker

**Rule TS-3. `Implements` learns only through associated-type bindings.**
`I: Supplier[Item = ?0]` holds with `learned = [?0 := string]` when `I`'s
impl binds `Item = string`
([`trait.binding.inference`](../../spec/lang/09-traits.md#r-trait.binding.inference)).
A unique head match never teaches anything. `Money: Add[?0]` with one
impl `Add[Money] for Money` stalls; it does not learn `?0 := Money`.
Choosing among instantiations is the job of `Instantiations`, which the
checker asks exactly where the spec says: bound-only parameters
([Inference Through A Bound](../../spec/lang/04-type-system.md#inference-through-a-bound))
and the instantiation choice
([Instantiations Of One Generic Trait](../../spec/lang/09-traits.md#instantiations-of-one-generic-trait)).
So adding an impl changes inference only where the spec's own rules let
it. This is a reading of the spec; section 16 notes it.

### 3.9 Compiler-Supplied Impls

Each sealed trait has one function that answers it from the type's form.
These functions are table rows in a `match`, not special cases scattered
through matching.

| Trait | Holds for | Subgoals | Evidence |
| --- | --- | --- | --- |
| `Any` | every value type, `never`, optionals | none | `Builtin(Any)` |
| `AnyVal` | `bool`, `char`, integers, floats, `string`, tuples; a newtype over one of them | the newtype's base | `Builtin(AnyVal)` |
| `AnyRef` | data, enums, `List`, `Map`, function types, trait values, `Any`, suspensions, handles; a newtype over one | the newtype's base | `Builtin(AnyRef)` |
| `Inspectable` | [inspectable types](../../spec/lang/09-traits.md#inspectable-types): primitives, `string`, module-level declarations with inspectable arguments, `List`, `Map`, tuples, `Inspectable` trait values; trait values and `Any` as arguments only | one per type argument, each one level deeper | `Builtin(Inspectable)`; codegen derives the `TypeId` from the type |
| `Tuple` | every tuple type | none | `Builtin(Tuple)` |
| `Num`, `Integer`, `Float` | the fixed primitive lists of [Numeric Traits](../../spec/lang/09-traits.md#numeric-traits) | none | `Builtin(Num)` and so on |
| `Suspend[T]` | compiler frames and the `std.task` types | none | `Builtin(Suspend)` |
| `Structure` | only the target of a template instance, through that instance's environment | none | `Bound` |

For a parameter, every row above answers through the environment only
([`types.sealed.type-parameter`](../../spec/lang/04-type-system.md#r-types.sealed.type-parameter)).
`never` implements neither `AnyVal` nor `AnyRef`.

**Written impls that act like built-ins.** Two kinds live in std's tables
as ordinary rows with special head keys:

- **Numeric families.** `impl[N < Num] Add for N` has head key `Param`.
  It matches a primitive that the family lists, or a parameter whose
  environment proves the family trait. Its bound `N < Num` is then an
  ordinary subgoal. Overlap expands it to one head per member type
  ([`trait.overlap.numeric-family`](../../spec/lang/09-traits.md#r-trait.overlap.numeric-family)).
- **Tuple templates.** `impl[T < Tuple] Eq for T by Structure` has head key
  `TupleAny`. Its bound plan is the walker obligation of
  [`annot.walker.obligation`](../../spec/lang/14-annotations.md#r-annot.walker.obligation):
  one subgoal per element, and for a rest element `List[T]...` either on
  `T` or on `List[T]`, by whether the walker implements `rest`. Resolution
  computes that plan once per template, from the walker's strengthened
  bounds. The evidence is `Impl { def: template, args: [tuple] }`: the
  spec calls each tuple instance "an ordinary implementation"
  ([`annot.template.tuple.instance`](../../spec/lang/14-annotations.md#r-annot.template.tuple.instance)).
  A tuple whose elements fail the obligation does not implement the trait;
  that is a `Fails`, not an error, until a use needs it.

Function types are not special: `Fn` and `SuspendFn` are std constructors
with head keys of their own, and their impls are ordinary rows.

### 3.10 Derives, Delegation And `@error`

The solver never reads a template body. It sees only heads.

| Source | Head in the target module's table | Bounds | Body checked by |
| --- | --- | --- | --- |
| `@derive(X)` on `data Box[T]` | `impl[T < X] X for Box[T]`, origin `Derived { template }` | `T < X` per walked parameter ([Derived Bounds](../../spec/lang/14-annotations.md#derived-bounds)) | the derive-instance body task ([§4.13.9](checking-and-tir.md#4139-derive-instances-templates-facts-and-test-overlays)) |
| a derivation block with a header | the header as written | the header's bounds | same |
| `@derive(X)` on a newtype | `impl X for Mile` | none at the head; the base type's impl is an impl-check goal | the impl check |
| `impl Tr for C by E` | as written | as written | generated forwarders, checked as written ([Generated Methods](../../spec/lang/09-traits.md#generated-methods)) |
| `@error`, `@from`, `@source` | `Display`, `Error`, `From[P]` heads | the generated bounds of [Generated Error Bounds](../../spec/lang/14-annotations.md#generated-error-bounds) | the generated bodies |

- **Delegated associated types.** Each associated type of `impl Tr for C
  by E` is bound to the projection `<E's type as Tr>::Name`, with `C`'s
  arguments substituted. Normalizing it is an ordinary `Project`.
- **Hidden helpers.** A template body may call private helpers of the
  trait's module, which the interface exports as hidden items
  (owner, 2026-10-07). Calls to them resolve through the template's
  resolution table. Trait goals inside the instance run under the
  instance's environment like any other body; the solver needs nothing
  more.
- **Member obligations and coinduction.** The derive-instance task checks
  each walked member against the walker's bound. Recursion is coinductive
  ([`annot.bound.recursive`](../../spec/lang/14-annotations.md#r-annot.bound.recursive)):
  while checking `Tree[T]`'s members, `Tree[T]: Eq` is assumed. No
  mechanism is needed for that (mine). The derived head
  `impl[T < Eq] Eq for Tree[T]` is already in the target module's table,
  and the solver never re-checks an impl's members at a use. So the member
  `children: List[Tree[T]]` asks `List[Tree[T]]: Eq`, which commits to
  `List`'s impl, then to the derived head, then proves `T: Eq` from the
  environment. The search meets no cycle and holds no assumption, so no
  memo entry can depend on one (rule TS-7). This is how rustc checks an
  impl's well-formedness with the impl itself in scope.
- **`missing-derived-bound`.** A use whose failing subgoal is a bound that
  a derived head added reports `missing-derived-bound`, not
  `unsatisfied-trait-bound`
  ([`trait.derive.bound-unmet`](../../spec/lang/09-traits.md#r-trait.derive.bound-unmet)).
  `FailInfo` carries the impl's origin, so the checker can tell (section
  10.2).

### 3.11 The Synthetic Impl Inventory

Every impl that no one wrote as `impl ... :` has a stable descriptor, a
place where its head lives, a dependency that invalidates it, an overlap
rule and an evidence form. The Codex review (A5) asked for this table.

| Family | Descriptor (stable) | Head lives in | Invalidated by | Overlap | Evidence |
| --- | --- | --- | --- | --- | --- |
| `@derive(X)`, derivation block | target path, trait path, `Derived` | the target folder's `heads` section | the target folder's interface; the template body via the trait folder's deep hash | ordinary heads; beside a written impl, `overlapping-impl` at folder time or in `Coherence` | `Impl` |
| tuple template of any trait | trait path, `TupleTemplate` | the trait folder's `heads` section, head key `TupleAny` | the trait folder's interface | `TupleAny` joins every tuple head: a written tuple impl of the trait is `overlapping-impl` ([`annot.template.tuple.overlap`](../../spec/lang/14-annotations.md#r-annot.template.tuple.overlap)) | `Impl` of the template at the tuple type |
| numeric family | impl path, `NumericFamily` | std's `heads` section, head key `Param` | std's interface | expanded per member type | `Impl` |
| `@error`, `@from`, `@source` | target path, trait path, member index | the target folder's `heads` section | the target folder's interface | ordinary heads; two `@from` of one type at folder time | `Impl` |
| delegation `by E` | impl path, `Delegated` | written head | the folder's interface | ordinary head | `Impl` |
| `Any`, `AnyVal`, `AnyRef`, `Tuple`, `Num`, `Integer`, `Float`, `Suspend` | the trait | the compiler (section 3.9) | the toolchain key | sealed: no user impl can exist | `Builtin` |
| `Inspectable`, `TypeId::of` | the trait | the compiler | the toolchain key; a type's inspectability depends only on its declaration's place (module level or block) | sealed | `Builtin` |
| `Structure` | the template instance | the instance's environment | the instance | never an impl | `Bound` |
| function types `Fn`, `SuspendFn` | not synthetic: std constructors | ordinary heads | ordinary | ordinary | `Impl` |

A user library's tuple template is no different from std's: the module
that uses `==` or `.encode()` on a tuple names the trait, so the trait
folder is in its key, and a template edit rechecks it.

## 4. Associated Types And Normalization

### 4.1 Where Projections Come From

A projection `<S as Tr[A]>::Name` is the pool's `Assoc { assoc, tref }`
type (section 2.1): the associated item and the instantiated reference of
the trait that declares it. Projections come from:

- signatures: `T::Item`, `Self::Item`, `I::Item` in a result type;
- instantiated method results: `Self::Out` of `add` becomes
  `<Money as Add[Money]>::Out`;
- impl bindings that name another projection, as a delegated impl's do;
- bounds with bindings: `I < Supplier[Item = T]` relates `I::Item` and `T`.

### 4.2 Elaboration Of The Environment

`elaborate` turns an item's declared bounds into its `ParamEnv` once:

1. Each declared bound `P < Tr[A, N = U]` is a clause, with its binding.
2. For each clause, walk `Tr`'s supertraits depth first, in declared
   order, substituting `P` for `Self` and the clause's arguments for the
   trait's parameters. A supertrait binding (`Num < Add[Out = Self]`)
   becomes a binding on the new clause (`P: Add[P, Out = P]`). A binding
   written on the bound for a supertrait's associated type
   ([`trait.binding.name-reach.meaning`](../../spec/lang/09-traits.md#r-trait.binding.name-reach.meaning))
   lands on that supertrait's clause.
3. A trait reference already present is skipped. The supertrait graph is
   acyclic (`supertrait-cycle` is a header error), so the walk ends, and
   diamonds produce each trait once.
4. Intern the clause list; its content hash is the `EnvKey`.

Cost: linear in the size of the supertrait closure of the declared bounds.
`Num`'s closure is about fifteen clauses, `Integer`'s about twenty-five.
Elaboration runs once per item, not per goal.

Header checks that need the environment, such as
`ambiguous-associated-type` on `I::Item` with two bounds declaring `Item`,
are made by resolution with the same walk, before any body.

### 4.3 Normalization: Lazy, At Three Points

The solver normalizes a projection only when someone needs its value, as
rustc's lazy normalization does. The checker asks `Project`:

1. **When unification meets a projection** against a type that is not the
   same projection. `Assoc` against `Assoc` with equal parts unifies
   structurally first.
2. **Before shallow resolution for method lookup or field access** on a
   value whose type is a projection.
3. **Before canonicalizing a goal** whose types contain a projection with
   a known base (section 2.2 step 3).

`Project` computes, for `<S as Tr[A]>::Name`:

| `S` is | Answer |
| --- | --- |
| an inference variable, or holds one at a position the head needs | `Stalled` |
| a parameter or existential with an environment binding for `Name` | the bound type: `I::Item` is `T` under `I < Supplier[Item = T]` |
| a parameter or existential with a bound on `Tr` but no binding | the projection itself, as a rigid type. It equals only itself |
| a trait value `Tr[A, Name = U]` or a subtrait's value that binds it | `U` ([`trait.dyn.bound.projection`](../../spec/lang/09-traits.md#r-trait.dyn.bound.projection)) |
| known | select the impl by head (section 3.4), read its binding for `Name`, substitute the impl's arguments, and normalize the result once more |
| known, but no impl | `Fails` |

- **One answer, no outer projection.** Each `Project` returns a type with
  no outer projection, by normalizing its result. A chain
  `<<T as A>::X as B>::Y` normalizes inside out. Each step is one goal
  level, so a chain counts against the depth limit and is memoized step by
  step.
- **Bindings in `Implements`.** `S: Tr[A, Name = U]` holds when `S: Tr[A]`
  holds and the normal form of `<S as Tr[A]>::Name` unifies with `U`. If
  `U` holds placeholders, the unification gives the learned bindings
  (rule TS-3). If they disagree, the answer is `Fails` with the binding
  as the reason (`unsatisfied-trait-bound`,
  [`trait.binding.mismatch`](../../spec/lang/09-traits.md#r-trait.binding.mismatch)).
- **Rigid projections and equality.** Under `T < Supplier`, `T::Item` and
  `string` never unify. Under `I < Supplier[Item = T]`, `I::Item` and `T`
  are the same type, because normalization replaces the projection before
  any comparison
  ([`trait.binding.interchangeable`](../../spec/lang/09-traits.md#r-trait.binding.interchangeable)).
- **The result.** `Project` answers `Normalized { ty, evidence, learned }`.
  `ty` is stored over placeholders in the memo and mapped back to the
  caller's variables. `evidence` is the impl, clause or trait value that
  gave the binding.
- **Projections over variables and the occurs check.** Unifying `?0` with
  `<?0 as Tr>::Item` must not bind `?0`. The checker asks `Project` first;
  if it stalls, it keeps a projection-equality obligation `?0 ==
  <?0 as Tr>::Item` instead of binding, and retries it when `?0` is bound.
  A rigid projection whose base holds `?0` counts as an occurrence of `?0`
  only after normalization fails.
- **Normalization cycles.** An impl binding that names its own projection,
  directly or through other impls, is a cycle on the stack: `Overflow`
  (section 6.3).
- **Aliases** are expanded when headers are lowered, so a projection never
  meets an alias.

### 4.4 Projections In Impl Heads

The spec does not say whether an impl head may contain a projection of an
impl parameter, as in `impl[I < Store] Summary for Feed[I::Item]`. Such a
head is not injective: two different `I` may give one `Feed` type, so
matching cannot solve `I`, and overlap cannot be decided from heads.
Section 16 question 2 asks the owner. Until then, resolution reports the
head as `unconstrained-impl-parameter`, since `I` appears only under a
projection.

## 5. Coherence

### 5.1 What Runs When

| Check | Needs | Runs at | Error |
| --- | --- | --- | --- |
| orphan rule (package level) | the impl head and package ownership | folder interface, step 5 | `orphan-impl` |
| module ownership | the head and the declaring modules | folder interface | `nonlocal-impl` |
| targets: bare parameter, trait value, outer `mut`, invalid inherent target | the head | folder interface | `bare-parameter-impl-target`, `trait-value-impl-target`, `mutable-impl-target`, `invalid-impl-target` |
| unconstrained parameters and the bound plan | the head and bounds | folder interface | `unconstrained-impl-parameter` |
| sealed traits | the trait | folder interface | `sealed-trait-implementation` |
| a second template; `@derive` beside a written impl or a block for the same trait | the module's heads | folder interface | `overlapping-impl` |
| law partners all derived or all written | the module's heads | folder interface | `mixed-derived-law` |
| supertraits and supertrait bindings of an impl | the solver, under the impl's environment | impl check, M2 | `missing-supertrait-implementation` |
| overlap between impls | heads of one trait across the program graph | `Coherence(trait)` task | `overlapping-impl` |
| duplicate inherent members | inherent heads of one type, in its module | folder interface | `duplicate-inherent-member` |
| local impls | the body's local impls | the body | the same codes |

Only the impl check needs goals. Everything else is a function of heads,
so it runs before any body and caches with the interface.

### 5.2 The Overlap Check

One `Coherence(trait)` task runs per trait with more than one impl in the
program's graph, as D1 has it
([§4.12.3](resolution-and-interfaces.md#4123-coherence)). D1's pairwise
unification within buckets is quadratic: the Codex review (T6) counts
50 million pair checks for 10,000 heads under one constructor. This design
replaces it with a near-linear algorithm (mine):

1. **Collect** the trait's heads from the `heads` sections of every folder
   in the graph, in content order. Derived, generated and delegated heads
   and tuple templates are among them (section 3.11).
2. **Expand** numeric-family heads to one head per member type.
3. **Ground heads** (no impl parameter): encode each head canonically and
   insert it into a hash set. A second equal head is an overlap. Linear.
4. **Generic heads**: insert each into a **discrimination tree**, a trie
   over the head's pre-order walk in which an impl parameter is a
   wildcard. Before inserting a head, query the tree for stored heads
   that unify with it, and query the ground set by walking the head's
   wildcard positions against the ground heads' trie. A row parameter
   matches every row; `Args < Tuple` matches every tuple; `TupleAny`
   matches every tuple head.
5. **Confirm** each candidate pair with full unification after renaming
   apart, and report it once, on the later impl in content order
   (package, module path, offset). The message names the earlier impl and
   a **witness**: the unifier's solution applied to the head, such as
   "both apply to `Box[Plain]`".
6. **Budget.** The task counts trie nodes visited and stops after the
   first overlap reported per impl, so an all-overlapping bucket costs one
   report per impl, not one per pair.

Cost: linear in the total head size for disjoint heads, which is the
normal case, and linear in the reported overlaps otherwise. 300 `From`
impls for one error type differ at the first trait argument, so the trie
separates them at its second level.

### 5.3 Why No Global Index

- **Selection needs no global view.** Ownership confines the impls a goal
  can use to at most `2 + len(Args)` modules, plus the candidate directory
  for open arguments (section 3.2), which is per trait and holds only the
  rare argument-owned impls.
- **Coherence needs a per-trait view, not a global one.** Each trait is
  one task keyed by its sorted head hashes, so an edit reruns only the
  traits whose heads changed.
- **A global index would be a shared mutable structure** built as folders
  finish, and its state at a given moment would depend on the schedule.
  The owner tables are frozen with their folder, so a body can solve
  while unrelated folders are still being resolved.

### 5.4 What The Solver Assumes

Bodies are checked before or while coherence runs; they do not wait for
it. The solver assumes no overlap. If two heads match a goal, which only an
overlap error allows, the solver takes the first in content order and
reports nothing (section 3.5). The program fails on `overlapping-impl`, so
the choice never reaches a build.

`orphan-impl` names the concrete modules that may hold the impl (the
trait's, the type's, or a trait argument's); `nonlocal-impl` adds a move
fix-it when exactly one module of the package qualifies.

## 6. Search

### 6.1 Depth First, On An Explicit Stack

```rust
struct Frame {                    // 24 bytes
    goal: CanonGoalRef,           // u32 into the memo's key arena
    impl_row: u32,                // the committed impl, or NONE
    step: u16,                    // next bound-plan step
    depth: u8,                    // depth from the use (section 7.3)
    height: u8,                   // deepest relative level reached below
    cost: u32,                    // intrinsic cost so far (section 7.4)
    subst: u32,                   // range in the scratch buffer: the impl's parameters
    seen: u32,                    // range in the scratch buffer: distinct children so far
}
```

**Rule TS-9. No native recursion in the solver.** Goals are frames on a
vector. A subgoal pushes a frame; an answer pops it and resumes the
parent. The deepest stack is bounded by the depth limit (section
7.3), so it never overflows the native stack, including in the browser.

### 6.2 The Search Graph

With rule TS-2, a goal has one way to be proven. The graph of goals is a
graph of AND nodes:

- a node is a canonical goal;
- its children are the steps of its committed impl's bound plan, or the
  inner goals of a compiler-supplied row, or one `Project`.

So a goal's answer is a function of its children's answers, combined in
plan order: the first `Fails`, `Overflow` or `Stalled` child decides,
otherwise `Holds`. There is no OR node except the choice among sources in
section 3.1, which is ordered.

### 6.3 Cycles

A goal that meets itself on the stack is a cycle.

- **Coinductive cycles.** The spec has one: a derived impl's member check
  ([`annot.bound.recursive`](../../spec/lang/14-annotations.md#r-annot.bound.recursive)).
  It never reaches the search: the derived head is in the table, and the
  solver never re-checks members at a use (section 3.10).
- **Inductive cycles.** Every other cycle has no finite proof. Its depth
  would exceed 64, so the spec's answer is `trait-resolution-depth`
  ([`trait.bound.depth.limit`](../../spec/lang/09-traits.md#r-trait.bound.depth.limit)).
  The solver answers `Overflow` at once, without going 64 levels deep.

**Rule TS-7. No provisional results, no fixpoint (mine).** A frame on the
stack is exploring its current plan step, and every earlier step of it has
completed with `Holds`. Those completed answers are context-free (section
7.1), so a fresh evaluation of any frame retraces the same path. Hence:

- **The SCC rule.** When goal `G` meets itself, the frames from `G` to the
  top of the stack form one strongly connected set: each reaches the
  others along the stack path. Every one of them answers `Overflow`,
  whichever was asked first.
- **Completion before publication.** A frame's entry is published only
  when the frame pops, with its final answer. Members of the set pop as
  `Overflow`. A frame that completed earlier with `Holds` did not reach
  `G`, so it is not in the set, and its entry stands.
- **No provisional answer ever leaves the stack.** So every entry, in a
  cycle or not, can be memoized at once.

This removes the part of rustc's search graph that took the most work:
provisional caches, cycle heads, fixpoint iteration, and the rule that
only cycle roots enter the global cache. It holds only because no node has
a second route (rule TS-2) and no cycle is coinductive.

The "bounded monotone fixpoint" of the prior-art lessons is therefore not
needed in the solver. The checker keeps the one fixpoint the spec defines,
the bound-only inference loop
([`types.generic.infer.bound.fixed-point`](../../spec/lang/04-type-system.md#r-types.generic.infer.bound.fixed-point)),
which is bounded by the number of bound-only parameters
([type-checking.md §2.4](type-checking.md#24-calls-and-use-site-type-arguments)).

### 6.4 Stalling

A goal stalls when its answer depends on a placeholder:

- the self type is a placeholder: `?0: Display` stalls on `?0`;
- a head match reports `Maybe`: `Money: Add[?0]` with two `Add` impls
  stalls on `?0`;
- a child stalls: `List[?0]: Eq` commits to `impl[T < Eq] Eq for List[T]`,
  and its child `?0: Eq` stalls, so the goal stalls on `?0`.

The stalled answer lists the placeholders in canonical order, mapped back
to the body's variables. A stall is never memoized globally, since its
scope is `Body`. The body memo keeps it, because asking the same canonical
goal again gives the same stall.

**Literal placeholders.** An integer-literal placeholder stalls like any
other. A head that is not an integer type rules it out (`No` instead of
`Maybe`). So `?int: Display` stalls, and `?int: Pairing` with no integer
impl fails at once. Literal defaulting at the end of the statement then
wakes the obligation.

### 6.5 Ambiguity

`Many` belongs to `Instantiations` and `Methods` only.

**`Instantiations { S, Tr }`** collects every impl head of `Tr` that
matches `S` with all trait arguments open: the owner tables of `Tr` and
`S`, and the candidate directory (section 3.2). For a parameter, it
collects the environment clauses on `S` for `Tr`; for a trait value, the
one instantiation the value names. The solver checks each candidate's
bound plan and drops those that fail. The answer is `Holds` for one
survivor, `Many` for several, in content order, and `Fails` for none. The
checker's trial then chooses among `Many`
([type-checking.md §2.5](type-checking.md#25-methods-and-operators)).

**`Methods { receiver, name }`** returns, in this order:

1. inherent candidates of the receiver's nominal type whose impl target
   matches it
   ([`trait.inherent.target-match`](../../spec/lang/09-traits.md#r-trait.inherent.target-match));
2. for a parameter, the methods named `name` of its elaborated clauses;
   for a trait value, the methods of its trait and supertraits;
3. otherwise, each available trait
   ([Trait Availability](../../spec/lang/09-traits.md#trait-availability))
   that declares `name` or reaches it through a supertrait, and that the
   receiver implements, by one `Instantiations` goal each.

A per-module method index maps a name to the available traits that
declare it, so step 3 asks one goal per trait with that name, not one per
available trait. The answer also lists, for diagnostics only, traits that
are not available but would match: the `use` fix-it.

## 7. Memoization And Budgets

### 7.1 Memo Keys And Eligibility

An answer may be reused only where every input that decided it is equal.
The Codex review (blocker 8) listed the inputs that D1's key missed. Each
is now part of the key, or makes the goal ineligible for the global memo:

| Input that can change an answer | How the key covers it |
| --- | --- |
| the goal's types, with variables | the canonical goal; placeholders keep their kinds (section 2.2) |
| declared bounds and supertrait bindings | `EnvKey`, the interned elaborated environment; `EMPTY` when the goal names no parameter |
| GADT arm equalities | substituted before keying; existentials make the goal `Body` scope |
| local impls, visible from their declaration point ([`trait.impl.local.lookup`](../../spec/lang/09-traits.md#r-trait.impl.local.lookup)) | `LocalVis`: the interned sorted list of visible local impls, in the `Body` key; a goal that names no local type or local trait cannot match a local impl and keys with `EMPTY` |
| trait availability (`Methods` only) | the module's `AvailKey` plus the lexical scope's local traits |
| coinductive assumptions | none exist (section 3.10) |
| proof depth | the stored height, checked at each use (section 7.3) |
| remaining fuel | never stored: `OutOfFuel` makes the frame and every ancestor in the same `solve` call ineligible |
| the interfaces of the run | the global memo lives for one run: one frozen set of interfaces. A long-lived process (the playground, the REPL, a watch loop) starts a new memo per run, so no entry survives a source revision |

**Rule TS-4. Only completed, context-free answers are published
globally.** An entry goes to the global memo only when its scope is
`Global` or `Env`, it completed (it popped with a final answer, rule
TS-7), and it never met the depth cut or ran out of fuel (section 7.3).
Every other entry goes to the body memo or nowhere.

**Remapping.** Answers are stored over placeholders and global types.
`learned`, `Normalized.ty` and `Stalled.on` are mapped back through the
caller's `CanonVars`. No entry holds a body-local `Ty` or `InferVar`, so a
rollback cannot leave a dangling entry.

```rust
pub struct GlobalMemo {                         // one per run; shared by checking and codegen
    shards: [Mutex<RawTable<u32>>; 64],         // hash(scope key) -> entry index
    entries: PerThreadAppend<MemoEntry>,        // as the InternPool's columns
    keys: PerThreadAppend<u32>,                 // canonical goals, encoded
}
pub struct BodyMemo {                           // per body; owned by BodyCx, lent per call
    table: RawTable<u32>,
    entries: Vec<MemoEntry>,
    keys: Vec<u32>,                             // its own arena; never truncated by a rollback
    met: BitVec,                                // goals this body has been charged for (7.4)
}
#[repr(C)]
pub struct MemoEntry {                          // 12 bytes
    answer: CanonAnswerRef,                     // u32: evidence and learned bindings over placeholders
    cost: u32,                                  // intrinsic cost (7.4); saturating
    height: u8,                                 // intrinsic height (7.3); 255: a cycle
    kind: u8,                                   // Holds | Fails | Stalled | Overflow | AtLeast
    _pad: [u8; 2],
}
```

- **First writer wins** in the global memo. Two threads that compute one
  key compute the same entry (rule TS-1), so either write is correct.
- **Not persisted** in the first release. A persisted memo would need its
  own key (the hashes of every impl table it read), and a warm check
  rechecks only edited modules, whose goals take microseconds.

### 7.2 Depth Is Charged From The Use

The spec counts depth from the use: a bound required directly by a use has
depth 1, and a bound of the impl that proves a depth-`n` bound has depth
`n + 1`. More than 64 is `trait-resolution-depth`, "whether or not a
deeper proof would succeed"
([`trait.bound.depth.limit`](../../spec/lang/09-traits.md#r-trait.bound.depth.limit)).

### 7.3 Heights And Lower Bounds

A memoized answer must not depend on the depth at which it was first
computed. rustc stores the depth an entry reached and caches overflow per
remaining depth. This design does the same in two entry kinds:

1. **Exploration is cut at the absolute limit.** A goal computed at depth
   `d` may explore `r = 65 - d` levels, itself included.
2. **A goal that completes without meeting the cut** stores its answer and
   its **height** `h`: the deepest level it used, counted from itself. The
   answer is context-free, because more depth would not change it. A use
   at depth `d'` gets `Overflow` when `d' + h - 1 > 64`, and the stored
   answer otherwise.
3. **A goal that meets the cut** stores `AtLeast(r + 1)`: "with `r` levels
   or fewer, this is `Overflow`". A later use with at most `r` levels left
   reuses it as `Overflow`. A use with more levels left computes the goal
   again, and may store a better entry. Such an entry is never global and
   never a `Fails`, so depth exhaustion is never cached as a semantic
   failure (review blocker 8).
4. **A cycle** stores `Overflow` with height 255: no depth suffices.

So whether a goal overflows depends only on the goal and on where it is
used, never on which body computed it first. The stack never holds more
than 64 goal frames plus their `Project` steps, which count as levels.

**What counts as a level** (a reading; section 16 question 3): a bound-plan
step of an impl, an element obligation of a tuple template, one
`Inspectable` step through a type argument, and one `Project` step. An
environment clause, a trait-value source and a sealed membership with no
subgoal are leaves at no extra level.

### 7.4 Fuel

type-checking.md fixes the principle: fuel used is a pure function of the
body and its frozen inputs
([type-checking.md §11.1](type-checking.md#111-what-counts)).

**Intrinsic cost.** A goal's cost is computed when its entry is made:

```text
cost(g) = 1                                (the goal)
        + heads matched in its probe       (rows that survived the fast reject)
        + sum of cost(c) over the distinct children c of g
```

"Distinct" removes repeats among one goal's own children, as in
`Pair[X, X]`, where both bound steps ask `X: Eq`. A child's cost is its
entry's stored cost, whether the child was computed now or found in a
memo. So `cost(g)` is a property of the goal alone. It saturates at
`u32::MAX`.

**Rule TS-5. Charge the intrinsic cost the first time a body meets a goal,
and 1 for each repeat (mine).** A body's `met` set records which canonical
goals it has been charged for. The first `solve` of a goal in a body
charges `cost(g)`, whether the entry was in a memo or not. Each later
`solve` of the same canonical goal in that body charges 1. The sequence of
goals a body asks is fixed by its source and its frozen inputs, so "first
time in this body" is deterministic. A rollback refunds nothing and does
not clear the `met` set.

Charging the full cost on every hit would bill the 1,600 `Cents::from(N)`
lines of the `pathological` metric 1,600 times for one goal. Charging only
what was computed would make a warm memo cheaper than a cold one, so a
body could pass its budget on one run and fail on the next (lesson 3 of
[Lessons For hd](prior-art-issues.md#lessons-for-hd)).

### 7.5 Limits And Their Diagnostics

| Limit | Value | Counted by | Diagnostic |
| --- | --- | --- | --- |
| proof depth | 64, fixed by the spec | heights (7.3) | `trait-resolution-depth` at the use, showing the first three goals of the chain and the last |
| body fuel | 2,000,000 steps per body | intrinsic costs (7.4) | `item-too-complex`, by the checker |
| type size | 10,000 nodes | `mk` | `type-too-large` (proposed), if a subgoal's argument grows |
| placeholders per goal | 255 | canonicalization | none: the goal stalls, which ends as `cannot-infer-type` |
| candidates in one `Many` | none | | none needed: bounded by the impls that exist |

**F-626-style blowups are impossible**, for four reasons:

1. No checker state is copied. The solver reads inference variables and
   writes nothing (rule TS-1), and the checker's trials truncate
   ([type-checking.md §3.5](type-checking.md#35-the-trail-and-the-one-rollback-contract)).
2. One `Instantiations` goal per distinct receiver and trait is computed
   once per run and found in the memo after that.
3. No goal has more than one committed impl (rule TS-2), so no search
   explores alternatives.
4. Shared subgoals are computed once and charged by intrinsic cost, so a
   diamond costs about the size of its DAG.

### 7.6 Bounded Is Not Linear

Fuel and depth make every case end. They do not make it fast. The Codex
review (T6) asks for the algorithms that make the common cases
near-linear; these are they, each with the input it is linear in:

| Work | Algorithm | Linear in |
| --- | --- | --- |
| repeated goals | the memo, keyed canonically; repeats cost one lookup | distinct goals |
| shared subgoals | memoized children, intrinsic DAG cost | distinct goals in the proof DAG |
| impl lookup | head key, then first two argument keys; the fast path for non-generic traits | candidates that share both keys, usually one |
| open trait arguments | the per-trait candidate directory, filtered by the closure bit set | argument-owned impls of that trait |
| overlap | ground heads hashed; generic heads in a discrimination tree (section 5.2) | total head size, plus reported overlaps |
| elaboration | once per item, skipping repeated trait references | the supertrait closure |
| canonicalization | one walk of the goal, charged one step per node | goal size |
| normalization | one memoized `Project` per projection | distinct projections |
| instantiation choice | prefilter candidates by the head of each known argument type before any trial (change 7); one trial per surviving candidate | surviving candidates |
| nested trial chains | a per-call-site trial memo keyed by the canonical expected type and the canonical argument types (mine; change 8). A site under `n` outer candidates sees at most `n` distinct expected types, so 20 nested levels of 2 candidates cost about 80 trials, not a million | sites × candidates per site |
| obligations | each obligation is queued once per wake round, deduplicated by index; a retry is charged its goal's size | obligations × bindings of their variables |

The `pathological` suite checks the scaling claims, not only the limits:
each case runs at sizes `n` and `2n`, and the run reports the observed
exponent. Above 1.2 fails the case (section 14.2).

## 8. Evidence

### 8.1 What A `Holds` Answer Carries

```rust
pub enum Evidence {
    Impl { row: ImplRef, args: TyList },     // written, derived, delegated, generated, numeric family, tuple template
    Bound { param: ParamRef, index: u16 },   // clause index in the elaborated environment
    TraitValue { trait_: DefId },            // the self type is a trait value of this trait or a subtrait
    Builtin(BuiltinImpl),                    // Any, AnyVal, AnyRef, Inspectable, Tuple, Num, Integer, Float, Suspend
    Poison,
}
// type-checking.md's `Coinductive` is not needed: a derive instance's member
// goals reach the derived head as an ordinary `Impl` (section 3.10).
```

Only the **top-level choice** is evidence. The nested proof (which impls
proved the bounds) is not stored in TIR, because codegen monomorphizes: at
each instance the nested bounds are concrete and are selected again by
head (rule TS-6). The solver keeps the nested proof only in the memo, for
`FailInfo` and for `hd debug solve`.

### 8.2 What TIR Records

| Use | TIR | Evidence recorded |
| --- | --- | --- |
| a trait method call with a known self type or a bound | `Call` with a `TraitMethod` callee | the choice: `Impl`, `Bound`, `TraitValue` or `Builtin` |
| a call through a GADT existential | `Call` with an `Evidence` callee | the matched value and the clause index of the existential's bound |
| a conversion to a trait value | `Coerce` of kind to-trait-value | the concrete type, the trait reference and the `Impl` choice |
| a variant construction with bounded existentials | `NewVariant` evidence choices | one `Evidence` per bound, at the construction's types |
| a call through a trait value whose method has bounded method-level parameters | `CallDyn` | one `Evidence` per method-level bound (section 9.2) |

The `TraitMethod` choice in D2's catalog is "the impl's `DefId` or the
index of the bound in scope". This design needs two more choices,
`TraitValue` and `Builtin` (section 16.4, changes 4 and 16). A tuple template
instance is an `Impl` choice and needs none.

### 8.3 What Codegen Does With It

**Rule TS-6. Codegen selects by head only (mine).** At an instance, every
type is concrete. Overlap is head-only, so at most one head matches a
concrete trait reference, and the checker has already proven its bounds.
`select` therefore matches heads in the owner modules and returns the impl
and its arguments. It never solves a subgoal, has no depth limit, and
charges no fuel. It is memoized in the global memo under `Global` scope.

- **No depth limit at instances.** The spec's 64 counts from a use in
  source. An instance of generic code may need a deep concrete proof that
  no source use asked for. The representation of generic code is free
  ([`trait.bound.representation`](../../spec/lang/09-traits.md#r-trait.bound.representation)),
  so this is no language error. Polymorphic recursion is stopped by the
  instantiation depth limit instead
  ([§13.4](codegen.md#134-the-instantiation-depth-limit)).
- **A failed select is an internal error**, never a user diagnostic: the
  checker proved the goal generically. CI asserts it
  ([§12.7](codegen.md#127-emission-time-checks)).
- **Default methods.** A `TraitMethod` call resolved to an impl that does
  not write the method instantiates the trait's default body, with `Self`
  set to the impl's target.
- **Vtables.** For each `(type, trait reference)` that a coercion, an
  evidence choice or a `CallDyn` bound needs, codegen selects the impl of
  the trait and of each supertrait, and fills the vtable shape (section
  9.2).

## 9. Trait Values And Dynamic Safety

### 9.1 Which Traits Can Be Values

Dynamic safety is a property of the trait declaration and of the value
type's bindings. So resolution decides it at interface time from headers;
the solver does not
([Dynamic Safety](../../spec/lang/09-traits.md#dynamic-safety)). The trait
record stores one flag and the first reason it fails:

| Reason it is not safe | Rule |
| --- | --- |
| an associated function in the trait or a supertrait | [`trait.dyn.safe.assoc-function`](../../spec/lang/09-traits.md#r-trait.dyn.safe.assoc-function) |
| `Self` outside the receiver | [`trait.dyn.safe.self`](../../spec/lang/09-traits.md#r-trait.dyn.safe.self) |
| a method-level type parameter whose elaborated bounds do not include `AnyRef` | [`trait.dyn.safe.implied-anyref-param`](../../spec/lang/09-traits.md#r-trait.dyn.safe.implied-anyref-param) |
| at the value type: an associated type left unbound | [`trait.dyn.binding.complete`](../../spec/lang/09-traits.md#r-trait.dyn.binding.complete) |

The method-level check uses `elaborate`: `T < Error` implies `AnyRef`,
because `Error` has `AnyRef` as a supertrait. Requirement keys reuse the
flag ([`req.key.dynamically-safe`](../../spec/lang/11-requirements-and-suspension.md#r-req.key.dynamically-safe)).

### 9.2 Vtable Shapes

A **vtable shape** is computed once per trait at interface time and stored
in the trait record (mine):

```rust
pub struct VtableShape {
    slots: Box<[SlotRef]>,          // the trait's own methods, in declaration order
    supers: Box<[DefId]>,           // direct supertraits, in declared order; each a pointer to its vtable
    type_id: bool,                  // the trait is Inspectable or extends it
    dyn_bounds: Box<[(u16, u8)]>,   // per slot: how many method-level bounds take evidence at the call
}
```

- **Supertraits by pointer, not by copying their slots.** A diamond such as
  `Error < Display & Inspectable & AnyRef` shares one vtable per
  supertrait. Widening to a supertrait value reads one pointer
  ([`trait.dyn.widen`](../../spec/lang/09-traits.md#r-trait.dyn.widen)).
- **Method-level bounds.** A dynamically safe method may have a parameter
  `T < AnyRef & Display`. Its one body takes the evidence for `Display`
  with each call
  ([`types.trait.safe.one-body`](../../spec/lang/04-type-system.md#r-types.trait.safe.one-body)).
  So a `CallDyn` carries one vtable per such bound, chosen by the checker
  at the call. This is the one place outside GADTs where a dictionary is
  passed at run time.
- **The slot's ABI (review blocker 5).** The slot's body is the impl
  method compiled once with each method-level parameter erased to the
  reference shape (`anyref`), which `AnyRef` guarantees, and one vtable
  parameter per bound. A caller that knows `T` casts the result back to
  `T`. Statically dispatched calls of the same method stay monomorphized;
  only the vtable slot uses the erased instance. `Inspectable.downcast[T]`
  is the std case: its `T < AnyRef & Inspectable` evidence carries the
  `TypeId` it compares.
- **Row parameters** pass their providers as one bundle and need no slot.

### 9.3 Trait Values As Self Types

A trait value type satisfies a bound on its own trait and on each
supertrait ([`trait.dyn.bound`](../../spec/lang/09-traits.md#r-trait.dyn.bound)).
These are the rows of source 2 in section 3.1:

| Goal | Holds when |
| --- | --- |
| `Tr[A]: Tr[B]` | `A` equals `B` ([`trait.dyn.bound.instantiation`](../../spec/lang/09-traits.md#r-trait.dyn.bound.instantiation)) |
| `Child[A]: Parent[B]` | the elaborated supertraits of `Child[A]` contain `Parent[B]` |
| with a binding `Name = U` | the value binds `Name` to `U` ([`trait.dyn.bound.binding`](../../spec/lang/09-traits.md#r-trait.dyn.bound.binding)) |
| with `mut_` set | the value is `mut Tr` ([`trait.dyn.bound.mut`](../../spec/lang/09-traits.md#r-trait.dyn.bound.mut)) |
| any other trait | never: the value satisfies no other bound and is no impl target ([`trait.dyn.bound.no-impl`](../../spec/lang/09-traits.md#r-trait.dyn.bound.no-impl)) |

The first row is the prototype's F-618: a trait value of a generic trait
did not satisfy the instantiated bound.

### 9.4 Conversion To A Trait Value

`S` converts to `Tr[A, N = U]` when `Implements { S: Tr[A], bindings:
[N = U], mut_ }` holds, with `mut_` set for `mut Tr`
([`trait.dyn.binding.convert`](../../spec/lang/09-traits.md#r-trait.dyn.binding.convert),
[`trait.dyn.mut`](../../spec/lang/09-traits.md#r-trait.dyn.mut)). `Any`
always converts. `Inspectable` converts through its compiler-supplied row,
which checks inspectability. A trait extending `Inspectable` still needs
its own impl
([`trait.erase.child-impl`](../../spec/lang/09-traits.md#r-trait.erase.child-impl)).
A failure is `type-mismatch`, not `unsatisfied-trait-bound`: the checker
picks the code from the site, and `FailInfo` gives the reason.

## 10. Diagnostics

The solver writes no text. It returns `FailInfo`, and the checker renders
it (rule TS-1).

### 10.1 `FailInfo`

```rust
pub struct FailInfo {
    pub leaf: CanonGoalRef,                  // the deepest failing goal on the committed path: the root cause
    pub chain: SmallVec<[ChainStep; 4]>,     // the committed impls from the asked goal down to the leaf
    pub reason: FailReason,
    pub near: SmallVec<[NearMiss; 3]>,       // at most three, in content order
}
pub struct ChainStep { pub impl_row: ImplRef, pub step: u16, pub origin: ImplOrigin }
pub enum FailReason {
    NoImpl,                                  // no head matches the leaf
    Binding { name: Symbol, found: Ty },     // the impl binds another type
    NotMutable,                              // a `mut` bound on a readonly type
    NotInspectable { arg: Ty },              // the first type argument that is not inspectable
    WrongCategory,                           // AnyVal for a reference, AnyRef for a value
    Sealed,                                  // a sealed trait the type is not listed for
}
pub enum NearMiss {
    OtherInstantiation(ImplRef),             // `Store[mut User]` exists; `Store[User]` was asked
    NotAvailable(DefId),                     // a trait with the method exists but is not in scope
    Derivable { trait_: DefId, target: DefId }, // the leaf could be derived
}
```

Because of rule TS-2, the failing path is unique: one chain from the
asked goal to one leaf. There is no "which of these alternatives did you
mean" tree to summarize.

### 10.2 Which Code

The checker picks the code from the site and the chain:

| Site or chain | Code |
| --- | --- |
| a bound of a call or a type, failing at the leaf | `unsatisfied-trait-bound` |
| the chain's last step is a bound that a derived head added | `missing-derived-bound` ([`trait.derive.bound-unmet`](../../spec/lang/09-traits.md#r-trait.derive.bound-unmet)) |
| a member obligation in a derive instance of `Eq`, `PartialOrd`, `Ord` or `Hash` | `derive-field-missing-trait`, at the field |
| a member obligation of any other template | `member-not-derivable`, at the opt-in, naming the member |
| an impl check of a supertrait or supertrait binding | `missing-supertrait-implementation` |
| a conversion to a trait value | `type-mismatch` |
| an operator with no fitting impl | `type-mismatch` naming the missing trait ([`expr.op.no-impl`](../../spec/lang/05-expressions.md#r-expr.op.no-impl)) |
| `Overflow` | `trait-resolution-depth` |

### 10.3 Messages And Fix-Its

**Name the root cause, then the path.** One line, then one hint:

```text
app.hd:12:9: unsatisfied-trait-bound: `Box[Handle]` does not implement `Eq`, because `Handle` does not
hint: add `@derive(Eq)` to `data Handle` (app.hd:3)
```

With `--verbose`, a note lists the chain: "`impl[T < Eq] Eq for Box[T]`
(derived, app.hd:5) requires `T < Eq`". The compact form stays within the
60-token target of the `mistakes` metric.

**Missing-derive fix-it.** When the leaf is `X: Tr`, `X` is a data type,
enum or newtype of the user's own package, not a GADT, and `Tr` has a
template, the fix-it inserts `@derive(Tr)` or extends an existing list. It
adds missing law partners, since `@derive(Hash)` alone is
`mixed-derived-law` ([Law Partners](../../spec/lang/09-traits.md#law-partners)),
and gives none when `X` has a hand-written partner impl.

| Near miss | Message |
| --- | --- |
| `OtherInstantiation` | "`Shelf` implements `Store[mut User]`, not `Store[User]`" ([`trait.target.inner-mut.distinct`](../../spec/lang/09-traits.md#r-trait.target.inner-mut.distinct)) |
| `NotMutable` | "`clear_value` needs a mutable `T`", with the `let mut` fix-it |
| `NotAvailable` | the `use` fix-it of [`trait.avail.suggest`](../../spec/lang/09-traits.md#r-trait.avail.suggest) |
| `Binding` | "`Money`'s `Add` gives `Out = i64`, but `Summable` requires `Out = Money`" |

**Overflow messages** show the first three goals of the chain and the
last, and name the impl that grows the goal when the chain repeats one.

### 10.4 One Diagnostic Per Root Cause

A failure's root key is the body plus the leaf goal's canonical key (mine).
Ten `==` uses on a `Point` without `Eq` in one body give one error at the
first use, with a note "and 9 more uses in this body". Different leaves
are different causes: `Box[Handle]: Eq` and `Handle: Hash` give two.
Section 16 question 4 asks whether the owner wants this, or one error per
use site.

A stalled goal still open at the end of the body becomes type-checking.md's
`cannot-infer-type`, naming the goal's trait: "cannot infer `U`: `Both`
implements `Source[i32]` and `Source[string]`".

## 11. Determinism And Parallelism

**Rule TS-8. Every order the solver shows comes from content.**

- Candidates, `Many` lists, `near` lists and coherence reports are ordered
  by each impl's content `rank`: its module's rank in stable-path order,
  then its source position. `rank` is computed when the interface is
  built, so sorting never compares paths at solve time, and never uses a
  `DefId`.
- Elaborated clauses are in declaration order, supertraits depth first in
  declared order. A `Bound` index is therefore the same on every run.
- Canonical placeholders are numbered by first occurrence in a fixed walk.
- The bound plan runs in its stored order, so the first failing step is
  fixed.

**Purity and sharing.** Answers are functions of their keys (rule TS-1),
and fuel is charged by intrinsic cost (rule TS-5), so a warm memo, a
shuffled check order or another thread count changes no answer and no
`fuel_used`. The only shared mutable state is the global memo, written
first-writer-wins, and the interners. Impl tables, the candidate directory
and environments are frozen and read through `&`. The global memo's 64
shards keep contention low; two threads that miss on one key both compute
it, which is rare and bounded by the thread count.

## 12. Data Structures

All tables are struct-of-arrays with `u32` indexes, in the style of
[§3.9.4](data-structures.md#394-tables-as-struct-of-arrays). Sizes are per
entry.

| Structure | Where | Size | Lifetime |
| --- | --- | --- | --- |
| `ImplTable` row | interface blob, per module | about 40 B: trait 4, def 4, head key 4, arg keys 8, head ref 4, plan 4, assoc 4, origin 2, params 1, rank 8 | the folder interface |
| bound-plan step | interface blob | 12 B plus its argument template | the folder interface |
| `arg_impls` row | interface blob, per folder | 16 B: trait, head key, module, row | the folder interface |
| `VtableShape` | trait record | 4 B per slot and per direct supertrait | the folder interface |
| `ParamEnv` clause | per item, interned | 16 B: self 4, trait 4, args 4, bindings 4; plus 1 bit `mut` and 2 B origin | process (interned by `EnvKey`) |
| canonical goal key | memo key arena | 20 B: kind and flags 4, trait 4, self 4, args 4, extra 4 | the memo |
| `MemoEntry` | memo | 12 B | global: process; body: the body |
| canonical answer | memo answer arena | 4 B tag plus evidence words plus 8 B per learned binding | the memo |
| `Frame` | solver stack | 24 B; at most about 64 goal frames plus their projection steps, a few KiB | one `solve` call |
| scratch (substitutions, seen children) | the worker's scratch buffer | 4 B per word | truncated at each pop |

A `solve` call allocates nothing per goal: frames go on a reused vector and
substitutions on the worker's scratch buffer
([§3.9.5](data-structures.md#395-building-scratch-buffer-checkpoints-truncation)).
A large body asks a few thousand distinct goals, under 100 KB of body
memo; the global memo for std and a 50,000-line package is a few
megabytes. The key and answer encodings come from one schema,
`hd_types/solve.ir`, like the pool's accessors.

## 13. Performance Targets And Pathological Cases

**Targets.**

| Measure | Target |
| --- | --- |
| memo hit, global or body | under 100 ns |
| a computed goal on the fast path (section 3.3) | under 1 µs |
| a computed goal with three bound steps | under 5 µs |
| solver share of check time on std and the generated packages | under 10% |
| `select` per instance call in codegen | under 200 ns, memoized |

**Pathological cases.** Each is in the `pathological` suite, well-typed
and ill-typed (section 14.2).

| Case | Danger | What bounds it | Expected cost |
| --- | --- | --- | --- |
| 1,600 `Cents::from(N)` over two `From` impls (F-626) | a search per call | one `Instantiations` entry, then hits; the checker's trials truncate | 1 computed goal, 1,599 hits; fuel about 5,000 steps; well under 100 ms in all |
| 500 `From[X]` impls for one error type | `Instantiations` returns 500 candidates | sub-buckets by argument key make `Implements` with a known argument a binary search; `Many` is computed once per body | the checker's trial count, not the solver, dominates; section 16.4 change 7 lets the checker prefilter by argument head |
| a supertrait chain 200 deep | elaboration per goal | elaboration once per item; clauses indexed by self type | 200 clauses per item; each goal one index probe |
| diamond supertraits (`Error`'s three supertraits, `Num`'s eight) | duplicate clauses | elaboration skips a trait reference already present | linear in distinct supertraits |
| an associated-type chain 60 deep | normalization blowup | one `Project` per level, memoized; depth counts | 60 goals; at 65, `trait-resolution-depth` |
| a recursive impl through a projection | a cycle | cycle detection on the stack, `Overflow` at the first repeat (rule TS-7) | the cycle's length |
| a goal whose trait arguments grow on each step | unbounded search | depth 64 first, type size limit second | at most 64 levels |
| `==` on a 1,000-element tuple | per-element search | the tuple template's plan: one step per element; distinct element types are memoized | linear in elements |
| `List` nested 64 deep, `Eq` | deep proof | depth limit | 64 goals; 65 deep is `trait-resolution-depth` |
| many bodies asking the same goals | repeated work | the global memo | each distinct goal computed about once per run |
| a method name declared by 50 available traits | 50 goals per call | the per-module method index; one `Instantiations` per trait | 50 memoized goals once per receiver type |

## 14. Testing

### 14.1 Spec Traceability

Each spec area maps to one solver component, a module of `hd_types::solve`
or a resolution check. A CI script reads the `trait.*` rule IDs and the
conformance index, and lists rules with no fixture, per component.

| Spec area | Component |
| --- | --- |
| [Supertraits](../../spec/lang/09-traits.md#supertraits), [Supertrait Bindings](../../spec/lang/09-traits.md#supertrait-bindings) | `env` (elaboration); impl checks |
| [Associated Types](../../spec/lang/09-traits.md#associated-types), [Associated Type Bindings](../../spec/lang/09-traits.md#associated-type-bindings) | `project` |
| [Implementation Targets](../../spec/lang/09-traits.md#implementation-targets), [Implementation Ownership](../../spec/lang/09-traits.md#implementation-ownership) | resolution header checks; `owners` |
| [Overlap](../../spec/lang/09-traits.md#overlap) | `Coherence(trait)`; `plan` (constrained parameters) |
| [Derived Implementations](../../spec/lang/09-traits.md#derived-implementations), [Templates](../../spec/lang/14-annotations.md#templates), [Derived Bounds](../../spec/lang/14-annotations.md#derived-bounds) | derived heads; derive-instance environment |
| [Method Resolution](../../spec/lang/09-traits.md#method-resolution), [Trait Availability](../../spec/lang/09-traits.md#trait-availability) | `methods` |
| [Instantiations Of One Generic Trait](../../spec/lang/09-traits.md#instantiations-of-one-generic-trait), [Inference Through A Bound](../../spec/lang/04-type-system.md#inference-through-a-bound) | `inst` |
| [Generic Bounds And Static Dispatch](../../spec/lang/09-traits.md#generic-bounds-and-static-dispatch), [Mutable Bounds](../../spec/lang/04-type-system.md#mutable-bounds) | `head`, `search`, `memo` (depth) |
| [Dynamic Trait Values](../../spec/lang/09-traits.md#dynamic-trait-values) | resolution (safety); `dynself`; vtable shapes |
| [Sealed Traits](../../spec/lang/09-traits.md#sealed-traits), [Runtime Type Identity](../../spec/lang/09-traits.md#runtime-type-identity), [`AnyVal` And `AnyRef`](../../spec/lang/04-type-system.md#anyval-and-anyref) | `builtin` |
| [Trait Delegation](../../spec/lang/09-traits.md#trait-delegation) | delegated heads; impl checks |
| [Error Derivation](../../spec/lang/14-annotations.md#error-derivation) | generated heads |
| [Tuple Templates](../../spec/lang/14-annotations.md#tuple-templates) | `builtin` (tuple-template plans) |

### 14.2 Test Kinds

| Test | What it checks |
| --- | --- |
| conformance fixtures (`spec/conformance`) | accept or reject and the code of each diagnostic, through the portable command contract; failures in the new compiler's `KNOWN_FAILURES.tsv` |
| differential against the frozen prototype | accept or reject and the set of codes per file, on the suite and on generated programs; known prototype bugs (F-617 to F-620, F-624) are expected disagreements, each a row in a triage list |
| solver unit snapshots | `hd debug solve` over small impl sets: goal in, answer, evidence, height and cost out. These are internal tests of the Rust crate, not conformance fixtures, so the fixture format stays implementation-neutral |
| impl-set fuzzer (mine) | random traits (zero to two parameters, zero or one associated type), random data types, random impl heads that obey ownership, random goals. The oracle is a naive solver: recursive, no memo, no index, no fast path, over every impl in the program. Answers and top-level evidence must agree on every goal that the oracle finishes within the depth limit |
| coherence fuzzer | random heads; the oracle enumerates ground types up to nesting 3 and reports two heads that both match one. Every oracle overlap must be reported, with a witness that matches both heads |
| memo invariance | the same goal sequence with a cold memo, a warm memo, a shuffled body order and 1, 4 and 16 threads: identical answers, identical `fuel_used` per body (rules TS-1, TS-5) |
| canonicalization property | alpha-renamed goals get one key; goals that differ only in a variable's kind get different keys |
| select soundness | in CI builds, every `select` at an instance re-solves the full goal and asserts `Holds` with the same impl (rule TS-6) |
| pathological suite | each case of section 13, well-typed and ill-typed, within its fuel and wall-time budget, with its limit diagnostic; each also at sizes `n` and `2n`, failing when the observed exponent exceeds 1.2 (section 7.6) |
| one-root-cause fuzzer | mutate one impl or bound of a well-typed program; more than two error diagnostics flags a cascade for review |

## 15. Prototype Failures And The Rules That Prevent Them

From [src/KNOWN_ISSUES.md](../../src/KNOWN_ISSUES.md), its history, the
[checker audit](../../audit/compiler/checker-2026-10-04.md) and the
[findings](../../audit/compiler/findings-2026-10-04.md):

| Prototype failure | Evidence | Rule here |
| --- | --- | --- |
| a call with several instantiations copies checker state per candidate: 1,600 `From` calls take 96 s | F-626 | TS-1 (the solver writes nothing); one memoized `Instantiations` per receiver; TS-5; the checker's trials truncate |
| `Trait::f()` for an associated function rejected instead of inferring `Self` | F-617 (fixed in the prototype) | an `Implements` goal with a placeholder self stalls, and the obligation resolves when `Self` is known; open at the end, `cannot-infer-type` ([`trait.assoc-call.trait.undetermined`](../../spec/lang/09-traits.md#r-trait.assoc-call.trait.undetermined)) |
| a trait value of a generic trait does not satisfy the instantiated bound; only argument-free bounds were forwarded | F-618 | the trait-value source compares the full trait reference (section 9.3) |
| `T < mut Any` drops the `Any` bound and its `mut` | F-619 | `mut_` on every goal; `Any` is an ordinary sealed row, never dropped |
| `TypeId` spelled by bare name; two same-named types of different modules could share an identity | F-620 | `Inspectable` evidence names the type, and codegen derives identity from the canonical type over stable paths ([§13.3](codegen.md#133-instance-keys)) |
| `Trait::method(item)` with a parameter receiver searched impls only | F-624 | one goal for every source, the environment first (section 3.1) |
| impl matching on canonical type strings | [CA-02](../../audit/compiler/findings-2026-10-04.md#ca-02-textual-types-and-semantic-cycles) | heads in the pool; the head index; TC-2 |
| dictionary plans in HIR, an erased-dictionary ABI shared with the emitter | [CA-04](../../audit/compiler/findings-2026-10-04.md#ca-04-cross-stage-representation-and-distributed-abi) | TIR records only the top choice; codegen selects by head (TS-6); dictionaries only for trait values, GADT evidence and `CallDyn` bounds |
| derivation diagnostics in process-global registries keyed by span | [CS-02](../../audit/compiler/findings-2026-10-04.md#cs-02-derivation-diagnostic-registries-reuse-keys-without-compilation-identity) | `FailInfo` carries the impl's origin; diagnostics are values in the body result |
| derives through generated source that is parsed again | checker audit, Derivation And Std Integration | the solver sees derived heads only; template bodies are token text with a resolution table, checked as derive instances |
| bound inference repeats until no solution changes | checker audit, `bound-inference.ts` | the spec's bounded loop stays in the checker; the solver has no fixpoint (TS-7) |
| associated calls and member calls use separate selection paths | checker audit | one `Methods` goal for both; one `Instantiations` goal for the choice |

## 16. Open Questions, Readings And Changes

### 16.1 Questions For The Owner

1. **Which impls does an instantiation set count?** `Instantiations` and
   bound-only inference ask for "every instantiation" a type implements.
   An impl owned through a trait argument may live in a package that the
   asking module does not depend on (section 3.2). If it counts, adding a
   package to the program can make a call in an unrelated module
   ambiguous, and the module's cache key would have to cover every
   package. **Recommendation:** count only impls declared in the asking
   module's dependency closure, as Rust's separate compilation does in
   effect. Add one sentence to
   [Instantiations Of One Generic Trait](../../spec/lang/09-traits.md#instantiations-of-one-generic-trait)
   and to [Inference Through A Bound](../../spec/lang/04-type-system.md#inference-through-a-bound).
   Coherence still checks the whole graph, so no two impls can disagree.
2. **Projections of impl parameters in impl heads.** The spec neither
   allows nor forbids `impl[I < Store] Summary for Feed[I::Item]`. Such a
   head cannot be matched or checked for overlap from heads alone.
   **Recommendation:** forbid it. Say in
   [`trait.overlap.constrained-head`](../../spec/lang/09-traits.md#r-trait.overlap.constrained-head)
   that a parameter that appears only inside a projection is not
   constrained, so the existing `unconstrained-impl-parameter` applies. No
   new code.
3. **What counts as one level of proof depth?** The spec defines depth by
   impl bounds only. **Recommendation:** count a bound-plan step, a tuple
   template's element obligation, one `Inspectable` step through a type
   argument and one projection step as one level each; count environment
   clauses, trait-value sources and sealed memberships as no level
   (section 7.3). Add this as a rule under
   [Generic Bounds And Static Dispatch](../../spec/lang/09-traits.md#generic-bounds-and-static-dispatch),
   so every implementation agrees on which programs pass.
4. **One error per missing impl per body, or per use?** Ten `==` on a type
   without `Eq` in one body can give ten errors or one with "and 9 more
   uses". **Recommendation:** one per body and leaf goal (section 10.4).
   The `mistakes` metric counts diagnostics per mistake, and the fix is
   usually one `@derive` line.

### 16.2 Readings Of The Spec To Confirm

These follow from the spec as written; no answer is needed unless the
owner disagrees.

1. **`Implements` never learns from a unique head match** (rule TS-3). Only
   associated-type bindings teach a variable, and only `Instantiations`
   chooses among instantiations, where the spec says so.
2. **A cycle reports `trait-resolution-depth` at once.** An inductive
   cycle has no finite proof, so the spec's depth rule already rejects it.
3. **No depth limit at instances.** Codegen's concrete proofs are not
   source uses (section 8.3).
4. **`Structure` holds only through a template instance's environment**,
   never as a written or compiler-wide impl
   ([`annot.structure.generated`](../../spec/lang/14-annotations.md#r-annot.structure.generated)).

### 16.3 Inconsistencies Found

1. **Tuple impls: compiler-derived or templates?** Chapter 12's table says
   tuple equality, ordering and hashing are "derived by the compiler for
   every arity". Chapter 9 and chapter 14 say every tuple trait comes from
   a std tuple template. type-checking.md §6.2 answers them with
   `Evidence::Builtin`, and codegen §13.6 generates them per arity. This
   design follows chapters 9 and 14: they are `Impl` evidence of the
   template. Chapter 12's table should link to tuple templates.
2. **Which std module owns a primitive?** Trait impls must live in a module
   that declares the trait, the target or a trait argument
   ([`trait.own.module.trait`](../../spec/lang/09-traits.md#r-trait.own.module.trait)).
   For `impl Display for i32` in std, no std module "declares" `i32`. D1
   maps built-in owners to "fixed std modules". This design reads it as:
   any std module may hold impls whose target is a std-owned built-in
   constructor, and std's interface gathers them into one synthetic owner
   table. The spec should say so, as
   [`trait.own.module.inherent.std`](../../spec/lang/09-traits.md#r-trait.own.module.inherent.std)
   does for inherent impls.
3. **D1 §4.12.1's "at most `2 + len(Args)` modules"** holds only when the
   trait arguments are known. Open arguments need the candidate directory
   (section 3.2).
4. **D1 §4.12.2's cycle rule** ("a goal already on the stack succeeds only
   inside a derived impl's member check") and type-checking.md's
   `Coinductive` evidence describe a mechanism that is not needed: the
   derived head is in the table, so the member check never meets a cycle
   (section 3.10). Every cycle that the search meets is `Overflow`.
5. **Smaller gaps**, each fixed by a change in section 16.4: D2's
   "dictionaries only for trait values and GADT evidence" misses `CallDyn`
   bound evidence (change 19); type-checking.md's goals have no `mut`, the
   prototype's F-619 (change 1); D2's collection re-solves where a head
   match suffices (change 18).

### 16.4 Changes Needed In type-checking.md And The Other Design Files

For the owners of those files to make. This document edits none of them.

**type-checking.md**

1. **§1.6 `Goal`:** add `mut_: bool` to `Implements` and `Instantiations`;
   wrap the self type and arguments in a `TraitRef`; `Project` names the
   associated item's `DefId` (section 2.1).
2. **§1.6 memo:** the keys and eligibility of section 7.1: the `Env`
   scope, `LocalVis`, `AvailKey`, a global memo per run, GADT arm
   equalities substituted before keying, and no global entry for a goal
   that met the depth cut or ran out of fuel.
3. **§1.6 fuel and §11.1:** a goal charges its intrinsic cost the first
   time a body meets it, and 1 on each repeat (rule TS-5).
4. **§1.6 `Answer` and `Evidence`:** add `Normalized { ty, evidence,
   learned }` for `Project`; add `TraitValue { trait_ }`; drop
   `Coinductive`; a `Bound` index is a clause index in the elaborated
   environment.
5. **§1.6 `Solver`:** add `elaborate`, `select` and `normalize_concrete`
   (section 1.3). The solver tracks depth itself, by heights.
6. **§6.2:** tuple `Eq`, `Ord`, `Hash` and `Debug` are `Impl` evidence of
   std's tuple templates, not `Builtin`.
7. **§2.5 instantiation choice:** before trials, drop candidates whose
   parameter's head constructor cannot match an argument whose type is
   already known.
8. **§2.5 trial memo (mine):** memoize each call site's trial outcomes by
   the canonical expected type and argument types, so nested chains of
   expected-type-dependent calls cost sites × candidates (section 7.6).
9. **§2.5 fits (review T4):** a candidate fits only if, when the call has
   an expected type, the method's result is assignable to it
   ([`trait.resolve.fits.expected`](../../spec/lang/09-traits.md#r-trait.resolve.fits.expected)).
   Each trial runs `coerce(result, want)` inside its rollback before the
   fits are counted, so `let n: i32 = money.pick()` picks `Pick[i32]`.
10. **§2.5 `Methods`:** the answer also carries the unavailable traits
    that would match, for the `use` fix-it.
11. **§2.7 obligations:** keep a projection-equality obligation instead of
    binding a variable to a projection over itself (section 4.3); queue
    each obligation once per wake round.

**resolution-and-interfaces.md**

12. **§4.12.1:** add the `ImplTable` columns of section 3.3, the
    `arg_impls` interface section, the per-run candidate directory, and
    std's synthetic table for built-in targets, inherent impls included.
13. **§4.10 step 5:** compute each impl's bound plan; reject projections
    of impl parameters in heads (question 2); store each trait's
    dynamic-safety flag and vtable shape; list tuple templates in the
    `heads` section with head key `TupleAny`.
14. **§4.12.3:** replace pairwise bucket unification with the hash set and
    discrimination tree of section 5.2, and report a witness type.

**data-structures.md**

15. **§3.4 and §3.9.2:** the projection type is `Assoc { assoc: DefId,
    tref }`, with the trait's arguments, not `{ base, trait_, name }`.

**checking-and-tir.md**

16. **§4.13.11 catalog:** the `TraitMethod` choice gains `TraitValue` and
    `Builtin`; `CallDyn` gains one evidence operand per method-level
    bound; `NewVariant`'s evidence choices are `Evidence` values.
17. **§4.13.9:** a derive instance needs no coinductive assumption
    (section 3.10). Add an impl-check body task per impl in M2:
    supertraits, supertrait bindings, delegation parts and newtype bases.

**codegen.md**

18. **§13.2 step 4:** select by head only, with no depth limit and no
    fuel; a failure is an internal error (rule TS-6).
19. **§13.5:** vtables follow the trait record's shape, with supertrait
    vtables by pointer; a slot for a method with method-level parameters
    is the erased instance with one vtable parameter per bound (section
    9.2).
20. **§13.6:** tuple `Eq`, `Ord`, `Hash` and `Debug` are tuple-template
    instances, instantiated per tuple type like any impl.

**cache.md**

21. **The `check` key** gains `arg_impls_closure_hash`: a Merkle hash over
    the `arg_impls` section hashes of every folder in the module's
    dependency closure, computed bottom-up beside the deep hashes (section
    3.2). The solver memo is never persisted and never part of a key.

### 16.5 Codex Review

The Codex review of commit 8bb6860d (`codex_review.md`, not committed)
predates this document. Each finding below was checked against the spec
before it was accepted.

| Finding | Verdict | Where |
| --- | --- | --- |
| Blocker 3: owner lookup cannot enumerate unknown trait arguments; std's inherent exception | **Accepted, fixed.** Verified: [`trait.own.argument`](../../spec/lang/09-traits.md#r-trait.own.argument) allows argument-owned impls, and [`trait.own.module.inherent.std`](../../spec/lang/09-traits.md#r-trait.own.module.inherent.std) allows any std module. Taken: the per-run, per-trait candidate directory and a coarse hash in the check key. Changed: the directory is filtered by the asking module's dependency closure, so a downstream package cannot change a library's result (question 1) | sections 3.2, 5.3; changes 12, 21 |
| Blocker 5: dynamically safe generic methods lack a codegen strategy | **Accepted, fixed** for the solver's part. Verified against [`types.trait.safe.one-body`](../../spec/lang/04-type-system.md#r-types.trait.safe.one-body): an erased slot instance with one vtable parameter per bound | section 9.2; changes 16, 19 |
| Blocker 8: memo entries are not functions of their keys | **Accepted, fixed.** Keys now hold the environment, visible local impls, availability, and a per-run memo; depth is a stored height with lower-bound entries; fuel and depth exhaustion are never cached as failures; completion before publication is the SCC rule. Not needed: a key for coinductive assumptions, since none exist | sections 6.3, 7.1, 7.3; change 2 |
| T1: projections cannot represent their inputs or return outputs | **Accepted, fixed.** Verified: D1's `Assoc` drops the trait arguments, and §1.6's answers had no normalized type. Projections now name the associated item and the instantiated trait reference; `Project` answers `Normalized`; normalization cycles, the occurs check and aliases are specified | sections 2.1, 4.3; changes 1, 4, 11, 15 |
| T4: candidate trials omit expected-result filtering | **Accepted.** Verified: [`trait.resolve.fits.expected`](../../spec/lang/09-traits.md#r-trait.resolve.fits.expected). The trial is the checker's; the change is stated for type-checking.md | change 9 |
| T6: fuel bounds work but does not make it near-linear | **Accepted, fixed** for the solver and coherence: the algorithms of section 7.6, the discrimination-tree overlap check, and the doubling test. The trial multiplication and obligation waking belong to the checker; changes 8 and 11 state them | sections 5.2, 7.6, 14.2; changes 7, 8, 11, 14 |
| A5: synthetic impls need dependency and coherence rules | **Accepted, fixed.** Verified: any trait may have a tuple template ([`annot.template.tuple.form`](../../spec/lang/14-annotations.md#r-annot.template.tuple.form)), and a written tuple impl beside it is an overlap ([`annot.template.tuple.overlap`](../../spec/lang/14-annotations.md#r-annot.template.tuple.overlap)) | section 3.11; change 13 |
