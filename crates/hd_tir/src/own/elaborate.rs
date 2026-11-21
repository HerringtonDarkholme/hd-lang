//! The exit set and drop elaboration as statements (steps 7c and 7d1 of
//! `docs/future-work/BACKEND_NEUTRAL_TIR.md`), after the passes' verifier,
//! as rustc elaborates drops after borrow checking. Each release of a
//! place goes under the drop flags ([`super::drop_flags`]) its drops read:
//! the place's own flag guards it all; with flags under it, it drops whole
//! when they all hold, else each part the same way (rustc's open drop), so
//! the hook of a partly moved value never runs. A `Drop` of a variant's
//! field drops only while the enum holds that variant. A parameter with no
//! flag releases under whether the instance owns it
//! ([`IntrinsicOp::ParamOwned`]), which its flag starts as otherwise. The
//! releases: a `Drop`, a cancel block's `Release` of a value, what a
//! `Store` overwrites, and the exit set ([`Body::exit_drops`], then the
//! parameters' exit counts) before each `return`, each cancel exit and at
//! a root's end; a value a `return` does not consume takes the caller's
//! count first, a stated `Retain`. Each way out takes its own copy:
//! [`super::scopes::lower`]'s chains wrap a body in blocks (bench +1.8%).
//! A cell keeps its flags in its layout.
//!
//! Cost: one walk and a consumer census, O(N), then per release O(the
//! flags at and under its place and the fields on their paths), the
//! output's size; one renumbering.

use hd_base::{HashMap, NodeIdx, SliceSort as _};
use hd_types::Ty;

use super::OwnCx;
use super::drop_flags::{Key, cells, key_of, read, step_words, word_of};
use super::reason::{CANCELLED, EXIT_DROP, OVERWRITTEN};
use crate::ir::count::{At, Info, Part};
use crate::ir::{Body, IntrinsicOp, NONE, PrimOp, ReadMode, Ref, Tag, Topology, grow};
use crate::ir::{local_flags, place, return_word, u32_of};

/// A root's flagged paths, as step words and flag local, sorted so the
/// paths under a path follow it.
type Paths = Vec<(Vec<u32>, u32)>;

/// The rows being written, and the release being written: its root, its
/// flags, its `Drop`s' reason and source position.
struct Elab<'a> {
    b: &'a mut Body,
    cx: &'a dyn OwnCx,
    flags: HashMap<Key, Paths>,
    out: Vec<u32>,
    key: Key,
    paths: Paths,
    reason: u32,
    syn: NodeIdx,
}

impl Elab<'_> {
    fn row(&mut self, tag: Tag, data: [u32; 2], ty: Ty) -> u32 {
        let r = self.b.len();
        let b = &mut *self.b;
        grow(b, r + 1);
        (b.tags[r], b.data[r], b.ty[r], b.syn[r]) = (tag, data, ty, self.syn);
        u32_of(r)
    }

    /// Reads flag local `flag`; `NONE`: whether the instance owns the
    /// parameter that is the root.
    fn read(&mut self, flag: u32) -> u32 {
        let (Key::Local(l), NONE) = (self.key, flag) else {
            let data = [place::BARE_LOCAL | flag, ReadMode::Copy as u32];
            return self.row(Tag::Read, data, Ty::BOOL);
        };
        self.b.consts.push((Ty::I32, u64::from(l)));
        let k = Ref::konst(u32_of(self.b.consts.len() - 1)).0;
        let args = self.b.push_record(&[k]);
        self.row(
            Tag::Intrinsic,
            [IntrinsicOp::ParamOwned as u32, args],
            Ty::BOOL,
        )
    }

    /// `If cond [arms]`, an arm `None` for none.
    fn when(&mut self, cond: u32, arms: [Option<Vec<u32>>; 2]) -> u32 {
        let arms = arms.map(|arm| {
            let list = arm.map(|list| self.b.push_record(&list));
            list.map_or(NONE, |list| self.row(Tag::Block, [list, NONE], Ty::VOID))
        });
        let arms = self.b.push_record(&arms);
        self.row(Tag::If, [cond, arms], Ty::VOID)
    }

    /// The open drop of the place at `steps`, of type `ty`, in evaluation
    /// order: each read before what it guards.
    fn open(&mut self, steps: &mut Vec<u32>, ty: Ty) -> Result<Vec<u32>, String> {
        let own = self.paths.iter().find(|p| p.0 == *steps).map(|p| p.1);
        let own = own.map(|f| self.read(f));
        let below: Vec<u32> = (self.paths.iter())
            .filter(|p| p.0.len() > steps.len() && p.0.starts_with(steps))
            .map(|p| p.1)
            .collect();
        let word = word_of(self.b, self.key, steps);
        let leaf = self.row(Tag::Drop, [word, self.reason], Ty::VOID);
        let mut list = vec![leaf];
        if let Some((&first, rest)) = below.split_first() {
            let mut all = self.read(first);
            list = vec![all];
            for &f in rest {
                let r = self.read(f);
                let args = self.b.push_record(&[all, r]);
                all = self.row(Tag::Prim, [PrimOp::And as u32, args], Ty::BOOL);
                list.extend([r, all]);
            }
            let parts = self.below(steps, ty)?;
            list.push(self.when(all, [Some(vec![leaf]), Some(parts)]));
        }
        Ok(match own {
            Some(c) => vec![c, self.when(c, [Some(list), None])],
            None => list,
        })
    }

    /// The parts of the place at `steps`: a struct's or tuple's fields, or
    /// each variant's payload fields.
    fn below(&mut self, steps: &mut Vec<u32>, ty: Ty) -> Result<Vec<u32>, String> {
        let (mut fields, mut out) = (Vec::new(), Vec::new());
        let variants = !self.cx.drop_parts(ty, None, &mut fields);
        let mut v = 0;
        while !variants || self.cx.drop_parts(ty, Some(v), &mut fields) {
            for (f, t) in (0u32..).zip(std::mem::take(&mut fields)) {
                let n = steps.len();
                steps.extend(variants.then_some(place::PROJ_VARIANT | v));
                steps.push(place::PROJ_FIELD | f);
                out.extend(self.open(steps, t)?);
                steps.truncate(n);
            }
            if !variants {
                return Ok(out);
            }
            v += 1;
        }
        // ruled out: the checker moves a part only out of a value laid out
        // in place, whose parts `OwnCx::drop_parts` names.
        let what = || format!("a drop flag under a part of {ty:?}, which does not open");
        (v > 0).then_some(out).ok_or_else(what)
    }

    /// Releases place `word` under its flags: a parameter's (`param`), with
    /// none, under whether the instance owns it.
    fn release(&mut self, word: u32, reason: u32, param: bool) -> Result<(), String> {
        self.key = key_of(self.b, word).ok_or("a release of a place in no local or value")?;
        self.reason = reason;
        let owned = param.then(|| (Vec::new(), NONE));
        self.paths = self
            .flags
            .get(&self.key)
            .cloned()
            .unwrap_or_else(|| owned.into_iter().collect());
        let mut steps = step_words(self.b, word).to_vec();
        let mut ty = match self.key {
            Key::Local(l) => self.b.local_ty[l as usize],
            Key::Value(v) => self.b.ty[v as usize],
        };
        let (mut variant, mut parts) = (None, Vec::new());
        for &s in &steps {
            if s & place::PROJ_KIND == place::PROJ_VARIANT {
                variant = Some(s & place::PROJ_INDEX);
            } else if self.cx.drop_parts(ty, variant.take(), &mut parts) {
                ty = parts[(s & place::PROJ_INDEX) as usize];
            }
        }
        let rows = self.open(&mut steps, ty)?;
        self.out.extend(rows);
        Ok(())
    }

    /// Whether a flag stands at or under place `word`.
    fn flagged(&self, word: u32) -> bool {
        let paths = key_of(self.b, word).and_then(|k| self.flags.get(&k));
        let steps = step_words(self.b, word);
        paths.is_some_and(|p| p.iter().any(|q| q.0.starts_with(steps)))
    }
}

/// Writes `b`'s exit set and elaborates its releases (module doc).
///
/// # Errors
/// A flag under a part that does not open, or a refused renumbering.
pub fn elaborate(b: &mut Body, cx: &dyn OwnCx) -> Result<(), String> {
    let (cells, _) = cells(b);
    let (n, topo) = (b.len(), Topology::new(b));
    let (mut spans, mut gone, mut blocks) = (Vec::new(), vec![false; n], Vec::new());
    let mut put = HashMap::<(u32, bool), Vec<u32>>::default();
    let mut flags = HashMap::<Key, Paths>::default();
    // Each sub-body's exit set: its exit `Drop`s, then its parameters'.
    let mut exit = vec![Vec::new(); b.sub_root.len()];
    for &(s, l) in &b.exit_drops {
        exit[s.idx()].push((place::BARE_LOCAL | l.raw(), NONE));
    }
    for (i, [word, flag]) in b.data.iter().copied().enumerate() {
        let info = Info::read(b, i).filter(|c| c.at == At::Exit && b.tags[i] == Tag::Release);
        if let Some(Part::Local(l)) = info.map(|c| c.part)
            && b.local_flags[l as usize] & local_flags::PARAM != 0
        {
            // Its count moves to each way out (`own::counts::verify`: once on each).
            exit[topo.sub[i] as usize].push((place::BARE_LOCAL | l, u32_of(i)));
            gone[i] = true;
            blocks.push(topo.blk[i]);
        }
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
    // A cancel block stands in no list: its point's sub-body is its own.
    let mut sub: Vec<usize> = topo.sub.iter().map(|&s| s as usize).collect();
    for &(p, blk) in &b.cancel {
        for &r in b.record(b.data[blk as usize][0]) {
            sub[r as usize] = sub[p as usize];
        }
    }
    let cancel: hd_base::HashSet<u32> = b.cancel.iter().map(|c| c.1).collect();
    let mut consumers = Vec::new();
    super::consumers(b, &topo, cx, &mut consumers);
    let (key, out, paths) = (Key::Value(0), Vec::new(), Vec::new());
    let mut e = Elab {
        b,
        cx,
        flags,
        out,
        key,
        paths,
        reason: 0,
        syn: NodeIdx::NONE,
    };
    let old = Some(Info::new(At::Operation, Part::Result, OVERWRITTEN));
    for x in 0..n {
        let ([a, w], blk, b) = (e.b.data[x], topo.blk[x], &*e.b);
        let list = |at: u32| b.record(b.data[at as usize][0]);
        let next = (blk != NONE).then(|| list(blk).get(topo.pos[x] as usize + 1));
        let next = next
            .flatten()
            .copied()
            .filter(|&m| Info::read(b, m as usize) == old);
        let ends = |m: Option<&u32>| m.is_none_or(|&m| b.tags[m as usize] != Tag::Return);
        let (site, end) = match b.tags[x] {
            Tag::Drop if e.flagged(a) => ((a, w), false),
            Tag::Release if cancel.contains(&blk) && e.flags.contains_key(&Key::Value(a)) => {
                ((word_of(e.b, Key::Value(a), &[]), CANCELLED), false)
            }
            Tag::Store if next.is_some() && e.flagged(a) => ((a, OVERWRITTEN), false),
            // A return's word: `NONE`, or one of [`return_word`]'s.
            Tag::Return if blk != NONE => ((NONE, w), false),
            // A root's end, unless it leaves by a `return`.
            Tag::Block if topo.is_root[x] && ends(list(u32_of(x)).last()) => ((NONE, NONE), true),
            _ => continue,
        };
        e.syn = e.b.syn[x];
        if site.0 == NONE {
            let returned =
                !end && Ref(a).as_inst().is_some() && consumers.get(a as usize) != Some(&u32_of(x));
            if returned
                && ![return_word::BORROWED, return_word::CANCELLED].contains(&w)
                && super::tracked(e.b, cx, a as usize)
            {
                let data = [a, Info::new(At::Value, Part::Result, 0).write(e.b)];
                let retain = e.row(Tag::Retain, data, Ty::VOID);
                e.out.push(retain);
            }
            for &(word, count) in &exit[sub[x]] {
                e.release(word, EXIT_DROP, count != NONE)?;
                if count != NONE {
                    let data = e.b.data[count as usize];
                    let count = e.row(Tag::Release, data, Ty::VOID);
                    e.out.push(count);
                }
            }
        } else {
            // A store stays; the count it released what it overwrote by goes.
            gone[next.map_or(x, |m| m as usize)] = true;
            e.release(site.0, site.1, false)?;
        }
        let first = spans.last().map_or(u32_of(n), |s: &(u32, u32, u32)| s.2);
        spans.push((u32_of(x), first, u32_of(e.b.len())));
        put.insert((u32_of(x), end), std::mem::take(&mut e.out));
        blocks.push(if end { u32_of(x) } else { blk });
    }
    // A suspending sub-body's frame, dropped before its first poll,
    // releases what it holds of the exit set, its parameters and captures:
    // the cancel block of its root (rustc's `coroutine_drop`).
    let caps: hd_base::HashSet<_> = e.b.cap_local.iter().map(|l| l.raw()).collect();
    let mut cold = Vec::new();
    for s in super::cold::sub_bodies(e.b, cx)
        .into_iter()
        .map(|s| s as usize)
    {
        let root = e.b.sub_root[s];
        e.syn = e.b.syn.get(root as usize).copied().unwrap_or(NodeIdx::NONE);
        for &(word, _) in exit[s].iter().filter(|&&(_, count)| count == NONE) {
            let l = word & !place::BARE_LOCAL;
            if e.b.local_flags[l as usize] & local_flags::PARAM != 0 || caps.contains(&l) {
                e.release(word, CANCELLED, false)?;
            }
        }
        if !e.out.is_empty() {
            let (first, out) = (
                spans.last().map_or(u32_of(n), |s: &(u32, u32, u32)| s.2),
                std::mem::take(&mut e.out),
            );
            let list = e.b.push_record(&out);
            cold.push((root, e.row(Tag::Block, [list, NONE], Ty::VOID)));
            spans.push((NONE, first, u32_of(e.b.len())));
        }
    }
    let b = e.b;
    b.cancel.extend(cold);
    blocks.sort_stable();
    blocks.dedup();
    for blk in blocks {
        let mut list = Vec::new();
        for &x in b.record(b.data[blk as usize][0]) {
            list.extend(put.get(&(x, false)).into_iter().flatten());
            list.extend(Some(x).filter(|&x| !gone[x as usize]));
        }
        list.extend(put.get(&(blk, true)).into_iter().flatten());
        b.data[blk as usize][0] = b.push_record(&list);
    }
    super::scopes::place(b, n, spans, &gone)?;
    b.cancel.sort_stable();
    Ok(())
}
