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
`and` or `or`, an unselected branch or match arm, or a loop body is not thereby
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

Member access first considers members declared directly by the receiver type,
then members promoted from embedded data types. A directly declared member hides
promoted members of the same name. A promoted member is usable only when there
is exactly one shortest embedding path to it. Multiple equally short paths are
ambiguous and require explicit qualification through an embedded field.

The same promotion rule applies to fields and methods. Embedding never grants
trait conformance. Inside an explicit trait `impl`, an unambiguous promoted
method may supply a required method; an ambiguous one requires an explicit
method body and qualified embedded-field call.

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
