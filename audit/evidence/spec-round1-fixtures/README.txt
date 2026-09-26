Round-1 spec decision fixtures (SPEC_DECISIONS.md, commit 54b0c70).
new-cases.tsv  rows appended to spec/conformance/cases.tsv
manifest.txt   selection manifest of the new cases
run-mvp.log    node --experimental-strip-types spec/tools/run-conformance.ts --manifest manifest.txt (MVP compiler, default command)
Reused existing cases: typing/invalid/missing-requirement.hd (C1 reject),
typing/invalid/nonhost-entry-requirement.hd + typing/valid/host-entry-requirement.hd (L4),
typing/invalid/unsafe-dynamic-trait.hd (G2 unbounded reject).
No reject fixture for L5, L7, L8: L5/L8 are runtime identity rules; the spec names no code for incompatible `is` operands (L7).
