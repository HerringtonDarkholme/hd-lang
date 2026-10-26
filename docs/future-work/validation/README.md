# Validation design materials

These files support [VALIDATION.md](../VALIDATION.md). They do not add std APIs or constitute owner approval.

- `before/` is a handwritten count-only configuration control. It decodes the same valid JSON as the model and prints `0`.
- `model/` uses real Structure/Deserialize with a local Validate template, own typed Rules facts, nested paths, reporting modes and an explicit record method. Seven focused tests run on each engine; its main prints `0`.
- `api-catalog.hd` checks proposed signatures and bounds. Every `panic("shape-only")` is an unimplemented stub; do not execute this as a library.

The model is post-decode validation. It neither changes reader protocols nor requires Inspectable on the reader or field types.
It demonstrates an independent Deserialize rename, an omitted leading validation member, nested signup/address errors, defaults, custom decoding and callback counts.
Its port-location renderer is deliberately one explicit example mapping, not a general alias resolver.
Its marker errors and boolean predicates omit the proposed full Problem/Definition error family and atomic rule implementations.
No checked public JSON/CLI/environment helper, general name capture, pattern/range constructor library or fused reader is implemented here.

Source/compiler baseline: `51d48ea128329748ae01ba53e84e76def5295b1d`.
Dev native compiler SHA-256: `7421c58a0cb5f81b5d58190cee4b5c2385b887d979fbfd3b20d12d6b72d75afa`, with Wasmtime enabled.
Before source SHA-256: `e3a1a742b3582bf01507f5a9f3ee1a2661f1e32ec3b5b0ad9d3b0980ab4f1199`.
Model source SHA-256: `8baa805cab8a24f3bd2e15e5c384cb16c5ad8b6ff0e185ccbefe16d13c093be0`.

From either package, with an absolute scratch HD_CACHE inside the isolated worktree:

```sh
hd check --all --format json
hd fmt --check
hd run --engine node
hd run --engine wasmtime
hd build
```

From `model/`, also run the focused seven-case cohort:

```sh
hd test --engine node --format json
hd test --engine wasmtime --format json
```

Check the catalog as a file from the repository root:

```sh
hd check --all --format json docs/future-work/validation/api-catalog.hd
hd fmt --check docs/future-work/validation/api-catalog.hd
```

The handwritten control has no structured diagnostic paths and is not a fully equivalent validation library.
Wasm footprints compare complete sample programs, not an isolated production std overhead.
The rejected hook experiments and intentional generic-evidence failures remain scratch evidence; no failed hook model is published as working code.
