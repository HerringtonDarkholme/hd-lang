//! Declaration rules that compare several declarations: module names
//! (`names.module.unique`, `names.tests.unique`), supertrait member names
//! (`trait.super.no-redeclare`), inherent member names
//! (`trait.inherent.unique-unifying`). They run once the
//! folder's items are lowered, over every module of the folder; a type
//! or trait from another folder is read from its frozen interface.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use hd_base::{DefId, Span};
use hd_diag::{Code, DiagBuf};
use hd_syntax::{NodeRef, SyntaxKind};
use hd_types::TyData;

use super::{Head, ModIn, ModOut, Resolver, member_name};
use crate::header::Universe;
use crate::iface::{HeadKind, ImplKind, Item, ItemData, Names};
use crate::view::Src;

/// The folder's modules with their heads and lowered items.
struct Folder<'a, 't> {
    names: Names<'a>,
    mods: &'a [ModIn<'t>],
    heads: &'a [Vec<Head<'t>>],
    /// Every lowered item of the folder, by definition; the first of a
    /// repeated declaration wins.
    own: HashMap<DefId, &'a Item>,
}

/// Reports the cross-declaration rules of one folder.
pub(super) fn check(
    r: &Resolver<'_, '_>,
    mods: &[ModIn<'_>],
    heads: &[Vec<Head<'_>>],
    out: &[ModOut],
    diags: &mut DiagBuf,
) {
    let mut own = HashMap::new();
    for it in out.iter().flat_map(|m| &m.items) {
        own.entry(it.def).or_insert(it);
    }
    let f = Folder {
        names: r.cx.names,
        mods,
        heads,
        own,
    };
    let item = |d: DefId| -> Option<Cow<'_, Item>> {
        f.own
            .get(&d)
            .map(|i| Cow::Borrowed(*i))
            .or_else(|| r.item(d).map(Cow::Owned))
    };
    f.module_names(diags);
    f.supertrait_members(&item, diags);
    f.inherent_members(diags);
}

impl Folder<'_, '_> {
    /// A module declares each name once, whatever the kind
    /// (`names.module.unique`, `fn.name.no-overloading`), reported on the
    /// later declaration. The items of a `tests:` block are the scope
    /// builder's (`test_scope_of`).
    fn module_names(&self, diags: &mut DiagBuf) {
        for (m, heads) in self.mods.iter().zip(self.heads) {
            let mut seen = HashSet::new();
            for h in heads
                .iter()
                .filter(|h| h.kind != HeadKind::Impl && h.local.is_none() && h.test.is_none())
            {
                if !seen.insert(h.name) {
                    let msg = format!(
                        "`{}` is already declared in this module",
                        self.names.text(h.name)
                    );
                    diags.error(Code::DuplicateModuleName, m.src.span(h.node), &msg);
                }
            }
        }
    }

    /// A child trait declares no member named like a member of any of its
    /// transitive supertraits, reported on the child's member.
    fn supertrait_members<'i>(
        &self,
        item: &dyn Fn(DefId) -> Option<Cow<'i, Item>>,
        diags: &mut DiagBuf,
    ) {
        let pool = self.names.pool;
        for (m, heads) in self.mods.iter().zip(self.heads) {
            for h in heads.iter().filter(|h| h.kind == HeadKind::Trait) {
                let Some(it) = self.own.get(&h.def) else {
                    continue;
                };
                let ItemData::Trait(t) = &it.data else {
                    continue;
                };
                // The member names of the transitive supertraits.
                let mut inherited = HashSet::new();
                let mut visited = HashSet::from([h.def]);
                let mut todo: Vec<_> = t.supers.clone();
                while let Some(s) = todo.pop() {
                    let TyData::TraitValue { def, .. } = pool.get(s) else {
                        continue;
                    };
                    if !visited.insert(def) {
                        continue;
                    }
                    if let Some(sup) = item(def)
                        && let ItemData::Trait(st) = &sup.data
                    {
                        inherited.extend(st.methods.iter().map(|(n, _)| *n));
                        inherited.extend(st.assoc.iter().map(|(n, _)| *n));
                        todo.extend(st.supers.iter().copied());
                    }
                }
                if inherited.is_empty() {
                    continue;
                }
                for f in Src::child(h.node, SyntaxKind::Block)
                    .into_iter()
                    .flat_map(NodeRef::children)
                    .filter(|c| {
                        matches!(
                            c.kind(),
                            SyntaxKind::FnDecl | SyntaxKind::AssociatedTypeDecl
                        )
                    })
                {
                    let Some(t) = member_name(&m.src, f) else {
                        continue;
                    };
                    let name = m.src.text(t);
                    if inherited.contains(&self.names.syms.intern(name)) {
                        let msg = format!(
                            "`{name}` is already a member of a supertrait of `{}`",
                            self.names.text(h.name)
                        );
                        diags.error(Code::DuplicateTraitMember, m.src.span(f), &msg);
                    }
                }
            }
        }
    }

    /// Two inherent members with one name are an error when their
    /// implementations' targets unify, two members of one implementation
    /// included. The later member is reported.
    fn inherent_members(&self, diags: &mut DiagBuf) {
        // By member name: each declaration's implementation, target and place.
        let mut by_name: HashMap<String, Vec<(DefId, hd_types::Ty, Span)>> = HashMap::new();
        let mut order = Vec::new();
        for (m, heads) in self.mods.iter().zip(self.heads) {
            for h in heads.iter().filter(|h| h.kind == HeadKind::Impl) {
                let Some(Item {
                    data:
                        ItemData::Impl {
                            trait_,
                            self_ty,
                            kind: ImplKind::Written,
                            ..
                        },
                    ..
                }) = self.own.get(&h.def).copied()
                else {
                    continue;
                };
                if *trait_ != DefId::NONE {
                    continue;
                }
                for f in Src::child(h.node, SyntaxKind::Block)
                    .into_iter()
                    .flat_map(NodeRef::children)
                    .filter(|c| c.kind() == SyntaxKind::FnDecl)
                {
                    let Some(t) = member_name(&m.src, f) else {
                        continue;
                    };
                    let name = m.src.text(t).to_owned();
                    if !by_name.contains_key(&name) {
                        order.push(name.clone());
                    }
                    by_name
                        .entry(name)
                        .or_default()
                        .push((h.def, *self_ty, m.src.span(f)));
                }
            }
        }
        let u = Universe {
            names: self.names,
            impls: HashMap::new(),
        };
        for name in order {
            let decls = &by_name[&name];
            for (j, (def, ty, span)) in decls.iter().enumerate() {
                let clash = decls[..j].iter().any(|(d, t, _)| {
                    let mut sub = HashMap::new();
                    u.unify(*t, *ty, *d, *def, &mut sub)
                });
                if clash {
                    let msg = format!(
                        "`{name}` is already an inherent member of a type this one unifies with"
                    );
                    diags.error(Code::DuplicateInherentMember, *span, &msg);
                }
            }
        }
    }
}
