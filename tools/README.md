# Repository tools

The existing `tools/` directory is one hd package. Named tasks live in
`tasks/`; shared document, inventory and reporting code lives in `src/`.
The tasks require released hd v0.0.5, which includes the URL and temporary-directory APIs of #2461 and #2462.

From the repository root:

```sh
bash scripts/hd-tools.sh check-host-sources
bash scripts/hd-tools.sh check-typescript-sources
bash scripts/hd-tools.sh check-sloppy
bash scripts/hd-tools.sh check-sloppy --update
bash scripts/hd-tools.sh check-rule-citations
bash scripts/hd-tools.sh check-removed-forms
bash scripts/hd-tools.sh ledger --hd /absolute/path/to/instrumented/hd
```

The launcher selects `HD` when set, then `target/release/hd`,
`target/debug/hd`, and `hd` on PATH. It uses Wasmtime and optimized guest
code, with two jobs and a ten-minute command limit. It neither installs
nor builds a compiler. `HD_CACHE` selects a cache; the default is
`target/tools-cache`. `HD_TOOLS_TIME_LIMIT` changes the command limit.

CI installs `HerringtonDarkholme/setup-hd@v1` once, pinned to v0.0.5,
and sets `HD=hd` so the tasks use stage0 independently of main's build
artifacts.

Inside `tools/`, use `hd check --all`, `hd lint --all`, `hd test --engine
wasmtime`, and `hd fmt --check`. Tests live in the modules' `tests:` blocks.

These tasks replace the corresponding TypeScript entry points; shared document
handling is `src/doc_files.hd`.

Removed-forms uses `std.url.Url.path`, as the owner chose: escapes are decoded,
queries are excluded, and a literal `+` is preserved. It does not contain its
own URI decoder.

Ledger's `--hd` selects the compiler being measured, separately from the
compiler running the task. Supply an absolute path to a compiler built with
`hd_driver/ledger` to avoid its default instrumented compiler build. The task
runs on Wasmtime. Its `hd build --engine node` calls compile modules for the
same target measured by the previous ledger, without executing Node.
Each measured build owns a fresh temporary directory and removes it on every
exit, with at most two builds and their workspaces active at once.
