# Annotations

Status: provisional design.

Annotations attach typed metadata to declaration shapes and derive ordinary
runtime information from those shapes. They do not alter a declaration's name,
type, behavior, or visibility, and they do not discover or register runtime
objects automatically.

The design principles are:

1. derivation is structural;
2. overrides are local;
3. annotation and metadata values are typed;
4. runtime information is produced explicitly when requested;
5. annotation evaluation is strict bottom-up;
6. the semantic foundation is ordinary traits, implementations, values, and
   compiler-provided shape values.

The surface `annotate` forms are syntax sugar over that foundation. Decorator
syntax is not part of the current design and may be added later as equivalent
locality sugar.

## Common Shape Representation

The compiler exposes declaration structure as ordinary runtime shape values:

```text
shape(User)          # StructShape
shape(User.email)    # FieldShape
shape(JobStatus)     # EnumShape
shape(get_user)      # FnShape
```

The common representation includes at least:

- `TypeShape` for a concrete type;
- `StructShape` and ordered `FieldShape` values;
- `EnumShape`, ordered `VariantShape` values, and payload `FieldShape` values;
- `FnShape` and ordered `ParamShape` values;
- names, positions, declared runtime type descriptors, documentation, and
  attached member metadata.

Shape types are not parameterized by the reflected declaration. In particular,
the type is `StructShape`, not `StructShape[S]`. This avoids a special HList or
mapped-record type in the language. Aggregate mapped results are uniform
collections such as `Dict[string, DatabaseColumn]`.

Shapes expose structure for generic handling but do not permit mutation of the
source declaration.

## Annotation Protocol

An annotation facet chooses one uniform output type:

```text
trait Annotation:
    type Info

trait Annotate[A: Annotation]:
    fn info() -> A::Info
```

This chapter provisionally extends traits and implementations with associated
type declarations:

```ebnf
associated_type_decl = "type", identifier,
                       [ "=", type ], NEWLINE ;

associated_type_projection = ( qualified_name | "Self" ), "::", identifier ;
```

Within a trait or implementation, `Self::Name` projects an associated type from
the current implementor. `A::Info` projects through a generic annotation type.
This production extends the core `type` grammar in annotation-aware code.
Associated type declarations also extend `trait_member` and implementation
bodies; implementations assign a concrete type with `type Name = T`.
The annotation protocol also provisionally permits receiverless associated
functions such as `fn info() -> A::Info` in these annotation traits and
implementations. This is a scoped extension to the core receiver requirement,
not a stable general-purpose associated-function feature.

`trait Annotation` declares `Info`; an implementation assigns it:

```text
impl Annotation for Validation:
    type Info = Validator
```

`Annotate[A] for T` means that target `T` can produce `A::Info`. It is a normal
trait conformance for constraint and coherence purposes.

## Two `annotate` Forms

### Member Metadata

`annotate Target` attaches metadata values to existing fields or variants:

```text
struct User:
    display_name: string

annotate User:
    display_name = [min_len(1), max_len(80)]
```

The target is a declared struct or enum, not an arbitrary type expression.
Every left-hand name must identify an existing direct member. The block cannot
add, rename, remove, or change the type of a member.

Field metadata is contextually typed as `list[FieldMetadata[T]]`, where `T` is
the declared field type. Variant and function parameter metadata use the
corresponding marker traits:

```text
trait FieldMetadata[T]
trait VariantMetadata
trait ParamMetadata[T]
```

Metadata objects are ordinary values. Constructors and helper functions are
equivalent ways to make them:

```text
struct MaxLen:
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

`annotate Facet for Target` generates the same conformance slot as
`impl Annotate[Facet] for Target`, while allowing structure-aware overrides:

```text
annotate Validation for User: pass
```

`pass` requests ordinary derivation with no field, variant, parameter, or whole
result override.

For an exact primitive, nominal type, or collection instantiation, the block
may provide `build` directly:

```text
annotate Validation for string:
    fn build(self, shape: TypeShape) -> Validator:
        Validator.String

annotate Validation for Email:
    fn build(self, shape: TypeShape) -> Validator:
        Validator.Email
```

Annotation facets are open across exact target types. There is no wildcard
`annotate Validation for type` fallback in the current design, and generic
families such as every `list[T]` do not yet have settled syntax.

## Grammar

This chapter adds module-level annotation declarations:

```ebnf
annotation_decl = member_metadata_decl | facet_annotation_decl ;

member_metadata_decl = "annotate", qualified_name, ":",
                       annotation_member_suite ;

facet_annotation_decl = "annotate", type, "for", annotation_target, ":",
                        facet_annotation_suite ;

annotation_target = type | qualified_name ;

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

annotation_runtime_access = qualified_name, "::", identifier,
                            argument_clause ;
```

The facet name is parsed as a type implementing `Annotation`. Target resolution
distinguishes a type target from a function target. Function parameter
assignment overrides are deferred; a function facet currently uses parameter
shape information or a whole `build` override. A future parameter-metadata
attachment syntax may use `ParamMetadata[T]`, but it is not defined here.

## Aggregate Annotators

A struct facet maps each field to one uniform `FieldTarget`, then builds the
facet's `Info`:

```text
trait StructAnnotator: Annotation:
    type FieldTarget

    fn map_field(
        self,
        field: FieldShape,
        type_metadata: AnnotationRef[Self::Info],
    ) -> Self::FieldTarget

    fn build(
        self,
        shape: StructShape,
        fields: Dict[string, Self::FieldTarget],
    ) -> Self::Info
```

An enum first maps every payload field, then maps each variant, then builds the
enum result:

```text
trait EnumAnnotator: Annotation:
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
        shape: EnumShape,
        variants: Dict[string, Self::VariantTarget],
    ) -> Self::Info
```

A function maps parameters and builds a function result:

```text
trait FuncAnnotator: Annotation:
    type ParamTarget

    fn map_param(
        self,
        param: ParamShape,
    ) -> Self::ParamTarget

    fn build(
        self,
        shape: FnShape,
        params: Dict[string, Self::ParamTarget],
    ) -> Self::Info
```

`FieldTarget`, `VariantTarget`, `ParamTarget`, and `Info` are uniform types.
They do not vary at the type level with the original member type. An annotator
may inspect each shape's runtime type descriptor and member metadata to choose a
value, but the core type system does not model a type-level function from field
type to mapped output type.

## Structural Overrides

Within `annotate Facet for Struct`, an assignment to a field name replaces that
field's default `map_field` result and must have type
`Facet::FieldTarget`:

```text
annotate UI for User:
    avatar = profile_image(size=40)
```

The assignment cannot target a missing or promoted field. Enum variant
overrides follow the same rule with `VariantTarget`.

A local `fn build` definition replaces aggregate `build` for that exact
facet/target pair. It receives the completed uniform map and may rewrite the
whole result. Other map steps still occur unless a later design explicitly adds
a full-derivation replacement hook.

Function parameter assignment overrides are not included in the current
design. Function-specific customization uses `ParamMetadata[T]` or a whole
function `build` override.

## Bottom-Up Evaluation

Annotation materialization follows a strict order:

1. resolve annotation information for each member's declared type;
2. attach and evaluate that member's field, variant, or parameter metadata;
3. call the enclosing annotator's `map_field`, `map_variant`, or `map_param`;
4. apply exact structural result overrides;
5. call the enclosing `build`, or the exact local `build` override.

For enums, every payload field is mapped before its containing variant; every
variant is mapped before enum `build`.

An enclosing annotator receives ordinary shape and mapped values. It may
interpret or replace child metadata values in its own result, but it cannot
retroactively alter the source declaration or another independent facet.

## Runtime Materialization

Derived information is requested explicitly:

```text
validator := Validation::annotation(User)
table := DatabaseSchema::annotation(User)
tool := Tool::annotation(get_user)
```

This is the current preferred spelling, but remains provisional. It returns an
ordinary value of the facet's `Info` type. A function annotation does not make
the function public, register it, or enable runtime discovery:

```text
tool_registry.register(Tool::annotation(get_user))
```

Registration is explicit library behavior.

Annotation builders and metadata expressions execute at runtime in a restricted
annotation-initialization phase, not as unrestricted compiler evaluation. They
must be pure, deterministic, non-suspending, and dependency-free: no `$`
context, bang calls, IO, clock, randomness, network, database, or escaping
mutation. Completed results are memoized per package.

The compiler's special role is to:

- validate target and member names;
- contextually type metadata and override values;
- check the corresponding annotation traits;
- generate ordinary shape construction and trait implementation code;
- arrange cycle-aware resolution and package initialization.

The lowering does not otherwise change the runtime semantics of annotation
values.

## Desugaring Model

The following is illustrative, not normative generated source. The compiler may
lower differently while preserving the checked behavior.

```text
struct User:
    email: string

annotate User:
    email = [max_len(320), contains("@")]

annotate Validation for User: pass
```

Conceptually becomes shape metadata plus an implementation:

```text
fn __user_shape() -> StructShape:
    StructShape {
        name: "User",
        fields: [
            FieldShape {
                name: "email",
                type: type(string),
                metadata: [max_len(320), contains("@")],
            },
        ],
    }

impl Annotate[Validation] for User:
    fn info() -> Validator:
        shape := __user_shape()
        field := Validation.map_field(
            shape.fields[0],
            Validation::annotation_ref(string),
        )
        Validation.build(shape, {"email": field})
```

The generated names and exact runtime calls are implementation details. This
desugaring exists to illustrate that `annotate` supplies typed structure-aware
sugar rather than a separate semantic object system.

## Recursive Annotations

Annotation resolution is keyed by `(facet, concrete target)`. A facet can embed
references to child information using the general runtime type
`AnnotationRef[T]`:

```text
struct FieldValidator:
    name: string
    target: AnnotationRef[Validator]
```

When resolution first enters a key, it marks the key active. Resolving a
different key normally returns a ready reference after construction. Re-entering
an active key returns a deferred reference to that key. Once the outer build
completes, the deferred reference resolves through the memoized registry entry.

This automatic cycle detection applies uniformly to struct and enum annotation
graphs. Facets do not need a custom `Validator.Ref` variant or a separate
`Annotation.reference` hook. A facet opts into recursive output by storing
`AnnotationRef[Info]` where child information appears.

For example, mutually recursive `Folder` and `Entry` validation can use the same
`AnnotationRef[Validator]` for struct fields and enum payloads. Non-recursive
edges produce ready references; only back edges are deferred.

Manual `lazy` member metadata remains a possible future escape hatch, but
automatic cycle detection is the default direction. Such metadata would delay
annotation information only; it would not make the program field itself lazy.

## Coherence And Package Rules

At most one annotation exists for an exact `(facet, target)` pair in the whole
resolved package graph. `annotate Facet for Target` and an explicit
`impl Annotate[Facet] for Target` occupy the same coherence slot.

Annotation blocks are package-global rather than lexical. If a library provides
an annotation for its target, a downstream package cannot override it. A
downstream application may provide package-local information for an imported
target only when no library in the resolved graph already provides that exact
facet/target pair.

There is no implicit library default annotation. A target has facet information
only when the facet/target conformance is explicitly present or structural
derivation is explicitly requested with `annotate Facet for Target`.

## Missing Child Information

An aggregate may contain a child type with no annotation for the requested
facet. Some facets should reject this statically; others should omit or replace
the child.

`map_field` runs in the runtime annotation phase, so it cannot itself choose a
compile-time error. The current candidate is a statically visible
`MissingAnnotationPolicy` associated with the aggregate annotator, selecting a
policy such as `Require` or `Ignore` before runtime mapping begins.

The policy type, default, granularity, and behavior for enum payloads and
function parameters are not decided. This chapter records the requirement but
does not adopt one policy.

## Full Validation Example

The repository file [`validation.hd`](../../validation.hd) is the current full
worked example. It demonstrates:

- exact primitive and nominal type annotations;
- field and variant metadata checked through ordinary traits;
- struct and enum annotators;
- uniform mapped dictionaries and lists;
- runtime materialization;
- recursive struct/enum graphs through `AnnotationRef`.

It is an executable-design sketch, not yet a conformance test, because generic
`impl` constraints, associated types, and annotation runtime APIs remain
provisional.

## Open Issues

1. Final shape fields and APIs.
2. Generic exact-target syntax for families such as every `list[T]`.
3. Final materialization spelling, currently `Facet::annotation(Target)`.
4. Missing-child policy and its static selection mechanism.
5. Generic `impl` and constraint syntax needed by reusable metadata helpers.
6. Whether type-level and aggregate information always share `Annotation::Info`.
7. Whole-derivation replacement beyond a local aggregate `build` override.
8. Whether future `@expr` decorator sugar is useful. If added, it must lower
   exactly to `annotate Target` member metadata and add no new semantics.
9. Manual lazy-reference metadata and which facets may consume it.
10. Declaration and value syntax for zero-sized facet types such as
    `Validation`. The examples treat the facet as an ordinary stateless value,
    but the core language has not yet selected an empty-struct spelling.
