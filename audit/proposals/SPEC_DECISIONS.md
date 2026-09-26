# Specification Decisions, Round 1

**Status:** decided by the owner on 2026-09-25, in the discussion after the
conformance round (W0 to W5). Each entry records the decision and the spec
change it requires. Evidence tags refer to
[`../evidence/07-spec-feedback/DECISION_LOG.md`](../evidence/07-spec-feedback/DECISION_LOG.md)
and to `audit/findings/`.

Deferred: replay and the host boundary (D1 to D6 of the decision log). They
stay in `future-work/OPEN_ISSUES.md`.

## Generics and the Object Model

| #  | Decision | Spec change |
| -- | -------- | ----------- |
| G1 | The runtime representation of generic code is not observable. The reference model is Go-style shape stenciling: one specialized copy per scalar shape (`i32`, `i64`, `f32`, `f64`), one shared erased copy for every reference-shaped type, and dictionaries for trait operations. | Chapter 04: replace the normative erasure sentence with representation independence. Add a non-normative "Implementation Model" section at the end of chapter 04. Align chapter 09's "may monomorphize" sentence. (C1) |
| G2 | A dynamically safe trait may declare method-level generic parameters when every such parameter is bounded by `Reference`. More bounds are allowed (`T: Reference + Display`), passed as dictionaries. Primitive, tuple, and optional arguments must be converted by the caller. | Chapters 04 and 09: dynamic-safety rule. |
| G3 | Tuples are immutable: a tuple element is not a place. | Chapter 05, place expressions; add the general code `invalid-assignment-target`. |

## Language Rules

| #   | Decision | Spec change |
| --- | -------- | ----------- |
| L1  | Module initialization: a trait method call through a generic bound or a trait value reaches every implementation of that method in the module. | Chapter 10, module initialization. (B2, F-603) |
| L2  | An integer exponent must have an unsigned integer type. An unsuffixed integer literal in exponent position is typed `u32`. A signed exponent is a `type-mismatch` error. No runtime failure exists for negative exponents. | Chapters 04 and 05. (B8, F-153) |
| L3  | Remove the `failed-checked-cast` panic category until a checked-cast construct exists. | Chapter 06. (B8) |
| L4  | An executable entry point is `pub fn main()` or `pub fn main!()`, returning `void` or `Result[void, E]` with `E: Display`, with an optional requirement row whose keys are host capabilities of the selected runtime profile (`nonhost-entry-requirement` otherwise). A non-`pub` `main` is an ordinary function. | Chapter 10, executable entry point. (B11) |
| L5  | An enum variant carrying shared constructor data has allocation identity. Only values that store no data are canonical. | Chapter 05, identity. (AMB-08) |
| L6  | Inferring `mut T` from a readonly argument is a `mutable-upgrade` error. | Chapter 04, permissions. (AMB-17) |
| L7  | `is` operands are compatible when, with `mut` removed at every level, their types are equal, or one is a trait value or `Any` type that the other converts to. | Chapter 05, identity. (AMB-24) |
| L8  | Fieldless data values are canonical: one identity per fieldless data type, with no allocation. | Chapters 05 and 08. (B10) |
| L9  | An indirect supertrait cycle is reported once, on the cycle member first in source order (across modules: first by module identity, then by position). | Chapter 09. (F-208) |
| L10 | One general code, `impure-default`, replaces `impure-data-default` and `impure-enum-default`, and covers parameter defaults. A default naming a later parameter is `binding-not-yet-visible`. | `spec/README.md`, chapters 07 and 08, and fixtures using the old codes. (A8, AMB-15) |
| L11 | A non-final vararg, in a declaration or a function type, is a `nonfinal-vararg` error. | `spec/README.md`, chapter 07. |
| L12 | Confirmed: provider values may escape their scope; there is no `pure` qualifier; `all!` and `race!` are compiler intrinsics. | Remove the question from `OPEN_ISSUES.md`. (B7, F-100) |

## Clarifications and Settled Questions

| #  | Item | Spec change |
| -- | ---- | ----------- |
| C1 | A named function without a requirement clause has the empty row. | Chapter 11. (B1) |
| C2 | Map equality ignores entry order. | Chapter 05, if not already explicit. |
| S1 | Defaults may read reassignable top-level bindings: chapter 07 forbids only reassignment. F-101 is a compiler bug. | Remove the question from `OPEN_ISSUES.md`. |
| S2 | A panicked instance must be discarded by the host (chapter 06 already says so). | Remove the question from `OPEN_ISSUES.md`. |
| S3 | A stray `$` is a `syntax-error` (applied in W4). | Remove the question from `OPEN_ISSUES.md`. |

# Specification Decisions, Round 2

**Status:** decided by the owner on 2026-09-25, in a discussion of whether
the language suits incremental and parallel compilation.

| #  | Decision | Spec change |
| -- | -------- | ----------- |
| I1 | Rule of thumb: a public item must be fully annotated. Nothing in a public signature is inferred from a body. Top-level bindings cannot be public (already true in the chapter 02 grammar). | Chapter 10, name resolution across packages. |
| I2 | Ordinary generic bodies are optional in a package interface; each ordinary generic function is compiled in its defining package, once per shape. Pack and reified bodies are carried as source. | Chapters 10 and 04. |
| I3 | A package interface is determined by its declarations alone, so a dependent compiles as soon as its dependencies' interfaces are known. | Chapter 10. |
| I4 | The module-initialization check stays module-local, with a non-normative note on computing it from per-function read summaries. | Chapter 10. |
| R1 | "Purity" is replaced by **requirement-free**: no provider use and no suspension, checked from callee signatures only. Defaults may mutate state and may call function values and dynamic trait methods with empty rows. Purity summaries leave package interfaces. | Chapters 02, 07, 08, 10. |
| R2 | Annotation builders and metadata are requirement-free too; they run once per key at the first request and observe state as of that moment. | Chapter 14. |
| R3 | A default that uses a provider is `requirement-in-default`, replacing `impure-default`; a suspending default is the existing `suspension-forbidden-context`. | `spec/README.md`, fixtures. |
| R4 | Non-entry module initialization is requirement-free initialization (renamed; same rule, plus no calls with non-empty rows). | Chapter 10. |
| I5 | A non-public function, inherent method, or local `fn` may omit its result type, inferred from its body (`void` when it produces none). Parameters stay explicit. Public functions, trait methods, and methods of trait implementations must declare results (`missing-result-type`). A call cycle among functions with omitted results is `recursive-function-needs-result-type`, reported on the first member in source order. | Chapters 02, 04, 07, 11; `spec/README.md`. |
| I6 | Such non-public callables may also omit their requirement clause; the row is inferred with the closure rule, and cyclic callers get the least rows satisfying the cycle. Public functions, trait methods, and methods of trait implementations keep "no clause means the empty row" (amends round-1 C1). Suspension is never inferred. | Chapters 07 and 11. |
| B1 | (Commit f492a30 calls this S1, which already names a round-1 row.) Bounds use `<`: supertraits (`trait A < B:`), generic parameter bounds (`[T < Display]`, `[reified T < Shape]`), and `where` predicates (`where T < Display`). `:` means only "has type" or opens a suite. A trait with supertraits may be bodiless (`trait Tagged < Named`). The prototype parser was patched to accept `<` (it still accepts the old `:` form). | Chapters 02, 04, 05, 07, 09, 10, 11, 14; all fixtures; guide and syntax notes; `src/parser/parser.ts`. |
