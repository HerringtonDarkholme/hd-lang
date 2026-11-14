<p align="center">
  <a href="https://herringtondarkholme.github.io/hd-lang/"><img src="website/assets/logo.svg" alt="hd" width="240"></a>
</p>

# hd-lang

**Rustic Effects on Pythonic Goland.**

A statically typed, indentation-based language that compiles to WebAssembly,
built for code that AI writes and humans review. hd is experimental: the
language and its standard library still change.

[Website](https://herringtondarkholme.github.io/hd-lang/) ·
[Playground](https://herringtondarkholme.github.io/hd-lang/playground.html) ·
[Learn hd in 10 minutes](website/guide/LEARN_IN_10_MINUTES.md) ·
[Specification](spec/README.md)

## A taste: algebraic effects in orbit

A satellite reports to Earth by radio. The radio and the clock are
**effects**: things the code needs from the world outside it. hd handles
them as **algebraic effects**, emulated with three pieces:

- **Requirements declare.** After `$`, `report!` lists the effects it can
  use: `Radio` and `Clock`. Using one it does not list is a compile error.
- **Providers handle.** `$.with(Radio=earth, Clock=clock)` supplies them
  for one block, so tests and production differ only in providers.
- **`!` suspends.** `fn!` and `f!(...)` mark one-shot suspension;
  `join!`, `race!`, and `retry!` compose it.

If you know Koka, OCaml 5, or Effect-TS, requirements are the effect rows
and providers are the handlers.

```hd
use std.time.{Clock, ManualClock, Timestamp, now}

# An effect is an ordinary trait. `!` marks a method that may suspend.
trait Radio:
    fn send!(&mut self, message: string) -> void

# After `$`: the effects this function may use, checked by the compiler.
fn report!(satellite: string) -> void $ Radio + Clock:
    radio := $.use(Radio) # whichever radio the caller provided
    radio.send!("$satellite is in orbit at ${now()}") # now() reads the Clock

# A provider: Earth implements Radio by keeping every message.
struct Earth:
    inbox: List[string]

impl Radio for Earth:
    fn send!(&mut self, message: string) -> void:
        self.inbox.push(message)

tests:
    use std.testing.assert_equal

    it("Earth hears the satellite"):
        earth := box(Earth { inbox: [] })
        # A clock that stands still at time zero.
        clock := ManualClock::new(Timestamp::from_unix_milliseconds(0))
        # Inside this block, Radio is Earth and Clock is the manual clock.
        $.with(Radio=earth, Clock=clock):
            report!("hd-1")
        assert_equal(earth.inbox, ["hd-1 is in orbit at 1970-01-01T00:00:00Z"])
```

The test swaps in a fake radio and a fixed clock, with no mocking library
and no patched globals.

## Derive from types, no macros

One `@derive` line gives a type JSON, random test data, debug output, and
equality. Each derive is a template written in plain hd, and you can write
your own ([Annotations](spec/lang/14-annotations.md)).

```hd
use std.json.{decode, encode}
use std.serde.{Deserialize, Serialize}
use std.testing.{Arbitrary, assert_equal, it_prop}

# JSON in and out, random values for tests, debug printing, and ==.
@derive(Serialize, Deserialize, Arbitrary, Debug, Eq)
struct Satellite:
    name: string
    altitude_km: i32

# A property: encoding then decoding gives back the same satellite.
fn round_trip!(s: Satellite) -> void:
    assert_equal(decode::[Satellite](encode(s)).expect("json"), s)

tests:
    # Runs round_trip on many generated satellites, edge cases first.
    it_prop("any satellite survives JSON and back", prop=round_trip)
```

The property test generates satellites, tries edge cases first, and shrinks
any failure to a small input.

## Built for agents

hd is judged by three goals for agentic development
([goals](docs/compiler/goals.md#how-it-is-judged-the-agentic-programming-language-arena)).
Correctness is a gate, not a goal.

1. **Cheap for one agent to get right.** Every signature is explicit, so
   an agent reads a header, never a body. Checking is incremental:
   edit-to-check took 52 ms at the median on a 10k-line package in the
   daily run of 2026-11-13 (target 50 ms).
2. **Many agents on one machine.** Check results and compiled code are
   cached by content and shared across processes. A program reaches the
   host only through the capabilities it is granted.
3. **Artifacts that are correct, then fast.** The conformance suite passes
   on V8 and wasmtime with the same behavior, and time, randomness and I/O
   sit behind capabilities that a test can fix.

[Daily results](https://github.com/HerringtonDarkholme/hd-lang/tree/metrics-data#readme)
track every target. The scripts are in [bench/metrics](bench/metrics/README.md).

## What hd borrows

| From | hd takes |
| --- | --- |
| Rust | traits, `Result` and `?`, exhaustive `match`, explicit `mut` |
| Effect | requirements and providers; `join!`, `race!`, `retry!` |
| Python | indentation and keyword arguments |
| Go | embedded structs, packages with minimal version selection, `defer` |
| Swift, Kotlin | `T?` optionals |

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
