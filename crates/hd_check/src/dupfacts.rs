//! One place holds one fact of each concrete type (annotations,
//! `annot.decorator.duplicate`, `annot.metadata.duplicate`,
//! `annot.line.duplicate`, `annot.fact.duplicate-decorator`): a function,
//! trait, implementation, newtype, method, member, variant or parameter
//! whose final facts repeat a type is reported once per repeated type, at
//! the later value, as `duplicate-fact`.
//!
//! The final facts of a place are its decorators' values, then the
//! elements of the derivation blocks' member lines in order, where `+=`
//! appends and `=` replaces. A fact's concrete type is the type of its
//! value after inference (`Tag[i32]` and `Tag[string]` differ). A value
//! that does not check, or whose type is not settled, is left out: its own
//! error is the one reported. Places with fewer than two facts are never
//! typed here.

use hd_base::HashMap;

use hd_base::DefId;
use hd_diag::{Code, DiagBuf};
use hd_intern::PathKind;
use hd_resolve::{Head, HeadKind, ItemData};
use hd_syntax::{NodeRef, SyntaxKind};
use hd_tir::ir::BodyKind;
use hd_types::{RowId, Ty, TyData};

use crate::body::{BodyCx, new_ck};
use crate::facts::{FactSource, FactTarget, fact_lines};
use crate::structure::{FactLines, Slot};

/// One fact of a place: its value's node, whether it is a decorator's
/// value, and whether a repeat it makes is reported by the check at hand.
struct Elem<'t> {
    node: NodeRef<'t>,
    single: bool,
    report: bool,
}

/// Reports every concrete type that the facts `elems` of one place repeat,
/// at the later value, where that value has `report` set. The values are
/// typed as the declaration `owner`'s code against `target`. With `prior`,
/// the types of facts that earlier layers left there count as earlier
/// values, and the types of `elems` that check are returned. Without
/// `diags` nothing is reported.
fn report_repeats(
    cx: &BodyCx<'_>,
    owner: DefId,
    target: FactTarget,
    (elems, prior): (&[Elem<'_>], Option<&[Ty]>),
    diags: Option<&mut DiagBuf>,
) -> Vec<Ty> {
    let needed = if prior.is_some() {
        !elems.is_empty()
    } else {
        elems.len() >= 2 && elems.iter().any(|e| e.report)
    };
    if !needed {
        return Vec::new();
    }
    let def = cx.names.member(owner, PathKind::Hidden, "duplicate facts");
    let local = hd_types::LocalPool::new();
    let mut scratch = DiagBuf::default();
    let mut ck = new_ck(
        cx,
        &local,
        owner,
        def,
        BodyKind::Default,
        (Ty::VOID, RowId::EMPTY),
        &mut scratch,
    );
    let mut seen: HashMap<Ty, u32> = HashMap::default();
    for ty in prior.unwrap_or_default() {
        *seen.entry(*ty).or_insert(0) += 1;
    }
    let mut found = Vec::new();
    let mut types = Vec::new();
    for e in elems {
        ck.move_to(cx.src.span(e.node).lo);
        let Some(ty) = ck.fact_type(e.node, (target, owner), e.single) else {
            continue;
        };
        types.push(ty);
        let count = seen.entry(ty).or_insert(0);
        *count += 1;
        if *count == 2 && e.report {
            let msg = format!(
                "this place already holds a fact of the type {}; a second one is a duplicate",
                ck.show(ty)
            );
            found.push((e.node, msg));
        }
    }
    if let Some(diags) = diags {
        for (node, msg) in found {
            diags.error(Code::DuplicateFact, cx.src.span(node), &msg);
        }
    }
    types
}

/// The facts of `sources` that are known before the program runs: values,
/// and the elements of list literals, where an element is reported if it
/// lies in `mine`. Only a literal's own elements are known: a list
/// expression, and what a spread brings into a literal, hold facts only
/// when the program runs, so a repeat among them is not found
/// (`annot.line.duplicate`). No source here holds another body's facts.
fn literal_elems<'t>(
    cx: &BodyCx<'t>,
    sources: &[FactSource<'t>],
    mine: Option<hd_base::Span>,
) -> Vec<Elem<'t>> {
    let mut elems = Vec::new();
    for source in sources {
        match *source {
            FactSource::Value(node) => elems.push(Elem {
                node,
                single: true,
                report: false,
            }),
            FactSource::List(list) if list.kind() == SyntaxKind::ListExpr => {
                for node in list
                    .children()
                    .filter(|e| e.kind() != SyntaxKind::SpreadExpr)
                {
                    let at = cx.src.span(node);
                    elems.push(Elem {
                        node,
                        single: false,
                        report: mine.is_some_and(|m| m.lo <= at.lo && at.hi <= m.hi),
                    });
                }
            }
            FactSource::List(_) | FactSource::Body(..) => {}
        }
    }
    elems
}

/// The concrete types of the facts that each slot of a data type or enum
/// holds, in order, after the layers so far, and whether some fact is known
/// only at run time. The latter is true for a non-literal member-line list or
/// a spread in a literal (`annot.line.duplicate`).
pub type SlotTypes = Vec<(Slot, Vec<Ty>, bool)>;

fn has_opaque_source(sources: &[FactSource<'_>]) -> bool {
    sources.iter().any(|source| match *source {
        FactSource::Value(_) => false,
        FactSource::List(list) if list.kind() == SyntaxKind::ListExpr => {
            list.children().any(|e| e.kind() == SyntaxKind::SpreadExpr)
        }
        FactSource::List(_) | FactSource::Body(..) => true,
    })
}

/// The types of the declaration facts `view` of the type `data`, which
/// module `cx` declares, in `cx`.
#[must_use]
pub fn declaration_types(cx: &BodyCx<'_>, data: DefId, view: &FactLines<'_>) -> SlotTypes {
    view.slots()
        .into_iter()
        .map(|(slot, sources)| {
            let elems = literal_elems(cx, sources, None);
            let types = report_repeats(cx, data, view.target(slot), (&elems, Some(&[])), None);
            (slot, types, has_opaque_source(sources))
        })
        .collect()
}

/// The types that each slot holds once the member lines of the derivation
/// block `h` (module `cx`'s) of a type that another module declares edit
/// the `prior` ones (`annot.line.extend`, `annot.line.replace`). With
/// `diags`, a line that repeats a type its slot already holds is reported
/// at its element (`annot.line.duplicate`, `duplicate-fact`). `None` when
/// `h` is no block of a data type or enum.
pub fn layer_types<'t>(
    cx: &BodyCx<'t>,
    h: &Head<'t>,
    prior: &SlotTypes,
    diags: Option<&mut DiagBuf>,
) -> Option<SlotTypes> {
    let (view, replaced) = crate::structure::layer_facts(cx, h.def, h.node)?;
    let mine = cx.src.span(h.node);
    let mut diags = diags;
    let mut out = SlotTypes::new();
    for (slot, sources) in view.slots() {
        let prior = prior.iter().find(|(s, ..)| *s == slot);
        let held: &[Ty] = match prior {
            Some((_, types, _)) if !replaced.contains(&slot) => types,
            _ => &[],
        };
        let held_opaque = !replaced.contains(&slot) && prior.is_some_and(|(_, _, opaque)| *opaque);
        let elems = literal_elems(cx, sources, Some(mine));
        let new = report_repeats(
            cx,
            h.def,
            view.target(slot),
            (&elems, Some(held)),
            diags.as_deref_mut(),
        );
        out.push((
            slot,
            [held, new.as_slice()].concat(),
            held_opaque || has_opaque_source(sources),
        ));
    }
    Some(out)
}

/// The nodes of the declaration `n` that decorators may precede: itself,
/// its members, variants, parameters and methods, but not the code inside
/// a function body, whose local declarations are heads of their own.
pub(crate) fn decorated<'t>(n: NodeRef<'t>, out: &mut Vec<NodeRef<'t>>) {
    if n.children().any(|c| c.kind() == SyntaxKind::Decorator) {
        out.push(n);
    }
    for c in n.children() {
        if c.kind() == SyntaxKind::Block && n.kind() == SyntaxKind::FnDecl {
            continue;
        }
        decorated(c, out);
    }
}

/// Reports the repeated concrete types among the facts that attach to the
/// declaration `h` and its members, and those that its derivation block's
/// member lines leave (`duplicate-fact`). `heads` are the module's.
pub fn check_duplicate_facts<'t>(
    cx: &BodyCx<'t>,
    heads: &[Head<'t>],
    h: &Head<'t>,
    diags: &mut DiagBuf,
) {
    let typed = if matches!(h.kind, HeadKind::Data | HeadKind::Enum) {
        crate::structure::member_fact_sites(cx, h.def, h.node)
    } else {
        Vec::new()
    };
    let mut places = Vec::new();
    decorated(h.node, &mut places);
    for n in places {
        let lines = fact_lines(cx, n);
        let Some(first) = lines.first() else { continue };
        let target = match typed.iter().find(|(e, ..)| e.index() == first.index()) {
            Some((_, ty, _)) => FactTarget::Field(*ty),
            None if h.kind == HeadKind::Fn && n.index() == h.node.index() => FactTarget::Fn,
            None => FactTarget::Other,
        };
        let elems: Vec<Elem<'_>> = lines
            .into_iter()
            .map(|node| Elem {
                node,
                single: true,
                report: true,
            })
            .collect();
        report_repeats(cx, h.def, target, (&elems, None), Some(diags));
    }
    if h.kind == HeadKind::Impl {
        derivation_repeats(cx, heads, h, diags);
    }
}

/// The repeats that the member lines of the derivation block `h` make:
/// the facts of its target's declaration, edited by the block with no
/// trait and then by `h`'s own lines, reported where the later value is
/// one of `h`'s own elements.
fn derivation_repeats<'t>(cx: &BodyCx<'t>, heads: &[Head<'t>], h: &Head<'t>, diags: &mut DiagBuf) {
    let Some(ItemData::Impl {
        trait_,
        by: Some(_),
        self_ty,
        ..
    }) = cx.lookup.item(h.def).map(|i| &i.data)
    else {
        return;
    };
    let pool = cx.names.pool;
    let TyData::Adt { def: data, args } = pool.get(*self_ty) else {
        return;
    };
    let lines = crate::derive::member_lines(&cx.src, h.node);
    let (Some(item), Some(decl)) = (
        cx.lookup.item(data),
        heads.iter().find(|x| x.def == data).map(|x| x.node),
    ) else {
        return;
    };
    if lines.is_empty() {
        return;
    }
    let mut view = if *trait_ == DefId::NONE {
        crate::structure::declaration_facts(cx, item, decl, Some(h.node))
    } else {
        let traitless = cx
            .lookup
            .own
            .iter()
            .find(|it| {
                matches!(&it.data, ItemData::Impl { trait_, by: Some(_), self_ty, .. }
                    if *trait_ == DefId::NONE
                        && matches!(pool.get(*self_ty), TyData::Adt { def, .. } if def == data))
            })
            .and_then(|it| heads.iter().find(|x| x.def == it.def).map(|x| x.node));
        let mut view = crate::structure::declaration_facts(cx, item, decl, traitless);
        crate::structure::edit_facts(cx, item, &mut view, &lines);
        view
    };
    view.retarget(cx, data, pool.list_items(args));
    let mine = cx.src.span(h.node);
    for (slot, sources) in view.slots() {
        let elems = literal_elems(cx, sources, Some(mine));
        report_repeats(
            cx,
            h.def,
            view.target(slot),
            (&elems, None),
            Some(&mut *diags),
        );
    }
}
