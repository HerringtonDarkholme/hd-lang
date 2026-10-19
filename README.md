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
struct User:
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

struct Column:
    name: string

fn column(name: string) -> Column:
    Column { name: name }

trait Table:
    fn columns() -> string

impl[T] Table for T by Structure:
    fn columns() -> string:
        let mut list = ColumnList { names: "" }
        _ := T::describe(&mut list)
        list.names

struct ColumnList:
    names: string

impl[S] Describer[S] for ColumnList:
    type Error = never

    fn variant(&mut self, v: Variant[S]) -> Result[void, never]:
        .Ok(())

    fn member[F](&mut self, h: Field[S, F]) -> Result[void, never]:
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

## Built for agents: three pillars

hd is judged by three pillars for agentic development, compared side by
side rather than folded into one score
([goals](docs/compiler/goals.md#how-it-is-judged-the-agentic-programming-language-arena)).
Correctness is a gate, not a pillar.

1. **Cheap for one agent to get right.** Elapsed time and tokens per
   correct solution, and compile time is agent wait time. Every signature
   is explicit, so the checker skips dependency bodies and an agent reads a
   header, never a body. Checking is incremental, with a per-module cache
   and early cutoff, and std is checked in advance. The targets: an
   edit-to-check of a 10k-line package in 50 ms at the median, a cold check
   in 1 s, an edit-to-test in 300 ms, and near-linear time on the
   pathological cases. Measured on 2026-10-17: edit-to-check takes 25 ms
   at the median (p95 27 ms), a cold check 111 ms, and edit-to-test 110 ms,
   all within target. On the pathological cases, 7 of 10 stay within 2 s and
   200 MB at 4x (the slowest, `large-enum-match`, takes 338 ms), but
   `large-enum-match` grows with exponent 2.42 against 1.3, and
   `deep-blocks`, `deep-parens`, and `method-chain` fail (nesting limit,
   stack overflow). Diagnostics are compact, and `hd check --format json`
   has a field for exact-edit fix-its that `hd fix` applies, but no corpus
   mistake offers one yet. Output is deterministic. Tests sit in the same
   file under `tests:`, and providers turn a fake clock or mailer into one
   line.
2. **Many agents on one machine.** Check results and compiled code are
   cached by content and shared across processes, and std is checked in
   advance and built into the binary. A program reaches the host only
   through the capabilities it is granted, with a time limit, so agents can
   run untrusted code side by side.
3. **Artifacts that are correct, then fast.** The portable conformance
   suite passes on two engines, V8 and wasmtime, with the same behaviour.
   Time, randomness and I/O sit behind capabilities, so a test can fix
   them. Property tests shrink failures, and only reachable code is
   compiled into the Wasm module.

The goal metrics are scripts under [bench/metrics](bench/metrics/README.md)
(`pnpm run metrics`). The current baseline (2026-10-17, commit
`493f95da4` plus the harness fixes of #347) is in [docs/audit/metrics-2026-10-17.md](docs/audit/metrics-2026-10-17.md):

- Pillar 1: 36 pass, 17 fail, 3 n/a.
- Pillar 2: 33 pass, 5 fail, 1 n/a.
- Pillar 3: 19 pass, 16 fail, 0 n/a (for example, runtime geomean 1.35x
  Node against 1.5x; allocations 0 B per iteration).
- Correctness gate: conformance passes 171 of 171; incremental soundness
  passes.

The remaining failures are measured values. They are listed in the
baseline with their targets.

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

This puts `hd` in `~/.cargo/bin`. To run programs without Node, build with
the wasmtime engine, `cargo install --path crates/hd_cli --features wasmtime`,
and choose it with `hd --engine wasmtime ...` or `HD_ENGINE=wasmtime`.
`hd build` takes the same choice and tunes its Wasm for that engine.

## Start

Read the [guide](website/guide/README.md), take the
[tour](website/tour/01-hello.md), and run `hd help` for the
commands ([The `hd` Command](website/guide/COMMANDS.md)). The
[specification](spec/README.md) holds the exact rules, and the standard
library is in [lib/std](lib/std/). To work on the compiler, see
[crates/README.md](crates/README.md). [docs/](docs/README.md) indexes the
compiler design, the open design questions, and the audit logs.
