//! Function facts (annotations, "Function Facts"): the values a module-level
//! function's decorators attach, read by `facts_of(f)`.
//!
//! Each such function gets one hidden body, `Facts { items: [...] }`, that
//! evaluates its decorators in source order as ordinary code of the
//! function's module (`annot.decorator.facts-of`, `annot.facts-of.result`).
//! A decorator call is its call; a bare name of a function with no
//! parameters calls it (`annot.decorator.bare-call`); a literal is its
//! value. `facts_of(f)` is a `DefaultCall` of that body, so the argument is
//! never evaluated (`annot.facts-of.target`).

use hd_base::{DefId, StageResult};
use hd_diag::{Code, DiagBuf};
use hd_intern::PathKind;
use hd_resolve::ItemData;
use hd_resolve::facts::fact_lines;
use hd_syntax::{NodeRef, SyntaxKind};
use hd_tir::Body;
use hd_tir::ir::{BodyKind, NONE, Ref, Tag, TirSink};
use hd_types::{RowId, Ty, TyData, TyList};

use crate::body::{BodyCx, Ck, new_ck, unsupported};
use crate::call::Args;
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

/// Checks the facts body of the module-level function `owner`, whose
/// declaration is `node`.
pub fn check_facts(
    cx: &BodyCx<'_>,
    owner: DefId,
    node: NodeRef<'_>,
    diags: &mut DiagBuf,
) -> StageResult<Body> {
    let names = &cx.names;
    let pool = names.pool;
    let (facts_ty, items_ty) = facts_types(cx)?;
    let elem = pool.intern_ty(&TyData::TraitValue {
        def: names.item("std.inspect", "Inspectable"),
        args: TyList::EMPTY,
        bindings: vec![],
    });
    let def = names.member(owner, PathKind::Hidden, "facts");
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
    for e in fact_lines(&cx.src, node) {
        ck.move_to(cx.src.span(e).lo);
        // A typed fact's type arguments come from the target's type
        // (`annot.typed-fact.infer`), which no body here infers yet.
        if ck.typed_fact_line(e) {
            continue;
        }
        let (r, t) = ck.fact_value(e)?;
        refs.push(ck.coerce(r, t, elem, e, "fact"));
    }
    let rec = ck.b.refs_record(&refs);
    let list = ck.b.emit(Tag::NewList, NONE, rec, items_ty, node.index());
    let rec = ck.b.refs_record(&[list]);
    let value = ck.b.emit(Tag::NewData, NONE, rec, facts_ty, node.index());
    let root = ck.b.close_block(blk, Some(value), facts_ty, node.index());
    ck.finish_body(root)
}

impl Ck<'_, '_> {
    /// Whether the decorator line calls a function whose result is a typed
    /// fact type (`annot.typed-fact.declare`).
    fn typed_fact_line(&self, e: NodeRef<'_>) -> bool {
        let callee = match e.kind() {
            SyntaxKind::CallExpr => e.children().next(),
            _ => Some(e),
        };
        let callee = match callee {
            Some(c) if c.kind() == SyntaxKind::TypeArgsExpr => c.children().next(),
            c => c,
        };
        let Some(callee) = callee.filter(|c| c.kind() == SyntaxKind::NameExpr) else {
            return false;
        };
        let text = self.cx.src.text(self.cx.src.first(callee)).to_owned();
        let Some(Named::Item(def)) = self.scope_name(&text) else {
            return false;
        };
        let Some(ItemData::Fn(sig)) = self.cx.lookup.item(def).map(|i| &i.data) else {
            return false;
        };
        matches!(
            self.cx.names.pool.get(sig.ret),
            TyData::Adt { def, .. } if self.cx.lookup.item(def).is_some_and(|i| i.typed_fact)
        )
    }

    /// The value one decorator line attaches.
    fn fact_value(&mut self, e: NodeRef<'_>) -> StageResult<(Ref, Ty)> {
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
                return self.call_item(def, &[], &args, e, false, None);
            }
        }
        self.expr(e, None)
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
            let global = self.cx.init.borrow().globals.contains_key(&sym);
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
            let value = self.b.emit(Tag::NewData, NONE, rec, facts_ty, n.index());
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
