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
    fn send!(&mut self, to: string, body: string) -> void

# The signature says it all: this sends mail and reads the clock.
fn welcome!(email: string) -> void $ Mailer + Clock:
    mailer := $.use(Mailer)
    mailer.send!(email, "Welcome, joined at ${$.use(Clock).now()}")

struct Outbox:
    sent: List[string]

impl Mailer for Outbox:
    fn send!(&mut self, to: string, body: string) -> void:
        self.sent.push("$to: $body")

tests:
    use std.testing.assert_equal

    it("welcome sends one mail at the fixed time"):
        let mut outbox = Outbox { sent: [] }
        clock := ManualClock::new(Timestamp::from_unix_milliseconds(0))
        $.with(Mailer=&mut outbox, Clock=clock):
            welcome!("ada@example.com")
        assert_equal(outbox.sent, ["ada@example.com: Welcome, joined at 1970-01-01T00:00:00Z"], reason="one mail")
```

Run it with `hd test`.

## Why hd

- **Effects you can review.** A signature shows every capability a function
  can touch, and tests swap providers instead of patching globals.
- **Behavior derived from your types.** `@derive` builds serialization,
  equality, debug output, property-test generators, and error types from a
  type's structure, in templates written in plain hd. No macros, no codegen
  step. See [Annotations](spec/lang/14-annotations.md).
- **Sandboxed.** A program compiles to one WebAssembly module and reaches the
  host only through the capabilities it is granted. Node and wasmtime are
  interchangeable engines, not different backends.
- **Fast feedback for agents.** Every signature is explicit, so the checker
  never reads a dependency's body. Checking is incremental and cached, and
  `hd check --format json` reports compact diagnostics with fix-its. The
  [goals](docs/compiler/goals.md) set the targets, and
  [bench/metrics](bench/metrics/README.md) measures them daily.

## What hd borrows

- **Rust:** traits, `Result` and `?`, exhaustive `match`, explicit `mut`.
- **Effect:** requirements and providers, and `all!`, `race!`, `retry!`.
- **Python:** indentation and keyword arguments.
- **Go:** embedded structs, packages with minimal version selection, and
  `defer`.
- **Swift and Kotlin:** `T?` optionals.

## Install

hd is installed from source for now. You need the Rust toolchain
([rustup](https://rustup.rs); the repository pins its version in
`rust-toolchain.toml`), git (used to fetch dependencies), and Node.js
(programs run on Node by default).

```sh
git clone https://github.com/HerringtonDarkholme/hd-lang
cd hd-lang
cargo install --path crates/hd_cli
```

This puts `hd` in `~/.cargo/bin`. To run programs on wasmtime instead of
Node, install with `--features wasmtime` and pass `hd --engine wasmtime`.

## Start

- [Learn hd in 10 minutes](website/guide/LEARN_IN_10_MINUTES.md), then take
  the [tour](website/tour/01-hello.md).
- [The `hd` command](website/guide/COMMANDS.md), or `hd help`.
- The [specification](spec/README.md) holds the exact rules, and the
  standard library is in [lib/std](lib/std/).
- To work on the compiler, see [crates/README.md](crates/README.md);
  [docs/](docs/README.md) indexes the design notes.
