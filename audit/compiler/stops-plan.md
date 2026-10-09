# Plan: Remaining Body And Emit Stops (R22)

Method: per-case listing at `bb52571e`
(`HD_CONFORMANCE_ONLY=.hd cargo test -q --release -p hd_driver --test
conformance -- --nocapture`), kept verdicts `unsupported:Body` (186)
and `unsupported:Emit` (100) — matching `compiler/CONFORMANCE.md`
exactly — grouped by the construct each message names, biggest first.
Task numbers come from `audit/muse_task.md` jobs and
`audit/compiler/*.md`; groups with none say so (orchestrator to
number). #46 (adapters) and #150 (bare paths) are on main; #74's
trailing-block Emit lowering is the remaining piece it names. Read
only.

## Body stops (186)

| Count | Construct | Task |
| ---: | --- | --- |
| 23 | `TrailingCallExpr` (incl. integration-test inits) | #74 (R6 design; Check part via #150 landed, Emit lowering pending) |
| 23 | data literal spread | #122 (R4) |
| 21 | spread argument | #122 (R4) |
| 18 | `statement DataDecl` (local data declarations in bodies) | no number |
| 14 | `PowerExpr` (`**`) | no number |
| 9 | open range pattern | no number |
| 7 | call through explicit type arguments of a type | #140 (R17 design; the call side) |
| 7 | assignment to this target | no number |
| 7 | type name used as a value (`Mailer`, `Priced`, `list`, `Set`, `T`) | no number |
| 6 | full range `..` | no number |
| 5 | `RestType` in a body | no number |
| 4 | mixed positional and named variant fields | no number |
| 4 | `statement FnDecl` (local named functions) | no number |
| 4 | list spread | #122 (R4) |
| 4 | compound assignment operator | no number |
| 4 | tuple pattern with a rest | no number |
| 2 | `statement ImplDecl` | no number |
| 2 | binding chain | no number |
| 2 | trait method whose impl waits for inference | no number (solver/checker boundary) |
| 2 | method without a signature | no number |
| 2 | `?` on unknown-typed value | no number (inference ordering) |
| 2 | call of a type name | no number |
| 2 | tuple spread | #122 (R4) |
| 2 | module member value that is not a function | no number |
| 2 | `$.context` and context types | no number |
| 2 | `$.context` and context types | no number |
| 1 | `statement TypeDecl` | no number |
| 1 | `statement EnumDecl` | no number |
| 1 | `statement TraitDecl` | no number |
| 1 | `for` over unknown-typed value | no number (inference ordering) |
| 1 | method on unknown-typed value | no number (inference ordering) |
| 1 | field of unknown-typed value | no number (inference ordering) |
| 1 | named arguments to a vararg function | no number |
| 1 | shared data of a generic enum | no number |

## Emit stops (100)

| Count | Construct | Task |
| ---: | --- | --- |
| 26 | intrinsic `MapIter` | #124 (R7) |
| 15 | `DefaultCall` tag | #32 (R8) |
| 12 | call that collection did not resolve | no number (collection) |
| 10 | trait-value call of a supertrait's method | #131 (R14) |
| 7 | `Is` tag | no number |
| 5 | f32 arithmetic | #147 (R13) |
| 5 | suspending closure (`fn!` value) | no number (suspension area; cf. #146) |
| 4 | map key type other than integer/string | #124 (R7) |
| 3 | non-concrete layout (inference var reaching Emit) | no number (inference ordering) |
| 3 | intrinsic `downcast_val` | no number (Inspectable builtin) |
| 2 | `MapRemove` | #124 (R7) |
| 2 | float `Conv` | #147 (R13) |
| 1 | `to_string` at f64 | float text (R11 census; no number) |
| 1 | `Poison` reaching Emit | no number (internal: check leaked Poison) |
| 1 | float `Rem` | #147 (R13) |
| 1 | coercion kind 7 | no number |
| 1 | suspension in `and`/`or` operand (on a warning-first verdict) | #146 family (cf. R12) |
| 1 | suspension in match guard (on a warning-first verdict) | #146 family (cf. R12) |
| 1 | suspension in match guard (on a warning-first verdict) | #146 family (cf. R12) |

## Notes

- R18 shrinks cover four Body rows already (nested-capture chains
  read zero; negative-float patterns; nested-string interpolation;
  map duplicates) and four Emit-adjacent runtime rows — those
  compiler tasks unblock fixtures without moving these counts.
- The `??`-style rows in the raw log (a warning first, the
  `unsupported` later) are counted under their verdict's construct;
  the leading warnings (`private-main`, `unused-local-binding`) are
  not separate stops.
- Biggest unnumbered clusters for the orchestrator to number:
  Body `DataDecl` (18), `PowerExpr` (14), open range patterns (9),
  Emit unresolved-collection calls (12), `Is` (7).
