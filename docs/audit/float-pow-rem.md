# Floating Power And Remainder In Std

Status: this is the implementation report for [#1209](https://github.com/HerringtonDarkholme/hd-lang/issues/1209); it proposes no new accepted behavior.

The change moves the existing `f64` power and truncated-remainder algorithms from generated Rust/Wasm helpers into private functions in `std.num`. The backend collects those functions and calls them for `f64`, and widens then narrows `f32` as before.

## Results

The base is `caece8ae2`. The power benchmark runs 400,000 dynamic `f64` powers per process. Fifteen fresh Node processes per side ran in alternating order after three discarded warmups per side. The machine load averages after the run were 6.37, 6.29, and 6.58.

| Release measurement | Base | This change | Change |
| --- | ---: | ---: | ---: |
| Power loop, median | 336.7 ms | 352.0 ms | +4.6% |
| Power loop, mean | 335.0 ms | 356.6 ms | +6.5% |
| Power loop, range | 327.8–341.0 ms | 346.5–392.8 ms | — |
| Tiny program, printed WAT | 56,619 bytes | 46,523 bytes | -17.8% |
| Tiny program, printed WAT | 2,292 lines | 1,740 lines | -24.1% |
| Tiny program, Wasm | 5,805 bytes | 5,229 bytes | -9.9% |

The tiny program calls both `9.0 ** 0.5` and `5.5 % 2.0` through non-constant wrapper functions. Printed WAT uses `hd_wat` with folded instructions.

## Exactness

The existing `f64` and `f32` Rust-generated remainder fixtures pass unchanged. The IEEE power/remainder edge fixture and the floating-power fixture also pass unchanged.

A separate base/head comparison generated 2,000 dynamic input rows, with positive and negative powers plus remainders in each row. All 6,000 result bit patterns were byte-identical. This check is supplemental and is not a committed fixture.
