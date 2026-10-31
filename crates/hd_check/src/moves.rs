//! Moves (`ref.move`, #1137, slice M1 of #1130): a `&mut T` value is
//! not copied. A read of a whole `&mut` binding moves it where no `&mut`
//! type is expected: a `:=` or untyped `let`, a generic argument, a tuple
//! or optional part, a return. A site that expects a declared `&mut`
//! reborrows it instead: a parameter, a typed `let`, an assignment, a
//! field declared `&mut`. A use of the binding after a move on some path is
//! `use-of-moved-value`, until an assignment gives it a value again. The
//! paths are the forward walk over the structured control flow that the
//! ownership passes use (`hd_tir::own::flow`).

use hd_base::{HashMap, LocalId, SliceSort as _};
use hd_diag::Code;
use hd_resolve::ItemData;
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

/// The transfer of the move walk: one bit per `&mut` local, set while the
/// local may have moved.
struct Walk<'x> {
    /// The bit of each local, or `NONE`.
    bit: &'x [u32],
    /// The reads that move their local.
    moving: &'x [bool],
    /// The first use after a move of each local, by its bit.
    found: &'x mut HashMap<u32, u32>,
}

impl Walk<'_> {
    fn use_of(&mut self, bit: u32, x: u32, s: &BitArena, st: usize) {
        // A loop's later round may reach an earlier use.
        if s.get(st, bit) {
            let at = self.found.entry(bit).or_insert(x);
            *at = (*at).min(x);
        }
    }
}

impl Forward for Walk<'_> {
    const SUITES: Suites = Suites::FromExits;

    fn stmt(&mut self, b: &Body, x: u32, s: &mut BitArena, st: usize) {
        let i = x as usize;
        match b.tags[i] {
            tag @ (Tag::Read | Tag::Ref | Tag::Store) => {
                let Some((l, whole)) = local_of(b, b.data[i][0]) else {
                    return;
                };
                let bit = self.bit[l.idx()];
                if bit == NONE {
                    return;
                }
                // `ref.move.reassign`: storing a whole value makes the
                // binding usable again.
                if tag == Tag::Store && whole {
                    s.set(st, bit, false);
                    return;
                }
                self.use_of(bit, x, s, st);
                if tag == Tag::Read && whole && self.moving[i] {
                    s.set(st, bit, true);
                }
            }
            // A closure's captures use the locals it captures.
            Tag::Closure if !b.is_instantiation(i) => {
                if let [start, len, ..] = *b.record(b.data[i][1]) {
                    for c in start..start.saturating_add(len) {
                        if let Some(&l) = b.cap_local.get(c as usize)
                            && self.bit[l.idx()] != NONE
                        {
                            self.use_of(self.bit[l.idx()], x, s, st);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn enter(&mut self, _: &Body, _: u32, _: &mut BitArena, _: usize) {}
}

impl Ck<'_, '_> {
    /// `ref.move`: each use of a `&mut` binding after its value moved on
    /// some path is `use-of-moved-value`, reported once per binding.
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
        let mut bits = 0;
        let bit: Vec<u32> = (b.local_ty.iter().zip(&hidden))
            .map(|(&t, &hidden)| {
                if !hidden && is_mut_ref(pool, t) {
                    bits += 1;
                    bits - 1
                } else {
                    NONE
                }
            })
            .collect();
        if bits == 0 {
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
        let moving: Vec<bool> = (0..n)
            .map(|i| {
                b.tags[i] == Tag::Read
                    && local_of(b, b.data[i][0])
                        .is_some_and(|(l, whole)| whole && bit[l.idx()] != NONE)
                    && self.moves(&cx, w(i), 0)
            })
            .collect();
        if !moving.contains(&true) {
            return;
        }
        let mut found = HashMap::default();
        let mut flow = Flow::default();
        flow.arena.words = (bits as usize).div_ceil(64);
        // The body's root is not yet in `sub_root`, which holds the
        // closures' roots.
        let roots = std::iter::once(root.0).chain(b.sub_root.iter().copied());
        for root in roots.filter(|&r| (r as usize) < n && b.tags[r as usize] == Tag::Block) {
            let st = flow.arena.alloc(None);
            let mut walk = Walk {
                bit: &bit,
                moving: &moving,
                found: &mut found,
            };
            forward(b, cx.topo, &mut flow, &mut walk, root, st);
            flow.arena.free(st);
        }
        let mut found: Vec<(u32, u32)> = found.into_iter().collect();
        found.sort_stable_by_key(|&(_, x)| x);
        for (bit_of, x) in found {
            let b = self.b.body();
            let at = b.syn[x as usize];
            let Some(l) = bit.iter().position(|&k| k == bit_of) else {
                continue;
            };
            if at == hd_base::NodeIdx::NONE {
                continue;
            }
            let name = self.cx.names.text(b.local_name[l]).to_string();
            let msg = format!(
                "`{name}` was moved before this use; pass `&mut *{name}` where it moves to keep using it"
            );
            let node = self.cx.src.parse.tree.node(at);
            self.err(Code::UseOfMovedValue, node, &msg);
        }
    }

    /// Whether value `v`, a `&mut`, moves where it is used
    /// (`ref.move.sites`, `ref.move.reborrow`).
    fn moves(&self, cx: &Uses<'_>, v: u32, depth: u32) -> bool {
        if depth > 64 {
            return false;
        }
        let (pool, b) = (self.pool(), cx.b);
        cx.users[v as usize].iter().any(|&(u, role)| {
            let i = u as usize;
            if b.tags[i] == Tag::Coerce {
                // A coercion to `&T` reborrows; one that keeps `&mut`
                // passes the value on.
                return is_mut_ref(pool, b.ty[i]) && self.moves(cx, u, depth + 1);
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
