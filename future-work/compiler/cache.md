# New Compiler Design: Cache

Part of the [compiler design](README.md).

## 5. Cache

### 5.1 Where Things Live

| Data | Location | Shared | Why |
| --- | --- | --- | --- |
| interface blobs, check results, TIR, coherence and init results, package results; D2's code | `$HD_CACHE/obj/` | per user, across worktrees and packages | keys are content hashes, so worktrees on one commit share everything |
| fetched dependency trees | `$HD_CACHE/pkg/` (spec today) | per user | [Cache](../../spec/cli/command-line.md#cache) |
| staging for atomic writes | `$HD_CACHE/tmp/` | per user | same file system as `obj/`, so rename is atomic |
| stat manifest, last test record, last link record per program (§5.4) | `build/.hd/` of each package | per worktree | holds paths, mtimes and build history, which must not enter shared keys |
| std interfaces | embedded in the `hd` binary | per binary | no cache read for std ([Q2](research.md#recommendation-1)) |

`$HD_CACHE` resolves as [`cli.cache.directory`](../../spec/cli/command-line.md#r-cli.cache.directory)
says. The spec's `hd clean --cache` today refuses a cache directory that
holds anything but `pkg`, `hash` and `tmp`
([`cli.clean.cache.layout`](../../spec/cli/command-line.md#r-cli.clean.cache.foreign)),
so `obj/` needs a spec change, which the owner accepted (open question
10-1).

### 5.2 Entry Kinds

| Kind | Content | Written by | Read by |
| --- | --- | --- | --- |
| `iface` | a folder interface blob (§4.11); its header carries `api_hash`, `deep_hash`, `heads_hash` | `FolderIface` | dependents' key computation, resolution, coherence, `hd doc` |
| `check` | a module's diagnostics, init summary, row results, and fact records | `ModuleFinish` | output, `InitOrder`, D2 |
| `tir` | a module's TIR, per-item TIR hashes and dependency lists (§4.13.11) | `ModuleFinish` | D2 |
| `check-test` | the test overlay's diagnostics and test registrations | `TestOverlay` | `hd check --tests`, `hd test` |
| `hdr` | one folder's stage-B header diagnostics ([resolution-and-interfaces.md §4.10.1](resolution-and-interfaces.md#4101-header-validation-stages)) | `HeaderCheck` | output |
| `coh` | one trait's overlap diagnostics | `Coherence` | output |
| `init` | one folder's statement order and its diagnostics | `InitOrder` | output, D2 |
| `pkgres` | the package's sorted diagnostics and summary counts | `PackageResult` | the warm fast path |
| `depfiles` | a fetched dependency's file list with content and api text hashes | first use of the dependency | every later run (§5.5) |
| `locs` | one file's declaration table: stable path to byte offset and line, keyed by `source_hash` | the parse or skim of a changed file | output, to resolve relative spans (§5.3) |
| `code`, `link`, `cwasm` | Wasm per instance, per program, and precompiled per engine (§11.2) | D2 | D2 |
| `cranelift` | one compiled function, keyed by wasmtime's per-function cache key (§18.2) | the `CacheStore` adapter of the incremental compilation cache | the same adapter |
| `codepack`, `clpack` | every `code` entry, or every `cranelift` entry, of one program's last link, with an index by key (lowering pass) | `Link` and the adapter, after a build | the next build of the same program in the same worktree |

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
                  host profile table hash, cache layout version, hash algorithm id,
                  semantic limits hash)

manifest_key  = H("manifest", normalized semantic project configuration, manifest diagnostics hash)

iface_key(F)  = H("iface", toolchain_key, package key, folder path,
                  sorted [(module path, role, api_text_hash) for each file of F],
                  sorted [(folder path, deep_hash) for each folder that F's uses reach])

check_key(m)  = H("check", toolchain_key, package key, module path, role, source_hash(m),
                  sorted [(folder path, deep_hash) for each folder in closure(m)])

hdr_key(F)    = H("hdr", iface_key(F),
                  sorted [(folder path, deep_hash) for each folder in closure(F)])

test_key(m)   = H("check-test", check_key(m), source_hash(m),
                  sorted [(folder path, deep_hash) for each folder in test_closure(m)],
                  sorted dev-dependency keys)

coh_key(T)    = H("coh", toolchain_key, stable path of T, sorted head hashes of T's impls)
init_key(F)   = H("init", toolchain_key, folder path, sorted [(module path, init summary hash)])
pkgres_key    = H("pkgres", sorted check, test, hdr, coh and init keys, folder graph hash,
                  manifest diagnostics hash, command mode)
fast_key      = H("fast", toolchain_key, package key, sorted [(path, source_hash)] of every
                  file, sorted dependency keys, command mode)
```

- **Dependency closures.** `closure(m)` is m's own folder plus every
  folder reachable from it through the folder graph's use edges. It is
  the transitive closure, not only the folders m names. `closure(F)` is
  the same for a folder. `test_closure(m)` adds every folder that the
  test code's uses reach, dev dependencies included. These are the bit
  sets the driver builds per context (scheduler.md §6.1), the same sets
  that filter the candidate directory. So each key lists every folder
  whose impls its check can see.
- **Argument-owned impls need no extra hash (Codex re-review N-A1).** A
  goal with an open trait argument reads impls owned only through a
  trait argument, from any folder in the asking context's closure
  ([trait-solver.md §3.2](trait-solver.md#32-owner-modules)). Such an
  impl has a nameable trait and target, so its head is in its folder's
  `api_hash` and deep hash
  ([resolution-and-interfaces.md §4.10](resolution-and-interfaces.md#410-folder-interface-construction)).
  The closure lists hold that deep hash. So adding `impl Pick[Product]
  for Receiver` in a third folder rechecks exactly the contexts whose
  closure holds it. An earlier draft added `argc`, a Merkle hash over
  the closure's `arg_impls` sections, to `check_key` and `hdr_key`. It
  was needed only while the lists named direct uses. With closure lists
  it is redundant, so it is removed.
- **What the closure list costs.** Before, a folder that m reached only
  through another folder's bodies entered m's key only through API
  mentions in deep hashes. Now any public API edit in it rechecks m.
  Public API edits are much rarer than body edits, and the
  `recheck-precision` metric counts them
  ([testing-the-compiler.md §8.2](testing-the-compiler.md#82-incremental-soundness)).
- **The solver memo is never persisted** and is never part of a key. It
  lives for one run (trait-solver.md §7.1). A `check` entry is a
  function of its key because the memo's answers are functions of
  their goals. A goal that reads the candidate directory holds its
  context's `ImplUniverseId`, which is a function of the closure that
  the key lists.
- **Header checks (stage B).** `HeaderCheck(F)` runs the solver over
  F's frozen impl tables and its dependencies'. Its inputs are F's
  interface and the interfaces of `closure(F)`, their `arg_impls`
  sections included, which `hdr_key` names by deep hash. Its entry
  holds only diagnostics.
- **Head hashes (Codex re-review N-D1).** A head hash in `coh_key` is
  `H(canonical head, rank)`. The canonical head is the impl's
  parameters, target and trait arguments. The rank is its content rank
  `(package, module path, item index)`
  ([trait-solver.md §5.3](trait-solver.md#53-why-no-global-index)).
  The report names the later impl of a pair by rank, so swapping two
  overlapping impls must miss. The rank uses the item index, not a byte
  offset, so a body edit above an impl keeps the key.
- **Package key.** `H("root", manifest_key)` for the package being built,
  never its path.
  A fetched dependency is `HOST_PATH@VERSION` plus its tree hash. A path
  dependency is its workspace-relative path plus its own file hashes.
- **The semantic project configuration (Codex review, A1).**
  `manifest_key` hashes what the manifest changes about checking and
  building: the package name, the resolved dependency bindings (name to
  package key), the executables and their roles, the required toolchain
  version, and the hash of the manifest's diagnostics. Fields that change
  nothing, such as the description, are left out. The root package key
  holds it, so it reaches every `iface`, `check` and `fast_key` key, and
  through `tir` keys the `prog_key` and code keys. A rename changes the
  printed `TypeId` names, so it must miss. A semantic manifest edit is
  rare, so rechecking everything after one costs little.
- **Manifest errors come first.** `hd check` step 1 (§7.1) reports a
  manifest error and stops before any cache lookup. So no cached success
  can hide a broken manifest.
- **Semantic limits (Codex review, A2).** The effective values of the
  limits in §4.15 that decide acceptance (body fuel, resolution depth,
  type size, nesting, instantiation depth) are hashed into
  `toolchain_key`. Today they are fixed defaults. If a flag or variable
  ever sets one, the effective value enters the key whatever its source.
  Thread count, output options and the opt-in memory cap are not
  semantic and stay out.
- **Interrupted work is never cached.** A task stopped by cancellation or
  by the memory cap writes no entry. Only a completed computation,
  including one that ran out of fuel, is published.
- **Command mode** is the set of options that change what is checked:
  `--tests`, `--all`, a FILE scope. Output options (`--format`,
  `--max-errors`) are not in keys, since entries hold structured
  diagnostics and rendering happens at output.
- **What a key never contains** (lesson 9): absolute paths, the working
  directory, the `hd` binary's location, mtimes, user names, thread
  counts, run IDs, and environment variables that do not change the
  result. An input that changes the result is hashed by its effective
  value, whatever supplied it. The path-independence test checks one
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
- **Suggestions too (Codex review, A7).** Diagnostic help searches only
  keyed inputs: std (in `toolchain_key`) and the folders the module's
  key names. A "did you mean" or "add `use`" suggestion for a trait in a
  folder outside the key is not offered in the first release. A
  package-wide suggestion index with its own hash in the key is a later
  option. [type-checking.md §10.5](type-checking.md#105-fix-its-for-common-mistakes)
  follows this rule.
- **Locations are not semantic (Codex review, A3).** `iface_key` and
  `coh_key` ignore source positions, so a whitespace edit or a longer
  body above a declaration keeps them. Entries under such keys therefore
  store no absolute position. A span in an `iface`, `coh` or `init`
  entry, and a span in a `check` entry that points into another file,
  is a **token anchor**: a declaration's stable path, the index of its
  first token among that declaration's tokens, a token count, and byte
  offsets inside the first and last token
  ([data-structures.md §3.7](data-structures.md#37-spans-and-files)).
  Whitespace and comments are not tokens, so an edit that keeps a
  declaration's API hash keeps every anchor into it, even an edit inside
  the declaration (Codex re-review N9). A byte offset from the
  declaration start would not: spaces inserted between a function's
  name and its parameters shift every later offset under the same key.
  Output resolves an anchor through the current file's `locs` entry,
  keyed by the file's `source_hash`, which holds each declaration's
  start; it re-lexes that declaration to find the token, which costs
  microseconds and runs only for a printed diagnostic. Any file whose
  bytes changed is parsed in this run, so its `locs` entry exists.
  Recomputed `iface` bytes are then equal under an equal key, which
  verify mode checks. rustc's incremental mode keeps spans relative to
  their item for the same reason.

### 5.4 Entry Format And Atomic Publish

```text
obj/
  LAYOUT                      "hd-obj 1 xxh3-128"
  <kind>/<2 hex>/<32 hex>     one file per key
  size                        approximate total bytes, for eviction (§5.7)
  gc                          time of the last full scan
```

- **Entry header:** magic, kind, layout version, the key itself, payload
  length, and an xxh3-64 checksum of the payload. Every kind shares one
  sectioned container and carries its own string, path and type tables,
  so no run ID reaches disk
  ([data-structures.md §3.20](data-structures.md#320-cache-entries-and-the-manifest)). A reader checks all of
  them. A mismatch, such as a truncated or corrupt file, counts as a miss,
  and the file is deleted.
- **Publish:** write to `$HD_CACHE/tmp/<random>`, close, rename to the
  final path. There is no `fsync`: a crash can lose a recent entry, and a
  torn one fails its checksum and is a miss. A cache entry is never the
  only copy of anything, so durability is not worth a sync per entry. On a platform where rename does not replace, an existing
  final file wins. Entries are immutable, and two writers of one key write
  the same bytes, so a lost race costs only the duplicate work.
- **No locks on reads.** Readers open and map files. Writers never modify
  a file in place.
- **Dedup across processes** is not coordinated: two processes that miss
  the same key both compute it. Module entries take milliseconds. So the
  `cache-contention` target bounds warm runs only: N concurrent warm
  checks cost at most 1.5x the CPU of one, and N cold checks of one
  package may cost up to N times one (open-questions.md, answered;
  Codex re-review N-C3). Lock files come only if a measurement of D2's
  expensive entries asks for them.
- **Write failures** (read-only or full disk) produce one warning and the
  run continues uncached. A cache never makes a check fail.
- **Large entries** (blobs over 64 KiB, D2's code) are memory-mapped.
  Small ones are read whole.
- **Per-program packs (lowering pass).** A program reads thousands of
  `code` entries per link and thousands of `cranelift` entries per
  precompile; one file each costs 15 to 30 µs, so 70 to 140 ms per link
  on one core at 10k lines. So after a build, `Link` writes a `codepack`
  entry holding every code entry of the program, and the Cranelift
  adapter a `clpack` holding every compiled function it served or
  produced. A pack's key is `H(kind, sorted member keys)`, a pure function
  of its content, so packs are shared like any entry. The **last link
  record** in `build/.hd/` names each program's last packs; it is the
  only place build history lives, and it never enters a key or the
  output bytes. The next build of the program maps those packs once and
  takes every member whose key is unchanged; a key a pack lacks is read
  as a separate entry, so a missing or evicted pack only costs speed.
  The Cranelift adapter serves `get` from the mapped `clpack` before
  falling back to single entries, and batches its `insert`s into the new
  pack. Separate `code` and `cranelift` entries are still written on a
  miss, so other programs and worktrees share them. Packs count toward
  the size cap and are evicted like any entry. S3 of spike 0c measures
  per-entry I/O on macOS and Linux.

### 5.5 The Stat Manifest And Change Detection

```rust
pub struct Manifest {            // build/.hd/manifest, one per package per worktree
    pub written_at: Timestamp,
    pub files: Vec<ManifestFile>, // sorted by path
}
pub struct ManifestFile {
    pub path: RelPath, pub size: u64, pub mtime_ns: i64, pub ctime_ns: i64, pub inode: u64,   // 88-byte record (§3.20.3)
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
3. **Trust** a record when size, mtime, change time (`ctime`) and inode
   are all equal, and both times are earlier than `written_at`. A file
   with a time at or after `written_at` is "racily clean" and is hashed
   anyway (git's rule).
   - **Why `ctime`** (Codex review, finding 9). An in-place rewrite of
     the same length, followed by restoring the old mtime, keeps size,
     inode and mtime. No API lets a user set `ctime`: every write and
     every `utimes` call sets it to the current time. So that edit
     changes `ctime`, and the file is hashed. Git's index records `ctime`
     for the same reason. On Windows, NTFS's change time plays the same
     role, read for a whole directory per call.
   - **`written_at` comes from the file system.** It is the mtime that
     the file system stamps on a probe file created at the start of the
     run, before any source is read. It therefore has the same clock and
     granularity as the files it is compared with. An edit made while the
     run reads files has a later time, and is hashed next run.
   - **Stat, read, stat.** When a file is read and hashed, its metadata is
     taken before and after the read. If they differ, the file changed
     while it was read: its record is not written, so the next run hashes
     it again.
4. **Hash** every other file.
5. **Where stat cannot decide.** A file system without a change time
   (FAT, some network mounts) gets no trusted records: every file is
   hashed every run. Hashing 350 KB costs well under a millisecond; the
   cost is opening the files. A clock set backwards can still fool the
   rule, as it fools git. `HD_CACHE_VERIFY=1` hashes every file, so CI
   never depends on stat.
6. **Write** the manifest atomically at the end of the run if anything
   changed. A concurrent run in the same worktree may overwrite it; that
   is safe, because every record is verified by stat, and a mismatch means
   re-hashing.
7. **Dependencies are not stat'ed.** A fetched version is read-only and
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
  writes the exact total back. On `hd check`, `hd test` and `hd run` the
  work is bounded to about 20 ms per run; a larger eviction continues in
  later runs or in `hd cache gc`. An agent's command does not wait on a
  full scan. A full scan also runs at least once a day
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
