//! The methods of `@derive` on a newtype (spec 09
//! `trait.derive.newtype.base`): each applies the base type's method to the
//! wrapped values and wraps the result back. Header lowering declares one
//! method per method of the trait, with the trait's signature at `Self` =
//! the newtype (`hd_resolve::lower`, as for a delegation). Erased newtypes
//! forward to their base implementation directly, so no adapter bodies are
//! built here; only the position findings are reported, by the same
//! forwarder that writes a delegation's (`crate::delegate`).

use hd_base::{DefId, LocalId, StageResult};
use hd_diag::DiagBuf;
use hd_intern::PathKind;
use hd_resolve::{ImplKind, Item, ItemData};
use hd_syntax::NodeRef;
use hd_tir::ir::{BlockMark, IntrinsicOp, LoopMark, NONE, PrimOp, Ref, Tag, TirSink, local_flags};
use hd_types::AdtId;
use hd_types::{ParamRef, Prim, Ty, TyData};

use crate::body::{BodyCx, Ck, ExpectPosition};
use crate::delegate::{Forwarded, Forwarder};

/// One problem with a derived newtype's forwarding: the derived
/// implementation, and what stops the method.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeriveFinding {
    pub impl_: DefId,
    pub message: String,
}

/// The problems with the methods of every `@derive` on the newtype
/// `newtype`, declared at `node`: a method with `Self` anywhere but the
/// receiver, `Self`, `Self?`, `Result[Self, E]` or `List[Self]` is one
/// finding (`trait.derive.newtype.self-error`). Erased newtypes forward
/// to their base implementation directly, so no adapter bodies are
/// built; the position analysis still runs to report its findings.
/// `derived` holds the module's derived impls that target `newtype`.
pub fn derived_newtype_findings<'i>(
    cx: &BodyCx<'_>,
    newtype: DefId,
    derived: impl IntoIterator<Item = &'i Item>,
    node: NodeRef<'_>,
    diags: &mut DiagBuf,
) -> StageResult<Vec<DeriveFinding>> {
    let names = &cx.names;
    let pool = names.pool;
    let mut findings = Vec::new();
    for it in derived {
        let ItemData::Impl {
            trait_,
            trait_args,
            self_ty,
            methods,
            kind: ImplKind::Derived,
            ..
        } = &it.data
        else {
            continue;
        };
        let Some((def, args)) = pool.adt(*self_ty) else {
            continue;
        };
        if def != newtype {
            continue;
        }
        let Some(ItemData::Newtype(inner, _)) = cx.lookup.item(def).map(|i| &i.data) else {
            continue;
        };
        let ca = pool.list_items(args);
        let base = pool.subst(*inner, &|q: ParamRef| {
            (q.owner == newtype)
                .then(|| ca.get(q.index as usize).copied())
                .flatten()
        });
        for &(name, def) in methods {
            let Some(sig) = cx.lookup.item(def).and_then(|i| i.sig()).cloned() else {
                continue;
            };
            let fwd = Forwarder {
                trait_: *trait_,
                trait_args: *trait_args,
                part: base,
                part_field: None,
                newtype: Some(*self_ty),
                def,
                tm: names.member(*trait_, PathKind::Member, names.text(name)),
            };
            match fwd.body(cx, &sig, node, diags)? {
                Forwarded::Body(_) => {}
                Forwarded::Position(message) => findings.push(DeriveFinding {
                    impl_: it.def,
                    message,
                }),
                Forwarded::Nothing => {}
            }
        }
    }
    Ok(findings)
}

/// How a derived newtype's method values cross between the newtype and
/// its base type: the receiver and `Self` unwrap and wrap, and so do the
/// `Self` inside `Self?`, `Result[Self, E]` and `List[Self]`
/// (`trait.derive.newtype.self-positions`). Every other position holds
/// the same type on both sides.
pub(crate) struct Conversion {
    pub newtype: Ty,
    pub base: Ty,
}

pub(crate) struct ConversionPlan {
    layers: Vec<ConversionLayer>,
    leaf: ConversionLeaf,
}

#[derive(Clone, Copy)]
enum ConversionLeaf {
    Same(Ty, Ty),
    Wrap(Ty, Ty),
    Unwrap(Ty, Ty),
}

enum ConversionLayer {
    Variant {
        from: Ty,
        to: Ty,
        element: Ty,
        other: Option<Ty>,
    },
    List {
        from: Ty,
        to: Ty,
        element: Ty,
    },
}

enum ConvertWork {
    Enter(Ref, std::vec::IntoIter<ConversionLayer>),
    Variant(BlockMark, Ref, Ty, Option<Ty>),
    List(ListConversion),
}

#[derive(Clone, Copy)]
struct ListConversion {
    out: LocalId,
    index: LocalId,
    loop_: LoopMark,
    body: BlockMark,
    then: BlockMark,
    condition: Ref,
    ty: Ty,
}

impl Conversion {
    /// Whether `from` and `to` are the same local-reference receiver at the
    /// newtype and its erased base. A reference elsewhere in a signature is
    /// still an unsupported `Self` position.
    pub(crate) fn local_receiver(&self, ck: &Ck<'_, '_>, from: Ty, to: Ty) -> bool {
        let pool = ck.pool();
        match (pool.get(from), pool.get(to)) {
            (
                TyData::Ref {
                    inner: a,
                    mutable: am,
                    counted: false,
                },
                TyData::Ref {
                    inner: b,
                    mutable: bm,
                    counted: false,
                },
            ) => am == bm && self.is_unwrap(ck.resolved(a), ck.resolved(b)),
            _ => false,
        }
    }

    /// A checked conversion shape. Only its validated layers can reach apply.
    pub(crate) fn can(&self, ck: &Ck<'_, '_>, from: Ty, to: Ty) -> Option<ConversionPlan> {
        let pool = ck.pool();
        let known = ck.cx.names.known;
        let leaf_at = |from, to, f, t| {
            if f == t {
                Some(ConversionLeaf::Same(from, to))
            } else if self.is_wrap(f, t) {
                Some(ConversionLeaf::Wrap(from, to))
            } else if self.is_unwrap(f, t) {
                Some(ConversionLeaf::Unwrap(from, to))
            } else {
                None
            }
        };
        let (f, t) = (ck.resolved(from), ck.resolved(to));
        if let Some(leaf) = leaf_at(from, to, f, t) {
            hd_types::work::type_node();
            return Some(ConversionPlan {
                layers: Vec::new(),
                leaf,
            });
        }
        let mut layers = Vec::new();
        let mut leaf = ConversionLeaf::Same(from, to);
        let fits = hd_base::stack::all((from, to), |(from, to), stack| {
            hd_types::work::type_node();
            let (f, t) = (ck.resolved(from), ck.resolved(to));
            if let Some(done) = leaf_at(from, to, f, t) {
                leaf = done;
                return true;
            }
            match (pool.get(f), pool.get(t)) {
                (TyData::Adt(AdtId::Declared(d), a), TyData::Adt(AdtId::Declared(e), b))
                    if d == e =>
                {
                    let (a, b) = (pool.list_items(a), pool.list_items(b));
                    if d == known.option || (d == known.result && a[1] == b[1]) {
                        layers.push(ConversionLayer::Variant {
                            from,
                            to,
                            element: a[0],
                            other: (d == known.result).then(|| a[1]),
                        });
                    } else if d == known.list {
                        layers.push(ConversionLayer::List {
                            from,
                            to,
                            element: a[0],
                        });
                    } else {
                        return false;
                    }
                    stack.push((a[0], b[0]));
                    true
                }
                _ => false,
            }
        });
        fits.then_some(ConversionPlan { layers, leaf })
    }

    fn is_wrap(&self, from: Ty, to: Ty) -> bool {
        from == self.base && to == self.newtype
    }

    fn is_unwrap(&self, from: Ty, to: Ty) -> bool {
        from == self.newtype && to == self.base
    }

    /// Apply the checked shape; no invalid constructor state is representable.
    pub(crate) fn apply(
        ck: &mut Ck<'_, '_>,
        r: Ref,
        plan: ConversionPlan,
        node: NodeRef<'_>,
    ) -> Ref {
        let ConversionPlan { layers, leaf } = plan;
        let at = node.index();
        if layers.is_empty() {
            hd_types::work::type_node();
            return Self::apply_leaf(ck, r, leaf, node);
        }
        let mut converted = r;
        hd_base::stack::all(ConvertWork::Enter(r, layers.into_iter()), |work, stack| {
            converted = match work {
                ConvertWork::Enter(input, mut layers) => {
                    hd_types::work::type_node();
                    match layers.next() {
                        Some(ConversionLayer::Variant {
                            from,
                            to,
                            element,
                            other,
                        }) => {
                            let (f, t) = (ck.resolved(from), ck.resolved(to));
                            if f == t {
                                ck.coerce(
                                    input,
                                    from,
                                    to,
                                    node,
                                    "a forwarded value",
                                    ExpectPosition::General,
                                )
                            } else {
                                let block = ck.b.open_block();
                                let element = ck.resolved(element);
                                let other = other.map(|t| ck.resolved(t));
                                let value =
                                    ck.b.payload(input, u32::from(other.is_none()), 0, element, at);
                                stack.push(ConvertWork::Variant(block, input, t, other));
                                stack.push(ConvertWork::Enter(value, layers));
                                value
                            }
                        }
                        Some(ConversionLayer::List { from, to, element }) => {
                            let (f, t) = (ck.resolved(from), ck.resolved(to));
                            if f == t {
                                ck.coerce(
                                    input,
                                    from,
                                    to,
                                    node,
                                    "a forwarded value",
                                    ExpectPosition::General,
                                )
                            } else {
                                let element = ck.resolved(element);
                                let (value, frame) =
                                    Self::start_list(ck, input, (f, t), element, node);
                                stack.push(ConvertWork::List(frame));
                                stack.push(ConvertWork::Enter(value, layers));
                                value
                            }
                        }
                        None => Self::apply_leaf(ck, input, leaf, node),
                    }
                }
                ConvertWork::Variant(block, input, ty, other) => {
                    let tag = u32::from(other.is_none());
                    let rec = ck.b.refs_record(&[converted]);
                    let made = ck.b.emit(Tag::NewVariant, tag, rec, ty, at);
                    let then = ck.b.close_block(block, Some(made), ty, at);
                    let block = ck.b.open_block();
                    let rec = if let Some(t) = other {
                        let value = ck.b.payload(input, 1 - tag, 0, t, at);
                        ck.b.refs_record(&[value])
                    } else {
                        ck.b.refs_record(&[])
                    };
                    let made = ck.b.emit(Tag::NewVariant, 1 - tag, rec, ty, at);
                    let other = ck.b.close_block(block, Some(made), ty, at);
                    let rec = ck.b.refs_record(&[Ref(tag), then, other]);
                    ck.b.emit(Tag::SwitchTag, input.0, rec, ty, at)
                }
                ConvertWork::List(frame) => Self::finish_list(ck, converted, frame, node),
            };
            true
        });
        converted
    }

    fn apply_leaf(ck: &mut Ck<'_, '_>, r: Ref, leaf: ConversionLeaf, node: NodeRef<'_>) -> Ref {
        let at = node.index();
        match leaf {
            ConversionLeaf::Same(from, to) => ck.coerce(
                r,
                from,
                to,
                node,
                "a forwarded value",
                ExpectPosition::General,
            ),
            ConversionLeaf::Unwrap(from, to) => {
                if ck.resolved(from) == ck.resolved(to) {
                    ck.coerce(
                        r,
                        from,
                        to,
                        node,
                        "a forwarded value",
                        ExpectPosition::General,
                    )
                } else {
                    ck.b.field(r, 0, to, at)
                }
            }
            ConversionLeaf::Wrap(from, to) => {
                if ck.resolved(from) == ck.resolved(to) {
                    return ck.coerce(
                        r,
                        from,
                        to,
                        node,
                        "a forwarded value",
                        ExpectPosition::General,
                    );
                }
                let t = ck.resolved(to);
                let rec = ck.b.refs_record(&[r]);
                let made = ck.b.emit(Tag::NewStruct, NONE, rec, t, at);
                ck.coerce(
                    made,
                    t,
                    to,
                    node,
                    "a wrapped value",
                    ExpectPosition::General,
                )
            }
        }
    }

    /// A list of `from`'s elements as a list of `to`'s, by a counting
    /// loop: each element converts, then joins the new list.
    fn start_list(
        ck: &mut Ck<'_, '_>,
        r: Ref,
        (list_from, list_to): (Ty, Ty),
        from: Ty,
        node: NodeRef<'_>,
    ) -> (Ref, ListConversion) {
        let at = node.index();
        let usize_ty = Ty::prim(Prim::Usize);
        let out_ty = list_to;
        let sym = |ck: &Ck<'_, '_>, s: &str| ck.cx.names.syms.intern(s);
        let (src_s, out_s, i_s, n_s) = (
            sym(ck, "$src"),
            sym(ck, "$out"),
            sym(ck, "$i"),
            sym(ck, "$len"),
        );
        let src_l = ck.b.local(list_from, src_s, local_flags::ASSIGNED, at);
        let out_l = ck.b.local(out_ty, out_s, local_flags::ASSIGNED, at);
        let i_l = ck.b.local(usize_ty, i_s, local_flags::ASSIGNED, at);
        let n_l = ck.b.local(usize_ty, n_s, local_flags::ASSIGNED, at);
        ck.set_local(src_l, r, at);
        let empty = ck.b.refs_record(&[]);
        let fresh = ck.b.emit(Tag::NewList, NONE, empty, out_ty, at);
        ck.set_local(out_l, fresh, at);
        let zero = ck.b.const_value(usize_ty, 0);
        ck.set_local(i_l, zero, at);
        let s = ck.b.get(src_l, list_from, at);
        let rec = ck.b.refs_record(&[s]);
        let len = ck.b.emit(
            Tag::Intrinsic,
            IntrinsicOp::ListLen as u32,
            rec,
            usize_ty,
            at,
        );
        ck.set_local(n_l, len, at);
        let lp = ck.b.open_loop();
        let lb = ck.b.open_block();
        let (i, n) = (ck.b.get(i_l, usize_ty, at), ck.b.get(n_l, usize_ty, at));
        let below = ck.b.prim(PrimOp::Lt as u32, &[i, n], Ty::BOOL, at);
        let tb = ck.b.open_block();
        let s = ck.b.get(src_l, list_from, at);
        let i = ck.b.get(i_l, usize_ty, at);
        let item = ck.b.element(s, i, from, at);
        (
            item,
            ListConversion {
                out: out_l,
                index: i_l,
                loop_: lp,
                body: lb,
                then: tb,
                condition: below,
                ty: list_to,
            },
        )
    }

    fn finish_list(ck: &mut Ck<'_, '_>, c: Ref, frame: ListConversion, node: NodeRef<'_>) -> Ref {
        let ListConversion {
            out: out_l,
            index: i_l,
            loop_: lp,
            body: lb,
            then: tb,
            condition: below,
            ty: list_to,
        } = frame;
        let at = node.index();
        let usize_ty = Ty::prim(Prim::Usize);
        let out_ty = list_to;
        let o = ck.b.get(out_l, out_ty, at);
        let rec = ck.b.refs_record(&[o, c]);
        ck.b.emit(
            Tag::Intrinsic,
            IntrinsicOp::ListPush as u32,
            rec,
            Ty::VOID,
            at,
        );
        let i = ck.b.get(i_l, usize_ty, at);
        let one = ck.b.const_value(usize_ty, 1);
        let next = ck.b.prim(PrimOp::Add as u32, &[i, one], usize_ty, at);
        ck.set_local(i_l, next, at);
        let then = ck.b.close_block(tb, None, Ty::VOID, at);
        let eb = ck.b.open_block();
        ck.b.emit(Tag::Break, lp.0.raw(), NONE, Ty::NEVER, at);
        let els = ck.b.close_block(eb, None, Ty::VOID, at);
        let rec = ck.b.refs_record(&[then, els]);
        ck.b.emit(Tag::If, below.0, rec, Ty::VOID, at);
        let body = ck.b.close_block(lb, None, Ty::VOID, at);
        ck.b.close_loop(lp, body, Ty::VOID, at);
        let o = ck.b.get(out_l, out_ty, at);
        ck.coerce(
            o,
            out_ty,
            list_to,
            node,
            "a rewrapped list",
            ExpectPosition::General,
        )
    }
}
