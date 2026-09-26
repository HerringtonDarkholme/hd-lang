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
the compiler-intrinsic exception: it generates ordinary trait implementations
directly and does not invoke the annotation protocol. An ordinary decorator
must not silently change a declaration's name, type, behavior, or visibility.

## Common Shape Representation

The compiler exposes declaration structure as ordinary runtime shape values:

```text
shape(User)          # DataShape
shape(User.email)    # FieldShape
shape(JobStatus)     # EnumShape
shape(get_user)      # FnShape
```

The common representation includes at least:

- `TypeShape` for a concrete type;
- `DataShape` and ordered `FieldShape` values;
- `EnumShape`, ordered `VariantShape` values, and payload `FieldShape` values;
- `FnShape` and ordered `ParamShape` values;
- names, positions, declared runtime type descriptors, documentation, and
  attached member metadata.

Shape types are not parameterized by the reflected declaration. In particular,
the type is `DataShape`, not `DataShape[S]`. This avoids a special HList or
mapped-record type in the language. Aggregate mapped results are uniform
collections such as `list[(string, DatabaseColumn)]`, where `DatabaseColumn`
is an example user-defined facet result rather than a prelude type.

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
    Tuple(elements: list[TypeShape])
    Named(decl: DeclarationId, args: list[TypeShape])
    Newtype(base: TypeShape)
    Fn(
        params: list[TypeShape],
        result: TypeShape,
        suspending: bool,
        requirements: list[TypeShape],
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
    fields: list[FieldShape]

data VariantShape:
    id: DeclarationId
    name: string
    qualified_name: string
    source: SourcePosition
    position: i32
    doc: string?
    payload: list[FieldShape]

data EnumShape:
    id: DeclarationId
    name: string
    qualified_name: string
    source: SourcePosition
    doc: string?
    variants: list[VariantShape]

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
    params: list[ParamShape]
    result: TypeShape
    suspending: bool
    requirements: list[TypeShape]

trait ShapeMetadata:
    fn metadata[reified M](self) -> M?
```

Every concrete shape type implements sealed `ShapeMetadata`; user code cannot
add implementations. `metadata[M]()` performs the one narrow runtime type
lookup supported for heterogeneous annotation metadata and preserves the
attached value's declared permission. It does not add a general `Any`
downcast. `TypeShape.is_optional() -> bool` is also a compiler-provided readonly
method and is true exactly for `TypeShape.Optional`.

`TypeShape` does not yet encode mutable access, a dynamic trait value, `Any`, or
`Suspend[T]`. A `shape(Target)` request, annotation derivation, or generated
shape implementation whose target or recursively inspected member signature
requires one of those encodings is rejected with
`unrepresentable-type-shape`; the underlying declaration remains legal. This
is an interim rejection rule rather than an opaque or lossy descriptor.

`shape(Target)` otherwise materializes the appropriate shape value. A generic
target requires every type parameter needed by the target to be `reified`.
Shape values are readonly runtime values and may be passed, stored, and
inspected like other composite values.

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

An `@value` line immediately before a named or embedded data field or an enum
variant attaches member metadata. For a field `name: string`, `@max_len(80)`
expands to the `max_len(80)` entry in
`annotate User: name = [max_len(80)]`. The value must implement
`FieldMetadata[string]`; a variant value must implement `VariantMetadata`.

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
`DataAnnotator` can emit implementations through `Annotation::Info`.

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

Field metadata is contextually typed as `list[FieldMetadata[T]]`, where `T` is
the declared field type. Variant metadata uses the corresponding marker trait:

```text
trait FieldMetadata[T]
trait VariantMetadata
trait ParamMetadata[T]
```

Function parameter metadata is contextually typed as
`list[ParamMetadata[T]]`, where `T` is the declared parameter type. One
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
let email_metadata: list[FieldMetadata[string]] = [
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

annotate[reified T < Annotate[Validation]] Validation for list[T]:
    fn build(self, target: TypeShape) -> Validator:
        Validator.List(Validation::annotation_ref(T))
```

Annotation facets are open across exact targets and generic target families.
A generic `annotate` declaration uses the same generic binders, bounds,
coherence, and overlap rules as its lowered generic `impl`. There is no
unconstrained wildcard `annotate Validation for type` fallback; a family names
a concrete type pattern such as `list[T]`.

## Grammar

Module-level annotation declarations use this grammar:

```ebnf
annotation_decl = member_metadata_decl | facet_annotation_decl ;

member_metadata_decl = "annotate", qualified_name, ":",
                       annotation_member_suite ;

facet_annotation_decl = "annotate", [ generic_params ], annotation_facet,
                        "for", annotation_target, [ where_clause ], ":",
                        facet_annotation_suite ;

annotation_facet = type | closed_expression ;

annotation_target = type ;

annotation_member_suite = "pass", SUITE_END
                        | NEWLINE, INDENT,
                          metadata_assignment,
                          { metadata_assignment }, DEDENT
                        ;

metadata_assignment = identifier, "=", expression, NEWLINE ;

facet_annotation_suite = "pass", SUITE_END
                       | NEWLINE, INDENT,
                         facet_override,
                         { facet_override }, DEDENT
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

Generic parameters and `where` predicates have the same meaning as on an
ordinary generic implementation. For example,
`annotate[T] Validation for list[T]` occupies the same coherence slot as
`impl[T] Annotate[Validation] for list[T]`; an overlapping exact annotation is
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
        fields: list[(string, Self::FieldTarget)],
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
        fields: list[Self::FieldTarget],
    ) -> Self::VariantTarget

    fn build(
        self,
        target: EnumShape,
        variants: list[(string, Self::VariantTarget)],
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
        params: list[(string, Self::ParamTarget)],
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
    shape(User)

impl Annotate[Validation] for User:
    fn info() -> Validator:
        target := __user_shape()
        facet := Validation {}
        # Attached values are available through
        # target.fields[0].metadata[FieldMetadata[string]]().
        field := facet.map_field(
            target.fields[0],
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

Annotation blocks are package-global rather than lexical. If a library provides
an annotation for its target, a downstream package cannot override it. Only
the root application package may provide an orphan annotation for a target and
facet it does not own, and only when no library in the resolved graph provides
that exact pair. The root annotation occupies the one global coherence slot.
If a dependency version later supplies the same pair, dependency resolution
fails rather than silently changing which annotation is selected.

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
