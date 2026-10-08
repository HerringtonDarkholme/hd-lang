# Names and Scopes

Status: language specification draft.

This chapter defines the scopes introduced by hd-lang programs and how names
resolve. It does not define type compatibility or access permission.

## Name Categories

1. r[names.category.four] hd-lang has four lookup categories, which this table lists:

| Rule | Names | Identify |
| --- | --- | --- |
| r[names.category.module] Module | **Module names** | top-level types, traits, functions, and names introduced by use declarations |
| r[names.category.value] Value | **Value names** | top-level executable bindings, parameters, local bindings, local named functions, loop bindings, pattern bindings, and captured values |
| r[names.category.local-type] Local type | **Local type names** | data types, enums, traits, aliases, and newtypes declared inside an executable suite |
| r[names.category.member] Member | **Member names** | data fields, embedded fields, methods, enum variants, and tuple fields within the namespace of their owning type |

1. r[names.category.syntax] A use is resolved in the category required by its syntax.
2. r[names.category.syntax.type] For example, the name before `{` in `User { ... }` is resolved as a type.
3. r[names.category.syntax.value] In `user.email`, `user` is resolved as a value and `email` as a member of its type.
4. r[names.type-as-value] A name resolved as a value that names a type, a trait, or a type alias and no value is an error. Examples are `let x = User` and `field.metadata(MaxLen)`. Error: `type-used-as-value`.
5. r[names.module.unique] A module cannot contain two declarations with the same module name, even if they are different kinds of declaration. Error: `duplicate-module-name`.
6. r[names.module.no-overloading] Function overloading is therefore not permitted.

```text
fn value(input: i32) -> i32: input
fn value(input: string) -> string: input  # error: duplicate-module-name
```

```text
data User:
    name: string

fn make() -> void:
    x := User  # error: type-used-as-value
```

> **Note.** By `names.category.syntax`, a form whose syntax names a type
> resolves the name as a type. So `User { name: "Ada" }`, a newtype
> constructor call such as `Mile(1)`, and a cast such as `i16(wide)` are not
> value uses.

### Prelude Names

1. r[names.prelude.source] Every built-in core type is supplied by the prelude described in [Modules and Packages](10-modules.md#prelude).
2. r[names.prelude.not-reserved] Prelude names remain ordinary identifier tokens rather than reserved words.
3. r[names.prelude.one-namespace] There is no separate predeclared-name namespace or shadowing rule.

```text
data i32:  # error: prelude-name-shadow
    value: string
```

See also: [`module.prelude.no-shadow`](10-modules.md#r-module.prelude.no-shadow), which makes
shadowing a prelude name an error.

### Type Parameters

1. r[names.type-param.local] Type parameters are type names local to their declaration.
2. r[names.type-param.shadow] A type parameter may shadow a module name within that declaration.
3. r[names.type-param.unique] A type parameter must not duplicate another type parameter in the same parameter list.
4. r[names.type-param.no-redeclare] No declaration within a type parameter's scope may reuse its name: no nested type parameter, local declaration, local value, or parameter. Error: `duplicate-binding`.
5. r[names.type-param.no-redeclare.method] So a method's own type parameter must not reuse a type parameter of its `impl` or trait, as `fn echo[T]` inside `impl[T] Box[T]` would.
6. r[names.type-param.no-redeclare.body] Inside `fn work[T]`, a local `data T`, a local value `T`, and a local `fn convert[T]` are each an error.

```text
data Box[+T]:
    value: T

impl[T] Box[T]:
    pub fn echo[T](self, value: T) -> T:  # error: duplicate-binding
        value

fn work[T < Display](value: T) -> string:
    data T:  # error: duplicate-binding
        count: i32
    T := value  # error: duplicate-binding
    "${value}"
```

> **Why.** One name then means one thing inside a declaration. A reader
> never has to work out which `T` a signature or a `T::` call names.

### The `Self` Name

1. r[names.self-type.reserved] `Self` is a reserved word that names the implementation target inside a trait or `impl` body.
2. r[names.self-type.trait] In a trait, `Self` denotes the eventual implementing type.
3. r[names.self-type.impl] In a trait or inherent implementation, `Self` denotes that implementation's target type.
4. r[names.self-type.outside] `Self` has no valid type meaning outside those bodies.

```hd
data Counter:
    value: i32

impl Counter:
    fn zero() -> Self:
        Counter { value: 0 }   # Self is Counter here
```

## Lexical Scopes

1. r[names.scope.lexical] Scopes are lexical.
2. r[names.scope.constructs] The following constructs introduce a local scope:

- a function or closure body;
- each indented or same-line suite of `if`, `else if`, and `else`;
- the body and `else` suite of a `for` or `while` loop;
- each `match` arm;
- the `else` block of a let-else statement;
- a comprehension;
- a trailing callback block.

1. r[names.scope.shadow] A name declared in an inner scope may shadow a name from an outer scope.
2. r[names.scope.duplicate] Two bindings with the same name in one scope are a compile-time error.
3. r[names.scope.reassign] Reassignment uses `=` and does not introduce another binding.

```hd
fn demo(flag: bool) -> i32:
    x := +1
    if flag:
        x := +2   # a new scope: this x shadows the outer one
        _ := x
    x   # the outer one: +1
```

### Binding Start

A local binding is not visible in its own initializer:

```text
name := normalize(name)  # the right-hand name, if valid, resolves outward
```

1. r[names.scope.start] The scope of a local binding begins immediately after its initializer has been evaluated.
2. r[names.scope.not-in-initializer] The binding is not visible in its own initializer.
3. r[names.scope.recursive-closure] The sole exception is a direct local closure initializer with an explicit result type: its body may refer to the name being bound for recursion.
4. r[names.scope.recursive-closure.eval] The initializer expression itself is still evaluated before the binding becomes available; only the closure body receives this forward name.
5. r[names.scope.recursive-closure.no-early-run] The closure body cannot run before initialization completes.

> **Why.** This permits ordinary inner-scope shadowing without making a new
> binding self-referential.

## Module Scope

1. r[names.module.file] Every source file is a module. Its path-derived identity is specified in [Modules](10-modules.md).
2. r[names.module.declarations] Top-level declarations belong to the module scope.
3. r[names.module.order-free] Their names are visible throughout the module, independent of textual order, which permits direct and mutual recursion between functions in one module.
4. r[names.module.well-typed] A declaration must still be well typed as a whole.
5. r[names.module.no-init-order] Forward visibility does not imply initialization order for executable top-level statements.
6. r[names.module.scope] A module's scope holds its own top-level declarations, the [prelude](10-modules.md#prelude) names, and the names that its own `use` declarations introduce.
7. r[names.module.other-module] Outside the prelude, a declaration of another module is in scope only through this module's own `use` of it. That holds for a module in the package, in a dependency, or in `std`.
8. r[names.module.use-own-module] A `use` declaration introduces its names into its own module only, never into another module of the same package.
9. r[names.module.other-module.value] A bare value name that is not in scope is an error, even when another module declares it. Error: `unknown-name`.
10. r[names.module.other-module.type] A bare type name that is not in scope is an error, even when another module declares it. Error: `unknown-type`.
11. r[names.module.other-module.trait] A bare trait name that is not in scope is an error, even when another module declares it. Error: `unknown-trait`.
12. r[names.module.other-module.hint] The message of such an error may name the module that declares the name and the `use` declaration that would import it.

```text
# src/cart.hd
pub data Cart:
    pub items: i32

pub fn total(cart: Cart) -> i32: cart.items * 100

fn fee() -> i32: +5

# src/checkout.hd
use pkg.cart.{Cart}

fn charge(cart: Cart) -> i32:
    total(cart) + fee()  # error: unknown-name
```

> **Why.** No action at a distance. Every bare name in a file traces to
> that file's declarations, the prelude, or one of its own `use` lines.

See also: [Use Declarations](#use-declarations), [Public Uses And Visibility](10-modules.md#public-uses-and-visibility).

### Module Execution Scope

1. r[names.exec.scope] Top-level executable statements run in a separate module execution scope.
2. r[names.exec.visible] Bindings created by those statements become visible to later top-level statements and to the bodies of functions declared after their binding point.
3. r[names.exec.access] Those functions may read a top-level `:=` or `let` binding and may reassign a top-level `let` binding.
4. r[names.exec.not-usable] Such bindings are not nameable by a `use` declaration.
5. r[names.exec.not-before] Such bindings are not visible before their binding point, including from the body of a function declared earlier. Error: `binding-not-yet-visible`.
6. r[names.exec.declarations] A top-level executable statement may refer to a named module declaration regardless of that declaration's textual position.
7. r[names.exec.methods] In the rules of this subsection, a function also means an inherent or trait method, and a method's position is that of its `impl` block.

```text
fn read_count() -> i32:
    count  # error: binding-not-yet-visible

let count = +0
```

### Initialization Order

1. r[names.init.no-bypass] Referring to a named module declaration from a top-level executable statement does not bypass initialization order.
2. r[names.init.transitive] At every top-level executable statement, the compiler computes the transitive set of top-level bindings read by every module function or closure referenced by that statement.
3. r[names.init.references] A function or closure counts as referenced whether it is called directly or passed as a value. It also counts when reached through a trait method, interpolation, iteration, or another implicit call.
4. r[names.init.required] Every binding in that set must have been initialized by an earlier top-level statement.
5. r[names.init.whole-module] This is a whole-module value-flow and call-graph check; an indirect read through a later function value is an error like a direct forward binding reference. Error: `top-level-read-before-initialization`.

```hd
start := +1

fn total() -> i32:
    start + 1

demo := total()   # reads start through total, after both are initialized
```

### Declarations In Blocks

1. r[names.block-decl.allowed] Named `fn`, `data`, `enum`, `trait`, and `type` declarations may also occur inside executable block suites.
2. r[names.block-decl.impl] `impl` declarations may occur at module scope or inside an executable block suite; they do not introduce an independently referencable name.

```hd
fn demo() -> i32:
    data Pair:
        a: i32
        b: i32
    p := Pair { a: +1, b: +2 }
    p.a + p.b   # 3; Pair is local to demo
```

See also: [Function And Closure Scopes](#function-and-closure-scopes).

### Tests Blocks

The items of a `tests:` block are module items that only the block sees:

```text
use std.testing.assert_equal

fn late_fee(days: i32) -> i32:
    if days > 30: 5 else: 0

tests:
    fn overdue() -> i32: 31
    pub fn shared_overdue() -> i32: 45  # error: public-test-item
    use std.testing.assert_equal  # valid: the block's import shadows the module's

    it("uses a private function"):
        assert_equal(late_fee(overdue()), 5, reason="the block sees late_fee")

fn report() -> i32:
    overdue()  # error: unknown-name

fn check_flag(ok: bool) -> void:
    assert_equal(ok, true, reason="outside the block, the module's own import applies")
```

1. r[names.tests.module-items] The items of a [`tests:` block](02-grammar.md#test-blocks) are module items of the file's module.
2. r[names.tests.sees-module] Code inside the block sees every module name, including private declarations and uses outside the block.
3. r[names.tests.inside-only] A name that an item of the block declares or uses is visible only inside the block. Naming it outside the block is an error. Error: `unknown-name`.
4. r[names.tests.unique] Because they are module items, a name declared in the block must not repeat a module name declared outside it. Error: `duplicate-module-name`.
5. r[names.tests.no-pub] An item inside a `tests:` block must not be marked `pub`. Error: `public-test-item`.
6. r[names.tests.shadow] A `use` inside the block may introduce a name the module declares or uses outside the block; inside the block (its cases and helpers) the block's import wins, as a nested scope. Outside the block nothing changes.

> **Why.** Module items, rather than local declarations, let the block hold
> implementations and derivation blocks, as Rust's `mod tests` can. Nothing
> outside the block sees its items, so `pub` would promise what it cannot
> give; shared test helpers belong in a [test module](10-modules.md#test-modules).

See also: [Test Cases](10-modules.md#test-cases).

### Module Privacy

1. r[names.pub.private-default] Declarations are module-private unless marked `pub`.
2. r[names.pub.eligible] `pub` makes a declaration eligible to be used from another module.
3. r[names.pub.no-export] `pub` does not register a Wasm export or make a declaration host-callable.

```hd
pub fn visible() -> i32:
    internal()   # same module: fine

fn internal() -> i32:
    +2
```

## Use Declarations

A `use` declaration introduces either one local module name or one or more
local declaration names into the using module's module scope:

```text
use pkg.user.types
use pkg.user.types as user_types
use pkg.user.types.{User, UserId}
use dep.billing.types.{UserId as BillingUserId}
```

1. r[names.use.introduces] A `use` declaration introduces either one local module name or one or more local declaration names into the using module's module scope.
2. r[names.use.path-only] A path-only use may name either a module namespace or one public declaration.
3. r[names.use.final-component] Without `as`, a path-only use binds the final path component; the first example therefore binds `types`.
4. r[names.use.grouped] A grouped use binds each selected name after applying any item alias.
5. r[names.use.no-collision] A used name must not collide with another module-scope declaration or use.
6. r[names.use.no-overload] Use declarations do not create overload sets and are not implicitly renamed.
7. r[names.use.resolution-order] Use declarations are resolved before declarations are type checked.
8. r[names.use.position] The textual position of a use declaration does not limit its visibility.
9. r[names.use.style] Style tools should place use declarations before other top-level items.
10. r[names.use.cycles-in-folder] The files of one folder may use each other in a loop.
11. r[names.use.folder-cycle] The folders of a package must not depend on each other in a loop, as [Dependency Cycles](10-modules.md#dependency-cycles) defines. Error: `folder-cycle`.

### Public Use Declarations

1. r[names.use.pub] Prefixing a grouped use declaration with `pub` makes every name it introduces available to other modules.
2. r[names.use.pub.source] The source declaration must already be public.
3. r[names.use.pub.binding] A `pub use` introduces the same local binding as an ordinary `use`; it additionally exposes that binding without creating a new declaration identity.

```text
pub use std.text.{trim, split}
```

### Literal Suffix Names

The suffix of a [suffixed literal](01-lexical-structure.md#literal-suffixes)
names a function in module scope, usually one brought in by `use`:

```text
use std.ops.num_suffix

@num_suffix
fn s(count: i64) -> i64: count * 1000

fn retry_after(s: i32) -> i32:
    limit := 5s  # the module's suffix function s, not the parameter
    s
```

1. r[names.literal-fn.bare-in-scope] A literal suffix or a string prefix is a bare name, resolved as a module name. That is a module-scope declaration, or a name that a use declaration or the prelude introduces. Ordinary rules apply, so it is brought in, renamed with `as`, or found in conflict as other used names are.
2. r[names.literal-fn.no-local] Parameters, local bindings, and local type declarations never take part, so a local named `s` or `r` does not change what `5s` or `r"..."` calls.
3. r[names.literal-fn.unknown-name] A suffix or prefix that names nothing in module scope is an error. Error: `unknown-name`.

```text
fn margin() -> i32:
    width := 12px  # error: unknown-name
    0
```

> **Why.** Resolving through `use` tells a reader where `ms` comes from, and
> clashes such as two libraries' `m` use the existing `as` renaming. Module
> scope alone keeps a local name from changing what a literal means.

See also: [Literal Suffixes](05-expressions.md#literal-suffixes).

### String Prefix Names

The prefix of a [prefixed string](01-lexical-structure.md#prefixed-strings)
names a function in module scope, as a literal suffix does:

```text
use std.ops.{Template, str_prefix}

@str_prefix
fn r(t: Template[string]) -> string:
    t.raw_parts[0]

fn escape(r: i32) -> string:
    r"\d+"  # the module's prefix function r, not the parameter
```

The rules of [Literal Suffix Names](#literal-suffix-names) cover a
prefix too, and so an unknown prefix is an error:

```text
fn query(id: i32) -> string:
    sql"select $id"  # error: unknown-name
```

> **Note.** A prefix is never written with a module path. To call the
> stdlib prefix function `r` of [Text](../std/text.md), write
> `use std.text.r`, then `r"\d+"`.

See also: [Prefixed Strings](05-expressions.md#prefixed-strings),
[`grammar.primary.prefix-after-dot`](02-grammar.md#r-grammar.primary.prefix-after-dot).

## Local Bindings

`:=` and `let` introduce local names:

```text
name := "Ada"
```

```text
let count = +0
count = count + 1
```

1. r[names.local.short] `:=` introduces inferred, non-reassignable local names. Reassigning one is an error. Error: `non-reassignable-binding`.
2. r[names.local.let] `let` introduces local names that may be reassigned.
3. r[names.local.same-scope] The two forms differ in rebinding permission, not in lexical scope.
4. r[names.local.mutation] Reference mutation permission comes from `mut T` in the binding's type and is independent of whether the local name can be reassigned.

```text
fn invalid() -> void:
    name := "Ada"
    name = "Grace"  # error: non-reassignable-binding
```

### Tuple Bindings

A tuple binding, like any `let` pattern, introduces several names at once:

```text
let (x, y) = point
let (name, score) = entry
```

1. r[names.pattern.simultaneous] A `let` pattern introduces every name it binds at once, after the initializer is evaluated and matched.
2. r[names.pattern.distinct] All names in one binding pattern must be distinct. A name bound twice is an error. Error: `duplicate-binding`.
3. r[names.tuple.annotation] When a `let` tuple binding has one type annotation, that annotation describes the complete right-hand tuple, not each individual name.
4. r[names.tuple.arity] The annotation's arity must match the binding pattern, and each local receives the corresponding element type.

### Let-Else Scope

The names that a let-else pattern binds are visible after the statement,
and not in its `else` block:

```text
fn find(id: i32) -> i32?:
    if id == 0: .Some(7) else: .None

fn read(id: i32) -> i32:
    let .Some(value) = find(id) else:
        return 0  # value is not visible here
    value + 1
```

1. r[names.let-else.after] The names that a `let` pattern binds are visible to the end of the enclosing scope. Their scope starts at the end of the statement, after any `else` block.
2. r[names.let-else.not-in-else] They are not visible in the `else` block. A use there resolves to an outer binding of the same name, if one exists.

> **Why.** The `else` block runs only when the pattern did not match, so
> no name of the pattern has a value there.

## Binding Expressions

A `:=` binding expression binds in its enclosing scope:

```text
if (size := input.len()) > 0:
    println(size)
```

1. r[names.bind.expression] `:=` may appear as the lowest-precedence expression.
2. r[names.bind.scope] The binding of a `:=` expression belongs to the nearest enclosing executable scope, not to a synthetic scope around the subexpression.
3. r[names.bind.visible] In the example, `size` is visible after its initializer completes, including in the selected `if` suite and in later statements of the enclosing scope.
4. r[names.bind.initialized] `size` is initialized regardless of which `if` branch executes because the condition is evaluated before branch selection.
5. r[names.bind.no-redeclare] A binding expression must not redeclare a name already bound in that same scope. Error: `duplicate-binding`.

```text
fn main() -> i32:
    value := 1
    (value := 2)  # error: duplicate-binding
```

> **Note.** Use assignment to update a `let` binding, or introduce an inner
> suite to shadow an outer name.

### Definite Initialization

1. r[names.definite.required] Lexical scope does not waive definite initialization.
2. r[names.definite.path] A `:=` name is usable on a control-flow path only after that path evaluates its initializer.
3. r[names.definite.merge] At a merge, the name is definitely initialized only if every incoming reachable path has evaluated the same binding.
4. r[names.definite.reject] A use that may observe an uninitialized binding is an error. Error: `possibly-uninitialized-binding`.
5. r[names.definite.condition] A direct binding in an `if` or `while` condition is evaluated whenever that condition is evaluated.
6. r[names.definite.skipped] Some paths skip a binding inside the conditionally evaluated operand of `&&` or `||`, an unselected branch or match arm, or a loop body. On those paths the binding is not thereby initialized.
7. r[names.definite.proof] Flow analysis may still prove it initialized inside a branch whose selection implies that the binding ran.
8. r[names.definite.diverging] A branch that diverges, as [`flow.let.else.diverge.forms`](06-control-flow.md#r-flow.let.else.diverge.forms) defines, is not an incoming path at the merge after it.

```text
fn invalid(flag: bool) -> string:
    if flag && (name := "Ada") != "":
        pass
    name  # error: possibly-uninitialized-binding
```

A guard whose branch returns, or enters an infinite loop, leaves only the
paths that ran the binding:

```text
fn greeting(args: List[string]) -> string:
    if args.len() < 2 || (name := args[1]) == "":
        return "usage: greet NAME"
    "hello $name"
```

## Function And Closure Scopes

1. r[names.fn.value-params.visible] A function's value parameters are visible throughout its signature after their declaration point and throughout its body.
2. r[names.generic.params.visible] The generic parameters of a function, method, data type, enum, trait, or `type` declaration are visible throughout that declaration, including its body.
3. r[names.generic.bounds.whole-list] A bound sees the whole list: it may name any parameter of its own list, earlier or later, as in `fn read[S < Source[U], U = string](s: S) -> U`.
4. r[names.generic.defaults.earlier] A type-argument default sees only the earlier parameters of its list, as [`types.generic.default.later`](04-type-system.md#r-types.generic.default.later) states. Error: `binding-not-yet-visible`.
5. r[names.fn.params.scope] All value parameters belong to the function body's outermost local scope and must have distinct names.

```hd
fn first[A, B](pair: (A, B)) -> A:
    pair._0
```

### Local Functions

1. r[names.local-fn.name] A named function declared in a block suite introduces a local value name at its declaration point.
2. r[names.local-fn.visible] The name is visible in the rest of that suite and in the function's own body, permitting recursion.
3. r[names.local-fn.not-visible] The name is not visible before its declaration, outside the suite, or from another module.
4. r[names.local-fn.rules] A local named function follows the same shadowing and duplicate-name rules as other local values.

```hd
fn demo() -> i32:
    fn down(n: i32) -> i32:
        if n == 0: 0
        else: down(n - 1)   # the name is visible in its own body
    down(+3)
```

### Local Type Declarations

1. r[names.local-type.name] A local `data`, `enum`, `trait`, or `type` declaration introduces a type name at its declaration point. The name is visible in its own definition and in the rest of its enclosing suite.
2. r[names.local-type.not-visible] The name is not visible before that point or outside the suite.
3. r[names.local-type.static] Local type declarations do not execute, capture runtime values, or become module members nameable by a `use` declaration.
4. r[names.local-type.refs] Local type declarations may refer to type names and type parameters visible at their declaration point.
5. r[names.local-type.identity] A local nominal type has one declaration identity, not a fresh identity per call to the enclosing function.
6. r[names.local-type.duplicate] Local type and value declarations cannot duplicate a name in the same scope.
7. r[names.local-type.prelude] Local type declarations are also subject to the prelude shadowing rule.

```hd
fn demo() -> i32:
    enum Choice:
        Yes
        No
    match Choice.Yes:
        Choice.Yes => +1
        Choice.No => +0
```

See also: [Prelude Names](#prelude-names).

### Local Declaration Limits

1. r[names.local.no-pub] `pub` is not permitted on local declarations.
2. r[names.local.no-metadata] Decorators and [trait-less derivation blocks](14-annotations.md#trait-less-derivation-blocks) are not permitted in a local scope.

```hd
fn demo() -> i32:
    fn helper(n: i32) -> i32: n * 2   # a plain local fn: no pub, no decorators
    helper(+21)
```

### Local Implementations

1. r[names.local-impl.extent] A local `impl` contributes methods or trait conformance from its declaration point to the end of its enclosing suite and its child scopes.
2. r[names.local-impl.static] A local `impl` has no runtime execution step.
3. r[names.local-impl.no-capture] Neither its methods nor methods on a local trait capture enclosing runtime values.
4. r[names.local-impl.involve] A local implementation must involve a visible local nominal type or local trait, as specified in [Traits](09-traits.md).

```text
fn describe(name: string) -> string:
    type Label = string
    data Entry:
        label: Label
    enum Format:
        Plain
        Loud
    trait Named:
        fn name(self) -> string
    impl Named for Entry:
        fn name(self) -> string: self.label
    impl Entry:
        fn shout(self) -> string: self.label + "!"

    entry := Entry { label: name }
    match Format.Plain:
        Format.Plain => entry.name()
        Format.Loud => entry.shout()
```

### Receivers And Captures

1. r[names.self.reserved] Within a method, `self` is a reserved word that names the receiver parameter supplied by the receiver syntax.
2. r[names.self.mut] `mut self` is shorthand for `self: mut Self`; it does not create a different lookup category.
3. r[names.capture.resolve] A closure or local named function resolves otherwise-unbound local names in lexically enclosing scopes.
4. r[names.capture.definition] Those resolved names are its captures.
5. r[names.capture.mutation-access] Whether a capture permits mutation is determined by the captured binding and its access type alone, as specified in [Functions](07-functions.md#captures).
6. r[names.return.closure] `return` in a closure or trailing block targets that closure, not the enclosing named function.

```hd
fn demo() -> i32:
    base := +10
    add := fn(n: i32) -> i32: n + base   # captures base
    add(+5)
```

## Control-Flow Binding Scopes

This section defines the scopes of bindings made inside conditionals, loops,
matches, and comprehensions.

### Conditional Suites

1. r[names.cond.child] Each `if`, `else if`, and `else` suite has its own child scope.
2. r[names.cond.no-leak] A binding made inside one branch is not visible in another branch or after the conditional. A use there is an error. Error: `unknown-name`.
3. r[names.cond.condition] A binding expression in a condition follows the enclosing-scope rule of [Binding Expressions](#binding-expressions).

```text
fn main() -> i32:
    if true:
        hidden := 1
    hidden  # error: unknown-name
```

### Loops

A `for` binding is local to the loop body:

```text
for item in items:
    println(item)
```

1. r[names.loop.for-local] A `for` binding is local to the loop body.
2. r[names.loop.for-else] A `for` binding is not visible in the loop's `else` suite or after the loop. A use there is an error. Error: `unknown-name`.
3. r[names.loop.separate] The loop body and `else` suite are separate child scopes.
4. r[names.loop.per-iteration] Each iteration creates the body bindings for that iteration; a closure that escapes an iteration captures that iteration's binding rather than one shared loop variable.
5. r[names.loop.while] A `while` body and its `else` suite likewise have separate child scopes.

```text
fn last(values: List[i32]) -> i32:
    for value in values:
        if value > 10:
            break value
    else:
        value  # error: unknown-name
```

### Matches

Names bound by an arm's pattern are visible in that arm only:

```text
match error:
    ToolError.NotFound(resource) => resource
    ToolError.Internal(message) => message
```

1. r[names.match.arm-scope] Every match arm has an independent child scope.
2. r[names.match.visible] Names bound by an arm's pattern are visible in that arm's optional guard and body only.
3. r[names.match.reuse] Pattern bindings in different arms may reuse the same spelling.
4. r[names.match.duplicate] Duplicate bindings within one pattern are a compile-time error. Error: `duplicate-binding`.

```text
fn sum(pair: (i32, i32)) -> i32:
    match pair:
        (n, n) => n  # error: duplicate-binding
```

### Comprehensions

A comprehension has one scope of its own:

```text
pairs := [for x in xs for y in ys if x.id == y.owner_id => (x, y)]
```

1. r[names.comp.scope] A comprehension introduces one scope that does not leak into the enclosing scope.
2. r[names.comp.order] Clauses are processed left to right.
3. r[names.comp.for] Each `for` binding is visible to later clauses and the result expression, but not to earlier clauses.
4. r[names.comp.bind] A `:=` expression in a comprehension binds in the comprehension scope and is visible after the point where it is evaluated.

```text
sizes := [for user in users
          if (size := user.name.len()) > 0
          => size]
```

1. r[names.comp.no-leak] The names `user` and `size` are not visible after the comprehension. A use there is an error. Error: `unknown-name`.

```text
fn doubled(values: List[i32]) -> i32:
    results := [for value in values => value * 2]
    value  # error: unknown-name
```

## Member Resolution

This section defines which field `x.name` and which method `x.name(args)`
select.

### Member Namespaces

1. r[names.member.namespaces] Each nominal type has two member namespaces, and the form of a use chooses between them.
2. r[names.member.fields] Its **fields**, including embedded fields named by their embedded type name, are found by field lookup.
3. r[names.member.methods] Its **methods** are found by method lookup.
4. r[names.member.trait-methods] A type's **trait methods** are the methods of every trait that a known implementation implements for the type.
5. r[names.member.known-impl] A **known implementation** is any implementation in the program's dependency graph whose target matches the type. The exception is a local implementation, which counts only where its methods are available for lookup.
6. r[names.member.trait-available] A trait method takes part in method lookup only where its trait is available.
7. r[names.member.associated] Associated functions are not dot-call members; they are reached through `Type::function` or `Trait::function`.
8. r[names.member.shared-name] A field and a method may share a name, whether the method is inherent or a trait method.
9. r[names.member.no-hiding] Neither hides the other, because no use looks in both namespaces.

```hd
data Timer:
    count: i32

impl Timer:
    fn count(self) -> i32: self.count   # a field and a method may share a name
```

> **Note.** Trait availability works as in Rust, where a trait method is a
> candidate only while its trait is in scope.

See also: [Implementation Declarations](09-traits.md#implementation-declarations).

### Lookup Forms

1. r[names.lookup.forms] Member lookup resolves `x.name` and `x.name(args)` where the receiver `x` has nominal type `S` or `mut S`.
2. r[names.lookup.only] Member lookup is the only lookup algorithm for these forms; the rest of the specification refers to it.
3. r[names.lookup.field-form] `x.name` without an argument clause uses field lookup only. It never selects a method, because a bare method is not a value.
4. r[names.lookup.method-form] `x.name(args)` uses method lookup only. It never selects a field.
5. r[names.lookup.stored-fn] A function stored in a field is called as `(x.name)(args)`.

```text
data Base:
    pub run: fn() -> i32

data Job:
    Base

fn invalid(job: Job) -> i32:
    job.run()  # error: unknown-method
```

See also: [Member Access](05-expressions.md#member-access).

### Visibility

1. r[names.visible.field-method] A field or inherent method is **visible** from a module when it is declared in that module or marked `pub`.
2. r[names.visible.trait] A trait method is **available** when its trait is available to dot-call lookup there.

```hd
data User:
    pub name: string
    id: i32   # private: usable inside this module only
```

See also: [Data Declarations](08-data-and-enums.md#data-declarations),
[Inherent Implementations](09-traits.md#inherent-implementations),
[Method Resolution](09-traits.md#method-resolution).

### Depths And Promoted Members

1. r[names.part.definition] A **part** of `S` is a value reached from `S` through one or more embedded fields.
2. r[names.part.depth] A part's **depth** is the number of embedded fields on its path; the own fields and inherent methods of `S` are at depth 0.
3. r[names.promote.member] Each `pub` field and `pub` inherent method of a part's type, at any depth, is a **promoted member** of `S`. It sits at the part's depth, reached through the part's path. A private member or a trait method of a part's type is never promoted, even in the module that declares it. It has no effect on lookup through `S`.

```text
data Base:
    secret: string

data Record:
    Base

fn invalid(record: Record) -> string:
    record.secret  # error: unknown-data-field
```

### Members That Take Part

1. r[names.take-part.definition] The members of `S` that **take part** in lookup are its own fields and inherent methods, whatever their visibility, and its promoted members.
2. r[names.take-part.uniform] They are the same for every use, in every module. A type has a single view of its members, and each name resolves to the same member for every caller. Visibility decides only whether a caller may use the member that lookup finds.

```hd
data Point:
    x: i32
    y: i32

fn demo(p: Point) -> i32:
    p.x + p.y
```

> **Note.** This differs from Rust's and Go's privacy-aware lookup, where a
> private name does not match outside its module.

### Hiding And Conflicts

1. r[names.hide.depth] In each namespace, a member **hides** every member with the same name at a greater depth.
2. r[names.conflict.definition] A **conflict** is two or more members with one name at the smallest depth where that name occurs. That includes one member reached through two different paths. An example is the embedded field name of a type embedded twice at one depth.
3. r[names.conflict.private-shadow] An own field or inherent method of `S` may have the name of a promoted member in its namespace. When that field or method is not `pub`, it is also a conflict.

Only a `pub` own member hides a promoted one. A private own member with
that name is an error, and its author picks another name:

```text
data CreatedBySystem:
    pub id: string

data AuditDraft:
    CreatedBySystem
    id: string  # error: ambiguous-promoted-member

data ReviewDraft:
    CreatedBySystem
    pub id: string  # valid: hides CreatedBySystem's id
```

#### Conflicts Are Declaration Errors

1. r[names.conflict.error] Every conflict is an error at the declaration of `S`, never at a use, so lookup never meets one. Error: `ambiguous-promoted-member`.

```text
data Left:
    pub id: string

data Right:
    pub id: string

data LeftBox:
    Left

data RightBox:
    Right

data Record:
    LeftBox
    RightBox  # error: ambiguous-promoted-member
```

> **Note.** For the error revamp: a conflict is reported on the later of
> the two embedded fields of `S` that reach the conflicting members. When
> both are reached through one embedded field `E`, it is
> reported at `S` only when it is not also a conflict of `E`'s type. The
> message names both paths, as in `Record.LeftBox.Left.id` and
> `Record.RightBox.Right.id`. A private own member's conflict is reported
> on that member, and its message names the promoted member's path, as in
> `AuditDraft.CreatedBySystem.id`.

> **Note.** A package that adds a `pub` member to a type used as a part can
> therefore break the declarations of types that embed it. They break in
> their own packages, and a use never breaks.

> **Note.** An implementation may compute one table of resolved members for
> each data type. It holds the type's own members at depth 0 and the `pub`
> members of every part at the part's depth. A shallower member
> replaces a deeper one. A part's private member is not in the table, so it hides
> nothing there. The rules above define only the result.

### Field Lookup

**Field lookup** of `x.name` from a module `M` proceeds as follows:

1. r[names.field-lookup.select] **Selection.** Among the fields of `S` that take part, the field named `name` at the smallest depth is selected. There is at most one, because a conflict is a declaration error.
2. r[names.field-lookup.private] **Visibility.** When the selected field is an own field of `S` that is not visible from `M`, the use is an error. Error: `private-member`.
3. r[names.field-lookup.unknown] **Not found.** If no field named `name` takes part, the use is an error. Error: `unknown-data-field`.

```hd
data User:
    name: string

fn demo(user: User) -> string:
    user.name
```

### Method Lookup

**Method lookup** of `x.name(args)` from a module `M` tries an own inherent
method, then the candidates, and reports an error when neither applies.

#### Own Inherent Methods

1. r[names.method-lookup.inherent] If `S` has a visible inherent method named `name`, it is selected; it wins over every trait method.
2. r[names.method-lookup.inherent.by-name] Selection is by name alone, whatever the method's arity or parameter types. A visible inherent method with the wrong signature is still selected, and the call is then checked against it.

```hd
data Counter:
    value: i32

impl Counter:
    fn read(self) -> i32: self.value

fn demo(c: Counter) -> i32:
    c.read()
```

#### Method Candidates

1. r[names.method-lookup.candidates] Otherwise the candidates are the promoted candidate and the trait candidates.
2. r[names.method-lookup.promoted-candidate] The **promoted candidate** is, among the promoted inherent methods that take part, the one named `name` at the smallest depth, if any.
3. r[names.method-lookup.trait-candidates] The **trait candidates** are the trait methods of `S` named `name` whose trait is available at the call, wherever their implementations are declared.
4. r[names.method-lookup.unavailable] A trait method whose trait is not available is not a candidate and has no effect on the lookup.
5. r[names.method-lookup.one] Exactly one candidate is selected; a single candidate with the wrong signature is still selected, and the call is then checked against it.
6. r[names.method-lookup.generic-trait] When the only candidates come from several instantiations of one generic trait, the call chooses among them as in [Method Resolution](09-traits.md#method-resolution).
7. r[names.method-lookup.ambiguous] A promoted candidate beside a trait candidate is an error, and so are trait candidates of two or more traits. This holds whatever the signatures, and wherever the implementation is declared, a delegating one included. Neither silently wins. Error: `ambiguous-method`.

> **Note.** For the error revamp: the `ambiguous-method` message suggests
> the trait-qualified form `Trait::name(x, ...)` or the explicit path
> `x.E1...Ek.name(args)`.

```text
trait Describe:
    fn describe(self) -> string

data Base:
    name: string

impl Base:
    pub fn describe(self) -> string:
        "base " + self.name

data Page:
    Base

impl Describe for Page:
    fn describe(self) -> string:
        "page " + self.Base.name

fn invalid(page: Page) -> string:
    page.describe()  # error: ambiguous-method
```

#### Methods Not Found

1. r[names.method-lookup.private] With no candidate, the use is an error when `S` itself has an inherent method named `name`, which is then not visible from `M`. Error: `private-member`.
2. r[names.method-lookup.unknown] With no candidate and no such inherent method of `S`, the use is an error. Error: `unknown-method`.
3. r[names.method-lookup.hint.use] The message should suggest a use declaration when `S` has a trait method named `name` whose trait is not available at the call.
4. r[names.method-lookup.hint.field] The message should suggest `(x.name)(args)` when the receiver has a field named `name`.

```text
data Button:
    on_click: fn(i32) -> i32

fn invalid(button: Button) -> i32:
    button.on_click(41)   # error: unknown-method
```

> **Note.** For the error revamp: the message should suggest the explicit
> path `x.E1...Ek.name(args)` when a part's type has a trait method named
> `name`.

#### Method Lookup Example

Here `Page` embeds `Label`, `Label` implements `Display` and embeds `Base`,
and `Base` has a `pub` inherent `to_string`:

```text
data Base:
    pub name: string

impl Base:
    pub fn to_string(self) -> string:
        "base " + self.name

data Label:
    Base

impl Display for Label:
    fn to_string(self) -> string:
        "label"

data Page:
    Label

fn show(page: Page) -> string:
    page.to_string()                # Base's, promoted at depth 2; Label's method is not promoted
    page.Label.to_string()          # error: ambiguous-method
    Display::to_string(page.Label)  # Label's Display method
    page.Label.Base.to_string()     # Base's to_string
```

If `Page` also implemented `Display`, `page.to_string()` would be
`ambiguous-method` too, and `Page`'s method is called as
`Display::to_string(page)`. Without `Base`, `page.to_string()` is
`unknown-method`, and its message should suggest `page.Label.to_string()`.

### Promoted Member Access

1. r[names.promoted.path] When the selected member is promoted, `x.name` means the explicit path `x.E1.E2...Ek.name` through the embedded fields `E1` to `Ek`. It has the same type, permission, and evaluation. Each embedded step follows its container's access, so through a `mut S` receiver a promoted field may be assigned and a promoted `mut self` method called. A promoted method runs as the embedded type's own method with the part as its receiver, so there is no overriding.
2. r[names.promoted.readonly] Through a readonly `S`, a promoted field is readonly, and a promoted `mut self` method is selected and then rejected. Lookup never skips it to try another member. Error: `mutable-receiver-required`.
3. r[names.promoted.explicit] Explicit qualification through an embedded field, as in `x.E1.name`, starts a new lookup with `E1`'s type as the receiver's type. There the member's own visibility applies. It reaches a private member of a part, as in `x.Part.secret`, where that member is visible.

```text
data Resetter:
    count: i32

impl Resetter:
    pub fn reset(self) -> i32:
        0

data Wrapper:
    Resetter

data Counter:
    value: i32

impl Counter:
    pub fn reset(mut self) -> void:
        self.value = 0

data Page:
    Counter
    Wrapper

fn invalid(page: Page) -> void:
    page.reset()  # error: mutable-receiver-required
```

See also: [Mutable Paths](04-type-system.md#mutable-paths).

### Dependency Changes

> **Note.** The shallower member wins even across packages. A part's type
> in a dependency may gain a public member at a shallower depth than the
> one a use selects. The use then silently selects the new member.

> **Note.** No other change outside the package that owns `S` switches a
> use silently. A new conflict is an error at the declaration of `S`, and
> a new candidate can make a call ambiguous but never takes its place.

> **Why.** This consequence is intended. Rust accepts the same kind of switch
> when a trait import changes which `Deref` step answers a method call.

### No Overriding

A promoted method is its explicit path, as
[`names.promoted.path`](#r-names.promoted.path) states, so inside it `self`
has the embedded type. A call `self.m()` inside `Base`'s methods resolves
against `Base` and never reaches a method of a type that embeds `Base`.

See also: [Embedding And Trait Satisfaction](09-traits.md#embedding-and-trait-satisfaction).

### Enum Variants And Tuple Members

Enum variants may be named with the qualified or the contextual spelling:

```text
status := JobStatus.Queued

match status:
    .Queued => "waiting"
```

1. r[names.variant.member] Enum variants are members of their enum.
2. r[names.variant.spelling] Construction and patterns may use the qualified spelling or `.Variant` with an unambiguous contextual enum type.
3. r[names.tuple-member.underscore] Tuple members are named `_0`, `_1`, and so on: `_` followed by the zero-based element index.
4. r[names.tuple-member.identifier] A tuple member name is an ordinary identifier.

## Unsupported Scope Extensions

1. r[names.unsupported.shadow-warning] hd-lang permits shadowing of outer local names; a style tool may warn about it, but that warning is not part of language semantics.
2. r[names.unsupported.variant-use] hd-lang does not support direct uses of enum variants.
3. r[names.unsupported.stored-value] Top-level stored values use ordinary `:=` and `let` bindings; there is no separate stored-value declaration form.

```hd
enum Choice:
    Yes
    No

fn demo() -> Choice:
    Choice.Yes   # the qualified spelling
```
