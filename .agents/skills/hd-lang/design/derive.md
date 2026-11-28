# Derive It: Built-In Derives And Your Own

hd has no macros. Code that follows a type's fields is **derived**:
`@derive(Trait)` on a struct or enum writes `impl Trait for` that type
from its fields. The built-in derives cover equality, ordering, hashing,
debug text, JSON, command-line options, validation and test data
([below](#the-built-in-derives)).
A **derive template**, written in plain hd, makes your own trait derivable
the same way.

## When

| You are about to | Do |
| --- | --- |
| write `==`, ordering, hashing, `Debug`, JSON, `Cli`, `Validate` or `Arbitrary` by hand | `@derive` it; configure it with field facts such as `@rename` or `@range` |
| write the same field-by-field `impl` for a third type | write a template for the trait, then `@derive` it everywhere |
| make one type's derived trait treat one field differently | a derivation block: `impl Trait for Type by Structure:` |
| attach a setting to a field that your template reads | a fact: any value, written `@name(...)` on the field |

A template is the only way to make `@derive(YourTrait)` work. Without
one, the derive is `underivable-trait`, and no decorator on the trait
fixes that.

## The built-in derives

`@derive(...)` writes a trait's implementation from the type's fields.
Derive before writing an `impl` by hand: a derived parser, codec or
checker stays in step with the fields.

| Derive | Gives | Import |
| --- | --- | --- |
| `Eq` | `==` and `!=` | none |
| `Eq, PartialOrd, Ord` | `<`, `>`, `sorted()`, `max` (list all three) | none |
| `Eq, Hash` | use as a map key or set element | none |
| `Debug` | `debug(x)`, and `assert_equal` on the type | none |
| `Default` | `T::default()`, from the field defaults | `std.ops.Default` |
| `Serialize, Deserialize` | JSON `encode` and `decode`, `env_decode`, regex `decode`, HTTP `json` and `set_json`, query and form values | `std.encoding` |
| `MapKey` | a payload-free enum as a JSON object key | `std.encoding` |
| `Cli` | a command-line parser with `--help` ([how-to/cli-tool.md](../scenarios/cli-tool.md)) | `std.cli` |
| `Validate` | `value.validated()`, checking the rule facts on its fields ([how-to/configuration.md](../scenarios/configuration.md)) | `std.validate` |
| `Arbitrary` | generated inputs for `it_prop` | `std.testing` |

Values copy, so there is no `Clone` to derive. An `@error` enum gets
`Display` and `Error` from its messages ([errors](errors.md)).

```hd
@derive(Eq, PartialOrd, Ord, Hash, Debug)
struct Version:
    major: u32
    minor: u32

impl Display for Version:
    fn to_string(&self) -> string: "${self.major}.${self.minor}"

a := Version { major: 1, minor: 2 }
b := Version { major: 1, minor: 10 }
let mut seen: Map[Version, string] = {}
seen[a] = "old"
println("$a < $b: ${a < b}; newest ${[a, b].sorted()[1]}; ${seen.len()} ${debug(a)}")
```

```output
1.2 < 1.10: true; newest 1.10; 1 Version { major: 1, minor: 2 }
```

Implementing `Display` is what lets a type appear in `"$value"`.

**Field facts** configure one derive from the field. `@rename("qty")` and
`@skip()` change the JSON view; `@rename_all(.CamelCase)` on the type
renames every field. `@short('n')` and `@positional` shape a `Cli`
argument, and `@range(1, 99)`, `@length(1, 80)` and the other
`std.validate` rules feed `Validate`. Each fact is imported from the
module of its trait.

```hd
use std.encoding.{Deserialize, MapKey, Serialize, rename, skip}
use std.encoding.json.{decode, encode}
use std.ops.Default

@derive(Eq, Hash, Debug, Serialize, Deserialize, MapKey)
enum Size:
    Small
    Large

@derive(Default, Serialize, Deserialize, Eq, Debug)
struct Order:
    @rename("qty")
    quantity: u32 = 1
    sizes: Map[Size, u32]
    @skip()
    cached_total: u32 = 0

order := Order { ...Order::default(), sizes: {.Large: 2}, cached_total: 7 }
text := encode(order)
println("${debug(Order::default())} $text")
println(debug(decode::[Order](text)))
```

```output
Order { quantity: 1, sizes: {}, cached_total: 0 } {"qty":1,"sizes":{"Large":2}}
Result.Ok(Order { quantity: 1, sizes: {Size.Large: 2}, cached_total: 0 })
```

## A template

A template is `impl[T] Trait for T by Structure:`. Inside it, `T`
implements `std.meta.Structure`, and `Structure::walk(&self, &mut walker)`
calls your `Walker` once per field, in declaration order, with the
field's handle and value. The handle's `info` is the field's `Member`:
its `name`, its `position`, and the `facts` written on it.

This package defines a `Secrets` trait, a `@secret()` fact, and the
template that lists the fields marked with it.

<!-- file: vault/hd.toml -->
```toml
[package]
name = "vault"
```

<!-- file: vault/src/secrets.hd -->
```hd
use std.meta.{Field, Structure, Variant, Walker}

## A fact: marks a field whose value must never be logged.
pub struct Secret:
    pass

pub fn secret() -> Secret:
    Secret {}

## The names of the fields marked `@secret()`, in declaration order.
pub trait Secrets:
    fn secret_fields(self) -> List[string]

# The walker the template drives: one call per field.
struct Collect:
    names: List[string]

impl[S] Walker[S] for Collect:
    type Error = void

    fn data_variant(&mut self, v: Variant[S]) -> Result[void, void]:
        .Ok(())

    fn variant(&mut self, v: Variant[S]) -> Result[void, void]:
        .Ok(())

    fn data_member[F](&mut self, h: Field[S, F], value: &F) -> Result[void, void]:
        if h.info.facts.find::[Secret]().is_some():
            self.names.push(h.info.name)
        .Ok(())

    fn member[F](&mut self, h: Field[S, F], value: &F) -> Result[void, void]:
        self.data_member(h, value)

    fn rest[T](&mut self, h: Field[S, List[T]], items: &List[T]) -> Result[void, void]:
        .Ok(())

# The template: each `@derive(Secrets)` instantiates it for that type.
impl[T] Secrets for T by Structure:
    fn secret_fields(self) -> List[string]:
        let mut collect = Collect { names: [] }
        _ := Structure::walk(&self, &mut collect)
        collect.names
```

Any module, and any test, can now derive it:

<!-- file: vault/src/login.hd -->
```hd
use pkg.secrets.{Secrets, secret}

@derive(Secrets)
pub struct Login:
    pub user: string
    @secret()
    pub password: string
    @secret()
    pub token: string?
```

<!-- file: vault/tests/secrets.hd -->
```hd
use pkg.login.Login
use pkg.secrets.{Secrets, secret}

@derive(Secrets)
struct Card:
    @secret()
    number: string
    holder: string

it("lists the marked fields"):
    login := Login { user: "ada", password: "x", token: .None }
    assert_equal(login.secret_fields(), ["password", "token"])
    assert_equal(Card { number: "4242", holder: "ada" }.secret_fields(), ["number"])
```

The pieces:

- **`Walker[S]`** has five methods, all required. For a struct, each
  field arrives at `data_member` (`member` and `rest` serve other
  shapes). For an enum, `variant` or `data_variant` names the value's
  variant first. Return `.Ok(())` from the ones you don't need. `Error`
  is the walker's error type; `void` when walking cannot fail.
- **`h.info`** is the field's `Member`: `name`, `position`, `facts`,
  `doc`. `value` is the field's value, by reference. Its type `F` is
  generic, so a walker that only needs names never constrains it.
- **A fact** is any value. `@secret()` calls `secret()` and stores the
  result on the field; `h.info.facts.find::[Secret]()` reads it back, and
  is `.None` on unmarked fields. The deriving module imports the fact's
  function, as `login.hd` imports `secret`.
- **`T::facts()`** inside a template reads a type-level fact, written as
  a decorator before the struct.

`hd doc std.meta` lists the rest: `describe` and `build` for templates
that need no value or construct one.

## Rules

| Rule | Diagnostic |
| --- | --- |
| `@derive(Trait)` needs a template for `Trait` | `underivable-trait` |
| The template is in the trait's module, or a module of the same package that re-exports the trait | `misplaced-derivation` |
| Its body calls `walk`, `describe` or `build`; a template that reads no field is not one | `marker-template` |
| A trait has one template | `overlapping-impl` |

A hand-written `impl Trait for Type:` still works next to the template,
for a type that needs something different.

## One field, one trait: derivation blocks

A derivation block changes what one derive sees, for one type, without
touching the other derives. `cache = pass` leaves a field out:

```hd
use std.encoding.{Serialize, Deserialize}
use std.encoding.json.encode
use std.meta.Structure

@derive(Eq, Debug)
struct Session:
    user: string
    cache: List[string] = []

impl Serialize for Session by Structure:
    cache = pass

println(encode(Session { user: "ada", cache: ["x"] }))
```

```output
{"user":"ada"}
```

`Eq` and `Debug` still see `cache`; only `Serialize` skips it. Prefer a
field fact such as `@skip()` when every derive should agree.
