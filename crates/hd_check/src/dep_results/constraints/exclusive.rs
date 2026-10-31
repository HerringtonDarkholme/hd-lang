//! Shared references against writes (`ref.excl`, stage 7a of
//! `docs/future-work/RETURNED_REFERENCES_PATH.md`). Each shared `Ref` of
//! a place a local's path reaches is a loan of its own, [`Origin::Site`],
//! which the solver carries with the local's. A write to a place that
//! overlaps a site's is `conflicting-access` when a binding that holds
//! the site is read after the write on some path. The order comes from
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

/// The path of a shared `Ref`'s place `word` that takes a site loan: one
/// a local's fields and dereferences reach, without an element, which
/// the pin covers (`ref.elem.access.pin`).
pub(super) fn site_path(b: &Body, word: u32) -> Option<Path> {
    path_of(b, word).filter(|(_, p)| !p.iter().any(|s| matches!(s, Proj::Elem(_))))
}

/// Whether shared `Ref` `i` takes a site loan: a written `&` (the address
/// a call takes for a receiver is stage 7b's) of a place [`site_path`] gives.
pub(super) fn site(s: &Inputs<'_>, i: u32) -> bool {
    let (b, i) = (s.b, i as usize);
    let at = b.syn[i];
    at != NodeIdx::NONE
        && s.cx.src.parse.tree.node(at).kind() == SyntaxKind::RefExpr
        && site_path(b, b.data[i][0]).is_some()
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
    /// `&mut` a call takes writes at the call.
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
                // A `&mut` a call takes, or a `&mut` binding a call
                // reborrows; one a binding keeps is stage 7c's.
                Tag::Ref if is_mut(pool, b.ty[i]) && calls(b.tags.get(u)) => (a, u, false),
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

    /// `ref.excl.shared`: each write to a place that overlaps a site's
    /// while a binding that holds the site is still read.
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
            let Some(place) = site_path(b, b.data[i as usize][0]) else {
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
            sites.push((i, place, holders));
        }
        if sites.iter().all(|s| s.2.is_empty()) {
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
        for (wi, (root, path), at) in self.writes(&users) {
            let node = b.syn[wi as usize];
            if node == NodeIdx::NONE || out.iter().any(|x| x.0 == node) {
                continue;
            }
            'site: for (i, (r, sp), holders) in &sites {
                if *r != root || !overlap(sp, &path) || *i == wi {
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
                        out.push((node, self.conflict_message(root, h)));
                        break 'site;
                    }
                }
            }
        }
        out
    }

    fn conflict_message(&self, root: u32, holder: u32) -> String {
        let (b, names) = (self.s.b, &self.s.cx.names);
        let name = |l: u32| {
            let n = names.text(b.local_name[l as usize]);
            (!n.starts_with('$')).then(|| format!("`{n}`"))
        };
        let place = name(root).unwrap_or_else(|| "a temporary".to_owned());
        match name(holder) {
            Some(h) => format!(
                "this changes {place} while {h} refers to it and is used later; make the change after {h}'s last use"
            ),
            None => format!(
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
