# Modules

Status: language specification draft.

This chapter defines modules, packages, the prelude, program initialization,
entry points, and the Wasm boundary.

1. r[module.kind.module] Modules organize names by source path.
2. r[module.kind.package] Packages organize compilation, dependencies, and Wasm artifacts.

## Package Manifest

An hd-lang package has an `hd.toml` manifest:

```toml
[package]
name = "my_app"
version = "0.1.0"

[source]
root = "src"

[dependencies]
billing = "1.2.0"
```

1. r[module.manifest.file] An hd-lang package has an `hd.toml` manifest.
2. r[module.manifest.source-root] The default source root is `src`.
3. r[module.manifest.tooling] The complete manifest schema, dependency resolution algorithm, lockfile, and version semantics remain tooling work.
4. r[module.manifest.dependency] The language-level use model assumes that the manifest maps each dependency name to one resolved package.

See also: [Tooling, ABI, And Unsupported Extensions](#tooling-abi-and-unsupported-extensions).

## Path-Inferred Modules

A source file's path relative to the source root determines its module name:

```text
src/user/types.hd    # user.types
src/user/service.hd  # user.service
```

1. r[module.path.name] A source file's path relative to the source root determines its module name.
2. r[module.path.no-declaration] There is no `module` or `package` declaration in source.

### Directory Modules

A directory is itself a module only when it contains `mod.hd`:

```text
src/user/mod.hd      # user
src/user/types.hd    # user.types
```

1. r[module.path.directory] A directory is itself a module only when it contains `mod.hd`.
2. r[module.path.mod-file] `mod.hd` is the directory module's source file and public index.
3. r[module.path.no-child-import] Child modules are not brought automatically into the parent.
4. r[module.path.no-parent-scope] Parent declarations are not implicitly visible in children.

### Module Identity

1. r[module.path.unique] Two files must not map to the same module identity.
2. r[module.path.case-source] Case sensitivity of module paths is defined by the source language rather than the host filesystem, by the rules below.
3. r[module.path.identifier] Every non-`mod.hd` path component used in a module identity must be an NFC Unicode identifier under the source identifier rules.
4. r[module.path.case-sensitive] Module identities are case-sensitive.
5. r[module.path.case-collision] A package is rejected if two source paths collide after full Unicode case folding followed by NFC normalization of each module path component. This holds even when the host filesystem could otherwise distinguish them.
6. r[module.path.suffix] The final `.hd` suffix and the special filename `mod.hd` are lowercase and case-sensitive.
7. r[module.path.unicode-version] The compiler applies the declared Unicode data version from the lexical rules to path validation and collision checks.

> **Why.** Checking paths this way rejects ambiguous module names before
> host filesystem case behavior can change the module graph.

### Test Modules

A file whose name ends in `_test.hd` is a test module, and the test root
holds integration tests:

```text
src/billing.hd         # billing
src/billing_test.hd    # billing_test, a test module
tests/checkout.hd      # an integration test module
```

1. r[module.test.module] A source file whose name ends in `_test.hd` is a **test module**, such as `src/billing_test.hd`, whose module is `billing_test`.
2. r[module.test.integration] An **integration test module** is a module under the package's test root, which is `tests` by default.
3. r[module.test.code] **Test code** is a package's `tests:` blocks, test modules, and integration test modules. Only a test build, such as `hd test` makes, compiles it.
4. r[module.test.module.view] A test module is otherwise an ordinary module of its package: it sees public declarations package-wide and may use other test modules.
5. r[module.test.integration.view] An integration test module sees the package as a dependent package does: its public declarations, built without its test code.
6. r[module.test.integration.uses] Integration test modules may use one another.
7. r[module.test.dependency] A **test dependency** is a dependency that the manifest declares for test builds only. Only test code may use it.
8. r[module.test.non-test-use] Non-test code that uses a test module or a test dependency is an error. Error: `test-only-use`.
9. r[module.test.cyclic-dependency] A test dependency that itself depends on the package may be used only from integration test modules. Using it from a `tests:` block or a test module is an error. Error: `cyclic-test-dependency`.

> **Why.** A test dependency that depends back would give a unit test a
> second copy of the package, whose types differ from the ones under test.
> An integration test sees only the one normal build.

See also: [Test Blocks](02-grammar.md#test-blocks),
[Standard Testing](#standard-testing).

## Use Roots

Every absolute use path begins with one of these roots:

| Root | Names |
| --- | --- |
| `pkg` | the current package |
| `std` | the standard library |
| `dep.<name>` | a manifest dependency |

1. r[module.root.absolute] Every absolute use path begins with one of the roots in the table.
2. r[module.root.manifest] The package manifest distinguishes standard library, current package, and external dependency namespaces.
3. r[module.root.dep-prefix] Source syntax does not require a different prefix for each dependency beyond `dep.<name>`.

### Relative Uses

Relative use paths use `self` and `super`:

```text
use self.types.{User, UserId}
use super.shared.{Email}
```

Relative lookup starts at a base that depends on the source file:

| Source file | Base |
| --- | --- |
| `src/user/service.hd`, a regular file | `user` |
| `src/user/mod.hd` | the `user` module itself |
| a file directly under `src` | the package root namespace |

1. r[module.relative.keywords] Relative use paths use `self` and `super`.
2. r[module.relative.base] Relative lookup starts at the source file's containing directory module, as the table shows.
3. r[module.relative.self] `self` names that base.
4. r[module.relative.super] Each leading `super` moves to its parent.
5. r[module.relative.example] Consequently, from `src/user/service.hd`, `self.types` resolves to `pkg.user.types` and `super.shared` resolves to `pkg.shared`.
6. r[module.relative.above-root] Moving above the package root is a compile-time error.
7. r[module.relative.no-cross] Relative use paths cannot cross into `std` or a dependency.

## Use Forms

A use names a single public declaration or a module namespace:

```text
use std.time.Duration
use pkg.user.types
use pkg.user.types as user_types
```

A grouped use names selected public declarations:

```text
use pkg.user.types.{User, UserId}
use dep.billing.types.{UserId as BillingUserId}
```

1. r[module.use.single] A use may name a single public declaration or a module namespace.
2. r[module.use.grouped] A grouped use names selected public declarations.
3. r[module.use.trailing-comma] Grouped uses may have a trailing comma.
4. r[module.use.no-wildcard] Wildcard uses are not supported.
5. r[module.use.no-variant] Enum variants are members, not module declarations, and cannot be used directly.
6. r[module.use.private-or-missing] Using a private or missing declaration is a compile-time error.
7. r[module.use.pub-grouped] Only the grouped form accepts a `pub` prefix.
8. r[module.use.facade] Public facades expose selected declarations rather than module namespace aliases.
9. r[module.use.whole-module] Use declarations introduce names for the whole module and are resolved before type checking.

```text
use std.testing.*                 # error
use pkg.status.{Status.Queued}    # error
pub use std.testing.assert_equal  # error
```

## Prelude

The **prelude** is the implicit scope of public standard-library names that
every module has:

| Origin module | Implicit names |
| --- | --- |
| `std.core` | `never`, `bool`, `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, `f32`, `f64`, `char`, `string`, `void`, `List`, `Map`, `Any`, `AnyVal`, `AnyRef`, `Option`, `Result`, `panic` |
| `std.format` | `Display` |
| `std.cmp` | `Eq`, `PartialOrd`, `Ord`, `Ordering` |
| `std.hash` | `Hash`, `Hasher` |
| `std.iter` | `Iterator`, `Iterable` |
| `std.console` | `Console`, `ConsoleError`, `println` |
| `std.testing` | `it` |
| `std.task` | `Suspend`, `Poll`, `PollContext`, `Waker` |
| `std.annotation` | `ShapeMetadata`, `DeclarationId`, `DeclarationKind`, `PrimitiveKind`, `SourcePosition`, `TypeShape`, `DataShape`, `FieldShape`, `EnumShape`, `VariantShape`, `FnShape`, `ParamShape`, `shape`, `shape_of` |

1. r[module.prelude.names] Every module implicitly has the public standard-library names in the table in scope.
2. r[module.prelude.fixed-uses] The prelude is equivalent to fixed `use` declarations.
3. r[module.prelude.no-authority] The prelude does not create ambient host authority.
4. r[module.prelude.no-shadow] A module declaration, use, type parameter, parameter, or local binding must not shadow a prelude name. Every conflict is an error. Error: `prelude-name-shadow`.
5. r[module.prelude.no-reimport] This includes a redundant `use` that names the same declaration already supplied by the prelude: prelude names are used directly and are not re-imported. Error: `prelude-name-shadow`.

```text
use std.format.Display                               # error: prelude-name-shadow

fn println() -> void: pass                           # error: prelude-name-shadow
fn consume(Console: i32) -> i32: Console             # error: prelude-name-shadow
fn identity[Result](value: Result) -> Result: value  # error: prelude-name-shadow

fn main() -> i32:
    let Hash: i32 = 42  # error: prelude-name-shadow
    Hash
```

### Standard Names Outside The Prelude

1. r[module.prelude.imported] Standard traits outside this table are imported.
2. r[module.prelude.from-error] The conversion trait `std.convert.From` and the error trait `std.error.Error` are not prelude names.
3. r[module.prelude.own-from-error] A module may therefore declare its own `From` or `Error`.
4. r[module.prelude.question-from] Postfix `?` still finds the standard `From` without an import.
5. r[module.prelude.inspect] Likewise `std.inspect` declares `Inspectable`, `TypeId`, and `downcast_val`, which code imports, as in `use std.inspect.{Inspectable, TypeId}`.
6. r[module.prelude.error-inspectable] `std.error.Error` extends `Inspectable` without its users importing it.
7. r[module.prelude.function] `std.function` declares the function type constructors `Fn` and `SuspendFn` and the vararg marker `Rest`, which code imports where it writes them, as in `use std.function.{Fn, SuspendFn}`.
8. r[module.prelude.function-sugar] The function type sugar `fn(...) -> T` needs no import.

See also: [Conversion Trait](09-traits.md#conversion-trait),
[Function Type Constructors](07-functions.md#function-type-constructors),
[Error Trait](09-traits.md#error-trait),
[Runtime Type Identity](09-traits.md#runtime-type-identity).

### Collection Type Names

1. r[module.prelude.collections] The built-in collection types are `List` and `Map`.
2. r[module.prelude.capitalized] Like every nominal type outside the primitives, they are capitalized.
3. r[module.prelude.lowercase] The lowercase names `list` and `map` are not prelude names; they resolve like any other identifier.
4. r[module.prelude.lowercase-unknown] A type written `list[i32]` is therefore an error unless a declaration in scope supplies `list`. Error: `unknown-type`.

```text
fn count() -> i32:
    let values: list[i32] = [1, 2]  # error: unknown-type
    return 2
```

### Prelude Functions

1. r[module.prelude.panic] The prelude function `panic` has the signature `panic(message: string) -> never`.
2. r[module.prelude.println] The prelude function `println` has the signature `println[T < Display](value: T) -> void $ Console`.
3. r[module.prelude.shape] `shape` and `shape_of` are compiler intrinsics whose result types depend on their arguments.
4. r[module.prelude.it] `it` is the test-case intrinsic that [Test Cases](#test-cases) specifies.

See also: [Shape Intrinsics](14-annotations.md#shape-intrinsics), which
specifies `shape` and `shape_of`.

### Console

The standard console surface includes:

```text
trait Console:
    fn write_line!(mut self, text: string) -> Result[void, ConsoleError]
```

1. r[module.console.host-trait] `Console` is a host capability trait.
2. r[module.console.write-line-mut] `write_line!` takes `mut self`, so `Console` is a mutable requirement trait and a provider may record what it writes.
3. r[module.console.error] `ConsoleError` is its standard boundary-safe error type, and `ConsoleError` implements `Display`.
4. r[module.console.println] Thus `println` is convenient to name but not a global host API. Each call must be covered by a `Console` requirement row or a lexical provider scope.

```text
pub fn main() -> void:
    println("missing")  # error
```

### Value-Category Traits

1. r[module.prelude.any-subtraits] `AnyVal` and `AnyRef` are the two sealed marker subtraits of `Any`.
2. r[module.prelude.anyref] `AnyRef` is implemented by data values, stored enum values (optionals included), lists, maps, dynamic trait values, and `Any`. It is also implemented by closures, suspensions, and runtime handles that have identity, and payload-free enum values with canonical variant identity.
3. r[module.prelude.anyref-not] `AnyRef` is not implemented by primitives or tuples.
4. r[module.prelude.anyval-types] `AnyVal` is implemented by exactly the primitives, `void`, tuples, and newtypes whose base type implements `AnyVal`.
5. r[module.prelude.newtype-category] A newtype implements `AnyRef` exactly when its base type does.
6. r[module.prelude.never-exempt] `never` implements neither, because it has no values.
7. r[module.prelude.any-sealed] User code cannot implement either.

See also: [Trait Values And `Any`](04-type-system.md#trait-values-and-any),
[Sealed Traits](09-traits.md#sealed-traits).

### Built-In Methods

The following built-in methods are normative:

| Receiver | Methods |
| --- | --- |
| `string` | `len(self) -> i32`; `trim(self) -> string`; `lower(self) -> string`; `split(self, separator: string) -> List[string]`; `replace(self, old: string, replacement: string) -> string`; `starts_with(self, prefix: string) -> bool` |
| `List[T]` | `len(self) -> i32`; `iter(self) -> mut Iterator[T]`; `map[U](self, transform: fn(T) -> U) -> List[U]` |
| `mut List[T]` | `append(mut self, value: T) -> void` plus the readonly methods |
| `Map[K, V]` | `len(self) -> i32`; `get(self, key: K) -> V?` |
| `mut Map[K, V]` | `remove(mut self, key: K) -> V?` plus the readonly methods |
| `T?` | `map[U](self, transform: fn(T) -> U) -> U?` |
| `Display` | `to_string(self) -> string` |

1. r[module.method.normative] The built-in methods in the table are normative.
2. r[module.method.i32] Lengths and scalar positions use `i32`.
3. r[module.method.map] `List.map` and optional `map` are non-suspending and evaluate the transform in source order.
4. r[module.method.no-set] No `set` type is part of the core prelude.

#### Map Complexity

1. r[module.map.complexity] Map lookup, insertion, and removal take expected amortized O(1) time.
2. r[module.map.complexity.operations] This covers `get`, `remove`, reading `entries[key]`, and inserting or replacing through `entries[key] = value`.
3. r[module.map.complexity.step] Each call of the key type's `Hash` or `Eq` implementation counts as one step.

See also: [Indexing](05-expressions.md#indexing).

#### String Methods

1. r[module.string.scalar] String methods operate on Unicode scalar-value strings without locale.
2. r[module.string.lower] `lower` uses Unicode Default Case Conversion with full mappings.
3. r[module.string.trim] `trim` removes the Unicode `White_Space` property at both ends.
4. r[module.string.split] `split(separator)` retains empty pieces between adjacent separators and at either end.
5. r[module.string.split.empty-separator] An empty separator splits into one-scalar strings, with an empty input producing an empty list.
6. r[module.string.split.absent] With a non-empty separator, an input without that separator, including the empty string, yields one piece, so `"".split(",")` is `[""]`.
7. r[module.string.replace] `replace` replaces non-overlapping matches from left to right.
8. r[module.string.replace.empty] An empty `old` inserts the replacement at scalar boundaries.
9. r[module.string.starts-with] `starts_with` compares scalar sequences exactly and performs no normalization or case folding.

## Standard Testing

`std.testing` exports these normative assertion functions:

```text
fn assert(condition: bool, reason: string) -> void
fn assert_equal[T < Eq](actual: T, expected: T, reason: string) -> void
```

1. r[module.testing.exports] `std.testing` exports the normative assertion functions `assert` and `assert_equal` with the signatures above.
2. r[module.testing.reason] `reason` is required and must explain the checked condition.
3. r[module.testing.assert-panic] A failed assertion causes a runtime panic, inside a test case or not. Panic: `assertion-failed`.
4. r[module.testing.uses-eq] `assert_equal` uses `Eq.eq`.
5. r[module.testing.no-implicit-eq] `assert_equal` does not grant implicit equality to its argument type.

```text
use std.testing.assert_equal

data Error:
    message: string

tests:
    it("result equality needs Eq"):
        let actual: Result[i32, Error] = .Ok(1)
        assert_equal(actual, .Ok(1), reason="values match")  # error
```

### Test Cases

A call of the prelude intrinsic `it` registers one test case:

```text
use std.testing.assert_equal

fn add(a: i32, b: i32) -> i32: a + b

tests:
    it("adds two values"):
        assert_equal(add(2, 3), 5, reason="small sums")

    it("adds large values", ignore="slow on shared runners"):
        assert_equal(add(1000, 2000), 3000, reason="large sums")
```

1. r[module.testing.it] `it` is a compiler intrinsic that `std.testing` declares and the prelude supplies. Each call registers one **test case**.
2. r[module.testing.it.form] A call passes the test name as its one positional argument, then optional named options, then the body as its final argument, usually as a trailing block.
3. r[module.testing.it.body] The body has type `fn!() -> T $ R` with `T < std.process.Termination`, so a trailing block body is a suspending closure.
4. r[module.testing.it.name] The name must be a string literal without interpolation. Any other name is an error. Error: `non-literal-test-argument`.
5. r[module.testing.it.options] The named options are those in the table below, and each value must be a string literal without interpolation. Any other value is an error. Error: `non-literal-test-argument`.
6. r[module.testing.it.unknown-option] Any other named argument is an error. Error: `unknown-named-argument`.
7. r[module.testing.it.statements] Every top-level statement of a `tests:` block or a [test module](#test-modules) must be a call of `it` or of `std.testing.it_each`. Any other statement is an error. Error: `invalid-test-statement`.
8. r[module.testing.it.elsewhere] A call of `it` anywhere else is an error. Error: `misplaced-test-case`.
9. r[module.testing.it.unique] Two test cases of one module must not have the same name. Error: `duplicate-test-name`.

| Rule | Option | Value | Effect |
| --- | --- | --- | --- |
| r[module.testing.option.ignore] Ignore | `ignore` | a reason | The runner does not run the test case and reports it as ignored, with the reason. |
| r[module.testing.option.expect-panic] Expected panic | `expect_panic` | a [panic category](06-control-flow.md#panic-categories) | The test case passes only when its body panics with that category. |
| r[module.testing.option.timeout] Timeout | `timeout` | a duration, such as `"5s"` | The runner fails the test case when its body runs longer than the duration. |

```text
fn name_of() -> string: "computed"

tests:
    let shared: i32 = 0  # error: invalid-test-statement

    it("counts"):
        pass

    it("counts"):  # error: duplicate-test-name
        pass

    it(name_of()):  # error: non-literal-test-argument
        pass

fn helper() -> void:
    it("nested"):  # error: misplaced-test-case
        pass
```

> **Note.** `it` is a prelude name, so a module cannot declare, use, or bind
> another `it` ([Prelude](#prelude)). A call spelled `it(...)` always
> registers a test case, and a tool can list test cases without running
> them.

#### Table Tests

`std.testing` also declares `it_each`, which registers one test case per
row:

```text
pub fn it_each[A, T < Termination, R](name: string, rows: List[A], body: fn!(A) -> T $ R) -> void $ R
```

1. r[module.testing.it-each] A top-level call of `std.testing.it_each` registers one test case for each element of `rows`, which runs `body` with that element.
2. r[module.testing.it-each.name] The test case for the element at index `i` is named `name[i]`.
3. r[module.testing.it-each.import] `it_each` is not a prelude name; code imports it with `use std.testing.it_each`.
4. r[module.testing.it-each.body] Its body has a parameter, so it is written as an explicit `fn!` closure rather than a trailing block.

```text
use std.testing.{assert_equal, it_each}

fn double(value: i32) -> i32: value * 2

tests:
    it_each("doubles", [1, 2, 3], fn!(value: i32):
        assert_equal(double(value), value + value, reason="doubling adds the value to itself")
    )
```

### Test Outcomes

Each test case runs alone and passes or fails by its result:

```text
fn first(items: List[i32]) -> i32: items[0]

tests:
    it("an empty list has no first item", expect_panic="index-out-of-bounds"):
        _ := first([])
```

1. r[module.testing.instance] Each test case runs in its own fresh program instance, after module initialization.
2. r[module.testing.no-reuse] Instances are not reused between test cases.
3. r[module.testing.driven] The runner drives the body's suspension to completion, as the host drives `main!`.
4. r[module.testing.unit-row] A test case in a `tests:` block or a test module gets no host providers. Its body's requirement row must be empty, so every requirement comes from a `$.with` provider scope. Error: `missing-requirement`.
5. r[module.testing.profile] A test run compiles against one [runtime profile](#runtime-profiles), the default profile unless the run selects another.
6. r[module.testing.integration-row] For a test case in an integration test module, the runner binds the body's requirement row from that profile, as the host binds the row of `main`.
7. r[module.testing.skipped] An integration test case whose row names a trait that the profile does not bind is not run. It is reported as skipped, and it is not an error.
8. r[module.testing.pass] A test case passes when its body completes and `report()` on its result returns `ExitCode(0)`.
9. r[module.testing.fail] It fails when `report()` returns another code or when its body panics, including by a failed assertion.
10. r[module.testing.err-print] When the result holds an `.Err`, the runner prints the error as [Entry Results](#entry-results) describes.
11. r[module.testing.expect-panic-fail] With `expect_panic`, the test case instead fails when its body completes or panics with another category.

```text
trait Clock:
    fn now(self) -> i32

data FixedClock:
    time: i32

impl Clock for FixedClock:
    fn now(self) -> i32: self.time

fn stamp() -> i32 $ Clock:
    $.use(Clock).now()

tests:
    it("stamps with a fake clock"):
        $.with(Clock=FixedClock { time: 7 }):
            _ := stamp()

    it("has no host clock"):
        _ := stamp()  # error: missing-requirement
```

> **Why.** Source cannot catch a panic, so only the runner can judge an
> expected one, from the panic's stable category. Unit tests run on fakes
> alone, so they pass on every machine; only integration tests reach real
> providers.

See also: [Propagation In Test Blocks](05-expressions.md#propagation-in-test-blocks),
[Exit Status](#exit-status).

## Module Initialization

This section defines which modules initialize, in what order, and what their
top-level code may do.

1. r[module.init.script] A **script** is an entry module whose top-level executable statements are the entry behavior and which has no `main` declaration.
2. r[module.init.entry-module] An **entry module** is the selected root module of an executable package.
3. r[module.init.statements-and-main] If an entry module contains both top-level statements and `main`, its top-level statements initialize the module first and then the runtime invokes `main`. That form is an executable entry module, not a script.
4. r[module.init.program-instance] A **program instance** is one instantiated Wasm module graph together with its module storage, provider bindings, and execution state.

### Initialization Order

1. r[module.init.graph] Before execution, the compiler resolves the acyclic use graph reachable from the selected script or executable entry module.
2. r[module.init.once] Every reachable module is initialized exactly once per program instance after all modules it uses have been initialized.
3. r[module.init.ready-order] When multiple modules are otherwise ready, their fully qualified module identities order them lexicographically.

> **Why.** Ordering by module identity makes initialization independent of
> filesystem enumeration.

### Top-Level Statements

1. r[module.init.declarations] Within one module, named declarations are available before initialization.
2. r[module.init.source-order] Top-level executable statements run in source order.
3. r[module.init.uses] Use declarations do not execute as statements.
4. r[module.init.binding] Top-level bindings are initialized at their statement, before later function bodies may access them.
5. r[module.init.storage] Their storage remains available to functions in that module for the lifetime of the program instance.
6. r[module.init.no-step] A module with no top-level executable statements has no observable initialization step.

### Definite Initialization

1. r[module.init.definite] Before accepting a top-level executable statement, the compiler checks the transitive read set of each function or closure it references. Every top-level binding in that set must already be initialized.
2. r[module.init.definite.implicit] References passed as values and functions reached by trait dispatch, interpolation, iteration, or another implicit call are included.
3. r[module.init.definite.dispatch] A trait method call through a generic bound or a dynamic trait value reaches every implementation of that method in the module.
4. r[module.init.definite.whole-module] This definite-initialization check covers the whole module value-flow and call graph.
5. r[module.init.definite.local] The check is local to one module.

```text
first := apply(first_name)  # error
let names: List[string] = ["Ada"]

fn apply(callback: fn() -> string) -> string:
    callback()

fn first_name() -> string:
    names[0]
```

> **Note.** An implementation can compute the check from a per-function
> summary of the top-level bindings each function reads, combined bottom-up
> over the module's call graph. It never needs another module's function
> bodies.

### Entry Behavior

1. r[module.init.script-body] After dependency initialization, a script executes its top-level statements as that module's initialization.
2. r[module.init.main] An executable package then invokes `main` after its entry module has initialized.
3. r[module.init.tests] Test runners initialize the module under test and the modules it uses before running its test cases.
4. r[module.init.test-cases] The `it` calls of a `tests:` block or a test module, and their bodies, are not part of module initialization.

### Requirement-Free Initialization

1. r[module.init.requirement-free] Top-level code in a non-entry module must be requirement-free initialization.
2. r[module.init.requirement-free.forms] It must not use `$.use`, enter a provider scope, make a bang call, or call a callable whose requirement row is not empty.
3. r[module.init.script-row] A script module may use requirements and suspension only through an inferred entry requirement row.
4. r[module.init.script-row.report] The compiler reports that row alongside `main!` rows for host configuration.
5. r[module.init.script-not-driver] A script's top level is not itself a suspension driver. Bang calls must occur in a suspending entry function or another specified driver context.

### Program Instances

1. r[module.init.per-instance] This initialization rule governs one program instance.
2. r[module.init.histories] Interactive cell re-execution and durable replay have separate runtime histories described in [`RUNTIME_AND_LIBRARY.md`](../future-work/RUNTIME_AND_LIBRARY.md).

## Public Uses And Visibility

Declarations are module-private by default, and prefix `pub` makes a
declaration available to other modules:

```text
pub type UserId(string)

pub data User:
    pub id: UserId
    pub email: string
```

1. r[module.vis.private-default] Declarations are module-private by default.
2. r[module.vis.pub] Prefix `pub` makes a declaration available to other modules.

### Public Uses

Use `pub use` in a `mod.hd` file to expose a package-facing API:

```text
pub use pkg.user.types.{User, UserId}
pub use pkg.user.service.{load_user, save_user}
```

Another module can then use the names through the directory module:

```text
use pkg.user.{User, UserId, load_user}
```

1. r[module.pub-use.facade] `pub use` in a `mod.hd` file exposes a package-facing API, whose names another module can use through the directory module.
2. r[module.pub-use.public-source] A publicly used declaration must already be public in its defining module.
3. r[module.pub-use.binding] `pub use` introduces the same local binding as `use` and additionally exposes that binding to other modules.
4. r[module.pub-use.identity] `pub use` does not create a new declaration identity.
5. r[module.pub-use.cycles] Cycles involving `use` or `pub use` are rejected.

### Member Visibility

1. r[module.vis.no-package-private] There is no package-private visibility modifier: another module in the same package can use only `pub` declarations.
2. r[module.vis.members] Named fields and inherent methods are module-private unless individually marked `pub`.
3. r[module.vis.embedded] Embedded fields take no marker and are always public.
4. r[module.vis.variants] Enum variants inherit their enum's visibility.
5. r[module.vis.trait-methods] Trait methods follow their trait's visibility.
6. r[module.vis.impl-target] A usable implementation additionally requires its target type to be visible.

### Public Signatures

1. r[module.vis.signature] A public declaration's complete source-level signature must not expose a module-private declaration.
2. r[module.vis.signature.coverage] This check recursively covers function parameters and results, data fields (every embedded field included), and enum constructor data and payloads. It also covers alias/newtype underlying types, trait bounds, supertraits, public generic arguments, and every requirement-row key.
3. r[module.vis.body] A private implementation detail may occur in a public function body but not in its public typed interface.

```text
data Secret:
    value: string

pub fn reveal() -> Secret:  # error
    Secret { value: "hidden" }
```

See also: [Field Visibility](08-data-and-enums.md#field-visibility).

## Name Resolution Across Packages

1. r[module.package.identity] An absolute qualified name identifies one declaration after dependency resolution.
2. r[module.package.no-sharing] Two dependencies do not share declarations merely because their package names and source paths match.
3. r[module.package.identity-part] Resolved package identity is part of a declaration's identity.
4. r[module.package.public-surface] A package must not access another package except through declarations reachable from that package's public module surface.
5. r[module.package.separate] Implementations may compile packages separately.

### Fully Annotated Declarations

1. r[module.package.annotated] Every public declaration is fully annotated: its complete signature is written in source.
2. r[module.package.annotated.parts] That signature includes parameter and result types, requirement rows, suspension, and generic parameters with their bounds, variance, and reification. It also includes the types of public fields and enum data.
3. r[module.package.no-inference] Nothing in a public signature is inferred from a function body.
4. r[module.package.no-pub-binding] Top-level bindings cannot be public.

```text
pub answer := 42  # error
```

### Package Interfaces

A package interface must contain:

| Interface content |
| --- |
| exported declaration identities and complete signatures |
| visibility |
| generic kinds, variance, bounds, and reification |
| requirement rows |
| associated types |
| every ordinary and local implementation head needed for coherence |
| the bodies of pack and reified code, which downstream compilation specializes |

1. r[module.interface.contents] A package interface must contain every item in the table.
2. r[module.interface.generic-bodies] An interface may also carry ordinary generic bodies to enable inlining, but downstream compilation must not require them.
3. r[module.interface.dictionaries] An implementation compiles each ordinary generic function in its defining package, and a downstream use supplies only its dictionaries.
4. r[module.interface.determined] A package interface is therefore determined by the package's declarations and does not depend on any other function body.
5. r[module.interface.early] A downstream package can be compiled as soon as the interfaces of its dependencies are known. It need not wait for their function bodies to be checked or compiled.
6. r[module.interface.coherence] Coherence is checked at link time over the complete set of resolved interface files.
7. r[module.interface.link-reject] Linking may therefore reject a graph even when each package compiled independently.

## Executable Entry Point

The conventional non-suspending entry point is:

```text
pub fn main() -> void:
    ...
```

1. r[module.entry.definition] An **executable entry point** is a public top-level function named `main` or `main!` with no parameters.
2. r[module.entry.result-termination] Its result type must implement `std.process.Termination`, as for an ordinary trait bound, so it may be `void`, `ExitCode`, or `Result[T, E]` with `T < Termination` and `E < Display`. Any other result type is an error. Error: `unsatisfied-trait-bound`.
3. r[module.entry.row] It may declare a requirement row.
4. r[module.entry.row.host] Every key in that row must be a host capability trait of the selected runtime profile. Any other key is an error. Error: `nonhost-entry-requirement`.
5. r[module.entry.private-main] A top-level `main` that is not public is an ordinary function and is not an entry point.
6. r[module.entry.suspending] A suspending entry point is spelled `main!`.

See also: [Mutable Providers](11-requirements-and-suspension.md#mutable-providers).

### Entry Results

1. r[module.entry.exit-report] When an entry point returns, the process exits with the `u8` held by the `ExitCode` that `report()` returns for its result, as [Exit Status](#exit-status) describes.
2. r[module.entry.err-host-prints] When the result holds an `.Err`, the host prints that error by the rules below before it exits.
3. r[module.entry.err-dynamic] A dynamic trait value type whose trait is `Display` or has it as a supertrait, such as the erased `std.error.Error`, satisfies the `E < Display` bound.
4. r[module.entry.err-render-chain] When `E` implements `std.error.Error`, including the erased `Error`, the host prints the error's `Display` text and then each cause that the standard-library `chain` yields after it.
5. r[module.entry.err-render-chain.line] Each cause is printed on its own line as `caused by: ` followed by the cause's `Display` text.
6. r[module.entry.err-render-display] Otherwise the host renders the error with `Display.to_string`.
7. r[module.entry.panic] A panic exits with a distinct nonzero status selected by the runtime profile and poisons the program instance.

```text
data HiddenError: pass

pub fn main() -> Result[void, HiddenError]:  # error: unsatisfied-trait-bound
    .Err(HiddenError {})
```

See also: [Dynamic Trait Values](09-traits.md#dynamic-trait-values),
[Error Trait](09-traits.md#error-trait).

#### Exit Status

`std.process` declares the exit code and the trait that produces it:

```text
pub type ExitCode(u8)

pub trait Termination:
    fn report(self) -> ExitCode
```

1. r[module.entry.exit-code] `std.process` declares the newtype `ExitCode`, whose `u8` is a process exit code. Every `u8` is valid, and 0 means success.
2. r[module.entry.termination] `std.process` declares the trait `Termination`, whose one method is `fn report(self) -> ExitCode`.
3. r[module.entry.termination.void] `void` implements `Termination`; its `report` returns `ExitCode(0)`.
4. r[module.entry.termination.exit-code] `ExitCode` implements `Termination`; its `report` returns the code itself.
5. r[module.entry.termination.result] `Result[T, E]` implements `Termination` when `T < Termination` and `E < Display`.
6. r[module.entry.termination.ok] For `.Ok(value)`, its `report` returns `value.report()`.
7. r[module.entry.termination.err-code] For `.Err(error)`, its `report` returns `ExitCode(1)`.
8. r[module.entry.termination.no-print] `report` only computes the code. The host or test runner prints the error, as [Entry Results](#entry-results) describes.
9. r[module.entry.process-import] `ExitCode` and `Termination` are not prelude names; code imports them from `std.process`, as in `use std.process.ExitCode`.

A program that picks its own code returns an `ExitCode`:

```text
use std.process.ExitCode

fn count_changes() -> Result[i32, string]:
    .Ok(3)

pub fn main() -> Result[ExitCode, string]:
    changes := count_changes()?
    if changes > 0: .Ok(ExitCode(1)) else: .Ok(ExitCode(0))
```

> **Why.** One trait serves entry points and tests, as Rust's `Termination`
> does, and the result type, not the error type, picks the code.

> **Note.** `main() -> Result[void, E]` therefore exits with 1 on every
> `.Err`. A tool that needs other codes returns `ExitCode` or
> `Result[ExitCode, E]`.

> **Note.** A tool that must exit without printing, such as one that stops
> quietly on a closed pipe, prints what it needs and returns an `ExitCode`.

### Entry Arguments

1. r[module.entry.no-arguments] `main` has no source-level arguments.
2. r[module.entry.host-facilities] Process arguments, console access, environment, and other host facilities are requirements supplied by the runtime through the requirement model.

## Wasm Boundary

This section defines runtime profiles, registered boundary functions, the
values that cross a boundary, and the official host boundary.

### Runtime Profiles

1. r[module.profile.definition] A **runtime profile** is a named compile-time set of host capability traits, their boundary adapters, and runtime choices such as panic exit statuses.
2. r[module.profile.trait-access] The profile binds each host provider with the access its trait gives: mutable when the trait has a `mut self` method, readonly otherwise.
3. r[module.profile.build] The compiler receives the selected profile as build configuration.
4. r[module.profile.default] The default profile contains at least the prelude `Console` trait.
5. r[module.profile.other] Another profile may add or omit host traits explicitly.

See also: [Mutable Providers](11-requirements-and-suspension.md#mutable-providers).

### Registration

1. r[module.register.pub] `pub` is module visibility, not Wasm export registration.
2. r[module.register.explicit] A tool, workflow, or other host-callable function becomes visible only through explicit registration provided by its library.
3. r[module.register.entry] Every registered boundary function is an entry point for provider checking.
4. r[module.register.contract-traits] Its registration contract selects a runtime profile and declares which requirement traits that profile can bind.
5. r[module.register.application] That bindable set may include application traits such as `Database` when the adapter explicitly supports them.
6. r[module.register.row] Every key in the registered function's row must be in that bindable set, and the host must bind all of them before invocation.
7. r[module.register.failure] Otherwise registration or startup fails before user code executes.

### Boundary-Safe Values

Registered boundaries initially allow recursively structural values:

- primitive scalar types and `string`;
- tuples;
- `List[T]` and `Map[K, V]` whose contents are boundary-safe;
- data types and enums whose complete fields and payloads are boundary-safe;
- `T?` and `Result[T, E]` whose contained types are boundary-safe.

1. r[module.boundary.allowed] Registered boundaries initially allow the recursively structural values in the list above.
2. r[module.boundary.not-safe] Mutable types, dynamic trait values (including `Inspectable` values), `std.inspect.TypeId`, closures, and live runtime handles are not boundary-safe.
3. r[module.boundary.rows] Requirement-row entries are host bindings and are not serialized parameters.
4. r[module.boundary.erased-error] In particular, the erased error `std.error.Error` never crosses a registered boundary.
5. r[module.boundary.domain-error] A registered function returns a boundary-safe error type, such as a domain error enum, and code converts an erased error to such a type explicitly.

> **Note.** An error type with a member of type `Error` is therefore not
> boundary-safe either. Before such an error crosses a boundary, code
> converts it with the standard-library `report_of` to an `ErrorReport`,
> a boundary-safe snapshot of its message and causes.

See also: [Error Trait](09-traits.md#error-trait).

### Boundary Encoding

1. r[module.boundary.tree] Boundary values have tree semantics.
2. r[module.boundary.cycle] Encoding a cycle is a boundary error.
3. r[module.boundary.sharing] When an acyclic graph shares a node, each incoming path encodes a duplicate tree value, and decoding does not restore sharing.
4. r[module.boundary.pub] Every field and enum payload that crosses a boundary must be `pub`.
5. r[module.boundary.map-decode] Decoding a map invokes the key type's ordinary `Eq` and `Hash` implementations.
6. r[module.boundary.decoder-panic] If either panics, the adapter reports a boundary failure and does not enter the registered function. Boundary failure: `boundary-decoder-panic`.
7. r[module.boundary.decoder-poison] The adapter treats that event as an ordinary poisoning panic: the program instance must be discarded.

> **Why.** Every crossing field and payload is `pub`, so a host cannot
> construct private state.

### Instances And Threads

1. r[module.instance.thread] One program instance executes on one thread and has no shared-memory parallelism or source-level atomics.
2. r[module.instance.parallel] Hosts may run multiple Wasm instances in parallel only by exchanging boundary-safe values.
3. r[module.instance.disjoint] Their heaps, mutable globals, and suspension drivers are disjoint.

### Host Boundary

1. r[module.host.wasm] The official compiler targets Wasm only.
2. r[module.host.runtime] The official runtime uses Wasm GC for managed language values and a WASI-compatible host boundary.
3. r[module.host.providers] Authority-bearing providers originate at that boundary.
4. r[module.host.component-model] The only normative host boundary is the WebAssembly Component Model, and strings cross it as canonical-ABI strings.
5. r[module.host.tooling-hooks] Hooks that a toolchain adds for development, testing, or tracing are implementation tooling, not a language boundary.
6. r[module.host.requirements] Every host facility is injected through an ordinary requirement trait.
7. r[module.host.capability-set] The eventual standard capability-trait set is runtime and library work.
8. r[module.host.abi] The exact component-model ABI and registration APIs are runtime and library specification work.

### Closable Handles

Closable runtime handles use `std.resource.ResourceError[E]`:

```text
enum ResourceError[E]:
    Operation(error: E)
    Disposed
```

1. r[module.resource.error] Closable runtime handles use `std.resource.ResourceError[E]`.
2. r[module.resource.result] An operation whose ordinary error type is `E` returns `Result[T, ResourceError[E]]`.
3. r[module.resource.disposed] Once the handle has been closed, every further operation returns `.Err(ResourceError.Disposed)` and must not trap or access the host resource.
4. r[module.resource.close] Closing is itself an operation, and a repeated close returns the same `Disposed` error.
5. r[module.resource.aliases] This checked behavior applies through every alias.

## Tooling, ABI, And Unsupported Extensions

1. r[module.tooling.package] The complete `hd.toml` schema, lockfile, version constraints, and dependency resolver belong to package tooling.
2. r[module.tooling.abi] The exact Wasm component boundary and registration mechanism belong to the runtime ABI.
3. r[module.unsupported.visibility] hd-lang has no package-private visibility or independent visibility for enum variants and trait methods.
