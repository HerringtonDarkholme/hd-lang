//! Omitted result types (checking-and-tir.md §4.13.1 "Omitted result
//! types (M1)"). A non-public function or inherent method written without
//! `-> T` (spec 07 `fn.decl.result-omitted-private`) takes its result from
//! its body: the least common type of the final value and every `return`
//! operand, `void` when the body produces no value
//! (`fn.decl.result-inferred`, `fn.decl.result-inferred.common`,
//! `fn.decl.result-inferred.void`). Such a body is checked before its
//! callers: in source order, and depth first when a body reads the result
//! of one that is not done. A callee still being checked closes a cycle
//! (`fn.decl.omitted-cycle`), reported once at the cycle's member that
//! comes first in source order (`fn.decl.omitted-cycle.report`).

use std::collections::HashMap;

use hd_base::{DefId, NodeIdx, StageResult};
use hd_diag::{Code, DiagBuf};
use hd_resolve::{FnSig, ImplKind, ItemData, Src};
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_tir::Body;
use hd_types::Ty;

use crate::body::{BodyCx, Ck, check_fn_body};

enum State {
    /// Not checked yet: its declaration.
    Pending(NodeIdx),
    /// Its body is being checked.
    Active(NodeIdx),
    /// Checked: its result, and its body with the body's diagnostics until
    /// `Body(m)` takes them.
    Done {
        ret: Ty,
        body: Option<Box<(StageResult<Body>, DiagBuf)>>,
    },
}

/// A module's private callables with omitted result types.
#[derive(Default)]
pub struct Results {
    state: HashMap<DefId, State>,
    /// The callables in source order.
    order: Vec<DefId>,
    /// The callables being checked, innermost last.
    active: Vec<DefId>,
    /// The `recursive-function-needs-result-type` reports.
    cycles: DiagBuf,
    reported: Vec<DefId>,
    /// Whether a checked body or a cycle reported an error.
    errors: bool,
}

impl Results {
    /// Whether an inferred body or a cycle reported an error, so a body
    /// whose types hold poison from it adds no `cannot-infer-type`.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.errors
    }

    /// One report per cycle, at its member first in source order.
    fn cycle(&mut self, cx: &BodyCx<'_>, def: DefId) {
        let Some(at) = self.active.iter().position(|d| *d == def) else {
            return;
        };
        let Some(first) = self.active[at..]
            .iter()
            .copied()
            .min_by_key(|d| self.order.iter().position(|o| o == d))
        else {
            return;
        };
        self.errors = true;
        if self.reported.contains(&first) {
            return;
        }
        self.reported.push(first);
        let Some(State::Active(n)) = self.state.get(&first) else {
            return;
        };
        let node = cx.src.parse.tree.node(*n);
        let (span, name) = match node
            .name(&cx.src.parse.tokens)
            .or_else(|| cx.src.name_after(node, TokenKind::KwFn))
        {
            Some(t) => (cx.src.token_span(t), cx.src.text(t)),
            None => (cx.src.span(node), "this function"),
        };
        let msg = format!(
            "`{name}` calls itself through functions without a result type; write its result type"
        );
        self.cycles
            .error(Code::RecursiveFunctionNeedsResultType, span, &msg);
    }
}

/// Registers the module's private callables whose result type is
/// omitted, from its bodies in source order: non-public functions and
/// non-public methods of inherent implementations.
pub fn omitted_results(cx: &BodyCx<'_>, bodies: &[(DefId, NodeRef<'_>)]) {
    let mut r = cx.results.borrow_mut();
    for &(def, node) in bodies {
        if Src::type_child(node).is_some() || Src::child(node, SyntaxKind::Block).is_none() {
            continue;
        }
        let Some(item) = cx.lookup.item(def) else {
            continue;
        };
        let private = !item.public
            && match &item.data {
                ItemData::Fn(_) => true,
                ItemData::Method { owner, .. } => matches!(
                    cx.lookup.item(*owner).map(|o| &o.data),
                    Some(ItemData::Impl {
                        trait_,
                        kind: ImplKind::Written,
                        ..
                    }) if *trait_ == DefId::NONE
                ),
                _ => false,
            };
        if private {
            r.state.insert(def, State::Pending(node.index()));
            r.order.push(def);
        }
    }
}

/// M1's sweep: infers every registered result in source order.
pub fn infer_results(cx: &BodyCx<'_>) {
    let order = cx.results.borrow().order.clone();
    for def in order {
        result_of(cx, def);
    }
}

/// The inferred result of `def` when it is a registered callable, its
/// body checked first when it is not done. A callee whose body is being
/// checked closes a cycle: the error is reported and the call is poison.
pub(crate) fn result_of(cx: &BodyCx<'_>, def: DefId) -> Option<Ty> {
    let node = {
        let mut r = cx.results.borrow_mut();
        match r.state.get(&def)? {
            State::Done { ret, .. } => return Some(*ret),
            State::Active(_) => {
                r.cycle(cx, def);
                return Some(Ty::POISON);
            }
            State::Pending(n) => {
                let n = *n;
                r.state.insert(def, State::Active(n));
                r.active.push(def);
                n
            }
        }
    };
    // Its own diagnostics: a trial that reads this result rolls its own
    // back, and the body is checked only here.
    let mut diags = DiagBuf::default();
    let node = cx.src.parse.tree.node(node);
    let checked = check_fn_body(cx, def, node, &mut diags, None, true);
    let ret = checked.as_ref().map_or(Ty::POISON, |(_, t)| *t);
    let mut r = cx.results.borrow_mut();
    r.active.pop();
    r.errors |= diags.has_errors();
    r.state.insert(
        def,
        State::Done {
            ret,
            body: Some(Box::new((checked.map(|(b, _)| b), diags))),
        },
    );
    Some(ret)
}

/// The body M1 checked for `def` and its diagnostics, once.
pub fn take(cx: &BodyCx<'_>, def: DefId) -> Option<(StageResult<Body>, DiagBuf)> {
    match cx.results.borrow_mut().state.get_mut(&def) {
        Some(State::Done { body, .. }) => body.take().map(|b| *b),
        _ => None,
    }
}

/// The cycle reports, once.
pub fn cycles(cx: &BodyCx<'_>) -> DiagBuf {
    std::mem::take(&mut cx.results.borrow_mut().cycles)
}

/// Every inferred result, in source order: what the module's items carry
/// as these callables' result types.
pub fn inferred(cx: &BodyCx<'_>) -> Vec<(DefId, Ty)> {
    let r = cx.results.borrow();
    r.order
        .iter()
        .filter_map(|d| match r.state.get(d) {
            Some(State::Done { ret, .. }) => Some((*d, *ret)),
            _ => None,
        })
        .collect()
}

impl Ck<'_, '_> {
    /// A callee's signature as its callers see it: an omitted result type
    /// is the inferred one.
    pub(crate) fn with_result(&self, def: DefId, mut sig: FnSig) -> FnSig {
        if let Some(ret) = result_of(self.cx, def) {
            sig.ret = ret;
        }
        sig
    }
}
