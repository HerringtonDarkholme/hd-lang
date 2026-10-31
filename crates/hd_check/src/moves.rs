//! Moves (`ref.move`, #1137 and #1138, slices M1 and M2 of #1130): a
//! `&mut T` value is not copied, nor is a value of a type that holds one
//! ([`copies`]). A read of a whole such binding moves it where no `&mut`
//! type is expected: a `:=` or untyped `let`, a generic argument, a tuple
//! or optional part, a return. A read of a field that is not copied
//! moves the field out the same way, and the binding is then partly
//! moved: its other fields stay usable. A site that expects a declared `&mut`
//! reborrows a `&mut` instead: a parameter, a typed `let`, an assignment,
//! a field declared `&mut`. A use of the binding after a move on some path
//! is `use-of-moved-value`, until an assignment gives it a value again. The
//! paths are the forward walk over the structured control flow that the
//! ownership passes use (`hd_tir::own::flow`).

use hd_base::{HashMap, LocalId, SliceSort as _};
use hd_diag::Code;
use hd_resolve::{ItemData, Lookup};
use hd_syntax::SyntaxKind;
use hd_tir::Callee;
use hd_tir::ir::{Body, NONE, PlaceRoot, Proj, Ref, Role, Tag, Topology, Word, slots};
use hd_tir::own::flow::{BitArena, Flow, Forward, Suites, forward};
use hd_types::{Ty, TyData, Types};

use crate::body::Ck;

/// An instruction index as a word.
fn w(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

/// Whether `t`, under any `mut`, is `&mut`: a local reference, since a
/// `*mut` is copied (`ref.move.mut`).
fn is_mut_ref(pool: Types<'_>, t: Ty) -> bool {
    match pool.get(t) {
        TyData::Ref {
            mutable, counted, ..
        } => mutable && !counted,
        TyData::Mut(i) => is_mut_ref(pool, i),
        _ => false,
    }
}

/// Whether a value of `t` is copied (`ref.move.holds`): not a `&mut`, nor
/// a struct, enum, tuple, optional or union that holds one by value. A
/// `*mut T` and a `&T` copy; so does a type parameter.
fn copies(pool: Types<'_>, lookup: &Lookup<'_>, t: Ty) -> bool {
    copies_in(pool, lookup, t, &mut Vec::new())
}

fn copies_in(pool: Types<'_>, lookup: &Lookup<'_>, t: Ty, seen: &mut Vec<Ty>) -> bool {
    if seen.contains(&t) || seen.len() > 32 {
        return true;
    }
    let all = |ts: &[Ty], seen: &mut Vec<Ty>| ts.iter().all(|&e| copies_in(pool, lookup, e, seen));
    match pool.get(t) {
        TyData::Ref {
            mutable, counted, ..
        } => !mutable || counted,
        TyData::Mut(i) | TyData::Option(i) => copies_in(pool, lookup, i, seen),
        TyData::Tuple { elems, rest } => {
            all(pool.list_items(elems), seen)
                && rest.is_none_or(|r| copies_in(pool, lookup, r, seen))
        }
        TyData::Union(ms) => all(pool.list_items(ms), seen),
        TyData::Adt { def, args } => {
            let Some(item) = lookup.item(def) else {
                return true;
            };
            let fields: Vec<&hd_resolve::Field> = match &item.data {
                ItemData::Data(fs) => fs.iter().collect(),
                ItemData::Enum { shared, variants } => (shared.iter())
                    .chain(variants.iter().flat_map(|v| &v.fields))
                    .collect(),
                _ => return true,
            };
            let argv = pool.list_items(args);
            seen.push(t);
            let ok = fields.iter().all(|f| {
                let ft = pool.subst(f.ty, &|p| {
                    (p.owner == def)
                        .then(|| argv.get(p.index as usize).copied())
                        .flatten()
                });
                copies_in(pool, lookup, ft, seen)
            });
            seen.pop();
            ok
        }
        _ => true,
    }
}

/// `t` without `mut`.
fn strip(pool: Types<'_>, t: Ty) -> Ty {
    match pool.get(t) {
        TyData::Mut(i) => strip(pool, i),
        _ => t,
    }
}

/// The local and whether the place is the whole local, for a place word
/// rooted at a local.
fn local_of(b: &Body, word: u32) -> Option<(LocalId, bool)> {
    let p = b.decode_place(word)?;
    match p.root {
        PlaceRoot::Local(l) => Some((l, p.proj.is_empty())),
        _ => None,
    }
}

/// What [`Ck::moves`] follows a value through.
struct Uses<'x> {
    b: &'x Body,
    topo: &'x Topology,
    /// Each value's users, with the role of each use.
    users: &'x [Vec<(u32, Role)>],
    hidden: &'x [bool],
    hidden_reads: &'x HashMap<LocalId, Vec<u32>>,
}

/// The binding a place reaches and the field path to it: a read of
/// `h.a.b` is a read of `h` and two field reads, each rooted at the last.
struct Access {
    local: LocalId,
    path: Vec<u32>,
    /// Whether the place is the field itself, not a place behind a
    /// dereference or an element of it.
    exact: bool,
}

/// The transfer of the move walk: one bit per binding or field path that
/// some read moves (`ref.move.field`), set while it may have moved.
struct Walk<'x> {
    acc: &'x [Option<Access>],
    /// The reads that only lead to a field read of the same binding.
    link: &'x [bool],
    /// The bit each moving read sets, or `NONE`.
    move_bit: &'x [u32],
    /// The binding and path of each bit.
    bits: &'x [(LocalId, Vec<u32>)],
    of_local: &'x HashMap<LocalId, Vec<u32>>,
    /// The first use after a move of each binding, and whether only a
    /// field of it moved.
    found: &'x mut HashMap<LocalId, (u32, bool)>,
}

impl Walk<'_> {
    /// A use of `path` of `l` conflicts with a moved path that contains
    /// it or lies inside it; with `strict`, only with one that contains
    /// it, as an assignment to the path needs.
    fn use_of(&mut self, l: LocalId, path: &[u32], strict: bool, x: u32, s: &BitArena, st: usize) {
        for &k in self.of_local.get(&l).into_iter().flatten() {
            let p = &self.bits[k as usize].1;
            let hit = if strict {
                p.len() < path.len() && path.starts_with(p)
            } else {
                path.starts_with(p) || p.starts_with(path)
            };
            // A loop's later round may reach an earlier use.
            if hit && s.get(st, k) {
                let at = self.found.entry(l).or_insert((x, !p.is_empty()));
                if x < at.0 {
                    *at = (x, !p.is_empty());
                }
            }
        }
    }
}

impl Forward for Walk<'_> {
    const SUITES: Suites = Suites::FromExits;

    fn stmt(&mut self, b: &Body, x: u32, s: &mut BitArena, st: usize) {
        let i = x as usize;
        match b.tags[i] {
            tag @ (Tag::Read | Tag::Ref | Tag::Store) => {
                let Some(a) = &self.acc[i] else {
                    return;
                };
                if self.link[i] {
                    return;
                }
                // `ref.move.reassign`: storing a whole value at a moved
                // path makes it usable again.
                if tag == Tag::Store && a.exact {
                    self.use_of(a.local, &a.path, true, x, s, st);
                    for &k in self.of_local.get(&a.local).into_iter().flatten() {
                        if self.bits[k as usize].1.starts_with(&a.path) {
                            s.set(st, k, false);
                        }
                    }
                    return;
                }
                self.use_of(a.local, &a.path, false, x, s, st);
                if self.move_bit[i] != NONE {
                    s.set(st, self.move_bit[i], true);
                }
            }
            // A closure's captures use the locals it captures.
            Tag::Closure if !b.is_instantiation(i) => {
                if let [start, len, ..] = *b.record(b.data[i][1]) {
                    for c in start..start.saturating_add(len) {
                        if let Some(&l) = b.cap_local.get(c as usize) {
                            self.use_of(l, &[], false, x, s, st);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn enter(&mut self, _: &Body, _: u32, _: &mut BitArena, _: usize) {}
}

/// The leading field projections of `proj`, and whether they are all.
fn field_prefix(proj: &[Proj]) -> (Vec<u32>, bool) {
    let mut path = Vec::new();
    for p in proj {
        match *p {
            Proj::Field(k) => path.push(k),
            _ => return (path, false),
        }
    }
    (path, true)
}

impl Ck<'_, '_> {
    /// `ref.move`: each use of a binding that is not copied, after its
    /// value moved on some path, is `use-of-moved-value`, reported once
    /// per binding.
    pub(crate) fn check_moves(&mut self, root: Ref) {
        let pool = self.pool();
        let b = self.b.body();
        // A body with an error has poison values, whose uses say nothing.
        if b.tags.contains(&Tag::Poison) {
            return;
        }
        // A hidden local, such as a call's `$ref`, holds a value for the
        // checker's own lowering, and is not a binding.
        let hidden: Vec<bool> = (b.local_name.iter())
            .map(|&s| self.cx.names.text(s).starts_with('$'))
            .collect();
        let held: Vec<bool> = (b.local_ty.iter())
            .map(|&t| !copies(pool, self.cx.lookup, t))
            .collect();
        let tracked = |l: LocalId| held[l.idx()] && !hidden[l.idx()];
        if !(0..b.local_ty.len()).any(|l| held[l] && !hidden[l]) {
            return;
        }
        let n = b.len();
        let mut users: Vec<Vec<(u32, Role)>> = vec![Vec::new(); n];
        for i in 0..n {
            slots(b, i, |_, v, word| {
                if let Word::Value(role) = word
                    && Ref(v).as_inst().is_some()
                    && (v as usize) < n
                {
                    users[v as usize].push((w(i), role));
                }
            });
        }
        // The reads of each whole hidden local.
        let mut hidden_reads: HashMap<LocalId, Vec<u32>> = HashMap::default();
        for i in 0..n {
            if b.tags[i] == Tag::Read
                && let Some((l, true)) = local_of(b, b.data[i][0])
                && hidden[l.idx()]
            {
                hidden_reads.entry(l).or_default().push(w(i));
            }
        }
        let cx = Uses {
            b,
            topo: &Topology::new(b),
            users: &users,
            hidden: &hidden,
            hidden_reads: &hidden_reads,
        };
        // The binding and field path each place reaches.
        let mut acc: Vec<Option<Access>> = Vec::with_capacity(n);
        for i in 0..n {
            let a = (matches!(b.tags[i], Tag::Read | Tag::Ref | Tag::Store))
                .then(|| b.decode_place(b.data[i][0]))
                .flatten()
                .and_then(|p| {
                    let (more, exact) = field_prefix(&p.proj);
                    let (local, mut path) = match p.root {
                        PlaceRoot::Local(l) if tracked(l) => (l, Vec::new()),
                        PlaceRoot::Value(r) => match acc.get(r.0 as usize) {
                            Some(Some(a)) if a.exact && b.tags[r.0 as usize] == Tag::Read => {
                                (a.local, a.path.clone())
                            }
                            _ => return None,
                        },
                        _ => return None,
                    };
                    path.extend(more);
                    Some(Access { local, path, exact })
                });
            acc.push(a);
        }
        // A read whose every use is a place rooted at it is not a use of
        // its own; the field reads at the ends are.
        let link: Vec<bool> = (0..n)
            .map(|i| {
                b.tags[i] == Tag::Read
                    && acc[i].is_some()
                    && !users[i].is_empty()
                    && users[i].iter().all(|&(u, _)| {
                        acc[u as usize].is_some()
                            && b.decode_place(b.data[u as usize][0])
                                .is_some_and(|p| p.root == PlaceRoot::Value(Ref(w(i))))
                    })
            })
            .collect();
        // A read of the binding, or of a field of it that is not copied,
        // moves it or the field where it moves (`ref.move.field`).
        let mut bits: Vec<(LocalId, Vec<u32>)> = Vec::new();
        let mut of_local: HashMap<LocalId, Vec<u32>> = HashMap::default();
        let mut move_bit = vec![NONE; n];
        for i in 0..n {
            let Some(a) = &acc[i] else { continue };
            let field_copies = !a.path.is_empty() && copies(pool, self.cx.lookup, b.ty[i]);
            if b.tags[i] != Tag::Read
                || link[i]
                || !a.exact
                || field_copies
                || !self.moves(&cx, w(i), 0)
            {
                continue;
            }
            let key = (a.local, a.path.clone());
            let k = bits.iter().position(|k| *k == key).unwrap_or_else(|| {
                bits.push(key);
                of_local.entry(a.local).or_default().push(w(bits.len() - 1));
                bits.len() - 1
            });
            move_bit[i] = w(k);
        }
        if bits.is_empty() {
            return;
        }
        let mut found = HashMap::default();
        let mut flow = Flow::default();
        flow.arena.words = bits.len().div_ceil(64);
        // The body's root is not yet in `sub_root`, which holds the
        // closures' roots.
        let roots = std::iter::once(root.0).chain(b.sub_root.iter().copied());
        for root in roots.filter(|&r| (r as usize) < n && b.tags[r as usize] == Tag::Block) {
            let st = flow.arena.alloc(None);
            let mut walk = Walk {
                acc: &acc,
                link: &link,
                move_bit: &move_bit,
                bits: &bits,
                of_local: &of_local,
                found: &mut found,
            };
            forward(b, cx.topo, &mut flow, &mut walk, root, st);
            flow.arena.free(st);
        }
        let mut found: Vec<(LocalId, (u32, bool))> = found.into_iter().collect();
        found.sort_stable_by_key(|&(_, (x, _))| x);
        for (l, (x, partial)) in found {
            let b = self.b.body();
            let at = b.syn[x as usize];
            if at == hd_base::NodeIdx::NONE {
                continue;
            }
            let l = l.idx();
            let name = self.cx.names.text(b.local_name[l]).to_string();
            let msg = if partial {
                format!(
                    "a field of `{name}` was moved out before this use; assign the field again to use `{name}` whole"
                )
            } else if is_mut_ref(self.pool(), b.local_ty[l]) {
                format!(
                    "`{name}` was moved before this use; pass `&mut *{name}` where it moves to keep using it"
                )
            } else {
                format!(
                    "`{name}` was moved before this use; its type holds a `&mut`, so it is not copied"
                )
            };
            let node = self.cx.src.parse.tree.node(at);
            self.err(Code::UseOfMovedValue, node, &msg);
        }
    }

    /// Whether value `v`, which is not copied, moves where it is used
    /// (`ref.move.sites`, `ref.move.reborrow`).
    fn moves(&self, cx: &Uses<'_>, v: u32, depth: u32) -> bool {
        if depth > 64 {
            return false;
        }
        let (pool, b) = (self.pool(), cx.b);
        cx.users[v as usize].iter().any(|&(u, role)| {
            let i = u as usize;
            if b.tags[i] == Tag::Coerce {
                // A coercion to `&T` reborrows; one that keeps a type
                // that is not copied passes the value on.
                return !copies(pool, self.cx.lookup, b.ty[i]) && self.moves(cx, u, depth + 1);
            }
            match role {
                Role::Consume => self.consume_moves(cx, i, v, depth),
                Role::Arg(k) => !self.param_is_mut(b, i, k as usize),
                // A block's value is its owner's. Nothing runs after a
                // sub-body's root block, so its result needs no move.
                Role::Tail => {
                    let owner = cx.topo.owner[i];
                    owner != NONE && self.moves(cx, owner, depth + 1)
                }
                _ => false,
            }
        })
    }

    /// Whether instruction `i`, which consumes value `v`, moves it: a
    /// store or a construction reborrows where a `&mut` type is expected
    /// (`ref.move.sites`, `ref.move.reborrow.typed`).
    fn consume_moves(&self, cx: &Uses<'_>, i: usize, v: u32, depth: u32) -> bool {
        let (pool, b) = (self.pool(), cx.b);
        let tag = b.tags[i];
        match tag {
            Tag::Store => {
                let Some(p) = b.decode_place(b.data[i][0]) else {
                    return true;
                };
                let (PlaceRoot::Local(h), true) = (p.root, p.proj.is_empty()) else {
                    return !self.place_is_mut(b, &p);
                };
                // A hidden local passes the value on to its reads.
                if cx.hidden[h.idx()] {
                    return (cx.hidden_reads.get(&h))
                        .is_some_and(|rs| rs.iter().any(|&r| self.moves(cx, r, depth + 1)));
                }
                // An assignment, or a `let` that writes the type, expects
                // the binding's `&mut` type; `:=` and a bare `let` do not.
                // Only a `&mut` reborrows (`ref.move.reborrow.typed`).
                if !is_mut_ref(pool, b.local_ty[h.idx()]) {
                    return true;
                }
                let at = b.syn[i];
                if at == hd_base::NodeIdx::NONE {
                    return true;
                }
                let n = self.cx.src.parse.tree.node(at);
                !(n.kind() == SyntaxKind::AssignmentStmt
                    || n.kind() == SyntaxKind::LetStmt && n.children().any(|c| c.kind().is_type()))
            }
            Tag::NewStruct | Tag::NewVariant | Tag::CopyStruct => {
                let rec = b.record(b.data[i][1]);
                let j = match tag {
                    Tag::CopyStruct => rec.chunks(2).find(|c| c.get(1) == Some(&v)).map(|c| c[0]),
                    _ => rec.iter().position(|&x| x == v).map(w),
                };
                let variant = (tag == Tag::NewVariant).then(|| b.data[i][0]);
                !(j.and_then(|j| self.field_decl(b.ty[i], variant, j)))
                    .is_some_and(|t| is_mut_ref(pool, t))
            }
            _ => true,
        }
    }

    /// The declared type of field `j` of `t`, of variant `variant` for an
    /// enum: a struct's or an enum's, not a tuple's or an optional's.
    fn field_decl(&self, t: Ty, variant: Option<u32>, j: u32) -> Option<Ty> {
        let TyData::Adt { def, .. } = self.pool().get(strip(self.pool(), t)) else {
            return None;
        };
        let fields = match (&self.cx.lookup.item(def)?.data, variant) {
            (ItemData::Data(fs), None) => fs,
            (ItemData::Enum { shared, variants }, Some(k)) if shared.is_empty() => {
                &variants.get(k as usize)?.fields
            }
            _ => return None,
        };
        Some(fields.get(j as usize)?.ty)
    }

    /// Whether place `p`'s declared type is `&mut`: a dereference's
    /// referent or a struct field written `&mut`. An element is a type
    /// argument's; a field of a field is not typed here, and reborrows.
    fn place_is_mut(&self, b: &Body, p: &hd_tir::ir::Place) -> bool {
        let pool = self.pool();
        let mut t = match p.root {
            PlaceRoot::Local(l) => b.local_ty[l.idx()],
            PlaceRoot::Value(r) if (r.0 as usize) < b.len() => b.ty[r.0 as usize],
            _ => return true,
        };
        let mut variant = None;
        for (k, proj) in p.proj.iter().enumerate() {
            let last = k + 1 == p.proj.len();
            t = match (*proj, pool.get(strip(pool, t))) {
                (Proj::Deref, TyData::Ref { inner, .. }) => inner,
                (Proj::Variant(x), _) => {
                    variant = Some(x);
                    continue;
                }
                (Proj::Field(j), _) if last => {
                    return self
                        .field_decl(t, variant, j)
                        .is_some_and(|d| is_mut_ref(pool, d));
                }
                (Proj::Elem(_), _) => return !last,
                _ => return true,
            };
        }
        is_mut_ref(pool, t)
    }

    /// Whether call `i` declares argument `k`'s parameter `&mut`, so the
    /// argument is reborrowed (`ref.move.reborrow`). An unknown callee
    /// reborrows.
    fn param_is_mut(&self, b: &Body, i: usize, k: usize) -> bool {
        let pool = self.pool();
        let sig = |def| {
            self.sig_of(def)
                .ok()
                .map(|s| s.params.iter().map(|p| p.1).collect())
        };
        let params: Option<Vec<Ty>> = match b.tags[i] {
            Tag::Call | Tag::Await => match Callee::from_words(b.record(b.data[i][0])) {
                Some(Callee::Item { def, .. } | Callee::TraitMethod { method: def, .. }) => {
                    sig(def)
                }
                None => None,
            },
            // A dynamic call's arguments follow its receiver.
            Tag::CallDyn => (b.record(b.data[i][1]).first())
                .and_then(|&m| sig(hd_base::DefId::from_raw(m)))
                .map(|ps: Vec<Ty>| ps.into_iter().skip(1).collect()),
            Tag::CallValue => match b.ty.get(b.data[i][0] as usize).map(|&t| pool.get(t)) {
                Some(TyData::Fn { params, .. }) => Some(pool.list_items(params).to_vec()),
                _ => None,
            },
            _ => None,
        };
        // A variadic parameter takes the arguments past the last.
        params.is_none_or(|ps| ps.get(k).or(ps.last()).is_none_or(|&t| is_mut_ref(pool, t)))
    }
}
