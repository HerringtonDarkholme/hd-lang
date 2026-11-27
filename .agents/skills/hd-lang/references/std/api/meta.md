# `std.meta`

Generated from `hd doc std.meta`. Run `hd doc std.meta.NAME` in a package for one item's full text.

## `Marker`

```text
pub trait Marker < ?Copy
```

A trait with no members that says what may cross into one kind of domain.

## `Send`

```text
pub trait Send < Marker
```

A value that may be copied, moved or lent to another thread of the same memory.

## `Sync`

```text
pub trait Sync < Marker
```

A value that threads may read at once: it holds no counted buffer, so

## `Wire`

```text
pub trait Wire < Marker
```

A value that crosses into separate memory as a self-contained tree.

## `facts_of`

```text
pub fn facts_of[F](f: F) -> Facts
```

The facts attached to the module-level function `f`; a compiler intrinsic.

## `Target`

```text
pub enum Target
```

The kind of declaration a fact value may be attached to.

## `Annotate`

```text
pub struct Annotate
```

- `pub kinds: List[Target]`: The target kinds the fact type allows.

## `annotate`

```text
pub fn annotate[T = &dyn Any](kinds...: List[Target]) -> Annotate
```

The `@annotate` decorator: it attaches an `Annotate` fact listing the target `kinds`.

## `Inspectable`

```text
pub trait Inspectable < ?Copy
```

The sealed trait of types with a runtime identity, which the compiler supplies.

## `TypeId`

```text
pub struct TypeId
```

The runtime identity of an inspectable type.

- `pub fn of[T < Inspectable]() -> TypeId`: The `TypeId` of `T`.

## `Facts`

```text
pub struct Facts
```

The facts attached to one type, member, variant, or module-level function, in source order.

- `pub items: List[*dyn Inspectable]`: The fact values.
- `pub fn find[F < Inspectable](self) -> F?`: Returns the fact whose concrete type is `F`, or `.None`.

## `Member`

```text
pub struct Member
```

What the compiler tells a template about one member: name, position, facts, doc comment, and shape.

- `pub name: string`: The member's declared name.
- `pub position: usize`: The member's position in its type.
- `pub facts: Facts`: The member's facts.
- `pub doc: string?`: The member's doc comment, in `.Some`, or `.None`.
- `pub embedded: bool`: Whether the member is embedded.
- `pub positional: bool`: Whether the member is positional.
- `pub self_ref: SelfRef`: How the member's type refers to the type being derived, which the compiler computes.

## `VariantInfo`

```text
pub struct VariantInfo
```

What the compiler tells a template about one variant.

- `pub name: string`: The variant's name.
- `pub index: usize`: The variant's index in its enum.
- `pub facts: Facts`: The variant's facts.
- `pub doc: string?`: The variant's doc comment, in `.Some`, or `.None`.
- `pub type_doc: string? = .None`: The owning type's documentation, identical for all its variants.
- `pub of_data: bool`: Whether the variant belongs to a data type.
- `pub positional: usize = 0`: The variant's number of positional payload members.
- `pub named: usize = 0`: The variant's number of named payload members.
- `pub included_positional: usize = 0`: The positional payload members included in this derivation's view.
- `pub shared: List[(string, *dyn Inspectable)]`: The facts the variants share.
- `pub self_ref: SelfRef`: How the variant's fields refer to the type being derived, which the compiler computes.

## `SelfRef`

```text
pub enum SelfRef
```

How a member or variant refers to the type being derived: `Absent`, `Optional`, or `Required`.

## `Field`

```text
pub type Field[HdS < ?Copy, HdF < ?Copy] = *FieldRecord[HdS, HdF]
```

A member's handle: a reference to its `FieldRecord`, which holds its `Member` information plus the calls a template needs.

## `FieldRecord`

```text
pub struct FieldRecord[HdS < ?Copy, HdF < ?Copy]
```

What a member's handle refers to: its `Member` information plus the calls a template needs.

- `pub info: Member`: The member's information.
- `pub fn get['a](&self, s: &'a HdS) -> &'a HdF`: Reads the member's value from `s`.
- `pub fn has_default(&self) -> bool`: Whether the member has a default value.
- `pub fn default(&self) -> HdF?`: The member's default value, in `.Some`, or `.None`.
- `pub fn fact[HdM < Inspectable](&self) -> HdM?`: The member's fact whose concrete type is `HdM`, or `.None`.

## `Variant`

```text
pub type Variant[HdS < ?Copy] = *VariantRecord[HdS]
```

A variant's handle: a reference to its `VariantRecord`, which holds its `VariantInfo` plus the check that a value holds it.

## `VariantRecord`

```text
pub struct VariantRecord[HdS < ?Copy]
```

What a variant's handle refers to: its `VariantInfo` plus the check that a value holds it.

- `pub info: VariantInfo`: The variant's information.
- `pub fn holds(&self, s: &HdS) -> bool`: Whether `s` holds this variant.

## `Key`

```text
pub type Key[HdS < ?Copy] = *KeyRecord[HdS]
```

A found member's key: a reference to its `KeyRecord`.

## `KeyRecord`

```text
pub struct KeyRecord[HdS < ?Copy]
```

What a member's key refers to: the member's information and target type.

## `Members`

```text
pub type Members[HdS < ?Copy] = *MembersRecord[HdS]
```

The members of the type being derived: a reference to their `MembersRecord`.

## `MembersRecord`

```text
pub struct MembersRecord[HdS < ?Copy]
```

What a member directory refers to: the members, which a template looks up by position or by a match.

- `pub infos: List[Member]`: The members' information, in order.
- `pub fn id(&self) -> usize`: The directory's id: one `usize` per member directory in the program, the same at every use of it, which a reader keys its per-type tables by.
- `pub fn at(&self, position: usize) -> Key[HdS]?`: The key of the member at `position`, or `.None`.
- `pub fn find(&self, matches: fn(Member) -> bool) -> Key[HdS]?`: The key of the first member that `matches` accepts, or `.None`.

## `Walker`

```text
pub trait Walker[S < ?Copy] < ?Copy
```

The protocol that walks a value variant and member by member.

## `Describer`

```text
pub trait Describer[S < ?Copy] < ?Copy
```

The protocol that describes a type variant and member by member, without a value.

## `Source`

```text
pub trait Source[S < ?Copy] < ?Copy
```

The protocol that builds a value: it chooses a variant and supplies each member.

## `FillsDefaults`

```text
pub struct FillsDefaults
```

A source implementation's fact: `build` fills each member that declares a default from that default.

## `fills_defaults`

```text
pub fn fills_defaults() -> FillsDefaults
```

Marks a source implementation whose `build` fills each member that declares a default from it.

## `Skips`

```text
pub struct Skips[F]
```

A walker's, describer's, or source's fact: a member that carries a fact of type `F` is omitted.

## `skips`

```text
pub fn skips[F]() -> Skips[F]
```

Marks a walker, describer, or source implementation that omits each member carrying a fact of type `F`.

## `Location`

```text
pub struct Location
```

Where a call begins in source: the file as diagnostics name it, and the

- `pub fn caller() -> Location`: The location of the call of the enclosing `@track_caller` function.
- `pub fn caller_args() -> List[CallArg]`: The source text and span of each argument of the call of the
- `pub fn in_root_package(self) -> bool`: Whether the call site is in the user's own code.

## `CallArg`

```text
pub struct CallArg
```

One argument of a call: its source text, where it starts and ends, and

- `pub name: string?`
