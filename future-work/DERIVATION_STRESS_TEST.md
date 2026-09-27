# Typed Derivation: Stress Test Of The M1-M11 Design

Status: design review, 2026-09-27. Nothing here is accepted language
behavior, and nothing here changes the design record, the specification, or
the prototype. Every design choice below is a question for the owner.

The design under test is the one recorded in
[Typed Derivation](TYPED_DERIVATION.md): the
[Current Design: Full Example (M1-M11)](TYPED_DERIVATION.md#current-design-full-example-m1-m11)
and the [Current Rules (M1-M11)](TYPED_DERIVATION.md#current-rules-m1-m11).
Decisions 1-12 (Design G and its refinements) are treated as superseded
wherever they conflict, so features that only they provided (the value-free
`describe[D < Describer]`, inferred bounds, `@derivable`, the standalone
`derive X for T`, the derived-function cache) are **not** assumed here unless
a use case shows that one of them is needed again.

JSON is the running example of the design record. This report tests the
design against 21 other derivation families, writes the library side and the
user side of each in hd, and records exactly where each one stops working.

## Contents

1. [Method](#method)
2. [Summary](#summary)
3. [The Surface Being Tested](#the-surface-being-tested)
4. [Use Cases](#use-cases) (1-21)
5. [Problems, Ranked](#problems-ranked)
6. [Comparison With Other Languages](#comparison-with-other-languages)
7. [Parse Log](#parse-log)

## Method

For each use case:

- **Library side:** the trait, the template `impl[T] Trait for T by
  Structure`, and the visitor or source impls, written against the
  `std.structure` surface below.
- **User side:** tier 1 (an annotation), tier 2 (a `by Structure` block with
  member lines), and tier 3 (hand-written), where each applies.
- **Verdict:** *works*, *works with friction*, or *breaks*, and the design
  element at fault.

The code follows the specification's syntax, not the design record's
spelling where they differ: data literals use braces (`Style { case: .Camel }`),
named arguments use `=` (`json(case=.Camel)`), a mutable local is written
`let v: mut Encoder = ...`, and no `mut` marker is written at argument sites
(chapters 05 and 07). Inside a template the target is spelled `T::`, because
`Self::visitor()` is not a primary expression in the chapter-02 grammar (see
problem P18). M3 member lines (`cache = pass`) are new syntax; blocks that
use them start with `# Hypothetical syntax`. The [Parse Log](#parse-log)
lists every block's result with the reference parser.

## Summary

| # | Use case | Verdict | Design element at fault |
| --- | --- | --- | --- |
| 1 | `PartialEq`, `Eq`, `PartialOrd`, `Ord`, `Hash` | **Breaks** (only `Hash` works) | `visit` sees one value; no two-value traversal (P1). A marker template (`Eq`) makes no call, so nothing is checked (P4). Tier 1 cannot pick a subset of `std.cmp` (P13). |
| 2 | Debug, Display, pretty printing | Works with friction | The `visitor()` hook makes `Display` not dynamically safe, which breaks `Error < Display` (P5). Redaction cannot drop a member's obligation (P9). |
| 3 | `Default`; `Clone`; `T -> U` mapping | Default: friction. Clone, mapping: **breaks** | A source cannot fall back to the declared default (P3). No value-to-value traversal (P1). |
| 4 | Builder | **Breaks** | Derivation fills existing traits only; no generated methods or companion types (P10). No declared defaults (P3). |
| 5 | Database rows (with and without I/O) | Works with friction; per-member I/O **breaks** | `describe()` has no member types (P2). `build` has an empty row and cannot suspend (P8). `= pass` repeated per trait (P13). |
| 6 | JSON Schema, OpenAPI, GraphQL, MCP input schema | **Breaks** | `describe() -> Shape` gives a `TypeShape` per member, not the member type's own impl. Nested types cannot be described at all, and a schema built from shapes disagrees with the members' own derivations (P2). |
| 7 | Protobuf-style codec | Works with friction | Declaration-order `build` forces a pre-scan (P7). Tag facts are checked only at run time (P14). Unknown-field capture is a convention. |
| 8 | CLI parsing | Parse: friction. Help and defaults: **breaks** | P2 (help needs value-free types), P3 (defaults). |
| 9 | `Arbitrary` with shrinking | Generate: friction. Shrink: **breaks** | No value-to-value traversal (P1). `Variant` carries nothing that detects recursion. |
| 10 | Validation | Works with friction | Facts are not typed against the member (P14). An override replaces the template method and cannot extend it (P16). |
| 11 | Error enums: `From`, messages, `Error` | **Breaks** | A template is one trait instantiation, but `From[P]` needs one impl per variant (P12). `cause()` needs a per-member obligation change (P9). The Display hook conflicts with `Error` (P5). |
| 12 | Diff and patch | **Breaks** | P1 (two values in, value to value), P10 (patch type). |
| 13 | Generic types, phantoms | **Breaks** for tier 1; tier 2 unspecified | M1-M11 has no rule for the derived impl's bounds (P6). |
| 14 | Common enum fields, payload-free enums, newtypes, embedding, private members, foreign types, `mut` fields | Mixed: see the case | Member model unspecified (P11). Foreign types hit `orphan-impl` (P15). `build` returns readonly `Self` (P17). |
| 15 | json + db + validation + cli on one type | Works with friction | Facts stay separate. Any skipped member forces tier 2 for every trait that visits it (P13). `visitor()` names collide (P5). |
| 16 | Function targets (MCP tools) | **Breaks** (blocked) | No function structure (FN_TYPE). Also needs P2, P3, and a row parameter the template can name (P8). |
| 17 | Performance claims | Holds for generated code only | Library code allocates per call (P7). `visit` must be specialized per visitor type (P19). |
| 18 | Package evolution | Works with friction | Silent wire changes on reorder or rename. A library that adds a trait changes downstream impls (P13). Interfaces must carry template and annotation bodies (P19). |
| 19 | Mock providers for traits | Out of scope | `Structure` covers data and enums only. |
| 20 | Stable fingerprints (`std.fingerprint`) | Works | Same as `Hash`; stability depends on P20. |
| 21 | Registered boundary adapters | **Breaks** if moved onto `Structure` | P2 (a WIT type needs a value-free description). |

What works cleanly is exactly the two shapes the design was built around:
consume **one** value member by member (encode, hash, fingerprint, debug
printing), and produce **one** value from an external input (decode, parse,
generate). Every use case that needs zero values (schemas, help text), two
values (equality, ordering, diff), or a value in and a value out (clone,
patch, shrink, conversion) breaks, and so does every use case that needs a
member's declared default at run time.

## The Surface Being Tested

The design record fixes `Structure`, `Visitor`, and `Source`. It uses
`Member`, `Variant`, and `Shape` without declaring them. This report assumes
the smallest declarations the record's code needs, plus two fields that
several use cases would need (`doc`, `member_count`). Where a use case needs
more than this, the report says so instead of assuming it.

```text
# std.structure, as in the design record
pub trait Structure:                      # sealed; only inside `by Structure` templates
    fn describe() -> Shape
    fn visit[V < Visitor](self, v: mut V) -> Result[(), V::Error]
    fn build[S < Source](s: mut S) -> Result[Self, S::Error]

pub trait Visitor:                        # an impl may strengthen member's bound (M9)
    type Error
    fn member[F](mut self, m: Member, value: F) -> Result[(), Self::Error]
    fn variant(mut self, v: Variant) -> Result[(), Self::Error]

pub trait Source:
    type Error
    fn member[F](mut self, m: Member) -> Result[F, Self::Error]
    fn variant(mut self, choices: List[Variant]) -> Result[Variant, Self::Error]

# Assumed by this report. The record uses m.name, m.facts, v.name, v.index.
pub data Member:
    pub name: string
    pub position: i32
    pub doc: string?                      # assumed; chapter 01 `##` comments
    pub facts: Facts

pub data Variant:
    pub name: string
    pub index: i32
    pub member_count: i32                 # assumed
    pub facts: Facts

pub data Facts:
    items: List[Fact]

impl Facts:
    pub fn find[reified M](self) -> M?:
        pass
```

`Shape` is taken to be chapter 14's `DataShape` or `EnumShape` for the
target: names, positions, docs, and a `TypeShape` per member. The record does
not say whether `describe()` reflects the calling impl's member lines (for
example whether a `cache = pass` member appears). That is question Q2-c.

Two properties of hd matter throughout:

- **Per-shape compilation** (04 Shapes and Generic Code): an ordinary
  generic function is compiled once per machine shape, and bounds are
  passed as dictionaries. M9 rule 3 already requires templates to call
  `visit` and `build` with a concrete visitor or source, so those calls are
  specialized per (type, visitor) pair.
- **Readonly results** (04 Parameters And Results): a function declared to
  return `T` exposes readonly `T`, even for a fresh value.

## Use Cases

### 1. Comparison And Hashing

Today `@derive` accepts only `PartialEq`, `Eq`, `PartialOrd`, `Ord`, and
`Hash`, with rules in 09 Comparison Traits: every field (embedded and common
enum fields included) in declaration order, variants by declaration order,
and a `T < Trait` bound per used parameter. TQ-13 asks for one protocol that
`std` also uses, so these five are the first test.

**Hash works.** It consumes one value:

```text
use std.structure.{Member, Variant, Visitor}

pub data HashStyle: pass

pub fn hash() -> HashStyle:
    HashStyle {}

pub trait Hash:
    fn hash_style() -> HashStyle:
        HashStyle {}
    fn hash(self, state: mut Hasher) -> void

data HashVisitor:
    state: mut Hasher

impl Visitor for HashVisitor:
    type Error = never

    fn member[F < Hash](mut self, m: Member, value: F) -> Result[(), never]:
        value.hash(self.state)
        .Ok(())

    fn variant(mut self, v: Variant) -> Result[(), never]:
        v.index.hash(self.state)
        .Ok(())

impl[T] Hash for T by Structure:
    fn hash(self, state: mut Hasher) -> void:
        let visitor: mut HashVisitor = HashVisitor { state: state }
        self.visit(visitor)
        pass
```

**PartialEq breaks.** `eq(self, other: Self)` needs member `m` of *both*
values with the same static type `F`. `visit` hands the visitor one value per
member, and nothing gives the visitor the matching member of `other`:

```text
data EqVisitor[T]:
    other: T
    equal: bool

impl[T] Visitor for EqVisitor[T]:
    type Error = never

    fn member[F < PartialEq](mut self, m: Member, value: F) -> Result[(), never]:
        # needs `other`'s member m as an F. No operation produces it:
        # Member is untyped, and only generated code reads members.
        pass

    fn variant(mut self, v: Variant) -> Result[(), never]:
        # needs `other`'s variant too, to answer "different variants"
        pass
```

The only encoding available today visits `other` first into a
`List[Inspectable]` and then visits `self`, recovering each element with
`downcast_val[F]`. That needs `F < PartialEq + Inspectable` on every member
(a type declared in a block suite is not inspectable), boxes every scalar
member (04 Value Categories), and turns a type mismatch into a run-time
`.None`. It is not a derivation anyone would ship.

`PartialOrd` and `Ord` break the same way, and they also need early exit at
the first non-equal member. That exit has to go through `Visitor::Error`,
so "found an ordering" travels on the error channel.

**Eq is accepted without checking anything.** `Eq` is a marker with no
methods. Its template has no body, so it calls neither `visit` nor `build`.
Under M9 rule 3 the member obligation comes only from those calls, so no
obligation is created:

```text
impl[T] Eq for T by Structure          # std.cmp: a marker template, no body

data Reading:
    celsius: f64

impl PartialEq for Reading by Structure    # cannot be written today (P1)
impl Eq for Reading by Structure           # accepted: no call, so no `f64 < Eq` obligation
```

09 says floating-point values do not satisfy `Eq`. The derived impl above
claims they do, and a `Map[Reading, V]` then misbehaves on NaN.

**User side.**

```text
use std.cmp
use std.hash

@cmp.cmp
data Point:
    x: i64
    y: i64

@hash.hash
data Key:
    tenant: string
    id: i64

data Money:
    cents: i64

impl hash.Hash for Money:
    fn hash(self, state: mut Hasher) -> void:
        self.cents.hash(state)
```

`@cmp.cmp` derives, by rule 5, every `std.cmp` template whose hook returns
`CmpStyle`: all four comparison traits at once. A type that should be
`PartialEq` only, such as `Reading`, cannot use tier 1 and needs one tier-2
line per trait. The current `@derive(PartialEq, Eq, Hash)` becomes three
tier-2 lines or three annotations with three style types.

**Verdict: breaks.** `Hash` works. `PartialEq`, `PartialOrd`, and `Ord`
need a two-value traversal (P1). `Eq` is silently unchecked (P4). Tier 1 is
too coarse for `std.cmp` (P13).

### 2. Debug, Display, And Pretty Printing

**Library side.** A one-value visitor with a formatter that tracks
indentation. Nesting works because a member's own `fmt` writes into the same
formatter (M7):

```text
use std.structure.{Member, Variant, Visitor}

pub data DebugStyle:
    pretty: bool = false

pub fn debug(pretty: bool = false) -> DebugStyle:
    DebugStyle { pretty: pretty }

pub data Redact: pass

pub fn redact() -> Redact:
    Redact {}

pub trait Debug:
    fn debug_style() -> DebugStyle:
        DebugStyle {}
    fn fmt(self, out: mut Formatter) -> void

data DebugVisitor:
    out: mut Formatter
    opened: bool

impl Visitor for DebugVisitor:
    type Error = never

    fn variant(mut self, v: Variant) -> Result[(), never]:
        self.out.write(v.name)
        .Ok(())

    fn member[F < Debug](mut self, m: Member, value: F) -> Result[(), never]:
        if !self.opened:
            self.out.open("(")
            self.opened = true
        self.out.label(m.name)
        match m.facts.find[Redact]():
            .Some(_) => self.out.write("<redacted>")
            .None => value.fmt(self.out)
        self.out.separator()
        .Ok(())

impl[T] Debug for T by Structure:
    fn fmt(self, out: mut Formatter) -> void:
        out.set_pretty(T::debug_style().pretty)
        if T::describe().is_data():
            out.write(T::describe().name)
        let visitor: mut DebugVisitor = DebugVisitor { out: out, opened: false }
        self.visit(visitor)
        if visitor.opened:
            out.close(")")
```

The visitor opens the parenthesis lazily, so a payload-free variant prints
`Queued`, not `Queued()`. For data types the template writes the type name,
because `visit` reports variant names only.

**User side.**

```text
use dep.fmt

@fmt.debug(pretty=true)
data Session:
    user: string
    @fmt.redact()
    token: Token          # Token must still implement fmt.Debug
    expires: i64
```

**Problems.**

1. **Redaction cannot drop the obligation.** The redacted member is never
   printed, but `Token < fmt.Debug` is still required, because facts never
   change generated calls (M6) and `= pass` removes the member from the
   output entirely. "Print the label, hide the value, and do not require
   `Debug`" is not expressible without changing the member's type (P9).
2. **Display cannot carry the hook.** If `std.format` makes `Display`
   derivable in the same way, `Display` gains `fn display_style() -> ...`.
   An associated function makes a trait not dynamically safe (09 Dynamic
   Trait Values). `std.error.Error < Display + Inspectable` must be
   dynamically safe, so a derivable `Display` breaks the erased application
   error (P5).
3. Cycles are not detected, as for today's derived equality (09).

**Verdict: works with friction** for `Debug`; `Display` needs P5.

### 3. Default, Deep Copy, And Member-Wise Transformation

**Default, library side.** A source that asks each member type for its
default:

```text
use std.structure.{Member, Source, Variant}

pub data DefaultStyle: pass

pub fn default() -> DefaultStyle:
    DefaultStyle {}

pub data DefaultVariant: pass

pub trait Default:
    fn default_style() -> DefaultStyle:
        DefaultStyle {}
    fn default() -> Self

data DefaultSource: pass

impl Source for DefaultSource:
    type Error = never

    fn member[F < Default](mut self, m: Member) -> Result[F, never]:
        .Ok(F::default())

    fn variant(mut self, choices: List[Variant]) -> Result[Variant, never]:
        for choice in choices:
            if choice.facts.find[DefaultVariant]().is_some():
                return .Ok(choice)
        .Ok(choices[0])

impl[T] Default for T by Structure:
    fn default() -> Self:
        let source: mut DefaultSource = DefaultSource {}
        match T::build(source):
            .Ok(value) => value
```

**User side.** Tier 1 silently ignores declared defaults, because the source
cannot ask for them. Tier 2 fixes it with one `= pass` per defaulted member:

```text
@default.default()
data Request:
    url: string
    timeout_ms: i32 = 30000      # tier 1 yields 0: i32's default, not 30000
    retries: i32 = 3             # tier 1 yields 0
```

```text
# Hypothetical syntax (M3 member lines)
impl default.Default for Request by Structure:
    timeout_ms = pass            # build uses the declared default
    retries = pass
```

Swift has the same surprise: synthesized `Codable` ignores property
initializers, a common complaint. hd makes it worse because hd has declared
field defaults and users will expect `Default` to honor them.

**Deep copy breaks.** `clone(self) -> Self` must build a new value from an
old one. A source cannot read the old value's members, and a visitor cannot
produce a value:

```text
data CloneSource[T]:
    original: T

impl[T] Source for CloneSource[T]:
    type Error = never

    fn member[F < Clone](mut self, m: Member) -> Result[F, never]:
        # needs self.original's member m as an F: no operation provides it
        pass

    fn variant(mut self, choices: List[Variant]) -> Result[Variant, never]:
        # needs self.original's variant: also unavailable
        pass
```

**`T -> U` mapping breaks** for the same reason, twice: `UserRow -> User` by
member name would need the source to read `UserRow`'s members while `User`'s
`build` drives. The only encoding is to visit into a `List[Inspectable]` and
downcast, as in case 1.

**Verdict:** `Default` works with friction (P3). Clone and mapping break
(P1).

### 4. Builder

The target API is `Request::builder().url("x").timeout_ms(5).build()`:
a method per member, required members checked, defaults applied.

**Library side.** A template can only implement the methods of a trait that
already exists. `url(...)` and `timeout_ms(...)` are new per-type names, and
the builder is a new per-type type. Neither can be produced, so the typed
builder cannot be written. The nearest library is dynamic:

```text
use std.inspect.{Inspectable, downcast_val}
use std.structure.{Member, Source, Variant}

pub data Builder[T]:
    values: Map[string, Inspectable]

pub enum BuildError:
    Missing(member: string)
    WrongType(member: string)

data BuilderSource:
    values: Map[string, Inspectable]

impl Source for BuilderSource:
    type Error = BuildError

    fn member[F < Inspectable](mut self, m: Member) -> Result[F, BuildError]:
        match self.values.get(m.name):
            .Some(value) => match downcast_val[F](value):
                .Some(typed) => .Ok(typed)
                .None => .Err(BuildError.WrongType(m.name))
            .None => .Err(BuildError.Missing(m.name))   # no way to ask for the declared default

    fn variant(mut self, choices: List[Variant]) -> Result[Variant, BuildError]:
        .Err(BuildError.Missing("variant"))
```

Every member name is a string, every value is boxed, a misspelled or
mistyped member is a run-time error, and a member left unset with a declared
default still fails (P3).

**User side.** hd already has the typed alternative in the core language:

```text
data Request:
    url: string
    timeout_ms: i32 = 30000
    retries: i32 = 3

fn make() -> Request:
    base := Request { url: "https://example.com" }
    Request { ...base, timeout_ms: 5000 }
```

Required members are checked at compile time, and defaults apply. What the
literal cannot do is construct a type with private fields from another
module, which is the main reason Rust users reach for builders.

**Verdict: breaks** for the typed builder (P10, P3). The literal covers most
of the need.

### 5. Database Row Mapping

**Library side.** Table and column configuration is db's own visitor
configuration and facts. Encoding a row and decoding a fetched row are
one-value traversals and work:

```text
use std.structure.{Member, Source, Variant, Visitor}

pub enum Case:
    Plain
    Snake

pub data Table:
    name: string
    case: Case = .Snake

pub fn table(name: string, case: Case = .Snake) -> Table:
    Table { name: name, case: case }

pub data Column:
    name: string

pub fn column(name: string) -> Column:
    Column { name: name }

pub data PrimaryKey: pass

pub fn primary_key() -> PrimaryKey:
    PrimaryKey {}

pub trait ToSql:                       # hand-written for i64, string, bool, T?
    fn bind(self, stmt: mut Statement, name: string) -> Result[void, DbError]

pub trait FromSql:
    fn read(row: Row, name: string) -> Result[Self, DbError]

pub trait Record:
    fn table() -> Table:
        Table { name: "" }
    fn insert(self, stmt: mut Statement) -> Result[void, DbError]
    fn from_row(row: Row) -> Result[Self, DbError]

data Binder:
    stmt: mut Statement
    table: Table

impl Visitor for Binder:
    type Error = DbError

    fn member[F < ToSql](mut self, m: Member, value: F) -> Result[(), DbError]:
        value.bind(self.stmt, column_name(self.table, m))?
        .Ok(())

    fn variant(mut self, v: Variant) -> Result[(), DbError]:
        .Err(DbError.Unsupported("enum row"))

data RowSource:
    row: Row
    table: Table

impl Source for RowSource:
    type Error = DbError

    fn member[F < FromSql](mut self, m: Member) -> Result[F, DbError]:
        F::read(self.row, column_name(self.table, m))

    fn variant(mut self, choices: List[Variant]) -> Result[Variant, DbError]:
        .Err(DbError.Unsupported("enum row"))

impl[T] Record for T by Structure:
    fn insert(self, stmt: mut Statement) -> Result[void, DbError]:
        let binder: mut Binder = Binder { stmt: stmt, table: T::table() }
        self.visit(binder)?
        .Ok()

    fn from_row(row: Row) -> Result[Self, DbError]:
        let source: mut RowSource = RowSource { row: row, table: T::table() }
        T::build(source)

fn column_name(table: Table, m: Member) -> string:
    match m.facts.find[Column]():
        .Some(column) => column.name
        .None => m.name
```

**Decode without I/O works.** The I/O happens before `build`:

```text
pub fn load![T < Record](id: i64) -> Result[T?, DbError] $ Db:
    match $.use(Db).fetch_one!(T::table().name, id)?:
        .Some(row) => .Ok(.Some(T::from_row(row)?))
        .None => .Ok(.None)
```

**Decode that needs I/O per member breaks.** A member loaded lazily from a
join table, such as `tags: List[Tag]`, needs `$ Db` and suspension inside
`Source::member`. That trait method has the empty row and does not suspend,
and an impl must agree with the trait on both (11 Requirement Rows). A stored
provider value is allowed (11 Provider Access), but a suspending `fetch!` on
it still cannot be called from a non-suspending `member`. `Structure::build`
cannot suspend either, so no library can fix this in its own code (P8).

**The static column list is not available.** `CREATE TABLE`, `SELECT a, b`,
and migrations need every column's SQL type without a value. `describe()`
gives names and `TypeShape`s but not `F::sql_type()` from each member type's
`ToSql` impl (P2). `describe()` also may or may not reflect `cache = pass`
(Q2-c).

**User side.**

```text
use dep.db

@db.table("orders")
pub data Order:
    @db.primary_key()
    pub id: i64
    @db.column("total")
    pub total_cents: i64
    pub cache: Cache = Cache.empty()
# error at @db.table: member `cache`: Cache does not implement db.ToSql
```

```text
# Hypothetical syntax (M3 member lines)
impl db.Record for Order by Structure:
    fn table() -> db.Table:
        db.table("orders")
    cache = pass
```

`@db.primary_key()` is a declaration fact and works. The `cache` skip forces
tier 2, so the table name moves from the annotation into the block.

**Verdict: works with friction** for rows. Per-member I/O breaks (P8), and
DDL generation needs P2.

### 6. Schema Generation Without A Value

JSON Schema, OpenAPI components, GraphQL SDL, and MCP `inputSchema` all
describe a *type*: every member of every variant, each member through its
own type's description, with recursion by reference.

**Library side, as the design allows it.** The only value-free operation is
`describe() -> Shape`:

```text
use std.structure.{Member}

pub trait Schema:
    fn schema_style() -> Style:
        Style {}
    fn schema(defs: mut Defs) -> Node

impl[T] Schema for T by Structure:
    fn schema(defs: mut Defs) -> Node:
        shape := T::describe()
        if defs.reserve(shape.qualified_name):
            let properties: mut List[(string, Node)] = []
            for field in shape.field_list:
                properties.append((field.name, node_for(field.field_type, defs)))
            defs.define(shape.qualified_name, object_node(properties))
        Node.Ref(shape.qualified_name)

fn node_for(t: TypeShape, defs: mut Defs) -> Node:
    match t:
        TypeShape.Primitive(kind) => primitive_node(kind)
        TypeShape.Optional(inner) => nullable(node_for(inner, defs))
        TypeShape.List(element) => array_node(node_for(element, defs))
        TypeShape.Named(decl, args) => Node.Opaque   # cannot reach the named type's Schema impl
        _ => Node.Opaque
```

`TypeShape.Named(decl, args)` carries a `DeclarationId`. Nothing maps a
`DeclarationId` back to a `DataShape`, and nothing maps it to that type's
`Schema` impl. So a nested type cannot be described at all.

Suppose a library extended shapes so that it could recurse into the nested
`DataShape`. The schema would then *disagree* with the codec:

```text
use dep.json

@json.json(case=.Snake)
pub data Address:
    pub streetLine: string     # encoded as "street_line"

pub data Money:
    cents: i64

impl json.Encode for Money:            # encoded as a decimal string "12.50"
    fn encode(self, out: mut json.Writer) -> Result[void, json.EncodeError]:
        out.raw(format_decimal(self.cents, places=2))
        .Ok()

@json.json(case=.Camel)
pub data Invoice:
    pub address: Address
    pub total: Money
```

A shape-walking schema for `Invoice` says `address.streetLine` (Address's
`Style` lives in Address's impl, not in its shape) and `total: {cents:
integer}` (Money's encoding is hand-written). Both are wrong, which is
exactly the drift M7 exists to prevent for codecs.

**Workarounds tried.**

- *Probe `build` with a schema source.* `member[F < Schema](m)` could record
  `F::schema(defs)`, but it must then return an `F`. Returning `.Err` stops
  `build` at the first member. `variant(choices)` sees every variant but
  must pick one, so the other variants' members are never reached.
- *Visit a default value.* `T::default().visit(SchemaVisitor)` reaches
  every member with its static type, ignoring the value. It forces
  `Default` on every described type, and for an enum it visits one variant.

**What would work** is the value-free typed traversal of decision 2, which
M1 kept in name only:

```text
# Hypothetical: a generated describe that visits every member of every variant
impl Describer for SchemaDescriber:
    type Error = never

    fn member[F < Schema](mut self, m: Member) -> Result[(), never]:
        self.properties.append((key_for(self.style, m), F::schema(self.defs)))
        .Ok(())

    fn variant(mut self, v: Variant) -> Result[(), never]:
        self.begin_variant(v)
        .Ok(())
```

Recursive types (`Tree` with `children: List[Tree]`) then work through
`defs.reserve`, as in the Design D schema example.

**Verdict: breaks** (P2). This is the use case hd's tool story (MCP, typed
RPC) depends on most.

### 7. Protobuf-Style Binary Codec

**Library side.** Field numbers are facts. Encoding is a one-value visit and
works. Decoding meets a mismatch between wire order (any order, repeated
fields merged) and `build`'s declaration order:

```text
use std.structure.{Member, Source, Variant, Visitor}

pub data Tag:
    number: i32

pub fn tag(number: i32) -> Tag:
    Tag { number: number }

pub data UnknownFields:
    bytes: List[u8]

pub data Message:
    reserved: List[i32] = []

pub fn message(reserved: List[i32] = []) -> Message:
    Message { reserved: reserved }

pub trait Field:                          # hand-written for scalars, strings, lists, T?
    fn wire_type() -> i32
    fn write(self, out: mut Buffer) -> void
    fn read(input: mut Reader) -> Result[Self, WireError]
    fn absent() -> Result[Self, WireError]

pub trait Proto:
    fn message_style() -> Message:
        Message {}
    fn encode(self, out: mut Buffer) -> Result[void, WireError]
    fn decode(input: List[u8]) -> Result[Self, WireError]

data ProtoWriter:
    out: mut Buffer

impl Visitor for ProtoWriter:
    type Error = WireError

    fn member[F < Field](mut self, m: Member, value: F) -> Result[(), WireError]:
        number := tag_of(m)?                      # run-time error when the fact is missing
        self.out.key(number, F::wire_type())
        value.write(self.out)
        .Ok(())

    fn variant(mut self, v: Variant) -> Result[(), WireError]:
        .Ok(())

data ProtoSource:
    index: Map[i32, List[Span]]               # built by a pre-scan of the whole message
    input: List[u8]

impl Source for ProtoSource:
    type Error = WireError

    fn member[F < Field](mut self, m: Member) -> Result[F, WireError]:
        number := tag_of(m)?
        match self.index.get(number):
            .Some(spans) => F::read(reader_over(self.input, spans))
            .None => F::absent()

    fn variant(mut self, choices: List[Variant]) -> Result[Variant, WireError]:
        first_present_variant(choices, self.index)

impl[T] Proto for T by Structure:
    fn encode(self, out: mut Buffer) -> Result[void, WireError]:
        let writer: mut ProtoWriter = ProtoWriter { out: out }
        self.visit(writer)?
        .Ok()

    fn decode(input: List[u8]) -> Result[Self, WireError]:
        let source: mut ProtoSource = ProtoSource { index: scan_tags(input)?, input: input }
        T::build(source)
```

**User side.**

```text
use dep.proto

@proto.message(reserved=[3])
pub data Person:
    @proto.tag(1)
    pub id: i64
    @proto.tag(2)
    pub name: string
    @proto.tag(4)
    pub email: string?
    @proto.tag(0)
    pub unknown: proto.UnknownFields = proto.UnknownFields { bytes: [] }
```

**Problems.**

1. **Pre-scan.** `build` asks for members in declaration order, so the
   source indexes every tag first: one `Map` per message plus span lists.
   serde and prost decode in wire order into per-field `Option` slots,
   with no index (P7).
2. **Tag facts are checked at run time.** A missing `@proto.tag`, a
   duplicated number, or a number listed in `reserved` is a wire-format bug.
   prost rejects them at compile time. Here they surface as the first
   `WireError` or never (P14).
3. **Unknown fields are a convention.** `tag(0)` marks the residue member.
   Its `Field` impl must know "everything the index did not claim", which the
   source can compute only because it pre-scanned. A member marked by a fact
   still has to satisfy `F < Field`, so `UnknownFields` implements the
   ordinary per-field trait with a special meaning.
4. **Versioning:** proto2 `[default = 5]` is a declared default read at run
   time when the tag is absent (P3). Renumbering and reordering are covered
   in case 18.
5. `oneof` maps to an enum member whose variants carry the tag facts. That
   works: the enum's own `Proto` derivation reads variant facts.

**Verdict: works with friction** (P7, P14, P3).

### 8. CLI Arguments From A Data Type

**Library side.** Parsing is a `build` from argv. Flags and positionals are
facts; boolean flags come from the member type's own impl; subcommands are
variants:

```text
use std.structure.{Member, Source, Variant}

pub data App:
    name: string
    about: string = ""

pub fn command(name: string, about: string = "") -> App:
    App { name: name, about: about }

pub data Short:
    letter: string

pub fn short(letter: string) -> Short:
    Short { letter: letter }

pub data Positional: pass

pub fn positional() -> Positional:
    Positional {}

pub trait Value:                          # hand-written for i64, string, bool, T?, List[T]
    fn takes_value() -> bool
    fn metavar() -> string
    fn parse_value(text: string?) -> Result[Self, CliError]

pub trait Command:
    fn app() -> App:
        App { name: "" }
    fn parse(args: List[string]) -> Result[Self, CliError]
    fn help() -> string

data ArgSource:
    args: ParsedArgs                      # argv pre-split into flags and positionals

impl Source for ArgSource:
    type Error = CliError

    fn member[F < Value](mut self, m: Member) -> Result[F, CliError]:
        if m.facts.find[Positional]().is_some():
            return F::parse_value(self.args.next_positional())
        F::parse_value(self.args.flag(m.name, short_of(m), F::takes_value()))

    fn variant(mut self, choices: List[Variant]) -> Result[Variant, CliError]:
        word := self.args.next_positional()
        find_subcommand(choices, word)
```

**User side.**

```text
use dep.cli

@cli.command("serve", about="Run the server")
pub data Serve:
    ## Port to listen on
    @cli.short("p")
    pub port: i32 = 8080
    ## Directory to serve
    @cli.positional()
    pub root: string

@cli.command("tool")
pub enum Tool:
    Serve(options: Serve)
    Version
```

**Problems.**

1. **Omitted flags cannot take the declared default.** `--port` absent must
   yield `8080`. `F::parse_value(.None)` can only produce a type-level
   answer (`i32` has none), and `port = pass` would make the flag
   unparseable. Every option with a default breaks (P3).
2. **Help needs value-free types.** `--port <INT>  Port to listen on
   [default: 8080]` needs `F::metavar()` for each member without a value
   (P2), the doc comment (assumed on `Member`), and the default as text
   (P3, plus a `Display` obligation that only the help path needs).
3. `options: Serve` as a variant payload flattens a data type into a
   subcommand's flags. The variant's single member is visited as one member
   named `options`, so the source must special-case it through a fact or a
   trait method on `Value`.

**Verdict:** parsing works with friction; help and defaults **break** (P2,
P3).

### 9. Property-Test Generators With Shrinking

**Library side.** Generation is a `build` from a seeded generator. The size
budget halves at each nesting level:

```text
use std.structure.{Member, Source, Variant}

pub data Gen: pass

pub fn arbitrary() -> Gen:
    Gen {}

pub data Weight:
    value: i32

pub fn weight(value: i32) -> Weight:
    Weight { value: value }

pub trait Arbitrary:
    fn gen_style() -> Gen:
        Gen {}
    fn generate(rng: mut Rng, size: i32) -> Self
    fn shrink(self) -> List[Self]

data RandomSource:
    rng: mut Rng
    size: i32

impl Source for RandomSource:
    type Error = never

    fn member[F < Arbitrary](mut self, m: Member) -> Result[F, never]:
        .Ok(F::generate(self.rng, self.size / 2))

    fn variant(mut self, choices: List[Variant]) -> Result[Variant, never]:
        if self.size <= 1:
            return .Ok(lightest(choices))         # by Weight facts or member_count
        .Ok(weighted_choice(self.rng, choices))

impl[T] Arbitrary for T by Structure:
    fn generate(rng: mut Rng, size: i32) -> Self:
        let source: mut RandomSource = RandomSource { rng: rng, size: size }
        match T::build(source):
            .Ok(value) => value

    fn shrink(self) -> List[Self]:
        []                                        # cannot be written: see below
```

**User side.**

```text
use dep.check

@check.arbitrary()
pub enum Tree:
    Leaf(value: i64)
    @check.weight(1)
    Node(children: List[Tree])
```

**Problems.**

1. **Size limits need facts.** `Leaf(value)` and `Node(children)` both have
   one member, so `member_count` does not tell the generator which variant
   recurses. Without the `Weight` fact the generator may recurse at size 0.
   QuickCheck's generic derivation and Hypothesis `from_type` both use
   type-level recursion information for this.
2. **Shrinking breaks.** `shrink` returns smaller copies of `self`: each
   member shrunk in turn with the others kept, and each variant moved toward
   a simpler one. That needs "rebuild `self` with member `m` replaced", a
   value-to-value traversal (P1).
3. The generator must be a pure `mut Rng` value. A `$ mut Random` provider
   cannot be used inside `member`, which has the empty row (P8). This matches
   the STDLIB `Strategy` design, so it is only a note.

**Verdict:** generation works with friction; shrinking **breaks** (P1).

### 10. Validation

**Library side.** Facts carry the rules. The per-member check is done by
the member type's own impl, which interprets the facts it understands:

```text
use std.structure.{Member, Variant, Visitor}

pub data Rules: pass

pub fn validate() -> Rules:
    Rules {}

pub data Range:
    min: i64
    max: i64

pub fn range(min: i64, max: i64) -> Range:
    Range { min: min, max: max }

pub data Pattern:
    source: string

pub fn pattern(source: string) -> Pattern:
    Pattern { source: source }

pub data Issue:
    path: string
    message: string

pub trait Validate:
    fn rules() -> Rules:
        Rules {}
    fn check(self, facts: Facts, path: string, out: mut List[Issue]) -> void

impl Validate for i64:
    fn check(self, facts: Facts, path: string, out: mut List[Issue]) -> void:
        match facts.find[Range]():
            .Some(r) =>
                if self < r.min || self > r.max:
                    out.append(Issue { path: path, message: "out of range" })
            .None => pass

impl Validate for string:
    fn check(self, facts: Facts, path: string, out: mut List[Issue]) -> void:
        match facts.find[Pattern]():
            .Some(p) =>
                if !Regex::compile(p.source).matches(self):   # compiled on every call
                    out.append(Issue { path: path, message: "does not match" })
            .None => pass

data Collector:
    prefix: string
    out: mut List[Issue]

impl Visitor for Collector:
    type Error = never

    fn member[F < Validate](mut self, m: Member, value: F) -> Result[(), never]:
        value.check(m.facts, self.prefix + "." + m.name, self.out)
        .Ok(())

    fn variant(mut self, v: Variant) -> Result[(), never]:
        .Ok(())

impl[T] Validate for T by Structure:
    fn check(self, facts: Facts, path: string, out: mut List[Issue]) -> void:
        let collector: mut Collector = Collector { prefix: path, out: out }
        self.visit(collector)
        pass
```

Error accumulation works: the visitor never fails (`Error = never`) and
appends to `out`, and nested paths come from `m.name`.

**User side.**

```text
use dep.validate

@validate.validate()
pub data Signup:
    @validate.range(13, 130)
    pub age: i64
    @validate.pattern(".+@.+")
    pub email: string
    @validate.range(1, 5)
    pub nickname: string          # accepted; string's impl ignores Range
    pub starts: i64
    pub ends: i64                 # cross-member rule: ends > starts
```

**Problems.**

1. **Facts are not checked against the member type.** Chapter 14 types
   metadata as `FieldMetadata[T]`, so `max_len(80)` on an `i32` is a compile
   error. M2 facts are "typed metadata values", but `Member` is not generic
   in the member type, so `@validate.range(1, 5)` on a `string` is accepted
   and ignored (P14).
2. **Compile-time facts cannot hold a compiled regex.** The proposed
   evaluator builds results from literals, data, enums, lists, maps,
   strings, numbers, and function references, so the fact stores the
   pattern text and the check compiles it on every call. Caching needs a
   per-(trait, type) plan, which M1-M11 does not have (P7).
3. **Cross-member rules cannot extend the template.** A tier-2 block may
   override `check`, but the override replaces the template body. Nothing
   lets it call the structural version and then add `ends > starts`. The
   decision-6 escape hatch, a hand-written impl calling a structural
   function, went away when `Structure` stopped bounding functions (P16).
   The library can anticipate this with a hook (`fn extra(self, out: mut
   List[Issue])`), but then the user writes a second trait impl anyway.

**Verdict: works with friction** (P14, P7, P16).

### 11. Error Enums

The Error Conversion decisions make `?` call the target's `From[E]` once,
make a single-payload variant constructor a function value, and defer
derived `From` to this protocol (decision 10).

```text
use std.error.Error

pub enum SyncError:
    Fs(error: FsError)
    Http(error: HttpError)
    Invalid(reason: string)
```

**`From` per variant breaks.** The wanted output is two impls,
`From[FsError] for SyncError` and `From[HttpError] for SyncError`. A template
is `impl[T] Trait for T by Structure`, and one opt-in applies one trait
instantiation. Writing the template generically in the trait argument does
not help:

```text
impl[T, P] From[P] for T by Structure:
    fn from(value: P) -> Self:
        let source: mut FromSource[P] = FromSource { value: value }
        match T::build(source):
            .Ok(result) => result

data FromSource[P]:
    value: P

impl[P] Source for FromSource[P]:
    type Error = never

    fn member[F](mut self, m: Member) -> Result[F, never]:
        # must return self.value (a P) as an F: P and F are unrelated types here
        pass

    fn variant(mut self, choices: List[Variant]) -> Result[Variant, never]:
        # must pick the variant whose payload type is P: Variant has no types
        pass
```

The head itself is legal (`P` is constrained by the trait argument), but
the source cannot be written. The user would write
`impl From[FsError] for SyncError by Structure` per variant, which is longer
than the two-line hand-written impl it replaces. Two variants with the same
payload type need an overlap diagnostic that names both variants; nothing
here produces it.

**Messages work with friction.** A variant fact carries a template; the
visitor substitutes members by name:

```text
use std.structure.{Member, Variant, Visitor}

pub data Message:
    text: string

pub fn message(text: string) -> Message:
    Message { text: text }

data MessageVisitor:
    text: string
    out: mut List[string]

impl Visitor for MessageVisitor:
    type Error = never

    fn variant(mut self, v: Variant) -> Result[(), never]:
        match v.facts.find[Message]():
            .Some(message) => self.text = message.text
            .None => self.text = v.name
        .Ok(())

    fn member[F < Display](mut self, m: Member, value: F) -> Result[(), never]:
        self.text = self.text.replace("{" + m.name + "}", value.to_string())
        .Ok(())
```

```text
use dep.errors

@errors.display()
pub enum FetchError:
    @errors.message("not found: {path}")
    NotFound(path: string)
    @errors.message("timed out after {secs}s")
    Timeout(seconds: i64)             # "{secs}" is never replaced; nothing checks it
```

Placeholder names are checked only at run time, while `thiserror` rejects
`{secs}` at compile time. And the template has to be a `Display` template:
if `Display` gains the style hook, `Display` stops being dynamically safe
and `Error < Display` breaks (P5).

**`cause()` breaks.** `Error::cause(self) -> Error?` should return the
payload that is itself an error. A visitor with `member[F < Error]` requires
every member to implement `Error`, including `reason: string`. A fact such
as `@errors.source` cannot relax the obligation (M6), and `= pass` on
`reason` would drop it from the message too (P9).

**Verdict: breaks** for `From` (P12) and `cause` (P9). Messages work with
friction (P14, P5).

### 12. Diffing And Patching

The wanted API: `diff(old, new) -> UserPatch`, `apply(value, patch) -> User`,
with `UserPatch` a companion type whose members are `Option`s.

- **Diff** consumes two values member by member: the same gap as equality
  (P1).
- **The patch type** is a new per-type declaration. A template cannot
  declare it, and an associated type `type Patch` in the trait cannot be
  bound by a template to a type that does not exist (P10).
- **Apply** is a value-to-value traversal: rebuild `value`, replacing each
  member present in the patch (P1).

The fallback is untyped: encode both values to `json.Json`, diff the trees,
and decode the patched tree. It allocates the whole tree twice and loses
static member names:

```text
use dep.json

pub fn diff_json[T < json.Encode](old: T, new: T) -> Result[json.Json, json.Error]:
    .Ok(json_diff(json.to_value(old)?, json.to_value(new)?))

pub fn apply_json[T < json.Encode + json.Decode](value: T, patch: json.Json) -> Result[T, json.Error]:
    json.from_value(json_merge(json.to_value(value)?, patch))
```

**Verdict: breaks** (P1, P10).

### 13. Generic Types And Phantom Parameters

```text
use dep.json

@json.json()
pub data Box[T]:
    pub value: T

@json.json()
pub data Pair[A, B]:
    pub first: A
    pub second: B

@json.json()
pub enum Tree[T]:
    Leaf(value: T)
    Branch(left: Tree[T], right: Tree[T])

@json.json()
pub data Id[T]:                   # phantom: T appears in no member
    pub raw: i64

@json.json()
pub data Tagged[T]:
    pub tags: Set[T]              # Set[T] < json.Decode needs T < json.Decode + Eq + Hash
```

M9 rule 3 checks "every visited member satisfies the bound" at the opt-in
site. For `Box[T]` the obligation is `T < json.Encode`, and it can hold only
if the derived impl's header says so. M1-M11 never says what that header is.
Tier 1 has no header to write, so every generic type above is unspecified
under tier 1. The candidate rules, all from earlier text:

- **The 09 built-in rule** (`@derive(PartialEq) data Box[T]` gets
  `T < PartialEq` for each parameter used in a compared field). It gives
  `Box`, `Pair`, and `Tree` the right header, gives `Id[T]` none, and gives
  `Tagged[T]` the wrong one: `T < json.Decode` is not enough for
  `Set[T] < json.Decode`.
- **Decision 6 inference** (trace each member obligation through the unique
  impl per type constructor). It gets `Tagged[T]` right
  (`T < json.Decode + Eq + Hash`), but the inferred header then depends on
  member types. When a member is private, a public impl head changes when a
  private field's type changes, which is a hidden semver break (case 18).
- **Tier 2 with a written header**, which `impl Trait for X by Structure`
  seems to allow:

```text
impl[T < json.Decode + Eq + Hash] json.Decode for Tagged[T] by Structure
```

`Tree[T]` needs the coinductive assumption `Tree[T] < json.Encode` while
checking its own members. Decision 6 stated it; M1-M11 does not.

A variant with its own generic parameters, or a GADT variant with a refined
result (13 Generalized Algebraic Data Types), cannot be built at an arbitrary
`Expr[T]`, so a generated `build` for such an enum is ill-typed. The record
has no rule for it (Design A's `RefinedCase` went with the views).

**Verdict: breaks** for tier 1, unspecified for tier 2 (P6, P11).

### 14. Member Model: Enum Data, Newtypes, Embedding, Visibility, Foreign Types, `mut`

**Common enum fields.**

```text
@json.json()
pub enum HttpStatus(code: i32, phrase: string):
    Ok -> HttpStatus(200, phrase="OK")
    NotFound -> HttpStatus(404, phrase="Not Found")
```

09 says derived equality compares common enum fields, so `visit` should
report them. `build` cannot set them: the variant's constructor expression
computes them. But shared fields are assignable through a `mut` root
(08 Shared Enum Constructor Data), so a decoded value can differ from the
encoded one. Whether `visit` reports shared fields, in which position, and
what `build` does with them is unspecified (P11).

**Payload-free enums** work, with one library cost. `json`'s template
writes `begin_object()` before `visit`, but a payload-free enum is usually
encoded as a bare string. The template must check the shape first:
`T::describe()` shows every variant has no payload, and the template picks
the string form. Fine, but every format library repeats that check.

**Newtypes.** `type Mile(i32)` is neither a data type nor an enum, so M1
does not give it `Structure`. Tier 1 cannot even be written: a decorator
before `type Mile(i32)` is a `syntax-error` today (`decorated_decl` excludes
`type_decl`, noted in the record's parse log). If newtypes get `Structure`
with one member named `0`, JSON writes `{"0": 10}` rather than serde's
transparent `10`. The 09 TQ-11 rule, derive through the base type, has no
counterpart in M1-M11 (P11).

**Embedding.**

```text
@json.json()
pub data Post:
    Timestamps
    pub id: string
```

The record does not say whether `Timestamps` is one member (name
`Timestamps`, value the part) or its fields are visited in place. As one
member, JSON writes `{"Timestamps": {...}, "id": ...}`. Go flattens embedded
structs by default, and serde needs `#[serde(flatten)]`. Flattening needs a
second trait method on `json.Encode` (write members without braces) and on
`json.Decode` (read members from the parent object), and a way for the
visitor to know the member is embedded. `Member` has no such flag. `build`
must fill the part with a copy (`Timestamps: ...value`), as 08 requires,
which the generated code can do. Member lines cannot address a part's own
members (`Timestamps.created_at = [...]`) (P11).

**Payload member lines.** In `Login(user: i64)` and `Logout(user: i64,
reason: string)`, a tier-2 line `user = [json.rename("uid")]` does not say
which variant's `user` it means. A variant line `Logout = pass` has no
defined meaning: a variant has no declared default for `build` to use (P11).

**Private members.** A tier-1 or tier-2 derivation in the owning module
visits and builds private members, so a derived decoder bypasses any
constructor invariant, as serde's does. The record does not say whether a
tier-2 block in *another module of the same package* sees private members:
visibility in hd is per module, but impl ownership is per package (Q11-e).

**Foreign types.**

```text
use dep.json
use dep.geo.Point

impl json.Encode for Point by Structure
# error: orphan-impl: the app owns neither json.Encode nor dep.geo.Point
```

Decisions 9 and 11 allowed a standalone derive for a foreign type with
public members only, plus the root-application orphan exception. M1-M11
drops both, and chapter 14's orphan exception covers annotations only, not
ordinary impls. So a type from another package can never be derived,
even when every member is public (P15). Rust has the same wall and serde
answers it with `#[serde(remote = "Point")]`, a local mirror type plus
generated conversion functions.

**`mut` members.**

```text
@json.json()
pub data Counter:
    pub hits: mut Cell

fn load(text: string) -> Result[void, json.Error]:
    let counter: mut Counter = json.from_json(text)?    # mutable-upgrade
    counter.hits.increment()
    .Ok()
```

`visit` reads `hits` through readonly `self`, so the member is `Cell`, which
is fine. `build` returns `Result[Self, S::Error]`, a readonly `Self`
(04 Parameters And Results). No template can return `mut Self`, so no
derived decoder, `Default`, or generator can produce a mutable value. The
caller must copy (`Counter { ...counter }`), and for `mut` members even the
copy is readonly (08 Construction And Access) (P17).

**Verdict:** mixed. Payload-free enums and private members work. Common
fields, newtypes, embedding, and payload member lines are unspecified (P11).
Foreign types break (P15). Mutable results break (P17).

### 15. Several Libraries On One Type

```text
use dep.json
use dep.db
use dep.validate
use dep.cli

@json.json(case=.Camel)
@db.table("users")
@validate.validate()
@cli.command("user")
pub data User:
    @db.primary_key()
    pub id: i64
    @json.rename("mail")
    @validate.pattern(".+@.+")
    @cli.short("e")
    pub email: string
    pub session: Session = Session.none()   # no json, db, or cli impl
```

**What works.** Declaration facts are typed values, so `json.Rename`,
`db.PrimaryKey`, `validate.Pattern`, and `cli.Short` never collide. Each
library reads its own types with `facts.find[M]()` and ignores the rest.

**What does not.**

1. **One skipped member removes tier 1 for every library that visits it.**
   `session` has no json, db, or cli impl. Each of `json.Encode`,
   `json.Decode`, `db.Record`, and `cli.Command` now needs its own tier-2
   block with `session = pass`, and each block must drop the matching
   annotation (tier 1 plus tier 2 is `overlapping-impl`, rule 6):

```text
# Hypothetical syntax (M3 member lines)
impl json.Encode for User by Structure:
    fn encode_style() -> json.Style:
        json.json(case=.Camel)
    session = pass

impl json.Decode for User by Structure:
    fn decode_style() -> json.Style:
        json.json(case=.Camel)
    session = pass

impl db.Record for User by Structure:
    fn table() -> db.Table:
        db.table("users")
    session = pass

impl cli.Command for User by Structure:
    fn app() -> cli.App:
        cli.command("user")
    session = pass
```

   Four blocks and four repeated lines for one member. Forgetting `session =
   pass` in `json.Decode` is caught only because `Session` lacks
   `json.Decode`. If it had one, the codec would silently stop round-tripping
   (M11 accepted that risk).
2. **Hook names collide.** The record names json's hook `visitor()` in both
   `Encode` and `Decode`. A type deriving both then has two `visitor()`
   associated functions. Inside a template, whether `Self::visitor()` (here
   `T::visitor()`) means the enclosing trait's is unspecified, and
   `User::visitor()` from outside is ambiguous. 09 gives no form that picks
   one trait's receiverless function for a type: `Trait::function()` selects
   the impl through a receiver or a `Self` argument, and this function has
   neither. The same happens across
   libraries that pick the same name. The example above uses distinct names
   (`encode_style`, `decode_style`), which only a convention guarantees (P5).
3. **Annotation or fact?** `@db.table("users")` is an annotation (it derives
   `db.Record`) and `@db.primary_key()` is a fact, purely because some db
   trait declares a hook returning `Table` and none returns `PrimaryKey`.
   The reader cannot tell which lines derive impls (P13).

**Verdict: works with friction** (P13, P5).

### 16. Function Targets: MCP Tools From Functions

```text
use dep.mcp

## Look up a user by id
pub fn get_user!(id: UserId, include_deleted: bool = false) -> Result[User, NotFound] $ Users:
    pass
```

A tool needs (1) an input schema from the parameters, (2) a decoder from
arguments to a call, with declared defaults, (3) the output schema, and
(4) the requirement row `Users` in the tool's type, so the host knows what to
bind (10 Wasm Boundary).

**Blocked.** `Structure` exists for data types and enums only, and a
function is not a type. FN_TYPE option B (per-declaration item types with a
compiler-generated `FnStructure`) is the only proposal that makes
`impl mcp.Tool for fn get_user by Structure` meaningful. Even with item
types, the template needs:

- a value-free parameter traversal for the schema (P2);
- a declared-default channel for `include_deleted` (P3; `FnShape` records
  only `has_default`);
- a way to name the function's row and suspension in its own method
  signature (`fn invoke!(args: Json) -> Result[Json, ToolError] $ Rq`). A
  template head is `impl[T] Tool for T`, and `Structure` has no associated
  type or row parameter for `Rq` (P8).

**The workaround today** is an argument record, which then hits P2 and P3:

```text
@json.json()
pub data GetUserArgs:
    pub id: UserId
    pub include_deleted: bool = false    # the decoder cannot use this default (P3)

fn tools() -> mcp.Registry[Users]:
    let registry: mut mcp.Registry[Users] = mcp.Registry::new()
    registry.add(mcp.tool("get_user", fn!(args: GetUserArgs) -> Result[User, NotFound] $ Users: get_user!(args.id, args.include_deleted)))
    registry
```

**Verdict: breaks** (FN_TYPE, P2, P3, P8).

### 17. Performance Under Per-Shape Compilation

The claim (M9 item 4): generated code is typed per member, each call passes
a constant dictionary and an unboxed value, no per-member allocation.

**What holds.** The generated `visit` and `build` do what the record says.
`v.member[i64](m_id, self.id)` passes the `i64 < Encode` dictionary as a
constant and the `i64` unboxed. `Encoder::member[F < Encode]` is compiled
once per machine shape in json's package, so each member costs one call
into that body plus one indirect call through `F`'s dictionary.

**What the claim leaves out.**

1. **`visit` is not ordinary generic code.** A generic `visit[V < Visitor]`
   compiled once per shape cannot call `v.member[F]` with `F`'s `Encode`
   dictionary, because `V < Visitor` says nothing about `Encode`. That is why
   M9 rule 3 requires a concrete visitor. So `visit` and `build` are
   specialized per (target type, visitor type) pair, like pack code, and code
   size grows with types times visitors. The spec's model (04 Shapes and
   Generic Code, 10 Name Resolution Across Packages) needs a sentence adding
   templates and generated `visit`/`build` to the specialized category (P19).
2. **Out-of-order decoders allocate per object.** JSON objects, protobuf
   messages, CLI argv, query strings, and SQL rows by name arrive in input
   order. `build` asks in declaration order, so the source either pre-indexes
   the input (one map per object, sized by member count) or rescans it for
   each member (quadratic). serde's derived `Deserialize` avoids both by
   letting the input drive: a match from key to field index, with an
   `Option` local per field (P7).
3. **Per-call recomputation.** `key_for(style, m)` runs for every member on
   every call: a `facts.find[Rename]()` scan (a runtime type test per fact,
   because facts are heterogeneous) and `apply_case`, which allocates a new
   string. A tier-2 `visitor()` override is an ordinary function that
   rebuilds its `Style` on every call. `build`'s `s.variant([v_login,
   v_logout])` allocates a list per call unless the compiler makes it a
   constant. Only a per-(trait, type) memoized plan removes these, and
   decision 10's cache is gone (P7).
4. **Forced boxing and dynamic dispatch.** Only in workarounds: the P1
   encodings (cases 1 and 3) box each member as `Inspectable`, and the
   builder (case 4) boxes every value. The existing `Hash` signature passes
   `mut Hasher` as a dynamic trait value, so `HashVisitor` dispatches
   dynamically per member regardless of this design.

**Verdict:** the claim holds for the generated code, not for libraries
written in the natural way (P7, P19).

### 18. Package Evolution

| Change | Effect on derived impls | Visible downstream? |
| --- | --- | --- |
| Add a member | Owner's derive re-checked; a member type without an impl fails there, naming the member | Wire output changes (a new JSON key); old inputs fail to decode unless the member has a default and P3 is fixed |
| Add a *private* member | Same; private members are visited | Wire output changes silently; with inferred bounds (P6) a public impl head may change |
| Remove or rename a member | Wire name changes unless a rename fact keeps it | Old data no longer decodes; nothing warns |
| Reorder members | `visit` order changes | JSON key order changes; positional binary formats break silently; derived `Ord` changes meaning (09: lexicographic in declaration order) |
| Add, remove, or reorder variants | `Variant.index` changes | Index-based codecs and derived `Ord` change silently; `build` of old data fails on a removed name |
| Library adds a trait whose hook returns its style type | Rule 5: every `@lib.style(...)` downstream now derives the new trait too | New impls appear with no source change. One can overlap a hand-written impl downstream (`overlapping-impl` after a dependency bump) or fail a member obligation there |
| Library changes a template body | Instantiated at each opt-in site (M9 rule 3) | Behavior changes after recompilation only; interfaces must carry template bodies |
| Library changes an annotation function's body | Evaluated at the user's compile time | Interfaces must carry the body; the user's derived constant changes on a dependency bump |
| Owner removes `@json` from a public type | Impl gone | Every downstream `T < json.Encode` use fails; visible as an interface change, which is correct |

The last row behaves correctly. The rows above it are silent. Rust has the
same wire-level silence (serde does not warn on reorder), but Rust has no
equivalent of the "library adds a trait" row, because a serde derive names
its traits.

10 Name Resolution Across Packages says a package interface "does not depend
on any other function body" and lists only pack and reified bodies as
carried. Templates, generated `visit`/`build`, and compile-time annotation
functions are three more kinds of carried body (P19).

**Verdict: works with friction** (P13, P19, P20).

### 19. Mock Providers For Traits

OPEN_ISSUES mentions generating mock providers from a trait (a read-only
`TraitShape`, or an external generator over interface files). A trait is not
a data type or enum, so `Structure` does not apply, and a mock is a new type
(P10). Out of scope for this design. Question Q10-c asks whether that is
intended.

### 20. Stable Fingerprints

`std.fingerprint.Fingerprintable` mirrors `Hash` with a stable output, and
derives exactly like case 1's `Hash`. Stability across versions needs member
*names* (or tags) in the digest rather than positions, which the visitor can
do with `m.name`. **Works**, subject to the evolution rows above (P20).

### 21. Registered Boundary Adapters

10 Wasm Boundary makes the compiler generate component-model lifting and
lowering for boundary-safe types. Rebuilding that on `Structure` would need
the value-free description (a WIT record or variant type per member type)
before any value exists (P2), and a check that every member is `pub`, which
facts cannot express. **Breaks** if moved onto the design. Keeping it
compiler-generated is consistent with the record.

## Problems, Ranked

Problems are listed from most to least severe. The ids are used throughout
the report. Each problem states its effect, one to three candidate fixes
with their trade-offs, and questions for the owner. None of the fixes is a
recommendation.

| Id | Problem | Severity | Use cases |
| --- | --- | --- | --- |
| P1 | No two-value or value-to-value traversal | Critical | 1, 3, 9, 12 |
| P2 | No value-free typed traversal | Critical | 5, 6, 8, 16, 21 |
| P3 | Declared defaults unreachable at run time | Critical | 3, 4, 7, 8, 16 |
| P4 | Marker templates check nothing | High (unsound impls) | 1 |
| P5 | The style hook is a trait associated function | High | 2, 11, 15 |
| P6 | No bound rule for generic targets | High | 13 |
| P7 | Declaration-order decode and per-call recomputation | Medium | 7, 10, 17 |
| P8 | No requirement row or suspension inside traversal | Medium | 5, 9, 16 |
| P9 | A member is either fully visited or absent | Medium | 2, 11 |
| P10 | Derivation cannot declare types or methods | Medium | 4, 12, 19 |
| P11 | Member model unspecified | Medium | 13, 14 |
| P12 | No impl family indexed by member types (`From`) | Medium | 11 |
| P13 | Tier-1 meaning depends on return types and package contents | Medium | 1, 5, 15, 18 |
| P14 | Facts are untyped against members and checked only at run time | Medium | 7, 10, 11 |
| P15 | Foreign types cannot be derived | Medium | 14 |
| P16 | Templates cannot be extended or composed | Low | 10 |
| P17 | `build` returns readonly `Self` | Low | 14 |
| P18 | The reference example does not parse | Low | all |
| P19 | Compilation and interface model not stated | Low | 17, 18 |
| P20 | Wire stability is silent | Low | 18, 20 |

### P1. No Two-Value Or Value-To-Value Traversal

**Effect.** `visit` consumes one value and `build` produces one value from
nothing. Equality, ordering, and diff need two values; clone, patch
application, one-step shrinking, and `T -> U` mapping need a value in and a
value out. None can be written, and the comparison traits TQ-13 wants on
the shared protocol are among them. The only encoding boxes every member as
`Inspectable` and downcasts.

**Candidate fixes.**

- **A. Two more generated methods.** `zip[Z < Zipper](self, other: Self,
  z: mut Z)` calls `z.variants(a, b)` and then `z.member[F](m, a: F, b: F)`
  for matching variants; `rebuild[M < Mapper](self, f: mut M) -> Result[Self,
  M::Error]` calls `f.member[F](m, old: F) -> Result[F, M::Error]` per
  member. Both use M9's strengthened-bound rule. *Trade-off:* two more
  sealed traits. Keeps typed, allocation-free code. Covers equality,
  ordering, diff, clone, patch, and shrink steps, but not `T -> U` across
  two types.
- **B. Typed member handles.** Generated code passes `Field[S, F]` (with
  `get(s: S) -> F`) in place of `Member`. A visitor holding a second `S`, or
  a source holding an original `S`, reads the same member. *Trade-off:* one
  mechanism for zip and rebuild, but visitors become generic in `S`, and on
  an enum `get` is valid only for the matching variant (a panic or an
  optional, and an optional allocates for reference-shaped members).
- **C. Keep these out of `Structure`.** Comparison derives stay compiler
  intrinsics (today's `@derive` list). Clone, diff, and patch are written by
  hand or go through `json.Json`. *Trade-off:* TQ-13's single protocol is
  not literal.

**Questions.** Q1-a: which of A, B, C? Q1-b: is `T -> U` mapping (row type
to domain type) in scope at all?

### P2. No Value-Free Typed Traversal

**Effect.** `describe() -> Shape` reports a `TypeShape` per member. A
`TypeShape` cannot reach the member type's own impl, and a `DeclarationId`
cannot be turned back into a shape. So schemas (JSON Schema, OpenAPI,
GraphQL, MCP), CLI help, DDL, and boundary type descriptions cannot describe
nested types, and a shape-based description disagrees with the members' own
derivations (case 6: `Address`'s snake case, `Money`'s string encoding).

**Candidate fixes.**

- **A. Restore decision 2's typed describe.** A generated
  `describe[D < Describer](d: mut D)` visits every variant and every member
  with its static type and no value, and a `Describer` impl may strengthen
  `member[F]` like a visitor. *Trade-off:* one more generated method and
  sealed trait. It is the Scala 3 `Mirror` element-type walk and the
  GHC.Generics `Proxy` walk.
- **B. Resolve `TypeShape.Named` at run time.** A registry from
  `DeclarationId` to each trait's description. *Trade-off:* dynamic lookup,
  and a missing `Schema` impl is found at run time, not at the opt-in site.
- **C. A probing mode for `build`.** A source may answer "no value" and ask
  for every variant. *Trade-off:* complicates `build`'s semantics for every
  source.

**Questions.** Q2-a: which of A, B, C? Q2-b: may a description be memoized
per (trait, type), since schemas are constants? Q2-c: does `describe()`
reflect the calling impl's member lines (is a `= pass` member absent, are
`+=` facts visible)?

### P3. Declared Defaults Unreachable At Run Time

**Effect.** `f = pass` uses a member's declared default statically, for
every construction. No source can decide at run time that an input lacks
the member and the default applies. Decoders with optional keys, CLI flags,
tool arguments, proto2 defaults, and builders all need that decision. Tier-1
`Default` silently ignores declared defaults (case 3). Facts that carry a
value of the member's type (a default, an example, a replacement) cannot be
read at that type either, because `Member` is not generic.

**Candidate fixes.**

- **A. A presence query.** `Source` gains `fn present(mut self, m: Member)
  -> Result[bool, Self::Error]`. Generated `build` calls it first for each
  member that has a declared default and evaluates the default when it
  returns `false`. *Trade-off:* one method, no allocation, and the source
  never sees the default's value (help text still cannot print it).
- **B. A typed slot.** Generated code passes `Slot[F]`, with
  `default() -> F?`, in place of `Member`. *Trade-off:* a source can use and
  display the default, and facts could be typed against `F` too (P14). The
  optional result allocates unless the compiler removes it.
- **C. Keep the static rule.** Document that derived decoders require every
  member without `= pass`. *Trade-off:* serde's `#[serde(default)]` is one of
  its most used attributes, and Swift's lack of it is a standing complaint.

**Questions.** Q3-a: which of A, B, C? Q3-b: should the declared default
also be what a derived `Default` returns, without `= pass` lines?

### P4. Marker Templates Check Nothing

**Effect.** A template for a trait without methods (`Eq`) has no body, so
it calls neither `visit` nor `build`, and M9 rule 3 produces no member
obligation. `impl Eq for Reading by Structure` is accepted for an `f64`
member. The same holds for any future marker, such as a boundary-safety or
redaction marker.

**Candidate fixes.**

- **A. Bodiless means "every member implements the trait".** A template
  with no body gets the obligation `F < Trait` for each visited member.
  *Trade-off:* a special rule for bodiless templates only.
- **B. A checking clause.** A template names a visitor type whose member
  bound is checked without generating code (for example
  `impl[T] Eq for T by Structure checks EqCheck`). *Trade-off:* new syntax;
  works for markers with unusual rules.
- **C. No marker templates.** A bodiless template is an error, and markers
  are hand-written or stay intrinsic. *Trade-off:* `Eq` rides with
  `PartialEq` by hand.

**Questions.** Q4-a: which of A, B, C? Q4-b: should the TQ-12 partner rule
apply, so `Eq` by `Structure` requires `PartialEq` by `Structure` (or by
hand) on the same type?

### P5. The Style Hook Is A Trait Associated Function

**Effect.** Three separate failures.

1. **Dynamic safety.** An associated function makes a trait not dynamically
   safe (09). A derivable `Display` breaks `std.error.Error < Display +
   Inspectable`, and `json.Encode` can no longer be a value type
   (`List[json.Encode]`).
2. **Name collisions.** Two traits with a hook of the same name on one type
   (json's `Encode` and `Decode` both use `visitor()` in the record) make
   `User::visitor()` ambiguous. 09 gives no form that selects one trait's
   receiverless associated function for a type, and the meaning of
   `Self::visitor()` inside a template is unspecified.
3. **Grammar.** `Self::visitor()` and `Self::build(...)` are not primary
   expressions (02 Primary Expressions); `T::visitor()` is.

**Candidate fixes.**

- **A. A separate configuration trait as a template bound.**
  `impl[T < json.Configured] Encode for T by Structure`, where `Configured`
  holds the hook. *Trade-off:* derived traits stay dynamically safe and
  names stop colliding. One configuration impl is then shared by `Encode`
  and `Decode`, which reopens M10 and M11's per-trait locality.
- **B. Configuration in the `by` clause.**
  `impl json.Encode for Order by Structure(json.json(key=.Some(legacy_key)))`,
  read by the template as a typed constant. *Trade-off:* new syntax. No trait
  member, so no dynamic-safety or naming issue. Tier 1 is already a constant,
  so nothing is lost.
- **C. Exempt hooks from dynamic safety.** A hook is callable only as `T::`
  inside a template and is left out of the trait-value table. *Trade-off:* a
  special case in 09's dynamic-safety rule, and the naming issue remains.

**Questions.** Q5-a: which of A, B, C? Q5-b: inside an impl, does
`Self::f()` select the enclosing impl's trait first? Q5-c: should the grammar
accept `Self::f(...)`?

### P6. No Bound Rule For Generic Targets

**Effect.** M9 checks member obligations at the opt-in site, but M1-M11 does
not say which bounds the derived impl for `Box[T]` has. Tier 1 has no header
to write them in. `Set[T]` members need bounds beyond `T < Trait`, and
recursive `Tree[T]` needs a coinductive assumption.

**Candidate fixes.**

- **A. The 09 built-in rule:** `T < Trait` per parameter used in a visited
  member, with an error that suggests a tier-2 header otherwise.
  *Trade-off:* predictable. Wrong for `Set[T]`-like members, which then need
  tier 2.
- **B. Decision 6 inference,** traced through the unique impl per
  constructor. *Trade-off:* right in more cases, but a public impl head then
  depends on private member types.
- **C. Tier 1 rejects generic targets;** tier 2 always writes the header.
  *Trade-off:* explicit, noisy for `Box[T]`.

**Questions.** Q6-a: which of A, B, C? Q6-b: is recursion checked
coinductively (the impl being derived is assumed while checking its
members)? Q6-c: may bounds that come from private members appear in a public
impl head?

### P7. Declaration-Order Decode And Per-Call Recomputation

**Effect.** `build` asks for members in declaration order, so every
order-free input (JSON objects, protobuf, argv, rows by name) is indexed per
object or rescanned per member. Per-member keys (`facts.find`, case
conversion) and tier-2 style values are rebuilt on every call, and
`variant([...])` builds a list per call. The "no per-member allocation"
claim holds for generated code only.

**Candidate fixes.**

- **A. Input-driven build.** Generated `build` asks the source for the next
  member index and keeps one local per member, as serde does. *Trade-off:*
  larger generated code and a harder `Source` protocol.
- **B. A memoized plan per (trait, type).** A library computes per-member
  keys once from `describe()` and the style, and the visitor receives the
  plan. This is decision 10's cache, narrowed. *Trade-off:* one runtime
  cache with cycle rules, like chapter 14's registry.
- **C. Static tables.** The spec states that `Member`, `Variant`, and
  `choices` lists are constants. *Trade-off:* removes only the list
  allocation, but it is free.

**Questions.** Q7-a: is order-free decoding a goal of the protocol or of
each library? Q7-b: does a per-(trait, type) cache come back? Q7-c: are
member and variant tables guaranteed constant?

### P8. No Requirement Row Or Suspension Inside Traversal

**Effect.** `visit`, `build`, `Visitor::member`, and `Source::member` have
the empty row and do not suspend. Decoders that need I/O per member (lazy
relations, streaming input from an async reader) cannot be derived. A tool
template cannot name its target's row `Rq`.

**Candidate fixes.**

- **A. Suspending variants** `visit!`/`build!` with a row parameter, used
  by `Source!`-style traits. *Trade-off:* doubles the protocol, and a
  suspension frame per member call.
- **B. Keep traversal pure.** I/O happens before `build`, which is how case
  5 works today. Document it. *Trade-off:* Rust's serde is also
  synchronous; async formats buffer first.
- **C. For function targets only,** a function structure exposes its row as
  an associated row. *Trade-off:* needs FN_TYPE option B first.

**Questions.** Q8-a: is per-member I/O in scope? Q8-b: how does a tool
template name the target function's row?

### P9. A Member Is Either Fully Visited Or Absent

**Effect.** A member line gives two settings: visited with the full
obligation, or `= pass` (absent, declared default on build). Redaction
(label shown, value hidden, no `Debug` required), error `cause()` (visit
only the member that is an error), a custom codec for one member, and
"skip on decode but not on encode" under tier 1 all need a third setting.
M6 answers "change the member's type", which leaks a wrapper type into every
use of the field.

**Candidate fixes.**

- **A. A codec line in tier 2** (`token = with(RedactedToken)`), which
  routes one member through a named type and changes its obligation. This is
  decision 3's `With[V, Codec]`. *Trade-off:* a second
  compiler-interpreted form next to `= pass`.
- **B. An opaque line** (`token = opaque`): the visitor's `member_opaque(m)`
  is called without the value, so there is no obligation. *Trade-off:* covers
  redaction and cause selection, not custom codecs.
- **C. Wrapper types** (M6 today). *Trade-off:* no new rule; the field's
  type changes for every user of the type.

**Questions.** Q9-a: which of A, B, C? Q9-b: should tier 1 be able to
express per-direction skips at all, or is that always tier 2?

### P10. Derivation Cannot Declare Types Or Methods

**Effect.** Templates implement methods of an existing trait. A builder
(per-member setters), a patch type, a field-key enum, and a mock provider are
new declarations, so they cannot be derived. hd's data literals with
defaults and copy-update cover most builder needs.

**Candidate fixes.**

- **A. Accept it.** Builders are literals; patches go through `json.Json`.
  *Trade-off:* no typed diff or patch.
- **B. One generated companion per type:** a field-key enum (Swift's
  `CodingKeys`) that templates can use as a typed member name. *Trade-off:*
  a new generated declaration kind, and values stay untyped.
- **C. External generation** from interface files (the OPEN_ISSUES mock
  option). *Trade-off:* a build step outside the language.

**Questions.** Q10-a: is a typed builder a goal? Q10-b: are typed patches a
goal? Q10-c: are mock providers for traits in the scope of derivation?

### P11. Member Model Unspecified

**Effect.** The record does not say what a member is in several cases the
spec already has.

**Questions.**

- Q11-a: are common enum fields visited, in which position, and what does
  `build` do with them, given that constructor expressions compute them but
  a `mut` root can reassign them?
- Q11-b: do newtypes get `Structure` (one member named `0`), derive through
  their base (TQ-11), or neither? Does `decorated_decl` gain `type_decl`?
- Q11-c: is an embedded part one member or flattened? Does `Member` carry
  an `embedded` flag? Can a member line address a part's members?
- Q11-d: how does a member line name a payload field that several variants
  share (`Login.user`)? What does `Variant = pass` mean?
- Q11-e: may a tier-2 block live in another module of the owning package,
  and does it then see private members?
- Q11-f: are GADT variants and variant-local generic parameters rejected, or
  visited but not built?
- Q11-g: which fields does `Member` have (`doc`, `position`,
  `has_default`, `embedded`), and `Variant` (`member_count`, payload
  members)?

### P12. No Impl Family Indexed By Member Types

**Effect.** A template gives one trait instantiation per opt-in. Error
enums need one `From[P]` per single-payload variant, each with a different
`P`, and a source cannot turn a `P` into the member's `F`. Derived `From`,
which Error Conversion decision 10 deferred to this protocol, cannot be
written.

**Candidate fixes.**

- **A. A `std.convert` rule** outside `Structure`: an annotation on the enum
  generates `impl From[P] for E` per single-payload variant, with an overlap
  diagnostic naming both variants for a repeated `P`. *Trade-off:* a
  compiler rule for one trait, but the error story depends on it.
- **B. Per-variant templates:** a template form that applies once per
  variant with the variant's payload type bound (`impl[T, P] From[P] for T
  by Variant`). *Trade-off:* a second template kind.
- **C. Hand-written impls,** two lines each. *Trade-off:* no derivation for
  the most common error-enum task.

**Questions.** Q12-a: which of A, B, C? Q12-b: is variant selection by a
fact (`@errors.from`) or by "exactly one payload field"?

### P13. Tier-1 Meaning Depends On Return Types And Package Contents

**Effect.** Rule 5 (still marked proposed) derives every trait in `V`'s
package whose hook returns `V`. So:

1. whether `@db.table(...)` derives impls or attaches a fact depends on
   whether some db trait returns `Table`;
2. a library that adds a trait returning its style type changes every
   downstream annotated type on a version bump, and can create
   `overlapping-impl` with a hand-written impl (case 18);
3. tier 1 cannot pick a subset (`std.cmp`'s four traits, case 1);
4. one skipped member moves every visiting library to tier 2 (case 15).

**Candidate fixes.**

- **A. Tier 1 names a declared group.** The library declares which traits an
  annotation derives (decision 12's group), and a change to the group is a
  visible interface change. *Trade-off:* one declaration form.
- **B. Tier 1 plus member edits.** Tier 1 supplies configuration only, and a
  tier-2 block for the same trait adds member lines instead of overlapping.
  *Trade-off:* reverses rule 6, and the reader looks in two places.
- **C. Keep rule 5,** with tooling that prints which impls each annotation
  produced. *Trade-off:* no language change; silent fan-out stays.

**Questions.** Q13-a: which of A, B, C? Q13-b: is rule 5 confirmed?

### P14. Facts Are Untyped Against Members And Checked Only At Run Time

**Effect.** Chapter 14 types member metadata as `FieldMetadata[T]`, so a
metadata value for the wrong member type is a compile error. M2 facts are
typed values, but nothing relates them to the member type, so
`@validate.range(1, 5)` on a `string` is accepted and ignored. Semantic
errors (duplicate protobuf tags, two renames to one key, a placeholder that
names no member) surface at run time or never. Compile-time facts cannot
hold a compiled regex.

**Candidate fixes.**

- **A. Keep chapter 14's typing:** a fact on a member of type `U` must
  implement `FieldMetadata[U]`. *Trade-off:* library authors write the
  marker impls; wrong placements become compile errors.
- **B. A compile-time check hook:** the library supplies
  `fn check(shape: Shape, style: V) -> List[string]`, run by the tier-1
  evaluator at the opt-in site. *Trade-off:* widens compile-time evaluation
  beyond annotation values.
- **C. Run time plus a toolchain pass** that evaluates every derivation's
  checks at build time (the record's old question 13 A). *Trade-off:* no
  language change; errors are late.

**Questions.** Q14-a: which of A, B, C? Q14-b: may facts hold values built
at first use (a compiled regex), or only compile-time data?

### P15. Foreign Types Cannot Be Derived

**Effect.** `impl json.Encode for dep.geo.Point by Structure` in an
application is `orphan-impl`. Decisions 9 and 11 allowed a standalone
derive with public members plus the root-application exception; M1-M11 has
neither.

**Candidate fixes.**

- **A. Root-application exception for `by Structure` impls,** public members
  only. *Trade-off:* the package that later adds the impl breaks resolution,
  as chapter 14 already accepts for annotations.
- **B. A mirror type:** a local data type with the same public members,
  plus a declared conversion (serde's `remote`). *Trade-off:* the user writes
  the members twice.
- **C. Newtype plus delegation.** *Trade-off:* needs Q11-b answered.

**Questions.** Q15-a: which of A, B, C?

### P16. Templates Cannot Be Extended Or Composed

**Effect.** A tier-2 override replaces the template's method; it cannot
call it. `Structure` bounds nothing but templates, so libraries cannot share
structural helpers across templates. Rule 7 forbids a generic visitor that
forwards `member` to an inner visitor, so tracing, filtering, and redaction
wrappers must be written per concrete visitor.

**Candidate fixes.**

- **A. A call to the template body** from an override (a reserved name such
  as `T::by_structure_check(self, ...)`). *Trade-off:* new name resolution.
- **B. Private structural helpers:** `fn f[X < Structure]` allowed inside
  the trait's module, not public. *Trade-off:* M9's concrete-visitor rule
  must extend through those helpers.
- **C. Bound-forwarding visitors:** a visitor generic in an inner visitor
  may call its `member` when it repeats the inner bound. *Trade-off:* the
  M9 exception grows.

**Questions.** Q16-a: is extension (override plus template body) needed?
Q16-b: is visitor composition needed?

### P17. `build` Returns Readonly `Self`

**Effect.** `Structure::build` returns `Result[Self, S::Error]`, readonly
by 04 Parameters And Results. A template cannot return `mut Self`, so every
derived decoder, `Default`, and generator produces readonly values, and the
caller copies to mutate. For a `field: mut U` member the record does not say
whether the generated call is `member[U]` or `member[mut U]`.

**Candidate fixes.**

- **A.** `build` returns `mut Self` (it always constructs a fresh value),
  and templates may declare `mut Self` results. *Trade-off:* none known
  beyond the signature change.
- **B.** Keep readonly results, and callers copy. *Trade-off:* a copy per
  decode, and copies of values with `mut` members stay readonly.

**Questions.** Q17-a: A or B? Q17-b: which type does a `mut U` member pass
as `F`?

### P18. The Reference Example Does Not Parse

**Effect.** The record's full example uses spellings the chapter-02 grammar
rejects, so it cannot be checked with the reference parser: named arguments
with `:` (`json(case: .Camel)`), data construction with call syntax
(`Style(case: case)`), `mut` at argument sites (`self.visit(mut
Encoder(...))`), `use json`, `back: User = ...` without `let`, and
`Self::visitor()`. It also writes `Result[(), E]` and `.Ok(())` where the
spec writes `Result[void, E]` and `.Ok()`. Separately, `by Structure` shares
its syntax with trait delegation `by E`: a data type that embeds a user type
named `Structure` makes `impl Trait for C by Structure` ambiguous.

**Questions.** Q18-a: should the example be rewritten in spec syntax when
the design is next edited? Q18-b: is `by Structure` a reserved form, or
resolved by name (embedded field first)?

### P19. Compilation And Interface Model Not Stated

**Effect.** Templates and generated `visit`/`build` are specialized per
(type, visitor), and tier-1 annotation functions run at the user's compile
time. 10 Name Resolution Across Packages lists only pack and reified bodies
as carried in interfaces and says an interface depends on no function body.

**Candidate fixes.**

- **A. Add three kinds of carried body** (templates, the visitors they name,
  annotation functions) to the interface list, and state the code-size model
  (types times visitors). *Trade-off:* interfaces grow; a dependency bump
  can change derived behavior without a source change.
- **B. Restrict annotation functions** to a data literal of the style type,
  so their "body" is part of the signature. *Trade-off:* no computed
  defaults in annotations.

**Questions.** Q19-a: A, B, or both?

### P20. Wire Stability Is Silent

**Effect.** Reordering members or variants changes positional encodings,
`Variant.index`, and derived `Ord` without a diagnostic. Renaming a member
changes every name-based format.

**Candidate fixes.**

- **A. A stability fact** a library may require (explicit tags or names on
  every member), with a toolchain check against the previous interface.
  *Trade-off:* tooling work; protobuf's model.
- **B. Documentation only.** *Trade-off:* Rust's status quo.

**Questions.** Q20-a: A or B?

## Comparison With Other Languages

| Need | Rust | Haskell | Scala 3 | Swift | Go | Zig | hd M1-M11 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| One value in (encode, hash) | serde `Serialize`, derive macros | `from :: a -> Rep a` | `productIterator`, `Mirror` | `Encodable` | `reflect.Value` | `inline for` over `@typeInfo` | `visit`: yes |
| Build from input | serde `Deserialize` (input order) | `to :: Rep a -> a` | `fromProduct` | `Decodable` keyed containers | `reflect.New` | `@field` assignment | `build`: yes, declaration order |
| Two values (eq, ord, diff) | per-trait derive macros | `from` on both, zip `Rep` | two products | compiler-synthesized `Equatable`, `Comparable` only | `reflect.DeepEqual` | `std.meta.eql` | **no** (P1) |
| Value to value (clone, patch) | derive macros | `to . f . from` | `fromProduct(map(...))` | no | `reflect` set | comptime | **no** (P1) |
| No value (schema) | schemars macro | classes on `Rep` via `Proxy` | `MirroredElemTypes` summons | no | `reflect.Type` | `@typeInfo(T)` | **no** (P2) |
| Declared defaults | `#[serde(default)]` | none (no field defaults) | macros only | ignored by synthesis | zero values | field default pointer | **static only** (P3) |
| Per-member custom codec | `with = "..."` | newtype | given instances | custom `CodingKeys` code | tags plus `Marshaler` | hooks | **type change only** (P9) |
| Foreign types | `remote` mirror | orphan instances | givens anywhere | extensions, without synthesis | yes | yes | **no** (P15) |

The pattern: systems with a full representation value (GHC.Generics
`from`/`to`, Scala's `Mirror` plus products, Zig's comptime field list)
get zip, map, and value-free walks for free, because the library can call
the representation twice, transform it, or walk its type. hd's visitor
design trades that for no generic-level machinery and zero-cost typed calls,
but a one-value visitor and a from-nothing source are two of the five
traversals these systems provide. Rust gets the other three by writing a
separate procedural macro per trait. hd has no macros, so it needs the
generated traversals themselves (P1, P2).

Swift is the closest precedent for P3: synthesized `Codable` ignores
property defaults, and it is one of the most cited complaints about it.
Kotlin's serialization plugin reads constructor defaults, and serde added
`default` early; both show that the need appears as soon as data types have
defaults, which hd's do.

## Parse Log

Every `text` block above was extracted and checked with the chapter-02
reference parser (`spec/reference-parser`, `parseSource`) on 2026-09-27.
Parsing checks syntax only; names such as `json`, `Member`, `Structure`, and
the library types are not resolved, and nothing here was type-checked. The
prototype compiler (`bin/hd.js`) implements none of the derivation surface.

| Blocks | Result |
| --- | --- |
| The `std.structure` surface; every library side (cases 1-11, 13, 16); every tier-1 user side; the tier-3 impls; the workaround code in cases 4, 12, 14, and 16 | Parse (40 blocks). |
| Case 3 `Default` tier-2 block, case 5 `db.Record` tier-2 block, case 15 tier-2 blocks | `syntax-error` at the first M3 member line (`timeout_ms = pass`, `cache = pass`, `session = pass`), as expected: member lines are new syntax. These blocks start with `# Hypothetical syntax`. |

The blocks that parse rely on four spelling choices that differ from the
design record (P18): `T::` in place of `Self::` inside templates, `=` for
named arguments, brace data literals, and no `mut` at argument sites. With
the record's spellings the same blocks fail. Probes, not shown:
`Self::visitor()` as an expression, `Style(case: .Camel)`, and
`self.visit(mut Encoder { out: out })` are each a `syntax-error`.
