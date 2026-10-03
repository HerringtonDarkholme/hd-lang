# Source, Modules, And Generated Code: First Pass

Status: preliminary audit evidence; nothing in this report is accepted behavior or an owner decision.

Baseline: `823f346878028aad4a4c9351593217f04445bd4c`. Review performed in the isolated audit worktree. Evidence is source, specification, and fixture inspection. No tests, checks, compiler probes, installation, fetch, or rebase were executed.

Under review: [Names and Scopes](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/lang/03-names-and-scopes.md), [Use Declarations](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/lang/02-grammar.md#use-declarations), [Modules and Packages](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/lang/10-modules.md), [Typed Derivation](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/lang/14-annotations.md#typed-derivation), [Testing](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/std/testing.md), and the tier direction in [AGENTS.md](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/AGENTS.md#spec-scope-for-the-standard-library). The applied-decision ledger in [KNOWN_ISSUES.md](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/KNOWN_ISSUES.md#applied-decisions-the-prototype-does-not-follow-yet) and [known failures](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/test/portable/KNOWN_FAILURES.tsv) supplied existing-gap context. Historical owner records have not yet been independently reconstructed.

## Actual Pass Boundaries

Package linking parses each file, validates selected imports, orders reachable modules, edits their source, and concatenates them. The remainder of the compiler receives one module with no declaration-level original module identity. See [package.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/package.ts), lines 193–518 and 528–562.

The checker validates std imports, lowers error derivation, rewrites module calls and decorators, lowers typed derivation, and finally joins std sources. Local declaration hoisting, type defaults, aliases, declaration preparation, and ordinary expression checking follow. See [program.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/program.ts), lines 51–77 and 125–175.

Std sources take another pipeline: intrinsic text expansion, parsing, textual renaming, reparsing, origin tagging, respanning, and AST concatenation. Structure and inspection declarations enter through separate checker passes. See [standard-sources.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/standard-sources.ts), lines 40–64; [standard-library.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/standard-library.ts), lines 149–170 and 678–727; and [standard-traits.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/standard-traits.ts), lines 66–79.

Typed derivation builds source strings with placeholder names, parses them, then recursively patches expressions, types, and source spans. This occurs before ordinary name resolution and expression typing. See [generated-source.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/generated-source.ts), lines 24–32 and 55–84.

## Ranked Findings

| ID | Classification | Priority | Evidence strength |
| --- | --- | --- | --- |
| SOURCE-1 | Architectural correctness defect: module identity disappears | High | Explicit implementation contract and matching known failures |
| SOURCE-2 | Compiler defect: module-call rewriting ignores shadowing | High | Direct static trace; runtime reproduction outstanding |
| SOURCE-3 | Compiler defect: generated type placeholders rewrite user member names | High | Direct static trace; compiler reproduction outstanding |
| SOURCE-4 | Compiler defect and architectural risk: std binding is textual renaming | Medium | Alias loss follows directly; broader spelling risks not reproduced |
| SOURCE-5 | Spec defects: incompatible test rules and stdlib tier leakage | High | Conflicting normative passages and existing fixture |

### SOURCE-1: The Package Linker Erases The Information Needed For Correct Modules

[package.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/package.ts), lines 14–31, explicitly says linked modules share one namespace and private declarations become accessible without imports. Lines 416–456 reject duplicate declaration spellings across modules. Lines 482–562 delete package imports and join source text.

Consequences include false acceptance of private cross-module access, false rejection of unrelated names in separate modules, and test-name uniqueness across the entire linked program. Integration tests also see library private names and test code. These are semantic changes caused by the representation, rather than missing optimization.

The rules require module-private declarations, public-only access from another module, module-local test-name uniqueness, and integration tests built against public library declarations without test code. See `module.vis.private-default`, `module.vis.no-package-private`, `module.testing.it.unique`, and `module.test.integration.view` in [Modules and Packages](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/lang/10-modules.md), lines 294, 874, 1137, and 1179.

The same representation fixes initialization groups into whole-file order. [package.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/package.ts), lines 565–601, sorts each strongly connected group by identity without scheduling individual statements. `module.init.group.dependency` and `module.init.group.step` require dependency-ready statements across modules. The existing [init-group-order fixture](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/conformance/runtime/valid/init-group-order.hd) and its [prices module](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/conformance/trees/init-group/src/shop/prices.hd) require catalog initialization to wait for prices' binding. This is already recorded as DC7 in [known failures](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/test/portable/KNOWN_FAILURES.tsv).

Package alias and namespace imports are explicitly rejected with `unsupported-package-use` at lines 274–305. Explicit rejection is preferable to executing changed semantics, but it does not make the module implementation conforming. The module root and relative-path changes recorded as ROOTS and SELF-CURRENT are additional known deviations, not newly reproduced findings.

Architecture implication: retain a program of modules, declaration ownership, import bindings, and source-file identities through resolution. Whole-module flattening can happen only after names and initialization ordering have been resolved into explicit identities and operations.

### SOURCE-2: Std Module Calls Are Rewritten By Receiver Spelling Before Resolution

[standard-library.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/standard-library.ts), lines 607–647, collects imported module aliases into a program-wide map. A generic object walk rewrites every call with a member receiver whose name occurs in that map. It tracks no parameters, bindings, lexical scopes, or shadowing.

[program.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/program.ts), lines 67–74, executes this rewrite before declaration preparation and ordinary expression checking. A function parameter named `arbitrary`, with a method named `with`, therefore loses its method call whenever the module also imports `std.testing.arbitrary`. Its call becomes a call of `__std_testing_arbitrary_with`, regardless of the parameter's actual type.

`names.scope.lexical`, `names.scope.shadow`, and `names.category.syntax.value` require the receiver to resolve in its lexical value scope. See [Names and Scopes](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/lang/03-names-and-scopes.md), lines 21 and 73–85. `arbitrary` is not a prelude name, so the separate prelude shadowing prohibition cannot justify this rewrite.

The [derived-arbitrary-with fixture](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/conformance/runtime/valid/derived-arbitrary-with.hd) exercises an unshadowed namespace call. It provides positive coverage for the rewritten shape, but no evidence for preserving lexical resolution when that spelling names a local value.

Architecture implication: resolve whether the receiver is a module or a local value before changing call form. The lowered callee should carry the resolved declaration identity.

### SOURCE-3: Type Placeholders Can Capture Legal User Field And Variant Names

[generated-source.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/generated-source.ts), lines 29–32, issues type placeholder spellings `HDTYPE0X`, `HDTYPE1X`, and so on. Lines 69–72 replace these substrings in every AST string property except a property named `value`. There is no check that the string is a type position or an issued placeholder node.

[typed-derivation.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/typed-derivation.ts), line 1033, assigns `HDTYPE0X` to the target's type. Lines 1123–1128 interpolate the user's field access spelling into generated getters. Lines 1257–1266 similarly interpolate user field and enum variant names into generated constructors.

A legal user field named `HDTYPE0X` therefore enters the parsed generated getter as its member `name` and is replaced with the target type string. An enum variant with that spelling suffers the same collision. The original user declaration is not patched equivalently, so member lookup subsequently refers to a different name.

`lex.ident.form` and `lex.ident.case` permit that identifier. `annot.walk.members` and `annot.handle.get` require the original member and its ordinary field semantics. See [Identifiers](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/lang/01-lexical-structure.md#identifiers) and [Walk](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/lang/14-annotations.md#walk).

This is a statically traced defect, not an executed reproducer. Generated `hdexprN` replacement also uses textual recognition at lines 75–78, but no concrete user expression collision has been established here. That mechanism remains a separate hygiene review lead.

Architecture implication: construct typed placeholder nodes or ordinary AST nodes directly. Generated identifiers need a separate identity domain rather than distinctive, legal source spellings.

### SOURCE-4: Std Declaration Identity Is Replaced With Textual Import Names

[standard-library.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/standard-library.ts), lines 491–515, maps each qualified std declaration to one local name. Two imports of the same declaration under distinct aliases overwrite the map entry. The std declaration is then physically renamed to the last alias at lines 682–690.

The import validator in [program-validation.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/program-validation.ts), lines 27–50, rejects duplicate local names, not multiple aliases of one origin. Two aliases of `std.cmp.min` therefore retain distinct entries in `imports`, but only one actual function declaration is joined.

[context.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/context.ts), lines 1429–1431, looks up function signatures by local spelling alone. [expression-calls.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/expression-calls.ts), lines 1154–1159, reports an unknown function for the alias that no longer has a signature. It does not follow the alias's recorded qualified origin.

The [Use Declarations grammar](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/lang/02-grammar.md#use-declarations), lines 907–920, permits aliases, and `names.module.unique` prohibits duplicate local declaration names. No reviewed rule prohibits two distinct local names for the same declaration. The existing [std alias test](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/test/std.test.ts), lines 121–129, covers only one alias and confirms that renaming currently replaces a declaration's local spelling.

Architecture implication: import bindings must point to a canonical declaration. An alias adds a binding; it must not rename the declaration itself. The final observable diagnostic still needs an executed probe.

#### Additional Architectural Risk: Undeclared Std Spelling Conventions

[standard-library.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/standard-library.ts), lines 249–262, explicitly assumes std top-level names never coincide with fields, parameters, or locals. Its regex substitutes by text, skipping immediate dotted names, associated names, and a name directly after a quote. It cannot distinguish declarations from lexical references or identify the interior of string literals and comments.

Thus a renamed std name appearing after whitespace inside a literal is eligible for replacement. A coincidentally named local binding is renamed together with unrelated references. Unicode token boundaries also differ from its ASCII `\\w` boundaries. No current literal corruption was found in the inspected std sources, so this finding is an architectural risk, not a demonstrated current runtime defect.

[intrinsic-methods.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/intrinsic-methods.ts), lines 71–114, expands numeric implementations and supplies intrinsic bodies using exact line shapes and indentation. Compiler-provided primitive operations are legitimate under the language tier. Depending on exact formatting and spelling to recognize their declarations is the avoidable shortcut.

Std joining itself is not automatically wrong: using ordinary hd implementations and a small primitive ABI is compatible with a naive correct compiler. The defect risk comes from binding changes through textual substitution and the loss of module ownership. The loader's `standardName` tag for annotation lang items, at [standard-library.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/standard-library.ts), lines 291–307, is a legitimate recognition requirement; qualified recognition is specified by `annot.target.recognized`.

Architecture implication: parse once, resolve imports, and lower recognized intrinsic declarations structurally. Preserve original source spans alongside generated origins. The current respanning of every std node to the importing `use`, lines 265–271 and 694–718, makes internal std failures hard to locate.

### SOURCE-5: The Testing Specification Conflicts With Itself And Its Tier Direction

[Grammar](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/lang/02-grammar.md), line 71, says each test-block statement must call the prelude `it`. [Modules](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/lang/10-modules.md), lines 872–873, allows other stdlib registration functions and restricts all such functions to direct test-position calls. [Testing](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/std/testing.md), lines 47–56, specifically permits `it_each`, `it_prop`, and `it_prop_with`.

These normative requirements disagree. The [derived-arbitrary-with fixture](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/conformance/runtime/valid/derived-arbitrary-with.hd), lines 16–23, uses `it_prop` directly. The parser implements the broader registration rule in [test-cases.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/parser/test-cases.ts), lines 393–435. Its acceptance cannot be labeled a compiler conformance defect until the contradictory grammar rule is resolved.

There is also a tier inconsistency. AGENTS.md puts a compiler-checked test-position rule, a diagnostic code about an item, and compiler knowledge of an item's name in the language tier. `std-testing.registration`, `.it-each.name-clash`, and `.variants.name` put those properties in the stdlib tier. `module.testing.position-statements` further makes a numbered language rule depend on the stdlib registration list.

The implementation explicitly knows the three names in [standard-uses.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/standard-uses.ts), line 34, and special-cases their syntax, options, result typing, and test metadata in [test-cases.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/parser/test-cases.ts), lines 298–390 and 393–435. That compiler knowledge is presently necessary to enforce the specified restrictions; simply deleting it would change behavior.

Owner question: should registration identity and the rules the compiler enforces live in the language tier, with generation and shrinking in stdlib, or should registration become ordinary library code through an existing language mechanism? The latter requires evidence that it preserves static listing, direct-call restrictions, and fresh instances. This report selects neither answer.

## Other Important Review Leads

Typed derivation claims templates are checked once in comments, while `annot.template.checked` specifies checking the instantiated implementation at its opt-in. The code also has an explicit `unsupported-derivation` rejection when a traversal visitor is not a local with an evident declared type. See [template-instances.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/template-instances.ts), lines 99–112 and 245–253. Rejection is a documented implementation limitation; proving whether template checks occur at the correct stage needs a broader checker review.

`localType` scans all nested objects and takes the first binding with matching spelling, without lexical scope or source-order resolution. `protocolError` selects by bare target head before full type matching. Both deserve composition probes for shadowing and multiple generic implementations. These were not promoted to confirmed findings in this pass.

The typed derivation header documents representation shortcuts for fact erasure, shared variant fields, handle permissions, and package-wide warning behavior. See [typed-derivation.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/typed-derivation.ts), lines 78–86. The permission rule is particularly important because `annot.handle.get.views` forbids upgrading readonly member access. This pass traced the generated-code mechanism but did not prove its final checker/runtime behavior.

## Coverage And Next Evidence

Reviewed-file inventory:

| Depth | Files |
| --- | --- |
| Full file | `src/checker/standard-library.ts`, `src/checker/standard-sources.ts`, `src/checker/generated-source.ts`, `src/checker/intrinsic-methods.ts`, `src/parser/test-cases.ts`, `src/checker/standard-uses.ts`, `src/checker/standard-traits.ts` |
| Complete relevant section | `src/package.ts` lines 1–605; `src/checker/program.ts` pass pipeline lines 1–175; `src/checker/program-validation.ts` import handling lines 27–51 |
| Selected implementation sections | `src/checker/typed-derivation.ts`, `src/checker/template-instances.ts`, `src/checker/program-declarations.ts`, `src/checker/context.ts`, `src/checker/expression-calls.ts` |
| Selected spec and evidence sections | `spec/lang/01-lexical-structure.md`, `spec/lang/02-grammar.md`, `spec/lang/03-names-and-scopes.md`, `spec/lang/10-modules.md`, `spec/lang/14-annotations.md`, `spec/std/testing.md`, `src/README.md`, `src/KNOWN_ISSUES.md`, `test/portable/KNOWN_FAILURES.tsv`, `test/std.test.ts`, `test/package.test.ts`, and the fixtures linked above |

Outstanding: folder graph details beyond initialization ordering, error derivation internals, tuple template instantiation, complete annotation and typed-fact semantics, generated-name collisions outside placeholder substitution, module ownership in trait coherence, and exhaustive fixture coverage. SOURCE-2 through SOURCE-4 need minimized compiler probes before their observable effects receive final defect status. SOURCE-1 and SOURCE-5 already have direct documentary evidence, but no known-failure row was revalidated by execution here.

No new hd code examples were authored, so no parse log or hd-writing-log entry applies. No source, spec, fixture, or existing test file was changed.
