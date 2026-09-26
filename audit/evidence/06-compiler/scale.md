# commit bd985d7 | command: node --experimental-strip-types audit/scripts/compiler/scale.ts <scratch>/scale 250,500,1000,2000 | date 2026-09-25 | CPU contended by other workers
| units | source lines | run | parse ms | check ms | emit ms | assemble ms | total ms |
|---:|---:|---|---:|---:|---:|---:|---:|
| 250 | 4253 | cold | 33 | 37 | 19 | 505 | 594 |
| 250 | 4253 | one-line edit | 13 | 31 | 18 | 493 | 554 |
| 500 | 8503 | cold | 35 | 81 | 38 | 982 | 1135 |
| 500 | 8503 | one-line edit | 16 | 56 | 36 | 964 | 1072 |
| 1000 | 17003 | cold | 30 | 101 | 60 | 1925 | 2115 |
| 1000 | 17003 | one-line edit | 32 | 99 | 60 | 2450 | 2641 |
| 2000 | 34003 | cold | 184 | 353 | 143 | 5915 | 6595 |
| 2000 | 34003 | one-line edit | 153 | 371 | 213 | 5600 | 6337 |
