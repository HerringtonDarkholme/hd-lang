# Annotations

Status: language specification draft.

## Terminology

- A **facet** is a type implementing `Annotation` and identifies one family of
  attached or derived information.
- A **facet value** is the concrete value retained as `self` while that facet
  runs; it may carry configuration.
- An **annotator** is a facet implementation of `TypeAnnotator`,
  `DataAnnotator`, `EnumAnnotator`, or `FuncAnnotator`.
- **Info** is the uniform output type selected by `Facet::Info`.
- A **coherence slot** is one `(facet type, concrete target)` pair over the
  resolved package graph; direct and structural implementations occupy the
  same slot.

Annotations attach typed metadata to declaration shapes and derive ordinary
runtime information from those shapes. They do not alter a declaration's name,
type, behavior, or visibility, and they do not discover or register runtime
objects automatically.

The design principles are:

1. facet derivation is structural;
2. overrides are local;
3. annotation and metadata values are typed;
4. runtime information is produced explicitly when requested;
5. annotation evaluation is strict bottom-up;
6. the semantic foundation is ordinary traits, implementations, values, and
   compiler-provided shape values.

Ordinary decorators expand to `annotate` blocks. The compiler lowers facet blocks to
`impl Annotate[Facet] for Target` and member blocks to shape metadata
construction. This is not runtime wrapper execution. `@derive(Trait, ...)` is
the exception: it generates ordinary trait implementations, either through a
compiler intrinsic or through a trait's template, as
[Typed Derivation](#typed-derivation) defines, and it does not invoke the
annotation protocol. An ordinary decorator must not silently change a
declaration's name, type, behavior, or visibility.

## Common Shape Representation

The compiler exposes declaration structure as ordinary runtime shape values
through two intrinsic functions in the prelude, `shape` and `shape_of`:

```text
shape[User]()                        # DataShape specialized for User
shape[User]().fields.email           # FieldShape
shape[JobStatus]()                   # EnumShape specialized for JobStatus
shape[JobStatus]().variants.Active   # VariantShape
shape_of(get_user)                   # FnShape
```

The common representation includes at least:

- `TypeShape` for a concrete type;
- `DataShape` and ordered `FieldShape` values;
- `EnumShape`, ordered `VariantShape` values, and payload `FieldShape` values;
- `FnShape` and ordered `ParamShape` values;
- names, positions, declared runtime type descriptors, documentation, and
  attached member metadata.

The shape types below are not parameterized by the reflected declaration: the
type is `DataShape`, not `DataShape[S]`, and annotators receive these generic
shape types. Only a direct `shape[T]()` request adds the typed member access
described in [Shape Intrinsics](#shape-intrinsics). Aggregate mapped results
are uniform collections such as `List[(string, DatabaseColumn)]`, where
`DatabaseColumn` is an example user-defined facet result rather than a prelude
type.

Shapes expose structure for generic handling but do not permit mutation of the
source declaration. Every shape provides a stable declaration identity, source
name, qualified name, source position, documentation string, and declaration
kind. `FieldShape`, `VariantShape`, and `ParamShape` additionally provide their
zero-based declaration position and declared `TypeShape`. Fields, variants,
and parameters expose metadata attached by `annotate Target`. `DataShape`,
`EnumShape`, and `FnShape` contain their ordered direct members. `FnShape`
additionally exposes its result type, whether it is suspending, and its
normalized unordered requirement row; each `ParamShape` records whether a
default is declared.
Promoted embedded members are not duplicated as direct fields.

The following declarations are the normative shape surface. `DeclarationId`
is an opaque, equality-comparable identity allocated by the compiler. A
`SourcePosition` identifies the beginning of the reflected declaration or
member. `position` on a member shape is its zero-based declaration ordinal;
source coordinates are kept separately in `source`.

```text
data SourcePosition:
    file: string
    line: i32
    column: i32

enum DeclarationKind:
    Data
    Enum
    Function
    Field
    Variant
    Parameter

enum PrimitiveKind:
    Bool
    Signed(bits: i32)
    Unsigned(bits: i32)
    Float(bits: i32)
    Char
    String
    Void
    Never

enum TypeShape:
    Primitive(kind: PrimitiveKind)
    Optional(inner: TypeShape)
    List(element: TypeShape)
    Map(key: TypeShape, value: TypeShape)
    Tuple(elements: List[TypeShape])
    Named(decl: DeclarationId, args: List[TypeShape])
    Newtype(decl: DeclarationId, base: TypeShape)
    Mut(inner: TypeShape)
    Trait(decl: DeclarationId, args: List[TypeShape])
    Any
    Suspend(result: TypeShape)
    Fn(
        params: List[TypeShape],
        result: TypeShape,
        suspending: bool,
        requirements: List[TypeShape],
    )

data FieldShape:
    id: DeclarationId
    name: string
    qualified_name: string
    source: SourcePosition
    position: i32
    doc: string?
    field_type: TypeShape

data DataShape:
    id: DeclarationId
    name: string
    qualified_name: string
    source: SourcePosition
    doc: string?
    field_list: List[FieldShape]

data VariantShape:
    id: DeclarationId
    name: string
    qualified_name: string
    source: SourcePosition
    position: i32
    doc: string?
    payload: List[FieldShape]

data EnumShape:
    id: DeclarationId
    name: string
    qualified_name: string
    source: SourcePosition
    doc: string?
    variant_list: List[VariantShape]

data ParamShape:
    id: DeclarationId
    name: string
    qualified_name: string
    source: SourcePosition
    position: i32
    doc: string?
    param_type: TypeShape
    has_default: bool

data FnShape:
    id: DeclarationId
    name: string
    qualified_name: string
    source: SourcePosition
    doc: string?
    params: List[ParamShape]
    result: TypeShape
    suspending: bool
    requirements: List[TypeShape]

trait ShapeMetadata:
    fn metadata[reified M](self) -> M?
```

Every concrete shape type implements sealed `ShapeMetadata`; user code cannot
add implementations. `metadata[M]()` performs the one narrow runtime type
lookup supported for heterogeneous annotation metadata and preserves the
attached value's declared permission. It does not add a general `Any`
downcast; recovering the concrete type of an erased value is the separate
facility in [Runtime Type Identity](09-traits.md#runtime-type-identity), and
a `TypeId` exposes no shape. `TypeShape.is_optional() -> bool` is also a compiler-provided readonly
method and is true exactly for `TypeShape.Optional`.

`TypeShape.Mut` encodes mutable access, `TypeShape.Trait` a dynamic trait
value type with its trait declaration and arguments, `TypeShape.Any` the
`Any` type, and `TypeShape.Suspend` a `Suspend[T]` type with its result.
`TypeShape.Newtype` carries the newtype's own declaration identity beside its
base type.

Shape values are readonly runtime values and may be passed, stored, and
inspected like other composite values.

### Shape Intrinsics

`shape` and `shape_of` are compiler intrinsics declared by `std.annotation`
and re-exported by the prelude. They are ordinary names, not reserved words:
they are called with ordinary call syntax, and the prelude shadowing rule
applies to them. Each use must be the callee of a direct call; using either
name as a function value is an `unknown-shape-target` error. Missing or extra
arguments follow the ordinary call rules.

`shape[T]()` takes exactly one explicit type argument and no value arguments.
`T` is a reified type argument under the rules of
[Generics](04-type-system.md#generics): inside a generic declaration, a type
parameter passed as `T` must itself be `reified`. The result depends on `T`:

- For a data type `D`, the result has the **specialized data shape type** of
  `D`.
- For an enum `E`, the result has the **specialized enum shape type** of `E`.
- For any other type, including a type parameter, a primitive, a collection, a
  tuple, an optional, and an applied generic nominal type such as
  `Box[i32]`, the result is a `TypeShape`.

A specialized shape type is generated by the compiler and cannot be named in
source. The specialized data shape type of `D` has every member of `DataShape`
with the same name and type, plus `fields`: a readonly record with exactly
one member for each direct field of `D`, named after that field and of type
`FieldShape`. `shape[D]().fields.email` is therefore statically checked, and
equals the element of `field_list` at the position of `email`. The
specialized enum shape type of `E` likewise adds `variants`, with one
`VariantShape` member per variant of `E`, equal to the corresponding element
of `variant_list`. Selecting a member the declaration does not have, such as
`shape[User]().fields.deleted`, is an `unknown-shape-target` error. The
`fields` and `variants` records are not collections: they support only member
selection, and iteration uses `field_list` or `variant_list`.

A specialized shape value is assignable to its generic shape type, so
`shape[User]()` can be passed where `DataShape` is expected; the conversion
does not change the value. No other specialized member survives that
conversion.

`shape_of(f)` takes exactly one argument, which must directly name a
module-level function declaration, optionally through a module qualifier or
a `use` import. It returns that function's `FnShape`. The argument is not
evaluated. A closure, a local binding, a parameter, a method, a field access,
a call, or any other function-valued expression is an `unknown-shape-target`
error, as is a name that resolves to a type or a non-function binding.

## Annotation Protocol

All annotation protocol and shape names in this chapter are declared by
`std.annotation` and re-exported by the prelude.

An annotation facet chooses one uniform output type:

```text
trait Annotation:
    type Info

trait Annotate[A < Annotation]:
    fn info() -> A::Info
```

Associated types and projections use the core trait grammar:

```ebnf
associated_type_decl = "type", identifier, [ "=", type ], NEWLINE ;

associated_type_projection = ( qualified_name | "Self" ), "::", identifier ;
```

Within a trait or implementation, `Self::Name` projects an associated type from
the current implementor. `A::Info` projects through a generic annotation type.
Associated type declarations are trait and implementation members;
implementations assign a concrete type with `type Name = T`. Receiverless
associated functions such as `fn info() -> A::Info` are ordinary core trait
members.

`trait Annotation` declares `Info`; an implementation assigns it:

```text
impl Annotation for Validation:
    type Info = Validator
```

An annotation facet is an ordinary type. A stateless facet uses an empty data type:

```text
data Validation: pass
```

Its ordinary value is `Validation {}`. Annotation lowering constructs that
value when invoking instance methods on its annotator implementation.

`Annotate[A] for T` means that target `T` can produce `A::Info`. It is a normal
trait conformance for constraint and coherence purposes.

## Two `annotate` Forms

### Prefix Decorators

An `@Facet` line immediately before a module-level data, enum, or function
declaration requests the same no-override facet derivation as `annotate Facet for
Target: pass`. The facet must implement `DataAnnotator`, `EnumAnnotator`, or
`FuncAnnotator` for the respective target, in addition to `Annotation`. Thus
`@Validation` before `data User` expands to `annotate Validation for User:
pass`, which occupies the `impl Annotate[Validation] for User` coherence slot.
The same rule applies to `@Validation` before an enum and `@Tool` before a
function. A decorator does not register the resulting runtime information.

A declaration decorator may instead be an ordinary configured facet
expression. If `tool(strict=true)` has static type `Tool`, then
`@tool(strict=true)` expands to `annotate tool(strict=true) for Target: pass`
and occupies the same `(Tool, Target)` coherence slot as `@Tool`. The expression
is evaluated once during annotation initialization, and that exact value is
used as `self` for every mapping and `build` call for the target. It must obey
the same requirement-free restriction as other annotation initialization
expressions.

Before a data or enum declaration, a decorator whose value's static type does
not implement `Annotation` is not a facet: it attaches a type-level fact, as
[Facts](#facts) defines. Before a function, such a decorator is a
`decorator-not-annotator` error.

An `@value` line immediately before a named or embedded data field or an enum
variant attaches member metadata. For a field `name: string`, `@max_len(80)`
expands to the `max_len(80)` entry in
`annotate User: name = [max_len(80)]`. The value must implement
`FieldMetadata[string]`; a variant value must implement `VariantMetadata`.
Member metadata is also the member's or variant's declaration facts for
[typed derivation](#facts).

For an embedded `Timestamps` field in `Post`, `@flatten()` expands to the
`flatten()` entry in `annotate Post: Timestamps = [flatten()]` and must
implement `FieldMetadata[Timestamps]`. A generic embedded `Box[T]` similarly
requires `FieldMetadata[Box[T]]`, while its member name remains `Box`. Metadata
is attached only to the embedded field's own `FieldShape`; it is not propagated
to promoted fields or methods.

Multiple lines retain source order and obey the same duplicate-concrete-type
rule as an explicit member metadata list. A decorator and an explicit
`annotate` block for the same target combine only when they do not assign the
same member or occupy the same facet/target coherence slot. Decorators attach
to declarations or members, never to type expressions. Declaration decorators
are module-level syntax; local declarations cannot be decorated or targeted by
an `annotate` declaration.

An `@value` prefix on a parameter of a module-level named function attaches
parameter metadata. For `id: UserId`, the value must implement
`ParamMetadata[UserId]`. The prefix may be inline or occupy its own line in a
multiline parameter clause:

```text
@Tool
fn get_user(
    @description("User identifier")
    id: UserId,
) -> User:
    ...
```

This expands to the `id` entry in `annotate get_user` and becomes visible
through `ParamShape` before `Tool.map_param` runs. Parameter decorators are not
accepted on closures, methods, trait requirements, receiver parameters, or
local functions.

`@derive` uses the same prefix position but is not an ordinary `@Facet`
decorator. Its arguments are trait names rather than metadata values. The
compiler checks and generates each requested implementation; no general
`DataAnnotator` can emit implementations through `Annotation::Info`. [Opting In](#opting-in) defines which traits it
accepts.

### Member Metadata

`annotate Target` attaches metadata values to existing fields, enum variants,
or function parameters:

```text
data User:
    display_name: string

annotate User:
    display_name = [min_len(1), max_len(80)]
```

The target is a declared data type, enum, or module-level named function, not
an arbitrary type expression. Every left-hand name must identify an existing
direct member or parameter. An embedded field is a direct member under its
final type name; members promoted through it are not direct members. The block
cannot add, rename, remove, or change the type of a member or parameter.

Field metadata is contextually typed as `List[FieldMetadata[T]]`, where `T` is
the declared field type. Variant metadata uses the corresponding marker trait:

```text
trait FieldMetadata[T]
trait VariantMetadata
trait ParamMetadata[T]
```

Function parameter metadata is contextually typed as
`List[ParamMetadata[T]]`, where `T` is the declared parameter type. One
parameter must not contain two metadata values with the same concrete metadata
type.

Metadata objects are ordinary values. Constructors and helper functions are
equivalent ways to make them:

```text
data MaxLen:
    value: i32

fn max_len(value: i32) -> MaxLen: MaxLen { value: value }

impl FieldMetadata[string] for MaxLen
```

The compiler accepts `max_len(80)` for a `string` field and rejects it for an
`i32` field through ordinary trait checking. One member must not contain two
metadata values with the same concrete metadata type.

Reusable compositions are ordinary values or lists, not new language syntax:

```text
let email_metadata: List[FieldMetadata[string]] = [
    min_len(3),
    max_len(320),
    contains("@"),
]
```

Different concrete metadata types coexist through the homogeneous dynamic
trait value `FieldMetadata[string]`.

### Derived Facet Information

`annotate Facet for Target` generates the same coherence slot as
`impl Annotate[Facet] for Target`, while allowing structure-aware overrides:

```text
annotate Validation for User: pass
```

`pass` requests ordinary facet derivation with no field, variant, parameter, or whole
result override.

For an exact primitive, nominal type, optional type, or collection
instantiation, the block may provide `build` directly:

```text
annotate Validation for string:
    fn build(self, target: TypeShape) -> Validator:
        Validator.String

annotate Validation for Email:
    fn build(self, target: TypeShape) -> Validator:
        Validator.Email

annotate[reified T < Annotate[Validation]] Validation for List[T]:
    fn build(self, target: TypeShape) -> Validator:
        Validator.List(Validation::annotation_ref(T))
```

Annotation facets are open across exact targets and generic target families.
A generic `annotate` declaration uses the same generic binders, bounds,
coherence, and overlap rules as its lowered generic `impl`. There is no
unconstrained wildcard `annotate Validation for type` fallback; a family names
a concrete type pattern such as `List[T]`.

## Grammar

Module-level annotation declarations use this grammar:

```ebnf
annotation_decl = member_metadata_decl | facet_annotation_decl ;

member_metadata_decl = "annotate", qualified_name, ":",
                       annotation_member_suite ;

facet_annotation_decl = "annotate", [ generic_params ], annotation_facet,
                        "for", annotation_target, ":",
                        facet_annotation_suite ;

annotation_facet = type | closed_expression ;

annotation_target = type ;

annotation_member_suite = "pass", SUITE_END
                        | NEWLINE, INDENT,
                          ( "pass", NEWLINE
                          | metadata_assignment, { metadata_assignment } ),
                          DEDENT
                        ;

metadata_assignment = identifier, "=", closed_expression, NEWLINE ;

facet_annotation_suite = "pass", SUITE_END
                       | NEWLINE, INDENT,
                         ( "pass", NEWLINE
                         | facet_override, { facet_override } ), DEDENT
                       ;

facet_override = metadata_assignment | function_decl ;

annotation_runtime_access = qualified_name, "::", "annotation", "(",
                            annotation_target, ")"
                          | qualified_name, "::", "annotation_ref", "(",
                            annotation_target, ")"
                          ;
```

The facet operand is either a facet type or a configured expression whose
static type implements `Annotation`. A type operand constructs the stateless
facet's default empty value and therefore requires a facet type with no required
fields. An expression value is retained and reused for the complete
facet derivation. In both cases, the facet's static type determines the
`Annotate[Facet]` coherence slot. Target resolution distinguishes a type target
from a function target. `annotate Target` metadata
assignments apply to data fields, enum variants, or module-level function
parameters. Function annotators read the resulting parameter shapes and may
replace the complete `build`.

Either suite accepts `pass` after the header's `:` or alone on an indented
line; both request ordinary derivation with no assignments or overrides.

`[` directly after `annotate` always opens generic parameters, as after
`impl`. A facet expression therefore cannot begin with `[`: in
`annotate [Tag("a")] for Point: pass`, the brackets are read as generic
parameters and the declaration is a `syntax-error`. Parenthesize such an
expression.

Generic parameters and their bounds have the same meaning as on an ordinary
generic implementation. For example,
`annotate[T] Validation for List[T]` occupies the same coherence slot as
`impl[T] Annotate[Validation] for List[T]`; an overlapping exact annotation is
rejected under the normal implementation-overlap rules.

## Aggregate Annotators

Exact primitive, newtype, and collection-family facet derivation uses the
type-level annotator protocol:

```text
trait TypeAnnotator < Annotation:
    fn build(self, target: TypeShape) -> Self::Info
```

An exact-type `build` override is checked against this signature and requires
the facet to implement `TypeAnnotator`.

A facet for data types maps each field to one uniform `FieldTarget`, then builds the
facet's `Info`:

```text
trait DataAnnotator < Annotation:
    type FieldTarget

    fn map_field(
        self,
        field: FieldShape,
        type_metadata: AnnotationRef[Self::Info],
    ) -> Self::FieldTarget

    fn build(
        self,
        target: DataShape,
        fields: List[(string, Self::FieldTarget)],
    ) -> Self::Info
```

`fields` is in declaration order. The name in each tuple is the declared field
name; an ordered list is used so deterministic builders never depend on map
hashing or insertion accidents.

An enum first maps every payload field, then maps each variant, then builds the
enum result:

```text
trait EnumAnnotator < Annotation:
    type FieldTarget
    type VariantTarget

    fn map_field(
        self,
        field: FieldShape,
        type_metadata: AnnotationRef[Self::Info],
    ) -> Self::FieldTarget

    fn map_variant(
        self,
        variant: VariantShape,
        fields: List[Self::FieldTarget],
    ) -> Self::VariantTarget

    fn build(
        self,
        target: EnumShape,
        variants: List[(string, Self::VariantTarget)],
    ) -> Self::Info
```

A function maps parameters and builds a function result:

```text
trait FuncAnnotator < Annotation:
    type ParamTarget

    fn map_param(
        self,
        param: ParamShape,
    ) -> Self::ParamTarget

    fn build(
        self,
        target: FnShape,
        params: List[(string, Self::ParamTarget)],
    ) -> Self::Info
```

The compiler supplies data fields, enum variants, and function parameters as
ordered `(declared_name, mapped_value)` lists in source declaration order. The
three aggregate annotator protocols therefore use one input shape and do not
depend on map hashing or insertion behavior.

`FieldTarget`, `VariantTarget`, `ParamTarget`, and `Info` are uniform types.
They do not vary at the type level with the original member type. An annotator
may inspect each shape's runtime type descriptor and member metadata to choose a
value, but the core type system does not model a type-level function from field
type to mapped output type. `map_param` may inspect metadata attached to its
`ParamShape` through `ParamMetadata[T]` values.

## Structural Overrides

Within `annotate Facet for Data`, an assignment to a field name replaces that
field's default `map_field` result and must have type
`Facet::FieldTarget`:

```text
# UI and profile_image are user-defined examples, not prelude declarations.
annotate UI for User:
    avatar = profile_image(size=40)
```

The assignment cannot target a missing or promoted field. Enum variant
overrides follow the same rule with `VariantTarget`.
An explicit field result also permits facet derivation when the field's declared
type has no `Annotate[Facet]` implementation. In that case the compiler uses
the override directly and does not call `map_field` for that field. An exact
variant result similarly bypasses mapping that variant's payload fields.

A local `fn build` definition replaces aggregate `build` for that exact
facet/target pair. It receives the completed uniform map and may rewrite the
whole result. Other map steps still occur. A local `build` replaces aggregate
assembly, not member mapping; hd-lang has no separate full facet derivation
replacement hook. To bypass child annotation resolution and every mapping step,
write a direct `impl Annotate[Facet] for Target`. That implementation produces
`Facet::Info` itself and occupies the same coherence slot as an `annotate`
declaration for the pair.

An assignment in `annotate Function` attaches parameter metadata; it does not
replace that parameter's `ParamTarget`. Exact function-parameter result
overrides in `annotate Facet for Function` are not part of the language.
Function-specific result customization uses parameter metadata, types, names,
defaults, documentation, or a whole-function `build` override.

## Bottom-Up Evaluation

Annotation materialization follows a strict order:

1. resolve annotation information for each member's declared type, except
   members whose exact facet result is overridden;
2. attach and evaluate that member's field, variant, or parameter metadata;
3. call the enclosing annotator's `map_field`, `map_variant`, or `map_param`
   for members without an exact result override;
4. use exact structural result overrides in place of those mapped results;
5. call the enclosing `build`, or the exact local `build` override.

For enums, every payload field is mapped before its containing variant; every
variant is mapped before enum `build`.

For functions, every parameter's metadata is evaluated and attached before
`map_param`; every parameter is mapped before function `build`.

An enclosing annotator receives ordinary shape and mapped values. It may
interpret or replace child metadata values in its own result, but it cannot
retroactively alter the source declaration or another independent facet.

## Runtime Materialization

The following user-defined facets illustrate explicit materialization; their
names are examples, not prelude declarations:

```text
validator := Validation::annotation(User)
table := DatabaseSchema::annotation(User)
tool := Tool::annotation(get_user)
```

`Facet::annotation(Target)` returns an ordinary value of the facet's `Info`
type. A function annotation does not make
the function public, register it, or enable runtime discovery:

```text
tool_registry.register(Tool::annotation(get_user))
```

Registration is explicit library behavior.

Annotation builders and metadata expressions execute lazily at runtime on the
first `annotation(...)` or `annotation_ref(...)` request for a concrete
`(facet, target)` key. One thread-free registry per program instance
materializes each key at most once. This restricted annotation-initialization
phase is not unrestricted compiler evaluation. Builders and metadata must be
requirement-free, as defined for defaults in
[Functions](07-functions.md#default-values): no provider access and no
suspension, checked from the signatures of the callables they use. IO, clocks,
randomness, networks, and databases are reachable only through providers, so
they are excluded by that rule. A builder may read or write other state; it
runs once per key, at the first request, and observes state as of that
moment.
A panic is an ordinary panic reported at the first
request site; it does not occur merely because the annotated declaration is
loaded.

Configured facet expressions execute once under these same restrictions when
their key is first materialized, before the target is mapped. Their value is retained for that target's facet derivation;
calling `Facet::annotation(Target)` returns the completed memoized `Info`, not
the facet configuration value.

The compiler's special role is to:

- validate target and member names;
- contextually type metadata and override values;
- check the corresponding annotation traits;
- generate ordinary shape construction and trait implementation code;
- arrange lazy cycle-aware resolution and per-instance memoization.

The lowering does not otherwise change the runtime semantics of annotation
values.

## Desugaring Model

The following is illustrative, not normative generated source. The compiler may
lower differently while preserving the checked behavior.

```text
data User:
    email: string

annotate User:
    email = [max_len(320), contains("@")]

annotate Validation for User: pass
```

Conceptually becomes shape metadata plus an implementation:

```text
fn __user_shape() -> DataShape:
    # The compiler constructs all required identity, source, position, doc,
    # and type fields and attaches the declared metadata to the field target.
    shape[User]()

impl Annotate[Validation] for User:
    fn info() -> Validator:
        target := __user_shape()
        facet := Validation {}
        # Attached values are available through
        # target.field_list[0].metadata[FieldMetadata[string]]().
        field := facet.map_field(
            target.field_list[0],
            Validation::annotation_ref(string),
        )
        facet.build(target, [("email", field)])
```

The generated names and exact runtime calls are implementation details. This
desugaring exists to illustrate that `annotate` supplies typed structure-aware
sugar rather than a separate semantic object system.

## Recursive Annotations

Annotation resolution is keyed by `(facet, concrete target)`. A facet can embed
references to child information using the general runtime type
`AnnotationRef[T]`:

```text
data FieldValidator:
    name: string
    target: AnnotationRef[Validator]
```

The two annotation access forms are compiler-recognized typed operations:

```text
Facet::annotation(Target) -> Facet::Info
Facet::annotation_ref(Target) -> AnnotationRef[Facet::Info]

trait AnnotationRef[T]:
    fn get(self) -> T
```

`Target` is a type or function target, not an ordinary value expression.
`get()` returns the completed memoized value. Reading a deferred reference
before its key completes panics with `annotation-reference-unresolved`.

When resolution first enters a key, it marks the key active. Resolving a
different key normally returns a ready reference after construction. Re-entering
an active key returns a deferred reference to that key. Once the outer build
completes, the deferred reference resolves through the memoized registry entry.

A direct `annotation(K)` call while `K` is being resolved is rejected
statically when the cycle is reachable from the annotation call graph. If the
cycle depends on a dynamic call and cannot be proven statically, re-entry is a
checked panic with category `annotation-resolution-reentry`. Recursive
builders must use `annotation_ref(K)` and delay `get()` until resolution has
completed.

This automatic cycle detection applies uniformly to data and enum annotation
graphs. Facets do not need a custom `Validator.Ref` variant or a separate
`Annotation.reference` hook. A facet opts into recursive output by storing
`AnnotationRef[Info]` where child information appears.

For example, mutually recursive `Folder` and `Entry` validation can use the same
`AnnotationRef[Validator]` for data fields and enum payloads. Non-recursive
edges produce ready references; only back edges are deferred.

Automatic cycle detection is the annotation recursion mechanism. It does not
make the program field itself lazy.

## Coherence And Package Rules

At most one annotation exists for an exact `(facet, target)` pair in the whole
resolved package graph. `annotate Facet for Target` and an explicit
`impl Annotate[Facet] for Target` occupy the same coherence slot.
Configured facet expressions use their static facet type in this key, so two
differently configured values of `Tool` cannot annotate the same target.

Annotation blocks are package-global rather than lexical. An annotation, like
its lowered `impl Annotate[Facet] for Target`, follows the ordinary
[implementation ownership rule](09-traits.md#implementation-ownership): the
package that declares the facet type, which is the trait argument, or the
package that owns the target's type constructor may declare it. For example,
a validation library that declares `Validation` may write
`annotate Validation for string`. If a library provides an annotation for its
target, a downstream package cannot override it. An orphan annotation is one
whose package owns neither the facet type nor the target. There is no orphan
exception: an orphan annotation is an `orphan-impl` error in every package,
including the root application package. A package that owns neither
annotates a local mirror type or a newtype of its own instead.

Local declarations cannot participate in annotation coherence. They may still
be reflected where ordinary local shape rules permit, but they cannot receive
facet or member metadata through decorators or `annotate` blocks.

There is no implicit library default annotation. A target has facet information
only when the facet/target conformance is explicitly present or structural
facet derivation is explicitly requested with `annotate Facet for Target`.

## Missing Child Information

An unoverridden field derives facet information from its declared type. Each
primitive, collection instantiation, or user-defined type that participates in
that facet must provide `Annotate[Facet]`; for example, an `i32` validator
checks that a value is an integer, while a custom type supplies its own default
validation. The same rule applies to enum payload fields.

An exact field override in `annotate Facet for Data` supplies
`Facet::FieldTarget` instead of deriving it from the field type. An exact enum
variant override supplies `Facet::VariantTarget` instead of mapping that
variant's payload fields. If neither an applicable type annotation nor an
applicable exact override exists, facet derivation is a compile-time error. The
compiler never silently omits a child. Ordinary `annotate Data` member metadata
can customize `map_field`, but metadata alone is not an `Annotate[Facet]`
implementation or a `FieldTarget` override.

## Full Validation Example

The conformance fixture
[`full-validation.hd`](conformance/typing/valid/full-validation.hd) is the
current full worked example. It demonstrates:

- exact primitive and nominal type annotations;
- field and variant metadata checked through ordinary traits;
- data and enum annotators;
- uniform mapped dictionaries and lists;
- runtime materialization;
- recursive data/enum graphs through `AnnotationRef`.

The focused annotation conformance fixtures cover individual parts of the
language surface. This larger fixture additionally sketches a validation
library built on it.

## Typed Derivation

**Typed derivation** implements a library's trait for a data type or enum
from the type's members. The library writes the implementation once, as a
template over the compiler-generated `Structure` trait. A type opts in with
`@derive`, and configures the result with facts.

```text
@derive(Encode, Keys)
@style(prefix="user_")
data User:
    id: i64
    @rename("mail")
    email: string
    nickname: string = ""
```

1. r[annot.derive.definition] Typed derivation implements a trait for a data type or enum from the type's members, through the trait's template.
2. r[annot.derive.supplied] The compiler supplies only `Structure`, the handle constants, and the generated `walk`, `describe`, and `build` bodies.
3. r[annot.derive.library] Templates, walkers, describers, sources, and facts are ordinary library code.

> **Why.** The compiler generates one statically typed traversal per type.
> Every format, comparison, or schema is library code over it, so the
> language needs no macro system and no per-library compiler support.

See also: [Derived Implementations](09-traits.md#derived-implementations),
[Typed Derivation design record](../future-work/TYPED_DERIVATION.md).

### Opting In

`@derive(...)` lists the traits a declaration derives.

1. r[annot.derive.opt-in] `@derive(...)` is the only form that creates a derived implementation from a declaration.
2. r[annot.derive.accepted] It accepts the intrinsic comparison traits of [Derived Implementations](09-traits.md#derived-implementations) and any trait that has a [template](#templates).
3. r[annot.derive.means] For a trait `X` with a template, `@derive(X)` before a data type or enum `T` means exactly the derivation block `impl X for T by Structure` with an empty body. A newtype derives through its base type instead, as [Derived Newtypes](09-traits.md#derived-newtypes) defines.
4. r[annot.derive.other] Any other trait in a `@derive` list is an error, reported on the `@derive` line. Error: `underivable-trait`.
5. r[annot.derive.error-trait] `Error` has neither a template nor an intrinsic derivation, so `@derive(Error)` is an error. Error: `underivable-trait`.
6. r[annot.derive.facts-only] Every other decorator only attaches information: a configuration decorator such as `@style(prefix="user_")` attaches a fact and creates no implementation.
7. r[annot.derive.overlap] Listing a trait in `@derive` and also writing a derivation block for it on the same type is an error, reported on the block. Error: `overlapping-impl`.

```text
use std.error.Error

@derive(Error)  # error: underivable-trait
enum LoadError:
    Missing

@derive(Show)
data Point:
    x: i64

impl Show for Point by Structure:  # error: overlapping-impl
    x = []
```

> **Note.** An error type uses the separate `@error` intrinsic, as
> [Error Conversion decision 10](../future-work/ERROR_CONVERSION.md#owner-decisions)
> records.

### The `std.structure` Module

The module `std.structure` declares the derivation protocol. The compiler
supplies every body written `pass`:

```text
pub trait Structure:
    fn facts() -> Facts
    fn walk[W < Walker[Self]](self, w: mut W) -> Result[void, W::Error]
    fn describe[D < Describer[Self]](d: mut D) -> Result[void, D::Error]
    fn build[S < Source[Self]](s: mut S) -> Result[mut Self, S::Error]

pub data Member:
    pub name: string
    pub position: i32
    pub facts: Facts
    pub doc: string?
    pub embedded: bool
    pub positional: bool

pub data VariantInfo:
    pub name: string
    pub index: i32
    pub facts: Facts
    pub doc: string?
    pub of_data: bool
    pub shared: List[(string, Any)]

pub data Field[-S, +F]:
    pub info: Member

impl[S, F] Field[S, F]:
    pub fn get(self, s: S) -> F: pass
    pub fn has_default(self) -> bool: pass
    pub fn default(self) -> F?: pass

pub data Variant[S]:
    pub info: VariantInfo

impl[S] Variant[S]:
    pub fn holds(self, s: S) -> bool: pass

pub data Members[S]:
    pub infos: List[Member]

impl[S] Members[S]:
    pub fn end(self) -> Key[S]: pass
    pub fn at(self, position: i32) -> Key[S]: pass
    pub fn find(self, matches: fn(Member) -> bool) -> Key[S]: pass

pub data Key[S]:
    pub info: Member
    pub is_end: bool

pub trait Walker[S]:
    type Error
    fn variant(mut self, v: Variant[S]) -> Result[void, Self::Error]
    fn member[F](mut self, h: Field[S, F], value: F) -> Result[void, Self::Error]

pub trait Describer[S]:
    type Error
    fn variant(mut self, v: Variant[S]) -> Result[void, Self::Error]
    fn member[F](mut self, h: Field[S, F]) -> Result[void, Self::Error]

pub trait Source[S]:
    type Error
    fn variant(mut self, choices: List[Variant[S]]) -> Result[Variant[S], Self::Error]
    fn next(mut self, members: Members[S]) -> Result[Key[S], Self::Error]
    fn member[F](mut self, h: Field[S, F], previous: F?) -> Result[F, Self::Error]
    fn missing[F](mut self, h: Field[S, F]) -> Result[F, Self::Error]
```

1. r[annot.structure.module] The standard module `std.structure` declares `Structure`, `Facts`, `Member`, `VariantInfo`, `Field`, `Variant`, `Members`, `Key`, `Walker`, `Describer`, and `Source`.
2. r[annot.structure.bodies] The compiler supplies every body written `pass` in the declarations above, and the `Facts` type.
3. r[annot.structure.facts-type] `Facts` holds the facts attached to one type, member, or variant, in source order. `facts.find[F]()` returns the fact whose concrete type is `F`, or `.None`.
4. r[annot.structure.find-lookup] `find` performs the same narrow runtime type lookup as `metadata[M]` in [Common Shape Representation](#common-shape-representation), and nothing more.
5. r[annot.structure.members-api] `members.end()` is the end key, `members.at(position)` is the key of the member at that position, and `members.find(matches)` is the key of the first member that `matches` accepts. Each returns the end key when no member fits.
6. r[annot.structure.no-names] The names of `Members`, `Key`, and the handle methods are fixed by these declarations. Further helpers over them are standard-library design, outside this specification.

> **Note.** Standard walkers, describers, and sources, such as the ones a
> `std.json` would use, are library design. The
> [design record](../future-work/TYPED_DERIVATION.md#current-design-full-example-m1-m21)
> shows a complete json library, and
> [STDLIB](../future-work/STDLIB.md) tracks the standard modules.

### The Structure Trait

`Structure` is the compiler-generated view of one type's members.

1. r[annot.structure.sealed] `Structure` is a sealed trait: an `impl Structure for T` outside the standard library is an error. Error: `sealed-trait-implementation`.
2. r[annot.structure.generated] The compiler generates `Structure` for a target only while it instantiates a template for that target. No type has it otherwise.
3. r[annot.structure.per-derivation] Each derivation sees its own `Structure` for the target, which reflects that derivation's facts and omitted members.
4. r[annot.structure.template-only] `Structure` may be named, as a bound or in a call such as `T::facts()`, only inside a template. Outside one, it may appear only after `by` in a derivation block's header.
5. r[annot.structure.template-only.error] Any other use of `Structure`, such as the bound in `fn fields[X < Structure]`, is an error. Error: `structure-outside-template`.
6. r[annot.structure.receivers] `walk` takes the value as a readonly `self`. `facts`, `describe`, and `build` are receiverless.
7. r[annot.structure.build-fresh] `build` returns `mut Self`, because a built value is fresh like a data literal. Callers weaken it by ordinary assignability.
8. r[annot.structure.private] `walk`, `describe`, and `build` include private members. Opting a type in is consent for the template's library to read every member.
9. r[annot.structure.pure] `walk`, `describe`, and `build`, and every method of `Walker`, `Describer`, and `Source`, have the empty requirement row and are not suspending.
10. r[annot.structure.pure.impl] An implementation method of `Walker`, `Describer`, or `Source` that declares a requirement or suspends is an error. Error: `trait-method-signature`.

```text
use std.structure.Structure

data Point:
    x: i64

impl Structure for Point  # error: sealed-trait-implementation

fn fields[X < Structure]() -> void:  # error: structure-outside-template
    pass
```

> **Why.** Traversal stays pure, as serde's is: input and output happen
> before `build` or after `walk`, so a derivation never needs a provider.

### Templates

A template is a trait's one derived implementation, written over
`Structure`:

```text
impl[T] Encode for T by Structure:
    fn encode(self) -> string:
        let w: mut Encoder = Encoder { style: style_of(T::facts()), out: "" }
        _ := Structure::walk(self, w)
        w.out
```

1. r[annot.template.form] `impl[T] Trait for T by Structure:` declares the **template** of `Trait`.
2. r[annot.template.not-impl] A template is not an implementation. It never applies by itself and occupies no coherence slot, so it never overlaps a hand-written implementation.
3. r[annot.template.module] A template must be declared in the module that declares its trait. One declared elsewhere is an error. Error: `misplaced-derivation`.
4. r[annot.template.unique] A trait has at most one template. A second one is an error. Error: `overlapping-impl`.
5. r[annot.template.structure] Inside a template, `T` implements `Structure`. Its bodies may call `Structure::walk(self, w)`, `T::describe(d)`, `T::build(s)`, and `T::facts()`.
6. r[annot.template.body] A template must have a body in which at least one method calls `walk`, `describe`, or `build`.
7. r[annot.template.body.error] A template without such a body, such as a bodiless marker template, is an error. Error: `marker-template`.
8. r[annot.template.instance] Each derivation instantiates the template once, as one ordinary implementation of the trait for the target, in the target's module.
9. r[annot.template.no-families] A template never derives an implementation family, a new type, or a builder: it implements the existing trait exactly once per derivation.
10. r[annot.template.checked] The instantiated implementation is type-checked at its opt-in, where the member types and the walker's bounds are both known.

```text
trait Tagged:
    fn tag(self) -> string:
        "tagged"

impl[T] Tagged for T by Structure  # error: marker-template
```

> **Why.** Only the trait's module may declare the template, so a trait has
> one derivation and a reader finds it beside the trait. A marker template
> would derive a trait without reading one member.

### Derivation Blocks

A derivation block applies one template to one type and may configure it:

```text
data Order:
    id: i64
    total_cents: i64
    cache: Cache = Cache {}

impl Encode for Order by Structure:
    Self += [style(prefix="order_")]
    total_cents = [rename("total")]
    cache = pass
```

1. r[annot.block.form] `impl Trait for X by Structure:` is a **derivation block**: it applies the template of `Trait` to `X`.
2. r[annot.block.template] `Trait` must have a template. A block for any other trait, including a comparison trait, is an error. Error: `underivable-trait`.
3. r[annot.block.module] A derivation block must be declared in the module that declares `X`. One declared elsewhere is an error. Error: `misplaced-derivation`.
4. r[annot.block.by-structure] In an implementation header, `by Structure` always denotes `std.structure.Structure`. It needs no import, and it is never a delegation.
5. r[annot.block.methods] A block may write a method of the trait. The written method replaces the template's method, as a written method replaces a default.
6. r[annot.block.lines] A block may carry [member lines](#member-lines), which edit the `Structure` that this derivation sees.
7. r[annot.block.local] Member lines are local to their block. Another derivation for the same type sees only the declaration facts.
8. r[annot.block.header] A block may state generic parameters and bounds in its header, as in `impl[T < Hash] Encode for Bag[T] by Structure:`. The derived implementation then has exactly those bounds.
9. r[annot.block.gadt] The target of a derivation, by `@derive` or by a block, must not be a GADT enum. Such a derivation is an error, reported at the opt-in. Error: `gadt-derivation`.

> **Note.** A foreign type has no derivation exception. Derive on a local
> mirror type with the foreign type's public fields, and convert, as with
> serde's `remote`.

#### Member Lines

A **member line** in a derivation block adjusts facts, or leaves a member
out, for that block only:

| Rule | Form | Meaning for this block |
| --- | --- | --- |
| r[annot.line.extend] Extend | `f += [facts]` | the declaration facts of `f`, followed by `facts` |
| r[annot.line.replace] Replace | `f = [facts]` | `facts` in place of the declaration facts of `f` |
| r[annot.line.omit] Omit | `f = pass` | `f` is left out of `walk`, `describe`, and `build` |
| r[annot.line.self] Type-level | `Self += [facts]`, `Self = [facts]` | the type-level facts, extended or replaced |

1. r[annot.line.name] The name on the left of a member line must be a direct member of the target, or `Self`. Any other name is an error. Error: `unknown-annotation-member`.
2. r[annot.line.enum] In an enum's block, a member line names a whole variant or `Self`. A line that names a payload member is an error. Error: `unknown-annotation-member`.
3. r[annot.line.right] The right side must be a list expression, or `pass` after a member name and `=`.
4. r[annot.line.right.error] Any other line is an error. That includes `f += pass`, `Self = pass`, and `pass` for a whole variant, such as `Busy = pass`. Error: `invalid-member-line`.
5. r[annot.line.typed] A member line's list is contextually typed as that member's metadata list, as in [Member Metadata](#member-metadata). A `Self` line's list is contextually typed as `List[Any]`.
6. r[annot.line.duplicate] After a line applies, one member, variant, or type must not hold two facts of the same concrete type. `+=` with a type already present is an error; `=` changes it instead. Error: `duplicate-fact`.
7. r[annot.line.unchanged] A member without a line keeps its declaration facts.
8. r[annot.line.placement] A member line anywhere other than a derivation block, including in a template or an ordinary implementation, is an error. Error: `misplaced-derivation`.

```text
data Pair:
    left: i64
    right: i64

impl Show for Pair by Structure:
    left += [rename("l")]
    left += [rename("first")]  # error: duplicate-fact

enum Reply:
    Sent(id: i64)
    Busy

impl Show for Reply by Structure:
    id = [rename("x")]  # error: unknown-annotation-member
    Busy = pass         # error: invalid-member-line
```

> **Why.** A whole variant cannot be omitted, because `build` could never
> produce it. A payload member has no line, because a line names what a
> derivation walks as a unit.

#### Omitted Members

1. r[annot.omit.skip] A member omitted with `= pass` is not walked or described, and `build` never offers it to the source.
2. r[annot.omit.default] `build` fills an omitted member by evaluating its declared default.
3. r[annot.omit.no-default] Omitting a member that declares no default is an error, reported on the member line. Error: `omitted-member-without-default`.
4. r[annot.omit.exempt] An omitted member's type need not satisfy any walker's, describer's, or source's bound.
5. r[annot.omit.only-code] `= pass` is the only member line that changes generated code. Every other line changes only facts.

```text
data Account:
    id: i64
    token: Token

impl Show for Account by Structure:
    token = pass  # error: omitted-member-without-default
```

#### Line Drift

1. r[annot.line.drift] Derivation blocks for traits of one package on one type whose member lines differ get a warning, reported on the later block. Warning: `derivation-line-drift`.
2. r[annot.line.drift.derive] A block beside a `@derive` of another trait from the same package gets the same warning when the block has any member line. Warning: `derivation-line-drift`.
3. r[annot.line.drift.legal] Different member lines stay legal: configuration may differ per direction, such as encoding and decoding.

> **Why.** Most blocks of one library should agree, so a difference is
> worth a second look. Some differences are deliberate, as serde's
> `skip_serializing` shows.

### Facts

A **fact** is an ordinary value attached to a type, member, or variant for
derivations to read:

```text
fn key_for(style: Style, m: Member) -> string:
    match m.facts.find[Rename]():
        .Some(r) => r.name
        .None => style.prefix + m.name
```

1. r[annot.fact.descriptive] Facts are descriptive: a fact changes no generated call. Only a template's own code reads it.
2. r[annot.fact.type-level] A decorator before a data or enum declaration whose value's static type does not implement `Annotation` attaches a type-level fact, as `@style(prefix="user_")` does.
3. r[annot.fact.member] A member's or variant's declaration facts are its member metadata: the values that its decorators and `annotate` member blocks attach, in source order.
4. r[annot.fact.payload] A payload member's declaration facts are the values of the decorators before its payload parameter.
5. r[annot.fact.shared] Declaration facts are seen by every derivation of the type. A block's member lines edit them for that block only.
6. r[annot.fact.eval] A fact expression is evaluated once, at compile time, by the evaluator that annotation values use. It must be requirement-free.
7. r[annot.fact.read] A template reads the type-level facts through `T::facts()`, and a member's or variant's facts through its handle's `info.facts`.
8. r[annot.fact.default] A template falls back to its own default when a fact is absent. An absent or foreign fact is never an error.
9. r[annot.fact.unused] A type-level fact whose package supplies no template that the type derives gets a warning, reported on its decorator. Warning: `unused-derivation-fact`.

```text
@style(prefix="p_")  # warning: unused-derivation-fact
data Plain:
    id: i64
```

> **Why.** Configuration is data on the type, not a hook on the trait, so
> a derived trait stays dynamically safe and two libraries' facts never
> collide.

### Walk, Describe, And Build

The compiler generates three traversals for each derivation. A data type is
traversed as an enum with one variant.

#### Walk

1. r[annot.walk.match] Generated `walk` matches the value once and calls `w.variant(v)` for the value's variant.
2. r[annot.walk.members] It then calls `w.member(h, value)` for each member of that variant, in declaration order.
3. r[annot.walk.value] Each member value is read through the readonly `self`, so it has the member's read type.
4. r[annot.walk.error] The first `.Err` that `variant` or `member` returns ends the walk, and `walk` returns it. Otherwise `walk` returns `.Ok()`.
5. r[annot.walk.nested] A member is passed as one value. Its own members are reached only through its own type's implementations.

#### Describe

1. r[annot.describe.order] Generated `describe` calls `d.variant(v)` for every variant in declaration order, each followed by `d.member(h)` for that variant's members in declaration order.
2. r[annot.describe.no-value] `describe` holds no value.
3. r[annot.describe.error] The first `.Err` ends the traversal and is returned, as for `walk`.

#### Build

1. r[annot.build.variant] Generated `build` first calls `s.variant(choices)` once, with every variant in declaration order. A data type offers its one variant.
2. r[annot.build.next] It then calls `s.next(members)` with the chosen variant's members, repeatedly, until the source returns the end key.
3. r[annot.build.member] For each member key, it calls `s.member(h, previous)`, where `previous` is the value already read for that member, or `.None`.
4. r[annot.build.missing] After the end key, it calls `s.missing(h)` once for each member that the source never named.
5. r[annot.build.construct] It then constructs the chosen variant from one local per member, and returns it as `mut Self`.
6. r[annot.build.errors] The first `.Err` from a source method ends the build and is returned. Generated code never creates an error value of its own.
7. r[annot.build.foreign-key] A key that does not name one of the chosen variant's members is a checked runtime panic with category `structure-variant-mismatch`.
8. r[annot.build.shared] `build` never reads shared constructor data: the chosen variant's `->` expression fixes it.

### Members And Variants

1. r[annot.member.info] Each member has a `Member` value: its name, its zero-based position within its variant, its facts, its doc comment, and the `embedded` and `positional` flags.
2. r[annot.member.embedded] An embedded field is one member, named by its embedded type's final name, with `embedded` true.
3. r[annot.member.embedded.part] Its value is the part itself, not a copy, as [`data.part.alias`](08-data-and-enums.md#r-data.part.alias) states.
4. r[annot.member.no-flatten] The language never flattens an embedded part. Flattening is library policy, read from the `embedded` flag.
5. r[annot.member.positional] An unnamed payload parameter is a member named `_0`, `_1`, and so on, by position, with `positional` true.
6. r[annot.variant.info] Each variant has a `VariantInfo` value: its name, its zero-based index, its facts, its doc comment, `of_data`, and `shared`.
7. r[annot.variant.data] A data type's one variant has `of_data` true, and its name and doc comment are the type's.
8. r[annot.variant.shared] `shared` holds the variant's shared constructor data as `(name, value)` pairs, built once at compile time. An unnamed shared parameter is named `_0`, `_1`, and so on.
9. r[annot.variant.shared.no-handle] Shared constructor data is never a member: it is never passed as a handle.

See also: [Data Embedding](08-data-and-enums.md#data-embedding),
[Shared Enum Constructor Data](08-data-and-enums.md#shared-enum-constructor-data).

### Handles

A **handle** is a compiler-generated constant that names one member or
variant of a derivation's target.

1. r[annot.handle.constants] Each member has one constant handle of type `Field[S, F]`, and each variant one of type `Variant[S]`.
2. r[annot.handle.read-type] `walk` and `describe` pass handles whose `F` is the member's read type: for `hits: mut Counter`, it is `Counter`.
3. r[annot.handle.declared-type] `build` passes handles whose `F` is the member's declared type: for `hits: mut Counter`, it is `mut Counter`.
4. r[annot.handle.variance] `Field` is contravariant in `S` and covariant in `F`, so a declared-type handle weakens to its read-type handle.
5. r[annot.handle.get] `h.get(x)` accepts any `S`. Its type is the type that the field access `x.member` would have, for a member declared `F`.
6. r[annot.handle.get.views] So `get` on a declared-type handle returns the readonly view for a readonly `x`, and `F` for a `mut S`. No member is upgraded.
7. r[annot.handle.get.variant] `h.get(x)` on a payload member, when `x` holds another variant, is a checked runtime panic with category `structure-variant-mismatch`.
8. r[annot.handle.holds] `v.holds(x)` is true exactly when `x` holds the variant `v`.
9. r[annot.handle.default] `h.has_default()` is true when the member declares a default. `h.default()` evaluates that default, or returns `.None` when there is none.
10. r[annot.handle.escape] Handles are ordinary values and may escape the traversal that passed them.

```text
data Counter:
    count: i64

@derive(Duplicate)
data Stats:
    hits: mut Counter
    label: string

data CopySource[S]:
    old: mut S

impl[S] Source[S] for CopySource[S]:  # variant, next, and member elided
    type Error = never

    fn missing[F](mut self, h: Field[S, F]) -> Result[F, never]:
        .Ok(h.get(self.old))
```

> **Note.** A walker that holds a second value, such as a diff, checks
> `v.holds(other)` in `variant` before any `h.get(other)`. For example, a
> `Clone` trait may declare `clone(self)`, which reads the readonly views,
> and `clone_mut(mut self) -> mut Self`, whose source reads the declared
> types from a `mut` value, as `CopySource` does.

> **Note.** Handles are constants, and each generated call passes a
> constant dictionary, so a traversal allocates nothing per member.

### Walkers, Describers, And Sources

A walker, describer, or source is an ordinary type that implements
`Walker`, `Describer`, or `Source`:

```text
impl[S] Walker[S] for Encoder:
    type Error = never

    fn variant(mut self, v: Variant[S]) -> Result[void, never]:
        if !v.info.of_data:
            self.out = self.out + self.style.tag + "=" + v.info.name + ";"
        .Ok()

    fn member[F < Encode](mut self, h: Field[S, F], value: F) -> Result[void, never]:
        self.out = self.out + key_for(self.style, h.info) + "=" + value.encode() + ";"
        .Ok()
```

1. r[annot.walker.strengthen] An implementation of `Walker`, `Describer`, or `Source` may strengthen the bound on `member[F]`, as `F < Encode` above. A `Source` implementation may also strengthen the bound on `missing[F]`.
2. r[annot.walker.not-sealed] `Walker`, `Describer`, and `Source` are not sealed. Any package may implement them.
3. r[annot.walker.obligation] Every member that a derivation walks, describes, or builds must satisfy the strengthened bounds of the walker, describer, or source that the template passes.
4. r[annot.walker.obligation.error] The obligation is checked at the opt-in. A member that fails it is an error, reported at the opt-in and naming the member. Error: `member-not-derivable`.
5. r[annot.walker.generic-call] `member` and `missing` may be called through a generic walker, describer, or source type only by generated code. Such a call written in source is an error. Error: `generic-member-call`.
6. r[annot.walker.concrete-call] A call on a concrete walker, describer, or source type applies that type's own bounds.

```text
@derive(Show)  # error: member-not-derivable
data Label:
    text: string

fn forward[S, W < Walker[S]](w: mut W, h: Field[S, i64], value: i64) -> void:
    _ := w.member(h, value)  # error: generic-member-call
```

> **Why.** A strengthened bound becomes one obligation per member, checked
> where both the member types and the walker are known. A generic call
> could bypass that check.

### Derived Bounds

1. r[annot.bound.params] A derived implementation for a generic type gets `T < Trait` for each type parameter `T` that appears in a walked, described, or built member.
2. r[annot.bound.omitted] An omitted member contributes no bound.
3. r[annot.bound.recursive] Recursion is checked coinductively: while checking the members of `Tree[T]`, its own derived implementation is assumed to hold.
4. r[annot.bound.more] When a member needs more than `T < Trait`, as a `Set[T]` member needs `T < Hash`, the error suggests a derivation block whose header states the bounds. Error: `member-not-derivable`.

See also: [`trait.derive.bounds`](09-traits.md#r-trait.derive.bounds),
[Derived Newtypes](09-traits.md#derived-newtypes).

### Limits

1. r[annot.limit.in-place] There is no in-place traversal: generated code never assigns a member of an existing value. A derived merge returns a new value.
2. r[annot.limit.interfaces] A package interface carries each template's body, the walker, describer, and source bodies it names, and the functions its facts call.
3. r[annot.limit.specialize] Each (target, walker) pair is one specialization of the generated traversal.

> **Note.** Wire stability is documented, not checked. Reordering or
> renaming members or variants changes derived formats, `VariantInfo.index`,
> and derived `Ord`. A library may require explicit tags as facts.

### Undecided Parts

These parts of typed derivation are open design questions, listed in
[Open Issues](../future-work/OPEN_ISSUES.md#typed-derivation-tool-adapters-and-secrets).
An implementation must not guess them:

| Part | State |
| --- | --- |
| Fact check hook | The form of a fact type's compile-time check against its member, and whether it covers cross-member and type-level checks. |
| Non-escaping handles | Whether a future non-escaping trait design makes handles non-escaping. |
| Plan constants | A template may declare a constant computed once per derivation at compile time. Its syntax and the evaluator's limits are open, so no syntax for it exists. |
| Typed shared constants | Typed handles for shared constructor data. |
| `T -> U` mapping | Whether derivation between two types is in scope. |
| Name clashes | How `walk`, `describe`, and `build` interact with trait methods of the same name. `Structure::walk(self, w)` is always unambiguous. |
| Derived bound | Whether a derived bound names the trait or the walker's strengthened bound, where the two differ. |
| `default()` allocation | Whether `h.default()` may allocate for every member type. |
| Composing templates | How a walker forwards to another walker's `member`, which only generated code may call generically. |
| Function targets | Deriving for functions, and how the facets and annotators of this chapter relate to typed derivation. |
