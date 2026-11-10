//! One branch per discriminant cell for nested enum matches (#1532,
//! `docs/future-work/ENUM-LAYOUT.md` "Discriminant read, write and
//! match"). An enum and the enums nested in its untagged payload share
//! the niche cell: `i32??`'s cell 0 holds the inner tag (0 or 1) or the
//! outer `.None` (2). A match's decision tests that cell once per level;
//! here the levels' `SwitchTag`s are grouped, each target mapped to the
//! raw cell values that select it (targets whose code is the same merged),
//! and the cell branched on once: a `br_table` on `cell - min` when it is
//! no larger than a compare chain, else that chain, the target most values
//! select last and taken with no compare. This is rustc's `SwitchInt` on
//! the niche (`rustc_codegen_ssa` `codegen_get_discr`), lowered as LLVM's
//! switch lowering lowers a dense range (a jump table) or a sparse one
//! (compares): `a < b` on `Ordering?` is one compare, as in rustc. Cost:
//! linear in the grouped switches' variants.

use crate::op::Op as I;
use hd_base::{HashMap, SliceSort as _, StageResult};
use hd_tir::ir::{NONE, PlaceRoot, Proj, ReadMode, Ref, Tag};

use super::{Ctl, Em};
use crate::layout::{Discr, Enc, Is, Kind};
use crate::module::Val;

/// The `SwitchTag`s that read one discriminant cell, from the outermost
/// enum's down through each untagged variant's payload enum.
pub(super) struct CellGroup {
    /// The outermost enum's cell, which every grouped switch reads.
    cell: Discr,
    /// The payload reads the nested switches decide on, in order: read
    /// before the branch, which needs no code for a value in locals or
    /// one read in place through a reference ([`Em::nested_lazy`]).
    hoisted: Vec<u32>,
    /// Each raw cell value that says a variant, with its target.
    values: Vec<(u32, usize)>,
    /// The target of every other valid value: the innermost untagged
    /// variant's.
    rest: Option<usize>,
    /// Each target: a case or `else` block of one of the switches.
    targets: Vec<u32>,
}

/// A switch chain on one value: each case's variant, block and switch,
/// and the last switch's `else` block with that switch; no `else` when it
/// is `unreachable` (the switches cover every tag).
pub(super) type Chain = (Vec<(u32, u32, u32)>, Option<(u32, u32)>);

/// What a target block's code is ([`Em::same`]): two targets with one
/// key emit the same code, so they are one.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Same {
    /// That block's own code.
    Block(u32),
    /// No statements and a constant as the value: its type and bits.
    Const(hd_types::Ty, u64),
}

/// The bytes of `v` as an unsigned LEB128.
fn uleb(v: u64) -> u64 {
    u64::from((64 - (v | 1).leading_zeros()).div_ceil(7))
}

/// The bytes of `v` as a signed LEB128.
fn sleb(v: i64) -> u64 {
    let bits = if v < 0 {
        64 - v.leading_ones()
    } else {
        64 - v.leading_zeros()
    };
    u64::from((bits + 1).div_ceil(7))
}

/// The bytes of reading the cell: a `local.get` of a local, which the
/// table reads once and the chain once per compare.
const CELL_READ: u64 = 2;

/// A nested level ([`Em::nested_switch`]): the reads before its switch,
/// the switch, the payload enum it decides on, and that enum's offset in
/// the outermost one.
type Nested = (Vec<u32>, u32, u32, u32);

impl Em<'_, '_, '_> {
    /// `SwitchTag` `i` on `a` and the ones its `else` chains to on the
    /// same value ([`Chain`]); `None` for a suspending body's switch or a
    /// variant tested twice.
    pub(super) fn switch_chain(&self, i: u32, a: u32) -> Option<Chain> {
        let mut cases: Vec<(u32, u32, u32)> = Vec::new();
        let mut cur = i;
        loop {
            if self.range(cur).is_some() {
                return None;
            }
            let r = self.rec(self.b.data[cur as usize][1]);
            if cases.iter().any(|(c, _, _)| *c == r[0]) {
                return None;
            }
            cases.push((r[0], r[1], cur));
            if r[2] == NONE {
                return Some((cases, None));
            }
            match self.sole_item(r[2]) {
                Some(k)
                    if self.b.tags[k as usize] == Tag::SwitchTag
                        && self.b.data[k as usize][0] == a =>
                {
                    cur = k;
                }
                // An `else` that is `unreachable`: no other tag gets here.
                Some(k) if self.b.tags[k as usize] == Tag::Unreachable => {
                    return Some((cases, None));
                }
                _ => return Some((cases, Some((r[2], cur)))),
            }
        }
    }

    /// The switches on enum value `a`'s cell from `SwitchTag` `i` down
    /// through each untagged variant's case block that only reads its
    /// payload enum and switches on it, where that enum's cell is `a`'s.
    /// `None` when there is no such nesting (one level, which
    /// [`Em::switch_to_arms`] branches on), or when `a`'s cell is not a
    /// 32-bit value in locals or read in place: a boxed enum's payload
    /// reads would load before the branch. A loop over levels, each one
    /// enum deeper, so no recursion; with no nesting it stops after the
    /// first level's chain, and each nested level's work is linear in its
    /// enum's variants.
    pub(super) fn cell_group(&self, i: u32, a: u32) -> StageResult<Option<CellGroup>> {
        let t = self.ty_of(a);
        let lazy = self.lazy.contains_key(&a);
        if !lazy && !matches!(self.kind(t), Kind::Value) {
            return Ok(None);
        }
        // A `SwitchTag`'s scrutinee, an enum or an optional.
        let Some(cell) = self.enum_flat(t)?.cell() else {
            return Ok(None);
        };
        if cell.leaf.val != Val::I32 {
            return Ok(None);
        }
        let mut g = CellGroup {
            cell,
            hoisted: Vec::new(),
            values: Vec::new(),
            rest: None,
            targets: Vec::new(),
        };
        // Blocks whose code is the same are one target: the `else`s of
        // a value switch's levels that each give `false`.
        let mut index: HashMap<Same, usize> = HashMap::default();
        let mut target = |g: &mut CellGroup, b: u32, key: Same| {
            *index.entry(key).or_insert_with(|| {
                g.targets.push(b);
                g.targets.len() - 1
            })
        };
        let (mut sw, mut val, mut off) = (i, a, 0u32);
        let mut levels = 0;
        loop {
            levels += 1;
            let Some((cases, other)) = self.switch_chain(sw, val) else {
                return Ok(None);
            };
            // A switch's scrutinee, an enum or an optional; each nested
            // one's cell is `a`'s, checked before descending.
            let flat = self.enum_flat(self.ty_of(val))?;
            let Some(d) = flat.cell() else {
                return Ok(None);
            };
            let mut by_variant: HashMap<u32, (u32, u32)> = HashMap::default();
            for &(c, b, s) in &cases {
                by_variant.insert(c, (b, s));
            }
            // Only an untagged variant's payload holds the cell; a tag's
            // payloads lie after it.
            let next = match d.enc {
                Enc::Niche { untagged, .. } => match by_variant.get(&untagged) {
                    Some(&(b, _)) => self.nested_switch(b, val, untagged, off, cell)?,
                    None => None,
                },
                Enc::Direct => None,
            };
            if levels == 1 && next.is_none() {
                return Ok(None);
            }
            for k in (0u32..).take(flat.positions.len()) {
                let Some((b, _)) = by_variant.get(&k).copied().or(other) else {
                    continue;
                };
                match d.enc.is(k) {
                    Is::Never => {}
                    Is::Eq(raw) => {
                        let j = target(&mut g, b, self.same(b));
                        g.values.push((raw, j));
                    }
                    Is::Outside { .. } if next.is_some() => {}
                    Is::Outside { .. } => g.rest = Some(target(&mut g, b, self.same(b))),
                }
            }
            match next {
                Some((reads, s, p, o)) => {
                    g.hoisted.extend(reads);
                    (sw, val, off) = (s, p, o);
                }
                None => break,
            }
        }
        Ok((levels >= 2).then_some(g))
    }

    /// What target block `b`'s code is ([`Same`]): a block of no
    /// statements whose value is a constant is that constant, as each
    /// level's `else` of a value switch (`a < b` on `Ordering?`) is.
    fn same(&self, b: u32) -> Same {
        if b == NONE
            || self.b.tags[b as usize] != Tag::Block
            || self.topo.label_of[b as usize] != NONE
            || self.range(b).is_some()
        {
            return Same::Block(b);
        }
        let [list, tail] = self.b.data[b as usize];
        if tail == NONE || Ref(tail).as_inst().is_some() || !self.items(list).is_empty() {
            return Same::Block(b);
        }
        let (ty, bits) = self.b.consts[(tail & !Ref::CONST_BIT) as usize];
        Same::Const(ty, bits)
    }

    /// Case block `b` of untagged variant `k` of `val`, which lies `off`
    /// bytes into the group's outermost enum, when it only reads parts of
    /// `val` and switches on a payload enum whose cell is `cell`: its
    /// reads, the switch, the enum and its offset. Each read is a borrow
    /// or a copy that counts nothing, so reading it on every path changes
    /// no count, and lies inline in `val`'s locals; through a reference,
    /// the one read is the payload enum, read in place.
    fn nested_switch(
        &self,
        b: u32,
        val: u32,
        k: u32,
        off: u32,
        cell: Discr,
    ) -> StageResult<Option<Nested>> {
        if b == NONE
            || self.b.tags[b as usize] != Tag::Block
            || self.topo.label_of[b as usize] != NONE
            || self.range(b).is_some()
        {
            return Ok(None);
        }
        let [list, tail] = self.b.data[b as usize];
        let items = self.items(list);
        let Some((&s, reads)) = items.split_last() else {
            return Ok(None);
        };
        if self.b.tags[s as usize] != Tag::SwitchTag || !(tail == NONE || tail == s) {
            return Ok(None);
        }
        let lazy = self.lazy.get(&val).copied();
        if lazy.is_some() && reads.len() != 1 {
            return Ok(None);
        }
        // Each value read so far, with its offset in the outermost enum.
        let mut known: Vec<(u32, u32)> = vec![(val, off)];
        for &r in reads {
            if self.b.tags[r as usize] != Tag::Read {
                return Ok(None);
            }
            let Some(p) = self.b.decode_place(self.b.data[r as usize][0]) else {
                return Ok(None);
            };
            let PlaceRoot::Value(base) = p.root else {
                return Ok(None);
            };
            let Some(&(_, at)) = known.iter().find(|(v, _)| *v == base.0) else {
                return Ok(None);
            };
            let bt = self.ty_of(base.0);
            let o = match p.proj[..] {
                // Only the untagged variant's payload holds the cell.
                [Proj::Variant(v), Proj::Field(f)] if base.0 != val || v == k => {
                    // A checked payload step of an enum or an optional.
                    at + self.enum_flat(bt)?.offsets[v as usize][f as usize]
                }
                [Proj::Field(f)] if lazy.is_none() => at + self.path_step(bt, f)?.1.offset,
                _ => return Ok(None),
            };
            let rt = self.ty(r);
            let mode = ReadMode::from_u32(self.b.data[r as usize][1]);
            let uncounted = !self.tracked(rt) && self.drop_calls(rt).is_empty();
            if !matches!(self.kind(rt), Kind::Value)
                || mode == Some(ReadMode::Move)
                || !(mode == Some(ReadMode::Borrow) || uncounted)
            {
                return Ok(None);
            }
            if let Some(z) = lazy
                && (self.flat(rt)?.is_none() || !self.lazy_borrow(r, rt, z.ref_ty))
            {
                return Ok(None);
            }
            known.push((r, o));
        }
        let p = self.b.data[s as usize][0];
        let Some(&(_, o)) = known.iter().skip(1).find(|(v, _)| *v == p) else {
            return Ok(None);
        };
        // A switch's scrutinee, an enum or an optional.
        let shares = self.enum_flat(self.ty_of(p))?.cell().is_some_and(|c| {
            o + c.leaf.offset == cell.leaf.offset
                && c.leaf.bytes == cell.leaf.bytes
                && c.leaf.val == cell.leaf.val
        });
        Ok(shares.then(|| (reads.to_vec(), s, p, o)))
    }

    /// [`Em::cell_switch`] of two targets, one of which has code: `if` on
    /// the other target's values, as the per-level switches emit, so no
    /// block per target is needed; or of one target, its block. False,
    /// emitting nothing, for more targets or two that only go to arms.
    /// Targets that go to one arm are one (`direct`).
    fn cell_if(
        &mut self,
        i: u32,
        a: u32,
        g: &CellGroup,
        direct: &[Option<u32>],
    ) -> StageResult<bool> {
        // Each target's class: its arm, or itself.
        let class = |j: usize| direct[j].map_or((1, j), |arm| (0, arm as usize));
        let mut count: HashMap<(u8, usize), (usize, usize)> = HashMap::default();
        for &(_, j) in &g.values {
            count.entry(class(j)).or_insert((0, j)).0 += 1;
        }
        if let Some(j) = g.rest {
            count.entry(class(j)).or_insert((0, j)).0 = usize::MAX;
        }
        if count.len() > 2 || count.len() == 2 && direct.iter().all(Option::is_some) {
            return Ok(false);
        }
        let mut classes: Vec<((u8, usize), (usize, usize))> = count.into_iter().collect();
        // The class most values select is the `else`; ties go to the later.
        classes.sort_stable_by_key(|&(c, (n, j))| (n, j, c));
        let Some(&(_, (_, other))) = classes.last() else {
            return Ok(false);
        };
        let Some(&(first, (_, then))) = classes.first().filter(|_| classes.len() == 2) else {
            self.block_into(g.targets[other], Some(i))?;
            return Ok(true);
        };
        let mut tests = 0;
        for &(v, j) in &g.values {
            if class(j) == first {
                self.discr_cell(a, g.cell)?;
                if v == 0 {
                    self.f.op(I::I32Eqz);
                } else {
                    self.f.u32(v);
                    self.f.op(I::I32Eq);
                }
                if tests > 0 {
                    self.f.op(I::I32Or);
                }
                tests += 1;
            }
        }
        self.f.op(I::If(wasm_encoder::BlockType::Empty));
        self.ctl.push(Ctl::Plain);
        self.block_into(g.targets[then], Some(i))?;
        self.f.op(I::Else);
        self.block_into(g.targets[other], Some(i))?;
        self.close();
        Ok(true)
    }

    /// Branches once on the cell of the outermost enum `a` of group `g`
    /// ([`Em::cell_group`]) from `SwitchTag` `i`: the hoisted reads, then
    /// one `br_table` on `cell - min` when its bytes are no more than a
    /// compare chain's, else that chain, whose last target, taken with no
    /// compare, is the one most values select. A target that only goes to
    /// an arm of the match being decided is that arm; any other is a
    /// block after the branch, as [`Em::switch_to_arms`] lays them out.
    pub(super) fn cell_switch(&mut self, i: u32, a: u32, g: &CellGroup) -> StageResult<()> {
        for &r in &g.hoisted {
            self.inst(r)?;
        }
        let m = self.deciding.last().copied();
        let direct: Vec<Option<u32>> = g
            .targets
            .iter()
            .map(|b| m.and_then(|_| self.sole_to_arm(*b)))
            .collect();
        if self.cell_if(i, a, g, &direct)? {
            return Ok(());
        }
        let coded: Vec<usize> = (0..g.targets.len())
            .filter(|&j| direct[j].is_none())
            .collect();
        let n = coded.iter().fold(0u32, |n, _| n + 1);
        for _ in 0..n + u32::from(n > 0) {
            self.open(Ctl::Plain);
        }
        // The `k`th target with code branches to the `k`th block out.
        let mut label = Vec::with_capacity(g.targets.len());
        let mut k = 0u32;
        for arm in &direct {
            if let (Some(arm), Some(m)) = (arm, m) {
                label.push(self.depth(Ctl::Arm(m, *arm)));
            } else {
                label.push(k);
                k += 1;
            }
        }
        // Targets with one label are one: two blocks that only go to the
        // same arm. Each label's count of values; the rest's is all the
        // others, so it is the most.
        let mut count: HashMap<u32, usize> = HashMap::default();
        for &(_, j) in &g.values {
            *count.entry(label[j]).or_default() += 1;
        }
        let default = if let Some(j) = g.rest {
            label[j]
        } else {
            let mut best: Option<(usize, u32)> = None;
            for &(_, j) in &g.values {
                let c = count[&label[j]];
                if best.is_none_or(|(bc, _)| c >= bc) {
                    best = Some((c, label[j]));
                }
            }
            best.map_or(0, |(_, l)| l)
        };
        // Each other label with its values, in decision order.
        let mut by_label: Vec<(u32, Vec<u32>)> = Vec::new();
        let mut slot: HashMap<u32, usize> = HashMap::default();
        for &(v, j) in &g.values {
            if label[j] == default {
                continue;
            }
            let k = *slot.entry(label[j]).or_insert_with(|| {
                by_label.push((label[j], Vec::new()));
                by_label.len() - 1
            });
            by_label[k].1.push(v);
        }
        let lo = g.values.iter().map(|(v, _)| *v).min().unwrap_or(0);
        let hi = g.values.iter().map(|(v, _)| *v).max().unwrap_or(0);
        let span = u64::from(hi - lo) + 1;
        // The table when it is no larger than the chain, in bytes: one
        // read, the index, and an entry per value in the span; against a
        // read, a compare and a `br_if` per value of the other targets,
        // and a `br`.
        let mut entries = (span - g.values.len() as u64) * uleb(u64::from(default));
        let mut chain_bytes = 1 + uleb(u64::from(default));
        for &(v, j) in &g.values {
            entries += uleb(u64::from(label[j]));
            if label[j] != default {
                let test = if v == 0 {
                    1
                } else {
                    2 + sleb(i64::from(v.cast_signed()))
                };
                chain_bytes += CELL_READ + test + 1 + uleb(u64::from(label[j]));
            }
        }
        let index = if lo == 0 {
            0
        } else {
            2 + sleb(i64::from(lo.cast_signed()))
        };
        let table_bytes = CELL_READ + index + 1 + uleb(span) + entries + uleb(u64::from(default));
        if table_bytes <= chain_bytes {
            self.discr_cell(a, g.cell)?;
            if lo != 0 {
                self.f.u32(lo);
                self.f.op(I::I32Sub);
            }
            let mut table = vec![default; (hi - lo) as usize + 1];
            for &(v, j) in &g.values {
                table[(v - lo) as usize] = label[j];
            }
            self.f.br_table(table, default);
        } else {
            for (l, vs) in &by_label {
                for &v in vs {
                    self.discr_cell(a, g.cell)?;
                    if v == 0 {
                        self.f.op(I::I32Eqz);
                    } else {
                        self.f.u32(v);
                        self.f.op(I::I32Eq);
                    }
                    self.f.op(I::BrIf(*l));
                }
            }
            self.f.op(I::Br(default));
        }
        for (pos, &j) in (0u32..).zip(&coded) {
            // The block of the `pos`th target with code ends here: its
            // code follows, then the switch's end, `n - 1 - pos` out.
            self.close();
            // Every nested switch's value is its case block's tail, so
            // each target's value is the outermost switch's.
            let b = g.targets[j];
            self.block_into(b, Some(i))?;
            if !self.diverges(b) {
                self.f.op(I::Br(n - 1 - pos));
            }
        }
        if n > 0 {
            self.close();
        }
        Ok(())
    }
}
