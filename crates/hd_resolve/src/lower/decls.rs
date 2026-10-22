//! Declaration rules that compare several declarations: module names
//! (`names.module.unique`, `names.tests.unique`), supertrait member names
//! (`trait.super.no-redeclare`, `trait.sealed.member-names`), inherent member names
//! (`trait.inherent.unique-unifying`) and embedded types
//! (`data.struct.embed.struct-only`, `data.embed.depth`, `data.embed.depth.self`). They run once the
//! folder's items are lowered, over every module of the folder; a type
//! or trait from another folder is read from its frozen interface.

use hd_base::{HashMap, HashSet};
use std::borrow::Cow;

use hd_base::{DefId, Span, Symbol};
use hd_diag::{Code, DiagBuf};
use hd_syntax::{NodeRef, SyntaxKind};
use hd_types::TyData;

use super::{Head, ModIn, ModOut, Resolver, member_name};
use crate::header::Universe;
use crate::iface::{HeadKind, ImplKind, Item, ItemData, Names};
use crate::view::Src;

/// The embedding chain limit of `data.embed.depth`.
const MAX_EMBED_DEPTH: usize = 3;

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
    let mut own = HashMap::default();
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
    f.impl_sealed_members(&item, diags);
    f.inherent_members(diags);
    f.embedding(&item, diags);
}

impl Folder<'_, '_> {
    /// A module declares each name once, whatever the kind
    /// (`names.module.unique`, `fn.name.no-overloading`), reported on the
    /// later declaration. The items of a `tests:` block are the scope
    /// builder's (`test_scope_of`).
    fn module_names(&self, diags: &mut DiagBuf) {
        for (m, heads) in self.mods.iter().zip(self.heads) {
            let mut seen = HashSet::default();
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
                    continue;
                }
                self.session_redeclared(m, h, diags);
            }
        }
    }

    /// An input of a session declares no name the session holds already
    /// (`cli.repl.redeclare.binding`, `cli.repl.redeclare.type`), reported
    /// on the declaration's name. Shadowing the prelude is not redeclaring.
    fn session_redeclared(&self, m: &ModIn<'_>, h: &Head<'_>, diags: &mut DiagBuf) {
        let Some(session) = &m.session else { return };
        let Some(&row) = session.scope.index.get(&h.name) else {
            return;
        };
        if session.scope.origin[row as usize] == crate::Origin::Prelude {
            return;
        }
        let Some(by) = session.declared_by.get(&h.name) else {
            return;
        };
        let (code, hint) = if h.kind.is_type() || h.kind == HeadKind::Trait {
            (Code::DuplicateType, "give the new one another name")
        } else {
            (
                Code::DuplicateBinding,
                "give it another name, or bind a `let` and assign it",
            )
        };
        let msg = format!(
            "`{}` is already declared by {by}; {hint}",
            self.names.text(h.name)
        );
        let span = super::head_name_span(&m.src, h);
        diags.error(code, span, &msg);
    }

    /// A child trait declares no member named like a member of any of its
    /// transitive supertraits, reported on the child's member.
    fn supertrait_members<'i>(
        &self,
        item: &dyn Fn(DefId) -> Option<Cow<'i, Item>>,
        diags: &mut DiagBuf,
    ) {
        for (m, heads) in self.mods.iter().zip(self.heads) {
            for h in heads.iter().filter(|h| h.kind == HeadKind::Trait) {
                let Some(it) = self.own.get(&h.def) else {
                    continue;
                };
                let ItemData::Trait(t) = &it.data else {
                    continue;
                };
                let inherited = self.inherited_members(item, h.def, &t.supers);
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
                    if let Some(&sealed) = inherited.get(&self.names.syms.intern(name)) {
                        // `trait.sealed.member-names.error`: a sealed
                        // trait's member has its own error code.
                        let (code, msg) = if sealed {
                            (
                                Code::SealedTraitImplementation,
                                format!(
                                    "`{name}` is a member of a sealed supertrait of `{}`, which the compiler supplies",
                                    self.names.text(h.name)
                                ),
                            )
                        } else {
                            (
                                Code::DuplicateTraitMember,
                                format!(
                                    "`{name}` is already a member of a supertrait of `{}`",
                                    self.names.text(h.name)
                                ),
                            )
                        };
                        diags.error(code, m.src.span(f), &msg);
                    }
                }
            }
        }
    }

    /// The member names of the transitive supertraits `supers` of `owner`,
    /// each with whether a sealed trait declares it.
    fn inherited_members<'i>(
        &self,
        item: &dyn Fn(DefId) -> Option<Cow<'i, Item>>,
        owner: DefId,
        supers: &[hd_types::Ty],
    ) -> HashMap<Symbol, bool> {
        let pool = self.names.pool;
        let mut inherited: HashMap<Symbol, bool> = HashMap::default();
        let mut visited = HashSet::from_iter([owner]);
        let mut todo = supers.to_vec();
        while let Some(s) = todo.pop() {
            let Some((def, _)) = pool.trait_value(s) else {
                continue;
            };
            if !visited.insert(def) {
                continue;
            }
            if let Some(sup) = item(def)
                && let ItemData::Trait(st) = &sup.data
            {
                let sealed = self.names.known.is_sealed(def);
                let names = st
                    .methods
                    .iter()
                    .map(|(n, _)| *n)
                    .chain(st.assoc.iter().map(|(n, _)| *n));
                for n in names {
                    *inherited.entry(n).or_default() |= sealed;
                }
                todo.extend(st.supers.iter().copied());
            }
        }
        inherited
    }

    /// An implementation of a trait with a sealed supertrait writes no
    /// member of that sealed trait (`trait.sealed.member-names.impl`),
    /// reported on the member.
    fn impl_sealed_members<'i>(
        &self,
        item: &dyn Fn(DefId) -> Option<Cow<'i, Item>>,
        diags: &mut DiagBuf,
    ) {
        for (m, heads) in self.mods.iter().zip(self.heads) {
            for h in heads.iter().filter(|h| h.kind == HeadKind::Impl) {
                let Some(Item {
                    data:
                        ItemData::Impl {
                            trait_,
                            kind: ImplKind::Written,
                            ..
                        },
                    ..
                }) = self.own.get(&h.def).copied()
                else {
                    continue;
                };
                let Some(tr) = item(*trait_) else {
                    continue;
                };
                let ItemData::Trait(t) = &tr.data else {
                    continue;
                };
                let inherited = self.inherited_members(item, *trait_, &t.supers);
                for f in Src::child(h.node, SyntaxKind::Block)
                    .into_iter()
                    .flat_map(NodeRef::children)
                    .filter(|c| c.kind() == SyntaxKind::FnDecl)
                {
                    let Some(t) = member_name(&m.src, f) else {
                        continue;
                    };
                    let name = m.src.text(t);
                    if inherited.get(&self.names.syms.intern(name)) == Some(&true) {
                        let msg = format!(
                            "`{name}` is a member of a sealed supertrait, which the compiler supplies"
                        );
                        diags.error(Code::SealedTraitImplementation, m.src.span(f), &msg);
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
        let mut by_name: HashMap<String, Vec<(DefId, Vec<hd_types::Ty>, Span)>> =
            HashMap::default();
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
                    by_name.entry(name).or_default().push((
                        h.def,
                        self.targets(h.def, *self_ty),
                        m.src.span(f),
                    ));
                }
            }
        }
        let u = Universe {
            names: self.names,
            impls: HashMap::default(),
        };
        for name in order {
            let decls = &by_name[&name];
            for (j, (def, tys, span)) in decls.iter().enumerate() {
                let clash = decls[..j].iter().any(|(d, ts, _)| {
                    ts.iter().any(|t| {
                        tys.iter().any(|ty| {
                            let mut sub = HashMap::default();
                            u.unify(*t, *ty, *d, *def, &mut sub)
                        })
                    })
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

    /// The targets an inherent implementation stands for. A target that is a
    /// bare parameter bounded by a trait, as in `impl[N < Integer] N`, is a
    /// numeric family and stands for one implementation per type that
    /// implements the bound (`trait.target.numeric-family.each`), so a
    /// member of it clashes only with a member of one of those types. Any
    /// other target stands for itself.
    fn targets(&self, def: DefId, self_ty: hd_types::Ty) -> Vec<hd_types::Ty> {
        let pool = self.names.pool;
        let TyData::Param(p) = pool.get(self_ty) else {
            return vec![self_ty];
        };
        let bound = self
            .own
            .get(&def)
            .filter(|_| p.owner == def)
            .and_then(|imp| imp.generics.get(usize::from(p.index)))
            .and_then(|g| g.bound);
        let Some(bound) = bound else {
            return vec![self_ty];
        };
        let members: Vec<hd_types::Ty> = self
            .own
            .values()
            .filter_map(|i| match &i.data {
                ItemData::Impl {
                    trait_,
                    self_ty,
                    kind,
                    ..
                } if *trait_ == bound && kind.is_impl() && !pool.has_param(*self_ty) => {
                    Some(*self_ty)
                }
                _ => None,
            })
            .collect();
        if members.is_empty() {
            vec![self_ty]
        } else {
            members
        }
    }

    /// An embedded field names a struct type, generic or not: not an enum, a
    /// newtype, a trait value, a built-in or collection type, a function
    /// type, or a type parameter (`data.struct.embed.struct-only`).
    fn embedded_data_only<'i>(
        &self,
        item: &dyn Fn(DefId) -> Option<Cow<'i, Item>>,
        diags: &mut DiagBuf,
    ) {
        let pool = self.names.pool;
        let known = self.names.known;
        // A data type embeds a data type, and a struct a struct
        // (`data.struct.embed`).
        let is_part = |ty, of_struct: bool| match pool.get(ty) {
            TyData::Adt { def, .. } => {
                def != known.list
                    && def != known.map
                    && item(def).is_none_or(|i| {
                        i.kind() == Some(HeadKind::Data) && i.is_struct == of_struct
                    })
            }
            _ => false,
        };
        for (m, heads) in self.mods.iter().zip(self.heads) {
            for h in heads.iter().filter(|h| h.kind == HeadKind::Data) {
                let Some(Item {
                    data: ItemData::Data(fields),
                    is_struct,
                    ..
                }) = self.own.get(&h.def).copied()
                else {
                    continue;
                };
                let nodes = Src::child(h.node, SyntaxKind::Block)
                    .into_iter()
                    .flat_map(NodeRef::children)
                    .filter(|c| c.kind() == SyntaxKind::EmbeddedField);
                for (f, node) in fields.iter().filter(|f| f.embedded).zip(nodes) {
                    if !pool.has_poison(f.ty) && !is_part(f.ty, *is_struct) {
                        let kind = if *is_struct { "struct" } else { "data type" };
                        let msg = format!(
                            "`{}` is not a {kind}, so it cannot be embedded",
                            self.names.text(f.name)
                        );
                        diags.error(Code::EmbeddedNonData, m.src.span(node), &msg);
                    }
                }
            }
        }
    }

    /// A data type embeds no chain deeper than three levels, and never
    /// itself: a cycle is reported on its first declared type, in place of
    /// the depth error.
    fn embedding<'i>(&self, item: &dyn Fn(DefId) -> Option<Cow<'i, Item>>, diags: &mut DiagBuf) {
        let pool = self.names.pool;
        // The data types embedded by `d`, by embedded field, in order.
        let parts = |d: DefId| -> Vec<Option<DefId>> {
            let Some(it) = item(d) else {
                return Vec::new();
            };
            let ItemData::Data(fields) = &it.data else {
                return Vec::new();
            };
            fields
                .iter()
                .filter(|f| f.embedded)
                .map(|f| match pool.get(f.ty) {
                    TyData::Adt { def, .. }
                        if matches!(item(def).map(|i| i.kind()), Some(Some(HeadKind::Data))) =>
                    {
                        Some(def)
                    }
                    _ => None,
                })
                .collect()
        };
        let reaches = |from: DefId, to: DefId| -> Option<Vec<DefId>> {
            // A path of embedded types from `from` to `to`, if any.
            let mut seen = HashSet::default();
            let mut stack = vec![vec![from]];
            while let Some(path) = stack.pop() {
                let last = path[path.len() - 1];
                if last == to && path.len() > 1 {
                    return Some(path);
                }
                if !seen.insert(last) {
                    continue;
                }
                for p in parts(last).into_iter().rev().flatten() {
                    let mut next = path.clone();
                    next.push(p);
                    stack.push(next);
                }
            }
            None
        };
        self.embedded_data_only(item, diags);
        // The declaration order of every data type of the folder.
        let order: Vec<DefId> = self
            .heads
            .iter()
            .flatten()
            .filter(|h| h.kind == HeadKind::Data)
            .map(|h| h.def)
            .collect();
        let rank = |d: DefId| order.iter().position(|o| *o == d).unwrap_or(usize::MAX);
        let show = |path: &[DefId]| {
            path.iter()
                .map(|d| self.names.display_name(*d))
                .collect::<Vec<_>>()
                .join(" > ")
        };
        for (m, heads) in self.mods.iter().zip(self.heads) {
            for h in heads.iter().filter(|h| h.kind == HeadKind::Data) {
                let embedded = parts(h.def);
                if embedded.is_empty() {
                    continue;
                }
                let nodes: Vec<NodeRef<'_>> = Src::child(h.node, SyntaxKind::Block)
                    .into_iter()
                    .flat_map(NodeRef::children)
                    .filter(|c| c.kind() == SyntaxKind::EmbeddedField)
                    .collect();
                // A cycle through `h`, reported by the first declared type
                // of its members, on the field that begins it.
                let cycle = embedded.iter().enumerate().find_map(|(i, p)| {
                    let path = if *p == Some(h.def) {
                        Some(vec![h.def, h.def])
                    } else {
                        p.and_then(|p| reaches(p, h.def)).map(|mut rest| {
                            rest.insert(0, h.def);
                            rest
                        })
                    };
                    path.map(|p| (i, p))
                });
                if let Some((i, path)) = cycle {
                    let first = path.iter().map(|d| rank(*d)).min().unwrap_or(usize::MAX);
                    if first == rank(h.def)
                        && let Some(node) = nodes.get(i)
                    {
                        let msg = format!(
                            "`{}` embeds itself: {}",
                            self.names.text(h.name),
                            show(&path)
                        );
                        diags.error(Code::EmbeddingCycle, m.src.span(*node), &msg);
                    }
                    continue;
                }
                // The longest chain below `h`; a type on a cycle cuts it.
                let deepest = |from: DefId| -> Vec<DefId> {
                    fn go(
                        d: DefId,
                        parts: &dyn Fn(DefId) -> Vec<Option<DefId>>,
                        path: &mut Vec<DefId>,
                    ) -> Vec<DefId> {
                        let mut best = vec![d];
                        path.push(d);
                        for p in parts(d).into_iter().flatten() {
                            if path.contains(&p) {
                                continue;
                            }
                            let mut chain = vec![d];
                            chain.extend(go(p, parts, path));
                            if chain.len() > best.len() {
                                best = chain;
                            }
                        }
                        path.pop();
                        best
                    }
                    go(from, &parts, &mut Vec::new())
                };
                let too_deep = embedded.iter().enumerate().find_map(|(i, p)| {
                    let mut via = vec![h.def];
                    via.extend(deepest((*p)?));
                    (via.len() > MAX_EMBED_DEPTH + 1).then_some((i, via))
                });
                if let Some((i, chain)) = too_deep
                    && let Some(node) = nodes.get(i)
                {
                    let msg = format!(
                        "`{}` reaches an embedded part at depth {}, past the limit of {MAX_EMBED_DEPTH}: {}",
                        self.names.text(h.name),
                        chain.len() - 1,
                        show(&chain)
                    );
                    diags.error(Code::EmbeddingTooDeep, m.src.span(*node), &msg);
                }
            }
        }
    }
}
