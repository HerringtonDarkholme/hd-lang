# 5.4 to 5.7: Requirements, Suspension, Runtime Library, Direction Benchmarks

Commit `bd985d7`, 2026-09-25. The worker could not write report files; the
coordinator wrote this file from the worker's final report. All timings ran
at a load average of 57 to 76 on 14 cores, because other audit workers were
running. The main estimate is the least-contended sample (minimum), and
comparisons use ratios within one run.

## Headline

- **Concrete requirement rows cost nothing per call.** A 10-deep chain with
  0, 1, 5, or 10 requirements costs 5.6 to 5.7 ns per call. The benchmark
  5-row/1-row ratio is about 1.0, in both dev and `-O2` builds.
- **Row-generic callbacks are quadratic.** With 1, 5, and 10 requirements
  they cost 13, 101, and 417 ns per call, and copy 1, 25, and 100 pack nodes
  per call (F-550).
- **Poll cost is O(depth).** At depths 1, 4, 16, and 64, a root poll costs
  404, 238, 1,094, and 7,760 ns, about 120 ns per level.
- **Suspension code size explodes.** 48 sequential bang calls in one function
  emit 549 KB of Wasm (9.3 MB of WAT) and take 1.17 s to compile. The
  control-flow form reaches 229 KB (F-552).
- **Cancellation order matches the spec.** With 3 frames and 6 registered
  `defer` suites, the log reads `214365`, as chapters 06 and 11 predict.
- **The runtime library is always inlined.** `fn main() -> i32: 0` emits 28
  functions, 27 of them unused; 2,458 bytes shrink to 37 after dead-code
  removal (F-557).
- **Strings cross the host boundary one byte per call.** A 1 MiB round trip
  through a host provider takes 374 ms (F-558).
- **MVP goal 2.** In-process compile plus validation has a median of 11 ms
  and a maximum of 90 ms; 0 of 301 programs exceed 1 s. Command-line wall
  time has a median of 687 ms, with 136 of 551 invocations over 1 s. The cost
  is Node startup plus loading Binaryen, not compilation (F-560).

## Commands

All run with `node --experimental-strip-types`:

- `audit/scripts/arch/req-probes.ts`;
- `req-constructs.ts audit/probes/arch/requirements/constructs.hd`;
- `bin/hd.js build --wat` on `requirements/abi.hd`,
  `requirements/host-wrapper.hd --profile ready-counter`, and
  `suspension/shape.hd`;
- `susp-probes.ts`;
- `timeout 5 ... bin/hd.js run --profile pending-gate audit/probes/arch/suspension/pending-spin.hd`;
- `rtlib-inventory.ts` (minimal and hello-world modules), `rtlib-imports.ts`,
  `rtlib-strings.ts`, `rtlib-assemble.ts`;
- `bench-run.ts`, twice;
- `bench-fixtures.ts`.

Shared helpers are in `bench-lib.ts`, and all scripts pass oxlint.

- The `-O2` build uses binaryen.js `setOptimizeLevel(2)`.
- `instantiateWithBytes` calls `instantiate()` from `src/compiler.ts`
  unchanged, with the optimized bytes swapped in, so both builds get the
  same host imports.

## 5.4 Requirement Passing

ABI (`abi.wat`, `row-chain-5.wat`, `host-wrapper.wat`):

| Row shape                          | What a call passes                                                        |
| ---------------------------------- | ------------------------------------------------------------------------- |
| concrete trait key                 | one `(ref null $traitN)` parameter per entry, after ordinary parameters, in sorted key order |
| opaque key                         | one `externref` parameter per entry                                       |
| row variable (including `r - K`)   | one `(ref null $hd.providers)` pack for the whole row                     |
| mixed row                          | the concrete parameters plus one pack                                     |

- **Pack layout:** a linked list of `{key i32, value anyref, parent}` nodes.
  `$.with` pushes onto the front, so inner bindings shadow outer ones.
- **Lookup:** `provider_get` walks the list linearly.
- **Host capabilities:** the entry export takes an `externref` and wraps it
  once per entry call (`$hd.host_traitN`, called only from the `main` and
  `__hd_start` exports).
- **Suspension frames:** one field per requirement, with no extra allocation.

Allocation per construct (`constructs-alloc.md`):

| Construct                     | Allocations                                                   |
| ----------------------------- | ------------------------------------------------------------- |
| `$.with` of an existing value | 0                                                             |
| `$.with` of a data value      | 1 data struct and 1 trait struct                              |
| `$.context`                   | 1 struct                                                      |
| spread `...context`           | 0                                                             |
| subtraction scope             | 1 node, plus a copy of the whole incoming pack (F-551)        |
| row-generic callback call     | 2 closure structs, K pack nodes, and K² copied nodes (F-550)  |
| closure capturing a provider  | 1 environment struct                                          |
| suspension frame              | 0 extra                                                       |
| host method call              | a host frame, a wrapper, and a JavaScript state object with a `Map` |

Per-call cost for a 10-deep chain, in ns (minimum / median):

| Requirements | Concrete chain | Row-generic chain | Nodes copied per call |
| ------------ | -------------- | ----------------- | --------------------- |
| 0            | 5.6            |                   |                       |
| 1            | 5.6            | 13.4 / 37         | 1                     |
| 5            | 5.7            | 101 / 516         | 25                    |
| 10           | 5.7            | 417 / 1,720       | 100                   |

**Verdict:** concrete rows add no per-call cost. Row-generic cost is
quadratic because of F-550; after fixing that, it would still be linear,
because lookup walks a list.

## 5.5 Suspension Lowering

Shape:

- **Frame:** one GC struct holding the state, poll count, parameters, bounds,
  providers, every local, one child slot per call site, and the result.
- **Dispatch:** nested `if state == k` chains.
  - Simple statement sites use linear lowering, which copies the rest of the
    body into every resume branch.
  - Other sites go through a `loop $cfg` that walks an `if pc == n` chain
    (F-553).
- **Spills:** every suspension point stores all locals, and every poll
  reloads all of them.
- **Child calls:** direct `call $pollN`.
- **Stored `Suspend[T]`:** a 4-field wrapper (inner value plus three function
  references); the result is boxed.

Measurements:

- **Poll cost against depth** (1, 4, 16, 64): 404, 238, 1,094, and 7,760 ns.
  Each level adds one `pending` and two `trace` host calls.
- **Code size against bang-call sites, linear lowering** (8, 16, 32, 48):
  9, 33, 182, and 549 KB of Wasm. 48 sites take 1.17 s to compile, or 4.3 s
  under heavier load.
- **Code size, control-flow lowering:** 229 KB for 48 sites.
- **Cancellation, 3 frames with several `defer` suites each:** log `214365`,
  the spec order. Normal completion gives `92148365`.
- **Driver guard:** one `$hd.driver-active` global per instance.
  - Instance B runs while instance A is started.
  - Re-entering the same instance panics with `suspension-competing-driver`,
    including re-entry from a host callback.
  - The guard stays set after a panic (F-556).
- **Pending host provider through `main`:** busy-polls forever; killed after
  5 s (F-555).

## 5.6 Runtime Library and Host Imports

Runtime helpers are inlined as WAT text.

- Always included: `runtime.wat`, the suspension runtime, and `map.wat`.
- Added on demand: the float, console, split, and transform pieces.

| Module         | Functions (kept after dead-code removal) | Globals unused | Types | Unused imports | Bytes (after dead-code removal) |
| -------------- | ---------------------------------------- | -------------- | ----- | -------------- | ------------------------------- |
| minimal `main` | 28 (1)                                   | 9 of 9         | 47    | `hd.panic`     | 2,458 (37)                      |
| hello world    | 29 (2)                                   |                |       |                | 2,585 (370)                     |

Assembling the minimal module takes 5.5 ms, compared with 0.12 ms for a
hand-written equivalent (F-557). Assembly is about 90% of in-process compile
time.

Host import surface, across 303 modules, all under module `hd`:

- `panic(i32)`: in every module;
- `trace(fn, event)` and `pending(fn, polls)`: in 88 modules, called on every
  frame poll;
- `console_byte(externref, i32)`: one call per byte;
- `pow_f64`, and `format_f64(f64, idx)`, which re-formats the value for each
  byte;
- `string_transform_begin`, `input`, and `output`;
- per host method: `host_T_M_begin`, `poll`, `cancel`, `result`,
  `argument_byte`, `result_length`, `result_byte`.

**Verdict:** the surface is narrow, but it is not versionable:

- it carries no version;
- import names come from declaration indices;
- call sites are keyed by source offset.

It is far from WASI 0.3 and the Component Model. It uses `externref`
handles, byte streaming, host polling without a waker, and development hooks
in the production surface, where those use the canonical ABI.

String crossing (F-558):

| Size    | Host-provider round trip        | `println` |
| ------- | ------------------------------- | --------- |
| 1 KiB   | 0.52 ms                         | 0.11 ms   |
| 128 KiB | 32.8 ms                         | 2.3 ms    |
| 1 MiB   | 374 ms (250 to 510 ns per byte) | 21.8 ms   |

## 5.7 Direction Benchmarks

Two runs; minimum values agree within 2%.

| Benchmark               | Dev bytes | `-O2` bytes | Dev ms (min) | `-O2` ms (min) |
| ----------------------- | --------- | ----------- | ------------ | -------------- |
| b1 `fib(27)`            | 2,548     | 194         | 1.26         | 0.84           |
| b2 generic sum          | 2,882     | 651         | 7.7          | 7.7            |
| b2 monomorphic sum      | 2,832     | 642         | 7.25         | 7.25           |
| b3 chain, 1 requirement | 2,982     | 548         | 8.2          | 7.9            |
| b3 chain, 5 requirements | 3,550    | 841         | 8.06         | 7.94           |
| b4 `fn!` loop           | 4,529     | 2,078       | 10.1         | 10.1           |
| b4 plain loop           | 2,617     | 217         | 0.79         | 0.79           |

Ratios (dev / `-O2`):

- erased / monomorphic: 1.06 to 1.08, both builds;
- 5-row / 1-row: about 1.0;
- suspending / plain: 12.8 / 12.7, or 11.1 with the development hooks
  stubbed out (F-554);
- scalar dev / `-O2` speedup: 1.49.

Compile time was 8 to 90 ms per benchmark, 70 to 95% of it in Binaryen
assembly.

**Reading:** V8 already removes most of the waste in development emission.
Only suspension carries a large cost that comes from the representation
itself (F-554).

Note: the b2 "monomorphic" program still stores into a concrete `list[i32]`,
which is itself boxed (F-505). The 5.3 erasure benchmark measured 3 to 7
times for generic against concrete sum. The coordinator's interpretation is
that boxing dominates both b2 variants, which would explain why b2 shows only
1.07. This is `UNVERIFIED`: the two benchmarks were not run side by side.

Hot-path WAT observations:

- `$adapt0` casts the same environment value twice.
- List elements are boxed on `append` and unboxed on each call in `tadapt`.
- The b4 poll function has 15 locals, all spilled and reloaded, with at
  most 3 live.
- `ref.as_non_null` runs on every loop test.
- `$hd.add_i32` and `$hd.sub_i32` are not inlined, even at `-O2`.
- Relational operators go through a three-way compare that survives `-O2`.
- Every top-level function is exported, which blocks dead-code removal and
  inlining.
- The trait struct allocated per element in the generic sum is covered by
  F-502.

MVP goal 2, over 551 invocations (209 selected conformance cases plus 342
`test/fixtures`):

| Measure                            | Median  | p90      | Max      | Over 1 s |
| ---------------------------------- | ------- | -------- | -------- | -------- |
| command-line wall time             | 687 ms  | 1,324 ms | 5,095 ms | 136      |
| in-process compile plus validation | 11.1 ms |          | 90 ms    | 0        |

Importing Binaryen alone takes 0.9 to 1.8 s under this load (F-560).
**Verdict:** compilation meets the goal; process startup does not.
Re-measure on an idle machine.

## Findings

| ID    | Severity | Title                                                                        |
| ----- | -------- | ---------------------------------------------------------------------------- |
| F-550 | major    | row-generic callback adapter copies the provider pack once per lookup        |
| F-551 | minor    | `provider_concat(left, null)` copies the left pack instead of sharing it     |
| F-552 | major    | suspension code size grows super-linearly with bang-call sites               |
| F-553 | minor    | resume and control-flow dispatch use linear `if` chains, not `br_table`      |
| F-554 | minor    | an immediately ready `fn!` call costs about 13 times a plain call; `-O2` does not help |
| F-555 | minor    | the `main` export busy-polls forever when a host provider stays pending      |
| F-556 | note     | the driver guard stays set after a panic, so later calls report the wrong panic code |
| F-557 | minor    | every module embeds the full runtime library                                 |
| F-558 | minor    | strings cross the host boundary one byte per import call                     |
| F-559 | minor    | two type-checked fixtures crash Wasm emission with an internal error         |
| F-560 | minor    | every command loads Binaryen, which dominates the edit loop                  |

## Phase-7 Questions

1. `Suspend[T]` is sealed. May a statically known frame skip the uniform
   wrapper and result boxing, or does storage in `mut Suspend[T]` require the
   dynamic wrapper?
2. Must row variables use keyed lookup, or could a canonical key order make
   packs positional? Does the requirement model force per-call cost only for
   row-generic code?
3. Is an entry driver that busy-polls without returning to the host
   conforming? No conformance case observes waker-driven progress.
4. Should invoking an instance after a panic have a defined result?
5. Should the spec define a development host boundary, or only the
   Component Model boundary? Should strings cross it through the canonical
   ABI?
6. Should the spec explicitly permit frame liveness optimization?
7. Should a portable fixture cover several `defer` suites per frame across
   three or more frames?

## Not Done

- Idle-machine timings: load stayed at 4 to 5 times the core count.
- Runtime allocation counts: only the provider-pack helpers were
  instrumented; other counts are static, read from the WAT.
- Pack lookup cost in isolation: it cannot be separated from the F-550
  copying without editing `src/`.
- Host-call counts for strings: derived from the emitted protocol, not
  counted at runtime.
