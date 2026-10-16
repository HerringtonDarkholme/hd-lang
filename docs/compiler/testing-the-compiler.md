# New Compiler Design: Testing The Compiler

Status: design record; may lag the code. The code and the spec win.

Part of the [compiler design](ARCHITECTURE.md).

## 8. Determinism And Soundness Tests

### 8.1 The Determinism Matrix

`hd_testkit::determinism_matrix` builds a program under every executor
in `EXECUTORS`: the serial executor in FIFO and priority order and
shuffled with three seeds, and the pool with 2 and 8 threads. Every run
must write the same Wasm bytes. It runs over the `examples/` samples;
`crates/hd_driver/tests/all/generics_determinism.rs` and
`rows_determinism.rs` run the same comparison over cache entries.

What a cell must match. The matrix compares Wasm bytes today; the rest
is the target:

- **Semantic output must be identical** in every cell: diagnostics,
  fix-its, exit status, test outcomes, written `.wasm` files, and every
  cache key.
- **Entry bytes** must be identical for equal keys. A warm run writes
  fewer entries, so only entries under the same key are compared.
- **Operational fields** (`modules_checked`, hit and miss counts,
  timings) are compared against the expectation for the cell's cache
  state, not across states.
- **`fuel_used` per body** is semantic in test mode: equal across
  threads, serial orders and a cold or warm solver memo
  (crates/hd_check/DESIGN.md (type checking) rule TC-8, (trait solver)
  rule TS-5).

Planned, not built: these further dimensions.

| Dimension | Values | Catches |
| --- | --- | --- |
| hasher seed | 3 seeds for every in-memory map | hash-map order leaking into output |
| ID shift | the interners pre-filled with 0, 1,000 or 50,000 junk entries | any interned ID that is printed, sorted or hashed |
| checkout path | two paths of different lengths | paths in keys |
| cache state | cold; warm; warm after an unrelated edit | hits that differ from recomputation |
| TIR emission | emit; no-emit (crates/hd_check/DESIGN.md (type checking) §1.5) | checking that depends on what was emitted (rule TC-3) |
| platform | Linux, macOS, the browser build | platform-dependent ordering or hashing |
| cases | the conformance suite, std, generated packages of 1k, 10k and 50k lines | |

**Impl universes in both orders.**
`crates/hd_driver/tests/all/impl_universe.rs`: modules A and B both ask
`Receiver: Pick[?]`, and only A's closure holds
`impl Pick[Product] for Receiver`. The case runs with A's body first and
with B's first, serially and on the pool. Every run gives A the impl and
B none, with the same diagnostics.

### 8.2 Incremental Soundness

`crates/hd_driver/tests/all/incremental.rs` runs scripted edits: a body
edit rechecks one module and a signature edit its importers; a comment
edit reuses the check and moves its positions; a doc edit keeps every
interface hash; a comment above an item leaves interface keys unchanged;
header findings survive a cached interface; a tuple template edit
reaches the header findings it decides; a body edit in a template's
module rechecks only the modules that derive its template, and none when
no template reads the body.

Planned, not built: the edit-script fuzzer (`hd_testkit::edit_script_fuzz`
is a stub). It applies random edits to a generated or fixture package.
After each edit it runs an incremental check and a clean check with an
empty `HD_CACHE`, and compares their outputs and entries byte for byte.
The edit vocabulary:

| Edit | Expected recheck (for `recheck-precision`) |
| --- | --- |
| private function body | that module only |
| private header (signature, new private item) | that module; its folder's interface is rebuilt with the same deep hash |
| public signature | the folder's modules and every module whose key includes a changed deep hash |
| field type of a type reached only through a used signature (`service.load_user().name`) | the reader of `.name`, through the deep hash |
| add, remove or redirect a `pub use` in a chain | every module that uses the re-exported name |
| add or remove an impl for a public type; a blanket impl | dependents, and that trait's coherence |
| add or remove an impl for a private type | that module and the trait's coherence; no dependent |
| add an argument-owned impl, `impl Pick[Product] for Receiver`, in a folder that module A reaches only through another folder's body uses | A, whose `Instantiations` answer gains the impl; not module B, whose closure lacks the folder |
| swap two overlapping impls of one trait, so that the other one ranks later | that trait's coherence reruns, and its report names the new later impl, as a clean run does |
| add or remove `@derive`, edit a template body in another package | the modules that derive the trait |
| edit a body in a template's module that no template reads | that module only |
| inside a kept template body, move a statement out of an `if` to after it | the trait's folder gets a new api text hash; every module that derives the trait rechecks and matches a clean run |
| change a template body's indentation width throughout | that module only; layout tokens are relative, so the api text hash is unchanged |
| edit a top-level statement read in a multi-module init group | that module and the folder's init order |
| edit a doc comment or a comment | that module rechecks under its new source hash; no dependent rechecks |
| delete a file, then restore it | the right modules both times |
| insert blank lines before a declaration that has a header or overlap diagnostic | that module only; every printed line is the new one |
| rename the package, or change a manifest field that selects executables | everything; a manifest error stops before any cache lookup |
| update a dependency without editing local files, then `hd test --affected` | the tests of programs that reach the dependency |
| move `x.hd` to `x/mod.hd` | the folder's and its dependents' modules |
| change a dependency version in `hd.toml`; change the std version | dependents of that package; everything |
| check from a second checkout path | nothing (all keys equal) |

### 8.3 Differential And Conformance Testing

- **Portable conformance suite.** `crates/hd_driver/tests/conformance.rs`
  runs every indexed case through the compiler. Every case must pass
  except a row of the root `KNOWN_FAILURES.tsv` (`path`, `issue`,
  `engine`, `reason`); a listed case that passes fails too, so the list
  is the exact spec sync.
- **CLI tier against the binary.** The CLI cases
  (`spec/conformance/cli-cases.tsv`) run in
  `crates/hd_cli/tests/cli_conformance.rs` against the built `hd` binary,
  under the same rule; their rows are `cli/NAME`. Each tier prints its
  pass, known, fail and unsupported counts.
- **Header pass against full parse.** The `hd_syntax` unit test
  `skim_and_layout_reference_agree` compares body ranges from both.
  Planned, not built: the same comparison on every fixture and std file.
- Planned, not built: a **pathological suite** with an ill-typed variant
  of each case, each within its time budget and with its limit
  diagnostic, including the nested trial cases of
  [crates/hd_check/DESIGN.md (type checking) §2.5](../../crates/hd_check/DESIGN.md#25-methods-and-operators).
- Planned, not built: **fuzzing** the lexer, layout and parser with
  `cargo fuzz` for panics and round-trip failures, and program
  generators as portable tools that take any `hd` binary.

## 21. Back-Half Determinism And Soundness Tests

### 21.1 Byte-Identical Wasm

The determinism matrix (§8.1) compares the linked Wasm across executors.
`HD_CONFORMANCE_WASM_DIGEST=record|check` records the hash of every Wasm
module the runtime conformance cases run and fails a later run on any
difference, so a refactor can prove it leaves emitted code unchanged.
The cache needs this internally too: equal keys must give equal bytes.

### 21.2 Cross-Engine Conformance

The runtime cases run on Node (V8) and, with
`HD_CONFORMANCE_ENGINE=wasmtime`, on wasmtime too, each with a build
tuned for its own engine target. There is **one** `KNOWN_FAILURES.tsv`.
A case that passes on one engine and fails on the other is a bug with
its own issue, and its row names that engine.

Implementation differs: the design ran the runtime cases in a headless
browser (Chromium with and without JSPI, and Firefox) beside wasmtime.
CI runs Node and wasmtime only, so SpiderMonkey and the JSPI path that
the playground uses are not tested.

### 21.4 Codegen-Cache Soundness

Planned, not built:

- The edit-script fuzzer (§8.2) also builds: after each edit, an
  incremental `hd build` and a clean one must write identical bytes.
- **Key completeness tests**, one per dependency kind of `code_key`: a
  field added to a type in another module; a field added to a type that
  reaches an unchanged generic body only as a type argument; an
  associated-type binding changed in the selected impl; a callee's
  signature changed; an inlined callee's body changed; a callee becoming
  small enough to inline; an impl added that changes nothing; a tier
  switch. Each must miss exactly the instances that touch it.
- **A non-inlined callee's body edit:** the caller's code key hits, the
  callee re-emits, and link resolves the caller's relocation to the new
  callee entry.
- **Provider order:** a function needing two providers, run with the
  interners ID-shifted so the keys' `Ty` order reverses. The bytes are
  identical, and each provider is still called in its own slot.
- A verify mode that recomputes `check`, `code` and `link` entries on
  hits and compares them.

`wasmparser` validates linked modules in the driver's tests
(`crates/hd_driver/tests/all/closure_code_targets.rs`).
