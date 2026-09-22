# Names and Scopes

Status: core specification draft.

This chapter defines the scopes introduced by hd-lang programs and how names
resolve. It does not define type compatibility or access permission.

## Name Categories

hd-lang has three lookup categories:

1. **Module names** identify top-level types, traits, functions, and imported
   declarations. A module cannot contain two declarations with the same module
   name, even if they are different kinds of declaration. Function overloading
   is therefore not permitted.
2. **Local value names** identify parameters, local bindings, loop bindings,
   pattern bindings, and captured values.
3. **Member names** identify struct fields, embedded fields, methods, enum
   variants, and tuple fields within the namespace of their owning type.

A use is resolved in the category required by its syntax. For example, the
name before `{` in `User { ... }` is resolved as a type, while `user` in
`user.email` is resolved as a value and `email` as a member of its type.

Every module has predeclared core type names: `bool`, `i8`, `i16`, `i32`,
`i64`, `u8`, `u16`, `u32`, `u64`, `f32`, `f64`, `char`, `string`, `void`,
`list`, `map`, `Result`, and `Any`. A module declaration or import must not
replace one of these names. They remain ordinary identifier tokens rather than
reserved words. A local value may use the same spelling under ordinary lexical
shadowing; type positions continue to resolve the predeclared type name, while
value expressions resolve the local value.

Type parameters are type names local to their declaration. They may shadow a
module name within that declaration but must not duplicate another type
parameter in the same parameter list.

`Self` is a contextual type name inside a trait or `impl` body. In a trait it
denotes the eventual implementing type; in a trait or inherent implementation
it denotes that implementation's target type. It is not a reserved word and
has no special meaning outside those bodies.

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
statements, but are not importable declarations and are not visible before
their binding point. A top-level executable statement may refer to a named
module declaration regardless of that declaration's textual position.

Named `fn`, `struct`, `enum`, `trait`, and `type` declarations occur only at
module scope in the current core. `impl` declarations also occur only at
module scope, but do not introduce an independently referencable module name.

Declarations are module-private unless marked `pub`. `pub` makes a declaration
eligible to be imported from another module. It does not register a Wasm export
or make a declaration host-callable.

## Imports

An import introduces either one local module name or one or more local
declaration names into the importing module's module scope:

```text
import pkg.user.types
import pkg.user.types as user_types
import pkg.user.types.{User, UserId}
import dep.billing.types.{UserId as BillingUserId}
```

Without `as`, a namespace import binds the final path component. The first
example therefore binds `types`. A grouped import binds each selected name,
after applying any item alias.

An imported name must not collide with another module-scope declaration or
import. Imports do not create overload sets and are not implicitly renamed.

Imports are resolved before declarations are type checked. Their textual
position does not limit their visibility, but style tools should place imports
before other top-level items. Import and re-export cycles are compile-time
errors in v1.

`export` re-exports selected public declarations. It does not introduce an
additional local alias beyond names already available through ordinary module
resolution.

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

Within a method, `self` is an ordinary parameter name supplied by the receiver
syntax. `mut self` is shorthand for `self: mut Self`; it does not create a
different lookup category.

A closure resolves otherwise-unbound local names in lexically enclosing
scopes. Those resolved names are its captures. Whether a capture permits
mutation is determined by the closure's function type and the captured value's
access type, as specified in [Functions](07-functions.md).

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
are visible in that arm body only:

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
then members promoted from embedded structs. A directly declared member hides
promoted members of the same name. A promoted member is usable only when there
is exactly one shortest embedding path to it. Multiple equally short paths are
ambiguous and require explicit qualification through an embedded field.

The same promotion rule applies to fields and methods. During trait
satisfaction, an unambiguous promoted method can satisfy a required method;
ambiguous promoted methods cannot.

Enum variants are members of their enum and must be qualified in construction
and patterns:

```text
status := JobStatus.Queued

match status:
    JobStatus.Queued => "waiting"
```

Tuple members use numeric selectors such as `.0` and `.1`. Numeric selectors
are not ordinary identifiers and cannot be declared by users.

## Unsupported Scope Extensions

v1 permits shadowing of outer local names; a style tool may warn about it, but
that warning is not part of language semantics. v1 does not support nested
named declarations, direct imports of enum variants, or top-level stored-value
declarations distinct from sequential script bindings.
