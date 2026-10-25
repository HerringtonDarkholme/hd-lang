# Standard Library: The Bar And What Is Left

Status: the redesign of `lib/std` against the #350 bar is applied, except
the decisions listed under [Decided, Not Yet In std](#decided-not-yet-in-std).
The per-module review and the log of individual std calls are in git
history (`git show 28072b8a1:docs/future-work/STDLIB_REDESIGN.md`,
`git show 28072b8a1:docs/future-work/STDLIB_CALLS.md`); the spec under
`spec/std/` and `lib/std` are the record of what std does. Std API
choices are the orchestrator's to make (owner, 2026-10-20); language-tier
changes, intrinsics and host hooks still go to the owner.

## The Bar

The owner's eight principles for #350:

1. The library has the batteries a common program needs.
2. It follows the designs that Go, Effect and Rust prove.
3. It uses hd's own features: capabilities in requirement rows, `!` only
   where a call can suspend, `Result` and `?`, derive, and providers
   swapped in tests.
4. The common 80% is ergonomic, judged by realistic programs, not
   one-liners.
5. It is built from composable atoms, never a function that fixes one
   combination of options.
6. Its designs are stress-tested for correctness.
7. Its built-ins are fast, with no naive algorithm and no cost that an
   implementation cannot remove later.
8. Strongly typed interfaces by default; untyped APIs may provide an
   escape hatch. The default path reads outside data into the program's
   own types through derive and makes invalid states unrepresentable; a
   string-keyed layer (a dynamic `Json`, regex groups by name) stays
   public beneath it.

Correctness, ergonomics and usability come first in the order of work;
tuning waits, but no design may force a cost.

## The Shared Convention

Every I/O module follows one shape:

- **Builder.** A plain data value that does no I/O, from `Type::new(...)`
  or a named constructor such as `Request::get(url)`. Setters are bare
  nouns (`header`, `arg`, `timeout`), take `&mut self`, change the value
  in place and return `&mut Type`, so calls chain. `with_x` names a copy of
  an immutable value with one part replaced (`Path.with_extension`);
  `set_x` is only for a live handle's setting (`TcpStream.set_timeout`).
- **Terminal.** One bang method per builder does the I/O and names the
  capability in its row (`Request.send!`, `Command.run!`). An outcome that
  is not success, such as HTTP 404 or exit status 1, is an `.Err` that
  carries the whole result.
- **Accessors.** Stored data is a public field (`response.status`); a
  method exists only when it computes (`response.text()`). `to_x` builds a
  new value, `as_x` reads one cheaply, `Type::parse(text)` reads text.
  There is no `into_x`.
- **Typed by default.** Outside data reads into the program's own type
  (`response.json::[T]()`, `Options::parse_args()`, `env_as::[T](name)`);
  outgoing values go through `Serialize` (`request.json(value)`,
  `request.form(value)`). The string-keyed form stays public beneath:
  `Parsed.value(name)`, `response.header(name)`, `env(name)`.
- **Free functions.** One per host-trait method that starts work, named as
  the method (`read_text!`, `connect!`), so a program need not write
  `$.use`. None fixes a combination of options.
- **Errors.** One enum per module, `XError`. Variants name their subject
  (`NotFound(path)`); every module has `NotGranted(subject)` and
  `Other(message)`; each implements `Eq`, `Debug`, `Display` and `Error`.
- **Test providers.** The prefixes `Scripted`, `Memory`, `Manual` and
  `Seeded`, beside their trait. A `Scripted` provider starts empty with
  `new()`; `reply(key, result)` queues an answer per key, the last one
  repeating; `received()` lists what it was asked, in order.
- **Cost.** A setter copies nothing and appends in O(1) amortized. An
  accessor copies the payload at most once, and only to convert it.
  Strings are immutable UTF-8, built with `StringBuilder`. A design may
  buffer a whole body only where a streaming terminal can later sit beside
  it unchanged ([Streaming Design](STREAMING_DESIGN.md)).

## Decided, Not Yet In std

**Non-success outcomes and exit status.**
- `send!` fails for status 400 to 599 and `run!` for any non-zero exit;
  the error carries the whole response or output; a builder setter
  (placeholder name `ok_status`) accepts an expected status.
- `Error` gains `exit_code()` with default 1, and `Termination` reads it.
  `CliError` already maps `Help` to 0 and every other variant to 2.
- An error may choose exit status 0, with the test-runner guard: a test
  whose result holds `.Err` fails.
- std adds `impl Error for string`.

**One data model per concern.** `Deserialize` and `Serialize` serve the
document formats (JSON, YAML, TOML, HTTP payloads, SQL rows, binary
formats); the command line and the environment keep their own derives,
`Cli` and `EnvConfig`; `Validate` is its own derive. Each concern is
configured in its own derivation block, `impl Cli for T by Structure:
field = [facts]`.
- A shared `rename` / `rename_all` in the `Deserialize`/`Serialize` block,
  read by every document format (a format may override it in its own
  block); `Cli` and `EnvConfig` never read it.
- An `untagged` fact: the derived template buffers the value once and
  tries each variant in order.
- Checked readers: an entry that requires `T < Deserialize & Validate`
  and validates while reading.
- `read_parsed`, a std helper for the three-line `impl Deserialize` of a
  `Parse` leaf.
- `ValidationError` carries a member path that each reader maps to its own
  name (`$.port`, `--port`, `APP_PORT`).
- `flatten` and `rest` facts.
- A bytes kind in `Serializer`/`Deserializer`; JSON writes base64.
- Scalar-form facts in a format's block (such as unix seconds), newtypes
  otherwise.
- The `EnvConfig` derive for typed environment configuration.

Validation (`Validate`, checked readers, `ValidationError` paths) gets a
design note first, approved by the owner, then implementation (#846).
