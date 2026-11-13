# Optimization Audit, 2026-11-13

Every optimization found that moves one of the three pillars of
[docs/compiler/goals.md](../compiler/goals.md). One numbered item per
change. Items from earlier audits are folded in and cited, so this file is
the single list.

Status: an audit log, not accepted behavior. Directions that change a rule
or a public surface are marked **owner**.

## Sources And Method

| Source | What it gave |
| --- | --- |
| CI metrics, `metrics-data` `daily/2026-11-13T04.json`, commit `8d928bfcf`, 4-CPU `ubuntu-latest` | the misses and their notes |
| Local release build of `a45fdfc17` (`cargo build --release -p hd_cli`), Apple arm64, 14 cores | profiles and timings |
| [samply](https://github.com/mstange/samply) profiles of generated 10k- and 50k-line packages (`bench/metrics/lib/gen.ts`) | phase costs |
| An agent's run of 2,827 `reject:` conformance fixtures and 720 one-edit typos of valid fixtures, on the installed `hd` of 2026-11-10 | diagnostic counts |
| An agent's code trace of the pillar 3 misses | host, test runner, JSON and regex causes |
| [metrics-audit-2026-11-13.md](metrics-audit-2026-11-13.md) | causes C1 to C15 of the missed metrics |
| [review-2026-11-12.md](review-2026-11-12.md) | architecture A1 to A10, defects F1 to F22, std patterns |
| [layout-and-wat-2026-11-13.md](layout-and-wat-2026-11-13.md) | allocator L1 to L4, emitted Wasm W1 to W9 |
| [string-collection-costs.md](string-collection-costs.md) | string, list and map costs, measured 2026-11-02 |

| Instructions retired (`/usr/bin/time -l`, median of 5) of hd programs on the Node host and of the same JS, run with one `node` binary | run-time and start-up costs that load does not distort |
| `node --cpu-prof` of the host | host JavaScript costs |
| An interposed allocator (`DYLD_INSERT_LIBRARIES`) recording live bytes per call site at the heap's peak | memory by site |

The machine's load average was 13 to 41 on 14 cores during the local runs.
Heap bytes and profile shares are reliable. Wall times are good for ratios
between runs taken together, not as absolute numbers. For run time this
audit reports instructions retired and CPU cycles, as medians of 7 runs.
One caveat: V8's optimizing tiers compile on background threads, so under
load a program reaches optimized code later and retires more
instructions. The same JS `sum` measured 90M and later 217M instructions
of work. Comparisons between hd builds, made in the same minute, are
reliable. Ratios to JS are approximate.

Cycles depend on load more than instructions do. On Apple Silicon a
loaded machine moves a process to efficiency cores, which changes cycles
per instruction. hd's `sum` took 69M cycles of work at a load of 20 to 40
and 22M at a load of 8. So every cycle comparison in this file compares
runs made back to back, and no absolute cycle figure is a target.

Run time in the quietest window of the session (load average 8 to 13),
whole run minus an empty program on each side, both cold, so V8's
compile time is included. Medians of 15 wall-clock runs:

| Case | hd | JS | hd/JS | Instructions hd/JS |
| --- | --- | --- | --- | --- |
| `fib` | 2.4 ms | 5.5 ms | 0.44x | 0.32x |
| `sum` | 8.3 ms | 6.9 ms | 1.20x | 2.15x |
| `string-build` | 2.7 ms | 6.2 ms | 0.44x | 0.43x |
| `map` | 8.5 ms | 5.7 ms | 1.49x | 2.02x |
| `sort` | 6.9 ms | 17.9 ms | 0.39x | 0.31x |

The empty program takes 29.9 ms through the hd host and 24.1 ms as bare
`node`. `sum` (E6) and `map` (R5, plus the host set-up in E1) are the two
cases where hd runs more instructions than JS. CI reports them at 1.45x
and 2.70x because it compares a cold hd run with warm JS (H4).

Diagnostic counts were rerun on the current release build (`a45fdfc17`).
The repo's own metrics (`pnpm run metrics -- --only
mistakes,errors-per-run,diag-location,fixit-safety,recheck-precision --hd
target/release/hd`) match CI exactly: fix-it 33.3%, largest diagnostic 92
tokens, mixed errors 2 of 10, signature edit 9 modules. The 720-typo probe
gives 538 of 707 with exactly one diagnostic (76.1%, against 74.8% on the
11-10 build). 0 of about 1,000 diagnostics carry a fix, and 118 of 120
variant typos cascade.

To run an hd program on Node without `hd`, the audit copied a run's
scratch directory (the host, `config.json`) while the program waited on
stdin, then ran `node --experimental-wasm-jspi run.js FILE.wasm
config.json`.

Marks: **[M]** measured here, **[V]** verified in code, **[I]** inferred,
not measured.

## Top Items By Expected Effect

| # | Item | Pillar | Targets it moves |
| --- | --- | --- | --- |
| 1 | [D1](#d1) poison an unknown-variant arm | 1 | one diagnostic per mistake, diagnostic size |
| 2 | [D4](#d4) spelling fixes for names, types, fields and variants | 1 | fix-it rate |
| 3 | [W1](#w1) decode cached interfaces lazily, without re-interning | 1, 2 | edit latency, startup, concurrency |
| 4 | [S1](#s1) less contention between workers, and a pool sized to free cores | 2 | concurrency CPU and latency, parallel speedup |
| 5 | [S6](#s6) single-flight for the two caches | 2 | cache contention, concurrency |
| 6 | [E1](#e1) wasmtime in-process with a module cache **owner** | 3 | start-up, test set-up, unit tests |
| 7 | [C1](#c1) a faster global allocator (measured: -13% instructions, -4% to -10% cycles) | 1, 2 | cold check, concurrency |
| 8 | [I1](#i1) early cutoff by recorded reads (#49) | 1 | recheck precision, REPL, warm check |
| 9 | [D10](#d10) check the items that parsed | 1 | errors per run |
| 10 | [R1](#r1) a lazy DFA for regex search | 3 | regex throughput |
| 11 | [E6](#e6) hot loops in `main` stay in V8's baseline tier | 3 | `sum`, every loop-in-`main` program |
| 12 | [M1](#m1) stream cache entries instead of holding them to the end | 2 | peak RSS, `resources` |
| 13 | [R5](#r5) a short path for small-map hits, lost in `f13fdb35c` | 3 | map lookups, 28% on small maps |
| 14 | [N4](#n4) one driver run for `hd check --tests` | 1 | the per-edit check agents run, +39% today |
| 15 | [N1](#n1) float text and ASCII case mapping in the guest | 3 | `serde-throughput`, text throughput |

## Pillar 1: Single-Agent Cost

### Diagnostics

<a id="d1"></a>
1. **D1. An unknown-variant arm does not poison its match. [V]**
   - Evidence: `crates/hd_check/src/pat.rs:447-449` reports
     `unknown-variant` and returns `Ok(())`. It neither sets
     `pattern_rejected` nor declares the arm's bindings poisoned. The
     arity error at `pat.rs:463-469` has the same gap. The
     `bare-variant-pattern` path 30 lines above does both.
   - Effect: 118 of 120 variant typos cascade, mostly into
     `nonexhaustive-match`, `unknown-variant` and `unknown-name`. In 106
     of them `nonexhaustive-match` comes first, ahead of the real
     mistake. The corpus's 92-token `some-on-result` is this case.
   - Fix: set `pattern_rejected = true` and return
     `declare_poisoned_fields(p, binds)`, in both places.
   - Tried [M]: the four-line change, built in a scratch worktree of
     `a45fdfc17`.
     - Variant typos with exactly one diagnostic: 2 of 120 before, 117 of
       120 after. The first code is `unknown-variant` in 120 of 120,
       against 13 before. The other 3 add a `type-mismatch`.
     - Whole probe: 538 to 653 of 707 with one diagnostic (76% to 92%).
     - Corpus metrics: largest diagnostic 92 to 65 tokens, still over the
       60 target. `some-on-result` now gives two `unknown-variant` errors,
       one per bad arm, and the metric counts the program's whole output
       with its summary line. Exactly-one-diagnostic stays at 68 of 69
       for the same reason. D9 (one report per match) or D8 (a shorter
       summary) finishes both.
     - `fixit-safety`, `diag-location` and `errors-per-run` were
       unchanged.
   - Reference: rustc skips exhaustiveness for a match with an erroneous
     pattern.

2. **D2. Error paths return `never` where they should return `POISON`.
   [V]**
   - Evidence: `crates/hd_check/src/expr.rs:2641` and `:2685` (unknown
     field), `:3474` and `:3480` (unknown type in a literal);
     `call.rs:3616` (unknown variant in a construction), `:943`, `:957`,
     and `:4680-4684` (a method on a poisoned receiver).
   - Effect: `never` is a real type, so later rules fire on it:
     `uninhabited-binding`, "expected never, found string", and
     `nonexhaustive-match`. About 20 of the probe's cascades.
   - Fix: return `Ty::POISON` at each site.

3. **D3. An unknown type in a signature cascades into the body and the
   trait check. [V by agent]**
   - Evidence: `hd_resolve` lowers the type to `POISON`
     (`lower.rs:3144`). Then `body.rs:5208-5242` still reports
     `cannot-infer-type`, because its guard looks only at the body's own
     errors. And `conform.rs:262` (`sig_differs`) compares `POISON` as a
     real type, reporting `trait-method-signature`.
   - Effect: 23 of the 32 type-typo cascades.
   - Fix: skip the unsolved-type report when the signature holds
     `POISON`, and let `POISON` match in `sig_differs`.

<a id="d4"></a>
4. **D4. Spelling suggestions exist only for methods, and never as a
   fix. [V by agent]**
   - Evidence: `unknown-method` says "did you mean" 104 times with 0
     fixes (`call.rs:4144`, `method_hint` at `:4199-4247`).
     `unknown-name`, `unknown-type`, `unknown-field` and
     `unknown-variant` suggest nothing across 580 probe typos. The method
     threshold, `max(ceil(len/2), 3)` at `call.rs:4221`, is loose enough
     to suggest `cmp` for `nope`.
   - Fix: one shared `name_distance` at the name, type, field and variant
     sites (`call.rs:564`, `expr.rs:667`, `lower.rs:2287`,
     `lower.rs:3144`, `expr.rs:3472`, `expr.rs:2641`, `:2685`,
     `pat.rs:448`, `call.rs:3615`). Use rustc's threshold
     `max(len, 3) / 3`. Emit a fix only when one candidate is best.
   - Expected, measured on the probe's typos [M]: with rustc's threshold
     and the identifiers of each file as candidates, the original name is
     the single best candidate for 439 of 480 typos (91%). Fields 116 of
     120, methods 120 of 120, variants 120 of 120, names 83 of 120. Names
     score lower because the threshold admits a distance of 2 only from 4
     characters on. A real scope has more candidates than one file, so
     the true rate is a little lower. Today 0 of these get a fix.

5. **D5. "add `use std.x.Y`" is prose with no edit. [V by agent]**
   - Evidence: 19 `unknown-name`, 5 `unknown-trait` and 2
     `unknown-type` messages say it (`call.rs:533-546`, `lower.rs:2208`,
     `:2661`, `ty.rs:730`). The hint function returns only a string.
   - Fix: return the path as well, and emit a fix that inserts the `use`
     line after the file's last `use`.

6. **D6. Other messages that name one edit carry no fix. [V by agent]**
   - Checker: `bare-variant-pattern` (insert `.`, `pat.rs:247`, `:420`),
     `non-reassignable-parameter-binding` (`value.rs:372`,
     `refs.rs:998`), `non-reassignable-binding` (`body.rs:4317`, `:4353`),
     `pipe-step-needs-placeholder` (`expr.rs:4732`, `:4744`),
     `suspending-pipe-step` (`call.rs:3082`), a duration such as `1.5s`
     (`literals.rs:116`), and `print` for `println` (`call.rs:543`).
   - Parser: `syntax-error` carries a fix in 1 of 194 fixtures, though
     `Parser::fix_error` (`parser/mod.rs:509`) exists. Messages that name
     the edit: `stmts.rs:262`, `exprs.rs:607`, `:680`, `:1143`, `:166`,
     `layout.rs:461`, `mod.rs:155`, `types.rs:251`.
   - Expected, counted case by case [V]: of the corpus's 46 mistakes with
     no fix-it, 20 have one unambiguous edit. 17 of those are covered by
     D4, D5 and D6. The other 3 are type-directed: `i32(x)` for a `usize`
     result, `?` for a `Result` where a value is expected, and
     `Path::new(…)` for a string. That gives 43 of 69 (62%), against the
     80% target.

<a id="d11"></a>
7. **D11. The last 18 points need a preferred repair where two exist.
    [V]**
    - Evidence: the 26 remaining corpus mistakes have two or more
      plausible repairs. Examples: `mixed-signedness` (a `usize` counter or
      `i64(index)`), `interpolate-optional` (`debug(x)` or
      `unwrap_or(...)`), `mut` in a type (three forms), and an `unwrap` on
      a `Result` (`unwrap_or` needs an argument). The metric applies a
      fix only when a diagnostic offers exactly one
      (`bench/metrics/lib/fixit.ts:46`).
    - Fix: offer the more likely repair first and mark it as a guess, as
      rustc's `MaybeIncorrect` suggestions do. **owner**: whether the
      metric should apply the first of several offered fixes.

8. **D7. Long diagnostics print whole types and fixed help text.
   [V by agent]**
   - `trait-resolution-depth` prints a 64-deep type, 108-126 tokens
     (`body.rs:3199`, `header.rs:1257`). F17 of the 2026-11-12 review is
     a 5.2 MB `type-mismatch` from one doubled tuple type.
   - An opaque type prints with its full module path, 63-73 tokens
     (`iface.rs:2752`).
   - `overlapping-impl` prints both impls by path, 57-79 tokens
     (`hd_driver/src/lib.rs:5880`, `:6114`).
   - Row mismatches add about 25 tokens of fixed explanation
     (`body.rs:2311`); `move-out-of-place` and `cannot-infer-type` do the
     same.
   - Fix: cut a printed type past a depth or length, as rustc does. Print
     the bare trait name. Label the second impl with a span, not text.
     Keep the edit in the message and move the reasons to the docs.

9. **D8. The output format spends tokens on nothing. [V by agent]**
   - The text summary `check result: FAILED. errors: 1; warnings: 0`
     (`hd_cli/src/check_cmd.rs:653`) is about 11 tokens, roughly 30% of
     a typical run's output.
   - In JSON, `"fixes":[]` appears on 96% of rows. `kind` and `severity`
     appear on every row, and an edit repeats its file. The `check`
     summary carries the test counters `passed`, `failed` and `ignored`
     at 0 (`report.rs:51`). Agents are told to use JSON, so this is
     their real cost.
   - Fix: leave out an empty `fixes` and the test counters in `check`.
     Shorten the summary, or drop it when diagnostics were printed.
     Measure JSON size in the metric too.

10. **D9. The root key is never used by the checker. [V by agent]**
   `RootKey` is set only at `hd_driver/src/package_result.rs:387`, so two
   bad arms in one match report twice. Key them by item, name and matched
   type, and give one message: "this is a Result: match `.Ok` and
   `.Err`".

<a id="d10"></a>
11. **D10. A syntax error blocks every check of its module and its folder.
    [V]**
    - Evidence: `crates/hd_driver/src/lib.rs:4400` stops the folder
      interface when a module of the folder fails to parse. `:6331` and
      `:6347` block preparation, body checks and finishing for that
      module. The parser has already recovered by then.
    - Effect: the mixed file reports 2 of its 10 mistakes.
    - Fix: check the items whose spans hold no parse error, and give a
      broken item a poisoned signature. The #310 rule that a warm run
      reports what a clean one does still holds.
    - Reference: rustc, Swift and TypeScript check trees with error
      nodes.

### Warm Check And Edit Latency

Profile of 40 warm `hd check --jobs 1` runs, each after a one-line body
edit, on the settled 10k-line package. 4,349 samples, about 32 ms per run.
CI's p50 is 52 ms against a target of 50.

<a id="w1"></a>
12. **W1. Cached interfaces are decoded in full, re-interning every path
    string. [M]**
    - Evidence: `hd_resolve::iface::decode_lazy_in` is 34% of the run,
      and `hd_types::wire::Tables::read` 32%. Within that,
      `PathTable::intern` is 12.8% and `ShardedInterner::intern` 10.3%,
      with a mutex lock on every call even in a serial run.
    - Fix: give each table an offset index and decode only the items
      used. Keep a per-entry string table, decoded once, and intern its
      strings only when first compared. Take no lock when the run has
      one thread.
    - Reference: rustc decodes crate metadata lazily by index. Go's
      indexed export format reads one object on demand.
    - Expected: the largest share of warm edit latency, and most of the
      empty-file startup cost (S2).

13. **W2. The package-result step decodes every folder's items again.
    [M]** `Run::package_result` is 18.2% of a warm edit check, of which
    `decode_items` is 16.5%. A one-body edit should not need every
    interface. Find which reads it needs, and give them their own small
    record.
    - Cause [V, M]: two whole-program checks each decode every module's
      items from its `check` entry, on every run:
      `check_inline_always` (`crates/hd_driver/src/lib.rs:8712`, decode at
      `:8735`) and `check_opaque_cycles` (`:8887`, decode at `:8901`). So
      every item is decoded twice per warm run, whatever changed.
    - Overlap with W1, by profile: of the 34.4% spent in
      `decode_lazy_in`, 14.9% is under `folder_iface`, 12.7% under
      `package_result`, and 6.8% directly under `build_packages_inner`.
    - Fix: compute each check's per-module facts when the module is
      checked, store them in its entry, and combine the stored facts at
      the end. Failing that, decode once and share the result between the
      two checks.

14. **W3. Each cache read opens, seeks and reads a pack file, then copies
    the bytes. [M]**
    - Evidence: `hd_cache::disk::packed::Store::get` is 12.3%; `open`,
      `lseek` and `read` sit under it. `memmove` is 12.3% of all self
      time.
    - Fix: memory-map each pack once per run, or keep the files open, and
      decode from the mapped bytes without copying.
    - Reference: rustc memory-maps `.rmeta`.

15. **W4. A warm check costs O(files). [V, review A2]** Keys of every
    module are recomputed and every `check` entry is looked up. At about
    0.1 ms per file, the 50 ms target fails near 500 files. W1 to W3 lower
    the constant. A daemon, or a record that lists which keys a run read,
    removes the O(files) part.

<a id="i1"></a>
16. **I1. Early cutoff is designed but not built. [V]**
    - Evidence: `check_key(m)` hashes `(folder, deep_hash)` for every
      folder in `closure(m)` (`crates/hd_cache/DESIGN.md` §5.3). The
      recorded-reads cutoff is "not built yet; see #49" (§5.3.1). Per-item
      hashes (`crates/hd_resolve/DESIGN.md` §4.11.2) and per-body reuse
      (`crates/hd_tir/DESIGN.md`, "Planned, not built") are not built
      either.
    - Effect: a public signature edit rechecks 9 modules, against a
      target of 3. A one-function edit rechecks its whole module.
    - Fix: build §5.3.1, then per-item hashes, then per-body reuse.
    - Reference: GHC's interface hashes, rustc's red-green marking,
      Salsa's backdating, Go's build cache.

17. **I2. Files under two seconds old are read and skimmed again. [V,
    review §7]**
    - Evidence: a file whose mtime or ctime is within `SETTLE`
      (`crates/hd_cli/src/disk.rs:132`) gets no stamp. Its skim is not
      recorded, and the whole-package fast path (#1510) is off for that
      run.
    - Effect: this is why `io-per-check` reads 79 files. After a `git
      checkout` or a code generator, an agent's next checks also re-read
      every file for two seconds.
    - Fix: keep the content hash with each recorded skim. An unstamped
      file is read and hashed, and if the hash matches it reuses its
      skim and parse instead of redoing them. That is git's racy-clean
      rule with a content check. The harness should also backdate its
      generated files.

### Cold Check

Profile of one serial cold `hd check --jobs 1` of the 50k-line package,
with std already cached. 4,100 samples, 0.64 CPU-s, 9.8G instructions,
180 MB peak RSS.

<a id="c1"></a>
18. **C1. The system allocator is in 21.6% of samples; a faster one saves
    5-12% of cycles. [M]**
    - Evidence: `libsystem_malloc` frames are in 21.6% of samples, and Vec
      growth (`finish_grow`) is 12.1% inclusive. No crate sets
      `#[global_allocator]`.
    - Tried: `a45fdfc17` built twice from one target directory, as is and
      with `mimalloc = { version = "0.1", default-features = false }` as
      the global allocator in `hd_cli`. Medians, runs interleaved:

      | Workload | Instructions, system to mimalloc | Cycles |
      | --- | --- | --- |
      | cold check, 50k lines, `--jobs 1` | 9,762M to 8,472M (-13%) | 2,334M to 2,231M (-4%) |
      | cold check, 50k lines, `--jobs 4` | 9,999M to 8,596M (-14%) | 2,726M to 2,440M (-10%) |
      | warm check after an edit, 10k lines | 191M to 162M (-15%) | 60M to 57M (-5%, noisy) |
      | empty file, checked, settled | 34M to 34M | 11M to 17M (worse) |

      Peak RSS did not change (173-184 MB at 50k lines). Wall time was
      within noise.
    - Fix: worth taking for `check` and `build` of real packages, and it
      helps S1's contention more than the serial case. Check the tiny-run
      start-up cost first, since `startup` is a target. Pre-size the
      vectors that grow in hot loops (the TIR writer, table decoders); that
      helps with any allocator.
    - Reference: rustc ships jemalloc.

19. **C2. `hd check` serializes TIR it does not use. [M]**
    - Evidence: `hd_tir::wire::write_body` is 10.2% of the cold check.
      About half of that is buffer growth and malloc. The no-emit `check`
      entry without TIR is "planned, not built"
      (`crates/hd_cache/DESIGN.md`, line 70).
    - Fix: write TIR only in `build` and `test`, or write it after the
      output on an I/O thread. Reuse one encode buffer per worker.
    - Reference: rustc's `FileEncoder` writes through one fixed buffer.

20. **C3. The TIR verifier runs on every body in release builds. [M]**
    - Evidence: `hd_tir::ir::verify` is 5.1% of the cold check, called
      unconditionally from `TirBuilder::finish_tolerating`
      (`crates/hd_tir/src/ir.rs:1781`). It is 28% of derived-method
      generation.
    - Tried [M]: one build of `a45fdfc17` with the call behind an
      environment switch, run both ways on a cold serial check of 50k
      lines. With the verifier, 10,840M and 10,751M instructions; without,
      10,148M and 10,102M. That is 650-740M instructions (6.5%) and 5-9% of
      cycles. The switch's own `var_os` per body adds to both runs equally.
    - Fix: verify in debug builds, in the checked profile, and under an
      `HD_VERIFY` switch that CI's conformance run sets.
    - Reference: rustc runs MIR validation only in debug builds or with
      `-Zvalidate-mir`. LLVM verifies only on request.

21. **C4. 385 `@derive(Eq)` enums cost 21% of the cold check. [M, V]**
    - Evidence: `Run::opt_ins` is 21.2%. Within it:
      - `structure_bodies` with `hd_structure::derivation`, 10.0%. It
        generates the hidden `Structure` methods as TIR and returns them.
        It reports no diagnostic (`crates/hd_check/src/structure.rs:530`).
      - `check_opt_in`, 3.9%. It type-checks the derive template's bodies
        once per deriving type (`crates/hd_check/src/derive.rs:409`). This
        is where a member-bound failure is found ("the member `x` of type
        T does not implement Eq").
      - Derived-header lowering, 3.7%.
      That is about 0.35 ms per four-variant enum.
    - Fix:
      - C3 removes about 3 points.
      - `structure_bodies` produces code and no findings, so `check` can
        skip it and `build` can produce it when it reaches an instance.
      - The template check per deriving type could run once per template.
        Failures already come from a direct query per member,
        `member_fails(mem.ty, b)` (`crates/hd_check/src/derive.rs:946`).
        The body check only discovers the bounds `b`, through
        `walker_bounds` (`:757`).
      - Checked [V]: `walker_bounds` chooses the implementation by solving
        `w: Protocol[T]` with `T` left as the template's own parameter
        (`Param(template, 0)`, `:764`, query at `:779-788`). `w` is the
        walker's type in the template body. The deriving type's facts
        (skips, declared defaults, `fills_defaults`) are read only after
        that, in the per-member loop (`:900-950`). So the bounds depend on
        the template alone.
      - Fix: compute `found` once per template, then run only
        `member_fails` and the fact loops per deriving type. The checked
        template bodies are kept per type for code generation only, so a
        `check` run can skip them as it skips `structure_bodies`.
    - Prototype built and tested [M]: at `101151bf0`, an environment switch
      in the driver's opt-in loop (`crates/hd_driver/src/lib.rs`, the
      `checked.failures.is_empty()` branch) that skips the per-type
      instance bodies and `structure_bodies`. The template check still
      runs. One binary, both ways, cold serial check of 50k lines:

      | | Instructions | Cycles | Peak RSS |
      | --- | --- | --- | --- |
      | derived bodies built | 9,764M to 9,844M | 2,318M to 2,334M | 181 MB |
      | derived bodies skipped | 7,507M to 7,553M | 1,716M to 1,747M | 108 MB |

      That is -23% instructions, -25% cycles and -40% peak memory.
      Correctness: `hd check --tests` gives byte-identical output both ways
      on all 290 conformance fixtures that use `@derive`. The switch alone
      is not shippable, because a later `build` would reuse a `check` entry
      without these bodies. The shippable form is the `check` entry without
      TIR that `crates/hd_cache/DESIGN.md` plans (C2), with derived bodies
      produced at `build`.
    - Reference: Swift checks each stored property's conformance directly,
      then synthesizes the body as already-typed AST with no constraint
      solving.

22. **C5. Result inference for private functions without a result type
    is 18% of the cold check, and serial. [M, review A8]**
    `hd_check::results::infer_results` is 17.9%. This is ordinary body
    checking. The cost is that it runs on one core per module (S3).

23. **C6. Sources are read with a `stat` and a read each, then copied.
    [M]** `DiskSources::read` is 5.8% of the serial cold check, about
    46 ms for 388 files, while `cat` reads them all in about 20 ms.
    `read_regular` makes a `metadata` call and then `fs::read`, and
    `Arc::from(Vec)` copies the bytes again
    (`crates/hd_cli/src/disk.rs:111-115`, `:210`). Fix: one `open`, then
    `fstat` on the handle and a read into an `Arc<[u8]>` of known size.
    Read in parallel (S4).

### Robustness Of Compile Time

24. **R0. Native recursion over left-deep chains crashes. [V, review F3,
    F4, F5]** A chain of private functions with omitted result types
    aborts at 500. `x + x + …` aborts at 100,000, and `.trim()` chains at
    100,000. Type alias chains abort at 20,000. The browser build fails
    5 to 25 times earlier. The parser's nesting guard does not cover
    binary and postfix loops (`parser/exprs.rs:730`, `:839`). Fix: walk
    these chains with an explicit stack, as
    `crates/hd_check/DESIGN.md` §11.3 plans. AGENTS.md forbids native
    recursion to an input-controlled depth.

25. **R0b. Depth caps return a default answer instead of an error. [V,
    review F1, F2]** A 64-field struct pattern makes later arms
    unreachable, and a 33-level nested move is missed. About 18 such
    sites are listed in the review. Fix: a diagnostic or a worklist at
    each.

26. **R0c. Exponential and quadratic checker paths. [V, review F8 to
    F16]**
    - Doubled tuple types, a walk per tree node (F8, F14, `unify.rs:233`).
    - Speculative method trials re-check the argument per candidate (F9,
      F10, `trial.rs:400`, `:483`).
    - Init summaries sorted by formatted path (F11).
    - An impl lookup by linear `find` (F12, `lib.rs:8441`).
    - `line_column` scans from the file start per diagnostic (F13,
      `lib.rs:687`).
    - A per-body field walk (F15).
    - Usefulness over tuple columns, with no fuel charged (F16).
    - Fixes: hash-consed type walks with a visited set, an argument cache
      across trials (rustc's `DeepRejectCtxt` fast reject is planned in
      `crates/hd_check/DESIGN.md`), an id index, and a line-start table.
      Count fuel in type nodes too.

27. **R0d. Pathological tests cover only `hd check`. [V, review §6]**
    Run the same shapes through `hd build`, `hd fmt` and the browser
    build, and count work in type nodes and diagnostics, not only fuel.

### Other Pillar 1 Targets

28. **K1. Overflow checks stay on additions the loop already bounds.
    [V]** `check-cost` is 1.93x against 1.3x. `hd_wasm` drops a check
    only when an operand is a constant (`checked_by_constant`,
    `crates/hd_wasm/src/emit.rs:4909`), and `hd_opt` has no range
    analysis. Fix: a range-fact pass on TIR, so `j + 1` under
    `j < len` is unchecked. Reference: LLVM SCEV and
    CorrelatedValuePropagation, V8 TurboFan range types.

29. **K2. `iterator-chain` exceeds a fixed spec limit. owner [V]** The 2x
    fixture has 100 adapters, and `trait.bound.depth.limit` makes 64 an
    error. Keep the rule and scale the fixture by the number of chains,
    or change the rule.

30. **K3. `lookup-latency` measures commands that do not exist. owner
    [V]** #181 is closed, but `hd callers` and `hd needs` are in no spec
    section and no build. Specify them, or retire the metric.

31. **K4. `test-latency` (266 ms, target 300 ms) is mostly Node. [M]**
    See E1 and S5.

## Pillar 2: Agent Scalability

<a id="s1"></a>
32. **S1. Parallel checking spends 40-45% more cycles on the same work.
    [M]**
    - Evidence: a cold check of 50k lines, std cached, median of 5:

      | `--jobs` | Instructions | Cycles | User CPU (hyperfine) |
      | --- | --- | --- | --- |
      | 1 | 9,777M | 2,439M | 620 ms |
      | 4 | 9,974M | 3,406M | 1,147 ms |
      | 8 | 10,092M | 3,544M | 1,158 ms |

      Instructions grow 2-3%, so workers do not spin. Cycles grow 40-45%,
      so each instruction costs more when threads share data.
    - Where the extra CPU goes [M]: two samply profiles, `--jobs 1` and
      `--jobs 4`, each sample weighted by the CPU time its thread used.
      The 4-job run used 196 ms more CPU (824 ms against 628 ms). Of
      that extra:
      - 36.6% is in the system allocator (C1);
      - 19.4% is mutex waits, hand-offs and unlocks;
      - 6.9% is `memmove`.
      Of the mutex time, 77% is the path and symbol interners
      (`PathTable::find` 21.8%, `ShardedInterner::find` 20.9%,
      `PathTable::intern` 20.8%, `ShardedInterner::intern` 13.8%), and
      19% the type pool (`Types::intern` 15.2%, `Cols::push` 4.2%).
    - Cause [V]: every interner shard is a `Mutex<HashMap>`, and lookups
      lock it as inserts do (`crates/hd_intern/src/paths.rs:108`, `:272`).
      A global `append` mutex guards the text arena (`:251`).
    - Correction to an earlier reading: the empty-file profile showed 86%
      of samples in idle rayon workers. Those samples are sleeping
      threads; samply records them too. On the empty check, 8 jobs cost
      88M instructions against 86M for 1.
    - Effect: `concurrency` CPU grows as N^1.11, and p95 is 2.23x at
      N = 4. The default is one thread per core, capped at 8
      (`crates/hd_cli/src/main.rs:379`). So N agents start 8N threads, and
      each pays the cycle cost above on a machine with fewer free cores.
    - Tried, no gain [M]: every interner shard as a `RwLock`, with lookups
      and hits under the shared lock (at `101151bf0`). Cold check of 50k
      lines at `--jobs 4`: 3,012M and 3,252M cycles against 3,075M and
      3,108M without it, which is within noise. A shared lock still writes
      a shared counter on every acquire, so the cache line bounces as
      before. That rules out the cheap fix.
    - Fix:
      - Lower the contention with lookups that write nothing shared: a
        per-thread cache of resolved ids in front of the shared table, or
        a map with lock-free reads. The same for the type pool. Also a
        thread-caching allocator (C1, which cut 4-job cycles by 10%).
      - Run serially until the task graph has more ready work than one
        thread can do, then grow the pool.
      - Size the pool by the machine's free cores, not its total. For many
        processes, share one token pool, as cargo and make do with a
        jobserver.
    - Reference: rustc's parallel front end defaults to one thread and
      uses sharded, mostly lock-free interners. Cargo shares parallelism
      through the jobserver.

33. **S2. Checking an empty file just written costs 108M instructions and
    17 MB RSS. [M]**
    - Evidence: `startup` measures 17.0 MB against a target of 10 MB. The
      same check on a file written more than two seconds earlier takes
      7.0 MB and 45M instructions, which passes. The metric writes
      `empty.hd` and measures at once
      (`bench/metrics/scripts/startup.ts:81`), so it gets the slow path
      (I2). So does an agent that writes a file and checks it.
    - The slow path decodes std's interfaces
      (`folder_iface` 19.4%, `Tables::read` 11.5%) and std's skims
      (`StdSkims::get` 13.8%) for a file that uses none of them. Heap peaks
      at 8.3 MB. `embedded_std()` holds two 1.1 MB copies of std's text,
      the sources and the texts map (`crates/hd_driver/src/lib.rs:109`),
      and hashes the whole set.
    - Fix: W1's lazy decode, and only the prelude's folders. Compute the
      std hash at build time. Borrow the static text instead of copying
      it.
    - Reference: Swift and GHC ship the standard library as prebuilt
      interfaces.

34. **S3. Result inference runs on one core per module. [V, review A8]**
    With C5, 18% of a cold check is serial within each module. Infer the
    omitted results of independent private-function groups in parallel
    (the strongly connected components of the call graph).

35. **S4. A third of a parallel cold check's wall time runs on one
    thread. [M]**
    - Evidence: a 4-job check of 50k lines spans 365 ms. In 10 ms
      buckets the timeline is
      `1111154411114444444444444444444421111`. The single-thread
      stretches are reading and skimming sources first (`FileSkim::of`,
      `DiskSources::read`), a run of `folder_iface` with nothing else
      ready, and then the cache publish (`Store::publish`,
      `replace_file`) with `main_visibility`. Speedup is 2.0x at 4 jobs
      and 2.07x at 8.
    - Fix:
      - Read and skim files in parallel.
      - Prioritize tasks: `FolderIface` by height in the folder DAG,
        `Body` by size, and the tasks that unblock many others. This is
        "planned, not built" in `crates/hd_sched/DESIGN.md` §6.3; every
        task runs at priority 0 today.
      - Publish the cache after printing the output, on an I/O thread.
        That is also planned (`crates/hd_cli/DESIGN.md`, line 46).
    - Expected: `parallel-speedup` from 1.92x to the 2.4x target.

36. **S5. `hd test` starts parallel workers that buy nothing on small
    suites. [M]** A warm `hd test` of 157 tests takes 0.57 s and 0.89 CPU-s
    at the default job count, and 0.52 s and 0.31 CPU-s at `--jobs 1`.
    Each worker is a Node process that compiles the module again
    (`test_cmd.rs:1601-1636`). Fix: size workers by the number of cases
    and their measured cost, and share one compiled module (E2).

<a id="s6"></a>
37. **S6. Neither cache stops two processes from doing the same work.
    [V]**
    - Evidence: dependency fetches stage and rename, and "the first rename
      wins" (`crates/hd_deps/src/cache.rs:104-118`). The compilation
      cache is first-writer-wins at publish (`crates/hd_cache/DESIGN.md`
      §5.4).
    - Effect: 8 concurrent fetches open 16 connections. 8 concurrent
      checks write 9 files where one writes 7. All N processes compute
      std.
    - Fix: a per-key `flock` and a second look before fetching. A lease on
      expensive shared compile keys, std's first.
    - Reference: cargo's package-cache lock, Go's `lockedfile` in the
      module cache.

38. **S7. A REPL input costs O(history). [V, review A9]** Each input
    uses the one before it (`session_uses`, `lib.rs:1532`), so
    `repl_texts` (`:1477`) reaches and rechecks every earlier input.
    Build time per input grows from 89 ms to 982 ms, and RSS by 96 MB.
    Fix: one session-scope interface holding the latest binding of each
    name. Reference: GHCi, the Swift REPL, IPython.

39. **S8. The back half is whole-program, serial and uncached. [V,
    review A1, A5]** A one-body edit re-emits the whole program on one
    thread: 0.17 s at 10k lines, 3.3 s at 200k. The dev pipeline also
    runs the link-time optimizer. Fix: make the instance the cached,
    relocatable unit again (`code_key` exists and nothing calls it), and
    move cleanup rules to the optimized pipeline. This drives
    `resources` for `build` and `test`, and test latency.

40. **S9. The binary is 18.0 MB, and stripping alone makes it 14.8 MB.
    [M]**
    - Evidence: there is no `[profile.release]`. Code is 10.5 MB.
      Constants are 1.7 MB, which include std's source and the 464 KB
      JS host. Adding wasmtime takes it to 33 MB (review A6).
    - Effect: `disk` measures 20.1 MB on CI against a target of 10 MB.
    - Fix: `strip = "symbols"`, `lto = "thin"` and `codegen-units = 1`
      for the shipped build, then measure. Whether the metric should
      count a shared toolchain at all is **owner**.

<a id="m1"></a>
41. **M1. A cold check holds its cache output in memory until the end.
    [M]**
    - Evidence: a serial cold check of 50k lines peaks at 154 MB of live
      heap (180 MB RSS). By call site at the peak:
      - 30.0 MB under `Run::put`: entries encoded by
        `hd_cache::encode_entry`, held by the store until it publishes
        (`crates/hd_driver/src/lib.rs:3871`).
      - 30.0 MB under `Run::module_finish`: the section buffers those
        entries were encoded from.
      - 17.5 MB in `Run::body`: a `Vec<hd_tir::Body>` of every module's
        checked bodies.
      - About 1.5 MB per site for syntax trees and token buffers, and
        2.6 MB for lowered items.
    - Effect: about 3 KB per line, and `resources` at 161 MB (check) and
      275 MB (test) for 50k lines on CI.
    - Fix: append each module's entries to the pending pack when the module
      finishes, and drop the sections and TIR once written. Encode
      straight into the pack's buffer, not into a copy. This also moves
      the publish off the end of the run (S4).
    - Reference: rustc's incremental cache streams query results through
      one `FileEncoder`.

42. **S11. Counters exist but nothing shows them. [V]** The driver counts
    parsed modules and tasks, but no flag or environment variable prints
    them, and there is no timing trace. Add `HD_TRACE=file` with
    trace-event JSON per task, plus `--timings`. Reference: `cargo
    --timings`, rustc `-Z self-profile`, Clang `-ftime-trace`.

## Pillar 3: Artifact Quality

### Engine And Host

<a id="e1"></a>
43. **E1. Every run and test program starts a Node process and compiles
    with no cache. owner [V, M]**
    - Evidence: CI builds without the `wasmtime` feature, and Node is the
      default engine (`crates/hd_cli/src/engine.rs:229`). Bare
      `node --experimental-wasm-jspi -e 0` takes 28 ms. A hello run takes
      about 69 ms, and `hd check` of it 4 ms.
    - The host's share, in instructions over bare Node with the JSPI flag
      [M]:

      | Step | Instructions |
      | --- | --- |
      | the JSPI flag itself | 5M |
      | loading the run's host modules, no program | 54M |
      | running a program with no imports (115-byte module) | 62M |
      | running the `map` program at n = 1 (6.5 KB, imports the runtime) | 112M |

      So a program that uses the runtime pays about 50M more than one that
      does not, before it does any work. `node --cpu-prof` puts 28% of the
      host's samples under `createStartupHost`. Another 28% is under
      `worker-control.js`, whose import of `node:worker_threads` alone
      costs 8M instructions. 16% is compiling the 29 separate ES modules.
    - Node's compile cache (`NODE_COMPILE_CACHE`), tried with the host at a
      stable path, saved 9M of those instructions (372M to 363M) [M].
    - Effect: start to first output is 56 ms (target 5 ms), and set-up
      per test program and per doc test is about 150 ms (target 20 ms).
      The review measured wasmtime at 0.02 s for hello and about 13 ms
      per test program.
    - wasmtime measured [M]: a build with `--features wasmtime` (33.3 MB
      binary) at `101151bf0`, the same programs through the same binary,
      at a load of 6 to 7. Medians of 15 wall-clock runs, except the
      tiny program (mean of 20) and `hd test` (mean of 7):

      | Run | `HD_ENGINE=node` | `HD_ENGINE=wasmtime` |
      | --- | --- | --- |
      | empty program, whole run | 47.3 ms | 6.8 ms |
      | `size-startup-heap` tiny program (`println("ready")`) | 89.8 ms mean, 50.1 ms best | 10.5 ms mean, 9.2 ms best |
      | `fib`, whole run | 51.9 ms | 12.8 ms |
      | `sum` | 61.0 ms | 12.9 ms |
      | `string_build` | 51.6 ms | 11.7 ms |
      | `map` | 60.7 ms | 13.0 ms |
      | `sort` | 53.0 ms | 19.3 ms |
      | `hd test`, 10k-line package, 157 tests, warm | 409 ms, 1.25 CPU-s | 120 ms, 0.10 CPU-s |

      Work over the empty program is lower on wasmtime for `sum` (6.1 ms
      against 13.7) and `map` (6.2 against 13.4), since Cranelift compiles
      ahead of time and E6 does not arise. It is higher for `sort` (12.5
      against 5.7), where TurboFan's code beats Cranelift's. The tiny
      program reaches about 10 ms, not the 5 ms target. What remains is the
      per-run translation that the function-level code cache does not
      skip (`crates/hd_run_wasmtime/src/code_cache.rs:1-11`).
    - Winch measured for compile time [M]: `wasmtime compile` 50.0.1 with
      `-C compiler=cranelift` or `-C compiler=winch`, no cache, GC and tail
      calls off as hd's engine sets them. Mean of 15, CLI start-up
      included, so CPU time is the useful column:

      | Module | Cranelift wall, CPU | Winch wall, CPU |
      | --- | --- | --- |
      | tiny program, 1,588 B | 15.8 ms, 8.2 ms | 8.1 ms, 6.3 ms |
      | `sort`, 12,363 B | 10.4 ms, 19.6 ms | 10.1 ms, 9.7 ms |
      | `map`, 6,497 B | 16.2 ms, 12.3 ms | 10.4 ms, 6.7 ms |

      Winch halves compile CPU on the larger modules. Its code's run time
      was not measured: the CLI cannot run hd modules without hd's host
      imports, and hd's engine has no Winch option. So Winch suits a dev
      tier, and the serialized-module cache helps either compiler.
    - Fix: run in-process on wasmtime with a serialized-module cache.
      Consider Winch as the dev tier, which compiles the 200k-line module
      in 0.23 s against Cranelift's 0.83 s (review A7). The owner decides,
      since wasmtime triples the cold build and adds 15 MB.
    - If Node stays, in order of measured effect:
      - Build the host's providers and ABI wrappers on first use, not all
        at start-up (`createStartupHost`, and E3).
      - Bundle the host into one module, and load `node:worker_threads`
        only when a feature starts a worker.
      - Take a V8 start-up snapshot of the loaded host
        (`node --build-snapshot`) and start from it.
      - Write the host once to a stable cached directory, not a fresh temp
        directory per run (29 files, 464 KB written and removed each run,
        `crates/hd_cli/src/node.rs:196-219`). On its own this saves little,
        as the compile-cache result shows.

44. **E2. Each test worker and each test program compiles the module
    again. [V]** Test programs run one after another (`test_cmd.rs:1565`).
    Fix: one host process per `hd test`. Compile each module once and send
    the `WebAssembly.Module` to workers, which does not recompile. Run
    programs concurrently, and link doc tests into one module. Reference:
    Rust 2024 merged doctests, nextest.

45. **E3. A fresh host and instance per unit test case. [V]**
    `host/cases.ts:283-297` rebuilds every provider and ABI wrapper per
    case. `cases.ts:272` and `core.ts:1562` copy the export object, which
    has one entry per test, twice per case. So a suite's cost is
    quadratic in its size. Fix: build the import object once per worker
    and reset only per-case state. Expected: `unit-test-perf` under 1 s.

46. **E4. The host ABI decodes and re-encodes every call. [V]**
    - `println` makes two host calls (`lib/std/console.hd:114-119`). Each
      decodes UTF-8, encodes it again with `Buffer.from`
      (`host/output.ts:158`), makes one unbuffered `writeSync`, and
      encodes a `Result` with a new encoder (`core.ts:629-644`). The two
      writes are a spec rule (`module.console.println-two-writes-sync`),
      so buffering is **owner**.
    - `read_text!` runs `realpath` per path component even under an
      unrestricted grant (`host/node-files.ts:41-75`). It adds a
      `statSync` and creates a new `TextDecoder` per call.
    - `list_dir!` allocates two `Buffer`s per sort comparison and encodes
      every integer as a BigInt LEB (`host/wire.ts:65-80`).
    - Fix: a guest-side output buffer. Skip resolution under an
      unrestricted grant, and use `fstat` on the open file. Precompute
      sort keys. Use fixed-width integers on the wire. Reference: Rust's
      `LineWriter`, cap-std's `openat`, the component model's flat ABI.

47. **E5. Property tests cross into the host on every draw. [V]**
    `self.runner.record(value)` per draw (`lib/std/testing.hd:131`), and
    `debug(value)` built and sent for every case, passing or not
    (`:651`, `:673`). Fix: draws in a guest list, handed over once per
    case. Render the input only on failure, since replay is
    deterministic. Run the case loop in the guest. Reference: proptest,
    Hypothesis.

<a id="e6"></a>
48. **E6. A hot loop in `main` stays in V8's baseline tier. [M]**
    - Evidence: V8 compiles Wasm with Liftoff first and moves a function to
      TurboFan only for its next call. It has no on-stack replacement for
      Wasm, and `main` is called once, so its loop runs as baseline code
      to the end. Work in instructions over the empty program, with each
      flag:

      | Case | Default | `--no-liftoff` | JS |
      | --- | --- | --- | --- |
      | `sum` (10M additions in `main`) | 217M | 52M | 90M |
      | `fib` | 46M | 43M | 142M |
      | `string-build` | 68M | 82M | 145M |
      | `map` | 132M | 127M | 62M |
      | `sort` | 116M | 174M | 367M |

      `sum` costs 21 instructions per iteration in the baseline tier and 5
      in TurboFan. `--no-liftoff` compiles everything at the top tier,
      which costs more for programs with more code, such as `sort`.
    - Effect: `sum` at 1.45x Node on CI. Any CLI program whose work is one
      long loop in `main` runs at baseline speed.
    - Workarounds tried on `sum` [M]:

      | Shape | Instructions | Cycles |
      | --- | --- | --- |
      | the loop in `main` (today) | 207M | 69M |
      | `main` calls a step function per iteration | 204M | 69M |
      | `main` calls `chunk` ten times, but hd inlines `chunk` into `main` | 219M | 69M |
      | a recursive `chunk` that hd cannot inline, 16 leaf calls | 73M | 23M |
      | JS, for reference | 90M to 217M | 55M |

      Raising V8's tier-up eagerness (`--wasm-tiering-budget=10000`,
      `--wasm-sync-tier-up`) did not help, because the code that runs is
      still the instance of `main` already on the stack.
      `--trace-wasm-compilation-times` shows TurboFan compiling `main`
      during the run, and that code is never entered.
    - A second finding: hd's inliner moved `chunk`'s loop into `main`,
      which on V8 pins the loop to baseline code. Inlining a callee that
      holds a loop into a function that runs once is a loss there.
    - Fix, in order of preference:
      - E1, since wasmtime compiles with Cranelift ahead of time.
      - In the emitter, for the Node engine: strip-mine a long loop in a
        function that runs once. The loop then calls an outlined,
        never-inlined function for each block of iterations. The table's
        last hd row shows the effect: 3x fewer cycles. Also, do not inline
        loop-holding callees into `main` or init code.
      - Prototype of the inlining half, built and tested [M]: at
        `101151bf0`, an environment switch in `fits`
        (`crates/hd_opt/src/inline.rs`) that refuses any callee whose body
        holds a `Tag::Loop`. Instructions of work, two rounds:

        | Program | Inlined (today) | Not inlined |
        | --- | --- | --- |
        | `sum` as `main` calling `chunk` ten times | 215M, 208M | 68M, 71M |
        | `sum` with its loop in `main` | 213M, 208M | 210M, 212M |
        | `fib`, `string_build`, `map`, `sort` | 46/68/134/117M | 42/64/134/118M |

        The chunked program runs at a third of the instructions, and the
        others do not change. All 300 sampled runtime conformance fixtures
        give the same output with the switch on. The shippable rule refuses
        such callees only into a function that runs once.
      - The strip-mining half, for a loop written directly in `main`, was
        not built. Its open design points are how long a loop must be to
        pay for the call per block, and how to tell at compile time that a
        function runs once. Its effect is bounded by the hand-written row
        above: 3x fewer cycles.
      - In the host, `--no-liftoff` for release builds only, accepting the
        compile cost shown for `sort`.

### Standard Library Engines

<a id="r1"></a>
49. **R1. Regex search always runs the Pike VM. [V]** `find_all` runs it
    on every call (`lib/std/regex.hd:1443-1554`), copies capture slots
    per step and per thread, and tests classes by a linear range scan.
    There is no DFA, and on text that is mostly letters the first-byte
    prefilter skips nothing. Fix: a lazy DFA over byte classes with a
    capped state cache, for `is_match`, `find`, `find_all`, `split` and
    `replace`; the Pike VM only when captures are asked for. Reference:
    the Rust `regex` crate. Also, `validate.Pattern` compiles its regex
    again on every `apply` (`validate.hd:717`).

50. **R2. JSON decodes in two passes and checks keys at run time. [V]**
    - Decode: a recursive validation scan builds an index
      (`lib/std/json.hd:580-662`), skipping whitespace one byte at a time.
      Then `TextReader` walks the text again, matches fields by comparing
      bytes, and records a path step per member (`:3339-3407`).
    - Encode: a frame-stack state machine returns a `Result` per call
      (`:1693-1930`), runs a duplicate-key check on every member
      (`:1818-1833`), and escapes each name again.
    - Fix: when `T::regular()` holds, skip the duplicate check and write
      keys quoted once per type. Add a one-pass typed reader. Reference:
      serde_json.
    - Also quadratic: `JsonObject.remove` rewrites every later key's index
      (`json.hd:111`; 8,000 removals take 1.8 s).

51. **R3. Iterator adapters do work per step. [V, review §9]** Each step
    returns an `Option`, copies the item and calls a closure
    (`lib/std/iter.hd:253`, `:494`). Over 4M `i64`, a `for` loop takes
    2 ms and `iter().map().sum()` 19 ms. The cure is optimizer work (O3,
    O4), not rewriting adapters.

52. **R4. String operations still cost 3 to 5 times Node. [M]**
    - Today's numbers, `bench/micro/costs.ts` at `a45fdfc17`, minimum of
      seven runs. The load average was 17 to 41, so treat them as rough:

      | Case | hd/JS on 2026-11-02 | hd/JS today |
      | --- | --- | --- |
      | `string-split` | about 4.6 | 4.68 |
      | `string-trim` | about 4.6 | 2.91 |
      | `string-eq-short` | about 5 | 3.08 |
      | `string-eq-long` | about 5 | 4.68 |
      | `map-get-key-string` | 5.24 | 1.37 |
      | `map-get-key-i32` | below 1 | 2.22 |

      The update section of `string-collection-costs.md`, also rerun today,
      has `string-add-build` at 2.40x (it was 1,473x) and the borrowing
      list loops at about 0.46x.
    - Cause, partly [M]: long equality costs about 175 instructions per
      64-byte comparison. The byte scan already uses SIMD
      (`crates/hd_wasm/src/scan.rs`). So the cost is around the scan: two
      calls (`string_equal`, then `rt:bytes_mismatch`), each with a frame
      prologue. That matches W6 of the layout review. Baseline-tier code
      in `main` (E6) explains only 9-13% here, measured with
      `--no-liftoff`.
    - Fix: inline `string_equal` and the length test into the caller, and
      call the scan only when the lengths match (O2, W6).
<a id="r5"></a>
53. **R5. Swiss-group map indexing made small-map hits 28% dearer. [M]**
    - Evidence: `map-get-key-i32` does 10M `get` hits on a two-entry map.
      Compared by instructions and cycles of work over the empty program,
      median of 7, the 11-10 compiler did 1,629M and today's 2,016M (4.8x
      JS's 418M). `git bisect run` over the 160 commits between them
      (script `step.sh`: build, measure, fail above 1,820M) names
      `f13fdb35c`, "Index maps with Swiss groups of 8 control bytes":

      | Commit | Instructions | Cycles |
      | --- | --- | --- |
      | `4c0be8644`, its parent | 1,578M | 255M |
      | `f13fdb35c` | 2,015M | 303M |

    - Cause: the commit message prices misses on a 500k-entry map, where
      a probe fell from 2.32 slots to 1.05 groups. A hit on a small map
      now does the group setup on every lookup: compute the group, load 8
      control bytes, run the byte match, then load the entry.
    - Fix: keep the group probe, and give a short path to the cases it
      does not help. A map of at most 8 entries fits one group, so look
      up its hash's tag directly, or scan the dense entries without the
      index, as Swift does for small native dictionaries. Measure the hit
      phase, not only the miss phase, of `race hashmap`, and add this
      micro case to the commit's evidence.
    - Bound on the gain [M]: the same 10M lookups written in hd as a
      linear scan of a two-element key list, with no hashing and no index
      (`fn lookup(keys: &List[i32], values: &List[i32], key: i32) -> i32?`),
      do 801M instructions and 126M cycles. The map does 2,015M and 298M.
      So a scan path for small maps can remove up to about 60% of a
      small-map hit's cost. That is more than the 28% the Swiss groups
      added.
    - Prototype built and tested [M]: 43 lines in `rt:map_find`
      (`crates/hd_wasm/src/map.rs`, `find`), at `101151bf0`, for integer
      keys. A table of capacity 8 or less first scans its dense entries,
      comparing the stored hash and then the key; a miss falls through to
      the group probe.
      - `map-get-key-i32`: 2,015M to 1,606M instructions (-20%), about
        -15% cycles, which recovers the regression (1,578M before
        `f13fdb35c`). The 100k-entry `map` benchmark did not change.
      - Correctness: all 282 runtime conformance fixtures that use a map
        give the same output with and without the patch.
      - The rest of the 60% bound is the hash, which the caller still
        computes (functions 36 and 6 in the profile, 19% of samples). A
        full fix also skips hashing for small maps on lookup, and covers
        string keys.

### Optimizer And Emission

54. **O1. The heap allocator: page search quadratic in pages, and medium
    blocks taking whole 64 KiB pages. [V, layout L1 to L3]** 8,000 lists
    of 520 `i32` take 4.1 s, and 65,536 blocks of 2.1 KiB exhaust the
    address space. A list's capacity ignores its block's slack. Fix: a
    page hint that advances, and medium size classes.
    - These are **not** the cause of `map` at 2.70x Node [M]. `map` grows
      linearly: in instructions per operation, hd costs 1,771 at
      n = 25,000, 620 at 100,000 and 352 at 400,000. JS costs 512, 335
      and 236. The marginal cost from 200,000 to 400,000 is about 255
      per operation against JS's 202, or 1.26x. The gap at the benchmark's
      size is the fixed host and runtime cost of E1, plus harness bias H4.

55. **O2. Emitted-Wasm gaps. [V, layout W1 to W9]** In order of value: a
    value-origin fact ("starts a block", "nonzero") that turns slow count
    calls into known ones and removes null tests (W2, W5); local value
    numbering over memory plus merging same-condition `if`s (W3, W4);
    count-site locals (W1); count pairing after inlining (W7); and the
    small ones, W6, W8 and W9.

56. **O3. The planned optimization passes are stubs. [V]** In
    `crates/hd_mono/DESIGN.md`'s table, constant folding during the walk,
    constant hoisting (so `.Err(ParseError.Empty)` allocates nothing),
    trivial inlining, scalar replacement and known-vtable
    devirtualization are "planned, not built". `escape`,
    `scalar_replace`, `use_counts` and `inline_plan` are declaration
    markers in `crates/hd_opt/src/passes.rs`. Order: escape analysis with
    scalar replacement first, since it removes adapter closures and
    temporary boxes (R3). Reference: LLVM SROA, the Swift SIL optimizer.

57. **O4. The iterator-chain schedule is an unbuilt proposal. [V]**
    Inline, scalar-replace, devirtualize, repeat (`hd_mono` DESIGN). This
    is what makes adapter chains cost what the loop costs.

58. **O5. Suspension lowering emits code quadratic in the number of
    awaits. [V, review F6]** At 400 awaits the module is 9 MB, and
    wasmtime rejects it at 500. Fix: save only what is live across each
    point, as `crates/hd_wasm/DESIGN.md` §14.3 says.

59. **O6. Quadratic back-half paths on one large body. [V, review F7,
    F19 to F22]** `Func::drain` per candidate in `opt.rs:1680` (F7), a
    sort under `opt::locals` (F19), `Topology::build` and `slots`
    (F20), `promotable_args` (F21), and a wide struct literal (F22). Each
    is a separate cause with a generated width test to add.

## Found By An Independent Review

A fresh agent read the four audits' item lists and then hunted the code for
optimizations none of them cover. It reported 8. Each code path below was
rechecked at the cited lines. Two were measured.

<a id="n1"></a>
60. **N1. Float text and case mapping cross into the JS host on every
   call. [V]**
   - Evidence: `Display for f64` calls `f64_text`, which is
     `@intrinsic("format_f64")` (`lib/std/format.hd:584`). So are
     `format_f64_fixed`, the `parse_f64` fallback, and `lower`/`upper`.
     Each one is a host primitive (`crates/hd_wasm/src/emit/text.rs:454`).
     The host encodes its answer through a new `Enc`, a `TextEncoder`, a
     BigInt length and two copies (`host/core.ts:506-511`).
   - Effect: every float written to JSON or text pays a host round trip.
     The `serde-throughput` fixture writes an `f64` on every line.
   - Fix: shortest-float formatting in std (Ryu or Schubfach), and ASCII
     case mapping in the guest, with only non-ASCII text sent to the host.
     A core-library change that also removes intrinsics (#1206).
   - Reference: Rust uses Ryu and Go uses `strconv`, both written in the
     language.

61. **N2. Reading all of stdin is quadratic on the Node host. [V]**
   `pending = Buffer.concat([pending, chunk])` runs on every 64 KiB fill
   (`host/node.ts:247`), and reading to the end loops on it. That copies
   about 0.8 GB for 10 MB of input. Fix: collect the chunks and join them
   once, and keep the offset of the newline search.

62. **N3. std builders grow by `push`, and in-place sort copies twice.
   [V]**
   - `List.sort` is `*self = self.sorted()`
     (`lib/std/collections.hd:2114`). `sorted` copies into a `UniqList`,
     sorts, then pushes every element into a new `List` with no `reserve`.
   - `map`, `reversed`, `to_list`, `keys` and `values` push with no
     `reserve`. `join` builds 2n-1 pieces before concatenating
     (`text.hd:669-678`). `Iterator` has no size hint, so `collect` grows
     step by step (`iter.hd:392`).
   - Fix: `reserve(n)` in each builder, an in-place sort of a unique
     buffer, one sized copy in `join`, and a size hint on `Iterator`.
   - Reference: Rust's `sort` and `with_capacity`, and `size_hint` with
     `TrustedLen` in `collect`.

<a id="n4"></a>
63. **N4. `hd check --tests` runs the driver twice. [V, M]**
   - Evidence: `check_one` calls `check_role` with `Goal::Analyze`, then
     again with `Goal::CheckTests` (`crates/hd_cli/src/check_cmd.rs:248-266`),
     and merges with a linear `shown.contains`. The `hd-cli` skill tells
     agents to add `--tests` while they work.
   - Measured: after a body edit on the 10k package, `hd check` takes
     39.7 ms and `hd check --tests` 55.0 ms (+39%). `edit-latency` measures
     only the first.
   - Fix: one driver session with both roles' roots, and an
     `edit-latency --tests` line in the metric.

64. **N5. `hd doc NAME` checks the whole package first, even for std
   names. [V, M]**
   - Evidence: `document` calls `build_goal_with(…, Goal::Analyze, …)`
     before the lookup (`crates/hd_cli/src/doc_cmd.rs:115`). A `std.` name
     is answered from the embedded std text alone
     (`crates/hd_doc/src/lookup.rs:73-75`).
   - Measured: `hd doc std.collections.List` in the warm 10k package takes
     73.1 ms.
   - Fix: answer `std.` names before loading the package. For package
     names, read the stored check or the cached interfaces.
   - Reference: `go doc` reads docs without compiling the user's code.

65. **N6. The test filter is part of the build key. [V]** Test programs are
   built with `Goal::Tests { filter }`, and the collection key is
   `"test:{module}:{filter}"` (`crates/hd_driver/src/lib.rs:9554`). So
   `hd test`, then `hd test --filter x` with no edit, collects, emits and
   links again (S8), then recompiles on Node. Fix: build every case and
   select at run time, as the runner already does for `it_each` rows.
   Reference: `cargo test`, nextest and `go test -run` build once.

66. **N7. Every run lists every dependency's sources and reads `hd.toml`
   several times. [V by the reviewer]** `sources_of` walks a dependency's
   `src`, `tasks` and `tests` (`crates/hd_cli/src/disk.rs:556`, `:864`).
   `walk_below` canonicalizes every directory (`:282`). The manifest is
   parsed again for dependencies, lints, capabilities and executables.
   Fix: key a fetched dependency's listing by its `hd.sum` hash, skip its
   `tests/` and `tasks/`, detect link cycles by device and inode, and
   parse the manifest once per command. The effect is small for packages
   with few dependencies.

67. **N8. `hd test` preparation does quadratic set work. [V by the
   reviewer]** Diagnostics are deduplicated by comparing formatted text
   pairwise (`crates/hd_cli/src/test_cmd.rs:1268`). Each program keeps
   sources with `spec.keep.iter().any` (`:1132`). `add_synthetic` sorts the
   file list once per doc test (`crates/hd_cli/src/disk.rs:57`). Fix: hash
   sets keyed by code, file, line and message. This is the same pattern
   as the review's F12 and F13, at sites they do not list.

The reviewer also reported as sound: foldhash map hashing, slices that
allocate nothing, the JSON object index after 8 keys, content
deduplication of static strings, and identical Wasm functions (3 of
1,400 across 298 baselines, so folding them is not worth it).

## Coverage

How to check this list for gaps: every target of the latest CI run, then
every stage of the compiler and runtime, each mapped to the items that
move it, then what this audit did not examine.

### Every Goal Metric

All targets of `daily/2026-11-13T04.json`. The 27 `pathological` lines and
the 18 `resources` lines are grouped. A met line lists items only when it
is within 15% of its limit.

| Metric | Target | Value | Goal | Status | Items |
| --- | --- | --- | --- | --- | --- |
| edit-latency | p50 check after an edit | 52 ms | ≤ 50 ms | missed | W1, W2, W3, W4, I2, C1 |
| edit-latency | p95 check after an edit | 55 ms | ≤ 200 ms | met |  |
| cold-check | median cold check, 10k lines | 241 ms | ≤ 1000 ms | met |  |
| test-latency | median edit-then-test | 266 ms | ≤ 300 ms | met | E1, E2, S8 (266 of 300 ms) |
| mistakes | programs with exactly one diagnostic | 98.6% | ≥ 95.0% | met |  |
| mistakes | one diagnostic with the expected code | 98.6% | ≥ 95.0% | met |  |
| mistakes | fix-it makes hd check pass | 33.3% | ≥ 80.0% | missed | D4, D5, D6, D11 |
| mistakes | largest diagnostic text (bytes / 4) | 92 tokens | ≤ 60 tokens | missed | D1 (92 to 65, measured), D9, D8, D7 |
| answer-size | hd doc of a data type | 153 B | ≤ 1000 B | met |  |
| answer-size | one failing hd test, text | 318 B | ≤ 800 B | met |  |
| answer-size | one failing test, JSON object | 139 B | ≤ 400 B | met |  |
| answer-size | one diagnostic, JSON object | 170 B | ≤ 600 B | met |  |
| determinism | inputs with byte-identical output | 8 of 8 | all identical | met |  |
| pathological | 30 lines | | | 27 met, 3 missed; missed: iterator-chain | K2 for iterator-chain; R0, R0c for shapes outside the metric |
| recheck-precision | modules rechecked, private body edit | 1 | ≤ 1 | met |  |
| recheck-precision | modules rechecked, public signature edit | 9 | ≤ 3 | missed | I1 |
| errors-per-run | semantic: 10 mistakes reported once | 10 of 10 | all, once | met |  |
| errors-per-run | mixed: 10 mistakes reported once | 2 of 10 | all, once | missed | D10 |
| diag-location | diagnostics on the mistake's line | 98.6% | ≥ 95.0% | met |  |
| fixit-safety | fix-its that add no error | 100.0% | ≥ 100.0% | met |  |
| lookup-latency | p95 hd callers | - | ≤ 100 ms | n/a | K3 |
| lookup-latency | p95 hd needs | - | ≤ 100 ms | n/a | K3 |
| fmt | hd fmt, 10k lines | 32 ms | ≤ 200 ms | met |  |
| fmt | second hd fmt changes nothing | no change | idempotent | met |  |
| check-cost | checked run time / unchecked run time | 1.93x | ≤ 1.30x | missed | K1 |
| dev-speed | geomean dev / optimized | 1.16x | ≤ 4.00x | met |  |
| dev-speed | worst case dev / optimized | 1.55x | ≤ 10.00x | met |  |
| resources | 20 lines | | | 20 met | C1 to C6, M1, S3, S8 (all met; costs in §Cold Check) |
| long-session | RSS growth, REPL inputs 100 to 1000 | 96.5 MB | ≤ 22.0 MB | missed | S7 |
| startup | hd help (no version command): wall time | 4.0 ms | ≤ 20 ms | met |  |
| startup | hd help (no version command): CPU time | 0.0 ms | ≤ 20 ms | met |  |
| startup | hd help (no version command): peak RSS | 3.9 MB | ≤ 10.0 MB | met |  |
| startup | hd check empty.hd: wall time | 18 ms | ≤ 20 ms | met | S2 (18 of 20 ms) |
| startup | hd check empty.hd: CPU time | 10 ms | ≤ 20 ms | met |  |
| startup | hd check empty.hd: peak RSS | 17.0 MB | ≤ 10.0 MB | missed | S2, I2, H5 |
| concurrency | p95 latency at N = 4 vs N = 1 | 2.23x | ≤ 1.50x | missed | S1, S6, C1 |
| concurrency | p95 latency at N = 16 vs N = 1 | 8.38x | ≤ 6.00x | missed | S1, S6, C1 |
| concurrency | total CPU growth exponent, N = 1 to 16 | 1.11 | < 1.00 (sublinear) | missed | S1, S6, C1 |
| disk | artifacts plus toolchain per worktree | 20.1 MB | ≤ 10.0 MB | missed | S9 |
| parallel-speedup | speedup, 50k lines, 4 threads vs 1 | 1.92x | ≥ 2.40x | missed | S4, S3, S1 |
| cache-contention | dependency cache, 8 concurrent fetches: no corruption | intact | no corruption | met |  |
| cache-contention | dependency cache, 8 concurrent fetches: fetched once | 16 | ≤ 2, one check's connections | missed | S6 |
| cache-contention | compilation cache, 8 concurrent checks: no corruption | intact | no corruption | met |  |
| cache-contention | compilation cache, 8 concurrent checks: written once | 9 files | the files of one check's cache | missed | S6 |
| cache-growth | cache size after 100 edits, cap 5 MB | 4.5 MB | ≤ 5.0 MB | met |  |
| io-per-check | source files read, warm check after one edit | 79 | ≤ 1 | missed | I2, H1 |
| fetch-dedup | connections to the remote, 4 worktrees | 2 | = the first worktree's | met |  |
| proptest-perf | property cases per second | 23k/s | ≥ 100k/s | missed | E5, E1 |
| proptest-perf | time to shrink a known failure | 125 ms | ≤ 1000 ms | met |  |
| unit-test-perf | 1,000 unit tests, warm | 1130 ms | ≤ 1000 ms | missed | E3, E2, E1 |
| unit-test-perf | overhead per unit test | 0.9 ms | ≤ 1.0 ms | met | E3 (0.9 of 1.0 ms) |
| integration-test-perf | setup per integration test program | 161 ms | ≤ 20 ms | missed | E1, E2 |
| integration-test-perf | total time growth in programs | 0.964 | exponent < 1 (sublinear) | met |  |
| integration-test-perf | cost per doc test | 146 ms | ≤ 20 ms | missed | E1, E2 |
| runtime | geomean hd / Node | 1.05x | ≤ 1.50x | met |  |
| runtime | fib: hd / Node | 0.53x | ≤ 3.00x | met |  |
| runtime | sum: hd / Node | 1.45x | ≤ 3.00x | met | E6 (1.45x) |
| runtime | string-build: hd / Node | 1.03x | ≤ 3.00x | met |  |
| runtime | map: hd / Node | 2.70x | ≤ 3.00x | met | R5, H4, E1 (2.70 of 3.00x) |
| runtime | sort: hd / Node | 0.61x | ≤ 3.00x | met |  |
| allocations | counted loop, bytes per iteration | 0.00 B/iter | 0 allocations (< 1 B/iter) | met |  |
| allocations | list loop, bytes per iteration | 0.00 B/iter | 0 allocations (< 1 B/iter) | met |  |
| allocations | map loop, bytes per iteration | 0.00 B/iter | 0 allocations (< 1 B/iter) | met |  |
| allocations | iterator chain, bytes per iteration | 0.00 B/iter | ≤ 1 allocation (≤ 32 B/iter) | met |  |
| size-startup-heap | tiny program, release Wasm size | 1589 B | ≤ 2048 B | met |  |
| size-startup-heap | tiny program, start to first output | 56 ms | ≤ 5.0 ms | missed | E1 |
| size-startup-heap | peak heap, map: hd / Node | 0.99x | ≤ 2.00x | met |  |
| size-startup-heap | peak heap, string-build: hd / Node | 0.39x | ≤ 2.00x | met |  |
| host-call-overhead | Console println, per call | 8.7 µs | ≤ 1.0 µs | missed | E4 |
| host-call-overhead | Fs read_text!, per call | 27 µs | ≤ 10 µs | missed | E4 |
| host-call-overhead | serde boundary (list_dir!), per call | 47 µs | ≤ 5.0 µs | missed | E4 |
| suspension-overhead | await of a ready value, per await | 2.2 ns | ≤ 100 ns | met |  |
| suspension-overhead | all! of two ready tasks, throughput | 11.9M/s | ≥ 1.0M/s | met |  |
| suspension-overhead | race! of two ready tasks, throughput | 7.2M/s | ≥ 1.0M/s | met |  |
| serde-throughput | JSON encode, hd / Node MB/s | 0.19x | ≥ 0.50x | missed | R2 |
| serde-throughput | JSON decode, hd / Node MB/s | 0.20x | ≥ 0.50x | missed | R2 |
| text-throughput | string build, hd / Node MB/s | 3.11x | ≥ 0.50x | met |  |
| text-throughput | split, hd / Node MB/s | 2.33x | ≥ 0.50x | met |  |
| text-throughput | regex find_all, hd / Node MB/s | 0.09x | ≥ 0.50x | missed | R1 |
| dead-code | Wasm bytes per 1,000 lines | 6720 B | ≤ 10240 B | met |  |
| dead-code | no unused std in the program | none | none | met |  |
| long-run-memory | RSS growth after warm-up, 1 min | 0.3 MB | ≤ 10.0 MB | met |  |
| conformance | pass rate, known failures excluded | 100.0% | ≥ 100.0% | met |  |
| conformance | listed known failures | 0 | ≤ 0 | met |  |
| incremental-soundness | incremental equals clean, after each edit | 100.0% | ≥ 100.0% | met |  |

Every missed or n/a line has at least one item.

### Every Stage

| Stage | Items |
| --- | --- |
| reading sources, the settle window | C6, I2, S4, N7 |
| lexing and parsing | R0, D6 (parser fixes), D10 |
| skims and module discovery | S2, S4 |
| interfaces, decoding from the cache | W1, W2, W3, W4, S2 |
| invalidation and early cutoff | I1, W4 |
| type checking and the trait solver | C5, S3, R0b, R0c, K2, D1, D2, D3, D9 |
| derives and structure | C4, C3 |
| TIR: build, verify, serialize | C2, C3, M1 |
| the cache store | W3, M1, S6, S4 (publish) |
| scheduling and threads | S1, S3, S4 |
| monomorphization and the optimizer | O3, O4, O6, K1 |
| Wasm emission and linking | O2, O5, O6, S8, E6, R5 |
| the run-time allocator | O1 |
| the standard library | R1, R2, R3, R4, R5, N1, N3 |
| the engine and host | E1, E2, E3, E4, E5, E6, N1, N2 |
| the test runner | E2, E3, E5, S5, N6, N8 |
| the REPL | S7 |
| diagnostics and output | D1 to D11 |
| CLI commands other than `check` and `test` | N5, N4 (`check --tests`) |
| the binary and start-up | S9, S2 |
| measurement and observability | S11, H1 to H5 |

### Not Examined

- `hd fmt` internals (32 ms of a 200 ms budget), `hd doc`, the lints, and
  dependency resolution apart from fetch deduplication.
- The browser build, beyond review A10.
- Winch's generated-code speed. Its compile speed is measured (E1, and
  the review's A7); running hd modules needs hd's host, which has no
  Winch option.
- The HTTP and network run time, and `suite-cpu`, which the daily run
  does not report.
- Linux. Every local measurement is macOS on arm64 and the CI runner is
  Linux on x86-64. Allocator shares (C1, S1) can differ under glibc.
- A fully quiet machine. Other sessions kept the load average at 13 to 41
  for most of the audit. The run-time and engine tables above come from a
  window at 6 to 13. Linux was not tried: Docker is installed but its
  daemon was not running, and starting it was left to the owner.
- Prototypes: R5's small-map scan, C4's skipped derived bodies, E6's
  inlining rule and S1's read/write lock were built in a scratch
  worktree, measured, and checked against conformance fixtures (282, 290
  and 300 fixtures, no output changed). Not built: E6's strip-mining
  pass, skipping the hash in R5, C4's per-template bound cache, and S1's
  per-thread interner cache. Each is bounded by a measurement in its
  item. None of the prototypes is shippable as written, and each item
  says what the shippable form needs.

## Harness

68. **H1. Backdate generated files.** `io-per-check`, `edit-latency` and
    `cache-contention` generate files and check within two seconds (I2).
69. **H2. Measure JSON output size** alongside the text form (D8).
70. **H3. Run the pathological shapes through `build`** as well as
    `check` (R0d).
71. **H4. `runtime` compares a cold hd run with warm Node. [V, M]**
    - hd's number is one process run minus a baseline program
      (`bench/metrics/fixtures/runtime/baseline.hd`) that imports no
      runtime. So it still carries about 50M instructions of host and
      runtime set-up (E1).
    - Node's number is the median of three calls of `main` in one process
      (`bench/micro/*.js`), so its second call runs with a warm JIT.
    - Fix: give the baseline a runtime import, such as one `println`, and
      time hd's work inside the program (`std.time`), as the JS side
      does.
72. **H5. `startup` measures the settle-window path.** It writes
    `empty.hd` and checks it at once (S2). Report both the just-written
    and the settled figure, since agents hit both.

## Owner Decisions

1. E1: wasmtime as the default engine, and Winch as its dev tier.
2. E4: `println`'s two host writes, a spec rule.
3. K2: the `iterator-chain` fixture or the 64-deep proof limit.
4. K3: specify `hd callers` and `hd needs`, or retire `lookup-latency`.
5. S9: whether `disk` counts the shared toolchain.
6. D11: whether the `mistakes` metric may apply the first of several
   offered fixes, which decides whether 80% is reachable.
7. E6: whether release runs on Node use `--no-liftoff`, if E1 is not
   taken.
