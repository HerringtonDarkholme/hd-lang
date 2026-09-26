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
method lookup. A type's **own methods** are its inherent methods and its
trait methods, which are the methods of every trait that a known
implementation implements for the type. A **known implementation** is any
implementation in the program's dependency graph whose target matches the
type, except that a local implementation counts only where its methods are
available for lookup
([Implementation Declarations](09-traits.md#implementation-declarations)).
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

A field or inherent method is **visible** from the calling module when it is
declared in that module or marked `pub`
([Data Declarations](08-data-and-enums.md#data-declarations),
[Inherent Implementations](09-traits.md#inherent-implementations)). A trait
method is **available** when its trait is available to dot-call lookup there
([Method Resolution](09-traits.md#method-resolution)). Both lookups search
embedded fields the same way. An embedded field of `S` is at depth 1, an
embedded field of that field's type is at depth 2, and so on. A member found
through embedded fields is visible only when it and every embedded field on
its path are visible, because the use means that explicit path (see below).

Lookup **skips** a field or inherent method that is not visible, as if it
were absent, and keeps searching. It never stops at an invisible member, and
it reports one only when nothing visible is found. Inside the defining module
every member is visible, so a private own member hides a promoted one there;
in another module, the same use reaches the visible promoted member. Adding a
private member therefore never changes or breaks a use in another module.
This is privacy-aware lookup as in Rust, and matches Go, where another
package's unexported names never match.

**Field lookup** proceeds as follows:

1. **Own fields.** If `S` declares a visible field named `name`, that field
   is selected and embedded fields are not searched. An own field that is not
   visible is skipped.
2. **Embedded fields.** Otherwise lookup searches the data types reachable
   through embedded fields, breadth first. At each depth, a **match** is a
   visible field named `name` of an embedded type at that depth. The first
   depth with a match decides the lookup, so a shorter path always wins over
   a longer one. Two or more matches at that depth, including one field
   reached through two different paths, are an `ambiguous-promoted-member`
   error.
3. **No visible field.** If neither step selects a field, the use is a
   `private-member` error when lookup skipped a field named `name`, and an
   `unknown-data-field` error otherwise.

**Method lookup** proceeds as follows:

1. **Own methods.** If `S` has a visible inherent method named `name`, it is
   selected; it wins over every trait method, and embedded fields are not
   searched. Otherwise, if `S` has a trait method named `name`, lookup stops
   at `S`, whether or not its trait is available:
   - Exactly one available trait method is selected. Methods of two or more
     available traits are an `ambiguous-method` error. When the methods come
     from several instantiations of one generic trait, the call chooses among
     them as in [Method Resolution](09-traits.md#method-resolution).
   - If no trait method named `name` is available, the use is a
     `trait-not-in-scope` error, whose message names the trait and suggests a
     use declaration or the qualified form `Trait::name(x, ...)`.

   Presence is decided by name alone, whatever the method's arity or
   parameter types: a visible inherent method or a trait method with the
   wrong signature still stops the search, and the call is then checked
   against it. An inherent method that is not visible is skipped.
2. **Embedded fields.** Only when `S` has no visible inherent method and no
   trait method named `name`, lookup searches the data types reachable
   through embedded fields, breadth first. At each depth, a **match** is a
   visible inherent method named `name` of an embedded type at that depth.
   Fields are never matches. Trait methods are selected only on the
   receiver's own type `S`, never through an embedded field, but their names
   still stop the search. An embedded type **blocks** `name` when it has a
   trait method named `name` and no visible inherent method named `name`,
   whether or not the trait is available. The first depth with a match or a
   blocking type decides the lookup, so a shorter path always wins over a
   longer one:
   - Exactly one match and no blocking type selects the match.
   - Two or more matches, including one method reached through two
     different paths, are an `ambiguous-promoted-member` error. So is one
     match beside a blocking type at the same depth.
   - Blocking types and no match are an
     `embedded-trait-method-not-promoted` error. Its message names the trait
     and suggests the explicit path, as in `x.E1.name(args)`, where lookup
     starts at the embedded type and finds the trait method as an own
     method.
3. **No visible method.** If neither step selects a method, the use is a
   `private-member` error when lookup skipped an inherent method named
   `name`, and an `unknown-method` error otherwise. When the receiver has a
   field named `name`, the message should suggest `(x.name)(args)`.

For example, if `Page` embeds `Label`, `Label` implements `Display` and
embeds `Base`, and `Base` has an inherent `to_string`, then
`page.to_string()` stops at depth 1, where `Label` blocks `to_string`, and is
an `embedded-trait-method-not-promoted` error; it never reaches `Base`'s
`to_string` at depth 2. `page.Label.to_string()` calls `Label`'s `Display`
method, and `page.Label.Base.to_string()` calls `Base`'s. A name that an
embedded type has only through a trait therefore never silently resolves to
a method deeper in the tree.

A member selected in step 2 of either lookup is a **promoted member**.
`x.name` then means the explicit path `x.E1.E2...Ek.name` through the
embedded fields `E1` to `Ek`, with the same type, permission, and evaluation.
Each embedded step follows its container's access
([Mutable Paths](04-type-system.md#mutable-paths)), so a promoted member has
the access the receiver grants. Through a `mut S` receiver, a promoted field
may be assigned and a promoted `mut self` method may be called. Through a
readonly `S`, a promoted field is readonly, and a promoted `mut self` method
is found and then rejected with `mutable-receiver-required`; lookup never
skips it to try another member. Explicit qualification through an embedded
field, as in `x.E1.name`, starts a new lookup at `E1`'s type and resolves
every promotion ambiguity. `Trait::name(x, ...)` selects a trait method
without member lookup.

Two consequences are intended. Shortest path wins even across packages, so
when an embedded type in a dependency gains a visible member at a shallower
depth, a use may silently select it; Rust accepts the same kind of switch
when a trait import changes which `Deref` step answers a method call. A
trait method counts on `S` wherever its implementation is declared, so an
implementation for `S` added in another package gives `S` a depth-0 method
that stops the search before any embedded member.

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
