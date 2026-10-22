//! Invariant 23 (`docs/compiler/tir-v2.md`, "Invariants" 4): after the
//! ownership passes, ownership balances. The verifier checks it on a body
//! the passes rewrote, with every structural invariant but 22, which holds
//! only before them.
//!
//! Checked here:
//! - every `Read` has a decided mode (`copy`, `move` or `borrow`);
//! - no `Read`, `IsUnique` or `Pin` of a place follows its `move` or its
//!   `Drop` on any path without a `Store` in between that refills it;
//! - no `borrow` value is live across a write to an overlapping place (a
//!   `Store`, a `Drop`, a `move` read, an `IsUnique` or a mutable `Ref`)
//!   or a suspension point;
//! - the exit set's rows are sorted, once each, and name a sub-body and a
//!   local of the body;
//! - nothing tier 2 reserves: no `FlagSet`, `FlagClear` or `regions` row.
//!
//! Not checked yet: that every owned value has exactly one consumer or one
//! `Drop` on each path. Which uses consume a value depends on the callee's
//! parameter modes and on whether a type holds a counted leaf, the facts
//! `OwnCx` gives the passes, so that half lands with insertion and `OwnCx`.
//!
//! The walks follow the body's layout: dense tables indexed by instruction,
//! bit sets over the body's moves and drops, no hash maps.

use super::{
    Body, NONE, Op, Place, PlaceRoot, Proj, ReadMode, Ref, Tag, VerifyError, is_suspension,
    owned_blocks, verify_structure,
};

/// Verifies a body after the ownership passes: the structural invariants
/// and invariant 23 ([the module](self)).
#[must_use]
pub fn verify_owned(b: &Body) -> Vec<VerifyError> {
    let mut errs = verify_structure(b);
    let places: Vec<Option<Place>> = (0..b.len())
        .map(|i| match b.tags[i].operands().0 {
            Op::Place => b.decode_place(b.data[i][0]),
            _ => None,
        })
        .collect();
    let mut addressed = vec![false; b.local_ty.len()];
    for (i, p) in places.iter().enumerate() {
        if b.tags[i] == Tag::Ref
            && let Some(Place {
                root: PlaceRoot::Local(l),
                ..
            }) = p
            && let Some(a) = addressed.get_mut(l.idx())
        {
            *a = true;
        }
    }
    for (l, m) in b.cap_local.iter().zip(&b.cap_mode) {
        if *m == super::CaptureMode::Shared
            && let Some(a) = addressed.get_mut(l.idx())
        {
            *a = true;
        }
    }
    let cx = Cx {
        b,
        places: &places,
        addressed: &addressed,
    };
    decided(b, &mut errs);
    Empties::new(&cx).walk(&mut errs);
    borrow_spans(&cx, &mut errs);
    errs
}

fn err(errs: &mut Vec<VerifyError>, inst: u32, what: String) {
    errs.push(VerifyError {
        invariant: 23,
        inst,
        what,
    });
}

fn u32_of(i: usize) -> u32 {
    u32::try_from(i).expect("insts")
}

/// The modes, the exit set and the tier-2 reservations.
fn decided(b: &Body, errs: &mut Vec<VerifyError>) {
    for (i, (&tag, &[_, w])) in b.tags.iter().zip(&b.data).enumerate() {
        match tag {
            Tag::Read => match ReadMode::from_u32(w) {
                Some(ReadMode::Use) => {
                    err(errs, u32_of(i), "a `Read` left `use` by the passes".into());
                }
                None => err(errs, u32_of(i), format!("a `Read` of unknown mode {w}")),
                Some(_) => {}
            },
            Tag::FlagSet | Tag::FlagClear => {
                err(errs, u32_of(i), format!("`{}` is tier 2's", tag.name()));
            }
            _ => {}
        }
    }
    if !b.regions.is_empty() {
        err(errs, NONE, "lifetime regions are tier 2's".into());
    }
    for (k, &(sub, local)) in b.exit_drops.iter().enumerate() {
        if sub.idx() >= b.sub_root.len() || local.idx() >= b.local_ty.len() {
            err(
                errs,
                NONE,
                format!(
                    "exit drop row {k} names sub-body {} or local {}, which the body lacks",
                    sub.raw(),
                    local.raw()
                ),
            );
        }
        let key = |(s, l): (hd_base::SubId, hd_base::LocalId)| (s.raw(), l.raw());
        if k > 0 && key(b.exit_drops[k - 1]) >= key((sub, local)) {
            err(errs, NONE, format!("exit drop row {k} is out of order"));
        }
    }
}

/// What both walks read: the body, each instruction's decoded place, and
/// the locals a reference or a shared capture can reach.
struct Cx<'a> {
    b: &'a Body,
    places: &'a [Option<Place>],
    addressed: &'a [bool],
}

impl Cx<'_> {
    /// Whether places `p` and `q` may name overlapping storage
    /// (`docs/compiler/tir-v2.md`, "Places"). With one root, one's
    /// projections are a prefix of the other's, element indices ignored.
    /// With two roots, one goes behind a reference or into a list's buffer
    /// and the other is where a reference can reach: behind one too, a
    /// top-level binding, or a local that is addressed or shared with a
    /// closure. A value root alone is a temporary that nothing else
    /// reaches.
    fn overlap(&self, p: &Place, q: &Place) -> bool {
        if same_root(p.root, q.root) {
            return p.proj.iter().zip(&q.proj).all(|(x, y)| match (x, y) {
                (Proj::Field(a), Proj::Field(c)) | (Proj::Variant(a), Proj::Variant(c)) => a == c,
                (Proj::Deref, Proj::Deref) | (Proj::Elem(_), Proj::Elem(_)) => true,
                // Steps of two kinds at one position: assume the worst.
                _ => true,
            });
        }
        let behind = |p: &Place| {
            p.proj
                .iter()
                .any(|s| matches!(s, Proj::Deref | Proj::Elem(_)))
        };
        let reachable = |p: &Place| {
            behind(p)
                || match p.root {
                    PlaceRoot::Global(..) | PlaceRoot::Frame(_) => true,
                    PlaceRoot::Local(l) => self.addressed.get(l.idx()).copied().unwrap_or(true),
                    PlaceRoot::Value(_) => false,
                }
        };
        (behind(p) || behind(q)) && reachable(p) && reachable(q)
    }
}

fn same_root(a: PlaceRoot, c: PlaceRoot) -> bool {
    match (a, c) {
        (PlaceRoot::Global(x, _), PlaceRoot::Global(y, _)) => x == y,
        _ => a == c,
    }
}

/// Whether a `Store` into `q` refills all of the emptied place `p`: one
/// root, and `q`'s projections a prefix of `p`'s, element indices equal.
fn covers(q: &Place, p: &Place) -> bool {
    same_root(q.root, p.root) && q.proj.len() <= p.proj.len() && q.proj == p.proj[..q.proj.len()]
}

/// A bit set over the body's moves and drops.
type Set = Vec<u64>;

fn union(a: Option<Set>, c: Option<Set>) -> Option<Set> {
    match (a, c) {
        (Some(mut x), Some(y)) => {
            for (w, v) in x.iter_mut().zip(y) {
                *w |= v;
            }
            Some(x)
        }
        (x, y) => x.or(y),
    }
}

/// The read-after-move walk: a forward walk over the structured control
/// flow of each sub-body, carrying the set of places a `move` read or a
/// `Drop` emptied. Joins are unions ("on any path"); a loop runs to a fixed
/// point.
struct Empties<'a> {
    cx: &'a Cx<'a>,
    /// Each instruction's number among the moves and drops, or `NONE`.
    id: Vec<u32>,
    /// The instruction of each move or drop.
    at: Vec<u32>,
    /// The label each `Loop` or labeled `Block` instruction is the target
    /// of, or `NONE`.
    label_of: Vec<u32>,
    /// Per label, the union of the states its `Break`s and `Continue`s
    /// carry, during the walk of its target.
    brk: Vec<Option<Set>>,
    cont: Vec<Option<Set>>,
    /// The open matches' arm entry states, innermost last.
    arms: Vec<Vec<Option<Set>>>,
    /// The open cleanup scopes' accumulated emptied places, innermost last.
    scopes: Vec<Set>,
    reported: Vec<bool>,
    errs: Vec<VerifyError>,
}

impl<'a> Empties<'a> {
    fn new(cx: &'a Cx<'a>) -> Self {
        let b = cx.b;
        let mut id = vec![NONE; b.len()];
        let mut at = Vec::new();
        for (i, slot) in id.iter_mut().enumerate() {
            let empties = match b.tags[i] {
                Tag::Read => b.data[i][1] == ReadMode::Move as u32,
                Tag::Drop => true,
                _ => false,
            };
            if empties && cx.places[i].is_some() {
                *slot = u32_of(at.len());
                at.push(u32_of(i));
            }
        }
        let mut label_of = vec![NONE; b.len()];
        for (l, &t) in b.label_inst.iter().enumerate() {
            if let Some(x) = label_of.get_mut(t as usize) {
                *x = u32_of(l);
            }
        }
        Empties {
            cx,
            id,
            at,
            label_of,
            brk: vec![None; b.label_inst.len()],
            cont: vec![None; b.label_inst.len()],
            arms: Vec::new(),
            scopes: Vec::new(),
            reported: vec![false; b.len()],
            errs: Vec::new(),
        }
    }

    fn walk(mut self, errs: &mut Vec<VerifyError>) {
        if self.at.is_empty() {
            return;
        }
        let words = self.at.len().div_ceil(64);
        for &root in &self.cx.b.sub_root {
            self.block(root, vec![0; words]);
        }
        errs.append(&mut self.errs);
    }

    fn is_block(&self, x: u32) -> bool {
        self.cx.b.tags.get(x as usize) == Some(&Tag::Block)
    }

    /// Walks block `blk` from `s`; `None` when every path leaves it by a
    /// jump.
    fn block(&mut self, blk: u32, s: Set) -> Option<Set> {
        if !self.is_block(blk) {
            return Some(s);
        }
        let b = self.cx.b;
        let mut cur = Some(s);
        for &x in b.record(b.data[blk as usize][0]) {
            let Some(st) = cur else {
                break;
            };
            // A block in the list runs where the instruction that owns it
            // runs it, as emission does; a closure's root block is walked as
            // its own sub-body.
            cur = if b.tags[x as usize] == Tag::Block {
                Some(st)
            } else {
                self.inst(x, st)
            };
        }
        let label = self.label_of[blk as usize];
        if label != NONE {
            cur = union(cur, self.brk[label as usize].take());
        }
        cur
    }

    fn inst(&mut self, x: u32, mut s: Set) -> Option<Set> {
        let b = self.cx.b;
        let [a, w] = b.data[x as usize];
        match b.tags[x as usize] {
            Tag::Read | Tag::IsUnique | Tag::Pin => {
                self.check(x, &s);
                self.empty(x, &mut s);
            }
            Tag::Drop => self.empty(x, &mut s),
            Tag::Store => self.refill(x, &mut s),
            Tag::If => {
                let r = b.record(w);
                let (then, els) = (r.first().copied(), r.get(1).copied());
                let t = then.map_or(Some(s.clone()), |t| self.block(t, s.clone()));
                let e = els.map_or(Some(s.clone()), |e| self.block(e, s));
                return union(t, e);
            }
            Tag::And | Tag::Or => {
                let right = self.block(w, s.clone());
                return union(Some(s), right);
            }
            Tag::With => return self.block(w, s),
            Tag::RefDyn => return self.block(a, s),
            Tag::Loop => return self.repeat(x, s),
            Tag::Match => return self.decide(x, s),
            Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar | Tag::SwitchStr => {
                let r = b.record(w).to_vec();
                let (cases, default) = r.split_at(r.len().saturating_sub(1));
                let mut out = None;
                for &blk in cases.iter().skip(1).step_by(2).chain(default) {
                    if self.is_block(blk) {
                        out = union(out, self.block(blk, s.clone()));
                    }
                }
                return out;
            }
            Tag::Guard => {
                let s = self.block(a, s)?;
                let r = b.record(w).to_vec();
                let mut out = None;
                for &blk in &r {
                    out = union(out, self.block(blk, s.clone()));
                }
                return out;
            }
            Tag::Scope => return self.scope(x, s),
            Tag::ToArm => {
                if let Some(arm) = self
                    .arms
                    .last_mut()
                    .and_then(|arms| arms.get_mut(a as usize))
                {
                    *arm = union(arm.take(), Some(s));
                }
                return None;
            }
            Tag::Break => {
                if let Some(t) = self.brk.get_mut(a as usize) {
                    *t = union(t.take(), Some(s));
                }
                return None;
            }
            Tag::Continue => {
                if let Some(t) = self.cont.get_mut(a as usize) {
                    *t = union(t.take(), Some(s));
                }
                return None;
            }
            Tag::Return | Tag::Unreachable => return None,
            _ => {}
        }
        Some(s)
    }

    /// A loop: its body from the head state until the head stops growing
    /// (states only grow, so this ends); its exits are its `Break`s and,
    /// when it has one, its else block from the final head.
    fn repeat(&mut self, x: u32, s: Set) -> Option<Set> {
        let b = self.cx.b;
        let [body, els] = b.data[x as usize];
        let label = self.label_of[x as usize] as usize;
        let mut head = s;
        for _ in 0..=self.at.len() + 1 {
            if let Some(t) = self.brk.get_mut(label) {
                *t = None;
            }
            if let Some(t) = self.cont.get_mut(label) {
                *t = None;
            }
            let end = self.block(body, head.clone());
            let back = self.cont.get_mut(label).and_then(Option::take);
            let next = union(union(Some(head.clone()), end), back).expect("the head");
            if next == head {
                break;
            }
            head = next;
        }
        let exits = self.brk.get_mut(label).and_then(Option::take);
        if els != NONE && self.is_block(els) {
            return union(exits, self.block(els, head));
        }
        exits
    }

    /// A match: its decision tree from `s`, each arm from the union of the
    /// states its `ToArm`s carry.
    fn decide(&mut self, x: u32, s: Set) -> Option<Set> {
        let b = self.cx.b;
        let r = b.record(b.data[x as usize][1]).to_vec();
        let Some((&decision, arms)) = r.split_first() else {
            return Some(s);
        };
        self.arms.push(vec![None; arms.len()]);
        // Every path of the tree ends in `ToArm` or `Unreachable`.
        let _ = self.block(decision, s);
        let entries = self.arms.pop().unwrap_or_default();
        let mut out = None;
        for (&arm, entry) in arms.iter().zip(entries) {
            if let Some(e) = entry {
                out = union(out, self.block(arm, e));
            }
        }
        out
    }

    /// A cleanup scope: its body, then each suite from every place the body
    /// emptied anywhere, since a suite runs at whichever exit the body
    /// takes.
    fn scope(&mut self, x: u32, s: Set) -> Option<Set> {
        let b = self.cx.b;
        let [body, suites] = b.data[x as usize];
        self.scopes.push(s.clone());
        let after = self.block(body, s);
        let mut seen = self.scopes.pop().unwrap_or_default();
        if let Some(a) = &after {
            for (w, v) in seen.iter_mut().zip(a) {
                *w |= v;
            }
        }
        let mut out = after;
        if suites != NONE {
            for &suite in &b.record(suites).to_vec() {
                let end = self.block(suite, seen.clone());
                if out.is_some() {
                    out = union(out, end);
                }
            }
        }
        out
    }

    /// Instruction `x` empties its place: a `move` read or a `Drop`.
    fn empty(&mut self, x: u32, s: &mut Set) {
        let k = self.id[x as usize];
        if k == NONE {
            return;
        }
        let (w, bit) = ((k / 64) as usize, 1u64 << (k % 64));
        s[w] |= bit;
        for acc in &mut self.scopes {
            acc[w] |= bit;
        }
    }

    /// A `Store` refills every emptied place it covers.
    fn refill(&self, x: u32, s: &mut Set) {
        let Some(q) = &self.cx.places[x as usize] else {
            return;
        };
        for (k, &e) in self.at.iter().enumerate() {
            let (w, bit) = (k / 64, 1u64 << (k % 64));
            if s[w] & bit != 0
                && let Some(p) = &self.cx.places[e as usize]
                && covers(q, p)
            {
                s[w] &= !bit;
            }
        }
    }

    /// A read of `x`'s place must not overlap a place emptied before it.
    fn check(&mut self, x: u32, s: &Set) {
        let Some(q) = &self.cx.places[x as usize] else {
            return;
        };
        if self.reported[x as usize] {
            return;
        }
        for (k, &e) in self.at.iter().enumerate() {
            if s[k / 64] & (1u64 << (k % 64)) != 0
                && let Some(p) = &self.cx.places[e as usize]
                && self.cx.overlap(p, q)
            {
                self.reported[x as usize] = true;
                let what = format!(
                    "`{}` of {q} after `{}` of {p} at %{e} empties it",
                    self.cx.b.tags[x as usize].name(),
                    self.cx.b.tags[e as usize].name()
                );
                err(&mut self.errs, x, what);
                return;
            }
        }
    }
}

/// The value operands instruction `i` reads, for liveness: the schema's
/// value words and value records, a place's values, a call's providers,
/// and `With`'s and `ContextNew`'s provider values.
fn uses(b: &Body, i: usize, out: &mut Vec<u32>) {
    out.clear();
    let [a, w] = b.data[i];
    let tag = b.tags[i];
    if let Some(v) = b.value_operands(i) {
        out.extend(v);
        // `[..., kind, x, n]`: key and value pairs at `x` (1), or a
        // context value (2).
        if tag == Tag::Call
            && let [.., kind, x, n] = *b.record(w)
        {
            match kind {
                1 => out.extend(b.extra.iter().skip(x as usize).take(n as usize)),
                2 => out.push(x),
                _ => {}
            }
        }
    } else if matches!(tag, Tag::With | Tag::ContextNew) {
        out.extend(b.record(a).iter().skip(1).step_by(2));
    } else {
        for (word, op) in [(a, tag.operands().0), (w, tag.operands().1)] {
            match op {
                Op::Value => out.push(word),
                Op::Values if word != NONE => out.extend_from_slice(b.record(word)),
                Op::Place => out.extend(b.place_values(word)),
                Op::Record
                    if word != NONE && matches!(tag, Tag::CallDyn | Tag::Await | Tag::NewList) =>
                {
                    out.extend_from_slice(b.record(word));
                }
                _ => {}
            }
        }
    }
    out.retain(|&r| r != NONE && Ref(r).as_inst().is_some() && (r as usize) < b.len());
}

/// The borrow walk. A `borrow` read's span runs from the read to the last
/// instruction its value reaches: a use, or the uses of a value that
/// carries it on (a block's tail and the instruction that owns the block,
/// a coercion, a `Break` value and its target), and all of a loop that
/// holds a use but not the read. No write to an overlapping place and no
/// suspension point may stand in the span. Instructions are in evaluation
/// order within a block, and a block's instructions precede the
/// instruction that runs it, so the span is a range of instructions; a
/// range over branches the value never takes is a stricter check.
fn borrow_spans(cx: &Cx<'_>, errs: &mut Vec<VerifyError>) {
    let b = cx.b;
    let n = b.len();
    let borrows = (0..n).any(|i| b.tags[i] == Tag::Read && b.data[i][1] == ReadMode::Borrow as u32);
    if !borrows {
        return;
    }
    // Each instruction's parent: the block that lists it, or the
    // instruction that owns it as a block operand; and the innermost loop
    // whose body holds it, with that loop's depth.
    let mut parent = vec![NONE; n];
    for i in 0..n {
        if b.tags[i] == Tag::Block {
            for &c in b.record(b.data[i][0]) {
                if let Some(p) = parent.get_mut(c as usize) {
                    *p = u32_of(i);
                }
            }
        }
        for blk in owned_blocks(b, i) {
            if let Some(p) = parent.get_mut(blk as usize) {
                *p = u32_of(i);
            }
        }
    }
    for &r in &b.sub_root {
        if let Some(p) = parent.get_mut(r as usize) {
            *p = NONE;
        }
    }
    let mut inner = vec![NONE; n];
    let mut depth = vec![0u32; n];
    for i in (0..n).rev() {
        let p = parent[i];
        if p == NONE || p as usize >= n {
            continue;
        }
        // A loop's body repeats; its else block runs once.
        if b.tags[p as usize] == Tag::Loop && b.data[p as usize][0] == u32_of(i) {
            inner[i] = p;
            depth[i] = depth[p as usize] + 1;
        } else {
            inner[i] = inner[p as usize];
            depth[i] = depth[p as usize];
        }
    }
    // Uses in compressed rows: `users[start[v]..start[v + 1]]` read `v`.
    let mut buf = Vec::new();
    let mut count = vec![0u32; n + 1];
    for i in 0..n {
        uses(b, i, &mut buf);
        for &v in &buf {
            count[v as usize] += 1;
        }
    }
    let mut start = vec![0u32; n + 1];
    for v in 0..n {
        start[v + 1] = start[v] + count[v];
    }
    let mut fill = start.clone();
    let mut users = vec![0u32; start[n] as usize];
    for i in 0..n {
        uses(b, i, &mut buf);
        for &v in &buf {
            users[fill[v as usize] as usize] = u32_of(i);
            fill[v as usize] += 1;
        }
    }
    // What carries a value on: a block's owner, a `Break`'s target.
    let mut carrier = vec![NONE; n];
    for (i, c) in carrier.iter_mut().enumerate() {
        if b.tags[i] == Tag::Block && parent[i] != NONE && b.tags[parent[i] as usize] != Tag::Block
        {
            *c = parent[i];
        }
        if b.tags[i] == Tag::Break && b.data[i][1] != NONE {
            *c = b
                .label_inst
                .get(b.data[i][0] as usize)
                .copied()
                .unwrap_or(NONE);
        }
    }
    let forwards = |u: usize, v: u32| match b.tags[u] {
        Tag::Block => b.data[u][1] == v,
        Tag::Coerce => b.data[u][0] == v,
        Tag::Break => b.data[u][1] == v,
        _ => false,
    };
    // The outermost loop around `u` that does not hold `v`'s definition:
    // the loops around a visible value's definition are the outer ones of
    // those around its use, and a loop instruction's depth counts the
    // loops around it.
    let around = |u: usize, v: usize| {
        if depth[u] <= depth[v] {
            return NONE;
        }
        let mut l = inner[u];
        while l != NONE && depth[l as usize] > depth[v] {
            l = inner[l as usize];
        }
        l
    };
    let later = |a: u32, c: u32| {
        if a == NONE || (c != NONE && c > a) {
            c
        } else {
            a
        }
    };
    // The last instruction each value reaches, from the last value back.
    let mut last = vec![NONE; n];
    for v in (0..n).rev() {
        let mut end = NONE;
        for &u in &users[start[v] as usize..start[v + 1] as usize] {
            end = later(later(end, u), around(u as usize, v));
            if forwards(u as usize, u32_of(v)) {
                let c = carrier[u as usize];
                end = later(end, last[u as usize]);
                if c != NONE {
                    end = later(later(end, c), last[c as usize]);
                }
            }
        }
        last[v] = end;
    }
    let subs = b.inst_subs();
    for r in 0..n {
        if b.tags[r] != Tag::Read || b.data[r][1] != ReadMode::Borrow as u32 {
            continue;
        }
        let (Some(p), end) = (&cx.places[r], last[r]) else {
            continue;
        };
        if end == NONE {
            continue;
        }
        for j in r + 1..=end as usize {
            if subs[j] != subs[r] {
                continue;
            }
            let tag = b.tags[j];
            let writes = match tag {
                Tag::Store | Tag::Drop | Tag::IsUnique => true,
                Tag::Read => b.data[j][1] == ReadMode::Move as u32,
                Tag::Ref => b.data[j][1] == 1,
                _ => false,
            };
            let hit = if writes {
                cx.places[j].as_ref().is_some_and(|q| cx.overlap(p, q))
            } else {
                is_suspension(tag)
            };
            if hit {
                err(
                    errs,
                    u32_of(r),
                    format!("a borrow of {p} lives across `{}` at %{j}", tag.name()),
                );
                break;
            }
        }
    }
}
