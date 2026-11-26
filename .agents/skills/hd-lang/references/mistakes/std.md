# Mistakes: Standard Library Names

Names carried over from Python, Rust or JavaScript that hd spells another
way, and the code `hd check` reports. When a name is not here, run
`hd doc std.MODULE` inside a package; the tables are in
[std/collections.md](../std/collections.md) and [std/text.md](../std/text.md).

## Lists and maps

| Wrote | Code | Write instead |
| --- | --- | --- |
| `xs.append(x)` for one element | `type-mismatch` | `xs.push(x)` |
| `xs.length`, `xs.size()` | `unknown-field`, `unknown-method` | `xs.len()` |
| `xs.remove_at(i)` | `unknown-method` | `xs.remove(i)` |
| `for i in range(3):` | `unknown-name` | `for i in 0..3:` |
| `m.insert(key, value)` as a statement | `discarded-must-use-value` | `m[key] = value`; `insert` returns the old value as `V?` |
| `ys := xs.sort()` or returning `xs.sort()` | `missing-return-value` or a `void` mismatch | `xs.sorted()` gives a new list; `xs.sort()` sorts in place |
| `Vec[T]`, `HashMap[K, V]`, `String` | `unknown-type` | `List[T]`, `Map[K, V]`, `string` |

## Strings

| Wrote | Code | Write instead |
| --- | --- | --- |
| `s.to_uppercase()`, `s.to_lowercase()` | `unknown-method` | `s.upper()`, `s.lower()` |
| `s[0]` | `type-mismatch` | `s.slice(0, 1)`, or walk `s.chars()` |
| `println("{}", n)` | `argument-count` | `println("$n")` |
| `i32(text)` to parse a string | `not-callable` | `text.parse::[i32]()`, which returns a `Result` |

## Imports

| Wrote | Code | Write instead |
| --- | --- | --- |
| `defer(...)` with no import | `unknown-name` | `use std.ops.defer` |
| `max(a, b)` with no import | `unknown-name` | `use std.cmp.max` |
| `sleep!(1ms)` with no import | `unknown-name` | `use std.time.{Clock, ms, sleep}` |
| `Fn() -> T` in a bound with no import | `unknown-trait` | `use std.ops.Fn` |
| `read_text!` imported as `use std.fs.read_text!` | `syntax-error` | `use std.fs.read_text`; the `!` is written only at the call |
