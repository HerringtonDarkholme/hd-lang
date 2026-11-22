# Warm Check Cost

Status: design exploration, 2026-11-21; nothing here is decided or in the
specification. Tracked by #1801 draft 89 (owner decision).

The question: a warm `hd check` should cost what changed, not the size of
the package. How should hd get there, and what should it accept as the
floor?

## Contents

- [The domain](#the-domain)
- [Problem, and what hd has today](#problem-and-what-hd-has-today)
- [Measurements](#measurements)
- [Survey](#survey)
- [Option 1: content-checked racy files, O(stat) accepted](#option-1-content-checked-racy-files-ostat-accepted)
- [Option 2: per-module rows in the last-run record](#option-2-per-module-rows-in-the-last-run-record)
- [Option 3: reverse-edge walk from the changed files](#option-3-reverse-edge-walk-from-the-changed-files)
- [Option 4: a change journal instead of the listing](#option-4-a-change-journal-instead-of-the-listing)
- [Option 5: a resident checker](#option-5-a-resident-checker)
- [Comparison](#comparison)
- [Recommendation](#recommendation)
- [Questions for the owner](#questions-for-the-owner)
- [Sources](#sources)

## The domain

An incremental checker keeps results from the last run and must decide,
on the next run, which of them still hold. That decision has two halves.

1. **Change detection.** Which inputs changed since the last run? The
   inputs are files. Reading and hashing every file answers this
   exactly but costs a read per file. Comparing each file's metadata
   (size, times, inode) with the last run's costs one `stat` per file.
   A file-system watcher or journal answers with the changed paths
   alone.
2. **Change propagation.** Which results depend on the changed inputs?
   A build system records, per result, the inputs it read and their
   hashes (a *trace*). A result is reused when every recorded input
   still has its recorded hash. *Early cutoff* stops the walk when a
   recomputed value equals its old value: an edited function body
   changes the module's text but not its interface, so the modules
   that only read the interface keep their results.

The hard parts:

- **Racy timestamps.** A file written in the same timestamp tick as the
  last run's `stat` can keep every metadata field while its bytes
  change. git calls such an entry *racily clean*. Every stat-based
  design needs a rule for files that are too new to trust.
- **Per-file cost hides in more than `stat`.** A run can stat each file
  cheaply and still do per-module work afterwards: recomputing every
  module's key, looking up every module's result, decoding every
  interface. A design that stops reading files but still visits every
  module stays O(files).
- **Watchers lose events.** A watcher that restarts, overflows or runs
  on a network mount must say so, and the reader must then fall back
  to a full scan.

A worked example. A package has 2,000 modules. An agent edits one
function body in `m001.hd` and runs `hd check`. Ideal work: notice that
`m001.hd` changed, re-check `m001`, see that its interface hash is
unchanged, and replay every other module's diagnostics. Anything that
reads 1,999 other results, decodes 1,999 interfaces or hashes 1,999
keys is O(files).

## Problem, and what hd has today

hd's cache design is in `crates/hd_cache/DESIGN.md` §5.5 ("The Stat
Manifest And Change Detection"). The owner's architecture fixes two
points: incremental checking uses the per-module on-disk cache, "with no
salsa and no daemon" (`docs/compiler/goals.md`, decided architecture),
and the `io-per-check` goal reads "reads proportional to what changed;
stats proportional to the source file count".

Today, per run:

- **Listing and stamps.** Each source directory is listed and each file
  stat'ed once. A file's stamp hashes its size, modification and change
  times, inode and device (`hd_command::disk::stamp`). A file whose times
  are within `SETTLE` (2 s) of the listing's start gets no stamp and is
  read.
- **The last-run record.** Per source set, one `depfiles` entry holds
  each stamped file's path, stamp and skim (the hashes keys read). A
  file whose stamp matches takes its skim from the record; keys are
  recomputed from skims as hashes of hashes.
- **The package result** (#1510). When every listed file has a stamp,
  the run hashes every path and stamp into one key and replays the
  stored report: one lookup, no per-module work.
- **Otherwise** every module computes its check key, looks up its
  `check` entry, reads it whole, and the folder interfaces are decoded,
  whatever changed.

Use cases every option is judged on:

1. **No change.** `hd check` twice in a row.
2. **Just saved, no change.** An agent writes a file with the same
   bytes, or an editor saves, then `hd check` within 2 s.
3. **One body edit.** The worked example above.
4. **Branch switch.** `git checkout` rewrites 200 files, then `hd
   check`.
5. **Large package.** 10,000 files, no change: the 50 ms
   `edit-latency` target (`goals.md`) applies.

## Measurements

Release `hd` at 379cdb2fa, Apple silicon, APFS, generated packages from
`bench/metrics/lib/gen.ts` (about 130 lines a module), `HD_CACHE` warm,
`hyperfine` means. "Settled" means every file is more than 2 s old.

| Run | 1 file | 500 files | 2,000 files | Per file |
| --- | --- | --- | --- | --- |
| `hd help` (process start) | 2.5 ms | | | |
| no change, settled | 4.7 ms | 7.5 ms | 15.2 ms | 5.1 µs |
| no change, one file saved < 2 s ago | | 45.6 ms | 150.8 ms | 70 µs |
| one body edit | | 43.1 ms | 136.9 ms | 62 µs |

Work counters (`HD_WORK`, `--jobs 1`) at 2,000 files: the settled
no-change run allocates 8.0 MB and hashes 55 values; the one-edit run
allocates 706 MB and hashes 865,262 values; a no-change run within 2 s
of a save allocates 696 MB.

Where a settled no-change run spends its time (samples, `--jobs 1`,
per run; sampling adds about 40%):

| Phase | 500 files | 2,000 files |
| --- | --- | --- |
| directory walk, `stat`, path sort | 1.90 ms | 8.35 ms |
| module discovery from the listing | 1.10 ms | 4.27 ms |
| package-result key hashing | 0.71 ms | 0.92 ms |
| manifest parse | 0.27 ms | 0.46 ms |
| cache lookups | 0.21 ms | 0.23 ms |
| store close | 0.31 ms | 0.48 ms |
| process start (not sampled) | 2.5 ms | 2.5 ms |

Where a one-edit run at 2,000 files spends its 138 ms of samples:
reading every module's `check` entry from the store, 39.2 ms; handling
those entries (copy, join, decode), 37.8 ms; decoding folder
interfaces, 23.7 ms; the edited module's own check and the rest, 19.8 ms;
walk and stat, 6.8 ms; module discovery, 5.4 ms; package result, 2.2 ms.

Floors on this machine, for 2,016 files: `find` walks them in about
2.7 ms beyond its own start (1.35 µs a file); `os.stat` from Python
takes 1.9 µs a file; open and read take 35 to 40 µs a file.

What this says:

- **No change is already near the stat floor**: 5.1 µs a file, of which
  the walk is about 3 µs (twice `find`) and module discovery 2 µs, pure
  CPU recomputed from an unchanged listing.
- **The O(files) that hurts is the small change**: 62 µs a file. Its
  cost is not `stat`. It is reading, copying and decoding every
  module's `check` entry and every folder interface, though one module
  changed.
- **The 2 s window turns a save into a full run.** One file younger than
  2 s takes the package-result fast path away, so an agent's
  write-then-check loop pays the small-change cost even when the bytes
  did not change.
- **Reading every file is not an option on this machine**: 35 to 40 µs a
  file is 70 to 80 ms at 2,000 files.

## Survey

| System | Change detection | Racy timestamps | Propagation and cutoff | Source |
| --- | --- | --- | --- | --- |
| git index | `lstat` per path, compared with cached stat fields | a path whose `st_mtime` is not older than the index file's own mtime is racily clean: its contents are compared; on rewrite its cached size is zeroed | none (status only) | racy-git.adoc |
| git fsmonitor | a built-in daemon reports changed paths; refuses network mounts by default | as above for the paths it reports | none | git-fsmonitor--daemon, core.fsmonitor |
| Go build cache | every source file read and SHA-256 hashed into the action ID on every build; the file-hash memo lasts one process | module index only: a directory with a file newer than 2 s is not cached (`modTimeCutoff`) | action IDs, no early cutoff below a package | Go 1.27.3 `cmd/go/internal/cache/hash.go` `FileHash`; `work/exec.go:429`; `modindex/read.go:78-110` |
| Zig cache manifest | per input: size, inode, mtime, content hash | a file whose mtime is not older than a `timestamp` file Zig writes in its manifest directory is recorded with mtime and inode 0, so it is hashed next time | manifest hit or miss per artifact | Zig 0.17.1 `lib/std/Build/Cache.zig`, `isProblematicTimestamp` 1644-1685, `setStatChanged` 575-588 |
| TypeScript `tsc -b` / `tsbuildinfo` | per input: mtime against the buildinfo's mtime; if newer, the text is read and its hash compared with the recorded *version* | the content compare makes a touched file cheap | per-file *signature* (hash of the emitted `.d.ts`): only files whose signature changed propagate to their referencers | tsbuildPublic.ts 1613-1660; builderState.ts 590-620 |
| Swift driver | build record per input: mtime and hash; equal mtime, or equal hash with `useHashes`, skips the input | content hash fallback | fine-grained provides/depends: top-level, nominal, member, dynamic-lookup | FirstWaveComputer.swift 351-380; DependencyAnalysis.md |
| rustc incremental | inputs are read by `eval_always` queries, which re-run in every session | n/a | red-green marking: `try_mark_green` reuses a node when its dependencies are green; fingerprints compare results | incremental-compilation-in-detail.md 94-200, 430-445; graph.rs 888 |
| Salsa (rust-analyzer) | the host sets inputs; each set bumps a revision | n/a (in memory) | per-query dependencies and revisions; *backdating* when a recomputed value is equal; *durability* skips walks for queries that read only stable inputs | algorithm.md 6-80; backdate.md; durability.rs |
| Bazel Skyframe | bottom-up invalidation needs `stat()` on every input of the last build; `--watchfs` uses the OS watch service instead | n/a | *change pruning*: an invalidated node whose new value equals the old resurrects its dependents | skyframe.mdx 95-120; `--watchfs`; LocalDiffAwareness.java 38-45 |
| Buck2 DICE | a daemon with a file watcher (Watchman, EdenFS, `notify`, or a hash crawler); a sync reports `FreshInstance` when it lost track | n/a | reverse dependencies give O(invalidated subset) traversal and O(changed subset) recomputation | incrementality.md; file_watcher.rs 37-160; daemon.md |
| Ninja | mtime per output in `.ninja_log` | n/a | `restat`: an output the command left unchanged is treated as never needing a build, pruning its dependents | manual.asciidoc 1111-1115 |
| Build Systems à la Carte (Shake) | traces | n/a | verifying traces (`verifyVT`: key, dependency hashes, value hash) and constructive traces (store the value) | Trace.hs 27-80 |
| Watchman | a daemon; `since` queries return changes after a clock; a restarted daemon yields a *fresh instance* | cookies: a query waits until a file it created is seen, so earlier writes are observed | none | file-query.md 60-85; cookies.md |
| macOS FSEvents | a persistent per-volume event database; a client stores an event ID and the volume UUID and replays events since it, with no process left running | events carry `MustScanSubDirs`, `UserDropped`, `KernelDropped`, `EventIdsWrapped` when the history cannot be trusted | none | SDK header `FSEvents.h` 120-185, 395-422 |

Takeaways:

1. **Every stat-based system falls back to the content, not to a full
   rebuild.** git, Zig, TypeScript and Swift all re-read a suspect file
   and compare its hash with the recorded one. hd's 2 s window instead
   turns off the whole-package fast path. hd and Go are the outliers
   with a wall-clock window; Go needs no racy rule for sources because
   it hashes every file on every build.
2. **The systems that reach O(change) all hold a change list from
   outside the scan**: Salsa from its host, Buck2 and Bazel
   `--watchfs` from a watcher, git fsmonitor from a daemon. The systems
   without one (git, Zig, TypeScript, Swift, Ninja, Bazel without
   `--watchfs`) stat every input and accept O(files) at a small
   constant.
3. **Early cutoff is universal and per result**: Bazel change pruning,
   Salsa backdating, Ninja `restat`, TypeScript signatures, rustc
   fingerprints. What differs is whether unchanged results are visited
   at all (Buck2, Salsa: no; Bazel bottom-up, TypeScript: yes, cheaply).

## Option 1: content-checked racy files, O(stat) accepted

The radical simplification: accept one `stat` per file as the floor, as
`io-per-check` already does, and fix only the racy-file rule.

- A file the listing finds racy is read and its content hash compared
  with the record's row. Equal: it reuses the recorded skim, and the
  package-result key takes its content hash in place of its stamp, so
  the fast path still applies. Unequal: it is an edit.
- The racy test compares the file's times with a reference the file
  system stamped, as Zig does, instead of the host clock minus 2 s: the
  record's own write time, or a `timestamp` file in the cache, when the
  cache and the sources share a device; the 2 s host-clock window
  otherwise.
- The walk and module discovery keep their per-file cost.

Per run, n files, k changed, r racy: n stats + (k + r) reads and hashes,
then the checker's work for k. Use case 2 drops from 150.8 ms to about
15 ms plus one read at 2,000 files. Use case 3 is unchanged (137 ms).

Cost to build: about 120 lines (record rows gain a content hash; the
stamp step gains the reference; the package-result key takes either).
It also closes #1801 draft 44.

Risks: the reference file and the sources must share a clock; on a
network file system whose server stamps both, Zig's rule holds, and a
different device falls back to today's window. Editors that rewrite a
file with equal bytes are the case this option makes cheap.

## Option 2: per-module rows in the last-run record

A verifying trace per module (à la Carte `verifyVT`, TypeScript's
`tsbuildinfo` file infos, Swift's build record): the last-run record
holds, for every module, what a later run needs from that module when
it did not change.

- Each row: path, stamp, content hash, skim, the module's check key,
  its diagnostics, and the facts the whole-program steps read today
  from the `check` entry (has errors, opaque results, `@inline(.Always)`
  items, init body, generic edges, items hash, entry rows).
- A run recomputes every module's check key from the skims, as it does
  today (hashes of hashes, no text). A module whose key equals its row's
  key takes the row; its `check` entry is not read and its folder
  interface is not decoded. Only modules whose key changed, through
  their own text or a changed deep hash in their closure, run.
- The package result becomes this record's sum: a run with no changed
  key replays all rows; a run with k changed keys re-checks k modules
  and rewrites k rows.
- Includes Option 1's racy rule, which is what makes use case 2 hit.

Per run: n × (stat + key hash + row read, about 100 to 300 bytes) +
the checker's work for the modules whose key changed. Estimate, not
measured: use case 3 at 2,000 files from 137 ms to 25 to 40 ms (walk
and discovery 12 ms, rows a few ms, the edited module's check).

Cost to build: about 400 to 600 lines; it deletes the hit path that
reads whole `check` entries for whole-program facts.

Risks: a fact a row lacks is a fact a warm run gets wrong, which no
crash reveals. This option needs #1801 draft 79's edit-script test
(every edit kind, incremental against clean) before it lands. Rows sit
in a `depfiles` entry keyed by the source set, beside today's skim
record, and name `check` entries by their content-addressed keys, so
the store is unchanged.

## Option 3: reverse-edge walk from the changed files

Top-down dirtying, as Buck2's DICE and Salsa propagate: unchanged
modules are not visited at all.

- The record also stores reverse edges: for each folder, the modules
  whose check key includes its deep hash.
- A run lists and stats to find the k changed files, re-skims them,
  recomputes their folders' deep hashes, and walks the reverse edges.
  A deep hash that did not change stops the walk (early cutoff). Rows
  of modules the walk never reaches are replayed unread, in bulk.

Per run: n stats + O(k + reached dependents). Compared with Option 2 it
saves the per-module key hash and row decode, about 1 to 2 µs a file by
the measurements; the stat and discovery cost stays.

Cost to build: about 700 lines (the edge index, its maintenance on
folder moves and new files, the walk).

Risks: a missing reverse edge is a stale result. The edges must be
written by the same code that computes keys, or derived from it.

## Option 4: a change journal instead of the listing

Read the changed paths from the operating system's journal, with no
process left running.

- macOS: the record stores the FSEvents event ID and volume UUID; the
  next run replays events since that ID for the package directory
  (`FSEventStreamCreateRelativeToDevice`, `FSEventStreamFlushSync`).
  Any of `MustScanSubDirs`, `UserDropped`, `KernelDropped`,
  `EventIdsWrapped`, or a different volume UUID, means a full listing.
- Other systems fall back to the listing. Linux has no persistent
  journal: inotify and its users (Bazel's `LocalDiffAwareness`, git's
  fsmonitor daemon) run as long-lived watchers.
- Combined with Option 2 or 3 for propagation.

Per run on macOS: the journal query (not measured; a probe must come
first) + O(k); elsewhere, as Option 2 or 3.

Cost to build: about 400 lines of platform code, behind the same
`SourceSet` listing.

Risks: events arrive with latency, so a run must flush before it trusts
the journal; edits on a network mount are not seen (git's fsmonitor
refuses network mounts by default); one platform only.

## Option 5: a resident checker

A long-lived process holds the module table, skims, interfaces and
results in memory, fed by a watcher or by an editor over LSP, as Buck2's
daemon, Bazel's server with `--watchfs`, git's fsmonitor daemon and
rust-analyzer do. `hd check` becomes a request to it.

Per run: about 1 ms of IPC + O(k). Process start (2.5 ms) and the
listing go away.

Cost to build: thousands of lines: lifecycle, one daemon per checkout,
memory bounds, stale-state recovery (Watchman's fresh instance and
cookies show what is needed), plus the browser playground, which has no
processes.

This reopens the owner's "no salsa and no daemon" decision. The new
evidence is only arithmetic: at the measured 4.7 ms + 5.1 µs a file, a
10,000-file package with no change takes about 56 ms, just above the
50 ms `edit-latency` target. Packages that size are rare today; the
generated 10k-line package has 77 files.

## Comparison

| | 1. Racy by content | 2. Module rows | 3. Reverse walk | 4. Journal | 5. Resident |
| --- | --- | --- | --- | --- | --- |
| No change, cost | n stats | n stats + n rows | n stats | journal query | IPC |
| One edit, cost | n stats + n entry reads + n iface decodes | n stats + n rows + work(k) | n stats + work(k) | work(k) on macOS | work(k) |
| Use case 2 at 2,000 files | about 15 ms | about 15 ms | about 15 ms | small | small |
| Use case 3 at 2,000 files | 137 ms | 25 to 40 ms (estimate) | 20 to 35 ms (estimate) | 15 to 30 ms (estimate, macOS) | small |
| Use case 4 (200 files rewritten) | 200 reads | 200 reads + work | 200 reads + work | 200 reads + work | 200 events + work |
| Use case 5, no change | about 56 ms | about 60 ms | about 56 ms | small on macOS | small |
| Lines | about 120 | about 400 to 600 | about 700 | about 400 | thousands |
| Correctness risk | low: a content compare | a missing fact in a row | a missing reverse edge | dropped or late events | stale daemon state |
| Clock and network file systems | uses the file system's own clock where it can | as 1 | as 1 | not on network mounts | as 4 |
| Fits the content-addressed cache | yes | yes, rows name entry keys | yes | yes | yes, as its backing store |
| Owner decisions touched | none | none | none | none | "no daemon" |
| Playground (one thread, no files) | n/a | yes | yes | n/a | no |

## Recommendation

**Recommendation:** Option 1 now, then Option 2; keep `stat` per file as
the floor.

- Option 1 fixes the case agents hit most: a file written, then checked
  within 2 s, which today costs as much as an edit (150.8 ms against
  15.2 ms at 2,000 files). It is small and follows git, Zig and
  TypeScript.
- Option 2 removes the cost that is actually O(files) today: reading
  and decoding every module's entry on a small change, 62 µs a file. It
  keeps the stat floor that `io-per-check` already accepts, and needs no
  platform code.
- Both stay inside the decided architecture: the on-disk cache, no
  daemon.
- Lower the remaining constants under Option 2's work: list directories
  in parallel, and derive the module table from the listing's hash so
  an unchanged listing skips discovery (4.3 ms at 2,000 files).

What this gives up: O(change) for the no-change case. A 10,000-file
package stays near 56 ms. The next best option is Option 4 on top of
Option 2, if packages that large appear; it keeps the no-daemon
decision on macOS and falls back to Option 2 elsewhere. Option 3's walk
saves 1 to 2 µs a file over Option 2, which the stat floor (1.4 µs a
file) makes hard to justify.

## Questions for the owner

1. **Is one `stat` per file the accepted floor for a warm check?**
   Options: yes, as `io-per-check` says (Options 1 and 2); no, use the
   journal where one exists (Option 4); no, a resident checker (Option
   5). Recommended: yes. Example: a 2,000-file package with no change
   costs about 15 ms; with a journal, a few ms on macOS.
2. **Which clock judges a racy file?** Options: the host clock minus
   2 s (today); a reference the file system stamps, the record's write
   time or a cache `timestamp` file, as git and Zig do, with today's
   window when the devices differ. Recommended: the file-system
   reference. Example: either way a file saved just before the check is
   read and compared by content; the reference rule also stays right
   when the host clock and the file server's clock disagree.
3. **May the last-run record hold each module's diagnostics and
   whole-program facts (Option 2)?** Options: yes, after draft 79's
   edit-script test lands; no, keep reading `check` entries. Recommended:
   yes, after the test. Example: an `@inline(.Always)` flag in the row
   lets a warm run skip the module's entry entirely.
4. **Does "no salsa and no daemon" stand?** Options: stands; revisit
   only for packages above 5,000 files. Recommended: stands.

## Sources

Read at the commits named; clones and files read on 2026-11-21.

- git `Documentation/technical/racy-git.adoc`, lines 23-104,
  <https://github.com/git/git/blob/f7de81456940dd6f3274e461149bba12965de609/Documentation/technical/racy-git.adoc>
- git `Documentation/git-fsmonitor--daemon.adoc`, lines 1-30
  and 73-89, <https://github.com/git/git/blob/f7de81456940dd6f3274e461149bba12965de609/Documentation/git-fsmonitor--daemon.adoc>
- git `Documentation/config/core.adoc`, lines 64-85,
  <https://github.com/git/git/blob/f7de81456940dd6f3274e461149bba12965de609/Documentation/config/core.adoc>
- Go 1.27.3 sources as installed (`go env GOROOT`):
  `src/cmd/go/internal/cache/hash.go` lines 140-192,
  `src/cmd/go/internal/work/exec.go` line 429,
  `src/cmd/go/internal/work/buildid.go` lines 389-395,
  `src/cmd/go/internal/modindex/read.go` lines 78-110.
- Zig 0.17.1 `lib/std/Build/Cache.zig` as installed (`zig env`), lines
  560-588 and 1644-1685.
- TypeScript `src/compiler/tsbuildPublic.ts` lines
  1613-1660, <https://github.com/microsoft/TypeScript/blob/cc2610fd347127c28609b4fca6fa9d55ee5063c6/src/compiler/tsbuildPublic.ts>;
  `src/compiler/builderState.ts` lines 357-410 and
  560-620, <https://github.com/microsoft/TypeScript/blob/cc2610fd347127c28609b4fca6fa9d55ee5063c6/src/compiler/builderState.ts>;
  `src/compiler/watch.ts` line 787 (a file's version is its text's hash).
- swift-driver `Sources/SwiftDriver/IncrementalCompilation/FirstWaveComputer.swift`
  lines 218-245 and 351-380, <https://github.com/swiftlang/swift-driver/blob/053868d430deb3862c403a430c745bf5a0dd3f0c/Sources/SwiftDriver/IncrementalCompilation/FirstWaveComputer.swift>
- swift `docs/DependencyAnalysis.md` lines 1-40,
  <https://github.com/swiftlang/swift/blob/9a9c2e9ae0d42a5711d14ac20353bff6a698ef0a/docs/DependencyAnalysis.md>
- rustc-dev-guide `src/queries/incremental-compilation-in-detail.md`
  lines 94-200 and 430-458, <https://github.com/rust-lang/rustc-dev-guide/blob/d946d7402d269990f3b64baabaf6592409791c11/src/queries/incremental-compilation-in-detail.md>
- rust `compiler/rustc_middle/src/dep_graph/graph.rs`
  lines 888-940, <https://github.com/rust-lang/rust/blob/342d74463af7dd3018ef6aa04dd5500c2deb5bd1/compiler/rustc_middle/src/dep_graph/graph.rs>
- salsa `book/src/reference/algorithm.md` lines 6-80,
  <https://github.com/salsa-rs/salsa/blob/496752ea3879af8cf3d71d1e28d76447a0cf6482/book/src/reference/algorithm.md>;
  `book/src/plumbing/terminology/backdate.md`;
  `src/durability.rs` lines 1-20.
- bazel `docs/reference/skyframe.mdx` lines 95-120,
  <https://github.com/bazelbuild/bazel/blob/227605fd2aecce4770c2f36053130aefb101406b/docs/reference/skyframe.mdx>;
  `docs/versions/9.2.0/reference/command-line-reference.mdx`
  lines 1855-1857;
  `src/main/java/com/google/devtools/build/lib/skyframe/LocalDiffAwareness.java`
  lines 38-45.
- buck2 `dice/dice/docs/incrementality.md`,
  <https://github.com/facebook/buck2/blob/907b7c7494dc4cc1ecc4b87141d890363632ba19/dice/dice/docs/incrementality.md>;
  `app/buck2_file_watcher/src/file_watcher.rs` lines
  37-160; `docs/concepts/daemon.md` lines 8-20.
- ninja `doc/manual.asciidoc` lines 1111-1115,
  <https://github.com/ninja-build/ninja/blob/a00b184d0cc3658ce2f5d2a81c7d86569d950b57/doc/manual.asciidoc>
- snowleopard/build `src/Build/Trace.hs` lines 27-80, the
  code of "Build Systems à la Carte" (ICFP 2018),
  <https://github.com/snowleopard/build/blob/43b18b9a362d7d27b64679ea4122e4b8c5dfedd9/src/Build/Trace.hs>
- watchman `website/docs/file-query.md` lines 60-85;
  `website/docs/cookies.md` lines 1-35,
  <https://github.com/facebook/watchman/tree/be24d697fcd9bb01923ca698815c52a91a18e094/website/docs>
- macOS SDK `FSEvents.framework/Headers/FSEvents.h` (Command Line Tools),
  lines 95-185 and 390-430.
- hd: `crates/hd_cache/DESIGN.md` §5.5; `docs/compiler/goals.md` (decided
  architecture, `edit-latency`, `io-per-check`);
  `crates/hd_command/src/disk.rs` (`SETTLE`, `stamp`).

No hd code appears in this record: every option changes the tool, not
the language, so there is no parse log.
