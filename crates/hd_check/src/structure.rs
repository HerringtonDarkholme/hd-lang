//! A derivation's `Structure` at check time (annotations, "The Structure
//! Trait", "Facts", "Members And Variants", "Self References";
//! `crates/hd_mono/DESIGN.md` §12.3 "Derives"): the target's member list, built from the
//! data type or enum and the derivation's omitted members, for the one
//! generator in `hd_structure`, which writes the bodies as hidden methods
//! of the `Structure`'s owner (`OptIn::structure_owner`); and the bodies
//! that build each `Facts` the derivation sees. Collection calls the same
//! generator for a tuple's `Structure` (`crates/hd_mono/DESIGN.md` §13.6).
//!
//! The declaration's `Structure` is shared: its methods, handles and facts
//! bodies are hidden methods of the data type or enum, generated once per
//! module, and every derivation without member lines calls them. A
//! derivation whose block has member lines gets its own, under the
//! derivation; a slot its lines do not edit reuses the declaration's facts
//! body.
//!
//! A typed fact in a facts body is checked against its member's type
//! (`annot.typed-fact.check`), and holds the value at the fact type that
//! type infers, which a handle's `fact` finds (`annot.handle.fact.exact`).
//!
//! A member line's list that is not a list literal, and a spread in one, is
//! evaluated with the place's other facts when the facts body runs, `=`
//! having dropped the facts before it at check time and `+=` keeping them
//! (`annot.line.right-typed`). Its facts are not known to the checker, so a
//! repeated type among them is not reported (`annot.line.duplicate`).
//!
//! Not generated yet, a structured "not implemented" that leaves the
//! derivation's `Structure` without bodies (a call of it then stops at
//! collection): the shared constructor data of a generic enum.

use hd_base::{DefId, Stage, StageResult};
use hd_diag::DiagBuf;
use hd_resolve::{Item, ItemData};
use hd_structure::{Mem, Shape, Shared, Target, Var, hidden_method};
use hd_syntax::{NodeRef, SyntaxKind};
use hd_tir::Body;
use hd_types::{ParamRef, Ty, TyData, TyList};

use crate::body::{BodyCx, unsupported};
use crate::call::subst_owner;
use crate::derive::{MemberLine, OptIn};
use crate::facts::{FactSource, FactTarget, check_fact_list, fact_lines};

pub use hd_structure::Generated;

/// The bodies that build the `Facts` a derivation sees, per slot: `None`
/// where it sees no fact.
#[derive(Clone, Debug, Default)]
pub struct FactBodies {
    /// `T::facts()`.
    pub of_type: Option<DefId>,
    /// Per variant, in declaration order.
    pub variants: Vec<Option<DefId>>,
    /// Per variant, per member position.
    pub members: Vec<Vec<Option<DefId>>>,
}

/// One place a fact attaches to (`annot.fact.*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Type,
    Variant(usize),
    Member(usize, usize),
}

/// The facts a derivation sees, per slot, as their sources in order.
#[derive(Clone, Default)]
pub struct FactLines<'t> {
    of_type: Vec<FactSource<'t>>,
    variants: Vec<Vec<FactSource<'t>>>,
    members: Vec<Vec<Vec<FactSource<'t>>>>,
    /// Per variant, per member position: the member's declared type, which
    /// a typed fact checks against (`annot.typed-fact.declared-type`).
    member_tys: Vec<Vec<Ty>>,
    /// What the type's, each variant's and each member's facts are attached
    /// to, as `Shape`, `Shape.Circle` and `Shape.Circle.radius`, which a
    /// panic in a fact names (`annot.fact.eval.panic-runtime`).
    type_label: String,
    variant_labels: Vec<String>,
    member_labels: Vec<Vec<String>>,
}

impl<'t> FactLines<'t> {
    fn slot(&mut self, s: Slot) -> Option<&mut Vec<FactSource<'t>>> {
        match s {
            Slot::Type => Some(&mut self.of_type),
            Slot::Variant(v) => self.variants.get_mut(v),
            Slot::Member(v, m) => self.members.get_mut(v)?.get_mut(m),
        }
    }

    /// The member types written over the type's own parameters, moved to
    /// a derivation's scope: the data type or enum `data` at `args`.
    pub fn retarget(&mut self, cx: &BodyCx<'_>, data: DefId, args: &[Ty]) {
        let pool = cx.names.pool.types();
        for t in self.member_tys.iter_mut().flatten() {
            *t = subst_owner(pool, data, args, *t);
        }
    }

    /// The target a slot's typed facts check against.
    pub(crate) fn target(&self, s: Slot) -> FactTarget {
        match s {
            Slot::Member(v, m) => self
                .member_tys
                .get(v)
                .and_then(|ms| ms.get(m))
                .map_or(FactTarget::Other, |t| FactTarget::Field(*t)),
            Slot::Type | Slot::Variant(_) => FactTarget::Other,
        }
    }

    /// The label of the place a slot's facts attach to.
    fn label(&self, s: Slot) -> &str {
        let found = match s {
            Slot::Type => None,
            Slot::Variant(v) => self.variant_labels.get(v),
            Slot::Member(v, m) => self.member_labels.get(v).and_then(|ms| ms.get(m)),
        };
        found.map_or(self.type_label.as_str(), String::as_str)
    }

    pub(crate) fn slots(&self) -> Vec<(Slot, &[FactSource<'t>])> {
        let mut out = vec![(Slot::Type, self.of_type.as_slice())];
        for (v, fs) in self.variants.iter().enumerate() {
            out.push((Slot::Variant(v), fs.as_slice()));
        }
        for (v, ms) in self.members.iter().enumerate() {
            for (m, fs) in ms.iter().enumerate() {
                out.push((Slot::Member(v, m), fs.as_slice()));
            }
        }
        out
    }
}

/// The member nodes of a data type, or the variant nodes of an enum, in
/// declaration order, which their item's fields and variants follow.
pub(crate) fn member_nodes(decl: NodeRef<'_>) -> Vec<NodeRef<'_>> {
    hd_resolve::Src::child(decl, SyntaxKind::Block)
        .into_iter()
        .flat_map(NodeRef::children)
        .filter(|c| {
            matches!(
                c.kind(),
                SyntaxKind::DataField
                    | SyntaxKind::Parameter
                    | SyntaxKind::EmbeddedField
                    | SyntaxKind::EnumVariant
            )
        })
        .collect()
}

/// Per variant of the data type or enum `item`, declared at `decl`: the
/// variant's node (`None` for a data type's one variant, which has no
/// facts, `annot.variant.data-facts`), and each member's node with its
/// declared type, in position order.
type MemberSites<'t> = Vec<(Option<NodeRef<'t>>, Vec<(NodeRef<'t>, Ty)>)>;

fn member_sites<'t>(item: &Item, decl: NodeRef<'t>) -> MemberSites<'t> {
    let nodes = member_nodes(decl);
    match &item.data {
        ItemData::Data(fields) => {
            let members = nodes
                .into_iter()
                .zip(fields)
                .map(|(n, f)| (n, f.ty))
                .collect();
            vec![(None, members)]
        }
        ItemData::Enum { variants, .. } => nodes
            .into_iter()
            .zip(variants)
            .map(|(v, var)| {
                let members = hd_resolve::Src::child(v, SyntaxKind::ParameterList)
                    .into_iter()
                    .flat_map(NodeRef::children)
                    .filter(|c| c.kind() == SyntaxKind::Parameter)
                    .zip(&var.fields)
                    .map(|(n, f)| (n, f.ty))
                    .collect();
                (Some(v), members)
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The fact expressions of the declaration `node` of `owner` that attach
/// to a member, with the member's declared type in `owner`'s scope: its
/// decorators, for a data type or enum, and the elements of its member
/// lines, for a derivation block (`annot.typed-fact.targets`,
/// `annot.typed-fact.check`). Each is marked when it is one decorator's
/// value rather than a list element.
#[must_use]
pub fn member_fact_sites<'t>(
    cx: &BodyCx<'t>,
    owner: DefId,
    node: NodeRef<'t>,
) -> Vec<(NodeRef<'t>, Ty, bool)> {
    let src = &cx.src;
    let Some(item) = cx.lookup.item(owner) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let ItemData::Impl {
        by: Some(_),
        self_ty,
        ..
    } = &item.data
    {
        let TyData::Adt { def: data, args } = cx.names.pool.get(*self_ty) else {
            return out;
        };
        let Some(target) = cx.lookup.item(data) else {
            return out;
        };
        let args = cx.names.pool.list_items(args).to_vec();
        for line in crate::derive::member_lines(src, node) {
            let Some(list) = line.list.filter(|l| l.kind() == SyntaxKind::ListExpr) else {
                continue;
            };
            let Some(Slot::Member(v, m)) = line_slot(cx, target, line.name) else {
                continue;
            };
            let ty = match &target.data {
                ItemData::Data(fields) if v == 0 => fields.get(m).map(|f| f.ty),
                _ => None,
            };
            let Some(ty) = ty else { continue };
            let ty = subst_owner(cx.names.pool.types(), data, &args, ty);
            out.extend(
                list.children()
                    .filter(|e| e.kind() != SyntaxKind::SpreadExpr)
                    .map(|e| (e, ty, false)),
            );
        }
        return out;
    }
    for (_, members) in member_sites(item, node) {
        for (n, ty) in members {
            out.extend(fact_lines(cx, n).into_iter().map(|e| (e, ty, true)));
        }
    }
    out
}

/// The declaration facts of the data type or enum `item`, declared at
/// `decl` (`annot.fact.type-level-decorator`, `annot.fact.member-metadata`,
/// `annot.fact.payload`), edited by its derivation block with no trait,
/// `traitless` (`annot.no-trait.after-decorators`, `annot.no-trait.self`).
#[must_use]
pub fn declaration_facts<'t>(
    cx: &BodyCx<'t>,
    item: &Item,
    decl: NodeRef<'t>,
    traitless: Option<NodeRef<'t>>,
) -> FactLines<'t> {
    let src = &cx.src;
    let values = |n: NodeRef<'t>| -> Vec<FactSource<'t>> {
        fact_lines(cx, n)
            .into_iter()
            .map(FactSource::Value)
            .collect()
    };
    let mut out = FactLines {
        of_type: values(decl),
        type_label: cx.names.text(item.name).to_owned(),
        ..FactLines::default()
    };
    (out.variant_labels, out.member_labels) = place_labels(cx, item, &out.type_label);
    for (variant, members) in member_sites(item, decl) {
        out.variants.push(variant.map(values).unwrap_or_default());
        out.member_tys
            .push(members.iter().map(|(_, t)| *t).collect());
        out.members
            .push(members.into_iter().map(|(n, _)| values(n)).collect());
    }
    if let Some(block) = traitless {
        edit_facts(cx, item, &mut out, &crate::derive::member_lines(src, block));
    }
    out
}

/// The facts that the member lines of the derivation block `node`, the
/// implementation `owner`, write by themselves: its target's slots with no
/// declaration facts, then its lines, in `owner`'s scope. A line whose
/// right side never has a list type is left out, as its form is already an
/// error. `None` when `owner` is not a derivation block of a data type or
/// enum.
#[must_use]
pub fn block_facts<'t>(cx: &BodyCx<'t>, owner: DefId, node: NodeRef<'t>) -> Option<FactLines<'t>> {
    block_view(cx, owner, node, true).map(|(facts, _)| facts)
}

/// The facts that the member lines of the derivation block `node` write
/// by themselves, as [`block_facts`] but with a `=` line dropping the
/// lines before it (`annot.line.replace`); also the slots such a line
/// replaces, whose earlier facts the block drops.
#[must_use]
pub fn layer_facts<'t>(
    cx: &BodyCx<'t>,
    owner: DefId,
    node: NodeRef<'t>,
) -> Option<(FactLines<'t>, Vec<Slot>)> {
    block_view(cx, owner, node, false)
}

fn block_view<'t>(
    cx: &BodyCx<'t>,
    owner: DefId,
    node: NodeRef<'t>,
    keep_all: bool,
) -> Option<(FactLines<'t>, Vec<Slot>)> {
    let pool = cx.names.pool;
    let ItemData::Impl {
        by: Some(_),
        self_ty,
        ..
    } = &cx.lookup.item(owner)?.data
    else {
        return None;
    };
    let TyData::Adt { def: data, args } = pool.get(*self_ty) else {
        return None;
    };
    let target = cx.lookup.item(data)?;
    let tys: Vec<Vec<Ty>> = match &target.data {
        ItemData::Data(fields) => vec![fields.iter().map(|f| f.ty).collect()],
        ItemData::Enum { variants, .. } => variants
            .iter()
            .map(|v| v.fields.iter().map(|f| f.ty).collect())
            .collect(),
        _ => return None,
    };
    let mut out = FactLines {
        variants: vec![Vec::new(); tys.len()],
        members: tys.iter().map(|t| vec![Vec::new(); t.len()]).collect(),
        member_tys: tys,
        ..FactLines::default()
    };
    let lines: Vec<MemberLine<'t>> = crate::derive::member_lines(&cx.src, node)
        .into_iter()
        .filter(|l| {
            l.list
                .is_none_or(|list| !hd_resolve::facts::never_a_list(list.kind()))
        })
        .collect();
    let replaced: Vec<Slot> = lines
        .iter()
        .filter(|l| !l.extend && l.list.is_some())
        .filter_map(|l| line_slot(cx, target, l.name))
        .collect();
    // With `keep_all`, every line is kept, so a `=` line does not drop the
    // lines before it.
    let lines: Vec<MemberLine<'t>> = lines
        .into_iter()
        .map(|l| MemberLine {
            extend: keep_all || l.extend,
            ..l
        })
        .collect();
    edit_facts(cx, target, &mut out, &lines);
    out.retarget(cx, data, pool.list_items(args));
    Some((out, if keep_all { Vec::new() } else { replaced }))
}

/// The facts left when the member `lines` of a block in another module of
/// a REPL session edit the facts that `base` builds for the data type or
/// enum `item`, as within one module (`annot.no-trait.after-decorators`,
/// `annot.no-trait.then-blocks`, `annot.line.extend`). A slot holds a call
/// of its base body, at `args`, where it holds the declaration's own
/// sources within one module: those are nodes of another source. The
/// member types are over `item`'s own parameters. Returns the slots the
/// lines edit.
#[must_use]
pub fn layered_facts<'t>(
    cx: &BodyCx<'_>,
    item: &Item,
    (base, args): (&FactBodies, TyList),
    lines: &[MemberLine<'t>],
) -> (FactLines<'t>, Vec<Slot>) {
    let tys: Vec<Vec<Ty>> = match &item.data {
        ItemData::Data(fields) => vec![fields.iter().map(|f| f.ty).collect()],
        ItemData::Enum { variants, .. } => variants
            .iter()
            .map(|v| v.fields.iter().map(|f| f.ty).collect())
            .collect(),
        _ => Vec::new(),
    };
    let call = |def: &Option<DefId>| -> Vec<FactSource<'t>> {
        def.iter().map(|d| FactSource::Body(*d, args)).collect()
    };
    let mut out = FactLines {
        of_type: call(&base.of_type),
        variants: (0..tys.len())
            .map(|v| call(base.variants.get(v).unwrap_or(&None)))
            .collect(),
        members: tys
            .iter()
            .enumerate()
            .map(|(v, ms)| {
                (0..ms.len())
                    .map(|m| call(base.members.get(v).and_then(|b| b.get(m)).unwrap_or(&None)))
                    .collect()
            })
            .collect(),
        member_tys: tys,
        type_label: cx.names.text(item.name).to_owned(),
        ..FactLines::default()
    };
    (out.variant_labels, out.member_labels) = place_labels(cx, item, &out.type_label);
    let edited = edit_facts(cx, item, &mut out, lines);
    (out, edited)
}

/// The labels of the variants and members of the data type or enum `item`
/// named `ty`, in declaration order.
fn place_labels(cx: &BodyCx<'_>, item: &Item, ty: &str) -> (Vec<String>, Vec<Vec<String>>) {
    let names = &cx.names;
    let label =
        |parent: &str, field: &hd_resolve::Field| format!("{parent}.{}", names.text(field.name));
    match &item.data {
        ItemData::Data(fields) => (
            vec![ty.to_owned()],
            vec![fields.iter().map(|f| label(ty, f)).collect()],
        ),
        ItemData::Enum { variants, .. } => {
            let labels: Vec<String> = variants
                .iter()
                .map(|v| format!("{ty}.{}", names.text(v.name)))
                .collect();
            let members = variants
                .iter()
                .zip(&labels)
                .map(|(v, parent)| v.fields.iter().map(|f| label(parent, f)).collect())
                .collect();
            (labels, members)
        }
        _ => (Vec::new(), Vec::new()),
    }
}

/// The slot a member line named `name` edits in `item`: `Self`, a field
/// of a data type, or a variant of an enum.
fn line_slot(cx: &BodyCx<'_>, item: &Item, name: &str) -> Option<Slot> {
    let names = &cx.names;
    if name == "Self" {
        return Some(Slot::Type);
    }
    match &item.data {
        ItemData::Data(fields) => fields
            .iter()
            .position(|f| {
                let n = names.text(f.name);
                n == name || name.strip_prefix('_') == Some(n)
            })
            .map(|m| Slot::Member(0, m)),
        ItemData::Enum { variants, .. } => variants
            .iter()
            .position(|v| names.text(v.name) == name)
            .map(Slot::Variant),
        _ => None,
    }
}

/// Applies member lines to `facts` (`annot.line.extend`,
/// `annot.line.replace`, `annot.line.self`); an omit line edits no fact.
/// Returns the slots the lines edit.
pub fn edit_facts<'t>(
    cx: &BodyCx<'_>,
    item: &Item,
    facts: &mut FactLines<'t>,
    lines: &[MemberLine<'t>],
) -> Vec<Slot> {
    let mut edited = Vec::new();
    for line in lines {
        let Some(list) = line.list else { continue };
        let Some(slot) = line_slot(cx, item, line.name) else {
            continue;
        };
        let Some(sources) = facts.slot(slot) else {
            continue;
        };
        if !line.extend {
            sources.clear();
        }
        sources.push(FactSource::List(list));
        if !edited.contains(&slot) {
            edited.push(slot);
        }
    }
    edited
}

/// The facts bodies of `facts`, hidden methods of `owner` with no
/// parameter, named after their slots with `prefix` before, checked in
/// `cx` and reported at `decl` (`annot.fact.read`). With `only`, a slot
/// outside its edited slots reuses its base's body; without, every slot
/// with a fact gets one. A fact that does not check declines them all.
pub fn fact_bodies(
    cx: &BodyCx<'_>,
    (owner, prefix): (DefId, &str),
    (facts, decl): (&FactLines<'_>, NodeRef<'_>),
    only: Option<(&[Slot], &FactBodies)>,
) -> StageResult<(FactBodies, Vec<Body>)> {
    let names = &cx.names;
    let mut out = FactBodies {
        of_type: None,
        variants: vec![None; facts.variants.len()],
        members: facts
            .members
            .iter()
            .map(|ms| vec![None; ms.len()])
            .collect(),
    };
    let mut bodies = Vec::new();
    for (slot, sources) in facts.slots() {
        let (name, base) = match slot {
            Slot::Type => (
                format!("{prefix}type facts"),
                only.and_then(|o| o.1.of_type),
            ),
            Slot::Variant(v) => (
                format!("{prefix}variant facts {v}"),
                only.and_then(|o| o.1.variants.get(v).copied().flatten()),
            ),
            Slot::Member(v, m) => (
                format!("{prefix}member facts {v} {m}"),
                only.and_then(|o| o.1.members.get(v)?.get(m).copied().flatten()),
            ),
        };
        let def = match only {
            Some((edited, _)) if !edited.contains(&slot) => base,
            _ if sources.is_empty() => None,
            _ => {
                let def = hidden_method(names, owner, &name);
                let mut scratch = DiagBuf::default();
                let body = check_fact_list(
                    cx,
                    (owner, def),
                    sources,
                    (facts.target(slot), facts.label(slot)),
                    decl,
                    &mut scratch,
                )?;
                if scratch.has_errors() {
                    return unsupported("a derivation fact that does not check");
                }
                bodies.push(body);
                Some(def)
            }
        };
        match slot {
            Slot::Type => out.of_type = def,
            Slot::Variant(v) => out.variants[v] = def,
            Slot::Member(v, m) => out.members[v][m] = def,
        }
    }
    Ok((out, bodies))
}

/// The `Structure` bodies of `opt`, hidden methods of its
/// `structure_owner`, whose facts `facts` builds. `cx` sees the target's
/// module and `std.structure`.
pub fn structure_bodies(
    cx: &BodyCx<'_>,
    opt: &OptIn,
    facts: &FactBodies,
) -> StageResult<Generated> {
    let names = &cx.names;
    let pool = names.pool;
    let Some(item) = cx.lookup.item(opt.data) else {
        return unsupported("a derivation target outside the module's view");
    };
    let owner = opt.structure_owner();
    // The declaration's `Structure` is over the type's own parameters.
    let ty = if owner == opt.data {
        let own: Vec<Ty> = (0..item.generics.len())
            .map(|i| param(cx, opt.data, i))
            .collect();
        pool.intern_ty(&TyData::Adt {
            def: opt.data,
            args: pool.list(&own),
        })
    } else {
        opt.target
    };
    let target = Target {
        ty,
        name: names.text(item.name).to_owned(),
        vars: variants(cx, opt, ty, facts)?,
        facts: facts.of_type,
    };
    let gcx = hd_structure::Cx {
        names,
        items: cx.lookup,
        stage: Stage::Body,
    };
    hd_structure::derivation(&gcx, &target, owner)
}

/// The type parameter `index` of `owner`.
fn param(cx: &BodyCx<'_>, owner: DefId, index: usize) -> Ty {
    let index = u16::try_from(index).unwrap_or(u16::MAX);
    let row = cx
        .lookup
        .item(owner)
        .and_then(|item| item.generics.get(usize::from(index)))
        .is_some_and(|generic| generic.row);
    if row {
        cx.names
            .pool
            .intern_ty(&TyData::Row(cx.names.pool.row(&hd_types::RowData {
                keys: Vec::new(),
                params: vec![hd_types::RowParamRef { owner, index }],
            })))
    } else {
        cx.names
            .pool
            .intern_ty(&TyData::Param(ParamRef { owner, index }))
    }
}

/// `owner`'s own type parameters, as the arguments of a body over them.
#[must_use]
pub fn own_args(cx: &BodyCx<'_>, owner: DefId) -> TyList {
    let n = cx.lookup.item(owner).map_or(0, |i| i.generics.len());
    let own: Vec<Ty> = (0..n).map(|i| param(cx, owner, i)).collect();
    cx.names.pool.list(&own)
}

/// The arguments of the `Structure` owner's parameters in `opt`'s
/// instance: the derivation's own parameters, or the target's arguments
/// for the declaration's `Structure`.
#[must_use]
pub fn owner_args(cx: &BodyCx<'_>, opt: &OptIn) -> TyList {
    let pool = cx.names.pool;
    if opt.structure_owner() == opt.impl_ {
        return own_args(cx, opt.impl_);
    }
    match pool.get(opt.target) {
        TyData::Adt { args, .. } => args,
        _ => TyList::EMPTY,
    }
}

/// The variants and members of `ty`, the target over the owner's
/// parameters, with the derivation's omitted members marked
/// (`annot.omit.skip`), as is a member whose declared default its `build`
/// fills (`OptIn::fills_declared`, `annot.impl-fact.fills-defaults`) and
/// a member a skip fact omits (`annot.impl-fact.skips`), and their facts
/// bodies.
fn variants(cx: &BodyCx<'_>, opt: &OptIn, ty: Ty, facts: &FactBodies) -> StageResult<Vec<Var>> {
    let names = &cx.names;
    let pool = names.pool;
    let Some(item) = cx.lookup.item(opt.data) else {
        return unsupported("a derivation target outside the module's view");
    };
    let TyData::Adt { args, .. } = pool.get(ty) else {
        return unsupported("a derivation target that is not a data type or enum");
    };
    let args = pool.list_items(args).to_vec();
    let data = opt.data;
    let sub = |t: Ty| {
        pool.subst(t, &|p: ParamRef| {
            (p.owner == data).then(|| args.get(usize::from(p.index)).copied())?
        })
    };
    let fact = |v: usize, m: usize| facts.members.get(v)?.get(m).copied().flatten();
    let label = |declared: &str| {
        if declared.starts_with(|c: char| c.is_ascii_digit()) {
            format!("_{declared}")
        } else {
            declared.to_owned()
        }
    };
    let member = |v: usize, i: usize, f: &hd_resolve::Field, omitted: bool| {
        let declared = names.text(f.name).to_owned();
        let t = sub(f.ty);
        Mem {
            name: label(&declared),
            positional: declared.starts_with(|c: char| c.is_ascii_digit()),
            declared,
            position: u32::try_from(i).unwrap_or(0),
            field: u32::try_from(i).unwrap_or(0),
            ty: t,
            embedded: f.embedded,
            has_default: f.has_default,
            omitted,
            rest: false,
            self_ref: SelfRefs::new(cx, data).member(t),
            facts: fact(v, i),
        }
    };
    let mut out = Vec::new();
    match &item.data {
        ItemData::Data(fields) => {
            let members: Vec<Mem> = fields
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    let omitted = opt.omitted.contains(&label(names.text(f.name)))
                        || (opt.fills_declared() && f.has_default)
                        || opt.skipped_at(0, i);
                    member(0, i, f, omitted)
                })
                .collect();
            out.push(Var {
                name: names.text(item.name).to_owned(),
                index: 0,
                def: data,
                shape: Shape::Data,
                self_ref: members.iter().map(|m| m.self_ref).max().unwrap_or(0),
                members,
                facts: None,
                shared: Vec::new(),
            });
        }
        ItemData::Enum { shared, variants } => {
            for (k, v) in variants.iter().enumerate() {
                let members: Vec<Mem> = v
                    .fields
                    .iter()
                    .enumerate()
                    .map(|(i, f)| member(k, i, f, opt.skipped_at(k, i)))
                    .collect();
                let shared = shared
                    .iter()
                    .map(|f| {
                        let declared = names.text(f.name);
                        Shared {
                            name: label(declared),
                            global: crate::init::shared_global(names, v.def, declared),
                            ty: f.ty,
                        }
                    })
                    .collect();
                out.push(Var {
                    name: names.text(v.name).to_owned(),
                    index: u32::try_from(k).unwrap_or(0),
                    def: v.def,
                    shape: Shape::Enum,
                    self_ref: members.iter().map(|m| m.self_ref).max().unwrap_or(0),
                    members,
                    facts: facts.variants.get(k).copied().flatten(),
                    shared,
                });
            }
        }
        _ => return unsupported("a derivation target that is not a data type or enum"),
    }
    Ok(out)
}

/// Self references (`annot.self-ref.*`): whether a member's type refers
/// to, or needs, the enclosing type `data`.
struct SelfRefs<'a, 'c> {
    cx: &'a BodyCx<'c>,
    data: DefId,
}

impl<'a, 'c> SelfRefs<'a, 'c> {
    fn new(cx: &'a BodyCx<'c>, data: DefId) -> Self {
        Self { cx, data }
    }

    /// `SelfRef`'s index for a member of type `t`.
    fn member(&self, t: Ty) -> u32 {
        if self.needs(t, &mut Vec::new()) {
            2
        } else {
            u32::from(self.refers(t, &mut Vec::new()))
        }
    }

    /// The fields of a data type or each variant of an enum, with `args`
    /// substituted.
    fn fields(&self, def: DefId, args: TyList) -> Option<(bool, Vec<Vec<Ty>>)> {
        let pool = self.cx.names.pool;
        let args = pool.list_items(args).to_vec();
        let ty = |t: Ty| {
            pool.subst(t, &|p: ParamRef| {
                (p.owner == def).then(|| args.get(usize::from(p.index)).copied())?
            })
        };
        match &self.cx.lookup.item(def)?.data {
            ItemData::Data(fs) => Some((true, vec![fs.iter().map(|f| ty(f.ty)).collect()])),
            ItemData::Enum { variants, .. } => Some((
                false,
                variants
                    .iter()
                    .map(|v| v.fields.iter().map(|f| ty(f.ty)).collect())
                    .collect(),
            )),
            _ => None,
        }
    }

    /// `annot.self-ref.refers` and `.refers.members`.
    fn refers(&self, t: Ty, seen: &mut Vec<Ty>) -> bool {
        let pool = self.cx.names.pool;
        if seen.contains(&t) {
            return false;
        }
        seen.push(t);
        match pool.get(t) {
            TyData::Mut(x) | TyData::Option(x) | TyData::Ref { inner: x, .. } => {
                self.refers(x, seen)
            }
            TyData::Tuple { elems, rest } => {
                pool.list_items(elems).iter().any(|e| self.refers(*e, seen))
                    || rest.is_some_and(|r| self.refers(r, seen))
            }
            TyData::Adt { def, args } => {
                def == self.data
                    || pool.list_items(args).iter().any(|a| self.refers(*a, seen))
                    || self
                        .fields(def, args)
                        .is_some_and(|(_, vs)| vs.iter().flatten().any(|f| self.refers(*f, seen)))
            }
            _ => false,
        }
    }

    /// `annot.self-ref.needs.*`.
    fn needs(&self, t: Ty, seen: &mut Vec<Ty>) -> bool {
        let pool = self.cx.names.pool;
        let k = &self.cx.names.known;
        if seen.contains(&t) {
            return false;
        }
        seen.push(t);
        match pool.get(t) {
            // `annot.self-ref.needs.reference`: a reference always refers to
            // a value, so it needs what its referent needs.
            TyData::Mut(x) | TyData::Ref { inner: x, .. } => self.needs(x, seen),
            TyData::Tuple { elems, rest } => {
                pool.list_items(elems).iter().any(|e| self.needs(*e, seen))
                    || rest.is_some_and(|r| self.needs(r, seen))
            }
            TyData::Adt { def, args } => {
                if def == self.data {
                    return true;
                }
                if def == k.list || def == k.map {
                    return false;
                }
                if def == k.result {
                    let first = pool.list_items(args).first().copied();
                    return first.is_some_and(|x| self.needs(x, seen));
                }
                match self.fields(def, args) {
                    Some((true, vs)) => vs.iter().flatten().any(|f| self.needs(*f, seen)),
                    Some((false, vs)) => {
                        !vs.is_empty()
                            && vs
                                .iter()
                                .all(|v| v.iter().any(|f| self.needs(*f, &mut seen.clone())))
                    }
                    None => false,
                }
            }
            _ => false,
        }
    }
}
