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

## Why hd

- **Readable like Python.** Indentation and one obvious way to write things.
- **Simple like Go.** One package model with minimal version selection and
  no macros.
- **Safe like Rust.** Traits, generics, exhaustive `match`, `Result`, and
  tracked `mut`.
- **Reviewable by design.** A function's signature shows everything it can
  touch, so a reviewer reads one line, not the body.
- **Library, not compiler.** Typed facts and templates derive `Eq`, `Hash`,
  `Debug`, `Default`, and your own formats in plain hd.
- **Sandboxed by default.** Wasm GC, with host access only through
  capabilities.

## Start

Read the [guide](guide/README.md), take the
[Language Tour](guide/LANGUAGE_TOUR.md), and use the
[specification](spec/README.md) for the exact rules. The toy compiler is in
[src](src/README.md), its standard library in [lib/std](lib/std/), and the
roadmap in [future-work](future-work/README.md).
