//! Moves out of a place behind a reference (`ref.move.out-of-place`,
//! #1144): a read of a value that is not copied, through a `&T` or a
//! `&mut T`, where the value moves, is `move-out-of-place` (Rust's
//! "cannot move out of `*r`"). The reference does not own the value, so
//! the read would give a second copy of a `&mut`. A borrow, `&*r`, or a
//! site that reborrows (`ref.move.reborrow`) reads nothing out.

use hd_base::{HashMap, LocalId};
use hd_diag::Code;
use hd_resolve::ItemData;
use hd_tir::ir::{Body, NONE, Place, PlaceRoot, Proj, Role, Tag, Topology, Word, slots};
use hd_types::{Ty, TyData};

use super::{Uses, local_of, strip, w};
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

impl Ck<'_, '_> {
    /// Which locals are hidden: a call's `$ref` and the like hold a value
    /// for the checker's own lowering, and are not bindings.
    pub(super) fn hidden_locals(&self, b: &Body) -> Vec<bool> {
        (b.local_name.iter())
            .map(|&s| self.cx.names.text(s).starts_with('$'))
            .collect()
    }

    /// `ref.move.out-of-place`: each read that moves a value that is not
    /// copied out of a place behind a reference is `move-out-of-place`.
    pub(crate) fn check_moves_out_of_place(&mut self) {
        let b = self.b.body();
        if b.tags.contains(&Tag::Poison) {
            return;
        }
        // The reads through a reference of a value that is not copied,
        // with whether the reference is `&mut` and the place's root.
        let reads: Vec<(u32, Option<bool>, Root)> = (0..b.len())
            .filter(|&i| b.tags[i] == Tag::Read && !self.copies(b.ty[i]))
            .filter_map(|i| {
                let p = b.decode_place(b.data[i][0])?;
                // Measurement switch: an element read moves out of the
                // collection, as Rust's "cannot move out of index".
                // A root local named `moving_*` is std's own mover (`pop`,
                // `swap`), whose copy-then-truncate is a move at run time.
                if hd_resolve::move_only_generics()
                    && p.proj.iter().any(|x| matches!(x, Proj::Elem(_)))
                {
                    if (b.local_name.iter()).any(|&n| self.cx.names.text(n).starts_with("moving_"))
                    {
                        return None;
                    }
                    return Some((w(i), None, root_of(b, p.root)));
                }
                let m = self.behind_ref(b, &p)?;
                Some((w(i), Some(m), root_of(b, p.root)))
            })
            .collect();
        if reads.is_empty() {
            return;
        }
        let hidden = self.hidden_locals(b);
        let users = users_of(b);
        let hidden_reads = hidden_reads_of(b, &hidden);
        let cx = Uses {
            b,
            topo: &Topology::new(b),
            users: &users,
            hidden: &hidden,
            hidden_reads: &hidden_reads,
        };
        // One error per reference, as for `use-of-moved-value`.
        let mut seen: Vec<Root> = Vec::new();
        let mut moved: Vec<(u32, Option<bool>)> = Vec::new();
        for (i, mutable, root) in reads {
            if !seen.contains(&root) && self.moves(&cx, i, 0) {
                seen.push(root);
                moved.push((i, mutable));
            }
        }
        for (i, mutable) in moved {
            let at = self.b.body().syn[i as usize];
            if at == hd_base::NodeIdx::NONE {
                continue;
            }
            let why = self.why_not_copied(self.b.body().ty[i as usize]);
            let msg = match mutable {
                None => format!(
                    "this moves a value out of a collection element; {why}. Borrow it with `&`, or take it out with `pop`/`remove_at`"
                ),
                Some(mutable) => {
                    let r = if mutable { "&mut" } else { "&" };
                    format!(
                        "this moves a value out of a place behind a `{r}` reference, which does not own it; {why}. Borrow it with `{r}`, or pass the reference on"
                    )
                }
            };
            let node = self.cx.src.parse.tree.node(at);
            self.err(Code::MoveOutOfPlace, node, &msg);
        }
    }

    /// Whether place `p` reaches its value through a `&T` or `&mut T`, and
    /// then whether the first such reference is `&mut`. A dereference of a
    /// data object's payload is not through a reference, and an element
    /// is left to the element rules.
    fn behind_ref(&self, b: &Body, p: &Place) -> Option<bool> {
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
                // A box's payload, `*mut T`, is left to a later rule.
                Proj::Deref => match pool.get(strip(pool, t)) {
                    TyData::Ref { counted: true, .. } => return None,
                    TyData::Ref { mutable, .. } => return Some(mutable),
                    _ => {}
                },
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
        match pool.get(strip(pool, t)) {
            TyData::Tuple { elems, .. } => pool.list_items(elems).get(j as usize).copied(),
            TyData::Adt { def, args } => {
                let fields = match (&self.cx.lookup.item(def)?.data, variant) {
                    (ItemData::Data(fs), None) => fs,
                    (ItemData::Enum { shared, variants }, Some(k)) if shared.is_empty() => {
                        &variants.get(k as usize)?.fields
                    }
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
