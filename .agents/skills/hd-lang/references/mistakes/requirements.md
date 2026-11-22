# Mistakes: Requirements, Suspension, Tests, Modules

What was written, the code `hd` reports, and what to write instead.

## Rows after `$`

| Wrote | Code | Write instead |
| --- | --- | --- |
| `pub fn main() -> void:` that calls `println` | `missing-requirement` | `pub fn main() -> void $ Console:` |
| a helper that prints, with no row | `missing-requirement` | add `$ Console` to it and to every function that calls it |
| `$ Console + Clock` with no import | `unknown-trait` | `use std.time.Clock`; only `Console` needs no import |
| a callback that prints, passed for `fn() -> void` | `type-mismatch` ("the function's row lists `Console`") | declare the parameter `fn() -> void $ Console` |
| an `impl` method with a row its trait does not declare | `trait-method-signature` | declare the same row on the trait's method |
| a private trait in a `pub fn` row | `private-type-leak` | `pub trait` |
| `$.with(Mailer=fake)` with `fake` a plain struct value | `type-mismatch` | `fake := box(Fake { ... })` |
| `fn main()` without `pub` | `missing-entry-point` when run; `hd check` accepts it | `pub fn main()` |
| `main` row names a capability missing from `hd.toml` | `missing-capability` | add `FsRead = true` under `[capabilities]`, or run `hd check --fix` |
| `main` in a module other than `src/main.hd` | "package has no executable to run" | move `main` to `src/main.hd`, or add an `[[executable]]` table |

## `!` calls

| Wrote | Code | Write instead |
| --- | --- | --- |
| `read_line!()` inside `fn main()` | `bang-call-outside-suspension` | `fn main!()`; every caller up the chain gets the `!` too |
| `sleep(1ms)` without the `!` | `discarded-must-use-value` ("a Suspend value is discarded") | `sleep!(1ms)` |
| `join!(first!(), second!())` | `type-mismatch` | `join!(first(), second())`: no `!` on the arguments |
| `?` inside `main!() -> void` | `invalid-result-propagation` | return `Result[void, E]` and end with `.Ok(())` |
| a `Result`-returning `main!` with no final `.Ok(())` | `missing-return-value` | end with `.Ok(())` |

## Tests

| Wrote | Code | Write instead |
| --- | --- | --- |
| a second `tests:` block | `duplicate-tests-block` | one `tests:` block per file |
| `assert_equal(...)` outside test code | `unknown-name` | move it into `tests:`; for a runtime check use `if ...: panic("...")` |
| a top-level `println` in a file with tests, under `hd test` | `missing-requirement` | put the script body in `pub fn main() -> void $ Console:` |
| `println` reached from a unit test | `missing-requirement` ("the runner does not bind") | install `MemoryConsole::new()` with `$.with(Console=console):` |
| a `tests:` block in `tests/NAME.hd` | `misplaced-tests-block` | top-level `it(...)` cases |
| `assert_equal(a, b)` on your own type | `unsatisfied-trait-bound` | `@derive(Eq, Debug)` on the type |

## Modules

| Wrote | Code | Write instead |
| --- | --- | --- |
| importing a name that is not `pub` | `private-import` | mark it `pub` |
| reading a private field, or building a struct that has one, from another module | `private-member` | mark the field `pub`, or add a `pub fn` constructor |
| `pub use self.format.receipt` | `syntax-error` | `pub use self.format.{receipt}` |
| `src/a/` imports `src/b/` and `src/b/` imports `src/a/` | `folder-cycle` | move the shared code into a folder both import |
| `use pkg...` or `use dep...` in a file run with `hd FILE.hd` | `unknown-module` | run it inside a package, with `hd run` |
