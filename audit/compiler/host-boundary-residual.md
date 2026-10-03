# Composite Host Boundary Residual

Date: 2026-10-03

The host-result repair validates every scalar width and scalar payload on
either side of `Result`, including exact integer ranges, Unicode text and
characters, raw floats, and replay encodings. The checker and emitter still do
not expose the rest of the composite boundary surface described by
`module.profile.host-result.shape`: optional values, tuples, lists, maps, data
types, and enums cannot be used as general host results.

This is not covered by the five `HOST-CONTRACT`, `HOST-NAN`, and
`PENDING-WRITE` fixtures repaired in item 5. Add focused conformance cases for
accepted nested shapes, recursive field/payload validation, malformed shapes,
and boundary-safe public fields before implementing it. The implementation
should use one structural boundary codec shared by live calls and replay, not
one emitter special case per container.
