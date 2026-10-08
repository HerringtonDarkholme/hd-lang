//! Body checking (type-checking.md §1.4 to §1.6; checking-and-tir.md
//! §4.13): one function body over the full parser's tree, typed with an
//! `InferTable` in the run's `InternPool`, trait goals through the
//! `Solver` with fuel, TIR written through `TirBuilder`. User errors go to
//! the `DiagBuf`; a construct the checker does not carry yet is a
//! structured "not implemented", which stops a build.
//!
//! Expressions are in `expr`, calls and member lookup in `call`, patterns
//! and `match` in `pat`, annotations in `ty`.

use std::collections::HashMap;

use hd_base::{DefId, Fuel, LocalId, ModuleId, NotImplemented, Stage, StageResult, Symbol};
use hd_diag::{Code, DiagBuf};
use hd_resolve::{ItemData, Lookup, ModuleScope, Names, Src};
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_tir::Body;
use hd_tir::ir::{
    BodyKind, Callee, Coercion, LoopMark, NONE, Ref, SubMark, Tag, TirBuilder, TirSink, local_flags,
};
use hd_types::solver::{
    Answer, BodyMemo, GlobalMemo, Goal, ImplTable, ImplUniverseId, ParamEnv, SolveCx, Solver,
    TraitRef,
};
use hd_types::{InferTable, ParamRef, RowId, Ty, TyData, TyList, VarKind};

/// What a body sees: the run's tables, its module's scope, the items of
/// its closure and the impl tables of its impl universe.
pub struct BodyCx<'a> {
    pub names: Names<'a>,
    pub src: Src<'a>,
    pub scope: &'a ModuleScope,
    pub lookup: &'a Lookup<'a>,
    pub impls: &'a [(ModuleId, &'a ImplTable)],
    pub universe: ImplUniverseId,
    pub global: &'a GlobalMemo,
    pub solver: &'a dyn Solver,
    /// The methods of the closure by name, built on first use.
    pub methods: std::cell::OnceCell<crate::MethodIndex>,
    /// The module's top-level bindings and its bodies' init facts.
    pub init: std::cell::RefCell<crate::init::ModuleInit>,
}

pub(crate) fn unsupported<T>(what: impl Into<String>) -> StageResult<T> {
    Err(NotImplemented::new(Stage::Body, what))
}

/// An open closure: its builder mark, the scope depth at its start, its
/// result type.
/// One level of the available-keys stack (type-checking.md §5.2).
pub(crate) enum RowFrame {
    /// A function's declared row: the bottom of a body's stack.
    Declared(RowId),
    /// The entry module's top level: every key is available.
    Any,
    /// A closure with a written row, or (`written: None`) the keys its
    /// body uses so far, which become its inferred row
    /// (`req.row.omitted.closure-row`).
    Closure {
        written: Option<RowId>,
        used: hd_types::RowData,
    },
    /// The keys a `$.with` block binds.
    With(RowId),
}

pub(crate) struct OpenSub {
    pub mark: SubMark,
    pub depth: usize,
}

pub(crate) struct Ck<'a, 'c> {
    pub cx: &'c BodyCx<'a>,
    pub b: TirBuilder,
    pub infer: InferTable,
    pub scopes: Vec<HashMap<Symbol, LocalId>>,
    pub env: ParamEnv,
    /// Open loops: label and the type of their `break` values.
    pub loops: Vec<(LoopMark, Option<Ty>)>,
    pub diags: &'c mut DiagBuf,
    pub fuel: Fuel,
    pub memo: BodyMemo,
    /// The result type of the body or closure being checked.
    pub rets: Vec<Ty>,
    /// Type parameters by name: the impl's, then the function's.
    pub gens: Vec<(Symbol, Ty)>,
    pub self_ty: Option<Ty>,
    /// The available keys: the body's row, then closures and `$.with`
    /// blocks, innermost last.
    pub rows: Vec<RowFrame>,
    /// Row parameters by name: the impl's, then the function's.
    pub row_gens: Vec<(Symbol, hd_types::RowParamRef)>,
    pub subs: Vec<OpenSub>,
    /// Bounds on types not yet known at their use: solved at the end.
    pub pending: Vec<(TraitRef, NodeRefIdx)>,
    /// A member call's explicit method type arguments (`x.f::[T]()`),
    /// taken by the method's instantiation.
    pub method_targs: Vec<Ty>,
    /// The next call's default bodies: callee, type arguments and the
    /// values before the first argument (a receiver).
    pub default_owner: Option<(DefId, TyList, Vec<Ref>)>,
    /// Whether the body or closure being checked suspends (a driver
    /// context for bang calls).
    pub suspends: Vec<bool>,
    /// Inside a `defer` suite: the loop depth at its start.
    pub defer_base: Option<usize>,
    /// The node of each literal constant, for range diagnostics.
    pub lit_nodes: Vec<(Ref, hd_base::NodeIdx)>,
    /// The literal node being checked as the argument of its literal
    /// function (`expr.literal-fn.ordinary-call`).
    pub lit_arg: Option<hd_base::NodeIdx>,
    /// Checking a module's top-level statements: the module path.
    pub module_init: Option<String>,
    /// The top-level statement being checked.
    pub init_stmt: usize,
    /// What this body reads and calls (module initialization).
    pub facts: crate::init::InitFacts,
    /// A pipe's value while its step is checked (`_`).
    pub placeholder: Option<(Ref, Ty)>,
    /// The next `for` loop's `else` suite and the loop's result type.
    pub loop_else: Option<(NodeRefIdx, Ty)>,
    /// The function body block, for `missing-return-value`.
    pub fn_body: Option<NodeRefIdx>,
    /// Declaring a `let` pattern's names: whether it has an annotation.
    pub let_view: Option<bool>,
    /// The locals the source binds by name, for `unused-local-binding`.
    pub user_locals: Vec<LocalId>,
    /// Nested projection normalizations, against binding cycles.
    pub norm_depth: u32,
}

/// A node index kept for a later diagnostic.
pub(crate) type NodeRefIdx = hd_base::NodeIdx;

/// The checker of one body: the item whose parameters and bounds are in
/// scope (`env`), the TIR item and kind, its result and row.
pub(crate) fn new_ck<'a, 'c>(
    cx: &'c BodyCx<'a>,
    env: DefId,
    item: DefId,
    kind: BodyKind,
    (ret, row): (Ty, RowId),
    diags: &'c mut DiagBuf,
) -> Ck<'a, 'c> {
    let pool = cx.names.pool;
    let mut ck = Ck {
        cx,
        b: TirBuilder::new(item, kind),
        infer: InferTable::default(),
        scopes: vec![HashMap::new()],
        env: ParamEnv::default(),
        loops: Vec::new(),
        diags,
        fuel: Fuel::new(Fuel::BODY_DEFAULT),
        memo: BodyMemo::default(),
        rets: vec![ret],
        gens: Vec::new(),
        self_ty: None,
        rows: vec![
            if kind == BodyKind::Init && !pool.row_data(row).params.is_empty() {
                RowFrame::Any
            } else {
                RowFrame::Declared(row)
            },
        ],
        row_gens: Vec::new(),
        subs: Vec::new(),
        pending: Vec::new(),
        method_targs: Vec::new(),
        default_owner: None,
        suspends: vec![false],
        defer_base: None,
        lit_nodes: Vec::new(),
        lit_arg: None,
        module_init: None,
        init_stmt: 0,
        facts: crate::init::InitFacts::default(),
        placeholder: None,
        loop_else: None,
        fn_body: None,
        let_view: None,
        user_locals: Vec::new(),
        norm_depth: 0,
    };
    let Some(it) = cx.lookup.item(env) else {
        return ck;
    };
    // The owner's parameters and bounds: an impl's, or a trait's `Self`.
    if let ItemData::Method { owner, .. } = &it.data
        && let Some(o) = cx.lookup.item(*owner)
    {
        match &o.data {
            ItemData::Impl { self_ty, kind, .. } => {
                ck.self_ty = Some(*self_ty);
                // A `by Structure` template's target has the structure
                // protocol by construction (annotations, "Templates").
                if matches!(
                    kind,
                    hd_resolve::ImplKind::Template | hd_resolve::ImplKind::TupleTemplate
                ) {
                    let st = cx.names.item("std.structure", "Structure");
                    let tv = pool.intern_ty(&TyData::TraitValue {
                        def: st,
                        args: TyList::EMPTY,
                        bindings: vec![],
                    });
                    ck.add_bound(*self_ty, tv, 0);
                }
                ck.add_generics(*owner, &o.generics);
            }
            ItemData::Trait(_) => {
                let p = pool.intern_ty(&TyData::Param(ParamRef {
                    owner: *owner,
                    index: 0,
                }));
                ck.self_ty = Some(p);
                let tv = pool.intern_ty(&TyData::TraitValue {
                    def: *owner,
                    args: TyList::EMPTY,
                    bindings: vec![],
                });
                ck.add_bound(p, tv, 0);
            }
            _ => {}
        }
    }
    let gs = match it.sig() {
        Some(sig) => sig.generics.clone(),
        None => it.generics.clone(),
    };
    ck.add_generics(env, &gs);
    ck
}

/// Checks one function body and returns its TIR.
pub fn check_fn(
    cx: &BodyCx<'_>,
    def: DefId,
    node: NodeRef<'_>,
    diags: &mut DiagBuf,
) -> StageResult<Body> {
    let Some(item) = cx.lookup.item(def) else {
        return unsupported(format!("body of {} has no header", cx.names.path(def)));
    };
    let Some(sig) = item.sig().cloned() else {
        return unsupported("a body of a non-function item");
    };
    let mut ck = new_ck(cx, def, def, BodyKind::Fn, (sig.ret, sig.row), diags);
    ck.check_impl_method(def, node);
    ck.check_literal_marker(def, node);
    ck.check_row_patterns(&sig, node);
    ck.suspends = vec![sig.suspends];
    let blk = ck.b.open_block();
    // Projections in the signature normalize under the bounds, so
    // `I::Item` and `T` are one type under `I < Supplier[Item = T]`
    // (`trait.binding.interchangeable`).
    for (name, ty) in sig.params.clone() {
        let ty = ck.norm_ty(ty);
        let l = ck.b.local(ty, name, local_flags::PARAM, node.index());
        ck.scopes[0].insert(name, l);
    }
    let Some(body) = Src::child(node, SyntaxKind::Block) else {
        return unsupported("a function without a body");
    };
    let ret = ck.norm_ty(sig.ret);
    ck.rets[0] = ret;
    ck.fn_body = Some(body.index());
    let (tail, _) = ck.block_value(body, Some(ret))?;
    let root = ck.b.close_block(blk, tail, ret, body.index());
    ck.finish(root)
}

/// The `DefId` of a default body: the declaration's path plus the field or
/// parameter name (checking-and-tir.md "Default calls").
#[must_use]
pub fn default_body_def(names: &Names<'_>, owner: DefId, name: &str) -> DefId {
    names.member(owner, hd_intern::PathKind::Hidden, name)
}

/// Checks one default expression as a body of kind `Default`
/// (type-checking.md §1.7): a data field's (no parameters) or a function
/// parameter's (the earlier parameters are its parameters).
pub fn check_default(
    cx: &BodyCx<'_>,
    owner: DefId,
    name: &str,
    expr: NodeRef<'_>,
    diags: &mut DiagBuf,
) -> StageResult<Body> {
    let Some(item) = cx.lookup.item(owner) else {
        return unsupported("a default of an item outside the module");
    };
    let sym = cx.names.syms.intern(name);
    let (ty, params) = match (&item.data, item.sig()) {
        (ItemData::Data(fields), _) => match fields.iter().find(|f| f.name == sym) {
            Some(f) => (f.ty, vec![]),
            None => return unsupported("a default of an unknown field"),
        },
        (_, Some(sig)) => match sig.params.iter().position(|p| p.0 == sym) {
            Some(i) => (sig.params[i].1, sig.params[..i].to_vec()),
            None => return unsupported("a default of an unknown parameter"),
        },
        _ => return unsupported("a default of this item kind"),
    };
    let def = default_body_def(&cx.names, owner, name);
    let mut ck = new_ck(cx, owner, def, BodyKind::Default, (ty, RowId::EMPTY), diags);
    let blk = ck.b.open_block();
    for (n, t) in params {
        let l = ck.b.local(t, n, local_flags::PARAM, expr.index());
        ck.scopes[0].insert(n, l);
    }
    let (r, t) = ck.expr(expr, Some(ty))?;
    let r = ck.coerce(r, t, ty, expr, "default");
    let root = ck.b.close_block(blk, Some(r), ty, expr.index());
    ck.finish(root)
}

impl Ck<'_, '_> {
    pub(crate) fn pool(&self) -> &hd_types::InternPool {
        self.cx.names.pool
    }

    pub(crate) fn charge(&mut self) -> StageResult<()> {
        if self.fuel.charge(1) {
            Ok(())
        } else {
            unsupported("the body's fuel ran out (limit diagnostic)")
        }
    }

    /// A construct not carried yet, with where it is (after ` @`).
    pub(crate) fn gap<T>(&self, n: NodeRef<'_>, what: &str) -> StageResult<T> {
        let span = self.cx.src.span(n);
        unsupported(format!("{what} @{}", span.lo))
    }

    pub(crate) fn err(&mut self, code: Code, n: NodeRef<'_>, msg: &str) {
        let span = self.cx.src.span(n);
        self.diags.error(code, span, msg);
    }

    pub(crate) fn show(&self, t: Ty) -> String {
        let t = self.infer.resolve(self.pool(), t);
        match self.pool().get(t) {
            TyData::Infer(_) => "{unknown}".into(),
            _ => hd_resolve::show_ty(&self.cx.names, t),
        }
    }

    /// `unused-local-binding` (flow.unused.warning): a local the source
    /// binds and never reads; a name beginning with `_` is exempt.
    fn unused_locals(&mut self) {
        // One warning per line: a `let` pattern's unread names share it.
        let mut lines = std::collections::HashSet::new();
        for l in std::mem::take(&mut self.user_locals) {
            let body = self.b.body_mut();
            let (flags, name, at) = (
                body.local_flags[l.idx()],
                body.local_name[l.idx()],
                body.local_syn[l.idx()],
            );
            let text = self.cx.names.text(name);
            if flags & local_flags::READ != 0 || text.starts_with('_') {
                continue;
            }
            let msg = format!("`{text}` is never read; name it `_{text}` to keep it");
            let span = self.cx.src.span(self.cx.src.parse.tree.node(at));
            let source = self.cx.src.text;
            let lo = usize::try_from(span.lo).unwrap_or(0).min(source.len());
            if !lines.insert(source[..lo].rfind('\n')) {
                continue;
            }
            self.diags.push(
                Code::UnusedLocalBinding,
                hd_diag::Severity::Warning,
                span,
                &msg,
                None,
            );
        }
    }

    /// `integer-literal-range`: an integer constant must fit its type.
    fn check_literal_ranges(&mut self) {
        use hd_types::Prim;
        let pool = self.cx.names.pool;
        for (r, at) in std::mem::take(&mut self.lit_nodes) {
            let Some((t, bits)) = self.b.const_of(r) else {
                continue;
            };
            let TyData::Prim(p) = pool.get(t) else {
                continue;
            };
            let node = self.cx.src.parse.tree.node(at);
            // A literal under unary `-` was negated by the checker: `bits`
            // holds the negated value (`types.literal.negation`).
            let negated = node.kind() == SyntaxKind::UnaryExpr;
            let v = bits.cast_signed();
            if negated && p.is_unsigned() {
                let msg = format!("a negated literal does not fit unsigned {}", p.name());
                self.err(Code::TypeMismatch, node, &msg);
                continue;
            }
            let ok = match p {
                Prim::I8 => i8::try_from(v).is_ok(),
                Prim::I16 => i16::try_from(v).is_ok(),
                Prim::I32 => i32::try_from(v).is_ok(),
                // A negated `i64` literal keeps its sign; a plain one
                // above `i64::MAX` wrapped into the negative range.
                Prim::I64 => {
                    if negated {
                        v <= 0
                    } else {
                        v >= 0
                    }
                }
                Prim::U8 => u8::try_from(bits).is_ok(),
                Prim::U16 => u16::try_from(bits).is_ok(),
                Prim::U32 | Prim::Usize => u32::try_from(bits).is_ok(),
                _ => true,
            };
            if !ok {
                let msg = format!("literal {v} does not fit {}", p.name());
                self.err(Code::IntegerLiteralRange, node, &msg);
            }
        }
    }

    /// Declares an item's type parameters with their bounds.
    fn add_generics(&mut self, owner: DefId, gs: &[hd_resolve::Generic]) {
        let pool = self.cx.names.pool;
        for (i, g) in gs.iter().enumerate() {
            let p = pool.intern_ty(&TyData::Param(ParamRef {
                owner,
                index: u16::try_from(i).unwrap_or(u16::MAX),
            }));
            self.gens.push((g.name, p));
            if g.row {
                self.row_gens.push((
                    g.name,
                    hd_types::RowParamRef {
                        owner,
                        index: u16::try_from(i).unwrap_or(u16::MAX),
                    },
                ));
            }
            let first = self.env.clause_self.len();
            for b in &g.bounds {
                self.add_bound(p, *b, 0);
            }
            // `T < mut Trait`: values of `T` have mutable access.
            if g.mut_bound {
                for m in &mut self.env.clause_mut[first..] {
                    *m = true;
                }
            }
        }
    }

    /// Adds a bound and, transitively, its supertraits to the environment.
    pub(crate) fn add_bound(&mut self, self_ty: Ty, bound: Ty, depth: u32) {
        let pool = self.cx.names.pool;
        let TyData::TraitValue {
            def,
            args,
            bindings,
        } = pool.get(bound)
        else {
            return;
        };
        // A defaulted trait argument left out (`Add[Out = Self]` is
        // `Add[Self, Out = Self]`), so the clause matches its goals.
        let args = match self.cx.lookup.item(def) {
            Some(it) if it.generics.len() > pool.list_items(args).len() => {
                let mut v = pool.list_items(args).to_vec();
                for g in it.generics.iter().skip(v.len()) {
                    let Some(d) = g.default else { break };
                    let known = v.clone();
                    v.push(pool.subst(d, &|p: ParamRef| {
                        if p.owner != def {
                            None
                        } else if p.index == 0 {
                            Some(self_ty)
                        } else {
                            known.get(p.index as usize - 1).copied()
                        }
                    }));
                }
                pool.list(&v)
            }
            _ => args,
        };
        if depth > 16
            || (0..self.env.clause_self.len()).any(|i| {
                self.env.clause_self[i] == self_ty
                    && self.env.clause_trait[i] == def
                    && self.env.clause_args[i] == args
            })
        {
            return;
        }
        self.env.clause_self.push(self_ty);
        self.env.clause_trait.push(def);
        self.env.clause_args.push(args);
        self.env.clause_bindings.push(bindings);
        self.env.clause_mut.push(false);
        self.env
            .clause_origin
            .push(u16::try_from(self.env.clause_origin.len()).unwrap_or(u16::MAX));
        if let Some(ItemData::Trait(t)) = self.cx.lookup.item(def).map(|i| &i.data) {
            let known = pool.list_items(args);
            for s in t.supers.clone() {
                let s = pool.subst(s, &|p: ParamRef| {
                    if p.owner != def {
                        return None;
                    }
                    if p.index == 0 {
                        Some(self_ty)
                    } else {
                        known.get(p.index as usize - 1).copied()
                    }
                });
                self.add_bound(self_ty, s, depth + 1);
            }
        }
    }

    /// A local by name, innermost first, with its scope depth.
    pub(crate) fn find_local(&self, s: Symbol) -> Option<(LocalId, usize)> {
        self.scopes
            .iter()
            .enumerate()
            .rev()
            .find_map(|(d, m)| m.get(&s).map(|l| (*l, d)))
    }

    /// Reads a local, capturing it into every open closure it crosses.
    pub(crate) fn read_local(&mut self, l: LocalId, depth: usize, n: NodeRef<'_>) -> (Ref, Ty) {
        let mut captured = false;
        for s in &self.subs {
            if s.depth > depth {
                self.b.capture(s.mark, l);
                captured = true;
            }
        }
        let flags = &mut self.b.body_mut().local_flags[l.idx()];
        *flags |= local_flags::READ;
        if captured {
            *flags |= local_flags::CAPTURED;
        }
        let t = self.b.local_ty(l);
        (self.b.get(l, t, n.index()), t)
    }

    /// Records an assignment to a local, from inside a closure or not.
    pub(crate) fn note_write(&mut self, l: LocalId, depth: usize) {
        let mut captured = false;
        for s in &self.subs {
            if s.depth > depth {
                self.b.capture(s.mark, l);
                captured = true;
            }
        }
        let flags = &mut self.b.body_mut().local_flags[l.idx()];
        if captured {
            *flags |= local_flags::CAPTURED | local_flags::CAPTURED_ASSIGNED;
        } else if *flags & local_flags::ASSIGNED != 0 {
            *flags |= local_flags::MUTATED;
        }
        *flags |= local_flags::ASSIGNED;
    }

    pub(crate) fn sym_of(&self, n: NodeRef<'_>) -> Symbol {
        self.cx
            .names
            .syms
            .intern(self.cx.src.text(self.cx.src.first(n)))
    }

    pub(crate) fn bind_local(&mut self, name: Symbol, t: Ty, at: NodeRef<'_>) -> LocalId {
        // `names.type-param.no-redeclare`: a local value must not reuse an
        // enclosing type parameter's name.
        if self.gens.iter().any(|(g, _)| *g == name) {
            let msg = format!(
                "`{}` is a type parameter in scope",
                self.cx.names.text(name)
            );
            self.err(Code::DuplicateBinding, at, &msg);
        }
        let l = self.b.local(t, name, local_flags::ASSIGNED, at.index());
        self.scopes.last_mut().expect("scope").insert(name, l);
        self.user_locals.push(l);
        l
    }

    /// Unifies `got` with `want`, inserting the coercions of
    /// type-checking.md §4.2 (`never`, `.Some` wrapping, trait values).
    pub(crate) fn coerce(&mut self, r: Ref, got: Ty, want: Ty, n: NodeRef<'_>, what: &str) -> Ref {
        let pool = self.cx.names.pool;
        let (got, want) = (self.norm_ty(got), self.norm_ty(want));
        let g = self.infer.shallow(pool, got);
        let w = self.infer.shallow(pool, want);
        if g == Ty::NEVER || w == Ty::NEVER || g == w {
            return r;
        }
        // A join variable (a branch or element already seen) is not a declared
        // type: the least common type weakens instead (types.lct).
        let joined = matches!(pool.get(want), TyData::Infer(_));
        if !joined && self.check_upgrade(got, want, n, what) {
            return r;
        }
        let strip = |t: Ty| match pool.get(t) {
            TyData::Mut(i) => i,
            _ => t,
        };
        let (gs, ws) = (strip(g), strip(w));
        if let TyData::Option(inner) = pool.get(ws)
            && !matches!(pool.get(gs), TyData::Option(_) | TyData::Infer(_))
        {
            if !self.check_upgrade(got, inner, n, what) {
                self.expect(got, inner, n, what);
            }
            return self.b.coerce(Coercion::WrapSome, NONE, r, want, n.index());
        }
        if let (TyData::TraitValue { def: to, .. }, TyData::TraitValue { def: from, .. }) =
            (pool.get(ws), pool.get(gs))
            && to != from
            && self.trait_extends(from, to, 0)
        {
            return self
                .b
                .coerce(Coercion::Supertrait, NONE, r, want, n.index());
        }
        if let TyData::TraitValue { def, args, .. } = pool.get(ws)
            && !matches!(
                pool.get(gs),
                TyData::TraitValue { .. } | TyData::Infer(_) | TyData::Poison
            )
        {
            let tref = TraitRef {
                trait_: def,
                self_ty: gs,
                args,
            };
            let _ = self.require_ref(tref, n);
            return self
                .b
                .coerce(Coercion::ToTraitValue, NONE, r, want, n.index());
        }
        let before = self.diags.len();
        self.expect(got, want, n, what);
        if self.diags.len() == before && !joined && !self.perm_fits(got, want, true, 0) {
            let msg = format!(
                "in {what}: expected {}, found {}; permissions inside a type convert only by declared variance",
                self.show(want),
                self.show(got)
            );
            self.err(Code::TypeMismatch, n, &msg);
        }
        // A function value fits a wider row (`req.row.subsume`); an open
        // row variable takes the least solution (`req.row.least`).
        if self.diags.len() == before
            && !joined
            && let (TyData::Fn { row: gr, .. }, TyData::Fn { row: wr, .. }) = (
                pool.get(self.infer.resolve(pool, gs)),
                pool.get(self.infer.resolve(pool, ws)),
            )
        {
            self.fit_row(gr, wr, n, what);
        }
        r
    }

    pub(crate) fn expect(&mut self, got: Ty, want: Ty, n: NodeRef<'_>, what: &str) {
        let pool = self.cx.names.pool;
        if got == Ty::NEVER || want == Ty::NEVER {
            return;
        }
        let (got, want) = (self.norm_ty(got), self.norm_ty(want));
        let snap = self.infer.snapshot();
        if self.infer.unify(pool, got, want).is_err() {
            self.infer.rollback(snap);
            // `types.num.narrowing`: a value of a wider type of the same
            // numeric family where a narrower one is expected.
            if !matches!(what, "operand" | "range bound") && self.narrows(got, want) {
                let msg = format!(
                    "converting to a narrower type in {what}: expected {}, found {}; write the conversion",
                    self.show(want),
                    self.show(got)
                );
                self.err(Code::ImplicitNarrowing, n, &msg);
                return;
            }
            let msg = format!(
                "in {what}: expected {}, found {}",
                self.show(want),
                self.show(got)
            );
            self.err(Code::TypeMismatch, n, &msg);
        }
    }

    /// Whether `want` is a narrower numeric type than `got` in the same
    /// family (`types.num.families`); `usize` is 32 bits wide.
    fn narrows(&self, got: Ty, want: Ty) -> bool {
        let pool = self.cx.names.pool;
        let prim = |t: Ty| {
            let t = self.strip_mut(self.infer.resolve(pool, t));
            match pool.get(t) {
                TyData::Prim(p) => Some(p),
                _ => None,
            }
        };
        let (Some(g), Some(w)) = (prim(got), prim(want)) else {
            return false;
        };
        let family = |p: hd_types::Prim| (p.is_float(), p.is_unsigned());
        let width = |p: hd_types::Prim| match p {
            hd_types::Prim::I8 | hd_types::Prim::U8 => 8,
            hd_types::Prim::I16 | hd_types::Prim::U16 => 16,
            hd_types::Prim::I32
            | hd_types::Prim::U32
            | hd_types::Prim::Usize
            | hd_types::Prim::F32 => 32,
            _ => 64,
        };
        (g.is_integer() || g.is_float())
            && (w.is_integer() || w.is_float())
            && family(g) == family(w)
            && width(w) < width(g)
    }

    /// Whether two types can unify, with no lasting effect.
    pub(crate) fn can_unify(&mut self, a: Ty, b: Ty) -> bool {
        let (a, b) = (self.norm_ty(a), self.norm_ty(b));
        let snap = self.infer.snapshot();
        let ok = self.infer.unify(self.cx.names.pool, a, b).is_ok();
        self.infer.rollback(snap);
        ok
    }

    /// Solves `tref` now, learning what the answer fixes; `Ok(None)` when
    /// it waits for inference.
    pub(crate) fn require_ref(
        &mut self,
        tref: TraitRef,
        at: NodeRef<'_>,
    ) -> StageResult<Option<hd_types::solver::Evidence>> {
        let pool = self.cx.names.pool;
        // Projections with a known base normalize before the goal is
        // solved (trait-solver.md §4.3, point 3).
        let tref = TraitRef {
            trait_: tref.trait_,
            self_ty: self.norm_ty(self.infer.resolve(pool, tref.self_ty)),
            args: pool.list(
                &pool
                    .list_items(tref.args)
                    .iter()
                    .copied()
                    .map(|a| {
                        let a = self.infer.resolve(pool, a);
                        self.norm_ty(a)
                    })
                    .collect::<Vec<_>>(),
            ),
        };
        if matches!(self.strip_mut(tref.self_ty), Ty::NEVER) {
            return Ok(Some(hd_types::solver::Evidence::Poison));
        }
        if let Some(b) = self.builtin_holds(tref) {
            return Ok(Some(b));
        }
        match self.solve(tref)? {
            Answer::Holds { evidence, learned } => {
                for (v, t) in learned {
                    let var = pool.intern_ty(&TyData::Infer(v));
                    let _ = self.infer.unify(pool, var, t);
                }
                Ok(Some(evidence))
            }
            Answer::Fails(_) => {
                let msg = format!(
                    "{} does not implement {}",
                    self.show(tref.self_ty),
                    self.cx.names.path(tref.trait_)
                );
                self.err(Code::UnsatisfiedTraitBound, at, &msg);
                Ok(None)
            }
            Answer::Stalled { .. } => {
                self.pending.push((tref, at.index()));
                Ok(None)
            }
            Answer::OutOfFuel => unsupported("the solver's fuel ran out (limit diagnostic)"),
            other => unsupported(format!("solver answer {other:?}")),
        }
    }

    pub(crate) fn solve(&mut self, tref: TraitRef) -> StageResult<Answer> {
        let goal = Goal::Implements {
            tref,
            bindings: vec![],
            mut_: false,
        };
        let mut scx = SolveCx {
            pool: self.cx.names.pool,
            env: &self.env,
            universe: self.cx.universe,
            tables: self.cx.impls,
            body_memo: &mut self.memo,
            global: self.cx.global,
        };
        self.cx.solver.solve(&mut scx, &goal, &mut self.fuel)
    }

    /// Compiler-answered traits (trait-solver.md §3.8): `Any` and its
    /// children, `Tuple`, and the numeric families on primitives.
    pub(crate) fn builtin_holds(&self, tref: TraitRef) -> Option<hd_types::solver::Evidence> {
        use hd_types::solver::{BuiltinImpl, Evidence};
        let pool = self.cx.names.pool;
        let path = self.cx.names.path(tref.trait_);
        let t = match pool.get(tref.self_ty) {
            TyData::Mut(i) => i,
            _ => tref.self_ty,
        };
        // A trait value implements its trait and supertraits.
        if let TyData::TraitValue { def, .. } = pool.get(t)
            && (def == tref.trait_ || self.trait_extends(def, tref.trait_, 0))
        {
            return Some(Evidence::TraitValue { trait_: def });
        }
        let prim = match pool.get(t) {
            TyData::Prim(p) => Some(p),
            _ => None,
        };
        let b = match path.as_str() {
            "std/core/Any" => BuiltinImpl::Any,
            // Every value type is in exactly one sealed category
            // (types.sealed.exactly-one); a parameter's comes from its bound.
            "std/core/AnyVal" if self.sealed_fits(t, false) => BuiltinImpl::AnyVal,
            "std/core/AnyRef" if self.sealed_fits(t, true) => BuiltinImpl::AnyRef,
            "std/function/Tuple" if matches!(pool.get(t), TyData::Tuple { .. }) => {
                BuiltinImpl::Tuple
            }
            "std/num/Num" if prim.is_some_and(|p| p.is_integer() || p.is_float()) => {
                BuiltinImpl::Num
            }
            "std/num/Integer" if prim.is_some_and(hd_types::Prim::is_integer) => {
                BuiltinImpl::Integer
            }
            "std/num/Float" if prim.is_some_and(hd_types::Prim::is_float) => BuiltinImpl::Float,
            "std/inspect/Inspectable" if self.inspectable(t, false, 0) => BuiltinImpl::Inspectable,
            _ => return None,
        };
        Some(Evidence::Builtin(b))
    }

    /// The inspectable types (spec/lang/09-traits.md#inspectable-types);
    /// `arg` admits what counts only as a type argument.
    pub(crate) fn inspectable(&self, t: Ty, arg: bool, depth: u32) -> bool {
        let pool = self.cx.names.pool;
        if depth > 32 {
            return false;
        }
        let t = self.infer.resolve(pool, t);
        let inspect = self.cx.names.item("std.inspect", "Inspectable");
        match pool.get(t) {
            TyData::Prim(p) => p != hd_types::Prim::Void || arg,
            TyData::Never | TyData::Poison => true,
            TyData::Mut(i) | TyData::Option(i) => self.inspectable(i, true, depth + 1),
            TyData::Tuple { elems, .. } => pool
                .list_items(elems)
                .iter()
                .copied()
                .all(|e| self.inspectable(e, false, depth + 1)),
            TyData::Adt { args, .. } => pool
                .list_items(args)
                .iter()
                .copied()
                .all(|a| self.inspectable(a, true, depth + 1)),
            TyData::TraitValue { def, .. } => {
                arg || def == inspect || self.trait_extends(def, inspect, 0)
            }
            TyData::Param(_) => (0..self.env.clause_self.len())
                .any(|i| self.env.clause_self[i] == t && self.env.clause_trait[i] == inspect),
            _ => false,
        }
    }

    // ------------------------------------------------------------ blocks

    /// A block's statements; with a wanted non-void type, the last
    /// expression statement is its value. Returns the tail and its type.
    pub(crate) fn block_value(
        &mut self,
        block: NodeRef<'_>,
        want: Option<Ty>,
    ) -> StageResult<(Option<Ref>, Ty)> {
        self.scopes.push(HashMap::new());
        // A suite with `defer` is a cleanup scope (`flow.defer.scopes`):
        // a `Scope` over its statements, whose suites are its `Defer`s.
        let r = if block.children().any(|c| c.kind() == SyntaxKind::DeferStmt) {
            let sm = self.b.open_scope();
            let inner = self.b.open_block();
            match self.lines(block, want) {
                Ok((tail, ty)) => {
                    let body = self.b.close_block(inner, tail, ty, block.index());
                    let scope = self.b.close_scope(sm, body, ty, block.index());
                    Ok((if tail.is_some() { Some(scope) } else { None }, ty))
                }
                Err(e) => Err(e),
            }
        } else {
            self.lines(block, want)
        };
        self.scopes.pop();
        r
    }

    fn lines(&mut self, block: NodeRef<'_>, want: Option<Ty>) -> StageResult<(Option<Ref>, Ty)> {
        let stmts: Vec<NodeRef<'_>> = block
            .children()
            .filter(|s| s.kind() != SyntaxKind::Error)
            .collect();
        let value = want != Some(Ty::VOID);
        let mut ty = Ty::VOID;
        let mut tail = None;
        let mut diverged = false;
        for (i, s) in stmts.iter().copied().enumerate() {
            let last = i + 1 == stmts.len();
            if last && value && s.kind() == SyntaxKind::ExprStmt {
                let Some(e) = s.children().next() else {
                    return unsupported("an empty expression statement");
                };
                let (r, t) = self.expr(e, want)?;
                // A last expression that cannot complete is a statement.
                if self.infer.shallow(self.cx.names.pool, t) == Ty::NEVER {
                    diverged = true;
                    continue;
                }
                // `fn.body.value-less-fallthrough`: a body that ends in a
                // value-less statement such as an `if` without `else`.
                if let Some(w) = want
                    && self.fn_body == Some(block.index())
                    && self.infer.shallow(self.cx.names.pool, t) == Ty::VOID
                    && !matches!(
                        self.pool().get(self.infer.shallow(self.cx.names.pool, w)),
                        TyData::Infer(_) | TyData::Prim(hd_types::Prim::Void)
                    )
                {
                    let msg = format!("this path ends without a {} value", self.show(w));
                    self.err(Code::MissingReturnValue, s, &msg);
                    diverged = true;
                    continue;
                }
                let r = match want {
                    Some(w) => self.coerce(r, t, w, e, "result"),
                    None => r,
                };
                tail = Some(r);
                ty = want.unwrap_or(t);
                continue;
            }
            let t = self.stmt(s)?;
            diverged |= t == Ty::NEVER;
            if last && diverged {
                ty = Ty::NEVER;
            }
        }
        if value
            && tail.is_none()
            && !diverged
            && let Some(w) = want
        {
            let w = self.infer.shallow(self.pool(), w);
            if w != Ty::VOID && w != Ty::NEVER {
                let mut snap_ok = false;
                if matches!(self.pool().get(w), TyData::Infer(_)) {
                    snap_ok = self.infer.unify(self.cx.names.pool, w, Ty::VOID).is_ok();
                }
                if !snap_ok && self.fn_body == Some(block.index()) {
                    // `fn.body.value-less-fallthrough`, at the statement
                    // the body falls through from.
                    let at = stmts.last().copied().unwrap_or(block);
                    let msg = format!("this path ends without a {} value", self.show(w));
                    self.err(Code::MissingReturnValue, at, &msg);
                } else if !snap_ok && !Self::ends_in_return(block) {
                    let msg = format!("in result: expected {}, found void", self.show(w));
                    self.err(Code::TypeMismatch, block, &msg);
                }
            }
        }
        if diverged && tail.is_none() {
            ty = Ty::NEVER;
        }
        Ok((tail, ty))
    }

    /// Whether a block's last statement leaves (`return`, `panic`-like
    /// never calls already typed `never` are handled by `lines`).
    fn ends_in_return(block: NodeRef<'_>) -> bool {
        block.children().last().is_some_and(|s| {
            matches!(
                s.kind(),
                SyntaxKind::ReturnStmt | SyntaxKind::BreakStmt | SyntaxKind::ContinueStmt
            )
        })
    }

    /// `flow.must-use.discard`: `Result[T, E]`, `T?` and `mut Suspend[T]`.
    pub(crate) fn must_use(&self, t: Ty) -> bool {
        let pool = self.cx.names.pool;
        let t = self.infer.resolve(pool, t);
        let (inner, is_mut) = match pool.get(t) {
            TyData::Mut(i) => (self.infer.resolve(pool, i), true),
            _ => (t, false),
        };
        match pool.get(inner) {
            TyData::Option(_) => true,
            TyData::Adt { def, .. } => match self.cx.names.path(def).as_str() {
                "std/core/Result" => true,
                p => is_mut && p.ends_with("/Suspend"),
            },
            _ => false,
        }
    }

    /// One statement; its type is `never` when it cannot complete.
    pub(crate) fn stmt(&mut self, s: NodeRef<'_>) -> StageResult<Ty> {
        let span = self.cx.src.span(s);
        let start = self.infer.var_count();
        let r = self.stmt_node(s).map_err(|e| e.at(span));
        // A `break` value joins with the loop's other values, so its class
        // stays open until the join (`types.literal.local.form.join-open`).
        if s.kind() != SyntaxKind::BreakStmt {
            self.close_literals(start);
        }
        r
    }

    /// `types.literal.local.class.open`: a literal class that met no type
    /// by the end of its statement takes its default type.
    fn close_literals(&mut self, start: usize) {
        let pool = self.cx.names.pool;
        for (t, k) in self.infer.open_literals_since(pool, start) {
            let d = match k {
                VarKind::SignedIntLit => Ty::I32,
                VarKind::IntLit => Ty::prim(hd_types::Prim::Usize),
                _ => Ty::prim(hd_types::Prim::F64),
            };
            let _ = self.infer.unify(pool, t, d);
        }
    }

    fn stmt_node(&mut self, s: NodeRef<'_>) -> StageResult<Ty> {
        self.charge()?;
        let kids: Vec<NodeRef<'_>> = s.children().collect();
        match s.kind() {
            SyntaxKind::ExprStmt => {
                let Some(e) = kids.first() else {
                    return Ok(Ty::VOID);
                };
                let (_, t) = self.expr(*e, Some(Ty::VOID))?;
                let t = self.infer.shallow(self.pool(), t);
                if self.must_use(t) {
                    let msg = format!(
                        "a {} value is discarded; handle it or write `_ := ...`",
                        self.show(t)
                    );
                    self.err(Code::DiscardedMustUseValue, *e, &msg);
                }
                return Ok(if t == Ty::NEVER { Ty::NEVER } else { Ty::VOID });
            }
            SyntaxKind::LetStmt => self.let_stmt(s, &kids)?,
            SyntaxKind::DiscardStmt => {
                if let Some(e) = kids.last() {
                    self.expr(*e, None)?;
                }
            }
            SyntaxKind::AssignmentStmt => self.assign(s, &kids)?,
            SyntaxKind::ReturnStmt => {
                if self.defer_base.is_some() {
                    self.err(Code::DeferControlFlow, s, "a `defer` suite cannot `return`");
                }
                let ret = *self.rets.last().expect("ret");
                let r = if let Some(e) = kids.first() {
                    let (r, t) = self.expr(*e, Some(ret))?;
                    self.coerce(r, t, ret, *e, "return")
                } else {
                    Ref(NONE)
                };
                self.b.emit(Tag::Return, r.0, NONE, Ty::NEVER, s.index());
                return Ok(Ty::NEVER);
            }
            SyntaxKind::BreakStmt | SyntaxKind::ContinueStmt => {
                let Some((lp, bt)) = self.loops.last().copied() else {
                    self.err(Code::BreakOutsideLoop, s, "break-outside-loop");
                    return Ok(Ty::NEVER);
                };
                if self.defer_base == Some(self.loops.len()) {
                    self.err(
                        Code::DeferControlFlow,
                        s,
                        "a `defer` suite cannot leave an enclosing loop",
                    );
                }
                if s.kind() == SyntaxKind::ContinueStmt {
                    self.b
                        .emit(Tag::Continue, lp.0.raw(), NONE, Ty::NEVER, s.index());
                    return Ok(Ty::NEVER);
                }
                let v = match (kids.first(), bt) {
                    (Some(e), Some(want)) => {
                        let (r, t) = self.expr(*e, Some(want))?;
                        self.coerce(r, t, want, *e, "break value").0
                    }
                    (Some(e), None) => {
                        self.err(
                            Code::BreakValueContext,
                            *e,
                            "only a loop with `else` takes a `break` value",
                        );
                        NONE
                    }
                    (None, Some(_)) => {
                        self.err(
                            Code::BreakValueContext,
                            s,
                            "a loop with `else` needs a `break` value",
                        );
                        NONE
                    }
                    (None, None) => NONE,
                };
                self.b.emit(Tag::Break, lp.0.raw(), v, Ty::NEVER, s.index());
                return Ok(Ty::NEVER);
            }
            SyntaxKind::DeferStmt => {
                let Some(blk) = Src::child(s, SyntaxKind::Block) else {
                    return unsupported("a `defer` without a suite");
                };
                let m = self.b.open_block();
                let saved = self.defer_base.replace(self.loops.len());
                self.block_value(blk, Some(Ty::VOID))?;
                self.defer_base = saved;
                let suite = self.b.close_block(m, None, Ty::VOID, blk.index());
                self.b.defer(suite, s.index());
            }
            other => return unsupported(format!("statement {other:?}")),
        }
        Ok(Ty::VOID)
    }

    /// `x = e`, `x op= e`, `a.f = e`, `a[i] = e`.
    fn assign(&mut self, s: NodeRef<'_>, kids: &[NodeRef<'_>]) -> StageResult<()> {
        let [lhs, rhs] = kids else {
            return unsupported("this assignment form");
        };
        let op_tok = hd_base::TokenIdx::from_raw(self.cx.src.last(*lhs).raw() + 1);
        let op = self.cx.src.tkind(op_tok);
        let compound = match op {
            Some(TokenKind::Eq) => None,
            Some(k) => Some(k),
            None => return unsupported("an assignment operator"),
        };
        match lhs.kind() {
            SyntaxKind::NameExpr => {
                let name = self.sym_of(*lhs);
                let global = self.cx.init.borrow().globals.get(&name).copied();
                if self.find_local(name).is_none()
                    && let Some(g) = global
                {
                    // A mutable top-level binding (`GlobalSet`).
                    if g.short {
                        let msg = format!(
                            "`{}` is bound with `:=`; use `let` to reassign it",
                            self.cx.names.text(name)
                        );
                        self.err(Code::NonReassignableBinding, *lhs, &msg);
                    }
                    if compound.is_some() {
                        return unsupported("compound assignment to a top-level binding");
                    }
                    let (r, t) = self.expr(*rhs, Some(g.ty))?;
                    let v = self.coerce(r, t, g.ty, *rhs, "assignment");
                    let a = self.b.refs_record(&[Ref(g.def.raw())]);
                    self.b.emit(Tag::GlobalSet, a, v.0, Ty::VOID, s.index());
                    return Ok(());
                }
                let Some((l, depth)) = self.find_local(name) else {
                    let msg = format!("`{}` is not defined", self.cx.names.text(name));
                    self.err(Code::UnknownName, *lhs, &msg);
                    return Ok(());
                };
                let flags = self.b.body_mut().local_flags[l.idx()];
                if flags & local_flags::SHORT != 0 {
                    let msg = format!(
                        "`{}` is bound with `:=`; use `let` to reassign it",
                        self.cx.names.text(name)
                    );
                    self.err(Code::NonReassignableBinding, *lhs, &msg);
                } else if flags & local_flags::PARAM != 0 {
                    let msg = format!(
                        "parameter `{}` cannot be reassigned",
                        self.cx.names.text(name)
                    );
                    self.err(Code::NonReassignableParameterBinding, *lhs, &msg);
                }
                let lt = self.b.local_ty(l);
                let v = match compound {
                    None => {
                        let (r, t) = self.expr(*rhs, Some(lt))?;
                        self.coerce(r, t, lt, *rhs, "assignment")
                    }
                    Some(k) => {
                        let (cur, _) = self.read_local(l, depth, *lhs);
                        self.compound(k, cur, lt, *rhs, s)?
                    }
                };
                self.note_write(l, depth);
                self.b.set(l, v, s.index());
            }
            SyntaxKind::FieldExpr => {
                let Some(base) = lhs.children().next() else {
                    return unsupported("a field without a base");
                };
                let (br, bt) = self.expr(base, None)?;
                let fname = self.cx.src.text(self.cx.src.last(*lhs)).to_owned();
                let (pr, pt) = self.promote_base(br, bt, &fname, *lhs);
                let Some((idx, ft)) = self.field_of(pt, &fname) else {
                    let msg = format!("no field `{fname}` on {}", self.show(bt));
                    self.err(Code::UnknownDataField, *lhs, &msg);
                    return Ok(());
                };
                self.check_store_target(base, (br, bt), *lhs);
                let br = pr;
                let v = match compound {
                    None => {
                        let (r, t) = self.expr(*rhs, Some(ft))?;
                        self.coerce(r, t, ft, *rhs, "assignment")
                    }
                    Some(k) => {
                        let cur = self.b.emit(Tag::Field, br.0, idx, ft, lhs.index());
                        self.compound(k, cur, ft, *rhs, s)?
                    }
                };
                let rec = self.b.refs_record(&[Ref(idx), v]);
                self.b.emit(Tag::FieldSet, br.0, rec, Ty::VOID, s.index());
            }
            SyntaxKind::IndexExpr => {
                let ik: Vec<NodeRef<'_>> = lhs.children().collect();
                let [base, key] = ik.as_slice() else {
                    return unsupported("an index target shape");
                };
                let (br, bt) = self.expr(*base, None)?;
                // `expr.index.trait.place`: a string is not a place, nor is
                // a slice.
                if key.kind() == SyntaxKind::RangeExpr || self.strip_mut(bt) == Ty::STRING {
                    self.err(
                        Code::InvalidAssignmentTarget,
                        *lhs,
                        "this index cannot be assigned",
                    );
                    return Ok(());
                }
                let Some((op_get, op_set, kt, vt)) = self.index_kind(bt) else {
                    return self.index_set((br, bt), (*base, *key), *rhs, compound, (*lhs, s));
                };
                self.check_store_target(*base, (br, bt), *lhs);
                let kr = self.index_key(*key, kt)?;
                let v = match compound {
                    None => {
                        let (r, t) = self.expr(*rhs, Some(vt))?;
                        self.coerce(r, t, vt, *rhs, "assignment")
                    }
                    Some(k) => {
                        let rec = self.b.refs_record(&[br, kr]);
                        let cur = self
                            .b
                            .emit(Tag::Intrinsic, op_get as u32, rec, vt, lhs.index());
                        self.compound(k, cur, vt, *rhs, s)?
                    }
                };
                let rec = self.b.refs_record(&[br, kr, v]);
                self.b
                    .emit(Tag::Intrinsic, op_set as u32, rec, Ty::VOID, s.index());
            }
            _ => return unsupported("assignment to this target"),
        }
        Ok(())
    }

    /// `r[k] = v` and `r[k] op= v` on a receiver without built-in
    /// indexing: `IndexSet::[K, V]::index_set(r, k, v)`, after
    /// `Index::[K]::index(r, k)` for a compound one
    /// (`expr.index.trait.write`).
    fn index_set(
        &mut self,
        (br, bt): (Ref, Ty),
        (base, key): (NodeRef<'_>, NodeRef<'_>),
        rhs: NodeRef<'_>,
        compound: Option<TokenKind>,
        (lhs, s): (NodeRef<'_>, NodeRef<'_>),
    ) -> StageResult<()> {
        let (kr, kt) = self.expr(key, None)?;
        let set = self.cx.names.item("std.ops", "IndexSet");
        if !self.op_fits(set, bt, Some(kt))? {
            self.err(
                Code::InvalidAssignmentTarget,
                lhs,
                &format!("{} does not implement `IndexSet`", self.show(bt)),
            );
            return Ok(());
        }
        self.check_store_target(base, (br, bt), lhs);
        let (vr, vt) = match compound {
            None => self.expr(rhs, None)?,
            Some(k) => {
                let index = self.cx.names.item("std.ops", "Index");
                if !self.op_fits(index, bt, Some(kt))? {
                    let msg = format!(
                        "{} has no `Index` implementation for this key",
                        self.show(bt)
                    );
                    self.err(Code::TypeMismatch, lhs, &msg);
                    return Ok(());
                }
                let (cur, ct) =
                    self.trait_call_args(index, "index", br, bt, &[(kr, kt, key)], lhs)?;
                let v = self.compound(k, cur, ct, rhs, s)?;
                (v, self.b.ty_of(v))
            }
        };
        self.trait_call_args(set, "index_set", br, bt, &[(kr, kt, key), (vr, vt, rhs)], s)?;
        Ok(())
    }

    /// `cur op rhs` for a compound assignment.
    fn compound(
        &mut self,
        k: TokenKind,
        cur: Ref,
        t: Ty,
        rhs: NodeRef<'_>,
        at: NodeRef<'_>,
    ) -> StageResult<Ref> {
        let op = match k {
            TokenKind::PlusEq => TokenKind::Plus,
            TokenKind::MinusEq => TokenKind::Minus,
            TokenKind::StarEq => TokenKind::Star,
            TokenKind::SlashEq => TokenKind::Slash,
            TokenKind::PercentEq => TokenKind::Percent,
            TokenKind::AmpEq => TokenKind::Amp,
            TokenKind::PipeEq => TokenKind::Pipe,
            TokenKind::CaretEq => TokenKind::Caret,
            TokenKind::ShlEq => TokenKind::Shl,
            TokenKind::ShrEq => TokenKind::Shr,
            _ => return unsupported("this compound assignment operator"),
        };
        let want = (!matches!(op, TokenKind::Shl | TokenKind::Shr)).then_some(t);
        let (r, rt) = self.expr(rhs, want)?;
        self.binary_values(op, (cur, t), (r, rt), at, rhs)
    }

    /// Resolves every type the body recorded; literal variables with no
    /// other constraint take their default type (type-checking.md §3.6).
    pub(crate) fn zonk(&mut self, t: Ty) -> Ty {
        let pool = self.cx.names.pool;
        let r = self.infer.resolve(pool, t);
        if !pool.has_infer(r) {
            return if pool.has_assoc(r) {
                self.normalize_deep(r).unwrap_or(r)
            } else {
                r
            };
        }
        match pool.get(r) {
            TyData::Infer(_) => {
                let d = match self.infer.kind_of(pool, r) {
                    Some(VarKind::FloatLit) => Ty::prim(hd_types::Prim::F64),
                    Some(VarKind::SignedIntLit) => Ty::I32,
                    Some(VarKind::IntLit) => Ty::prim(hd_types::Prim::Usize),
                    _ => Ty::POISON,
                };
                if d != Ty::POISON {
                    let _ = self.infer.unify(pool, r, d);
                }
                d
            }
            TyData::Adt { def, args } => {
                let a = self.zonk_list(args);
                pool.intern_ty(&TyData::Adt { def, args: a })
            }
            TyData::Tuple { elems, rest } => {
                let e = self.zonk_list(elems);
                pool.intern_ty(&TyData::Tuple { elems: e, rest })
            }
            TyData::Option(i) => {
                let i = self.zonk(i);
                pool.intern_ty(&TyData::Option(i))
            }
            TyData::Mut(i) => {
                let i = self.zonk(i);
                pool.intern_ty(&TyData::Mut(i))
            }
            TyData::Fn {
                params,
                result,
                row,
                suspends,
            } => {
                let p = self.zonk_list(params);
                let r = self.zonk(result);
                let row = self.zonk_row(row);
                pool.intern_ty(&TyData::Fn {
                    params: p,
                    result: r,
                    row,
                    suspends,
                })
            }
            TyData::Row(row) => {
                let row = self.zonk_row(row);
                pool.intern_ty(&TyData::Row(row))
            }
            TyData::TraitValue {
                def,
                args,
                bindings,
            } => {
                let a = self.zonk_list(args);
                pool.intern_ty(&TyData::TraitValue {
                    def,
                    args: a,
                    bindings,
                })
            }
            TyData::Assoc {
                assoc,
                trait_,
                self_ty,
                args,
            } => {
                let s = self.zonk(self_ty);
                let a = self.zonk_list(args);
                let x = pool.intern_ty(&TyData::Assoc {
                    assoc,
                    trait_,
                    self_ty: s,
                    args: a,
                });
                self.normalize(x).unwrap_or(x)
            }
            _ => Ty::POISON,
        }
    }

    /// A row with its variables solved; a row variable nothing constrained
    /// is the empty row, the least solution.
    pub(crate) fn zonk_row(&mut self, row: RowId) -> RowId {
        let pool = self.cx.names.pool;
        let row = self.infer.resolve_row(pool, row);
        let mut d = pool.row_data(row);
        if !d.keys.iter().any(|k| pool.has_infer(*k)) {
            return row;
        }
        let mut keys = Vec::new();
        for k in d.keys {
            if matches!(pool.get(k), TyData::Infer(_)) {
                let empty = pool.intern_ty(&TyData::Row(RowId::EMPTY));
                let _ = self.infer.unify(pool, k, empty);
            } else {
                keys.push(self.zonk(k));
            }
        }
        d.keys = keys;
        pool.row(&d)
    }

    fn zonk_list(&mut self, l: TyList) -> TyList {
        // Owned: `zonk` needs `&mut self` while the items are walked.
        let mut items: Vec<Ty> = self.pool().list_items(l).to_vec();
        for x in &mut items {
            *x = self.zonk(*x);
        }
        self.pool().list(&items)
    }

    pub(crate) fn finish_body(self, root: Ref) -> StageResult<Body> {
        self.finish(root)
    }

    fn finish(mut self, root: Ref) -> StageResult<Body> {
        self.unused_locals();
        if self.module_init.is_none() {
            let item = self.b.body_mut().item;
            let facts = std::mem::take(&mut self.facts);
            self.cx.init.borrow_mut().facts.insert(item, facts);
        }
        let pool = self.cx.names.pool;
        // Literal defaults first, so `x := +0` then `x = y` resolves both.
        let n = self.b.body_mut().ty.len();
        for i in 0..self.b.body_mut().consts.len() {
            let t = self.b.body_mut().consts[i].0;
            let z = self.zonk(t);
            self.b.body_mut().consts[i].0 = z;
        }
        // Bounds that waited for inference.
        let pending = std::mem::take(&mut self.pending);
        for (tref, at) in pending {
            let tref = TraitRef {
                trait_: tref.trait_,
                self_ty: self.zonk(tref.self_ty),
                args: self.zonk_list(tref.args),
            };
            if pool.has_poison(tref.self_ty) || self.builtin_holds(tref).is_some() {
                continue;
            }
            // Its evidence is chosen again per instance at collection; a
            // bound that fails once the defaults are in is an error.
            if let Answer::Fails(_) = self.solve(tref)? {
                let msg = format!(
                    "{} does not implement {}",
                    self.show(tref.self_ty),
                    self.cx.names.path(tref.trait_)
                );
                let node = self.cx.src.parse.tree.node(at);
                self.err(Code::UnsatisfiedTraitBound, node, &msg);
            }
        }
        for i in 0..n {
            let t = self.b.body_mut().ty[i];
            let z = self.zonk(t);
            self.b.body_mut().ty[i] = z;
        }
        for i in 0..self.b.body_mut().local_ty.len() {
            let t = self.b.body_mut().local_ty[i];
            let z = self.zonk(t);
            self.b.body_mut().local_ty[i] = z;
        }
        for i in 0..n {
            let tag = self.b.body_mut().tags[i];
            if tag != Tag::Call && tag != Tag::Await {
                continue;
            }
            let at = self.b.body_mut().data[i][0] as usize + 1;
            let words = self
                .b
                .body_mut()
                .record(u32::try_from(at - 1).expect("extra"))
                .to_vec();
            let Some(c) = Callee::from_words(&words) else {
                continue;
            };
            let z = match c {
                Callee::Item { def, targs } => Callee::Item {
                    def,
                    targs: self.zonk_list(targs),
                },
                Callee::TraitMethod {
                    trait_,
                    method,
                    self_ty,
                    targs,
                    choice,
                } => Callee::TraitMethod {
                    trait_,
                    method,
                    self_ty: self.zonk(self_ty),
                    targs: self.zonk_list(targs),
                    choice,
                },
            };
            let nw = z.words();
            self.b.body_mut().extra[at..at + nw.len()].copy_from_slice(&nw);
        }
        // Types and type lists kept in records.
        for i in 0..n {
            let tag = self.b.body_mut().tags[i];
            let [a, bw] = self.b.body_mut().data[i];
            let list_at = match tag {
                Tag::ProviderGet => {
                    let at = a as usize + 1;
                    let t = Ty(self.b.body_mut().extra[at]);
                    self.b.body_mut().extra[at] = self.zonk(t).0;
                    continue;
                }
                Tag::With => {
                    let len = self.b.body_mut().extra[a as usize] as usize;
                    for k in 0..len / 2 {
                        let at = a as usize + 1 + 2 * k;
                        let t = Ty(self.b.body_mut().extra[at]);
                        self.b.body_mut().extra[at] = self.zonk(t).0;
                    }
                    continue;
                }
                Tag::ItemRef => bw as usize + 1,
                Tag::DefaultCall => a as usize + 2,
                _ => continue,
            };
            let l = TyList(self.b.body_mut().extra[list_at]);
            self.b.body_mut().extra[list_at] = self.zonk_list(l).0;
        }
        if !self.diags.has_errors() {
            for i in 0..n {
                let t = self.b.body_mut().ty[i];
                let at = self.b.body_mut().syn[i];
                if pool.has_poison(t)
                    && self.b.body_mut().tags[i] != Tag::Poison
                    && at != hd_base::NodeIdx::NONE
                {
                    let node = self.cx.src.parse.tree.node(at);
                    self.err(Code::CannotInferType, node, "annotate this value's type");
                    break;
                }
            }
        }
        self.check_literal_ranges();
        // Capture modes (checking-and-tir.md "Data and closures"): `Copy`
        // when no side assigns the local after its declaration, `Shared`
        // otherwise. `Move` needs the liveness pass and is not chosen yet.
        let modes: Vec<hd_tir::ir::CaptureMode> = {
            let body = self.b.body_mut();
            body.cap_local
                .iter()
                .map(|l| {
                    let f = body.local_flags[l.idx()];
                    if f & (local_flags::CAPTURED_ASSIGNED | local_flags::MUTATED) != 0 {
                        hd_tir::ir::CaptureMode::Shared
                    } else {
                        hd_tir::ir::CaptureMode::Copy
                    }
                })
                .collect()
        };
        self.b
            .finish(root, &modes)
            .map_err(|e| NotImplemented::new(Stage::Body, format!("TIR verifier: {e:?}")))
    }
}
