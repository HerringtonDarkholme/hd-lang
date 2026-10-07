# New Compiler Design: Cache

Part of the [compiler design](README.md).

## 5. Cache

### 5.1 Where Things Live

| Data | Location | Shared | Why |
| --- | --- | --- | --- |
| interface blobs, check results, TIR, coherence and init results, package results; D2's code | `$HD_CACHE/obj/` | per user, across worktrees and packages | keys are content hashes, so worktrees on one commit share everything |
| fetched dependency trees | `$HD_CACHE/pkg/` (spec today) | per user | [Cache](../../spec/cli/command-line.md#cache) |
| staging for atomic writes | `$HD_CACHE/tmp/` | per user | same file system as `obj/`, so rename is atomic |
| stat manifest, last test record | `build/.hd/` of each package | per worktree | holds paths and mtimes, which must not enter shared keys |
| std interfaces | embedded in the `hd` binary | per binary | no cache read for std ([Q2](research.md#recommendation-1)) |

`$HD_CACHE` resolves as [`cli.cache.directory`](../../spec/cli/command-line.md#r-cli.cache.directory)
says. The spec's `hd clean --cache` today refuses a cache directory that
holds anything but `pkg`, `hash` and `tmp`
([`cli.clean.cache.layout`](../../spec/cli/command-line.md#r-cli.clean.cache.layout)),
so `obj/` needs a spec change. That is open question 1.

### 5.2 Entry Kinds

| Kind | Content | Written by | Read by |
| --- | --- | --- | --- |
| `iface` | a folder interface blob (§4.11); its header carries `api_hash`, `deep_hash`, `heads_hash` | `FolderIface` | dependents' key computation, resolution, coherence, `hd doc` |
| `check` | a module's diagnostics, init summary, row results, and fact records | `ModuleFinish` | output, `InitOrder`, D2 |
| `tir` | a module's TIR, per-item TIR hashes and dependency lists (§4.13.11) | `ModuleFinish` | D2 |
| `check-test` | the test overlay's diagnostics and test registrations | `TestOverlay` | `hd check --tests`, `hd test` |
| `coh` | one trait's overlap diagnostics | `Coherence` | output |
| `init` | one folder's statement order and its diagnostics | `InitOrder` | output, D2 |
| `pkgres` | the package's sorted diagnostics and summary counts | `PackageResult` | the warm fast path |
| `depfiles` | a fetched dependency's file list with content and api text hashes | first use of the dependency | every later run (§5.5) |
| `code`, `link`, `cwasm` | Wasm per instance, per program, and precompiled per engine (§11.2) | D2 | D2 |

**Fact records** (the program-database hook,
[Q6](research.md#q6-program-database-hook)) are a
section of each `check` entry: declarations with spans and signature text,
resolved references with spans, call edges, impls and derives, inferred
rows, and the diagnostics. Symbols are printed stable paths. No schema is
public yet; a merged index comes before `hd callers` ships (prior-art
change 13).

### 5.3 Key Composition

Every key is `H(kind tag, fields...)` over canonical bytes, with xxh3-128.

```text
toolchain_key = H("tc", compiler build id, std pack hash, target ("wasm32-gc"),
                  host profile table hash, cache layout version, hash algorithm id)

iface_key(F)  = H("iface", toolchain_key, package key, folder path,
                  sorted [(module path, role, api_text_hash) for each file of F],
                  sorted [(folder path, deep_hash) for each folder that F's uses reach])

check_key(m)  = H("check", toolchain_key, package key, module path, role, source_hash(m),
                  deep_hash(own folder),
                  sorted [(folder path, deep_hash) for each folder that m's uses reach])

test_key(m)   = H("check-test", check_key(m), source_hash(m),
                  sorted [(folder path, deep_hash) for each folder the test code uses],
                  sorted dev-dependency keys)

coh_key(T)    = H("coh", toolchain_key, stable path of T, sorted head hashes of T's impls)
init_key(F)   = H("init", toolchain_key, folder path, sorted [(module path, init summary hash)])
pkgres_key    = H("pkgres", sorted check, test, coh and init keys, folder graph hash,
                  manifest diagnostics hash, command mode)
fast_key      = H("fast", toolchain_key, package key, sorted [(path, source_hash)] of every
                  file, sorted dependency keys, command mode)
```

- **Package key.** `root` for the package being built, never its path.
  A fetched dependency is `HOST_PATH@VERSION` plus its tree hash. A path
  dependency is its workspace-relative path plus its own file hashes.
- **Command mode** is the set of options that change what is checked:
  `--tests`, `--all`, a FILE scope. Output options (`--format`,
  `--max-errors`) are not in keys, since entries hold structured
  diagnostics and rendering happens at output.
- **What a key never contains** (lesson 9): absolute paths, the working
  directory, the `hd` binary's location, mtimes, user names, environment
  variables, thread counts, run IDs. The path-independence test checks one
  package from two checkout paths and compares every key byte for byte.
- **What changing an input reaches** (lesson 1 and TypeScript #64552): the
  std version, the target and the host profile table are in
  `toolchain_key`, so changing any of them misses every entry.
- **The fast path (mine).** `fast_key` names a `pkgres` entry directly, so
  a warm run with no edit reads the manifest and one entry. When it
  misses, the run computes the real keys bottom-up and still reuses every
  entry that did not change.
- **Hermetic by API.** A module's check receives its source and the
  interface handles its key names. It cannot reach the file system or
  another module's source, so it cannot read an input its key misses
  ([Q2](research.md#recommendation-1)).

### 5.4 Entry Format And Atomic Publish

```text
obj/
  LAYOUT                      "hd-obj 1 xxh3-128"
  <kind>/<2 hex>/<32 hex>     one file per key
  size                        approximate total bytes, for eviction (§5.7)
  gc                          time of the last full scan
```

- **Entry header:** magic, kind, layout version, the key itself, payload
  length, and an xxh3-64 checksum of the payload. A reader checks all of
  them. A mismatch, such as a truncated or corrupt file, counts as a miss,
  and the file is deleted.
- **Publish:** write to `$HD_CACHE/tmp/<random>`, flush, rename to the
  final path. On a platform where rename does not replace, an existing
  final file wins. Entries are immutable, and two writers of one key write
  the same bytes, so a lost race costs only the duplicate work.
- **No locks on reads.** Readers open and map files. Writers never modify
  a file in place.
- **Dedup across processes** is not coordinated: two processes that miss
  the same key both compute it. Module entries take milliseconds. This
  conflicts with the proposed `cache-contention` target "each entry
  computed once", which is open question 3.
- **Write failures** (read-only or full disk) produce one warning and the
  run continues uncached. A cache never makes a check fail.
- **Large entries** (blobs over 64 KiB, D2's code) are memory-mapped.
  Small ones are read whole.

### 5.5 The Stat Manifest And Change Detection

```rust
pub struct Manifest {            // build/.hd/manifest, one per package per worktree
    pub written_at: Timestamp,
    pub files: Vec<ManifestFile>, // sorted by path
}
pub struct ManifestFile {
    pub path: RelPath, pub size: u64, pub mtime_ns: i128, pub inode: u64,
    pub source_hash: Hash128, pub api_text_hash: Hash128,
    pub uses: Vec<ModulePath>,   // for the folder graph and --affected, without reading the file
    pub role: FileRole,
}
```

1. **List.** Walk the roots' directories every run. A file that the
   manifest lists but the walk does not find is deleted; a file that the
   walk finds but the manifest lacks is new (Gleam #4320).
2. **Stat** every file. The research's `io-per-check` target expects
   stats proportional to the file count.
3. **Trust** a record when size, mtime and inode are equal, and the mtime
   is earlier than `written_at`. A file whose mtime is at or after
   `written_at` is "racily clean" and is hashed anyway (git's rule). This
   covers same-second writes on file systems with coarse timestamps.
4. **Hash** every other file. A restored file with an old mtime has a
   different inode or size, or its content hash decides.
5. **Write** the manifest atomically at the end of the run if anything
   changed. A concurrent run in the same worktree may overwrite it; that
   is safe, because every record is verified by stat, and a mismatch means
   re-hashing.
6. **Dependencies are not stat'ed.** A fetched version is read-only and
   verified by its tree hash ([`cli.cache.hash`](../../spec/cli/command-line.md#r-cli.cache.hash)).
   Its file hashes and use lists are a `depfiles` entry keyed by the tree
   hash, computed once per machine.

### 5.6 Verify Mode

`HD_CACHE_VERIFY=1` turns every cache hit into a recompute and a byte
comparison. A mismatch is an internal error that names the entry kind,
the key, and a structural diff of the two decoded entries, and exits with
status 101. CI runs the conformance suite and the incremental-soundness
fuzzer in verify mode, as rustc's incremental verification does.

### 5.7 Eviction And The Size Cap

Owner, 2026-10-07: the shared cache defaults to **10 GB**, evicted
least-recently-used. Eviction runs automatically when the cache is over
the cap, `hd cache gc` runs it on demand, and the cap is configurable.

- **Use marks.** A cache hit touches the entry's mtime if it is more than
  an hour old (Go's rule), so the mtime is the last-use time to within an
  hour, at most one write per entry per hour.
- **Size accounting.** Each run adds the bytes it wrote to `obj/size`,
  under a short advisory file lock, once at the end of the run.
- **Automatic eviction.** When the total passes the cap, the run that
  noticed it scans `obj/` after its output is flushed, deletes entries in
  order of oldest use until the total is at most 90% of the cap, and
  writes the exact total back. A full scan also runs at least once a day
  (`obj/gc`), which corrects drift in `size`.
- **`hd cache gc`** runs the same scan and eviction now, and prints the
  bytes before and after.
- **The cap** comes from `HD_CACHE_MAX_SIZE` (bytes, with `K`, `M`, `G`
  suffixes). Whether a manifest key or a user config file can also set it
  is a CLI spec detail.
- **Safety.** Eviction deletes only `obj/<kind>/` files whose names parse
  as keys, never follows a symbolic link, and never touches `pkg/`.
- A process holding a mapped entry keeps working if another process
  deletes it, because unlinking a mapped file is safe on Unix. On Windows,
  eviction skips files it cannot delete.

### 5.8 The Browser `CacheStore`

```rust
pub trait CacheStore: Sync {
    fn get(&self, kind: EntryKind, key: &Hash128) -> Option<EntryBytes>;  // synchronous
    fn put(&self, kind: EntryKind, key: &Hash128, bytes: &[u8]);
    fn touch(&self, kind: EntryKind, key: &Hash128);
}
pub struct MemoryStore { entries: Mutex<HashMap<(EntryKind, Hash128), Arc<[u8]>>>, new: Mutex<Vec<..>> }
```

- The core never awaits storage. Before a run the JS host loads the
  playground's entries from IndexedDB into the `MemoryStore`. After the
  run it drains the new entries and writes them back.
- Keys are the same as native. `toolchain_key` keeps a new compiler build
  from reading old entries.
- The IndexedDB store keeps a last-use time per entry and evicts the
  oldest past a small browser cap, such as 50 MB, when it loads.
- Std is embedded, and playground programs are small, so a cold check is
  the normal browser case. The store mainly speeds up "Run" after "Check".
