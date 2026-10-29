# Issue #976 runtime reproduction

This package preserves the validation probe that blocks the final #976
commit. It uses four fields and eight data-only facts, then repeatedly builds
and drops both valid and invalid results. The valid-only shape survives 4,000
validations; this mixed batch traps while destroying its owned validation
paths and their copied `Member`/`Facts` metadata.

From the repository root on `wip/976-validation`, build the dev CLI and this
package for Node, then run 1,000 rounds:

```sh
cargo build -p hd_cli
(
  cd issue-976-repro
  HD_CACHE="$PWD/../.hd-cache-976-repro" ../target/debug/hd build --engine node
)
node host/run.mjs issue-976-repro/build/debug/validation_derived_976.wasm 1000
```

Observed result:

```text
panic: collection-pinned: list length changed while pinned
```

The expected successful checksum is `8000`: every round contributes six
issues for the first invalid value and two for the second. This package is a
WIP diagnostic artifact for blocker #992, not a mergeable #976 fixture.
