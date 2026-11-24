# Mistakes: Changing And Sharing Values

What was written, the code `hd check` reports, and what to write instead.
The rules are in [language/references.md](../language/references.md).

## `not-writable` and friends: the value cannot be changed here

| Wrote | Code | Write instead |
| --- | --- | --- |
| `let xs: List[string] = []` then `xs.push(x)` | `not-writable` | `let mut xs: List[string] = []` |
| `p := P { x: 1 }` then `p.x = 2` | `non-reassignable-binding` | `let mut p = P { x: 1 }` |
| `let p = P { x: 1 }` then `p.x = 2` | `not-writable` | `let mut p = ...` |
| `p.bump()` on a `:=` binding, where `bump` takes `&mut self` | `not-writable` | `let mut p = ...` |
| `xs.push(x)` on a parameter `xs: List[i32]` | `not-writable` | to change the caller's list: `xs: &mut List[i32]`, called as `f(&mut xs)`. To change a private copy: `let mut ys = xs` |
| `fn bump(self)` that writes `self.x` | `non-reassignable-parameter-binding` | `fn bump(&mut self)` |
| a write through a `&P` parameter | `not-writable` | take `&mut P` |
| `for task in tasks:` then `task.done = true` | `not-writable` | `for task in &mut tasks:` |
| `bump(p)` where `bump` takes `&mut P` | `type-mismatch` | `bump(&mut p)` |

## `local-reference-escapes`: a `&` reference was kept

| Wrote | Code | Write instead |
| --- | --- | --- |
| a `&P` or `&mut P` put in a field or a collection | `local-reference-escapes` | make the object a box where it is declared, `p := box(P { ... })`, and store the `*P` |
| a function parameter `f: fn() -> void` stored in a field | `local-reference-escapes` | a generic `F < FnMut() -> void & 'static` field, with `use std.ops.FnMut` |
| `$.with(Mailer=&mut fake)` | `local-reference-escapes` | `fake := box(Fake { ... })`, then `$.with(Mailer=fake)` |

## `conflicting-access`: changed while a reference is in use

| Wrote | Code | Write instead |
| --- | --- | --- |
| `r := &mut xs[0]`, then `xs.push(3)`, then `*r = 5` | `conflicting-access` | finish with `r` before changing the list, or keep the index and write `xs[0] = 5` |
| `f(&mut value, value.len())` | `conflicting-access` | copy what the second argument needs into a local first |

## Copies that surprise

| Wrote | Code | Write instead |
| --- | --- | --- |
| `shared := box(p)`, then changes through `shared`, then reads `p` | none: `box` copied, so `p` is unchanged | box at the declaration: `p := box(P { ... })`, and use `p` everywhere |
| `b := a`, then changes to `b`, expecting `a` to change | none: `b` is a copy | change `a` itself, or share one object with `box` |
| `a == b` on two boxes | `reference-equality` | `a is b` for the same object, `*a == *b` for equal values |
| `max(best, score)` with `score` from `for (_, score) in map` | `type-mismatch` | `max(best, *score)`: a map loop lends references |
