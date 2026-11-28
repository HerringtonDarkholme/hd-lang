//! The C call trampolines (#2388 step 6, `native::c_trampoline`): one
//! native function per distinct C signature, compiled once per process,
//! that places any number of arguments as the target's C calling
//! convention does. The slots here are this file's own C-ABI functions.
mod tests {
    use std::ffi::c_void;
    use std::sync::Arc;

    use hd_run_wasmtime::internals::native::c_trampoline::{CType, Sig, Trampoline, compiles};

    /// 48 arguments of mixed types, more on the stack than step 4's bridge
    /// allowed on any target: each weighted by its position, so a
    /// misplaced one changes the sum.
    #[allow(
        clippy::too_many_arguments,
        clippy::cast_precision_loss,
        clippy::cast_lossless,
        clippy::unnecessary_cast,
        reason = "one uniform weighted sum over every argument type"
    )]
    extern "C" fn many(
        a0: i64,
        a1: f64,
        a2: i8,
        a3: u16,
        a4: f32,
        a5: u64,
        a6: i64,
        a7: f64,
        a8: i8,
        a9: u16,
        a10: f32,
        a11: u64,
        a12: i64,
        a13: f64,
        a14: i8,
        a15: u16,
        a16: f32,
        a17: u64,
        a18: i64,
        a19: f64,
        a20: i8,
        a21: u16,
        a22: f32,
        a23: u64,
        a24: i64,
        a25: f64,
        a26: i8,
        a27: u16,
        a28: f32,
        a29: u64,
        a30: i64,
        a31: f64,
        a32: i8,
        a33: u16,
        a34: f32,
        a35: u64,
        a36: i64,
        a37: f64,
        a38: i8,
        a39: u16,
        a40: f32,
        a41: u64,
        a42: i64,
        a43: f64,
        a44: i8,
        a45: u16,
        a46: f32,
        a47: u64,
    ) -> f64 {
        1.0 * a0 as f64
            + 2.0 * a1 as f64
            + 3.0 * a2 as f64
            + 4.0 * a3 as f64
            + 5.0 * a4 as f64
            + 6.0 * a5 as f64
            + 7.0 * a6 as f64
            + 8.0 * a7 as f64
            + 9.0 * a8 as f64
            + 10.0 * a9 as f64
            + 11.0 * a10 as f64
            + 12.0 * a11 as f64
            + 13.0 * a12 as f64
            + 14.0 * a13 as f64
            + 15.0 * a14 as f64
            + 16.0 * a15 as f64
            + 17.0 * a16 as f64
            + 18.0 * a17 as f64
            + 19.0 * a18 as f64
            + 20.0 * a19 as f64
            + 21.0 * a20 as f64
            + 22.0 * a21 as f64
            + 23.0 * a22 as f64
            + 24.0 * a23 as f64
            + 25.0 * a24 as f64
            + 26.0 * a25 as f64
            + 27.0 * a26 as f64
            + 28.0 * a27 as f64
            + 29.0 * a28 as f64
            + 30.0 * a29 as f64
            + 31.0 * a30 as f64
            + 32.0 * a31 as f64
            + 33.0 * a32 as f64
            + 34.0 * a33 as f64
            + 35.0 * a34 as f64
            + 36.0 * a35 as f64
            + 37.0 * a36 as f64
            + 38.0 * a37 as f64
            + 39.0 * a38 as f64
            + 40.0 * a39 as f64
            + 41.0 * a40 as f64
            + 42.0 * a41 as f64
            + 43.0 * a42 as f64
            + 44.0 * a43 as f64
            + 45.0 * a44 as f64
            + 46.0 * a45 as f64
            + 47.0 * a46 as f64
            + 48.0 * a47 as f64
    }

    /// Calls `f` through the trampoline of `sig` with `args`.
    #[allow(unsafe_code)]
    fn call(sig: &Sig, f: *const c_void, args: &[u64]) -> u64 {
        let t = Trampoline::of(sig).expect("a Cranelift host");
        // SAFETY: each caller passes a function of `sig` and its values.
        unsafe { t.call(f, args) }
    }

    #[test]
    fn a_call_with_forty_eight_mixed_arguments_places_each_one() {
        let sig = Sig {
            params: vec![
                CType::I64,
                CType::F64,
                CType::I8,
                CType::U16,
                CType::F32,
                CType::U64,
                CType::I64,
                CType::F64,
                CType::I8,
                CType::U16,
                CType::F32,
                CType::U64,
                CType::I64,
                CType::F64,
                CType::I8,
                CType::U16,
                CType::F32,
                CType::U64,
                CType::I64,
                CType::F64,
                CType::I8,
                CType::U16,
                CType::F32,
                CType::U64,
                CType::I64,
                CType::F64,
                CType::I8,
                CType::U16,
                CType::F32,
                CType::U64,
                CType::I64,
                CType::F64,
                CType::I8,
                CType::U16,
                CType::F32,
                CType::U64,
                CType::I64,
                CType::F64,
                CType::I8,
                CType::U16,
                CType::F32,
                CType::U64,
                CType::I64,
                CType::F64,
                CType::I8,
                CType::U16,
                CType::F32,
                CType::U64,
            ],
            result: Some(CType::F64),
        };
        let args = [
            (-1i64).cast_unsigned(),
            (2.0f64).to_bits(),
            (-3i64).cast_unsigned(),
            4,
            u64::from((5.0f32).to_bits()),
            6,
            (-7i64).cast_unsigned(),
            (8.0f64).to_bits(),
            (-9i64).cast_unsigned(),
            10,
            u64::from((11.0f32).to_bits()),
            12,
            (-13i64).cast_unsigned(),
            (14.0f64).to_bits(),
            (-15i64).cast_unsigned(),
            16,
            u64::from((17.0f32).to_bits()),
            18,
            (-19i64).cast_unsigned(),
            (20.0f64).to_bits(),
            (-21i64).cast_unsigned(),
            22,
            u64::from((23.0f32).to_bits()),
            24,
            (-25i64).cast_unsigned(),
            (26.0f64).to_bits(),
            (-27i64).cast_unsigned(),
            28,
            u64::from((29.0f32).to_bits()),
            30,
            (-31i64).cast_unsigned(),
            (32.0f64).to_bits(),
            (-33i64).cast_unsigned(),
            34,
            u64::from((35.0f32).to_bits()),
            36,
            (-37i64).cast_unsigned(),
            (38.0f64).to_bits(),
            (-39i64).cast_unsigned(),
            40,
            u64::from((41.0f32).to_bits()),
            42,
            (-43i64).cast_unsigned(),
            (44.0f64).to_bits(),
            (-45i64).cast_unsigned(),
            46,
            u64::from((47.0f32).to_bits()),
            48,
        ];
        let raw = call(&sig, many as *const c_void, &args);
        // Every term is a small integer, so the sum is exact.
        assert_eq!(raw, 15016.0f64.to_bits());
    }

    extern "C" fn i8_result() -> i8 {
        -5
    }
    extern "C" fn u8_result() -> u8 {
        250
    }
    extern "C" fn i16_result() -> i16 {
        -300
    }
    extern "C" fn u16_result() -> u16 {
        65_000
    }
    extern "C" fn i32_result() -> i32 {
        -7
    }
    extern "C" fn u32_result() -> u32 {
        4_000_000_000
    }
    extern "C" fn bool_result() -> bool {
        true
    }
    extern "C" fn f32_result() -> f32 {
        1.5
    }
    extern "C" fn f64_result() -> f64 {
        -2.25
    }
    extern "C" fn ptr_result() -> usize {
        usize::MAX - 3
    }
    /// Narrow arguments arrive extended as C extends them.
    extern "C" fn narrow_args(a: i8, b: u8, c: i16, d: u16, e: bool) -> i64 {
        i64::from(a) * 1_000_000
            + i64::from(b) * 1_000
            + i64::from(c)
            + i64::from(d) * 10
            + i64::from(e)
    }

    /// Each result comes back extended by its signedness, a float as its
    /// bits, and narrow arguments go in extended.
    #[test]
    fn narrow_and_float_results_come_back_as_their_bits() {
        let none = |result| Sig {
            params: vec![],
            result: Some(result),
        };
        let f = |p: *const c_void| p;
        assert_eq!(
            call(&none(CType::I8), f(i8_result as *const c_void), &[]).cast_signed(),
            -5
        );
        assert_eq!(
            call(&none(CType::U8), f(u8_result as *const c_void), &[]),
            250
        );
        assert_eq!(
            call(&none(CType::I16), f(i16_result as *const c_void), &[]).cast_signed(),
            -300
        );
        assert_eq!(
            call(&none(CType::U16), f(u16_result as *const c_void), &[]),
            65_000
        );
        assert_eq!(
            call(&none(CType::I32), f(i32_result as *const c_void), &[]).cast_signed(),
            -7
        );
        assert_eq!(
            call(&none(CType::U32), f(u32_result as *const c_void), &[]),
            4_000_000_000
        );
        assert_eq!(
            call(&none(CType::Bool), f(bool_result as *const c_void), &[]),
            1
        );
        assert_eq!(
            call(&none(CType::F32), f(f32_result as *const c_void), &[]),
            u64::from(1.5f32.to_bits())
        );
        assert_eq!(
            call(&none(CType::F64), f(f64_result as *const c_void), &[]),
            (-2.25f64).to_bits()
        );
        assert_eq!(
            call(&none(CType::Ptr), f(ptr_result as *const c_void), &[]),
            (usize::MAX - 3) as u64
        );
        let sig = Sig {
            params: vec![CType::I8, CType::U8, CType::I16, CType::U16, CType::Bool],
            result: Some(CType::I64),
        };
        let args = [
            (-3i64).cast_unsigned(),
            200,
            (-4i64).cast_unsigned(),
            60_000,
            1,
        ];
        assert_eq!(
            call(&sig, narrow_args as *const c_void, &args).cast_signed(),
            -3_000_000 + 200_000 - 4 + 600_000 + 1
        );
    }

    /// A signature compiles once in a process, however many slots and runs
    /// share it.
    #[test]
    fn a_signature_compiles_once_however_many_slots_share_it() {
        // A signature no other test uses.
        let sig = Sig {
            params: vec![CType::U16, CType::F32, CType::U16, CType::F32, CType::U16],
            result: Some(CType::U16),
        };
        let first = Trampoline::of(&sig).expect("a Cranelift host");
        for _ in 0..100 {
            let again = Trampoline::of(&sig).expect("a Cranelift host");
            assert!(Arc::ptr_eq(&first, &again));
        }
        assert_eq!(compiles(&sig), 1);
    }
}
