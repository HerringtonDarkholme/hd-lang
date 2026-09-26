W7 evidence (audit-derived conformance cases).
sources.tsv      every source -> added path(s) or skipped with reason (parts: sources-{parent,blind,coverage}.tsv)
new-cases.tsv    the 107 rows appended to spec/conformance/cases.tsv; manifest.txt lists their paths
mvp-all.log      run-conformance.ts --manifest manifest.txt against the MVP (72 pass, 35 fail)
mvp-*.log        per-part runs made while authoring (parent run predates two duplicate removals)
portable-*.tsv / known-*.tsv   rows appended to test/portable/cases.tsv and KNOWN_FAILURES.tsv
spec-check.log, test-portable.log   final bash spec/check.sh and npm run test:portable
Parent part: fuzz findings F-300..F-316, audit/probes/{gaps,gates,coordinator}, compiler/init-dispatch-miss.hd.
