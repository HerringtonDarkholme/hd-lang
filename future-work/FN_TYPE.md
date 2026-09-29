# Nominal Function Types: Per-Declaration Data For Tools

Status: open. Nothing here is accepted behavior. Owner decisions 1-9 of
this record (2026-09-27) are applied, and the specification is
authoritative for them:
[Function Type Constructors](../spec/07-functions.md#function-type-constructors),
[Captures](../spec/07-functions.md#captures),
[Variance](../spec/04-type-system.md#variance),
[Identity](../spec/05-expressions.md#identity),
[Implementation Targets](../spec/09-traits.md#implementation-targets), and
[Inspectable Types](../spec/09-traits.md#inspectable-types). The survey and
design options behind them are in this file's git history.

What remains is questions 9 and 10: how a tool adapter gets per-declaration
data about a function. They are listed as
[Function targets](../spec/14-annotations.md#undecided-parts) in the
specification.

## Owner Decisions

Not yet applied:

10. **Q9 and Q10 (per-declaration data for tools, item types) are parked
    with typed derivation;** tools register functions by hand for now.
    Since then, [Prefix Decorators](../spec/14-annotations.md#prefix-decorators)
    let a decorator attach a value to a function, and
    `shape_of(f).metadata[M]()` reads it. Deriving for functions still
    waits.

The options below were written before typed derivation was decided. "Design
1" is the function-type design now applied, "Design G" is typed
derivation's `Structure`, and decisions 6, 7 and 9 are the early typed
derivation decisions, since superseded by M1-M29.

## Per-Declaration Data For Tools

A tool adapter for

```text
pub fn get_user!(id: UserId, include_deleted: bool = false) -> Result[User, NotFound] $ Users:
    pass
```

needs (1) a typed callable, `SuspendFn[(UserId, bool), Result[User, NotFound], Users]`;
(2) the parameter names, documentation, and metadata, which `FnShape`
already carries; and (3) the defaults, which neither carries as values. A
function type gives (1) only. The options differ in how (2) and (3) reach the
adapter.

### Option A: Function types plus a shape at registration

**A1, a pair.** The adapter is a generic function over `Fn` or `SuspendFn` and takes
`shape_of(f)` beside the value:

```text
fn register() -> void:
    registry.add(mcp.tool(get_user, shape_of(get_user)))
```

```text
pub fn tool[Ps... < Decode & Schema, O < Encode & Schema, Rq](f: fn!(Ps...) -> O $ Rq, shape: FnShape) -> Tool[Rq]:
    pass
```

Nothing new beyond Design 1. But the name is written twice and nothing ties
the two arguments together: `mcp.tool(get_user, shape_of(delete_user))`
type-checks. The adapter can compare parameter counts and `TypeShape`s at
run time, not statically. Defaults are unusable, because `FnShape` records
only `has_default`.

**A2, a typed view.** A `fn_view(f)` intrinsic, as in Typed Derivation
question 8, returns one value holding the callable, the `FnShape`, and typed
default thunks (defaults are requirement-free by
[Default Values](../spec/07-functions.md#default-values), so a thunk can run
anywhere). With nominal types it can be typed by the function type:
`FnView[SuspendFn[(Ps...), O, Rq]]`.

```text
fn register() -> void:
    registry.add(mcp.tool(fn_view(get_user)))
```

This is Kotlin's `KFunction` and C#'s `Delegate.Method`, made explicit. It
is one intrinsic and one type. It does not make `@derive` meaningful on a
function: the adapter is an ordinary generic function, and a decorator such
as `@mcp.tool` could only attach metadata.

### Option B: Per-declaration item types

Each non-generic module-level function declaration also declares a unique
zero-size **item type**, printed `fn get_user`. The item type converts
implicitly to the declaration's function type. The compiler implements a
sealed `std.derive.FnStructure[F]` for it, the function analog of Design G's
`Structure`, where `F` is the function type. A value-free `describe` visits
each parameter with its static type and `ParamShape`. An `arguments` method
builds the argument tuple from a source, evaluating a declared default when
the source supplies no value. A derivable trait then maps its methods to
structural functions over `FnStructure`, and `@derive(mcp.Tool)` works on a
function as it does on a data type:

```text
@derive(mcp.Tool)
pub fn get_user!(id: UserId) -> Result[User, NotFound] $ Users:
    pass
```

```text
fn tool_structure[T < FnStructure[SuspendFn[I, O, Rq]], I, O < Encode & Schema, Rq]() -> ToolSpec[Rq]:
    pass
```

As in Design G, the structural function uses no packs. `I` is a
tuple-kinded parameter the function never spreads. The per-parameter bounds
(`Decode & Schema`) sit on the library's describer and argument-source
methods, as `param[P < Decode & Schema]`. Decision 6 then reports a
parameter that fails them at the derive site, naming the parameter.

```text
impl[Rq] Registry[Rq]:
    pub fn add[T < Tool[Rq]](mut self, item: T) -> void:
        self.tools.append(T::tool())
```

Registration is `registry.add(get_user)`: the argument's item type is
inferred for `T`, and `T::tool()` is a static call through the bound (TQ-9).
The zero-size value is never read. A hand-written implementation for one
function, the escape hatch decision 6 describes, needs a way to name the item
type in a head:

```text
# Hypothetical syntax: item types in implementation and derive heads
impl mcp.Tool[Users] for fn get_user:
    fn tool() -> ToolSpec[Users]:
        with_strict_schema(tool_structure[fn get_user, (UserId,), Result[User, NotFound], Users]())

derive mcp.Tool for fn dep.users.get_user
```

**How visible should item types be?** Rust gives every function name its item
type and coerces on demand, which produces the familiar "expected fn item,
found a different fn item" errors. The narrower rule proposed in
[question 10](#10-where-are-item-types-visible) keeps the item type only
where a generic parameter is inferred from the argument (`registry.add(get_user)`
with `add[T < Tool[Rq]]`). Everywhere else the name has its function type: a
binding `f := get_user` has type `fn!(UserId) -> ...`, and `[add, sub]` is a
`List[fn(i32) -> i32]` without a least-common-type rule for item types. Item
types are printed as `fn get_user` in diagnostics and can be written only in
implementation and derive heads.

**Scope.** Item types exist for module-level, non-generic functions, the
same targets `shape_of` accepts. Generic functions would need a complete
explicit instantiation (the former Shape Intrinsic Coverage issue, now
superseded by [typed derivation](TYPED_DERIVATION.md)).
Methods stay with P6. Closures never have item types.

**Ownership.** The item type belongs to the function's package, so
`@derive(mcp.Tool)` beside the declaration is always allowed, and a standalone
derive for another package's function follows decision 9 (the trait's
package, or the root-application exception).

### Comparison

| | A1 pair | A2 `fn_view` | B item types |
| --- | --- | --- | --- |
| New language surface | none beyond Design 1 | one intrinsic, one view type | a type category, one conversion, `FnStructure`, head syntax |
| Name and value tied statically | no | yes | yes |
| Defaults usable | no | yes, through thunks | yes, generated `arguments` |
| `@derive` and decorators on functions | no | no, metadata only | yes, same rule as data |
| Per-function hand-written impl | no | no | yes |
| Registration | `mcp.tool(get_user, shape_of(get_user))` | `mcp.tool(fn_view(get_user))` | `registry.add(get_user)` |
| Canonical identity of named functions | unspecified (decision 9) | unspecified (decision 9) | zero-size items would imply it, unlike decision 9 |

Option B is the only one that makes decision 7's "`@Facet` is sugar for
`@derive(Facet)`" meaningful on a function. Option A2 is the cheaper fallback,
and neither needs to block Design 1.

## Questions For The Owner

### 9. How do tool adapters get per-declaration data?

- **A1.** Function types plus `shape_of(f)` passed beside the value.
- **A2.** Function types plus a `fn_view(f)` intrinsic returning a typed view
  with the callable, the shape, and default thunks.
- **B.** Per-declaration item types with a compiler-generated
  `FnStructure`, so `@derive(mcp.Tool)` works on functions.

**Recommendation: B**, staged after Design 1, because it gives functions the
same derive and decorator rule as data (decisions 1, 7, and 8). If item types
are too much surface, **A2**; A1 lets the value and its shape disagree.

```text
@derive(mcp.Tool)
pub fn get_user!(id: UserId) -> Result[User, NotFound] $ Users:
    pass
```

### 10. Where are item types visible?

- **A.** Only where a generic parameter is inferred from the argument, and in
  implementation and derive heads written `fn name`; everywhere else a
  function name has its function type.
- **B.** Everywhere, as in Rust: every function name has its item type and
  converts to its function type on demand, including in least common types.

**Recommendation: A.** Bindings and list literals keep today's types, and the
Rust errors about distinct item types cannot arise.

```text
fn tools() -> mcp.Registry[Users]:
    registry := mcp.Registry::new()
    registry.add(get_user)    # T is the item type `fn get_user`
    handler := get_user       # the function type fn!(UserId) -> ...
    registry
```


## Parse Log

Every block without the **Hypothetical syntax** first line was parsed with
the reference parser on 2026-09-27; statements were parsed as module items
or inside the function shown. The hypothetical block is rejected because
`fn name` is not a type and a standalone `derive ... for` form is not in
the grammar.
