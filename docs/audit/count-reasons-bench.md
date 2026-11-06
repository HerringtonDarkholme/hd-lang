# Benchmark Count Reasons

Status: audit evidence for [#1362](https://github.com/HerringtonDarkholme/hd-lang/issues/1362);
this report accepts no new behavior. It checks the count reasons in
[Ownership Architecture](../future-work/OWNERSHIP_ARCHITECTURE.md),
“The One Place Each Count Is Decided”.

At main `a5e29b1ac`, all 32 benchmark programs build in release. Their emitted
bodies contain 1,299 static count-operation sites: 460 application, 452 std,
and 387 generated runtime sites. Of these, 747 have a conservative reason.
These totals include repeated helper instances across programs; they are
static sites, not execution counts.

All 955 operation lines in the 248 counts baselines name their reason.
The labels were introduced by
[`b8144ac98`](https://github.com/HerringtonDarkholme/hd-lang/commit/b8144ac98)
and refined by later ownership work. The corpus tool now uses the same
reason formatter as the counts view, includes std and runtime bodies, and
writes every operation to `operations.tsv`.

## Conservative Sites

[The complete inventory](count-reasons-bench.tsv) records all 747 conservative
sites, including the program, function, type arguments, Wasm byte offset,
originating TIR instruction, action, subject, reason and disposition.
Linked tickets mark counts whose necessity is still unproven.

| Sites | Explanation or follow-up |
| ---: | --- |
| 480 | Runtime primitive implementations: recursive counting/freeing, COW buffer transfer, allocator reclamation and waker lifetime. The invoking source operation has its own ownership decision. |
| 117 | Suspension/frame lifetime: [reference roots (#1355)](https://github.com/HerringtonDarkholme/hd-lang/issues/1355), [cold frames (#1391)](https://github.com/HerringtonDarkholme/hd-lang/issues/1391), [by-value transfer (#1392)](https://github.com/HerringtonDarkholme/hd-lang/issues/1392). |
| 57 | COW guards before mutation: [freshness/hoisting (#1353)](https://github.com/HerringtonDarkholme/hd-lang/issues/1353), [fresh lists (#1371)](https://github.com/HerringtonDarkholme/hd-lang/issues/1371), [repeated checks (#1396)](https://github.com/HerringtonDarkholme/hd-lang/issues/1396). |
| 32 | [String results at calls and intrinsics (#1412)](https://github.com/HerringtonDarkholme/hd-lang/issues/1412). |
| 23 | [Generated metadata callback entry/return counts (#1414)](https://github.com/HerringtonDarkholme/hd-lang/issues/1414). |
| 17 | [Unexplained copy reads (#1413)](https://github.com/HerringtonDarkholme/hd-lang/issues/1413). |
| 10 | Stores replace a previous owner. [Known-empty places (#1386)](https://github.com/HerringtonDarkholme/hd-lang/issues/1386) need no release. |
| 7 | [Dynamic trait roots (#1354)](https://github.com/HerringtonDarkholme/hd-lang/issues/1354) and [provider contexts (#1394)](https://github.com/HerringtonDarkholme/hd-lang/issues/1394). |
| 2 | An unowned argument can be a representation view requiring cleanup: [loan/count separation (#1360)](https://github.com/HerringtonDarkholme/hd-lang/issues/1360). |
| 1 | [Closure environment ownership (#1383)](https://github.com/HerringtonDarkholme/hd-lang/issues/1383). |
| 1 | [Function-value argument ownership (#1376)](https://github.com/HerringtonDarkholme/hd-lang/issues/1376). |

The 480 runtime implementation sites include explicit calls inside count
primitives, collection copy/growth glue, frame/waker cleanup and std's
allocator/Buffer code. They implement an entered primitive; eliminating
the source operation can also make its helper unreachable. This inventory
does not claim those source operations are all required.

## Reproduce

From the repository root:

```sh
HD_CACHE="$PWD/target/count-reasons/cache" \
CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2 HD_TEST_JOBS=2 \
HD_CORPUS_PARTS=bench HD_CORPUS_OUT=target/count-reasons \
cargo test -q -j 2 -p hd_driver --test count_reasons count_reasons \
    -- --exact --ignored --nocapture
```

The run must report `bench / built / 32`, with no skipped programs.
`operations.tsv` contains application, std and generated runtime bodies.
Filtering its reason column to `conservative:` produces the sites reviewed
in the inventory; its final disposition column is the review recorded here.
`origin` is a TIR instruction number, not a source line.

`HD_CORPUS_PARTS` rejects unknown part names. The command above selects no
conformance, stage, example or documentation modules.
