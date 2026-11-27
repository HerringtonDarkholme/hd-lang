//! Calls into a C provider's slots with arguments known only at run time
//! (#2388 step 4), until per-signature trampolines replace this bridge
//! (step 6).
//!
//! A slot's C parameters are integer-class (a pointer, an integer,
//! `bool`, `size_t`) or float-class (`float`, `double`). The bridge calls
//! every slot through one fixed function type per calling convention, with
//! every argument register of each class and a fixed run of stack words,
//! after placing each argument where the slot's own signature puts it
//! ([`Plan`]): the C calling conventions this crate builds for assign
//! registers by class and position alone, and a callee ignores registers
//! and stack words past its own parameters.
//!
//! | Convention | Integer registers | Float registers | Stack |
//! | --- | --- | --- | --- |
//! | System V x86-64 | the first 6 integer-class arguments | the first 8 float-class ones | the rest, 8 bytes each, in argument order |
//! | AAPCS64 | the first 8 integer-class | the first 8 float-class | the rest, 8 bytes each |
//! | Apple arm64 | the first 8 integer-class | the first 8 float-class | the rest, each at its own size and alignment |
//! | Windows x64 | argument `i < 4`, by its class | argument `i < 4`, by its class | the rest, 8 bytes each |
//!
//! A slot whose arguments need more than [`STACK_WORDS`] stack words has no
//! plan. A narrow integer is passed extended to 64 bits, which Apple arm64
//! asks of a caller and the others allow; a `float` goes in the low 32
//! bits of its register or slot.
//!
//! Cost per call: one pass over the arguments into fixed arrays, then one
//! indirect call that moves every register and stack word of the fixed
//! type, about 30 words; a typed trampoline moves only the slot's own.

use std::ffi::c_void;

/// The stack words every bridged call passes: 128 bytes of arguments past
/// the registers.
pub(crate) const STACK_WORDS: usize = 16;

/// A parameter's class and size, as its C type has them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// An integer-class parameter of this many bytes.
    Int(u8),
    F32,
    F64,
}

/// An argument's value: an integer extended to 64 bits, or a float.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Arg {
    Int(u64),
    F32(f32),
    F64(f64),
}

/// Where one argument goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    /// Integer register `k` (by position on Windows x64).
    Int(usize),
    /// Float register `k` (by position on Windows x64).
    Float(usize),
    /// The stack, at this byte offset and of this size.
    Stack(usize, usize),
}

/// Where each argument of one slot goes, made once when the slot is bound.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Plan {
    places: Vec<Place>,
    /// Whether the slot returns a float.
    float_result: bool,
}

#[cfg(all(target_arch = "x86_64", not(windows)))]
const INT_REGS: usize = 6;
#[cfg(target_arch = "aarch64")]
const INT_REGS: usize = 8;
#[cfg(any(all(target_arch = "x86_64", not(windows)), target_arch = "aarch64"))]
const FLOAT_REGS: usize = 8;

impl Plan {
    /// The plan of a slot with parameters `kinds` (`self` first), returning
    /// a float when `float_result`; `None` when its arguments need more than
    /// [`STACK_WORDS`] stack words, or the target has no bridge.
    #[must_use]
    pub(crate) fn new(kinds: &[Kind], float_result: bool) -> Option<Plan> {
        let places = places(kinds)?;
        Some(Plan {
            places,
            float_result,
        })
    }
}

#[cfg(any(all(target_arch = "x86_64", not(windows)), target_arch = "aarch64"))]
fn places(kinds: &[Kind]) -> Option<Vec<Place>> {
    let (mut ints, mut floats, mut stack) = (0, 0, 0usize);
    let mut out = Vec::with_capacity(kinds.len());
    for &kind in kinds {
        let float = matches!(kind, Kind::F32 | Kind::F64);
        if float && floats < FLOAT_REGS {
            out.push(Place::Float(floats));
            floats += 1;
        } else if !float && ints < INT_REGS {
            out.push(Place::Int(ints));
            ints += 1;
        } else {
            let size = match kind {
                Kind::Int(bytes) => usize::from(bytes),
                Kind::F32 => 4,
                Kind::F64 => 8,
            };
            // Apple arm64 packs stack arguments at their own size and
            // alignment; the others give each an 8-byte slot.
            let (size, align) = if cfg!(target_vendor = "apple") && cfg!(target_arch = "aarch64") {
                (size, size)
            } else {
                (8, 8)
            };
            stack = stack.next_multiple_of(align);
            out.push(Place::Stack(stack, size));
            stack += size;
        }
    }
    (stack <= STACK_WORDS * 8).then_some(out)
}

#[cfg(all(target_arch = "x86_64", windows))]
fn places(kinds: &[Kind]) -> Option<Vec<Place>> {
    let out: Vec<Place> = kinds
        .iter()
        .enumerate()
        .map(|(i, kind)| match kind {
            _ if i >= 4 => Place::Stack((i - 4) * 8, 8),
            Kind::F32 | Kind::F64 => Place::Float(i),
            Kind::Int(_) => Place::Int(i),
        })
        .collect();
    (kinds.len() <= 4 + STACK_WORDS).then_some(out)
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn places(_: &[Kind]) -> Option<Vec<Place>> {
    None
}

/// Whether this target has a bridge at all.
#[must_use]
pub(crate) const fn supported() -> bool {
    cfg!(any(target_arch = "x86_64", target_arch = "aarch64"))
}

/// The bits of `arg` as a register or stack word holds them.
fn word(arg: Arg) -> u64 {
    match arg {
        Arg::Int(bits) => bits,
        Arg::F32(value) => u64::from(value.to_bits()),
        Arg::F64(value) => value.to_bits(),
    }
}

/// Calls the C function `f` with `args`, placed by `plan`, and returns
/// the raw bits of its result register: an integer's 64 bits, or a
/// float's as `f64` bits (a `float`'s in the low 32).
///
/// # Safety
///
/// `f` is a C function whose parameters are `plan`'s kinds, in order, and
/// whose result is integer-class, a float when the plan says so, or none.
#[allow(unsafe_code)]
pub(crate) unsafe fn call(f: *const c_void, plan: &Plan, args: &[Arg]) -> u64 {
    debug_assert_eq!(plan.places.len(), args.len(), "a C call's arity");
    // SAFETY: the caller's contract, passed on.
    unsafe { arch::call(f, plan, args) }
}

#[cfg(any(all(target_arch = "x86_64", not(windows)), target_arch = "aarch64"))]
#[allow(unsafe_code)]
mod arch {
    use super::{Arg, FLOAT_REGS, INT_REGS, Place, Plan, STACK_WORDS, word};
    use std::ffi::c_void;

    /// One call's registers and stack image.
    struct Frame {
        ints: [u64; INT_REGS],
        floats: [f64; FLOAT_REGS],
        stack: [u8; STACK_WORDS * 8],
    }

    pub(super) unsafe fn call(f: *const c_void, plan: &Plan, args: &[Arg]) -> u64 {
        let mut frame = Frame {
            ints: [0; INT_REGS],
            floats: [0.0; FLOAT_REGS],
            stack: [0; STACK_WORDS * 8],
        };
        for (place, &arg) in plan.places.iter().zip(args) {
            match *place {
                Place::Int(k) => frame.ints[k] = word(arg),
                Place::Float(k) => frame.floats[k] = f64::from_bits(word(arg)),
                Place::Stack(at, size) => {
                    frame.stack[at..at + size].copy_from_slice(&word(arg).to_le_bytes()[..size]);
                }
            }
        }
        let s: [u64; STACK_WORDS] = std::array::from_fn(|k| {
            let mut bytes = [0; 8];
            bytes.copy_from_slice(&frame.stack[k * 8..k * 8 + 8]);
            u64::from_le_bytes(bytes)
        });
        let (i, d) = (frame.ints, frame.floats);
        // SAFETY: the caller's contract: `f` takes the plan's parameters,
        // which sit where the convention reads them; the rest it ignores.
        unsafe {
            if plan.float_result {
                let f: Bridge<f64> = std::mem::transmute(f);
                bridge!(f, i, d, s).to_bits()
            } else {
                let f: Bridge<u64> = std::mem::transmute(f);
                bridge!(f, i, d, s)
            }
        }
    }

    #[cfg(target_arch = "x86_64")]
    type Bridge<R> = unsafe extern "C" fn(
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
    ) -> R;

    #[cfg(target_arch = "x86_64")]
    macro_rules! bridge {
        ($f:expr, $i:expr, $d:expr, $s:expr) => {
            $f(
                $i[0], $i[1], $i[2], $i[3], $i[4], $i[5], $d[0], $d[1], $d[2], $d[3], $d[4], $d[5],
                $d[6], $d[7], $s[0], $s[1], $s[2], $s[3], $s[4], $s[5], $s[6], $s[7], $s[8], $s[9],
                $s[10], $s[11], $s[12], $s[13], $s[14], $s[15],
            )
        };
    }

    #[cfg(target_arch = "aarch64")]
    type Bridge<R> = unsafe extern "C" fn(
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
    ) -> R;

    #[cfg(target_arch = "aarch64")]
    macro_rules! bridge {
        ($f:expr, $i:expr, $d:expr, $s:expr) => {
            $f(
                $i[0], $i[1], $i[2], $i[3], $i[4], $i[5], $i[6], $i[7], $d[0], $d[1], $d[2], $d[3],
                $d[4], $d[5], $d[6], $d[7], $s[0], $s[1], $s[2], $s[3], $s[4], $s[5], $s[6], $s[7],
                $s[8], $s[9], $s[10], $s[11], $s[12], $s[13], $s[14], $s[15],
            )
        };
    }
    use bridge;
}

#[cfg(all(target_arch = "x86_64", windows))]
#[allow(unsafe_code)]
mod arch {
    use super::{Arg, Place, Plan, STACK_WORDS, word};
    use std::ffi::c_void;

    pub(super) unsafe fn call(f: *const c_void, plan: &Plan, args: &[Arg]) -> u64 {
        // Argument `i < 4` takes integer register `i` or float register
        // `i` by its class; the rest take 8-byte stack slots in order.
        let mut regs = [0u64; 4];
        let mut float = [false; 4];
        let mut s = [0u64; STACK_WORDS];
        for (place, &arg) in plan.places.iter().zip(args) {
            match *place {
                Place::Int(k) => regs[k] = word(arg),
                Place::Float(k) => {
                    regs[k] = word(arg);
                    float[k] = true;
                }
                Place::Stack(at, _) => s[at / 8] = word(arg),
            }
        }
        let r = regs;
        let d = |k: usize| f64::from_bits(r[k]);
        // `self` is always argument 0, an integer; arguments 1 to 3 pick
        // one of eight fixed types.
        macro_rules! pick {
            ($ret:ty) => {
                match (float[1], float[2], float[3]) {
                    (false, false, false) => call4!($ret, u64, u64, u64, r[1], r[2], r[3]),
                    (true, false, false) => call4!($ret, f64, u64, u64, d(1), r[2], r[3]),
                    (false, true, false) => call4!($ret, u64, f64, u64, r[1], d(2), r[3]),
                    (true, true, false) => call4!($ret, f64, f64, u64, d(1), d(2), r[3]),
                    (false, false, true) => call4!($ret, u64, u64, f64, r[1], r[2], d(3)),
                    (true, false, true) => call4!($ret, f64, u64, f64, d(1), r[2], d(3)),
                    (false, true, true) => call4!($ret, u64, f64, f64, r[1], d(2), d(3)),
                    (true, true, true) => call4!($ret, f64, f64, f64, d(1), d(2), d(3)),
                }
            };
        }
        macro_rules! call4 {
            ($ret:ty, $a:ty, $b:ty, $c:ty, $x:expr, $y:expr, $z:expr) => {{
                let g: unsafe extern "C" fn(
                    u64,
                    $a,
                    $b,
                    $c,
                    u64,
                    u64,
                    u64,
                    u64,
                    u64,
                    u64,
                    u64,
                    u64,
                    u64,
                    u64,
                    u64,
                    u64,
                    u64,
                    u64,
                    u64,
                    u64,
                ) -> $ret = std::mem::transmute(f);
                g(
                    r[0], $x, $y, $z, s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7], s[8], s[9],
                    s[10], s[11], s[12], s[13], s[14], s[15],
                )
            }};
        }
        // SAFETY: the caller's contract: `f` takes the plan's parameters,
        // which sit where the convention reads them; the rest it ignores.
        unsafe {
            if plan.float_result {
                let value: f64 = pick!(f64);
                value.to_bits()
            } else {
                pick!(u64)
            }
        }
    }
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
mod arch {
    use super::{Arg, Plan};
    use std::ffi::c_void;

    #[allow(unsafe_code)]
    pub(super) unsafe fn call(_: *const c_void, _: &Plan, _: &[Arg]) -> u64 {
        unreachable!("ruled out: `Plan::new` makes no plan on a target without a bridge")
    }
}
