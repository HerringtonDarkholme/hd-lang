//! Names, calls and member lookup (type-checking.md §2.3, §5): local and
//! item names, variant constructors, generic calls, inherent, built-in and
//! trait methods, static members, conversions, function values, bang calls
//! and the row check of a callee's requirements.

use std::collections::HashMap;

use hd_base::{DefId, StageResult, Symbol};
use hd_diag::Code;
use hd_intern::PathKind;
use hd_resolve::{FnSig, HeadKind, ItemData, Src};
use hd_syntax::{NodeRef, SyntaxKind};
use hd_tir::ir::{Callee, ChoiceKind, IntrinsicOp, NONE, PrimOp, Providers, Ref, Tag, TirSink};
use hd_types::solver::{Answer, Candidate, Evidence, Goal, TraitRef};
use hd_types::{ParamRef, Prim, Ty, TyData, TyList, VarKind, with_assoc_args};

use crate::body::{Ck, unsupported};
use crate::ty::Named;

/// The methods of the items a body sees, by name (built once per module).
#[derive(Default)]
pub struct MethodIndex {
    /// Inherent methods: (impl, method).
    pub inherent: HashMap<Symbol, Vec<(DefId, DefId)>>,
    /// Trait methods: (trait, method).
    pub traits: HashMap<Symbol, Vec<(DefId, DefId)>>,
}

impl MethodIndex {
    #[must_use]
    pub fn build(lookup: &hd_resolve::Lookup<'_>) -> Self {
        let mut ix = MethodIndex::default();
        let mut add = |it: &hd_resolve::Item| match &it.data {
            ItemData::Impl {
                trait_, methods, ..
            } if *trait_ == DefId::NONE => {
                for (n, m) in methods {
                    ix.inherent.entry(*n).or_default().push((it.def, *m));
                }
            }
            ItemData::Trait(t) => {
                for (n, m) in &t.methods {
                    ix.traits.entry(*n).or_default().push((it.def, *m));
                }
            }
            _ => {}
        };
        for it in lookup.own {
            add(it);
        }
        for f in &lookup.ifaces {
            for it in &f.items {
                add(it);
            }
        }
        for v in ix.inherent.values_mut() {
            v.dedup();
        }
        for v in ix.traits.values_mut() {
            v.dedup();
        }
        ix
    }
}

/// A resolved method.
pub(crate) enum Hit {
    /// An inherent method with its impl's arguments.
    Inherent {
        method: DefId,
        impl_def: DefId,
        impl_args: Vec<Ty>,
    },
    /// A language-tier built-in method.
    Builtin {
        op: IntrinsicOp,
        params: Vec<Ty>,
        ret: Ty,
    },
    /// A trait method: the trait's arguments and the evidence choice.
    Trait {
        trait_: DefId,
        method: DefId,
        args: Vec<Ty>,
        choice: (ChoiceKind, u32),
    },
    /// A method of one trait that the receiver implements through these
    /// impl candidates (an `Instantiations` answer, in content order).
    Choice {
        trait_: DefId,
        method: DefId,
        cands: Vec<Candidate>,
    },
    /// Methods of two or more traits (`trait.resolve.ambiguous`).
    Ambiguous(Vec<DefId>),
}

/// A trait method with its `Self` and the trait's arguments.
pub(crate) struct TraitTarget {
    pub trait_: DefId,
    pub method: DefId,
    pub self_ty: Ty,
    pub args: Vec<Ty>,
}

/// Argument nodes of a call: positional values, then named ones, then a
/// trailing block (`fn.trailing.form`), which supplies the final parameter.
pub(crate) struct Args<'t> {
    pub positional: Vec<NodeRef<'t>>,
    /// The operand of a positional spread `x...`, which stands after every
    /// plain positional value (`expr.call.spread`).
    pub spread: Option<NodeRef<'t>>,
    /// Positional arguments written after the spread
    /// (`expr.call.spread.position`).
    pub after_spread: Vec<NodeRef<'t>>,
    pub named: Vec<(String, NodeRef<'t>)>,
    pub trailing: Option<NodeRef<'t>>,
}

/// The parameters a call's arguments are checked against.
pub(crate) struct Formals<'a> {
    pub params: &'a [(Symbol, Ty)],
    pub defaults: &'a [bool],
    /// Whether the final parameter is a vararg (`fn.vararg.form`).
    pub variadic: bool,
}

/// Where a positional spread stands among the parameters not yet given.
#[derive(Clone, Copy)]
struct SpreadAt<'a> {
    /// Whether the final of those parameters is a vararg.
    variadic: bool,
    /// How many plain positional arguments precede the spread.
    given: usize,
    /// The parameters a named argument supplies; the spread skips them.
    named: &'a [bool],
}

impl Args<'_> {
    pub(crate) fn empty() -> Self {
        Args {
            positional: Vec::new(),
            spread: None,
            after_spread: Vec::new(),
            named: Vec::new(),
            trailing: None,
        }
    }
}

impl Ck<'_, '_> {
    pub(crate) fn args_of<'t>(&self, al: Option<NodeRef<'t>>) -> Args<'t> {
        let mut a = Args::empty();
        for c in al.iter().flat_map(|l| l.children()) {
            match c.kind() {
                SyntaxKind::Argument => {
                    let Some(e) = c.children().next() else {
                        continue;
                    };
                    let operand = if e.kind() == SyntaxKind::SpreadExpr {
                        e.children().next()
                    } else {
                        None
                    };
                    match (operand, a.spread) {
                        (Some(o), None) => a.spread = Some(o),
                        (_, Some(_)) => a.after_spread.push(operand.unwrap_or(e)),
                        (None, None) => a.positional.push(e),
                    }
                }
                SyntaxKind::NamedArgument => {
                    let name = self.cx.src.text(self.cx.src.first(c)).to_owned();
                    let Some(e) = c.children().next() else {
                        continue;
                    };
                    a.named.push((name, e));
                }
                _ => {}
            }
        }
        a
    }

    fn method_index(&self) -> &crate::MethodIndex {
        self.cx
            .methods
            .get_or_init(|| crate::MethodIndex::build(self.cx.lookup))
    }

    // ------------------------------------------------------------ names

    pub(crate) fn name_expr(&mut self, n: NodeRef<'_>, want: Option<Ty>) -> StageResult<(Ref, Ty)> {
        let s = self.sym_of(n);
        if let Some((l, d)) = self.find_local(s) {
            return Ok(self.read_local(l, d, n));
        }
        if let Some(v) = self.global_get(s, n) {
            return Ok(v);
        }
        let text = self.cx.names.text(s).to_owned();
        if self.is_poison_name(&text) {
            return Ok(self.poison_value(n));
        }
        if let Some(Named::Item(def)) = self.scope_name(&text)
            && let Some(item) = self.cx.lookup.item(def)
        {
            if let ItemData::Fn(sig) = &item.data {
                let sig = sig.clone();
                return self.item_value(def, &sig, &[], want, n);
            }
            if let ItemData::Data(fields) = &item.data
                && fields.is_empty()
            {
                let t = self.ctor(def, &[])?;
                let rec = self.b.refs_record(&[]);
                return Ok((self.b.emit(Tag::NewData, NONE, rec, t, n.index()), t));
            }
        }
        if let Some(Named::Item(def)) = self.scope_name(&text)
            && self.kind_of_item(def).is_some_and(HeadKind::is_type)
        {
            let msg = format!("`{text}` is a type, not a value");
            self.err(Code::TypeUsedAsValue, n, &msg);
            return Ok((Ref(NONE), Ty::NEVER));
        }
        let msg = format!("`{text}` is not defined");
        self.err(Code::UnknownName, n, &msg);
        Ok((Ref(NONE), Ty::NEVER))
    }

    /// `f::[T]` as a value, or a member's `Owner::name::[T]`.
    pub(crate) fn explicit_item_value(
        &mut self,
        n: NodeRef<'_>,
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let Some(inner) = n.children().next() else {
            return unsupported("a type-argument value shape");
        };
        let mut explicit = Vec::new();
        if let Some(tl) = Src::child(n, SyntaxKind::TypeArgumentList) {
            for t in tl.children().filter(|c| c.kind().is_type()) {
                explicit.push(self.ty_node(t)?);
            }
        }
        if inner.kind() == SyntaxKind::PathExpr {
            return self.path_value(inner, want, &explicit);
        }
        let segs = self.segments_expr(inner);
        if segs.first().is_some_and(|s| self.is_poison_name(s)) {
            return Ok(self.poison_value(n));
        }
        let Some(def) = self.resolve_path(&segs) else {
            let msg = format!("`{}` is not defined", segs.join("."));
            self.err(Code::UnknownName, n, &msg);
            return Ok((Ref(NONE), Ty::NEVER));
        };
        if let Some(ItemData::Fn(sig)) = self.cx.lookup.item(def).map(|i| &i.data) {
            let sig = sig.clone();
            return self.item_value(def, &sig, &explicit, want, n);
        }
        unsupported("a type with arguments used as a value")
    }

    pub(crate) fn fresh_generics(&mut self, sig: &FnSig, explicit: &[Ty], skip: usize) -> Vec<Ty> {
        let pool = self.pool();
        sig.generics
            .iter()
            .enumerate()
            .map(|(i, _)| {
                explicit
                    .get(i.saturating_sub(skip))
                    .copied()
                    .filter(|_| i >= skip)
                    .unwrap_or_else(|| self.infer.fresh(pool, VarKind::General))
            })
            .collect()
    }

    // ------------------------------------------------------------ calls

    pub(crate) fn call(
        &mut self,
        n: NodeRef<'_>,
        kids: &[NodeRef<'_>],
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let (Some(callee), al) = (
            kids.first().copied(),
            Src::child(n, SyntaxKind::ArgumentList),
        ) else {
            return unsupported("a call shape");
        };
        let args = self.args_of(al);
        self.call_args(n, callee, &args, want)
    }

    /// `f(a): block` and `f: block` (`fn.trailing.form`,
    /// `fn.trailing.empty-parentheses`): the call with one more final
    /// argument, the block, which `check_args` checks as a zero-argument
    /// closure typed by the callee's final parameter (checking-and-tir.md,
    /// "What The Checker Desugars").
    pub(crate) fn trailing_call(
        &mut self,
        n: NodeRef<'_>,
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let (Some(head), Some(block)) = (n.children().next(), Src::child(n, SyntaxKind::Block))
        else {
            return unsupported("a trailing-block call shape");
        };
        let (call, callee, al) = if head.kind() == SyntaxKind::CallExpr {
            let Some(callee) = head.children().next() else {
                return unsupported("a call shape");
            };
            (head, callee, Src::child(head, SyntaxKind::ArgumentList))
        } else {
            (n, head, None)
        };
        let mut args = self.args_of(al);
        args.trailing = Some(block);
        self.call_args(call, callee, &args, want)
    }

    /// A call of `callee` with `args`; `n` is the call node.
    fn call_args(
        &mut self,
        n: NodeRef<'_>,
        callee: NodeRef<'_>,
        args: &Args<'_>,
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let (callee, bang) = if callee.kind() == SyntaxKind::SuspendExpr {
            match callee.children().next() {
                Some(c) => (c, true),
                None => return unsupported("a bang call shape"),
            }
        } else {
            (callee, false)
        };
        match callee.kind() {
            SyntaxKind::VariantExpr => self.variant_value(callee, args, want, Some(n)),
            SyntaxKind::NameExpr => {
                let s = self.sym_of(callee);
                if let Some((l, d)) = self.find_local(s) {
                    let (f, ft) = self.read_local(l, d, callee);
                    return self.call_value(f, ft, args, n, bang);
                }
                let text = self.cx.names.text(s).to_owned();
                if let Some(p) = Prim::ALL.iter().find(|p| p.name() == text) {
                    return self.conversion(Ty::prim(*p), args, n);
                }
                if let Some(Named::Item(def)) = self.scope_name(&text) {
                    return self.call_item(def, &[], args, n, bang, want);
                }
                if self.is_poison_name(&text) {
                    return Ok(self.poison_value(n));
                }
                let msg = format!("`{text}` is not defined");
                self.err(Code::UnknownName, callee, &msg);
                Ok((Ref(NONE), Ty::NEVER))
            }
            SyntaxKind::TypeArgsExpr => {
                let Some(inner) = callee.children().next() else {
                    return unsupported("a type-argument call shape");
                };
                let mut explicit = Vec::new();
                if let Some(tl) = Src::child(callee, SyntaxKind::TypeArgumentList) {
                    for t in tl.children().filter(|c| c.kind().is_type()) {
                        explicit.push(self.ty_node(t)?);
                    }
                }
                let segs = self.segments_expr(inner);
                if segs.first().is_some_and(|s| self.is_poison_name(s)) {
                    return Ok(self.poison_value(n));
                }
                match self.resolve_path(&segs) {
                    Some(def)
                        if matches!(
                            self.cx.lookup.item(def).map(|i| &i.data),
                            Some(ItemData::Fn(_))
                        ) =>
                    {
                        self.call_item(def, &explicit, args, n, bang, want)
                    }
                    _ if matches!(inner.kind(), SyntaxKind::FieldExpr | SyntaxKind::PathExpr) => {
                        self.member_call(n, inner, args, bang, want, explicit)
                    }
                    _ => unsupported("a call through explicit type arguments of a type"),
                }
            }
            SyntaxKind::FieldExpr | SyntaxKind::PathExpr => {
                self.member_call(n, callee, args, bang, want, vec![])
            }
            _ => {
                let (f, ft) = self.expr(callee, None)?;
                self.call_value(f, ft, args, n, bang)
            }
        }
    }

    /// `i64(x)`: a numeric conversion.
    fn conversion(&mut self, to: Ty, args: &Args<'_>, n: NodeRef<'_>) -> StageResult<(Ref, Ty)> {
        plain_args(args, "a conversion")?;
        let [e] = args.positional.as_slice() else {
            return unsupported("a conversion shape");
        };
        let (r, t) = self.expr(*e, None)?;
        // A newtype converts to the type it wraps (`string(path)`).
        let pool = self.pool();
        let tr = self.infer.resolve(pool, t);
        if let TyData::Adt { def, .. } = pool.get(match pool.get(tr) {
            TyData::Mut(i) => i,
            _ => tr,
        }) && let Some(ItemData::Newtype(inner)) = self.cx.lookup.item(def).map(|i| &i.data)
        {
            let inner = *inner;
            if self.can_unify(inner, to) {
                return Ok((self.b.emit(Tag::Field, r.0, 0, to, n.index()), to));
            }
            if self.numeric(inner) {
                let v = self.b.emit(Tag::Field, r.0, 0, inner, n.index());
                return Ok((self.b.prim(PrimOp::Conv as u32, &[v], to, n.index()), to));
            }
        }
        if !self.numeric(t) && self.infer.shallow(self.pool(), t) != Ty::BOOL {
            let msg = format!("in conversion: {} is not a number", self.show(t));
            self.err(Code::TypeMismatch, *e, &msg);
        }
        // A literal argument takes the target's type directly.
        if self.b.const_of(r).is_some()
            && matches!(
                self.infer.kind_of(self.pool(), t),
                Some(VarKind::IntLit | VarKind::SignedIntLit | VarKind::FloatLit)
            )
            && self.can_unify(t, to)
        {
            self.expect(t, to, *e, "conversion");
            return Ok((r, to));
        }
        Ok((self.b.prim(PrimOp::Conv as u32, &[r], to, n.index()), to))
    }

    /// A call of a function value.
    fn call_value(
        &mut self,
        f: Ref,
        ft: Ty,
        args: &Args<'_>,
        n: NodeRef<'_>,
        bang: bool,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
        let full = ft;
        let ft = self.infer.resolve(pool, ft);
        let ft = match pool.get(ft) {
            TyData::Mut(i) => i,
            _ => ft,
        };
        // `s!()` on a stored suspension: a suspension point.
        let suspend = self.cx.names.known.suspend;
        if let TyData::Adt { def, args: sa } = pool.get(ft)
            && def == suspend
            && bang
            && args.positional.is_empty()
            && args.trailing.is_none()
        {
            // Driving a suspension advances it (`mut self`).
            if !self.has_mut_access(full) {
                self.err(
                    Code::MutableReceiverRequired,
                    n,
                    "driving a suspension needs `mut` access to it",
                );
            }
            let t = pool.list_items(sa).first().copied().unwrap_or(Ty::POISON);
            self.check_bang(true, n);
            return Ok((self.b.emit(Tag::AwaitValue, f.0, NONE, t, n.index()), t));
        }
        let TyData::Fn {
            params,
            result,
            row,
            suspends,
        } = pool.get(ft)
        else {
            if matches!(pool.get(ft), TyData::Infer(_)) {
                return unsupported("a call of a value whose type is not yet known");
            }
            let msg = format!("{} is not a function", self.show(ft));
            self.err(Code::NotCallable, n, &msg);
            return Ok((Ref(NONE), Ty::NEVER));
        };
        if !args.named.is_empty() {
            return unsupported("named arguments to a function value");
        }
        let ps = pool.list_items(params);
        let refs = if args.spread.is_some() {
            self.spread_value_args(ps, args, n)?
        } else {
            let given = args.positional.len() + usize::from(args.trailing.is_some());
            if ps.len() != given {
                let msg = format!("the function takes {} arguments", ps.len());
                self.err(Code::ArgumentCount, n, &msg);
            }
            let mut refs = Vec::new();
            for (i, e) in args.positional.iter().enumerate() {
                let w = ps.get(i).copied();
                let (r, t) = self.expr(*e, w)?;
                refs.push(match w {
                    Some(w) => self.coerce(r, t, w, *e, "argument"),
                    None => r,
                });
            }
            if let Some(block) = args.trailing {
                let w = self.trailing_param(ps.last().copied(), "the function value")?;
                let (r, t) = self.trailing_closure(block, w)?;
                refs.push(self.coerce(r, t, w, block, "argument"));
            }
            refs
        };
        self.check_row(row, n);
        let rec = self.b.refs_record(&refs);
        if !suspends {
            if bang {
                self.check_bang(false, n);
            }
            return Ok((
                self.b.emit(Tag::CallValue, f.0, rec, result, n.index()),
                result,
            ));
        }
        // A suspending function value: a plain call is cold, a bang call
        // awaits it.
        let st = pool.intern_ty(&TyData::Mut(pool.intern_ty(&TyData::Adt {
            def: suspend,
            args: pool.list(&[result]),
        })));
        let cold = self.b.emit(Tag::CallValue, f.0, rec, st, n.index());
        if bang {
            self.check_bang(true, n);
            return Ok((
                self.b
                    .emit(Tag::AwaitValue, cold.0, NONE, result, n.index()),
                result,
            ));
        }
        Ok((cold, st))
    }

    /// The type of the final parameter `last` that a trailing block fills,
    /// which must be a zero-argument function type (`fn.trailing.form`,
    /// `grammar.call.trailing-block.eligible`). No diagnostic code names a
    /// callee without one yet (audit/compiler/diagnostic-notes.md), so it
    /// stays a stop.
    fn trailing_param(&mut self, last: Option<Ty>, name: &str) -> StageResult<Ty> {
        let pool = self.pool();
        if let Some(t) = last {
            let r = self.infer.resolve(pool, t);
            if let TyData::Fn { params, .. } = pool.get(self.strip_mut(r))
                && pool.list_items(params).is_empty()
            {
                return Ok(t);
            }
        }
        unsupported(format!(
            "a trailing block for `{name}`, whose final parameter is not a zero-argument function"
        ))
    }

    /// Checks arguments against parameters (positional, then named by
    /// parameter name), skipping `skip` leading parameters already given.
    fn check_args(
        &mut self,
        formals: &Formals<'_>,
        skip: usize,
        args: &Args<'_>,
        n: NodeRef<'_>,
        name: &str,
    ) -> StageResult<Vec<Ref>> {
        // The callee's default bodies, taken before any argument is checked.
        let owner = self.default_owner.take();
        let rest = formals.params.get(skip..).unwrap_or(&[]);
        let variadic = formals.variadic && !rest.is_empty();
        if variadic && args.trailing.is_some() {
            return unsupported("a trailing block for a vararg function");
        }
        // A vararg is the final parameter; the fixed ones come before it.
        let nfixed = rest.len() - usize::from(variadic);
        let mut slots: Vec<Option<Ref>> = vec![None; rest.len()];
        // A trailing block supplies the final parameter, so positional
        // arguments fill the ones before it (`fn.trailing.arguments`).
        let room = rest.len() - usize::from(args.trailing.is_some() && !rest.is_empty());
        if !variadic && args.positional.len() > room {
            let msg = format!("`{name}` takes {} arguments", rest.len());
            self.err(Code::ArgumentCount, n, &msg);
        }
        // Bare variable parameters still open: several arguments solving
        // one join at the readonly view (types.generic.infer.join).
        let pool = self.pool();
        let open: Vec<bool> = rest
            .iter()
            .map(|p| {
                matches!(pool.get(p.1), TyData::Infer(_))
                    && matches!(pool.get(self.infer.shallow(pool, p.1)), TyData::Infer(_))
            })
            .collect();
        // The separate arguments a vararg collects, with the types each
        // contributes to a tuple (`fn.vararg.collect`).
        let mut items: Vec<(Ref, Ty)> = Vec::new();
        let mut item_wants = Vec::new();
        for (i, e) in args.positional.iter().enumerate() {
            if variadic && i >= nfixed {
                // Asked once the fixed arguments have solved what they can.
                if i == nfixed {
                    item_wants =
                        self.vararg_wants(rest[nfixed].1, args.positional.len() - nfixed)?;
                }
                let w = item_wants[i - nfixed];
                let (r, t) = self.arg_value(*e, w)?;
                let r = match w {
                    Some(w) => self.coerce(r, t, w, *e, "argument"),
                    None => r,
                };
                items.push((r, w.unwrap_or(t)));
                continue;
            }
            let mut w = match rest.get(i) {
                Some(p) => Some(self.normalize_deep(p.1)?),
                None => None,
            };
            let (r, t) = self.arg_value(*e, w)?;
            if let Some(p) = rest.get(i)
                && open.get(i).copied().unwrap_or(false)
                && self.join_down(p.1, t)
            {
                w = Some(self.normalize_deep(p.1)?);
            }
            let r = match w {
                Some(w) => self.coerce(
                    r,
                    t,
                    w,
                    *e,
                    if rest
                        .get(i)
                        .is_some_and(|p| matches!(pool.get(p.1), TyData::Infer(_)))
                    {
                        "inferred argument"
                    } else {
                        "argument"
                    },
                ),
                None => r,
            };
            if i < room
                && let Some(s) = slots.get_mut(i)
            {
                *s = Some(r);
            }
        }
        if variadic && !items.is_empty() {
            slots[nfixed] = Some(self.pack_vararg(&items, rest[nfixed].1, n)?);
        }
        if let Some(s) = args.spread {
            let tys: Vec<Ty> = rest.iter().map(|p| p.1).collect();
            let named: Vec<bool> = rest
                .iter()
                .map(|p| {
                    let text = self.cx.names.text(p.0);
                    args.named.iter().any(|(nm, _)| nm == text)
                })
                .collect();
            let at = SpreadAt {
                variadic,
                given: args.positional.len(),
                named: &named,
            };
            self.spread_arg(s, &tys, at, &mut slots, args)?;
        }
        for (pname, e) in &args.named {
            let Some(i) = rest
                .iter()
                .position(|p| self.cx.names.text(p.0) == pname.as_str())
            else {
                let msg = format!("`{name}` has no parameter `{pname}`");
                self.err(Code::UnknownNamedArgument, *e, &msg);
                continue;
            };
            if slots[i].is_some() {
                let msg = format!("`{pname}` is given twice");
                self.err(Code::DuplicateArgument, *e, &msg);
                continue;
            }
            let mut w = self.normalize_deep(rest[i].1)?;
            let (r, t) = self.arg_value(*e, Some(w))?;
            if open[i] && self.join_down(rest[i].1, t) {
                w = self.normalize_deep(rest[i].1)?;
            }
            slots[i] = Some(self.coerce(
                r,
                t,
                w,
                *e,
                if matches!(pool.get(rest[i].1), TyData::Infer(_)) {
                    "inferred argument"
                } else {
                    "argument"
                },
            ));
        }
        if let Some(block) = args.trailing {
            let w = match rest.last() {
                Some(p) => Some(self.normalize_deep(p.1)?),
                None => None,
            };
            let w = self.trailing_param(w, name)?;
            let last = rest.len() - 1;
            if slots[last].is_some() {
                let pname = self.cx.names.text(rest[last].0).to_owned();
                let msg = format!("`{pname}` is given twice");
                self.err(Code::DuplicateArgument, block, &msg);
            } else {
                let (r, t) = self.trailing_closure(block, w)?;
                slots[last] = Some(self.coerce(r, t, w, block, "argument"));
            }
        }
        // A vararg nothing supplied collects no arguments.
        if variadic && slots[nfixed].is_none() {
            slots[nfixed] = Some(self.pack_vararg(&[], rest[nfixed].1, n)?);
        }
        let mut out = Vec::new();
        for (i, s) in slots.into_iter().enumerate() {
            match s {
                Some(r) => out.push(r),
                None if formals.defaults.get(skip + i).copied().unwrap_or(false) => {
                    // `DefaultCall` of the parameter's default body, after
                    // every explicit argument, over the earlier values
                    // (checking-and-tir.md "Default calls").
                    let Some((def, targs, prefix)) = &owner else {
                        return unsupported("a default argument of this callee");
                    };
                    let pname = self.cx.names.text(rest[i].0).to_owned();
                    let body = self.cx.names.member(*def, PathKind::Hidden, &pname);
                    let a = self.b.refs_record(&[Ref(body.raw()), Ref(targs.0)]);
                    let mut earlier = prefix.clone();
                    earlier.extend(&out);
                    let bw = self.b.refs_record(&earlier);
                    let t = self.normalize_deep(rest[i].1)?;
                    out.push(self.b.emit(Tag::DefaultCall, a, bw, t, n.index()));
                }
                None => {
                    let msg = format!("`{name}` takes {} arguments", rest.len());
                    self.err(Code::ArgumentCount, n, &msg);
                    break;
                }
            }
        }
        Ok(out)
    }

    /// The expected type of each of `count` separate arguments a vararg of
    /// type `lt` collects: the list's element, or the tuple's elements
    /// when the type is already a tuple of that many.
    fn vararg_wants(&mut self, lt: Ty, count: usize) -> StageResult<Vec<Option<Ty>>> {
        let pool = self.pool();
        let lt = self.normalize_deep(lt)?;
        let list = self.cx.names.known.list;
        Ok(match pool.get(self.strip_mut(lt)) {
            TyData::Adt { def, args } if def == list => {
                let et = pool.list_items(args).first().copied().unwrap_or(Ty::POISON);
                vec![Some(et); count]
            }
            TyData::Tuple { elems, rest: None } if pool.list_items(elems).len() == count => {
                pool.list_items(elems).iter().copied().map(Some).collect()
            }
            _ => vec![None; count],
        })
    }

    /// The collected value of a vararg of type `lt` from separate
    /// arguments: a list, or the tuple expression of them
    /// (`fn.vararg.collect.list`, `fn.vararg.collect.tuple-expr`,
    /// `fn.vararg.tuple-param.infer`).
    fn pack_vararg(&mut self, items: &[(Ref, Ty)], lt: Ty, n: NodeRef<'_>) -> StageResult<Ref> {
        let pool = self.pool();
        let lt = self.normalize_deep(lt)?;
        let list = self.cx.names.known.list;
        let refs: Vec<Ref> = items.iter().map(|i| i.0).collect();
        let shape = pool.get(self.strip_mut(lt));
        if matches!(shape, TyData::Adt { def, .. } if def == list) {
            let rec = self.b.refs_record(&refs);
            return Ok(self.b.emit(Tag::NewList, NONE, rec, lt, n.index()));
        }
        if matches!(shape, TyData::Tuple { rest: Some(_), .. }) {
            return unsupported("a tuple vararg with a rest element");
        }
        let tys: Vec<Ty> = items.iter().map(|i| i.1).collect();
        let tt = pool.intern_ty(&TyData::Tuple {
            elems: pool.list(&tys),
            rest: None,
        });
        self.expect(tt, lt, n, "argument");
        let rec = self.b.refs_record(&refs);
        Ok(self.b.emit(Tag::NewTuple, NONE, rec, tt, n.index()))
    }

    /// The positional spread `e...` of a call (`expr.call.spread.fills`).
    /// `tys` are the parameters not yet given, `at` where the spread
    /// stands; the spread fills `slots`.
    fn spread_arg(
        &mut self,
        e: NodeRef<'_>,
        tys: &[Ty],
        at: SpreadAt,
        slots: &mut [Option<Ref>],
        args: &Args<'_>,
    ) -> StageResult<()> {
        let pool = self.pool();
        let SpreadAt {
            variadic,
            given,
            named,
        } = at;
        let nfixed = tys.len() - usize::from(variadic);
        // The parameters the spread fills: those after the given ones that
        // no named argument supplies (`expr.call.spread.fills`).
        let free: Vec<usize> = (given..tys.len())
            .filter(|&i| !named.get(i).copied().unwrap_or(false))
            .collect();
        let at_vararg = variadic && (given >= nfixed || free.first() == Some(&nfixed));
        // A positional after the spread is a second supply of a vararg, or
        // else the spread is not final (`expr.call.spread.position`,
        // `expr.call.spread.vararg-duplicate`).
        let mut failed = false;
        if let Some(late) = args.after_spread.first() {
            let (code, msg) = if at_vararg {
                (
                    Code::DuplicateArgument,
                    "the vararg is given by the spread and by a value",
                )
            } else {
                (
                    Code::NonfinalPositionalSpread,
                    "a positional spread must be the final positional argument",
                )
            };
            self.err(code, *late, msg);
            failed = true;
        }
        let lt = if at_vararg {
            Some(self.normalize_deep(tys[nfixed])?)
        } else {
            None
        };
        let (r, t) = self.arg_value(e, lt)?;
        let poison = |slots: &mut [Option<Ref>], free: &[usize]| {
            for &i in free {
                if let Some(s) = slots.get_mut(i) {
                    s.get_or_insert(Ref(NONE));
                }
            }
        };
        if failed {
            poison(slots, &free);
            return Ok(());
        }
        let t = self.infer.resolve(pool, t);
        if matches!(self.strip_mut(t), Ty::NEVER | Ty::POISON) {
            poison(slots, &free);
            return Ok(());
        }
        // At the vararg: the operand is its collected value
        // (`expr.call.spread.at-vararg`), passed through without a copy.
        if let Some(lt) = lt {
            if slots[nfixed].is_some() {
                // Separate arguments already supplied it.
                let list = self.cx.names.known.list;
                if matches!(pool.get(self.strip_mut(lt)), TyData::Adt { def, .. } if def == list) {
                    self.err(
                        Code::DuplicateArgument,
                        e,
                        "the vararg is given by separate values and by a spread",
                    );
                    return Ok(());
                }
                return unsupported("a spread after the separate arguments of a tuple vararg");
            }
            slots[nfixed] = Some(self.coerce(r, t, lt, e, "argument"));
            return Ok(());
        }
        // Before fixed parameters: a tuple of the remaining inputs
        // (`expr.call.spread.inputs`).
        let list = self.cx.names.known.list;
        match pool.get(self.strip_mut(t)) {
            TyData::Tuple { elems, rest } => {
                if rest.is_some() && variadic {
                    return unsupported("a spread of a tuple with a rest element");
                }
                let elems = pool.list_items(elems);
                if rest.is_some() || variadic || elems.len() != free.len() {
                    let msg = format!(
                        "in spread: the remaining inputs are {} values, found {}",
                        free.len(),
                        self.show(t)
                    );
                    self.err(Code::TypeMismatch, e, &msg);
                    poison(slots, &free);
                    return Ok(());
                }
                for (k, (&et, &i)) in elems.iter().zip(&free).enumerate() {
                    let idx = u32::try_from(k).unwrap_or(0);
                    let g = self.b.emit(Tag::TupleGet, r.0, idx, et, e.index());
                    let w = self.normalize_deep(tys[i])?;
                    slots[i] = Some(self.coerce(g, et, w, e, "argument"));
                }
            }
            TyData::Adt { def, .. } if def == list => {
                self.err(
                    Code::PositionalSpreadNeedsVararg,
                    e,
                    "a list spreads only at a vararg parameter",
                );
                poison(slots, &free);
            }
            TyData::Infer(_) | TyData::Param(_) => {
                return unsupported("a spread of a value whose arity is not known");
            }
            _ => {
                let msg = format!("in spread: expected a tuple, found {}", self.show(t));
                self.err(Code::TypeMismatch, e, &msg);
                poison(slots, &free);
            }
        }
        Ok(())
    }

    /// The arguments of a function-value call with a positional spread.
    fn spread_value_args(
        &mut self,
        ps: &[Ty],
        args: &Args<'_>,
        n: NodeRef<'_>,
    ) -> StageResult<Vec<Ref>> {
        if args.trailing.is_some() {
            return unsupported("a trailing block with a spread argument");
        }
        let mut slots: Vec<Option<Ref>> = vec![None; ps.len()];
        if args.positional.len() > ps.len() {
            let msg = format!("the function takes {} arguments", ps.len());
            self.err(Code::ArgumentCount, n, &msg);
        }
        for (i, e) in args.positional.iter().enumerate() {
            let w = ps.get(i).copied();
            let (r, t) = self.expr(*e, w)?;
            let r = match w {
                Some(w) => self.coerce(r, t, w, *e, "argument"),
                None => r,
            };
            if let Some(s) = slots.get_mut(i) {
                *s = Some(r);
            }
        }
        if let Some(s) = args.spread {
            let at = SpreadAt {
                variadic: false,
                given: args.positional.len(),
                named: &[],
            };
            self.spread_arg(s, ps, at, &mut slots, args)?;
        }
        if slots.iter().any(Option::is_none) {
            let msg = format!("the function takes {} arguments", ps.len());
            self.err(Code::ArgumentCount, n, &msg);
        }
        Ok(slots.into_iter().flatten().collect())
    }

    /// A call of a function item, generic or not.
    pub(crate) fn call_item(
        &mut self,
        def: DefId,
        explicit: &[Ty],
        args: &Args<'_>,
        n: NodeRef<'_>,
        bang: bool,
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
        let Some(item) = self.cx.lookup.item(def) else {
            return unsupported("a call of an item outside the closure");
        };
        self.note_call(def);
        let name = self.cx.names.text(item.name).to_owned();
        // A registration function is called only by a test registration
        // call in test position (`module.testing.direct-call`).
        let registering = std::mem::take(&mut self.registering);
        let runner = crate::tests::case_runner(self.cx.names.known, def);
        if !registering && self.is_registration_fn(def) {
            let msg = format!("`{name}` is called only as a test registration in test position");
            self.err(Code::MisplacedTestCase, n, &msg);
            return Ok((Ref(NONE), Ty::NEVER));
        }
        if def == self.cx.names.known.snapshot {
            self.check_snapshot_expect(args);
        }
        match &item.data {
            ItemData::Fn(sig) => {
                let sig = self.with_result(def, sig.clone());
                let vars = self.fresh_generics(&sig, explicit, 0);
                let inst = |t: Ty| subst_owner(pool, def, &vars, t);
                let params: Vec<(Symbol, Ty)> =
                    sig.params.iter().map(|(s, t)| (*s, inst(*t))).collect();
                if runner.is_some() {
                    self.registration_body_result(&params, args)?;
                }
                if item
                    .intrinsic
                    .is_some_and(|k| self.cx.names.text(k) == "task_all")
                {
                    return self.await_all(args, n, bang);
                }
                // The result first, so an expected type guides literals.
                let ret = self.normalize_deep(inst(sig.ret))?;
                if let Some(w) = want
                    && !sig.suspends
                    && pool.has_infer(ret)
                    && w != Ty::VOID
                {
                    let snap = self.infer.snapshot();
                    if self.infer.unify(pool, ret, w).is_err() {
                        self.infer.rollback(snap);
                    }
                }
                self.default_owner = Some((def, pool.list(&vars), vec![]));
                let formals = Formals {
                    params: &params,
                    defaults: &sig.defaults,
                    variadic: sig.variadic,
                };
                let refs = self.check_args(&formals, 0, args, n, &name)?;
                // Parameters named only in bounds, then defaults (§2.4
                // steps 7 and 8); a bound naming one still open waits.
                let open = self.infer_through_bounds(def, &sig, &vars, n)?;
                let waits = |b: Ty| {
                    open.iter()
                        .any(|&k| crate::trial::names_param(pool, b, def, k))
                };
                for (i, g) in sig.generics.iter().enumerate() {
                    if g.mut_bound {
                        self.check_mut_bound(vars[i], n);
                    }
                    for b in &g.bounds {
                        let waiting = waits(*b);
                        let b = inst(*b);
                        if let TyData::TraitValue {
                            def: tr, args: ta, ..
                        } = pool.get(b)
                        {
                            let tref = TraitRef {
                                trait_: tr,
                                self_ty: vars[i],
                                args: ta,
                            };
                            if waiting {
                                self.pending.push((tref, n.index()));
                            } else {
                                self.require_ref(tref, n)?;
                            }
                        }
                    }
                }
                for (i, g) in sig.generics.iter().enumerate() {
                    for b in &g.bounds {
                        self.check_bindings(vars[i], inst(*b), n)?;
                    }
                }
                // The arguments fixed the parameters a projection waited on.
                let ret = self.norm_ty(ret);
                let row = self.call_row(def, &sig, &vars, 0);
                self.check_row(row, n);
                // A registration call's test case runs its std body with
                // the arguments that body takes, and returns the result of
                // the registration's body (checking-and-tir.md §4.13.9).
                if let Some((body_fn, takes)) = runner {
                    let mut picked = Vec::new();
                    for t in takes {
                        let at = params
                            .iter()
                            .position(|p| self.cx.names.text(p.0) == *t)
                            .and_then(|i| refs.get(i));
                        match at {
                            Some(r) => picked.push(*r),
                            None => return Ok((Ref(NONE), Ty::NEVER)),
                        }
                    }
                    let last = match params.last() {
                        Some((_, t)) => self.normalize_deep(*t)?,
                        None => Ty::VOID,
                    };
                    let result = match pool.get(self.strip_mut(last)) {
                        TyData::Fn { result, .. } => self.norm_ty(result),
                        _ => Ty::VOID,
                    };
                    let c = Callee::Item {
                        def: body_fn,
                        targs: pool.list(&vars),
                    };
                    return Ok(self.emit_call(&c, &picked, result, true, true, n));
                }
                // A checked `assert_equal` call runs std's `check_equal`
                // (lib/std/testing.hd), which has the same signature.
                let def = if item
                    .intrinsic
                    .is_some_and(|k| self.cx.names.text(k) == "assert_equal")
                {
                    self.cx.names.known.check_equal
                } else {
                    def
                };
                let c = Callee::Item {
                    def,
                    targs: pool.list(&vars),
                };
                Ok(self.emit_call(&c, &refs, ret, sig.suspends, bang, n))
            }
            ItemData::Newtype(inner) => {
                let inner = *inner;
                plain_args(args, "a newtype constructor")?;
                let [e] = args.positional.as_slice() else {
                    return unsupported("a newtype constructor shape");
                };
                let t = self.ctor(def, explicit)?;
                let TyData::Adt { args: targs, .. } = pool.get(t) else {
                    return unsupported("a newtype's type");
                };
                let it = subst_owner(pool, def, pool.list_items(targs), inner);
                let (r, rt) = self.expr(*e, Some(it))?;
                let r = self.coerce(r, rt, it, *e, "argument");
                // A newtype over a composite carries its base value's
                // permission (types.newtype.construct-permission).
                let t = if self.is_composite(rt) && self.has_mut_access(rt) {
                    pool.intern_ty(&TyData::Mut(t))
                } else {
                    t
                };
                let rec = self.b.refs_record(&[r]);
                Ok((self.b.emit(Tag::NewData, NONE, rec, t, n.index()), t))
            }
            ItemData::Data(_) | ItemData::Enum { .. } | ItemData::Alias(_) => {
                unsupported("a call of a type name")
            }
            _ => {
                let msg = format!("`{name}` is not a function");
                self.err(Code::NotCallable, n, &msg);
                Ok((Ref(NONE), Ty::NEVER))
            }
        }
    }

    /// A bang call needs a suspending callee (`not-suspending`) and a
    /// driver context (`req.bang.driver-contexts`).
    pub(crate) fn check_bang(&mut self, callee_suspends: bool, n: NodeRef<'_>) {
        if !callee_suspends {
            self.err(
                Code::NotSuspending,
                n,
                "a bang call of a function that does not suspend",
            );
        }
        if self.defer_base.is_some() {
            // `flow.defer.suspend`: a `defer` suite cannot suspend.
            self.err(
                Code::SuspensionForbiddenContext,
                n,
                "a `defer` suite cannot suspend",
            );
        } else if !self.suspends.last().copied().unwrap_or(false) {
            self.err(
                Code::BangCallOutsideSuspension,
                n,
                "a bang call needs a suspending function or closure",
            );
        }
    }

    /// `all!(a(), b())`: cold suspensions, then one `AwaitAll` whose
    /// value is the tuple of their results (suspension.md §14.5).
    fn await_all(&mut self, args: &Args<'_>, n: NodeRef<'_>, bang: bool) -> StageResult<(Ref, Ty)> {
        if let Some(s) = args.spread {
            self.err(
                Code::TypeMismatch,
                s,
                "`all` takes its tasks as direct arguments, not as a spread",
            );
            return Ok((Ref(NONE), Ty::NEVER));
        }
        plain_args(args, "`all`")?;
        let pool = self.pool();
        if !bang {
            self.err(Code::NotSuspending, n, "`all` is called as `all!(...)`");
        }
        if bang {
            self.check_bang(true, n);
        }
        let suspend = self.cx.names.known.suspend;
        let mut refs = Vec::new();
        let mut tys = Vec::new();
        for e in &args.positional {
            let (r, t) = self.expr(*e, None)?;
            let st = self.strip_mut(t);
            let x = match pool.get(st) {
                TyData::Adt { def, args } if def == suspend => {
                    pool.list_items(args).first().copied().unwrap_or(Ty::POISON)
                }
                _ => {
                    let msg = format!(
                        "in argument: expected a cold suspension, found {}",
                        self.show(t)
                    );
                    self.err(Code::TypeMismatch, *e, &msg);
                    Ty::POISON
                }
            };
            refs.push(r);
            tys.push(x);
        }
        let t = pool.intern_ty(&TyData::Tuple {
            elems: pool.list(&tys),
            rest: None,
        });
        let rec = self.b.refs_record(&refs);
        Ok((self.b.emit(Tag::AwaitAll, NONE, rec, t, n.index()), t))
    }

    /// Emits a `Call`, or an `Await` for a bang call of a suspending
    /// callee; a plain call of one is a cold `mut Suspend[T]`.
    fn emit_call(
        &mut self,
        c: &Callee,
        refs: &[Ref],
        ret: Ty,
        suspends: bool,
        bang: bool,
        n: NodeRef<'_>,
    ) -> (Ref, Ty) {
        let pool = self.pool();
        if bang {
            self.check_bang(suspends, n);
        }
        if suspends && !bang {
            let suspend = self.cx.names.known.suspend;
            let st = pool.intern_ty(&TyData::Adt {
                def: suspend,
                args: pool.list(&[ret]),
            });
            let st = pool.intern_ty(&TyData::Mut(st));
            return (self.b.call(c, refs, Providers::None, st, n.index()), st);
        }
        let r = self.b.call(c, refs, Providers::None, ret, n.index());
        if suspends && bang {
            self.b.body_mut().tags[r.0 as usize] = Tag::Await;
        }
        (r, ret)
    }

    // ------------------------------------------------------------ variants

    /// `.V`, `.V(args)`: a variant of the expected enum, `Option` or
    /// `Result`.
    pub(crate) fn variant_value(
        &mut self,
        v: NodeRef<'_>,
        args: &Args<'_>,
        want: Option<Ty>,
        call: Option<NodeRef<'_>>,
    ) -> StageResult<(Ref, Ty)> {
        plain_args(args, "a variant")?;
        let pool = self.pool();
        let at = call.unwrap_or(v);
        let name = self.cx.src.text(self.cx.src.last(v)).to_owned();
        let w = want
            .map(|w| self.infer.resolve(pool, w))
            .map(|w| match pool.get(w) {
                TyData::Mut(i) => i,
                _ => w,
            });
        let result = self.cx.names.known.result;
        let enum_ty = match w.map(|w| pool.get(w)) {
            Some(TyData::Option(_) | TyData::Adt { .. }) => w.unwrap_or(Ty::POISON),
            _ => match name.as_str() {
                "Some" | "None" => {
                    let t = self.infer.fresh(pool, VarKind::General);
                    pool.intern_ty(&TyData::Option(t))
                }
                "Ok" | "Err" => {
                    let a = self.infer.fresh(pool, VarKind::General);
                    let b = self.infer.fresh(pool, VarKind::General);
                    pool.intern_ty(&TyData::Adt {
                        def: result,
                        args: pool.list(&[a, b]),
                    })
                }
                _ => {
                    let msg = format!("`.{name}` needs an expected enum type");
                    self.err(Code::MissingContextualEnumType, v, &msg);
                    return Ok((Ref(NONE), Ty::NEVER));
                }
            },
        };
        self.variant_by_name(enum_ty, &name, args, at)
    }

    /// A variant's index and payload field types under the enum's
    /// arguments; `Option`'s are `None` 0 and `Some` 1.
    pub(crate) fn variant_fields(&self, enum_ty: Ty, name: &str) -> Option<(u32, Vec<Ty>)> {
        let pool = self.pool();
        let t = self.infer.resolve(pool, enum_ty);
        let t = match pool.get(t) {
            TyData::Mut(i) => i,
            _ => t,
        };
        match pool.get(t) {
            TyData::Option(inner) => match name {
                "None" => Some((0, vec![])),
                "Some" => Some((1, vec![inner])),
                _ => None,
            },
            TyData::Adt { def, args } => {
                let ItemData::Enum { variants, .. } = &self.cx.lookup.item(def)?.data else {
                    return None;
                };
                let sym = self.cx.names.syms.intern(name);
                let i = variants.iter().position(|v| v.name == sym)?;
                let argv = pool.list_items(args);
                let fs = variants[i]
                    .fields
                    .iter()
                    .map(|f| subst_owner(pool, def, argv, f.ty))
                    .collect();
                Some((u32::try_from(i).ok()?, fs))
            }
            _ => None,
        }
    }

    // ------------------------------------------------------------ members

    /// `Type.Variant`, `module.item` and other static member values, or
    /// `None` when `base` is a value.
    pub(crate) fn static_member_value(
        &mut self,
        base: NodeRef<'_>,
        name: &str,
        n: NodeRef<'_>,
    ) -> StageResult<Option<(Ref, Ty)>> {
        let text = self.cx.src.text(self.cx.src.first(base)).to_owned();
        match self.scope_name(&text) {
            Some(Named::Poison) => Ok(Some(self.poison_value(n))),
            Some(Named::Module(m)) => {
                let Some(def) = self.export(m, name) else {
                    self.missing_export(m, name, n);
                    return Ok(Some((Ref(NONE), Ty::NEVER)));
                };
                if let Some(ItemData::Fn(sig)) = self.cx.lookup.item(def).map(|i| &i.data) {
                    let sig = sig.clone();
                    return self.item_value(def, &sig, &[], None, n).map(Some);
                }
                unsupported("a module member value that is not a function")
            }
            Some(Named::Item(def)) if self.kind_of_item(def) == Some(HeadKind::Enum) => {
                let t = self.ctor(def, &[])?;
                let vnode = n;
                Ok(Some(self.variant_by_name(
                    t,
                    name,
                    &Args::empty(),
                    vnode,
                )?))
            }
            _ => Ok(None),
        }
    }

    /// A variant value of `t` from positional or named payload values.
    fn variant_by_name(
        &mut self,
        t: Ty,
        name: &str,
        args: &Args<'_>,
        n: NodeRef<'_>,
    ) -> StageResult<(Ref, Ty)> {
        let Some((index, fields)) = self.variant_fields(t, name) else {
            let msg = format!("no variant `{name}` of {}", self.show(t));
            self.err(Code::UnknownVariant, n, &msg);
            return Ok((Ref(NONE), Ty::NEVER));
        };
        let names = self.variant_field_names(t, name);
        let mut slots: Vec<Option<Ref>> = vec![None; fields.len()];
        let mut extra = false;
        for (i, e) in args.positional.iter().enumerate() {
            let ft = fields.get(i).copied();
            let (r, rt) = self.expr(*e, ft)?;
            match (ft, slots.get_mut(i)) {
                (Some(ft), Some(slot)) => *slot = Some(self.coerce(r, rt, ft, *e, "payload")),
                _ => extra = true,
            }
        }
        for (fname, e) in &args.named {
            let Some(i) = names.iter().position(|x| x == fname) else {
                let msg = format!("no field `{fname}` of variant `{name}`");
                self.err(Code::UnknownDataField, *e, &msg);
                continue;
            };
            let ft = fields[i];
            let (r, rt) = self.expr(*e, Some(ft))?;
            slots[i] = Some(self.coerce(r, rt, ft, *e, "payload"));
        }
        if extra || slots.iter().any(Option::is_none) {
            let msg = format!("`{name}` takes {} values", fields.len());
            self.err(Code::ArgumentCount, n, &msg);
        }
        let refs: Vec<Ref> = slots.into_iter().map(|r| r.unwrap_or(Ref(NONE))).collect();
        let rec = self.b.refs_record(&refs);
        Ok((self.b.emit(Tag::NewVariant, index, rec, t, n.index()), t))
    }

    /// A variant's payload field names (`0`, `1`... when positional).
    fn variant_field_names(&self, t: Ty, name: &str) -> Vec<String> {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        let t = match pool.get(t) {
            TyData::Mut(i) => i,
            _ => t,
        };
        let TyData::Adt { def, .. } = pool.get(t) else {
            return vec!["0".into()];
        };
        let Some(ItemData::Enum { variants, .. }) = self.cx.lookup.item(def).map(|i| &i.data)
        else {
            return vec![];
        };
        let sym = self.cx.names.syms.intern(name);
        variants
            .iter()
            .find(|v| v.name == sym)
            .map(|v| {
                v.fields
                    .iter()
                    .map(|f| self.cx.names.text(f.name).to_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// `base.name(args)` and `Base::name(args)`.
    fn member_call(
        &mut self,
        n: NodeRef<'_>,
        callee: NodeRef<'_>,
        args: &Args<'_>,
        bang: bool,
        want: Option<Ty>,
        method_targs: Vec<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let Some(base) = callee.children().next() else {
            return unsupported("a member call without a base");
        };
        let name = self.cx.src.text(self.cx.src.last(callee)).to_owned();
        // Static forms: a type, a type parameter, a trait or a module.
        if matches!(base.kind(), SyntaxKind::NameExpr | SyntaxKind::TypeArgsExpr)
            && (base.kind() == SyntaxKind::TypeArgsExpr
                || self.find_local(self.sym_of(base)).is_none())
        {
            let (bnode, explicit) = if base.kind() == SyntaxKind::TypeArgsExpr {
                let Some(inner) = base.children().next() else {
                    return unsupported("a type-argument base");
                };
                let mut ex = Vec::new();
                if let Some(tl) = Src::child(base, SyntaxKind::TypeArgumentList) {
                    for t in tl.children().filter(|c| c.kind().is_type()) {
                        ex.push(self.ty_node(t)?);
                    }
                }
                (inner, ex)
            } else {
                (base, vec![])
            };
            let text = self.cx.src.text(self.cx.src.first(bnode)).to_owned();
            if let Some(p) = self.type_param(&text) {
                self.method_targs = method_targs;
                return self.static_on_type(p, &name, args, (n, bang), true, want);
            }
            if let Some(p) = Prim::ALL.iter().find(|p| p.name() == text) {
                self.method_targs = method_targs;
                return self.static_on_type(Ty::prim(*p), &name, args, (n, bang), false, want);
            }
            match self.scope_name(&text) {
                Some(Named::Poison) => return Ok(self.poison_value(n)),
                Some(Named::Module(m)) => {
                    let Some(def) = self.export(m, &name) else {
                        self.missing_export(m, &name, callee);
                        return Ok((Ref(NONE), Ty::NEVER));
                    };
                    return self.call_item(def, &explicit, args, n, bang, want);
                }
                Some(Named::Item(def)) => match self.kind_of_item(def) {
                    Some(HeadKind::Enum)
                        if {
                            let et = self.ctor(def, &explicit)?;
                            self.variant_fields(et, &name).is_some()
                        } =>
                    {
                        let t = self.ctor(def, &explicit)?;
                        let t = match want {
                            Some(w) if self.can_unify(w, t) => {
                                self.expect(t, w, n, "variant");
                                t
                            }
                            _ => t,
                        };
                        return self.variant_by_name(t, &name, args, n);
                    }
                    Some(HeadKind::Trait) => {
                        self.method_targs = method_targs;
                        return self.trait_static(def, explicit, &name, args, n, bang);
                    }
                    Some(k) if k.is_type() => {
                        let t = self.ctor(def, &explicit)?;
                        self.method_targs = method_targs;
                        return self.static_on_type(t, &name, args, (n, bang), false, want);
                    }
                    _ => {}
                },
                None => {}
            }
        }
        let (recv, rt) = self.expr(base, None)?;
        self.method_targs = method_targs;
        self.method_on(recv, rt, &name, args, (n, bang), want)
    }

    /// `T.name(args)` on a type: an inherent associated function, or a
    /// trait's static method through a bound of a parameter.
    fn static_on_type(
        &mut self,
        t: Ty,
        name: &str,
        args: &Args<'_>,
        (n, bang): (NodeRef<'_>, bool),
        is_param: bool,
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
        if !is_param && let Some((method, impl_def, impl_args)) = self.find_inherent(t, name) {
            let sig = self.sig_of(method)?;
            let explicit = std::mem::take(&mut self.method_targs);
            let vars = self.fresh_generics(&sig, &explicit, 0);
            let inst = |x: Ty| {
                subst_owner(
                    pool,
                    method,
                    &vars,
                    subst_owner(pool, impl_def, &impl_args, x),
                )
            };
            let params: Vec<(Symbol, Ty)> =
                sig.params.iter().map(|(s, x)| (*s, inst(*x))).collect();
            let mut all = impl_args.clone();
            all.extend(&vars);
            self.default_owner = Some((method, pool.list(&all), vec![]));
            let formals = Formals {
                params: &params,
                defaults: &sig.defaults,
                variadic: sig.variadic,
            };
            let refs = self.check_args(&formals, 0, args, n, name)?;
            self.bounds_of(&sig, &vars, &inst, n)?;
            self.check_row(sig.row, n);
            let mut targs = impl_args.clone();
            targs.extend(&vars);
            let c = Callee::Item {
                def: method,
                targs: pool.list(&targs),
            };
            let ret = self.normalize_deep(inst(sig.ret))?;
            return Ok(self.emit_call(&c, &refs, ret, sig.suspends, bang, n));
        }
        // A trait method with no receiver through a bound (`N::zero()`).
        let sym = self.cx.names.syms.intern(name);
        let traits: Vec<(DefId, DefId)> = self
            .method_index()
            .traits
            .get(&sym)
            .cloned()
            .unwrap_or_default();
        // Through a type, the trait part of `Methods`
        // (`trait.assoc-call.type.traits`): `X::from(v)` chooses among
        // `X`'s instantiations of `From` like a dot call.
        if !is_param {
            match self.trait_part(t, sym, &traits)? {
                Some(Hit::Ambiguous(found)) => {
                    let names: Vec<String> = self
                        .cx
                        .names
                        .display_names(&found)
                        .iter()
                        .map(|d| format!("`{d}`"))
                        .collect();
                    let msg = format!(
                        "`{}::{name}` is ambiguous: the traits {} each supply it; write `Trait::{name}(..)`",
                        self.show(t),
                        names.join(" and ")
                    );
                    self.err(Code::AmbiguousMethod, n, &msg);
                    return Ok((Ref(NONE), Ty::NEVER));
                }
                Some(Hit::Choice {
                    trait_,
                    method,
                    cands,
                }) => {
                    return self.choose_method(
                        (trait_, method, t),
                        &cands,
                        None,
                        args,
                        (n, bang),
                        want,
                    );
                }
                Some(Hit::Trait {
                    trait_,
                    method,
                    args: targs,
                    ..
                }) => {
                    return self.trait_method_call(
                        TraitTarget {
                            trait_,
                            method,
                            self_ty: t,
                            args: targs,
                        },
                        None,
                        args,
                        n,
                        bang,
                    );
                }
                _ => {
                    let msg = format!("no method `{name}` on {}", self.show(t));
                    self.no_method(t, name, n, &msg);
                    return Ok((Ref(NONE), Ty::NEVER));
                }
            }
        }
        for (tr, m) in traits {
            let tref = TraitRef {
                trait_: tr,
                self_ty: t,
                args: TyList::EMPTY,
            };
            let holds = if is_param {
                (0..self.env.clause_self.len())
                    .any(|i| self.env.clause_self[i] == t && self.env.clause_trait[i] == tr)
            } else {
                matches!(self.solve(tref)?, hd_types::solver::Answer::Holds { .. })
            };
            if holds {
                return self.trait_method_call(
                    TraitTarget {
                        trait_: tr,
                        method: m,
                        self_ty: t,
                        args: vec![],
                    },
                    None,
                    args,
                    n,
                    bang,
                );
            }
        }
        let msg = format!("no method `{name}` on {}", self.show(t));
        self.no_method(t, name, n, &msg);
        Ok((Ref(NONE), Ty::NEVER))
    }

    /// `Trait::method(args)`: the self type comes from the arguments; the
    /// trait's arguments are the written ones (`Combine::[i32]::combine`,
    /// `trait.qualified.trait-arguments`), the rest fresh.
    fn trait_static(
        &mut self,
        tr: DefId,
        trait_args: Vec<Ty>,
        name: &str,
        args: &Args<'_>,
        n: NodeRef<'_>,
        bang: bool,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
        let Some(ItemData::Trait(td)) = self.cx.lookup.item(tr).map(|i| &i.data) else {
            return unsupported("a trait path");
        };
        let sym = self.cx.names.syms.intern(name);
        let Some(&(_, m)) = td.methods.iter().find(|(s, _)| *s == sym) else {
            let msg = format!(
                "no method `{name}` on trait {}",
                self.cx.names.display_name(tr)
            );
            self.err(Code::UnknownMethod, n, &msg);
            return Ok((Ref(NONE), Ty::NEVER));
        };
        // Inside a template, `Structure::f(..)` and `Trait::f(..)` of the
        // derived trait have the template's `T` as `Self`
        // (`annot.template.qualified-self`).
        let self_ty = match self.template_self(tr) {
            Some(t) => t,
            None => self.infer.fresh(pool, VarKind::General),
        };
        self.trait_method_call(
            TraitTarget {
                trait_: tr,
                method: m,
                self_ty,
                args: trait_args,
            },
            None,
            args,
            n,
            bang,
        )
    }

    pub(crate) fn sig_of(&self, d: DefId) -> StageResult<FnSig> {
        match self.cx.lookup.item(d).and_then(|i| i.sig().cloned()) {
            Some(s) => Ok(self.with_result(d, s)),
            None => unsupported("a method without a signature"),
        }
    }

    /// The bounds of a callee's own parameters, under its full
    /// instantiation (`Self` and the owner's parameters included).
    pub(crate) fn bounds_of(
        &mut self,
        sig: &FnSig,
        vars: &[Ty],
        inst: &dyn Fn(Ty) -> Ty,
        n: NodeRef<'_>,
    ) -> StageResult<()> {
        let pool = self.pool();
        for (i, g) in sig.generics.iter().enumerate() {
            let before = self.diags.len();
            for b in &g.bounds {
                let b = inst(*b);
                if let TyData::TraitValue {
                    def: tr, args: ta, ..
                } = pool.get(b)
                {
                    let tref = TraitRef {
                        trait_: tr,
                        self_ty: vars[i],
                        args: ta,
                    };
                    self.require_ref(tref, n)?;
                }
            }
            if g.mut_bound && self.diags.len() == before {
                self.check_mut_bound(vars[i], n);
            }
        }
        for (i, g) in sig.generics.iter().enumerate() {
            for b in &g.bounds {
                self.check_bindings(vars[i], inst(*b), n)?;
            }
        }
        Ok(())
    }

    /// An inherent method of a type that is visible here
    /// (`names.method-lookup.inherent`): (method, impl, impl arguments).
    pub(crate) fn find_inherent(&mut self, t: Ty, name: &str) -> Option<(DefId, DefId, Vec<Ty>)> {
        self.inherent_where(t, name, true)
    }

    /// An inherent method of a type, whatever its visibility: what the
    /// compiler's own desugarings call.
    pub(crate) fn find_inherent_any(
        &mut self,
        t: Ty,
        name: &str,
    ) -> Option<(DefId, DefId, Vec<Ty>)> {
        self.inherent_where(t, name, false)
    }

    /// No member was selected for `t.name(..)`: `private-member` when `t`
    /// itself has an inherent method `name` that is not visible here
    /// (`names.method-lookup.private`), else `unknown-method` with `msg`
    /// (`names.method-lookup.unknown`).
    pub(crate) fn no_method(&mut self, t: Ty, name: &str, n: NodeRef<'_>, msg: &str) {
        let snap = self.infer.snapshot();
        let private = self.find_inherent_any(t, name).is_some();
        self.infer.rollback(snap);
        if private {
            let msg = format!(
                "the method `{name}` of {} is private to its module",
                self.show(t)
            );
            self.err(Code::PrivateMember, n, &msg);
        } else {
            self.err(Code::UnknownMethod, n, msg);
        }
    }

    fn inherent_where(
        &mut self,
        t: Ty,
        name: &str,
        visible_only: bool,
    ) -> Option<(DefId, DefId, Vec<Ty>)> {
        let pool = self.pool();
        let sym = self.cx.names.syms.intern(name);
        let cands = self.method_index().inherent.get(&sym).cloned()?;
        let t = match pool.get(self.infer.shallow(pool, t)) {
            TyData::Mut(i) => i,
            _ => t,
        };
        for (impl_def, method) in cands {
            // A local impl's methods are found only in its extent
            // (`trait.impl.local.lookup`).
            if self.hidden.contains(&impl_def) || (visible_only && !self.method_visible(method)) {
                continue;
            }
            let Some(item) = self.cx.lookup.item(impl_def) else {
                continue;
            };
            let ItemData::Impl { self_ty, .. } = &item.data else {
                continue;
            };
            let n = item.generics.len();
            let vars: Vec<Ty> = (0..n)
                .map(|_| self.infer.fresh(pool, VarKind::General))
                .collect();
            let head = subst_owner(pool, impl_def, &vars, *self_ty);
            let snap = self.infer.snapshot();
            if self.infer.unify(pool, head, t).is_ok() {
                // The impl's bounds must hold (`impl[T < Eq] List[T]`).
                let mut ok = true;
                for (i, g) in item.generics.iter().enumerate() {
                    for b in &g.bounds {
                        let b = subst_owner(pool, impl_def, &vars, *b);
                        if let TyData::TraitValue { def: tr, args, .. } = pool.get(b) {
                            let tref = TraitRef {
                                trait_: tr,
                                self_ty: self.infer.resolve(pool, vars[i]),
                                args,
                            };
                            if matches!(self.solve(tref), Ok(hd_types::solver::Answer::Fails(_))) {
                                ok = false;
                            }
                        }
                    }
                }
                if ok {
                    return Some((method, impl_def, vars));
                }
            }
            self.infer.rollback(snap);
        }
        None
    }

    /// A built-in method of `List`, `Map` (spec/lang/10-modules.md#built-in-methods).
    fn builtin_method(&mut self, t: Ty, name: &str) -> Option<(IntrinsicOp, Vec<Ty>, Ty)> {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        let t = match pool.get(t) {
            TyData::Mut(i) => i,
            _ => t,
        };
        let TyData::Adt { def, args } = pool.get(t) else {
            return None;
        };
        let a = pool.list_items(args);
        let usize_t = Ty::prim(Prim::Usize);
        let iterator = self.cx.names.known.iterator;
        let iter_of = |x: Ty| {
            let it = pool.intern_ty(&TyData::Adt {
                def: iterator,
                args: pool.list(&[x]),
            });
            pool.intern_ty(&TyData::Mut(it))
        };
        let known = self.cx.names.known;
        let which = if def == known.list {
            1
        } else if def == known.map {
            2
        } else {
            0
        };
        match (which, name) {
            (1, "len") => Some((IntrinsicOp::ListLen, vec![], usize_t)),
            (1, "push") => Some((IntrinsicOp::ListPush, vec![a[0]], Ty::VOID)),
            (1, "iter") => Some((IntrinsicOp::ListIter, vec![], iter_of(a[0]))),
            (2, "len") => Some((IntrinsicOp::MapLen, vec![], usize_t)),
            (2, "get") => Some((
                IntrinsicOp::MapGet,
                vec![a[0]],
                pool.intern_ty(&TyData::Option(a[1])),
            )),
            (2, "remove") => Some((
                IntrinsicOp::MapRemove,
                vec![a[0]],
                pool.intern_ty(&TyData::Option(a[1])),
            )),
            (2, "iter") => {
                let pair = pool.intern_ty(&TyData::Tuple {
                    elems: pool.list(&[a[0], a[1]]),
                    rest: None,
                });
                Some((IntrinsicOp::MapIter, vec![], iter_of(pair)))
            }
            _ => None,
        }
    }

    pub(crate) fn resolve_method(&mut self, rt: Ty, name: &str) -> StageResult<Option<Hit>> {
        let pool = self.pool();
        let t = self.infer.resolve(pool, rt);
        let t = match pool.get(t) {
            TyData::Mut(i) => i,
            _ => t,
        };
        if let Some((method, impl_def, impl_args)) = self.find_inherent(t, name) {
            return Ok(Some(Hit::Inherent {
                method,
                impl_def,
                impl_args,
            }));
        }
        if let Some((op, params, ret)) = self.builtin_method(t, name) {
            return Ok(Some(Hit::Builtin { op, params, ret }));
        }
        let sym = self.cx.names.syms.intern(name);
        let traits = self
            .method_index()
            .traits
            .get(&sym)
            .cloned()
            .unwrap_or_default();
        // A parameter: its bounds, supertraits included.
        if let TyData::Param(p) = pool.get(t) {
            for i in 0..self.env.clause_self.len() {
                if self.env.clause_self[i] != t {
                    continue;
                }
                let tr = self.env.clause_trait[i];
                if let Some(&(_, m)) = traits.iter().find(|(x, _)| *x == tr) {
                    return Ok(Some(Hit::Trait {
                        trait_: tr,
                        method: m,
                        args: pool.list_items(self.env.clause_args[i]).to_vec(),
                        choice: (ChoiceKind::Bound, u32::try_from(i).unwrap_or(0)),
                    }));
                }
            }
            let _ = p;
            return Ok(None);
        }
        // A trait value: its trait and supertraits, a supertrait at the
        // arguments the value's trait gives it (trait.dyn.value-methods).
        if let TyData::TraitValue { def, args, .. } = pool.get(t) {
            for (tr, m) in &traits {
                if let Some(a) = self.super_args(def, t, args, *tr, 0) {
                    return Ok(Some(Hit::Trait {
                        trait_: *tr,
                        method: *m,
                        args: pool.list_items(a).to_vec(),
                        choice: (ChoiceKind::TraitValue, tr.raw()),
                    }));
                }
            }
            return Ok(None);
        }
        // A literal's type is its default once a member is asked of it.
        if matches!(pool.get(t), TyData::Infer(_))
            && let Some(k @ (VarKind::IntLit | VarKind::SignedIntLit | VarKind::FloatLit)) =
                self.infer.kind_of(pool, t)
        {
            let d = match k {
                VarKind::SignedIntLit => Ty::I32,
                VarKind::IntLit => Ty::prim(Prim::Usize),
                _ => Ty::prim(Prim::F64),
            };
            let _ = self.infer.unify(pool, t, d);
            return self.resolve_method(d, name);
        }
        if matches!(pool.get(t), TyData::Infer(_)) {
            return Err(hd_base::NotImplemented::new(
                hd_base::Stage::Body,
                "a method on a value whose type is not yet known",
            ));
        }
        self.trait_part(t, sym, &traits)
    }

    /// The trait methods named `sym` of a concrete type: the
    /// compiler-answered traits, then the trait part of `Methods`
    /// (trait-solver.md §6.5), each other trait with the method by the
    /// impl candidates the type has. Two traits are ambiguous.
    fn trait_part(
        &mut self,
        t: Ty,
        sym: Symbol,
        traits: &[(DefId, DefId)],
    ) -> StageResult<Option<Hit>> {
        let pool = self.pool();
        let mut builtin = Vec::new();
        let mut asked = Vec::new();
        for &(tr, m) in traits {
            let n_args = self.cx.lookup.item(tr).map_or(0, |i| i.generics.len());
            let fresh: Vec<Ty> = (0..n_args)
                .map(|_| self.infer.fresh(pool, VarKind::General))
                .collect();
            let tref = TraitRef {
                trait_: tr,
                self_ty: t,
                args: pool.list(&fresh),
            };
            if self.supplied_holds(tref)?.is_some() {
                builtin.push((tr, m, fresh));
            } else {
                asked.push(tr);
            }
        }
        let goal = Goal::Methods {
            receiver: t,
            name: sym,
            traits: asked,
        };
        let cands = match self.solve_goal(&goal)? {
            Answer::Candidates(c) => c,
            Answer::Stalled { .. } => {
                return unsupported("a trait method whose impl waits for inference");
            }
            Answer::OutOfFuel => {
                return unsupported("the solver's fuel ran out (limit diagnostic)");
            }
            other => return unsupported(format!("solver answer {other:?} to a method lookup")),
        };
        // The candidates by trait, in content order of their first one.
        let mut groups: Vec<(DefId, Vec<Candidate>)> = Vec::new();
        for c in cands {
            let Some(table) = self.cx.impl_table(c.row.module) else {
                continue;
            };
            let tr = table.trait_[c.row.row as usize];
            match groups.iter_mut().find(|g| g.0 == tr) {
                Some(g) => g.1.push(c),
                None => groups.push((tr, vec![c])),
            }
        }
        if builtin.len() + groups.len() > 1 {
            let mut found: Vec<DefId> = builtin.iter().map(|b| b.0).collect();
            found.extend(groups.iter().map(|g| g.0));
            return Ok(Some(Hit::Ambiguous(found)));
        }
        if let Some((tr, m, args)) = builtin.pop() {
            return Ok(Some(Hit::Trait {
                trait_: tr,
                method: m,
                args,
                choice: (ChoiceKind::Builtin, 0),
            }));
        }
        Ok(groups.pop().and_then(|(tr, cands)| {
            let &(_, method) = traits.iter().find(|(x, _)| *x == tr)?;
            Some(Hit::Choice {
                trait_: tr,
                method,
                cands,
            })
        }))
    }

    /// `recv.name(args)`.
    fn method_on(
        &mut self,
        recv: Ref,
        rt: Ty,
        name: &str,
        args: &Args<'_>,
        (n, bang): (NodeRef<'_>, bool),
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
        let rt = self.norm_ty(rt);
        // A receiver whose type an earlier error left unknown adds nothing.
        if matches!(self.strip_mut(rt), Ty::NEVER | Ty::POISON) {
            for e in &args.positional {
                self.expr(*e, None)?;
            }
            return Ok((Ref(NONE), Ty::NEVER));
        }
        // A field holding a function: `(x.f)(...)` is written `x.f(...)` only
        // when no method of that name exists.
        let hit = self.resolve_method(rt, name)?;
        if matches!(
            hit,
            None | Some(Hit::Trait { .. } | Hit::Choice { .. } | Hit::Ambiguous(_))
        ) && let Some(path) = self.promoted_method_path(rt, name)
        {
            // A trait candidate beside the promoted one: neither wins
            // (names.method-lookup.ambiguous).
            if hit.is_some() {
                let msg = format!(
                    "`{name}` on {} is ambiguous: a trait method and the promoted method `{}.{name}`; write `Trait::{name}(..)` or the explicit path",
                    self.show(rt),
                    path.join(".")
                );
                self.err(Code::AmbiguousMethod, n, &msg);
                return Ok((Ref(NONE), Ty::NEVER));
            }
            let (recv, rt) = self.walk_path(recv, rt, &path, n);
            return self.method_on(recv, rt, name, args, (n, bang), want);
        }
        match hit {
            Some(Hit::Ambiguous(traits)) => {
                let names: Vec<String> = self
                    .cx
                    .names
                    .display_names(&traits)
                    .iter()
                    .map(|t| format!("`{t}`"))
                    .collect();
                let msg = format!(
                    "`{name}` on {} is ambiguous: the traits {} each supply it; write `Trait::{name}(..)`",
                    self.show(rt),
                    names.join(" and ")
                );
                self.err(Code::AmbiguousMethod, n, &msg);
                Ok((Ref(NONE), Ty::NEVER))
            }
            Some(Hit::Choice {
                trait_,
                method,
                cands,
            }) => {
                let t = self.strip_mut(self.infer.resolve(pool, rt));
                if let Some(&(s, st)) = self.sig_of(method)?.params.first()
                    && self.cx.names.text(s) == "self"
                {
                    self.check_receiver(st, rt, n);
                }
                self.choose_method(
                    (trait_, method, t),
                    &cands,
                    Some(recv),
                    args,
                    (n, bang),
                    want,
                )
            }
            Some(Hit::Inherent {
                method,
                impl_def,
                impl_args,
            }) => {
                let sig = self.sig_of(method)?;
                let explicit = std::mem::take(&mut self.method_targs);
                let vars = self.fresh_generics(&sig, &explicit, 0);
                let inst = |x: Ty| {
                    subst_owner(
                        pool,
                        method,
                        &vars,
                        subst_owner(pool, impl_def, &impl_args, x),
                    )
                };
                let params: Vec<(Symbol, Ty)> =
                    sig.params.iter().map(|(s, x)| (*s, inst(*x))).collect();
                let has_self = sig
                    .params
                    .first()
                    .is_some_and(|p| self.cx.names.text(p.0) == "self");
                if !has_self {
                    let msg = format!("`{name}` is an associated function, not a method");
                    self.err(Code::UnknownMethod, n, &msg);
                    return Ok((Ref(NONE), Ty::NEVER));
                }
                self.check_receiver(params[0].1, rt, n);
                let mut refs = vec![recv];
                let mut all = impl_args.clone();
                all.extend(&vars);
                self.default_owner = Some((method, pool.list(&all), vec![recv]));
                let formals = Formals {
                    params: &params,
                    defaults: &sig.defaults,
                    variadic: sig.variadic,
                };
                refs.extend(self.check_args(&formals, 1, args, n, name)?);
                // `h.fact::[D]()` on a structure handle reads a typed fact
                // (annot.handle.fact.typed): `D` is matched against the
                // member's type, not bounded by `Inspectable`.
                let handle_fact = name == "fact"
                    && matches!(
                        pool.get(self.strip_mut(rt)),
                        TyData::Adt { def, .. } if def == self.cx.names.known.field
                    );
                if !handle_fact {
                    self.bounds_of(&sig, &vars, &inst, n)?;
                }
                self.check_row(sig.row, n);
                let mut targs = impl_args.clone();
                targs.extend(&vars);
                let c = Callee::Item {
                    def: method,
                    targs: pool.list(&targs),
                };
                let ret = self.normalize_deep(inst(sig.ret))?;
                Ok(self.emit_call(&c, &refs, ret, sig.suspends, bang, n))
            }
            Some(Hit::Builtin { op, params, ret }) => {
                if matches!(op, IntrinsicOp::ListPush | IntrinsicOp::MapRemove) {
                    let st = pool.intern_ty(&TyData::Mut(self.strip_mut(rt)));
                    self.check_receiver(st, rt, n);
                }
                let ps: Vec<(Symbol, Ty)> = params
                    .iter()
                    .map(|t| (self.cx.names.syms.intern("value"), *t))
                    .collect();
                let mut refs = vec![recv];
                let mut full = vec![(self.cx.names.syms.intern("self"), rt)];
                full.extend(ps);
                let formals = Formals {
                    params: &full,
                    defaults: &[],
                    variadic: false,
                };
                refs.extend(self.check_args(&formals, 1, args, n, name)?);
                let rec = self.b.refs_record(&refs);
                Ok((
                    self.b.emit(Tag::Intrinsic, op as u32, rec, ret, n.index()),
                    ret,
                ))
            }
            Some(Hit::Trait {
                trait_,
                method,
                args: targs,
                choice,
            }) => {
                let t = self.infer.resolve(pool, rt);
                let t = match pool.get(t) {
                    TyData::Mut(i) => i,
                    _ => t,
                };
                if let Some(&(s, st)) = self.sig_of(method)?.params.first()
                    && self.cx.names.text(s) == "self"
                {
                    self.check_receiver(st, rt, n);
                }
                self.trait_method_call(
                    TraitTarget {
                        trait_,
                        method,
                        self_ty: t,
                        args: targs,
                    },
                    Some((recv, choice)),
                    args,
                    n,
                    bang,
                )
            }
            None => {
                // A field of function type.
                if let Some((idx, ft)) = self.field_of(rt, name)
                    && matches!(pool.get(self.infer.resolve(pool, ft)), TyData::Fn { .. })
                {
                    self.check_field_visible(rt, name, n);
                    let f = self.b.emit(Tag::Field, recv.0, idx, ft, n.index());
                    return self.call_value(f, ft, args, n, bang);
                }
                let msg = format!("no method `{name}` on {}", self.show(rt));
                self.no_method(rt, name, n, &msg);
                Ok((Ref(NONE), Ty::NEVER))
            }
        }
    }

    /// A trait method call: `Self` is `self_ty`, the trait's parameters
    /// are `targs` (fresh when empty), the receiver is `recv` when given.
    pub(crate) fn trait_method_call(
        &mut self,
        target: TraitTarget,
        recv: Option<(Ref, (ChoiceKind, u32))>,
        args: &Args<'_>,
        n: NodeRef<'_>,
        bang: bool,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
        let TraitTarget {
            trait_,
            method,
            self_ty,
            args: mut targs,
        } = target;
        let sig = self.sig_of(method)?;
        let n_trait = self.cx.lookup.item(trait_).map_or(0, |i| i.generics.len());
        while targs.len() < n_trait {
            targs.push(self.infer.fresh(pool, VarKind::General));
        }
        let explicit = std::mem::take(&mut self.method_targs);
        let vars = self.fresh_generics(&sig, &explicit, 0);
        let tv = targs.clone();
        let tl = pool.list(&tv);
        let inst = |x: Ty| {
            let x = subst_owner(pool, method, &vars, x);
            let x = pool.subst(x, &|p: ParamRef| {
                if p.owner != trait_ {
                    return None;
                }
                if p.index == 0 {
                    Some(self_ty)
                } else {
                    tv.get(p.index as usize - 1).copied()
                }
            });
            with_assoc_args(pool, x, trait_, tl)
        };
        let params: Vec<(Symbol, Ty)> = sig.params.iter().map(|(s, x)| (*s, inst(*x))).collect();
        let has_self = sig
            .params
            .first()
            .is_some_and(|p| self.cx.names.text(p.0) == "self");
        let mut refs = Vec::new();
        let skip = match recv {
            Some((r, _)) if has_self => {
                refs.push(r);
                1
            }
            _ => 0,
        };
        let formals = Formals {
            params: &params,
            defaults: &sig.defaults,
            variadic: sig.variadic,
        };
        let method_name = self
            .cx
            .names
            .text(
                self.cx
                    .lookup
                    .item(method)
                    .map_or(Symbol::from_raw(0), |i| i.name),
            )
            .to_owned();
        refs.extend(self.check_args(&formals, skip, args, n, &method_name)?);
        self.bounds_of(&sig, &vars, &inst, n)?;
        // At an opt-in, a `walk`, `describe` or `build` call's walker,
        // describer or source type carries the member obligations.
        if self.opt_in.is_some()
            && trait_ == self.cx.names.known.structure
            && let Some(&w) = vars.first()
        {
            self.structure_calls.push((method, w));
        }
        self.check_row(sig.row, n);
        let choice = if let Some((_, c)) = recv {
            c
        } else {
            let st = self.infer.resolve(pool, self_ty);
            let tref = TraitRef {
                trait_,
                self_ty: st,
                args: pool.list(&targs),
            };
            let ev = self.require_ref(tref, n)?;
            self.choice_of(ev.as_ref())
        };
        let ret = self.normalize_deep(inst(sig.ret))?;
        let mut all = targs.clone();
        all.extend(&vars);
        let c = Callee::TraitMethod {
            trait_,
            method,
            self_ty: self.infer.resolve(pool, self_ty),
            targs: pool.list(&all),
            choice,
        };
        Ok(self.emit_call(&c, &refs, ret, sig.suspends, bang, n))
    }

    /// `trait_.name(recv, args...)` for operators, `Display` and `iter`.
    pub(crate) fn trait_call(
        &mut self,
        trait_: DefId,
        name: &str,
        recv: Ref,
        rt: Ty,
        args: &[(Ref, Ty, NodeRef<'_>)],
        n: NodeRef<'_>,
    ) -> StageResult<(Ref, Ty)> {
        self.trait_call_args(trait_, name, recv, rt, args, n)
    }

    pub(crate) fn trait_call_args(
        &mut self,
        trait_: DefId,
        name: &str,
        recv: Ref,
        rt: Ty,
        args: &[(Ref, Ty, NodeRef<'_>)],
        n: NodeRef<'_>,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
        let method = self.cx.names.member(trait_, PathKind::Member, name);
        let sig = self.sig_of(method)?;
        let t = self.infer.resolve(pool, rt);
        let t = match pool.get(t) {
            TyData::Mut(i) => i,
            _ => t,
        };
        let n_trait = self.cx.lookup.item(trait_).map_or(0, |i| i.generics.len());
        let mut targs: Vec<Ty> = (0..n_trait)
            .map(|_| self.infer.fresh(pool, VarKind::General))
            .collect();
        // An operator's right operand fixes `Rhs` when the impl is generic.
        if let (Some(first), Some((_, at, _))) = (targs.first().copied(), args.first())
            && sig.params.get(1).is_some_and(
                |p| matches!(pool.get(p.1), TyData::Param(q) if q.owner == trait_ && q.index == 1),
            )
        {
            let _ = self.infer.unify(pool, first, *at);
        }
        let tref = TraitRef {
            trait_,
            self_ty: t,
            args: pool.list(&targs),
        };
        let ev = self.require_ref(tref, n)?;
        let choice = self.choice_of(ev.as_ref());
        targs = targs
            .into_iter()
            .map(|x| self.infer.resolve(pool, x))
            .collect();
        let tv = targs.clone();
        let tl = pool.list(&tv);
        let inst = |x: Ty| {
            let x = pool.subst(x, &|p: ParamRef| {
                if p.owner != trait_ {
                    return None;
                }
                if p.index == 0 {
                    Some(t)
                } else {
                    tv.get(p.index as usize - 1).copied()
                }
            });
            with_assoc_args(pool, x, trait_, tl)
        };
        let mut refs = vec![recv];
        for (i, (r, at, an)) in args.iter().enumerate() {
            let w = sig.params.get(i + 1).map(|p| inst(p.1));
            refs.push(match w {
                Some(w) => self.coerce(*r, *at, w, *an, "operand"),
                None => *r,
            });
        }
        let ret = self.normalize_deep(inst(sig.ret))?;
        let c = Callee::TraitMethod {
            trait_,
            method,
            self_ty: t,
            targs: pool.list(&targs),
            choice,
        };
        Ok(self.emit_call(&c, &refs, ret, sig.suspends, false, n))
    }

    /// `inherent.name(recv, args)` on a known type.
    pub(crate) fn inherent_call(
        &mut self,
        t: Ty,
        name: &str,
        recv: Ref,
        args: &[Ref],
        n: NodeRef<'_>,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
        let Some((method, impl_def, impl_args)) = self.find_inherent_any(t, name) else {
            return unsupported(format!("the inherent method `{name}`"));
        };
        let sig = self.sig_of(method)?;
        let explicit = std::mem::take(&mut self.method_targs);
        let vars = self.fresh_generics(&sig, &explicit, 0);
        let ret = subst_owner(
            pool,
            method,
            &vars,
            subst_owner(pool, impl_def, &impl_args, sig.ret),
        );
        let ret = self.normalize_deep(ret)?;
        let mut refs = vec![recv];
        refs.extend_from_slice(args);
        let mut targs = impl_args;
        targs.extend(&vars);
        let c = Callee::Item {
            def: method,
            targs: pool.list(&targs),
        };
        Ok(self.emit_call(&c, &refs, ret, sig.suspends, false, n))
    }

    // ------------------------------------------------------------ projections

    /// The trait that declares `name` for a projection on `base`.
    pub(crate) fn assoc_owner(&self, base: Ty, name: &str) -> Option<(DefId, TyList)> {
        let sym = self.cx.names.syms.intern(name);
        let has = |tr: DefId| match self.cx.lookup.item(tr).map(|i| &i.data) {
            Some(ItemData::Trait(t)) => t.assoc.iter().any(|(s, _)| *s == sym),
            _ => false,
        };
        (0..self.env.clause_self.len())
            .find(|&i| self.env.clause_self[i] == base && has(self.env.clause_trait[i]))
            .map(|i| (self.env.clause_trait[i], self.env.clause_args[i]))
    }

    /// A bound's associated-type bindings at a use site
    /// (`trait.binding.use-site`): each projection of the argument
    /// normalizes and unifies with the bound type, which infers a
    /// parameter named there (`trait.binding.inference`). A projection
    /// still waiting on inference is skipped.
    pub(crate) fn check_bindings(
        &mut self,
        self_ty: Ty,
        bound: Ty,
        n: NodeRef<'_>,
    ) -> StageResult<()> {
        let pool = self.pool();
        let TyData::TraitValue {
            def,
            args,
            bindings,
        } = pool.get(bound)
        else {
            return Ok(());
        };
        let st = self.strip_mut(self.infer.resolve(pool, self_ty));
        if matches!(st, Ty::NEVER | Ty::POISON) {
            return Ok(());
        }
        for (key, want) in bindings {
            let proj = pool.intern_ty(&TyData::Assoc {
                assoc: key,
                trait_: def,
                self_ty: st,
                args,
            });
            let got = self.normalize_deep(proj)?;
            // Waiting on inference, or no impl (the bound itself reported it).
            if matches!(pool.get(got), TyData::Assoc { self_ty, .. }
                if !matches!(pool.get(self.strip_mut(self_ty)), TyData::Param(_)))
                || pool.has_poison(got)
            {
                continue;
            }
            let snap = self.infer.snapshot();
            if self.infer.unify(pool, got, want).is_err() {
                self.infer.rollback(snap);
                let msg = format!(
                    "{} does not satisfy {}: its `{}` is {}, not {}",
                    self.show(st),
                    self.cx.names.display_name(def),
                    self.assoc_name(key),
                    self.show(got),
                    self.show(want)
                );
                self.err(Code::UnsatisfiedTraitBound, n, &msg);
            }
        }
        Ok(())
    }

    /// The name of an associated item or binding key.
    fn assoc_name(&self, d: DefId) -> &str {
        self.cx
            .names
            .paths
            .segment(hd_base::PathId::from_raw(d.raw()))
    }

    /// The trait that declares the associated type `name`: `trait_` or
    /// one of its supertraits, with its arguments over `self_ty`, and the
    /// associated item (`trait.binding.name-reach`).
    fn assoc_decl(
        &self,
        trait_: DefId,
        self_ty: Ty,
        args: TyList,
        name: &str,
        depth: u32,
    ) -> Option<(DefId, TyList, DefId)> {
        let pool = self.pool();
        let Some(ItemData::Trait(t)) = self.cx.lookup.item(trait_).map(|i| &i.data) else {
            return None;
        };
        if let Some(&(_, d)) = t.assoc.iter().find(|(s, _)| self.cx.names.text(*s) == name) {
            return Some((trait_, args, d));
        }
        if depth > 16 {
            return None;
        }
        let known = pool.list_items(args);
        for s in &t.supers {
            let s = pool.subst(*s, &|p: ParamRef| {
                if p.owner != trait_ {
                    None
                } else if p.index == 0 {
                    Some(self_ty)
                } else {
                    known.get(p.index as usize - 1).copied()
                }
            });
            if let TyData::TraitValue { def, args, .. } = pool.get(s)
                && let Some(x) = self.assoc_decl(def, self_ty, args, name, depth + 1)
            {
                return Some(x);
            }
        }
        None
    }

    /// The arguments of `target` as `trait_[args]` or one of its
    /// transitive supertraits, `Self` replaced by `self_ty`: the first
    /// found depth first in declared order.
    fn super_args(
        &self,
        trait_: DefId,
        self_ty: Ty,
        args: TyList,
        target: DefId,
        depth: u32,
    ) -> Option<TyList> {
        if trait_ == target {
            return Some(args);
        }
        let pool = self.pool();
        let Some(ItemData::Trait(t)) = self.cx.lookup.item(trait_).map(|i| &i.data) else {
            return None;
        };
        if depth > 16 {
            return None;
        }
        let known = pool.list_items(args);
        for s in &t.supers {
            let s = pool.subst(*s, &|p: ParamRef| {
                if p.owner != trait_ {
                    None
                } else if p.index == 0 {
                    Some(self_ty)
                } else {
                    known.get(p.index as usize - 1).copied()
                }
            });
            if let TyData::TraitValue { def, args, .. } = pool.get(s)
                && let Some(x) = self.super_args(def, self_ty, args, target, depth + 1)
            {
                return Some(x);
            }
        }
        None
    }

    /// A projection's normal form, itself normalized once more; a cycle of
    /// bindings stops at the depth limit.
    fn normalized(&mut self, x: Ty) -> StageResult<Ty> {
        if self.norm_depth > 32 {
            return Ok(x);
        }
        self.norm_depth += 1;
        let r = self.normalize_deep(x);
        self.norm_depth -= 1;
        r
    }

    /// `t` with its projections normalized as far as inference allows;
    /// a type without projections is returned as is.
    pub(crate) fn norm_ty(&mut self, t: Ty) -> Ty {
        let pool = self.pool();
        let r = self.infer.resolve(pool, t);
        if !pool.has_assoc(r) {
            return t;
        }
        self.normalize_deep(r).unwrap_or(r)
    }

    /// Normalizes projections whose self type is known (trait-solver.md
    /// §4.3): a bound's binding, a trait value's binding, or an impl's
    /// binding. A projection on a parameter with no binding stays rigid,
    /// named by the trait that declares it.
    pub(crate) fn normalize(&mut self, t: Ty) -> StageResult<Ty> {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        let TyData::Assoc {
            assoc,
            trait_,
            self_ty,
            args,
        } = pool.get(t)
        else {
            return Ok(t);
        };
        let st = match pool.get(self_ty) {
            TyData::Mut(i) => i,
            _ => self_ty,
        };
        let name = self.assoc_name(assoc).to_owned();
        let (trait_, args, assoc) = self
            .assoc_decl(trait_, st, args, &name, 0)
            .unwrap_or((trait_, args, assoc));
        let bound = |bs: &[(DefId, Ty)], me: &Self| {
            bs.iter()
                .find(|(d, _)| me.assoc_name(*d) == name)
                .map(|(_, b)| *b)
        };
        match pool.get(st) {
            TyData::Param(_) => {
                // A binding on the parameter's bound or on a supertrait
                // clause it reaches (`trait.binding.inside`).
                let found = (0..self.env.clause_self.len())
                    .filter(|&i| self.env.clause_self[i] == st)
                    .find_map(|i| bound(&self.env.clause_bindings[i], self));
                if let Some(b) = found {
                    return self.normalized(b);
                }
                return Ok(pool.intern_ty(&TyData::Assoc {
                    assoc,
                    trait_,
                    self_ty: st,
                    args,
                }));
            }
            // `trait.dyn.bound.projection`: the trait value's binding.
            TyData::TraitValue { bindings, .. } => {
                return match bound(&bindings, self) {
                    Some(b) => self.normalized(b),
                    None => Ok(t),
                };
            }
            _ => {}
        }
        if pool.has_infer(st) {
            return Ok(t);
        }
        // `P::Error` under a bound `P < Walker[Self]` leaves the trait's
        // arguments implicit: the impl that matches supplies them.
        let n_trait = self.cx.lookup.item(trait_).map_or(0, |i| i.generics.len());
        let mut av = pool.list_items(args).to_vec();
        while av.len() < n_trait {
            av.push(self.infer.fresh(pool, VarKind::General));
        }
        let tref = TraitRef {
            trait_,
            self_ty: st,
            args: pool.list(&av),
        };
        if let hd_types::solver::Answer::Holds {
            evidence:
                Evidence::Impl {
                    row,
                    args: impl_args,
                },
            ..
        } = self.solve(tref)?
            && let Some(table) = self.cx.impl_table(row.module)
        {
            let r = row.row as usize;
            let impl_def = table.def[r];
            if let Some((_, b)) = table.assoc[r].iter().find(|(d, _)| *d == assoc) {
                let ia = pool.list_items(impl_args);
                let x = subst_owner(pool, impl_def, ia, *b);
                return self.normalized(x);
            }
        }
        // A trait's default for the associated type.
        if let Some(ItemData::AssocType {
            default: Some(d), ..
        }) = self.cx.lookup.item(assoc).map(|i| &i.data)
        {
            let x = pool.subst(*d, &|p: ParamRef| {
                (p.owner == trait_ && p.index == 0).then_some(st)
            });
            return self.normalized(x);
        }
        Ok(t)
    }
}

impl Ck<'_, '_> {
    /// Normalizes every projection inside a type.
    pub(crate) fn normalize_deep(&mut self, t: Ty) -> StageResult<Ty> {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        let list = |me: &mut Self, l: TyList| -> StageResult<TyList> {
            let mut v = Vec::new();
            for x in pool.list_items(l).iter().copied() {
                v.push(me.normalize_deep(x)?);
            }
            Ok(pool.list(&v))
        };
        let d = match pool.get(t) {
            TyData::Assoc {
                assoc,
                trait_,
                self_ty,
                args,
            } => {
                let s = self.normalize_deep(self_ty)?;
                let a = list(self, args)?;
                let x = pool.intern_ty(&TyData::Assoc {
                    assoc,
                    trait_,
                    self_ty: s,
                    args: a,
                });
                return self.normalize(x);
            }
            TyData::Adt { def, args } => TyData::Adt {
                def,
                args: list(self, args)?,
            },
            TyData::Tuple { elems, rest } => TyData::Tuple {
                elems: list(self, elems)?,
                rest,
            },
            TyData::Option(i) => TyData::Option(self.normalize_deep(i)?),
            TyData::Mut(i) => TyData::Mut(self.normalize_deep(i)?),
            TyData::Fn {
                params,
                result,
                row,
                suspends,
            } => TyData::Fn {
                params: list(self, params)?,
                result: self.normalize_deep(result)?,
                row,
                suspends,
            },
            _ => return Ok(t),
        };
        Ok(pool.intern_ty(&d))
    }
}

impl Ck<'_, '_> {
    /// A type with its outer `mut` view removed, variables resolved.
    pub(crate) fn strip_mut(&self, t: Ty) -> Ty {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        match pool.get(t) {
            TyData::Mut(i) => i,
            _ => t,
        }
    }

    /// The template's `T` when the body is a template's method and `tr`
    /// is `Structure` or the template's own trait
    /// (`annot.template.qualified-self`).
    pub(crate) fn template_self(&mut self, tr: DefId) -> Option<Ty> {
        let item = self.b.body_mut().item;
        let ItemData::Method { owner, .. } = &self.cx.lookup.item(item)?.data else {
            return None;
        };
        let ItemData::Impl {
            trait_,
            self_ty,
            kind: hd_resolve::ImplKind::Template | hd_resolve::ImplKind::TupleTemplate,
            ..
        } = &self.cx.lookup.item(*owner)?.data
        else {
            return None;
        };
        (tr == *trait_ || tr == self.cx.names.known.structure).then_some(*self_ty)
    }

    /// The callee choice a solver answer names (checking-and-tir.md,
    /// "callee record").
    pub(crate) fn choice_of(&self, ev: Option<&Evidence>) -> (ChoiceKind, u32) {
        match ev {
            Some(Evidence::Impl { row, .. }) => {
                let def = self
                    .cx
                    .impl_table(row.module)
                    .map_or(DefId::NONE, |t| t.def[row.row as usize]);
                (ChoiceKind::Impl, def.raw())
            }
            Some(Evidence::Bound { index, .. }) => (ChoiceKind::Bound, u32::from(*index)),
            Some(Evidence::TraitValue { trait_ }) => (ChoiceKind::TraitValue, trait_.raw()),
            _ => (ChoiceKind::Builtin, 0),
        }
    }
}

/// Substitutes one owner's parameters.
/// A callee with no final function parameter takes no trailing block
/// (`grammar.call.trailing-block.eligible`); no diagnostic code names that
/// yet (audit/compiler/diagnostic-notes.md), so it stays a stop.
/// The arguments of a form that takes neither a trailing block nor a
/// positional spread.
impl Ck<'_, '_> {
    /// Whether `def` is a test registration function: `it`, `it_each`,
    /// `it_prop` or `it_prop_with` (`module.testing.direct-call`).
    pub(crate) fn is_registration_fn(&self, def: DefId) -> bool {
        crate::tests::case_runner(self.cx.names.known, def).is_some()
            || self
                .cx
                .lookup
                .item(def)
                .and_then(|i| i.intrinsic)
                .is_some_and(|k| self.cx.names.text(k) == "test_case")
    }

    /// `module.testing.snapshot.literal`: a `snapshot` call's `expect`,
    /// named or second, is a string literal without interpolation.
    fn check_snapshot_expect(&mut self, args: &Args<'_>) {
        let expect = args
            .named
            .iter()
            .find(|(name, _)| name == "expect")
            .map(|(_, e)| *e)
            .or_else(|| args.positional.get(1).copied());
        if let Some(e) = expect
            && crate::expr::literal_text(&self.cx.src, e).is_none()
        {
            self.err(
                Code::NonLiteralTestArgument,
                e,
                "a snapshot's `expect` is a string literal",
            );
        }
    }

    /// `module.testing.reg.body-closure-result`: the body closure of a
    /// registration call that writes no result type returns `void`, or
    /// `Result[void, dyn Error]` when it uses `?`
    /// (`expr.try.test.with-try`, `expr.try.test.without-try`). The body
    /// is the final parameter, given by name, as a trailing block or last
    /// in position.
    fn registration_body_result(
        &mut self,
        params: &[(Symbol, Ty)],
        args: &Args<'_>,
    ) -> StageResult<()> {
        let Some((pname, pty)) = params.last() else {
            return Ok(());
        };
        let pname = self.cx.names.text(*pname).to_owned();
        let body = args
            .named
            .iter()
            .find(|(name, _)| *name == pname)
            .map(|(_, e)| *e)
            .or_else(|| args.positional.get(params.len() - 1).copied());
        let Some(body) = body.filter(|b| b.kind() == SyntaxKind::ClosureExpr) else {
            return Ok(());
        };
        let written = body
            .children()
            .any(|c| c.kind().is_type() && c.kind() != SyntaxKind::RequirementRow);
        if written {
            return Ok(());
        }
        let want = crate::tests::case_result(self.cx, crate::tests::has_try(body));
        let pool = self.pool();
        let pt = self.normalize_deep(*pty)?;
        if let TyData::Fn { result, .. } = pool.get(self.strip_mut(pt)) {
            let _ = self.infer.unify(pool, result, want);
        }
        Ok(())
    }
}

fn plain_args(args: &Args<'_>, what: &str) -> StageResult<()> {
    if args.trailing.is_some() {
        return unsupported(format!("a trailing block for {what}"));
    }
    if args.spread.is_some() {
        return unsupported(format!("a spread argument for {what}"));
    }
    Ok(())
}

pub(crate) fn subst_owner(pool: hd_types::Types<'_>, owner: DefId, args: &[Ty], t: Ty) -> Ty {
    pool.subst(t, &|p: ParamRef| {
        (p.owner == owner)
            .then(|| args.get(p.index as usize).copied())
            .flatten()
    })
}
