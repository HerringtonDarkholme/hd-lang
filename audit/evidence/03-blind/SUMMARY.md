# Blind fixture authoring: summary

Copied from audit/blind/AUTHOR_NOTES.md by the coordinator; the author's harness blocked writing this file.

# Blind Fixture Author Notes

Commit bd985d7. Authored 2026-09-25.

## Method

1. I read the specification: `spec/README.md`, chapters 01, 03 to 08, 10,
   11, and the relevant parts of 02 (statements, match arms) and 09
   (comparison traits, derive). I also read `spec/conformance/README.md`,
   the `spec/conformance` runtime fixtures and selected typing fixtures, but
   only to learn the format and marker placement. I read the fixture format
   section of `test/README.md`, section 3.1 of `audit/PLAN.md`, and
   `audit/AGENT_BRIEF.md`.
2. For each of the nine target areas I listed the normative sentences, the
   ones with **must**, "is evaluated", "panics", or "is rejected". I then wrote
   fixtures that observe them. Runtime fixtures record a trace string in a
   `mut` data value, or in a top-level `let` for cancellation scenarios. They
   then check the trace with `assert` / `assert_equal`, or through
   `cleanup_ran()`.
3. Reject fixtures put the accepted twin in the same file, next to the
   rejected line. The twin must produce no diagnostic, so the check fails if
   it is over-rejected.
4. Cancellation fixtures log the statements that follow the pending call.
   Because those entries are absent from the expected trace, a runtime that
   just runs `main!` to completion fails `cleanup_ran()`.
5. I ran every fixture through the reference parser
   (`audit/blind/tools/refparse.ts`). It reports 60/60 matching expectation.
   Where the reference parser rejected source that the spec allows, I
   rewrote the fixture to avoid the construct and recorded the conflict
   below (AMB-01 to AMB-04).

## Fixture count per area

| Area | Count |
| --- | --- |
| 1 evaluation order | 7 |
| 2 cancellation and defer | 8 |
| 3 requirement rows | 7 |
| 4 erasure boundaries | 8 |
| 5 identity | 6 |
| 6 permission weakening | 6 |
| 7 iterator invalidation | 6 |
| 8 defaults and copy-update | 5 |
| 9 strings | 7 |
| Total | 60 |

Phases: 1 parse, 12 type, 47 runtime (8 of them panic).

## Specification ambiguities (as questions)

Reference parser against spec text:

- **AMB-01.** Section 01 allows `${expression}` "with balanced nested
  delimiters". The reference parser rejects a string literal inside the
  braces (`"${f("y")}"`). Are nested string literals allowed in
  interpolation?
- **AMB-02.** The reference parser reports `argument-order` for
  `g(f(a=1, b=2), 3)`. It seems to apply the inner call's named arguments to
  the outer call. Is this a reference-parser defect?
- **AMB-03.** Section 11 `context_entries` allows entries in any order, and
  the text says "applied left to right, the later binding wins". Even so, the
  reference parser rejects `$.with(Tag=x, ...ctx)` with `argument-order`. Is
  a spread after an explicit binding legal?
- **AMB-04.** Section 01 defines `\u{HEX}` escapes. The reference lexer's
  valid-escape set has no `u`, so it rejects every Unicode escape, including
  `"\u{41}"`. Should the lexer accept `\u{...}` for scalar values?

Runtime and typing semantics:

- **AMB-05.** A cancel called from a `defer` suite can trigger
  `suspension-reentrant-poll`. Which source line does the panic report: the
  cancel call or the driving `!()`? The existing conformance fixture marks
  the driving line.
- **AMB-06.** `fixture-runtime-pending-function` holds a function "at its
  next poll". Does that apply only to the first call, or to every call?
  My fixtures call it once.
- **AMB-07.** When a payload-free variant is converted to `Any`, is it boxed
  freshly like a primitive, or does it keep its canonical identity like a
  heap composite?
- **AMB-08.** Is a variant with shared enum data but no payload
  (`StatusCode.NotFound`) canonical for `is`?
- **AMB-09.** Does "shortest round-trip decimal digits" for `f32` mean
  shortest for the `f32` width, so `0.1` rather than `0.10000000149011612`?
- **AMB-10.** What does `"".split(",")` return: `[""]` or `[]`? Only the
  empty-separator case is specified.
- **AMB-11.** For `replace` with an empty `old`, do the string's start and end
  count as scalar boundaries? That decides `"ab" -> "-a-b-"` and
  `"" -> "x"`.
- **AMB-12.** Does removing an absent map key count as a shape change that
  invalidates iterators?
- **AMB-13.** An iterator is exhausted, the collection then changes shape,
  and `next` is called. Does that panic, or return `nil`?
- **AMB-14.** Section 04 Mutable Paths requires "a mutable root expression".
  The permission table allows `list[mut User]` to mutate its elements. Is
  `users[0].name = x` or `box.value.name = x` legal through a readonly root
  whose generic argument is `mut`?
- **AMB-15.** Which diagnostic code applies when a parameter default refers
  to a later parameter? I assumed `binding-not-yet-visible`.
- **AMB-16.** A plain closure passes a captured `mut T` to a `mut T`
  parameter. Is the code `mutable-capture-requires-mut-fn` or
  `readonly-argument-to-mutable-parameter`? I assumed the first.
- **AMB-17.** For `let x: mut T = keep(readonly_value)` with a generic
  identity function, is the code `mutable-upgrade`? That is my assumption.
- **AMB-18.** Is a call that returns `mut T` a valid receiver for a field or
  index assignment place, as in `list_of(log, v)[i] = x`?
- **AMB-19.** Defaults must be pure, so the rule "defaults are evaluated
  after explicit arguments, in declaration order" can only be seen through
  data flow. Is any observable ordering intended?
- **AMB-20.** Can a `test` block, which has no declared row, call functions
  with requirements inside `$.with`, and bind closures there? I assumed yes.
- **AMB-21.** Each test block reads and writes top-level `let` storage only
  through helper functions. Do test blocks get the same top-level-binding
  access as functions?
- **AMB-22.** `pass_through(x)` with an expected `T?` could solve `T` as
  `i32` or as `i32?`. Is that an ambiguous inference? I avoided the case.
- **AMB-23.** Is a trait method that reassigns a top-level `let` (a
  side-effecting `Display.to_string`) legal? The interpolation-order fixture
  relies on it.
- **AMB-24.** The spec says "compatible composite reference types" for `is`.
  Are `list[User]` and `mut list[mut User]` compatible?
- **AMB-25.** Can a call like `readonly.produce()` select a field of
  function type? Section 05 says a member followed by arguments does method
  lookup. I read the field into a local first.

## Information rules followed

- I did not read, grep, or list `src/`, `bin/`, `examples/`, `guide/`,
  `future-work/`, or `test/` (other than `test/README.md`). I read no other
  `audit/` content beyond `PLAN.md` section 3.1 and `AGENT_BRIEF.md`.
- I ran no `hd` command, `npm test`, `npm run hd`, or anything else that
  invokes the compiler. The only tools run on fixtures were the reference
  parser (through `refparse.ts`) and `oxlint` on the script itself.
- I read `spec/reference-parser/lexer.ts` lines 48 and 90 to 125 only to
  diagnose AMB-04. That directory is on the allowed list.
- I made no git changes. The only git command run was the read-only
  `git rev-parse --short HEAD`, to record the commit.
