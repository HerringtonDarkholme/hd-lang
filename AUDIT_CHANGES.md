# Design Consistency Work

This working-tree pass aligns the accepted language chapters, guides, runtime
examples, and conformance inventory. It is a change summary, not a claim that
every review finding is closed.

The pass:

- reconciles value-pack, closure-capture, row-subtraction, runtime-shape, and
  must-use examples with their normative rules;
- repairs suite termination and trailing-block grammar and compiles the
  chapter-02 EBNF into an Earley parser run across every conformance fixture;
- closes soundness gaps in variance conversion, top-level initialization,
  generic provider keys, and nested suspension driving;
- unifies built-in names under one prelude and one shadowing rule;
- defines the standard console and testing surfaces used by examples;
- accepts synchronous block-scoped `defer` while keeping ownership,
  alias-escape prevention, and asynchronous cleanup in future work;
- aligns panic, boundary, annotation, and runtime examples with the accepted
  specification; and
- keeps unresolved language-design choices in `future-work/OPEN_ISSUES.md`
  with options and recommendations.

The authoritative verification command is `spec/check.sh`.
