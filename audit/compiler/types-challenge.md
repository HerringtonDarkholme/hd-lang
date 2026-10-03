# Independent Challenge Of Types Findings

> Historical baseline review. Fixed findings remain here only as original evidence; use [REPORT.md](REPORT.md) and [findings.tsv](findings.tsv) for remaining work.

Status: preliminary audit evidence; no behavior or recommendation here is accepted. This challenges [types-first-pass.md](types-first-pass.md), read in full, against baseline `823f346878028aad4a4c9351593217f04445bd4c`.

Scope: original findings T2–T5, [Method Resolution](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/lang/09-traits.md#method-resolution), [Variance](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/lang/04-type-system.md#variance), [Least Common Type](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/lang/04-type-system.md#least-common-type), and [Local Implementations](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/lang/03-names-and-scopes.md#local-implementations). Evidence is static inspection only. No compiler probe, check, or newly authored hd program was run.

## Challenge Results

| Original ID | Result | Required correction or qualification |
| --- | --- | --- |
| T2 | Mechanism confirmed | Applies to multiple candidate instantiations, not all calls of generic traits; trial state corruption is unproved |
| T3 | Missing validation confirmed | Public readonly inherent methods establish the gap without resolving private-method scope; runtime unsoundness is not demonstrated |
| T4 | Missing general LCT confirmed; scope broader than reported | Lists/maps lack general candidate generation; unannotated control-flow joins use a different, more restrictive path |
| T5 | Lost implementation extent confirmed | Local type/trait spellings remain scoped; the leak concerns implementation availability for identities visible at the affected call |

### T2: The Syntax Blacklist Really Runs Before Candidate Fit

[expression-calls.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/expression-calls.ts), lines 594–647, constructs candidates by receiver type, implementation target, trait, and method. It does not first filter those candidates by the call arguments or expected result. Lines 756–764 take the single-candidate path immediately, but reject multiple candidates when their traits differ or their arguments fail the speculation-safe check.

[call-speculation.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/call-speculation.ts), lines 7–20 and 40–47, recursively rejects the expression kinds named in the first report. Therefore a control expression with a unique, noncontextual argument type still cannot select one of multiple instantiations of the same trait. Expected-result typing cannot rescue that case: its fit check occurs later, during trials at lines 770–780.

The attempted falsification was whether the blacklist only rejects inherently ambiguous calls, or whether expected types eliminate candidates before it. Neither is supported by this path. `trait.resolve.fits`, `.fits.expected`, and `.one-fit` in [Traits](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/lang/09-traits.md), lines 1047–1049, require checking fit; their criteria contain no expression-shape restriction.

Qualification: a single candidate accepts these expression shapes through ordinary argument checking. Inherent calls and other dispatch paths need separate review. The report should say “selection among multiple instantiations of one generic trait,” rather than imply generic trait dispatch as a whole is syntax-restricted.

The diagnostic saying “multiple traits” is statically misleading when all candidates share one trait. Only diagnostic length is restored by the local trial function, but that fact alone does not establish corruption of another state variable. Preserve the blacklist finding and the need for isolated candidate checking; keep trial-state corruption as an investigation lead.

Evidence level: rejection mechanism is confirmed by source. A complete valid program reaching the branch, its exact diagnostic, and equivalent-expression behavior remain unexecuted.

### T3: Inherent Method Signatures Never Enter The Variance Validator

[variance.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/variance.ts), lines 22–25, accepts only data and enum maps. Its validation at lines 86–121 examines data fields, enum shared fields, and variant payload fields. Searching `src/` for `invalid-variance` finds only this checker and the parser's rejection of variance markers on trait parameters.

[program.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/program.ts), lines 167–175, invokes variance validation before implementation preparation. Later checking of ordinary method bodies verifies those bodies against their instantiated signatures; it does not recompute polarity across the nominal declaration's method surface. No compensating method-surface validator was found.

The strongest case uses a public method with a readonly receiver and a parameter containing the declaration's covariant parameter. `types.polarity.negative` explicitly makes a method parameter negative. `types.variance.surface` explicitly includes inherent methods, and `.covariant-check` rejects a negative occurrence. See [Type System](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/lang/04-type-system.md), lines 1257–1258, 1277–1282, and 1287–1288.

This establishes missing validation without taking a position on private methods, mutable receivers, or specialized inherent implementation heads. Their exact inclusion and substitution need further semantics review. The first report's private-method question is useful for eventual comprehensive implementation, but it does not block identifying the public readonly case as a conformance gap.

Qualification: this is acceptance of a declaration that the spec requires rejecting. Calling it a demonstrated runtime soundness exploit would overstate the evidence. GADT variance remains a separate unsupported path, as the original report already says.

Evidence level: missing validator and inputs confirmed. Final acceptance of a minimal public-method case remains predicted, since unrelated checks can also reject an attempted program.

### T4: Candidate Enumeration And Control-Flow Joins Are Separate Gaps

[assignability.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/assignability.ts), lines 143–151, enumerates identity, outer permission weakening, and readonly `List` element candidates. It receives no declaration variance information. Lines 163–179 intersect candidate spellings and compare them using `isPermissionWeakening`.

Counterevidence considered: `isPermissionWeakening` does know readonly `Map` value conversion and function variance at lines 101–131. That does not repair enumeration. Two different readonly Map or function spellings produce disjoint singleton candidate lists even when one converts to the other. User covariance also has no candidates in this algorithm.

[context.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/context.ts), lines 449–466, separately checks expected-type conversions with declaration-aware `varianceConversion`. The asymmetry claimed by T4 is therefore real. The extra claim about nested user covariance follows from [variance.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/variance.ts), lines 152–161: its recursive argument test uses a helper without user declaration data. No broader recursive conversion fallback was found on that path.

The important correction concerns control flow. An unannotated `if` does not directly use `leastCommonType`; [expression-control.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/expression-control.ts), lines 94–113, permits identical results, `never`, or a function-row union, then rejects differing other types. Its `match` helper at lines 949–961 similarly joins differing types only through `rowUnionType`. [context.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/context.ts), lines 1188–1200, uses that same row-only strategy for inferred returns.

Consequently an inference probe involving a branch might fail before it ever reaches `leastTypeCandidates`. The report should distinguish incomplete list/map candidate enumeration from failure to use a common conversion relation across control-flow and inferred-return sites. This is directly relevant to T4's architectural claim of duplicated semantics.

`types.lct.sites` includes all those sites, and `.uses`, `.conversions`, and `.no-combine` prescribe one relation. See [Type System](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/lang/04-type-system.md), lines 1459–1474. The no-combination restriction still applies: a proposed example must use already-readonly outer views, so the expected result does not require both outer weakening and variance in one conversion.

Qualification: the clear findings are missing permission/variance joins at specified sites. Optional injection's exact participation remains a separate spec interpretation question; it is not needed to establish this defect. The diagnostic distinction between `no-common-type` and `no-least-common-type` also requires a concrete type pair before judging the emitted error.

Evidence level: algorithms and their separation confirmed. Exact source programs, diagnostics, and order-independence probes remain unexecuted.

### T5: Implementation Visibility Is Lost, While Nominal Name Visibility Survives

[local-declarations.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/local-declarations.ts), lines 108–169, processes suites in source order. It rewrites nominal and trait references with fresh identities only after each local declaration. Child suites receive a copied mapping. This is counterevidence against the broad claim that local type or trait source names become globally accessible.

However, lines 137–153 append every accepted local implementation to one collected array and remove it from the executable statement list. Lines 174–182 append that array to all module implementations. No scope identifier or declaration-point availability token is retained with the implementation or the affected call.

[program-lower.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/program-lower.ts), lines 80–120, constructs HIR implementations with target, trait, bounds, method mappings, and span. The span exists, so wording that all location information is lost would be wrong. A span alone cannot determine suite extent, and the reviewed implementation and method lookup paths do not compare it with call position.

[implementation-index.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/implementation-index.ts), lines 21–40, filters only by target head. [expression-calls.ts](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/expression-calls.ts), lines 610–645, filters by target and trait matching, without a lexical implementation environment. Local synthetic names containing `#` bypass the nominal-head optimization and correctly fall back to all implementations; that fallback is not itself a bug.

The defensible consequence is availability leakage where a local type or trait was already visible before a later implementation, or remains visible in a parent suite after an implementation declared in a child suite. An unrelated function cannot automatically name the fresh local type identity through its original source spelling.

`names.local-impl.extent` and `trait.impl.local.lookup` explicitly limit lookup to declaration point and enclosing suite. See [Names and Scopes](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/lang/03-names-and-scopes.md), line 404, and [Traits](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/lang/09-traits.md), lines 600–607. Global overlap checks remain required by `.checks` and `.no-second`; lexical availability must not be confused with permission to duplicate an implementation.

Evidence level: lost implementation-availability structure is confirmed. Predicted before/after-scope acceptance remains unexecuted. The safest initial probe is an inherent implementation for a previously declared local type, avoiding independent trait-availability questions.

## Consolidation Guidance

Retain all four findings as static architectural evidence, with T2 and T4 emphasizing false rejection and T3 and T5 emphasizing missing validation or availability boundaries. Do not label runtime exploits, state corruption, or end-to-end observed diagnostics as confirmed.

The corrections narrow T2 and T5, remove private-visibility ambiguity as a prerequisite for T3's strongest case, and make T4's multiple inference paths explicit. No original report, compiler, specification, or fixture was modified during this challenge pass.
