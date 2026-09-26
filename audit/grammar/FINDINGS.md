# Grammar Audit Findings

Scope: [Lexical Structure](../../spec/01-lexical-structure.md),
[Grammar](../../spec/02-grammar.md), the grammar fragments in chapters 11 to
14, and the [reference parser](../../spec/reference-parser/). Roadmap item:
[Grammar Audit](../../future-work/ROADMAP.md#1-grammar-audit).

Decisions for the owner are in [QUESTIONS.md](QUESTIONS.md). Each finding
names the question that settles it.

Severity:

- **High**: two readings with different meaning and no rule that picks one,
  or a tool that accepts or rejects code against explicit spec text.
- **Medium**: a rule exists but readers will misread the code, or two parts
  of the spec disagree.
- **Low**: consistency or ergonomics.

## Method

All tools are under [`tools/`](tools/). They import only Node built-ins and
`spec/`.

- [`tools/ambiguity.ts`](tools/ambiguity.ts) (wrapper: `tools/amb`) reads the
  `ebnf` fences of chapter 02, turns them into BNF, and runs an Earley parser
  that **counts derivations** instead of only accepting. For every input with
  more than one parse it prints each locally ambiguous node and its competing
  trees. Source text goes through the reference lexer, so layout tokens are
  the spec's.
  - `amb fixtures`: every accept fixture in `spec/conformance` (765 files).
  - `amb generate N SEED`: random derivations from the grammar, checked at
    the token level (9,000 derivations over seeds `a1` and `b7`).
  - `amb firstfollow`: LL(1) FIRST/FOLLOW conflicts on `:`, `[`, `$`, `!`,
    `?`, `<`, `else`, `for`, `if`, `...`, and the layout tokens.
  - `amb cases FILE`: hand-written probes in [`probes/`](probes/), each run
    through both the reference parser and the derivation counter.
- The probe files hold about 150 minimal programs. Rerun with
  `audit/grammar/tools/amb cases audit/grammar/probes/*.cases`.

### Mechanical results

- **No unintended ambiguity in the fixtures.** Every ambiguity in the 765
  fixtures is one the spec already hands to name resolution: generic
  reference versus indexing (`first[string](names)`), a dotted name versus
  member access (`user.name`), a single row key versus a type argument
  (`Box[Log]`), `@derive(...)` versus an ordinary decorator, a facet type
  versus a facet expression, and `pack.map(...)`.
- **Random derivations found four more classes**: the owner of a trailing
  requirement clause (GR-02), `Type::annotation(...)` versus a qualified call
  (settled: `annotation` always wins), `annotate [T] ...` (GR-17), and the
  facet `T?` type versus `T?` propagation (settled like the facet
  type/expression overlap).
- **`:` has no LL(1) conflict at the token level.** Once layout has produced
  `NEWLINE INDENT` or `SUITE_END` context, the preceding token decides every
  use of `:`. The risk around `:` is entirely in layout: the lexer must know
  which `:` opens a suite. The remaining FIRST/FOLLOW conflicts are `else`
  (dangling else, decided by `SUITE_END` placement), `[` (generic versus
  index), and `$` at statement start (`$.with(...):` suite versus
  `$.use(...)` expression, decided by the third token).
- **The reference parser disagreed with the spec in eight places** (GR-01,
  GR-03, GR-10). Five are fixed; the rest are listed.

## Findings

### GR-01: The literal `"type"` aliased the `type` production in the reference parser

Severity: High. Status: fixed.

The reference parser compiled a quoted EBNF literal to the same symbol as a
production of that name, and its scanner matched any symbol against a token.
`"type"` is both a keyword literal and the `type` production, so the keyword
position accepted any type and a type position accepted the keyword:

```text
fn f(x: type) -> void: pass      # accepted, spec: syntax error
i32 Id = i32                     # accepted as a type declaration
list[i32] Id(i32)                # accepted as a newtype
```

Fix applied: literals compile to quoted terminals and productions never scan
(`spec/reference-parser/grammar.ts`). Fixtures:
`parse/invalid/keyword-type-as-parameter-type.hd`,
`parse/invalid/type-in-place-of-type-keyword.hd`.

### GR-02: A requirement clause after a function-typed result has two owners

Severity: High. Question: Q1. Status: fixed (option A; spec Revision Note GQ1).

```text
fn make() -> fn() -> i32 $ Console:
    ...
```

Reading A: `make` requires `Console` and returns a plain `fn() -> i32`.
Reading B: `make` requires nothing and returns `fn() -> i32 $ Console`.

The same split occurs in closure headers and bodyless trait methods. The
grammar derives both. The prose says a clause "following nested function
types" belongs to the innermost ungrouped function type. That picks B, but
the sentence is about types, and no example shows a declaration. Reading B
silently removes the requirement from the function the reader meant to
annotate; the checker then reports `missing-requirement` far from the cause.

Other languages: Swift attaches `throws` and `async` to the declaration and
needs parentheses to put them on a returned function type. Koka writes the
effect before the result, so the question cannot arise.

Fixes: (1, recommended) a clause directly before a declaration's or
closure's `:` belongs to that declaration, and a function-typed result with
its own row is parenthesized; (2) keep the innermost rule and state it for
declarations; (3) reject the unparenthesized form.

### GR-03: The reference lexer did not close a nested suite at a closing delimiter

Severity: High. Status: fixed.

Chapter 01 says a closing delimiter at the nested suite's depth ends the last
body line. The reference lexer closed nested suites only at the start of a
line, so `apply(fn(a: i32) -> i32:` newline `    a + 1)` and a parenthesized
multi-line `if` were rejected. Fixed in `spec/reference-parser/lexer.ts`.
Fixture: `parse/valid/closing-delimiter-ends-nested-suite.hd`.

### GR-04: A multiline closure argument absorbs lines that look like later arguments

Severity: Medium. Question: Q2. Status: fixed (option A; spec Revision Note GQ2).

```text
choice(fn(a):
    println(a)
    fallback)          # a body statement, not a second argument

choice(
    fn(a):
        println(a)
    ,                  # valid: the dedent ends the body, the comma follows
    fn(b):
        println(b)
)
```

The first example is one argument whose body ends in `fallback`. The second
is valid, but the spec never writes it that way. Python has no multiline
lambdas for this reason; Kotlin and Swift use braces.

Fixes: (1, recommended) after an indented closure body inside brackets, only
a line starting with `,` or a closing delimiter at the header's indentation
may follow; (2) document both forms; (3) require parentheses around every
non-final multiline closure.

### GR-05: A nested suite's body may sit to the left of its enclosing block

Severity: Medium. Question: Q3.

```text
fn main() -> void:
    if ready:
        x := run(
    fn(v):
        v
)
        go()
```

The indentation reference of a suite opened inside brackets is the physical
line holding its header, so the body can be less indented than the
statement containing it. Fix (recommended): the body must also be deeper
than the logical line that contains the header.

### GR-06: `pack.map(...)` is ambiguous with a method call on a local named `pack`

Severity: Medium. Question: Q4.

`pack`, `map`, and `map_list` are contextual only in `pack.map(...)`, and
`pack` is not a prelude name, so `pack := Packer {}` then `pack.map(items,
size)` derives both ways. Chapter 02 settles the parallel `::annotation`
case but not this one. Fix (recommended): `pack.map(` always forms the pack
operation.

### GR-07: `else` closed only a same-line `if` suite, contradicting the loop productions

Severity: Medium. Status: fixed. Chapter 01 now names same-line `if`, `for`,
and `while` suites. Fixture: `parse/valid/same-line-loop-else.hd`.

### GR-08: The vararg grammar allowed a default that the prose forbids

Severity: Medium. Status: fixed. `values: i32 = 0...` parsed although
chapter 07 says a vararg has no default. The vararg alternative in chapters
02 and 12 is now `identifier, ":", type, "..."`. Fixture:
`parse/invalid/vararg-default-before-ellipsis.hd`.

### GR-09: Stale grammar fragments in chapters 12 and 14

Severity: Medium. Status: fixed.

- Chapter 12 wrote the generic bound with `:` instead of `<`.
- Chapter 14's `metadata_assignment` used `expression` where chapter 02
  correctly uses `closed_expression`.

Fixtures: `parse/invalid/colon-generic-bound.hd`,
`parse/invalid/colon-supertrait.hd`.

### GR-10: Reference-parser heuristics disagreed with the spec

Severity: Medium. Status: a to c fixed; d to g open.

| # | Input | Reference | Spec | Status |
| - | ----- | --------- | ---- | ------ |
| a | `[for x in xs:` or `[fn(x: i32) -> i32:` ending a line | `trailing-block-position` | valid bracketed expression | fixed |
| b | literal line `on: a == b,` or `a != b` | `missing-let` | valid | fixed |
| c | `c := r'a'` | accepted as a raw string | no raw character literal | fixed |
| d | `"$self"`, `"$true"` | accepted | `$name` takes an identifier | Q15 |
| e | `f(x = 1, fn (y): y)` | accepted | positional after named is `argument-order` | open |
| f | dedent to an unused column inside a bracketed suite | `syntax-error` | `invalid-dedent` | open |
| g | `t.1_0` | accepted | a tuple index is decimal digits only | open |

Also open: a multi-line parameter list with a default value
(`fn f(\n    x: i32 = 1,\n) -> void:`) reports `missing-let`, from the
line-based check in `spec/reference-parser/contextual.ts` (found by the
standard-library agent).

### GR-11: Data literals use `:` where every other labelled form uses `=`

Severity: Medium. Question: Q5. Status: fixed (option B, patterns use `:`; spec Revision Note GQ5).

```text
p := Point { x: 1, y: 2 }            # data literal: colon
m := { x: 1, y: 2 }                  # map literal: colon, x and y are values
f(x = 1, y = 2)                      # named arguments: equals
match p:
    Point { x = 0, y } => ...        # data pattern: equals, shorthand y
Status.NotFound(code = 404)          # named payload: equals
```

Fix (recommended): data literals use `=`, with `Point { x, y }` shorthand;
`:` inside braces then always means a map entry.

### GR-12: Reserved words cannot be member names or argument labels

Severity: Medium. Question: Q6.

`token.type`, `f(type = 1)`, and a field named `type` are syntax errors. Host
APIs and serialized data use such names often. Swift accepts most keywords
after `.` and as labels; Kotlin and Swift escape with backticks; Rust has
`r#type`. Fix (recommended): accept any reserved word after `.`, as a
named-argument label, and in field and parameter declarations.

### GR-13: Leading-dot chains and trailing operators cannot continue a line

Severity: Medium. Question: Q7. Status: fixed for leading `.` (option B; spec Revision Note GQ7).

Only brackets continue a logical line, so a chain written with leading `.`
lines is a syntax error. Fix (recommended): a line that starts with `.`
followed by an identifier, indented farther than the previous line,
continues it.

### GR-14: A missing comma inside brackets joins two elements silently

Severity: Low. Question: Q8.

Inside brackets a line break is not a token, so `first` newline `[1]` becomes
`first[1]`, and a following `-b` or `(c)` becomes subtraction or a call.
Swift avoids this: `(` or `[` on a new line is not a suffix. Fix
(recommended): a `(`, `[`, or `{` suffix must start on the same physical
line as its operand.

### GR-15: Nested same-line `if` with two `else` branches cannot be written

Severity: Low. Question: Q9.

`v := if a: if b: 1 else: 2 else: 3` is a syntax error because `else` closes
only the innermost same-line suite. Fix (recommended): Kotlin's rule, `else`
closes suites up to the nearest same-line `if`, `for`, or `while` without an
`else`.

### GR-16: Statement forms accept suites and trailing blocks unevenly

Severity: Low. Question: Q10.

A trailing block may follow `:=` and `let ... =` but not `=`, `return`, or
`_ :=`; chained multi-name bindings parse only when the chain ends in a
suite. Fix (recommended): allow a trailing block wherever a suite expression
may be a right-hand side, and drop chained multi-name bindings.

### GR-17: `annotate [` opens generic parameters or a list-literal facet

Severity: Low. Question: Q11. `annotate [T] (Validation(max = 3)) for
Box[T]:` also reads as a list literal called as the facet. Fix
(recommended): `[` directly after `annotate` always opens generic
parameters.

### GR-18: `!` goes before generic parameters in a declaration, after type arguments in a call

Severity: Low. Question: Q12. `fn all![Ts...](...)` versus
`all[i32, string]!(a, b)`. Fix (recommended): declare as
`fn all[Ts...]!(...)`.

### GR-19: Spread `...` is a prefix in some places and a suffix in others

Severity: Low. Question: Q13. Prefix `...` copies named members
(copy-update, context entries); suffix `...` expands positional elements.
List literals accept neither. Fix (recommended): document the rule and add
`[a, xs...]`.

### GR-20: Empty bodies use `pass` inconsistently

Severity: Low. Question: Q14.

| Body | same-line `pass` | indented `pass` | no body |
| ---- | ---------------- | --------------- | ------- |
| `data P:` | yes | yes | no |
| `annotate X for Y:` | yes | **no** | no |
| `annotate P.name:` | yes | **no** | no |
| `trait T` | no | no | yes (marker) |
| `impl T for X` | no | no | yes |
| `enum E:` | no | no | no |

### GR-21: `a?.b` and `x is T` look like Kotlin and Swift but mean something else

Severity: Low; no question. `a?.b` is propagation then member access, which
returns from the enclosing function on `nil`, not optional chaining. `x is
T`-style reading is wrong too: `is` is identity comparison, as in Python.
Teaching material should call this out.

### GR-22: A bare `$` in a string is an error

Severity: Low. Question: Q15. `"costs $5"` must be written `"costs \$5"`.
The strict rule catches typos such as `"$ name"`; recorded because Kotlin
users will expect otherwise.

### GR-23: Name-resolution ambiguities the spec already settles

| Form | Readings | Resolution |
| ---- | -------- | ---------- |
| `first[string](names)` | generic reference or index | name resolution |
| `user.name`, `Color.Red` | qualified name or member access | name resolution |
| `Box[Log]`, `run[A + B]` | type argument or row argument | parameter kind |
| `annotate Facet for T` | facet type or facet expression | name and type resolution |
| `Point::annotation(F)` | runtime access or qualified call | `annotation` always wins |
| `@derive(Eq)` | derive or ordinary decorator | chapter 14: derive is not a facet |
| `fn() -> T?` | optional result or optional function | innermost |
| `(a, b := pair)` | grouped binding, never a tuple | chapter 02 |

## Keyword Set

This section reviews the reserved words other than `shape`, `and`, `or`,
`not`, and `where`, which were decided separately (K1 to K3).

A word can be contextual when, at every position where it acts as a keyword,
reading it as an identifier gives a token sequence no other production
accepts. The usual evidence is juxtaposition: two adjacent identifiers never
form an expression, so `data Point` can only be a declaration. The main cost
is layout: the lexer uses `data`, `test`, `annotate`, `fn`, `if`, `while`,
`for`, `match`, `else`, `defer`, and `with` to decide which `:` opens a
suite, so a contextual `data` must be recognized by the same rule in the
lexer and the parser.

### Can become contextual

| Word | Grammar positions | Evidence | Recommendation |
| ---- | ----------------- | -------- | -------------- |
| `type` | `type_decl`, `associated_type_decl` | Statement-initial and followed by an identifier; Python 3.12 made `type` soft. | **Contextual** (Q16) |
| `data` | `data_decl` | `data` then an identifier at statement start; Kotlin's `data` is soft. | **Contextual** (Q16) |
| `reified` | `generic_parameter` | Only first inside generic parameters, before an identifier. | **Contextual** (Q17) |
| `super` | `use_root` only | Same position as the contextual `pkg`, `std`, `dep`. | **Contextual** (Q17) |
| `as` | `use_decl`, `use_item` only | No `as` cast is planned. | **Contextual** (Q17) |
| `use` | `use_decl`, `context_use` | Line-initial `use` followed by a use root, a closed set. | **Contextual** (Q17) |
| `enum` | `enum_decl` | Same argument as `data`. | Optional (Q16) |
| `trait` | `trait_decl` | Same argument as `data`. | Optional (Q16) |
| `let` | statements | Rarely an identifier. | Keep reserved; no gain |
| `pub` | declarations, fields, methods, `use` | Always followed by a keyword or field name. | Keep reserved; no gain |
| `in` | `for` headers, comprehensions | A future membership operator would need it. | Keep reserved |
| `is` | comparison operator | Contextual infix operators make error recovery poor. | Keep reserved |
| `mut` | types, receivers, closures, bounds | Appears in five productions. | Keep reserved |
| `self` | receiver, expression, use root | Could be an ordinary binding, as in Python. | Keep reserved; allow `"$self"` (Q15) |
| `Self` | types, projections | Reserving it prevents `data Self`. | Keep reserved |

### Must stay reserved

| Word | Why a contextual reading fails |
| ---- | ------------------------------ |
| `fn` | `fn(x):` with a body is exactly a trailing-block call of a callable named `fn`. |
| `defer` | `defer:` with a body is a trailing-block call of a callable named `defer`. |
| `match` | `match (x):` with a body is a trailing-block call. |
| `if`, `while`, `for`, `else` | Layout uses them to find suite colons and same-line suite boundaries. |
| `return`, `break` | `return (x)` would read as a call, and a bare `return` as a name. |
| `continue` | A bare `continue` would read as a name expression. |
| `pass` | Also a primary expression, so it has no keyword-only position. |
| `impl` | `impl[T] Show for X` reads as indexing until after `]`: unbounded lookahead. |
| `annotate` | Same problem as `impl`, and `annotate (f) for X` reads as a call. |
| `true`, `false`, `nil` | Literals, also in patterns. |

### Contextual words already in the spec

`test`, `pkg`, `std`, `dep`, `derive`, `annotation`, `annotation_ref`,
`context`, `with`, `Context`, `pack`, `map`, and `map_list`. Only `pack` has
a reachable ambiguity (GR-06).

### Net effect

After K1 to K3 the set has 31 words. Q16 and Q17 would remove `type`,
`data`, `reified`, `super`, `as`, and `use` (25 words), or also `enum` and
`trait` (23 words).

### Grammar facts for prefix `!` (K3)

No expression started with `!` before K3, so `!(` after an expression was
always a bang call, even across a line break inside brackets. With a prefix
`!`, `[a` newline `!(b)]` has two readings, one of them a list missing a
comma. GR-14's fix 1 (Q8) settles that case.
