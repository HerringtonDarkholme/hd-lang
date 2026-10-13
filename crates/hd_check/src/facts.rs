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
//!
//! A value of a typed fact type is checked where it attaches ("Member-Typed
//! Facts"): the fact type's pattern is matched against the target's type,
//! as a call infers its type arguments from its one argument, and the value
//! then checks like `let f: D[A] = v`. Every error is reported once, at the
//! decorator or member-line element.

use hd_base::{DefId, StageResult};
use hd_diag::{Code, DiagBuf};
use hd_intern::PathKind;
use hd_resolve::ItemData;
use hd_resolve::facts::fact_lines;
use hd_syntax::{NodeRef, SyntaxKind};
use hd_tir::Body;
use hd_tir::ir::{BodyKind, Coercion, NONE, Ref, Tag, TirSink};
use hd_types::solver::TraitRef;
use hd_types::{ParamRef, RowId, Ty, TyData, TyList, VarKind};

use crate::body::{BodyCx, Ck, new_ck, unsupported};
use crate::call::{Args, subst_owner};
use crate::structure::FactLines;
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
/// declaration is `node`; a typed fact checks against the function's
/// signature type (`annot.typed-fact.declared-type`).
pub fn check_facts(
    cx: &BodyCx<'_>,
    owner: DefId,
    node: NodeRef<'_>,
    diags: &mut DiagBuf,
) -> StageResult<Body> {
    let def = cx.names.member(owner, PathKind::Hidden, "facts");
    let sources: Vec<FactSource<'_>> = fact_lines(&cx.src, node)
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
    /// (`annot.decorator.bare-call` included).
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
/// `annot.structure.facts-contents`); `at` is the node its value is
/// reported at. The body is a `Fact` body: the program evaluates it once,
/// on the first read (`annot.fact.eval.lazy`), and a panic in it names
/// `label`, what the facts are attached to (`annot.fact.eval.panic-runtime`).
pub fn check_fact_list(
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
    let elem = pool.intern_ty(&TyData::TraitValue {
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
                let items = ck.b.emit(Tag::Field, call.0, 0, items_ty, at.index());
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
            if let Some(fact) = ck.typed_fact_type(e) {
                let Some(target) = ck.fact_target(target, owner)? else {
                    continue;
                };
                // The value is `D[A]`, which need not be inspectable (a
                // function type in `A`): it is erased to the item type
                // without the conversion's bound, as `Facts` holds any
                // value (`annot.metadata.list-any`).
                if let Some((r, _)) = ck.typed_fact(fact, e, target, single)? {
                    refs.push(ck.b.coerce(Coercion::ToTraitValue, NONE, r, elem, e.index()));
                }
                continue;
            }
            let (r, t) = if single {
                ck.fact_value(e, None)?
            } else {
                ck.expr(e, None)?
            };
            refs.push(ck.coerce(r, t, elem, e, "fact"));
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
    let value = ck.b.emit(Tag::NewData, NONE, rec, facts_ty, at.index());
    let root = ck.b.close_block(blk, Some(value), facts_ty, at.index());
    let mut body = ck.finish_body(root)?;
    body.kind = BodyKind::Fact;
    body.label = label.into();
    Ok(body)
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
/// includes the block-on ban (`annot.fact.block-on.direct`) and a typed
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
        .flat_map(|p| fact_lines(&cx.src, p))
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
        if !sources.is_empty() {
            // A fact the checker cannot type yet leaves its list unchecked.
            let _ = check_fact_list(cx, (owner, def), sources, (target, ""), node, diags);
        }
    }
}

impl Ck<'_, '_> {
    /// A list of facts that is an expression, not a list literal, or a
    /// spread's operand in a literal (`annot.line.right-typed`,
    /// `annot.line.typed`): `e` checked as `List[dyn Any]`, and its
    /// elements as the `items_ty` of a `Facts`, which they are laid out as
    /// (`hd_wasm::layout::dyn_key`). A value that is not a list is
    /// `not_list` (`annot.line.right.not-list`); one of a different element
    /// type is a type mismatch. `None` when it does not check.
    fn fact_list(
        &mut self,
        e: NodeRef<'_>,
        not_list: Code,
        items_ty: Ty,
    ) -> StageResult<Option<Ref>> {
        let pool = self.pool();
        let any = pool.intern_ty(&TyData::TraitValue {
            def: self.cx.names.known.any,
            args: TyList::EMPTY,
            bindings: vec![],
        });
        let want = pool.intern_ty(&TyData::Adt {
            def: self.cx.names.known.list,
            args: pool.list(&[any]),
        });
        let (r, t) = self.expr(e, Some(want))?;
        let t = self.infer.resolve(pool, t);
        if self.list_item(t).is_none() {
            if !matches!(self.strip_mut(t), Ty::NEVER | Ty::POISON) {
                let msg = format!("expected a list of facts, found {}", self.show(t));
                self.err(not_list, e, &msg);
            }
            return Ok(None);
        }
        let mark = self.diags.mark();
        let r = self.coerce(r, t, want, e, "member line list");
        if self.diags.errors_since(mark) > 0 {
            return Ok(None);
        }
        Ok(Some(self.b.coerce(
            Coercion::Variance,
            NONE,
            r,
            items_ty,
            e.index(),
        )))
    }

    /// The typed fact type of the value that the decorator line or
    /// member-line element `e` writes: the result type's constructor of the
    /// function it calls or names (`annot.typed-fact.declare`,
    /// `annot.decorator.bare-call`).
    pub(crate) fn typed_fact_type(&self, e: NodeRef<'_>) -> Option<DefId> {
        let callee = match e.kind() {
            SyntaxKind::CallExpr => e.children().next(),
            _ => Some(e),
        };
        let callee = match callee {
            Some(c) if c.kind() == SyntaxKind::TypeArgsExpr => c.children().next(),
            c => c,
        }?;
        if !matches!(callee.kind(), SyntaxKind::NameExpr | SyntaxKind::FieldExpr) {
            return None;
        }
        let def = self.resolve_path(&self.segments_expr(callee))?;
        let Some(ItemData::Fn(sig)) = self.cx.lookup.item(def).map(|i| &i.data) else {
            return None;
        };
        match self.cx.names.pool.get(sig.ret) {
            TyData::Adt { def, .. }
                if self
                    .cx
                    .lookup
                    .item(def)
                    .is_some_and(|i| i.fact_pattern.is_some()) =>
            {
                Some(def)
            }
            _ => None,
        }
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
        // A `mut` target passes as a `mut T` argument does: whole, or
        // weakened to its readonly form (`annot.typed-fact.infer.weaken`).
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
        let Some(vars) = self.match_fact_pattern(fact, member, n)? else {
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
    ) -> StageResult<Option<(Ref, Ty)>> {
        let pool = self.pool();
        let Some(vars) = self.match_fact_pattern(fact, target, e)? else {
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
        let r = self.coerce(r, t, want, e, "fact");
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
        let ty = if let Some(fact) = self.typed_fact_type(e) {
            let target = self.fact_target(target, owner).ok().flatten()?;
            self.typed_fact(fact, e, target, single).ok().flatten()?.1
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
