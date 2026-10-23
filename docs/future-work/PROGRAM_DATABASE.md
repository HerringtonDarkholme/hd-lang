# Program database

Status: proposal for [#181](https://github.com/HerringtonDarkholme/hd-lang/issues/181), 2026-10-23; nothing here is accepted behavior or specification.

## Recommendation

Recommend a SQLite artifact in the selected package’s `build/program.sqlite`, with named CLI queries over a small, versioned projection of checked source facts. Keep extraction independent of SQLite, so the browser can consume the same projection in memory. Preserve the existing per-module compiled cache, without adding Salsa or a daemon.

Workspace selection follows `-p`, as documentation does. Each snapshot identifies its root package and dependency graph; one member’s index never claims to cover every workspace member.

The public contract would be the documented fact views and query answers, not serialized Rust types or compiler cache files. Approve that contract before implementation. The compiler's evolving data remains a reason to delay publication, as [#181](https://github.com/HerringtonDarkholme/hd-lang/issues/181) explains.

## What exists today

The [compiler goals](../compiler/goals.md#summary) require incremental per-module checking, no daemon or Salsa, and a single-threaded browser compiler without filesystem dependence. They put the program database [after the first release](../compiler/goals.md#pillar-1-features-agent-wait-time-and-retries). This proposal changes none of those decisions.

[`hd doc`](../../spec/cli/command-line.md#documentation) already answers item lookups. Its [renderer](../../crates/hd_doc/src/lib.rs) combines written source signatures, documentation, interfaces and module scopes. That is useful input, but rendered Markdown is not a typed declaration or call index.

[`check_key`](../../crates/hd_cache/src/lib.rs) deliberately ignores ordinary comments and blank lines. Cached positions are code-relative, and the driver [validates and re-interns cache sections](../../crates/hd_driver/src/lib.rs). A semantic cache hit therefore cannot certify unchanged source bytes, documentation or absolute offsets.

The [lookup metric](../../bench/metrics/scripts/lookup-latency.ts) checks support for `hd callers` and `hd needs` before sampling. Both remain proposed commands. The [p95 ≤ 100 ms target](../compiler/goals.md#pillar-1-single-agent-development-cost-proxies) is not an achieved result.

The fixed use cases are finding a declaration/signature, finding incoming source calls, listing checked requirement rows, and sharing documentation or REPL context. Every answer must identify the code and coverage it describes.

## One program, three storage choices

This ordinary hd program supplies the same facts under every option:

```text
fn twice(n: i32) -> i32:
    n + n

pub fn show_twice(n: i32) -> void $ Console:
    println(twice(n))

pub fn main() -> void $ Console:
    show_twice(+21)
```

`show_twice` contains the source call to private `twice`. Both public functions declare `Console`; `twice` does not. Those are source facts, not evidence that any call executes.

Proposed queries, unavailable today:

```sh
hd callers twice --private --format json
hd needs Console --format json
```

The first would identify `show_twice` and its call site. The second would list the two checked rows containing the resolved `Console` key. Open row parameters remain explicit; an unknown requirement is not an empty row.

| Option | Public contract and mechanism | Main cost or loss |
| --- | --- | --- |
| SQLite artifact — recommended | Versioned read-only views and CLI answers; compiler facts survive process exit and can be joined by external tools. | Adds a native storage adapter, transactional publication and a persistent schema contract. |
| In-process API first | A snapshot query API returns storage-neutral records; each CLI process reconstructs its view from compiled entries. | Fits the browser, but external agents need the CLI or bindings; warm-start cost remains unmeasured. Persistent queries wait. |
| Immutable JSON Lines shards | Smallest alternative: write per-module fact files and a generation manifest; a CLI loads the shards it needs. | Avoids a database dependency, but joins, incoming-edge indexes, update publication and format stability still need design. |

All three preserve the same hd program. They add CLI/compiler tooling, with no syntax, semantic exception, intrinsic or std API. The [language design cost order](../../AGENTS.md#design-cost-order) therefore does not distinguish them; storage and process mechanisms do.

| Concern | SQLite artifact | In-process first | JSON Lines shards |
| --- | --- | --- | --- |
| Queries after compiler exit | Direct read-only access | Restart and hydrate, or embed | Load selected shards |
| Declaration, row and incoming-call queries | Indexed joins | Query methods and indexes | Canned indexes or scans |
| Incremental module updates | Replace generation membership transactionally | Reuse module records in one view | Write changed shards, then publish manifest |
| Browser compatibility | Memory projection; no required SQLite linkage | Natural fit | Memory/file adapter |
| Agent and human access | CLI answers plus documented SQL | CLI answers; embedding for custom queries | CLI answers; ad-hoc scripts |
| Stability cost | Published views and query meanings | Published records and query meanings | Published records and manifest meanings |
| Implementation/evolution | More storage work; arbitrary joins need no new CLI verb | Less storage work; persistent export comes later | Least initial machinery; more query/index code later |
| p95 lookup target | Unproved | Unproved | Unproved |

The in-process option is the next choice if a persistent public schema remains premature. JSON Lines gives up general indexed joins first. Choosing a file does not settle the fact vocabulary or make incomplete analysis correct.

## Lessons from other tools

These are primary-source observations, checked on 2026-10-23. The hd choices below remain recommendations.

| Tool | Relevant design | Lesson for hd |
| --- | --- | --- |
| [SCIP](https://github.com/scip-code/scip/blob/main/scip.proto) | Separates documents, symbol descriptors, occurrences, relationships and position encodings. | Name source entities independently of process-local IDs; specify offset encoding. |
| [Kythe](https://www.kythe.io/docs/kythe-storage.html) | Stores facts and edges under structured corpus/root/path/language/signature identities. | A name alone does not identify code across repositories or generated sources. |
| [rust-analyzer](https://rust-analyzer.github.io/book/contributing/architecture.html) | Separates its serializable-shaped IDE boundary from internal HIR; `Analysis` is an immutable snapshot. | Copy the boundary principle, not its Salsa engine or server architecture. |
| [clangd](https://clangd.llvm.org/design/indexing) | Has live, background and externally built indexes; background indexing caches file shards. | State which files/configurations an index covers, independently of storage. |
| [gopls](https://github.com/golang/tools/blob/master/gopls/doc/design/implementation.md) | Distinguishes build-option views, workspace snapshots and unsaved overlays. | File-backed and REPL views must not silently share one identity. |
| [TypeScript's language-service API](https://github.com/microsoft/TypeScript/wiki/Using-the-Language-Service-API) | The host supplies files and `ScriptSnapshot`s; a document registry shares parsed files. | A query interface does not inherently require an editor protocol or background service. |
| [SQLite isolation](https://sqlite.org/isolation.html) | Separate connections see committed transactions; a read transaction can hold one consistent database view. | Pin one generation per answer; transaction consistency alone does not prove source freshness. |

The common boundary is a source snapshot plus explicit derived facts. Neither an editor protocol nor an SQL engine supplies semantic completeness. A storage-neutral projection also leaves room for a future editor adapter without making it this job's prerequisite.

## Facts and schema stability

Propose these logical families, not a finalized table layout:

| Family | Proposed contents and limits |
| --- | --- |
| Snapshot and coverage | Fact-schema version, producer identity, input identity, analysis scope and per-module completion/error/skipped state. |
| Source and declarations | Package identity, relative file path, exact parsed-byte digest, declaration identity, parent, kind, name, visibility and source ranges. |
| Signatures and rows | Written signature/doc text plus a versioned structural projection of checked types, generic bounds and requirement keys/row parameters. Display strings are not equality keys. |
| Source references/calls | Caller and call-site range, resolved target or trait slot, and resolution kind. Unresolved/dynamic cases remain distinct from statically resolved calls. |
| Implementations and derives | Checked impl heads, bounds, associated types, and explicit derivation provenance where extraction is supported. Missing generated provenance is marked incomplete. |
| Diagnostics | Existing diagnostic codes, labels and fixes tied to the exact source snapshot. Unsupported analysis has an explicit coverage reason. |

Start with declarations, signatures, rows, source calls and diagnostics sufficient for the two canned queries. Publish impl/derive projections only after their provenance and coverage are defined. Experimental families must not masquerade as a complete stable family.

Use opaque declaration IDs scoped to a snapshot, backed by package/file/declaration descriptors. Never persist run-local `DefId`, `Ty`, `Symbol` or TIR row numbers as public identities. A qualified name is a lookup key that can be ambiguous, not a unique identifier across edits.

Keep schema version separate from compiler identity and input identity. Document compatibility only for named views and answer fields. Internal storage may change; incompatible meanings require a new API version, and unsupported versions are refused rather than guessed.

SQLite [reserves an application-controlled `user_version`](https://sqlite.org/fileformat2.html), but that number does not define compatibility for hd. A cache rebuild can replace an obsolete local database; an explicitly exported snapshot remains readable only by a compatible reader.

## Correct answers need a snapshot

### Identity and freshness

A snapshot identifies exact parsed source bytes, manifests and resolved dependency identities, toolchain/std identity, and analysis goal/configuration. Include tests, target and REPL context whenever they affect facts. Identical file paths, Git HEAD or modification times alone are insufficient.

Ranges use UTF-8 byte offsets against the captured bytes, matching [existing fix-it offsets](../../spec/cli/command-line.md#r-cli.json.fix.offsets). Newlines and Unicode are part of that identity. This is not a promise of reproducible builds across machines, which the owner has parked.

Recommend refreshing the required scope from captured inputs before a normal query answers. The response includes snapshot identity, scope, coverage and whether the answer is complete. An explicitly pinned historical query may use older facts, but must say so.

If input changes during refresh, return the captured identity or retry within a documented bound. Do not label an older result as current. A mutable “latest” pointer is a convenience, never proof that its input identity matches the request.

### Incomplete and indirect answers

An empty result means “none in this complete scope” only when all required modules/fact families are complete. Errors, skipped bodies or an obsolete fact format produce an incomplete answer with named gaps. A dependency indexed only for signatures cannot support exhaustive incoming-call claims about its bodies.

Define `callers` initially as direct source call sites, before monomorphization, inlining and dead-code removal. Distinguish a statically resolved target from dispatch through a trait slot. Candidate implementations are possible targets, not an exact runtime call graph.

Define `needs` over checked declaration rows and resolved requirement keys, including type arguments. Preserve open rows and unresolved keys. It reports a static obligation, not which host calls happened or which provider was installed at runtime.

### Visibility and names

Use compiler-produced bindings and call resolution, including imports, re-exports, private declarations and generated contexts. Do not rebuild hd lookup from a global name table plus one `public` flag. Query defaults and `--private` would follow the documentation command's package visibility boundary.

Private facts can support internal analysis without making private names importable. Ambiguous name queries return candidates or a structured ambiguity, never choose by database row order. Test and REPL contexts retain their distinct scope identities.

## Reuse the compiled cache without publishing it

Extract a module's semantic facts on a cache miss, alongside the checked result. On a valid cache hit, reuse a compatible fact projection or reconstruct it; otherwise recompute the missing facts. A skipped extractor must not silently produce an empty complete module.

Key semantic reuse by the existing checked inputs plus the fact-projection version. Bind source positions and written documentation separately to the current source-byte identity. Comment-only edits can reuse semantic facts while still updating ranges and rendered source text.

Changing a public interface invalidates affected dependents under the existing dependency rules. A body edit updates that module's call facts even when its interface is unchanged. Deletions, renames and changed re-exports remove old membership from the next snapshot.

Assemble a new snapshot off to the side, then publish its complete membership in one short transaction. A failed/aborted writer leaves the previous published snapshot intact. Readers select one snapshot in one read transaction, never a mixture of module generations.

Overlapping compilers must not let a late older build overwrite a newer current pointer unchecked. Publication validates its captured identity and predecessor; each reader still matches the requested identity independently. Indexing must not hold a database write lock while checking bodies in parallel.

Use ordinary SQLite transactions initially; choose WAL only after measuring the intended filesystem and concurrency. [WAL requires a shared local-host environment](https://sqlite.org/wal.html), so it cannot be an unstated workspace assumption. Export a consistent snapshot through the [backup API](https://sqlite.org/backup.html), not by copying an active database file arbitrarily.

Retain the current and actively pinned generations; collect unreferenced module facts. Historical tokens may expire under an explicit retention policy, and expiration is an error rather than a redirect to newer facts. Include index storage in the existing artifact-size budget; do not promise unbounded query history.

## Documentation, REPL and browser

`hd doc` could consume the same declaration/documentation projection while preserving its existing Markdown/HTML and `--private` behavior. Its [check-before-documenting rule](../../spec/cli/command-line.md#r-cli.doc.check) still applies. A partially indexed broken package must not bypass that rule.

A REPL view needs a session identity, input identities and its accepted declaration watermark. A panicked input adds no [binding or declaration](../../spec/cli/command-line.md#r-cli.repl.panic.declarations); its diagnostics can remain a separate failed-input view. Earlier runtime mutations and host calls are outside the static fact database.

Package queries must not absorb a live REPL's private bindings by accident. Rebuilds establish a new input/context identity. [Journal compaction](https://github.com/HerringtonDarkholme/hd-lang/issues/646) and [durable execution](https://github.com/HerringtonDarkholme/hd-lang/issues/18) remain separate proposals; the fact store neither replays nor checkpoints effects.

The compiler-side projection must work with in-memory sources and stores. SQLite is an optional native adapter, not a requirement for `hd_web`, a new browser filesystem dependency or a public dump of internal cache sections.

## Measurement and acceptance

Keep the existing benchmark shape: a generated 10k-line package, one untimed warm-up per canned query, then ten timed invocations. The target remains **p95 ≤ 100 ms per query**. Count process startup, input validation, database opening, name resolution, query work and answer encoding together.

Report initial index construction, edit refresh, bytes retained and writer contention separately. A warm SQL statement benchmark cannot certify the CLI target. A fast incomplete `needs Http` answer is incorrect, even if the generated package has no such requirement.

Before publishing a schema, compare incremental and clean fact sets after body/signature/comment edits, deletion, re-export changes and dependency replacement. Test Unicode/newline positions, private/test/REPL visibility, open rows, trait dispatch, failed checks and missing old-format cache facts. Concurrent readers, interrupted publication and out-of-order writers must preserve one identified snapshot.

## Approval before implementation

The owner would approve SQLite as the native artifact, its location and bounded retention, and the initial stable fact families. Approval also defines `callers`/`needs`, default refresh, incomplete/stale/ambiguous answers and private-query visibility. Those are future CLI contracts; this note adds no spec rule or implementation.

After approval, scoped issues would establish the projection and clean/incremental equivalence first, then transactional persistence and named queries. Documentation and REPL adapters follow that common boundary. The unresolved compiler data shapes remain a prerequisite, not a reason to expose unstable TIR or duplicate the checker in SQL.

## Example validation

The single hd block parsed cleanly, checked with zero errors/warnings, and passed `hd fmt --check`. Validation used verified frozen parser/CLI binaries from `bc3ffb05a3` plus #526’s runtime-only change, copied from the #646 evidence. Parser/core source is unchanged at this note’s base `fd34a283d6`. These checks validate the ordinary example, not database commands, fact extraction or the latency target.
