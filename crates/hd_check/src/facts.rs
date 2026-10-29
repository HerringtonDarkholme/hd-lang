//! Function facts (annotations, "Function Facts"): the values a module-level
//! function's decorators attach, read by `facts_of(f)`.
//!
//! Each such function gets one hidden body, `Facts { items: [...] }`, that
//! evaluates its decorators in source order as ordinary code of the
//! function's module (`annot.decorator.facts-of`, `annot.facts-of.result.compiler-decorators`).
//! A decorator call is its call; a bare name of a function whose
//! parameters all have defaults calls it
//! (`annot.decorator.bare-call-defaults`); a literal is its value. `facts_of(f)` is a `DefaultCall` of that body, so the argument is
//! never evaluated (`annot.facts-of.target`).
//!
//! A value of a typed fact type is checked where it attaches ("Member-Typed
//! Facts"): the fact type's pattern is matched against the target's type,
//! as a call infers its type arguments from its one argument, and the value
//! then checks like `let f: D[A] = v`. Every error is reported once, at the
//! decorator or member-line element.

use hd_base::{DefId, StageResult};
use hd_diag::{Code, DiagBuf};
use hd_intern::PathKind;
use hd_resolve::facts::fact_lines as resolved_fact_lines;
use hd_resolve::iface::{LITERAL_PREFIX, LITERAL_SUFFIX};
use hd_resolve::{BindingKind, Head, HeadKind, ItemData};
use hd_syntax::{NodeRef, SyntaxKind};
use hd_tir::Body;
use hd_tir::ir::{BodyKind, Coercion, NONE, Ref, Tag, TirSink};
use hd_types::solver::TraitRef;
use hd_types::{ParamRef, RowId, Ty, TyData, TyList, VarKind};

use crate::body::{BodyCx, Ck, ExpectPosition, is_out_of_fuel, new_ck, too_complex, unsupported};
use crate::call::{Args, subst_owner};
use crate::structure::{FactLines, Slot};
use crate::ty::Named;

/// `Facts` and the type of its `items`.
fn facts_types(cx: &BodyCx<'_>) -> StageResult<(Ty, Ty)> {
    let facts = cx.names.item("std.structure", "Facts");
    let Some(ItemData::Data(fields)) = cx.lookup.item(facts).map(|i| &i.data) else {
        return unsupported("`std.structure.Facts` outside the module's view");
    };
    let Some(items) = fields.first().map(|f| f.ty) else {
        return unsupported("`std.structure.Facts` without items");
    };
    let ty = cx.names.pool.intern_ty(&TyData::Adt {
        def: facts,
        args: TyList::EMPTY,
    });
    Ok((ty, items))
}

/// A decorator's resolved declaration, including a qualified path.
fn decorator_def(cx: &BodyCx<'_>, d: NodeRef<'_>) -> Option<DefId> {
    hd_resolve::facts::decorator_def(
        &cx.src,
        &cx.names,
        cx.scope_at(cx.src.span(d).lo),
        &|module, name| {
            cx.lookup
                .ifaces
                .iter()
                .find_map(|f| f.export(module, name))
                .map(|e| e.def)
        },
        d,
    )
}

/// The identifier segments of an expression's callee, excluding its call.
fn expression_segments(cx: &BodyCx<'_>, e: NodeRef<'_>) -> Vec<String> {
    fn segments(cx: &BodyCx<'_>, n: NodeRef<'_>) -> Vec<String> {
        match n.kind() {
            SyntaxKind::NameExpr => vec![cx.src.text(cx.src.first(n)).to_owned()],
            SyntaxKind::FieldExpr | SyntaxKind::PathExpr => {
                let mut out = n
                    .children()
                    .next()
                    .map(|base| segments(cx, base))
                    .unwrap_or_default();
                out.push(cx.src.text(cx.src.last(n)).to_owned());
                out
            }
            _ => Vec::new(),
        }
    }

    let mut callee = e;
    if callee.kind() == SyntaxKind::CallExpr {
        let Some(inner) = callee.children().next() else {
            return Vec::new();
        };
        callee = inner;
    }
    if callee.kind() == SyntaxKind::TypeArgsExpr {
        let Some(inner) = callee.children().next() else {
            return Vec::new();
        };
        callee = inner;
    }
    segments(cx, callee)
}

fn expression_def(cx: &BodyCx<'_>, e: NodeRef<'_>) -> Option<DefId> {
    let segs = expression_segments(cx, e);
    let first = segs.first()?;
    let binding = cx
        .scope_at(cx.src.span(e).lo)
        .lookup(cx.names.syms.intern(first))?;
    match (binding.kind, segs.len()) {
        (BindingKind::Item, 1) => Some(DefId::from_raw(binding.value)),
        (BindingKind::Module, _) if segs.len() >= 2 => {
            let base = cx
                .scope_at(cx.src.span(e).lo)
                .modules
                .get(binding.value as usize)?
                .clone();
            if segs.len() > 2 && base.contains('.') {
                return None;
            }
            let module = if segs.len() == 2 {
                base
            } else {
                format!("{base}.{}", segs[1..segs.len() - 1].join("."))
            };
            let name = cx.names.syms.intern(segs.last()?);
            Some(
                cx.lookup
                    .ifaces
                    .iter()
                    .find_map(|f| f.export(&module, name))?
                    .def,
            )
        }
        _ => None,
    }
}

fn expression_variant_def(cx: &BodyCx<'_>, e: NodeRef<'_>, expected_owner: DefId) -> Option<DefId> {
    let mut segs = expression_segments(cx, e);
    let variant = segs.pop()?;
    let first = segs.first()?;
    let binding = cx
        .scope_at(cx.src.span(e).lo)
        .lookup(cx.names.syms.intern(first))?;
    let owner = match (binding.kind, segs.len()) {
        (BindingKind::Item, 1) => DefId::from_raw(binding.value),
        (BindingKind::Module, _) if segs.len() >= 2 => {
            let base = cx
                .scope_at(cx.src.span(e).lo)
                .modules
                .get(binding.value as usize)?
                .clone();
            if segs.len() > 2 && base.contains('.') {
                return None;
            }
            let module = if segs.len() == 2 {
                base
            } else {
                format!("{base}.{}", segs[1..segs.len() - 1].join("."))
            };
            let name = cx.names.syms.intern(segs.last()?);
            cx.lookup
                .ifaces
                .iter()
                .find_map(|f| f.export(&module, name))?
                .def
        }
        _ => return None,
    };
    (owner == expected_owner).then(|| cx.names.member(owner, PathKind::Variant, &variant))
}

/// The literal marker of a decorator, by its resolved declaration.
pub(crate) fn literal_marker(cx: &BodyCx<'_>, d: NodeRef<'_>) -> Option<u8> {
    let def = decorator_def(cx, d)?;
    if def == cx.names.known.num_suffix {
        Some(LITERAL_SUFFIX)
    } else if def == cx.names.known.str_prefix {
        Some(LITERAL_PREFIX)
    } else {
        None
    }
}

/// Whether `n` is a path of names, as `a.b.c`, and not a method chain.
fn is_path(n: NodeRef<'_>) -> bool {
    match n.kind() {
        SyntaxKind::NameExpr => true,
        SyntaxKind::FieldExpr | SyntaxKind::PathExpr => n.children().next().is_some_and(is_path),
        _ => false,
    }
}

/// The decorator expressions that attach values, with literal markers and
/// std's `inline` and `cold` (`annot.inline.recognized`) filtered by the
/// same resolved identity as the function's marker.
pub(crate) fn fact_lines<'t>(cx: &BodyCx<'t>, node: NodeRef<'t>) -> Vec<NodeRef<'t>> {
    let known = &cx.names.known;
    resolved_fact_lines(&cx.src, node, |d| {
        let def = decorator_def(cx, d);
        literal_marker(cx, d).is_some() || def == Some(known.inline) || def == Some(known.cold)
    })
}

/// Checks the facts body of the module-level function `owner`, whose
/// declaration is `node`; a typed fact checks against the function's
/// signature type (`annot.typed-fact.declared-type`).
pub fn check_facts(
    cx: &BodyCx<'_>,
    owner: DefId,
    node: NodeRef<'_>,
    diags: &mut DiagBuf,
) -> StageResult<Body> {
    let def = cx.names.member(owner, PathKind::Hidden, "facts");
    let sources: Vec<FactSource<'_>> = fact_lines(cx, node)
        .into_iter()
        .map(FactSource::Value)
        .collect();
    let label = cx
        .lookup
        .item(owner)
        .map_or_else(String::new, |item| cx.names.text(item.name).to_owned());
    check_fact_list(
        cx,
        (owner, def),
        &sources,
        (FactTarget::Fn, &label),
        node,
        diags,
    )
}

/// Where one fact, or a list of facts, of a `Facts` value is written.
#[derive(Clone, Copy)]
pub enum FactSource<'t> {
    /// A decorator line's expression: the one value it attaches
    /// (`annot.decorator.bare-call-defaults` included).
    Value(NodeRef<'t>),
    /// A member line's right side: a list of facts (`annot.line.right-typed`).
    List(NodeRef<'t>),
    /// The facts that another facts body builds, called at these instance
    /// arguments: the declaration facts that the member lines of a block
    /// in another module of a REPL session edit, whose nodes are in that
    /// other module's source.
    Body(DefId, TyList),
}

/// The target a list of facts attaches to, which a typed fact's pattern
/// is matched against (`annot.typed-fact.targets`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FactTarget {
    /// The module-level function that owns the body: its signature type.
    Fn,
    /// A field or payload member of this type, as declared, in the body
    /// owner's scope.
    Field(Ty),
    /// Any other target, where a typed fact is a `decorator-target-kind`
    /// error the resolver reports: it is left out.
    Other,
}

/// Checks the body `def` that builds one `Facts` value from `sources`, in
/// order, as ordinary code of `owner`'s module (`annot.fact.read`,
/// `annot.structure.reference-facts`); `at` is the node its value is
/// reported at. The body is a `Fact` body: the program evaluates it once,
/// on the first read (`annot.fact.eval.lazy`), and a panic in it names
/// `label`, what the facts are attached to (`annot.fact.eval.panic-runtime`).
/// A list that runs out of fuel is one `item-too-complex` on the decorated
/// item `at`, and a poison body (`too_complex`).
pub fn check_fact_list(
    cx: &BodyCx<'_>,
    (owner, def): (DefId, DefId),
    sources: &[FactSource<'_>],
    (target, label): (FactTarget, &str),
    at: NodeRef<'_>,
    diags: &mut DiagBuf,
) -> StageResult<Body> {
    match fact_list_steps(cx, (owner, def), sources, (target, label), at, diags) {
        Err(e) if is_out_of_fuel(&e) => {
            let mut body = too_complex(cx, (def, BodyKind::Fact), at, diags)?;
            body.label = label.into();
            Ok(body)
        }
        r => r,
    }
}

fn fact_list_steps(
    cx: &BodyCx<'_>,
    (owner, def): (DefId, DefId),
    sources: &[FactSource<'_>],
    (target, label): (FactTarget, &str),
    at: NodeRef<'_>,
    diags: &mut DiagBuf,
) -> StageResult<Body> {
    let names = &cx.names;
    let pool = names.pool;
    let (facts_ty, items_ty) = facts_types(cx)?;
    let trait_value = pool.intern_ty(&TyData::TraitValue {
        def: names.item("std.inspect", "Inspectable"),
        args: TyList::EMPTY,
        bindings: vec![],
    });
    let local = hd_types::LocalPool::new();
    let mut ck = new_ck(
        cx,
        &local,
        owner,
        def,
        BodyKind::Default,
        (facts_ty, RowId::EMPTY),
        diags,
    );
    let blk = ck.b.open_block();
    let mut refs = Vec::new();
    // The positions of `refs` that hold a whole list of facts, which the
    // list of facts spreads in place.
    let mut spreads = Vec::new();
    for source in sources {
        let (values, single) = match *source {
            FactSource::Value(e) => (vec![e], true),
            FactSource::List(e) if e.kind() == SyntaxKind::ListExpr => {
                (e.children().collect(), false)
            }
            // A list expression other than a literal: its facts are known
            // when the body runs (`annot.line.right-typed`).
            FactSource::List(e) => {
                ck.move_to(cx.src.span(e).lo);
                if let Some(r) = ck.fact_list(e, Code::InvalidMemberLine, items_ty)? {
                    spreads.push(u32::try_from(refs.len()).unwrap_or(0));
                    refs.push(r);
                }
                continue;
            }
            // Another body's facts, spread in place: a call of it, like
            // the `Structure`'s own, and its items.
            FactSource::Body(def, targs) => {
                let a = ck.b.refs_record(&[Ref(def.raw()), Ref(targs.0)]);
                let earlier = ck.b.refs_record(&[]);
                let call =
                    ck.b.emit(Tag::DefaultCall, a, earlier, facts_ty, at.index());
                let items = ck.field_value(call, 0, items_ty, at.index());
                spreads.push(u32::try_from(refs.len()).unwrap_or(0));
                refs.push(items);
                continue;
            }
        };
        for e in values {
            if e.kind() == SyntaxKind::ComprehensionFor {
                return unsupported("a member line list with a comprehension");
            }
            ck.move_to(cx.src.span(e).lo);
            if e.kind() == SyntaxKind::SpreadExpr {
                let Some(operand) = e.children().next() else {
                    return unsupported("a spread without an operand");
                };
                ck.move_to(cx.src.span(operand).lo);
                if let Some(r) = ck.fact_list(operand, Code::TypeMismatch, items_ty)? {
                    spreads.push(u32::try_from(refs.len()).unwrap_or(0));
                    refs.push(r);
                }
                continue;
            }
            if let Some(fact) = ck.typed_fact_of(e, single)? {
                let optional_payload = matches!(target, FactTarget::Field(_));
                let Some(target) = ck.fact_target(target, owner)? else {
                    continue;
                };
                // The value is `D[A]`, which need not be inspectable (a
                // function type in `A`): it is erased to the item type
                // without the conversion's bound, as `Facts` holds any
                // value (`annot.metadata.reference-storage`).
                if let Some((r, t)) = ck.typed_fact(fact, e, target, single, optional_payload)? {
                    let (r, t) = ck.new_object_ref(r, t, false, e);
                    refs.push(ck.build_dyn_ref(
                        r,
                        t,
                        trait_value,
                        false,
                        Coercion::RefToTraitValue,
                        e,
                    ));
                }
                continue;
            }
            let (r, t) = if single {
                ck.fact_value(e, None)?
            } else {
                ck.expr(e, None)?
            };
            let (r, t) = ck.new_object_ref(r, t, false, e);
            refs.push(ck.build_dyn_ref(r, t, trait_value, false, Coercion::RefToTraitValue, e));
        }
    }
    let rec = ck.b.refs_record(&refs);
    let marks = if spreads.is_empty() {
        NONE
    } else {
        ck.b.words_record(&spreads)
    };
    let list = ck.b.emit(Tag::NewList, marks, rec, items_ty, at.index());
    let rec = ck.b.refs_record(&[list]);
    let value = ck.b.emit(Tag::NewStruct, NONE, rec, facts_ty, at.index());
    let root = ck.b.close_block(blk, Some(value), facts_ty, at.index());
    let mut body = ck.finish_body(root)?;
    body.kind = BodyKind::Fact;
    body.label = label.into();
    Ok(body)
}

fn expression_arguments<'t>(
    cx: &BodyCx<'t>,
    call: NodeRef<'t>,
) -> Vec<(Option<String>, NodeRef<'t>)> {
    let Some(list) = hd_resolve::Src::child(call, SyntaxKind::ArgumentList) else {
        return Vec::new();
    };
    list.children()
        .filter_map(|arg| match arg.kind() {
            SyntaxKind::Argument | SyntaxKind::NamedArgument => {
                let name = (arg.kind() == SyntaxKind::NamedArgument)
                    .then(|| cx.src.first_ident(arg).map(|t| cx.src.text(t).to_owned()))
                    .flatten();
                arg.children().next().map(|value| (name, value))
            }
            _ => None,
        })
        .collect()
}

fn expression_fact_type(cx: &BodyCx<'_>, owner: DefId, value: NodeRef<'_>) -> Option<Ty> {
    let def = cx
        .names
        .member(owner, PathKind::Hidden, "enum tagging check");
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
    ck.move_to(cx.src.span(value).lo);
    ck.fact_type(value, (FactTarget::Other, owner), true)
}

fn renamed(source: &str, style: Option<&str>) -> String {
    match style {
        Some("Uppercase" | "ScreamingSnakeCase") => source.to_ascii_uppercase(),
        Some("KebabCase") => source.replace('_', "-"),
        Some("ScreamingKebabCase") => source.to_ascii_uppercase().replace('_', "-"),
        Some("PascalCase" | "CamelCase") => {
            let camel = style == Some("CamelCase");
            let mut out = String::new();
            let mut first_word = true;
            for word in source.split('_') {
                let mut chars = word.chars();
                let Some(first) = chars.next() else { continue };
                if camel && first_word {
                    out.push(first.to_ascii_lowercase());
                } else {
                    out.push(first.to_ascii_uppercase());
                }
                out.extend(chars);
                first_word = false;
            }
            out
        }
        _ => source.to_owned(),
    }
}

fn internal_payload_allowed(cx: &BodyCx<'_>, mut ty: Ty) -> bool {
    loop {
        match cx.names.pool.get(ty) {
            TyData::Mut(inner) | TyData::Ref { inner, .. } => ty = inner,
            TyData::Adt { def, args } if def == cx.names.known.map => {
                return cx.names.pool.list_items(args).first().copied() == Some(Ty::STRING);
            }
            TyData::Adt { def, .. } if def == cx.names.known.list => return false,
            TyData::Adt { def, .. } => {
                return matches!(
                    cx.lookup.item(def),
                    Some(item) if item.is_struct && matches!(&item.data, ItemData::Data(_))
                );
            }
            _ => return false,
        }
    }
}

#[derive(Clone, Copy)]
struct WrittenFact<'t> {
    owner: DefId,
    node: NodeRef<'t>,
}

fn fact_owner(
    cx: &BodyCx<'_>,
    node: NodeRef<'_>,
    declaration: DefId,
    traitless: Option<&Head<'_>>,
    view: Option<&Head<'_>>,
) -> DefId {
    let span = cx.src.span(node);
    view.into_iter()
        .chain(traitless)
        .filter(|head| {
            let block = cx.src.span(head.node);
            block.lo <= span.lo && span.hi <= block.hi
        })
        .map(|head| head.def)
        .next()
        .unwrap_or(declaration)
}

fn written_facts<'t>(
    cx: &BodyCx<'t>,
    sources: &[FactSource<'t>],
    declaration: DefId,
    traitless: Option<&Head<'t>>,
    view: Option<&Head<'t>>,
) -> Vec<WrittenFact<'t>> {
    let mut out = Vec::new();
    for source in sources {
        match *source {
            FactSource::Value(node) => out.push(WrittenFact {
                owner: fact_owner(cx, node, declaration, traitless, view),
                node,
            }),
            FactSource::List(list) if list.kind() == SyntaxKind::ListExpr => {
                let owner = fact_owner(cx, list, declaration, traitless, view);
                out.extend(
                    list.children()
                        .filter(|node| node.kind() != SyntaxKind::SpreadExpr)
                        .map(|node| WrittenFact { owner, node }),
                );
            }
            FactSource::List(_) | FactSource::Body(..) => {}
        }
    }
    out
}

fn data_field<'t>(cx: &BodyCx<'t>, value: NodeRef<'t>, name: &str) -> Option<NodeRef<'t>> {
    (value.kind() == SyntaxKind::DataExpr)
        .then(|| {
            value.children().find_map(|field| {
                (field.kind() == SyntaxKind::DataFieldInit
                    && cx
                        .src
                        .first_ident(field)
                        .is_some_and(|token| cx.src.text(token) == name))
                .then(|| field.children().next())
                .flatten()
            })
        })
        .flatten()
}

fn concrete_fact(cx: &BodyCx<'_>, value: WrittenFact<'_>, expected: DefId) -> bool {
    matches!(
        expression_fact_type(cx, value.owner, value.node).map(|ty| cx.names.pool.get(ty)),
        Some(TyData::Adt { def, .. }) if def == expected
    )
}

fn rename_name(
    cx: &BodyCx<'_>,
    value: WrittenFact<'_>,
    rename: DefId,
    constructor: DefId,
    diags: &mut DiagBuf,
) -> Option<String> {
    if !concrete_fact(cx, value, rename) {
        return None;
    }
    let argument = if expression_def(cx, value.node) == Some(constructor) {
        expression_arguments(cx, value.node)
            .first()
            .map(|(_, argument)| *argument)
    } else {
        data_field(cx, value.node, "name")
    };
    let Some(argument) = argument else {
        diags.error(
            Code::EnumTagNameNotStatic,
            cx.src.span(value.node),
            "a tagged enum member rename must be a string literal",
        );
        return None;
    };
    let Some(name) = crate::expr::literal_text(&cx.src, argument) else {
        diags.error(
            Code::EnumTagNameNotStatic,
            cx.src.span(argument),
            "a tagged enum member rename must be a string literal",
        );
        return None;
    };
    Some(name)
}

fn rename_style_argument<'t>(
    cx: &BodyCx<'t>,
    value: WrittenFact<'t>,
    constructor: DefId,
) -> Option<NodeRef<'t>> {
    if expression_def(cx, value.node) == Some(constructor) {
        expression_arguments(cx, value.node)
            .first()
            .map(|(_, argument)| *argument)
    } else {
        data_field(cx, value.node, "style")
    }
}

fn rename_style_value(
    cx: &BodyCx<'_>,
    value: WrittenFact<'_>,
    rename_all: DefId,
    constructor: DefId,
    rename_case: DefId,
) -> Option<String> {
    if !concrete_fact(cx, value, rename_all) {
        return None;
    }
    let style = rename_style_argument(cx, value, constructor)?;
    (style.kind() == SyntaxKind::VariantExpr
        || expression_variant_def(cx, style, rename_case).is_some())
    .then(|| {
        cx.src
            .tokens(style)
            .filter(|token| cx.src.tkind(*token) == Some(hd_syntax::TokenKind::Ident))
            .last()
            .map(|token| cx.src.text(token).to_owned())
    })
    .flatten()
}

/// Validates std.serde's internal and adjacent enum shapes against the
/// declaration facts after its no-trait metadata block has edited them
/// (`std-serde.enum-tagging.*`).
fn check_enum_tagging(
    cx: &BodyCx<'_>,
    item: &hd_resolve::Item,
    decl: NodeRef<'_>,
    traitless: Option<&Head<'_>>,
    view: Option<&Head<'_>>,
    tagging_facts: &FactLines<'_>,
    view_facts: &FactLines<'_>,
    declaration_checks: bool,
    diags: &mut DiagBuf,
) {
    let ItemData::Enum { variants, .. } = &item.data else {
        return;
    };
    let tagging = cx.names.item("std.serde", "EnumTagging");
    let tag = cx.names.item("std.serde", "tag");
    let untagged = cx.names.item("std.serde", "untagged");
    let internal_variant = cx.names.member(tagging, PathKind::Variant, "Internal");
    let adjacent_variant = cx.names.member(tagging, PathKind::Variant, "Adjacent");
    let untagged_variant = cx.names.member(tagging, PathKind::Variant, "Untagged");
    let type_sources = tagging_facts
        .slots()
        .into_iter()
        .find_map(|(slot, sources)| (slot == Slot::Type).then_some(sources))
        .unwrap_or_default();
    let type_values = written_facts(cx, type_sources, item.def, traitless, None);
    let mut tag_value = None;
    for value in &type_values {
        match expression_def(cx, value.node) {
            Some(def) if def == tag => {
                tag_value = Some(*value);
                break;
            }
            Some(def) if def == untagged => return,
            _ => {}
        }
        if !concrete_fact(cx, *value, tagging) {
            continue;
        }
        match expression_variant_def(cx, value.node, tagging) {
            Some(def) if def == internal_variant || def == adjacent_variant => {
                tag_value = Some(*value);
                break;
            }
            Some(def) if def == untagged_variant => return,
            _ => {
                if declaration_checks {
                    diags.error(
                        Code::EnumTagNameNotStatic,
                        cx.src.span(value.node),
                        "enum tagging must use `tag`, `untagged`, or an `EnumTagging` variant directly",
                    );
                }
                return;
            }
        }
    }
    let Some(tag_value) = tag_value else {
        return;
    };
    let arguments = expression_arguments(cx, tag_value.node);
    let mut positional = 0;
    let mut tag_arg = None;
    let mut content_arg = None;
    for (name, value) in &arguments {
        match name.as_deref() {
            Some("tag") => tag_arg = Some(*value),
            Some("content") => content_arg = Some(*value),
            Some(_) => {}
            None => {
                if positional == 0 {
                    tag_arg = Some(*value);
                } else if positional == 1 {
                    content_arg = Some(*value);
                }
                positional += 1;
            }
        }
    }
    let Some(tag_arg) = tag_arg else { return };
    let Some(tag_name) = crate::expr::literal_text(&cx.src, tag_arg) else {
        if declaration_checks {
            diags.error(
                Code::EnumTagNameNotStatic,
                cx.src.span(tag_arg),
                "the enum tag name must be a string literal",
            );
        }
        return;
    };
    let content = content_arg.and_then(|arg| crate::expr::literal_text(&cx.src, arg));
    if let Some(arg) = content_arg
        && content.is_none()
    {
        if declaration_checks {
            diags.error(
                Code::EnumTagNameNotStatic,
                cx.src.span(arg),
                "the adjacent content name must be a string literal",
            );
        }
        return;
    }
    if let Some(content_name) = content.as_deref() {
        if declaration_checks && content_name == tag_name {
            diags.error(
                Code::EnumTagNameConflict,
                cx.src.span(content_arg.unwrap_or(tag_arg)),
                "the adjacent tag and content names must be distinct",
            );
        }
        if declaration_checks || view.is_none() {
            return;
        }
    }

    let rename_type = cx.names.item("std.serde", "Rename");
    let rename = cx.names.item("std.serde", "rename");
    let rename_all_type = cx.names.item("std.serde", "RenameAll");
    let rename_all = cx.names.item("std.serde", "rename_all");
    let rename_case = cx.names.item("std.serde", "RenameCase");
    let view_type_sources = view_facts
        .slots()
        .into_iter()
        .find_map(|(slot, sources)| (slot == Slot::Type).then_some(sources))
        .unwrap_or_default();
    let view_type_values = written_facts(cx, view_type_sources, item.def, traitless, view);
    let style_value = view_type_values
        .iter()
        .copied()
        .find(|value| concrete_fact(cx, *value, rename_all_type));
    let style = style_value
        .and_then(|value| rename_style_value(cx, value, rename_all_type, rename_all, rename_case));
    if let Some(value) = style_value
        && view.is_some_and(|head| value.owner == head.def)
        && style.is_none()
    {
        let site = rename_style_argument(cx, value, rename_all).unwrap_or(value.node);
        diags.error(
            Code::EnumTagNameNotStatic,
            cx.src.span(site),
            "a per-trait rename style on a tagged enum must name a `RenameCase` variant directly",
        );
        return;
    }
    let dynamic_style = style_value.is_some() && style.is_none();
    let variant_nodes: Vec<_> = hd_resolve::Src::child(decl, SyntaxKind::Block)
        .into_iter()
        .flat_map(NodeRef::children)
        .filter(|n| n.kind() == SyntaxKind::EnumVariant)
        .collect();
    for (variant_index, (variant, variant_node)) in variants.iter().zip(variant_nodes).enumerate() {
        let parameters: Vec<_> = hd_resolve::Src::child(variant_node, SyntaxKind::ParameterList)
            .into_iter()
            .flat_map(NodeRef::children)
            .filter(|n| n.kind() == SyntaxKind::Parameter)
            .collect();
        let positional = variant.fields.iter().all(|field| {
            cx.names
                .text(field.name)
                .starts_with(|c: char| c.is_ascii_digit())
        });
        if declaration_checks && positional && variant.fields.len() > 1 {
            let name = cx.names.text(variant.name);
            diags.error(
                Code::InvalidInternalTaggedVariant,
                cx.src.span(variant_node),
                &format!(
                    "`{name}` has multiple positional members; internal tagging cannot represent it"
                ),
            );
            continue;
        }
        if declaration_checks && positional && variant.fields.len() == 1 {
            let field = &variant.fields[0];
            if !internal_payload_allowed(cx, field.ty) {
                let site = parameters
                    .first()
                    .and_then(|p| p.children().find(|n| n.kind().is_type()))
                    .unwrap_or(variant_node);
                diags.error(
                    Code::InvalidInternalTaggedVariant,
                    cx.src.span(site),
                    &format!(
                        "`{}` wraps {}; an internally tagged newtype must wrap a struct or Map",
                        cx.names.text(variant.name),
                        hd_resolve::show_ty(&cx.names, field.ty)
                    ),
                );
            }
            continue;
        }
        for (member_index, (field, parameter)) in variant.fields.iter().zip(parameters).enumerate()
        {
            let source = hd_resolve::error_type::label(&cx.names, field);
            let member_sources = view_facts
                .slots()
                .into_iter()
                .find_map(|(slot, sources)| {
                    (slot == Slot::Member(variant_index, member_index)).then_some(sources)
                })
                .unwrap_or_default();
            let member_values = written_facts(cx, member_sources, item.def, traitless, view);
            let rename_value = member_values
                .iter()
                .copied()
                .find(|value| concrete_fact(cx, *value, rename_type));
            if view.is_some_and(|head| rename_value.is_some_and(|value| value.owner != head.def)) {
                continue;
            }
            let wire = match rename_value {
                Some(value) => {
                    let Some(name) = rename_name(cx, value, rename_type, rename, diags) else {
                        continue;
                    };
                    name
                }
                None if dynamic_style => continue,
                None => renamed(&source, style.as_deref()),
            };
            let conflicts = wire == tag_name || content.as_deref() == Some(wire.as_str());
            if conflicts {
                let view_site = view.and_then(|head| match rename_value {
                    Some(value) => (value.owner == head.def).then(|| cx.src.span(value.node)),
                    None => style_value
                        .filter(|value| value.owner == head.def)
                        .map(|value| cx.src.span(value.node)),
                });
                if view.is_some() && view_site.is_none() {
                    continue;
                }
                let site = view_site.unwrap_or_else(|| {
                    parameter
                        .name(&cx.src.parse.tokens)
                        .map_or_else(|| cx.src.span(parameter), |name| cx.src.token_span(name))
                });
                diags.error(
                    Code::EnumTagNameConflict,
                    site,
                    &format!("the member `{wire}` conflicts with the tagged enum field name"),
                );
            }
        }
    }
}

/// Checks the effective enum declaration metadata, and rejects direct
/// per-view tagging facts, once all exact local and module item heads are
/// available.
pub fn check_enum_tagging_decls(cx: &BodyCx<'_>, heads: &[Head<'_>], diags: &mut DiagBuf) {
    let serialize = cx.names.item("std.serde", "Serialize");
    let deserialize = cx.names.item("std.serde", "Deserialize");
    let tagging = cx.names.item("std.serde", "EnumTagging");
    for head in heads.iter().filter(|head| head.kind == HeadKind::Enum) {
        let Some(item) = cx.lookup.item(head.def) else {
            continue;
        };
        let traitless = heads.iter().find(|candidate| {
            matches!(
                cx.lookup.item(candidate.def).map(|item| &item.data),
                Some(ItemData::Impl { trait_, by: Some(_), self_ty, .. })
                    if *trait_ == DefId::NONE
                        && matches!(cx.names.pool.get(*self_ty), TyData::Adt { def, .. } if def == head.def)
            )
        });
        let declaration = crate::structure::declaration_facts(
            cx,
            item,
            head.node,
            traitless.map(|head| head.node),
        );
        check_enum_tagging(
            cx,
            item,
            head.node,
            traitless,
            None,
            &declaration,
            &declaration,
            true,
            diags,
        );
        let declaration_sources = declaration
            .slots()
            .into_iter()
            .find_map(|(slot, sources)| (slot == Slot::Type).then_some(sources))
            .unwrap_or_default();
        let declaration_has_tagging =
            written_facts(cx, declaration_sources, head.def, traitless, None)
                .into_iter()
                .any(|value| concrete_fact(cx, value, tagging));

        for view_head in heads.iter().filter(|candidate| {
            matches!(
                cx.lookup.item(candidate.def).map(|item| &item.data),
                Some(ItemData::Impl { trait_, by: Some(_), self_ty, .. })
                    if (*trait_ == serialize || *trait_ == deserialize)
                        && matches!(cx.names.pool.get(*self_ty), TyData::Adt { def, .. } if def == head.def)
            )
        }) {
            let Some(block) = crate::structure::block_facts(cx, view_head.def, view_head.node) else {
                continue;
            };
            let block_sources = block
                .slots()
                .into_iter()
                .find_map(|(slot, sources)| (slot == Slot::Type).then_some(sources))
                .unwrap_or_default();
            let block_values =
                written_facts(cx, block_sources, head.def, traitless, Some(view_head));
            let mut direct_tagging = false;
            for value in block_values.iter().copied() {
                if !concrete_fact(cx, value, tagging) {
                    continue;
                }
                direct_tagging = true;
                diags.error(
                    Code::EnumTaggingNotOnDeclaration,
                    cx.src.span(value.node),
                    "enum tagging belongs on the enum declaration or its no-trait metadata block",
                );
            }
            let lines = crate::derive::member_lines(&cx.src, view_head.node);
            let replacement = lines
                .iter()
                .find(|line| line.name == "Self" && !line.extend);
            if declaration_has_tagging && !direct_tagging
                && let Some(line) = replacement
            {
                diags.error(
                    Code::EnumTaggingNotOnDeclaration,
                    line.list.map_or_else(
                        || cx.src.span(view_head.node),
                        |list| cx.src.span(list),
                    ),
                    "a per-trait replacement cannot remove the enum declaration's tagging fact",
                );
            }
            if !declaration_has_tagging || replacement.is_some() {
                continue;
            }
            let mut view = declaration.clone();
            crate::structure::edit_facts(cx, item, &mut view, &lines);
            check_enum_tagging(
                cx,
                item,
                head.node,
                traitless,
                Some(view_head),
                &declaration,
                &view,
                false,
                diags,
            );
        }
    }
}

/// Checks every fact and metadata expression written on the declaration
/// `node` of `owner` and on its members, at check time, though nothing
/// reads them (`annot.metadata.eval-as-fact`, `annot.fact.eval.unread`,
/// `cli.build.facts-at-run-time`): its decorators, the decorators of its
/// variants, members, parameters and methods, and the elements and list
/// expressions of the member lines of a derivation block. Nothing is
/// evaluated; each expression is type-checked as the code of `owner`'s
/// module, in the same bodies that a `Structure` builds its facts from, so
/// that the same errors are reported here once and dropped there. This
/// includes the block-on ban (`annot.fact.block-on.direct-call`) and a typed
/// fact against its member's type (`annot.typed-fact.check`). The
/// decorators of a module-level function are its facts body, which
/// [`check_facts`] checks.
pub fn check_decl_facts(cx: &BodyCx<'_>, owner: DefId, node: NodeRef<'_>, diags: &mut DiagBuf) {
    let data = cx.lookup.item(owner).map(|i| &i.data);
    let is_fn = matches!(data, Some(ItemData::Fn(_)));
    let lines = match (cx.lookup.item(owner), data) {
        (Some(item), Some(ItemData::Data(_) | ItemData::Enum { .. })) => {
            crate::structure::declaration_facts(cx, item, node, None)
        }
        (_, Some(ItemData::Impl { by: Some(_), .. })) => {
            crate::structure::block_facts(cx, owner, node).unwrap_or_default()
        }
        _ => FactLines::default(),
    };
    // The decorators no slot holds: the declaration's own for a block,
    // trait, newtype or function (whose own are in its facts body), and
    // those of methods and parameters.
    let mut held = Vec::new();
    for (_, sources) in lines.slots() {
        for source in sources {
            if let FactSource::Value(e) = *source {
                held.push(e.index());
            }
        }
    }
    let mut places = Vec::new();
    crate::dupfacts::decorated(node, &mut places);
    let rest: Vec<FactSource<'_>> = places
        .into_iter()
        .filter(|p| !(is_fn && p.index() == node.index()))
        .flat_map(|p| fact_lines(cx, p))
        .filter(|e| !held.contains(&e.index()))
        .map(FactSource::Value)
        .collect();
    let def = cx.names.member(owner, PathKind::Hidden, "facts");
    let slots = lines.slots();
    let groups = slots
        .iter()
        .map(|(slot, sources)| (lines.target(*slot), *sources))
        .chain(std::iter::once((FactTarget::Other, rest.as_slice())));
    for (target, sources) in groups {
        if sources.is_empty() {
            continue;
        }
        // A fact the checker cannot type yet leaves its list unchecked.
        // The declaration is one item: its first list that runs out of
        // fuel reports it, and the rest are not checked.
        if let Err(e) = fact_list_steps(cx, (owner, def), sources, (target, ""), node, diags)
            && is_out_of_fuel(&e)
        {
            let _ = too_complex(cx, (def, BodyKind::Fact), node, diags);
            break;
        }
    }
}

impl Ck<'_, '_> {
    /// A list of facts that is an expression, not a list literal, or a
    /// spread's operand in a literal (`annot.line.right-typed`,
    /// `annot.line.reference-elements`): `e` keeps its list element type,
    /// and each concrete value is referenced before it is stored in `Facts`. A value that is not a list is
    /// `not_list` (`annot.line.right.not-list`); one of a different element
    /// type is a type mismatch. `None` when it does not check.
    fn fact_list(
        &mut self,
        e: NodeRef<'_>,
        not_list: Code,
        items_ty: Ty,
    ) -> StageResult<Option<Ref>> {
        let pool = self.pool();
        let (r, t) = self.expr(e, Some(items_ty))?;
        let t = self.infer.resolve(pool, t);
        if self.list_item(t).is_none() {
            if !matches!(self.strip_mut(t), Ty::NEVER | Ty::POISON) {
                let msg = format!("expected a list of facts, found {}", self.show(t));
                self.err(not_list, e, &msg);
            }
            return Ok(None);
        }
        let mark = self.diags.mark();
        let r = self.coerce(
            r,
            t,
            items_ty,
            e,
            "member line list",
            ExpectPosition::General,
        );
        Ok((self.diags.errors_since(mark) == 0).then_some(r))
    }

    /// The declared result type of the function that the decorator line
    /// or member-line element `e` calls or names
    /// (`annot.decorator.bare-call-defaults`), or `None` when `e` names no
    /// function, as for a method chain.
    fn called_result(&self, e: NodeRef<'_>) -> Option<Ty> {
        let callee = match e.kind() {
            SyntaxKind::CallExpr => e.children().next(),
            _ => Some(e),
        };
        let callee = match callee {
            Some(c) if c.kind() == SyntaxKind::TypeArgsExpr => c.children().next(),
            c => c,
        }?;
        if !is_path(callee) {
            return None;
        }
        let def = self.resolve_path(&self.segments_expr(callee))?;
        match self.cx.lookup.item(def).map(|i| &i.data) {
            Some(ItemData::Fn(sig)) => Some(sig.ret),
            _ => None,
        }
    }

    /// Whether `def` is a typed fact type (`annot.typed-fact.declare`).
    fn is_typed_fact(&self, def: DefId) -> bool {
        self.cx
            .lookup
            .item(def)
            .is_some_and(|i| i.fact_pattern.is_some())
    }

    /// The typed fact type of the value `e` writes
    /// (`annot.typed-fact.declare`): the constructor of the called
    /// function's declared result type, or else of the type `e` checks at
    /// with no expected type, in a trial whose effects are undone. So a
    /// method chain, as in `length(3, 40).message("m")`, then checks at the
    /// fact type as its expected type, as a `let` binding would
    /// (`annot.typed-fact.check.inferred`).
    fn typed_fact_of(&mut self, e: NodeRef<'_>, single: bool) -> StageResult<Option<DefId>> {
        let pool = self.cx.names.pool;
        if let Some(TyData::Adt { def, .. }) = self.called_result(e).map(|t| pool.get(t)) {
            return Ok(self.is_typed_fact(def).then_some(def));
        }
        let mut found = None;
        self.trial(|ck| {
            let (_, t) = if single {
                ck.fact_value(e, None)?
            } else {
                ck.expr(e, None)?
            };
            let pool = ck.pool();
            if let TyData::Adt { def, .. } = pool.get(ck.infer.resolve(pool, t))
                && ck.is_typed_fact(def)
            {
                found = Some(def);
            }
            Ok(())
        })?;
        Ok(found)
    }

    /// The type `target` stands for in the body of `owner`, or `None`
    /// where a typed fact is left out.
    fn fact_target(&mut self, target: FactTarget, owner: DefId) -> StageResult<Option<Ty>> {
        Ok(match target {
            FactTarget::Fn => Some(self.own_fn_ty(owner)?),
            FactTarget::Field(t) => Some(t),
            FactTarget::Other => None,
        })
    }

    /// Matches the pattern of the typed fact type `fact` against `target`
    /// (`annot.typed-fact.infer`): the arguments it infers, one per
    /// parameter of `fact` with a fresh variable for each the pattern does
    /// not mention, or `None` once a mismatch or a broken bound is
    /// reported at `at`.
    pub(crate) fn match_fact_pattern(
        &mut self,
        fact: DefId,
        target: Ty,
        at: NodeRef<'_>,
    ) -> StageResult<Option<Vec<Ty>>> {
        let pool = self.pool();
        let Some(item) = self.cx.lookup.item(fact) else {
            return Ok(None);
        };
        let Some(pattern) = item.fact_pattern else {
            return Ok(None);
        };
        let generics = item.generics.clone();
        let name = self.cx.names.text(item.name).to_owned();
        let vars: Vec<Ty> = generics
            .iter()
            .map(|_| self.infer.fresh(pool, VarKind::General))
            .collect();
        let want = subst_owner(pool, fact, &vars, pattern);
        // A `mut Suspend[T]` target passes as an argument does: whole, or
        // weakened to its readonly form (`annot.typed-fact.infer.call`,
        // `types.mut.weaken`).
        let target = self.norm_ty(target);
        let fits = self.unify_now(target, want)
            || matches!(pool.get(target), TyData::Mut(inner) if self.unify_now(inner, want));
        if !fits {
            let msg = format!(
                "the type {} does not fit the pattern of the typed fact type `{name}`",
                self.show(target)
            );
            self.err(Code::TypeMismatch, at, &msg);
            return Ok(None);
        }
        // A function target passes as a function value does, so its row
        // must fit the pattern's (`annot.typed-fact.infer.row`).
        let before = self.diags.len();
        if let (TyData::Fn { row: got, .. }, TyData::Fn { row: wanted, .. }) = (
            pool.get(self.infer.resolve(pool, self.strip_mut(target))),
            pool.get(self.infer.resolve(pool, want)),
        ) {
            self.fit_row(got, wanted, at, "a typed fact's target");
        }
        if self.diags.len() != before {
            return Ok(None);
        }
        // The bounds of the parameters the pattern mentions
        // (`annot.typed-fact.check.bounds`).
        let mentioned = std::cell::RefCell::new(Vec::new());
        pool.subst(pattern, &|p: ParamRef| {
            if p.owner == fact {
                mentioned.borrow_mut().push(p.index as usize);
            }
            None
        });
        let mentioned = mentioned.into_inner();
        for (i, g) in generics.iter().enumerate() {
            if !mentioned.contains(&i) {
                continue;
            }
            for b in &g.bounds {
                if let TyData::TraitValue { def, args, .. } =
                    pool.get(subst_owner(pool, fact, &vars, *b))
                {
                    let tref = TraitRef {
                        trait_: def,
                        self_ty: vars[i],
                        args,
                    };
                    self.require_ref(tref, at)?;
                }
            }
        }
        Ok((self.diags.len() == before).then_some(vars))
    }

    /// The settled type a fact expression has without a target-provided
    /// expected type. Context-only builders leave an inference variable and
    /// return `None`; a value such as `range(1, 10)` settles to `Range[i32]`.
    fn settled_fact_type(&mut self, e: NodeRef<'_>, single: bool) -> StageResult<Option<Ty>> {
        let mut found = None;
        let failed = self.trial(|ck| {
            let (_, ty) = if single {
                ck.fact_value(e, None)?
            } else {
                ck.expr(e, None)?
            };
            let ty = ck.zonk(ty);
            let pool = ck.pool();
            if !pool.has_poison(ty) && !pool.has_infer(ty) {
                found = Some(ty);
            }
            Ok(())
        })?;
        Ok((!failed).then_some(found).flatten())
    }

    /// Whether matching `fact` against `target` produces the concrete fact
    /// type `written`, without changing inference or reporting a diagnostic.
    fn fact_type_fits_target(
        &mut self,
        fact: DefId,
        target: Ty,
        written: Ty,
        at: NodeRef<'_>,
    ) -> StageResult<bool> {
        let mut fits = false;
        self.trial(|ck| {
            let Some(vars) = ck.match_fact_pattern(fact, target, at)? else {
                return Ok(());
            };
            let inferred = ck.pool().intern_ty(&TyData::Adt {
                def: fact,
                args: ck.pool().list(&vars),
            });
            fits = ck.unify_now(written, inferred);
            Ok(())
        })?;
        Ok(fits)
    }

    /// A member of type `T?` offers the payload `T` when the fact expression
    /// already has the fact type that matching against `T` produces. A
    /// context-only builder has no settled type, so it keeps the ordinary
    /// whole-member match.
    fn match_fact_target(
        &mut self,
        fact: DefId,
        target: Ty,
        at: NodeRef<'_>,
        optional_written: Option<Ty>,
    ) -> StageResult<Option<Vec<Ty>>> {
        if let Some(written) = optional_written {
            let target = self.norm_ty(target);
            if let TyData::Option(payload) = self.pool().get(target)
                && self.fact_type_fits_target(fact, payload, written, at)?
            {
                return self.match_fact_pattern(fact, payload, at);
            }
        }
        self.match_fact_pattern(fact, target, at)
    }

    /// `h.fact::[D]()` on the handle type `handle`, `Field[S, F]`, whose
    /// written type argument is `m`: the pattern of the typed fact type
    /// `D` is matched against `F` as at attach time, and `m` is `D` at the
    /// arguments the match infers (`annot.handle.fact.pattern`). Arguments
    /// written on `D` must be those (`annot.handle.fact.pattern.written`).
    pub(crate) fn handle_fact_type(
        &mut self,
        handle: Ty,
        m: Ty,
        n: NodeRef<'_>,
    ) -> StageResult<()> {
        let pool = self.pool();
        let handle = self.infer.resolve(pool, self.strip_mut(handle));
        let TyData::Adt { args, .. } = pool.get(handle) else {
            return Ok(());
        };
        let Some(&member) = pool.list_items(args).get(1) else {
            return Ok(());
        };
        let m = self.infer.resolve(pool, m);
        let TyData::Adt { def: fact, .. } = pool.get(m) else {
            return Ok(());
        };
        if self
            .cx
            .lookup
            .item(fact)
            .is_none_or(|i| i.fact_pattern.is_none())
        {
            return Ok(());
        }
        let Some(vars) = self.match_fact_target(fact, member, n, Some(m))? else {
            return Ok(());
        };
        let inferred = pool.intern_ty(&TyData::Adt {
            def: fact,
            args: pool.list(&vars),
        });
        if !self.unify_now(m, inferred) {
            let msg = format!(
                "the handle's member type infers {}, but the call writes {}",
                self.show(inferred),
                self.show(m)
            );
            self.err(Code::TypeMismatch, n, &msg);
        }
        Ok(())
    }

    /// Unifies `a` with `b`, or leaves both as they were.
    fn unify_now(&mut self, a: Ty, b: Ty) -> bool {
        let snap = self.infer.snapshot();
        let ok = self.infer.unify(self.pool(), a, b).is_ok();
        if !ok {
            self.infer.rollback(snap);
        }
        ok
    }

    /// The value of the typed fact `e`, of the typed fact type `fact`, on a
    /// target of type `target`: it checks like `let f: D[A] = v`, where `A`
    /// is what the pattern infers from `target` (`annot.typed-fact.check`).
    /// `None` once an error is reported at `e`, so nothing follows from it.
    fn typed_fact(
        &mut self,
        fact: DefId,
        e: NodeRef<'_>,
        target: Ty,
        single: bool,
        optional_payload: bool,
    ) -> StageResult<Option<(Ref, Ty)>> {
        let pool = self.pool();
        let optional_written = if optional_payload {
            self.settled_fact_type(e, single)?
        } else {
            None
        };
        let Some(vars) = self.match_fact_target(fact, target, e, optional_written)? else {
            return Ok(None);
        };
        let want = pool.intern_ty(&TyData::Adt {
            def: fact,
            args: pool.list(&vars),
        });
        let before = self.diags.len();
        let (r, t) = if single {
            self.fact_value(e, Some(want))?
        } else {
            self.expr(e, Some(want))?
        };
        if self.diags.len() != before {
            return Ok(None);
        }
        let r = self.coerce(r, t, want, e, "fact", ExpectPosition::General);
        Ok((self.diags.len() == before).then_some((r, want)))
    }

    /// The concrete type of the fact `e` written for `target`, or `None`
    /// once it does not check (the error is reported elsewhere) or its type
    /// is not settled. `single` marks a decorator's value.
    pub(crate) fn fact_type(
        &mut self,
        e: NodeRef<'_>,
        (target, owner): (FactTarget, DefId),
        single: bool,
    ) -> Option<Ty> {
        let before = self.diags.len();
        let ty = if let Some(fact) = self.typed_fact_of(e, single).ok().flatten() {
            let optional_payload = matches!(target, FactTarget::Field(_));
            let target = self.fact_target(target, owner).ok().flatten()?;
            self.typed_fact(fact, e, target, single, optional_payload)
                .ok()
                .flatten()?
                .1
        } else if single {
            self.fact_value(e, None).ok()?.1
        } else {
            self.expr(e, None).ok()?.1
        };
        if self.diags.len() != before {
            return None;
        }
        let ty = self.zonk(ty);
        let pool = self.pool();
        (!pool.has_poison(ty) && !pool.has_infer(ty)).then_some(ty)
    }

    /// The value one decorator line attaches, expected to be `want`.
    fn fact_value(&mut self, e: NodeRef<'_>, want: Option<Ty>) -> StageResult<(Ref, Ty)> {
        if e.kind() == SyntaxKind::NameExpr {
            let text = self.cx.src.text(self.cx.src.first(e)).to_owned();
            if let Some(Named::Item(def)) = self.scope_name(&text)
                && matches!(
                    self.cx.lookup.item(def).map(|i| &i.data),
                    Some(ItemData::Fn(_))
                )
            {
                let args = Args {
                    positional: vec![],
                    spread: None,
                    after_spread: vec![],
                    named: vec![],
                    trailing: None,
                };
                return self.call_item(def, &[], &args, e, false, want);
            }
        }
        self.expr(e, want)
    }

    /// `facts_of(f)`: the call of `f`'s facts body, for a module-level
    /// function `f` named by the one argument (`annot.facts-of.target`).
    pub(crate) fn facts_of_call(
        &mut self,
        args: &Args<'_>,
        n: NodeRef<'_>,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
        let (facts_ty, items_ty) = facts_types(self.cx)?;
        let plain = args.spread.is_none()
            && args.after_spread.is_empty()
            && args.named.is_empty()
            && args.trailing.is_none();
        let (true, [arg]) = (plain, args.positional.as_slice()) else {
            self.err(Code::ArgumentCount, n, "`facts_of` takes one argument");
            return Ok((Ref(NONE), Ty::NEVER));
        };
        let segs = self.segments_expr(*arg);
        let Some(first) = segs.first() else {
            return Ok(self.bad_facts_target(*arg, "this argument is not a name"));
        };
        if self.find_local(self.cx.names.syms.intern(first)).is_some() {
            return Ok(self.bad_facts_target(*arg, "a local binding is not a declaration"));
        }
        if self.is_poison_name(first) {
            return Ok(self.poison_value(n));
        }
        let Some(def) = self.resolve_path(&segs) else {
            let sym = self.cx.names.syms.intern(first);
            let global = {
                let init = self.cx.init.borrow();
                init.globals.contains_key(&sym) || init.earlier.contains_key(&sym)
            };
            if self.scope_name(first).is_none() && !global {
                let msg = format!("`{first}` is not defined");
                self.err(Code::UnknownName, *arg, &msg);
                return Ok((Ref(NONE), Ty::NEVER));
            }
            return Ok(self.bad_facts_target(*arg, "this name is not a function"));
        };
        if !matches!(
            self.cx.lookup.item(def).map(|i| &i.data),
            Some(ItemData::Fn(_))
        ) {
            return Ok(self.bad_facts_target(*arg, "this name is not a function"));
        }
        // A function with no value-bearing decorator holds no facts.
        if !self.cx.lookup.item(def).is_some_and(|i| i.has_facts) {
            let rec = self.b.refs_record(&[]);
            let list = self.b.emit(Tag::NewList, NONE, rec, items_ty, n.index());
            let rec = self.b.refs_record(&[list]);
            let value = self.b.emit(Tag::NewStruct, NONE, rec, facts_ty, n.index());
            return Ok((value, facts_ty));
        }
        let body = self.cx.names.member(def, PathKind::Hidden, "facts");
        let targs = pool.list(&[]);
        let a = self.b.refs_record(&[Ref(body.raw()), Ref(targs.0)]);
        let earlier = self.b.refs_record(&[]);
        let call = self
            .b
            .emit(Tag::DefaultCall, a, earlier, facts_ty, n.index());
        Ok((call, facts_ty))
    }

    /// `facts_of` used as a value, or on a target that is not a
    /// module-level function (`annot.facts-of.target.error`).
    pub(crate) fn bad_facts_target(&mut self, at: NodeRef<'_>, why: &str) -> (Ref, Ty) {
        let msg = format!("`facts_of` takes a module-level function: {why}");
        self.err(Code::InvalidFactsOfTarget, at, &msg);
        (Ref(NONE), Ty::NEVER)
    }
}
