//! Moves (`ref.move`, #1137 and #1138, slices M1 and M2 of #1130): a
//! `&mut T` value is not copied, nor is a value of a type that holds one
//! ([`Ck::copies`]). A read of a whole such binding moves it where no `&mut`
//! type is expected: a `:=` or untyped `let`, a generic argument, a tuple
//! or optional part, a return. A read of a field that is not copied
//! moves the field out the same way, and the binding is then partly
//! moved: its other fields stay usable. A pattern that binds a payload
//! reads it as a field of its variant, so a match moves the payload out
//! the same way (`ref.move.pattern`). A site that expects a declared `&mut`
//! reborrows a `&mut` instead: a parameter, a typed `let`, an assignment,
//! a field declared `&mut`. A use of the binding after a move on some path
//! is `use-of-moved-value`, until an assignment gives it a value again. The
//! paths are the forward walk over the structured control flow that the
//! ownership passes use (`hd_tir::own::flow`).

use hd_base::{HashMap, HashSet, LocalId, SliceSort as _};
use hd_diag::Code;
use hd_resolve::{ItemData, Lookup};
use hd_syntax::SyntaxKind;
use hd_tir::Callee;
use hd_tir::ir::{Body, NONE, PlaceRoot, Proj, Ref, Role, Tag, Topology};
use hd_tir::own::flow::{BitArena, Flow, Forward, Suites, forward};
use hd_types::{CopyDecl, ParamRef, Ty, TyData, Types};

use crate::body::Ck;

mod out_of_place;

/// What [`Ck::check_moves`] finds for the exclusivity rules.
#[derive(Default)]
pub(crate) struct MoveFacts {
    /// The reads of a whole `&mut` binding or field that a typed `let`,
    /// an assignment or a field declared `&mut` reborrows, each a loan
    /// like `&mut *c` (`ref.move.reborrow.excl`).
    pub(crate) reborrows: HashSet<u32>,
    /// The reads that move a binding or a part of it (`ref.move.sites`,
    /// `ref.move.field`, `ref.move.pattern`), which `ref.excl.move` checks.
    pub(crate) moves: HashSet<u32>,
}

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
        _ => false,
    }
}

/// The part of `t` that keeps a value of it from being copied
/// (`ref.move.holds`, `ref.copy.structure`): a `&mut`, a type declared
/// `!Copy`, or a type parameter that is not `Copy`, held by value in a
/// struct, enum, tuple, optional or union. A `*mut T` and a `&T` copy.
/// `relaxed` lists the body's local functions' `?Copy` parameters.
fn blocker(
    cx: (Types<'_>, &Lookup<'_>, &[Ty]),
    t: Ty,
    seen: &mut Vec<Ty>,
    normalize: &mut dyn FnMut(Ty) -> Ty,
) -> Option<Ty> {
    let (pool, lookup, relaxed) = cx;
    let t = if matches!(pool.get(t), TyData::Assoc { .. }) {
        normalize(t)
    } else {
        t
    };
    if seen.contains(&t) || seen.len() > 32 {
        return None;
    }
    let mut any =
        |ts: &[Ty], seen: &mut Vec<Ty>| ts.iter().find_map(|&e| blocker(cx, e, seen, normalize));
    match pool.get(t) {
        TyData::Ref {
            mutable, counted, ..
        } => (mutable && !counted).then_some(t),
        TyData::Tuple { elems, rest } => any(pool.list_items(elems), seen)
            .or_else(|| rest.and_then(|r| blocker(cx, r, seen, normalize))),
        TyData::Union(ms) => any(pool.list_items(ms), seen),
        TyData::Param(_) if relaxed.contains(&t) => Some(t),
        TyData::Param(p) => (!param_copies((pool, lookup), p)).then_some(t),
        TyData::Assoc { assoc, .. } => {
            let copies = lookup.item(assoc).is_some_and(|item| match &item.data {
                ItemData::AssocType { bounds, .. } => bounds.iter().any(|&bound| {
                    pool.trait_value(bound).is_some_and(|(trait_, _)| {
                        lookup
                            .item(trait_)
                            .is_some_and(hd_resolve::Item::requires_copy)
                    })
                }),
                _ => false,
            });
            (!copies).then_some(t)
        }
        TyData::Adt { def, args } => {
            let members = match lookup.item(def)?.copy_decl() {
                CopyDecl::Copies => return None,
                CopyDecl::NotCopy => return Some(t),
                CopyDecl::Members(ms) => ms,
            };
            let argv = pool.list_items(args);
            seen.push(t);
            let found = members.iter().find_map(|&m| {
                let ft = pool.subst(m, &|p| {
                    (p.owner == def)
                        .then(|| argv.get(p.index as usize).copied())
                        .flatten()
                });
                blocker(cx, ft, seen, normalize)
            });
            seen.pop();
            found
        }
        _ => None,
    }
}

/// Whether type parameter `p` is `Copy`: it has a bound on a trait that
/// implies it, as every parameter has unless declared `?Copy`
/// (`ref.copy.generic.default`, `ref.copy.trait.default`). A trait's
/// `Self` is `Copy` unless the trait is declared `< ?Copy`.
fn param_copies((pool, lookup): (Types<'_>, &Lookup<'_>), p: ParamRef) -> bool {
    let Some(item) = lookup.item(p.owner) else {
        return true;
    };
    if let ItemData::Trait(t) = &item.data
        && p.index == 0
    {
        return !t.relaxed_copy;
    }
    let generics = item.sig().map_or(&item.generics[..], |s| &s.generics[..]);
    let at = usize::from(p.index) - usize::from(matches!(item.data, ItemData::Trait(_)));
    generics.get(at).is_none_or(|g| {
        g.bounds.iter().any(|&b| {
            pool.trait_value(b)
                .is_some_and(|(d, _)| lookup.item(d).is_some_and(hd_resolve::Item::requires_copy))
        })
    })
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
    copied: &'x [bool],
    b: &'x Body,
    topo: &'x Topology,
    /// Each value's users, with the role of each use.
    users: &'x [Vec<(u32, Role)>],
    hidden: &'x [bool],
    hidden_reads: &'x HashMap<LocalId, Vec<u32>>,
    /// Whether a sub-body's result moves its value: the caller takes it.
    /// Use-after-move needs no such move, since nothing runs after it.
    root_result_moves: bool,
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
    /// The first use after a move of each binding, and the bit of the
    /// move it reaches.
    found: &'x mut HashMap<LocalId, (u32, u32)>,
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
                let at = self.found.entry(l).or_insert((x, k));
                if x < at.0 {
                    *at = (x, k);
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

/// The mark of a variant step in a field path, beside its index: a
/// match arm's payload is field `j` of variant `k`, the path `[k | VARIANT, j]`.
const VARIANT: u32 = 1 << 31;

/// The leading field and variant projections of `proj`, and whether they
/// are all.
fn field_prefix(proj: &[Proj]) -> (Vec<u32>, bool) {
    let mut path = Vec::new();
    for p in proj {
        match *p {
            Proj::Field(k) => path.push(k),
            Proj::Variant(k) => path.push(k | VARIANT),
            _ => return (path, false),
        }
    }
    (path, true)
}

impl Ck<'_, '_> {
    /// Resolve associated projections before the immutable move walk.
    /// A substituted field can name a concrete associated type even when
    /// the enclosing type's argument list contains no projection.
    fn copy_facts(&mut self) -> Vec<bool> {
        (0..self.b.body().len())
            .map(|i| {
                let b = self.b.body();
                let (tag, ty) = (b.tags[i], b.ty[i]);
                matches!(tag, Tag::Read | Tag::Coerce) && self.copies(ty)
            })
            .collect()
    }

    /// Whether a value of `t` is copied ([`blocker`]).
    pub(crate) fn copies(&mut self, t: Ty) -> bool {
        self.not_copied_by(t).is_none()
    }

    /// The part of `t` that keeps it from being copied ([`blocker`]).
    fn not_copied_by(&mut self, t: Ty) -> Option<Ty> {
        let relaxed = self.relaxed_local_params();
        blocker(
            (self.pool(), self.cx.lookup, &relaxed),
            t,
            &mut Vec::new(),
            &mut |ty| self.norm_ty(ty),
        )
    }

    /// The type parameters of the body's generic local functions declared
    /// `?Copy`: their bounds hold no trait that implies `Copy`
    /// (`ref.copy.generic.relaxed`).
    fn relaxed_local_params(&self) -> Vec<Ty> {
        let pool = self.pool();
        let lookup = self.cx.lookup;
        let copies = |b: &Ty| {
            pool.trait_value(*b)
                .is_some_and(|(d, _)| lookup.item(d).is_some_and(hd_resolve::Item::requires_copy))
        };
        (self.local_generics.values())
            .flat_map(|g| g.params.iter().zip(&g.generics))
            .filter(|(_, generic)| !generic.row && !generic.bounds.iter().any(copies))
            .map(|(&p, _)| p)
            .collect()
    }

    /// Why a value of `t` is not copied, for a message: the `&mut`, the
    /// `!Copy` type or the `?Copy` parameter it holds.
    pub(crate) fn why_not_copied(&mut self, t: Ty) -> String {
        let pool = self.pool();
        let Some(part) = self.not_copied_by(t) else {
            return "its type is not copied".to_string();
        };
        let shown = hd_resolve::show_ty(&self.cx.names, part);
        match pool.get(part) {
            TyData::Adt { .. } if part == t => {
                format!("its type `{shown}` is declared `impl !Copy`, so it is not copied")
            }
            TyData::Adt { .. } => {
                format!("its type holds `{shown}`, declared `impl !Copy`, so it is not copied")
            }
            TyData::Param(p)
                if p.index == 0
                    && self
                        .cx
                        .lookup
                        .item(p.owner)
                        .is_some_and(|i| matches!(i.data, ItemData::Trait(_))) =>
            {
                "its type holds the `Self` of a trait declared `< ?Copy`, so it is not copied"
                    .to_string()
            }
            TyData::Param(_) => {
                format!("its type holds `{shown}`, declared `?Copy`, so it is not copied")
            }
            TyData::Assoc { .. } => {
                format!("its associated type `{shown}` has no `Copy` bound, so it is not copied")
            }
            _ => "its type holds a `&mut`, so it is not copied".to_string(),
        }
    }

    /// `ref.move`: each use of a binding that is not copied, after its
    /// value moved on some path, is `use-of-moved-value`, reported once
    /// per binding. Returns the reads that move ([`MoveFacts`]).
    pub(crate) fn check_moves(&mut self, root: Ref) -> MoveFacts {
        let pool = self.pool();
        let mut facts = MoveFacts::default();
        // A body with an error has poison values, whose uses say nothing.
        if self.b.body().tags.contains(&Tag::Poison) {
            return facts;
        }
        let held: Vec<bool> = (0..self.b.body().local_ty.len())
            .map(|i| {
                let ty = self.b.body().local_ty[i];
                !self.copies(ty)
            })
            .collect();
        let hidden = self.hidden_locals(self.b.body());
        let tracked = |l: LocalId| held[l.idx()] && !hidden[l.idx()];
        if !(0..held.len()).any(|l| held[l] && !hidden[l]) {
            return facts;
        }
        let copied = self.copy_facts();
        let b = self.b.body();
        let n = b.len();
        let users = out_of_place::users_of(b);
        let hidden_reads = out_of_place::hidden_reads_of(b, &hidden);
        let cx = Uses {
            copied: &copied,
            b,
            topo: &Topology::new(b),
            users: &users,
            hidden: &hidden,
            hidden_reads: &hidden_reads,
            root_result_moves: false,
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
            let field_copies = !a.path.is_empty() && copied[i];
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
        for i in 0..n {
            if b.tags[i] == Tag::Read
                && acc[i].as_ref().is_some_and(|a| a.exact)
                && !link[i]
                && is_mut_ref(pool, b.ty[i])
                && self.reborrows(&cx, w(i), 0)
            {
                facts.reborrows.insert(w(i));
            }
        }
        if bits.is_empty() {
            return facts;
        }
        facts.moves = (0..n).filter(|&i| move_bit[i] != NONE).map(w).collect();
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
        let mut found: Vec<(LocalId, (u32, u32))> = found.into_iter().collect();
        found.sort_stable_by_key(|&(_, (x, _))| x);
        for (l, (x, k)) in found {
            let path = &bits[k as usize].1;
            let b = self.b.body();
            let at = b.syn[x as usize];
            if at == hd_base::NodeIdx::NONE {
                continue;
            }
            let l = l.idx();
            let name = self.cx.names.text(b.local_name[l]).to_string();
            let msg = if path.iter().any(|&p| p & VARIANT != 0) {
                format!(
                    "a payload of `{name}` was moved out by a pattern before this use; assign the matched value again to use `{name}` whole"
                )
            } else if !path.is_empty() {
                format!(
                    "a field of `{name}` was moved out before this use; assign the field again to use `{name}` whole"
                )
            } else if is_mut_ref(self.pool(), b.local_ty[l]) {
                format!(
                    "`{name}` was moved before this use; pass `&mut *{name}` where it moves to keep using it"
                )
            } else {
                let why = self.why_not_copied(b.local_ty[l]);
                format!("`{name}` was moved before this use; {why}")
            };
            let node = self.cx.src.parse.tree.node(at);
            self.err(Code::UseOfMovedValue, node, &msg);
        }
        facts
    }

    /// Whether `&mut` value `v` reaches a site that reborrows it as
    /// `&mut` with a binding, a place or a field to hold it: a typed
    /// `let`, an assignment, or a field or payload declared `&mut`
    /// (`ref.move.reborrow.typed`). A call's reborrow ends when it
    /// returns, and a hidden local's value is its reads'.
    fn reborrows(&self, cx: &Uses<'_>, v: u32, depth: u32) -> bool {
        let (pool, b) = (self.pool(), cx.b);
        depth <= 64
            && cx.users[v as usize].iter().any(|&(u, role)| {
                let i = u as usize;
                match (b.tags[i], role) {
                    (Tag::Coerce, _) => {
                        is_mut_ref(pool, b.ty[i]) && self.reborrows(cx, u, depth + 1)
                    }
                    (Tag::Store, Role::Consume)
                        if local_of(b, b.data[i][0])
                            .is_some_and(|(h, whole)| whole && cx.hidden[h.idx()]) =>
                    {
                        false
                    }
                    (
                        Tag::Store | Tag::NewStruct | Tag::NewVariant | Tag::CopyStruct,
                        Role::Consume,
                    ) => !self.consume_moves(cx, i, v, depth),
                    _ => false,
                }
            })
    }

    /// Whether value `v`, which is not copied, moves where it is used
    /// (`ref.move.sites`, `ref.move.reborrow`).
    fn moves(&self, cx: &Uses<'_>, v: u32, depth: u32) -> bool {
        if depth > 64 {
            return false;
        }
        let b = cx.b;
        cx.users[v as usize].iter().any(|&(u, role)| {
            let i = u as usize;
            if b.tags[i] == Tag::Coerce {
                // A coercion to `&T` reborrows; one that keeps a type
                // that is not copied passes the value on.
                return !cx.copied[i] && self.moves(cx, u, depth + 1);
            }
            match role {
                Role::Consume => self.consume_moves(cx, i, v, depth),
                Role::Arg(k) => !self.param_is_mut(b, i, k as usize),
                // A block's value is its owner's. A sub-body's root block
                // gives its result to the caller ([`Uses::root_result_moves`]).
                Role::Tail => {
                    let owner = cx.topo.owner[i];
                    if owner == NONE {
                        cx.root_result_moves
                    } else {
                        self.moves(cx, owner, depth + 1)
                    }
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
        let TyData::Adt { def, .. } = self.pool().get(t) else {
            return None;
        };
        let fields = match (&self.cx.lookup.item(def)?.data, variant) {
            (ItemData::Struct(fs, _), None) => fs,
            (
                ItemData::Enum {
                    shared, variants, ..
                },
                Some(k),
            ) if shared.is_empty() => &variants.get(k as usize)?.fields,
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
            t = match (*proj, pool.get(t)) {
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
