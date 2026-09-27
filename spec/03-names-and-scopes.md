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
4. r[names.module.unique] A module cannot contain two declarations with the same module name, even if they are different kinds of declaration.
5. r[names.module.no-overloading] Function overloading is therefore not permitted.

```text
fn value(input: i32) -> i32: input
fn value(input: string) -> string: input  # error
```

### Prelude Names

1. r[names.prelude.source] Every built-in core type is supplied by the prelude described in [Modules and Packages](10-modules.md#prelude).
2. r[names.prelude.not-reserved] Prelude names remain ordinary identifier tokens rather than reserved words.
3. r[names.prelude.no-shadow] A module declaration, use, type parameter, parameter, or local binding must not shadow a prelude name. Every such conflict is an error. Error: `prelude-name-shadow`.
4. r[names.prelude.one-namespace] There is no separate predeclared-name namespace or shadowing rule.

```text
data i32:  # error: prelude-name-shadow
    value: string
```

### Type Parameters

1. r[names.type-param.local] Type parameters are type names local to their declaration.
2. r[names.type-param.shadow] A type parameter may shadow a module name within that declaration.
3. r[names.type-param.unique] A type parameter must not duplicate another type parameter in the same parameter list.

### The `Self` Name

1. r[names.self-type.reserved] `Self` is a reserved word that names the implementation target inside a trait or `impl` body.
2. r[names.self-type.trait] In a trait, `Self` denotes the eventual implementing type.
3. r[names.self-type.impl] In a trait or inherent implementation, `Self` denotes that implementation's target type.
4. r[names.self-type.outside] `Self` has no valid type meaning outside those bodies.

## Lexical Scopes

1. r[names.scope.lexical] Scopes are lexical.
2. r[names.scope.constructs] The following constructs introduce a local scope:

- a function or closure body;
- a module-level `test` body;
- each indented or same-line suite of `if`, `else if`, and `else`;
- the body and `else` suite of a `for` or `while` loop;
- each `match` arm;
- a comprehension;
- a trailing callback block.

1. r[names.scope.shadow] A name declared in an inner scope may shadow a name from an outer scope.
2. r[names.scope.duplicate] Two bindings with the same name in one scope are a compile-time error.
3. r[names.scope.reassign] Reassignment uses `=` and does not introduce another binding.
4. r[names.scope.test] Each `test` block has an independent local scope.
5. r[names.scope.test.isolated] The bindings of a `test` block do not become module execution bindings and are not visible to another test block.

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

### Module Execution Scope

1. r[names.exec.scope] Top-level executable statements run in a separate module execution scope.
2. r[names.exec.visible] Bindings created by those statements become visible to later top-level statements and to the bodies of functions declared after their binding point.
3. r[names.exec.access] Those functions may read a top-level `:=` or `let` binding and may reassign a top-level `let` binding.
4. r[names.exec.not-usable] Such bindings are not nameable by a `use` declaration.
5. r[names.exec.not-before] Such bindings are not visible before their binding point, including from the body of a function declared earlier.
6. r[names.exec.declarations] A top-level executable statement may refer to a named module declaration regardless of that declaration's textual position.
7. r[names.exec.methods] In the rules of this subsection, a function also means an inherent or trait method, and a method's position is that of its `impl` block.

```text
fn read_count() -> i32:
    count  # error

let count: i32 = 0
```

### Initialization Order

1. r[names.init.no-bypass] Referring to a named module declaration from a top-level executable statement does not bypass initialization order.
2. r[names.init.transitive] At every top-level executable statement, the compiler computes the transitive set of top-level bindings read by every module function or closure referenced by that statement.
3. r[names.init.references] A function or closure counts as referenced whether it is called directly, passed as a value, or reached through a trait method, interpolation, iteration, or another implicit call.
4. r[names.init.required] Every binding in that set must have been initialized by an earlier top-level statement.
5. r[names.init.whole-module] This is a whole-module value-flow and call-graph check; an indirect read through a later function value is rejected like a direct forward binding reference.

### Declarations In Blocks

1. r[names.block-decl.allowed] Named `fn`, `data`, `enum`, `trait`, and `type` declarations may also occur inside executable block suites.
2. r[names.block-decl.impl] `impl` declarations may occur at module scope or inside an executable block suite; they do not introduce an independently referencable name.

See also: [Function And Closure Scopes](#function-and-closure-scopes).

### Module Privacy

1. r[names.pub.private-default] Declarations are module-private unless marked `pub`.
2. r[names.pub.eligible] `pub` makes a declaration eligible to be used from another module.
3. r[names.pub.no-export] `pub` does not register a Wasm export or make a declaration host-callable.

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
10. r[names.use.cycles] Cycles involving `use` or `pub use` are compile-time errors.

### Public Use Declarations

1. r[names.use.pub] Prefixing a grouped use declaration with `pub` makes every name it introduces available to other modules.
2. r[names.use.pub.source] The source declaration must already be public.
3. r[names.use.pub.binding] A `pub use` introduces the same local binding as an ordinary `use`; it additionally exposes that binding without creating a new declaration identity.

## Local Bindings

`:=` and `let` introduce local names:

```text
name := "Ada"
```

```text
let count: i32 = 0
count = count + 1
```

1. r[names.local.short] `:=` introduces inferred, non-reassignable local names.
2. r[names.local.let] `let` introduces local names that may be reassigned.
3. r[names.local.same-scope] The two forms differ in rebinding permission, not in lexical scope.
4. r[names.local.mutation] Reference mutation permission comes from `mut T` in the binding's type and is independent of whether the local name can be reassigned.

```text
fn invalid() -> void:
    name := "Ada"
    name = "Grace"  # error
```

### Tuple Bindings

A tuple binding introduces several names at once:

```text
x, y := point
let name, score = entry
```

1. r[names.tuple.simultaneous] Tuple binding introduces every listed name simultaneously after evaluating the initializer.
2. r[names.tuple.distinct] All names in one binding pattern must be distinct.
3. r[names.tuple.annotation] When a `let` tuple binding has one type annotation, that annotation describes the complete right-hand tuple, not each individual name.
4. r[names.tuple.arity] The annotation's arity must match the binding pattern, and each local receives the corresponding element type.

## Binding Expressions

A `:=` binding expression binds in its enclosing scope:

```text
if (trimmed := input.trim()) != "":
    println(trimmed)
```

1. r[names.bind.expression] `:=` may appear as the lowest-precedence expression.
2. r[names.bind.scope] The binding of a `:=` expression belongs to the nearest enclosing executable scope, not to a synthetic scope around the subexpression.
3. r[names.bind.visible] In the example, `trimmed` is visible after its initializer completes, including in the selected `if` suite and in later statements of the enclosing scope.
4. r[names.bind.initialized] `trimmed` is initialized regardless of which `if` branch executes because the condition is evaluated before branch selection.
5. r[names.bind.no-redeclare] A binding expression must not redeclare a name already bound in that same scope.

```text
fn main() -> i32:
    value := 1
    (value := 2)  # error
```

> **Note.** Use assignment to update a `let` binding, or introduce an inner
> suite to shadow an outer name.

### Definite Initialization

1. r[names.definite.required] Lexical scope does not waive definite initialization.
2. r[names.definite.path] A `:=` name is usable on a control-flow path only after that path evaluates its initializer.
3. r[names.definite.merge] At a merge, the name is definitely initialized only if every incoming reachable path has evaluated the same binding.
4. r[names.definite.reject] The compiler rejects a use that may observe an uninitialized binding.
5. r[names.definite.condition] A direct binding in an `if` or `while` condition is evaluated whenever that condition is evaluated.
6. r[names.definite.skipped] A binding inside the conditionally evaluated operand of `&&` or `||`, an unselected branch or match arm, or a loop body is not thereby initialized on paths that skip it.
7. r[names.definite.proof] Flow analysis may still prove it initialized inside a branch whose selection implies that the binding ran.

```text
fn invalid(flag: bool) -> string:
    if flag && (name := "Ada") != "":
        pass
    name  # error
```

## Function And Closure Scopes

1. r[names.fn.params.visible] A function's generic parameters and value parameters are visible throughout its signature after their declaration point and throughout its body.
2. r[names.fn.params.scope] All value parameters belong to the function body's outermost local scope and must have distinct names.

### Local Functions

1. r[names.local-fn.name] A named function declared in a block suite introduces a local value name at its declaration point.
2. r[names.local-fn.visible] The name is visible in the rest of that suite and in the function's own body, permitting recursion.
3. r[names.local-fn.not-visible] The name is not visible before its declaration, outside the suite, or from another module.
4. r[names.local-fn.rules] A local named function follows the same shadowing and duplicate-name rules as other local values.

### Local Type Declarations

1. r[names.local-type.name] A local `data`, `enum`, `trait`, or `type` declaration introduces a type name at its declaration point, visible in its own definition and in the rest of its enclosing suite.
2. r[names.local-type.not-visible] The name is not visible before that point or outside the suite.
3. r[names.local-type.static] Local type declarations do not execute, capture runtime values, or become module members nameable by a `use` declaration.
4. r[names.local-type.refs] Local type declarations may refer to type names and type parameters visible at their declaration point.
5. r[names.local-type.identity] A local nominal type has one declaration identity, not a fresh identity per call to the enclosing function.
6. r[names.local-type.duplicate] Local type and value declarations cannot duplicate a name in the same scope.
7. r[names.local-type.prelude] Local type declarations are also subject to the prelude shadowing rule.

See also: [Prelude Names](#prelude-names).

### Local Declaration Limits

1. r[names.local.no-pub] `pub` is not permitted on local declarations.
2. r[names.local.no-decorators] Decorators and `annotate` declarations are not permitted in a local scope.
3. r[names.local.annotations-global] Annotation coherence, initialization, and memoization remain package-global even though undecorated local declarations are available.

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
5. r[names.capture.mutation] Whether a capture permits mutation is determined by the function type and the captured value's access type, as specified in [Functions](07-functions.md).
6. r[names.return.closure] `return` in a closure or trailing block targets that closure, not the enclosing named function.

## Control-Flow Binding Scopes

This section defines the scopes of bindings made inside conditionals, loops,
matches, and comprehensions.

### Conditional Suites

1. r[names.cond.child] Each `if`, `else if`, and `else` suite has its own child scope.
2. r[names.cond.no-leak] A binding made inside one branch is not visible in another branch or after the conditional.
3. r[names.cond.condition] A binding expression in a condition follows the enclosing-scope rule of [Binding Expressions](#binding-expressions).

```text
fn main() -> i32:
    if true:
        hidden := 1
    hidden  # error
```

### Loops

A `for` binding is local to the loop body:

```text
for item in items:
    println(item)
```

1. r[names.loop.for-local] A `for` binding is local to the loop body.
2. r[names.loop.for-else] A `for` binding is not visible in the loop's `else` suite or after the loop.
3. r[names.loop.separate] The loop body and `else` suite are separate child scopes.
4. r[names.loop.per-iteration] Each iteration creates the body bindings for that iteration; a closure that escapes an iteration captures that iteration's binding rather than one shared loop variable.
5. r[names.loop.while] A `while` body and its `else` suite likewise have separate child scopes.

```text
fn last(values: List[i32]) -> i32:
    for value in values:
        if value > 10:
            break value
    else:
        value  # error
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
4. r[names.match.duplicate] Duplicate bindings within one pattern are a compile-time error.

```text
fn sum(pair: (i32, i32)) -> i32:
    match pair:
        (n, n) => n  # error
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
labels := [for user in users
           if (label := user.name.trim().lower()) != ""
           => label]
```

1. r[names.comp.no-leak] The names `user` and `label` are not visible after the comprehension.

```text
fn doubled(values: List[i32]) -> i32:
    results := [for value in values => value * 2]
    value  # error
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
3. r[names.visible.promoted] Embedded fields are always public, and only `pub` members are promoted, so a promoted member is visible wherever its receiver's type is.

See also: [Data Declarations](08-data-and-enums.md#data-declarations),
[Inherent Implementations](09-traits.md#inherent-implementations),
[Method Resolution](09-traits.md#method-resolution).

### Depths And Promoted Members

1. r[names.part.definition] A **part** of `S` is a value reached from `S` through one or more embedded fields.
2. r[names.part.depth] A part's **depth** is the number of embedded fields on its path.
3. r[names.part.depth.levels] An embedded field of `S` holds a part at depth 1. An embedded field of that part's type holds a part at depth 2, and so on, up to depth 3, the deepest that [Data Embedding](08-data-and-enums.md#data-embedding) allows.
4. r[names.part.depth.own] The own fields and inherent methods of `S` are at depth 0.
5. r[names.promote.member] Each `pub` field and `pub` inherent method of a part's type is a **promoted member** of `S` at the part's depth, reached through the part's path.
6. r[names.promote.private] A private field or inherent method of a part's type is never promoted, even when the part's type is declared in the same module as `S`.
7. r[names.promote.private.path] Such a member is reached only through an explicit path, as in `x.Part.secret`, where lookup starts at the part's type and the member's own visibility applies.
8. r[names.promote.no-trait] Trait methods of a part's type are never promoted members, and they have no effect on lookup through `S`.

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
2. r[names.take-part.uniform] They are the same for every use, in every module. A type has a single view of its members, and each name resolves to the same member for every caller.
3. r[names.take-part.visibility] Visibility decides only whether a caller may use the member that lookup finds.
4. r[names.take-part.private-part] A private member of a part's type is invisible to lookup through `S` everywhere, so adding one never changes or breaks a use of `S`.
5. r[names.take-part.caller] The caller's module never changes which field or inherent method a name of `S` means.
6. r[names.take-part.private-member] A private member is either the member every caller finds, when it is an own member, or absent from lookup through `S`, when it belongs to a part.
7. r[names.take-part.trait-caller] Only trait methods depend on the caller, through trait availability. A caller that cannot see an own inherent method skips it, and may then select an available trait method of that name.

> **Note.** This differs from Rust's and Go's privacy-aware lookup, where a
> private name does not match outside its module.

### Hiding And Conflicts

1. r[names.hide.depth] In each namespace, a member **hides** every member with the same name at a greater depth.
2. r[names.hide.own-names] Each type therefore decides its own names. A `pub` own member of `S` hides the promoted ones, and a `pub` member of a part's type hides the members promoted into that part.
3. r[names.conflict.definition] A **conflict** is two or more members with one name at the smallest depth where that name occurs, including one member reached through two different paths.
4. r[names.conflict.private-own] A private own member of `S` that has the name of a promoted member in its namespace is also a conflict: a private member never shadows a promoted one.
5. r[names.conflict.namespace] Fields and methods conflict only within their own namespace.
6. r[names.conflict.diamond] A data type reached through several paths needs no further rule. Its members reached at different depths resolve to the shallower copy, and at the same depth they conflict, starting with its embedded field name.

#### Conflicts Are Declaration Errors

1. r[names.conflict.error] Every conflict is an error at a declaration, never at a use, so lookup never meets one. Error: `ambiguous-promoted-member`.
2. r[names.conflict.one-check] Because every module sees the same members, one check of each data type covers every use.
3. r[names.conflict.promoted-site] A conflict between promoted members is reported at the data declaration of `S`, on the later of the two embedded fields of `S` through which the conflicting members are reached.
4. r[names.conflict.same-field] When both are reached through the same embedded field `E`, the conflict is reported at `S` only when it is not also a conflict of `E`'s type, which reports it itself.
5. r[names.conflict.paths-message] The message names both paths, as in `Record.LeftBox.Left.id` and `Record.RightBox.Right.id`.
6. r[names.conflict.private-site] A conflict of a private own member is reported on that member: the field in the data declaration of `S`, or the method in its inherent implementation.
7. r[names.conflict.private-message] Its message says that a private member cannot shadow a promoted one, and names the promoted member's path, as in `Record.Base.id`.

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

> **Note.** A private own member in a conflict is made `pub` or renamed.

> **Note.** A package that adds a `pub` member to a type used as a part can
> therefore break the declarations of types that embed it, in their own
> packages, but never a use.

> **Note.** An implementation may compute one table of resolved members for
> each data type. The table holds its own members at depth 0 and the `pub`
> entries of its direct parts' tables one level deeper, where the shallower
> member replaces the deeper one. A part's private entry never replaces a deeper one, because
> that would be a conflict of the part's type. The rules above define only
> the result.

### Field Lookup

**Field lookup** of `x.name` from a module `M` proceeds as follows:

1. r[names.field-lookup.select] **Selection.** Among the fields of `S` that take part, the field named `name` at the smallest depth is selected. There is at most one, because a conflict is a declaration error.
2. r[names.field-lookup.private] **Visibility.** When the selected field is an own field of `S` that is not visible from `M`, the use is an error. Error: `private-member`.
3. r[names.field-lookup.unknown] **Not found.** If no field named `name` takes part, the use is an error. Error: `unknown-data-field`.

These rules refine the steps:

1. r[names.field-lookup.private.alone] When the selected own field is not visible from `M`, no visible field can have its name, because a promoted field beside it would be a conflict.
2. r[names.field-lookup.part-private] A private field of a part's type never leads to `private-member`, even in the module that declares it.

### Method Lookup

**Method lookup** of `x.name(args)` from a module `M` tries an own inherent
method, then the candidates, and reports an error when neither applies.

#### Own Inherent Methods

1. r[names.method-lookup.inherent] If `S` has a visible inherent method named `name`, it is selected; it wins over every trait method.
2. r[names.method-lookup.inherent.by-name] Selection is by name alone, whatever the method's arity or parameter types. A visible inherent method with the wrong signature is still selected, and the call is then checked against it.
3. r[names.method-lookup.inherent.skip] An inherent method that is not visible is skipped. No promoted method has its name, because that would be a conflict, so only trait candidates remain.

#### Method Candidates

1. r[names.method-lookup.candidates] Otherwise the candidates are the promoted candidate and the trait candidates.
2. r[names.method-lookup.promoted-candidate] The **promoted candidate** is, among the promoted inherent methods that take part, the one named `name` at the smallest depth, if any.
3. r[names.method-lookup.trait-candidates] The **trait candidates** are the trait methods of `S` named `name` whose trait is available at the call, wherever their implementations are declared.
4. r[names.method-lookup.unavailable] A trait method whose trait is not available is not a candidate and has no effect on the lookup.
5. r[names.method-lookup.one] Exactly one candidate is selected; a single candidate with the wrong signature is still selected, and the call is then checked against it.
6. r[names.method-lookup.generic-trait] When the only candidates come from several instantiations of one generic trait, the call chooses among them as in [Method Resolution](09-traits.md#method-resolution).
7. r[names.method-lookup.ambiguous] A promoted candidate beside a trait candidate, or trait candidates of two or more traits, are an error, whatever the signatures. Error: `ambiguous-method`.
8. r[names.method-lookup.ambiguous.hint] The `ambiguous-method` message suggests the trait-qualified form `Trait::name(x, ...)` or the explicit path `x.E1...Ek.name(args)`.
9. r[names.method-lookup.no-silent] A trait method of `S` never silently wins over a promoted method, and a promoted method never silently wins over a trait method.

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
3. r[names.method-lookup.part-private] A private method of a part's type never leads to `private-member`, even in the module that declares it.
4. r[names.method-lookup.hint.use] The message should suggest a use declaration when `S` has a trait method named `name` whose trait is not available at the call.
5. r[names.method-lookup.hint.part-trait] The message should suggest the explicit path `x.E1...Ek.name(args)` when a part's type has a trait method named `name`.
6. r[names.method-lookup.hint.field] The message should suggest `(x.name)(args)` when the receiver has a field named `name`.

#### Method Lookup Example

Suppose `Page` embeds `Label`, `Label` implements `Display` and embeds `Base`,
and `Base` has a `pub` inherent `to_string`.

1. r[names.method-example.promoted] `page.to_string()` calls `Base`'s `to_string`, the promoted candidate at depth 2: `Label`'s `Display` method is not promoted and does not hide it.
2. r[names.method-example.part] `page.Label.to_string()` is an error: lookup starts at `Label`, where `Label`'s `Display` method is a trait candidate and `Base`'s `to_string` is the promoted candidate at depth 1. Error: `ambiguous-method`.
3. r[names.method-example.explicit] `Label`'s method is called as `Display::to_string(page.Label)`, and `Base`'s as `page.Label.Base.to_string()`.
4. r[names.method-example.outer-trait] If `Page` also implemented `Display`, `page.to_string()` would be an error too, and `Page`'s method is called as `Display::to_string(page)`. Error: `ambiguous-method`.
5. r[names.method-example.no-base] Without `Base`, `page.to_string()` is an error whose message should suggest `page.Label.to_string()`, which then calls `Label`'s `Display` method. Error: `unknown-method`.

### Promoted Member Access

1. r[names.promoted.path] When the selected member is promoted, `x.name` means the explicit path `x.E1.E2...Ek.name` through the embedded fields `E1` to `Ek`, with the same type, permission, and evaluation.
2. r[names.promoted.access] Each embedded step follows its container's access, so a promoted member has the access the receiver grants.
3. r[names.promoted.mut] Through a `mut S` receiver, a promoted field may be assigned and a promoted `mut self` method may be called.
4. r[names.promoted.readonly] Through a readonly `S`, a promoted field is readonly, and a promoted `mut self` method is selected and then rejected; lookup never skips it to try another member. Error: `mutable-receiver-required`.
5. r[names.promoted.explicit] Explicit qualification through an embedded field, as in `x.E1.name`, starts a new lookup with `E1`'s type as the receiver's type.
6. r[names.promoted.trait-qualified] `Trait::name(x, ...)` selects a trait method without member lookup.

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

1. r[names.change.shallower] The shallower member wins even across packages. When a part's type in a dependency gains a public member at a shallower depth than the one a use selects, the use may silently select the new member.
2. r[names.change.no-other] No other change outside the package that owns `S` switches a use silently.
3. r[names.change.conflict] A new conflict is an error at the declaration of `S`.
4. r[names.change.candidate] A new trait implementation, use declaration, or promoted method adds a candidate, which can make a call ambiguous but never selects a different method in its place.

> **Why.** This consequence is intended. Rust accepts the same kind of switch
> when a trait import changes which `Deref` step answers a method call.

### No Overriding

1. r[names.no-override] There is no overriding. A promoted method runs as the embedded type's own method, with the embedded value as its receiver.
2. r[names.no-override.self] Inside that method, `self` has the embedded type. So a call `self.m()` inside `Base`'s methods always resolves against `Base` and never reaches a method of a type that embeds `Base`.
3. r[names.no-conformance] Embedding never grants trait conformance, and a promoted method never fills a method of a trait implementation.

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
