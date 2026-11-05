# Future Work

These documents cover deferred, undecided, or not yet applied work outside
the accepted language specification. Accepted language behavior belongs in
the [formal specification](../../spec/README.md), which is authoritative. Once
a record is applied or superseded, it is deleted; git history keeps it.

## Direction

hd is written mostly by coding agents and read by humans. Tools answer
questions about a program by name and in machine-readable output. The
language prefers explicit, checked forms over inference, and forbids
action at a distance.

## What Remains

Parked designs are not kept here; each lives in its GitHub issue, indexed by #833, which
stays open (#18, #163, #166, #181, #182, #646, #807).

### Application and host

- [HTTP Transport Settings](HTTP_CLIENT_SETTINGS.md): accepted 2026-10-29 (chaining on the `Http` provider; literal-IP DNS servers; roots extend the platform store; a zero phase timeout expires at once) for DNS, TLS, and phase timeouts (#950).
- [Server Applications](SERVER_APPLICATIONS.md): an ordinary `main!` with worker loops over one state object, one retained instance per adapter, an SDK-owned `HttpServer` capability (#51), lifecycle and draining, limits, grants, and SDK packaging tasks for Cloudflare and Vercel (#806, #266).
- [Host Embedding](HOST_EMBEDDING.md): parked proposal for embedding hd in an application; package host capabilities are designed in #51.
- [Native Package Host Bindings](NATIVE_PACKAGE_HOST_BINDINGS.md): accepted design, not built: generated Rust traits, codecs, two-key consent, and Wasmtime registration for package host capabilities (#1191).
- [Streaming Design](STREAMING_DESIGN.md): one reader and writer shape for HTTP bodies, process pipes, files and `TcpStream`, on plain `Disposable` handles bound with `using` (#350).

### Language and library

- [Raw Buffers](RAW_BUFFER.md): accepted 2026-11-03, being built: `Buffer` as raw memory with one owner, lists own their elements through a std-only `Drop`, and a verifier that no non-copyable value is ever copied (#1205).
- [Compile-Time Values](CONST_VALUES.md): typed const parameters and explicit `const fn`, staged; direction accepted 2026-10-28, not scheduled (#875).
- [Positional Union Types](UNION_TYPES.md): fully built with conformance, both-engine parity, and size measurements (#1120-#1125).
- [Decimal](DECIMAL.md): fixed 128-bit exact decimal with explicit rounding and exact string serde; design accepted 2026-10-28 as a future package, not in std and not scheduled (#840).
- [Scope Guards](SCOPE_GUARDS.md): accepted: sync `using`, `using!` and error-level `unbound-disposable` built (#962); `on_exit` with tier-2 capture lifetimes, `@auto_closing` (#1099) and library `defer`/`ensure` (#1101) built; the `defer:` statement is removed (#1117).
- [Non-Copyable Types](NON_COPYABLE.md): accepted: `Copy` is the default, `impl !Copy` opts a type out, `[T < ?Copy]` takes any type, Rust partial moves, `&self` read traits (#1141).
- [Standard Library For References](STDLIB_REFERENCES.md): accepted: accessors return references, `fn(&T)` callbacks, a borrowing `for x in xs`, operators through `&T`, real movers over three intrinsics, two collection families (copy-on-write `List`/`Map` for copyable items, the default for literals; `UniqList`/`UniqMap`, `!Copy` with no copy-on-write, for any item), move-only generics last (slices 5-9 open: #1348-#1352); 9 slices.
- [Standard Library](STDLIB.md): the #350 bar and shared module convention, and the std decisions not yet in `lib/std`.
- [Design By Contract](DESIGN_BY_CONTRACT.md): `require(message, condition)` as ordinary std code, checked in every profile until `@cfg` splits it; `ensure` deferred to tier 2 (#978); design accepted 2026-10-28 (#880).
- [Inline And Cold Decorators](INLINE_AND_COLD.md): `@inline(.Always)`, `@inline(.Never)` and `@cold` compiler decorators over the release inliner and table calls; accepted, phases 1-3 implemented, the std rare paths (#884) measured a miss, `.Always` open (#873).

### Tooling

- [Releasing a package](RELEASE.md): proposed API comparison, SemVer advice and explicit local tagging without automatic pushes; every version is compared with the last stable release (#28).
- [Live Execution](live-execution.md): the built REPL, journal, replay and session rebuilds (#17).

### Memory model and performance


- [Pointer Loans](POINTER_LOANS.md): accepted 2026-11-03, model A, built (#1336); loan placement moves into TIR with #1399 steps 20-21: aliasing through `*T` by per-object loans with per-reference elision, with a stress test and a 424-case matrix (#1323).
- [Parallelism And Locality](PARALLELISM_AND_LOCALITY.md): workers, copied messages and shared bytes on Wasm today; frozen data later is parked. There is no `^` mode (#163).
- [The Path To Returned Local References](RETURNED_REFERENCES_PATH.md): accepted 2026-10-29: Rust lifetime variables and elision over counted references, in stages; stages 1 to 9 built (6i not planned for now; stage 8 without borrow flags); `&mut` copies and parameter view types not scheduled.
- [One Lifetime Solver](LIFETIME_SOLVER.md): accepted 2026-10-30: region variables per reference slot, outlives constraints per TIR instruction and one SCC solver in place of the per-shape paths; S1 to S12 built, and every spec union is dropped; stage 7 built on it.
- [Ownership Per Instance](OWNERSHIP_ARCHITECTURE.md): accepted 2026-11-04, migration in progress: decide every count once per instance in one TIR pass, with never-addressed locals as values, a storage pass, explicit count and loan statements with checked reasons, and an emitter that decides nothing; 31 one-hour steps; reopens "work once per generic body" (#1364; steps tracked in #1399).
- [Refcount Runtime](REFCOUNT_RUNTIME.md): the linear-memory runtime of reference counting with cycle detection, priced on Wasm; it is the only backend (#426).
- [Performance Roadmap](PERFORMANCE_ROADMAP.md): where time and memory go from the measurements on record, fixes by mechanism, and ranked hour-sized steps (#426).
- [Release-Only Interprocedural Optimization](RELEASE_INTERPROC.md): built 2026-10-25 in the optimized pipeline only: per-function summaries over SCCs (slices 2-5: kept parameters, writes, lent parameters). Slices 6-9 measured no gain and were not merged (#681 closed); results on #165.

No native backend is planned (#188 closed), and no direct TLS streams are planned.

The compiler's design is in
[docs/compiler/ARCHITECTURE.md](../compiler/ARCHITECTURE.md).
