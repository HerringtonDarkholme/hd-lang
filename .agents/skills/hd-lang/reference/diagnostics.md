# Diagnostics: Code, Cause, Fix

Look up the code that `hd check` reports, as in
`error: cart.hd:4:13: trait-used-as-type: ...`. Each row is what
was written, and what to write instead. When the code is not here, read the
message and its `fix:` line first; then the page that owns the topic, or
`hd doc` for a std name ([explore](explore.md)).

Fix the first error first: later ones are often follow-ons. `syntax-error`
covers many causes, so match on the middle column.

| Code | Wrote | Write instead |
| --- | --- | --- |
| "package has no executable to run" | `main` in a module other than `src/main.hd` | move `main` to `src/main.hd`, or add an `[[executable]]` table |
| `argument-count` | `println("{}", n)` | `println("$n")` |
| `bang-call-outside-suspension` | `read_line!()` inside `fn main()` | `fn main!()`; every caller up the chain gets the `!` too |
| `cannot-infer-type` | `xs.iter().sum()` with no expected type | `xs.iter().sum::[i32]()` |
| `cannot-infer-type` | `ys := xs.iter().map(f).collect()` | `let ys: List[i32] = ...collect()`, or `.collect::[List[i32]]()` |
| `conflicting-access` | `f(&mut value, value.len())` | copy what the second argument needs into a local first |
| `conflicting-access` | `r := &mut xs[0]`, then `xs.push(3)`, then `*r = 5` | finish with `r` before changing the list, or keep the index and write `xs[0] = 5` |
| `discarded-must-use-value` | `m.insert(key, value)` as a statement | `m[key] = value`; `insert` returns the old value as `V?` |
| `discarded-must-use-value` ("a Suspend value is discarded") | `sleep(1ms)` without the `!` | `sleep!(1ms)` |
| `discarded-must-use-value` | `xs.pop()` or `m.remove(k)` as a statement | `_ := xs.pop()` |
| `duplicate-tests-block` | a second `tests:` block | one `tests:` block per file |
| `extra-trait-member` | `type Output = Money` in an operator impl | `type Out = Money` |
| `folder-cycle` | `src/a/` imports `src/b/` and `src/b/` imports `src/a/` | move the shared code into a folder both import |
| `invalid-result-propagation` | `?` inside `main!() -> void` | return `Result[void, E]` and end with `.Ok(())` |
| `local-reference-escapes` | `$.with(Mailer=&mut fake)` | `fake := box(Fake { ... })`, then `$.with(Mailer=fake)` |
| `local-reference-escapes` | a `&P` or `&mut P` put in a field or a collection | make the object a box where it is declared, `p := box(P { ... })`, and store the `*P` |
| `local-reference-escapes` | a function parameter `f: fn() -> void` stored in a field | a generic `F < FnMut() -> void & 'static` field, with `use std.ops.FnMut` |
| `member-not-derivable` | `@derive(Validate)` with a field of your own type | `@derive(Validate)` on that type too |
| `misplaced-tests-block` | a `tests:` block in `tests/NAME.hd` | top-level `it(...)` cases |
| `missing-capability` | `main` row names a capability missing from `hd.toml` | add `FsRead = true` under `[capabilities]`, or run `hd check --fix` |
| `missing-entry-point` when run; `hd check` accepts it | `fn main()` without `pub` | `pub fn main()` |
| `missing-let` | `(a, b) := pair` | `let (a, b) = pair` |
| `missing-let` | `total: i64 = 0` | `let total: i64 = 0` |
| `missing-requirement` ("the runner does not bind") | `println` reached from a unit test | install `MemoryConsole::new()` with `$.with(Console=console):` |
| `missing-requirement` | `pub fn main() -> void:` that calls `println` | `pub fn main() -> void $ Console:` |
| `missing-requirement` | a helper that prints, with no row | add `$ Console` to it and to every function that calls it |
| `missing-requirement` | a top-level `println` in a file with tests, under `hd test` | put the script body in `pub fn main() -> void $ Console:` |
| `missing-return-value` or a `void` mismatch | `ys := xs.sort()` or returning `xs.sort()` | `xs.sorted()` gives a new list; `xs.sort()` sorts in place |
| `missing-return-value` | a `Result`-returning `main!` with no final `.Ok(())` | end with `.Ok(())` |
| `missing-return-value` | a function body that ends without `.Ok(...)` | end with `.Ok(value)`, or `.Ok(())` for `Result[void, E]` |
| `mixed-derived-law` | `@derive(Ord)` alone | `@derive(Eq, PartialOrd, Ord)` |
| `mixed-signedness` | `limit < xs.len()` with `limit: i32` | declare `limit: usize`, or `usize(limit) < xs.len()` |
| `mixed-signedness` | an `i32` plus `xs.len()` | keep the counter `usize`, or convert: `count + i32(xs.len())` |
| `non-reassignable-binding` | `n := 0` then `n = n + 1` | `let n = 0` |
| `non-reassignable-binding` | `p := P { x: 1 }` then `p.x = 2` | `let mut p = P { x: 1 }` |
| `non-reassignable-parameter-binding` | `fn bump(self)` that writes `self.x` | `fn bump(&mut self)` |
| `non-reassignable-parameter-binding` | `n = 0` on a parameter `n` | copy it first: `let m = n` |
| `nonexhaustive-match` | a `match` that misses a variant | add the arm, or `_ => ...` |
| `not-callable` | `i32(text)` to parse a string | `text.parse::[i32]()`, which returns a `Result` |
| `not-writable` | `for task in tasks:` then `task.done = true` | `for task in &mut tasks:` |
| `not-writable` | `let p = P { x: 1 }` then `p.x = 2` | `let mut p = ...` |
| `not-writable` | `let xs: List[string] = []` then `xs.push(x)` | `let mut xs: List[string] = []` |
| `not-writable` | `p.bump()` on a `:=` binding, where `bump` takes `&mut self` | `let mut p = ...` |
| `not-writable` | `xs.push(x)` on a parameter `xs: List[i32]` | to change the caller's list: `xs: &mut List[i32]`, called as `f(&mut xs)`. To change a private copy: `let mut ys = xs` |
| `not-writable` | a write through a `&P` parameter | take `&mut P` |
| `pattern-arity` | `.Rect(_)` for a variant with two fields | one pattern per field: `.Rect(_, _)` |
| `prelude-name-shadow` | a function named `string`, `f32`, `println`, `List` | another name |
| `private-import` | importing a name that is not `pub` | mark it `pub` |
| `private-member` | reading a private field, or building a struct that has one, from another module | mark the field `pub`, or add a `pub fn` constructor |
| `private-type-leak` | a private trait in a `pub fn` row | `pub trait` |
| `redundant-let-mut`, from `hd lint` only | `let mut n = 0` for a number | `let n = 0` |
| `reference-equality` | `a == b` on two boxes | `a is b` for the same object, `*a == *b` for equal values |
| `reserved-semicolon` | `let x = 1;` | no semicolons; one statement per line |
| `syntax-error` | `$ Console, Clock` in a row | `$ Console + Clock` |
| `syntax-error` | `.Left \| .Right => 1` | one arm per pattern, or `_ =>` for the rest |
| `syntax-error` | `.Some(v) => { v }` in a `match` | `.Some(v) => v`, or an indented body under the arm |
| `syntax-error` | `0xFFu64` | `let mask: u64 = 0xFF` |
| `syntax-error` | `data Item:` | `struct Item:` |
| `syntax-error` | `elif` | `else if` |
| `syntax-error`, `unclosed-delimiter` | `fn f() -> i32 {` ... `}` | `fn f() -> i32:` and an indented body |
| `syntax-error` | `if not ok`, `a and b`, `a or b` | `!ok`, `a && b`, `a \|\| b` |
| `syntax-error` | `n as i32` | `i32(n)` |
| `syntax-error` | `P { ..p, x: 1 }` | `P { ...p, x: 1 }`, three dots |
| `syntax-error` | `pair.0` | `pair._0` |
| `missing-binding-type` | `pub let LIMIT = 3` | write the public binding's type: `pub let LIMIT: usize = 3` |
| `syntax-error` | `pub LIMIT := 3` | write `pub let LIMIT: usize = 3` |
| `syntax-error` | `pub use self.format.receipt` | `pub use self.format.{receipt}` |
| `syntax-error` | `read_text!` imported as `use std.fs.read_text!` | `use std.fs.read_text`; the `!` is written only at the call |
| `syntax-error` | `trait Marker: pass` or `impl T for X: pass` on one line | put `pass` on its own indented line; only `struct Name: pass` fits one line |
| `syntax-error` | `use std.cmp.max` inside a function | move it to the top of the file |
| `syntax-error` | `xs.map(\|x\| x + 1)` | `xs.map(fn(x): x + 1)` |
| `syntax-error` | `{:}` for an empty map | `let mut m: Map[K, V] = {}` |
| `syntax-error` | a long expression continued on the next line, as `a` newline `&& b` | wrap it in parentheses: `(a` newline `&& b)` |
| `syntax-error` | a rest pattern, `.Rect(..)` | hd has no rest pattern: write `_` for each field, `.Rect(_, _)` |
| `syntax-error` | a variable named `type`, `in`, `is`, `mut`, `tests`, `dyn` or `pass` | another name: these are reserved words |
| `trailing-block-position` | `Item(name: "a")` | `Item { name: "a" }` |
| `trait-method-signature` | an `impl` method with a row its trait does not declare | declare the same row on the trait's method |
| `trait-used-as-type` | `List[Shape]` with a trait | `List[*dyn Shape]`, filled with `box(Circle { ... })`; `&dyn Shape` for a parameter |
| `type-mismatch` | `"count: " + n` | `"count: $n"` |
| `type-mismatch` | `$.with(Mailer=fake)` with `fake` a plain struct value | `fake := box(Fake { ... })` |
| `type-mismatch` | `a == b` on your own struct | add `@derive(Eq)` to the type |
| `type-mismatch` | `bump(p)` where `bump` takes `&mut P` | `bump(&mut p)` |
| `type-mismatch` | `if ok: "yes" else: 0` | both branches of one type |
| `type-mismatch` | `join!(first!(), second!())` | `join!(first(), second())`: no `!` on the arguments |
| `type-mismatch` | `max(best, score)` with `score` from `for (_, score) in map` | `max(best, *score)`: a map loop lends references |
| `type-mismatch` | `s[0]` | `s.slice(0, 1)`, or walk `s.chars()` |
| `type-mismatch` | `xs.append(x)` for one element | `xs.push(x)` |
| `type-mismatch` | `xs[0..2]`, `s[0..2]` | `xs.slice(0, 2)`, `s.slice(0, 2)` |
| `type-mismatch` ("the function's row lists `Console`") | a callback that prints, passed for `fn() -> void` | declare the parameter `fn() -> void $ Console` |
| `underivable-trait` | `@derive(MyTrait)` for your own trait, with no template | write the template beside the trait: `impl[T] MyTrait for T by Structure:` ([derive](../design/derive.md#a-template)) |
| `unknown-field`, `unknown-method` | `xs.length`, `xs.size()` | `xs.len()` |
| `unknown-method` | `holder.callback()` on a function-typed field | `(holder.callback)()` |
| `unknown-method` | `s.to_uppercase()`, `s.to_lowercase()` | `s.upper()`, `s.lower()` |
| `unknown-method` | `xs.remove_at(i)` | `xs.remove(i)` |
| `unknown-module` | `use pkg...` or `use dep...` in a file run with `hd FILE.hd` | run it inside a package, with `hd run` |
| `unknown-name` | `@range(1, 9)` or `@rename("x")` with no import | `use std.validate.range`, `use std.encoding.rename` |
| `unknown-name` | `assert_equal(...)` outside test code | move it into `tests:`; for a runtime check use `if ...: panic("...")` |
| `unknown-name` | `defer(...)` with no import | `use std.ops.defer` |
| `unknown-name` | `for i in range(3):` | `for i in 0..3:` |
| `unknown-name` | `loop:` | `while true:` |
| `unknown-name` | `max(a, b)` with no import | `use std.cmp.max` |
| `unknown-name` | `None`, `Some(x)`, `Ok(x)`, `Err(e)` | `.None`, `.Some(x)`, `.Ok(x)`, `.Err(e)` |
| `unknown-name` | `sleep!(1ms)` with no import | `use std.time.{Clock, ms, sleep}` |
| `unknown-trait` | `$ Console + Clock` with no import | `use std.time.Clock`; only `Console` needs no import |
| `unknown-trait` | `@derive(Cli)` with no import | `use std.cli.Cli`; each derive outside the prelude imports its trait |
| `unknown-trait` | `@derive(Clone)` | nothing: values copy |
| `unknown-trait` | `Fn() -> T` in a bound with no import | `use std.ops.Fn` |
| `unknown-trait` | `impl Add for Money` with no import | `use std.ops.Add` |
| `unknown-type` | `isize`, `int`, `float` | `i64` or `usize`; `i32`; `f64` |
| `unknown-type` | `Vec[T]`, `HashMap[K, V]`, `String` | `List[T]`, `Map[K, V]`, `string` |
| `unregistered-auto-closing` | `_ := ensure(...)`, or `guard := ensure(...)` | `ensure("message"):` on its own line, then the condition ([contracts](../design/contracts.md)) |
| `unsatisfied-trait-bound` | `"$opt"` with `opt: string?` | `"${opt.unwrap_or("")}"` or `"${debug(opt)}"` |
| `unsatisfied-trait-bound` | `"$xs"` with a list, map or struct | `"${debug(xs)}"`, or `xs.join(", ")` |
| `unsatisfied-trait-bound` | `assert_equal(a, b)` on your own type | `@derive(Eq, Debug)` on the type |
| `unsatisfied-trait-bound` | `decode::[Plain](text)` on a struct with no derive | `@derive(Deserialize)` on it, with `use std.encoding.Deserialize`; `Serialize` for `encode` |
| `unsatisfied-trait-bound` | `r.unwrap()` with unconstrained `Result[T, E]` | Add `E < Debug`, or use `r.unwrap_or(fallback)` without formatting the error |

## No diagnostic: wrong when it runs

These check, then print or compute the wrong thing.

| Wrote | What happens | Write instead |
| --- | --- | --- |
| `shared := box(p)`, then changes through `shared`, then reads `p` | `box` copied, so `p` is unchanged | box at the declaration: `p := box(P { ... })`, and use `p` everywhere |
| `b := a`, then changes to `b`, expecting `a` to change | `b` is a copy | change `a` itself, or share one object with `box` |
| `"{x}"` | it prints `{x}` | `"$x"` or `"${x}"` |
| `s.len()` used as a character count | wrong for non-ASCII text | `s.chars().count()` |
| a hand-written `impl Eq`, `Debug`, `Serialize` or parser for a plain struct | more code to keep in step | `@derive(...)` ([derive](../design/derive.md)) |
