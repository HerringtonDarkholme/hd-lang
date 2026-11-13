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

Parked designs are not kept here; each lives in its GitHub issue, indexed by #833. The
execution-boundaries design (serializable closures, formerly #166) is PR #1139.

### Application and host

- [Server Applications](SERVER_APPLICATIONS.md): an ordinary `main!` with worker loops over one state object, one retained instance per adapter, an SDK-owned `HttpServer` capability (#51), lifecycle and draining, limits, grants, and SDK packaging tasks for Cloudflare and Vercel (#806, #266).
- [Native Package Host Bindings](NATIVE_PACKAGE_HOST_BINDINGS.md): accepted design, not built: generated Rust traits, codecs, two-key consent, and Wasmtime registration for package host capabilities (#1191).
- [Streaming Design](STREAMING_DESIGN.md): one reader and writer shape for HTTP bodies, process pipes, files and `TcpStream`, on plain `Disposable` handles bound with `using` (#350).

### Language and library

- [Compile-Time Values](CONST_VALUES.md): typed const parameters and explicit `const fn`, staged; direction accepted 2026-10-28, not scheduled (#875).
- [Decimal](DECIMAL.md): fixed 128-bit exact decimal with explicit rounding and exact string serde; design accepted 2026-10-28 as a future package, not in std and not scheduled (#840).
- [Standard Library](STDLIB.md): the #350 bar and shared module convention, and the std decisions not yet in `lib/std`.

### Tooling

- [Releasing a package](RELEASE.md): proposed API comparison, SemVer advice and explicit local tagging without automatic pushes; every version is compared with the last stable release (#28).

### Memory model and performance

- Parallelism and locality: folded into the execution-boundaries record, PR #1139 (`EXECUTION_BOUNDARIES.md` Appendix D), owner 2026-11-13.
- [The Path To Returned Local References](RETURNED_REFERENCES_PATH.md): accepted 2026-10-29, stages 1 to 9 built (the checker's region solver is [crates/hd_check/DESIGN.md §15](../../crates/hd_check/DESIGN.md#15-lifetimes-one-region-solver)); kept for the parts accepted but not built: view types for `&mut` parameters (#1357) and phantom markers.

No native backend is planned (#188 closed), and no direct TLS streams are planned.

The compiler's design is in
[docs/compiler/ARCHITECTURE.md](../compiler/ARCHITECTURE.md). Built
designs leave this folder for it: counts per instance, pointer loans and
raw buffers are [docs/compiler/ownership.md](../compiler/ownership.md),
the lifetime solver is
[crates/hd_check/DESIGN.md §15](../../crates/hd_check/DESIGN.md#15-lifetimes-one-region-solver),
the enum layout decisions are [crates/hd_wasm/DESIGN.md §23.14](../../crates/hd_wasm/DESIGN.md#2314-inline-aggregate-layouts),
release summaries are [crates/hd_opt/DESIGN.md §9](../../crates/hd_opt/DESIGN.md#9-release-summaries),
the REPL and its journal are [docs/compiler/live-execution.md](../compiler/live-execution.md),
and package host capabilities are [docs/compiler/package-host-capabilities.md](../compiler/package-host-capabilities.md).
