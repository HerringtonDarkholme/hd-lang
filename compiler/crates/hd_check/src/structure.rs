//! A derivation's `Structure` at check time (annotations, "The Structure
//! Trait", "Facts", "Members And Variants", "Self References";
//! codegen.md §12.3 "Derives"): the target's member list, built from the
//! data type or enum and the derivation's omitted members, for the one
//! generator in `hd_structure`, which writes the bodies as hidden methods
//! of the `Structure`'s owner (`OptIn::structure_owner`); and the bodies
//! that build each `Facts` the derivation sees. Collection calls the same
//! generator for a tuple's `Structure` (codegen.md §13.6).
//!
//! The declaration's `Structure` is shared: its methods, handles and facts
//! bodies are hidden methods of the data type or enum, generated once per
//! module, and every derivation without member lines calls them. A
//! derivation whose block has member lines gets its own, under the
//! derivation; a slot its lines do not edit reuses the declaration's facts
//! body.
//!
//! Not generated yet, each a structured "not implemented" that leaves the
//! derivation's `Structure` without bodies (a call of it then stops at
//! collection): a typed fact (`annot.typed-fact.check`), a member line
//! whose list is not a list literal, and the shared constructor data of a
//! generic enum.

use hd_base::{DefId, Stage, StageResult};
use hd_diag::DiagBuf;
use hd_resolve::{Item, ItemData};
use hd_structure::{Mem, Shape, Shared, Target, Var, hidden_method};
use hd_syntax::{NodeRef, SyntaxKind};
use hd_tir::Body;
use hd_types::{ParamRef, Ty, TyData, TyList};

use crate::body::{BodyCx, unsupported};
use crate::derive::{MemberLine, OptIn};
use crate::facts::{FactSource, TypedFacts, check_fact_list};

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
}

impl<'t> FactLines<'t> {
    fn slot(&mut self, s: Slot) -> Option<&mut Vec<FactSource<'t>>> {
        match s {
            Slot::Type => Some(&mut self.of_type),
            Slot::Variant(v) => self.variants.get_mut(v),
            Slot::Member(v, m) => self.members.get_mut(v)?.get_mut(m),
        }
    }

    fn slots(&self) -> Vec<(Slot, &[FactSource<'t>])> {
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
fn member_nodes(decl: NodeRef<'_>) -> Vec<NodeRef<'_>> {
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
        hd_resolve::facts::fact_lines(src, n)
            .into_iter()
            .map(FactSource::Value)
            .collect()
    };
    let mut out = FactLines {
        of_type: values(decl),
        ..FactLines::default()
    };
    let nodes = member_nodes(decl);
    match &item.data {
        // A data type's one variant has no facts (`annot.variant.data-facts`).
        ItemData::Data(_) => {
            out.variants.push(Vec::new());
            out.members.push(nodes.into_iter().map(values).collect());
        }
        ItemData::Enum { .. } => {
            for v in nodes {
                out.variants.push(values(v));
                let payloads = hd_resolve::Src::child(v, SyntaxKind::ParameterList)
                    .into_iter()
                    .flat_map(NodeRef::children)
                    .filter(|c| c.kind() == SyntaxKind::Parameter)
                    .map(values)
                    .collect();
                out.members.push(payloads);
            }
        }
        _ => {}
    }
    if let Some(block) = traitless {
        edit_facts(cx, item, &mut out, &crate::derive::member_lines(src, block));
    }
    out
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
    let names = &cx.names;
    let mut edited = Vec::new();
    for line in lines {
        let Some(list) = line.list else { continue };
        let slot = if line.name == "Self" {
            Some(Slot::Type)
        } else {
            match &item.data {
                ItemData::Data(fields) => fields
                    .iter()
                    .position(|f| {
                        let n = names.text(f.name);
                        n == line.name || line.name.strip_prefix('_') == Some(n)
                    })
                    .map(|m| Slot::Member(0, m)),
                ItemData::Enum { variants, .. } => variants
                    .iter()
                    .position(|v| names.text(v.name) == line.name)
                    .map(Slot::Variant),
                _ => None,
            }
        };
        let Some(slot) = slot else { continue };
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
/// parameter, checked in the target's module `cx` and reported at `decl`
/// (`annot.fact.read`). With `only`, a slot outside its edited slots
/// reuses its base's body; without, every slot with a fact gets one. A
/// fact that does not check declines them all.
pub fn fact_bodies(
    cx: &BodyCx<'_>,
    owner: DefId,
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
            Slot::Type => ("type facts".to_owned(), only.and_then(|o| o.1.of_type)),
            Slot::Variant(v) => (
                format!("variant facts {v}"),
                only.and_then(|o| o.1.variants.get(v).copied().flatten()),
            ),
            Slot::Member(v, m) => (
                format!("member facts {v} {m}"),
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
                    TypedFacts::Unsupported,
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
    cx.names.pool.intern_ty(&TyData::Param(ParamRef {
        owner,
        index: u16::try_from(index).unwrap_or(u16::MAX),
    }))
}

/// The arguments of the `Structure` owner's parameters in `opt`'s
/// instance: the derivation's own parameters, or the target's arguments
/// for the declaration's `Structure`.
#[must_use]
pub fn owner_args(cx: &BodyCx<'_>, opt: &OptIn) -> TyList {
    let pool = cx.names.pool;
    if opt.structure_owner() == opt.impl_ {
        let n = cx.lookup.item(opt.impl_).map_or(0, |i| i.generics.len());
        let own: Vec<Ty> = (0..n).map(|i| param(cx, opt.impl_, i)).collect();
        return pool.list(&own);
    }
    match pool.get(opt.target) {
        TyData::Adt { args, .. } => args,
        _ => TyList::EMPTY,
    }
}

/// The variants and members of `ty`, the target over the owner's
/// parameters, with the derivation's omitted members marked
/// (`annot.omit.skip`), as is a member whose declared default its `build`
/// fills (`OptIn::fills_declared`), and their facts bodies.
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
                        || (opt.fills_declared && f.has_default);
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
            // `init` holds no shared data of a generic enum yet.
            if !shared.is_empty() && !item.generics.is_empty() {
                return unsupported("the shared constructor data of a generic enum's variants");
            }
            for (k, v) in variants.iter().enumerate() {
                let members: Vec<Mem> = v
                    .fields
                    .iter()
                    .enumerate()
                    .map(|(i, f)| member(k, i, f, false))
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
            TyData::Mut(x) | TyData::Option(x) => self.refers(x, seen),
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
            TyData::Mut(x) => self.needs(x, seen),
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
