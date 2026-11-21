//! Drop elaboration (step 7d1 of `docs/future-work/BACKEND_NEUTRAL_TIR.md`),
//! as rustc's `ElaborateDrops` runs after borrow checking: each release of
//! a place whose drop flags ([`super::drop_flags`]) the backend would read
//! becomes statements over those flags. The flag of the place guards it
//! all; with flags under it, the place drops whole when they all hold,
//! else each part drops the same way (rustc's open drop), so the hook of
//! a partly moved value never runs. A `Drop` of a variant's field drops
//! only while the enum holds that variant. The sites: a `Drop`, a cancel
//! block's `Release` of a value, a `Store` that releases what it
//! overwrites, and a parameter's exit `Release`, which runs under its flag
//! or, with none, under whether the instance owns it
//! ([`IntrinsicOp::ParamOwned`]). A cell keeps its flags in its layout.
//!
//! Cost: one walk, O(N), then per site O(the flags at and under its place
//! and the fields on their paths), the output's size; one renumbering.

use hd_base::{HashMap, NodeIdx, SliceSort as _};
use hd_types::Ty;

use super::OwnCx;
use super::drop_flags::{Key, cells, key_of, read, step_words, word_of};
use crate::ir::count::{At, Info, Part};
use crate::ir::{
    Body, IntrinsicOp, NONE, PrimOp, ReadMode, Ref, Tag, Topology, grow, local_flags, place, u32_of,
};

/// One root's flagged paths, as step words and flag local, sorted so the
/// paths under a path follow it.
type Paths = Vec<(Vec<u32>, u32)>;

/// The open drop of one site: its root, its flags and its `Drop`s' reason.
struct Tree<'a> {
    cx: &'a dyn OwnCx,
    paths: &'a [(Vec<u32>, u32)],
    key: Key,
    reason: u32,
    syn: NodeIdx,
}

impl Tree<'_> {
    fn row(&self, b: &mut Body, tag: Tag, data: [u32; 2], ty: Ty) -> u32 {
        let r = b.len();
        grow(b, r + 1);
        (b.tags[r], b.data[r], b.ty[r], b.syn[r]) = (tag, data, ty, self.syn);
        u32_of(r)
    }

    fn read(&self, b: &mut Body, flag: u32) -> u32 {
        let data = [place::BARE_LOCAL | flag, ReadMode::Copy as u32];
        self.row(b, Tag::Read, data, Ty::BOOL)
    }

    /// `If cond [then]`, into `out`.
    fn when(&self, b: &mut Body, cond: u32, then: &[u32], out: &mut Vec<u32>) {
        let list = b.push_record(then);
        let then = self.row(b, Tag::Block, [list, NONE], Ty::VOID);
        let arms = b.push_record(&[then, NONE]);
        out.push(self.row(b, Tag::If, [cond, arms], Ty::VOID));
    }

    fn leaf(&self, b: &mut Body, steps: &[u32]) -> u32 {
        let word = word_of(b, self.key, steps);
        self.row(b, Tag::Drop, [word, self.reason], Ty::VOID)
    }

    /// The open drop of the place at `steps`, of type `ty`, into `out`. The
    /// rows go in evaluation order: each read before what it guards.
    fn open(
        &self,
        b: &mut Body,
        steps: &mut Vec<u32>,
        ty: Ty,
        out: &mut Vec<u32>,
    ) -> Result<(), String> {
        let own = (self.paths.iter().find(|p| p.0 == *steps)).map(|p| self.read(b, p.1));
        let below: Vec<u32> = (self.paths.iter())
            .filter(|p| p.0.len() > steps.len() && p.0.starts_with(steps))
            .map(|p| p.1)
            .collect();
        let mut list = Vec::new();
        if let Some((&first, rest)) = below.split_first() {
            let mut all = self.read(b, first);
            list.push(all);
            for &f in rest {
                let r = self.read(b, f);
                let args = b.push_record(&[all, r]);
                all = self.row(b, Tag::Prim, [PrimOp::And as u32, args], Ty::BOOL);
                list.extend([r, all]);
            }
            let whole = [self.leaf(b, steps)];
            let whole = b.push_record(&whole);
            let whole = self.row(b, Tag::Block, [whole, NONE], Ty::VOID);
            let mut parts = Vec::new();
            self.below(b, steps, ty, &mut parts)?;
            let parts = b.push_record(&parts);
            let parts = self.row(b, Tag::Block, [parts, NONE], Ty::VOID);
            let arms = b.push_record(&[whole, parts]);
            list.push(self.row(b, Tag::If, [all, arms], Ty::VOID));
        } else {
            list.push(self.leaf(b, steps));
        }
        match own {
            Some(c) => {
                out.push(c);
                self.when(b, c, &list, out);
            }
            None => out.extend(list),
        }
        Ok(())
    }

    /// The parts of the place at `steps`: a struct's or tuple's fields, or
    /// each variant's payload fields.
    fn below(
        &self,
        b: &mut Body,
        steps: &mut Vec<u32>,
        ty: Ty,
        out: &mut Vec<u32>,
    ) -> Result<(), String> {
        let mut fields = Vec::new();
        let variants = !self.cx.drop_parts(ty, None, &mut fields);
        let mut v = 0;
        while !variants || self.cx.drop_parts(ty, Some(v), &mut fields) {
            for (f, t) in (0u32..).zip(std::mem::take(&mut fields)) {
                let n = steps.len();
                steps.extend(variants.then_some(place::PROJ_VARIANT | v));
                steps.push(place::PROJ_FIELD | f);
                self.open(b, steps, t, out)?;
                steps.truncate(n);
            }
            if !variants {
                return Ok(());
            }
            v += 1;
        }
        if v == 0 {
            // ruled out: the checker moves a part only out of a value laid
            // out in place, whose parts `OwnCx::parts` names.
            return Err(format!(
                "a drop flag under a part of {ty:?}, which does not open"
            ));
        }
        Ok(())
    }
}

/// The type of the part at `steps` of a `ty` value.
fn ty_at(cx: &dyn OwnCx, mut ty: Ty, steps: &[u32]) -> Option<Ty> {
    let (mut variant, mut out) = (None, Vec::new());
    for &s in steps {
        if s & place::PROJ_KIND == place::PROJ_VARIANT {
            variant = Some(s & place::PROJ_INDEX);
            continue;
        }
        cx.drop_parts(ty, variant.take(), &mut out).then_some(())?;
        ty = *out.get((s & place::PROJ_INDEX) as usize)?;
    }
    Some(ty)
}

/// Elaborates `b`'s flagged releases (module doc). Runs after the exit
/// set ([`super::exits`]).
///
/// # Errors
/// A flag under a part that does not open, or a refused renumbering.
pub fn elaborate(b: &mut Body, cx: &dyn OwnCx) -> Result<(), String> {
    use super::reason::{CANCELLED, EXIT_DROP, OVERWRITTEN};
    let (cells, _) = cells(b);
    let (n, topo) = (b.len(), Topology::new(b));
    let (mut spans, mut gone, mut before) = (Vec::new(), vec![false; n], HashMap::default());
    let mut blocks = Vec::new();
    let mut flags: HashMap<Key, Paths> = HashMap::default();
    for (i, [word, flag]) in b.data.iter().copied().enumerate() {
        // A parameter's whole flag starts as whether it is owned.
        let (word, flag) = match (b.tags[i], b.param_owned(flag)) {
            (Tag::FlagSet | Tag::FlagClear, _) => (word, flag),
            (Tag::Store, Some(l)) => (place::BARE_LOCAL | l, word & !place::BARE_LOCAL),
            _ => continue,
        };
        match key_of(b, word).filter(|k| !matches!(k, Key::Local(l) if cells[*l as usize])) {
            Some(key) if read(b, cx, key) => {
                let paths = flags.entry(key).or_default();
                paths.push((step_words(b, word).to_vec(), flag));
            }
            // A flag no drop reads goes (an instance whose type holds no hook).
            Some(_) => {
                gone[i] = true;
                blocks.push(topo.blk[i]);
            }
            None => {}
        }
    }
    for paths in flags.values_mut() {
        paths.sort_stable();
        paths.dedup_by(|a, b| a.0 == b.0);
    }
    let cancel: hd_base::HashSet<u32> = b.cancel.iter().map(|c| c.1).collect();
    for x in 0..n {
        let [a, w] = b.data[x];
        let next = || {
            b.record(b.data[topo.blk[x] as usize][0])
                .get(topo.pos[x] as usize + 1)
        };
        let old = Some(Info::new(At::Operation, Part::Result, OVERWRITTEN));
        // The place, the `Drop`s' reason, the parameter an exit count
        // releases, and the count a store releases what it overwrites by.
        let (word, reason, param, marker) = match (b.tags[x], Info::read(b, x)) {
            (Tag::Drop, _) => (a, w, None, None),
            (Tag::Release, _) if cancel.contains(&topo.blk[x]) && Ref(a).as_inst().is_some() => {
                (word_of(b, Key::Value(a), &[]), CANCELLED, None, None)
            }
            (
                Tag::Release,
                Some(Info {
                    at: At::Exit,
                    part: Part::Local(l),
                    ..
                }),
            ) if b.local_flags[l as usize] & local_flags::PARAM != 0 => {
                (place::BARE_LOCAL | l, EXIT_DROP, Some(l), None)
            }
            (Tag::Store, _) if topo.blk[x] != NONE => match next().copied() {
                Some(m)
                    if Info::read(b, m as usize) == old && b.tags[m as usize] == Tag::Release =>
                {
                    (a, OVERWRITTEN, None, Some(m))
                }
                _ => continue,
            },
            _ => continue,
        };
        let Some(key) = key_of(b, word) else { continue };
        let mut steps = step_words(b, word).to_vec();
        let paths = flags.get(&key).map_or(&[][..], Vec::as_slice);
        let flagged = paths.iter().any(|p| p.0.starts_with(&steps));
        if !flagged && param.is_none() {
            continue;
        }
        if let Some(m) = marker {
            gone[m as usize] = true;
        }
        let first = u32_of(b.len());
        let tree = Tree {
            cx,
            paths,
            key,
            reason,
            syn: b.syn[x],
        };
        let mut out = Vec::new();
        if let Some(l) = param.filter(|_| !flagged) {
            b.consts.push((Ty::I32, u64::from(l)));
            let args = b.push_record(&[Ref::konst(u32_of(b.consts.len() - 1)).0]);
            let owned = tree.row(
                b,
                Tag::Intrinsic,
                [IntrinsicOp::ParamOwned as u32, args],
                Ty::BOOL,
            );
            out.push(owned);
            let leaf = tree.leaf(b, &steps);
            tree.when(b, owned, &[leaf], &mut out);
        } else {
            let root = match key {
                Key::Local(l) => b.local_ty[l as usize],
                Key::Value(v) => b.ty[v as usize],
            };
            let ty = ty_at(cx, root, &steps).ok_or("a dropped part with no type")?;
            tree.open(b, &mut steps, ty, &mut out)?;
        }
        spans.push((u32_of(x), first, u32_of(b.len())));
        // A store stays, after what it overwrote drops; a parameter's exit
        // count stays, a marker the verifier reads and the backend skips.
        gone[x] |= b.tags[x] == Tag::Drop || param.is_none() && b.tags[x] == Tag::Release;
        before.insert(u32_of(x), out);
        blocks.push(topo.blk[x]);
    }
    if blocks.is_empty() {
        return Ok(());
    }
    blocks.sort_stable();
    blocks.dedup();
    for blk in blocks {
        let mut list = Vec::new();
        for &x in b.record(b.data[blk as usize][0]) {
            list.extend(before.get(&x).into_iter().flatten());
            list.extend(Some(x).filter(|&x| !gone[x as usize]));
        }
        b.data[blk as usize][0] = b.push_record(&list);
    }
    super::scopes::place(b, n, spans, &gone)
}
