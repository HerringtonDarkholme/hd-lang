//! The Wasm lowering of a layout (`crates/hd_wasm/DESIGN.md` §23.3): a
//! leaf's load and store, a discriminant test's instructions and the
//! steps that build an enum's values. The layouts themselves are
//! `hd_layout`'s, which this module re-exports, as `rustc_codegen_ssa`
//! maps `rustc_abi`'s `Primitive` and `Scalar` to backend values.

pub use hd_layout::*;
use hd_mono::ProgramEnv;
use hd_types::{InternPool, Ty};

use crate::module::{HEAP, mem as memarg};
use crate::op::Op as I;

/// [`hd_layout::cached`] with the emitter's reference slot plans in
/// place, for one link.
pub fn cached<R>(link: impl FnOnce() -> R) -> R {
    let out = hd_layout::cached(|| crate::emit::ref_slots::cached(link));
    ledger!(flush);
    out
}

/// How a plain function's parameter takes its argument
/// (`crates/hd_wasm/DESIGN.md` §23.25; git history at 7ccbb4ea8, `REFCOUNT_RUNTIME.md` §4.3), fixed
/// by the parameter's type alone so that every caller and the callee
/// agree without reading each other's bodies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pass {
    /// Nothing in the value is counted.
    Uncounted,
    /// A value parameter `T`: the callee's own copy, which arrives with a
    /// count of each counted reference the walk names and which the
    /// callee releases. The caller moves its value in at its last use,
    /// else counts a copy.
    Owned(Walk),
    /// A `&T` parameter: a borrowed read. The caller keeps the argument
    /// alive for the call, and neither side counts it.
    Borrowed,
}

/// How a plain function takes a parameter of type `t`; `counts` is whether
/// the program counts at all.
pub fn pass(pool: &InternPool, env: &dyn ProgramEnv, counts: bool, t: Ty) -> Pass {
    pass_mode(pool, env, counts, t, env.parameter_mode(t, false))
}

/// Physical lowering of TIR's ownership answer; no type or borrowing policy.
pub fn pass_mode(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    counts: bool,
    t: Ty,
    mode: hd_tir::own::ParameterMode,
) -> Pass {
    use hd_tir::own::ParameterMode;
    if !counts {
        return Pass::Uncounted;
    }
    match mode {
        ParameterMode::Lent => Pass::Borrowed,
        ParameterMode::Uncounted => Pass::Uncounted,
        ParameterMode::Owned => count_walk(pool, env, t).map_or(Pass::Uncounted, Pass::Owned),
    }
}

/// The leaf loops of a value at a heap address in a local, `base` 0
/// (`crates/hd_wasm/DESIGN.md` §23.3).
impl crate::module::Func {
    /// Pushes each of `leaves`, loaded from the address in local `at`.
    pub fn push_leaves(&mut self, at: u32, leaves: &[Leaf]) {
        for leaf in leaves {
            self.get(at);
            self.op(leaf.load(0));
        }
    }

    /// Loads each of `leaves` from the address in local `at` into `xs`.
    pub fn load_into(&mut self, at: u32, leaves: &[Leaf], xs: &[u32]) {
        for (leaf, x) in leaves.iter().zip(xs) {
            self.get(at);
            self.op(leaf.load(0));
            self.set(*x);
        }
    }

    /// [`Self::load_into`] fresh locals, which it returns.
    pub fn load_locals(&mut self, at: u32, leaves: &[Leaf]) -> Vec<u32> {
        let xs: Vec<u32> = leaves.iter().map(|l| self.local(l.val)).collect();
        self.load_into(at, leaves, &xs);
        xs
    }

    /// Stores locals `xs` as `leaves` at the address in local `at`.
    pub fn store_locals(&mut self, at: u32, leaves: &[Leaf], xs: impl IntoIterator<Item = u32>) {
        for (leaf, x) in leaves.iter().zip(xs) {
            self.get(at);
            self.get(x);
            self.op(leaf.store(0));
        }
    }
}

/// A leaf's Wasm memory access.
pub trait LeafOps {
    fn load(self, base: u32) -> I;
    fn load_in(self, memory: u32, base: u32) -> I;
    fn store(self, base: u32) -> I;
    fn store_in(self, memory: u32, base: u32) -> I;
}

/// A discriminant test's instructions.
pub trait IsOps {
    fn ops(self) -> Vec<I>;
    fn rel(start: u32, count: u32, bytes: u8) -> Vec<I>;
}

impl LeafOps for Leaf {
    /// The load of this leaf from the heap at an address on the stack,
    /// `base` bytes before the value.
    fn load(self, base: u32) -> I {
        self.load_in(HEAP, base)
    }

    /// [`LeafOps::load`] from memory `memory`.
    fn load_in(self, memory: u32, base: u32) -> I {
        let m = memarg(
            memory,
            u64::from(base + self.offset),
            u32::from(self.bytes).trailing_zeros(),
        );
        match (self.val, self.bytes, self.signed) {
            (Val::I32, 1, true) => I::I32Load8S(m),
            (Val::I32, 1, false) => I::I32Load8U(m),
            (Val::I32, 2, true) => I::I32Load16S(m),
            (Val::I32, 2, false) => I::I32Load16U(m),
            (Val::I32, _, _) => I::I32Load(m),
            (Val::I64, _, _) => I::I64Load(m),
            (Val::F32, _, _) => I::F32Load(m),
            (Val::F64, _, _) => I::F64Load(m),
        }
    }

    /// The store of this leaf to the heap at an address, then the value,
    /// on the stack, `base` bytes before the value.
    fn store(self, base: u32) -> I {
        self.store_in(HEAP, base)
    }

    /// [`LeafOps::store`] to memory `memory`.
    fn store_in(self, memory: u32, base: u32) -> I {
        let m = memarg(
            memory,
            u64::from(base + self.offset),
            u32::from(self.bytes).trailing_zeros(),
        );
        match (self.val, self.bytes) {
            (Val::I32, 1) => I::I32Store8(m),
            (Val::I32, 2) => I::I32Store16(m),
            (Val::I32, _) => I::I32Store(m),
            (Val::I64, _) => I::I64Store(m),
            (Val::F32, _) => I::F32Store(m),
            (Val::F64, _) => I::F64Store(m),
        }
    }
}

impl IsOps for Is {
    /// The instructions that turn the cell's value on the stack into an
    /// `i32` that is nonzero exactly when the test holds: one compare, or
    /// none for a null address's niche (the address itself), and a
    /// subtraction and a range compare for an untagged variant beside
    /// several niche values.
    fn ops(self) -> Vec<I> {
        match self {
            Is::Never => vec![I::Drop, I::I32Const(0)],
            Is::Eq(0) => vec![I::I32Eqz],
            Is::Eq(k) => vec![I::I32Const(k.cast_signed()), I::I32Eq],
            Is::Outside {
                start: 0, count: 1, ..
            } => vec![],
            Is::Outside {
                start, count: 1, ..
            } => {
                vec![I::I32Const(start.cast_signed()), I::I32Ne]
            }
            Is::Outside {
                start,
                count,
                bytes,
            } => {
                let mut ops = Is::rel(start, count, bytes);
                ops.extend([I::I32Const((count - 1).cast_signed()), I::I32GtU]);
                ops
            }
        }
    }

    /// The instructions that turn a niche cell on the stack into its
    /// index among the `count` values from `start`: any index past them
    /// for any other value. A narrow niche whose values wrap past its
    /// width is masked; one that does not wrap needs no mask, as a value
    /// below `start` leaves a negative, a large unsigned index.
    fn rel(start: u32, count: u32, bytes: u8) -> Vec<I> {
        let max = u32::MAX >> (32 - 8 * u32::from(bytes));
        let mut ops = Vec::new();
        if start != 0 {
            ops.extend([I::I32Const(start.cast_signed()), I::I32Sub]);
        }
        if bytes < 4 && u64::from(start) + u64::from(count) > u64::from(max) + 1 {
            ops.extend([I::I32Const(max.cast_signed()), I::I32And]);
        }
        ops
    }
}

/// Pushes the variant index of an enum whose cell's value `push` pushes,
/// given the value's index (`read_discr`,
/// `crates/hd_wasm/DESIGN.md` §23.14): the tag itself, a
/// constant, or for a niche one compare and a `select`. Several niche
/// values need the index twice, in a local of `f`'s.
pub fn read_discr(
    f: &mut crate::module::Func,
    disc: Disc,
    push: &dyn Fn(&mut crate::module::Func, u32),
) {
    let d = match disc {
        Disc::One(k) => return f.op(I::I32Const(k.cast_signed())),
        Disc::Cell(d) => d,
    };
    match d.enc {
        Enc::Direct => push(f, d.value),
        Enc::Niche {
            untagged,
            first,
            last,
            start,
            ..
        } if first == last => {
            f.op(I::I32Const(first.cast_signed()));
            f.op(I::I32Const(untagged.cast_signed()));
            push(f, d.value);
            for op in Is::Eq(start).ops() {
                f.op(op);
            }
            f.op(I::Select);
        }
        Enc::Niche {
            untagged,
            first,
            last,
            start,
            bytes,
        } => {
            let rel = f.local(Val::I32);
            push(f, d.value);
            for op in Is::rel(start, last - first + 1, bytes) {
                f.op(op);
            }
            f.op(I::LocalTee(rel));
            f.op(I::I32Const(first.cast_signed()));
            f.op(I::I32Add);
            f.op(I::I32Const(untagged.cast_signed()));
            f.op(I::LocalGet(rel));
            f.op(I::I32Const((last - first).cast_signed()));
            f.op(I::I32LeU);
            f.op(I::Select);
        }
    }
}

/// One step of building an enum's values, which the caller carries out:
/// push a payload leaf's value, or an instruction.
#[derive(Clone, Copy, Debug)]
pub enum Step {
    Leaf(usize, usize),
    Op(I),
}

/// Carries out the steps `steps` ([`Step`]) in `f`: `leaf` pushes field
/// `f`'s leaf `k`.
pub fn run_steps(
    f: &mut crate::module::Func,
    steps: &[Step],
    leaf: &dyn Fn(&mut crate::module::Func, usize, usize),
) {
    for s in steps {
        match *s {
            Step::Leaf(fi, k) => leaf(f, fi, k),
            Step::Op(op) => f.op(op),
        }
    }
}

/// A narrow leaf's mask, as a 64-bit value.
fn mask64(bytes: u8) -> i64 {
    match bytes {
        1 => 0xff,
        2 => 0xffff,
        4 => 0xffff_ffff,
        _ => -1,
    }
}

/// The steps that push the bytes the leaves `parts` of a variant put in
/// value `s`'s range, as `s` reads them: each part's bits, as a 64-bit
/// integer, shifted to its place in `s` and or-ed together, masked to
/// `s`'s width and read back as its type, sign-extended when `s` is a
/// narrow signed integer, as a load of `s` from memory reads it.
fn shadow_steps(s: Leaf, parts: &[(usize, usize, Leaf)], out: &mut Vec<Step>) {
    for (n, &(f, k, l)) in parts.iter().enumerate() {
        out.push(Step::Leaf(f, k));
        match l.val {
            Val::I32 => {
                if l.signed && l.bytes < 4 {
                    out.extend([
                        Step::Op(I::I32Const(if l.bytes == 1 { 0xff } else { 0xffff })),
                        Step::Op(I::I32And),
                    ]);
                }
                out.push(Step::Op(I::I64ExtendI32U));
            }
            Val::F32 => out.extend([Step::Op(I::I32ReinterpretF32), Step::Op(I::I64ExtendI32U)]),
            Val::I64 => {}
            Val::F64 => out.push(Step::Op(I::I64ReinterpretF64)),
        }
        if l.offset > s.offset {
            out.extend([
                Step::Op(I::I64Const(i64::from(8 * (l.offset - s.offset)))),
                Step::Op(I::I64Shl),
            ]);
        } else if l.offset < s.offset {
            out.extend([
                Step::Op(I::I64Const(i64::from(8 * (s.offset - l.offset)))),
                Step::Op(I::I64ShrU),
            ]);
        }
        if n > 0 {
            out.push(Step::Op(I::I64Or));
        }
    }
    if s.bytes < 8 {
        out.extend([Step::Op(I::I64Const(mask64(s.bytes))), Step::Op(I::I64And)]);
    }
    match s.val {
        Val::I32 => {
            out.push(Step::Op(I::I32WrapI64));
            match (s.signed, s.bytes) {
                (true, 1) => out.push(Step::Op(I::I32Extend8S)),
                (true, 2) => out.push(Step::Op(I::I32Extend16S)),
                (true | false, _) => {}
            }
        }
        Val::F32 => out.extend([Step::Op(I::I32WrapI64), Step::Op(I::F32ReinterpretI32)]),
        Val::I64 => {}
        Val::F64 => out.push(Step::Op(I::F64ReinterpretI64)),
    }
}

/// How to build variant `v`'s values: for each of the enum's values,
/// `None` where it is zero, else the steps that push it: the tag or niche
/// value it writes (`write_discr`), a leaf
/// of the variant, or the bytes the variant's leaves put in another
/// variant's value ([`Flat`]).
///
/// Cost: one pass over the variant's leaves per value they overlap.
#[must_use]
pub fn variant_steps(flat: &Flat, v: u32) -> Vec<Option<Vec<Step>>> {
    let at = v as usize;
    let values = &flat.mem.leaves;
    let mut out: Vec<Option<Vec<Step>>> = vec![None; values.len()];
    let mut own: Vec<(usize, usize, Leaf)> = Vec::new();
    for (f, (ks, leaves)) in flat.positions[at].iter().zip(&flat.leaves[at]).enumerate() {
        for (k, (&slot, &l)) in ks.iter().zip(leaves).enumerate() {
            out[slot as usize] = Some(vec![Step::Leaf(f, k)]);
            own.push((f, k, l));
        }
    }
    // `write_discr`: the tag, or the niche value of a variant that is not
    // the untagged one, in the cell.
    let written = flat
        .cell()
        .and_then(|d| Some((d.value as usize, d.enc.raw(v)?)));
    if let Some((k, raw)) = written
        && raw != 0
    {
        out[k] = Some(vec![Step::Op(I::I32Const(raw.cast_signed()))]);
    }
    for (slot, s) in values.iter().enumerate() {
        if out[slot].is_some() || written.is_some_and(|(k, _)| k == slot) {
            continue;
        }
        let end = s.offset + u32::from(s.bytes);
        let parts: Vec<(usize, usize, Leaf)> = own
            .iter()
            .copied()
            .filter(|(_, _, l)| l.offset < end && l.offset + u32::from(l.bytes) > s.offset)
            .collect();
        if !parts.is_empty() {
            let mut steps = Vec::new();
            shadow_steps(*s, &parts, &mut steps);
            out[slot] = Some(steps);
        }
    }
    out
}

/// The steps that push value `c` of a variant ([`variant_steps`]): its
/// steps, or a zero.
#[must_use]
pub fn value_or_zero(values: &[Leaf], c: usize, steps: Option<&Vec<Step>>) -> Vec<Step> {
    match steps {
        Some(s) => s.clone(),
        None => vec![Step::Op(zero_of(values[c].val))],
    }
}

/// A zero of `v`.
fn zero_of(v: Val) -> I {
    match v {
        Val::I32 => I::I32Const(0),
        Val::I64 => I::I64Const(0),
        Val::F32 => I::F32Const(wasm_encoder::Ieee32::new(0)),
        Val::F64 => I::F64Const(wasm_encoder::Ieee64::new(0)),
    }
}
