# `hd doc` (M11, 2026-10-10, #266)

`hd doc` and `hd doc NAME` have no implementation (`hd_doc` is a stub
reporting its stage). This designs the Rust implementation from the
current spec (`spec/cli/command-line.md` Documentation, rules
`cli.doc.*`), superseding the stale parts of `future-work/HD_DOC.md`
(whose 2026-10-06 owner decision predates the `NAME` lookup, `llms.txt`
and per-module pages) and keeping what is still true in
`commands.md` §7.7 (build interfaces, render from `iface` entries plus
doc text through skeleton ranges). The TS prototype's `doc`/`def`
lookup (`src/commands/queries.ts`: longest-prefix module, `dep.KEY`
restriction to `pub`, suggestions) is prior art for `NAME` only. No code
changes.

## Which crate

`hd_doc` grows into the renderer; a new `doc_cmd` in `hd_cli` owns
flag parsing, the check gate, and file writing — the same split as
`test_cmd`/`check_cmd`. New `hd_doc` inputs, all read-only:

- `hd_project` (new dep): package sources for signature slicing and doc
  tests (`doc_tests(file, text)` already exists).
- `hd_resolve::FolderIface` (already a dep): items, exports, and
  `private_names` for visibility.
- The `hd check` output of the same invocation (`cli.doc.check`):
  `doc_cmd` reuses `build_goal` and refuses to write on error, so the
  checker exposes nothing new except what §2 lists.

No Markdown/HTML crate: pages are string-built (fixed headings, lists,
fences); HTML wraps the Markdown content under identical anchors with a
minimal self-contained template (`cli.doc.html` leaves styling
unspecified). `--open` needs no crate: `std::process` dispatch on
`open`/`xdg-open`/OS default. CommonMark fidelity is by construction —
doc text is copied, never transformed — except link rewriting (§4),
which is plain string work.

## What the checker must expose

1. **Doc text per item.** The parser already attaches `##` lines
   (`attach_docs`); what is missing is an API from item to its comment
   text: `(DefId) -> &str` resolved through the skeleton ranges plus the
   source slice (`skim.rs`), excluding the `##` markers. Module docs
   (first `##` block) ride the same API on the module root.
2. **Signatures as written** (`cli.doc.signature` copies source text).
   Skeleton ranges give declaration spans; the renderer slices them from
   `program.sources`. No pretty-printing: bodies are cut by the
   declaration's own span end (functions), members filtered by `pub`
   with `# private lines omitted` markers per the items table.
3. **Visibility.** `FolderIface.exports` (pub surface) vs
   `private_names` (`--private` adds both; default skips private).
   `pub use` re-exports list under the using module with a link only
   (`cli.doc.reexport`), resolved through the export table.
4. **Members and impls.** Inherent-impl method signatures under the
   type's heading; `cli.doc.impls` trait-implementation lines
   (including cross-module, `(derived)` marks) from the impl tables.
5. **Examples.** `hd_project::doc_tests` per file, rendered in place
   with `Example <name>` labels; compile-fail tests keep their
   `# error: CODE` line plus the `Does not compile (CODE)` label.
6. **Anchors.** Item anchors equal the doc-test names
   (`cli.doc.anchor` = `cli.test.doc.name` form), so one lookup finds
   item, section, and examples.

Doc edits touch none of this: comments stay out of every interface hash
(commands.md §7.7), so a doc-only edit rechecks nothing and re-renders
from cache.

## Page and link generation

- **Pages:** `index.md/html` (package name, `pkg` link, module list),
  `pkg.md/html` iff `src/lib.hd` exists, one `PATH.md/html` pair per
  other module (`.` → `/`; top-level `index` nests under `index/`),
  `llms.txt` (name, root-module summary blockquote, `## Modules` list),
  `llms-full.txt` (all pages concatenated, root first, path order).
  Default dir `build/doc`, `--out DIR` overrides.
- **Links:** `` [`NAME`] `` resolves in the documented item's module
  scope (own declarations + its `use` bindings; dotted NAME walks
  members/modules). Resolved → relative link to target anchor/page
  (`cli.doc.relative`); `std` → standard-library reference URL;
  dependency → checked, plain code; broken/private-undocumented →
  `broken-doc-link` warning + plain code (`cli.doc.link.broken`
  deliberately never fails the build).
- **`hd doc NAME`:** longest module prefix wins (`pkg` = root; bare
  NAME = whole module page); `dep.KEY.…` searches that dependency's
  `pub` items only; `std.…` prints signature + summary + reference
  link from the std reference data (spec/std, read-only); section
  sliced from heading to next same/higher heading. Missing NAME is an
  error; adopt the TS suggestions (`candidates: …`) for the message.

## Flags

- `--private`: include `private_names` items/members in pages and
  lookups; gives `src/main.hd` its `main` page (`cli.doc.main`).
- `--out DIR`: write below DIR instead of `build/doc` (same layout).
- `--open`: after a successful write, open `index.html` via the
  platform opener; a failed open warns but keeps exit 0 (files are
  written; opening is a courtesy, not the product).

## CLI cases it must pass

`doc-out` (full file set incl. `llms.txt`/`llms-full.txt`),
`doc-outside-package` (error outside a package),
`doc-check-error` (no output on check failure),
`doc-name` (`--out site text.slugify`, missing NAME → 101, module page,
`std.text.split` + `std.text.missing` → 101),
`doc-private`, `doc-main-page`, `doc-broken-link` (warning, still
writes), `doc-index-module` (`index/` nesting + llms link). All eight
are file/stdout assertions — no host, no network — so they run on both
engines trivially.

## Questions with a recommendation

- **Q1. Render in `hd_doc` from interfaces + source slices, or from a
  dedicated doc IR?** Interfaces + slices (recommended): signatures are
  verbatim source and the iface already answers visibility/exports;
  a doc IR would duplicate both.
- **Q2. HTML template: minimal inline, or share the website's?**
  Minimal inline (recommended): the spec leaves styling unspecified and
  a website dependency would couple the CLI to `website/`.
- **Q3. `std` NAME data: read `spec/std` at build time, or ship a
  generated table?** Generated table in `hd_doc` (recommended): the CLI
  must answer offline, and the reference is frozen spec text.
- **Q4. Keep HD_DOC.md's dropped query/JSON/`--html` modes?** No
  (recommended): the owner decision stands; the stale sections should
  be marked superseded when the freeze lifts.

Recommendation: implement `doc_cmd` + `hd_doc` rendering in Q1's shape
against the eight CLI cases, then the `NAME` lookup with TS-style
suggestions.
