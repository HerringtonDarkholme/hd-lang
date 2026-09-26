# commit bd985d7 | command: node --experimental-strip-types audit/scripts/compiler/row-ripple.ts audit/probes/compiler/row-ripple | date 2026-09-25
## base: check exit 0, 0 error diagnostics
## edited: check exit 1, 1 error diagnostics
    audit/probes/compiler/row-ripple/row-ripple-edited.hd:74:13: missing-requirement: closure call requires Cache
## edited-fixed: check exit 0, 0 error diagnostics

functions (named, incl. synthetic): 34; closures: 15
named functions whose rows changed base -> edited-fixed: 5: f0, f2, f6, f14, f30
closures whose inferred rows changed: 4: $closure0(Logger -> Cache+Logger), $closure2(Logger -> Cache+Logger), $closure6(Logger -> Cache+Logger), $closure14(Logger -> Cache+Logger)
source signatures edited by hand to restore the build: 4 (ancestors of f30)

## diff of hd explain-requirements (base vs edited-fixed)
1c1,2
< f0: $ Logger
---
> f0: $ Cache + Logger
>   Cache: f0 -> $.use(Cache)
12c13,14
< f2: $ Logger
---
> f2: $ Cache + Logger
>   Cache: f2 -> $.use(Cache)
29c31,32
< f6: $ Logger
---
> f6: $ Cache + Logger
>   Cache: f6 -> $.use(Cache)
54c57,58
< f14: $ Logger
---
> f14: $ Cache + Logger
>   Cache: f14 -> $.use(Cache)
87c91,92
< f30: $ Logger
---
> f30: $ Cache + Logger
>   Cache: f30 -> $.use(Cache)
