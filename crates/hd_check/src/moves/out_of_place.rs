//! Moves out of a place behind a reference (`ref.move.out-of-place`,
//! #1144): a read of a value that is not copied, through a `&T` or a
//! `&mut T`, where the value moves, is `move-out-of-place` (Rust's
//! "cannot move out of `*r`"). The reference does not own the value, so
//! the read would give a second copy of a `&mut`. A borrow, `&*r`, or a
//! site that reborrows (`ref.move.reborrow`) reads nothing out. So is a read
//! that moves a binding or a field of one out of the closure that captures it
//! (`ref.move.out-of-place.capture`): a closure may run any number of times.

use hd_base::{HashMap, LocalId, SliceSort as _};
use hd_diag::Code;
use hd_resolve::ItemData;
use hd_tir::ir::{Body, NONE, Place, PlaceRoot, Proj, Role, Tag, Topology, Word, slots};
use hd_types::{Ty, TyData};

use super::{Uses, local_of, w};
use crate::body::Ck;

/// Each value's users, with the role of each use.
pub(super) fn users_of(b: &Body) -> Vec<Vec<(u32, Role)>> {
    let n = b.len();
    let mut users: Vec<Vec<(u32, Role)>> = vec![Vec::new(); n];
    for i in 0..n {
        slots(b, i, |_, v, word| {
            if let Word::Value(role) = word
                && hd_tir::ir::Ref(v).as_inst().is_some()
                && (v as usize) < n
            {
                users[v as usize].push((w(i), role));
            }
        });
    }
    users
}

/// The reads of each whole hidden local.
pub(super) fn hidden_reads_of(b: &Body, hidden: &[bool]) -> HashMap<LocalId, Vec<u32>> {
    let mut reads: HashMap<LocalId, Vec<u32>> = HashMap::default();
    for i in 0..b.len() {
        if b.tags[i] == Tag::Read
            && let Some((l, true)) = local_of(b, b.data[i][0])
            && hidden[l.idx()]
        {
            reads.entry(l).or_default().push(w(i));
        }
    }
    reads
}

/// What a place reads through: a binding, read directly or through a
/// value that reads it whole, or another value.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Root {
    Local(LocalId),
    Value(u32),
}

fn root_of(b: &Body, root: PlaceRoot) -> Root {
    let whole = |i: usize| {
        (i < b.len() && b.tags[i] == Tag::Read)
            .then(|| local_of(b, b.data[i][0]))
            .flatten()
    };
    match root {
        PlaceRoot::Local(l) => Root::Local(l),
        PlaceRoot::Value(r) => match whole(r.0 as usize) {
            Some((l, true)) => Root::Local(l),
            _ => Root::Value(r.0),
        },
        _ => Root::Value(NONE),
    }
}

/// Where a read that must not move reads from.
#[derive(Clone, Copy)]
enum From {
    /// A place behind a reference of this kind.
    Reference(RefKind),
    /// A binding the read's closure captures.
    Capture,
}

/// The reference a place is behind: `&T`, `&mut T`, or a counted `*T`
/// or `*mut T`.
#[derive(Clone, Copy)]
struct RefKind {
    mutable: bool,
    counted: bool,
}

impl RefKind {
    /// The reference as written: `&`, `&mut`, `*` or `*mut`.
    fn spelling(self) -> &'static str {
        match (self.counted, self.mutable) {
            (false, false) => "&",
            (false, true) => "&mut",
            (true, false) => "*",
            (true, true) => "*mut",
        }
    }
}

/// Each closure's captures, as (sub-body, local), sorted.
fn captures_of(b: &Body) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for i in 0..b.len() {
        if b.tags[i] == Tag::Closure
            && !b.is_instantiation(i)
            && let [start, len, ..] = *b.record(b.data[i][1])
        {
            for c in start..start.saturating_add(len) {
                if let Some(&l) = b.cap_local.get(c as usize) {
                    out.push((b.data[i][0], w(l.idx())));
                }
            }
        }
    }
    out.sort_stable();
    out.dedup();
    out
}

/// Whether place `p` of a read in sub-body `sub` is all of, or a field or
/// payload of, a binding that sub-body captures.
fn captured(captures: &[(u32, u32)], sub: u32, root: Root, p: &Place) -> bool {
    let Root::Local(l) = root else {
        return false;
    };
    p.proj
        .iter()
        .all(|x| matches!(x, Proj::Field(_) | Proj::Variant(_)))
        && captures.binary_search(&(sub, w(l.idx()))).is_ok()
}

impl Ck<'_, '_> {
    /// Which locals are hidden: a call's `$ref` and the like hold a value
    /// for the checker's own lowering, and are not bindings.
    pub(super) fn hidden_locals(&self, b: &Body) -> Vec<bool> {
        (b.local_name.iter())
            .map(|&s| self.cx.names.text(s).starts_with('$'))
            .collect()
    }

    /// `ref.move.out-of-place`: each read that moves a value that is not
    /// copied out of a place behind a reference, or out of a binding its
    /// closure captures, is `move-out-of-place`.
    pub(crate) fn check_moves_out_of_place(&mut self) {
        if self.b.body().tags.contains(&Tag::Poison) {
            return;
        }
        let copied = self.copy_facts();
        let b = self.b.body();
        let topo = Topology::new(b);
        let captures = captures_of(b);
        // The reads of a value that is not copied through a reference or
        // out of a capture, with where from and the place's root.
        let reads: Vec<(u32, From, Root)> = (0..b.len())
            .filter(|&i| b.tags[i] == Tag::Read && !copied[i])
            .filter_map(|i| {
                let p = b.decode_place(b.data[i][0])?;
                let root = root_of(b, p.root);
                let from = match self.behind_ref(b, &p) {
                    Some(kind) => From::Reference(kind),
                    None if captured(&captures, topo.sub[i], root, &p) => From::Capture,
                    None => return None,
                };
                Some((w(i), from, root))
            })
            .collect();
        if reads.is_empty() {
            return;
        }
        let hidden = self.hidden_locals(b);
        let users = users_of(b);
        let hidden_reads = hidden_reads_of(b, &hidden);
        let cx = Uses {
            copied: &copied,
            b,
            topo: &topo,
            users: &users,
            hidden: &hidden,
            hidden_reads: &hidden_reads,
            root_result_moves: true,
        };
        // One error per reference, as for `use-of-moved-value`.
        let mut seen: Vec<Root> = Vec::new();
        let mut moved: Vec<(u32, From)> = Vec::new();
        for (i, from, root) in reads {
            if !seen.contains(&root) && self.moves(&cx, i) {
                seen.push(root);
                moved.push((i, from));
            }
        }
        for (i, from) in moved {
            let at = self.b.body().syn[i as usize];
            if at == hd_base::NodeIdx::NONE {
                continue;
            }
            let why = self.why_not_copied(self.b.body().ty[i as usize]);
            let msg = match from {
                From::Reference(kind) if kind.counted => {
                    let r = kind.spelling();
                    let borrow = if kind.mutable { "&mut" } else { "&" };
                    format!(
                        "this moves a value out of an object behind a `{r}` reference, which other references to the object still read; {why}. Borrow it with `{borrow}`, or build a copy"
                    )
                }
                From::Reference(kind) => {
                    let r = kind.spelling();
                    format!(
                        "this moves a value out of a place behind a `{r}` reference, which does not own it; {why}. Borrow it with `{r}`, or pass the reference on"
                    )
                }
                From::Capture => format!(
                    "this moves a value out of a binding the closure captures, and a closure may run any number of times; {why}. Borrow it with `&`"
                ),
            };
            let node = self.cx.src.parse.tree.node(at);
            self.err(Code::MoveOutOfPlace, node, &msg);
        }
    }

    /// The first reference place `p` reaches its value through, if any:
    /// a `&T`, `&mut T`, `*T` or `*mut T`. An element is left to the
    /// element rules.
    ///
    /// A place rooted at a value that reads a place behind a reference is
    /// behind it too: a pattern on `*r` reads its payload out of that
    /// read (`ref.move.pattern`). The innermost such read decides; a
    /// loop finds it, as deep as the source writes them, where a depth
    /// cap of 16 answered from the outer place past it (#1777).
    fn behind_ref(&self, b: &Body, p: &Place) -> Option<RefKind> {
        let mut inner: Vec<Place> = Vec::new();
        loop {
            let at = inner.last().unwrap_or(p);
            let PlaceRoot::Value(r) = at.root else { break };
            if (r.0 as usize) < b.len()
                && b.tags[r.0 as usize] == Tag::Read
                && !at.proj.is_empty()
                && self.written_deref(b, r.0 as usize)
                && let Some(next) = b.decode_place(b.data[r.0 as usize][0])
            {
                inner.push(next);
            } else {
                break;
            }
        }
        inner
            .iter()
            .rev()
            .chain(std::iter::once(p))
            .find_map(|q| self.behind_ref_here(b, q))
    }

    /// Whether value `i` is a dereference written in the source, `*r`.
    /// Matching on `r` itself reads its referent implicitly and binds
    /// through the reference instead.
    fn written_deref(&self, b: &Body, i: usize) -> bool {
        let at = b.syn[i];
        at != hd_base::NodeIdx::NONE
            && self.cx.src.parse.tree.node(at).kind() == hd_syntax::SyntaxKind::DerefExpr
    }

    /// [`Self::behind_ref`] of `p`'s own type and projections.
    fn behind_ref_here(&self, b: &Body, p: &Place) -> Option<RefKind> {
        let pool = self.pool();
        let mut t = match p.root {
            PlaceRoot::Local(l) => b.local_ty[l.idx()],
            PlaceRoot::Value(r) if (r.0 as usize) < b.len() => b.ty[r.0 as usize],
            _ => return None,
        };
        if p.proj.iter().any(|x| matches!(x, Proj::Elem(_))) {
            return None;
        }
        let mut variant = None;
        for proj in &p.proj {
            match *proj {
                // The first dereference decides, for a counted reference
                // as for a plain one: other references to a counted
                // object still read the field.
                // ruled out: a dereference of a value that is no
                // reference; the checker builds `*` only on a value whose
                // type `ref_of` reads as a reference, since 064bf1dc7
                // removed the data objects whose `*` kept the object's own
                // type.
                Proj::Deref => {
                    let TyData::Ref {
                        counted, mutable, ..
                    } = pool.get(t)
                    else {
                        return None;
                    };
                    return Some(RefKind { mutable, counted });
                }
                Proj::Variant(x) => variant = Some(x),
                Proj::Field(j) => {
                    t = self.field_ty(t, variant.take(), j)?;
                }
                Proj::Elem(_) => return None,
            }
        }
        None
    }

    /// The type of field `j` of `t`, of variant `variant` for an enum, with
    /// `t`'s type arguments: a struct's, an enum's or a tuple's.
    fn field_ty(&self, t: Ty, variant: Option<u32>, j: u32) -> Option<Ty> {
        let pool = self.pool();
        match pool.get(t) {
            TyData::Tuple { elems, .. } => pool.list_items(elems).get(j as usize).copied(),
            TyData::Adt { def, args } => {
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
                let ft = fields.get(j as usize)?.ty;
                let argv = pool.list_items(args);
                Some(pool.subst(ft, &|q| {
                    (q.owner == def)
                        .then(|| argv.get(q.index as usize).copied())
                        .flatten()
                }))
            }
            _ => None,
        }
    }
}
