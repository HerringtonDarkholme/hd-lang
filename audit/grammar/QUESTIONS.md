# Grammar Questions For The Owner

## Owner Decisions

Decided 2026-09-26; not yet applied to the specification.

- **Q1: the declaration owns a trailing requirement clause.** A clause
  directly before a declaration's or closure's `:` (or a bodyless trait
  method's line end) belongs to the declaration. A function-typed result with
  its own row is parenthesized: `fn make() -> (fn() -> i32 $ Log) $ Console:`.
  Inside types, the row still attaches to the innermost function type.
- **Q2: after an indented closure body inside brackets, the next line must
  start with `,` or a closing delimiter** at the header's indentation.
- **Q7: leading-dot continuation.** A line that starts with `.` followed by an
  identifier, indented deeper than the previous line, continues it when the
  previous line does not open a suite.

Each question stands alone. Findings with the evidence are in
[FINDINGS.md](FINDINGS.md). The `shape`, `and`/`or`/`not`, and `where`
questions were decided separately (K1 to K3).

## Q1. Which construct owns a requirement clause written after a function-typed result?

```text
fn make() -> fn() -> i32 $ Console:
    ...
```

Today the grammar allows two readings, and the "innermost function type"
prose picks the returned type, so `make` itself requires nothing.

- A. **Recommended.** A clause directly before a declaration's or closure's
  `:` (or a bodyless trait method's line end) belongs to the declaration. To
  give the returned function its own row, parenthesize it:
  `fn make() -> (fn() -> i32 $ Log) $ Console:`.
- B. Keep "innermost" and state it for declarations with this example.
- C. Reject an unparenthesized function-type result followed by `$` in a
  declaration or closure header.

(GR-02)

## Q2. How are multiline closures passed as arguments that are not last?

```text
choice(fn(a):
    println(a)
    fallback)       # today: `fallback` is the closure's last statement
```

Chapter 07 shows each closure wrapped in parentheses. The layout rules also
accept a bare closure followed by a line starting with `,`, and they read a
line at body indentation as part of the body.

- A. **Recommended.** After an indented closure body inside brackets, the
  next line must start with `,` or a closing delimiter at the header's
  indentation. The example above becomes an error; parenthesized closures
  keep working.
- B. Require parentheses around every multiline closure that is not the
  final argument.
- C. Keep today's rules and document the example above as a pitfall.

(GR-04)

## Q3. Must a nested suite be indented farther than the statement that contains it?

```text
    if ready:
        x := run(
    fn(v):
        v            # column 8: left of or level with `x := ...`
)
```

Today the body only has to be deeper than the line holding `fn(v):`.

- A. **Recommended.** The body must also be deeper than the logical line
  that contains the header.
- B. Keep today's rule; leave it to a formatter.

(GR-05)

## Q4. Is `pack.map(...)` always the pack operation, even when a local is named `pack`?

```text
pack := Packer {}
y := pack.map(items, size)
```

- A. **Recommended.** Yes: `pack.map(` and `pack.map_list(` always form the
  pack operation, as `::annotation` always forms annotation access.
- B. Make `pack` a prelude name, so `pack := ...` is a
  `prelude-name-shadow` error.
- C. Spell the operations with a reserved prefix, such as `$.map(...)`.

(GR-06)

## Q5. Should data literals label fields with `=` instead of `:`?

```text
p := Point { x: 1, y: 2 }       # today
match p:
    Point { x = 0, y } => ...   # patterns already use =
f(x = 1)                        # named arguments use =
m := { x: 1 }                   # map: x is a value
```

- A. **Recommended.** `Point { x = 1, y = 2 }`, plus `Point { x, y }`
  shorthand. `:` inside braces then always means a map entry.
- B. Change patterns to `:` instead (Rust's choice).
- C. Keep both.

(GR-11)

## Q6. May reserved words be used as member names and argument labels?

```text
kind := token.type      # syntax error today
f(match = true)         # syntax error today
```

- A. **Recommended.** Accept any reserved word after `.` and as a named
  argument label, and therefore also as a field or parameter name.
- B. Add a raw-identifier escape such as `` `type` `` (Kotlin, Swift).
- C. Keep the restriction.

(GR-12)

## Q7. Should a line starting with `.` continue the previous line?

```text
names := users
    .map(fn(u): u.name)
    .filter(fn(n): n != "")
```

Today this is a syntax error; the chain must be wrapped in parentheses.

- A. Keep the rule (Python's behavior).
- B. **Recommended.** A line that starts with `.` followed by an identifier,
  and is indented farther than the previous line, continues it (Swift,
  Kotlin).
- C. B, and also continue after a trailing binary operator such as `+`.

(GR-13)

## Q8. Must a `(` or `[` suffix start on the same line as its operand?

```text
xs := [
    first
    [1]         # today: first[1], because the comma is missing
]
```

- A. **Recommended.** Yes. Inside brackets, a line that starts with `(`,
  `[`, or `{` never continues the previous element as a call, index, or data
  literal, so the example is a missing-comma error. This also settles
  `[a` newline `!(b)]` once prefix `!` exists (K3).
- B. No; keep Python's behavior and leave it to a formatter.

(GR-14)

## Q9. Which `if` does a second same-line `else` belong to?

```text
v := if a: if b: 1 else: 2 else: 3     # syntax error today
```

- A. **Recommended.** `else` closes same-line suites up to the nearest
  same-line `if`, `for`, or `while` without an `else` (Kotlin's rule). The
  line above becomes valid.
- B. Forbid a same-line `if` directly inside another same-line suite
  (Python's rule).
- C. Keep today's rule and document it.

(GR-15)

## Q10. Should suite and trailing-block right-hand sides work in every statement form?

```text
r := compute(1):      # valid
    2
r = compute(1):       # syntax error today
    2
a, b := c, d := fn() -> i32: 1    # valid
a, b := c, d := pair              # syntax error today
```

- A. **Recommended.** Allow a trailing block wherever a suite expression may
  be a right-hand side (`=`, `return`, `break`, `_ :=`), and drop chained
  multi-name bindings.
- B. Keep today's forms and list them in chapter 07.

(GR-16)

## Q11. Does `[` right after `annotate` always open generic parameters?

```text
annotate [T] (Validation(max = 3)) for Box[T]:
    pass
```

The grammar also allows reading `[T](...)` as a list literal called as the
facet expression.

- A. **Recommended.** Yes, as after `impl`.
- B. Forbid facet expressions that start with `[`.

(GR-17)

## Q12. Where does `!` go in a generic suspending declaration?

```text
fn all![Ts...](tasks: mut Suspend[Ts]...) -> (Ts...):   # today
results := all[i32, string]!(a, b)                      # call
```

- A. **Recommended.** Declare as `fn all[Ts...]!(...)`, matching the call.
- B. Call as `all![i32, string](a, b)`, matching the declaration.
- C. Keep both orders.

(GR-18)

## Q13. Should list literals accept a suffix spread?

```text
f(xs...)                    # argument spread (suffix)
q := Point { ...p, x: 1 }   # copy-update (prefix)
ys := [0, xs...]            # syntax error today
```

- A. **Recommended.** Allow `[a, xs...]`, and document the rule: a prefix
  `...` copies named members, a suffix `...` expands positional elements.
- B. Keep today's forms and only document the rule.

(GR-19)

## Q14. Should every declaration suite accept both `pass` forms?

```text
annotate Validation for Point: pass     # valid
annotate Validation for Point:
    pass                                # syntax error today
data P:
    pass                                # valid
```

- A. **Recommended.** Yes, for every declaration body that is a suite (data
  and annotate). `trait` and `impl` keep their bodyless forms.
- B. Keep today's forms.

(GR-20)

## Q15. May `$self` be interpolated, and is a bare `$` in a string an error?

```text
fn show(self) -> string:
    "value: $self"      # spec: error (self is not an identifier); reference parser accepts
price := "costs $5"     # error today; Kotlin prints it literally
```

- A. **Recommended.** `$self` interpolates `self`; a bare `$` stays an
  error (it catches typos).
- B. Both stay errors; write `${self}` and `\$`.
- C. `$self` allowed, and a `$` that starts neither form is literal text
  (Kotlin).

(GR-10 d, GR-22)

## Q16. Should `type` and `data` become contextual words?

```text
fn describe(type: string, data: Bytes) -> void:   # both syntax errors today
    ...
type UserId(i32)       # still a type declaration
data Point:            # still a data declaration
    x: i32
```

Each is a keyword only at the start of a statement and when followed by an
identifier; two adjacent identifiers are never an expression. Python 3.12
made `type` a soft keyword; Kotlin's `data` is a soft modifier. The layout
rules and the reference lexer would apply the same test.

- A. **Recommended.** Make `type` and `data` contextual.
- B. A, and also `enum` and `trait`, so every declaration word except `fn`
  and `impl` follows one rule.
- C. Keep them reserved.

(Keyword Set)

## Q17. Should `reified`, `super`, `as`, and `use` become contextual words?

```text
use super.shared.{Email as E}    # still use-declaration syntax
fn pick[reified T]() -> T        # still a reified parameter
resource.use(fn(r): r.read())    # valid if `use` is contextual
```

- `reified` appears only first in generic parameters, before an identifier.
- `super` and `as` appear only in use declarations, like the contextual
  `pkg`, `std`, and `dep`. No `as` cast is planned (casts are `i16(x)`).
- `use` starts a use declaration only when followed by a use root, which is
  a closed set; `$.use(` is already a dedicated form.

Options:

- A. **Recommended.** Make all four contextual.
- B. Only `reified`, `super`, and `as`; keep `use` reserved.
- C. Keep them reserved.

(Keyword Set)
