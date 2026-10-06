# Error-Code Proposal, 2026-10-06

**Status: proposal only.** Nothing here is applied or accepted behavior. It
covers the open part of the error-code revamp (task #101, task E1 Part B):
the remaining rows of
[Codes Waiting For The Code Revamp](../future-work/OPEN_ISSUES.md#codes-waiting-for-the-code-revamp),
rules that forbid something but name no code, one conflict, and the codes
the prototype emits that the spec never names. Every recommendation is
labeled and waits for the owner.

Base: `origin/main` at `8b589207` plus E1 Part A (SC-Q2 and SC-Q3 applied).
"Prototype" means the toy compiler in `src/` at that commit.

## Summary

| Count | Now | After this proposal |
| --- | ---: | ---: |
| Codes in the [Diagnostics](../spec/README.md#diagnostics) table | 219 | 224 |
| Distinct codes the prototype emits | 267 | 226 |
| Emitted codes the spec never names | 60 | 0 user-facing; 14 stay prototype-internal |

Part A already took the table from 229 to 219. This proposal adds 5 codes
to the table, folds 41 unnamed codes into existing ones, and marks 14 as
prototype-internal. It adds no other new code.

The five decisions that matter most:

1. **Finish the operator merge.** Unary `-` on a non-number still reports
   `invalid-unary-operand`; it should be `type-mismatch`, as SC-Q3 made
   unary `+`. Four more mismatch codes fold into `type-mismatch`.
2. **Parse and lexical errors use the general codes.** Seventeen
   `expected-*`, `empty-*`, and character codes fold into `syntax-error`,
   `invalid-token`, and `unterminated-string`.
3. **Module layout reuses module codes.** `src/mod.hd` is
   `reserved-module-name`, path clashes are `duplicate-module-name`, and
   reaching `tests` or crossing a root is `unknown-module`.
4. **Command-line usage errors keep no code**, as Cargo's and Go's do.
   File-located manifest errors reuse `invalid-requirement` and the other
   manifest codes.
5. **Five new table codes**, each for a mistake no existing code names:
   `uninhabited-binding`, `extra-trait-member`, `entry-point-parameters`,
   `duplicate-supertrait`, and `invalid-module-path`.

Probing found four prototype gaps, listed under
[Prototype Findings](#prototype-findings).

## A. Remaining Rows Of "Codes Waiting For The Code Revamp"

| Item | Rule | Current behavior | Proposed code | Reason |
| --- | --- | --- | --- | --- |
| AT-code | [`annot.walker.obligation.error`](../spec/lang/14-annotations.md#r-annot.walker.obligation.error), [`std-testing.arbitrary.derive.not-derivable`](../spec/std/testing.md#r-std-testing.arbitrary.derive.not-derivable) | Walker, describer, and source opt-ins report `member-not-derivable`. `@derive(Arbitrary)` reports `unsatisfied-trait-bound`, naming the member (`src/checker/program.ts`). | `member-not-derivable` for both; reword the std-testing rule. | One check, one code. Every other template already reports it at the opt-in. |
| LP-codes | [Let Patterns](../spec/lang/06-control-flow.md#let-patterns) | `refutable-let-pattern` and `let-else-falls-through` are in the table, in rules, and emitted. A pattern before `:=` reports `missing-let`. | Keep all three; close the row. | Already applied and specified. |
| TU2-code | [`types.tuple.no-mut`](../spec/lang/04-type-system.md#r-types.tuple.no-mut) | `mut-on-tuple` is in the table and a 04 rule; emitted. | Keep; close the row. | Already applied. |
| VA-type-code | [`fn.vararg.type.kinds`](../spec/lang/07-functions.md#r-fn.vararg.type.kinds) | The rule names `type-mismatch`; emitted. | Keep; close the row. | Already applied. |
| Q3-codes | [`data.embed.unique`](../spec/lang/08-data-and-enums.md#r-data.embed.unique), [`trait.by.invalid`](../spec/lang/09-traits.md#r-trait.by.invalid) | Each is the only rule naming its code (`duplicate-embedded-field`, `invalid-delegation`). | Keep both numbered; close the row. | Not a code question. |
| TR-code | [`types.tuple.rest.list`](../spec/lang/04-type-system.md#r-types.tuple.rest.list) | The rule names `type-mismatch`; emitted. | Keep; close the row. | Same reading as VA-type-code. |
| SINGLE-CODE | [`module.single-file.roots`](../spec/lang/10-modules.md#r-module.single-file.roots) | The rule names `unknown-module`; emitted. | Keep; close the row. | Already applied. |
| NEVER-BIND | none | `x := while true: return 5 else: 0` reports `uninhabited-binding`, "an inferred binding cannot have type never". | New table code `uninhabited-binding`, with a rule in Bindings. | `type-mismatch` misnames it: nothing is expected. `cannot-infer-type` misleads: the type is inferred. Rust accepts such a binding and warns that the code after it is unreachable; choosing that is a semantic change, not a code choice. |
| CLI-CODES, usage | [`cli.run.package-only`](../spec/cli/command-line.md#r-cli.run.package-only), [`cli.new.existing`](../spec/cli/command-line.md#r-cli.new.existing), [`cli.test.file-empty`](../spec/cli/command-line.md#r-cli.test.file-empty), [`cli.test.filter.none`](../spec/cli/command-line.md#r-cli.test.filter.none), [`cli.mode.member.unlisted`](../spec/cli/command-line.md#r-cli.mode.member.unlisted), and the other command-line errors of the row | Exit 101 with an uncoded message. | No code; keep exit 101 by [`cli.exit.hd-failure`](../spec/cli/command-line.md#r-cli.exit.hd-failure). | Cargo and `go` print usage errors as plain `error:` text; Cargo exits 101 ([Cargo exit status](https://doc.rust-lang.org/cargo/commands/cargo.html#exit-status)). No source line exists to locate. |
| CLI-CODES, manifest | two dependency keys with one `dep.NAME`; [`module.path.no-lib-dependency`](../spec/lang/10-modules.md#r-module.path.no-lib-dependency) | Exit 101, uncoded. | `invalid-requirement` | It already names a bad dependency entry in `hd.toml`. |
| CLI-CODES, applied | [`cli.exe.main-unlisted`](../spec/cli/command-line.md#r-cli.exe.main-unlisted), [`cli.task.name-clash`](../spec/cli/command-line.md#r-cli.task.name-clash) | `unlisted-entry` and `duplicate-executable-name`, in the rules and emitted. | Keep; drop them from the row. | Applied since the row was written. |
| CLI-CODES, layout | [`module.path.no-root-mod`](../spec/lang/10-modules.md#r-module.path.no-root-mod), [`module.test.integration.beside-dir`](../spec/lang/10-modules.md#r-module.test.integration.beside-dir), [`cli.task.beside-dir`](../spec/cli/command-line.md#r-cli.task.beside-dir) | `invalid-module-path` | See F5 below: `reserved-module-name` and `duplicate-module-name`. | Existing module codes name both mistakes. |
| CLI-CODES, uses | [`module.path.main-no-use`](../spec/lang/10-modules.md#r-module.path.main-no-use), [`cli.exe.entry-no-use`](../spec/cli/command-line.md#r-cli.exe.entry-no-use), [`module.test.cyclic-dev-unit`](../spec/lang/10-modules.md#r-module.test.cyclic-dev-unit) | `unknown-module` and `cyclic-test-dependency`, as applied. | Keep both names. | The row suggested renaming to `cyclic-dev-dependency`; a rename changes no behavior, and the name still fits its test-code use. |
| GR-24 | [`lex.indent.unknown-column`](../spec/lang/01-lexical-structure.md#r-lex.indent.unknown-column), [`lex.closure.between`](../spec/lang/01-lexical-structure.md#r-lex.closure.between) | `syntax-error` | `syntax-error`, with a Note under `lex.closure.between` that it wins. | The more specific rule applies, as the general-codes table says. |
| VIEW-CODE | [`std-collections.view.invalid-use`](../spec/std/collections.md#r-std-collections.view.invalid-use) | Panic `iterator-invalidated`. | Keep `iterator-invalidated`. | Java's `subList` throws the same exception as its iterator. A rename adds churn and no meaning. |
| VA-unbounded-code | [`fn.vararg.type.kinds`](../spec/lang/07-functions.md#r-fn.vararg.type.kinds) area | `generic-kind-mismatch` | Keep. | One rule covers both forms. |
| K1-code | [`annot.structure.find-key`](../spec/lang/14-annotations.md#r-annot.structure.find-key) | The rule names `unsatisfied-trait-bound`. | Keep. | Any failed bound reports it. |
| TY-29 | `trait-method-visibility`, `local-impl-nonlocal-pair`, `missing-partial-eq`, `duplicate-annotation-impl`, `overlapping-annotation-impl` | The first two now have rules and table rows. The last three occur nowhere in `spec/` or `src/`. | Close the row. | Nothing left to decide. |
| DEFAULT-CODE | [`std-ops.default.derive.one-variant`](../spec/std/ops.md#r-std-ops.default.derive.one-variant) | `invalid-default-variant`, in the table and emitted. | Keep; close the row. | Already applied. |
| RACE-PANIC | [`req.combinator.race-empty-run`](../spec/lang/11-requirements-and-suspension.md#r-req.combinator.race-empty-run) | The rule names `explicit-panic`. | Keep. | Same category as `chunks` with size 0. |
| SERDE-code | [`module.boundary.consent.error`](../spec/lang/10-modules.md#r-module.boundary.consent.error) | `boundary-private-field`, in the table and the rule. | Keep; close the row. | Already applied. |

## B. Rules That Forbid Something But Name No Code

"Current behavior" is what the prototype reports for a small probe of the
rule. Probes are in [Method](#method).

### F1, Data Types And Enums

| Item | Rule | Current behavior | Proposed code | Reason |
| --- | --- | --- | --- | --- |
| F1 | [`data.vis.literal`](../spec/lang/08-data-and-enums.md#r-data.vis.literal) | `private-member`: field 'balance' is not visible from this module | `private-member` | One visibility code for every private field. Rust splits literal and access (E0451, E0616); one hd code is simpler. |
| F1 | [`data.vis.private-fields`](../spec/lang/08-data-and-enums.md#r-data.vis.private-fields) | `private-member` | `private-member` | Same check. |
| F1 | [`data.shared.constructor`](../spec/lang/08-data-and-enums.md#r-data.shared.constructor) | `missing-variant-result`: variant 'NotFound' must initialize shared enum data | `missing-required-field` | A missing constructor leaves every shared field uninitialized; `data.shared.arguments` gets the same code. |
| F1 | [`data.shared.arguments`](../spec/lang/08-data-and-enums.md#r-data.shared.arguments) | `missing-required-field` | `missing-required-field` | Already emitted; a data literal's missing field uses it. |
| F1 | [`data.shared.payload-names`](../spec/lang/08-data-and-enums.md#r-data.shared.payload-names) | `duplicate-field` | `duplicate-field` | Same as [`data.field.unique`](../spec/lang/08-data-and-enums.md#r-data.field.unique). |
| F1 | [`data.shared.payload-defaults`](../spec/lang/08-data-and-enums.md#r-data.shared.payload-defaults) | `syntax-error`: expected ')', found '=' | `syntax-error` | The grammar's variant parameters take no `=`. |
| F1 | [`data.shared.payload-fields`](../spec/lang/08-data-and-enums.md#r-data.shared.payload-fields) | `unknown-data-field`: enum 'Shape' has no shared field 'radius' | `unknown-data-field` | The general-codes table already defines it for this. |
| F1 | [`data.enum.fn-value.shorthand`](../spec/lang/08-data-and-enums.md#r-data.enum.fn-value.shorthand) | `missing-contextual-enum-type`, as F1's fixture assumed | `missing-contextual-enum-type` | [`expr.name.contextual.no-type`](../spec/lang/05-expressions.md#r-expr.name.contextual.no-type) already names it. |

### F2, GADTs

| Item | Rule | Current behavior | Proposed code | Reason |
| --- | --- | --- | --- | --- |
| F2 | [`gadt.construct.no-explicit`](../spec/lang/13-gadts.md#r-gadt.construct.no-explicit) | Accepted: `Expr.Lit::[i64](1)` checks clean. | `argument-count` | Its meaning already covers a type-argument list longer than the generic parameters, here zero. |
| F2 | [`gadt.construct.ambiguous`](../spec/lang/13-gadts.md#r-gadt.construct.ambiguous) | `cannot-infer-type`: cannot infer `U` in `Expr[U]` | `cannot-infer-type` | An inference failure, as everywhere else. |
| F2 | [`gadt.existential.no-escape`](../spec/lang/13-gadts.md#r-gadt.existential.no-escape) | `type-mismatch`: expected i32, found S | `type-mismatch` | The escaping value meets a type it is not assignable to. |
| F2 | [`gadt.erasure.no-descriptor`](../spec/lang/13-gadts.md#r-gadt.erasure.no-descriptor) | `unsatisfied-trait-bound`: 'T' does not implement Inspectable | `unsatisfied-trait-bound` | The missing evidence is a failed bound. |

### F4, Traits

| Item | Rule | Current behavior | Proposed code | Reason |
| --- | --- | --- | --- | --- |
| F4 | [`trait.impl.extra-methods`](../spec/lang/09-traits.md#r-trait.impl.extra-methods) | `extra-trait-method` | New table code `extra-trait-member`, also for an extra associated type | No existing code names "not a member of the trait". Rust has E0407 and E0437 ([E0407](https://doc.rust-lang.org/error_codes/E0407.html), [E0437](https://doc.rust-lang.org/error_codes/E0437.html)); one hd code covers both. |
| F4 | [`trait.impl.local.inherent`](../spec/lang/09-traits.md#r-trait.impl.local.inherent) | `local-impl-nonlocal-pair` | `local-impl-nonlocal-pair` | Already in the table for the local-pair rules. |
| F4 | [`trait.impl.local.no-capture`](../spec/lang/09-traits.md#r-trait.impl.local.no-capture) | `unknown-name`: unknown name 'base' | `unknown-name` | The value is not in scope there. Rust's own code for this, E0434, is not worth a twin. |
| F4 | [`trait.default.no-self-members`](../spec/lang/09-traits.md#r-trait.default.no-self-members) | Accepted: `self.name` in a default method checks clean. | `unknown-method` | Its meaning covers member lookup that finds no method or field. |
| F4 | [`trait.assoc-call.trait.undetermined`](../spec/lang/09-traits.md#r-trait.assoc-call.trait.undetermined) | `cannot-infer-type`: could not infer Self of 'Make::make' | `cannot-infer-type` | An inference failure; the message already suggests `Type::f`. |
| F4 | [`trait.derive.newtype.not-inherited`](../spec/lang/09-traits.md#r-trait.derive.newtype.not-inherited) | The use reports its own code, as `type-mismatch` for `==` | No code of its own | It states a consequence; the use site's rule reports. |
| F4 | [`trait.decl.generic.invariant`](../spec/lang/09-traits.md#r-trait.decl.generic.invariant) | `type-mismatch`: expected Source[User], found Feed | `type-mismatch` | An unrelated instantiation is a mismatch. |
| F4 | [`trait.dyn.no-narrow`](../spec/lang/09-traits.md#r-trait.dyn.no-narrow) | `type-mismatch` | `type-mismatch` | No conversion applies. |
| F4 | [`trait.dyn.binding.convert`](../spec/lang/09-traits.md#r-trait.dyn.binding.convert), the mismatch side | `type-mismatch`: expected Source[Item=string], found Ints | `type-mismatch` | Same. |
| F4 | [`trait.erase.child-impl`](../spec/lang/09-traits.md#r-trait.erase.child-impl) | `type-mismatch`: expected Plugin, found Echo | `type-mismatch` | Same as any conversion to a trait value without an implementation. |
| F4 | [`trait.own.inherent.std.no-tuple`](../spec/lang/09-traits.md#r-trait.own.inherent.std.no-tuple) | `unknown-method`: type '(i32, i32)' has no supported method 'len' | `unknown-method` | Member lookup finds nothing. |
| F4 | [`trait.decl.typed`](../spec/lang/09-traits.md#r-trait.decl.typed) | `syntax-error`: expected ':' | `syntax-error` | The grammar requires the type. |

### F5, Modules

| Item | Rule | Current behavior | Proposed code | Reason |
| --- | --- | --- | --- | --- |
| F5 | [`module.path.no-root-mod`](../spec/lang/10-modules.md#r-module.path.no-root-mod) | `invalid-module-path` | `reserved-module-name` | Same shape as [`module.path.reserved-pkg`](../spec/lang/10-modules.md#r-module.path.reserved-pkg): a reserved name directly under the source root. |
| F5 | [`module.path.unique`](../spec/lang/10-modules.md#r-module.path.unique) | `duplicate-module-path` | `duplicate-module-name` | [`names.module.unique`](../spec/lang/03-names-and-scopes.md#r-names.module.unique) already names one module name declared twice. |
| F5 | [`module.path.case-collision`](../spec/lang/10-modules.md#r-module.path.case-collision) | `duplicate-module-path` | `duplicate-module-name` | Same clash, after case folding. |
| F5 | [`module.test.integration.beside-dir`](../spec/lang/10-modules.md#r-module.test.integration.beside-dir) | `invalid-module-path` | `duplicate-module-name` | `tests/common.hd` and `tests/common/` claim one name. `cli.task.beside-dir` follows. |
| F5 | [`module.test.no-tests-root`](../spec/lang/10-modules.md#r-module.test.no-tests-root) | Accepted: `use tests.common` in a test module passes. | `unknown-module` | `tests` names no use root. |
| F5 | [`module.relative.above-root`](../spec/lang/10-modules.md#r-module.relative.above-root) | `unknown-module`: 'super' moves above the package root | `unknown-module` | As a `super` above the test root already is. |
| F5 | [`module.relative.no-cross`](../spec/lang/10-modules.md#r-module.relative.no-cross) | `unknown-module`: no package module 'user.std' | `unknown-module` | The relative path names no module. |
| F5 | [`module.path-dep.no-package`](../spec/lang/10-modules.md#r-module.path-dep.no-package) | `invalid-requirement`: money holds no hd.toml | `invalid-requirement` | Already emitted for a bad path requirement. |
| F5 | a `pub main` with parameters, [`module.entry.definition`](../spec/lang/10-modules.md#r-module.entry.definition) | `entry-point-parameters`: public main cannot declare source-level parameters | New table code `entry-point-parameters`, also for type parameters | Go rejects `func main` with arguments; Rust has E0131 for generic `main` ([E0131](https://doc.rust-lang.org/error_codes/E0131.html)). No hd code names it. |
| Probe | [`module.path.identifier`](../spec/lang/10-modules.md#r-module.path.identifier), found while probing | `invalid-module-path`, as for `src/my-file.hd` | New table code `invalid-module-path` | No existing code names a file whose path is no module path. |

## C. Conflict: A Child Module Reached Through Its Parent

| Item | Rule | Current behavior | Proposed code | Reason |
| --- | --- | --- | --- | --- |
| Conflict | [`expr.name.qualified.missing`](../spec/lang/05-expressions.md#r-expr.name.qualified.missing) gives `unknown-import`; the example under [`module.path.no-std-child-import`](../spec/lang/10-modules.md#r-module.path.no-std-child-import) says `unknown-type` | `typing/invalid/child-module-not-in-parent.hd` expects and gets `unknown-import` for `user.types.User`. `typing/invalid/std-child-path-through-parent.hd` expects and gets `unknown-type` for `testing.arbitrary.With`. | `unknown-import` for both | The rule says `std` follows the package rule. Fixing it touches the example comment, one fixture marker, and the prototype's std path message. |

## D. Codes The Prototype Emits That The Spec Never Names

Regenerated with the method of the 2026-10-04 sweep
(`git show a3f8e3b3:audit/job2-error-codes-spec-vs-compiler.md`). The 70
codes of that sweep are now 60: 10 have left `src/` since, among them the
two respellings `duplicate-data-field` and `mismatched-delimiter`. No new
unnamed code appeared.

### Merge Into An Existing Code (41)

| Item | Rule | Current behavior | Proposed code | Reason |
| --- | --- | --- | --- | --- |
| `invalid-unary-operand` | [`expr.op.no-impl`](../spec/lang/05-expressions.md#r-expr.op.no-impl) | Unary `-` on a non-number, as `-"x"` | `type-mismatch` | SC-Q3 made unary `+` and unsigned `-` report it; `Neg` is an operator trait. |
| `associated-type-mismatch` | [`trait.binding`](../spec/lang/09-traits.md) area | A projection resolves to another type | `type-mismatch` | A type is not the one required. |
| `pattern-type-mismatch` | [Patterns](../spec/lang/06-control-flow.md) | A pattern invalid for the subject's type | `type-mismatch` | Same. |
| `provider-type-mismatch` | [Requirements](../spec/lang/11-requirements-and-suspension.md) | A provider binding given a non-provider value | `type-mismatch` | Same. |
| `tuple-binding-annotation` | [Let Patterns](../spec/lang/06-control-flow.md#let-patterns) | A tuple binding annotated with a non-tuple type | `type-mismatch` | Same. |
| `expected-expression` | [Grammar](../spec/lang/02-grammar.md) | A token where an expression must start | `syntax-error` | Rust's and Go's parsers report each "expected X" as one syntax error class. |
| `expected-comprehension-clause` | Grammar | A comprehension without `for`, `if`, or `=>` | `syntax-error` | Same. |
| `expected-interpolation-end` | Grammar | An interpolation that does not close | `syntax-error` | Same. |
| `expected-pattern` | Grammar | `-` in a pattern before a non-number | `syntax-error` | Same. |
| `interpolated-pattern` | Grammar | A string pattern with interpolation | `syntax-error` | Same. |
| `empty-enum` | Grammar, `enum_decl` | An enum with no variant | `syntax-error` | The production requires one variant. |
| `empty-match` | Grammar, `match_expression` | A match with no arm | `syntax-error` | The production requires one arm. |
| `empty-suite` | Grammar | An empty `tests:` block | `syntax-error` | Same. |
| `missing-method-body` | Grammar | An implementation method without a body | `syntax-error` | Same. |
| `duplicate-mutable-permission` | Grammar | `mut mut T` | `syntax-error` | Same. |
| `reserved-name` | [`lex.ident.reserved`](../spec/lang/01-lexical-structure.md#r-lex.ident.reserved) | `self` or `Self` as a parameter name | `syntax-error` | A reserved word is no identifier token. |
| `unexpected-character` | [Lexical Structure](../spec/lang/01-lexical-structure.md) | A character that starts no token | `invalid-token` | Its table meaning is exactly this. |
| `bare-carriage-return` | [`lex.line.bare-cr`](../spec/lang/01-lexical-structure.md#r-lex.line.bare-cr) | A lone CR | `invalid-token` | The rule calls it a lexical error. |
| `identifier-not-nfc` | [`lex.ident.non-nfc`](../spec/lang/01-lexical-structure.md#r-lex.ident.non-nfc) | A non-NFC identifier | `invalid-token` | Same. |
| `invalid-character-literal` | Lexical Structure | A character literal with more than one scalar | `invalid-token` | The text forms no valid literal token. |
| `unterminated-string-interpolation` | Lexical Structure | `${` without its `}` | `unterminated-string` | The string never closes. |
| `continue-outside-loop` | [General codes](../spec/README.md#diagnostics) | `continue` outside a loop | `break-outside-loop` | The table already says it covers `continue`. Rust uses one code, E0268, for both ([E0268](https://doc.rust-lang.org/error_codes/E0268.html)). |
| `closure-result-needs-annotation` | [Functions](../spec/lang/07-functions.md) | A closure result not inferred | `cannot-infer-type` | An inference failure. |
| `ambiguous-bound-method` | [`trait.resolve.ambiguous`](../spec/lang/09-traits.md#r-trait.resolve.ambiguous) | Two bounds supply one method name | `ambiguous-method` | Same ambiguity, reached through bounds. |
| `generic-kind-conflict` | [Requirement rows](../spec/lang/11-requirements-and-suspension.md) | A parameter used as a type and a row | `generic-kind-mismatch` | Same kind error. |
| `generic-arity` | [Type System](../spec/lang/04-type-system.md) | A supertrait with the wrong type-argument count | `argument-count`, or `partial-generic-arguments` when too few | Their table meanings cover both directions. |
| `unexpected-type-arguments` | Type System | Type arguments on a non-generic callee | `argument-count` | More type arguments than generic parameters. |
| `unsupported-generic-impl` | [`types.generic.default.written-missing`](../spec/lang/04-type-system.md#r-types.generic.default.written-missing) | `impl Box:` for a generic `Box` | `partial-generic-arguments` | A spec error, misnamed "unsupported". |
| `duplicate-generic-parameter` | Type System | One generic parameter name twice | `duplicate-type` | A type name declared twice in one scope. Rust's E0403 is the same check. |
| `duplicate-impl-member` | [`trait.decl.unique`](../spec/lang/09-traits.md#r-trait.decl.unique) | An implementation binds one member twice | `duplicate-trait-member` | Rust reports duplicate impl items as E0201 ([E0201](https://doc.rust-lang.org/error_codes/E0201.html)). |
| `duplicate-variant-pattern-field` | [`flow.match.data.fields.duplicate`](../spec/lang/06-control-flow.md#r-flow.match.data.fields.duplicate) | A payload field listed twice in a pattern | `duplicate-data-pattern-field` | Same mistake on a variant. |
| `duplicate-module-path` | [`module.path.unique`](../spec/lang/10-modules.md#r-module.path.unique) | Two files for one module | `duplicate-module-name` | See F5. |
| `extra-trait-method` | [`trait.impl.extra-methods`](../spec/lang/09-traits.md#r-trait.impl.extra-methods) | A method the trait does not declare | `extra-trait-member` (new) | See F4. |
| `generic-entry-point` | [`module.entry.definition`](../spec/lang/10-modules.md#r-module.entry.definition) | A generic `main` | `entry-point-parameters` (new) | Type parameters are parameters. |
| `missing-associated-type` | [`trait.impl.required`](../spec/lang/09-traits.md#r-trait.impl.required) | An implementation that binds no associated type | `missing-trait-method` | Rust reports every missing item as E0046 ([E0046](https://doc.rust-lang.org/error_codes/E0046.html)). |
| `missing-variant-result` | [`data.shared.constructor`](../spec/lang/08-data-and-enums.md#r-data.shared.constructor) | A variant without its constructor | `missing-required-field` | See F1. |
| `member-on-non-data` | [`expr.assign.target`](../spec/lang/05-expressions.md#r-expr.assign.target) | Assigning a field of a non-data value | `invalid-assignment-target` | The target is no place. |
| `named-argument-needs-declaration` | [`fn.arg.unknown-name`](../spec/lang/07-functions.md#r-fn.arg.unknown-name) | A named argument to a callable value | `unknown-named-argument` | No declaration gives the name. |
| `qualified-receiver-position` | [`grammar.call.positional-first`](../spec/lang/02-grammar.md#r-grammar.call.positional-first) | A trait-qualified receiver not first | `argument-order` | An argument in the wrong position. |
| `recursive-trait-dictionary` | [`trait.bound.depth.limit`](../spec/lang/09-traits.md#r-trait.bound.depth.limit) | A dictionary that needs itself | `trait-resolution-depth` | A proof that does not end. |
| `unknown-tuple-member` | General codes | A tuple member that does not exist | `unknown-method` | Member lookup finds nothing. |

### Keep And Specify (5 New Table Codes)

| Item | Rule | Current behavior | Proposed code | Reason |
| --- | --- | --- | --- | --- |
| `uninhabited-binding` | new, Bindings | A binding inferred as `never` | Keep | See NEVER-BIND. |
| `extra-trait-member` | [`trait.impl.extra-methods`](../spec/lang/09-traits.md#r-trait.impl.extra-methods) | An associated type the trait does not declare | Keep; absorbs `extra-trait-method` | See F4. |
| `entry-point-parameters` | [`module.entry.definition`](../spec/lang/10-modules.md#r-module.entry.definition) | A `pub main` with parameters | Keep; absorbs `generic-entry-point` | See F5. |
| `duplicate-supertrait` | [Supertraits](../spec/lang/09-traits.md) | One supertrait listed twice | Keep | `duplicate-trait-member` names members, not supertraits. |
| `invalid-module-path` | [`module.path.identifier`](../spec/lang/10-modules.md#r-module.path.identifier) | A file whose path is no module path | Keep, narrowed to this rule | No existing code names it; `src/mod.hd` and beside-dir files leave it. |

### Prototype-Internal (14)

None of these reaches a conforming program as a spec error. Each is either
a feature the toy does not implement, or a REPL message kind.

| Item | Rule | Current behavior | Proposed code | Reason |
| --- | --- | --- | --- | --- |
| `unsupported-bound-associated-call` | none | A suspending associated function through a bound | none; prototype gap | The spec allows it. |
| `unsupported-context-operation` | none | A `$.` operation not built | none; prototype gap | Same. |
| `unsupported-derivation` | none | A walker on a local without its type | none; prototype gap | Same. |
| `unsupported-host-provider-signature` | none | A generic host capability method | none; prototype gap | A consent failure already reports `boundary-private-field`. |
| `unsupported-local-default` | none | A local function parameter default | none; prototype gap | Same. |
| `unsupported-local-generic-function` | none | A generic local function | none; prototype gap | Same. |
| `unsupported-local-vararg` | none | A variadic local function | none; prototype gap | Same. |
| `unsupported-match-subject` | none | A subject type the toy cannot match | none; prototype gap | Same. |
| `unsupported-nested-variant-pattern` | none | A nested pattern the toy cannot lower | none; prototype gap | Same. |
| `unsupported-type-form` | none | A type form the toy does not parse | none; prototype gap | Same. |
| `package-name-collision` | [`cli.exe.unselected-main`](../spec/cli/command-line.md#r-cli.exe.unselected-main) | A `main` in a non-entry module is an error | none; prototype bug | The spec makes it an ordinary function with an `unselected-main` warning; the toy reports both. |
| `internal-enum-literal` | none | Generated code only | none | Unreachable from source. |
| `internal-error` | none | A REPL message kind | none | Not a diagnostic. |
| `runtime-panic` | none | A REPL message kind | none | Not a diagnostic; panics have their own categories. |

## Prototype Findings

Probing found four places where the prototype and the spec disagree. They
are facts for KNOWN_ISSUES, not part of the code choice.

| Rule | Spec | Prototype |
| --- | --- | --- |
| [`gadt.construct.no-explicit`](../spec/lang/13-gadts.md#r-gadt.construct.no-explicit) | `Expr.Lit::[i64](1)` is an error | Accepts it. |
| [`trait.default.no-self-members`](../spec/lang/09-traits.md#r-trait.default.no-self-members) | `self.name` in a default method is an error | Accepts it. |
| [`module.test.no-tests-root`](../spec/lang/10-modules.md#r-module.test.no-tests-root) | `use tests.common` is an error | Accepts it. |
| [`cli.exe.unselected-main`](../spec/cli/command-line.md#r-cli.exe.unselected-main) | A `main` in another module is ordinary | Reports `package-name-collision`. |

## Method

- **Table count:** the codes of the Error, Warning, and Boundary failure
  rows of [Diagnostics](../spec/README.md#diagnostics).
- **Spec names:** that table, plus every `Error:`, `Warning:`, or `Panic:`
  code and `# error:` marker in `spec/lang`, `spec/std`, and `spec/cli`,
  plus fixture `# diagnostic:` markers and `cases.tsv` `reject:` codes.
- **Emitted codes:** first arguments of `fail(`, `report(`, `error(`,
  `warn(`, and `unsupported(`; path-first `report(path, "...")`; `code:`
  literals; and both branches of a ternary in those positions, all in
  `src/`. Four ternary hits are HIR kinds, not codes (`map-index`,
  `map-remove`, `result-ok`, `result-error`). A scan of every other
  hyphenated string literal in `src/` found no further code.
- **Probes:** for each rule in Part B, a small program run with
  `hd check --tests`, or `hd check` and `hd test` in a scratch package.
  The quoted messages are the prototype's first diagnostic.
