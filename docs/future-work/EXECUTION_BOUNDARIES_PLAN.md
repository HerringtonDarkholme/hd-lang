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
| 3 | `Remote`: R1, R2, stubs, dispatchers, `remote::[T]`, `handle_call!`, build ID and interface numbers; the `Wire` codec template, the host boundary moved onto it, host handles as stubs | 2,500 | 1,750 | +750 | 4, 5 |
| 4 | `std.remote` plain parts and HTTP adapters, in hd; the `std/remote.md` chapter | 500 | 0 | +500 | none |
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
| `hd_driver` | `src/own.rs`, near the `BodyKind::Handle` bodies | | the public entry wrapper per implementation method; inlining removes it |
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

**Deletes or replaces.** The host codec emission (about −1,150 with the
codec above). A host handle is a stub whose transport is the host:
`BodyKind::Handle` and `handle_vtable` fold into stubs (about −600; Q21,
owner yes).

**Per-run cost.**

| Pass | Cost | Bound test |
| --- | --- | --- |
| R1 | each method's distinct types, as `Wire` goals | a trait of 1,000 methods |
| R2 | one signature comparison per implementation method | none beyond fixtures |
| stub and dispatcher generation | linear in the trait's methods, once per trait or root type | 1,000 methods; count generated instructions |
| a remote call | encode the arguments, one transport call, decode the reply | none |
| a dispatch | build ID compare, one served-table lookup, a `br_table` on the method number | none |

## 6. Stage 4: std.remote In hd

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
