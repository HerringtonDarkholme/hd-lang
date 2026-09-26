# F-302: Reference parser applies argument-order to context entries and rejects the chapter 11 example
Severity: major
Area: spec
Duplicates: F-360, found independently by the fuzzer; this file adds that the chapter 11 example itself is rejected
Evidence: audit/evidence/03-fuzz/replay.txt (F-302 block); fuzz signature parse-agreement|ref-reject/impl-accept|argument-order, round 1
Effect: `$.with(Database=db, Logger=logger, ...prod_context()):` appears verbatim in chapter 11, and `parseSource` rejects it with `argument-order`. So does `$.context(...a, Clock = c, ...b)`. Chapter 11 says context spreads and explicit bindings apply left to right, with the later binding winning, so any order is valid. hd accepts these forms.
Recommendation: implementation change (reference parser). Exclude `$.with(` and `$.context(` entry lists from `argumentOrderDiagnostics`. Add the chapter 11 example as a parse fixture.
