# Typed Derivation Lowering (M13, 2026-10-10, #253)

Nine valid fixtures fail with `unsupported: stage Collect: a call of the
body-less std/structure/Structure.<facts|walk|build|describe>`
(`derive-members-of-data-and-enums`, `typed-derivation-walk`,
`serde-derive-read-order`, `json-typed-members`,
`derived-arbitrary-with`, `derived-default-*`,
`serde-variant-member-facts`, plus the `handle-fact-exact-type` twin).
This designs the Rust lowering. No code changes.

## What must exist

Per `spec/lang/14-annotations.md` Typed Derivation and
`lib/std/structure.hd`: for each derivation (a `@derive(X)` or `by
Structure` block on a data/enum target), per-type bodies for the
`Structure` methods the template calls — `facts`, `walk[W]`,
`describe[D]`, `build[S]` — plus handle constants (`Field` per member,
`Variant` per variant) and the derived-trait `impl` itself. `walk`
matches once, calls `variant` then `member` per member in order (`?`
propagates the first `.Err`); `describe` enumerates variants/members
without a value; `build` takes `variant`/`next`/`member`/`missing` and
constructs; `facts` returns the source-order fact list. Derivations see
their own `Structure` (`annot.structure.per-derivation`); tuple types
instantiate on use.

## Which stage generates

At **Collect**, not pre-check source text. The TS prototype generates hd
source before checking (`typed-derivation.ts`, `template-instances.ts`:
traversal fns, handle constants, forwarding impls, cached once per trait
and shared per target). The Rust front end already checked the template
against `T` (`hd_check::derive::check_opt_in`, `call::template_self`)
and re-checks it as an instance (`derive::instance`,
`BodyKind::DeriveInstance`); what gaps is the body generator
(`hd_check::structure::structure_bodies` declines, so Collect finds
`body(def).is_none()` at `hd_mono/src/lib.rs:1103` and stops). Filling
that generator keeps one checked template, one elaboration, and the
existing instance machinery — no second parse, no generated source to
re-resolve, no debug-printing of synthetic code.

Concretely: `structure_bodies` returns, per derivation target, TIR
`Body`s (`BodyKind::DeriveInstance`, `hd_tir/src/ir.rs`) for each called
`Structure` method: `walk` lowers to match-on-self → `variant(v)` →
`member(h, field)` per member with `?` early-out (the same shape the
checker already understands); `describe`/`build` analogously;
`facts` to the items-list construction `structure.hd` already declares;
handles to the `Field`/`Variant` literals over compiler-filled
`hd_get`/`hd_witness` slots. `hd_structure::derivation()` already emits
this shape (hidden methods, `TirBuilder::DeriveInstance`); the work is
completing it past the current gaps (facts of a derivation's
`Structure`, shared ctor data, non-ADT targets) rather than a new
pipeline.

## How they stay cached per type

Follow the tuple precedent (`supplied_target`, `hd_mono/src/lib.rs`):
memoize each generated body in `Collected::supplied[(method, self_ty)]`
and push the instance with `Sub::Body(0)`, so `(Structure.walk,
[Point, DebugShape[Point]])` is built once and shared by every
derivation and call site (codegen.md §13.6 already specifies this
keying; §13.2–13.3 cover worklist and key formula). Handles and facts
share per `(target)` the way the TS `hd__s_Point` part does; member-line
`+=`/`=`/`=pass` variants key by (target, block) exactly as template
instances do (`annot.template.instance`). Cache keys gain nothing new:
bodies are pure functions of (template TIR, target descriptor, block
lines), all already in the code key.

## Footprint for a program that derives nothing

Zero, by gating: the generator runs only for targets with derivations
(TS gates the whole pass on `needsStructureDeclarations`). No bodies, no
handles, no impls, no `Structure` template code reaches Collect output;
`std.structure` still joins for the trait declarations but contributes
no instances. A `#[derive]`-free program's link is byte-identical with
and without this feature.

## Questions with a recommendation

- **Q1. Collect-time TIR vs pre-check source text (TS-style)?** TIR at
  Collect (recommended): the template is already checked once and the
  instance machinery (keys, `supplied` cache, folding) exists; source
  generation would re-parse, re-resolve, and re-check synthetic code.
- **Q2. One body per (method, target, walker) or per (method, target)?**
  Per (method, target, walker/describer/source) triple (recommended):
  the walker type changes the called methods, so sharing across walkers
  is unsound; sharing across derivations of one target falls out of the
  instance key.
- **Q3. Who builds tuple `Structure`?** The same generator via the
  existing tuple `supplied` path (recommended): tuples already work;
  route them through one function.
- **Q4. `find`/`fact::[D]()` inference (M4 C2)?** Out of scope here
  (recommended): this design covers called-method bodies; typed-fact
  argument inference is its own check-side task.

Recommendation: complete `structure_bodies` for the called-method set
first (unblocks all nine fixtures), then handles/facts sharing, keeping
the TS pass as the behavioral oracle for generated-code equivalence.
