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
use std.testing.assert_equal
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
