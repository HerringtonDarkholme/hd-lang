# Traits

Status: language specification draft.

Traits describe behavior shared by otherwise unrelated types. Conformance is
explicit; hd-lang does not use structural method matching or class inheritance.

## Trait Declarations

A trait declares required methods:

```text
trait Display:
    fn display(self) -> string
```

A method with a body is a default implementation:

```text
trait Named:
    fn name(self) -> string

    fn label(self) -> string:
        "name: " + self.name()
```

Trait member names must be unique within the trait. Every parameter and result
type is explicit. A function whose first parameter is `self` or `mut self` is a
method; a mutable receiver is a requirement callers must satisfy. A function
without a receiver is an associated function and is called with qualified
`Trait::function(...)` or `Type::function(...)` syntax.

Traits may be generic:

```text
trait Add[T]:
    fn add(self, other: T) -> T
```

A trait with no methods may be declared as a marker without a body, and its
implementation likewise has no body:

```text
trait Serializable

impl Serializable for User
```

A trait may require another trait using a supertrait bound:

```text
trait Formattable: Display:
    fn format(self) -> string
```

An implementation of `Formattable` must also satisfy `Display`.
The supertrait graph must be acyclic; a direct or indirect cycle is a
compile-time error.

Traits may declare associated types, and implementations bind them:

```text
trait Supplier:
    type Item
    fn get(mut self) -> Self::Item?

impl Supplier for NameSupplier:
    type Item = string

    fn get(mut self) -> string?:
        ...
```

`Self::Item` projects from the current implementation. `T::Item` projects from
a generic type whose bounds select exactly one associated type declaration.
Ambiguous projections are compile-time errors.

## Trait Implementations

An explicit implementation names the trait and target type:

```text
impl Display for User:
    fn display(self) -> string:
        self.email
```

The implementation must provide every required method not supplied by a
default. It may override a default with the exact instantiated signature.
Method signatures must match the instantiated trait signatures.
Additional methods do not become part of that trait implementation; place them
in an inherent `impl` instead.

At most one implementation of the same instantiated trait for the same target
type may exist in a resolved program.

An `impl` inside an executable block suite is a compile-time declaration. A
local trait implementation must involve a local trait or a local nominal target
type visible at its declaration point. A local inherent implementation must
target a local nominal type. Implementations for a pair of nonlocal types belong
at module scope. Local implementations obey the same signature, orphan,
overlap, and uniqueness checks as module-level implementations; lexical scope
does not permit a second implementation for an existing pair. Local methods
and local-trait default methods cannot capture enclosing runtime values.
Their methods are available for lookup from the local `impl` declaration point
through its enclosing suite and child scopes, not before or outside that scope.

An ordinary trait implementation may be declared only in a package that owns
either the trait declaration or the target nominal type's declaration. For a
generic target, ownership is determined by its outer nominal type constructor.
Transparent aliases do not create ownership; nominal newtypes do. The standard
library owns primitives and built-in collection type constructors.

An inherent implementation may be declared only in the package that owns its
target nominal type. It cannot target a trait value, primitive, transparent
alias, or type owned by another package.

These orphan rules prevent downstream packages from creating globally
surprising conformance. The compiler must also reject a resolved dependency
graph containing duplicate exact implementations, including the possible
conflict where the trait-owning and type-owning packages each provide the same
pair.

The annotation chapter defines one explicit coherence exception for
package-local `annotate Facet for ImportedTarget` blocks when no authoritative
library annotation exists. That exception does not apply to ordinary `impl`.

Implementations may be generic and may state additional bounds inline or in a
`where` clause:

```text
impl[T: Display] Printable for Box[T]:
    fn print(self) -> string:
        self.value.display()

impl[T] Iterable[T] for mut T where T: Iterator[T]:
    fn iter(self) -> mut Iterator[T]: self
```

All generic implementation parameters must be constrained by the implemented
trait, target type, or a bound reachable from them. Two implementations overlap
when their trait and target heads can unify under any satisfying substitutions;
potential overlap is rejected. `where` predicates are not used to claim that
otherwise unifying implementations are disjoint.

## Inherent Implementations

An inherent implementation attaches methods to one nominal type without a
trait:

```text
impl User:
    fn domain(self) -> string:
        self.email.split("@")[1]
```

Inherent methods and associated functions are module-private unless
individually marked `pub`, including when their nominal type is public.
Methods in trait declarations and trait implementations follow the trait's
visibility; `pub` is not written on an individual trait method or its
implementation. A trait has one visibility level for all its methods; it
cannot mix public and private methods.

An inherent member name must not duplicate another inherent member on the same
type. hd-lang has no method or associated-function overloading.

Receiverless inherent functions are called through the nominal type:

```text
impl User:
    fn guest() -> User:
        User { name: "guest" }

guest := User::guest()
```

## Method Resolution

For `value.method(args)`, the compiler considers:

1. an inherent method on the receiver's nominal type;
2. an unambiguous method promoted from an embedded field;
3. methods from explicitly implemented traits available to type checking.

Only members visible from the calling module participate in method lookup.

For a concrete receiver, a trait is available to dot-call lookup when its name
is declared in or imported into the current module, visible in the current
lexical scope, or supplied by the prelude.
For a generic receiver, its declared bounds are also available. A dynamic trait
value always exposes the methods of its own erased trait. An implementation in
the dependency graph does not inject its trait's method names into every module
that can name the target type.

An inherent method takes precedence over a promoted method. Multiple remaining
candidates with no unique resolution are a compile-time ambiguity; the compiler
does not select by conversion ranking or declaration order.

Explicit qualification through an embedded field resolves promotion conflicts:

```text
record.CreatedByUser.display()
```

Select one trait explicitly with `Trait::method(receiver, arguments...)`:

```text
label := Display::display(value)
sum := Add[Money]::add(left, right)
```

The receiver is the first ordinary argument and must implement the named trait
instantiation. Remaining arguments follow normal positional/named ordering.
This form bypasses inherent and promoted-method lookup and selects exactly the
named trait method.

## Generic Bounds And Static Dispatch

A generic bound requires explicit conformance and uses static dispatch:

```text
fn show[T: Display](value: T) -> string:
    value.display()
```

Bounds compose with `+`:

```text
fn audit[T: Display + Named](value: T) -> string:
    value.display() + " / " + value.name()
```

`T: mut Trait` additionally requires `T` to be a mutable-root type. `T: mut Any`
requires mutable-root access without a type-specific behavior requirement.

The compiler may monomorphize static calls or use another representation, but
the choice must preserve reflection behavior for reified parameters.

## Dynamic Trait Values

Using a trait name directly as a value type creates a Go-style dynamic trait
value:

```text
fn print_display(value: Display) -> void:
    println(value.display())
```

Such a value contains a concrete value plus dispatch metadata for the trait.
There is no `dyn` marker. Only methods declared by the trait are available
through the erased value.

The trait must be dynamically safe: neither it nor a supertrait may declare an
associated type, associated function, or method-level generic parameter, and
`Self` may occur only as a method receiver.
A trait that is not dynamically safe can still be implemented and used as a
static generic bound. Generic parameters of the trait itself are allowed when
the value type names one complete instantiation.

A child-trait bound or dynamic value exposes the methods of its transitive
supertraits. A dynamic child-trait value widens implicitly to a supertrait
value, losing access to child-only methods; there is no reverse downcast.

Converting a concrete value to a trait value requires an explicit
implementation. The concrete type can be composite or primitive. Mutable
dynamic access uses `mut Trait` and cannot be recovered from a const `Trait`
value.

Dynamic trait-value type tests and downcasts are not supported.

## `Any`

`Any` is the universal empty trait. Every non-optional value type implements it
automatically. As a value type, `Any` erases the concrete type and exposes no
type-specific methods.

`Any` excludes `nil`; `Any?` permits absence through ordinary optional typing.
`mut Any` preserves mutable root access to an erased composite value.

## Embedding And Trait Satisfaction

An unambiguous method promoted from an embedded struct may satisfy a trait
requirement for the outer type. If multiple embedded structs promote conflicting
methods, the outer type does not satisfy the trait automatically.

Diagnostics for this failure should identify the required signature, list the
ambiguous promoted methods, and suggest either an explicit implementation or a
qualified embedded-field call.

Embedding is still composition, not subtype inheritance. An outer struct is not
assignable to an embedded type merely because it promotes that type's methods.

## Default-Method Conflicts

If a type implements two traits that provide default methods with the same
name, ordinary method-call resolution may be ambiguous even though both trait
implementations are individually valid. The type may define an inherent method
to provide its ordinary dot-call behavior, or the caller may use
`Trait::method(value, ...)` to select one implementation.

## Unsupported Trait Extensions

The language has no specialization, negative implementations, implicit
structural conformance, or trait-value downcasting. Dynamic trait-value
representation is an ABI detail and must preserve the dispatch semantics in
this chapter.
