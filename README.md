# hd-lang

**Rustic Effects on Pythonic Goland.**

A statically typed, indentation-based language that compiles to WebAssembly,
built for code that AI writes and humans review.

## Algebraic effects, emulated by three pillars

1. **Requirements declare.** `fn load!(id: Id) -> User $ Db + Cache`: after
   `$`, a function lists every capability it may use, and the checker
   enforces it.
2. **Providers handle.** `$.with(Db=mock_db): ...` binds a capability for a
   block, so tests, sandboxes, and production differ only in providers.
3. **`!` suspends.** `fn!` and `f!(...)` mark one-shot suspension. `all!`,
   `race!`, and `retry!` compose it.

Errors stay plain values: `Result[T, E]` and `?`.

### Why it matters

Without requirements, a function that sends mail and reads the clock looks
like any other, and a test must patch globals or pull in a mocking library to
check it. With them, the signature lists what the function touches, and the
test installs a fake mailer and a fixed clock for one block. Forgetting a
provider is a `missing-requirement` compile error, not a runtime surprise.

```hd
use std.time.{Clock, ManualClock, Timestamp}

trait Mailer:
    fn send!(mut self, to: string, body: string) -> void

# The signature says it all: this sends mail and reads the clock.
fn welcome!(email: string) -> void $ Mailer + Clock:
    let mut mailer = $.use(Mailer)
    mailer.send!(email, "Welcome, joined at ${$.use(Clock).now()}")

data Outbox:
    sent: mut List[string]

impl Mailer for Outbox:
    fn send!(mut self, to: string, body: string) -> void:
        self.sent.push("$to: $body")

tests:
    use std.testing.assert_equal

    it("welcome sends one mail at the fixed time"):
        let outbox: mut Outbox = Outbox { sent: [] }
        let clock: mut ManualClock = ManualClock::new(Timestamp::from_unix_milliseconds(0))
        $.with(Mailer=outbox, Clock=clock):
            welcome!("ada@example.com")
        assert_equal(outbox.sent, ["ada@example.com: Welcome, joined at 1970-01-01T00:00:00Z"], reason="one mail")
```

Run it with `hd test`. The playground has more
examples that each show one feature at work: property tests, typed errors,
exhaustive `match`, and `all!`.

## Annotations and derivation

Attach typed facts to declarations, and derive behavior from a type's
structure. `@derive(Trait)` generates an ordinary implementation from the
trait's template. A template is plain hd over the compiler's view of the
type's members, and it reads the facts it understands. Serialization,
equality, debug output, property-test generators, and error types all work
this way. So can your own library traits. There are no macros and no
codegen step: facts are typed values, and templates are checked like any
other code.

```hd
@derive(Serialize, Deserialize, Table)
data User:
    @column("user_id")
    id: i64
    email: string

pub fn main() -> void $ Console:
    println("SELECT ${User::columns()} FROM users")  # SELECT user_id, email FROM users
    text := encode(User { id: 7, email: "ada@example.com" })
    println(text)                                    # {"id":7,"email":"ada@example.com"}
```

`Serialize` and `Deserialize` come from `std.serde`; JSON reads them, and so
does the host boundary. `Table` is a library trait. Its template describes
the members and reads the `column` fact, which only `Table` cares about:

```hd
use std.json.encode
use std.serde.{Serialize, Deserialize}
use std.structure.{Structure, Describer, Field, Variant}

data Column:
    name: string

fn column(name: string) -> Column:
    Column { name: name }

trait Table:
    fn columns() -> string

impl[T] Table for T by Structure:
    fn columns() -> string:
        let list: mut ColumnList = ColumnList { names: "" }
        _ := T::describe(list)
        list.names

data ColumnList:
    names: string

impl[S] Describer[S] for ColumnList:
    type Error = never

    fn variant(mut self, v: Variant[S]) -> Result[void, never]:
        .Ok(())

    fn member[F](mut self, h: Field[S, F]) -> Result[void, never]:
        name := match h.info.facts.find::[Column]():
            .Some(column) => column.name
            .None => h.info.name
        separator := if self.names == "": "" else: ", "
        self.names = "${self.names}$separator$name"
        .Ok(())
```

See [Annotations](spec/lang/14-annotations.md) for facts, derivation
blocks, and `@error`.

## Why hd

- **Serialization, schemas, property tests, and fake data from your types.**
  Typed facts and structural derive generate them in plain hd. No macros, no
  codegen.
- **Effects you can review.** A signature shows every capability a function
  can touch, and tests swap providers instead of patching globals.
- **Sandboxed.** Wasm GC, with host access only through requirements.

## What hd borrows

- **Rust:** traits, `Result` and `?`, exhaustive `match`, explicit `mut`.
- **Effect:** requirements and providers, and `all!`, `race!`, `retry!`.
- **Python:** indentation and keyword arguments.
- **Go:** embedded structs, packages with minimal version selection, and
  `defer`.
- **Swift and Kotlin:** `T?` optionals.

## Start

Read the [guide](guide/README.md), take the
[Language Tour](guide/LANGUAGE_TOUR.md), and use the
[specification](spec/README.md) for the exact rules. The toy compiler is in
[src](src/README.md), its standard library in [lib/std](lib/std/), and the
roadmap in [future-work](future-work/README.md).
