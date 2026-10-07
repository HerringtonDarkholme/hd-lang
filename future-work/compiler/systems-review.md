# New Compiler: Systems Performance Review

Status: Review, 2026-10-07.

Part of the [compiler design](README.md). This review prices the design.
It decides nothing and edits no design file. The lowering pass
(`wasm-layout.md`, `codegen.md`, `cache.md`, `engines-and-test-runner.md`,
`runtime-and-host.md`) was being edited while this was written, so a
finding against those files may already be stale. Each finding names the
sentence it was read from.

## Why This Review Exists

The owner found that the design spawned one work-stealing task per
function body. That was fixed in [scheduler.md](scheduler.md), section
6.2, "Granularity" (commit ea647647). The Codex reviews covered
soundness, determinism and incrementality. Nobody had priced the design:
counted the units of work, the overhead per unit, the locks, the
syscalls and the bytes. This review does that for every stage, against
the targets in [goals.md](goals.md).

## How It Was Done

- Read every design file the README lists, plus the two representation
  studies.
- Measured `lib/std` with a throwaway example over `hd_syntax`'s `lex`,
  `skim` and `parse` (not committed).
- Measured cache-style file I/O on this machine with a throwaway example:
  2,000 entries of 2 KiB, written by write-then-rename into 256
  sub-directories, then read, stat'ed, scanned and deleted.

### What Was Measured

**`lib/std` shape** (36 files, 484,556 bytes, 11,527 lines):

| Measure | Value |
| --- | --- |
| bytes per line, tokens per line | 42.0 and 6.0 (the design assumes 35 and 8) |
| identifier tokens | 23,064 of 69,163 tokens |
| top-level function bodies (skim's `Function` kind) | 835 |
| body size, mean and median | 240 B and 37 tokens; 118 B and 19 tokens |
| body size, p90, p99, max | 543 B / 83 tokens; 1,750 B / 294 tokens; 9,212 B / 584 tokens |
| bodies of 20 tokens or fewer | 436 (52%) |
| share of body bytes in the largest 10% of bodies | 44% |
| bodies per file, median, p90, max | 10, 56, 125 |
| full parse of all of std, one thread | 3.8 ms (about 127 MB/s); largest file `json.hd`, 0.40 ms |

So the granularity fix was right, and needed more than the design says.
[scheduler.md](scheduler.md) says "`lib/std` averages about 500 bytes per
body". The measured mean is 240 bytes and the median is 118. Half the
bodies would check in a few microseconds, about the cost of one spawn and
steal.

**File I/O** (APFS on an Apple Silicon machine with 14 cores, shared
with other agents; load average 5 to 16 during the runs; two runs,
consistent):

| Operation | Cost per entry |
| --- | --- |
| publish a 2 KiB entry (create temp, write, close, rename), 1 thread | 342 to 354 µs |
| the same with 8 threads | 276 to 281 µs of wall time per entry: no scaling |
| open, read, close a 2 KiB entry | 59 to 128 µs |
| `stat` | 2.0 to 2.4 µs |
| directory scan plus `stat` per entry | 4.6 to 8.5 µs |
| unlink | 48 to 73 µs |

These numbers are far above the 15 to 30 µs per open that
[representation-compile.md](representation-compile.md) assumes. Part of
the gap is the load, and part is likely the platform's file-event and
security hooks. A Linux CI runner is likely 5 to 20 times cheaper. That
is an estimate, not a measurement. The owner's machine class is the Mac,
so these are the numbers an agent on that machine pays.

## The Ten Most Important Findings

Ranked by what they cost against a target, times how often the case
occurs.

### 1. One File Per Cache Entry, At Every Granularity

**Where.** [cache.md](cache.md) §5.4: "`<kind>/<2 hex>/<32 hex>` one
file per key". [codegen.md](codegen.md) §11.1: "write Wasm ═► [code
entry] per instance, parallel". [engines-and-test-runner.md](engines-and-test-runner.md)
§18.2: "an adapter maps it onto `CacheStore` as entry kind `cranelift`",
one entry per function. Coherence writes one `coh` entry per trait, the
parser one `locs` entry per file, stage B one `hdr` entry per folder.

**The problem.** This is the per-body task problem again, at the
storage layer. Each entry pays a create, a write, a close and a rename
on publish, and an open, a read and a close on every hit. The work
inside an entry is often smaller than the syscalls around it. An empty
`coh` or `hdr` entry holds no diagnostics at all.

**The numbers** (10k-line package, 100 files, 20 folders):

| Run | Entries | Cost at the measured Mac rates |
| --- | --- | --- |
| cold `hd check` | about 100 `check` + 100 `tir` + 100 `locs` + 20 `iface` + 20 `hdr` + 40 to 80 `coh` + 1 `pkgres` ≈ 400 publishes | about 140 ms, 14% of the 1 s `cold-check` target, and threads do not help |
| cold `hd build` | adds about 4,800 `code` + 4,700 `cranelift` + 1 `link` + 1 `cwasm` ≈ 9,500 publishes | about 3.2 s of syscalls alone |
| warm `hd run` after one edit | reads about 4,800 `code` + 4,700 `cranelift` entries | 0.6 to 1.2 s on one core |
| warm `hd check` after one edit | reads about 200 entries (finding 5) | 12 to 25 ms of the 50 ms `edit-latency` p50 |

**What breaks.** `cold-check` loses a seventh of its budget.
`test-latency` (300 ms) and `hd run` latency fail on the Mac unless
the link reads few files. `cache-growth` and eviction pay per file too
(finding 6).

**The fix.**

1. One entry per module for everything keyed by that module: `check`,
   `tir` and `locs` become sections of one container. `tir` is already
   keyed `H("tir", check_key(m))`, and `locs` is keyed by the same
   file's `source_hash`.
2. One `graph` entry per package run for `coh`, `hdr` and `init`, keyed
   by the sorted keys of its parts. A part whose result is empty stores
   nothing; its key in the list is enough.
3. Code packs per module and tier: every instance whose item lives in
   module `m` goes in one pack, keyed by the sorted code keys of its
   members. A pack miss re-emits the module's instances (about 50 µs
   each) and reuses unchanged bytes from the old pack. This is close
   to change 3 of [representation-compile.md](representation-compile.md),
   which packs per program.
4. The Cranelift adapter reads and writes one pack per program, never a
   file per function.
5. Measure the per-file cost on both machines in slice 4 and slice 6.
   Keep a file per entry only where entries are large (`cwasm`, `link`,
   big `tir`).

**Confidence.** High on the counts. Medium on the per-file cost, which
was measured on one loaded machine. Even at 20 µs per file, the cold
build pays about 0.2 s and the warm `hd run` about 0.2 s.

### 2. One Test Program Per Module Multiplies Link, Cranelift And Disk

**Where.** [engines-and-test-runner.md](engines-and-test-runner.md)
§19.1: "one program per module with unit tests, one per integration test
file, one per module's doc tests." §19.1 step 3: "Each program goes
through `Collect`, `Emit`, `Link` and `Precompile`".

**The problem.** Programs share code entries, but each one collects,
links, Cranelift-compiles and stores its own copy of the shared code.
The shared part dominates: std's collections, formatting, `Debug` and the
`std.testing` harness are in every program.

**The numbers.** `tests-1k` (1,000 tests in 100 modules) gives about 100
unit programs and up to 100 doc-test programs.
[representation-compile.md](representation-compile.md) §2.4 sizes one
module's test program at 800 instances and 250 KB of code.

| Cost | One program | 100 programs |
| --- | --- | --- |
| collect | 2 ms | 0.2 CPU-s |
| link reads (800 entries) | 50 to 100 ms at the Mac's rates | 5 to 10 s, unless packed |
| Cranelift, cold, at 1.5 µs per byte | 0.38 CPU-s | 38 CPU-s if nothing is shared |
| Cranelift, all functions hit the cache (25% of a compile) | 0.09 CPU-s | 9 CPU-s |
| `cwasm` on disk, 1 to 2 MB each | 1 to 2 MB | 100 to 200 MB per package state |

An edit to a module that most modules use changes the `prog_key` of
every program that reaches it (finding 4 makes this worse). One such
edit then costs 100 links, 100 Cranelift lookups and 100 to 200 MB of
new `cwasm` entries. At a few hundred edits a day per agent, a 10 GB
cache turns over within a day.

**What breaks.** `unit-test-perf` cold. A full `hd test` after an edit:
several CPU-seconds where `test-latency` budgets 300 ms for one module.
`cache-growth`, and `disk`.

**The fix.** One unit-test program per package (or per folder), with one
init export per module. A case calls the init export of its own module,
which runs exactly the init groups that module reaches. That is the same
set a per-module program initializes today, so `module.testing.instance`
holds unchanged. Shared code is collected, linked and compiled once. A
module's edit then relinks one program. With per-module code packs
(finding 1) the link reads about 100 packs, not 80,000 entries. Doc tests
join the same program.

**Confidence.** Medium. The Cranelift cache's hit cost and the `cwasm`
size are unmeasured (spike 0c S3). The fan-out arithmetic does not depend
on them. This is open question 1.

### 3. A Public Edit Rechecks Every Module Whose Closure Holds Its Folder

**Where.** [cache.md](cache.md) §5.3: `check_key(m)` hashes "sorted
[(folder path, deep_hash) for each folder in closure(m)]", m's own
folder included. "Now any public API edit in it rechecks m. Public API
edits are much rarer than body edits".

**The problem.** The unit of invalidation is the folder's whole public
interface. Adding a `pub fn` to `src/util` rechecks every module of the
package whose closure holds `src/util`, whether or not it names the new
function. In a flat package (one folder) it rechecks every module.
Agents add public items and change signatures often: a new helper, a new
parameter, a new field. "Much rarer than body edits" is not
established for agent edit streams.

**The numbers.** `ordinary-10k`: 100 modules over 12 folders. A public
edit in a low folder, which most modules reach, rechecks about 100
modules. At 5 to 10 ms of CPU per 100-line module, that is 0.5 to 1
CPU-s, or 60 to 125 ms of wall time on 8 cores, and about 1 s on one
core. Then every program that reaches those modules relinks (finding 4).

**What breaks.** `edit-latency` p95 (200 ms) on one core, for the edit
kinds that add or change a public item. `recheck-precision` says
"dependents only for a signature edit". This design rechecks
non-dependents in the same folder and modules that never name the item.

**The fix: early cutoff by recorded reads.**

1. Each `check` entry stores what the module actually read from other
   folders: the `(stable path, per-item interface hash)` list. The
   `tir` entry already stores this per body as `deps`. Add the
   `heads_hash` of each owner module whose impl table the solver probed,
   and the `ImplUniverseId`'s folder list.
2. The per-worktree last-run record (finding 5) maps each module path to
   its previous `check_key`.
3. On a `check_key` miss whose `source_hash` is unchanged, read the old
   entry's read list and compare every hash against the current blobs.
   All equal: reuse the old result under the new key. One unequal:
   recheck.

This is rustc's red-green marking at module granularity, not a query
system. It keeps "no salsa, no daemon". Verify mode checks it on every
reuse. It needs one extra contract: every read that can change a
module's result goes into its read list. The checker already funnels all
cross-folder reads through interface readers, so the list is collected
in one place.

**Confidence.** High on the cost. Medium on the effort: the read list
must be complete, and the key-completeness tests (§21.4 of the backend)
must cover it. This is open question 2.

### 4. Codegen Has No Early Cutoff: A Comment Edit Relinks Every Program

**Where.** [codegen.md](codegen.md) §11.3: `prog_key` hashes "sorted
[(module path, tir key)] of the modules reachable in the use graph".
[checking-and-tir.md](checking-and-tir.md), "Lifetime And The `tir`
Entry": "The entry's key is `H("tir", check_key(m))`." `check_key`
holds `source_hash(m)`.

**The problem.** Any byte change in a module changes its `tir` key: a
comment, a blank line, a `tests:` block, a doc comment. So every program
that reaches the module misses its `prog_key`. Each one then collects,
computes 800 to 4,800 code keys, reads every code entry, folds, links,
and runs every function through the Cranelift cache. The design says it
itself: "An edit to a reachable module that changes no instance still
misses, and then rebuilds from code-entry hits."

**The numbers.** Editing a `tests:` block in a module that 50 test
programs reach: 50 links of 800 entries (40,000 reads, 2.4 to 5 s at the
Mac's rates) and 50 Cranelift lookup passes (12 to 90 ms each). The
emitted bytes are identical every time.

**What breaks.** `test-latency` on any edit to a widely used module.
`hd test --affected`, whose fingerprint is `H(prog_key, ...)`
([commands.md](commands.md) §7.4): a comment edit reruns every affected
program's tests.

**The fix.** Key programs by TIR content, not by check keys:
`prog_key = H("prog", toolchain, tier, profile, roots, sorted [(module
path, tir_content_hash)])`, where `tir_content_hash` hashes the module's
per-item TIR hashes, inline summaries and dependency lists, all already
in the `tir` entry. A comment edit then rechecks one module, writes an
equal `tir` content hash, and every program hits. Use the same content
hash for `--affected` fingerprints.

**Confidence.** High.

### 5. A Warm Edit Reads Entries In Proportion To The Program

**Where.** [data-structures.md](data-structures.md) §3.20.4: "A warm
check reads the manifest, about 100 `iface` and `check` headers (64 bytes
each, plus their `diags`), and writes one `check` and one `tir` entry."
[commands.md](commands.md) §7.1 steps 6 to 8: each folder's `iface` key
"on a hit, read the entry's header", each module's `check` key "on a
hit, take its result", then `coh` and `init`.
[goals.md](goals.md), `io-per-check`: "reads proportional to what
changed".

**The problem.** On a fast-key miss the run rebuilds `pkgres` from
every part. It opens every `iface` entry to learn deep hashes, every
`check` entry to collect diagnostics, and every `coh` and `hdr` entry.
That is linear in the program, not in the edit, and it contradicts the
`io-per-check` target.

**The numbers.** 10k lines: about 20 `iface` + 100 `check` + 40 to 80
`coh` + 20 `hdr` ≈ 200 opens, 12 to 25 ms on the Mac: 25 to 50% of the
50 ms p50. 50k lines: about 1,000 opens, 60 to 120 ms.

**The fix.** A per-worktree last-run record, `build/.hd/last-run`, written
with the manifest:

- each folder's `iface_key` with its `deep_hash` and `heads_hash`;
- each module's `check_key` with its diagnostics (or their offsets in the
  previous `pkgres` entry);
- the `coh`, `hdr` and `init` keys with their diagnostics.

On a fast-key miss, keys are recomputed bottom-up from the manifest and
this record. Only parts whose key changed are read or computed. A missing
or stale record falls back to today's path. The record holds paths, so it
stays local, like the manifest. Reads become proportional to the edit.

**Confidence.** High.

### 6. Eviction Cannot Be Both LRU And Bounded To 20 ms

**Where.** [cache.md](cache.md) §5.7: the run "scans `obj/` after its
output is flushed, deletes entries in order of oldest use until the
total is at most 90% of the cap", and "the work is bounded to about
20 ms per run".

**The problem.** Oldest-first needs a full scan. A full scan of a 10 GB
cache of small files does not fit in 20 ms. A partial scan does not give
LRU order. Meanwhile one `hd test` after a widely used edit can write
100 MB or more (finding 2).

**The numbers.** With per-instance and per-function entries, a 10 GB
cache of 5 to 10 KB entries holds 1 to 2 million files. At the measured
4.6 to 8.5 µs per scanned entry, a full scan takes 5 to 17 s. At 48 to
73 µs per unlink, 20 ms deletes about 300 files, or 1.5 to 3 MB of small
entries. Inflow can exceed that by 50 times.

**What breaks.** `cache-growth` (the cap is not enforced while agents
are busy), and the owner's 10 GB cap.

**The fix.** ccache's scheme. Each of the 256 shard directories keeps
its own size counter. A run that pushes a shard over cap / 256 cleans
that shard alone: about 4,000 to 8,000 files at 1 to 2 million total,
20 to 70 ms of scan at the Mac's rates, LRU within the shard. Packs (finding 1) cut the file
count 20 to 50 times, which makes each shard scan cheap. Large entries
(`cwasm`, `link`) live in their own shards, so one unlink frees
megabytes. This is approximate LRU; open question 3 asks the owner to
accept that.

**Confidence.** High on the arithmetic.

### 7. The Browser's Uninterruptible Step Grew From One Body To One Module

**Where.** [scheduler.md](scheduler.md) §6.2: "A step is one whole task,
and a task cannot yield inside itself". The granularity rule: "Inside a
module's body task, the bodies run as one rayon parallel iterator". The
page "terminates the compiler worker and starts a fresh one" after
200 ms without a return.

**The problem.** The granularity fix is right for threads and wrong for
the stepping executor. Before it, a step was one body. Now it is every
M2 body of a module. A playground program is usually one module, so one
step is the whole body check.

**The numbers.** A 1,000-line program checks in an estimated 10 to 30 ms
natively, and 1.5 to 2.5 times that in Wasm: 15 to 75 ms in one step. A
3,000-line paste takes 50 to 200 ms or more and crosses the 200 ms kill.
A restarted worker reloads `hd_web` (several MB of Wasm, compiled from a
cache) and IndexedDB, and rebuilds interners and std memos: an estimated
50 to 300 ms. A user who types faster than one check plus one restart
never sees a result.

**The fix.** The serial and stepping executors run a module's body batch
as a loop with a cursor, one body per step, and keep M1's walk resumable
the same way. The batch is a scheduling unit for threads only; the item
stays the logical unit, as scheduler.md says. With that, the longest
step is the largest body, bounded by its fuel. Also debounce the kill:
terminate only if the run is stale and has not yielded for 200 ms.

**Confidence.** High on the mechanism; the times are estimates.

### 8. A One-Function Edit Pays For Its Whole Module, Serially In Parts

**Where.** [type-checking.md](type-checking.md) §1.7: "The first
release still rechecks the whole module". M1 and M3 are "serial".
[build-order.md](build-order.md) §9.1: "a 10k-line single-file module
writes about 2 MB of TIR".

**The problem.** Three costs scale with the edited module, not the edit:
M1 checks every private function with an omitted result serially; M2
checks every body; `ModuleFinish` remaps and writes the whole module's
`tir` entry before anything is printed. On one core (the `concurrency`
metric gives an agent about one), M2 is serial too.

**The numbers.** A 1,000-line module: about 8,000 tokens, an estimated
10 to 25 ms of checking on one core, plus 0.2 ms of remap and one 210 KB
write. `one-file-10k`: 80 to 250 ms of checking on one core, 2 ms of
remap (200,000 ID words at 10 ns), a 2 MB write. That misses the 50 ms
p50 and approaches the 200 ms p95.

**The fix.**

- Print diagnostics before publishing entries. Publishing the `check`,
  `tir` and manifest entries moves after output, as
  [representation-compile.md](representation-compile.md) already
  proposes for `cwasm`. On the Mac this takes 1 to 3 ms of publish off
  every warm edit.
- Write `tir` only for modules whose result is error-free; a module with
  errors cannot be built.
- Later, a per-body cache inside the module entry (type-checking.md
  already notes that M2 results are functions of frozen inputs).
  Finding 3's read lists are the same mechanism one level down.

**Confidence.** Medium; the check throughput is unmeasured until slice 3.

### 9. Shared Locks On Every Intern And Memo Hit

**Where.** [data-structures.md](data-structures.md) §3.3: "**Lookup**
hashes the content, locks the shard `hash % 64`, and probes".
[trait-solver.md](trait-solver.md) §7.1: `GlobalMemo` has `shards:
[Mutex<RawTable<u32>>; 64]`, and §13 targets "memo hit, global or body:
under 100 ns". Rule TS-5: "When a body asks a goal, the solver walks the
goal's proof DAG depth first".

**The problem.** Hits take the lock too. The hottest keys (`i32`,
`string`, `Option[i32]`, `i32: Display`, `string: Eq`) land on fixed
shards, so 8 threads pass a few cache lines back and forth. TS-5 makes a
memo hit cost one lookup per node of the proof DAG when a body first
asks a goal. If the memo stores children as canonical goals, each node
is another locked lookup.

**The numbers** (estimates for a cold 10k-line check with std): about
200,000 intern calls, 50,000 memo lookups, and about 150,000 DAG-walk
nodes (1,000 bodies × 30 distinct goals × 5 nodes). That is about
400,000 locked operations. Uncontended, a lock and unlock costs 20 to
40 ns. With the line last written by another core it costs 60 to 150 ns,
and more across clusters. Total: 10 to 60 ms of CPU, 1 to 6% of the 1 s
cold check, with bursts on hot shards. The "under 100 ns" hit target
fails under contention.

**The fix.**

1. Store a memo entry's children as entry indices into the append-only
   arena, so the TS-5 walk reads without locks.
2. Put a small per-worker read-through table (for example 4,096 slots,
   direct-mapped) in front of the interner and the global memo. Global
   entries never change once published, so a stale-free cache needs no
   invalidation within a run.
3. Look up pre-seeded types (primitives, common options and lists) in
   the static table before hashing to a shard.

**Confidence.** Medium. The counts are estimates; slice 4 should count
lock acquisitions and contended waits.

### 10. A Non-Generational Collector Gives Pauses Proportional To The Live Heap

**Where.** [engines-and-test-runner.md](engines-and-test-runner.md)
§18.1: "collector: the default copying collector".
[representation-runtime.md](representation-runtime.md) §5.1 covers heap
growth but not pause length.

**The problem.** A semi-space copier copies the whole live set on every
collection and needs twice the live heap in address space. There is no
young generation, so short-lived garbage is collected at the price of
the whole live set.

**The numbers.** The `runtime-suite` fixture has a 200 MB live heap. At
1 to 2 GB/s of copying, each collection pauses for 100 to 200 ms and
reserves 400 MB. A service in `long-run-memory` with 50 MB live pauses
25 to 50 ms per collection. One such case can break the `runtime` guard
("no case > 3x") whatever the geomean.

**The fix.** Measure pause length and collection count per fixture in
slice 7, not only throughput. Size the initial heap from the profile, as
the runtime study proposes. Report p99 latency of a request loop in
`long-run-memory`. If pauses fail, the options are the engine's other
collectors or a smaller live set by layout (proposal A, B); none is a
compiler-side fix.

**Confidence.** Medium; it depends on the pinned wasmtime's collector.

## Per-Stage Table

Units are for the `ordinary-10k` fixture (100 files, 12 to 20 folders,
about 1,000 bodies) unless the row says otherwise. "Mac" means the
measured file costs above.

| Stage | Unit of work | Units: check / build | Overhead per unit | Risk | Verdict |
| --- | --- | --- | --- | --- | --- |
| lexing | file | 100 / 100 | none beyond the loop; 263 MB/s, 1.3 ms in all | none | fine |
| skimming | file, warm misses only | 0 to 20 | same as lexing; 147 MB/s | none | fine |
| parsing | file | 100 cold, 1 warm | 95 MB/s; 3.7 ms cold in all; the largest 1,000-line file about 0.4 ms | parse tasks have no dependency, so cold runs hold every tree at once (about 2.3 MB), not "8 modules" as §3.24 assumes | fine; fix the estimate |
| discovery and stat | file, directory | 100 stats, about 20 directory reads | 2 µs per stat (measured) | `tiny-files` (10,000 files): 20 to 50 ms of stats on the fast path | fine; use `getattrlistbulk` on macOS |
| folder graph | package | 1 | tiny | none | fine |
| resolution and `FolderIface` | folder, serial inside | 12 to 20 | one task per folder | a flat package is one serial interface task over all headers: an estimated 3 to 10 ms on the cold critical path | fine; measure |
| interface blobs | type, path or string record on first use | a few thousand | one shard lock per first intern | finding 9 | fine with the per-worker table |
| cache keys | key | about 400 cold | xxh3 over tens of bytes; closure lists cost O(modules × folders): 6,000 pairs at 10k, 50,000 at 50k, under 1 ms | none | fine |
| cache writes | entry | about 400 check / 9,500 build | 340 µs (Mac) | finding 1 | **pack** |
| cache reads, warm edit | entry | about 200 | 60 to 120 µs (Mac) | finding 5 | **last-run record** |
| stat manifest | file record | 100 | 88 B; one read, one write | a probe file is created every run, even when nothing changes | fine; create the probe only before the first hash |
| eviction | entry | up to 1 to 2 million in the cache | 5 to 8 µs scan, 50 to 70 µs unlink | finding 6 | **shard it** |
| scheduler | module task | about 600 cold (6 per module), a few warm | 0.2 to 1 µs per task, a lock per edge | none after ea647647 | fine |
| M1 (`ModulePrep`) | module, serial inside | 100 cold, 1 warm | serial omitted-result walk | finding 8 | watch; lint suggests result types |
| M2 (bodies) | body batch per module | about 1,000 bodies in about 100 batches | rayon split on steal; LPT order | the stepping executor (finding 7) | fine natively |
| M3 (`ModuleFinish`) | module | 100 | linear row sweep; the tier is in the word, no pool load | a deep `Includes` worklist is bounded by fuel, not linear (fine) | fine |
| trait solver memo | goal | an estimated 50,000 lookups | a shard lock per lookup | finding 9 | fix the walk |
| TIR building | instruction | about 70,000 | reused worker columns; one output copy per body | none | fine |
| TIR serialization | ID word | about 200,000 cold, 20,000 warm | about 10 ns (memo probe) | `one-file-10k` writes 2 MB per edit | fine; finding 8 |
| coherence | trait | 40 to 80 | one task and one entry per trait | empty entries (finding 1) | **fold into one entry** |
| fix-it verification | fix-it | up to 20 per file | one re-lex and re-parse of the whole file each | 20 × 0.4 ms = 8 ms per 1,000-line file with many errors | re-parse only the enclosing item |
| collection | instance | 4,800 per program | about 1 µs | × programs in a test plan (finding 2) | fine per program |
| emission | instance, batched per module | 4,800 cold, tens warm | about 50 µs | in-process duplicates when two programs miss one code key at once | key `Emit` tasks by code key |
| merging | body per round | 4,700 × 2 to 3 rounds | one hash | deep call chains: the worklist fix in the study | fine |
| linking | code entry | 4,800 reads | 60 to 120 µs (Mac) | finding 1 | **pack** |
| Cranelift | function | 4,700 | 1.5 µs per byte cold; about 25% of that per hit | numbering (the study's change 1); per-function entries (finding 1) | fix both |
| `cwasm` | program | 1 for `hd run`, 100 to 200 for `hd test` | 1 to 2 MB each | finding 2 | **fewer programs** |
| instantiation | test case | 1,000 | 5 to 20 µs, plus init: about 1,500 constant allocations, 50 to 100 µs | covered by the runtime and compile studies | measure in slice 8 |
| GC | collection | depends on the program | ∝ live heap | finding 10 | measure pauses |
| host calls | call | depends | 20 to 50 ns for a scalar import (estimate) | an epoch thread wakes every 1 ms per process | fine; tick only while a store has a deadline |
| test runner | case | 1,000 | about 0.1 ms plus init | as above | fine if init is small |
| REPL | input | 1,000 per session | name lookup walks every earlier input | quadratic lookups (other findings) | fix the scope |
| browser worker | step | one module per step | up to the whole program | finding 7 | **step per body** |
| CLI startup | process | 1 | 8 to 9 MB of pages (estimate) | binary relocations, std pack checks | plausible; measure |

## All Other Findings

Grouped by the checklist. Each line gives where, the number, and the
fix. Findings already in the representation studies are marked "known"
and only cross-checked.

### 1. Granularity

- **The task enum still has per-item tasks.** [scheduler.md](scheduler.md)
  §6.1 lists `Body(ModuleId, ItemIdx)`, and [data-structures.md](data-structures.md)
  §3.21 says "a 10k-line package makes about 2,000 tasks cold". After
  ea647647 it is about 600 (six per module plus folders). The `TaskNode`
  layout's `ItemIdx` payload and the 2,000 figure should follow the
  granularity rule, or a later agent will rebuild per-item tasks from the
  enum.
- **`Coherence(trait)` and `HeaderCheck(folder)` are tiny units.** Most
  traits have two to five impls; a coherence task does microseconds of
  work, then a cache publish of hundreds of microseconds (Mac). Run all
  coherence as one task over the sorted list of traits whose key
  changed, and store one entry (finding 1).
- **The serial cost estimate is in skim token counts, which skim does
  not keep.** scheduler.md orders bodies "by the skim token count".
  Skim stores no tokens; `BodyRange` holds byte offsets. Body bytes are
  an equally good estimate (the measured token-to-byte ratio is steady).
- **Per-diagnostic work is fine.** Diagnostics are buffered per body and
  sorted per module. The one per-unit cost is fix-it verification (the
  table above).
- **Per-host-call and per-await units are fine.** A scalar import is one
  native call. The epoch ticker is per process, not per call.
- **Per-test-case cost** is init-dominated (known; engines §19.3,
  representation-compile §2.5).

### 2. Contention

- **Interner and memo shards:** finding 9.
- **The interface memo arrays** (`ty_memo`, `path_memo`, `sym_memo`, one
  `AtomicU32` per record) take racing first writes from many threads.
  16 records share a cache line, so the first decode of a popular blob
  bounces lines across workers. It is a one-time cost per record per
  run: a few thousand misses, under 1 ms. Fine.
- **Task graph edges** take a small lock per producer. At about 600
  tasks it cannot show up. Fine.
- **Diagnostic sink:** none is shared; diagnostics stay in module
  results until `PackageResult`. Fine.
- **Instance table:** per program, serial collection. Fine. The
  selection table is shared across programs (codegen.md §13.2); give it
  the same per-worker read-through table as the memo.
- **Cache directory:** writes from 8 threads did not scale on APFS
  (276 µs per entry with 8 threads against 342 µs with one). Parallel
  publishing does not hide the per-file cost; packing does.
- **Cross-process:** two processes that miss one key both compute it
  ([cache.md](cache.md) §5.4). With packs, the duplicated unit is a
  module or a program, so the waste per collision grows. Keep it, but
  count duplicate computations in `cache-contention`.

### 3. Locality

- **TIR columns** are read together by emission (tag, data, type, span
  for each instruction). data-structures.md §3.9.4 already says slice 3
  measures them both ways. Collection reads `tags` alone, which suits
  columns.
- **Body batches keep a module's scope on one core** only until rayon
  steals half the range. A steal moves the module's scope tables to the
  thief's cache. With a 0.5 to 1 ms minimum split, a steal happens at
  most once per 1 ms of work, so the refill (tens of KB) is noise.
- **Emission reads mapped `tir` entries** of other modules when it
  inlines. Each inlined callee is a random read into another mapping.
  With packs per module this is one mapping per module, warm after the
  first touch. Fine.
- **The global pool is split per owner thread.** A type interned by
  thread 3 lives in thread 3's columns, so other threads read it from a
  line that thread 3 may be writing nearby. Appends never write an
  existing item's line again after publication, except the line that
  holds the newest items. Fine.

### 4. Allocation And Memory

- **Cold check peak.** §3.24 assumes 8 modules' trees in flight. Parse
  tasks have no predecessors, so a cold run parses every file first:
  about 2.3 MB of tokens and trees at 10k lines and 11.5 MB at 50k lines,
  plus module results waiting on `ModuleFinish`. Fine against 50 MB, but
  the estimate should use the whole package. Throttling parses behind
  folder readiness would restore the 8-module figure.
- **Per-body output allocation:** one exact-size copy per body. At about
  1,000 bodies that is about 1,000 allocations, fine.
- **Emission buffers:** a few hundred KB per worker; fine.
- **Cranelift dominates a build** (20 to 40 MB estimated). With 100 test
  programs compiled in parallel, the peak is per concurrent program, so
  cap concurrent `Precompile` tasks at 2 or 3, not `--jobs`, or the peak
  multiplies.
- **The playground `MemoryStore`.** [cache.md](cache.md) §5.8: "Before a
  run the JS host loads the playground's entries from IndexedDB into the
  `MemoryStore`." If that happens before each run rather than once per
  worker, a 50 MB store costs about 100 to 500 ms of IndexedDB reads per
  run. Load once at worker start; write back only new entries. commands.md
  §7.9 step 2 says the same thing per run.
- **Long sessions:** known (live-execution.md §9.2, one instance per
  input).
- **Pooling allocator reservations** scale with workers and slot size.
  With a 64 MiB initial GC heap (runtime study), each slot reserves at
  least that in address space, and slot reset cost follows the pages a
  case touched. Use a small initial heap for unit tests.

### 5. Hidden Superlinearity

- **REPL name lookup.** [live-execution.md](live-execution.md) §2.2
  walks "input n-1's interface, then n-2's, ..., then input 1's". A name
  that resolves to `lib.hd` or std probes every earlier input. At 1,000
  inputs and about 20 such names per input, that is 20,000 probes, about
  1 to 2 ms per input, and quadratic over a session. Keep one session
  scope map from name to the newest input that declares it; shadowing is
  an overwrite.
- **Fold rounds:** known; the study's worklist fix makes it linear.
- **Witness fixpoint:** known; the study bounds it.
- **Closure lists in keys:** O(modules × folders) pairs hashed per run.
  Under 1 ms even at 50k lines. Fine.
- **Fix-it verification:** O(fix-its × file size), see the table.
- **M3's worklist:** each edge transfer costs a substitution, and a row
  can grow per pass; bounded by fuel and by the instantiation depth. The
  `pathological` suite should include a 200-function mutual-recursion
  group with 20 row keys.
- **Link renumbering:** known (the study's change 1).
- **`it_each`:** each row is a fresh instance, linear in rows. Fine.
- **Sorting by content keys** (diagnostics by rendered message, instances
  by key): O(n log n) over small n. Fine.

### 6. I/O And Syscalls

- **Per-file entries:** finding 1. **Warm reads:** finding 5.
- **The probe file.** [cache.md](cache.md) §5.5: `written_at` is "the
  mtime that the file system stamps on a probe file created at the start
  of the run". That is a create and an unlink (about 0.4 ms at the Mac's
  rates) on every run, including the no-edit fast path, and two
  concurrent runs in one worktree need distinct names. Create it only
  when the run is about to hash a file, which is the only time a new
  manifest record can be written.
- **Touch on hit.** Each hit older than an hour gets a `utimes`. After an
  idle hour, the first warm run writes about 200 entries' times (finding
  5's count) or about 10,000 for a build. With packs and the last-run
  record this drops to tens.
- **`startup` (20 ms, 10 MB).** The fast path is: read the manifest,
  walk 20 directories, stat 100 files (0.2 ms measured), open `pkgres`,
  print. On the Mac's rates that is 1 to 2 ms of I/O. The binary's page
  faults and relocations are the unknown; with wasmtime and Cranelift
  linked in, the binary is likely 30 to 60 MB. Measure resident pages on
  `hd --version` in slice 1, as build-order.md plans.
- **`hd.sum`.** [commands.md](commands.md) §7.1 step 1 verifies `hd.sum`
  each run; [`cli.cache.hash`](../../spec/cli/command-line.md) compares
  a recorded hash, so no dependency tree is re-hashed. Fine.
- **`depfiles`** is one entry per dependency version, once per machine.
  Fine.
- **No `fsync`.** Correct choice; it would add milliseconds per entry.

### 7. Incremental Cost Per Edit

| Edit | Recomputed today | Proportional to | After the fixes |
| --- | --- | --- | --- |
| private body edit | the module; reads about 200 entries; relinks every program that reaches it | module size + program size | the module; reads the edited entries; programs hit if the TIR is unchanged, else relink one test program |
| comment or whitespace edit | as above | module + program | the module only (finding 4) |
| `tests:` block edit | the module's check and test overlay; every program that reaches the module | program | the overlay and the module's test program |
| private header edit | the folder interface (reads every file of the folder), the module | folder + program | as today for the folder; reads stay local |
| public signature edit | every module whose closure holds the folder; their programs | program | modules that read the changed item (finding 3) |
| new file | discovery, folder graph, the folder's interface, folder dependents | folder + dependents | modules that read the new names |
| std upgrade | everything (`toolchain_key`) | program | the same; expected |

Every "proportional to program" cell is a finding above.

### 8. Determinism Cost

- **Content sorting** happens at the end: diagnostics per module and once
  at the end, instances once per program, interface rows once per blob.
  None is on a per-instruction path. Fine.
- **Remapping run IDs** costs about 10 ns per ID word: 2 ms per cold
  10k-line check, 0.2 ms per 1,000-line module. Fine.
- **Canonical hashing of TIR items** hashes referenced types by content,
  which walks each referenced type's structure. Memoize the content hash
  per pool item per run (one `Hash128` column parallel to the pool), or a
  large generic type is re-walked once per mention.
- **The determinism matrix** runs shuffled serial orders and ID shifts in
  CI, not on the user's path. Fine.
- **Print at the end** costs a cold run's time-to-first-diagnostic. For
  a cold check with an early error the agent waits the full run (up to
  the 1 s target). Accepted by the design; note it in `edit-latency`'s
  report.

### 9. Single Thread And Browser

- **Longest step:** finding 7.
- **Worker memory:** the compiler's linear memory never shrinks. A
  cold check of a large paste grows it to its peak (about 22 MB at 10k
  lines by §3.24), and it stays. Fine for the playground; record it in
  slice 5.
- **IndexedDB round trips:** load once per worker, not per run (above).
  Write-back after each run is one transaction; fine.
- **Single core natively.** The `concurrency` metric runs up to 64
  processes, so an agent often has one core. Every wall-time figure in
  the design is for 8 cores; slice 4 should report one-core numbers next
  to them (the compile study does this for `test-latency`).

### 10. Tail Latency

- **Big modules:** finding 8.
- **Big bodies:** one 9 KB body in `text.hd` is 4.6% of std's function
  body bytes. A body near the fuel limit (2,000,000 steps) can take an
  estimated 0.2 to 2 s. It runs alone in its batch, and LPT ordering
  starts it first, so it bounds the wall time of a cold check but not
  the throughput.
- **Deep generics:** `select` and layout memos make repeated instances
  cheap. The depth limit (32) bounds a chain. Fine.
- **GC pauses:** finding 10.
- **Eviction inside a user command:** bounded to 20 ms by design; the
  sharded scheme keeps it bounded and correct (finding 6).
- **Cranelift super-linearity in large functions:** known (the study's
  risk 3 and spike S6).

## Measurements To Add To The Slice Exit Tests

These are proposals for [build-order.md](build-order.md); this review
does not edit it.

| Slice | Measurement | Gate or report |
| --- | --- | --- |
| 1 | Body size distribution of std and `ordinary-10k`: bytes and tokens per body, p50, p90, p99, max (this review's numbers as the baseline) | report |
| 1 | Resident pages and wall time of `hd --version` with the release binary, wasmtime linked in | gate (`startup`) |
| 3 | Check throughput in tokens per µs per body size class, one thread | report |
| 3 | Lock acquisitions and contended waits per run for the interners and the global memo; memo hit cost at 1 and 8 threads | report; gate the 100 ns hit at 1 thread |
| 3 | Fix-it verification time on a 1,000-line file with 20 fix-its | report |
| 4 | Per-entry publish and read cost on both machines, by entry size (2 KiB, 64 KiB, 2 MB) | report; drives packing |
| 4 | Files opened, created and renamed per warm edit, by edit kind (the table in item 7) | gate: proportional to the edit (`io-per-check`) |
| 4 | Modules rechecked per public edit in `ordinary-10k`, and in a one-folder variant | report (finding 3) |
| 4 | One-core `edit-latency` p50 and p95, next to 8 cores | gate on both |
| 4 | Time from start to first printed diagnostic, against total time | report |
| 4 | Eviction: entries scanned and deleted per run, bytes over cap after a scripted day | gate (`cache-growth`) |
| 5 | Longest stepping-executor step, in ms, on a 1,000- and a 3,000-line program in Chrome; worker restarts during a scripted typing session | gate: no step over 50 ms on the 1,000-line program |
| 6 | `prog_key` hits after a comment edit, a `tests:` edit and a private body edit that keeps the TIR | gate: hit for the first two |
| 8 | `hd test` on `tests-1k` cold, and after an edit to a module that 50 modules use: CPU-s, link reads, Cranelift functions compiled, `cwasm` bytes written | report; feeds open question 1 |
| 8 | Concurrent `Precompile` peak RSS | report |
| 10 | GC collections, total pause and max pause per `runtime-suite` case; p99 of a request loop in `long-run-memory` | report; gate max pause once a budget is set |

## Open Questions For The Owner

Only decisions. Implementation choices (packs, the last-run record, the
per-body browser step, lock-free walks) are left to the lowering pass and
listed in the findings.

1. **Test program granularity.** Today each module with unit tests is
   its own program, so shared std and harness code is linked, compiled
   and stored once per module (finding 2). One program per package, with
   an init export per module, runs each case after exactly the init
   groups its module reaches, as today. It cuts cold Cranelift work and
   `cwasm` disk use by the module count (about 100 times on `tests-1k`).
   The cost: one edit relinks the whole package's test program (at most
   a few thousand functions, mostly cache hits), and a build error in
   any module's tests blocks the program. **Recommendation:** one unit
   and doc-test program per package, an init export per module, and
   integration tests unchanged (one program per file, as the spec's
   environments differ).
2. **Early cutoff by recorded reads.** A public edit today rechecks
   every module whose closure holds the edited folder (finding 3). A
   `check` entry that records the item hashes it read could be reused
   when none of them changed. That is a second incremental mechanism
   beside the per-module key, at module granularity, with no query
   system and no daemon. Its risk is an incomplete read list, which
   verify mode and the key-completeness tests check. **Recommendation:**
   adopt it after slice 4 measures how often public edits recheck
   modules that never read the edited item; keep today's keys as the
   coarse first check.
3. **Approximate LRU for the 10 GB cap.** Exact LRU needs a full scan,
   which cannot fit the 20 ms bound at millions of entries (finding 6).
   ccache cleans one shard at a time and evicts the oldest within it.
   **Recommendation:** accept per-shard LRU (256 shards). The cap holds
   within one shard's size, about 40 MB at 10 GB.
4. **Is the Mac a gate for I/O-bound targets?** [build-order.md](build-order.md)
   §22.2 makes the Apple Silicon machine "never a gate on its own". The
   per-file costs measured here are 5 to 20 times what a Linux runner
   likely pays, and the owner's agents run on the Mac. A design that
   passes on Linux can miss `edit-latency` on the Mac by the I/O alone.
   **Recommendation:** gate `edit-latency`, `io-per-check` and
   `test-latency` on both machines, with the Mac run on an idle machine.
