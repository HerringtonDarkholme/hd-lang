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
| `iface` | a folder interface blob (§4.11), its anchored diagnostics, private and loop names, module uses | `FolderIface` | dependents' keys, resolution, coherence, `hd doc` |
| `check` | one module and role, in sections: diagnostics with severities and fix-its, metadata with the TIR content hash, the module's items, TIR (only when the module has no error), test registrations, the init summary, `dbg` references, frame names; a REPL input adds its bindings | `ModuleFinish` | output, `InitOrder`, D2, `hd check --tests`, `hd test` |
| `parse` | empty: its presence says a file's bytes parse with no diagnostic | `ModulePrep` | `ModulePrep`, before trusting a `check` hit |
| `graph` | stage-B header findings per folder (`hdr_key`); the package's overlap findings (one coherence key); one folder's init order (`init_key`) | `HeaderCheck`, `Coherence`, `InitOrder` | output, D2 |
| `depfiles` | the skims of every embedded std file | the first run of a toolchain | every later run (§5.5) |
| `code` | one instance's emitted code (§13.8) | `Emit` | `Collect` |
| `link` | the program's Wasm, keyed by `prog_key`; the REPL prelude with its session exports | `Link` | D2, the REPL |

`EntryKind` also names `check-test`, `pkgres`, `codepack`, `clpack`,
`packhint` and `cwasm`; no code writes them.

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
  by `hdr_key`. Implementation differs: coherence is its own entry and
  init order is one entry per folder, which is worse: more files to open
  and publish, and #177 measured a per-folder init entry costing more
  than recomputing it.

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
iface_key(F)  = H("iface", toolchain_key, package key, folder path,
                  [(module path, role, api_text_hash) for each file of F],
                  [(folder path, deep_hash) for each other folder in closure(F)])
check_key(m)  = H("check", toolchain_key, package key, module path, role, token_hash(m),
                  [(folder path, deep_hash) for each folder in closure(m)],
                  template text hashes, REPL library and earlier inputs)
parse_key(f)  = H("parse", toolchain_key, source_hash(f))
hdr_key(F)    = H("hdr", toolchain_key, package key, folder path, own deep_hash,
                  [(folder path, deep_hash) for each folder in closure(F)], tuple-template text hashes)
coh_key(T)    = H("coh", toolchain_key, stable path of T, sorted head hashes of T's impls)
init_key(F)   = H("init", toolchain_key, folder path, [(module path, init summary hash)])
depfiles_key  = H("depfiles", toolchain_key, "std")
prog_key      = H("prog", toolchain_key, pipeline, profile, entry, [(module, TIR content hash)])
code_key      = H("code", pipeline, profile, instance key, item TIR hash,
                  H(toolchain_key, the program's data layouts, callee representation summaries))
```

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
- **Interface reach, not import reach.** A folder's `deep_hash` folds its
  own `api_hash` and the deep hashes of the folders its interface
  mentions (§4.11.3), and `iface_key(F)` should name the same set:
  folders named by F's public interface, not every folder F's uses
  reach. Implementation differs: `iface_key` lists every other folder in
  F's closure, which is worse: an API edit in a folder F only uses
  privately rebuilds F's interface.
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
  the key across a body edit above an impl. Implementation differs: one
  key over every built folder's deep hash, so any interface change
  reruns all of coherence; worse, see #177.
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

### 5.4 Entry Format And Atomic Publish

```text
obj/
  <kind>/<2 hex>/<32 hex>     one file per key; the 2 hex digits (the key's top byte) name the shard
  stats                       the shard-size ledger (§5.7)
```

- **Entry header** (64 bytes, little-endian): magic `HDCE`, kind, layout
  version, the key, the toolchain key, payload length, a checksum of the
  payload (the low half of its xxh3-128), the section count and flags.
  A section table of 16-byte rows (kind, flags, rows, offset) follows,
  then the payloads, each with a length prefix
  ([docs/compiler/data-structures.md §3.20](../../docs/compiler/data-structures.md#320-cache-entries-and-the-manifest)).
  A reader checks the magic, kind, length and checksum, and the driver
  checks the key, toolchain and layout. Any mismatch is a miss.
- **Publish:** write to a temporary file next to the final path (named
  by the process ID), then rename it over the final path. There is no
  `fsync`: a crash can lose a recent entry, and a torn one fails its
  checksum and is a miss. A cache entry is never the only copy of
  anything, so durability is not worth a sync per entry. Entries are
  immutable, and two writers of one key write the same bytes, so a lost
  race costs only the duplicate work.
- **No locks on reads.** Readers read whole files. Writers never modify
  a file in place.
- **Dedup across processes** is not coordinated: two processes that miss
  the same key both compute it.
- **Write failures** (read-only or full disk) produce one warning and the
  run continues uncached. A cache never makes a check fail.
- **Print, then publish.** Diagnostics are printed as soon as the last
  task that can add one finishes. Publishing runs on one I/O thread that
  drains a queue of finished entries, so output never waits for a
  publish. Implementation differs: there is no I/O thread. `hd check`,
  `hd build`, `hd run` and `hd test` hold a run's entries in memory
  (`DeferredStore`) and publish them on the main thread after the
  report, so the report never waits for a publish, but the process exit
  still does.
- The layout version lives in every entry header and in
  `toolchain_key`, not in an `obj/LAYOUT` file; a layout change misses
  instead of misreading, with nothing extra to read.
- A corrupt entry is a miss but is not deleted; eviction removes it like
  any old entry, which is equal and needs no delete on the read path.

Planned, not built: code stored in packs per program and folder group
(`codepack`, with a `packhint` per program), so a build reads and
publishes tens of files instead of one per instance (see #49); compiled
machine code cached across runs (`clpack`, `cwasm`), which wasmtime's
`unsafe` deserialization rules out today (crates/hd_run_wasmtime/DESIGN.md
§17.9).

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
`store::ManifestRecord` defines the per-file record; nothing uses it.

### 5.6 Verify Mode

Planned, not built: `HD_CACHE_VERIFY=1` turning every cache hit into a
recompute and a byte comparison, with a mismatch an internal error that
names the entry kind and key. `store::verify_entry` compares two encoded
entries section by section; nothing calls it yet.

### 5.7 Eviction And The Size Cap

The disk store caps `obj/` at 10 GiB by default, evicting
least-recently-used entries (`cli.cache.obj.cap`, `cli.cache.obj.evict`).

- **Use marks.** A hit sets the entry's mtime to now if it is more than
  an hour old (Go's rule), so the mtime is the last use to within an
  hour, at most one write per entry per hour.
- **Approximate LRU, per shard.** Exact LRU needs a full scan, and a
  full scan of a 10 GB cache does not fit in a user command. So eviction
  follows ccache: 256 shards named by a key's top byte, across every kind
  directory. Each shard's budget is the cap divided by 256, and LRU holds
  within a shard.
- **Size accounting.** `obj/stats` holds 256 approximate shard sizes and
  the next shard to rescan. When a used store is dropped, it adds the
  bytes it published per shard under a short lock of that file.
- **Automatic eviction.** On drop, a used store rescans one shard in
  round-robin order (drift correction, every shard once per 256 runs),
  then cleans the shards over budget and the shards of unknown size:
  oldest first, ties by kind and key, down to 90% of the budget. It
  stops starting shards after 20 ms (`EVICT_BOUND`); later commands
  clean the rest.
- **`hd cache gc`** cleans every shard now, with no time bound, and
  prints the bytes before and after.
- **The cap** comes from `HD_CACHE_MAX_SIZE` (bytes, with `K`, `M`, `G`
  suffixes).
- **Safety.** Eviction deletes only regular files at
  `<kind>/<shard>/<32 hex>` whose name parses as a key of that shard,
  follows no symbolic link, and never touches `pkg/`. A file another
  process removed first, or one in use on Windows, is skipped.

### 5.8 The Browser `CacheStore`

```rust
pub trait CacheStore: Sync {
    fn get(&self, kind: EntryKind, key: &Hash128) -> Option<Arc<[u8]>>;  // synchronous
    fn put(&self, kind: EntryKind, key: &Hash128, bytes: &[u8]);
    fn touch(&self, kind: EntryKind, key: &Hash128);
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
`code` entry per instance in the program. The planned packs (§5.4), stat
manifest (§5.5) and early cutoff (§5.3.1) exist to cut those counts to
a few files per edit.

The first release rechecks a whole module on a body edit
(crates/hd_check/DESIGN.md (type checking) §1.7). Per-body reuse inside
the module's entry
([crates/hd_tir/DESIGN.md §4.13.1](../hd_tir/DESIGN.md#4131-task-structure-per-module))
is the next lever for a large module.
