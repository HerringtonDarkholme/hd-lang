# Traits

Status: language specification draft.

Traits describe behavior shared by otherwise unrelated types. Conformance is
explicit; hd-lang does not use structural method matching or class inheritance.

## Trait Declarations

A trait declares required methods:

```text
trait Describe:
    fn describe(self) -> string
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

A bodyless implementation is also permitted when every method of the trait
has a default. A method promoted from an embedded field never fills a trait
method, so it never makes a body optional; see
[Embedding And Trait Satisfaction](#embedding-and-trait-satisfaction).

A trait may require another trait using a supertrait bound:

```text
trait Formattable < Display:
    fn format(self) -> string
```

An implementation of `Formattable` must also satisfy `Display`. An
`impl Child for X` for which `X` has no implementation of a supertrait of
`Child` is a `missing-supertrait-implementation` error.
The supertrait graph must be acyclic; a direct or indirect cycle is a
`supertrait-cycle` compile-time error. An indirect cycle is reported once, on
the member of the cycle that appears first: first by module identity, then
by source position within the module.

Member names must be unique within a trait; a repeated associated type, method,
or associated function name is a `duplicate-trait-member` error.

A child trait must not declare a member whose name is also the name of a
member of any of its transitive supertraits. The check is made at the child
trait's declaration, whether or not any type implements it, and applies to
associated types, methods, and associated functions alike. Such a member is a
`duplicate-trait-member` error, reported on the child's member. For
example, when `Greeter` declares `fn greet(self) -> string`,
`trait Loud < Greeter` must not declare `greet`, with or without a body.
A child trait therefore cannot provide a default body for a supertrait's
method. A supertrait's defaults come only from the supertrait.

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
Ambiguous projections are compile-time errors. A bound may also fix a
projection to a type; see
[Associated Type Bindings](#associated-type-bindings).

## Trait Implementations

### Comparison Traits

The standard library defines `PartialEq`, `Eq`, `PartialOrd`, `Ord`, and
`Ordering` in `std.cmp`. `PartialEq` requires
`fn eq(self, other: Self) -> bool`; `Eq < PartialEq` is a marker asserting
reflexive equality. `PartialOrd < PartialEq` requires
`fn partial_cmp(self, other: Self) -> Ordering?`, where `.None` means unordered.
`Ord < Eq + PartialOrd` requires
`fn cmp(self, other: Self) -> Ordering`. `Ordering` has `Less`, `Equal`, and
`Greater` cases. `Eq` and `Ord` implementations must agree with their partial
counterparts. These semantic laws are obligations of the implementer; ordinary
trait checking cannot prove them. In particular, floating-point values do not
satisfy `Eq` or `Ord` because of NaN.

The standard library also defines `Hash` and `Hasher` in `std.hash`:

```text
trait Hasher:
    fn write(mut self, bytes: List[u8]) -> void

trait Hash:
    fn hash(self, state: mut Hasher) -> void
```

A map key must
implement both `Eq` and `Hash`; neither trait is inferred for user-defined
data or enums. The compiler does not verify any relationship between their
implementations. A readonly key can still change through another mutable
alias, potentially leaving its map entry unreachable.

There is no automatic conformance for user-defined data or enum types. An
explicit implementation may choose domain-specific equality or ordering.
`@derive(PartialEq, Eq)` is an explicit compiler intrinsic on a data or enum
declaration. Its arguments name traits, not annotator values. The compiler
generates ordinary implementations of the named traits from the declaration's
shape, checks trait requirements and coherence, and rejects traits for which it
has no derivation rule. It does not generate `Annotate[A]` conformance or run a
`DataAnnotator`. For each derived trait, the generated implementation adds a
`T < Trait` bound for every declaration type parameter `T` that occurs in a
field compared, ordered, or hashed by that derivation. Thus
`@derive(PartialEq) data Box[T]` produces conformance only when `T < PartialEq`.
Derived `PartialEq` compares every declared data field,
including embedded fields, by its `PartialEq` implementation. No field is
implicitly excluded. Derived enum equality first compares the variant, then
every payload field of that variant, including common enum fields; different
variants are unequal. Derived `Eq` requires every compared field to satisfy
`Eq`. Derived equality does not detect cycles or track previously compared
objects: it recursively invokes each field's `PartialEq` implementation. A
comparison that repeatedly traverses a cycle may exhaust the execution stack.
`@derive(PartialOrd, Ord)` also supports data and enums. Derived ordering is
lexicographic in declared data-field order, including embedded fields. For
enums, distinct variants compare by variant declaration order; values of the
same variant compare shared enum data in declaration order, followed by that
variant's payload parameters in declaration order. Constructor argument order
does not affect comparison. Derived `PartialOrd` requires every compared field
to satisfy `PartialOrd` and returns `.None` if a field comparison is unordered
before a comparison result is determined. Derived `Ord` requires every compared
field to satisfy `Ord`.
`@derive(Hash)` supports data and enums. It generates an ordinary `Hash`
implementation that hashes every declared data field in declaration order,
including embedded fields. For an enum, it hashes the variant identity, then
shared enum data in declaration order, then that variant's payload fields in
declaration order. Every hashed field must implement `Hash`; no field is
implicitly excluded. Like derived equality, derived hashing does not detect
cycles, so hashing a cyclic graph may exhaust the execution stack. Hash values
are not guaranteed to be stable across processes or runtime versions.
The target must also satisfy each derived trait's supertraits, whether through
an existing implementation or another derivation. Implementers remain
responsible for consistency with any manually implemented comparison traits.
Comparison traits are the only traits invoked by operator syntax.

### Conversion Trait

The standard library declares the general conversion trait `From` in
`std.convert`:

```text
trait From[T]:
    fn from(value: T) -> Self
```

An implementation `impl From[T] for X` converts a `T` into an `X`. `From` is
not a prelude name; code that names it imports it, as in
`use std.convert.From`. Postfix `?` uses the trait without an import: when an
error is not assignable to the enclosing function's error type, `?` calls
that error type's `From` implementation once
([Propagation](05-expressions.md#propagation)). Any code may also call a
conversion directly as `X::from(value)`.

A conversion is pure. The trait method `from` has the empty requirement row
and is not suspending, and an implementation method must agree with it on
both ([Requirement Rows](11-requirements-and-suspension.md#requirement-rows)).
An implementation whose `from` declares a requirement clause, as in
`fn from(value: FsError) -> SyncError $ Console`, or is suspending, as in
`fn from!(value: FsError) -> SyncError`, is a `trait-method-signature` error.
Its body may still panic.

`From` implementations follow the ordinary rules for implementation targets,
ownership, overlap, and uniqueness. `impl From[FsError] for SyncError` may be
declared by the package that owns `SyncError`, the package that owns
`FsError`, or the standard library. Implementations for different source
types never overlap, because their trait arguments differ, so one error type
may implement both `From[FsError]` and `From[HttpError]`. A reflexive
`impl[T] From[T] for T` is a `bare-parameter-impl-target` error, and a trait
value type is never a target, so `impl From[FsError] for Error` is a
`trait-value-impl-target` error.

When `X` implements `From` at several instantiations, `X::from(value)`
chooses among them by the rule for instantiations of one generic trait in
[Method Resolution](#method-resolution). Each instantiation is a candidate,
and the one whose parameter the argument fits is selected; when none fits,
the call is a `type-mismatch` error.

### Error Trait

The standard library declares the standard error trait `Error` in
`std.error`. `Error` is dynamically safe and has `Display` as a supertrait.
Every member it declares has a default, so an implementation needs no body:
`impl Error for FsError` is complete when `FsError` implements `Display`.
Those members, and error-chain helpers built on them, are standard-library
API. `Error` is not a prelude name; code imports it with
`use std.error.Error`.

The dynamic trait value `Error` is the erased application error. A
`Result[T, Error]` holds any error that implements `Error`, and `?` reaches
it by assignability, constructing the dynamic value
([Propagation](05-expressions.md#propagation)). Because a dynamic trait value
satisfies bounds on its own trait and its supertraits
([Dynamic Trait Values](#dynamic-trait-values)), `Error` satisfies an entry
point's `E < Display` requirement, so `pub fn main() -> Result[void, Error]`
is a valid entry point. Like every dynamic trait value, an erased `Error` is
not boundary-safe and never crosses a registered boundary
([Wasm Boundary](10-modules.md#wasm-boundary)); code converts it explicitly
to a boundary-safe error type first.

```text
use std.error.Error

enum FsError:
    NotFound(path: string)

impl Display for FsError:
    fn to_string(self) -> string:
        match self:
            FsError.NotFound(path) => "not found: " + path

impl Error for FsError

fn read_config(path: string) -> Result[string, FsError]:
    Err(FsError.NotFound(path))

fn load(path: string) -> Result[string, Error]:
    text := read_config(path)?
    Ok(text.trim())
```

### Implementation Declarations

An explicit implementation names the trait and target type:

```text
impl Display for User:
    fn to_string(self) -> string:
        self.email
```

The implementation must write every required method not supplied by a
default or it is a `missing-trait-method` error. Only a method written in the
implementation or a trait default fills a trait method; an inherent method of
the target and a method promoted from an embedded field never do. The
implementation may override a default with the exact instantiated signature. A mismatched method is a
`trait-method-signature` error.

The exact signature includes the method-level generic parameters. An
implementation method declares as many generic parameters as the trait
method, and they correspond by position; names may differ. Each parameter
keeps the trait method's `reified` and pack markers and the same bounds: the
same traits, with `mut` and the same instantiated arguments and associated
type bindings, written in the same order. An implementation method therefore
cannot add, drop, reorder, weaken, or strengthen a bound. Any mismatch is a
`trait-method-signature` error reported at the implementation method.

Additional methods do not become part of that trait implementation; place them
in an inherent `impl` instead.

At most one implementation of the same instantiated trait for the same target
type may exist in a resolved program.

An `impl` inside an executable block suite is a compile-time declaration. A
local trait implementation must involve a local trait or a local nominal target
type visible at its declaration point. A local inherent implementation must
target a local nominal type. Implementations for a pair of nonlocal types belong
at module scope. Local implementations obey the same target, ownership,
overlap, and uniqueness checks as module-level implementations; lexical scope
does not permit a second implementation for an existing pair. Local methods
and local-trait default methods cannot capture enclosing runtime values.
Their methods are available for lookup from the local `impl` declaration point
through its enclosing suite and child scopes, not before or outside that scope.

### Implementation Targets

The target of every implementation, trait or inherent, starts with a type
constructor: a data, enum, or newtype declaration; a built-in type
constructor such as `i32`, `string`, `List`, or `Map`; or a tuple
constructor. Tuples have one built-in constructor per arity, so `(A, B)` is
the two-element tuple constructor applied to `A` and `B`, and
`impl Display for (i32, string)` is a valid target. Tuples of different
arity never share a constructor. An optional target is the prelude enum
`Option` applied to its contained type: `annotate Validation for string?`
targets `Option[string]`, and by [Overlap](#overlap) it does not overlap an
implementation for `i32?`. The constructor's arguments may be any
types, including implementation parameters, as in
`impl[T < Display] Printable for Box[T]`. A target that is a bare type
parameter, as in `impl[T] Describe for T`, is a `bare-parameter-impl-target`
error. hd-lang has no blanket implementations over every type; a later
revision may add them as a compatible extension.

A function type is never an implementation target, whatever its parameter,
result, or requirement types. `impl Marker for fn(i32) -> i32` is a
`function-impl-target` error.

A trait value type is never an implementation target either: `Display` used
as a type names a dynamic trait value, not a type constructor.
`impl Marker for Display` and `impl Marker for Any` are
`trait-value-impl-target` errors. A trait value type may still be a
constructor's argument, as in `impl Marker for List[Display]`.

A target must not be written with an outer `mut`. `impl Marker for mut Counter`
is a `mutable-impl-target` error. Permission belongs to method receivers
(`self` and `mut self`) and to bounds (`T < mut Trait`), not to
implementations. One implementation for `X` serves both the readonly view `X`
and the mutable view `mut X`: lookup through either view considers the same
implementations, and a `mut self` method still requires mutable access at each
call.

### Implementation Ownership

An `impl Trait[Args] for Target` may be declared only in a package that owns
one of these declarations:

1. the trait;
2. the target's outer type constructor;
3. the outer type constructor of one of the trait arguments `Args`.

Any other trait implementation is an `orphan-impl` error. For example, the
package that declares `Money` may write `impl Add[Money] for i32`, because it
owns the trait argument `Money`. The third case never applies to a target that
is a bare type parameter. Transparent aliases do not create ownership; nominal
newtypes do. The standard library owns primitives, built-in collection type
constructors, tuple constructors, and the prelude enum `Option`, so an
implementation for `string?` needs the package of the trait or of a trait
argument, as in `annotate Validation for string?` in the package that owns
`Validation`.

An inherent implementation may be declared only in the package that owns its
target nominal type. It cannot target a trait value, primitive, tuple,
transparent alias, or type owned by another package.

These ownership rules prevent downstream packages from creating globally
surprising conformance. The compiler must also reject a resolved dependency
graph containing duplicate exact implementations, including the possible
conflict where two owning packages each provide the same pair.

`annotate Facet for Target` lowers to `impl Annotate[Facet] for Target` and
follows the same rule: the package owning the facet type, which is the trait
argument, or the target's type constructor may declare it. The annotation chapter
defines one further exception, for a root application's orphan annotation
when no library annotation exists; see
[Coherence And Package Rules](14-annotations.md#coherence-and-package-rules).
That exception does not apply to ordinary `impl`.

### Overlap

Implementations may be generic and state their bounds inline in the generic
parameter list:

```text
impl[T < Display] Printable for Box[T]:
    fn print(self) -> string:
        self.value.to_string()
```

All generic implementation parameters must be constrained by the implemented
trait, target type, or a bound reachable from them.

Two implementations overlap when they implement the same trait and their
full heads unify: after each implementation's parameters are renamed apart,
one substitution makes both their trait arguments and their complete target
types equal. Overlap is decided from the implementation heads alone. Bounds,
including associated type bindings, are never used to claim that two
implementations are disjoint. Thus `impl[T] Marker for List[T]` overlaps
`impl Marker for List[i32]`, and `impl[T] Marker for Box[T]` overlaps
`impl Marker for Box[i32]`. `impl Marker for Box[i32]` and
`impl Marker for Box[string]` do not overlap, nor do
`impl Add[i32] for Money` and `impl Add[Money] for Money`.
Overlapping implementations are an `overlapping-impl` error. Because bounds
are ignored, an implementation added later in a dependency cannot make two
existing implementations overlap.

## Inherent Implementations

An inherent implementation, written `impl T:` without a trait, declares
members attached directly to the nominal type `T` rather than through a trait.
Its members are the type's inherent members:

- an **inherent method** has `self` or `mut self` as its first parameter and
  is called with dot syntax, as in `user.domain()`;
- an **inherent associated function** has no receiver and is called through
  the type, as in `User::guest()`.

Only the package that owns `T` may declare them; see
[Implementation Ownership](#implementation-ownership). An inherent method
differs from a trait method, which a trait declares and a trait
implementation supplies for `T`, and from a promoted method, which belongs to
the type of an embedded field and is reached through the outer type; see
[Member Resolution](03-names-and-scopes.md#member-resolution).

```text
impl User:
    fn domain(self) -> string:
        self.email.split("@")[1]
```

Inherent methods and inherent associated functions are module-private unless
individually marked `pub`, including when their nominal type is public.
Methods in trait declarations and trait implementations follow the trait's
visibility; `pub` is not written on an individual trait method or its
implementation. A trait has one visibility level for all its methods; it
cannot mix public and private methods.

An inherent member name must not duplicate another inherent member on the same
type; a duplicate is a `duplicate-inherent-member` error. An inherent member
may share its name with a field of the type, named or embedded, because fields
and methods are separate namespaces
([Member Resolution](03-names-and-scopes.md#member-resolution)). hd-lang has no
method or associated-function overloading.

Inherent associated functions are called through the nominal type:

```text
impl User:
    fn guest() -> User:
        User { name: "guest" }

guest := User::guest()
```

## Method Resolution

For a receiver of nominal type `S` or `mut S`, `value.method(args)` selects a
method with the method lookup in
[Member Resolution](03-names-and-scopes.md#member-resolution), which covers
inherent methods, trait methods, and promoted methods together; it never
selects a field. This
section defines which trait methods are usable at a use and how trait
candidates are reported.

For a concrete receiver, a trait is available to dot-call lookup when its name
is declared in or introduced by a use declaration in the current module,
visible in the current lexical scope, or supplied by the prelude.
For a generic receiver, its declared bounds are also available. A dynamic trait
value always exposes the methods of its own erased trait. An implementation in
the dependency graph does not inject its trait's method names into every module
that can name the target type. A method of a trait that is not available is
not a candidate at all, wherever its implementation is declared: lookup
proceeds as if the implementation were absent, so a promoted method of the
same name may be selected, and a call that finds no method should suggest a
use declaration for the trait.

A visible inherent method of the receiver's nominal type is always selected
over trait methods, including a method of a trait that the same type
implements. Only the package that owns the type can declare an inherent
method, so no other package can change which method such a call reaches.

When more than one available trait that the receiver implements supplies a
method with that name, and no inherent method of that name is usable, the call
is an `ambiguous-method` error. So is a call where an available trait method
of the receiver's type meets a promoted method of the same name
([Member Resolution](03-names-and-scopes.md#member-resolution)): neither
silently wins over the other. This holds whether each method is
written in its implementation or comes from a default. The compiler does not
select by conversion ranking or declaration order. A trait-qualified call
resolves the ambiguity.

When the receiver implements one generic trait at several instantiations that
each supply the method, as with `impl Add[i32] for Money` and
`impl Add[Money] for Money`, the call chooses the instantiation. Each
instantiation is a candidate. A candidate **fits** when the call's arguments
check against its method's parameter types, with that instantiation's trait
arguments substituted, and, when the call has an expected type, the method's
result type is assignable to it. Exactly one fitting candidate is selected, so
`price.add(5)` calls the `Add[i32]` method. When two or more candidates fit,
and exactly one of them fits with every integer literal argument at `i32`
and every floating-point literal argument at `f64`, the literals' default
types, that candidate is selected: with `impl Add[i32] for Money` and
`impl Add[i64] for Money`, `price.add(5)` calls the `Add[i32]` method.
Otherwise two or more fitting candidates are an `ambiguous-method` error, and
a trait-qualified call such as `Add[i64]::add(price, 5)` resolves it. When no
candidate fits, the call is a `type-mismatch` error whose message lists the
available instantiations. This choice applies only among instantiations of
one trait; methods of two different traits stay `ambiguous-method` whatever
the argument types.

Select one trait explicitly with `Trait::method(receiver, arguments...)`:

```text
label := Display::to_string(value)
sum := Add[Money]::add(left, right)
```

The receiver is the first ordinary argument and must implement the named trait
instantiation. Remaining arguments follow normal positional/named ordering.
This form bypasses member lookup and selects exactly the named trait
method. A generic trait method takes its explicit type arguments
after the method name, as in `Identity::select[i32](picker, 42)`; the trait's
own type arguments stay before `::`. The list follows the rules of
[Generic Functions](07-functions.md#generic-functions).

## Generic Bounds And Static Dispatch

A generic bound requires explicit conformance and uses static dispatch:

```text
fn show[T < Display](value: T) -> string:
    value.to_string()
```

Bounds compose with `+`:

```text
fn audit[T < Display + Named](value: T) -> string:
    value.to_string() + " / " + value.name()
```

A bound may list the same trait more than once, as in `T < Display + Display`.
The repetition adds no requirement and is not diagnosed.

`T < mut Trait` additionally requires `T` to be a mutable-root type. `T < mut Any`
requires mutable-root access without a type-specific behavior requirement.

A type argument, explicit or inferred, that does not implement a trait its
parameter's bound requires is an `unsatisfied-trait-bound` error. This includes
a non-reference type for `T < Reference`, and a readonly argument for
`T < mut Trait`.

The compiler may monomorphize static calls, share one body among
instantiations, or use another representation, as long as the choice preserves
the observable semantics, including reflection behavior for reified
parameters. See the non-normative
[Implementation Model](04-type-system.md#implementation-model-non-normative).

### Associated Type Bindings

A trait in a generic parameter bound may bind associated types after its
positional type arguments:

```text
trait Supplier:
    type Item
    fn get(self) -> Self::Item

fn describe[T < Display, I < Supplier[Item = T]](source: I) -> string:
    source.get().to_string()

data Feed[I]:
    source: I

impl[T < Display, I < Supplier[Item = T]] Display for Feed[I]:
    fn to_string(self) -> string:
        describe(self.source)
```

`I < Supplier[Item = T]` means that `I` implements `Supplier` and that its
associated type `Item` equals `T`. The binding is an equality constraint on
the projection `I::Item`, not a new type:

- Inside the declaration, `I::Item` remains a valid projection and denotes the
  same type as `T`. The two spellings are interchangeable in parameter, result,
  and body types.
- At a use site, after substitution, the argument's implementation of the
  trait must bind the associated type to the bound type. The constraint takes
  part in generic argument inference, so `T` above is inferred from the
  `Supplier` implementation of the argument passed for `I`. An argument whose
  implementation binds a different type is an `unsatisfied-trait-bound` error.
- A binding counts as a bound reachable from its parameter. In the
  implementation above, `T` is therefore constrained through `I`, and the
  implementation satisfies the rule that every generic implementation
  parameter be constrained.

The bound type may name any parameter of the same generic parameter list. A
binding name must be an associated type declared by the named trait itself;
naming anything else, including an associated type that only a supertrait
declares, is an `unknown-associated-type` error. Bind a supertrait's
associated type with a separate bound on that supertrait. Each projection may
be bound at most once in one generic parameter list: a second binding of the
same parameter's associated type, in the same bound or another bound, is a
`duplicate-associated-binding` error even when both bindings name the same
type.

Bindings appear only in generic parameter bounds. A supertrait list, the
trait of an `impl` header, a trait-qualified call, a type argument, and a
dynamic trait value type do not accept them; the grammar reports a
`syntax-error` there. A binding does not make an ambiguous projection
unambiguous: when two bounds on `I` both declare `Item`, `I::Item` is still
ambiguous even if one of them binds it.

## Dynamic Trait Values

Using a trait name directly as a value type creates a Go-style dynamic trait
value:

```text
fn print_display(value: Display) -> void $ Console:
    println(value.to_string())
```

Such a value contains a concrete value plus dispatch metadata for the trait.
There is no `dyn` marker. Only methods declared by the trait are available
through the erased value.

The trait must be dynamically safe: neither it nor a supertrait may declare an
associated type or associated function, every method-level generic parameter
must be bounded by `Reference`, and `Self` may occur only as a method
receiver. Further bounds on such a parameter are allowed; see
[Trait Values And `Any`](04-type-system.md#trait-values-and-any).
A trait that is not dynamically safe can still be implemented and used as a
static generic bound. Generic parameters of the trait itself are allowed when
the value type names one complete instantiation.

A child-trait bound or dynamic value exposes the methods of its transitive
supertraits. A dynamic child-trait value widens implicitly to a supertrait
value, losing access to child-only methods; there is no reverse downcast.

A dynamic trait value type satisfies a generic bound on its own trait and on
each direct or transitive supertrait of that trait. For a generic trait, the
bound must name the same instantiation, so `Repository[User]` satisfies
`T < Repository[User]`. A statically dispatched call through such a bound
dispatches each method through the value's table. Dynamic safety guarantees
the trait has no associated function or associated type that a bound could
need. A readonly trait value never satisfies a `mut` bound; `mut Tr`
satisfies `T < mut Tr`. The rule adds no implementation: the trait value type
satisfies no other bound through it, and it still cannot be an
implementation target.

```text
trait Named < Display:
    fn name(self) -> string

fn show[T < Display](value: T) -> string:
    value.to_string()

fn tag[T < Named](value: T) -> string:
    value.name()

fn describe(named: Named, shown: Display) -> string:
    tag(named) + show(named) + show(shown)
```

Converting a concrete value to a trait value requires an explicit
implementation. The concrete type can be composite or primitive. Mutable
dynamic access uses `mut Trait` and cannot be recovered from a readonly `Trait`
value.

Dynamic trait-value type tests and downcasts are not supported.

## `Any`

`Any` is the universal empty trait. Every value type, including an optional
type, implements it automatically. As a value type, `Any` erases the concrete type and exposes no
type-specific methods.

An optional value erases to `Any` like any other enum value. A bare `.None`
still needs an expected optional type, so `let value: Any = .None` is a
`missing-contextual-enum-type` error while `let value: Any? = .None` is valid.
`mut Any` preserves mutable access to an erased composite value.

## Embedding And Trait Satisfaction

Embedding never grants trait conformance. The outer type must declare an
explicit `impl`, and that implementation must write every required method that
has no default. A method promoted from an embedded field never fills a trait
method, whether required or defaulted, and whatever its receiver. An
implementation that reuses an embedded type's behavior forwards to it
explicitly, for example `fn label(self) -> string: self.Base.label()`, or,
when the embedded type implements the trait, delegates the whole trait with
`impl Trait for C by E` ([Trait Delegation](#trait-delegation)). A
forwarding `mut self` method may call a `mut self` method of the embedded part,
as in `fn reset(mut self) -> void: self.Base.reset()`, because `self.Base` has
`mut` access through `mut self`
([Data Embedding](08-data-and-enums.md#data-embedding)). A bodyless
implementation of a trait with a required method is therefore a
`missing-trait-method` error even when an embedded type has a matching method.
Where the trait is available, a dot call with the forwarded name meets both
the implementation's method and the promoted method, and is
`ambiguous-method`; the forwarding implementation is called as
`Trait::method(x)`, as in `Describe::describe(service)`, and the embedded
type's method through its path, as in `service.Logger.describe()`.

An embedded type's trait methods are not promoted either
([Member Resolution](03-names-and-scopes.md#member-resolution)). If
`Label` implements `Display` and `Page` embeds `Label`, `page.to_string()`
never calls `Label`'s implementation, and `Label`'s method does not hide a
`to_string` promoted from a type that `Label` embeds. When `Page` has no
`to_string` of its own, no promoted one, and no available trait method with
that name, the call is an `unknown-method` error whose message should suggest
`page.Label.to_string()`. `Page` satisfies no `Display` bound unless `Page` itself implements
`Display`.

Embedding is composition, not subtype inheritance. An outer data type is not
assignable to an embedded type merely because it promotes that type's methods.

## Trait Delegation

A trait implementation may delegate the trait to an embedded field of its
target by naming the field after `by`:

```text
trait Describe:
    fn describe(self) -> string
    fn headline(self) -> string:
        "* " + self.describe()

data Logger:
    name: string

impl Describe for Logger:
    fn describe(self) -> string:
        self.name

data Service:
    Logger
    port: i32

impl Describe for Service by Logger

data Worker:
    Logger

impl Describe for Worker by Logger:
    fn headline(self) -> string:
        "worker " + self.Logger.headline()
```

In `impl Trait for C by E`, `C` must be a data type, `E` must name one of
its embedded fields, and the type of that field, with `C`'s type arguments
substituted, must implement the same instantiation of `Trait`. Otherwise the
implementation is an `invalid-delegation` error, reported on the line of
`by`. `E` names a direct embedded field; a deeper part is reached by
delegating to the field that contains it.

For every method of `Trait` with a receiver, including methods that have a
default body, the implementation has a generated method unless its body
writes one with that name. The generated method has the trait method's
signature and calls the part's implementation of the same method with the
same arguments, as `Trait::method(self.E, arguments...)` would; a variadic
parameter is passed on as a spread. A `mut self` method forwards through
`self.E`, which has `mut` access through `mut self`
([Data Embedding](08-data-and-enums.md#data-embedding)). A method written in
the body replaces the generated one and follows the rules of
[Implementation Declarations](#implementation-declarations), as does any
other member of the body. The part's implementation runs with the part as
its receiver, so a default body there calls the part's methods, never the
delegating implementation's; there is no overriding
([Member Resolution](03-names-and-scopes.md#member-resolution)).

Associated types take the part's bindings: each associated type of the
delegating implementation is bound to the type that the part's
implementation binds, and an associated type binding in the body is an
`invalid-delegation` error. Associated functions have no receiver to forward
through and are never generated. The body writes each of them unless the
trait gives it a default; a missing one is a `missing-trait-method` error.

Otherwise a delegating implementation is an ordinary trait implementation.
It obeys [Implementation Ownership](#implementation-ownership),
[Overlap](#overlap), and trait visibility, satisfies bounds, supports
dynamic trait values, and its methods are candidates in
[Method Resolution](#method-resolution) where the trait is available. A dot
call such as `service.describe()` therefore has one candidate unless an
embedded type also has an inherent method of that name, in which case it is
`ambiguous-method` as for any implementation.

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
