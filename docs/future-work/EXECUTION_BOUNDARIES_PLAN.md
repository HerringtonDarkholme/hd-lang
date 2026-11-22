# Execution Boundaries: The Staged Plan

Status: a plan for owner review, not accepted behavior. It stages the
compiler work for the accepted record
[EXECUTION_BOUNDARIES.md](EXECUTION_BOUNDARIES.md) (owner 2026-11-21),
tracked in #2288. Stage 0, the language rules, is in the specification:
[Execution Domains](../../spec/lang/10-modules.md#execution-domains),
[Crossing Closures](../../spec/lang/07-functions.md#crossing-closures) and
[Frozen Loans](../../spec/lang/13-values-and-references.md#frozen-loans).
Their 53 failing conformance cases are rows of `KNOWN_FAILURES.tsv`.

Line counts are estimates from reading the sites named below, rounded to
tens. Each stage lands as its own lane step and deletes what it replaces
in the same commits.

- [1. Summary](#1-summary)
- [2. Order](#2-order)
- [3. Stage 1: Markers And Codecs](#3-stage-1-markers-and-codecs)
- [4. Stage 2: Crossing Calls And Frozen Loans](#4-stage-2-crossing-calls-and-frozen-loans)
- [5. Stage 3: Remote Traits](#5-stage-3-remote-traits)
- [6. Stage 4: std.remote In hd](#6-stage-4-stdremote-in-hd)
- [7. Stage 5: Targets](#7-stage-5-targets)
- [8. Stage 6: Runtime](#8-stage-6-runtime)
- [9. Prior Art Read For This Plan](#9-prior-art-read-for-this-plan)
- [10. Readings Stage 0 Chose](#10-readings-stage-0-chose)

## 1. Summary

| Stage | Delivers | Added | Deleted | Net | Unblocks |
| --- | --- | ---: | ---: | ---: | --- |
| 1 | `Marker`, `Send`, `Wire` (built: 424dcdfb7) | 390 | 63 | +327 | 2, 3 |
| 2 | crossing parameters, capture mode, reached top-level bindings, frozen loans (built: 1dd4c2a5c to 127293f15) | 880 | 88 | +792 | 6 |
| 3 | `Remote`: R1, R2, stubs, dispatchers, `remote::[T]`, `handle_call!`, build ID and interface numbers, the `Wire` codec (built: 5c9646dac to 00e3b32b8); the host boundary move and host handles as stubs wait on the codec's size | 1,170 | 150 | +1,018 | 4, 5 |
| 4 | `std.remote` plain parts and HTTP adapters, in hd; the `std/remote.md` chapter (built: std code net +86, tests +63) | 500 | 0 | +500 | none |
| 5 | one use-graph walk per executable | 280 | 20 | +260 | none |
| 6 | worker instances, Wasm threads for `Scope` and `Background`, a jobs store | 2,200 | 0 | +2,200 | none |

Every stage grows the code, since each adds a language feature;
merge-batch needs Growth-OK for them. Stage 3 pays most of itself back:
its codec replaces the host codecs, and host handles become stubs (Q21,
owner yes). The codec moved there from stage 1 (decision 2026-11-22),
since stubs are its first user.

## 2. Order

Stage 1 comes first: every later check asks a marker goal, and stage 3's
stubs call its codecs. Stage 2 and stage 3 then depend only on stage 1.
Stage 2 goes first because it is smaller and stage 6 waits on it.

| Stage | Needs | Why |
| --- | --- | --- |
| 1 | nothing | |
| 2 | 1 | a crossing closure's captures are checked against a marker |
| 3 | 1 | R1 asks `Wire` goals; stubs and dispatchers call the codecs |
| 4 | 3 | `per_request`, `home` and `http_handler` call `handle_call!` |
| 5 | 3 | the build ID and interface numbers are shared by every executable |
| 6 | 2 | workers and threads run crossing closures; servers already run on the SDK |

Stage 3 fixes the build ID per compile and numbers interfaces by hash
(section 5), so it needs no cross-executable state. Stage 5 is then only
the per-target walk, a compile-time saving.

## 3. Stage 1: Markers And Codecs

**Built** in 424dcdfb7, net +327: the markers only. The codec moved to
stage 3 (section 5).

**What.** `std.marker` declares `Marker`, `Send` and `Wire`. The solver
answers them from the type's form, as it answers `Copy`
(`module.marker.structural`). A library marker answers through explicit
impls and the same structural walk. B5's codecs become TIR functions.

**Sites.**

| Crate | File | Function | Change |
| --- | --- | --- | --- |
| `hd_types` | `src/sealed.rs` | `SealedTraits`, `row`, `copy_parts` | add `send` and `wire`; turn `copy_parts` into one memoized parts walk parameterized by the marker, so `Copy`, `Send` and `Wire` share it |
| `hd_types` | `src/solver.rs` | `Reads::candidates` and the goal search | a library marker with no matching explicit impl falls back to the structural walk, whose parts become subgoals, as rustc's auto-trait candidates do |
| `hd_resolve` | `src/known.rs` | `sealed`, the known-item table | the `std.marker` items and `std.remote.Remote` |
| `hd_resolve` | `src/lower/decls.rs`, `src/lower.rs` | `sealed_impl_rejected` | reuse for `Send` and `Wire`; reject a member of a marker trait (`invalid-marker-trait`) |
| `lib/std` | `marker.hd` (new) | | the three declarations |
| `hd_tir` | `src/codec.rs` (new) | | one encoder and one decoder body per `Wire` type reached, over TIR byte-buffer statements; decoders return a `Result`, never trap |
| `hd_wasm` | `src/boundary.rs` | `enc`, `enc_body`, `dec`, `dec_body`, `dec_map`, `put`, `copy` | deleted: the host boundary calls the TIR codecs |

**Deletes or replaces.** `copy_parts` becomes the shared walk (−60). The
host codec emission in `hd_wasm/src/boundary.rs`, lines 1747 to 2686
today, moves to TIR (−900). The layout-to-`LTy` description shrinks to
what handles and private-field codecs still need (−250). This follows
the backend-neutral rule: a backend lowers codecs, it does not invent
them.

**Per-run cost.**

| Pass | Cost | Bound test |
| --- | --- | --- |
| marker goal | the asked type's distinct nodes, once per (marker, type), memoized as `CopyMemo` is | a struct 1,000 fields wide and 20 deep; count nodes visited |
| library marker fallback | one impl-table probe, then the walk | same test, with a library marker |
| codec generation | one encoder and decoder per `Wire` type a crossing or host signature reaches, linear in its fields; a recursive type shares one pair | 1,000 types; count generated bodies |
| codec at run time | one encode, one copy, one decode, linear in the value | none beyond the stage fixtures |

## 4. Stage 2: Crossing Calls And Frozen Loans

**Built** in eight commits, net +792 against +1,370: the parse and the
signature record, the literal rule and capture markers, the capture
mode on TIR's existing `Copy` and `Move`, the reached-binding check with
a package join like `inst_cycles`' (a dependency's callees stay a gap),
and frozen loans as a capture slot that takes the bound's lifetime plus
one flag on element loans; no new loan checker.

**What.** The parser takes `fn!() -> T + M & 's $ R` as a parameter's
whole type. A call that passes a closure for it checks the closure's
captures and picks copy or move for each. A captured `&T` is lent until
`'s` ends. A program-wide pass finds mutable top-level bindings a
crossing closure reaches.

**Sites.**

| Crate | File | Function | Change |
| --- | --- | --- | --- |
| `hd_syntax` | `src/parser/types.rs` | `ty_parameter`, `ty_function` | parse the marker bound after the result, then `&` and a lifetime, then the row, as `impl` parameters parse their `& 'a` |
| `hd_resolve` | `src/lower.rs` | `anonymous_parameter` and its caller | record the marker bound on the parameter of the signature, not in `TyData::Fn`, since it is no type |
| `hd_check` | `src/call.rs` | argument checking | `crossing-closure-not-literal`; a forwarded crossing parameter of the same marker is valid |
| `hd_check` | `src/body.rs` | the capture-mode block after `check_dependent_results` | a crossing closure takes `Copy` or `Move`, never `Shared` or `Frame`; `note_capture_write` already records the first written capture, which becomes `crossing-capture-written` |
| `hd_check` | `src/moves.rs` | move sites | a `Move` capture is a move of its source (`use-of-moved-value`); a crossing parameter's call moves it |
| `hd_check` | `src/regions.rs`, `src/dep_results.rs` | `Constraints::solve`, conflict checks | outlives edges from each captured reference to the bound's lifetime; a frozen loan counts an element store as a write |
| `hd_check` | `src/init.rs` | `InitFacts`, `definite_init` | record every body's top-level reads and callees, not only same-module ones; a program pass answers "reaches a mutable or non-`Send` binding" per body |
| `hd_tir` | `src/ir.rs` | `CaptureMode` | `Move`, declared and never chosen today, is chosen here |
| `hd_tir` | `src/own/loans.rs` | `LoanBegin`, `LoanClose` placement | a frozen loan's run-time loan closes where `'s` ends, on the lending thread |

**Deletes or replaces.** The comment and branch that say `Move` "is not
chosen yet" go (−20). Nothing else is replaced: crossing closures are a
new form. The retired rule fn.capture.serializable-deferred had no code.

**Per-run cost.**

| Pass | Cost | Bound test |
| --- | --- | --- |
| crossing argument check | the closure's captures, once per crossing call | 1,000 captures; count marker goals |
| frozen loan edges | one outlives edge per captured reference, in the existing solver | 1,000 spawns in one scope |
| top-level reach | Tarjan over the call graph's strongly connected components, once: bodies plus call edges; warm, only components whose summaries changed | width 1,000 callees, depth 20; count bodies visited |

## 5. Stage 3: Remote Traits

**Built so far.** `std.remote`'s items, R1 and R2 (5c9646dac). The
public result (`module.remote.r2.public`, `module.remote.r2.wrap`): an
implementation method's body checks against its written `Result[V, E]`,
and its three exits (the final value, `return`, `?`) widen to
`Result[V, E | CrossError]` with one `SwitchTag` each, on the function's
own frame only. The module's items carry the public result, so every
reader past the body check (mono, the dispatch table, the backend) sees
one ABI. Reading: no separate wrapper body, so the dispatcher encodes
the public result as is; R2 keeps case `_1` out of an implementation, so
a stub decodes it without rewrapping.

The built-ins' checks. `remote::[T]` lowers its one argument as `$.use(K)`
lowers a key, to the trait value `dyn T`; a trait that is not remote is
`unsatisfied-trait-bound`. Readings: std declares the result `*mut T`,
which is `*mut dyn T` at that argument, since a type parameter cannot
follow `dyn`. `handle_call!` asks whether its root `S` implements a
remote trait, one goal per remote trait with a row for `S`'s head; a root
that is a type parameter, as in std's generic `http_handler`, is not
asked, and its generated body serves what the instance implements, so a
root with no remote trait refuses every frame with `WrongHome`. `Home`
holds only its request factory until returned objects (R3) need the
served table. Both built-ins now lower for real (below).

Stubs and dispatchers (steps 2 and 3 below) are built. `remote[T]`'s
generated body boxes std's private `Stub { transport, interface, object }`
for object 0 and converts it to `*mut dyn T`; collection's selection
answers `Stub: T` for every remote trait `T`, and each method of it is one
generated `stub_call!` of the method's number and its arguments as one
tuple. `handle_call!` is plain std that calls the private intrinsic
`serve![S]`, whose generated body reads the head with `serve_open`, tests
the interface of each remote trait `S` implements, switches on the method
number, and runs one arm: `serve_args`, the root's implementation,
`serve_ran`. Framing, the `Reply` envelope and every refusal are std's hd.
Readings:

- The frame is the build ID, the interface, the object and the method
  number as LEB128 numbers, then the arguments as one tuple, all through
  the `Wire` codec; a reply is an encoded `Reply`, whose `Ran` payload is
  the method's public result, so a stub returns it as decoded.
- The build ID is the toolchain key folded with the path and source hash
  of each library and executable module, which every executable of a
  build holds (`Env::build_id`). Std reads it through an out-of-line
  `build_id()`, so no stub or dispatcher body holds the constant and stage
  baselines do not move with the compiler.
- An interface number is the low 64 bits of the `canon` digest of
  `dyn T[args]`; a method's number is its declaration index, which a
  dispatcher switches on as a `u32`.
- `Stub: T` is answered in collection's selection only: `Stub` is private
  to std, so no checked goal asks it.
- A root serves the remote traits it implements through a written impl
  whose head matches it; a delegated impl (`by E`) is not served yet.
- A frame for an object other than the root is `WrongHome` until returned
  objects (R3) give it a served table.

The codec (step 1 below) is std code in `std.remote`: `wire_encode[T]`
and `wire_decode[T]` over two intrinsics, `wire_put[T]` and `wire_take[T]`,
whose bodies `hd_structure::wire_codec_body` selects per concrete type as
`dbg_printer`'s are (`hd_mono` `dbg_target` caches them). A `bool`, an
integer (generic, zigzag through `Integer.to_u64`), `char`, the floats,
`string`, `List[u8]`, `List`, `Map` and `never` select a std function; any
type with a structure (struct, enum, tuple, union, newtype, and so
`Option` and `Result`, plain enums) selects the private `WireStructural`
template. The private templates are one list of rows (template method,
trait method, trait) in `ProgramEnv::private_templates`, read by the four
sites that read `dbg_structure`'s one triple. Readings:

- Two intrinsics, not one `wire_codec`: an encoder is
  `fn(&T, &mut WireOut) -> void`, a decoder `fn(&mut WireIn) -> T?`; a
  decoder's `.None` is the caller's `Unknown` (stub) or `WrongHome`
  (dispatcher). `wire_decode` also refuses trailing bytes.
- Decoding is strict where the host's decoder trusts its peer: a LEB128
  number past 64 bits, an integer past its type's range, a `bool` other
  than 0 or 1, a `char` that is no scalar, malformed UTF-8, a variant
  index past the last and a length past the input are `.None`. A list's
  count reserves at most the bytes left. A map's repeated key keeps the
  later value, as the host's decoder inserts.
- `List[u8]` exactly is bytes. A list of a newtype over `u8` is one LEB128
  number per element, where the host strips the newtype; the boundary
  move settles it.
- A counted reference and a remote reference have no codec yet (`gap:
  #2288`): the first crosses only the host boundary until the move, the
  second is R3. An enum with shared constructor data has no structure
  target, so it has none either.
- `int_min` and `int_max` are private to `std.num`; `std.remote`
  declares its own, by the same intrinsic keys.

**The host boundary move fails its size gate; the host codecs stay.**
A ceiling run (2026-11-22, release builds, the encoder or decoder made
public for the run only) priced the std codec against the 0.5% per-module
gate, which is about 30 to 50 bytes on these modules:

| Program | Added | Before | After | Growth |
| --- | --- | ---: | ---: | ---: |
| hello, printing a string | `wire_encode::[string]` | 5,593 | 7,306 | +30.6% |
| `bench/study/nbody.hd` | `wire_decode::[List[string]]` | 9,172 | 12,852 | +40.1% |

The host's codec of the same type is 30 to 150 bytes, and every bench
program links only four small host decoders (about 270 bytes with
`rt:leb_get`), so deleting them cannot pay for the std codec's growth,
which is a lower bound on the move's: an `rt:enc` or `rt:dec` that calls
it still needs its copy into `hd.x`. Time was not measured, since size
fails first. The move waits for a codec that is close to the host's in
size: a reading of that is that the host keeps its own codecs, and a frame
and a host exchange share only the format, as the codec's tests check.

**Next, in this order.** Reading: the codec comes before stubs and
dispatchers, since each stub method and each dispatcher arm is an encode,
one call and a decode, and a stub with no codec is dormant code. The
price also forces it: stage 3 stands at +401 against +750, and the codec,
stubs and dispatchers add about +700 before any deletion, so the host
boundary move (about −900) lands with the codec or the stage passes its
+975 stop line.

1. Codec. `wire_codec::[T]()` is an intrinsic whose body collection
   generates per concrete `T`, as `dbg_printer` is (`hd_mono`
   `dbg_target`, `hd_structure::dbg`): a scalar, `string`, `List`, `Map`
   or `Option` selects a std function; a struct, enum, tuple or union
   selects a private `by Structure` template in std, as `DbgStructural`
   is, whose members select their own codec. The format is the host's
   tree encoding (zigzag LEB128 integers, length-prefixed strings and
   lists, a variant index then its payload), so the host boundary can
   move onto it with one `memory.copy` into `hd.x`. A decoder returns a
   `Result` and never traps on its input.

   Sites, read for this step. The dbg printer is the model:
   `hd_structure::dbg::printer_body` picks a std function per type and
   returns it as an `ItemRef` (about 60 lines); `hd_mono` `Cx::target`
   routes the `dbg_printer` key to `dbg_target`, which caches the
   generated body in `out.supplied`. The private template is special in
   four places, each keyed by one `(method, Structure, trait)` triple from
   `ProgramEnv::dbg_structure`: `hd_driver` `lib.rs` (the triple),
   `own.rs` `select_choice` and the instance builder's `walk`, and
   `hd_mono` `Cx::scan`'s `private_structure`. The codec's
   `WireStructural` template has two methods (put, take), so those four
   sites take a short list of private templates instead of one triple.
   Its format follows `hd_wasm` `boundary.rs` `enc_body`: `usize` is 32
   bits there, `bool` a one-bit unsigned integer, `Option` and `Result`
   are plain enums, and a map writes its live count. Counted references
   and private-field `Codec` types stay host-only until the move.
2. Stubs. One std struct `Stub { transport, interface, object }`. The
   solver supplies `Stub: T` for each remote trait `T`, as it supplies a
   tuple's `Structure`; `supplied_body(method, Stub)` generates each
   method as one call of a std generic `stub_call![A, V, E]` with the
   method's number and its arguments as a tuple. `remote[T]`'s body boxes
   a `Stub` for object 0 and converts it to `*mut dyn T`.
3. Dispatchers. Per remote trait `U` a generated `serve[U]`: a
   `SwitchInt` on the method number, each arm one call of a std generic
   `serve_call![A, V, E]` over the method through `&mut dyn U`. Per root
   `S`, `handle_call[S, C]` checks the build ID, picks among the remote
   traits `S` implements by interface hash, converts `&mut S` and calls
   that trait's `serve`. A returned object (R3) is served the same way
   through its `*mut dyn U`, so the served table holds trait values and
   needs no per-type code.

**What.** R1 and R2 are checked at the trait and at each implementation.
Each implementation method gets a public entry that wraps its error as
`._0`. Post-mono, the compiler generates a stub per remote trait a target
holds away from home, and a dispatcher per root type a `handle_call!`
instance names.

**Sites.**

| Crate | File | Function | Change |
| --- | --- | --- | --- |
| `hd_check` | `src/conform.rs` | `check_impl_method`, `sig_differs` | R2: the implementation's result is the trait's without its `CrossError` member |
| `hd_check` | `src/header.rs` | beside `boundary` | R1 on every remote trait's methods: bang, `Wire` types, no generics, no row (`invalid-remote-method`) |
| `hd_syntax`, `hd_check` | `src/parser/exprs.rs` `function_type_arguments`; `src/call.rs` | | `remote::[T]` takes a trait argument, as `$.use(K)` takes a key |
| `hd_check` | `src/remote.rs`, `src/body.rs`, `src/expr.rs` | `public_result`, `Ck::public_exit` | the public result: the body's exits widen in place, no wrapper body (built) |
| `hd_mono` | `src/instance.rs`, `src/layout.rs` | `Sub` | the `remote[T]` and `handle_call[S, C]` instances name generated bodies |
| `hd_tir` | `src/ir.rs` | `BodyKind` | `Stub` and `Dispatch`, generated like `DeriveInstance` in `hd_structure` |
| `lib/std` | `remote.hd` (new) | | `Remote`, `CrossError`, `Transport`, `Reply`, `Session`, `Rooted`, `Home` and its served table, the two `@intrinsic` declarations |

**Identity on the wire.** The build ID is a hash of the package's source
hashes and the toolchain ID, computed once per build. An interface is a
64-bit hash of the trait's path and type arguments, and a method is its
declaration index. So every executable agrees with no shared table,
unlike a number assigned once per build (section 10, Q18).

**Codec (moved from stage 1).** B5's codec is a std derive template
over `Structure`, as serde's `Serialize` derive is, encoding to and
decoding from `List[u8]` and returning a `Result`. The host boundary
moves onto it, one `memory.copy` into the exchange buffer, and
`rt:enc`/`rt:dec` in `hd_wasm/src/boundary.rs` are deleted, if the perf
gates pass; otherwise the host codecs stay and the numbers are reported.

**Deletes or replaces.** The host codec emission was to go (about −1,150
with the codec above); the size gate kept it (see the ceiling run under
"Built so far"). A host handle is a stub whose transport is the host:
`BodyKind::Handle` and `handle_vtable` fold into stubs (about −600; Q21,
owner yes).

**Q21 waits on the same size gate (2026-11-22).** A stub's method is a
frame: `stub_call!` encodes the arguments and decodes a `Reply` and the
result with the std `Wire` codec. Folding a host handle into a stub so
puts that codec on every handle call, where today a vtable slot passes
the values to one typed host call (`handle_slot`, `rt:enc`/`rt:dec`).
The ceiling run above bounds the cost from below: +1,713 bytes for a
string's encoder, +3,680 for a `List[string]`'s decoder, against a
0.5% gate of about 30 to 50 bytes, on every program that holds a handle
(`std.fs`, `std.net`, `std.http`). The host runtimes (`host/`,
`hd_run_wasmtime`) would also need a frame decoder for every handle
trait. Reading: the fold is not built; handles keep their typed slots,
and the −600 is not counted in the stage's price. It becomes worth
building when the std codec is close to the host's in size, the same
condition as the boundary move.

**Per-run cost.**

| Pass | Cost | Bound test |
| --- | --- | --- |
| R1 | each method's distinct types, as `Wire` goals | a trait of 1,000 methods |
| R2 | one signature comparison per implementation method | none beyond fixtures |
| stub and dispatcher generation | linear in the trait's methods, once per trait or root type | 1,000 methods; count generated instructions |
| a remote call | encode the arguments, one transport call, decode the reply | none |
| a dispatch | build ID compare, one served-table lookup, a `br_table` on the method number | none |

## 6. Stage 4: std.remote In hd

**Built**, std code +89 / −3 (net +86) and 63 test lines against +500:
`per_request`, `home`, `Call`, `action`, `http_transport` and
`http_handler` in lib/std/remote.hd, specified in spec/std/remote.md
(prefix `std-remote`). `Action` and `HttpTransport` are private. No
compiler change. Std tests run a stub against a fixed home through
`handle_call!`, and a stub through `http_transport` and `http_handler`
over an in-memory `Http`, a refused caller and `ScriptedHttp` failures
included. Readings:

- A home outlives the call that made it, so its factory and an action's
  closure may capture no local reference. hd says that only through a
  type parameter bounded by `'static` (`ref.generic.outlives.box`): a
  direct `fn!` parameter may hold local captures and is never stored
  (`fn.capture.local-store`). So `per_request` takes
  `F < Fn(&C) -> Suspend[Result[Rooted[S], Refused]] & 'static` and stores
  a `fn!` that calls it, and `action` likewise; the call site is the
  record's, a closure literal. `Home[S, C]` keeps its two parameters.
- `home(root, session)` gives each call a copy of `root`, as hd values
  copy; a worker's state that outlives a call sits behind a box in the
  root, as `PgUsers.pool` does (`std-remote.home.fixed.copy`).
- `http_transport` maps the record's "a connection never opened" to
  `HttpError` cases: `NotGranted`, `InvalidUrl`, `Dns`, `Connect`, `Tls`,
  `InvalidHeader` and `UnsupportedSetting` are `Unreachable`; any other
  error, a failure status other than 403 and an unreadable body are
  `Unknown`. It follows no redirect.
- `http_handler` runs the factory before it reads the body, so a refused
  caller is a 403 whatever its body.

**What.** The plain parts the record writes out in hd go into
lib/std/remote.hd (new): `per_request`, `home`, `Call`, `Action`, `action`,
`HttpTransport`, `http_transport` and `http_handler`. A stdlib chapter
spec/std/remote.md (new) specifies them, with its prefix row,
`STD_CHAPTERS` and `CHAPTER_PREFIXES` entries.

**Sites.** lib/std/remote.hd (new); `lib/std/http.hd` is read, not changed:
`Http.send!` and `Request` already exist. The SDK's `serve!` takes
`http_handler::[S]` as written in record section 5.

**Deletes or replaces.** Nothing. **Cost.** No compiler pass; a call's
cost is stage 3's plus one HTTP round trip.

## 7. Stage 5: Targets

**What.** Each executable parses and checks only the modules its entry
reaches through `use` (B1). Today `hd_project::in_program` keeps every
library module for every executable.

| Crate | File | Function | Change |
| --- | --- | --- | --- |
| `hd_project` | `src/lib.rs` | `in_program` | replaced by the closure of the entry's `use` edges |
| `hd_syntax` | `src/skim.rs` | `skim` | report a file's `use` paths without a full parse |
| `hd_command` | `src/lib.rs`, `src/disk.rs` | `build_package`, `only_program` | one build ID for the loop over executables |

**Deletes or replaces.** The role-based filter in `in_program` (−20).

**Per-run cost.** One skim per changed file and one walk over `use`
edges per executable: linear in reached modules. Warm with nothing
changed, the walk is cached by the file hashes it read.

## 8. Stage 6: Runtime

**What.** Three runtime slices, each separately testable:

| Slice | Where | What |
| --- | --- | --- |
| workers | `host/` (Node `worker_threads`, browser `Worker`), `hd_run_wasmtime` | a second instance of the same module; a crossing sends the lifted function's index and the encoded captures |
| threads | `hd_wasm` (shared memory, `memory.atomic.*`), `lib/std/rt` | `Scope` and `Background` run a `Send` closure on another thread of one shared memory |
| jobs | `lib/std`, `host/` | a store of the index, the captures and the build ID; a run after a deploy is `OtherBuild` |

**Counts and the allocator with threads.** No box is `Send`, so no
cycle-capable object reaches another thread; each thread keeps its own
cycle collector. Only `string`, `List` and `Map` buffers are shared, so
their counts become atomic. A whole-program pass beside
`hd_opt/src/whole.rs` marks the types a `Send` crossing reaches, and only
those get atomic counts. A program with no `Send` crossing keeps plain
counts and an unshared memory. The allocator needs per-thread
heaps with a cross-thread free path; its design reads the prior art
first.

**Deletes or replaces.** Nothing. `module.instance.one-thread` replaced
the old single-thread rule at stage 0.

**Per-run cost.** The reach pass is linear in the types and instances it
walks, once per release link. At run time, a program without `Send`
crossings pays nothing; one with them pays an atomic operation per count
change of a reached type.

## 9. Prior Art Read For This Plan

| System | Read | Taken |
| --- | --- | --- |
| rustc auto traits | `compiler/rustc_next_trait_solver/src/solve/assembly/structural_traits.rs`, `instantiate_constituent_tys_for_auto_trait`: an ADT's field types, a closure's tupled upvars, a reference's pointee | stage 1's structural walk; stage 2's closure-literal check is the upvar case |
| Rust `thread::scope` | `library/std/src/thread/scoped.rs`: `spawn` takes `F: FnOnce() -> T + Send + 'scope`; the scope waits on a running-thread count | the marker bound's `& 's`, and a frozen loan lasting until `'s` ends |
| serde derive | `serde_derive/src/ser.rs`: `serialize_struct_as_struct` writes one field call per field; `serialize_variant` writes the variant index; `bound::with_bound` adds a bound per type parameter | one generated codec per type, a variant index then its payload |
| Swift distributed actors | SE-0336: parameters and results conform to the system's `SerializationRequirement`, calls are implicitly throwing; SE-0344: a synthesized thunk calls `remoteCall` with a `RemoteCallTarget`, and a signature change breaks resolution | R1's `Wire` check, a stub per trait, and `OtherBuild` in place of a silent mismatch; hd differs in barring generic methods |
| kotlinx.rpc | `README.md`: an `@Rpc` interface, `withService<T>()` for a client, `registerService` on the server | `remote::[T](transport)` and a `Home` |
| Comlink | `src/comlink.ts`: an id per request in `requestResponseMessage`, proxies released by `releaseProxy` or a `FinalizationRegistry` | a stub's frame and the lease that frees a served object |

## 10. Readings Stage 0 Chose

Where the record left a rule open, stage 0 wrote the reading below into
the specification. Each is a **Recommendation** for the owner to confirm
or overturn; overturning one changes its rule and its cases.

| # | Open point | Reading written | Rule |
| --- | --- | --- | --- |
| Q1 | the record says `Wire` names today's boundary-safe values, but its table excludes boxes, which are boundary-safe | the table wins; the host boundary is unchanged | `module.marker.table.counted` |
| Q2 | a trait's `Self` is `Copy` (`ref.copy.trait.default`), which would bar the `impl !Copy` image of program 1 | a marker does not bound `Self` by `Copy` | `module.marker.not-copy` |
| Q3 | may user code implement `Send` or `Wire`? | no: both are sealed | `module.marker.sealed` |
| Q4 | may an explicit impl give a struct a library marker, as `GpuData` for `Texture`? | yes; that is what keeps a texture `GpuData` but neither `Wire` nor `Send` | `module.marker.explicit` |
| Q5 | which module declares the markers | `std.marker`, outside the prelude | `module.marker.std` |
| Q6 | `Remote` is called a marker, but its traits have methods | `Remote` is sealed and supplied for every type, so no `impl Remote` is written | `module.remote.std` |
| Q7 | section 6 puts `CrossError`, `Transport` and `Home` in the stdlib tier, but generated code names them | language tier; `per_request`, `home` and the HTTP parts stay stdlib | `module.remote.items` |
| Q8 | `E \| CrossError` when `E` is itself a union | exactly two members; a union `E` is grouped | `module.remote.r2.result` |
| Q9 | where the row and a lifetime go with `+ M` | `fn!() -> T + M & 's $ R`, one marker | `grammar.fn.crossing-parameter` |
| Q10 | how `Scope['s]` lets a closure borrow for `'s` | the lifetime on the marker bound; none means no local reference | `ref.frozen.lifetime`, `ref.frozen.no-lifetime` |
| Q11 | what a callee may do with a crossing parameter | call it once, or pass it on whole to one of the same marker | `fn.crossing.literal`, `fn.crossing.parameter.uses` |
| Q12 | moving a capture out of a closure is an error today | a crossing closure runs once, so it may | `fn.crossing.move-out` |
| Q13 | does "no capture is written" cover `push`? | yes, any in-place change | `fn.crossing.no-write` |
| Q14 | `lib/std/rt` keeps collector state in `let` bindings, so every allocating closure would reach one | the runtime modules' storage does not count; `std.time`'s `let mut idle_clocks` still does | `fn.crossing.global.runtime` |
| Q15 | the record implies threads; the retired rule module.instance.thread forbade them | retired for `module.instance.one-thread` | `module.instance.one-thread` |
| Q16 | `trait.unsupported.features` bars implicit structural conformance | unchanged: marker impls are compiler-supplied, as `Copy`'s | `module.marker.supplied` |
| Q17 | a struct with private fields needs `Serialize` at the host boundary | it is `Wire` by structure; a remote method validates its input | `module.marker.structural` |
| Q18 | interfaces "numbered once per build" need a table shared by every executable | a hash of path and type arguments | plan, stage 3 |
| Q19 | which counts become atomic with threads | only those of types a `Send` crossing reaches | plan, stage 6 |
| Q20 | `docs/compiler/ownership.md` moves a run-time loan with its reference | a frozen loan's run-time loan stays with the lending thread | `ref.frozen.alias.lender` |
| Q21 | host handles and stubs are both trait objects over a transport | keep them apart until the owner decides | plan, stage 3 |
