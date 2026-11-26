# Mistakes: Syntax

What was written, the code `hd check` reports, and what to write instead.
`syntax-error` covers most of these, so match on the left column.

## Blocks and layout

| Wrote | Code | Write instead |
| --- | --- | --- |
| `fn f() -> i32 {` ... `}` | `syntax-error`, `unclosed-delimiter` | `fn f() -> i32:` and an indented body |
| `.Some(v) => { v }` in a `match` | `syntax-error` | `.Some(v) => v`, or an indented body under the arm |
| `let x = 1;` | `reserved-semicolon` | no semicolons; one statement per line |
| a long expression continued on the next line, as `a` newline `&& b` | `syntax-error` | wrap it in parentheses: `(a` newline `&& b)` |
| `trait Marker: pass` or `impl T for X: pass` on one line | `syntax-error` | put `pass` on its own indented line; only `struct Name: pass` fits one line |
| `use std.cmp.max` inside a function | `syntax-error` | move it to the top of the file |

## Words and operators

| Wrote | Code | Write instead |
| --- | --- | --- |
| `if not ok`, `a and b`, `a or b` | `syntax-error` | `!ok`, `a && b`, `a \|\| b` |
| `elif` | `syntax-error` | `else if` |
| `loop:` | `unknown-name` | `while true:` |
| `n as i32` | `syntax-error` | `i32(n)` |
| `xs.map(\|x\| x + 1)` | `syntax-error` | `xs.map(fn(x): x + 1)` |
| `.Left \| .Right => 1` | `syntax-error` | one arm per pattern, or `_ =>` for the rest |
| a variable named `type`, `in`, `is`, `mut`, `tests`, `dyn` or `pass` | `syntax-error` | another name: these are reserved words |

## Bindings

| Wrote | Code | Write instead |
| --- | --- | --- |
| `total: i64 = 0` | `missing-let` | `let total: i64 = 0` |
| `(a, b) := pair` | `missing-let` | `let (a, b) = pair` |
| `pub let LIMIT = 3`, `pub LIMIT := 3` | `syntax-error` | a private `LIMIT := 3`, plus `pub fn limit() -> usize: LIMIT` |

## Literals and records

| Wrote | Code | Write instead |
| --- | --- | --- |
| `"{x}"` | none: it prints `{x}` | `"$x"` or `"${x}"` |
| `pair.0` | `syntax-error` | `pair._0` |
| `{:}` for an empty map | `syntax-error` | `let mut m: Map[K, V] = {}` |
| `Item(name: "a")` | `trailing-block-position` | `Item { name: "a" }` |
| `P { ..p, x: 1 }` | `syntax-error` | `P { ...p, x: 1 }`, three dots |
| `data Item:` | `syntax-error` | `struct Item:` |
| `0xFFu64` | `syntax-error` | `let mask: u64 = 0xFF` |
| `$ Console, Clock` in a row | `syntax-error` | `$ Console + Clock` |
