//! The one body copy-with-remap (#2132). The inliner's copy of a callee,
//! step 4's copy of a generic local function per instantiation
//! (`local_fns`), a closure literal's sparse sub-body copy and the splice's
//! renumbering all map a body's words here, each through the tables its
//! call site passes ([`Remap`]).
//!
//! What a word names decides how it maps: an instruction or constant word
//! by the operand schema ([`slots`]); a local (a bare place, a place
//! record's local root, a closure's captures, a fallback row); a label; a
//! closure's sub-body; a record, copied once per copy; a type or type
//! list; and the side rows that name instructions ([`SideRows`]).
//!
//! Cost: one walk over the copied rows' words, with each record copied
//! once (a memo by source position) and each `extra` word mapped once (a
//! mark per word), so linear in the copied words. The tables are the call
//! site's O(1) lookups.

use hd_base::{HashMap, LocalId, Range32};
use hd_types::{Ty, TyList};

use super::{
    Body, CaptureMode, LoanSlot, Loc, NONE, Op, Providers, Ref, SuspRow, Tag, TypeWord,
    inst_record_type_words, place, slots, u32_of,
};

/// A copy's tables: what each kind of word becomes. The defaults keep a
/// word, so a renumbering names only its instruction table.
pub(crate) trait Remap {
    /// Instruction `w`'s copy; `None` when the copy leaves it out.
    fn inst(&self, w: u32) -> Option<u32>;
    /// Constant row `c`'s row in `out`, which may append it; `None` for
    /// a malformed constant.
    fn konst(&mut self, _out: &mut Body, c: u32) -> Option<u32> {
        Some(c)
    }
    fn local(&self, l: u32) -> Option<u32> {
        Some(l)
    }
    /// Label `l`'s copy, which may append its `label_inst` row.
    fn label(&mut self, _out: &mut Body, l: u32) -> Option<u32> {
        Some(l)
    }
    /// The sub-body source closure `i` names, `s` in the source.
    fn sub(&self, _i: u32, s: u32) -> Option<u32> {
        Some(s)
    }
    /// Whether [`Remap::ty`] and [`Remap::list`] change anything.
    fn types(&self) -> bool {
        false
    }
    fn ty(&mut self, t: Ty) -> Ty {
        t
    }
    fn list(&mut self, l: TyList) -> TyList {
        l
    }
    /// Where source record `w` already stands in `out`, when the call site
    /// placed the source's `extra` there whole (the placed range starts at
    /// `at(0)`); `None` copies each record once, as first named.
    fn at(&self, _w: u32) -> Option<u32> {
        None
    }
    /// Pushes block member `m`'s copies; `false` when it has none it
    /// should have.
    fn members(&self, m: u32, push: &mut dyn FnMut(u32)) -> bool {
        self.inst(m).map(push).is_some()
    }
}

impl Body {
    /// The instruction behind representation-preserving coercions of `a`.
    #[must_use]
    #[inline]
    pub fn direct_value(&self, a: u32) -> Option<usize> {
        let mut v = Ref(a).as_inst()?.0 as usize;
        while self.same_value_coercion(v) {
            v = Ref(self.data[v][0]).as_inst()?.0 as usize;
        }
        Some(v)
    }

    /// Appends a record of `words`; returns its length word's position.
    #[inline]
    pub fn push_record(&mut self, words: &[u32]) -> u32 {
        let at = u32_of(self.extra.len());
        self.extra.push(u32_of(words.len()));
        self.extra.extend_from_slice(words);
        at
    }
}

/// Appends an inlined frame of `callee`, called at anchor `call`, whose
/// rows stand for its body's rows `rows`, with an origin row for each
/// that is not its own position ([`Body::inlined`]).
pub(crate) fn push_frame(out: &mut Body, rows: impl Iterator<Item = u32>, callee: u32, call: u32) {
    let first = out.inlined.last().map_or(0, |e| e[0] + e[1]);
    let mut len = 0;
    for source in rows {
        if source != len {
            out.inlined_origins.push([first + len, source]);
        }
        len += 1;
    }
    out.inlined.push([first, len, callee, call]);
}

/// Grows `out`'s instruction columns to `len` rows, for a copy to fill.
pub(crate) fn grow(out: &mut Body, len: usize) {
    out.tags.resize(len, Tag::Poison);
    out.data.resize(len, [NONE, NONE]);
    out.ty.resize(len, Ty::VOID);
    out.syn.resize(len, hd_base::NodeIdx::NONE);
    out.anchors.resize(len, NONE);
}

fn bad(what: &str, w: u32) -> String {
    format!("{what} {w} outside its context")
}

fn local(map: &dyn Remap, l: u32) -> Result<u32, String> {
    map.local(l)
        .ok_or_else(|| format!("local {l} outside its scope"))
}

/// The `extra` words a copy has mapped, from where its records start.
struct Done {
    base: usize,
    seen: Vec<bool>,
}

impl Done {
    /// Whether word `at` is mapped for the first time; marks it.
    fn first(&mut self, at: usize) -> bool {
        let k = at - self.base;
        if self.seen.len() <= k {
            self.seen.resize(k + 1, false);
        }
        !std::mem::replace(&mut self.seen[k], true)
    }
}

/// Copies source rows `rows` (source, output row) of `src` into `out`,
/// whose columns hold those rows ([`grow`]), every word mapped by `map`:
/// first each row's columns and records, then each instruction and
/// constant word, which the operand schema finds in the copied records.
///
/// # Errors
/// A word `map` has no copy for.
pub(crate) fn copy_rows(
    src: &Body,
    rows: &[(u32, u32)],
    out: &mut Body,
    map: &mut dyn Remap,
) -> Result<(), String> {
    let mut done = Done {
        base: map.at(0).map_or(out.extra.len(), |at| at as usize),
        seen: Vec::new(),
    };
    let mut records: HashMap<u32, u32> = HashMap::default();
    let mut record = |out: &mut Body, map: &dyn Remap, w: u32| {
        map.at(w).unwrap_or_else(|| {
            *records
                .entry(w)
                .or_insert_with(|| out.push_record(src.record(w)))
        })
    };
    for &(i, n) in rows {
        let (i, n) = (i as usize, n as usize);
        let tag = src.tags[i];
        out.tags[n] = tag;
        out.ty[n] = map.ty(src.ty[i]);
        out.syn[n] = src.syn[i];
        out.anchors[n] = src.anchor(u32_of(i));
        let (oa, ob) = tag.operands();
        let mut data = src.data[i];
        for (slot, op) in [oa, ob].into_iter().enumerate() {
            let w = data[slot];
            if w == NONE {
                continue;
            }
            data[slot] = match op {
                Op::Place if w & place::BARE_LOCAL != 0 => {
                    place::BARE_LOCAL | local(map, w & !place::BARE_LOCAL)?
                }
                Op::Place => {
                    let at = record(out, map, w) as usize;
                    // A local root is renumbered and a global root's type
                    // mapped here; a value, frame or pointer root's word is
                    // an instruction, which the schema maps below.
                    let root = out.extra[at + 1];
                    if root == place::ROOT_LOCAL {
                        if done.first(at + 2) {
                            out.extra[at + 2] = local(map, out.extra[at + 2])?;
                        }
                    } else if root == place::ROOT_GLOBAL && map.types() && done.first(at + 3) {
                        out.extra[at + 3] = map.ty(Ty(out.extra[at + 3])).0;
                    }
                    u32_of(at)
                }
                Op::Label => map.label(out, w).ok_or_else(|| bad("label", w))?,
                Op::Meta if tag == Tag::Closure => {
                    map.sub(u32_of(i), w).ok_or_else(|| bad("sub-body", w))?
                }
                Op::Record if tag == Tag::Closure => {
                    let rec = src.record(w);
                    let (start, count) = (rec[0], rec[1]);
                    // Each closure its own rows: the passes read a
                    // capture's row as that closure's use.
                    let first = u32_of(out.cap_local.len());
                    for j in start as usize..(start + count) as usize {
                        let l = local(map, src.cap_local[j].raw())?;
                        out.cap_local.push(LocalId::from_raw(l));
                        // The checker's `Move` assumed the one defining
                        // closure; each instantiation makes an
                        // environment, so it copies.
                        let mode = src.cap_mode[j];
                        out.cap_mode.push(
                            if src.is_instantiation(i) && mode == CaptureMode::Move {
                                CaptureMode::Copy
                            } else {
                                mode
                            },
                        );
                    }
                    out.push_record(&[first, count])
                }
                Op::Record if tag == Tag::Block => {
                    let mut members = Vec::new();
                    for &m in src.record(w) {
                        if !map.members(m, &mut |x| members.push(x)) {
                            return Err(bad("block member", m));
                        }
                    }
                    // A placed list keeps its place when its length holds.
                    let at = match map.at(w) {
                        Some(at) if src.record(w).len() == members.len() => {
                            let at = at as usize;
                            out.extra[at + 1..=at + members.len()].copy_from_slice(&members);
                            u32_of(at)
                        }
                        _ => out.push_record(&members),
                    };
                    for p in at as usize..=at as usize + members.len() {
                        done.first(p);
                    }
                    at
                }
                Op::Values | Op::Blocks | Op::Record => {
                    let at = record(out, map, w);
                    // A call's provider pairs sit in their own range,
                    // which the record's footer names.
                    if let Some(footer) = src.call_footer(i)
                        && slot == 1
                        && let Providers::Pairs(p) = src.call_providers(i)
                        && p.len > 0
                    {
                        let foot = at as usize + (footer - w as usize);
                        if done.first(foot + 1) {
                            let start = out.extra.len();
                            let range = p.start as usize..(p.start + p.len) as usize;
                            out.extra.extend_from_slice(&src.extra[range]);
                            for key in out.extra[start..].iter_mut().step_by(2) {
                                *key = map.ty(Ty(*key)).0;
                            }
                            out.extra[foot + 1] = u32_of(start);
                        }
                    }
                    at
                }
                Op::Meta if matches!(tag, Tag::ItemRef | Tag::ProviderGet) => record(out, map, w),
                _ => w,
            };
        }
        out.data[n] = data;
        if map.types() {
            let mut words = Vec::new();
            inst_record_type_words(out, n, &mut |at, word| words.push((at, word)));
            for (at, word) in words {
                if done.first(at) {
                    let w = out.extra[at];
                    out.extra[at] = match word {
                        TypeWord::Ty => map.ty(Ty(w)).0,
                        TypeWord::List => map.list(TyList(w)).0,
                    };
                }
            }
        }
    }
    let mut locs = Vec::new();
    for &(_, n) in rows {
        map_slots(out, n as usize, map, &mut done, &mut locs).map_err(|w| bad("instruction", w))?;
    }
    Ok(())
}

/// Maps every instruction and constant word of row `n` of `b`, each
/// `extra` word once; the error is a word `map` has no entry for.
fn map_slots(
    b: &mut Body,
    n: usize,
    map: &mut dyn Remap,
    done: &mut Done,
    locs: &mut Vec<(Loc, u32)>,
) -> Result<(), u32> {
    locs.clear();
    slots(b, n, |loc, w, _| locs.push((loc, w)));
    for &(loc, w) in locs.iter() {
        if let Loc::Extra(p) = loc
            && !done.first(p as usize)
        {
            continue;
        }
        if w == NONE {
            continue;
        }
        let new = if w & Ref::CONST_BIT != 0 {
            Ref::CONST_BIT | map.konst(b, w & !Ref::CONST_BIT).ok_or(w)?
        } else {
            map.inst(w).ok_or(w)?
        };
        match loc {
            Loc::Data(d) => b.data[n][usize::from(d)] = new,
            Loc::Extra(p) => b.extra[p as usize] = new,
        }
    }
    Ok(())
}

/// Instruction tables for a renumbering in place: instruction `w` becomes
/// `new_of[w]` (`NONE`: deleted), constant `c` moves `konst` rows on.
pub(crate) struct Through<'a> {
    pub new_of: &'a [u32],
    pub konst: u32,
}

impl Remap for Through<'_> {
    fn inst(&self, w: u32) -> Option<u32> {
        self.new_of.get(w as usize).copied()
    }

    fn konst(&mut self, _: &mut Body, c: u32) -> Option<u32> {
        Some(c + self.konst)
    }
}

/// Renumbers `b` in place through `map`'s instructions and constants:
/// every operand word, each shared record's once, and every column that
/// names an instruction. A side row whose instruction `map` deletes goes
/// with it.
///
/// # Errors
/// An instruction word `map` has no entry for.
pub(crate) fn renumber(b: &mut Body, map: &mut Through<'_>) -> Result<(), String> {
    let mut done = Done {
        base: 0,
        seen: vec![false; b.extra.len()],
    };
    let mut locs = Vec::new();
    for i in 0..b.len() {
        map_slots(b, i, map, &mut done, &mut locs)
            .map_err(|w| format!("instruction operand %{w} out of range while splicing"))?;
    }
    let word = |w: u32| match w {
        NONE => NONE,
        w => map.inst(w).unwrap_or(w),
    };
    for x in &mut b.label_inst {
        *x = word(*x);
    }
    for x in &mut b.sub_root {
        *x = word(*x);
    }
    for row in &mut b.forwarded_reads {
        *row = (word(row.0), word(row.1));
    }
    b.forwarded_reads
        .retain(|&(value, read)| value != NONE && read != NONE);
    // Both words name instructions no edit deletes (a suspension point and
    // its cancel block), and the splice keeps their order.
    for row in &mut b.cancel {
        *row = (word(row.0), word(row.1));
    }
    // A read the edits delete takes its row with it.
    for row in &mut b.full_cells {
        row.0 = word(row.0);
    }
    b.full_cells.retain(|r| r.0 != NONE);
    // A loop is never deleted; the place is rooted at a local.
    for row in &mut b.stable_lists {
        row.0 = word(row.0);
    }
    // Suspension scopes are mapped where they stand, so the body's
    // records keep their places.
    for mut row in std::mem::take(&mut b.susp) {
        let Some(inst) = map.inst(row.inst).filter(|&n| n != NONE) else {
            continue;
        };
        for p in row.scopes.start as usize..(row.scopes.start + row.scopes.len) as usize {
            if done.first(p) {
                b.extra[p] = word(b.extra[p]);
            }
        }
        row.inst = inst;
        row.hook_site = word(row.hook_site);
        b.susp.push(row);
    }
    SideRows::take(b).copy(b, map)
}

/// Calls `f` with each local instruction `i` names: a place's root, and
/// a closure's captures.
pub(crate) fn local_words(b: &Body, i: usize, f: &mut dyn FnMut(u32)) {
    let (oa, ob) = b.tags[i].operands();
    for (w, op) in [(b.data[i][0], oa), (b.data[i][1], ob)] {
        if op != Op::Place || w == NONE {
            continue;
        }
        if w & place::BARE_LOCAL != 0 {
            f(w & !place::BARE_LOCAL);
        } else if b.extra.get(w as usize + 1) == Some(&place::ROOT_LOCAL) {
            f(b.extra[w as usize + 2]);
        }
    }
    if b.tags[i] == Tag::Closure {
        let rec = b.record(b.data[i][1]);
        for l in &b.cap_local[rec[0] as usize..(rec[0] + rec[1]) as usize] {
            f(l.raw());
        }
    }
}

/// Side rows that name instructions: `fallback`, `held_args`,
/// `loan_slots` and `susp` (with its scope words).
#[derive(Clone, Default)]
pub(crate) struct SideRows {
    pub fallback: Vec<[u32; 2]>,
    pub held: Vec<(u32, u32)>,
    pub loan_slots: Vec<LoanSlot>,
    susp: Vec<(u32, Vec<u32>, u32)>,
}

impl SideRows {
    /// `b`'s side rows in `groups` groups, by `group` of the instruction
    /// each names: one pass over them.
    pub(crate) fn group(b: &Body, groups: usize, group: impl Fn(u32) -> usize) -> Vec<Self> {
        let mut out = vec![Self::default(); groups];
        for &row in &b.fallback {
            out[group(row[0])].fallback.push(row);
        }
        for &row in &b.held_args {
            out[group(row.0)].held.push(row);
        }
        for row in &b.loan_slots {
            out[group(row.call)].loan_slots.push(row.clone());
        }
        for row in &b.susp {
            out[group(row.inst)].susp.push(Self::susp_row(b, row));
        }
        out
    }

    fn susp_row(b: &Body, row: &SuspRow) -> (u32, Vec<u32>, u32) {
        let (s, len) = (row.scopes.start as usize, row.scopes.len as usize);
        (row.inst, b.extra[s..s + len].to_vec(), row.hook_site)
    }

    /// `b`'s side rows but `susp`, taken out of it.
    pub(crate) fn take(b: &mut Body) -> Self {
        Self {
            fallback: std::mem::take(&mut b.fallback),
            held: std::mem::take(&mut b.held_args),
            loan_slots: std::mem::take(&mut b.loan_slots),
            susp: Vec::new(),
        }
    }

    /// Appends the rows to `out`'s, mapped by `map`; a row whose
    /// instruction `map` leaves out is dropped.
    ///
    /// # Errors
    /// A local or suspension scope `map` has no copy for.
    pub(crate) fn copy(&self, out: &mut Body, map: &mut dyn Remap) -> Result<(), String> {
        let kept = |w: u32| map.inst(w).filter(|&n| n != NONE);
        for &[i, l] in &self.fallback {
            if let Some(n) = kept(i) {
                let l = if l == 0 { 0 } else { local(map, l - 1)? + 1 };
                out.fallback.push([n, l]);
            }
        }
        for &(call, arg) in &self.held {
            if let (Some(call), Some(arg)) = (kept(call), kept(arg)) {
                out.held_args.push((call, arg));
            }
        }
        for row in &self.loan_slots {
            let mut row = row.clone();
            if row.remap_values(&kept).is_some() {
                out.loan_slots.push(row);
            }
        }
        for (inst, scopes, hook) in &self.susp {
            let Some(inst) = kept(*inst) else {
                continue;
            };
            let mut words = Vec::with_capacity(scopes.len());
            for &w in scopes {
                words.push(kept(w).ok_or_else(|| bad("scope", w))?);
            }
            let at = out.push_record(&words);
            out.susp.push(SuspRow {
                inst,
                scopes: Range32 {
                    start: at,
                    len: u32_of(words.len()),
                },
                hook_site: if *hook == NONE {
                    NONE
                } else {
                    kept(*hook).unwrap_or(NONE)
                },
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{BodyKind, Callee, TirBuilder, TirSink};
    use hd_base::{DefId, NodeIdx};

    /// A root block of three calls, with a fallback and a held-argument
    /// row naming each.
    fn calls() -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let root = b.open_block();
        let callee = Callee::Item {
            def: DefId::from_raw(7),
            targs: TyList::EMPTY,
        };
        for _ in 0..3 {
            b.call(&callee, &[], Providers::None, Ty::VOID, NodeIdx::NONE);
        }
        let root = b.close_block(root, None, Ty::VOID, NodeIdx::NONE);
        let mut b = b.finish(root, &[]).unwrap();
        for i in 0..3 {
            b.fallback.push([i, 0]);
            b.held_args.push((i, i));
        }
        b
    }

    /// A renumbering moves every row that names a kept instruction and
    /// drops the rows of the one it deletes.
    #[test]
    fn renumbering_drops_the_side_rows_of_a_deleted_instruction() {
        let mut b = calls();
        let n = b.len();
        let mut new_of: Vec<u32> = (0..u32_of(n)).collect();
        new_of[1] = NONE;
        new_of[2] = 1;
        renumber(
            &mut b,
            &mut Through {
                new_of: &new_of,
                konst: 0,
            },
        )
        .unwrap();
        assert_eq!(b.fallback, [[0, 0], [1, 0]]);
        assert_eq!(b.held_args, [(0, 0), (1, 1)]);
    }

    /// Instructions shifted by one, as a copy into a body that holds one
    /// row before them.
    struct Shift;

    impl Remap for Shift {
        fn inst(&self, w: u32) -> Option<u32> {
            Some(w + 1)
        }
    }

    /// Each copy into a body copies the records its rows name once, and
    /// maps each of their words once.
    #[test]
    fn a_copy_maps_each_word_once_into_its_own_records() {
        let src = calls();
        let mut out = Body::new(src.item, src.kind);
        grow(&mut out, src.len() + 1);
        let rows: Vec<(u32, u32)> = (0..u32_of(src.len())).map(|i| (i, i + 1)).collect();
        copy_rows(&src, &rows, &mut out, &mut Shift).unwrap();
        SideRows::group(&src, 1, |_| 0)[0]
            .copy(&mut out, &mut Shift)
            .unwrap();
        let root = src.len();
        assert_eq!(out.record(out.data[root][0]), [1, 2, 3]);
        assert_eq!(out.fallback, [[1, 0], [2, 0], [3, 0]]);
        assert_eq!(out.held_args, [(1, 1), (2, 2), (3, 3)]);
        // A second copy has records of its own.
        let first = out.extra.len();
        let mut again = out.clone();
        copy_rows(&src, &rows, &mut again, &mut Shift).unwrap();
        assert_eq!(again.extra.len(), 2 * first);
    }
}
