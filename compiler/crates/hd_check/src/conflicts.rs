//! Promotion conflicts (spec 03 "Hiding And Conflicts", "Conflicts Are
//! Declaration Errors"): the members of a data type are its own fields and
//! inherent methods at depth 0 and the `pub` ones of every embedded part at
//! the part's depth. A part's embedded fields count as its members, so a
//! type embedded through two paths at one depth conflicts.
//!
//! Two members with one name at the smallest depth are a conflict reported
//! on the later embedded field of the declaration. A private own member
//! with the name of a promoted member is a conflict reported on that
//! member.

use std::collections::HashMap;

use hd_base::{DefId, Symbol};
use hd_resolve::{ImplKind, Item, ItemData, Lookup, Names};
use hd_types::TyData;

/// The deepest chain of embedded parts (`data.embed.depth`).
const MAX_DEPTH: usize = 3;

/// The inherent methods of each data type: name and method item.
pub type Inherent = HashMap<DefId, Vec<(Symbol, DefId)>>;

/// One conflict: where to report it, and the message.
pub struct Conflict {
    /// The data type, or the private method that is reported.
    pub item: DefId,
    /// The field of `item` (its index in the declaration); `None` reports
    /// `item` itself, a method.
    pub field: Option<usize>,
    pub message: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Ns {
    Field,
    Method,
}

struct Member {
    depth: usize,
    /// The embedded fields (their index in the declaration) from the root.
    path: Vec<usize>,
    /// The embedded field names from the root, then the member's name.
    shown: String,
    /// A private own member: the field's index or the method's item.
    private: Option<Own>,
}

#[derive(Clone, Copy)]
enum Own {
    Field(usize),
    Method(DefId),
}

/// The inherent methods of every data type visible from `lookup`.
#[must_use]
pub fn inherent_methods(names: &Names<'_>, lookup: &Lookup<'_>) -> Inherent {
    let pool = names.pool;
    let mut out = Inherent::new();
    let all = lookup
        .own
        .iter()
        .chain(lookup.ifaces.iter().flat_map(|f| f.items.iter()));
    for it in all {
        if let ItemData::Impl {
            trait_,
            self_ty,
            methods,
            kind: ImplKind::Written,
            ..
        } = &it.data
            && *trait_ == DefId::NONE
            && let TyData::Adt { def, .. } = pool.get(*self_ty)
        {
            out.entry(def).or_default().extend(methods.iter().copied());
        }
    }
    out
}

/// The conflicts of one data declaration's members.
#[must_use]
pub fn promotion_conflicts(
    names: &Names<'_>,
    lookup: &Lookup<'_>,
    inherent: &Inherent,
    it: &Item,
) -> Vec<Conflict> {
    let mut out = Vec::new();
    let ItemData::Data(root_fields) = &it.data else {
        return out;
    };
    let pool = names.pool;
    let root = names.text(it.name).to_owned();
    let mut table: HashMap<(Ns, Symbol), Vec<Member>> = HashMap::new();
    // Parts at the current depth: (type, field indices, shown path).
    let mut level: Vec<(DefId, Vec<usize>, String)> = vec![(it.def, Vec::new(), root.clone())];
    for depth in 0..=MAX_DEPTH {
        let mut next = Vec::new();
        for (def, path, shown) in &level {
            let fields = if *def == it.def {
                root_fields
            } else if let Some(Item {
                data: ItemData::Data(fields),
                ..
            }) = lookup.item(*def)
            {
                fields
            } else {
                continue;
            };
            for (i, f) in fields.iter().enumerate() {
                let visible = f.public || f.embedded;
                let name = names.text(f.name);
                if depth == 0 || visible {
                    table.entry((Ns::Field, f.name)).or_default().push(Member {
                        depth,
                        path: path.clone(),
                        shown: format!("{shown}.{name}"),
                        private: (depth == 0 && !visible).then_some(Own::Field(i)),
                    });
                }
                if f.embedded
                    && let TyData::Adt { def: part, .. } = pool.get(f.ty)
                {
                    let mut p = path.clone();
                    p.push(i);
                    next.push((part, p, format!("{shown}.{name}")));
                }
            }
            for (name, m) in inherent.get(def).into_iter().flatten() {
                let public = lookup.item(*m).is_some_and(|m| m.public);
                if depth == 0 || public {
                    table.entry((Ns::Method, *name)).or_default().push(Member {
                        depth,
                        path: path.clone(),
                        shown: format!("{shown}.{}", names.text(*name)),
                        private: (depth == 0 && !public).then_some(Own::Method(*m)),
                    });
                }
            }
        }
        level = next;
    }
    let mut keys: Vec<_> = table.keys().copied().collect();
    // Content order (scheduler.md §6.5): by name text, not symbol id.
    keys.sort_by_cached_key(|(ns, n)| (*ns as u8, names.text(*n).to_owned()));
    for key in keys {
        let members = &table[&key];
        let name = names.text(key.1);
        if let Some(own) = members.iter().find_map(|m| m.private)
            && let Some(promoted) = members.iter().find(|m| m.depth > 0)
        {
            let (item, field) = match own {
                Own::Field(i) => (it.def, Some(i)),
                Own::Method(m) => (m, None),
            };
            out.push(Conflict {
                item,
                field,
                message: format!(
                    "the private member `{root}.{name}` is also promoted as `{}`; rename the private member",
                    promoted.shown
                ),
            });
            continue;
        }
        let least = members.iter().map(|m| m.depth).min().unwrap_or(0);
        if least == 0 {
            continue;
        }
        let at: Vec<&Member> = members.iter().filter(|m| m.depth == least).collect();
        let later = at.iter().filter_map(|m| m.path.first().copied()).max();
        let earlier = at.iter().filter_map(|m| m.path.first().copied()).min();
        if at.len() < 2 || later == earlier {
            continue;
        }
        let (Some(later), Some(first), Some(second)) = (
            later,
            at.iter().find(|m| m.path.first().copied() == earlier),
            at.iter().find(|m| m.path.first().copied() == later),
        ) else {
            continue;
        };
        out.push(Conflict {
            item: it.def,
            field: Some(later),
            message: format!(
                "`{}` and `{}` are both named `{name}` at depth {least}; hide them with a `pub` member of `{root}`",
                first.shown, second.shown
            ),
        });
    }
    out
}
