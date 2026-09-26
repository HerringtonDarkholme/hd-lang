# Names and Scopes

Status: language specification draft.

This chapter defines the scopes introduced by hd-lang programs and how names
resolve. It does not define type compatibility or access permission.

## Name Categories

hd-lang has four lookup categories:

1. **Module names** identify top-level types, traits, functions, and names
   introduced by use declarations. A module cannot contain two declarations
   with the same module name, even if they are different kinds of declaration.
   Function overloading is therefore not permitted.
2. **Value names** identify top-level executable bindings, parameters, local
   bindings, local named functions, loop bindings, pattern bindings, and
   captured values.
3. **Local type names** identify data types, enums, traits, aliases, and newtypes
   declared inside an executable suite.
4. **Member names** identify data fields, embedded fields, methods, enum
   variants, and tuple fields within the namespace of their owning type.

A use is resolved in the category required by its syntax. For example, the
name before `{` in `User { ... }` is resolved as a type, while `user` in
`user.email` is resolved as a value and `email` as a member of its type.

Every built-in core type is supplied by the prelude described in
[Modules and Packages](10-modules.md#prelude). Prelude names remain ordinary
identifier tokens rather than reserved words, but a module declaration, use,
type parameter, parameter, or local binding must not shadow one. Every such
conflict is a `prelude-name-shadow` error; there is no separate predeclared-name
namespace or shadowing rule.

Type parameters are type names local to their declaration. They may shadow a
module name within that declaration but must not duplicate another type
parameter in the same parameter list.

`Self` is a reserved word that names the implementation target inside a trait
or `impl` body. In a trait it denotes the eventual implementing type; in a
trait or inherent implementation it denotes that implementation's target type.
It has no valid type meaning outside those bodies.

## Lexical Scopes

Scopes are lexical. The following constructs introduce a local scope:

- a function or closure body;
- a module-level `test` body;
- each indented or same-line suite of `if`, `else if`, and `else`;
- the body and `else` suite of a `for` or `while` loop;
- each `match` arm;
- a comprehension;
- a trailing callback block.

A name declared in an inner scope may shadow a name from an outer scope. Two
bindings with the same name in one scope are a compile-time error. Reassignment
uses `=` and does not introduce another binding.

Each `test` block has an independent local scope. Its bindings do not become
module execution bindings and are not visible to another test block.

The scope of a local binding begins immediately after its initializer has been
evaluated. The binding is not visible in its own initializer:

```text
name := normalize(name)  # the right-hand name, if valid, resolves outward
```

This permits ordinary inner-scope shadowing without making a new binding
self-referential.
The sole exception is a direct local closure initializer with an explicit
result type: its body may refer to the name being bound for recursion. The
initializer expression itself is still evaluated before the binding becomes
available; only the closure body receives this forward name. The closure body
cannot run before initialization completes.

## Module Scope

Every source file is a module. Its path-derived identity is specified in
[Modules](10-modules.md).

Top-level declarations belong to the module scope. Their names are visible
throughout the module, independent of textual order, which permits direct and
mutual recursion between functions in one module. A declaration must still be
well typed as a whole; forward visibility does not imply initialization order
for executable top-level statements.

Top-level executable statements run in a separate module execution scope.
Bindings created by those statements become visible to later top-level
statements and to the bodies of functions declared after their binding point.
Those functions may read a top-level `:=` or `let` binding and may reassign a
top-level `let` binding. Such bindings are not nameable by a `use` declaration and are
not visible before their binding point, including from the body of a function
declared earlier. A top-level executable statement may refer to a named module
declaration regardless of that declaration's textual position. In this
paragraph, a function also means an inherent or trait method, and a method's
position is that of its `impl` block.

Referring to such a declaration does not bypass initialization order. At every
top-level executable statement, the compiler computes the transitive set of
top-level bindings read by every module function or closure referenced by that
statement, whether called directly, passed as a value, or reached through a
trait method, interpolation, iteration, or another implicit call. Every binding
in that set must have been initialized by an earlier top-level statement. This
is a whole-module value-flow and call-graph check; an indirect read through a
later function value is rejected like a direct forward binding reference.

Named `fn`, `data`, `enum`, `trait`, and `type` declarations may also occur
inside executable block suites. `impl` declarations may occur at module scope
or inside an executable block suite; they do not introduce an independently
referencable name.

Declarations are module-private unless marked `pub`. `pub` makes a declaration
eligible to be used from another module. It does not register a Wasm export
or make a declaration host-callable.

## Use Declarations

A `use` declaration introduces either one local module name or one or more
local declaration names into the using module's module scope:

```text
use pkg.user.types
use pkg.user.types as user_types
use pkg.user.types.{User, UserId}
use dep.billing.types.{UserId as BillingUserId}
```

A path-only use may name either a module namespace or one public declaration.
Without `as`, it binds the final path component; the first example therefore
binds `types`. A grouped use binds each selected name after applying any item
alias.

A used name must not collide with another module-scope declaration or use.
Use declarations do not create overload sets and are not implicitly renamed.

Use declarations are resolved before declarations are type checked. Their
textual position does not limit their visibility, but style tools should place
them before other top-level items. Cycles involving `use` or `pub use` are
compile-time errors.

Prefixing a grouped use declaration with `pub` makes every name it introduces
available to other modules. The source declaration must already be public. A
`pub use` introduces the same local binding as an ordinary `use`; it
additionally exposes that binding without creating a new declaration identity.

## Local Bindings

`:=` introduces inferred, non-reassignable local names:

```text
name := "Ada"
```

`let` introduces local names that may be reassigned:

```text
let count: i32 = 0
count = count + 1
```

The two forms differ in rebinding permission, not in lexical scope. Reference
mutation permission comes from `mut T` in the binding's type and is independent
of whether the local name can be reassigned.

Tuple binding introduces every listed name simultaneously after evaluating the
initializer:

```text
x, y := point
let name, score = entry
```

All names in one binding pattern must be distinct.
When a `let` tuple binding has one type annotation, that annotation describes
the complete right-hand tuple, not each individual name. Its arity must match
the binding pattern, and each local receives the corresponding element type.

## Binding Expressions

`:=` may appear as the lowest-precedence expression. Its binding belongs to the
nearest enclosing executable scope, not to a synthetic scope around the
subexpression:

```text
if (trimmed := input.trim()) != "":
    println(trimmed)
```

`trimmed` is visible after its initializer completes, including in the selected
`if` suite and in later statements of the enclosing scope. It is initialized
regardless of which `if` branch executes because the condition is evaluated
before branch selection.

A binding expression must not redeclare a name already bound in that same
scope. Use assignment to update a `let` binding, or introduce an inner suite to
shadow an outer name.

Lexical scope does not waive definite initialization. A `:=` name is usable on
a control-flow path only after that path evaluates its initializer. At a merge,
the name is definitely initialized only if every incoming reachable path has
evaluated the same binding. The compiler rejects a use that may observe an
uninitialized binding.

A direct binding in an `if` or `while` condition is evaluated whenever that
condition is evaluated. A binding inside the conditionally evaluated operand of
`&&` or `||`, an unselected branch or match arm, or a loop body is not thereby
initialized on paths that skip it. Flow analysis may still prove it initialized
inside a branch whose selection implies that the binding ran.

## Function And Closure Scopes

A function's generic parameters and value parameters are visible throughout
its signature after their declaration point and throughout its body. All value
parameters belong to the function body's outermost local scope and must have
distinct names.

A named function declared in a block suite introduces a local value name at
its declaration point. The name is visible in the rest of that suite and in
the function's own body, permitting recursion. It is not visible before its
declaration, outside the suite, or from another module. It follows the same
shadowing and duplicate-name rules as other local values.

A local `data`, `enum`, `trait`, or `type` declaration introduces a type name
at its declaration point, visible in its own definition and in the rest of its
enclosing suite. It is not visible before that point or outside the suite.
Local type declarations do not execute, capture runtime values, or become
module members nameable by a `use` declaration. They may refer to type names and type parameters
visible at their declaration point. A local nominal type has one declaration
identity, not a fresh identity per call to the enclosing function. Local type
and value declarations cannot duplicate a name in the same scope; local type
declarations are also subject to the prelude shadowing rule. `pub` is not
permitted on local declarations.

Decorators and `annotate` declarations are not permitted in a local scope.
Annotation coherence, initialization, and memoization remain package-global
even though undecorated local declarations are available.

A local `impl` contributes methods or trait conformance from its declaration
point to the end of its enclosing suite and its child scopes. It has no runtime
execution step. Neither its methods nor methods on a local trait capture
enclosing runtime values. A local implementation must involve a visible local
nominal type or local trait, as specified in [Traits](09-traits.md).

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

Within a method, `self` is a reserved word that names the receiver parameter
supplied by the receiver syntax. `mut self` is shorthand for `self: mut Self`;
it does not create a different lookup category.

A closure or local named function resolves otherwise-unbound local names in
lexically enclosing scopes. Those resolved names are its captures. Whether a
capture permits mutation is determined by the function type and the captured
value's access type, as specified in [Functions](07-functions.md).

`return` in a closure or trailing block targets that closure, not the enclosing
named function.

## Control-Flow Binding Scopes

### Conditional Suites

Each `if`, `else if`, and `else` suite has its own child scope. A binding made
inside one branch is not visible in another branch or after the conditional.
A binding expression in a condition follows the enclosing-scope rule described
above.

### Loops

A `for` binding is local to the loop body:

```text
for item in items:
    println(item)
```

It is not visible in the loop's `else` suite or after the loop. The loop body
and `else` suite are separate child scopes. Each iteration creates the body
bindings for that iteration; a closure that escapes an iteration captures that
iteration's binding rather than one shared loop variable.

A `while` body and its `else` suite likewise have separate child scopes.

### Matches

Every match arm has an independent child scope. Names bound by an arm's pattern
are visible in that arm's optional guard and body only:

```text
match error:
    ToolError.NotFound(resource) => resource
    ToolError.Internal(message) => message
```

Pattern bindings in different arms may reuse the same spelling. Duplicate
bindings within one pattern are a compile-time error.

### Comprehensions

A comprehension introduces one scope that does not leak into the enclosing
scope. Clauses are processed left to right:

```text
pairs := [for x in xs for y in ys if x.id == y.owner_id => (x, y)]
```

Each `for` binding is visible to later clauses and the result expression, but
not to earlier clauses. A `:=` expression in a comprehension binds in the
comprehension scope and is visible after the point where it is evaluated:

```text
labels := [for user in users
           if (label := user.name.trim().lower()) != ""
           => label]
```

The names `user` and `label` are not visible after the comprehension.

## Member Resolution

Each nominal type has two member namespaces, and the form of a use chooses
between them. Its **fields**, including embedded fields named by their
embedded type name, are found by field lookup. Its **methods** are found by
method lookup. A type's **trait methods** are the methods of every trait
that a known implementation implements for the type. A **known
implementation** is any implementation in the program's dependency graph
whose target matches the type, except that a local implementation counts
only where its methods are available for lookup
([Implementation Declarations](09-traits.md#implementation-declarations)).
A trait method takes part in method lookup only where its trait is
available, as in Rust, where a trait method is a candidate only while its
trait is in scope.
Associated functions are not dot-call members; they are reached through
`Type::function` or `Trait::function`.

A field and a method may share a name, whether the method is inherent or a
trait method. Neither hides the other, because no use looks in both
namespaces.

Member lookup resolves `x.name` and `x.name(args)` where the receiver `x` has
nominal type `S` or `mut S`. It is the only lookup algorithm for these forms;
the rest of the specification refers to it.

- `x.name` without an argument clause uses field lookup only. It never
  selects a method, because a bare method is not a value
  ([Member Access](05-expressions.md#member-access)).
- `x.name(args)` uses method lookup only. It never selects a field. A
  function stored in a field is called as `(x.name)(args)`.

A field or inherent method is **visible** from a module when it is declared
in that module or marked `pub`
([Data Declarations](08-data-and-enums.md#data-declarations),
[Inherent Implementations](09-traits.md#inherent-implementations)). A trait
method is **available** when its trait is available to dot-call lookup there
([Method Resolution](09-traits.md#method-resolution)). Embedded fields are
always public
([Data Declarations](08-data-and-enums.md#data-declarations)), and only
`pub` members are promoted (below), so a promoted member is visible wherever
its receiver's type is.

**Depths and promoted members.** A **part** of `S` is a value reached from
`S` through one or more embedded fields. Its **depth** is the number of
embedded fields on its path: an embedded field of `S` holds a part at depth
1, an embedded field of that part's type holds a part at depth 2, and so on,
up to depth 3, the deepest that
[Data Embedding](08-data-and-enums.md#data-embedding) allows. The own fields and inherent methods of `S` are at depth 0. Each `pub` field and
`pub` inherent method of a part's type is a **promoted member** of `S` at
the part's depth, reached through the part's path. A private field or
inherent method of a part's type is never promoted, even when the part's
type is declared in the same module as `S`; it is reached only through an
explicit path, as in `x.Part.secret`, where lookup starts at the part's type
and the member's own visibility applies. Trait methods of a part's type are
never promoted members, and they have no effect on lookup through `S`.

The members of `S` that **take part** in lookup are its own fields and
inherent methods, whatever their visibility, and its promoted members. They
are the same for every use, in every module: a type has a single view of its
members, and each name resolves to the same member for every caller.
Visibility decides only whether a caller may use the member that lookup
finds. A private member of a part's type is invisible to lookup through `S`
everywhere, so adding one never changes or breaks a use of `S`.

In each namespace, a member **hides** every member with the same name at a
greater depth. Each type therefore decides its own names: a `pub` own member
of `S` hides the promoted ones, and a `pub` member of a part's type hides the
members promoted into that part. A **conflict** is two or more members with
one name at the smallest depth where that name occurs, including one member
reached through two different paths. A private own member of `S` that has
the name of a promoted member in its namespace is also a conflict: a private
member never shadows a promoted one.

**Conflicts are declaration errors.** Every conflict is an
`ambiguous-promoted-member` error at a declaration, never at a use, so
lookup never meets one. Because every module sees the same members, one
check of each data type covers every use. A conflict between promoted
members is reported at the data declaration of `S`, on the later of the two
embedded fields of `S` through which the conflicting members are reached.
When both are reached through the same embedded field `E`, it is reported at
`S` only when it is not also a conflict of `E`'s type, which reports it
itself. The message names both paths, as in `Record.LeftBox.Left.id` and
`Record.RightBox.Right.id`. A conflict of a private own member is reported
on that member: the field in the data declaration of `S`, or the method in
its inherent implementation. Its message says that a private member cannot
shadow a promoted one, and names the promoted member's path, as in
`Record.Base.id`. Such a member is made `pub` or renamed. Fields and methods
conflict only within their own namespace. A data type reached through
several paths needs no further rule: its members reached at different depths
resolve to the shallower copy, and at the same depth they conflict, starting
with its embedded field name. A package that adds a `pub` member to a type
used as a part can therefore break the declarations of types that embed it,
in their own packages, but never a use.

Unlike Rust's and Go's privacy-aware lookup, where a private name does not
match outside its module, the caller's module never changes which field or
inherent method a name of `S` means. A private member is either the member
every caller finds, when it is an own member, or absent from lookup through
`S`, when it belongs to a part. Only trait methods depend on the caller,
through trait availability: a caller that cannot see an own inherent method
skips it, and may then select an available trait method of that name.

Note: an implementation may compute, for each data type, one table of
resolved members: its own members at depth 0 and the `pub` entries of its
direct parts' tables one level deeper, where the shallower member replaces
the deeper one. A part's private entry never replaces a deeper one, because
that would be a conflict of the part's type. The rules above define only the
result.

**Field lookup** of `x.name` from a module `M` proceeds as follows:

1. **Selection.** Among the fields of `S` that take part, the field named
   `name` at the smallest depth is selected. There is at most one, because
   a conflict is a declaration error.
2. **Visibility.** When the selected field is an own field of `S` that is
   not visible from `M`, the use is a `private-member` error. No visible
   field can have its name, because a promoted field beside it would be a
   conflict.
3. **Not found.** If no field named `name` takes part, the use is an
   `unknown-data-field` error. A private field of a part's type never leads
   to `private-member`, even in the module that declares it.

**Method lookup** of `x.name(args)` from a module `M` proceeds as follows:

1. **Own inherent method.** If `S` has a visible inherent method named
   `name`, it is selected; it wins over every trait method. Selection is by
   name alone, whatever the method's arity or parameter types: a visible
   inherent method with the wrong signature is still selected, and the call
   is then checked against it. An inherent method that is not visible is
   skipped. No promoted method has its name, because that would be a
   conflict, so only trait candidates remain.
2. **Candidates.** Otherwise the candidates are:
   - the **promoted candidate**: among the promoted inherent methods that
     take part, the one named `name` at the smallest depth, if any;
   - the **trait candidates**: the trait methods of `S` named `name` whose
     trait is available at the call, wherever their implementations are
     declared. A trait method whose trait is not available is not a
     candidate and has no effect on the lookup.

   Exactly one candidate is selected; a single candidate with the wrong
   signature is still selected, and the call is then checked against it.
   When the only candidates come from several instantiations of one generic
   trait, the call chooses among them as in
   [Method Resolution](09-traits.md#method-resolution). A promoted candidate
   beside a trait candidate, or trait candidates of two or more traits, are
   an `ambiguous-method` error, whatever the signatures; its message suggests
   the trait-qualified form `Trait::name(x, ...)` or the explicit path
   `x.E1...Ek.name(args)`. A trait method of `S` never silently wins over a
   promoted method, and a promoted method never silently wins over a trait
   method.
3. **Not found.** With no candidate, the use is a `private-member` error
   when `S` itself has an inherent method named `name`, which is then not
   visible from `M`, and an `unknown-method` error otherwise. A private
   method of a part's type never leads to `private-member`, even in the
   module that declares it. The message should suggest a use declaration
   when `S` has a trait method named `name` whose trait is not available at
   the call; the explicit path `x.E1...Ek.name(args)` when a part's type has
   a trait method named `name`; and `(x.name)(args)` when the receiver has a
   field named `name`.

For example, if `Page` embeds `Label`, `Label` implements `Display` and
embeds `Base`, and `Base` has a `pub` inherent `to_string`, then
`page.to_string()` calls `Base`'s `to_string`, the promoted candidate at
depth 2: `Label`'s `Display` method is not promoted and does not hide it.
`page.Label.to_string()` is an `ambiguous-method` error: lookup starts at
`Label`, where `Label`'s `Display` method is a trait candidate and `Base`'s
`to_string` is the promoted candidate at depth 1. `Label`'s method is called
as `Display::to_string(page.Label)`, and `Base`'s as
`page.Label.Base.to_string()`. If `Page` also implemented `Display`,
`page.to_string()` would be an `ambiguous-method` error too, and `Page`'s
method is called as `Display::to_string(page)`. Without `Base`,
`page.to_string()` is an `unknown-method` error whose message should suggest
`page.Label.to_string()`, which then calls `Label`'s `Display` method.

When the selected member is promoted, `x.name` means the explicit path
`x.E1.E2...Ek.name` through the embedded fields `E1` to `Ek`, with the same
type, permission, and evaluation. Each embedded step follows its container's
access ([Mutable Paths](04-type-system.md#mutable-paths)), so a promoted
member has the access the receiver grants. Through a `mut S` receiver, a
promoted field may be assigned and a promoted `mut self` method may be
called. Through a readonly `S`, a promoted field is readonly, and a promoted
`mut self` method is selected and then rejected with
`mutable-receiver-required`; lookup never skips it to try another member.
Explicit qualification through an embedded field, as in `x.E1.name`, starts
a new lookup with `E1`'s type as the receiver's type. `Trait::name(x, ...)`
selects a trait method without member lookup.

One consequence is intended. The shallower member wins even across
packages, so when a part's type in a dependency gains a public member at a
shallower depth than the one a use selects, the use may silently select the
new member; Rust accepts the same kind of switch when a trait import changes
which `Deref` step answers a method call. No other change outside the
package that owns `S` switches a use silently: a new conflict is an error at
the declaration of `S`, and a new trait implementation, use declaration, or
promoted method adds a candidate, which can make a call ambiguous but never
selects a different method in its place.

There is no overriding. A promoted method runs as the embedded type's own
method, with the embedded value as its receiver. Inside that method, `self`
has the embedded type, so a call `self.m()` inside `Base`'s methods always
resolves against `Base` and never reaches a method of a type that embeds
`Base`.

Embedding never grants trait conformance, and a promoted method never fills a
method of a trait implementation; see
[Embedding And Trait Satisfaction](09-traits.md#embedding-and-trait-satisfaction).

Enum variants are members of their enum. Construction and patterns may use the
qualified spelling or `.Variant` with an unambiguous contextual enum type:

```text
status := JobStatus.Queued

match status:
    .Queued => "waiting"
```

Tuple members use numeric selectors such as `.0` and `.1`. Numeric selectors
are not ordinary identifiers and cannot be declared by users.

## Unsupported Scope Extensions

hd-lang permits shadowing of outer local names; a style tool may warn about it,
but that warning is not part of language semantics. hd-lang does not support
direct uses of enum variants. Top-level stored values use ordinary `:=` and
`let` bindings; there is no separate stored-value declaration form.
