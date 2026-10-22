# New Compiler Design: Cache

Status: design record; may lag the code. The code and the spec win.

Part of the [compiler design](../../docs/compiler/ARCHITECTURE.md).

## 5. Cache

`hd_cache` holds the `CacheStore` trait, the memory, overlay and disk
stores, the entry framing and the key functions. `hd_driver` decides
what goes in each entry and when to look one up.

### 5.1 Where Things Live

| Data | Location | Shared | Why |
| --- | --- | --- | --- |
| compiled entries (§5.2) | `$HD_CACHE/obj/` | per user, across worktrees and packages | keys are content hashes, so worktrees on one commit share everything |
| fetched dependency trees | `$HD_CACHE/pkg/` | per user | [Cache](../../spec/cli/command-line.md#cache) |
| std sources | embedded in the `hd` binary | per binary | std's interfaces and checks are ordinary `obj/` entries under a toolchain key that holds std's hash |

`$HD_CACHE` resolves as [`cli.cache.directory`](../../spec/cli/command-line.md#r-cli.cache.directory)
says.

### 5.2 Entry Kinds

| Kind | Content | Written by | Read by |
| --- | --- | --- | --- |
| `iface` | a folder interface blob (§4.11), its anchored diagnostics, private and loop names, module uses; under `reads_key`, the folder's read list | `FolderIface` | dependents' keys, resolution, coherence, `hd doc` |
| `check` | one module and role, in sections: diagnostics with severities and fix-its, metadata with the module's content hash, the module's items, TIR (only when the module has no error), test registrations, the init summary, `dbg` references, frame names; a REPL input adds its bindings | `ModuleFinish` | output, `InitOrder`, D2, `hd check --tests`, `hd test` |
| `parse` | empty: its presence says a file's bytes parse with no diagnostic | `ModulePrep` | `ModulePrep`, before trusting a `check` hit |
| `graph` | stage-B header findings and the folder's use diagnostics per folder (`hdr_key`); one entry per package run for the overlap findings and every folder's init order, keyed by their part keys; one entry per package for the coherence rows (§5.3) | `HeaderCheck`; the last of `Coherence` and the `InitOrder`s to finish; `Coherence` for the rows | output, `PackageResult`, D2; `Coherence` |
| `depfiles` | the skims of every embedded std file | the first run of a toolchain | every later run (§5.5) |
| `codepack` | one folder group's emitted code (§13.8), keyed by the group's code keys | `Emit`, or `Collect` for a group it filled from older packs | `Collect` |
| `packhint` | each folder group's last four `codepack` keys, one entry per package | `Link` | `Collect`, when a group misses its pack |
| `link` | the program's Wasm, keyed by `prog_key`; the REPL prelude with its session exports | `Link` | D2, the REPL |
| `clpack` | the Cranelift machine code of every function of one module, by Cranelift's key, keyed by the module's bytes and the engine's compilation settings; a private entry (mode 0600, read only when the reader owns it alone) | `hd_run_wasmtime::code_cache`, after a compile that compiled a function | the next compile of the same module on wasmtime |

`EntryKind` also names `check-test`, `pkgres`, `cwasm` and `code`; no
code writes them.

**Sections, not files.** A file costs hundreds of µs to publish and tens
to over a hundred to open on macOS, more than the work inside most small
entries. So the unit of an entry is the unit of invalidation, never a
smaller one: a module's diagnostics, items and TIR are sections of one
`check` entry, all functions of `check_key(m)`. A reader takes only the
sections it needs.

- **Graph parts.** Coherence and init order are parts of one `graph`
  entry per package run, keyed by the sorted keys of their parts. A part
  whose result is empty stores nothing; its key in the list is enough.
  Header findings are the exception: one `graph` entry per folder, keyed
  by `hdr_key`. As built: the parts are coherence's (the coarse key over
  every built interface) and each built folder's init order
  (`init_key`). `PackageResult` computes their keys and looks up the
  one entry. A hit runs neither task. On a miss it adds both tasks, and
  the last part to finish writes the entry, unless a part is blocked or
  reports an initialization cycle. So a change to one part reruns every
  part: an interface change reruns each folder's init order too, and
  `Coherence` then checks again only the traits whose `coh_key` changed
  (§5.3). `Coherence` runs after `PackageResult`, so the key can hold
  both kinds of part.
- **Init order stays a part.** Measured on 2026-10-16 (#177; a generated
  package of 24 folders with a two-module init cycle each, 28 init
  parts, dev build, median of 20 warm runs): computing the parts' keys
  and reading the runs costs about 4.5 µs per folder, and running
  `InitOrder` instead about 8.4 µs. The earlier measurement, 7.1 µs
  cached against 1.0 µs, was of one entry per folder, gone since #249.

Planned, not built: a `reads` section for early cutoff (§5.3.1), fact
records for a program database (#181), a `pkgres` entry for a warm fast
path (§5.3), and a no-emit `check` entry without TIR (header flag
`HAS_TIR` clear) that a later build replaces under the same key.

### 5.3 Key Composition

Every key is `H(kind tag, fields...)` over canonical bytes, with
xxh3-128 (`hd_base::StableHasher`). The built keys:

```text
toolchain_key = H("tc", compiler id, target ("wasm32-gc"), layout version, "xxh3-128",
                  options: lowering mode and the std sources hash)
package key   = root: H("pkg", name), plus the traits of a non-default runtime profile;
                a dependency: H("pkg", its identity); a dependency's module check
                hashes both the root's key and its own
reads_key(F)  = H("iface-reads", toolchain_key, package key, folder path,
                  [(module path, role, api_text_hash) for each file of F], use roots hash)
iface_key(F)  = H("iface", reads_key(F), [(module path, answer) for each read of F's lowering])
check_key(m)  = H("check", toolchain_key, package key, module path, role, token_hash(m),
                  [(folder path, deep_hash) for each folder in closure(m)],
                  template text hashes, REPL library and earlier inputs)
parse_key(f)  = H("parse", toolchain_key, source_hash(f))
hdr_key(F)    = H("hdr", toolchain_key, package key, folder path,
                  H(own deep_hash, own heads_hash, reads_key(F), closure(F)'s names hashes),
                  [(folder path, deep_hash) for each folder in closure(F)], tuple-template text hashes)
coh_key(T)    = H("coh", toolchain_key, stable path of T, sorted head hashes of T's impls)
init_key(F)   = H("init", toolchain_key, folder path, [(module path, init summary hash)])
depfiles_key  = H("depfiles", toolchain_key, "std")
prog_key      = H("prog", toolchain_key, pipeline, profile, entry, [(module, content hash)])
code_key      = H("code", pipeline, profile, instance key, item TIR hash,
                  H(toolchain_key, the program's data layouts, callee representation summaries))
```

- **A module's content hash.** `prog_key` holds, per module, a hash of
  everything in its `check` entry that linking reads: each body's TIR
  hash, the init summary, the encoded items, and the test registrations
  without their positions. The items are there because the link reads
  declarations a body edit need not touch: a profile handle trait's
  methods, a boundary type's fields (#316).
- **Roles.** The role is `lib`, or `test` for the root package's modules
  in a test run; an entry module's role gets `+entry`, since its entry
  holds the entry-point findings. A module whose top level runs as its
  program's entry behavior in this run gets `+entry-behavior`: the goal
  decides it (`hd FILE` and `hd check FILE` run a script's body, a test
  run runs none), and the entry holds the init checked either way, so
  one command's entry never answers another's (#282).
- **Token hash and positions.** `check_key` hashes the module's token
  hash (each code line from its first to its last token, plus `##`
  comment lines; plain comments, blank lines and trailing whitespace
  left out), and the entry stores positions as (code line, offset). So
  diagnostics follow an edit that shifts lines, and a comment edit costs
  the skim and one parse. A comment can still change the parse (one
  between a `##` block and its item detaches it), so a `check` hit also
  needs the `parse` entry of the file's current bytes. This is better
  than the design's `source_hash(m)`, which rechecked a module on every
  comment or blank-line edit.
- **Dependency closures.** `closure(m)` is m's own folder plus every
  folder reachable from it through the folder graph's use edges. A
  role-`test` `check_key` uses `test_closure(m)`, which adds every folder
  that test code reaches, dev dependencies included. So each key lists
  every folder whose impls its check can see. A `tests:` block's uses,
  of the same package or another, make no folder edge
  (`module.cycle.test-code`), so they never widen `closure(m)`.
- **Interface keys hold what the headers read.** A folder's `deep_hash`
  folds its own `api_hash` and the deep hashes of the folders its
  interface mentions (§4.11.3). The design keyed `iface_key(F)` by that
  same set. That set is known only after the build, and it misses what F's
  lowering read without naming: a `pub use` chain's middle folder, a row
  alias's folder, a lookup that failed. So the built key holds the
  lowering's recorded reads (#248). `hd_resolve::Recorded` wraps the
  run's `World` and notes each module path asked about and each folder
  whose interface was opened. Binding F's uses reads quietly: a use's
  reads count only when a lookup during the lowering consults a name it
  binds, and are then read again, recorded. That read list is an `iface`
  entry keyed by `reads_key(F)`. On a run, `FolderIface(F)` reads the list, answers each
  read now, and looks up `iface_key(F)` over those answers. A miss builds
  F and rewrites the list when it changed.
- **A read's answer.** For a module path: its folder, whether a use may
  name it, whether it is test-only code, and, when its folder's
  interface was opened, that interface's deep hash and names hash (the
  hash of its private-name and loop-name sections). The names hash is
  there because a failed lookup reads those names: `private-import` and
  `re-export-loop` come from them. A folder that F's uses reach but its
  headers never consult, such as one F or a dependency imports only for
  bodies, is in no read. So an API edit there rebuilds no interface of
  F's, and a name removed there is still reported, by `HeaderCheck(F)`.
- **The use roots.** A use whose root fails (`dep.NAME` with no such
  dependency) reads nothing, so `reads_key` holds the hash of each
  module's `UseRoots`: the package root, the relative bases and the
  dependency names.
- **A REPL input keeps its closure.** Its session scope reads the
  earlier inputs outside the `World`, so its `iface_key` lists every
  other folder of its closure with its deep hash, as before #248, and it
  writes no read list.
- **What a recorded read costs.** A warm `FolderIface` opens two entries,
  the read list and the interface, where a closure key needs one. The
  list is written only when it changes.
- **Argument-owned impls need no extra hash.** A goal with an open trait
  argument reads impls owned only through a trait argument, from any
  folder in the asking context's closure
  ([crates/hd_check/DESIGN.md (trait solver) §3.2](../hd_check/DESIGN.md#32-owner-folders)).
  Such an impl's head is in its folder's `api_hash` and deep hash
  ([crates/hd_resolve/DESIGN.md §4.10](../hd_resolve/DESIGN.md#410-folder-interface-construction)),
  which the closure lists hold. So adding one rechecks exactly the
  contexts whose closure holds its folder.
- **What the closure list costs.** Any public API edit in a folder misses
  the key of every module whose closure holds it, even modules that never
  name the edited item. Early cutoff (§5.3.1) is the planned fix, and a
  planned `recheck-precision` metric would count these misses
  ([docs/compiler/testing-the-compiler.md §8.2](../../docs/compiler/testing-the-compiler.md#82-incremental-soundness)).
- **Code keys hold the callees' representation summaries.** A callee's
  summary picks the symbol its caller relocates to and whether the caller
  casts the result
  ([crates/hd_mono/DESIGN.md §13.8](../hd_mono/DESIGN.md#138-code-entries)).
  A change to it re-emits the callers, even when their own TIR is
  unchanged. Instance keys name types by path only, so the program's data
  layouts are in every code key: two programs that declare one path with
  different fields never share code. The design also put the interface,
  selected-impl, inline-summary, inlined-body and literal dependencies in
  the code key. Implementation differs: those have no field of their
  own, only what the instance key and TIR hash carry; not judged.
- **Coherence runs per changed trait.** `coh_key(T)` names one part of
  the `graph` entry, and `Coherence` runs as one task over the sorted list
  of traits whose `coh_key` changed. A head hash is `H(canonical head,
  rank)`: the impl's parameters, target and trait arguments, and its
  content rank `(package, module path, item index)`
  ([crates/hd_check/DESIGN.md (trait solver) §5.3](../hd_check/DESIGN.md#53-why-no-global-index)).
  The report names the later impl of a pair by rank, so swapping two
  overlapping impls must miss; the item index, not a byte offset, keeps
  the key across a body edit above an impl. As built:
  - **When it runs.** The `graph` entry's key holds the coarse key over
    every built interface, not the `coh_key`s: computing every `coh_key`
    would cost a warm run more than the one entry it opens. On a miss,
    `Coherence` computes each trait's `coh_key` and checks only the
    traits the package's coherence rows lack.
  - **The coherence rows** are one `graph` entry per package, keyed by
    `H("coh-rows", toolchain_key, package key)` and rewritten when
    `Coherence` hashes a folder or checks a trait. Per trait they hold
    the findings under its `coh_key`, each naming the reported impl by
    stable path. Each trait and folder keeps its newest row and one
    older one, so undoing an edit checks nothing.
  - **Head hashes per folder.** A head hash is
    `hd_resolve::Universe::head_hash`: the interface encoding of the
    impl records and the rank the check sorts them by, whose anchor
    stands in for the item index. The rows keep each folder's head
    hashes, one per trait it implements, under a key over its
    `heads_hash`, its impls' anchors and its REPL input numbers. So
    only an edited folder's impls are encoded again. One impl edit in
    the 24-folder package of "Init order stays a part": `Coherence`
    about 0.41 ms, against 0.90 ms checking every trait. A cold run
    encodes every impl: about 2.3 ms in all.
  - **Numeric families.** A trait's key also holds the head hashes of
    each family bound its heads expand over
    (`Universe::family_traits`), since the expansion reads that trait's
    impls.
- **The solver memo is never persisted** and is never part of a key. It
  lives for one run (crates/hd_check/DESIGN.md (trait solver) §7.1). A
  goal that reads the candidate directory holds its context's
  `ImplUniverseId`, a run ID computed from the closure that the key
  lists.
- **Header checks (stage B)** read interfaces only, no bodies, so
  `hdr_key` names interface hashes. A tuple template's member steps come
  from its body, so the template text hash of each user tuple-template
  module is added: its API text, its template method bodies, and every
  body whose signature a caller infers. A check that read other private items would have to widen the
  key.
- **Use diagnostics.** `HeaderCheck(F)` also reports what F's uses
  report when they name nothing: `unknown-import`, `private-import`,
  `unknown-module`, `ambiguous-import`, `test-only-use` and
  `cyclic-test-dependency` (`hd_resolve::use_diags`). They read every
  folder F's uses reach, which `iface_key` no longer lists, and F's own
  uses, which a body-only use edit changes without changing F's
  interface. So `hdr_key`'s own part adds `reads_key(F)` and the names
  hash of every closure folder. They are a second section of the `graph`
  entry, as anchors. No body waits for them: a module's `ModulePrep`
  notes whether a use of the module reported an error, which is what its
  `Body` and `ModuleFinish` ask. `re-export-loop` stays in the interface entry: it reads only F's
  own source, and is reported once per loop.
- **Template text hashes.** No interface carries a template body yet
  ([crates/hd_resolve/DESIGN.md §4.10](../hd_resolve/DESIGN.md#410-folder-interface-construction)),
  so a `check` key takes the template text hash of each user template
  module whose template the module may instantiate: every tuple
  template, and a derivation template when the module's text names
  `derive` or `Structure` and a name of its trait. A body edit outside
  the template, in a function whose signature is written, rechecks no
  other module.
- **Locations are not semantic.** `iface_key` ignores source positions,
  so a whitespace edit above a declaration keeps it. An `iface` entry
  therefore stores no absolute position: a span in it is a **token
  anchor**, a declaration's stable path, a token index and count among
  that declaration's tokens, and byte offsets inside the first and last
  token ([docs/compiler/data-structures.md §3.7](../../docs/compiler/data-structures.md#37-spans-and-files)).
  Whitespace and comments are not tokens, so an edit that keeps a
  declaration's API hash keeps every anchor into it.
- **What a key never contains:** absolute paths, the working directory,
  the `hd` binary's location, mtimes, user names, thread counts, run IDs,
  and environment variables that do not change the result. Output
  options (`--format`, `--max-errors`) are not in keys, since entries
  hold structured diagnostics and rendering happens at output.
- **What changing an input reaches.** The std sources, the target, the
  lowering mode and the entry layout version are in `toolchain_key`, so
  changing any of them misses every entry.
- **Hermetic by API.** A module's check receives its source and the
  interfaces its key names. It cannot reach the file system or another
  module's source, so it cannot read an input its key misses.

- **The toolchain.** The compiler id includes the build id, so the host
  profile table and the semantic limits (body fuel, resolution depth and
  the rest of §4.15), all compiled in, are covered without fields of
  their own. That is simpler than the design's separate hashes and
  equally complete while no flag sets a limit.
- **The semantic project configuration.** The root package key is
  `H("root", manifest_key)`, where `manifest_key` hashes what the
  manifest changes about checking and building: the package name, the
  resolved dependency bindings, the executables and their roles, the
  required toolchain version and the manifest's diagnostics.
  Implementation differs: the key holds the package name (and a runtime
  profile's traits) only; not judged, since dependency folders already
  enter keys through their deep hashes.

Planned, not built: a `fast_key` naming a whole-package result, so a
warm run with no edit reads one entry.

#### 5.3.1 Early Cutoff By Recorded Reads

Not built yet; see #49. The design: each `check` entry stores a read
list (per-item interface hashes it used, name lookups into other folders
including ones that found nothing, the impl tables it probed). On a
`check_key` miss with an unchanged source, toolchain, package and role,
the run compares every read with the current interfaces and reuses the
previous entry when all are equal. Verify mode (§5.6) would recheck
every reuse.

Interfaces already key on a coarser read list (§5.3, "Interface keys
hold what the headers read"): per folder opened, not per item, and as
part of the key, not as a check after a miss.

### 5.4 Entry Format And Atomic Publish

```text
obj/
  pack-v1/
    <32 hex>.pending          unpublished pack, exclusively leased by its writer
    <32 hex>.pack             immutable concatenated HDCE frames
    index                     sorted logical-key directory, atomically replaced
    lock                      stable publication/GC writer lock
  clpack/<2 hex>/<32 hex>      private native code, with the existing owner checks
  stats                       legacy/private shard ledger (§5.7)
```

- **Logical framing is unchanged.** Each range in a data pack is one complete
  HDCE frame: the same 64-byte header, 16-byte section rows, length-prefixed
  payloads and checksum as before. The driver still checks kind, key,
  toolchain and layout; `LAYOUT_VERSION` remains 19. Pack IDs and offsets are
  storage addresses, never semantic cache keys. The storage namespace and
  its `HDPACK01` magic version the container independently.
- **One pending batch, one pack.** `DeferredStore` retains a command's writes
  in memory, and its reads see those waiting entries. After reporting, it
  sends the pending batch to `CacheStore::put_many`. `DiskStore` writes one
  data pack; the default batch implementation preserves individual put order
  for memory/browser stores. Explicit publishes can create more than one
  pack in a command. A same-key update, including Graph coherence rows,
  replaces that key's index location; keys are not assumed insert-only.
- **Lease, then publish.** Under the stable writer lock, create a unique
  `.pending` file and take its exclusive file lock. Release the writer lock
  while writing the frame bytes. Reacquire it, reload the latest index and
  merge the pending locations and use marks. While still excluding GC,
  release the file lease, rename to `.pack`, then write a temporary index
  and rename it over `index`. Thus every indexed pack is complete and
  unlocked before a reader can observe it. No `fsync` is needed for derived
  data: a crash can lose an entry; invalid framing is a miss.
- **Readers take no publication or lease lock.** A command loads one index
  snapshot and reads only the addressed frame ranges. A process-local mutex
  protects that snapshot and cached file cursors; it is not shared with
  other processes. At most 64 open pack handles are cached. A concurrent
  replacement or eviction leaves an older reader whole frame bytes or a
  miss, never a partial publication.
- **Index encoding and limits.** The 32-byte little-endian header is magic
  `HDPACK01`, row count (`u32`), reserved zero flags (`u32`) and the 128-bit
  `StableHasher("cache-storage-index-v1", rows)` checksum. Sorted unique
  58-byte rows hold kind (`u16`), key (`u128`), pack ID (`u128`), offset,
  length and last-use Unix seconds (three `u64`s). The index is limited to
  64 MiB before allocation; every row's kind, ordering, arithmetic and
  checksum are checked. A frame read is limited to 64 MiB and to the opened
  pack's bounds. Decoded row storage is bounded by that index limit;
  compaction streams payload ranges instead of collecting a whole pack in
  memory. Oversized persistence fails with the normal cache warning while
  compilation continues.
- **Crash recovery and GC races.** GC uses the same writer lock and tries a
  pending/orphan pack's file lock before deleting it. It cannot observe a
  newly created but not yet leased file because creation and locking share
  that writer lock. A live lease is retained; process death releases it,
  so no age heuristic delays orphan recovery. A corrupt index reads as
  misses and is replaced under the writer lock on the next write/GC; active
  leases remain protected. Missing or truncated packs/ranges are misses.
  Recognized stale temporary indexes are reclaimable because their writer
  also holds the stable lock. Foreign filenames and symbolic links stay.
- **Write failures** warn once and disable further puts for that command;
  the build still succeeds. If GC cannot write, it reports the physical
  bytes it could not remove rather than pretending the packed cache is
  empty. Deduplication of compilation across processes is not coordinated.
- **Private native code is separate.** Clpack keeps its loose paths, owner,
  regular-file identity, single-link and mode-0600 checks. The new data
  index cannot name a Clpack entry. Ordinary loose records belong to the
  retired storage layout; GC retires them once `pack-v1` exists. Old
  binaries racing that cleanup still see ordinary cache misses.

**Planned logical code packs.** These are distinct from physical data packs.
Current producers write Parse, Check, Iface, Graph, Depfiles and Link; the
older Codepack/Packhint emit-and-collect plan below is not implemented.
Code is stored in one `codepack` per folder group, so a
build reads and publishes tens of files instead of one per instance.
- **The key** is the group's code keys in content order
  (`codepack_key`). Two programs that use the same instances of a
  folder share its pack.
- **An edit** changes one group's key. `Collect` then reads the packs
  the package's `packhint` lists for that folder, newest first, and
  takes the code of every instance whose code key they hold. Only the
  rest is emitted, and the group gets a new pack.
- **The hint** is the one entry a later run rewrites with other bytes
  (`packhint_key`: pipeline, profile, package). Each code key is checked
  on read, so a stale or lost hint only costs emission. It keeps four
  packs per folder, so an edit that is undone and made again still
  finds the undone state's code.
- **Cost.** A program whose instances of a folder differ from every
  cached pack's, such as a new package's first build, emits that
  folder's instances again: packs share whole groups only.

Planned, not built: precompiled modules cached across runs (`cwasm`),
which wasmtime's `unsafe` deserialization rules out today
(crates/hd_run_wasmtime/DESIGN.md §17.9). Machine code is cached function
by function instead, one `clpack` per module
(docs/compiler/engines-and-test-runner.md §18.2).

#### 5.4.1 Historical Packing Experiment (#751, 2026-10-22)

**Historical outcome.** Recommended immutable per-run packs with a shared
index (A) for the production follow-up measured in §5.4.2. The experiment
beat the loose-file control in store time on both workloads. That design-only
commit kept the production loose-file store; the temporary prototype
established useful headroom, not a production-ready format or a large-cache
result. The owner ruled out per-module packs (B):
`CacheStore` has no module provenance, and Iface/Graph/Link do not describe
module records. Loose files (C) are the control A must beat. This follows the
[measurement-first decision](https://github.com/HerringtonDarkholme/hd-lang/issues/751#issuecomment-6349274346).

**Method.** Base `f64b9a814da07e32ca8fa39a1e663c4be5d3e911`, macOS arm64,
release compiler, `hd build --release --jobs 1 --engine node`, separately on
`bench/study/json.hd` and `nbody.hd` copied into fixed scratch packages. Cold
means an empty `HD_CACHE`, including std; edit appends one unused function;
no-edit reuses that cache and leaves the edited file untouched. Each cohort
has 15 samples per program, state and binary, alternating program and binary
order. Child process wall and CPU use `monotonic_ns` and `wait4`. The first
cohort pairs unmodified and instrumented loose stores; the second pairs an
instrumented loose control and the instrumented packed prototype. All
instrumentation and prototype Rust changes were restored byte for byte and
are absent from this commit.

Timers aggregate call counts and elapsed wall time in memory and print once
at CLI exit. Store time sums the non-overlapping deferred get, deferred put,
deferred touch, deferred flush and disk settlement scopes. Nested disk
get/put, index load/merge and GC timings explain that total; they are not
added again. With one job this measures synchronous store work, including
queued-byte copies, flush and eviction before process exit. It is **not** a
syscall-only percentage or CPU attribution. Observer output and process
startup are outside those scopes. The old 7.6%/14.5% syscall shares from
`b4c5768e3` are not the current measurement.

**Current loose-store attribution.** Medians in ms; share is the median of
per-build store-wall/process-wall ratios, not a ratio of the two medians.

| Program/state | Plain process wall | Instrumented process wall | Store wall | Store share |
| --- | ---: | ---: | ---: | ---: |
| JSON cold | 425.510 | 414.889 | 37.148 | 8.66% |
| JSON edit | 229.395 | 235.818 | 6.374 | 2.85% |
| JSON no-edit | 58.110 | 56.959 | 4.947 | 8.16% |
| nbody cold | 254.523 | 234.333 | 35.417 | 15.13% |
| nbody edit | 55.333 | 53.870 | 6.514 | 11.85% |
| nbody no-edit | 30.684 | 30.188 | 4.742 | 16.03% |

Both programs have the following exact counts in all 15 samples. Each state
flushes once and settles once; neither calls touch. Deferred gets can hit
waiting entries, so disk get counts differ.

| State | Deferred get | Deferred put | Disk get | Disk put |
| --- | ---: | ---: | ---: | ---: |
| Cold | 138 | 103 | 98 | 103 |
| Edit | 141 | 6 | 140 | 6 |
| No-edit | 142 | 0 | 142 | 0 |

Cold JSON flush/settlement medians are 29.726/7.214 ms; nbody
27.989/6.977 ms. No-edit disk reads take 4.585/4.519 ms respectively.
These counts find no duplicate queued puts in these workloads. Moving writes
after diagnostics is already what `DeferredStore` does. JSON edit is below
the owner's roughly 5% threshold, but its no-edit state and both nbody warm
states exceed it, so the owner-authorized A experiment applies.

**Measured A.** One immutable pack contains the pending complete HDCE frames.
A shared sorted index maps `(kind,key)` to `(pack,offset,length,last-use)`;
it is loaded once for lookups. Each process caches opened pack handles and
reads individual frame ranges into the existing entry-sized `Arc<[u8]>`.
Flush materializes the deferred batch in both A and the loose control;
only A publishes it as one pack. Pack writes finish before the stable
index-writer lock is taken. Under that lock, publication reloads the latest
index, replaces same-key locations, writes a temporary index and renames it.
Readers take no publication lock. A cache race or missing pack remains a
miss. Native private Clpack entries keep their existing separate storage.

GC tracks recency per entry, never by pack age. The prototype scans physical
packs on settlement, merges hour-old use marks, evicts by entry age under a
small cap, and compacts live records when a pack is over 25% dead or the cap
requires reclamation. Compaction preserves the original entry ages. Its
index merge, directory scans and automatic GC all run inside the measured
process; none is moved to an untimed worker. Empty warm flushes write no
pack or index. Normal study builds do not reach the compaction threshold.

| Program/state | Loose CPU | A CPU | Loose process wall | A process wall | Loose store wall | A store wall |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| JSON cold | 384.233 | 359.753 | 404.912 | 375.550 | 31.396 | 2.853 |
| JSON edit | 212.840 | 210.997 | 218.616 | 216.673 | 6.052 | 2.206 |
| JSON no-edit | 51.634 | 50.203 | 54.725 | 54.455 | 4.330 | 1.548 |
| nbody cold | 221.047 | 192.513 | 239.834 | 202.737 | 31.352 | 3.692 |
| nbody edit | 49.435 | 45.218 | 56.078 | 51.148 | 6.130 | 2.086 |
| nbody no-edit | 25.446 | 22.523 | 30.071 | 25.858 | 4.327 | 1.435 |

A's nested index and maintenance wall totals per build, medians in ms:

| Program/state | Index load | Index merge | GC/settlement |
| --- | ---: | ---: | ---: |
| JSON cold | 0.130 | 0.238 | 0.229 |
| JSON edit | 0.108 | 0.311 | 0.124 |
| JSON no-edit | 0.071 | 0 | 0.169 |
| nbody cold | 0.056 | 0.237 | 0.155 |
| nbody edit | 0.104 | 0.312 | 0.131 |
| nbody no-edit | 0.077 | 0 | 0.168 |

There are three index loads per cold/edit build (lookup, publication merge,
GC), two per no-edit build, one publication merge per cold/edit build and
none per no-edit build. Index-load time includes missing-index probes.
Merge time includes lock acquisition, the nested reload and index writing;
GC time also includes its nested reload. These columns overlap.

**Files and bytes.** Both programs have 103 logical objects cold and 109
after the edit; the no-edit build adds nothing. Loose storage has 104/110
regular files including its 2,064-byte stats ledger. A has 3/4: one/two
packs, the index and the stable lock file. JSON cold uses 6,405,407 loose
bytes versus 6,409,325 packed bytes; edit 6,621,505 versus 6,625,771. nbody
uses 6,228,603 versus 6,232,521 cold and 6,268,001 versus 6,272,267 after
edit. These are file lengths, not allocated disk blocks. Packing reduces
file operations, not payload size: the index adds bytes.

**Validation and limits.** All 180 outputs in each cohort match their paired
binary byte for byte (360 builds total). Extracting A's indexed frame ranges
and validating with the actual `hd_cache::decode_entry` finds all 103 cold
and 109 edited entry bodies and all 425/447 section payloads literally
identical to the loose control, for both programs. HDCE layout remains 19;
header keys/toolchain hashes differ normally because compiler source affects
the build ID. There is no payload normalization. Two focused scratch tests
also pass: same-key Graph replacement, concurrent independent publishers,
truncated-pack misses, entry-level LRU, age-preserving compaction and a
5,000-byte physical cap. No production code or fixture changed; full suites
were not run for this design note.

Shared-machine scheduling is visible: cohort-two JSON cold wall spans
370–1,083 ms for loose and 353–1,295 ms for A. The small JSON edit/no-edit
whole-process differences are not a robust speed claim. Store attribution
and the cold/nbody results justify investigating A, not extrapolating a
universal percentage. Initial load was about 22 for attribution and 29 for
A/control, falling to about 20; no own builds/tests overlapped timing.

Before production adoption, measure index growth and many historical packs,
contention and crash recovery, age updates and compaction under pressure,
and Windows open-handle/deletion behavior. The prototype serializes GC and
compaction with index publication, which can lengthen writer lock holds.
Unindexed orphan packs younger than an hour are retained so GC cannot delete
an active writer's unpublished pack; a production lease/recovery protocol
must make explicit GC and cap accounting sound in that case. Index
validation, bounded memory, corruption recovery and write-failure warnings
also need production hardening. A separate storage namespace is required;
packing alone does not justify changing semantic keys or HDCE framing.

Raw evidence, under the isolated worktree's `target/`:
`muse-751-store-1792697209532455000/` (attribution) and
`muse-751-pack-compare-1792697543256597000/` (A/control, samples, summaries,
first-trial immutable cache snapshots, extracted frames and decoder reports).
Temporary observers and prototype builders are
`/private/tmp/muse-751-build-{instrumented,prototype}.py`; they restore the
three source files in `finally`. The production loose-store source is
unchanged. This note supersedes the issue's historical syscall estimate as
current evidence; it does not authorize a production layout switch.

#### 5.4.2 Production Packs (#792, 2026-10-22)

**Outcome.** The owner-approved A layout is now the ordinary disk store,
with the lease, validation, bounded reads and shared-cap protocols above.
The preceding experiment is historical; its one-hour orphan heuristic is
not the production recovery rule. Logical framing and keys are unchanged.

**Method.** Base `84db1696aabf783406e8dbdc4d03ad79caee92ce`, macOS arm64.
Frozen release compilers build the same fixed JSON and nbody packages with
`hd build --release --jobs 1 --engine node`. Each of two cohorts has 15
samples per program, state and binary, alternating program and binary
order. Cold includes std in an empty scratch cache; edit appends one unused
function; no-edit preserves the edited file and cache. One cohort uses
plain compilers for whole-process results; the other instruments both
versions for store attribution with the same non-overlapping scopes as
§5.4.1. The before compiler is the base's loose store, not the prototype.
All observer changes were restored byte for byte before validation.

Plain compiler medians, ms:

| Program/state | Loose CPU | Packed CPU | Loose process wall | Packed process wall |
| --- | ---: | ---: | ---: | ---: |
| JSON cold | 362.293 | 341.333 | 370.316 | 348.510 |
| JSON edit | 202.652 | 197.643 | 209.221 | 204.001 |
| JSON no-edit | 50.606 | 46.912 | 54.356 | 50.388 |
| nbody cold | 209.499 | 185.604 | 219.263 | 196.144 |
| nbody edit | 46.539 | 42.317 | 51.420 | 47.389 |
| nbody no-edit | 24.175 | 21.224 | 27.379 | 24.262 |

Instrumented store medians, ms; shares are medians of per-build
store-wall/process-wall ratios. These are synchronous wall scopes, not CPU
or syscall-only attribution. Nested timers are not added to store totals.

| Program/state | Loose store | Packed store | Loose share | Packed share | Packed index load | Packed merge | Packed GC |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| JSON cold | 31.042 | 2.959 | 8.06% | 0.83% | 0.056 | 0.361 | 0.184 |
| JSON edit | 5.573 | 2.119 | 2.65% | 1.04% | 0.113 | 0.433 | 0.165 |
| JSON no-edit | 4.174 | 1.464 | 7.05% | 2.74% | 0.083 | 0 | 0.215 |
| nbody cold | 29.347 | 3.074 | 13.45% | 1.42% | 0.060 | 0.369 | 0.186 |
| nbody edit | 5.441 | 2.083 | 10.45% | 4.20% | 0.113 | 0.396 | 0.159 |
| nbody no-edit | 3.890 | 1.385 | 13.58% | 5.38% | 0.079 | 0 | 0.187 |

In every sample, deferred get/put counts are 138/103 cold, 141/6 edit,
and 142/0 no-edit; disk get counts are 98, 140 and 142. Flush and settlement
each run once, with no explicit touch calls. Packed cold/edit flushes write
one batch and merge once; an empty no-edit batch writes no pack or index.
Index loads total three cold/edit (lookup, publication, GC), two no-edit;
GC runs once and normal study builds perform no compaction. Merge includes
lock acquisition, its nested index load and atomic save. GC includes its
own nested load, scans and recency work before process exit.

Shared-machine scheduling remains visible: plain JSON cold wall ranges
366.719–650.954 ms loose and 339.950–631.486 ms packed; instrumented ranges
are 361.902–1,207.429 and 349.750–1,032.942 ms. No own builds/tests overlap
timing; another lane's single-thread measurement was active, with load
falling from about 11.5 to 9.2. Small whole-process differences are not a
universal speed guarantee. RSS medians/ranges are retained in the raw data;
this change does not establish a memory reduction. Plain JSON cold system
CPU falls 37.857→16.348 ms, nbody cold 31.214→9.926 ms.

**Files and bytes.** Both programs retain 103 logical frames cold, 109 after
edit. Loose storage has 104/110 regular files including its stats ledger;
packed storage has 3/4 (one/two packs, index, zero-length stable lock).
No-edit adds no files. File lengths, not allocated blocks:

| Program/state | Loose bytes | Packed bytes |
| --- | ---: | ---: |
| JSON cold | 6,404,992 | 6,408,934 |
| JSON edit | 6,620,675 | 6,624,965 |
| nbody cold | 6,228,603 | 6,232,545 |
| nbody edit | 6,268,001 | 6,272,291 |

The index adds bytes; packing removes per-entry file operations.

**Growth and pressure.** A release probe creates 1,000 historical packs and
1,000 index rows through the real store. Fifteen alternating first-load and
same-key replacement probes measure median index load 0.079 ms
(0.070–0.107), publication merge 0.391 ms (0.313–0.523). Merge includes
locking, reload and save; bulk pack writing is outside it. Explicit GC to
16,384 bytes leaves 14,726 from 65,036 bytes in 56.463 ms; these single-entry
packs need deletion rather than compaction. A second probe retains those
1,000 historical entries, adds 100 entries in a shared pack and replaces
half. GC compacts 130,832→110,832 bytes under a 114,688-byte cap without
evicting any of the 1,100 live entries. One streaming compaction takes
0.786 ms inside 12.477 ms total GC, including index work and directory
scans. These growth/GC measurements are single-cache experiments, not a
claim about arbitrary large-cache contention.

**Validation.** All 360 paired measurement outputs and 45 existing Wasm
probe outputs match byte for byte, with no baseline failures or regressions.
The actual decoder validates extracted frame ranges in both cohorts: for
both programs, every cold/edit comparison matches 103/109 complete bodies
and 425/447 section metadata/payloads literally. HDCE layout remains 19;
only normal build-ID-derived header key/toolchain fields differ. No payload
normalization or fixture blessing is used.

Focused checks pass: 25 disk tests (plus the ignored subprocess helper
actually invoked by its parent crash test), five store tests, seven CLI GC
tests, 20 driver cache/interface/session tests, all 65 unchanged stage cases,
five native private-code-cache tests, and web compiler build/smoke. The 40
selected conformance cases pass on both engines in dev and optimized
pipelines: 26 runtime cases each, zero output/status differences; 61/61 dev
and 55/55 optimized test outcome matches. Rust formatting, default and
native host/CLI clippy, and Windows-MSVC `hd_cache` cross-clippy pass with
warnings denied. The open-handle regression has a Windows branch that
withholds delete sharing and verifies later reclamation; its Windows
runtime execution belongs to normal CI, not a claimed local Windows pass.
No full suites were run.

Raw evidence under the isolated worktree's `target/`:
`muse-792-plain-1792702188140307000/` and
`muse-792-timed-1792702217003936000/` (samples, summaries, immutable snapshots,
extracted frames and eight decoder comparisons), and
`muse-792-output-compare/` (45 existing probes). Growth/compaction logs are
`/private/tmp/muse-792-release-{growth,compaction}.log`; focused gate logs
are `/private/tmp/muse-792-*.log`. Temporary observers and measurement
scripts are `/private/tmp/muse-792-{build-timed,measure}.py`.

### 5.5 The Stat Manifest And Change Detection

Every run lists and reads every source file of the root package and
skims it; there is no stat manifest. Embedded std files are not read
from disk: their skims are one `depfiles` entry keyed by the toolchain,
computed once per machine.

Planned, not built: a stat manifest in `build/.hd/` of each package
(size, mtime, ctime and inode per file, trusted only when both times are
earlier than a file-system-stamped `written_at`, git's racy-clean rule),
so a run hashes only changed files; a last-run record in the same file
naming each folder's and module's keys from the previous run, so a miss
reads only what changed; `depfiles` entries for fetched dependencies.

### 5.6 Verify Mode

Planned, not built: `HD_CACHE_VERIFY=1` turning every cache hit into a
recompute and a byte comparison, with a mismatch an internal error that
names the entry kind and key.

### 5.7 Eviction And The Size Cap

The disk store caps `obj/` at 10 GiB by default, evicting
least-recently-used entries (`cli.cache.obj.cap`, `cli.cache.obj.evict`).

- **Entry recency.** A packed entry's last-use timestamp is refreshed when
  over an hour old and merged at publication/settlement, at most one index
  update per entry per hour. Compaction preserves every surviving entry's
  age; pack age never substitutes for entry recency. Eviction orders live
  entries by age, ties by kind and key, down to 90% when the live set exceeds
  its available cap. Dead records/orphan bytes alone do not evict a live set
  that already fits.
- **Physical accounting.** Final pack files, including dead/unindexed
  records, and the index count toward the compiled-data cap. Pending leased
  writes, the zero-length lock and the small legacy stats ledger are not
  committed entries, as with the old temporary publish files. Unindexed
  final packs and dead pending files are reclaimed by lease-aware GC.
- **Compaction.** A referenced pack over 25% dead, or needing space under
  the physical cap, is copied by live frame range into a new leased pack.
  Commit the new locations before removing the old file. An older reader
  can retain the old descriptor; Windows may postpone deletion until that
  descriptor closes, and later GC retries it. Index loading and directory
  scans are included in a command's settlement work.
- **Private/legacy accounting.** `obj/stats` still tracks 256 approximate
  shard sizes and a drift cursor. Its scans retire ordinary loose files;
  Clpack entries retain their existing per-shard mtime LRU. Automatic
  packed cleanup subtracts known legacy bytes from the shared cap. Explicit
  `hd cache gc` scans both namespaces and uses exact remaining legacy bytes
  as the packed budget; the two stores do not each get a fresh 10 GiB.
- **Automatic eviction.** Drop shares one soft 20 ms budget between legacy
  scanning and packed cleanup. The first shard/pack operation completes
  even when scanning exhausted the budget; subsequent commands continue
  reclamation. Scanning and a single large compaction can exceed that soft
  budget. Publication and compaction serialize writers; readers take no
  cross-process lock. `hd cache gc` has no time limit and prints physical
  bytes before/after, subject to outstanding readers preventing deletion.
- **The cap** comes from `HD_CACHE_MAX_SIZE` (bytes, with `K`, `M`, `G`
  suffixes). GC follows no links, removes only recognized managed names,
  and never touches fetched versions in `pkg/`.

### 5.8 The Browser `CacheStore`

```rust
pub trait CacheStore: Sync {
    fn get(&self, kind: EntryKind, key: &Hash128) -> Option<Arc<[u8]>>;  // synchronous
    fn put(&self, kind: EntryKind, key: &Hash128, bytes: &[u8]);
    fn touch(&self, kind: EntryKind, key: &Hash128);
    fn put_many(&self, entries: &[(EntryKind, Hash128, Arc<[u8]>)]) {
        for (kind, key, bytes) in entries { self.put(*kind, key, bytes); }
    }
}
pub struct MemoryStore { entries: Mutex<HashMap<(EntryKind, Hash128), Arc<[u8]>>>, new: Mutex<Vec<..>> }
pub struct OverlayStore<'a> { inner: &'a dyn CacheStore, memory: MemoryStore }  // a REPL session's
```

- The core never awaits storage. `hd_web` keeps one `MemoryStore` per
  worker session across compiles, so "Run" after "Check" reuses the
  check. It drops the store once it holds more than 64 MiB.
- Keys are the same as native. `toolchain_key` keeps a new compiler build
  from reading old entries.
- `OverlayStore` reads its own memory first, then the inner store, and
  writes only to memory, so a REPL session's per-input entries never
  reach the disk store.

Planned, not built: IndexedDB persistence. The JS host would load the
entries into the `MemoryStore` once when the worker starts, then drain
the new ones (`MemoryStore::drain_new`) after each run and write them
back in one transaction.

### 5.9 What Cache I/O Costs

On macOS a publish costs about 340 µs and an open and read 60 to 120 µs
(under load), and a Linux runner is likely 5 to 20 times cheaper. With
one file per entry, a warm one-function edit opens one `check` entry per
module, publishes the edited module's `check`, and on a build opens one
`codepack` per folder group (§5.4). The planned stat manifest (§5.5)
and early cutoff (§5.3.1) exist to cut those counts to a few files per
edit.

The first release rechecks a whole module on a body edit
(crates/hd_check/DESIGN.md (type checking) §1.7). Per-body reuse inside
the module's entry
([crates/hd_tir/DESIGN.md §4.13.1](../hd_tir/DESIGN.md#4131-task-structure-per-module))
is the next lever for a large module.
