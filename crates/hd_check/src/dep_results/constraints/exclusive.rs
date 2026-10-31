//! Exclusivity (`ref.excl`, stages 7a to 7c2 of
//! `docs/future-work/RETURNED_REFERENCES_PATH.md`). Each written `Ref` of
//! a place a local's path reaches is a loan of its own, [`Origin::Site`],
//! which the solver carries with the local's: shared for `&`, exclusive
//! for `&mut`. So is each reference a call takes for a receiver or an
//! argument, which reaches the call's result when the result depends on
//! it. A write to a place that overlaps a site's, or any use of it for an
//! exclusive site, is `conflicting-access` when a binding that holds the
//! site is read after it on some path. The order comes from
//! the block structure: an instruction runs after the earlier ones of its
//! block's statements, and a loop's body runs again, so a read earlier in
//! a loop follows a write later in it when the binding outlives one
//! iteration.

use hd_base::NodeIdx;
use hd_tir::ir::{
    Body, IntrinsicOp, NONE, PlaceRoot, Proj, Tag, Topology, Word, local_flags, slots,
};
use hd_types::Ty;

use hd_syntax::SyntaxKind;

use super::{Gen, Inputs, Origin, Region, Solution, is_mut, w};

/// A place as a local and a path from it: fields, variants,
/// dereferences, and elements, which overlap any other step.
type Path = (u32, Vec<Proj>);

/// The path of place `word`, through the reads its value root is made of.
fn path_of(b: &Body, word: u32) -> Option<Path> {
    let p = b.decode_place(word)?;
    match p.root {
        PlaceRoot::Local(l) => Some((l.raw(), p.proj)),
        PlaceRoot::Value(v) => {
            let mut v = v.0;
            while b.ty.get(v as usize).is_some() && b.tags[v as usize] == Tag::Coerce {
                v = b.data[v as usize][0];
            }
            if b.ty.get(v as usize).is_none() || b.tags[v as usize] != Tag::Read {
                return None;
            }
            let (l, mut path) = path_of(b, b.data[v as usize][0])?;
            path.extend(p.proj);
            Some((l, path))
        }
        _ => None,
    }
}

/// The place loan `i` covers, without an element, which the pin covers
/// (`ref.elem.access.pin`), and whether the loan is exclusive: a `Ref`'s
/// place, or what a `&mut` binding refers to where a call reads it as `&`
/// or as `&mut` (`ref.excl.call`).
fn loan_path(s: &Inputs<'_>, i: u32) -> Option<(Path, bool)> {
    let (b, i) = (s.b, i as usize);
    let excl = is_mut(s.pool, b.ty[i]);
    let path = match b.tags[i] {
        Tag::Ref => path_of(b, b.data[i][0]),
        Tag::Read if excl => path_of(b, b.data[i][0]).map(|(l, mut p)| {
            p.push(Proj::Deref);
            (l, p)
        }),
        Tag::Coerce if b.same_value_coercion(i) && !excl => {
            let v = b.data[i][0] as usize;
            if b.ty.get(v).is_none() || b.tags[v] != Tag::Read || !is_mut(s.pool, b.ty[v]) {
                return None;
            }
            path_of(b, b.data[v][0]).map(|(l, mut p)| {
                p.push(Proj::Deref);
                (l, p)
            })
        }
        _ => None,
    }?;
    (!path.1.iter().any(|s| matches!(s, Proj::Elem(_)))).then_some((path, excl))
}

/// Whether `Ref` `i` is a written `&` or `&mut` that takes a site loan
/// (`ref.excl.place`, `ref.excl.mut`).
pub(super) fn site(s: &Inputs<'_>, i: u32) -> bool {
    let at = s.b.syn[i as usize];
    at != NodeIdx::NONE
        && s.cx.src.parse.tree.node(at).kind() == SyntaxKind::RefExpr
        && s.b.tags[i as usize] == Tag::Ref
        && loan_path(s, i).is_some()
}

/// Whether two paths of one local overlap: no step of the one is a field
/// other than the other's at the same depth (`ref.excl.place`).
fn overlap(a: &[Proj], b: &[Proj]) -> bool {
    !a.iter()
        .zip(b)
        .any(|p| matches!(p, (Proj::Field(x), Proj::Field(y)) if x != y))
}

impl Gen<'_, '_> {
    /// The writes of the body: each instruction that writes a place
    /// (`ref.excl.write`), the place, and where the write happens. A
    /// `&mut` a call takes writes at the call, a bound one where it is
    /// taken.
    fn writes(&self, users: &[u32]) -> Vec<(u32, Path, u32)> {
        let (b, pool) = (self.s.b, self.s.pool);
        let mut out = Vec::new();
        for (i, &u) in users.iter().enumerate() {
            let [a, d] = b.data[i];
            let (u, calls) = (u as usize, |t: Option<&Tag>| {
                matches!(
                    t,
                    Some(Tag::Call | Tag::CallDyn | Tag::CallValue | Tag::Await)
                )
            });
            // The place, where it is written, and whether through a `&mut`.
            let (word, at, through) = match b.tags[i] {
                Tag::Store if !self.declares(i) => (a, i, false),
                // A `&mut` a call takes or a binding keeps, or a `&mut`
                // binding a call reborrows.
                Tag::Ref if is_mut(pool, b.ty[i]) => {
                    (a, if calls(b.tags.get(u)) { u } else { i }, false)
                }
                Tag::Read if is_mut(pool, b.ty[i]) && calls(b.tags.get(u)) => (a, u, true),
                Tag::Intrinsic
                    if matches!(
                        IntrinsicOp::from_u32(a),
                        Some(
                            IntrinsicOp::ListPush
                                | IntrinsicOp::ListReserve
                                | IntrinsicOp::MapSet
                                | IntrinsicOp::MapRemove
                        )
                    ) =>
                {
                    match b.record(d).first() {
                        Some(&c) if b.tags.get(c as usize) == Some(&Tag::Read) => {
                            (b.data[c as usize][0], i, false)
                        }
                        _ => continue,
                    }
                }
                _ => continue,
            };
            if let Some((l, mut p)) = path_of(b, word) {
                if through {
                    p.push(Proj::Deref);
                }
                out.push((w(i), (l, p), w(at)));
            }
        }
        out
    }

    /// Whether store `i` declares the local it stores: a new place, which
    /// no reference points into yet, as in a loop's next iteration.
    fn declares(&self, i: usize) -> bool {
        let b = self.s.b;
        let (Some(p), at) = (b.decode_place(b.data[i][0]), b.syn[i]) else {
            return false;
        };
        let PlaceRoot::Local(l) = p.root else {
            return false;
        };
        let decl = b.local_syn[l.idx()];
        if !p.proj.is_empty() || at == NodeIdx::NONE || decl == NodeIdx::NONE {
            return false;
        }
        let (lo, hi) = super::super::range(self.s.cx.src.parse.tree.node(at));
        lo <= decl.raw() && decl.raw() < hi
    }

    /// The instructions that read each local: a read, a reference or a
    /// store through it, and a closure that captures it; and the stores
    /// of each whole local.
    fn reads(&self) -> (Vec<Vec<u32>>, Vec<Vec<u32>>) {
        let b = self.s.b;
        let mut out = vec![Vec::new(); self.locals as usize];
        let mut sets = vec![Vec::new(); self.locals as usize];
        for i in 0..b.tags.len() {
            let tag = b.tags[i];
            if tag == Tag::Store
                && let Some(p) = b.decode_place(b.data[i][0])
                && let (PlaceRoot::Local(l), true) = (p.root, p.proj.is_empty())
            {
                sets[l.idx()].push(w(i));
            }
            if let Tag::Read | Tag::Ref | Tag::Store = tag
                && let Some(p) = b.decode_place(b.data[i][0])
                && let PlaceRoot::Local(l) = p.root
                && (tag != Tag::Store || p.proj.first() == Some(&Proj::Deref))
            {
                out[l.idx()].push(w(i));
            }
            if tag == Tag::Closure
                && !b.is_instantiation(i)
                && let [start, len, ..] = *b.record(b.data[i][1])
            {
                for c in start..start.saturating_add(len) {
                    if let Some(l) = b.cap_local.get(c as usize) {
                        out[l.idx()].push(w(i));
                    }
                }
            }
        }
        (out, sets)
    }

    /// The uses of a place for an exclusive loan (`ref.excl.mut`): each
    /// read, reference and store of it but a declaring one, and each
    /// closure that captures its local. A read that only roots a longer
    /// place is that place's use.
    fn accesses(&self) -> Vec<(u32, Path)> {
        let b = self.s.b;
        let mut base = vec![false; b.tags.len()];
        for i in 0..b.tags.len() {
            if let Tag::Read | Tag::Ref | Tag::Store = b.tags[i]
                && let Some(p) = b.decode_place(b.data[i][0])
                && let PlaceRoot::Value(v) = p.root
            {
                let mut v = v.0 as usize;
                while v < b.tags.len() && b.tags[v] == Tag::Coerce {
                    base[v] = true;
                    v = b.data[v][0] as usize;
                }
                if let Some(x) = base.get_mut(v) {
                    *x = true;
                }
            }
        }
        let mut out = Vec::new();
        for i in (0..b.tags.len()).filter(|&i| !base[i]) {
            match b.tags[i] {
                Tag::Read | Tag::Ref => {}
                Tag::Store if !self.declares(i) => {}
                Tag::Closure if !b.is_instantiation(i) => {
                    if let [start, len, ..] = *b.record(b.data[i][1]) {
                        for c in start..start.saturating_add(len) {
                            if let Some(l) = b.cap_local.get(c as usize) {
                                out.push((w(i), (l.raw(), Vec::new())));
                            }
                        }
                    }
                    continue;
                }
                _ => continue,
            }
            if let Some(p) = path_of(b, b.data[i][0]) {
                out.push((w(i), p));
            }
        }
        out
    }

    /// `ref.excl.call`: each reference a call takes for a receiver
    /// or an argument is a site loan, which the call's result holds when
    /// it depends on that argument.
    pub(super) fn call_loans(&mut self) {
        let b = self.s.b;
        for c in 0..b.tags.len() {
            if !matches!(
                b.tags[c],
                Tag::Call | Tag::CallDyn | Tag::CallValue | Tag::Await
            ) {
                continue;
            }
            let mut args = Vec::new();
            slots(b, c, |_, v, word| {
                if matches!(word, Word::Value(_)) && (v as usize) < c {
                    args.push(v);
                }
            });
            for v in args {
                if loan_path(self.s, v).is_some()
                    && let Some(&top) = self.val(v).as_ref().and_then(|x| x.0.first())
                {
                    self.seed(top, Origin::Site(v));
                }
            }
        }
    }

    /// `ref.excl.shared`, `ref.excl.mut`: each write to a place that
    /// overlaps a site's, or each use of one for an exclusive site, while
    /// a binding that holds the site is still read.
    pub(super) fn conflicts(&self, sol: &Solution) -> Vec<(NodeIdx, String)> {
        let b = self.s.b;
        if !self.loans.iter().any(|o| matches!(o, Origin::Site(_))) {
            return Vec::new();
        }
        let has = |v: &[Region], loan: u32| v.iter().any(|&g| sol.value(g).contains(loan));
        let (topo, (reads, sets)) = (Topology::new(b), self.reads());
        // Each site: its `Ref`, its place, and the bindings that hold it,
        // each with its stores of a value that does not.
        let mut sites = Vec::new();
        for (loan, o) in self.loans.iter().enumerate() {
            let Origin::Site(i) = *o else { continue };
            let Some((place, excl)) = loan_path(self.s, i) else {
                continue;
            };
            let holders: Vec<(u32, Vec<u32>)> = (0..self.locals)
                .filter(|&h| h != place.0 && has(&self.local(h).0, w(loan)))
                .map(|h| {
                    let other = (sets[h as usize].iter().copied())
                        .filter(|&s| {
                            !self
                                .val(b.data[s as usize][1])
                                .is_some_and(|v| has(&v.0, w(loan)))
                        })
                        .collect();
                    (h, other)
                })
                .collect();
            sites.push((i, place, excl, holders));
        }
        if sites.iter().all(|s| s.3.is_empty()) {
            return Vec::new();
        }
        let mut users = vec![NONE; b.tags.len()];
        for j in 0..b.tags.len() {
            slots(b, j, |_, v, word| {
                if matches!(word, Word::Value(_)) && (v as usize) < j && users[v as usize] == NONE {
                    users[v as usize] = w(j);
                }
            });
        }
        let order = Order {
            b,
            t: &topo,
            cx: self,
        };
        let mut out: Vec<(NodeIdx, String)> = Vec::new();
        let writes = self
            .writes(&users)
            .into_iter()
            .map(|(i, p, at)| (i, p, at, true));
        let touches = self.accesses().into_iter().map(|(i, p)| (i, p, i, false));
        // One error for a node, or a node inside or around it.
        let range = |n: NodeIdx| super::super::range(self.s.cx.src.parse.tree.node(n));
        let nests = |(a, b): (u32, u32), (c, d): (u32, u32)| a <= c && d <= b || c <= a && b <= d;
        for (wi, (root, path), at, write) in writes.chain(touches) {
            let node = b.syn[wi as usize];
            if node == NodeIdx::NONE || out.iter().any(|x| nests(range(x.0), range(node))) {
                continue;
            }
            'site: for (i, (r, sp), excl, holders) in &sites {
                if *r != root || !overlap(sp, &path) || *i == wi || !write && !excl {
                    continue;
                }
                for (h, other) in holders {
                    let (i, h) = (*i, *h);
                    if !order.hop(i, at, h) || order.kills(other, i, at) {
                        continue;
                    }
                    let live =
                        |u: u32| order.hop(at, u, h) && !order.kills(&sets[h as usize], at, u);
                    if reads[h as usize].iter().any(|&u| live(u)) {
                        out.push((node, self.conflict_message(root, h, *excl)));
                        break 'site;
                    }
                }
            }
        }
        out
    }

    /// `ref.excl.args`: a `&mut` a call takes against the call's other
    /// arguments and receiver. One that holds a loan of an overlapping
    /// place conflicts, and so does a use of one after a written `&mut`.
    /// A call that holds an error of `found` already gets no other.
    pub(super) fn call_conflicts(
        &self,
        sol: &Solution,
        found: &[(NodeIdx, String)],
    ) -> Vec<(NodeIdx, String)> {
        let b = self.s.b;
        let range = |n: NodeIdx| super::super::range(self.s.cx.src.parse.tree.node(n));
        // Each site loan: its id, its `Ref`, and its place.
        let sites: Vec<(u32, u32, Path)> = (self.loans.iter().enumerate())
            .filter_map(|(loan, o)| match *o {
                Origin::Site(i) => loan_path(self.s, i).map(|(p, _)| (w(loan), i, p)),
                _ => None,
            })
            .collect();
        let mut uses = None;
        let mut out: Vec<(NodeIdx, String)> = Vec::new();
        for c in 0..b.tags.len() {
            if !matches!(
                b.tags[c],
                Tag::Call | Tag::CallDyn | Tag::CallValue | Tag::Await
            ) || b.syn[c] == NodeIdx::NONE
            {
                continue;
            }
            let (lo, hi) = range(b.syn[c]);
            if found.iter().any(|x| (lo..hi).contains(&range(x.0).0)) {
                continue;
            }
            let mut args = Vec::new();
            slots(b, c, |_, v, word| {
                if matches!(word, Word::Value(_)) && (v as usize) < c {
                    args.push(v);
                }
            });
            for &v in &args {
                let Some(((root, path), true)) = loan_path(self.s, v) else {
                    continue;
                };
                let hit = |q: &Path| q.0 == root && overlap(&path, &q.1);
                let mut hits = Vec::new();
                for &u in args.iter().filter(|&&u| u != v) {
                    let held = self.val(u).map(|x| x.0).unwrap_or_default();
                    if sites.iter().any(|(loan, i, q)| {
                        *i != v && hit(q) && held.iter().any(|&g| sol.value(g).contains(*loan))
                    }) {
                        hits.push(u);
                    }
                }
                if site(self.s, v) {
                    let uses = uses.get_or_insert_with(|| self.accesses());
                    // An element reference keeps `ref.elem.conflict`.
                    let elem = |j: u32, q: &Path| {
                        b.tags[j as usize] == Tag::Ref
                            && q.1.iter().any(|s| matches!(s, Proj::Elem(_)))
                    };
                    let after = |j: u32, q: &Path| v < j && (j as usize) < c && !elem(j, q);
                    hits.extend(
                        uses.iter()
                            .filter(|(j, q)| after(*j, q) && hit(q))
                            .map(|x| x.0),
                    );
                }
                // One error for a call.
                let node = hits
                    .iter()
                    .map(|&u| b.syn[u as usize])
                    .find(|&n| n != NodeIdx::NONE);
                if let Some(node) = node.filter(|n| !out.iter().any(|x| x.0 == *n)) {
                    let n = self.s.cx.names.text(b.local_name[root as usize]);
                    let place = if n.starts_with('$') {
                        "a temporary".to_owned()
                    } else {
                        format!("`{n}`")
                    };
                    out.push((node, format!(
                        "this uses {place} while the same call takes `&mut` of it; copy what this argument needs into a local before the call"
                    )));
                    break;
                }
            }
        }
        out
    }

    fn conflict_message(&self, root: u32, holder: u32, excl: bool) -> String {
        let (b, names) = (self.s.b, &self.s.cx.names);
        let name = |l: u32| {
            let n = names.text(b.local_name[l as usize]);
            (!n.starts_with('$')).then(|| format!("`{n}`"))
        };
        let place = name(root).unwrap_or_else(|| "a temporary".to_owned());
        match (name(holder), excl) {
            (Some(h), true) => format!(
                "this uses {place} while {h} refers to it as `&mut` and is used later; use {place} after {h}'s last use, or through {h}"
            ),
            (None, true) => format!(
                "this uses {place} while a `&mut` reference to it is used later; use {place} after that use"
            ),
            (Some(h), false) => format!(
                "this changes {place} while {h} refers to it and is used later; make the change after {h}'s last use"
            ),
            (None, false) => format!(
                "this changes {place} while a reference to it is used later; make the change after that use"
            ),
        }
    }
}

/// The order of a body's instructions by its blocks.
struct Order<'x, 's, 'a> {
    b: &'x Body,
    t: &'x Topology,
    cx: &'x Gen<'s, 'a>,
}

impl Order<'_, '_, '_> {
    /// The instruction that holds `x`: the one that runs its block, else
    /// the block that lists it.
    fn up(&self, x: u32) -> Option<u32> {
        let (t, i) = (self.t, x as usize);
        if i >= t.blk.len() || t.is_root[i] {
            return None;
        }
        let p = if t.owner[i] == NONE {
            t.blk[i]
        } else {
            t.owner[i]
        };
        (p != NONE).then_some(p)
    }

    fn chain(&self, x: u32) -> Vec<u32> {
        std::iter::successors(Some(x), |&y| self.up(y))
            .take(self.t.blk.len())
            .collect()
    }

    /// Whether `y` may run after `x` along a path through local `h`'s
    /// lifetime: later in the block structure, or in a later iteration of
    /// a loop that holds both and that `h` was declared outside.
    fn hop(&self, x: u32, y: u32, h: u32) -> bool {
        let (cx, cy) = (self.chain(x), self.chain(y));
        let Some(k) = cy.iter().position(|a| cx.contains(a)) else {
            return x < y;
        };
        let lca = cy[k];
        let again = cy[k..]
            .iter()
            .any(|&l| self.b.tags[l as usize] == Tag::Loop && self.outside(h, l));
        again || x < y && !self.apart(&cx, &cy, k, lca) && !self.ends(&cx, lca)
    }

    /// Whether one of the stores `sets` of a binding runs on every path
    /// from `x` forward to `y`: a statement between theirs in a block that
    /// holds both. What the binding holds after it, it read after `x`.
    fn kills(&self, sets: &[u32], x: u32, y: u32) -> bool {
        let (cx, cy) = (self.chain(x), self.chain(y));
        let at = |c: &[u32], blk: u32| {
            let k = c.iter().position(|&a| a == blk)?;
            k.checked_sub(1).map(|j| self.t.pos[c[j] as usize])
        };
        x < y && sets.iter().any(|&s| {
            let (blk, pos) = (self.t.blk[s as usize], self.t.pos[s as usize]);
            blk != NONE
                && matches!((at(&cx, blk), at(&cy, blk)), (Some(a), Some(b)) if a < pos && pos < b)
        })
    }

    /// Whether `x` and `y` lie in two arms of a branch.
    fn apart(&self, cx: &[u32], cy: &[u32], k: usize, lca: u32) -> bool {
        let Some(j) = cx.iter().position(|&a| a == lca) else {
            return false;
        };
        if j == 0 || k == 0 {
            return false;
        }
        let (bx, by) = (cx[j - 1], cy[k - 1]);
        let b = self.b;
        match b.tags[lca as usize] {
            Tag::If | Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar | Tag::SwitchStr => {
                bx != by
            }
            // A match's first block decides, and its arms follow it.
            Tag::Match => {
                let first = b.record(b.data[lca as usize][1]).first().copied();
                bx != by && Some(bx) != first && Some(by) != first
            }
            _ => false,
        }
    }

    /// Whether a statement after `x`'s, in a block between `x` and `lca`,
    /// never finishes and leaves no way out of that block forward: a
    /// `return`, a `continue`, a call that never returns.
    fn ends(&self, cx: &[u32], lca: u32) -> bool {
        let b = self.b;
        cx.windows(2)
            .take_while(|p| p[0] != lca && p[1] != lca)
            .any(|p| {
                let (c, blk) = (p[0], p[1] as usize);
                b.tags[blk] == Tag::Block
                    && self.t.blk[c as usize] == p[1]
                    && (b.record(b.data[blk][0]).iter())
                        .skip(self.t.pos[c as usize] as usize + 1)
                        .any(|&s| {
                            let tag = b.tags.get(s as usize);
                            matches!(
                                tag,
                                Some(Tag::Return | Tag::Continue | Tag::Call | Tag::CallDyn)
                            ) && b.ty[s as usize] == Ty::NEVER
                        })
            })
    }

    /// Whether local `h` was declared outside loop `l`, so that it keeps
    /// what an iteration put in it.
    fn outside(&self, h: u32, l: u32) -> bool {
        let b = self.b;
        let decl = b.local_syn[h as usize];
        if b.local_flags[h as usize] & local_flags::PARAM != 0 || decl == NodeIdx::NONE {
            return true;
        }
        let at = b.syn[l as usize];
        if at == NodeIdx::NONE {
            return true;
        }
        let (lo, hi) = super::super::range(self.cx.s.cx.src.parse.tree.node(at));
        !(lo <= decl.raw() && decl.raw() < hi)
    }
}
