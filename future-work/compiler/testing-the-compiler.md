# New Compiler Design: Testing The Compiler

Part of the [compiler design](README.md).

## 8. Determinism And Soundness Tests

### 8.1 The Determinism Matrix

Each case runs over the product of these dimensions. Two kinds of output
are compared differently (Codex review, D4):

- **Semantic output must be identical** in every cell: diagnostics,
  fix-its, exit status, test outcomes, written `.wasm` files, and every
  cache key.
- **Entry bytes** must be identical for equal keys. Two runs may write
  different sets of entries (a warm run writes fewer), so only entries
  present under the same key are compared.
- **Operational fields** are compared against the expectation for the
  cell's cache state, not across states: `modules_checked`, hit and miss
  counts, timings. A cold run computes every module, and a warm run with
  no edit computes none.
- The opt-in memory cap is the one non-deterministic outcome (§4.15). The
  matrix runs without it.

| Dimension | Values | Catches |
| --- | --- | --- |
| threads | 1, 2, 4, 16 | schedule-dependent output |
| serial order | FIFO, shuffled with 3 seeds | order dependence without threads, in the browser build too |
| hasher seed | 3 seeds for every in-memory map (`HD_DEBUG_HASH_SEED`) | hash-map order leaking into output (Gleam #6383) |
| ID shift | the interners pre-filled with 0, 1,000 or 50,000 junk entries (mine) | any interned ID that is printed, sorted or hashed |
| checkout path | two paths of different lengths | paths in keys (Swift's module variants) |
| cache state | cold; warm; warm after an unrelated edit; verify mode | hits that differ from recomputation |
| platform | Linux, macOS, the browser build under Node | platform-dependent ordering or hashing |

The cases are the conformance suite, std, and three generated packages of
1k, 10k and 50k lines. CI runs a sample of the product on each change and
the full product nightly.

### 8.2 Incremental Soundness

An edit-script fuzzer applies random edits to a generated or fixture
package. After each edit it runs an incremental check and a clean check
with an empty `HD_CACHE`, and compares their outputs and entries byte for
byte. The edit vocabulary:

| Edit | Expected recheck (for `recheck-precision`) |
| --- | --- |
| private function body | that module only |
| private header (signature, new private item) | that module; its folder's interface is rebuilt with the same deep hash |
| public signature | the folder's modules and every module whose key includes a changed deep hash |
| field type of a type reached only through a used signature (`service.load_user().name`) | the reader of `.name`, through the deep hash |
| add, remove or redirect a `pub use` in a chain | every module that uses the re-exported name |
| add or remove an impl for a public type; a blanket impl | dependents, and that trait's coherence |
| add or remove an impl for a private type | that module and the trait's coherence; no dependent |
| add or remove `@derive`, edit a template body in another package | the modules that derive the trait |
| edit a top-level statement read in a multi-module init group | that module and the folder's init order |
| edit a doc comment or a comment | nothing |
| delete a file, then restore it with its old mtime | the right modules both times |
| rewrite a file in place with the same length (one identifier or literal changed), then restore its old mtime | that module: `ctime` differs (§5.5) |
| insert blank lines before a declaration that has a header or overlap diagnostic; lengthen a body above a later header | no semantic recheck; every printed line is the new one (§5.3) |
| rename the package, or change a manifest field that selects executables | everything; a manifest error stops before any cache lookup (§5.3) |
| update a dependency without editing local files, then `hd test --affected` | the tests of programs that reach the dependency (§7.4) |
| edit two modules, run `hd test --filter` for one, then `--affected` | the other module's programs still run (§7.4) |
| move `x.hd` to `x/mod.hd` | the folder's and its dependents' modules |
| touch without change; rewrite within the same second | nothing; the change is found |
| change a dependency version in `hd.toml`; change the std version | dependents of that package; everything |
| switch `--tests` on and off | test overlays only |
| check from a second checkout path | nothing (all keys equal) |

Sorbet's lesson is tracked too: `recheck-precision` reports the share of
edits in a scripted session that recheck more than one module.

### 8.3 Differential And Conformance Testing

- **Portable conformance suite.** The new compiler answers the command
  contract (`hd parse`, `hd check`, `hd test`, exit codes, `code:` in
  diagnostics; [portable README](../../test/portable/README.md)). Its own
  `KNOWN_FAILURES.tsv` is the spec sync, as the prototype's is.
- **Against the frozen prototype.** Run both on the conformance suite and
  on generated programs; compare accept or reject and the set of
  diagnostic codes per file, not messages. Each disagreement is triaged as
  a prototype bug or a new-compiler bug.
- **Header pass against full parse** on every fixture and std file (§4.3).
- **Pathological suite** with an ill-typed variant of each case (Swift's
  lesson), each within its time budget and with its limit diagnostic.
- **Fuzzing.** `cargo fuzz` on the lexer, layout and parser for panics and
  round-trip failures. Program generators live under `test/` as portable
  tools that take any `hd` binary.

## 21. Back-Half Determinism And Soundness Tests

### 21.1 Byte-Identical Wasm

D1's determinism matrix (§8.1) adds every `code` and `link` entry and
every written `.wasm` to what must be byte-identical across thread counts,
serial orders, hasher seeds, ID shifts, checkout paths, cache states and
platforms. Reproducible builds are parked as a metric, but the cache
needs them internally: equal keys must give equal bytes.

### 21.2 Cross-Engine Conformance

The portable runtime cases run on wasmtime (in `hd`) and in a headless
browser (Chromium with and without JSPI, and Firefox), with **one**
`KNOWN_FAILURES.tsv`. A case that passes on one engine and fails on the
other is a bug, never a per-engine known failure; this is the Day 1
cross-backend rule.

### 21.3 Differential Tests Against The Prototype

The frozen prototype and the new compiler run the runtime cases and
generated programs; stdout, exit status and panic category are compared.
Each disagreement is triaged against the spec, as for checking (§8.3).
Program generators are portable tools under `test/`.

### 21.4 Codegen-Cache Soundness

- The edit-script fuzzer (§8.2) also builds: after each edit, an
  incremental `hd build` and a clean one must write identical bytes.
- **Key completeness tests**, one per dependency kind of `code_key`
  (§13.8): a field added to a type in another module; a field added to a
  type that reaches an unchanged generic body only as a type argument; an
  associated-type binding changed in the selected impl; a callee's
  signature changed; an inlined callee's body changed; a callee becoming
  small enough to inline; an impl added that changes nothing (coherence
  forbids changing a selection); a tier switch. Each must miss exactly
  the instances that touch it.
- **A non-inlined callee's body edit** (Codex review, finding 2): the
  caller's code key hits, the callee re-emits, and link resolves the
  caller's relocation to the new callee entry. The program's output
  changes accordingly.
- **Provider order** (Codex review, D1): a function needing two
  providers, run with the interners ID-shifted so the keys' `Ty` order
  reverses. The bytes are identical, and each provider is still called
  in its own slot at run time.
- **Dynamic generic methods** (Codex review, finding 5): a trait value
  whose method has its own type parameter, called at two concrete types,
  and `Inspectable.downcast` through a trait value.
- Verify mode (§5.6) recomputes `tir`, `code` and `link` entries on hits.
- `wasmparser` validates every linked module in CI.
