# New Compiler Design: Cache

Part of the [compiler design](README.md).

## 5. Cache

### 5.1 Where Things Live

| Data | Location | Shared | Why |
| --- | --- | --- | --- |
| interface blobs, check results, TIR, coherence and init results, package results; D2's code | `$HD_CACHE/obj/` | per user, across worktrees and packages | keys are content hashes, so worktrees on one commit share everything |
| fetched dependency trees | `$HD_CACHE/pkg/` (spec today) | per user | [Cache](../../spec/cli/command-line.md#cache) |
| staging for atomic writes | `$HD_CACHE/tmp/` | per user | same file system as `obj/`, so rename is atomic |
| stat manifest with the last-run record (§5.5.1), last test record | `build/.hd/` of each package | per worktree | holds paths, mtimes and build history, which must not enter shared keys |
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
| `check` | one module and role, in sections: diagnostics with severities (layout 3 keeps warnings as warnings, so a cached warning no longer fails the CLI), init summary, row results, fact records, the read list (§5.3), the file's declaration table (`locs`), headers, and, only when the module has no error, its TIR with per-item TIR hashes and dependency lists (§4.13.11); role `test` also holds synthesized test items and registrations | `ModuleFinish` | output, `InitOrder`, D2, `hd check --tests`, `hd test` |
| `graph` | every package-wide part in one entry: each folder's stage-B header diagnostics ([resolution-and-interfaces.md §4.10.1](resolution-and-interfaces.md#4101-header-validation-stages)), each trait's overlap diagnostics, each folder's statement order and its diagnostics | `HeaderCheck`, `Coherence` and `InitOrder`, gathered at `PackageResult` | output, D2 |
| `pkgres` | the package's sorted diagnostics and summary counts | `PackageResult` | the warm fast path |
| `depfiles` | a fetched dependency's file list with content and api text hashes | first use of the dependency | every later run (§5.5) |
| `codepack` | the code entries (§13.8) of one program's instances whose items live in one folder, with an index by code key | `Link` | `Link` of any program that maps it |
| `clpack` | the compiled functions of one `codepack`, keyed by wasmtime's per-function cache key (§18.2), with an index | the `CacheStore` adapter of wasmtime's incremental compilation cache | the same adapter |
| `packhint` | the pack keys of the last link of one program, by any worktree | `Link` | `Link`, when this worktree's last-run record has none |
| `link`, `cwasm` | the program's Wasm, and its precompiled form per engine (§11.2) | D2 | D2 |

**Sections, not files (systems review, finding 1).** A file costs about
340 µs to publish and 60 to 120 µs to open and read on the owner's Mac
(the review's measurement, under load). That is more than the work
inside most small entries. So the unit of an entry is the unit of
invalidation, never a smaller one:

- `check`, `tir` and `locs` are one entry. All three are functions of
  `check_key(m)`: the TIR is checked from the same inputs, and the
  declaration table from `source_hash(m)`, which the key holds. A reader
  maps the entry and touches only the sections it needs. Other files call
  the TIR sections "the `tir` entry".
- **The headers section (walking skeleton, SK-1).** It holds the
  module's private signatures and private data and enum layouts, by
  stable path. The folder interface holds only public items, and on a
  `check` hit M1 does not run. D2 still needs every callee's result type
  and every data type's fields, so it reads them from this section.
- **An entry without TIR.** A module with errors, or one checked in the
  no-emit mode (type-checking.md §1.7), is published without TIR
  sections, and its header flag `HAS_TIR` is clear. A build that finds
  such an entry rechecks the module and publishes the full entry under
  the same key, replacing the first. This is the one case where a key's
  bytes change: both versions hold equal diagnostics, so a reader that
  needs only those is right with either, and verify mode compares the
  sections both hold.
- Coherence, header checks and init order are one `graph` entry per
  package run, keyed by the sorted keys of its parts (§5.3). A part whose
  result is empty stores nothing; its key in the list is enough. An edit
  that changes one part copies the others from the previous `graph`
  entry, which the last-run record names (§5.5.1).
- `code` and `cranelift` entries exist only as members of packs. No file
  holds one function.
- `iface` (one per folder), `link`, `cwasm` and the packs keep a file
  each.

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

coh_key(T)    = H("coh", toolchain_key, stable path of T, sorted head hashes of T's impls)
init_key(F)   = H("init", toolchain_key, folder path, sorted [(module path, init summary hash)])
graph_key     = H("graph", sorted hdr keys, sorted coh keys, sorted init keys)
pkgres_key    = H("pkgres", sorted role-keyed check, hdr, coh and init keys, folder graph hash,
                  manifest diagnostics hash, command mode)
fast_key      = H("fast", toolchain_key, package key, sorted [(path, source_hash)] of every
                  file, sorted dependency keys, command mode)
```

- **Interface reach, not import reach (M1 finding 5).** The intended
  `iface_key(F)` dependency set is `mentions(F)` from §4.11.3: folders
  named by F's public interface, not every folder reached by F's `use`s.
  M1's `deep_hash` currently folds all used folders and therefore
  over-invalidates. This is an open implementation gap; narrow the
  input when interface mention extraction lands.
- **TIR locations do not invalidate code (M1 finding 1).** A body's TIR
  hash excludes its trailing `syn` and `local_syn` columns. A header edit
  that only shifts later node indices must keep the body hash and reuse
  its code entry. M1 recomputed that code in this case, so the lookup and
  publication path still has an open invalidation bug even though the
  wire hash has the required boundary.

- **Code keys hold the callees' representation summaries (walking
  skeleton, SK-3).** A callee's A1 summary picks the symbol its caller
  relocates to and whether the caller casts the result. So the caller's
  code key holds it, through each callee's instance key after A1
  classification ([codegen.md §13.8](codegen.md#138-code-entries)). A
  change to a callee's summary then re-emits its callers, even when
  their own TIR is unchanged.
- **M4b gap 4.** M4b's `code_key` stops at the pipeline, instance, item
  TIR and an aggregate callee-representation hash. It does not yet carry
  the interface, layout, selected-impl, inline-summary, inlined-body or
  literal dependencies required by codegen.md §13.8. Code-cache hits are
  therefore provisional until that complete dependency record lands.
- **Dependency closures.** `closure(m)` is m's own folder plus every
  folder reachable from it through the folder graph's use edges. It is
  the transitive closure, not only the folders m names. `closure(F)` is
  the same for a folder. A role-`test` `check_key` uses `test_closure(m)`,
  which adds every folder that test code reaches, dev dependencies included. These are the bit
  sets the driver builds per context (scheduler.md §6.1), the same sets
  that filter the candidate directory. So each key lists every folder
  whose impls its check can see.
- **Argument-owned impls need no extra hash (Codex re-review N-A1).** A
  goal with an open trait argument reads impls owned only through a
  trait argument, from any folder in the asking context's closure
  ([trait-solver.md §3.2](trait-solver.md#32-owner-folders)). Such an
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
  mentions in deep hashes. Now any public API edit in it misses m's key.
  Agents add public items and change signatures often, so this is not
  rare (systems review, finding 3): adding a `pub fn` to a folder that
  most modules reach misses about 100 keys at 10k lines. Early cutoff by
  recorded reads (below) turns most of those misses back into reuse. The
  `recheck-precision` metric counts both
  ([testing-the-compiler.md §8.2](testing-the-compiler.md#82-incremental-soundness)).
- **Part keys.** `hdr_key`, `coh_key` and `init_key` name parts of the
  one `graph` entry; no file has them as its name. Coherence runs as one
  task over the sorted list of traits whose `coh_key` changed.
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
  holds it, so it reaches every `iface`, `check` and `fast_key` key. The
  package name is in every stable path, so a rename also changes every
  TIR hash, and with them the `prog_key` and code keys. A rename changes the
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
  Output resolves an anchor through the current file's `locs` section,
  keyed by the file's `source_hash`, which holds each declaration's
  start; it re-lexes that declaration to find the token, which costs
  microseconds and runs only for a printed diagnostic. Any file whose
  bytes changed is parsed in this run, so its `locs` section exists.
  Recomputed `iface` bytes are then equal under an equal key, which
  verify mode checks. rustc's incremental mode keeps spans relative to
  their item for the same reason.

#### 5.3.1 Early Cutoff By Recorded Reads

**Status: designed; enabled after slice 4 measures the hit rate**
(systems review, finding 3; the owner accepted the review's
recommendation). Until then each `check` entry writes its read list, and
slice 4 counts the reuses it would allow without acting on them.

`check_key(m)` names whole folders. So a public edit in a folder misses
the key of every module whose closure holds it, even modules that never
name the edited item. Early cutoff asks a second, finer question before
rechecking: did anything that m actually read change?

1. **The read list.** Each `check` entry stores, in a `reads` section,
   every input from outside m's own file that its check read:
   - `(stable path, per-item interface hash)` of every item it named or
     whose signature it used, from M1's headers and M2's bodies (the TIR
     already keeps these per body as `deps`);
   - `(folder path, name, item hash or ABSENT)` for every name lookup
     into another folder, so a lookup that found nothing is a read too,
     and adding that name later misses;
   - `(folder path, heads_hash)` for every folder holding an owner module
     whose impl table the solver probed;
   - the context's `ImplUniverseId` folder list, with a hash of each
     listed folder's `arg_impls` section (computed with the interface);
   - `(folder path, names hash)` for every folder whose whole namespace a
     diagnostic's suggestion scanned (§5.3, "Suggestions too").
   The checker reads other folders only through the interface readers,
   so the list is collected in one place, deduplicated and sorted.
2. **The previous key.** The last-run record (§5.5.1) maps each module
   path to the key of the entry it used last time.
3. **The test.** On a `check_key(m)` miss, when `source_hash(m)`, the
   toolchain, the package key and the role all equal those of the
   previous entry, open that entry and compare every read against the
   current interfaces. All equal: reuse its result, and record its key as
   m's entry key in the last-run record. One unequal: recheck m. No new
   entry is published for a reuse, so a public edit that reaches 100
   modules costs 100 opens, not 100 publishes.
4. **Downstream keys use the entry actually used.** `pkgres_key`,
   `init_key` and D2's `prog_key` take m's reused entry key or its
   content hashes, so they hit as before.

**Soundness.** Every read that can change m's result must be in the
list. Verify mode (§5.6) rechecks every reuse and compares the result
byte for byte. The key-completeness tests (testing-the-compiler.md §8.2 and §21.4)
gain one case per read kind above: an edit that changes that read must
make the test fail. This is rustc's red-green marking at module
granularity. It needs no query system and no daemon.

**Cost** (`ordinary-10k`, a public edit in a folder that every module
reaches; the review's Mac costs):

| Step | Units | Cost per unit | Total |
| --- | --- | --- | --- |
| recheck, today | about 100 modules | 5 to 10 ms of CPU | 0.5 to 1 CPU-s: 60 to 125 ms on 8 cores, about 1 s on one |
| early cutoff: open the previous entry | about 100 | 60 to 120 µs | 6 to 12 ms |
| early cutoff: compare reads | about 100 × 50 to 200 reads | a hash compare, under 0.1 µs | under 2 ms |
| recheck the modules that read the edited item | the few that name it | 5 to 10 ms | a few ms per module |
| `reads` section on disk | 100 entries | 20 to 24 bytes per read | 1 to 5 KB per entry |

### 5.4 Entry Format And Atomic Publish

```text
obj/
  LAYOUT                      "hd-obj 3 xxh3-128"
  <kind>/<2 hex>/<32 hex>     one file per key; the 2 hex digits name the shard
  stats                       256 approximate shard sizes, for eviction (§5.7)
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
- **Packs per program and folder (lowering pass; systems review,
  finding 1).** A program has thousands of code entries and thousands of
  compiled functions. One file each would cost about 3.2 s of publishes
  for a cold 10k-line build and 0.6 to 1.2 s of reads for a warm one, at
  the Mac's measured rates. So code is stored only in packs:
  - **Groups.** A program's instances are grouped by the folder that
    holds their item (std's folders included). `Link` writes one
    `codepack` per group, and the Cranelift adapter one `clpack` per
    group, holding the compiled functions of that group's members. A
    10k-line program has about 30 groups (20 of its own folders and
    about 10 of std's).
  - **Keys.** A pack's key is `H(kind, pipeline_hash, profile, sorted member keys)`, a pure
    function of its content, so equal groups are shared by every program
    and worktree.
  - **Finding packs.** The last-run record (§5.5.1) names, per program,
    the packs of its last link. A worktree without one reads the
    program's `packhint` entry, keyed by `H("packhint", toolchain_key,
    pipeline_hash, profile, package key, root description)`, which names the packs of
    that program's last link in any worktree. `Link` maps every named
    pack once, builds one in-memory index by code key, and takes every
    member whose key is still wanted. A wanted key that no named pack
    holds is emitted again; nothing is read as a separate file.
  - **A hint is not an answer.** `packhint` is the one kind whose
    content is not a function of its key: the last writer wins. Every
    member is checked by its own key before use, so a stale, foreign or
    evicted hint only costs speed. It holds no path.
  - **What an edit rewrites.** A body edit changes the members of one
    group, plus the groups of callers that inlined the body, so it
    publishes one or two new `codepack`s and `clpack`s. The other groups
    keep their keys.
  - The Cranelift adapter serves `get` from the mapped `clpack`s and
    batches its `insert`s into the new pack of each changed group.
  - Packs count toward the size cap and are evicted like any entry.
    Spike 0c's S3 measures per-entry I/O on macOS and Linux.
- **Print, then publish (systems review, finding 8).** Diagnostics are
  printed as soon as the last task that can add one finishes. Publishing
  runs on one I/O thread that drains a queue of finished entries. On a
  cold run it overlaps the checking of later modules. On a warm edit
  nothing is waiting in the queue when checking ends, so output comes
  first and the publishes drain after it, before the process exits.
  Output never waits for a publish. An agent that waits for exit still
  pays for them, which is why entries are few (§5.9).

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
     the file system stamps on a probe file, created before the run reads
     its first source file. It therefore has the same clock and
     granularity as the files it is compared with. An edit made while the
     run reads files has a later time, and is hashed next run. The probe
     is created only when the run is about to hash a file, which is the
     only time it can write a new record, so the no-edit fast path pays
     no create and unlink (about 0.4 ms on the Mac). Its name is random,
     so two runs in one worktree never share it.
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

#### 5.5.1 The Last-Run Record

(Systems review, finding 5.) Without it, a fast-key miss rebuilds
`pkgres` from every part: it opens every `iface` entry to learn its deep
hashes, every `check` entry for its diagnostics, and every coherence,
header and init part. That is about 200 opens at 10k lines, 12 to 25 ms
on the Mac, linear in the program and not in the edit. The `io-per-check`
target asks for reads proportional to what changed.

So the manifest file carries a second section, the **last-run record**,
written in the same atomic write as the stat records:

| Row | Holds |
| --- | --- |
| folder | `iface_key`, `deep_hash`, `heads_hash`, `api_hash` |
| module | `check_key`, the key of the entry it used (equal, or an older key reused by early cutoff, §5.3.1), its TIR content hash (codegen.md §11.3), and its diagnostics' row range in the previous `pkgres` entry |
| package | the `graph_key` and every part key in it; the `pkgres` key |
| program | `prog_key`, the `link` key, and the keys of its packs (§5.4) |

1. On a fast-key miss, the run recomputes keys bottom-up from the
   manifest and this record. A folder whose files kept their
   `api_text_hash` and whose dependencies kept their deep hashes keeps
   its `iface_key`, and its hashes come from the record without an open.
2. A module whose `check_key` is unchanged takes its diagnostics from
   the previous `pkgres` entry, one open for all of them, and none when
   the package had no diagnostics.
3. Only parts whose key changed are read or computed.
4. A missing, unreadable or stale record (a toolchain change, another
   package root) falls back to the full path. The record holds paths and
   build history, so it stays in the worktree, like the manifest. It
   never enters a key or the output.

At 10k lines the record is about 8 KB: 100 module rows of about 48
bytes, 20 folder rows of 64, and the program rows.

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
- **Approximate LRU, per shard (systems review, finding 6; the owner
  accepted the review's recommendation).** Exact LRU needs a full scan,
  and a full scan of a 10 GB cache does not fit in a user command. So
  eviction follows ccache: the cache is 256 shards, named by the first
  two hex digits of a key, across every kind directory. Each shard has
  its own budget, the cap divided by 256 (about 40 MB at 10 GB), and LRU
  holds within a shard. The cap therefore holds to within one shard's
  budget.
- **Size accounting.** `obj/stats` holds 256 approximate shard sizes.
  Each run adds the bytes it wrote per shard, once at the end of the
  run, under a short advisory file lock: one read and one write of a
  2 KB file.
- **Automatic eviction.** A run that pushes a shard over its budget
  cleans that shard after its output is flushed: it lists
  `<kind>/<shard>/` for every kind, sorts by mtime, deletes the oldest
  until the shard is at most 90% of its budget, and writes the shard's
  exact size back. A run cleans shards until about 20 ms have passed;
  shards still over budget are cleaned by the next run. An agent's
  command never waits on a full scan.
- **Drift.** Each run also rescans one shard in round-robin order (its
  number is in `obj/stats`) and corrects its size. So every shard is
  corrected every 256 runs, with no daily full scan.
- **`hd cache gc`** cleans every shard now, with no time bound, and
  prints the bytes before and after.
- **The cap** comes from `HD_CACHE_MAX_SIZE` (bytes, with `K`, `M`, `G`
  suffixes). Whether a manifest key or a user config file can also set it
  is a CLI spec detail.
- **Safety.** Eviction deletes only `obj/<kind>/` files whose names parse
  as keys, never follows a symbolic link, and never touches `pkg/`.
- A process holding a mapped entry keeps working if another process
  deletes it, because unlinking a mapped file is safe on Unix. On Windows,
  eviction skips files it cannot delete.

**Cost** (a full 10 GB cache; the review's Mac costs):

| Step | Files per file-per-entry cache (before packs) | With packs (§5.2, §5.4) | Cost per unit | Per run |
| --- | --- | --- | --- | --- |
| files in the cache | 1 to 2 million of 5 to 10 KB | about 50,000 to 100,000 (packs are 100 KB to a few MB) | | |
| full scan, the old design | 1 to 2 million | | 4.6 to 8.5 µs | 5 to 17 s: never fits 20 ms |
| scan one shard | 4,000 to 8,000 | 200 to 400 | 4.6 to 8.5 µs | 1 to 3.4 ms with packs |
| delete 10% of a shard | 400 to 800 | 20 to 40 | 48 to 73 µs | 1 to 3 ms with packs |
| update `obj/stats` | | 1 | about 0.5 ms | 0.5 ms |

Inflow stays below what eviction can clean: a cold build of a 10k-line
program writes about 64 files and a warm edit 3 to 6 (§5.9), against
three to five shard cleanings per 20 ms.

### 5.8 The Browser `CacheStore`

```rust
pub trait CacheStore: Sync {
    fn get(&self, kind: EntryKind, key: &Hash128) -> Option<EntryBytes>;  // synchronous
    fn put(&self, kind: EntryKind, key: &Hash128, bytes: &[u8]);
    fn touch(&self, kind: EntryKind, key: &Hash128);
}
pub struct MemoryStore { entries: Mutex<HashMap<(EntryKind, Hash128), Arc<[u8]>>>, new: Mutex<Vec<..>> }
```

- The core never awaits storage. The JS host loads the playground's
  entries from IndexedDB into the `MemoryStore` once, when the compiler
  worker starts, not before each run: a 50 MB store would cost 100 to
  500 ms of IndexedDB reads per run (systems review). After each run it
  drains only the new entries and writes them back in one transaction.
- Keys are the same as native. `toolchain_key` keeps a new compiler build
  from reading old entries.
- The IndexedDB store keeps a last-use time per entry and evicts the
  oldest past a small browser cap, such as 50 MB, when it loads.
- Std is embedded, and playground programs are small, so a cold check is
  the normal browser case. The store mainly speeds up "Run" after "Check".

### 5.9 What Cache I/O Costs

(Systems review, findings 1, 5 and 8.) Counts are for `ordinary-10k`:
100 modules in 20 folders, 40 to 80 traits with impls, one program of
about 4,800 instances and 4,700 functions, about 30 folder groups. Costs
are the review's Mac measurements under load: publish about 340 µs, open
and read 60 to 120 µs, `stat` about 2 µs. A Linux runner is likely 5 to
20 times cheaper; slice 4 measures both.

| Run | Before (file per entry) | Now | Now, at the Mac's rates |
| --- | --- | --- | --- |
| cold `hd check`: publishes | about 400: 100 `check`, 100 `tir`, 100 `locs`, 20 `iface`, 20 `hdr`, 20 `init`, 40 to 80 `coh`, `pkgres` | about 123: 100 `check`, 20 `iface`, `graph`, `pkgres`, the manifest | about 42 ms of I/O-thread time, overlapping the check; was about 140 ms |
| cold `hd build`: added publishes | about 9,500: 4,800 `code`, 4,700 `cranelift`, `link`, `cwasm` | about 64: 30 `codepack`, 30 `clpack`, `link`, `cwasm` with its sidecar, `packhint` | about 22 ms; was about 3.2 s |
| warm `hd check`, private body edit: opens | about 200: every `iface`, `check`, `coh`, `hdr` | 2 or 3: the manifest with its record, the edited file, and the previous `pkgres` when it has diagnostics | 0.2 to 0.4 ms; was 12 to 25 ms |
| warm `hd check`, private body edit: publishes | 4 or 5, before output | 3, after output: `check`, `pkgres`, the manifest | about 1 ms, after output |
| warm `hd check`, public edit in a folder every module reaches | about 200 opens; 100 rechecks; about 300 publishes | without early cutoff: 2 opens, 100 rechecks, about 103 publishes; with it (§5.3.1): about 100 opens, a few rechecks, about 5 publishes | without: about 35 ms of publishes plus the rechecks; with: 6 to 12 ms of opens |
| warm `hd run` after a body edit: reads | about 9,500 entries | the manifest and about 60 packs, each mapped once | 4 to 7 ms; was 0.6 to 1.2 s |
| warm `hd run` after a body edit: publishes | the changed `code` and `cranelift` entries, `link`, `cwasm` | 1 or 2 of each pack kind, `link`, `cwasm`, `packhint`, `check`, `pkgres`, the manifest | about 3 ms, after the program starts |
| warm run with no edit | the manifest and `pkgres` | the same | under 0.3 ms |

A lookup that misses is a failed `open`. It is cheaper than a read but
not measured; a cold check makes about 123 of them.

**`edit-latency` estimate, one-function edit** (one core, the Mac's
rates; the check times are estimates until slice 3 measures them):

| Step | `ordinary-10k` (a 100-line module) | `one-file-10k` (one 10,000-line module) |
| --- | --- | --- |
| process start, manifest and record | 3 to 8 ms | 3 to 8 ms |
| stat 100 files, read and hash the edited one | 0.3 ms | 0.5 ms |
| keys from the record | under 1 ms | under 1 ms |
| parse; M1, M2 and M3 of the module, with the per-run solver memo warm-up | 2 to 10 ms | 80 to 250 ms on one core; about 15 to 45 ms on 8 |
| TIR remap | 0.02 ms | 2 ms |
| print | under 1 ms | under 1 ms |
| **time to output** | **about 6 to 20 ms** | **about 90 to 260 ms on one core; 25 to 60 ms on 8** |
| publishes after output | about 1 ms | about 3 ms (a 2 MB `check` entry) |

So `ordinary-10k` meets the 50 ms p50 with room on one core.
`one-file-10k` meets it only on several cores. On one core it misses the
p50 and approaches the 200 ms p95. M1's walk and M3's sweep are a small
share of that. The cost is M2 over every body of the module, because the
first release rechecks a whole module (type-checking.md §1.7). The next
lever is per-body reuse inside the module's entry
([checking-and-tir.md §4.13.1](checking-and-tir.md#4131-task-structure-per-module)),
which the first release does not include.
