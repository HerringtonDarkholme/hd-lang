# Repository tools

The existing `tools/` directory is one hd package. Named tasks live in
`tasks/`; shared document, inventory and reporting code lives in `src/`.
They check and run with released hd v0.0.4.

From the repository root:

```sh
bash scripts/hd-tools.sh check-sloppy
bash scripts/hd-tools.sh check-sloppy --update
bash scripts/hd-tools.sh check-rule-citations
```

The launcher selects `HD` when set, then `target/release/hd`,
`target/debug/hd`, and `hd` on PATH. It uses Wasmtime and optimized guest
code, with two jobs and a ten-minute command limit. It neither installs
nor builds a compiler. `HD_CACHE` selects a cache; the default is
`target/tools-cache`. `HD_TOOLS_TIME_LIMIT` changes the command limit.

CI installs `HerringtonDarkholme/setup-hd@v1` once, pinned to v0.0.4,
and sets `HD=hd` so the tasks use stage0 independently of main's build
artifacts.

Inside `tools/`, use `hd check --all`, `hd lint --all`, `hd test --engine
wasmtime`, and `hd fmt --check`. Tests live in the modules' `tests:` blocks.

The removed-forms and ledger scripts stay in TypeScript until a published
release contains the new URL and temporary-directory APIs. Their migration is in
[#2460](https://github.com/HerringtonDarkholme/hd-lang/issues/2460).
Their public std API gaps are tracked in
[#2461](https://github.com/HerringtonDarkholme/hd-lang/issues/2461) and
[#2462](https://github.com/HerringtonDarkholme/hd-lang/issues/2462).
