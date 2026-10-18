# Serializable Closures And Remote Calls: Prototype Plan

This is the work plan for
[Serializable Closures And Remote Calls](SERIALIZABLE_CLOSURE_DESIGN.md)
(issue #166): how one hd program becomes several place-specific modules
whose calls meet on the wire, in slices of about an hour each. Every
slice lands with its spec rules and conformance fixtures, so the new
chapter grows one slice at a time. The plan uses no ownership: where the
design leans on borrows or owned values, it says how today's compiler
enforces the rule without them ([Without Ownership](#without-ownership)),
and a resource is a plain `Disposable` value that holds a live host
handle ([Using Bindings](../../spec/lang/06-control-flow.md#using-bindings)).

No slice is scheduled while the design is under review; slice 1's draft
is kept on the local branch `park/remote-slice1`.

- [1. What The Prototype Proves](#1-what-the-prototype-proves)
- [2. Where It Lives](#2-where-it-lives)
- [3. Slices](#3-slices)
- [4. Hard Cases By Slice](#4-hard-cases-by-slice)
- [5. Questions For The Owner](#5-questions-for-the-owner)

## 1. What The Prototype Proves

The design was stress-tested on paper with 39 cases and parsed as
written. Paper cannot show three things: that one source program builds
into several place-specific modules whose calls meet on the wire, that
a hostile caller cannot reach or forge anything the design says it
cannot, and that programs which use none of this keep their code byte
for byte. The prototype is the smallest end-to-end path that shows all
three. Its only transport runs every place in one process (design 4.3,
"in-process for tests"), so every fixture is deterministic and needs no
network.

### In Scope

- **Remote functions and calls.** `@remote`, `remote!(f, args)` and its
  `Result[T, RemoteError]`, the cold form `remote(f, args)`, call
  identities, routing by the `serves` lists, and the build errors for an
  unmarked crossing call and for a call no place can serve (design 2,
  4.2, 4.3, 4.4).
- **Places.** `[places]` in `hd.toml` with `profile`, `serves`,
  `opaque`, `trust` and `sends`; one module per place from one build;
  edges and their trust; `Caller` and its principal (design 3.6, 4.1).
- **Opaque values.** `Opaque[T]`, `opaque`, `open`, `OpaqueStore`, both
  strategies, the check at the home's boundary, and `bind_caller=true`
  (design 3.2, 4.5).
- **Portable closures.** `portable(fn ...)`, its capture rules, content
  identity, the build's portable table, local and remote calls, sealing
  on untrusted edges, and `sends` with `portable-row-not-bound` (design
  3.4, 3.5, 4.4, 4.7).
- **Failure.** `RemoteError`'s variants, cancellation across places,
  transport retries with request IDs (design 3.8, 4.3, 4.8).
- **Queries.** `@query`, tracked inputs, the memo and its revisions,
  `watch` and pushed results (design 3.7, 4.6).
- **Durability.** Recorded runs, replay after a crash, and host calls in
  flight (design 3.8, 4.8).

### Deferred

| Deferred | Why it waits | What the prototype does meanwhile |
| --- | --- | --- |
| Network transports: HTTP, WebSocket, worker messages, service RPC with mutual TLS | Deployment and host work outside the language; each needs its own authentication | only the in-process loopback transport; a build with `[places]` runs all its places in one process |
| The Origin and CSRF guard of the default untrusted transport (design 6) | It belongs to the first network transport | nothing to guard: loopback has no browser. The first network transport must ship with it, or "secure by default" does not hold |
| Generic fan-out providers such as `Workers.map![A, B]` and a generic `OrderStore.where!` (design 3.4, 3.5) | A host method with type parameters needs a codec per instantiation, which the host ABI has no form for | slice 23 routes a portable through a non-generic fixture trait; a generic method over `Portable` is `not-in-remote-prototype` |
| GPU places and kernels (design 3.9) | A separate study, by owner decision | nothing |
| Callbacks from home to a caller's closure under `ServerRefs` (design 4.5) | Home would call back over the connection while the caller waits, which needs nested routing in the transport | a `Portable` argument at a `ServerRefs` home is accepted only when it was made at that home; passing one of the caller's own is `not-in-remote-prototype` |
| A sealed value's expiry (design 3.2, 4.5) | No spelling is decided | `opaque` takes no expiry |
| Deployment keys and key rotation | Runtime configuration | the host draws one sealing key per run; the key identifier is fixed at 0 |
| A grant key for `Remote` in `[capabilities]` | Loopback reaches nothing outside the process | `Remote` needs no grant |

A program that reaches a deferred form gets `not-in-remote-prototype`,
reported once per form. The chapter's status note lists those forms, as
the ownership chapter's note lists its own.

## 2. Where It Lives

### The Spec

A new language chapter, `spec/lang/15-places-and-remote-calls.md`, holds
the rules of places, remote functions, calls, identities, opaque values,
portable closures, queries and durability. A new stdlib chapter,
`spec/std/remote.md`, holds the items of `std.remote`. Smaller edits
land where today's rules live: the `[places]` table and the boundary-safe
list in [Modules](../../spec/lang/10-modules.md), the compiler-known
decorators in [Annotations](../../spec/lang/14-annotations.md), the
build and run outputs in [Command Line](../../spec/cli/command-line.md),
and the fixture environments in the
[conformance README](../../spec/conformance/README.md).

The chapter starts with a status note that lists what is not yet
specified, and each slice removes its item from the note. A program that
uses an unlisted form is diagnosed, as
[Specification Status](../../spec/README.md#specification-status)
requires. The design's diagnostic names become the spec's codes as
written. The one build error the design names no code for, a plain call
to a function another place serves, uses `remote-call-unmarked`.

### The Places In A Build

A program without `[places]` is one implicit place that serves every
module, so every `remote!` call stays local and runs in process
(question 1). This covers every single-file fixture and every program
that has not split yet. With `[places]`, `hd build` compiles the front
half once and the back half once per place. Each place's module holds:

- the entry, when the place runs it (question 1);
- one adapter export per `@remote` function of the modules it serves,
  `hd.remote.<i>`, plus a custom section `hd.remote` that maps each call
  identity to its export;
- one adapter per `portable` literal in the build (design 4.7), named in
  the same section by content identity;
- one stub per crossing call site's callee, which imports `hd:Remote`
  `call.start` and `call.finish`.

A program that uses none of this emits exactly today's module: no
section, no export, no import. The tiny-program footprint (hello:
4257 bytes, sha256 starting `3972913d91b1ebc7`) is checked by every
slice that touches the back half, and any byte change blocks it.

### The Loopback Transport

The prototype's `Remote` provider runs each place as a Wasm instance of
its own module, all in one host process. A crossing call works as
follows:

1. The caller's stub writes the 16-byte call identity and the canonical
   arguments into its exchange buffer and calls `hd:Remote`
   `call.start`.
2. The host finds the serving place by the identity in that place's
   `hd.remote` section and starts a fresh instance of its module
   (question 5). It copies the bytes into the new instance's buffer and
   calls the adapter export.
3. The adapter decodes the arguments by the declared parameter types and
   binds the function's row from the place's providers, `Caller`
   included. It runs the function as a root suspension, driven by the
   host's own poll loop, and encodes the result.
4. The host copies the result bytes into the caller's `.finish` buffer
   as `.Ok` of them. A failed hop becomes `.Err` of a `RemoteError`:
   `Incompatible` for an unknown identity, `Rejected` for arguments that
   fail to decode, and `Panicked` for a panic at home.

The caller's `.start` returns pending, so the caller keeps running other
tasks while the call is in flight. No host call calls back into the same
instance, so the one-buffer rule of the exchange buffer still holds.
Node (`host/remote.mjs`, new) and wasmtime
(`crates/hd_run_wasmtime/src/host/remote.rs`, new) each implement it,
and the parity tests in `crates/hd_run_wasmtime/tests/parity/` compare
them.

### Without Ownership

The design states four rules in terms of borrows, owned values or
handles. Today's compiler enforces each one without `own` or `^`, and no
slice touches the ownership checker.

| Design rule | How the prototype enforces it |
| --- | --- |
| A capture is never a borrow or an owned value (design 4.7) | The checker already diagnoses any closure that captures a moded value (status note of [Ownership and Borrows](../../spec/lang/13-ownership.md)), so a `portable` literal never gets one. A moded type also fails `Boundary` by `own.boundary.not-safe` |
| No owned position, no type with an `own` part, and no type with a lifetime implements `Boundary` (design 4.5) | `Boundary` is decided by the same predicate as today's boundary check in `crates/hd_check/src/header.rs`, which already rejects moded types (`own.boundary.not-safe`). A moded type argument to a plain type parameter is already an error |
| "A connection is owned, so it cannot ride along" (design 3.4) | Under the `Disposable` decision a connection is a plain value that holds a live host handle, and live runtime handles are not boundary-safe (`module.boundary.not-safe`). The capture fails with the same `portable-capture-not-boundary-safe`. The spec example says "a live handle", not "owned" |
| No handle travels; captures are copies (design 3.4, 4.8) | Every value that crosses is boundary-safe, so it is immutable data with tree semantics. A `using` binding at home disposes when the call ends or is cancelled, through today's cleanup on cancellation |

### Which Stage Does What

| Stage | Adds | Granularity | Allocation and copies | Superlinear risk | Incremental |
| --- | --- | --- | --- | --- | --- |
| Manifest (`hd_project`) | `[places]` parsing and problems | per package | none | none | part of the manifest's key |
| Interfaces (`hd_resolve`) | the `@remote` and `@query` marks and each call identity | per item | one `Hash128` per marked function | none | a mark and a call identity are part of the interface hash, so adding `@remote` rechecks dependents and a body edit does not |
| Body checking (`hd_check`) | `remote!` typing, `portable` capture rules, `@query` rows | per expression | none | none | per module, as today |
| Placement (`hd_driver`, new task) | `remote-call-unmarked`, `remote-no-place`, `portable-row-not-bound` | per place, over each module's call summary | none | calls times places, linear | keyed by the places table and the modules' call summaries, so an edit to `[places]` reruns placement and codegen and never rechecks a body |
| Content identity (`hd_driver`) | SHA-256 of a function and its callees | per strongly connected group, only for `portable` literals and queries | one hash per group | none: one visit per call edge | memoized by TIR hash and callee identities |
| Collection (`hd_mono`) | adapter roots, lifted portable bodies | per place, per instance | none | none | code keys as today; an instance shared by two places is emitted once and linked twice |
| Emission (`hd_wasm`) | adapters, stubs, the `hd.remote` section, codecs for maps and opaque values | per instance | encoders and decoders shared per boundary type, as today | none | code keys as today |
| Run (`host/`, `hd_run_wasmtime`) | the loopback transport, seals, the memo, histories | per call | caller encode, host copy, callee decode; the same back | none | none |

**Build cost.** A place costs one more collection, optimization and
link over code that the cache mostly has, since a function instance
reached from two places has one code key. Checking runs once whatever
the number of places.

**Wire and run cost.** A call is a 16-byte identity plus canonical
arguments; a result is one tag byte plus the value (design 7). A local
`remote!` costs one direct call and one `.Ok` value, with no encoding.
A crossing call costs one encode, two buffer copies, one decode, the same
for the result, and one instantiation with module initialization
(question 5). A sealed value adds 33 bytes: a 12-byte nonce, a 16-byte
tag, a 4-byte key identifier and a strategy byte. A server reference is
10 to 17 bytes. A portable value is 16 bytes of identity plus its encoded
captures.

## 3. Slices

Each slice is about an hour of agent work and is one feature, in
dependency order. Every slice runs the usual gates, including the web
compiler's size gate when it touches a crate linked into `hd_web`, and
the parity tests when it touches `host/`. Every slice that touches the
back half reports the tiny-program footprint before and after. A
fixture the compiler cannot pass yet gets a `KNOWN_FAILURES.tsv` row
with its issue; no slice is done while it adds one for its own feature.

| # | Slice | Needs | Done check | Risk |
| --- | --- | --- | --- | --- |
| 1 | `std.remote` and the chapter | none | items type-check; the chapter's status note lists every form | low |
| 2 | The `Boundary` marker | 1 | accepted and rejected bounds for each category | medium |
| 3 | `@remote` declarations | 2 | a fixture per diagnostic | low |
| 4 | `remote!` in one place | 3 | local calls run; nested results match | medium |
| 5 | `[places]` in `hd.toml` | 1 | a fixture per manifest problem | low |
| 6 | Placement checks | 4, 5 | unmarked and unservable calls reject | medium |
| 7 | One module per place | 6 | `hd build` writes a module per place | medium |
| 8 | Loopback transport on Node | 7 | a call crosses and returns | high |
| 9 | Every boundary type, and `RemoteError` | 8 | each type round-trips; each error arrives | medium |
| 10 | Loopback on wasmtime | 9 | parity tests pass | medium |
| 11 | Maps at the boundary | 9 | maps round-trip in key order | medium |
| 12 | Trust and `Caller` | 8 | the principal per trust level | low |
| 13 | The hostile client | 12 | forged requests never run a body | medium |
| 14 | Opaque values in one place | 2, 4 | `open(opaque(v))` is `v` | medium |
| 15 | Opaque arguments checked at home | 13, 14 | tampered values fail before the body | medium |
| 16 | Seals bound to a caller | 15 | a replay by another principal fails | low |
| 17 | `ServerRefs` | 15 | references round-trip; foreign ones fail | medium |
| 18 | Content identity | 7 | identity unit tests | medium |
| 19 | `portable` literal checks | 2 | a fixture per diagnostic | low |
| 20 | Portable values in one place | 18, 19 | plain and `remote!` calls run | medium |
| 21 | Portables across places | 20, 9 | a closure crosses and runs at home | high |
| 22 | Portables on untrusted edges | 21, 15 | a forged closure fails | medium |
| 23 | `sends` and `portable-row-not-bound` | 21 | the row check rejects; a job runs | medium |
| 24 | Cancellation across places | 9 | a cancelled call stops at home | medium |
| 25 | Retries and request IDs | 9 | a lost reply runs the body once | medium |
| 26 | `@query` checks | 3 | tracked rows accept; others reject | low |
| 27 | The memo | 26, 18, 9 | a repeated query does not rerun | high |
| 28 | `watch` and pushed results | 27 | a write pushes a new result | high |
| 29 | Recorded runs and replay | 9 | a crash between two host calls charges once | high |
| 30 | Host calls in flight | 29 | a lost non-idempotent call is `OutcomeUnknown` | medium |
| 31 | Tour and cost report | all | the tour's in-scope programs run; a cost table | low |

### 1. `std.remote` And The Chapter

- **Work.** `lib/std/remote.hd` declares `RemoteError` with its five
  variants, `Caller` with `principal()`, the enum it returns
  (`.User(name)`, `.Service(name)`, `.Anonymous`), and `Remote`, a host
  capability trait whose only method is the compiler-only `call`, entered
  in `hd_host_abi::TABLE` and regenerated into `host/gen/abi.json`,
  `host/gen/abi.mjs` and `hd_run_wasmtime/src/abi.rs` with
  `HD_UPDATE_ABI=1`. The chapter file with its status note, a row in the
  spec README's chapter table, and `spec/std/remote.md` with the items'
  rows in `stdlib-items.tsv`.
- **Spec.** `spec/std/remote.md` (items); chapter 15's introduction,
  terms (place, edge, home) and status note.
- **Done when.** A `typing/valid` fixture matches every `RemoteError`
  variant and every principal; a `typing/invalid` fixture puts `Caller`
  in `main`'s row, which no entry profile binds
  (`nonhost-entry-requirement`, today's code).
- **Footprint.** std's interface hash changes once; no program's bytes
  change.
- **Risk.** Low.

### 2. The `Boundary` Marker

- **Work.** A sealed marker trait `std.remote.Boundary`, decided in
  `hd_types` (`solver.rs`, `sealed.rs`) as a new `BuiltinImpl` beside
  `AnyVal`. Its predicate is the one `hd_check/src/header.rs` uses for
  host signatures: the structural types of `module.boundary.allowed`,
  minus mutable types, trait values, closures, live handles and moded
  types. Recursive data types take the least fixed point, memoized per
  instance.
- **Spec.** A rule in [Boundary-Safe Values](../../spec/lang/10-modules.md#boundary-safe-values)
  that `Boundary` is implemented exactly by boundary-safe types; the item
  in `spec/std/remote.md`.
- **Done when.** `typing/valid` bounds for data, lists, options, results,
  tuples and a recursive data type; one `typing/invalid` case each for
  `mut List`, a closure, `dyn Display`, a `TcpStream` and a user data type
  with a closure field. An owned type argument is covered by the existing
  ownership rule, cited in the case.
- **Footprint.** None: no code.
- **Risk.** Medium: the predicate must agree with header checking, so
  both call one function.

### 3. `@remote` Declarations

- **Work.** `@remote` as a compiler-known decorator, recognized without
  an import as `@derive` is (`hd_check`, a new `remote.rs`). It may
  precede only a module-level function. Its signature must be
  boundary-safe in the direction each part crosses (arguments in, result
  out), through `header.rs`'s check, so `boundary-private-field` applies
  as it does to host traits. It may have no type parameters. The mark
  and the call identity, a `Hash128` over the module path, the name and
  the exact layouts of the parameter, result and row types, go into the
  folder interface (`hd_resolve`); the interface wire version changes
  once.
- **Spec.** Chapter 15, Remote Functions (design 4.2); in Annotations,
  `@remote` and `@query` join `@derive` and `@error` as compiler
  intrinsics that `annot.decorator.attach` does not cover.
- **Done when.** `typing/invalid`: `remote-signature-not-boundary-safe`
  (a `mut List` parameter), `remote-generic`, `boundary-private-field`
  (a result with a private field and no `Serialize`), and `@remote`
  before a method (`decorator-not-annotator`, as `@derive` there is);
  `typing/valid`: design 2's `post!` with stub traits.
- **Footprint.** None.
- **Risk.** Low.

### 4. `remote!` In One Place

- **Work.** The intrinsic declaration in `std.remote` (question 2) and
  its key in `hd_host_abi::INTRINSICS`. Typing: the caller's row holds
  `Remote`; the target is an `@remote` function, else `remote-no-place`;
  arguments check as a call's; the result is `Result[T, RemoteError]`.
  In the implicit place (question 1) the call lowers to a direct call of
  `f` wrapped in `.Ok`, with `f`'s row bound from the place's host
  providers, as an entry's row is (`hd_wasm` `root_poll`'s provider
  construction, reused). A row the place cannot bind is
  `remote-no-place`. The cold form `remote(f, args)` is the same call
  without `!`.
- **Spec.** Chapter 15, Calls (design 4.3, rules 1 to 3, 6 and 7); the
  implicit place.
- **Done when.** `runtime/valid`: `remote!` of a function that prints
  through `Console`, a nested match on `.Ok(.Err(...))`, and
  `race!(remote(f, x), other())`; `typing/invalid`: `remote!` of a plain
  function (`remote-no-place`), a caller without `Remote`
  (`missing-requirement`), and a target whose row needs a key the
  default profile lacks (`remote-no-place`).
- **Footprint.** Hello byte-identical. A program that calls `remote!`
  pays one direct call and one `.Ok` per call, and no import.
- **Risk.** Medium: until now only the entry builds host providers.

### 5. `[places]` In `hd.toml`

- **Work.** `hd_project/src/manifest.rs` parses `[places.NAME]` with
  `profile`, `serves`, `opaque`, `trust` and `sends`, and reports
  problems with codes and lines: an unknown key, a module that does not
  exist, a module two places serve, an unknown `opaque` impl, an unknown
  trust level, and a `profile` key that names no trait (question 4).
  Trust defaults to `untrusted`.
- **Spec.** A `[places]` section under
  [Package Manifest](../../spec/lang/10-modules.md#package-manifest)
  (design 4.1, rules 3, 4 and 7); the fixture format of question 6.
- **Done when.** One `typing/invalid` tree case per problem, each tree
  with its own `hd.toml`, judged by code.
- **Footprint.** None.
- **Risk.** Low.

### 6. Placement Checks

- **Work.** Body checking records each body's calls to marked functions
  as a short side list in its check entry. A new driver task,
  `Placement`, runs per place when `[places]` exists, during `hd check`
  as well as `hd build`. It reports a plain call to a function whose
  module another place serves as `remote-call-unmarked`, with a fix-it
  that wraps the call in `remote!`. It reports a `remote!` whose target
  module no place serves, or whose serving place cannot bind the
  target's row, as `remote-no-place`. A dependency's marks stay inert
  until a `serves` list names their module.
- **Spec.** Chapter 15, Places (design 4.1, rule 5) and Calls (rules 4
  and 5).
- **Done when.** Tree cases: design 2's browser `send!` passes; the same
  with a plain `post!` rejects; a `remote!` into an unserved module and
  one into a place without the row reject; `hd fix` applies the fix-it.
- **Footprint.** None.
- **Risk.** Medium: the call summary must survive cache hits, so it is
  part of the check entry, not recomputed from TIR.

### 7. One Module Per Place

- **Work.** `Goal::Place { name }` in `hd_driver`. `hd_mono` roots: the
  entry, when this place runs it, and every `@remote` function the place
  serves. `hd_wasm` emits the adapter exports, the `hd.remote` section
  and, for each crossing call, the stub that encodes the identity and
  arguments and imports `hd:Remote`. An adapter decodes through
  `hd_wasm::boundary`'s decoders, but a decoding failure returns a
  status to the host instead of raising `host-contract`, since a bad
  request must not poison the home. `hd build` writes one module per
  place, and `hd run` refuses with a pointer to slice 8 until then.
- **Spec.** [Command Line](../../spec/cli/command-line.md): `hd build`'s
  output per place; a CLI case.
- **Done when.** The CLI case finds one module per place; a module
  inspection test finds the export and section at the server and the
  import at the client; a single-place build is byte-identical to
  today's.
- **Footprint.** Hello byte-identical. Per the design's estimates, an
  adapter is about 0.3 to 2 KB and a stub about 0.2 to 1 KB; this slice
  measures them.
- **Risk.** Medium: the adapter is a new kind of root that takes
  arguments.

### 8. Loopback Transport On Node

- **Work.** `host/remote.mjs`: the place table read from the modules'
  `hd.remote` sections, `call.start` and `call.finish`, a fresh instance
  per call (question 5), its own poll loop, and the result copy.
  `host/run.mjs` loads one module per place for `hd run`;
  `host/conformance-worker.mjs` does the same for the fixture runner.
  Scalars and strings only; slice 9 widens it.
- **Spec.** Chapter 15, routing (design 4.3, rule 3) and the wire (4.8,
  rule 4); the `loopback-places` profile in the conformance README
  (question 6).
- **Done when.** A `runtime/valid` tree case: the client's `main!` calls
  `remote!(post, room, text)` and prints the server's answer. The case
  has a wasmtime `KNOWN_FAILURES.tsv` row for slice 10 if the parity
  suite runs every runtime case.
- **Footprint.** Hello byte-identical; the client module gains one import
  pair.
- **Risk.** High: two instances, two poll loops, and wakes that cross
  between them.

### 9. Every Boundary Type, And `RemoteError`

- **Work.** The stub and adapter for every boundary type `hd_wasm`
  already encodes: data, enums, lists, options, results, tuples and
  private-field types through their JSON codec. The errors: a panic at
  home is `.Panicked(category)` and the home instance is dropped; an
  unknown identity is `.Incompatible`; arguments that fail to decode are
  `.Rejected(message)`. The caller is never poisoned by a remote panic.
- **Spec.** Chapter 15, Failure (design 3.8 and the `RemoteError` rules).
- **Done when.** `runtime/valid` round trips of each type, a home panic
  arriving as `Panicked`, and design 3.1's `greeting!` with stub traits.
- **Footprint.** Hello byte-identical.
- **Risk.** Medium: recursive types do not cross the codec today; such a
  signature is `not-in-remote-prototype` until the codec handles them.

### 10. Loopback On wasmtime

- **Work.** `crates/hd_run_wasmtime/src/host/remote.rs`: the same router
  on the reactor, with one store per call from the pooling allocator.
  The `loopback-places` profile in `profiles.rs`, and the parity cases
  in `tests/parity/fixture_profiles.rs`.
- **Spec.** None new.
- **Done when.** Slices 8 and 9's fixtures pass on both hosts, slice 8's
  `KNOWN_FAILURES.tsv` row is gone, and the parity tests pass.
- **Footprint.** No compiler change.
- **Risk.** Medium: a store per call must come from the pool, or a busy
  home allocates per request.

### 11. Maps At The Boundary

- **Work.** `BTy::Map` in `hd_wasm/src/boundary.rs`, `host/wire.mjs` and
  `hd_run_wasmtime/src/codec.rs`. Entries are written sorted by their
  encoded key bytes, so the encoding is canonical without an `Ord` bound
  and two places agree whatever their hash seeds (design 4.8, rule 5).
- **Spec.** Chapter 15's canonical encoding rule; no change to
  `module.boundary.allowed`, which already allows maps.
- **Done when.** A remote call with a `Map[string, i64]` argument
  returns it unchanged, and a host test shows two maps built in
  different orders encode to the same bytes.
- **Footprint.** Hello byte-identical; a sort per encoded map.
- **Risk.** Medium: decoding still runs `Eq` and `Hash`
  (`module.boundary.map-decode`).

### 12. Trust And `Caller`

- **Work.** The edge into a place takes that place's `trust`. The
  loopback transport binds `Caller` per call by the fixed principals of
  question 6: `.Anonymous` or the fixture user on an untrusted edge,
  `.Service(name)` on a service or trusted edge, naming the calling
  place.
- **Spec.** Chapter 15, Edges (design 4.1, rule 7) and the `Caller` rule
  (4.3, rule 11).
- **Done when.** Design 3.3's `reserve!` answers a service and refuses a
  user; design 2's `post!` answers the fixture user.
- **Footprint.** None in compiled code.
- **Risk.** Low.

### 13. The Hostile Client

- **Work.** The `hostile-client` scenario (question 6) in
  `host/conformance-worker.mjs` and `crates/hd_driver/tests/conformance/wasmtime.rs`.
  The runner calls the fixture's server place directly with an identity
  no build has, with an `@remote` function's arguments truncated by one
  byte, and with the identity an unmarked function would have. The
  profile's `Probe` trait counts body runs.
- **Spec.** Chapter 15, Exposure (design 4.1, rule 4; 4.2, rule 4; the
  guarantees of design 6 that the transport does not own); the scenario
  in the conformance README.
- **Done when.** The scenario case gets `Incompatible`, `Rejected` and
  `Incompatible`, and `Probe` counts zero runs.
- **Footprint.** None.
- **Risk.** Medium: the scenario is a second implementation of a client
  and must stay small.

### 14. Opaque Values In One Place

- **Work.** `Opaque[T < Boundary]`, a std data type with a private bytes
  field; the intrinsic `opaque(value)` and the method `open()`; the trait
  `OpaqueStore` and the impl `SealedValues`. `opaque` seals at once:
  it encodes `T` canonically and calls the host, which encrypts with
  ChaCha20-Poly1305 under the run's key, with `T`'s type identity as the
  associated data (question 3). `open()` returns `T`, since every opaque
  value a function holds was either made in this run or checked at the
  boundary (slice 15). On Node the AEAD is `node:crypto`; on wasmtime it
  is the RustCrypto `chacha20poly1305` crate, a new dependency without
  `unsafe` in our code. The implicit place binds `OpaqueStore` as
  `SealedValues` (question 1).
- **Spec.** Chapter 15, Opaque Values (design 4.5, rules 1 to 5);
  `Opaque` joins the boundary-safe list; the items in
  `spec/std/remote.md`.
- **Done when.** `runtime/valid`: `open(opaque(v))` equals `v` for a
  data value; `typing/invalid`: an `Opaque[T]` passed where
  `Opaque[U]` is expected, and reading its field (`private-member`).
- **Footprint.** Hello byte-identical. A program that uses `opaque` gains
  two host imports and `T`'s encoder and decoder; each `opaque` call
  allocates its bytes, the value's size plus 33.
- **Risk.** Medium: an eager seal costs a host call even for a value
  that never leaves; the cost report measures it.

### 15. Opaque Arguments Checked At Home

- **Work.** For each `@remote` parameter type that contains an
  `Opaque`, at any depth, the adapter runs a generated check over the
  decoded value before the body. A value whose tag, key or type identity
  fails is the boundary failure `opaque-invalid`, which reaches the
  caller as `.Rejected` naming it. A value made by another place fails
  too, since its key differs.
- **Spec.** Design 4.5, rule 9; a new step of `hostile-client` that
  flips one bit of each opaque argument.
- **Done when.** Design 3.2's `open_draft!` and `publish!` round trip
  through the client; the scenario step fails before the body, and
  `Probe` counts no run.
- **Footprint.** Hello byte-identical; one host call per opaque argument.
- **Risk.** Medium: opaque values inside lists and options need the walk
  generated per boundary type, like the encoders.

### 16. Seals Bound To A Caller

- **Work.** `opaque(value, bind_caller=true)` adds the sealing call's
  principal to the associated data; the check at home compares it with
  the current `Caller`. Without the setting a seal is not bound, so
  shared links and cached pages keep working.
- **Spec.** Design 4.5, rule 6, without expiry (see Deferred).
- **Done when.** A `hostile-client` step replays a bound seal as another
  principal and gets `opaque-invalid`; the same seal without the
  setting is accepted.
- **Footprint.** None beyond slice 15.
- **Risk.** Low.

### 17. `ServerRefs`

- **Work.** The second impl. `opaque` stores the canonical bytes in a
  host table for the connection and returns a reference: a strategy
  byte, an index of 1 to 8 bytes (LEB128) and an 8-byte random check. In loopback, a
  connection is one calling place's run. The table ends with it. A value
  of one strategy decodes only under the same strategy.
- **Spec.** Design 4.5, rules 4 and 7.
- **Done when.** Design 3.2's form passes unchanged under
  `opaque = "ServerRefs"`; a `hostile-client` step presents a reference
  from another connection and gets `opaque-invalid`.
- **Footprint.** No compiler change: the strategy lives in the host.
- **Risk.** Medium: the table must be per connection, never global.

### 18. Content Identity

- **Work.** A driver task computes SHA-256 over a body's TIR wire form
  with every callee replaced by the callee's content identity, plus type
  layouts, the compiler version and the build profile. Names are left
  out, and a strongly connected group is hashed as one unit (design 4.4,
  rules 2 and 3). It runs only for `portable` literals and queries. Its
  results are memoized by TIR hash and callee identities. The `sha2`
  crate already in the lockfile (`hd_deps`) becomes a dependency of
  `hd_driver`.
- **Spec.** Rules 2 to 4 of design 4.4. No single build can observe a
  hash, so `crates/hd_driver/tests/all/content_id.rs` checks them: a
  renamed function keeps its identity, a callee's body edit changes its
  callers', a recursive pair is stable, and the order of declarations
  does not matter.
- **Done when.** The unit tests pass; `hd_web`'s size gate passes.
- **Footprint.** Hello byte-identical; the web compiler grows by the
  `sha2` code, reported.
- **Risk.** Medium: the TIR wire form names callees by `DefId`, so the
  hash must map them first.

### 19. `portable` Literal Checks

- **Work.** The intrinsic declaration (question 2) and the type
  `Portable[F]`, boundary-safe. The checker requires a closure literal
  (`portable-not-literal`), checks each capture against `Boundary`
  (`portable-capture-not-boundary-safe`), and rejects a capture whose
  TIR capture mode is not `Copy` (`portable-capture-mutable`), and a
  literal whose type or captures mention an enclosing type parameter
  (`portable-generic`). A provider captured from `$.use` is a trait
  value and fails `Boundary`.
- **Spec.** Chapter 15, Portable Closures (design 4.7, rules 1, 2 and 4
  to 6).
- **Done when.** `typing/invalid`: a non-literal argument; design 3.4's
  connection capture, written with a `TcpStream`; design 3.5's provider
  capture; an assignment to a capture; a capture the enclosing body
  reassigns afterward; a generic literal. `typing/valid`: design 3.4's
  scoring closure.
- **Footprint.** None.
- **Risk.** Low.

### 20. Portable Values In One Place

- **Work.** `hd_mono` lifts each literal's body to a function whose
  leading parameters are the captures (design 4.7, rule 7). A portable
  value is a data value with the 16-byte identity, the captures encoded
  at creation, and the ordinary closure. A plain call `p(args)` calls
  the closure in process and needs the caller's row to cover `F`'s row
  (owner, 2026-10-17). A `remote!(p, args)` in the implicit place runs
  it locally, as slice 4 does for functions.
- **Spec.** Design 4.7, rule 3.
- **Done when.** `runtime/valid`: a plain call and a `remote!` call of
  one portable value give the same result; `typing/invalid`: a plain
  call from a caller whose row lacks the closure's key.
- **Footprint.** Hello byte-identical. Per literal evaluation, one
  capture encode and one allocation the size of the captures.
- **Risk.** Medium: the captures are encoded once, so the closure and
  its encoding must not diverge, which slice 19's `Copy` rule
  guarantees.

### 21. Portables Across Places

- **Work.** Every literal in the build gets a portable table entry and
  an adapter in every place's module (design 4.7, rule 8). A
  `remote!(p, args)` routes by the module of the literal that made `p`.
  A `Portable` that arrives as an argument or result is decoded by
  looking its identity up in the receiving place's table; a missing
  identity is `Incompatible`. On a trusted edge nothing is sealed.
- **Spec.** Design 4.7, rules 3, 8 and 9.
- **Done when.** Design 3.2's `open_page!` returns a `Page` whose
  `publish` the client sends home with `remote!`; a portable value
  received at the client is also called there with a plain call.
- **Footprint.** Hello byte-identical. A build with portable literals and
  `[places]` carries one adapter per literal in each place.
- **Risk.** High: decoding builds a closure from an identity, so the
  table must be complete and identical in every place's module.

### 22. Portables On Untrusted Edges

- **Work.** On an untrusted edge, a portable value travels as an opaque
  value under the place's strategy, with its content identity as the
  label (design 4.7, rule 10). A place accepts one from an untrusted
  edge only if it sealed or stored it; otherwise it is the boundary
  failure `portable-not-own`.
- **Spec.** Design 4.7, rule 10; a new `hostile-client` step that sends a
  portable built by the runner from a real identity and its own
  captures.
- **Done when.** Slice 21's fixture passes with the server untrusted; the
  scenario step gets `portable-not-own` and `Probe` counts no run.
- **Footprint.** 33 more bytes per portable value on such an edge.
- **Risk.** Medium.

### 23. `sends` And `portable-row-not-bound`

- **Work.** A place's `sends` names the provider traits that send work to
  it. Placement checks each call of such a trait's method with a
  `Portable[F]` argument: `F`'s row must be bound by the place, or it is
  `portable-row-not-bound`. At run time, `loopback-places` routes the
  fixture trait `Pool` (question 6) to the place that lists it, which
  runs the portable.
- **Spec.** Design 4.1, rule 6.
- **Done when.** Design 3.6's `tag_all!`, written with `Pool`, rejects;
  a `Pool` job with an empty row runs at the worker place; a generic
  method over `Portable` is `not-in-remote-prototype`.
- **Footprint.** None beyond slice 21.
- **Risk.** Medium.

### 24. Cancellation Across Places

- **Work.** Cancelling a pending call's suspension calls `hd:rt/abort` on
  its handle; the transport cancels the home's root suspension, which
  runs its `defer` suites and disposes its `using` bindings, and drops
  the instance. A late result is dropped.
- **Spec.** Design 4.3, rule 10.
- **Done when.** Design 3.3's `hold!`, with a home that waits on a
  `pending-gate`-style trait, returns after the timer wins; `Probe`
  shows the home's cleanup ran.
- **Footprint.** None in compiled code.
- **Risk.** Medium: cancellation crosses two poll loops.

### 25. Retries And Request IDs

- **Work.** The transport attaches a request ID to every call and retries
  once when a reply is lost. Home keeps each ID's encoded result for the
  connection and answers a repeat from it. `loopback-places` drops the
  first reply of a call when the fixture's place sets `drop_first_reply`
  (question 6).
- **Spec.** Design 4.8, rule 3.
- **Done when.** A call whose first reply is dropped returns `.Ok`, and
  `Probe` counts one run.
- **Footprint.** None in compiled code.
- **Risk.** Medium: retained results need a bound; the slice keeps them
  for the connection only.

### 26. `@query` Checks

- **Work.** `@query` implies `@remote`. Each key in its row is a tracked
  input of the place's profile, or `Caller`, else
  `query-row-not-tracked`. A tracked input trait has no `mut` method. A
  query body that reads a top-level `mut` binding gets a warning. A new
  fixture profile, `tracked-notes`, declares a tracked `Notes` and an
  untracked `NotesWrite` (question 6).
- **Spec.** Design 4.6, rules 1, 2 and 6; the profile in the conformance
  README.
- **Done when.** `typing/valid` for a `Notes` query; `typing/invalid` for
  a query with `NotesWrite`; `typing/warnings` for the top-level read.
- **Footprint.** None.
- **Risk.** Low.

### 27. The Memo

- **Work.** The home records each query run's tracked host calls and the
  revision each read returned, at the host boundary where the journal
  already records calls (`hd_run/src/journal.rs`). The memo key is the
  query's content identity, its canonical arguments, and the principal
  when `Caller` is in the row. A call whose recorded revisions are
  unchanged returns the memo without running. The table holds a fixed
  number of entries and evicts the least recently used.
- **Spec.** Design 4.6, rules 3 to 5, without pushing.
- **Done when.** Two calls with no write between them run the body once
  (`Probe`); a write between them runs it twice; two principals get
  separate entries.
- **Footprint.** None in compiled code; the memo lives in the host.
- **Risk.** High: a read the recorder misses gives a stale answer, so
  the fixture covers every tracked method.

### 28. `watch` And Pushed Results

- **Work.** `std.remote.watch(f, args)` returns a `Watch[T]` whose
  `next!()` completes when the query's result changes, or with `.Err` on
  a failed hop. The home keeps each subscription for the connection; a
  write that stales an entry reruns it and pushes the result.
- **Spec.** Design 3.7 and 4.6, rule 5; the items in
  `spec/std/remote.md`.
- **Done when.** Design 3.7's `follow!`, bounded to two updates, prints
  the count before and after another task's write.
- **Footprint.** Hello byte-identical.
- **Risk.** High: a push is a completion with no request, so the
  transport needs a standing handle per subscription.

### 29. Recorded Runs And Replay

- **Work.** Recording is opt-in per run, set by the host (design,
  Decisions So Far). A recorded call keeps its identity, its arguments
  and each host result in a history in the journal format. A
  `crash-resume` scenario stops the home after a given host call
  completes, starts a fresh instance, and replays: recorded results come
  from the history and later calls run live. No stack is saved.
- **Spec.** Design 4.8, rule 1; the scenario in the conformance README.
- **Done when.** Design 3.8's `fulfil!` with fixture `Payments` and
  `Mailer` charges once and mails once across the crash.
- **Footprint.** None in compiled code.
- **Risk.** High: replay must reach the same host calls in the same
  order, which `req.determinism.*` promises but nothing has tested
  across instances.

### 30. Host Calls In Flight

- **Work.** A per-method flag in `hd_host_abi` marks a host method
  idempotent with a stable key. A host call in flight at the crash
  reruns when its method is marked, and otherwise completes as
  `OutcomeUnknown` for the program.
- **Spec.** Design 4.8, rule 2.
- **Done when.** The scenario crashes during `charge!`: unmarked, the
  program sees `OutcomeUnknown`; marked, the charge reruns with its key,
  and the fixture provider counts one charge.
- **Footprint.** The ABI model regenerates; no program's bytes change.
- **Risk.** Medium: where a host method's result type has no
  `OutcomeUnknown` variant, the slice reports it as a remote failure, so
  the flag fits only methods called inside a remote run.

### 31. Tour And Cost Report

- **Work.** Design sections 2 and 3's in-scope programs as fixtures, with
  the stubs they assume. Measurements, appended to this record: the
  tiny-program footprint; `hd build` time for one place and three; adapter
  and stub sizes; wire bytes per call against design 7; a crossing call's
  latency on both hosts, split into encode, instantiate, run and decode;
  the eager seal's cost per `opaque`; and memo hit time.
- **Done when.** The suite is green and the numbers are recorded; a
  number far from design 7's estimate gets a line on whether the
  implementation or the design is at fault.
- **Risk.** Low.

## 4. Hard Cases By Slice

The design's tour and appendix give most cases. The rest are what this
plan found while placing the rules on today's compiler.

| Case | What it tests | Slice |
| --- | --- | --- |
| design 2, `send!` and `post!` | the basic crossing call and nested result | 6, 8 |
| design 3.1, `greeting!` | a `@query` read through `remote!` | 9, 26 |
| design 3.1, an unmarked function with `UserRead` | the ordinary `missing-requirement` | 4 |
| design 3.1, a private-field result | `boundary-private-field` on a remote signature | 3 |
| design 3.2, `open_draft!` and `publish!` | an opaque value round trip, and tampering | 14, 15 |
| design 3.2, `Page.publish` | a portable closure returned and sent home | 21, 22 |
| design 3.3, `reserve!` and `hold!` | service principal, deadline, retry, `Incompatible` | 12, 24, 25 |
| design 3.4, `rank!` | a portable job; the generic `Workers.map!` is deferred | 19, 23 |
| design 3.4, `warm!` | a live handle capture is not boundary-safe | 19 |
| design 3.5, `big_in_usd!` | a provider capture | 19 |
| design 3.6, `tag_all!` | `portable-row-not-bound` | 23 |
| design 3.7, `follow!` | memo and push | 27, 28 |
| design 3.8, `fulfil!` | replay after a crash | 29, 30 |
| design 3.9 | GPU | deferred |
| A local `remote!` binds `f`'s row from the place's host providers, never from the caller's `$.with` | the place binds the row (design 4.3, rule 3), so a mock installed around the call does not reach `f` | 4 |
| A panic inside a local `remote!` | in process it is an ordinary panic and poisons the instance (design 4.3, rule 6 runs `f` in process); across places it is `Panicked` | 4, 9 |
| `@remote` while a `remote` name is in scope | the decorator is compiler-known, as `@derive` is, so it never names the intrinsic | 3 |
| A module two places serve | a manifest problem, since routing needs one home | 5 |
| A dependency's `@remote` function | inert until a `serves` list names its module (design 4.1, rule 4) | 6 |
| Malformed arguments at home | `Rejected`, never a `host-contract` panic that would poison the home | 7, 9 |
| A recursive type in a remote signature | the codec does not cross recursive types yet | 9 |
| Two maps with the same entries | canonical bytes whatever the insertion order or hash seed | 11 |
| An opaque value inside a list argument | the check walks every depth | 15 |
| An opaque value made by another place | a different key fails the check | 15 |
| A capture the enclosing body reassigns after the literal | the captures are encoded once, so a later write would split the closure from its encoding; rejected as `portable-capture-mutable` | 19, 20 |
| A portable literal inside a generic function whose captures are concrete | accepted unless its type or captures mention the type parameter | 19 |
| A portable value received at a place, then called with a plain call | runs in process there, needing the caller's row | 21 |
| A query that reads a top-level `mut` binding | warned, not checked (design appendix) | 26 |
| `Caller` in a query's row | the principal joins the memo key | 27 |
| `remote(f, x)` as a child of `race!` | the loser's call is cancelled at home | 24 |
| A `using` binding at home when the call is cancelled | disposed through today's cancellation cleanup | 24 |

### Choices This Plan Makes

These are implementation choices, not questions. The owner may veto
any of them.

- `open()` returns `T`, as the tour writes `seen.open().value`; the
  trait's own `open` keeps its `Result` for the adapter's check.
- A seal is made eagerly when `opaque` runs, so an opaque value is
  always bytes and encoding never calls the host.
- Captures are encoded when the `portable` literal runs.
- Map entries are ordered by their encoded key bytes.
- The plain cross-place call's code is `remote-call-unmarked`.
- The only transport is in-process loopback.

## 5. Questions For The Owner

Each question is a point the design leaves open or a conflict the plan
found with today's spec. Each gives the recommendation first.

1. **Default placement.** Recommended: a program without `[places]` is
   one implicit place that serves every module and binds the run's
   profile plus `Remote` and `OpaqueStore` (as `SealedValues`), so
   `remote!` runs in process there. With `[places]`, the entry runs at
   the place whose `serves` names its module, or else at an implicit
   client place that serves nothing and binds the run's profile.
   Without this, every single-file program and fixture that writes
   `remote!` is `remote-no-place`, and a browser-like client must be
   declared with an empty `serves` list.
2. **How `remote` and `portable` are declared.** An intrinsic's
   signature must be an ordinary hd declaration, and `remote!` takes
   either an `@remote` function or a `Portable[F]`. Recommended: a sealed
   std trait in `std.remote` (working name `RemoteTarget[Args, O, $R]`)
   that `Fn`, `SuspendFn` and `Portable` of either implement, so that
   `fn remote[T < RemoteTarget[Args, O, $R], Args < Tuple, O, $R](target: T, args...: Args) -> Result[O, RemoteError] $ Remote`
   and `portable` takes the same bound. The alternative is two
   intrinsics, which changes the decided spelling `remote!(page.publish)`.
3. **The seal's purpose label.** Design 4.5 binds "a label fixed at the
   `opaque` site", but home checks a value by its declared parameter
   type and knows no site, so `publish!` cannot check a label that
   `open_draft!` chose. Recommended: the label is `T`'s type identity,
   and a portable's content identity for a closure (design 4.7, rule
   10). A program that wants two purposes for one payload uses two
   types, such as a newtype. The alternative adds a label to the type,
   as `Opaque[DocVersion, "publish"]`, which the design does not have.
4. **A place's `profile` list.** `module.profile.toolchain-names` says a
   manifest cannot define a runtime profile, yet `[places.server]`
   lists keys. Recommended: the list is the place's registration
   contract (`module.register.contract-traits`): the keys the host must
   bind before the place starts, checked as a registered function's row
   is. Profile choices such as panic statuses stay the toolchain's.
5. **A fresh instance per remote call.** Recommended for the prototype:
   home runs each call in a fresh instance of its module, as a test
   case does. Callers then share no top-level `mut` value, poison or
   leftover handle, which matters most when they are different users on
   an untrusted edge. The cost is one instantiation and module
   initialization per call: tens of microseconds from wasmtime's pool,
   more on Node. The alternative, one long-lived instance per place,
   shares top-level state between callers and lets one caller's panic
   poison the rest.
6. **The fixture format for places.** Recommended: a package tree may
   hold an `hd.toml` whose only table is `[places]`. A new runtime
   profile, `loopback-places`, runs each place as an instance of its own
   module in one process. It binds a fixed principal per trust level,
   and has a `Probe` trait that counts body runs, a `Pool` trait for
   `sends`, and a `drop_first_reply` place setting. A second profile,
   `tracked-notes`, gives queries a tracked input. Two new scenarios,
   `hostile-client` and `crash-resume`, drive forged requests and
   crashes. All of it is spec text in `spec/conformance/README.md`, so
   another implementation can run the suite.
