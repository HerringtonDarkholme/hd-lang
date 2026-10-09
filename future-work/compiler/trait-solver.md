# New Compiler: The Trait Solver

Status: Design, not decided. Frontend lane, 2026-10-07. Revised the same
day with the owner's answers to section 16.1 and the Codex review of
8bb6860d.
Revised again for the Codex re-review through 54f249b7
([codex-rereview-response-frontend.md](codex-rereview-response-frontend.md)),
the removal of GADTs and the `dyn` decisions.

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

The solver lives in `hd_types`; other crates call its interface rather
than owning a second solver
([reconciliation, item 12](reconciliation.md#design-changes-proposed)).

| Component | Owns | Does not own |
| --- | --- | --- |
| Resolution (`hd_resolve`) | impl heads, bound plans and derived heads in the interface; every header check that needs no solver: orphan, module ownership, targets, unconstrained parameters, sealed traits, per-member `dyn` availability, template placement | goals |
| **Trait solver (`hd_types::solve`, this document)** | the four goals; parameter-environment elaboration; normalization of projections; canonical goals and the memo; impl selection for codegen; `FailInfo` | inference variables (it reads them, never writes them), diagnostics text, spans |
| Impl checks (`HeaderCheck(F)`, one task per folder, before bodies need it) | each impl's supertrait and binding checks, newtype bases, delegation targets: goals under the impl's environment, asked through the solver ([resolution-and-interfaces.md §4.10.1](resolution-and-interfaces.md#4101-header-validation-stages)) | the answers |
| Derive instances (body tasks in M2) | derive member obligations, checked with the instance body | the answers |
| Type checker (`hd_check`) | when to ask, applying learned bindings, obligations, the instantiation choice by trial, every diagnostic | impl search |
| Coherence (`Coherence(trait)` task) | overlap across the program graph, per trait, from heads | goals |
| Codegen collection (D2) | instances and vtables | anything but a head match at concrete types (rule TS-6) |

**Rule TS-1. The solver is a pure function of its memo key.** An answer
depends only on the canonical goal, the parameter environment, the local
impls visible at the asking point, the trait list (for `Methods`),
the impl universe of the asking context (for `Instantiations`, `Methods`,
and any goal with an open argument, section 3.2), and the run's frozen interfaces. Section 7.1 makes each of these part of
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
| `Instantiations` | parameters named only in bounds; the instantiation choice of a method or operator; `X::from(v)` | `AppError::from(code)` asks `Instantiations { AppError, From }` | `Many([From[i32], From[string]])` in content order |
| `Methods` | `value.name(...)`, `Type::name(...)` | `w.show()` on a `Wrapper` with an inherent `show` and an available `Show` | `Many([Inherent(show), Trait(Show::show)])`; the checker takes the inherent one |

The answer shapes:

| Answer | Meaning | Checker action |
| --- | --- | --- |
| `Holds { evidence, learned }` | proven; `learned` binds inference variables that the proof forced (section 3.8) | apply `learned` on its own trail; record `evidence` in TIR |
| `Normalized { ty, evidence, learned }` | `Project` only: the projection's normal form, over the caller's variables | unify `ty` with the other side |
| `Many(candidates)` | one or more candidates, in content order (`Instantiations`, `Methods` only); an `Instantiations` candidate is a scheme with residual bounds (section 6.5) | the instantiation choice or method rules |
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
    /// A head match, then the plan's `Bind` steps to fix every impl argument;
    /// no proof, no depth limit, no fuel (rule TS-6, section 8.3).
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
  Header lowering fills every written trait reference once, so a
  projection written under a bound takes the bound's filled arguments:
  `T::Out` under `T < Add` is `<T as Add[T]>::Out`. `Self::X` inside a
  trait keeps no filled arguments; each use fills its own.

### 2.2 Canonical Goals

A goal is canonicalized before any memo lookup:

1. **Resolve shallowly.** Each inference variable is replaced by its
   binding, recursively, through the checker's `InferRead`.
2. **Normalize concrete projections.** A projection whose base is known is
   replaced by its normal form (section 4.3). Projections on parameters
   stay as rigid types.
3. **Number the variables.** The remaining inference variables are
   replaced by canonical placeholders `Canon(0)`, `Canon(1)`, ... in order
   of first occurrence in a fixed pre-order walk: self type first, then
   trait arguments left to right, then bindings in name order. Each
   placeholder keeps its variable's kind: general, integer literal or float
   literal.
4. **Pick the key's scope.** See the table below. (An earlier step that
   applied GADT arm equalities is removed with GADTs.)

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
| `Global` | no placeholder, no parameter, no local type | the run's global memo, shared by every body and by codegen | the canonical goal |
| `Env` (mine) | parameters, but no placeholder, no local type | the run's global memo | the canonical goal and the item's `EnvKey` |
| `Body` | any placeholder or local type | the body's memo | the canonical goal and the visible local impls |

An `Instantiations` goal adds the context's `ImplUniverseId` in every
scope (section 3.2), as does a `Methods` goal. A `Methods` goal is never
memoized: it carries the trait list from the method index instead of an
availability key, and no availability key exists yet.

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
    clause_self: Box<[Ty]>,      // a Param
    clause_trait: Box<[DefId]>,
    clause_args: Box<[TyList]>,
    clause_bindings: Box<[AssocList]>,
    clause_mut: BitBox,          // the bound was written `mut`
    clause_origin: Box<[u16]>,   // declared bound index, or the supertrait path's first step
    by_self: Box<[(Ty, u32, u32)]>,  // sorted index: self type -> clause range
}
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
| 1 | the parameter environment (section 4.2) | `S` is a parameter |
| 2 | a trait value as self (section 9.3) | `S` is a trait value type of `Tr` or of a subtrait |
| 3 | compiler-supplied impls (section 3.9) | `Tr` is sealed and compiler-supplied: `Any`, `AnyVal`, `AnyRef`, `Inspectable`, `Tuple`, `Suspend`, `Structure` (std writes `Num`, `Integer` and `Float`) |
| 4 | impl tables of the owner folders (section 3.2) | written impls, derived impls, delegations, numeric families, tuple templates |
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

### 3.2 Owner Folders

An impl `impl Tr[Args] for Target` lives in a folder that declares `Tr`,
the outer constructor of `Target`, or the outer constructor of one of
`Args`
([Implementation Modules](../../spec/lang/09-traits.md#implementation-modules)).
Tables are per folder, not per module: each folder's interface holds one
`ImplTable`, and lookup reads the tables of the owner folders
([§4.12.1](resolution-and-interfaces.md#4121-owner-module-impl-tables-mine)).

```rust
fn owner_folders(g: &TraitRef) -> Folders {
    // the folders declaring g.trait_, the head constructor of the self
    // type, and the head constructor of each known trait argument
}
```

**Unowned rows.** std's impls for built-in targets (primitives, `List`,
`Map`, tuples, `Fn`, `Option`, `Result`) live in the synthetic
`STD_BUILTIN_TABLE`, which is an "unowned" table: it also holds the impls
the ownership check rejected as `nonlocal-impl` or `orphan-impl`.
Rejected impls are already one error each; they stay in the table so the
rest of the pipeline sees a stable row, but coherence skips them and the
overlap check never reports them twice.

**The gap: impls owned through a trait argument.** When an argument is
still an inference variable, its owner folder is unknown. `Money:
Add[?0]` may be answered by `impl Add[Cents] for Money` in the folder that
declares `Cents`
([`trait.own.argument`](../../spec/lang/09-traits.md#r-trait.own.argument)).
`Instantiations` and the trait part of `Methods` are the same case with
every argument open. The `2 + len(Args)` bound covers only goals whose
arguments are known. The Codex review found the same gap (blocker 3).

The fix is a **candidate directory**, after the review's proposal:

1. Resolution flags each impl that its folder owns only through a trait
   argument. The interface lists those heads in an `arg_impls` section,
   sorted by `(trait, target head key)`. There is no section hash:
   nothing persists the directory, so nothing hashes it.
2. Once per impl universe, the driver merges the `arg_impls` sections of
   that universe's folders into a **directory**: per trait, rows of
   `(target head key, folder, impl row)`, in content order. Merging takes
   no barrier, and scheduler.md §6.1's "universes add no edge" holds:
   every folder in a context's closure is already a dependency of its
   task through the `FolderIface` chain.
3. A goal with an open argument reads the trait's directory rows for its
   target head key. Lookup itself takes no memo: `candidates` is a pure
   function of the tables, the directory and the goal.
4. **Cache key (incremental soundness).** An argument-owned impl's trait
   and target are nameable outside its folder, so its head is in its
   folder's `api_hash` and therefore in the deep hash
   ([resolution-and-interfaces.md §4.10](resolution-and-interfaces.md#410-folder-interface-construction)).
   A folder's `check` key already holds the deep hash of every folder in
   its closure, so adding `impl Pick[Product] for Receiver` in a third
   folder rechecks every folder whose closure holds it. No separate
   `arg_impls` hash is needed (Codex re-review N-A1; the backend lane
   removes `argc` from cache.md).

**The impl universe (Codex re-review N1).** The directory makes an
answer depend on which folders the asking context sees. Two folders may
both ask `Instantiations { Receiver, Pick }` while only one's universe
holds `impl Pick[Product] for Receiver`. The goal has no placeholder, so
without more it would be one global memo entry, and whichever folder
asked first would decide the other's answer. So:

- Every solving context carries an **`ImplUniverseId`** in `SolveCx`: the
  interned, sorted list of the folders in its closure that have
  `arg_impls` or unowned rows. A folder with neither joins no universe.
  The driver computes it once per context. For a folder's bodies and its
  test overlay, that is after M1 builds the closure bit set.
- The contexts are a folder's bodies, the folder's test overlay (its
  closure includes test-only dependencies), a folder's header checks
  (the folder's closure universe, with no own table), a program build
  (every folder, section 8.3), and a derive instance (its folder's).
  Contexts with equal lists share one id, so most folders of a package
  share one.
- **The id is part of the key of every goal with an open argument**, in
  every scope: `Instantiations` and `Methods` always, and an
  `Implements` or `Project` goal whose argument is open (`Infer`,
  `Canon` or `Poison`). The checker phrases instantiation and
  method-trait questions with fresh variables, so they land here.
- An `Implements` or `Project` goal in `Global` or `Env` scope with all
  arguments known never reads the directory: its owner folders are
  fixed, and the folder that declares a known type lies in the closure
  of every folder that can name that type. Its bound-plan steps are
  `Implements` and `Project` goals with known arguments too — except a
  `Bind` step whose bound leaves trait arguments implicit
  (`n < arity`): that step reads the directory after all.
- **No `HeaderCheck` goal kind exists.** Header checks are not solver
  goals and carry no universe in any key. They run a separate
  `TableSolver` from `hd_check::header`, under the item's environment
  with the folder's closure universe and no own table (section 3.7).
- **Debug check.** Each frame keeps a "read the directory" bit and a
  "read unowned rows" bit (section 7.1). A frame whose key has no
  universe and sets the directory bit is an internal error.

The memo-invariance test asks the two-folder pair in both orders
and on several threads, with equal trait availability and different
universes.

Why the closure, not the whole graph: a library's check result must not
depend on which downstream packages a program adds. A downstream package
may write `impl Pick[Product] for Receiver` for a library's `Receiver`,
and the library, checked alone or inside that program, must see the same
instantiations.

### 3.3 The Head Index

```rust
pub struct ImplTable {                     // per folder, SoA, frozen with its interface
    // D1's columns
    pub trait_: Box<[DefId]>, pub def: Box<[DefId]>, pub head_key: Box<[HeadKey]>,
    // added by this design
    pub arg_key: Box<[[HeadKey; 2]]>,      // head keys of the first two trait arguments; ANY when generic
    pub n_params: Box<[u8]>,
    pub head: Box<[HeadRef]>,              // target and trait arguments, as interface type records
    pub plan: Box<[PlanRange]>,            // the bound plan (section 3.6)
    pub assoc: Box<[AssocRange]>,          // associated-type bindings, by associated item DefId
    pub origin: Box<[ImplOrigin]>,         // Written | Derived { template } | Delegated { field } | Error | NumericFamily | TupleTemplate
    pub rank: Box<[u64]>,                  // content rank: (module path rank, item index in source order); the sort key
    pub by_trait: HashMap<DefId, (u32, u32)>,  // an index into the sorted rows
}
pub enum HeadKey { Ctor(DefId), Prim(Prim), Tuple(u16), TupleAny, Fn, SuspendFn, Param, Any }
```

Rows are sorted by `(trait rank, head key, rank)`. The head index is a
per-trait permutation over that order: it sorts each trait's row range
once, and a probe walks the bucket in row order. An impl that appears in
two tables is one head (rule TS-2): lookup dedups rows by impl `DefId`
before matching. A probe for `S: Tr[A..]` in one folder is:

1. `by_trait[Tr]` gives the trait's rows.
2. Binary search for `head_key(S)`; add the `Param` rows (numeric
   families) and, for a tuple, the `TupleAny` rows (tuple templates).
3. Match the survivors' heads (section 3.4).

The `arg_key` fast reject is written but not read: resolution fills the
first two argument keys per row, but nothing skips on them — rustc's
`DeepRejectCtxt` cut to two keys is not done.

**Fast path (mine, after MoonBit).** A non-generic trait (`Display`, `Eq`,
`Hash`, `Debug`) with a known self constructor usually has one row per
head key in all owner folders together. It may have more: `Tr for
Box[i32]` and `Tr for Box[string]` share the head key `Box`. So the probe
is one binary search per owner folder, and then a match of every row in
the bucket (Codex re-review N-T6). The global memo makes the second probe
free. In
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
- **Numeric families match by trait list.** A `Param`-keyed head matches
  a primitive only when the impl bound's family trait lists that type
  (`family_excludes`): `impl[N < Num] Add for N` does not match `string`.
  A parameter matches through its environment instead (section 3.9).
- **Placeholders never learn, and carry no literal kind.** A placeholder
  against a constructor or a bound parameter is `Maybe` even when exactly
  one head matches: no unique head ever binds a placeholder (rule TS-3).
  The `Maybe` carries no `VarKind` split and there is no `Canon` arm, so
  literal-kind-specific matching is not done.
- **Cost.** Linear in the head's size. Heads are small, so matching is a
  few dozen steps at most.

### 3.5 Committing

After the probe:

| Matches | Answer |
| --- | --- |
| no `Yes`, no `Maybe` | `Fails`: no impl |
| exactly one `Yes`, no `Maybe` | commit: solve its bound plan |
| any `Maybe` | `Stalled` on the placeholders |
| two `Yes` | only possible with an overlap error elsewhere: take the first in content order and continue. Coherence reports the overlap (section 5.4); the solver reports nothing |

- **A failing bound fails with its own failure.** Under a committed head
  there is no second route, so a bound that fails answers with that
  bound's `FailInfo`, with this impl and step prepended to its `chain`
  (section 10.1).
- **Exact exhaustion (M1 finding 4).** When every candidate head is exact
  and none matches, the answer is `Fails(FailInfo { reason: NoImpl,
  ... })`; it does not stall and is not an internal error.

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
- **A `Bind` step can read the directory.** Most steps are `Implements`
  goals with known arguments, but a `Bind` whose bound leaves trait
  arguments implicit (`n < arity`) reads the candidate directory like an
  open-argument goal (section 3.2).
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
That check runs in the folder's header stage, from headers and
impl tables only, so a dependency's impls are checked even when its bodies
are not (review T7). It lives in `hd_check::header`: each item asks its
supertrait `Implements` goals through `TableSolver` under the item's
environment, with the folder's closure universe and no own table, and
resolution keeps only the overlap check. The findings are cached in one
`graph` entry per folder keyed by `hdr_key` (#95). Not done: supertrait
bindings are not asked as `Project` (the solver has no `Project` path;
the checker normalizes); there is no fuel diagnostic per item (§4.10.1).

### 3.8 What A Goal May Teach The Checker

**Rule TS-3. `Implements` learns only through associated-type bindings.**
`I: Supplier[Item = ?0]` holds with `learned = [?0 := string]` when `I`'s
impl binds `Item = string`
([`trait.binding.inference`](../../spec/lang/09-traits.md#r-trait.binding.inference)).
A unique head match never teaches anything. `Money: Add[?0]` with one
impl `Add[Money] for Money` stalls; it does not learn `?0 := Money`.
Choosing among instantiations is the job of `Instantiations`, which the
checker asks exactly where the spec says: parameters named only in
bounds
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
| `AnyVal` | `bool`, `char`, integers, floats, `string`, tuples, enums (optionals included), function types; a newtype over one of them ([`types.sealed.anyval-values`](../../spec/lang/04-type-system.md#r-types.sealed.anyval-values)) | the newtype's base | `Builtin(AnyVal)` |
| `AnyRef` | data, `List`, `Map`, trait values, `Any`, suspensions, handles; a newtype over one | the newtype's base | `Builtin(AnyRef)` |
| `Inspectable` | [inspectable types](../../spec/lang/09-traits.md#inspectable-types): primitives, `string`, module-level declarations with inspectable arguments, `List`, `Map`, tuples, `Inspectable` trait values; trait values and `Any` as arguments only | one per type argument, each one level deeper | `Builtin(Inspectable)`; codegen derives the `TypeId` from the type |
| `Tuple` | every tuple type | none | `Builtin(Tuple)` |
| `Num`, `Integer`, `Float` | the fixed primitive lists of [Numeric Traits](../../spec/lang/09-traits.md#numeric-traits): std writes one impl per type (`lib/std/num.hd`, with the bodies of `zero`, `one`, `from_i64`), so the impl tables answer them | the impl's | `Impl` |
| `Suspend[T]` | compiler frames and the `std.task` types | none | `Builtin(Suspend)`; no row while std declares no such trait |
| `Structure` | the target of a template instance, through that instance's environment; and every concrete tuple type (#126), since the compiler generates a tuple's `Structure` while it instantiates a tuple template (`annot.tuple.structure`) | none | `Bound` in a template; `Builtin(Structure)` at a tuple, lowered per tuple type (codegen.md §13.6) |

For a parameter or a rigid projection, every row above but `Any`
answers through the environment only
([`types.sealed.type-parameter`](../../spec/lang/04-type-system.md#r-types.sealed.type-parameter));
every type parameter is `Any`. `never` implements neither `AnyVal` nor
`AnyRef`, and is not inspectable. A row on an inference variable stalls,
except `Any`, which holds.

**Where the rows live (#121).** `hd_types::sealed::row` is the table: one
`match` over the sealed trait and the type's form. `Search::implements`
asks it as source 3, after the environment and the trait-value rows of
section 9.3 (source 2), so body checks, header checks and codegen's
`select` all get its answer; the checker has no answerer of its own
(`builtin_holds` is gone, and `is` asks `AnyRef`). A row's subgoals are
`Implements` goals on the same trait one level deeper (a newtype's base;
an inspectable type's arguments, where a trait value counts without a
subgoal), asked and memoized like a committed impl's bound plan. The
rows read no impl table, so their memo entries have no probes and need
no universe; they read declarations through `SolveCx::decls` (a
`Declarations`: the sealed traits' ids, a type's declaration kind, a
trait's supertraits), which are the same in every context of a run.
Codegen's `select` returns `Selection::Builtin` for them.

Not done: no `Suspend` row (std declares no such trait); block-local
declarations, which `trait.inspectable.not.local` excludes, do not exist
in the compiler yet; `Inspectable`'s compiler-supplied `runtime_type`
body is a panicking vtable slot (codegen.md §13.6).

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
  resolution table. Each helper's signature is written in full, so a
  dependent reads it from the interface, never from a body
  ([type-checking.md §9.1](type-checking.md#91-who-can-omit), an open
  owner question). Trait goals inside the instance run under the
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
3. **One clause per trait reference, with merged bindings (Codex
   re-review N-T3).** Clauses are keyed by their `TraitRef`; bindings sit
   beside it, so a reference met again may bring bindings the first
   visit lacked. A diamond may reach `Supplier` once unbound and once
   with `Item = i32`. On a second visit the walk merges binding by
   binding: a binding not yet present is added, an equal one is a no-op,
   and a different one is a conflict. The walk does not descend again:
   the reference's supertraits were walked on the first visit, and a
   supertrait binding that names this reference's projection is stored
   as that projection, which normalization resolves through the merged
   binding later (section 4.3). The result does not depend on the order
   in which paths are met. The supertrait graph is acyclic
   (`supertrait-cycle` is a header error), so the walk ends.
4. **A conflict is a header error.** Two different bound types for one
   projection make the bounds unsatisfiable, and inside the body the two
   types would be equal. Resolution makes the same walk before any body
   and reports `duplicate-associated-binding` on the parameter's bound
   list (needs owner: the spec's
   [`trait.binding.once`](../../spec/lang/09-traits.md#r-trait.binding.once)
   covers only written bindings).
5. Intern the clause list; its content hash is the `EnvKey`.

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
| a parameter with an environment binding for `Name` | the bound type: `I::Item` is `T` under `I < Supplier[Item = T]` |
| a parameter with a bound on `Tr` but no binding | the projection itself, as a rigid type. It equals only itself |
| a trait value `Tr[A, Name = U]` or a subtrait's value that binds it | `U` ([`trait.dyn.bound.projection`](../../spec/lang/09-traits.md#r-trait.dyn.bound.projection)) |
| known | prove `Implements { tref }` first, as its own memoized goal at the same depth. On `Holds` with `Impl` evidence, read that impl's binding for `Name`, substitute the impl's arguments, and normalize the result once more |
| known, but `Implements` fails | `Fails`, with the `Implements` goal's `FailInfo`: `Box[NoDisplay]::Item` is no type when the impl for `Box[T]` needs `T < Display` |
| known, but `Implements` stalls or overflows | the same answer |

**Normalization proves applicability (Codex re-review N-T1).** A head
match alone does not make an impl apply: its bounds must hold too. So a
source-level `Project` on a known base always rests on a proof of the
`Implements` goal. That goal is memoized, and most projections meet a
goal that a use has already asked, so the extra cost is one memo hit.
Rigid projections under a declared bound are unchanged. Codegen's
`normalize_concrete` is a separate mode: at an instance the checker has
already proven the goal, so it reads the binding after a head match and
keeps its answers apart from the proof memo (section 8.3).

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
The owner decided (2026-10-07) to forbid it: resolution reports the head
as `unconstrained-impl-parameter`, since `I` appears only under a
projection. A parameter that appears only inside a projection is not
constrained. No new code.

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
| supertraits and supertrait bindings of an impl | the solver, under the impl's environment | `HeaderCheck(F)` | `missing-supertrait-implementation` |
| overlap between impls | heads of one trait across the program graph | `Coherence(trait)` task | `overlapping-impl` |
| duplicate inherent members | inherent heads of one type, in its module | folder interface | `duplicate-inherent-member` |
| local impls | the body's local impls | the body | the same codes |

Only the impl checks need goals. Everything else is a function of heads,
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
3. **Phase one, ground heads** (no impl parameter), every one before any
   generic head: encode each head canonically and insert it into a hash
   set, where a second equal head is an overlap, and into a **ground
   trie** over the head's pre-order walk.
4. **Phase two, generic heads**, in content order: before inserting a
   head into the **generic trie** (a discrimination tree in which an impl
   parameter is a wildcard), query the generic trie for stored heads that
   may unify with it, and walk its wildcard positions through the ground
   trie. Every ground head is already in the ground trie, so a generic
   head meets every ground head and every earlier generic head, whichever
   came first in the source (Codex re-review N-T6). A row parameter
   matches every row; `Args < Tuple` matches every tuple; `TupleAny`
   matches every tuple head.
5. **Confirm** each candidate pair with full unification after renaming
   apart. The tries treat each parameter occurrence as its own wildcard,
   so a head with a repeated parameter, such as `Pair[T, T]`, yields
   candidates that only unification can reject. Report a pair once, on
   the later impl in content order (package, module path, item index).
   The message names the earlier impl and a **witness**: the unifier's
   solution applied to the head, such as "both apply to `Box[Plain]`".
6. **Budget.** The task counts trie nodes visited and stops after the
   first overlap reported per impl, so an all-overlapping bucket costs one
   report per impl, not one per pair.

Cost: linear in the total head size plus the candidate pairs the tries
return. For heads that differ at a constructor, which is the normal
case, the tries return no false candidates. Repeated parameters can add
candidates that unification rejects; the trie-node budget of step 6
bounds them, and the `pathological` suite measures them. This is not a
proof of linearity. 300 `From`
impls for one error type differ at the first trait argument, so the trie
separates them at its second level.

**Known M3 simplification (M3 gap 6).** `hd_resolve::Universe::overlaps`
currently sorts each trait's heads, then unifies every later head with
every earlier head. Its answer and content-order blame match this section,
but its work is quadratic. M4 replaces that loop with the ground and
generic tries above; the pairwise loop is not another supported strategy.

### 5.3 Why No Global Index

- **Selection needs no global view.** Ownership confines the impls a goal
  can use to at most `2 + len(Args)` modules, plus the candidate directory
  for open arguments (section 3.2), which is per trait and holds only the
  rare argument-owned impls.
- **Coherence needs a per-trait view, not a global one.** Each trait is
  one task keyed by its sorted head hashes, so an edit reruns only the
  traits whose heads changed. A **head hash** is `H(head, rank)`: the
  head's canonical encoding (its parameters, target and trait
  arguments) and its content rank (package, module path, item index).
  The rank decides which impl of a pair is reported, so it must be an
  input: swapping two overlapping impls changes both ranks, and the task
  reruns instead of reusing a report on the wrong impl (Codex re-review
  N-D1).
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
    heads: u32,                   // heads matched in the probe (section 7.4)
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
the bound-inference loop
([`types.generic.infer.bound.fixed-point`](../../spec/lang/04-type-system.md#r-types.generic.infer.bound.fixed-point)),
which is bounded by the number of parameters named only in bounds
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
impl fails at once. When several candidates fit only while literals stay
open, the solver retries with the defaults (`i32`, `usize`, `f64`)
before stalling. Literal defaulting at the end of the statement then
wakes the obligation.

### 6.5 Ambiguity

`Many` belongs to `Instantiations` and `Methods` only.

**`Instantiations { S, Tr }`** collects every impl head of `Tr` that
matches `S` with all trait arguments open: the owner tables of `Tr` and
`S`, and the candidate directory (section 3.2). For a parameter, it
collects the environment clauses on `S` for `Tr`; for a trait value, the
one instantiation the value names.

**Candidates are schemes with residual bounds (Codex re-review N-T4).**
Matching `S` fixes only the impl parameters that occur in the target. A
parameter that occurs only in the trait arguments, as `U` in
`impl[U < Display] Pick[U] for Family`, stays open, and a bound on it
cannot be decided yet. So each candidate is a **scheme**:

```rust
pub struct Candidate {
    pub row: ImplRef,          // or the clause or trait value it came from
    pub n_fresh: u8,           // impl parameters the target did not fix
    pub impl_args: TyList,     // each impl parameter: the fixed type, or the parameter itself when fresh
    pub args: TyList,          // the trait arguments, over the fixed types and the fresh parameters
    pub residual: Vec<u16>,    // the plan steps the solver did not decide, in plan order
}
```

- The solver runs every plan step whose parameters the target fixed. A
  candidate whose step `Fails` is dropped. A step that overflows makes
  the whole answer `Overflow`, since depth exhaustion is never a
  failure.
- The steps that read a fresh parameter are the candidate's
  **residual obligations**. So is a fixed plan step that stalls on a
  variable of `S`. A residual `Bind` step is never an obligation: only
  `Bound` steps become goals.
- The solver does not run them.
- The checker instantiates a scheme inside each trial: a fresh variable
  per fresh parameter, unified with the call's argument types, then the
  residual steps as obligations under that trial
  ([type-checking.md §2.5](type-checking.md#25-methods-and-operators)).

The outcomes, which the checker handles in one way:

| Outcome | Answer |
| --- | --- |
| `S` holds a placeholder at a position the heads need | `Stalled` |
| no candidate survives | `Fails` |
| one or more survive | `Many`, in content order, even for one: the checker always instantiates the scheme and its residual obligations |
| fuel runs out | `OutOfFuel` |

A scheme is stored over the impl's parameters and global types, so it
is memoizable as before.

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

A per-folder method index maps a name to the available traits that
declare it, so step 3 asks one goal per trait with that name, not one per
available trait. The `Methods` goal carries that trait list from the
index; it carries no `AvailKey`. The answer also lists, for diagnostics only, traits that
are not available but would match: the `use` fix-it.

**Checked once vs once per trial.** The checker infers each call
argument once before the trials unless its check needs the expected
type. Those go per trial, one check per surviving candidate: closures,
`.V` contextual variants (including through a call head), `if`, `match`
and block expressions, empty collections, and any list, map, tuple or
parenthesized argument holding one of those
([type-checking.md §2.4](type-checking.md#24-calls-and-use-site-type-arguments)).

## 7. Memoization And Budgets

### 7.1 Memo Keys And Eligibility

An answer may be reused only where every input that decided it is equal.
The Codex review (blocker 8) listed the inputs that D1's key missed. Each
is now part of the key, or makes the goal ineligible for the global memo:

| Input that can change an answer | How the key covers it |
| --- | --- |
| the goal's types, with variables | the canonical goal; placeholders keep their kinds (section 2.2) |
| declared bounds and supertrait bindings | `EnvKey`, the interned elaborated environment; `EMPTY` when the goal names no parameter |
| local impls, visible from their declaration point ([`trait.impl.local.lookup`](../../spec/lang/09-traits.md#r-trait.impl.local.lookup)) | `LocalVis`: the interned sorted list of visible local impls, in the `Body` key; a goal that names no local type or local trait cannot match a local impl and keys with `EMPTY` |
| trait availability (`Methods` only) | not keyed: `Methods` goals are not memoized, so there is no availability key yet (section 6.5) |
| which argument-owned impls the asking context sees (`Instantiations`, `Methods`, open-argument `Implements`/`Project`) | the context's `ImplUniverseId` (section 3.2), in every scope |
| what the proof read | each entry records the `(trait, head key)` probes of its subtree; a global entry serves a folder only when that folder's own table has no row for any of them (the own-table rule) |
| unowned rows read | an answer whose proof read unowned rows is scoped: it is published under its key with the universe filled in, and the universe counts folders with unowned rows (section 3.2) |
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
    children: u32,                              // range in the children arena of entry indices (not goals): the proof DAG (7.4)
    height: u8,                                 // intrinsic height (7.3); 255: a cycle
    kind: u8,                                   // Holds | Fails | Stalled | Overflow | AtLeast
    heads: u16,                                 // heads matched in the probe (7.4); saturating
}
```

- **First writer wins** in the global memo. Two threads that compute one
  key compute the same entry (rule TS-1), so either write is correct.
- **Hits take no lock (systems review, finding 9).** A memo hit used to
  lock a shard, and rule TS-5's walk did one locked lookup per node of
  the proof DAG. So:
  1. **Children are entry indices.** `MemoEntry.children` is a range of
     `u32` entry indices into the append-only `entries` arena, not
     canonical goals to look up again. A global entry's children are
     global entries, since a goal that names no parameter or local type
     asks only such goals. A body memo entry marks each child as body or
     global with the high bit. The TS-5 walk then follows indices and
     reads published, immutable entries with no lock and no hashing.
  2. **A per-worker read-through table** of 4,096 direct-mapped slots,
     `(hash, entry index)`, sits in front of the shards, as for the
     interners ([data-structures.md §3.3](data-structures.md#33-interners)).
     A published entry never changes, so a slot is never stale within a
     run. The table is cleared when a new memo starts (a new run).
  3. **The selection table** that codegen shares across programs
     ([codegen.md §13.2](codegen.md#132-collection)) gets the same
     per-worker table.

  | Operation (cold 10k-line check, review's estimates) | Count | Before | After |
  | --- | --- | --- | --- |
  | memo lookups | about 50,000 | a shard lock each: 20 to 150 ns | an estimated 80% per-worker hits at 5 to 10 ns; the rest locked |
  | TS-5 walk nodes | about 150,000 | a locked lookup each | an index load each: 2 to 5 ns |
  | total, with interning (data-structures.md §3.3) | about 400,000 locked operations | 10 to 60 ms of CPU | about 2 to 10 ms of CPU |

  Slice 3 measures the hit cost at 1 and 8 threads; the 100 ns hit
  target (§13) is gated at 1 thread and reported at 8.
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

**What counts as a level** (owner, 2026-10-07): a bound-plan
step of an impl, an element obligation of a tuple template, one
`Inspectable` step through a type argument, and one `Project` step. An
environment clause, a trait-value source and a sealed membership with no
subgoal are leaves at no extra level.

### 7.4 Fuel

type-checking.md fixes the principle: fuel used is a pure function of the
body and its frozen inputs
([type-checking.md §11.1](type-checking.md#111-what-counts)).

**Proof nodes, charged once per body (Codex re-review N-T2).** Each memo
entry stores its **children**: the canonical goals its committed plan
asked, in plan order, with repeats among its own children removed (as in
`Pair[X, X]`, where both bound steps ask `X: Eq`). It also stores how
many heads its probe matched (rows that survived the fast reject). The
entries and their children form the goal's **proof DAG**. The children
are a function of the goal (rule TS-1), so the DAG is the same whether
an entry was computed now or found in a memo.

An earlier version charged a stored recursive cost, `1 + heads + sum of
the children's costs`. That sum counts a shared descendant once per path
to it, so a DAG with two goals per level, each needing the same two
goals at the next level, cost `2^n` for about `2n` goals. It is
replaced.

**Rule TS-5. Charge each proof node once per body (mine).** A body's
`met` set records the canonical goals it has paid for. When a body asks a
goal, the solver walks the goal's proof DAG depth first, in plan order.
Each node not yet in `met` costs `1 + heads matched` and joins `met`
before its children are walked. A node already in `met` costs nothing,
and the walk does not enter it; the asked goal itself costs 1 when it is
already met. A goal computed now is charged by the same walk, as its
frames pop. So a body pays once for each distinct goal in the union of
the proof DAGs it used, whatever the memo held. The sequence of goals a
body asks is fixed by its source and its frozen inputs, so the charge,
and the point where fuel runs out, are deterministic. A rollback refunds
nothing and does not clear the `met` set. A cycle ends the walk, since a
node joins `met` before its children.

Charging the full DAG on every hit would bill the 1,600 `Cents::from(N)`
lines of the `pathological` metric 1,600 times for one goal. Charging only
what was computed would make a warm memo cheaper than a cold one, so a
body could pass its budget on one run and fail on the next (lesson 3 of
[Lessons For hd](prior-art-issues.md#lessons-for-hd)).

### 7.5 Limits And Their Diagnostics

| Limit | Value | Counted by | Diagnostic |
| --- | --- | --- | --- |
| proof depth | 64, fixed by the spec | heights (7.3) | `trait-resolution-depth` at the use, showing the first three goals of the chain and the last |
| body fuel | 2,000,000 steps per body | proof nodes, once per body (7.4) | `item-too-complex`, by the checker |
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
4. Shared subgoals are computed once and charged once per body as proof
   nodes, so a diamond costs the size of its DAG.

### 7.6 Bounded Is Not Linear

Fuel and depth make every case end. They do not make it fast. The Codex
review (T6) asks for the algorithms that make the common cases
near-linear; these are they, each with the input it is linear in:

| Work | Algorithm | Linear in |
| --- | --- | --- |
| repeated goals | the memo, keyed canonically; repeats cost one lookup | distinct goals |
| shared subgoals | memoized children; each proof node charged once per body (rule TS-5) | distinct goals in the proof DAG |
| impl lookup | head key, then first two argument keys; the fast path for non-generic traits | candidates that share both keys, usually one |
| open trait arguments | the per-trait candidate directory, filtered by the closure bit set | argument-owned impls of that trait |
| overlap | ground heads hashed; generic heads in a discrimination tree (section 5.2) | total head size, plus reported overlaps |
| elaboration | once per item, one clause per trait reference, bindings merged | the supertrait closure |
| canonicalization | one walk of the goal, charged one step per node | goal size |
| normalization | one memoized `Project` per projection | distinct projections |
| instantiation choice | prefilter candidates by the head of each known argument type before any trial (change 7); one trial per surviving candidate | surviving candidates |
| nested trial chains | a per-call-site trial memo keyed by the canonical expected type and the canonical argument types (mine; change 8). It removes repeats only: when outer candidates give an inner site the same context, trials cost sites × candidates; when they give distinct contexts, the count can grow exponentially, and fuel ends it with `item-too-complex` (Codex re-review N-T7) | distinct trial contexts, plus tainted trials |
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
| a conversion to a trait value | `Coerce` of kind to-trait-value | the concrete type, the trait reference and the `Impl` choice |
| a call through a trait value | `CallDyn` | none: its method-level bounds are proven at the call and reach the erased body through the type witness (section 9.2) |

The `TraitMethod` choice in D2's catalog is "the impl's `DefId` or the
index of the bound in scope". This design needs two more choices,
`TraitValue` and `Builtin` (section 16.4, changes 4 and 16). A tuple template
instance is an `Impl` choice and needs none.

### 8.3 What Codegen Does With It

**Rule TS-6. Codegen selects through owner lookup (mine).** At an
instance, every type is concrete. A program build's impl universe is
every folder, and selection shares the run's global memo with checking.
`select` solves the concrete trait reference as an `Implements` goal and
takes the `Impl` evidence, or a sealed trait's `Builtin` evidence
(`Selection::Builtin`, section 3.9): the designed head-only `select` with its own
table is not built. It has no depth limit of its own and charges no
fuel. The returned `Selection` holds the impl row with the args the
proof found; `Selection.args` returning every impl argument verbatim
from the solver is not done.

**Selection, then reconstruction (Codex re-review N7).** A head match
fixes only the impl parameters that occur in the head. In
`impl[T < Display, I < Store[Item = T]] Summary for Feed[I]`, matching
`Feed[ConcreteStore]` fixes `I` but not `T`, and the selected method's
body needs `T`. So after the head match, `select` runs the plan's `Bind`
steps (section 3.6), in plan order, with `normalize_concrete`: each one
reads a binding at concrete types and fixes its target parameter. It
skips the `Bound` steps, which the checker proved. This is deterministic
normalization, not search: the plan order already exists, and every
parameter is fixed by the head or by a `Bind` step, or resolution has
rejected the impl as `unconstrained-impl-parameter`. The returned
`Selection` holds every impl argument.

`select` solves through the run's shared global memo, not in a proof
memo of its own: there is no separate codegen table with its own keys,
since selection assumes proofs instead of making them.

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
- **Vtables.** For each `(type, trait reference)` that a coercion or an
  evidence choice needs, codegen selects the impl of
  the trait and of each supertrait, and fills the vtable shape (section
  9.2).

## 9. Trait Values And Dynamic Safety

### 9.1 Which Traits Can Be Values

**Every trait can be a `dyn` type** (owner, 2026-10-07, in
[goals.md](goals.md#summary); applied in S1d as
[`trait.dyn.any-trait`](../../spec/lang/09-traits.md#r-trait.dyn.any-trait)). The
per-trait dynamic-safety gate of
[Dynamic Safety](../../spec/lang/09-traits.md#dynamic-safety) is
dropped, Swift 5.7 style. A trait value type is written `dyn Tr`
([syntax.md §4.4](syntax.md#44-parser-and-green-tree)). What cannot work dynamically is decided **per member**,
and the error is at the call, not at the type.

Resolution computes, per trait member at interface time, from headers:

| Member | On a `dyn` value |
| --- | --- |
| an associated function | unavailable |
| a method with `Self` outside the receiver | unavailable |
| a method with method-level type parameters, whatever their bounds | available, through the erased slot (section 9.2; [`trait.dyn.safe.method-type-param`](../../spec/lang/09-traits.md#r-trait.dyn.safe.method-type-param), S1c) |
| a method whose signature mentions an associated type | available when the `dyn` type binds that type; otherwise see below |

The trait record stores, per member, an "unavailable" flag with its
reason and a small set of the associated items its signature mentions.
The checker reads both at a call through a `dyn` value: a cheap test, no
solver goal.

**An unbound associated type (Codex re-review, authors' question 2).**
Decided (owner, 2026-10-07): the type itself stays an error, as
`trait.dyn.binding.complete` says; the per-member recommendation below
was not taken.
Today [`trait.dyn.binding.complete`](../../spec/lang/09-traits.md#r-trait.dyn.binding.complete)
requires a `dyn` type to bind every associated type. With the gate gone,
this design recommends, as an owner question, the per-member rule
instead of existentials:

- A `dyn Tr` that leaves `Item` unbound is a valid type. Every member
  whose signature mentions `Item` is unavailable on it, with the error at
  the call and a fix-it that adds `Item = ...` to the type. Members that
  do not mention it work.
- Such a type satisfies no bound on `Tr`, or on a trait that reaches
  `Item` (Swift's "an existential does not conform to its protocol").
  Generic code under `T < Tr` may name `T::Item`, and
  `<dyn Tr as Tr>::Item` has no normal form, so `Project` on it `Fails`.
- No call produces an existential value, and no member is opened.

The alternative keeps `trait.dyn.binding.complete` unchanged, as Rust
does: the type itself is an error. It needs no language change, but it
is the one remaining per-type gate. Either way the solver's part is the
same table; only where the error lands differs.

Requirement keys
([`req.key.any-trait`](../../spec/lang/11-requirements-and-suspension.md#r-req.key.any-trait))
follow the same per-member rule.

### 9.2 Vtable Shapes

A **vtable shape** is computed once per trait at interface time and stored
in the trait record (mine):

```rust
pub struct VtableShape {
    slots: Box<[SlotRef]>,          // the trait's own methods, in declaration order
    supers: Box<[DefId]>,           // direct supertraits, in declared order; each a pointer to its vtable
    type_id: bool,                  // the trait is Inspectable or extends it
}
```

- **Supertraits by pointer, not by copying their slots.** A diamond such as
  `Error < Display & Inspectable` shares one vtable per
  supertrait. Widening to a supertrait value reads one pointer
  ([`trait.dyn.widen`](../../spec/lang/09-traits.md#r-trait.dyn.widen)).
- **Method-level type parameters.** A method available on `dyn` may have
  a parameter `T < Display`, called with any type argument
  ([`types.trait.safe.method-type-arg`](../../spec/lang/04-type-system.md#r-types.trait.safe.method-type-arg)).
  The checker proves each such bound at the call's type argument, as an
  ordinary `Implements` goal, and records nothing more: `CallDyn` has no
  evidence operands. The slot's body is the impl method compiled once,
  erased, and it receives one type witness per method type parameter
  (owner, 2026-10-07, in [goals.md](goals.md#summary)). The witness
  carries the layout operations and, as outlined thunks, the bound
  methods at the caller's concrete type, each selected by head there
  (rule TS-6). That ABI is codegen's
  ([codegen.md §13.5](codegen.md#135-dictionaries-trait-values-and-gadt-evidence),
  Codex re-review N5). Statically dispatched calls of the same method
  stay monomorphized.
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
enum or newtype of the user's own package, and `Tr` has a
template, the fix-it inserts `@derive(Tr)` or extends an existing list. It
adds missing law partners, since `@derive(Hash)` alone is
`mixed-derived-law` ([Law Partners](../../spec/lang/09-traits.md#related-traits)),
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
The owner chose this over one error per use site (2026-10-07).

A stalled goal still open at the end of the body becomes type-checking.md's
`cannot-infer-type`, naming the goal's trait: "cannot infer `U`: `Both`
implements `Source[i32]` and `Source[string]`".

## 11. Determinism And Parallelism

**Rule TS-8. Every order the solver shows comes from content.**

- Candidates, `Many` lists, `near` lists and coherence reports are ordered
  by each impl's content `rank`: its module's rank in stable-path order,
  then its item index in source order. An item index, unlike a byte
  offset, does not move when a body above the impl is edited, so the
  interface that stores it stays valid. `rank` is computed when the interface is
  built, so sorting never compares paths at solve time, and never uses a
  `DefId`.
- Elaborated clauses are in declaration order, supertraits depth first in
  declared order. A `Bound` index is therefore the same on every run.
- Canonical placeholders are numbered by first occurrence in a fixed walk.
- The bound plan runs in its stored order, so the first failing step is
  fixed.

**Purity and sharing.** Answers are functions of their keys (rule TS-1),
and fuel is charged per distinct proof node (rule TS-5), so a warm memo, a
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
| `MemoEntry` | memo | 12 B, plus 4 B per child in the children arena | global: process; body: the body |
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
| diamond supertraits (`Error`'s three supertraits, `Num`'s eight) | duplicate clauses | elaboration keeps one clause per trait reference and merges its bindings | linear in distinct supertraits |
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
| memo invariance | the same goal sequence with a cold memo, a warm memo, a shuffled body order and 1, 4 and 16 threads: identical answers, identical `fuel_used` per body (rules TS-1, TS-5). It includes two modules that ask one `Instantiations` goal with different closures, in both orders (section 3.2) |
| canonicalization property | alpha-renamed goals get one key; goals that differ only in a variable's kind get different keys |
| select soundness | in CI builds, every `select` at an instance re-solves the full goal and asserts `Holds` with the same impl (rule TS-6) |
| pathological suite | each case of section 13, well-typed and ill-typed, within its fuel and wall-time budget, with its limit diagnostic; each also at sizes `n` and `2n`, failing when the observed exponent exceeds 1.2 (section 7.6) |
| one-root-cause fuzzer | mutate one impl or bound of a well-typed program; more than two error diagnostics flags a cascade for review |
| adversarial solver fixtures (Codex re-review N-R1, gate 4; frontend lane) | built before the full checker, run with shuffled scheduling before threads: two modules with different candidate universes in both orders (section 3.2); binding-constrained impl parameters through `select` (section 8.3); supertrait diamonds that bind on one path only, and with conflicting bindings (section 4.2); shared-subgoal DAGs with a fixed fuel charge (section 7.4); residual bounds that stall, then fit or fail (section 6.5); projections through impls whose bounds fail (section 4.3); trials that hold early returns or wake older stalled calls (type-checking.md §3.5). Accept or reject goes into implementation-neutral conformance fixtures; costs and answers go into solver snapshots |

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
| dictionary plans in HIR, an erased-dictionary ABI shared with the emitter | [CA-04](../../audit/compiler/findings-2026-10-04.md#ca-04-cross-stage-representation-and-distributed-abi) | TIR records only the top choice; codegen selects by head (TS-6); dictionaries only for trait values; `dyn` generic methods get type witnesses (codegen.md §13.5.1); GADT evidence is removed with GADTs |
| derivation diagnostics in process-global registries keyed by span | [CS-02](../../audit/compiler/findings-2026-10-04.md#cs-02-derivation-diagnostic-registries-reuse-keys-without-compilation-identity) | `FailInfo` carries the impl's origin; diagnostics are values in the body result |
| derives through generated source that is parsed again | checker audit, Derivation And Std Integration | the solver sees derived heads only; template bodies are token text with a resolution table, checked as derive instances |
| bound inference repeats until no solution changes | checker audit, `bound-inference.ts` | the spec's bounded loop stays in the checker; the solver has no fixpoint (TS-7) |
| associated calls and member calls use separate selection paths | checker audit | one `Methods` goal for both; one `Instantiations` goal for the choice |

## 16. Open Questions, Readings And Changes

### 16.1 Questions For The Owner

All four are answered (owner, 2026-10-07) and used above:

| Question | Answer | Where |
| --- | --- | --- |
| 1. Which impls an instantiation set counts | only impls in the asking module's dependency closure; the spec pass adds a sentence to [Instantiations Of One Generic Trait](../../spec/lang/09-traits.md#instantiations-of-one-generic-trait) and [Inference Through A Bound](../../spec/lang/04-type-system.md#inference-through-a-bound) | section 3.2 |
| 2. Projections of impl parameters in impl heads | forbidden, as `unconstrained-impl-parameter`; the spec pass says so in [`trait.overlap.constrained-head`](../../spec/lang/09-traits.md#r-trait.overlap.constrained-head) | section 4.4 |
| 3. One level of proof depth | a bound-plan step, a tuple template's element obligation, an `Inspectable` step and a projection step; environment clauses, trait-value sources and sealed memberships are no level; the spec pass adds the rule | section 7.3 |
| 4. One error per missing impl per body, or per use | one per body and leaf goal, with "and N more uses" | section 10.4 |

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
4. **`Structure` holds through a template instance's environment**,
   never as a written impl
   ([`annot.structure.generated`](../../spec/lang/14-annotations.md#r-annot.structure.generated)).
   The one compiler-wide answer is a concrete tuple type (#126): its row is
   shared by checking and codegen because row memo entries carry no mode.
   Checking cannot reach it from user code, since naming `Structure`
   outside a template is `structure-outside-template` and a template's `T`
   is a parameter its environment answers. A template body that names
   `Structure` at a concrete tuple would get the tuple answer; nothing
   writes that today.

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
   "dictionaries only for trait values and GADT evidence" (both GADT
   evidence and `CallDyn` bound evidence are now gone; change 19); type-checking.md's goals have no `mut`, the
   prototype's F-619 (change 1); D2's collection re-solves where a head
   match suffices (change 18).

### 16.4 Changes Needed In type-checking.md And The Other Design Files

For the owners of those files to make. This document edits none of them.
Status, 2026-10-07: changes 1 to 11 are applied in type-checking.md and
changes 12 to 14 in resolution-and-interfaces.md. Changes 15 to 21 are
the backend lane's, applied in codegen.md, checking-and-tir.md, cache.md
and data-structures.md.

**type-checking.md**

1. **§1.6 `Goal`:** add `mut_: bool` to `Implements` and `Instantiations`;
   wrap the self type and arguments in a `TraitRef`; `Project` names the
   associated item's `DefId` (section 2.1).
2. **§1.6 memo:** the keys and eligibility of section 7.1: the `Env`
   scope, `LocalVis`, `AvailKey`, a global memo per run, and no global
   entry for a goal
   that met the depth cut or ran out of fuel.
3. **§1.6 fuel and §11.1:** each proof node is charged once per body, and
   a repeated ask costs 1 (rule TS-5).
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
   ([`trait.resolve.fits.expected`](../../spec/lang/09-traits.md#r-trait.resolve.applies.expected)).
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
    per-member `dyn` availability and vtable shape; list tuple templates in the
    `heads` section with head key `TupleAny`.
14. **§4.12.3:** replace pairwise bucket unification with the hash set and
    discrimination tree of section 5.2, and report a witness type.

**data-structures.md**

15. **§3.4 and §3.9.2:** the projection type is `Assoc { assoc: DefId,
    tref }`, with the trait's arguments, not `{ base, trait_, name }`.

**checking-and-tir.md**

16. **§4.13.11 catalog:** the `TraitMethod` choice gains `TraitValue` and
    `Builtin`. `CallDyn` has no evidence operands: the erased body gets
    its bound methods from the type witness (Codex re-review N5).
    (`NewVariant` evidence choices are removed with GADTs.)
17. **§4.13.9:** a derive instance needs no coinductive assumption
    (section 3.10). Supertraits, supertrait bindings, delegation parts
    and newtype bases are checked by the folder's `HeaderCheck(F)` task
    (resolution-and-interfaces.md §4.10.1), not by an M2 body task, and
    scheduler.md gains that task.

**codegen.md**

18. **§13.2 step 4:** select by head only, with no depth limit and no
    fuel; a failure is an internal error (rule TS-6).
19. **§13.5:** vtables follow the trait record's shape, with supertrait
    vtables by pointer; a slot for a method with method-level parameters
    is the erased instance with one type witness per parameter (section
    9.2; codegen.md §13.5.1).
20. **§13.6:** tuple `Eq`, `Ord`, `Hash` and `Debug` are tuple-template
    instances, instantiated per tuple type like any impl.

**cache.md**

21. **The `check` key** covers argument-owned impls through the deep
    hashes of the module's dependency closure; the `argc` hash that this
    change first asked for is redundant and goes (Codex re-review N-A1,
    section 3.2). The solver memo is never persisted and never part of a
    key.

### 16.5 Codex Review

The Codex review of commit 8bb6860d (`codex_review.md`, not committed)
predates this document. Each finding below was checked against the spec
before it was accepted.

| Finding | Verdict | Where |
| --- | --- | --- |
| Blocker 3: owner lookup cannot enumerate unknown trait arguments; std's inherent exception | **Accepted, fixed.** Verified: [`trait.own.argument`](../../spec/lang/09-traits.md#r-trait.own.argument) allows argument-owned impls, and [`trait.own.module.inherent.std`](../../spec/lang/09-traits.md#r-trait.own.module.inherent.std) allows any std module. Taken: the per-run, per-trait candidate directory and a coarse hash in the check key. Changed: the directory is filtered by the asking module's dependency closure, so a downstream package cannot change a library's result (question 1) | sections 3.2, 5.3; changes 12, 21 |
| Blocker 5: dynamically safe generic methods lack a codegen strategy | **Accepted, fixed** for the solver's part. Verified against [`types.trait.safe.method-type-arg`](../../spec/lang/04-type-system.md#r-types.trait.safe.method-type-arg): an erased slot instance with one vtable parameter per bound | section 9.2; changes 16, 19 |
| Blocker 8: memo entries are not functions of their keys | **Accepted, fixed.** Keys now hold the environment, visible local impls, availability, and a per-run memo; depth is a stored height with lower-bound entries; fuel and depth exhaustion are never cached as failures; completion before publication is the SCC rule. Not needed: a key for coinductive assumptions, since none exist | sections 6.3, 7.1, 7.3; change 2 |
| T1: projections cannot represent their inputs or return outputs | **Accepted, fixed.** Verified: D1's `Assoc` drops the trait arguments, and §1.6's answers had no normalized type. Projections now name the associated item and the instantiated trait reference; `Project` answers `Normalized`; normalization cycles, the occurs check and aliases are specified | sections 2.1, 4.3; changes 1, 4, 11, 15 |
| T4: candidate trials omit expected-result filtering | **Accepted.** Verified: [`trait.resolve.fits.expected`](../../spec/lang/09-traits.md#r-trait.resolve.applies.expected). The trial is the checker's; the change is stated for type-checking.md | change 9 |
| T6: fuel bounds work but does not make it near-linear | **Accepted, fixed** for the solver and coherence: the algorithms of section 7.6, the discrimination-tree overlap check, and the doubling test. The trial multiplication and obligation waking belong to the checker; changes 8 and 11 state them | sections 5.2, 7.6, 14.2; changes 7, 8, 11, 14 |
| A5: synthetic impls need dependency and coherence rules | **Accepted, fixed.** Verified: any trait may have a tuple template ([`annot.template.tuple.form`](../../spec/lang/14-annotations.md#r-annot.template.tuple.form)), and a written tuple impl beside it is an overlap ([`annot.template.tuple.overlap`](../../spec/lang/14-annotations.md#r-annot.template.tuple.overlap)) | section 3.11; change 13 |
