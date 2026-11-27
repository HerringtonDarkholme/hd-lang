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
| `iface` | a unit interface blob (§4.11), its anchored diagnostics, private and loop names, module uses; under `reads_key`, the unit's read list | `UnitIface` | dependents' keys, resolution, coherence, `hd doc` |
| `check` | one module and role, in sections: diagnostics with severities and fix-its, metadata with the module's content hash, the module's items, TIR (only when the module has no error), test registrations, the init summary, `dbg` references, frame names; a REPL input adds its bindings | `ModuleFinish` | output, `InitOrder`, D2, `hd check --tests`, `hd test` |
| `parse` | empty: its presence says a file's bytes parse with no diagnostic | `ModulePrep` | `ModulePrep`, before trusting a `check` hit |
| `graph` | stage-B header findings and the unit's use diagnostics per unit (`hdr_key`); one entry per package run for the overlap findings and every unit's init order, keyed by their part keys; one entry per package for the coherence rows (§5.3) | `HeaderCheck`; the last of `Coherence` and the `InitOrder`s to finish; `Coherence` for the rows | output, `PackageResult`, D2; `Coherence` |
| `depfiles` | the skims of every embedded std file; per source set, its last-run record (stamp and skim per file) and, per check run kind, its module rows and unit rows; per run key, the stored package result and a caller's stored report (§5.5) | the first run of a toolchain; a run that changed the set's files; a check run that changed a row or missed its result | every later run (§5.5) |
| `link` | the program's Wasm, keyed by `prog_key`; the REPL prelude with its session exports | `Link` | D2, the REPL |
| `owned` | one body after the ownership passes, in the wire form's owned section, and its hash. A std body is keyed by the toolchain (std's hash), pipeline, build profile, the checked and verify settings and the checked body's TIR hash (#1423). A dev build's package body (#2028) and generic instance (#2106) add the root package's key and settings and the program's declarations, each non-std module's checked items hash and each unit's impl heads, so a body edit that infers the same signature keeps them; an instance adds its instance key. Their entry lists the bodies the passes read, each by path and TIR hash, checked on load | `Collect`, after the passes on a miss | `Collect`, before running them |
| `clpack` | the Cranelift machine code of every function of one module, by Cranelift's key, keyed by the module's bytes and the engine's compilation settings; a private entry (mode 0600, read only when the reader owns it alone) | `hd_run_wasmtime::code_cache`, after a compile that compiled a function | the next compile of the same module on wasmtime |

`EntryKind` names only the kinds above; a kind's discriminant is part of
the entry layout. The per-group code cache left with the Wasm GC backend
(`dc358a56e`); `Link` emits the whole program on every miss of its `link`
entry. Planned, not built: code packs again (§5.4, #49 point 4).

**Sections, not files.** A file costs hundreds of µs to publish and tens
to over a hundred to open on macOS, more than the work inside most small
entries. So the unit of an entry is the unit of invalidation, never a
smaller one: a module's diagnostics, items and TIR are sections of one
`check` entry, all functions of `check_key(m)`. A reader takes only the
sections it needs.

- **Graph parts.** Coherence and init order are parts of one `graph`
  entry per package run, keyed by the sorted keys of their parts. A part
  whose result is empty stores nothing; its key in the list is enough.
  Header findings are the exception: one `graph` entry per unit, keyed
  by `hdr_key`. As built: the parts are coherence's (the coarse key over
  every built interface) and each built unit's init order
  (`init_key`). `PackageResult` computes their keys and looks up the
  one entry. A hit runs neither task. On a miss it adds both tasks, and
  the last part to finish writes the entry, unless a part is blocked or
  reports an initialization cycle. So a change to one part reruns every
  part: an interface change reruns each unit's init order too, and
  `Coherence` then checks again only the traits whose `coh_key` changed
  (§5.3). `Coherence` runs after `PackageResult`, so the key can hold
  both kinds of part.
- **Init order stays a part.** Measured on 2026-10-16 (#177; a generated
  package of 24 units with a two-module init cycle each, 28 init
  parts, dev build, median of 20 warm runs): computing the parts' keys
  and reading the runs costs about 4.5 µs per unit, and running
  `InitOrder` instead about 8.4 µs. The earlier measurement, 7.1 µs
  cached against 1.0 µs, was of one entry per unit, gone since #249.

Planned, not built: a `reads` section for early cutoff (§5.3.1), a
`pkgres` entry for a warm fast path (§5.3), and a no-emit `check` entry without TIR (header flag
`HAS_TIR` clear) that a later build replaces under the same key.

### 5.3 Key Composition

Every key is `H(kind tag, fields...)` over canonical bytes, with
xxh3-128 (`hd_base::StableHasher`). The built keys:

```text
toolchain_key = H("tc", compiler id, target ("wasm32"), layout version, "xxh3-128",
                  options: lowering mode and the std sources hash)
package key   = root: H("pkg", name), plus the traits of a non-default runtime profile;
                a dependency: H("pkg", its identity); a dependency's module check
                hashes both the root's key and its own
reads_key(F)  = H("iface-reads", toolchain_key, package key, unit path,
                  [(module path, role, api_text_hash) for each file of F], use roots hash)
iface_key(F)  = H("iface", reads_key(F), [(module path, answer) for each read of F's lowering])
check_key(m)  = H("check", toolchain_key, package key, module path, role, token_hash(m),
                  [(unit path, deep_hash) for each unit in closure(m)],
                  template text hashes, REPL library and earlier inputs)
parse_key(f)  = H("parse", toolchain_key, source_hash(f))
hdr_key(F)    = H("hdr", toolchain_key, package key, unit path,
                  H(own deep_hash, own heads_hash, reads_key(F), closure(F)'s names hashes),
                  [(unit path, deep_hash) for each unit in closure(F)], tuple-template text hashes)
coh_key(T)    = H("coh", toolchain_key, stable path of T, sorted head hashes of T's impls)
init_key(F)   = H("init", toolchain_key, unit path, [(module path, init summary hash)])
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
  needs the `parse` entry of the file's current bytes. A module without
  one is checked with its broken items left out (`grammar.recovery.item`),
  and its `check`, `iface` and header entries are neither read nor
  stored, so a warm run reports what a clean one does (#310). This is better
  than the design's `source_hash(m)`, which rechecked a module on every
  comment or blank-line edit.
- **Dependency closures.** `closure(m)` is m's own unit (a strongly
  connected group of modules, `crates/hd_resolve/DESIGN.md` §4.8) plus every
  unit reachable from it through the module graph's dependency edges,
  walked on demand. A
  role-`test` `check_key` uses `test_closure(m)`, which adds every unit
  that test code reaches, dev dependencies included. So each key lists
  every unit whose impls its check can see. A `tests:` block's uses,
  of the same package or another, make no unit edge
  (`module.cycle.test-code`), so they never widen `closure(m)`.
- **Interface keys hold what the headers read.** A unit's `deep_hash`
  folds its own `api_hash` and the deep hashes of the units its
  interface mentions (§4.11.3). The design keyed `iface_key(F)` by that
  same set. That set is known only after the build, and it misses what F's
  lowering read without naming: a `pub use` chain's middle unit, a row
  alias's unit, a lookup that failed. So the built key holds the
  lowering's recorded reads (#248). `hd_resolve::Recorded` wraps the
  run's `World` and notes each module path asked about and each unit
  whose interface was opened. Binding F's uses reads quietly: a use's
  reads count only when a lookup during the lowering consults a name it
  binds, and are then read again, recorded. That read list is an `iface`
  entry keyed by `reads_key(F)`. On a run, `UnitIface(F)` reads the list, answers each
  read now, and looks up `iface_key(F)` over those answers. A miss builds
  F and rewrites the list when it changed.
- **A read's answer.** For a module path: its unit, whether a use may
  name it, whether it is test-only code, and, when its unit's
  interface was opened, that interface's deep hash and names hash (the
  hash of its private-name and loop-name sections). The names hash is
  there because a failed lookup reads those names: `private-import` and
  `re-export-loop` come from them. A unit that F's uses reach but its
  headers never consult, such as one F or a dependency imports only for
  bodies, is in no read. So an API edit there rebuilds no interface of
  F's, and a name removed there is still reported, by `HeaderCheck(F)`.
- **The use roots.** A use whose root fails (`dep.NAME` with no such
  dependency) reads nothing, so `reads_key` holds the hash of each
  module's `UseRoots`: the package root, the relative bases and the
  dependency names.
- **A REPL input keeps its closure.** Its session scope reads the
  earlier inputs outside the `World`, so its `iface_key` lists every
  other unit of its closure with its deep hash, as before #248, and it
  writes no read list.
- **What a recorded read costs.** A warm `UnitIface` opens two entries,
  the read list and the interface, where a closure key needs one. The
  list is written only when it changes.
- **Argument-owned impls need no extra hash.** A goal with an open trait
  argument reads impls owned only through a trait argument, from any
  unit in the asking context's closure
  ([crates/hd_check/DESIGN.md (trait solver) §3.2](../hd_check/DESIGN.md#32-owner-units)).
  Such an impl's head is in its unit's `api_hash` and deep hash
  ([crates/hd_resolve/DESIGN.md §4.10](../hd_resolve/DESIGN.md#410-unit-interface-construction)),
  which the closure lists hold. So adding one rechecks exactly the
  contexts whose closure holds its unit.
- **What the closure list costs.** Any public API edit in a unit misses
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
    `Coherence` hashes a unit or checks a trait. Per trait they hold
    the findings under its `coh_key`, each naming the reported impl by
    stable path. Each trait and unit keeps its newest row and one
    older one, so undoing an edit checks nothing.
  - **Head hashes per unit.** A head hash is
    `hd_resolve::Universe::head_hash`: the interface encoding of the
    impl records and the rank the check sorts them by, whose anchor
    stands in for the item index. The rows keep each unit's head
    hashes, one per trait it implements, under a key over its
    `heads_hash`, its impls' anchors and its REPL input numbers. So
    only an edited unit's impls are encoded again. One impl edit in
    the 24-unit package of "Init order stays a part": `Coherence`
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
  unit F's uses reach, which `iface_key` no longer lists, and F's own
  uses, which a body-only use edit changes without changing F's
  interface. So `hdr_key`'s own part adds `reads_key(F)` and the names
  hash of every closure unit. They are a second section of the `graph`
  entry, as anchors. No body waits for them: a module's `ModulePrep`
  notes whether a use of the module reported an error, which is what its
  `Body` and `ModuleFinish` ask. `re-export-loop` stays in the interface entry: it reads only F's
  own source, and is reported once per loop.
- **Template text hashes.** No interface carries a template body yet
  ([crates/hd_resolve/DESIGN.md §4.10](../hd_resolve/DESIGN.md#410-unit-interface-construction)),
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
  profile's traits) only; not judged, since dependency units already
  enter keys through their deep hashes.

A warm `hd check`, `hd lint`, `hd build`, `hd run`, `hd FILE` or
`hd test`'s build with no edit reads one entry: its stored package
result, or the CLI's stored report, artifact or build half (§5.5,
#1510).

#### 5.3.1 Early Cutoff By Recorded Reads

Not built yet; see #49. The design: each `check` entry stores a read
list (per-item interface hashes it used, name lookups into other units
including ones that found nothing, the impl tables it probed). On a
`check_key` miss with an unchanged source, toolchain, package and role,
the run compares every read with the current interfaces and reuses the
previous entry when all are equal. Verify mode (§5.6) would recheck
every reuse.

Interfaces already key on a coarser read list (§5.3, "Interface keys
hold what the headers read"): per unit opened, not per item, and as
part of the key, not as a check after a miss.

### 5.4 Entry Format And Atomic Publish

```text
obj/
  pack-v3/
    <32 hex>.new              a lease marker being created, before its lock
    <32 hex>.pending          zero-byte marker, exclusively leased by its writer
    <32 hex>.data             unpublished payload, written without locking its data
    <32 hex>.pack             immutable HDCE frames, then a footer of their keys
    index/<1 hex>.lock        writer exclusion, separate from readable data
    index/<1 hex>.log         one of 16 index shards, by a key's top four bits (#1932):
                              a header (fold epoch, head copy, running total)
                              and appended record batches
    index/<1 hex>/            the shard's folded pages
      head                    linear hash level, split pointer, rows, fold epoch
      <16 hex>                one bucket: a page of sorted rows stamped with
                              the fold epoch that wrote it, replaced whole
      packs                   the pack table: the packs whose home is this shard
  clpack/<2 hex>/<32 hex>      private native code, with the existing owner checks
  stats                       legacy/private shard ledger (§5.7)
  gc                          the collection lock and the last collection's time and cap
```

- **Logical framing is unchanged.** Each range in a data pack is one complete
  HDCE frame: the same 64-byte header, 16-byte section rows, length-prefixed
  payloads and checksum as before. The driver still checks kind, key,
  toolchain and layout; packs did not change `LAYOUT_VERSION`. Pack IDs and offsets are
  storage addresses, never semantic cache keys. The storage namespace
  (`pack-v3`) versions the container independently.
- **One pending batch, one pack.** `DeferredStore` retains a command's writes
  in memory, and its reads see those waiting entries. After reporting, it
  sends the pending batch to `CacheStore::put_many`. `DiskStore` writes one
  data pack; the default batch implementation preserves individual put order
  for memory/browser stores. Explicit publishes can create more than one
  pack in a command. A same-key update, including Graph coherence rows,
  replaces that key's index location; keys are not assumed insert-only.
- **Lease, then publish, shard by shard (#1932).** Each new pack has its
  own lease marker. Create a zero-byte `<id>.new` marker, lock it and
  rename it `.pending` before creating the separate `<id>.data` payload.
  A free or absent marker proves its writer ended. Write the frame bytes
  and footer without locking the payload. The index is 16 shards by a
  key's top four bits. Each has a readable `index/<x>.log`, writer exclusion
  on the separate `index/<x>.lock`, and folded pages under `index/<x>/`.
  The pack's ID carries its home shard, that of its first key. Commit
  the home shard first: under its lock, grow its total by the pack's bytes,
  rename `.data` to `.pack`, then append one framed record batch. Commit
  the other shards in order, one lock and one append at a time. Release,
  close and remove the marker after the last commit. Readers can read any
  visible pack while its writer continues indexing other shards. GC keeps
  a leased pack whole: it cannot delete or compact its unindexed frames.
  Free orphan markers and payloads are reclaimed; a crash before locking a
  `.new` marker uses the existing age guard. No `fsync` is needed for
  derived data: a crash can lose an entry; invalid framing is a miss.
  Namespace `pack-v3` isolates this protocol from version 2, which locked
  readable logs and payloads. Windows enforces those locks against readers;
  Unix treats them as advisory. Each changed shard opens one additional
  zero-byte lock file, and each published pack opens one additional marker.
  Unchanged readers add no work. Older namespaces' entries are misses,
  and GC leaves their directories alone: the build ID in every key makes
  them unreadable, and `hd clean --cache` removes them.
- **Logs and folds (#1932).** A batch is framed: its op count and a
  checksum of its 59-byte ops (set a row, remove a row, track or untrack a
  pack). A torn or failing batch ends the log; readers skip it and the
  next writer cuts it. Ops are absolute, so replaying ones a fold already
  applied changes nothing. Once a shard's records pass 4,096 bytes (about
  one page), the writer that crosses it folds them under the lock: it
  applies them to the pages with the split and merge below, writes the
  changed pages, table and head stamped with the next epoch, writes a
  header naming that epoch and head, and then shortens the log, so a log
  never shows an old epoch without the records its fold applied (#2054).
  A collection folds every log. A reader reads a shard's log once, header
  and all, instead of a head; a change in it wins over a page no later
  fold wrote, and a page a later fold wrote already holds the change. A
  page newer than the reader's log, or gone, that lacks the key is
  checked as a seqlock reader checks: the log is read again, and an
  unchanged epoch means the fold is still writing, so the log's answer
  stands; a changed one means a fold committed, and the read starts over. A torn header is rebuilt by
  the next writer from the head file, with the total unknown.
- **The paged index (#1502)** is a linear hash table. Bucket `b` holds the
  rows whose key's low `i` bits are `b`, or low `i + 1` bits once `b` is
  below the split pointer `s`; there are `2^i + s` buckets. A row added
  past 35 rows per bucket on average splits bucket `s` into `s` and
  `s + 2^i`; eviction merges the last bucket back under half that, and an
  index with no rows has no files. Each bucket is one page file of 70 row
  slots (4,092 bytes), whole pages when a bucket overflows.
- **A split stays readable.** It writes the old bucket with its moved rows
  still in it and the new bucket, then the head, then the old bucket
  without them. A reader or a crash at any point finds every row in the
  bucket the stored head names; a writer drops rows a page holds for
  another bucket. A merge writes the merged bucket, then the head, then
  removes the last page.
- **A lock ends at its unlock, not its close (#2055).** Every file this
  crate locks is a `LockFile`, whose drop unlocks before it closes; std's
  file lock methods are disallowed elsewhere (`clippy.toml`). A lock
  belongs to the open file, and a child any thread spawns shares that
  file from its fork to its exec, so a lock released by its close alone
  stays held for that moment: a peer's try-lock fails, and a free claim
  or lease looks live.
- **Readers take no publication or lease lock.** A command reads each
  shard's log once and each page it needs once, and reads only the addressed frame
  ranges. A process-local mutex protects those pages and cached file
  cursors; it is not shared with other processes. At most 64 open pack
  handles are cached. A concurrent replacement or eviction leaves an older
  reader whole frame bytes or a miss, never a partial publication. A merge
  or GC deletes a pack only after committing an index that no longer names
  it, so a pack a reader's pages name but it cannot find means another
  process committed since: the reader reads the head and its pages again,
  once per vanished pack, as git rescans its packs when one disappears.
  A pack, once renamed into place, is removed only by GC under the lock,
  also when the commit that follows fails.
- **Each entry computed once (#1932).** A store claims only when its
  caller publishes a claimed entry as soon as it puts it
  (`DiskStore::with_claims`, which the CLI's store sets; `DeferredStore`
  does so through `CacheStore::wanted`). Every store takes a shared lock
  on the `pack-v3/run` regular file at its first read. The `run-lock` file
  serializes joining and probing membership, for a constant number of lock
  calls only. A probe explicitly releases its own shared lock, tries an
  exclusive lock, releases it if granted, and restores its shared lock.
  This uses neither directory locking nor lock conversion: Windows stacks
  overlapping locks on one handle. The gate keeps another join or probe
  out of that temporary membership gap. A successful probe runs alone
  and claims nothing, since a claim costs a file: it notes the miss in
  memory, and its first such probe takes the `solo` regular file's lock
  exclusively, inside the gate (#2056). A run whose probe finds another
  member waits for `solo` before its first claim; it holds no claim, so
  the wait closes no cycle. The lone run probes again at its next miss
  and at the put of a noted miss. Finding the other run, it claims each
  noted miss not yet published, a put one as computed so that it is
  published at once, and lets `solo` go. The joiner then waits on those
  claims as on any other, so no entry a lone run computes is computed
  twice, and a joiner waits at most one entry's computation per lone
  run. A lone run whose noted misses are all published or abandoned
  lets `solo` go at once, and takes it again at its next lone miss: a
  parent that publishes its build before it starts a child never makes
  the child wait. Cost alone: one `solo` lock per batch of lone misses,
  one probe per miss and per put of a miss, no claim file; the claims
  are made once, and only when another run joins.
- **No wait outlasts its deadline (#2056).** A lone run blocked on the
  run that joined it, as `hd test` is on the test child it starts, never
  misses again, so it never claims or lets `solo` go; a claim holder
  blocked on its waiter never publishes. So every cross-process wait of
  the claim protocol, for `solo` and for a claim, polls its lock with
  SQLite's busy-handler sleeps (1, 2, 5, 10, 15, 20, 25, 25, 25, 50, 50,
  then 100 ms; `sqliteDefaultBusyCallback`) for at most 1 s. A holder
  that keeps its lock that long shows no progress: the waiter goes on,
  computes the entry itself (a bounded duplicate, never a hang), and
  appends a line to `pack-v3/stalls`, which starts over past 64 KiB.
  A store that stalled waits for no run again, so a run waits out at most
  one deadline. Price: waiting longer than computing the entry only pays
  when the holder is about to finish, so the deadline is the largest
  entry's time with headroom for a loaded machine. A whole cold
  `hd check` of a std-using program takes 65 ms, of the race programs
  0.3 s, and one entry is a part of that. 1 s is 19 polls; a free lock is
  seen within 100 ms of release.
  Beside other runs, a miss of
  `parse`, `iface`, `check`, `check-test` or `graph` locks
  `claims/<kind><key>`.
  - If the lock is free, the store reads the key's shard again: its
    snapshot may predate another run's publication. If the entry is still
    missing, the run computes it. At its put, `DeferredStore` publishes it
    with everything put before it, in order (an interface before its read
    list), and the claim goes. A claim is held only while its entry is
    computed.
  - If another run holds it, this run polls the lock within its deadline,
    reads the index again and hits. If the holder died (the system
    releases its lock) or published nothing, it computes under the claim
    it now holds; past the deadline it computes without one.
  - A computation that produces no cacheable entry calls
    `CacheStore::abandon` before another query. A `parse` entry certifies a
    clean parse, so a rejected source releases its claim without writing
    that marker. The disk store removes only the addressed unfinished
    claim, O(log claims); a completed deferred put stays claimed until its
    publication. Overlay, deferred and counting stores forward the call;
    a single-flight run cancels only its own flight and wakes its waiters.
  - A contended reader announces its wait before checking its own
    completed claims. A concurrent completion therefore either sees the
    announcement and publishes, or is found by the reader's flush check.
    That check's request to publish and read again belongs to the reader's
    thread (#2053): another worker's put cannot take it, or the reader
    would take its miss as its own and compute beside the holder.
    The store records the wait-file length before acquiring its first
    claim. Any later announcement keeps publication eager until this
    store ends: publishing an unrelated entry cannot consume the interest
    of a reader waiting for an unfinished entry. Historical announcements
    do not force a later command to publish eagerly.
    This follows the lost-wakeup discipline of
    [Linux futex compare-and-block](https://man7.org/linux/man-pages/man2/futex.2.html),
    using portable file locks and durable interest instead of shared memory.
    Cost: one wait-file open and length read before the first claimed
    miss; ordinary hits and solitary runs add no work. A contended wait
    appends one byte before its flush check. After contention, each put
    may publish its current batch until the command ends.
  - A store that finds other runs beside it after running alone publishes
    what it put while alone at its next put or miss.
  - Runs of different programs wait only for keys both miss, such as
    std's: keys are content hashes.
  - `depfiles` is not claimed, except the std skims. A run writes its
    source records and package results as it ends, so a claim on one
    would make other programs wait for that whole command; runs that start
    together each compute those. The std skims entry is claimed through
    `CacheStore::get_claimed`: its holder skims every std file at once and
    puts the entry, and the runs waiting for it read it rather than skim
    std too. A run alone skims each std file as it reads it and records
    the entry as it ends.
- **Why claims cannot deadlock.** A claim is held from a miss to the put of
  the same entry. Every place `hd_driver` misses a claimed kind, and what
  it reads while it holds the claim (`crates/hd_driver/src/lib.rs`):
  - `parses` (`parse`): it parses one module's text and reads no entry.
  - `unit_iface` (`iface`, the reads record then the interface): it
    returns blocked until every unit of its closure has an interface in
    memory. It resolves against those, reading no other entry.
  - `header_check` (`graph`, a unit's findings): blocked until its own
    and its closure's interfaces and template sources are in memory; then
    it reads no entry.
  - `StdSkims::read` (`depfiles`, the std skims): it skims the embedded
    std texts and reads no entry.
  - `module_prep` (`check`): `check_key` needs every dependency in
    memory. Its `parse` and `lint` reads come before the `check` miss.
    Preparation, bodies and the put execute in that one scheduler task.
    REPL input tasks depend on earlier inputs and the library's complete
    check. Bodies within a module fork and join on the pool, and a join
    lets its worker run another task, such as another module's
    `module_prep`, while this one is suspended under it.
  - `graph_hit` (`graph`, the package's entry): after every interface and
    check. The Coherence part it starts reads the package's coherence rows
    (`graph`), from which the entry is built: a dependency edge. Reading
    those rows takes no other entry, and the InitOrder parts read summaries
    in memory.

  Each of these misses returns a guard (`Run::lookup_owed`) that
  abandons the claim when it drops, so a computation that stops before
  its put, on an internal stop or a key it no longer puts, ends its claim
  on that return (#2021). A command's deferred store that publishes
  after a contested miss reads the key again; when that read hits, the
  entry another run published meanwhile stands, no put follows, and the
  store ends the first read's claim there (#2037). The package graph's claim passes to its part
  tasks, and the last part ends it, with or without the put.
  `DeferredStore::abandon` keeps the claim of an entry that is put and
  waits for publication.

  So a holder waits for no claim, except a package graph holder for its
  coherence rows, whose holder waits for nothing. Every wait follows a
  dependency edge, and no wait forms a cycle; no order of process IDs is
  needed. A cycle through something outside the cache, such as a parent
  that waits for the child process it started, is what the wait deadline
  ends (#2056). A store never waits for a claim it holds itself, and a store's
  threads never wait for each other. The driver runs each scheduler task
  inside `hd_cache::task`, and a claim records the thread's task depth.
  A task the pool runs inside another's join, while that suspended task
  holds a claim of a smaller depth, does not wait for another run's claim:
  it computes the entry, which another run may compute too (#2018). Waiting
  there could close a cycle, since the suspended task resumes only after
  this one returns. Before a thread waits, a debug build checks that the
  only unfinished claim of its own depth or deeper is a package `graph`; a
  release build lets any other such claim go.
- **Cost per run.** A run that reads `k` keys reads at most `k` shard
  logs (a 112-byte header and at most about 4 KiB of records each) and at
  most `k` pages; one that writes `j` entries or marks `j` hits appends one
  batch to each shard they fall in, 59 bytes per change, and folds a log
  that passes 4,096 bytes; a warm run whose hits are all fresh writes
  nothing. None of it depends on the number of entries.
- **Encodings and limits.** Pages and the pack table share a 32-byte
  little-endian header: magic (`HDPAGE02`, `HDPKTB01`), row count (`u32`),
  the page's fold epoch or the table's zero flags (`u32`) and the 128-bit
  `StableHasher("cache-storage-index-v1", rows)` checksum. Sorted unique
  58-byte page rows hold kind (`u16`), key (`u128`), pack ID (`u128`),
  offset, length and last-use Unix seconds (three `u64`s); table rows hold
  pack ID and length. The 64-byte head is magic `HDHEAD01`, level (`u32`),
  zero flags, split pointer and row count (`u64`s), a 128-bit
  fold epoch and a checksum of the rest. A pack's footer holds 34-byte rows
  (kind, key, offset, length), then the row count, their checksum and magic
  `HDFOOT01`; readers never read it. Every file is limited to 64 MiB before
  allocation; every row's kind, ordering, arithmetic and checksum are
  checked. A frame read is limited to 64 MiB and to the opened pack's
  bounds; compaction streams payload ranges instead of collecting a whole
  pack in memory. Oversized persistence fails with the normal cache warning
  while compilation continues.
- **Crash recovery and GC races.** The collection walks with no lock,
  then deletes a pending file only when it can take its lock. A lease is
  locked before it is named `.pending`; a `.new` file an hour old is a
  crash between creating and locking one. A live lease is retained; process death releases it,
  so no age heuristic delays orphan recovery. A corrupt page or head reads
  as misses and is replaced under the writer lock on the next write/GC (a
  corrupt head starts an empty index); active leases remain protected.
  Missing or truncated packs/ranges are misses. Recognized stale temporary
  files, and pages past the last bucket, are removed under their shard's
  lock because their writer also held it. A pack the walk found and no
  row names is removed only when no run holds its lock and no key of its
  footer has a row naming it, checked shard by shard; rows never move
  into an existing pack, so a key once found elsewhere stays elsewhere.
  Foreign filenames and symbolic links stay.
- **Write failures** warn once and disable further puts for that command;
  the build still succeeds. If GC cannot write, it reports the physical
  bytes it could not remove rather than pretending the packed cache is
  empty. Deduplication of compilation across processes is not coordinated.
- **Private native code is separate.** Clpack keeps its loose paths, owner,
  regular-file identity, single-link and mode-0600 checks. The new data
  index cannot name a Clpack entry. Ordinary loose records belong to the
  retired storage layout; GC retires them once `pack-v3` exists. Old
  binaries racing that cleanup still see ordinary cache misses.

**Planned logical code packs (#49, point 4).** These are distinct from
physical data packs. Current producers write `parse`, `check`, `iface`,
`graph`, `depfiles`, `link`, `owned` and `clpack`; the `codepack` and
`packhint` design below is not built. In it, code is stored in one
`codepack` per unit group, so a build reads and publishes tens of files
instead of one per instance.
- **The key** is the group's code keys in content order
  (`codepack_key`). Two programs that use the same instances of a
  unit share its pack.
- **An edit** changes one group's key. `Collect` then reads the packs
  the package's `packhint` lists for that unit, newest first, and
  takes the code of every instance whose code key they hold. Only the
  rest is emitted, and the group gets a new pack.
- **The hint** is the one entry a later run rewrites with other bytes
  (`packhint_key`: pipeline, profile, package). Each code key is checked
  on read, so a stale or lost hint only costs emission. It keeps four
  packs per unit, so an edit that is undone and made again still
  finds the undone state's code.
- **Cost.** A program whose instances of a unit differ from every
  cached pack's, such as a new package's first build, emits that
  unit's instances again: packs share whole groups only.

Portable-Wasm execution caches machine code function by function, one
`clpack` per module. Separately, Wasmtime release builds publish reusable
`.cwasm` native artifacts outside this cache, with compatibility and source
keys. Their loader confines unsafe deserialization to the native-artifact
boundary; it maps the open file without translating Wasm. See
[compile caches](../../docs/compiler/engines-and-test-runner.md#182-compile-caches).

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

A warm run reads no source file it does not check, and hashes none
(#1503). Built as follows.

- **Stamps.** A source set lists each file with a change stamp
  (`SourceEntry::stamp`): equal stamps under one set promise equal
  bytes. The disk set (`hd_command::disk::stamp`) hashes the file's size,
  modification and change times (seconds and nanoseconds), inode and
  device, from the `stat` its listing makes anyway. Any write moves the
  change time, which no tool can set back, so an edit that keeps the
  size and restores the modification time is seen; an editor's save by
  rename and a `git checkout` bring a new inode. The racy-clean rule: a
  file whose modification or change time is not 2 s (`SETTLE`, above
  the 2 s FAT granularity and the kernel's coarse-clock lag) before the
  time the listing began gets no stamp and is read. A write after that
  time lands in a later timestamp tick than any trusted time, so a
  trusted stamp never names bytes the run did not see. The time is the
  host clock, not a file-system stamp: `hd check` writes nothing into
  the package (`build/` stays absent), and on a local file system the
  kernel stamps files from that clock. A network file system whose
  server clock lags the client's by more than `SETTLE` on a coarse
  timestamp could trust a racy file; no stamp is trusted off Unix, where
  the standard library gives no change time.
- **Content stamps** (#1801 draft 44, `hd_driver::stamps`). A set whose
  stamps are file metadata (`SourceSet::metadata_stamps`) keeps a stamp
  table: one `depfiles` entry, keyed by the toolchain and the set's
  identity, that holds per settled file its metadata stamp and the hash
  of the text read under it. Every key and record of the run uses that
  content stamp. A file the table holds under its listed stamp is not
  read; a racy, new or changed file is read and hashed once, and a racy
  one gets no row, as git's racily clean entries are compared again. So
  a file saved with the same bytes, or written within `SETTLE`, keeps
  the package result below. Cost: one table read per set and one lookup
  per file; a read per racy or changed file. An in-memory set
  (`MemorySources`) stamps a file with its content hash when it is
  added, once per edit, not per run.
- **The last-run record.** Per source set with an identity
  (`SourceSet::identity`: the disk set's real directory path, a
  single-file program's real file path, one name for every standard
  input, whose file is stamped with its text's hash,
  `MemorySources::named`), one `depfiles` entry keyed by
  `depfiles_key(toolchain, "src:" + identity)` holds, per stamped file,
  its path, stamp and `FileSkim`: the hashes the keys read (source, API
  text, template text, tokens, docs), the length and code lines, the
  written `use` paths, the flags the driver otherwise scans the text for
  (`tests:` block, `main`, a host marker, `derive`, a fence), the
  newlines of a text that may register a test case, and once a run has
  parsed an entry module, its capability-row spans (`EntryFacts`). A
  module whose
  listed stamp is the recorded one takes its skim from the record, so
  every key (`reads_key`, `check_key`, `hdr_key`, the unit deep
  hashes) is computed without its text. The unit and module keys are
  recomputed from the skims each run, as hashes of hashes; they are not
  stored. A run writes the record again only when it skimmed a stamped
  file the record lacked or a recorded file is no longer listed, from
  this run's skims and the old rows; a partial listing (a test program's
  `retain`, `SourceSet::partial`) keeps the rows of the files it does
  not list. Dependencies' sets are disk sets of their own, so a fetched
  dependency has a record the same way; the embedded std keeps its
  toolchain-keyed `depfiles` entry.
- **Module rows** (#1801 option 2, `hd_driver::rows`). Per source set
  with an identity and per check run kind (`hd check`, `hd check
  --tests`, each under the root package's key), one `depfiles` entry
  holds per module: its last check key, the hash of the text it checked,
  and the `check` entry's diagnostics, meta, test registrations, init
  summary, `dbg` references and generic edges, sorted by path. A module
  whose key and text hash match its row takes the row: no `check` entry
  is read, and no `parse` entry, since a row is made only from a stored
  entry, whose text parsed. A module that a step after the checks reads
  whole gets no row (`Run::record_row`): one with an `@inline(.Always)`
  item or a hidden result, an entry module, a module with test bodies.
  These facts are functions of the check key, so a row never stands for
  a module whose entry the run needs; and an evicted entry is an ordinary
  miss. As rustc keeps a fingerprint per node of the previous dep graph
  (`SerializedDepGraph`) and loads a node's result from the on-disk cache
  only when asked (`OnDiskCache::try_load_query_value`), but without
  rustc's end-of-session promotion of every green result
  (`exec_cache_promotions`): the entries stay in the store, named by key.
  A build takes no rows; it reads every entry it collects. The record is
  written again only when a row was made that it lacked, or a file it
  holds is gone; a module not checked keeps its row; a partial listing
  keeps the rows of the files it does not list.
- **Unit rows** (#2347, `hd_driver::rows::UnitRows`). Beside the module
  rows, under their own kind, one `depfiles` entry holds per unit, by
  its path: its `reads_key`, its interface key, the read list and the
  `iface` entry's sections, then its last header key and the `graph`
  entry's findings. A unit whose `reads_key` matches and whose read
  answers give the row's interface key takes its interface from the row;
  a header check whose key matches takes the findings. So a warm run
  reads no entry for an unchanged unit, as with folders it read one
  unit's. The keys hash each unit's closure through a digest folded once
  per unit (`Run::digest`), not its closure's units one by one.
- **Texts on demand.** Module texts are read on first use
  (`hd_driver::Texts`): by a check that misses its entry, a parse whose
  `parse` entry misses, a module that may opt in to a user derivation
  template, a file whose skim holds a fence (`hd test`'s doc tests). The
  output keeps the same cells and reads a text the run never read
  through `SourceSet::detach`, for a diagnostic's position, a panic's
  line (the position pool tells modules apart by their skims' hashes)
  or a missing capability's row.
- **New and deleted files.** The listing finds them: a new path has no
  row and is read; a deleted one is not listed, and its row is dropped.
  A renamed unit is new paths. Directory modification times would
  spare the listing of an unchanged directory, but not the `stat` of
  each file that an edit needs, so the listing stays.
- **The package result** (#1510, `hd_driver::package_result`). An
  `hd check` run (`Goal::Analyze` or `Goal::CheckTests`, with or
  without check roots or an analysis consumer; no session, no doc
  scopes) whose every listed file has a stamp keys one `depfiles` entry
  by the toolchain, the run's settings, each package's identity, and
  every listed path and stamp. The run stores its diagnostics, its
  analysis findings, its entry rows and, with check roots, the `use`
  edges of the files they reach; a later run with the same key replays
  them and returns at once, with no record decode and no per-module
  lookup. A run with an internal error stores nothing: it may come
  from a damaged entry, not the sources. A caller stores what it
  derives from such a run under the same key and a hash of its own
  further inputs (`hd_driver::result_key`): the CLI stores a whole
  `hd check` or `hd lint` report there, lint source allows, rendered
  doc links and doc tests included, and `hd build`'s artifact with its
  positions section, for a build that reported nothing. `hd run` stores
  its module the same way, without positions: a panic's frames need each
  module's `check` entry, so a stored module's panic builds the program
  again, per module, and names its frames only when that build's module
  is the stored one. A run that does not panic resolves nothing. A
  single-file program (`hd FILE`, `hd check FILE`, `hd test FILE`) and
  standard input take the same paths: the key holds the single-file
  flag, and a report holds the FILE as the command line wrote it.
  `hd test` stores only its build half: each test program's module, its
  selected cases, the source parts of its `--affected` fingerprint and
  the build's warnings. Its outcomes are never stored, since they depend
  on capabilities, not only on the sources; its frames come from building
  again, as `hd run`'s do.
- **Fingerprints** of `hd test --affected` hold content hashes, never
  stamps: a source file's from the build's skims
  (`Output::source_hashes`), any other package file's from
  `build/test-records/stamps` (path, stamp, content hash), read again
  only when its stamp moved.

Per-run cost, n files of which k changed: one listing per source
directory and one `stat` per file (metadata only; git's status does the
same, and nothing short of a file-system watcher sees an edit with
less); one read of the record, O(n) skim bytes; O(n) key hashing over
skims; in a check run, one read of the module rows, O(n) row bytes, and
a binary search per module, with no `check` or `parse` entry read for a
module whose row matches; the reads, skims and record rewrite of the k
changed files (the rewrites are O(n) bytes, on a run that changed
something); plus whatever those k changes make the checker redo. Still
O(n) beyond the record reads: the unit interfaces each key's deep
hashes come from. A no-change run reads no source
file and writes nothing; an `hd check` run also makes one cache lookup
per disk set (its stamp table) and one more (the package result), hashes
each listed path and content stamp once, and
reads the files its replayed diagnostics are located in.

### 5.6 Verify Mode

Planned, not built: `HD_CACHE_VERIFY=1` turning every cache hit into a
recompute and a byte comparison, with a mismatch an internal error that
names the entry kind and key.

### 5.7 Eviction And The Size Cap

The disk store caps `obj/` at 10 GiB by default, evicting
least-recently-used entries (`cli.cache.obj.cap`, `cli.cache.obj.evict`).

- **Entry recency.** A packed entry's last-use timestamp is refreshed when
  over an hour old and merged at publication/settlement into its page, at
  most one page write per marked entry per hour. Compaction preserves every surviving entry's
  age; pack age never substitutes for entry recency. Eviction orders live
  entries by age, ties by kind and key, down to 90% when the live set exceeds
  its available cap. Dead records/orphan bytes alone do not evict a live set
  that already fits.
- **Physical accounting.** Final pack files, including dead/unindexed
  records, and the index count toward the compiled-data cap. Pending leased
  writes, the 32-byte lock and the small legacy stats ledger are not
  committed entries, as with the old temporary publish files. Unindexed
  final packs and dead pending files are reclaimed by lease-aware GC.
- **Running total (#1501).** Each shard's log header records that shard's
  physical share (its folded index files and the packs whose home it is),
  under the header's checksum, written only under the shard's lock; the
  total is their sum. Log records are not counted: at most 4,096 bytes per
  shard. A commit grows the total by its pack before renaming it; a fold
  grows it by its page growth before writing, and records the exact change
  after. A collection records its walk's total plus the tracked packs
  published since. A command reads the headers with no lock; a torn header
  fails its checksum and the total is unknown. A shard a writer creates
  starts with a known zero. Deletions without an index change only lower
  the true total.
- **Compaction.** In a collection, a referenced pack over 25% dead, or any
  pack with dead bytes when the walk found the cache over the cap, is
  copied by live frame range into a new leased pack. The copy takes no
  lock. The commits, one shard lock at a time, repoint each row still at
  its old range, and the old pack goes once no key of its footer names it.
  A row replaced or evicted meanwhile keeps its new state; a copy no row
  takes is dropped with its lease. An older reader can retain the old
  descriptor; Windows may postpone deletion until that descriptor closes,
  and a later collection retries it.
- **Bounded pack count.** Every command that publishes adds a pack, so its
  settlement merges small packs without a walk: the pack table (in the
  index, read only by a publishing run) groups packs under 8 MiB by power
  of two, and once a class holds four, their live frames, found from each
  pack's footer and its rows' pages, are copied into one pack and the four
  removed. Each shard's pack table lists the packs whose home it is; a
  publishing run merges within the home shards it published to, so packs
  stay bounded per shard. The copy is the compaction's: no lock while copying,
  each shard's lock for its commit and at most four unlinks. A frame is copied once per
  class it climbs; packs stay at most three per class plus the 8 MiB packs,
  which the cap bounds. 1,000 publishing commands leave at most 14 packs.
- **Private/legacy accounting.** `obj/stats` tracks 256 shard sizes and the
  packed-migration state. Publication and eviction serialize file changes
  and their size updates with the ledger lock. Replacing a key subtracts
  its previous size; a lost publication race adds nothing. The fixed-size
  ledger header becomes invalid before file changes and valid only after
  accounting commits. Process interruption or an older ledger makes sizes
  unknown, recovered by bounded scans. An incomplete scan leaves its shard
  unknown. The first packed publication invalidates known loose sizes once
  so scans retire obsolete ordinary files. Known under-budget shards never
  get listed automatically; Clpack keeps its per-shard mtime LRU. A selected
  root may alias its directory; kind and shard links remain excluded. Automatic
  packed cleanup subtracts known legacy bytes from the shared cap. Explicit
  `hd cache gc` scans both namespaces and uses exact remaining legacy bytes
  as the packed budget; incomplete loose inventory returns an error before
  packed reclamation. The two stores do not each get a fresh 10 GiB.
- **A command never walks (#1932).** A command's settlement applies its
  recency marks and, after a publication, one small-pack merge: O(entries
  written and marked). It then reads each shard's 112-byte log header with
  no lock. Over the cap, or unknown, it asks for a collection.
  It also asks when the ledger knows no size for some legacy shard. Known
  legacy shards that writes put over budget are cleaned one per command,
  a count rather than a time; unknown ones are left to the collection.
  The only wall-clock deadline is a wait for another run's claim or
  `solo` lock (#2056).
- **Collection, once per interval.** `<obj>/gc` holds a lock and the start
  time and cap of the last collection. A request is dropped when that
  collection started less than an hour ago under a cap no smaller, or
  when another process holds the lock (`try_lock`), so no command waits.
  The CLI starts a detached `hd cache gc` with `HD_CACHE_GC=auto` for it,
  as `git gc --auto` detaches (`gc.autoDetach`); a store with no spawner
  collects on drop. A collection walks `pack-v3/` and its index shards
  and reads every page, with no lock, retrying while the head changes.
  It evicts the oldest rows until the live set and its index fit 90% of
  the cap, in one commit per shard, under that shard's lock, that keeps
  rows hit or replaced since the read. Then it removes unnamed packs with no lock,
  compacts, merges one size class, sweeps crashed files, walks again and
  records the total. Every legacy shard is cleaned too. Cost: O(packs +
  rows) once per interval, plus the bytes it reclaims; a shard lock
  covers that shard's index commits only. Writes within the interval can take the
  cache past the cap, as ccache's cleanup lags its counters.
- **References.** cargo's global cache GC runs at most once per
  `gc.auto.frequency` (a day), its last run recorded in its SQLite
  tracker, and holds the package-cache lock that other cargos wait for;
  hd's collector holds a lock no publisher or reader takes. ccache keeps
  per-directory size counters and cleans a full directory down to
  `limit_multiple` of its share; hd's legacy ledger works the same way,
  but the packed store needs a whole walk to find orphans, so it collects
  whole, per interval. sccache's single server walks once at startup and
  evicts on insert; hd has no server, so LRU order lives in the shared
  index. rustc's incremental GC try-locks other sessions' lock files and
  deletes only those it can lock, as hd's lease and collection locks do.
- `hd cache gc` takes the collection lock, waiting for a running
  collection, has no time limit and prints physical bytes before/after,
  subject to outstanding readers preventing deletion.
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
- `SingleFlight` (`flight.rs`) wraps a store that concurrent builds
  share, such as the conformance runner's. Each build reads through its
  own `FlightRun`. The first build that misses a key of a chosen kind owns
  it, and another build's read of that key waits until the owner writes
  it, unless waiting would close a cycle. A dropped run lets go of the
  keys it never wrote. A kind fits when a build writes its entry soon
  after the miss, so `depfiles`, written when the build ends, stays out.

Planned, not built: IndexedDB persistence. The JS host would load the
entries into the `MemoryStore` once when the worker starts, then drain
the new ones (`MemoryStore::drain_new`) after each run and write them
back in one transaction.

### 5.9 What Cache I/O Costs

On macOS a publish costs about 340 µs and an open and read 60 to 120 µs
(under load), and a Linux runner is likely 5 to 20 times cheaper. With
one file per entry, a warm one-function edit opens one `check` entry per
module, publishes the edited module's `check`, and on a build misses the
program's `link` entry and emits the whole program again (code packs per
unit group are planned, §5.4). The stat manifest (§5.5) keeps
source reads to the changed files; the planned early cutoff (§5.3.1)
exists to cut the entry counts to a few files per edit.

The first release rechecks a whole module on a body edit
(crates/hd_check/DESIGN.md (type checking) §1.7). Per-body reuse inside
the module's entry
([crates/hd_tir/DESIGN.md §4.13.1](../hd_tir/DESIGN.md#4131-task-structure-per-module))
is the next lever for a large module.
