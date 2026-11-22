# Mistakes: Types, Numbers, Bindings

What was written, the code `hd check` reports, and what to write instead.

## Numbers

| Wrote | Code | Write instead |
| --- | --- | --- |
| an `i32` plus `xs.len()` | `mixed-signedness` | keep the counter `usize`, or convert: `count + i32(xs.len())` |
| `limit < xs.len()` with `limit: i32` | `mixed-signedness` | declare `limit: usize`, or `usize(limit) < xs.len()` |
| `"count: " + n` | `type-mismatch` | `"count: $n"` |
| `isize`, `int`, `float` | `unknown-type` | `i64` or `usize`; `i32`; `f64` |
| `s.len()` used as a character count | none: wrong for non-ASCII text | `s.chars().count()` |

## Bindings

| Wrote | Code | Write instead |
| --- | --- | --- |
| `n := 0` then `n = n + 1` | `non-reassignable-binding` | `let n = 0` |
| `let mut n = 0` for a number | `redundant-let-mut`, from `hd lint` only | `let n = 0` |
| `n = 0` on a parameter `n` | `non-reassignable-parameter-binding` | copy it first: `let m = n` |
| `xs.pop()` or `m.remove(k)` as a statement | `discarded-must-use-value` | `_ := xs.pop()` |
| a function named `string`, `f32`, `println`, `List` | `prelude-name-shadow` | another name |

## Optionals and results

| Wrote | Code | Write instead |
| --- | --- | --- |
| `x.unwrap()` | `unknown-method` | `x.expect("why it is present")` or `x.unwrap_or(fallback)` |
| `None`, `Some(x)`, `Ok(x)`, `Err(e)` | `unknown-name` | `.None`, `.Some(x)`, `.Ok(x)`, `.Err(e)` |
| `"$opt"` with `opt: string?` | `unsatisfied-trait-bound` | `"${opt.unwrap_or("")}"` or `"${debug(opt)}"` |
| `"$xs"` with a list, map or struct | `unsatisfied-trait-bound` | `"${debug(xs)}"`, or `xs.join(", ")` |
| a function body that ends without `.Ok(...)` | `missing-return-value` | end with `.Ok(value)`, or `.Ok(())` for `Result[void, E]` |

## Structs, enums, traits

| Wrote | Code | Write instead |
| --- | --- | --- |
| `a == b` on your own struct | `type-mismatch` | add `@derive(Eq)` to the type |
| `@derive(Ord)` alone | `mixed-derived-law` | `@derive(Eq, PartialOrd, Ord)` |
| a `match` that misses a variant | `nonexhaustive-match` | add the arm, or `_ => ...` |
| `if ok: "yes" else: 0` | `type-mismatch` | both branches of one type |
| `List[Shape]` with a trait | `trait-used-as-type` | `List[*dyn Shape]`, filled with `box(Circle { ... })`; `&dyn Shape` for a parameter |
| `impl Add for Money` with no import | `unknown-trait` | `use std.ops.Add` |
| `type Output = Money` in an operator impl | `extra-trait-member` | `type Out = Money` |
| `holder.callback()` on a function-typed field | `unknown-method` | `(holder.callback)()` |

## Generic calls

| Wrote | Code | Write instead |
| --- | --- | --- |
| `ys := xs.iter().map(f).collect()` | `cannot-infer-type` | `let ys: List[i32] = ...collect()`, or `.collect::[List[i32]]()` |
| `xs.iter().sum()` with no expected type | `cannot-infer-type` | `xs.iter().sum::[i32]()` |
| `xs[0..2]`, `s[0..2]` | `type-mismatch` | `xs.slice(0, 2)`, `s.slice(0, 2)` |
