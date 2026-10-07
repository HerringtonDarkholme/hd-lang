//! Body checking (type-checking.md §1.4 to §1.6; checking-and-tir.md
//! §4.13): one function body over the full parser's tree, typed with an
//! `InferTable` in the run's `InternPool`, trait goals through the
//! `Solver` with fuel, TIR written through `TirBuilder`. User errors go to
//! the `DiagBuf`; a construct the checker does not carry yet is a
//! structured "not implemented", which stops a build.

use std::collections::HashMap;

use hd_base::{DefId, Fuel, LocalId, ModuleId, NotImplemented, Stage, StageResult, Symbol};
use hd_diag::{Code, DiagBuf};
use hd_resolve::{BindingKind, FnSig, ItemData, Lookup, ModuleScope, Names, Src};
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_tir::Body;
use hd_tir::ir::{
    BodyKind, Callee, ChoiceKind, NONE, PrimOp, Providers, Ref, Tag, TirBuilder, TirSink,
    local_flags,
};
use hd_types::solver::{
    Answer, GlobalMemo, Goal, ImplTable, ImplUniverseId, ParamEnv, SolveCx, Solver, TraitRef,
};
use hd_types::{InferTable, ParamRef, Prim, Ty, TyData, TyList, VarKind};

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
}

fn unsupported<T>(what: impl Into<String>) -> StageResult<T> {
    Err(NotImplemented::new(Stage::Body, what))
}

struct Ck<'a, 'c> {
    cx: &'c BodyCx<'a>,
    def: DefId,
    sig: FnSig,
    b: TirBuilder,
    infer: InferTable,
    locals: Vec<HashMap<Symbol, LocalId>>,
    env: ParamEnv,
    loops: Vec<hd_tir::ir::LoopMark>,
    diags: &'c mut DiagBuf,
    fuel: Fuel,
    memo: hd_types::solver::BodyMemo,
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
    let mut env = ParamEnv::default();
    for (i, g) in sig.generics.iter().enumerate() {
        if let Some(tr) = g.bound {
            let p = cx.names.pool.intern_ty(&TyData::Param(ParamRef {
                owner: def,
                index: u16::try_from(i).expect("generics"),
            }));
            env.clause_self.push(p);
            env.clause_trait.push(tr);
            env.clause_args.push(TyList::EMPTY);
            env.clause_bindings.push(vec![]);
            env.clause_mut.push(false);
            env.clause_origin.push(u16::try_from(i).expect("generics"));
        }
    }
    let mut ck = Ck {
        cx,
        def,
        sig,
        b: TirBuilder::new(def, BodyKind::Fn),
        infer: InferTable::default(),
        locals: vec![HashMap::new()],
        env,
        loops: Vec::new(),
        diags,
        fuel: Fuel::new(Fuel::BODY_DEFAULT),
        memo: hd_types::solver::BodyMemo::default(),
    };
    let blk = ck.b.open_block();
    for (name, ty) in ck.sig.params.clone() {
        let l = ck.b.local(ty, name, local_flags::PARAM, node.index());
        ck.locals[0].insert(name, l);
    }
    let Some(body) = Src::child(node, SyntaxKind::Block) else {
        return unsupported("a function without a body");
    };
    let ret = ck.sig.ret;
    let tail = ck.lines(body, ret != Ty::VOID)?;
    let root = ck.b.close_block(blk, tail, Ty::VOID, body.index());
    ck.finish(root)
}

impl Ck<'_, '_> {
    fn charge(&mut self) -> StageResult<()> {
        if self.fuel.charge(1) {
            Ok(())
        } else {
            unsupported("the body's fuel ran out (limit diagnostic)")
        }
    }

    fn err(&mut self, code: Code, n: NodeRef<'_>, msg: &str) {
        let span = self.cx.src.span(n);
        self.diags.error(code, span, msg);
    }

    fn show(&self, t: Ty) -> String {
        let t = self.infer.resolve(self.cx.names.pool, t);
        match self.cx.names.pool.get(t) {
            TyData::Adt { def, .. } => self.cx.names.path(def),
            TyData::Infer(_) => "{integer}".into(),
            _ => self.cx.names.pool.display(t),
        }
    }

    fn expect(&mut self, got: Ty, want: Ty, n: NodeRef<'_>, what: &str) {
        let pool = self.cx.names.pool;
        if got == Ty::NEVER || want == Ty::NEVER {
            return;
        }
        let snap = self.infer.snapshot();
        if self.infer.unify(pool, got, want).is_err() {
            self.infer.rollback(snap);
            let msg = format!(
                "type-mismatch in {what}: expected {}, found {}",
                self.show(want),
                self.show(got)
            );
            self.err(Code::TypeMismatch, n, &msg);
        }
    }

    fn find_local(&self, s: Symbol) -> Option<LocalId> {
        self.locals.iter().rev().find_map(|m| m.get(&s).copied())
    }

    fn sym(&self, n: NodeRef<'_>) -> Symbol {
        self.cx
            .names
            .syms
            .intern(self.cx.src.text(self.cx.src.first(n)))
    }

    /// The statements of a block. With `want_tail`, a last expression
    /// statement is the block's value.
    fn lines(&mut self, block: NodeRef<'_>, want_tail: bool) -> StageResult<Option<Ref>> {
        self.locals.push(HashMap::new());
        let stmts: Vec<NodeRef<'_>> = block.children().collect();
        let mut tail = None;
        for (i, s) in stmts.iter().copied().enumerate() {
            let last = i + 1 == stmts.len();
            if s.kind() == SyntaxKind::ExprStmt {
                let Some(e) = s.children().next() else {
                    return unsupported("an empty expression statement");
                };
                match e.kind() {
                    SyntaxKind::IfExpr => self.if_expr(e)?,
                    SyntaxKind::WhileExpr => self.while_loop(e)?,
                    SyntaxKind::BindingExpr => self.binding(e)?,
                    _ if last && want_tail => {
                        let (r, t) = self.expr(e)?;
                        self.expect(t, self.sig.ret, e, "result");
                        tail = Some(r);
                    }
                    _ => {
                        self.expr(e)?;
                    }
                }
                continue;
            }
            self.stmt(s)?;
        }
        self.locals.pop();
        Ok(tail)
    }

    /// `if c: ... else if c: ... else: ...` as a statement.
    fn if_expr(&mut self, e: NodeRef<'_>) -> StageResult<()> {
        let Some(cond) = e.children().next() else {
            return unsupported("an `if` without a condition");
        };
        let Some(then_node) = Src::child(e, SyntaxKind::Block) else {
            return unsupported("an `if` without a block");
        };
        let (c, ct) = self.expr(cond)?;
        self.expect(ct, Ty::BOOL, cond, "condition");
        let tb = self.b.open_block();
        self.lines(then_node, false)?;
        let then = self.b.close_block(tb, None, Ty::VOID, then_node.index());
        let els = if let Some(clause) = Src::child(e, SyntaxKind::ElseClause) {
            let eb = self.b.open_block();
            if let Some(nested) = Src::child(clause, SyntaxKind::IfExpr) {
                self.if_expr(nested)?;
            } else {
                let Some(blk) = Src::child(clause, SyntaxKind::Block) else {
                    return unsupported("an `else` without a block");
                };
                self.lines(blk, false)?;
            }
            self.b.close_block(eb, None, Ty::VOID, clause.index())
        } else {
            Ref(NONE)
        };
        let rec = self.b.refs_record(&[then, els]);
        self.b.emit(Tag::If, c.0, rec, Ty::VOID, e.index());
        Ok(())
    }

    /// `while c: body` is `Loop { Block { if c { body } else { break } } }`.
    fn while_loop(&mut self, e: NodeRef<'_>) -> StageResult<()> {
        if Src::child(e, SyntaxKind::ElseClause).is_some() {
            return unsupported("a `while` with an `else` suite");
        }
        let Some(cond) = e.children().next() else {
            return unsupported("a `while` without a condition");
        };
        let Some(body) = Src::child(e, SyntaxKind::Block) else {
            return unsupported("a `while` without a block");
        };
        let lp = self.b.open_loop();
        let lb = self.b.open_block();
        let (c, ct) = self.expr(cond)?;
        self.expect(ct, Ty::BOOL, cond, "condition");
        self.loops.push(lp);
        let tb = self.b.open_block();
        self.lines(body, false)?;
        let then = self.b.close_block(tb, None, Ty::VOID, body.index());
        self.loops.pop();
        let eb = self.b.open_block();
        self.b
            .emit(Tag::Break, lp.0.raw(), NONE, Ty::NEVER, e.index());
        let els = self.b.close_block(eb, None, Ty::VOID, e.index());
        let rec = self.b.refs_record(&[then, els]);
        self.b.emit(Tag::If, c.0, rec, Ty::VOID, e.index());
        let lbody = self.b.close_block(lb, None, Ty::VOID, e.index());
        self.b.close_loop(lp, lbody, Ty::VOID, e.index());
        Ok(())
    }

    /// `name := value` (or `let name = value`): a new local.
    fn bind(&mut self, pat: NodeRef<'_>, e: NodeRef<'_>, at: NodeRef<'_>) -> StageResult<()> {
        if pat.kind() != SyntaxKind::BindingPattern
            || self.cx.src.tkind(self.cx.src.first(pat)) == Some(TokenKind::KwMut)
        {
            return unsupported("this binding pattern");
        }
        let name = self.sym(pat);
        let (r, t) = self.expr(e)?;
        let l = self.b.local(t, name, local_flags::ASSIGNED, pat.index());
        self.locals.last_mut().expect("scope").insert(name, l);
        self.b.set(l, r, at.index());
        Ok(())
    }

    fn binding(&mut self, e: NodeRef<'_>) -> StageResult<()> {
        self.charge()?;
        let kids: Vec<NodeRef<'_>> = e.children().collect();
        let [pat, rhs] = kids.as_slice() else {
            return unsupported("this binding form");
        };
        if rhs.kind() == SyntaxKind::BindingExpr {
            return unsupported("a binding chain");
        }
        self.bind(*pat, *rhs, e)
    }

    fn stmt(&mut self, s: NodeRef<'_>) -> StageResult<()> {
        self.charge()?;
        let kids: Vec<NodeRef<'_>> = s.children().collect();
        match s.kind() {
            SyntaxKind::LetStmt => {
                let [pat, e] = kids.as_slice() else {
                    return unsupported("this `let` form");
                };
                self.bind(*pat, *e, s)?;
            }
            SyntaxKind::AssignmentStmt => {
                let [lhs, rhs] = kids.as_slice() else {
                    return unsupported("this assignment form");
                };
                let op = hd_base::TokenIdx::from_raw(self.cx.src.last(*lhs).raw() + 1);
                if self.cx.src.tkind(op) != Some(TokenKind::Eq) {
                    return unsupported("compound assignment");
                }
                if lhs.kind() != SyntaxKind::NameExpr {
                    return unsupported("assignment to a non-local");
                }
                let name = self.sym(*lhs);
                let Some(l) = self.find_local(name) else {
                    let msg = format!("unknown-name `{}`", self.cx.names.text(name));
                    self.err(Code::UnknownName, *lhs, &msg);
                    return Ok(());
                };
                let (r, t) = self.expr(*rhs)?;
                let lt = self.b.local_ty(l);
                self.expect(t, lt, *rhs, "assignment");
                self.b.set(l, r, s.index());
            }
            SyntaxKind::ReturnStmt => {
                let r = if let Some(e) = kids.first() {
                    let (r, t) = self.expr(*e)?;
                    self.expect(t, self.sig.ret, *e, "return");
                    r
                } else {
                    Ref(NONE)
                };
                self.b.emit(Tag::Return, r.0, NONE, Ty::NEVER, s.index());
            }
            SyntaxKind::BreakStmt | SyntaxKind::ContinueStmt => {
                if !kids.is_empty() {
                    return unsupported("`break` with a value");
                }
                let Some(lp) = self.loops.last().copied() else {
                    return unsupported("`break` outside a loop");
                };
                let tag = if s.kind() == SyntaxKind::BreakStmt {
                    Tag::Break
                } else {
                    Tag::Continue
                };
                self.b.emit(tag, lp.0.raw(), NONE, Ty::NEVER, s.index());
            }
            other => return unsupported(format!("statement {other:?}")),
        }
        Ok(())
    }

    fn int_lit(&mut self, bits: u64) -> (Ref, Ty) {
        let t = self.infer.fresh(self.cx.names.pool, VarKind::IntLit);
        (self.b.const_value(t, bits), t)
    }

    fn expr(&mut self, n: NodeRef<'_>) -> StageResult<(Ref, Ty)> {
        self.charge()?;
        let pool = self.cx.names.pool;
        let kids: Vec<NodeRef<'_>> = n.children().collect();
        Ok(match n.kind() {
            SyntaxKind::LiteralExpr => {
                let t = self.cx.src.first(n);
                match self.cx.src.tkind(t) {
                    Some(TokenKind::KwTrue) => (self.b.const_value(Ty::BOOL, 1), Ty::BOOL),
                    Some(TokenKind::KwFalse) => (self.b.const_value(Ty::BOOL, 0), Ty::BOOL),
                    Some(TokenKind::Number) => {
                        let text = self.cx.src.text(t).replace('_', "");
                        let Ok(v) = text.parse::<i64>() else {
                            return unsupported(format!("the literal `{text}`"));
                        };
                        self.int_lit(v.cast_unsigned())
                    }
                    _ => return unsupported("this literal form"),
                }
            }
            SyntaxKind::UnaryExpr => {
                let op = self.cx.src.tkind(self.cx.src.first(n));
                let Some(e) = kids.first() else {
                    return unsupported("an operand-less unary");
                };
                let (r, t) = self.expr(*e)?;
                match op {
                    Some(TokenKind::Plus) => (r, t),
                    Some(TokenKind::Minus) => {
                        if let Some((ct, bits)) = self.b.const_of(r) {
                            (
                                self.b.const_value(
                                    ct,
                                    bits.cast_signed().wrapping_neg().cast_unsigned(),
                                ),
                                t,
                            )
                        } else {
                            (self.b.prim(PrimOp::Neg as u32, &[r], t, n.index()), t)
                        }
                    }
                    _ => return unsupported("this unary operator"),
                }
            }
            SyntaxKind::ParenExpr => match kids.as_slice() {
                [e] => self.expr(*e)?,
                _ => return unsupported("a parenthesized expression shape"),
            },
            SyntaxKind::NameExpr => {
                let s = self.sym(n);
                if let Some(l) = self.find_local(s) {
                    let t = self.b.local_ty(l);
                    (self.b.get(l, t, n.index()), t)
                } else {
                    let msg = format!("unknown-name `{}`", self.cx.names.text(s));
                    self.err(Code::UnknownName, n, &msg);
                    (Ref(NONE), Ty::NEVER)
                }
            }
            SyntaxKind::AdditiveExpr
            | SyntaxKind::MultiplicativeExpr
            | SyntaxKind::ComparisonExpr
            | SyntaxKind::LogicalAndExpr
            | SyntaxKind::LogicalOrExpr => {
                let [l, r] = kids.as_slice() else {
                    return unsupported("a binary expression shape");
                };
                let op_tok = hd_base::TokenIdx::from_raw(self.cx.src.last(*l).raw() + 1);
                let op = match self.cx.src.tkind(op_tok) {
                    Some(TokenKind::Plus) => PrimOp::Add,
                    Some(TokenKind::Minus) => PrimOp::Sub,
                    Some(TokenKind::Star) => PrimOp::Mul,
                    Some(TokenKind::Slash) => PrimOp::Div,
                    Some(TokenKind::Percent) => PrimOp::Rem,
                    Some(TokenKind::EqEq) => PrimOp::Eq,
                    Some(TokenKind::NotEq) => PrimOp::Ne,
                    Some(TokenKind::Lt) => PrimOp::Lt,
                    Some(TokenKind::LtEq) => PrimOp::Le,
                    Some(TokenKind::Gt) => PrimOp::Gt,
                    Some(TokenKind::GtEq) => PrimOp::Ge,
                    Some(TokenKind::AndAnd) => PrimOp::And,
                    Some(TokenKind::OrOr) => PrimOp::Or,
                    _ => return unsupported("this binary operator"),
                };
                let (a, at) = self.expr(*l)?;
                let (c, ct) = self.expr(*r)?;
                let logical = matches!(op, PrimOp::And | PrimOp::Or);
                if logical {
                    self.expect(at, Ty::BOOL, *l, "operand");
                    self.expect(ct, Ty::BOOL, *r, "operand");
                } else {
                    self.expect(ct, at, *r, "operand");
                    let resolved = self.infer.shallow(pool, at);
                    let numeric = match pool.get(resolved) {
                        TyData::Prim(p) => {
                            p.is_integer()
                                || (p == Prim::Bool && matches!(op, PrimOp::Eq | PrimOp::Ne))
                        }
                        TyData::Infer(_) | TyData::Never | TyData::Poison => true,
                        _ => false,
                    };
                    if !numeric {
                        let msg = format!(
                            "type-mismatch in operand: expected an integer, found {}",
                            self.show(at)
                        );
                        self.err(Code::TypeMismatch, *l, &msg);
                    }
                }
                let result = if matches!(
                    op,
                    PrimOp::Add | PrimOp::Sub | PrimOp::Mul | PrimOp::Div | PrimOp::Rem
                ) {
                    at
                } else {
                    Ty::BOOL
                };
                (self.b.prim(op as u32, &[a, c], result, n.index()), result)
            }
            SyntaxKind::FieldExpr => {
                let Some(base) = kids.first() else {
                    return unsupported("a field without a base");
                };
                let (r, bt) = self.expr(*base)?;
                let name = self.cx.src.text(self.cx.src.last(n)).to_owned();
                let bt = self.infer.resolve(pool, bt);
                let fields = match pool.get(bt) {
                    TyData::Adt { def, .. } => match self.cx.lookup.item(def).map(|i| &i.data) {
                        Some(ItemData::Data(fs)) => Some(fs.clone()),
                        _ => None,
                    },
                    _ => None,
                };
                let sym = self.cx.names.syms.intern(&name);
                if let Some((i, ft)) = fields
                    .and_then(|fs| fs.iter().position(|f| f.name == sym).map(|i| (i, fs[i].ty)))
                {
                    let idx = u32::try_from(i).expect("fields");
                    (self.b.emit(Tag::Field, r.0, idx, ft, n.index()), ft)
                } else {
                    let msg = format!("unknown-data-field `{name}` on {}", self.show(bt));
                    self.err(Code::UnknownDataField, n, &msg);
                    (Ref(NONE), Ty::NEVER)
                }
            }
            SyntaxKind::DataExpr => self.data_expr(n, &kids)?,
            SyntaxKind::CallExpr => self.call(n, &kids)?,
            other => return unsupported(format!("expression {other:?}")),
        })
    }

    fn data_expr(&mut self, n: NodeRef<'_>, kids: &[NodeRef<'_>]) -> StageResult<(Ref, Ty)> {
        let pool = self.cx.names.pool;
        let Some(name_node) = kids.first() else {
            return unsupported("a data literal without a name");
        };
        let s = self.sym(*name_node);
        let def = match self.cx.scope.lookup(s) {
            Some(b) if b.kind == BindingKind::Item => DefId::from_raw(b.value),
            _ => {
                let msg = format!("unknown-type `{}`", self.cx.names.text(s));
                self.err(Code::UnknownType, *name_node, &msg);
                return Ok((Ref(NONE), Ty::NEVER));
            }
        };
        let Some(ItemData::Data(fields)) = self.cx.lookup.item(def).map(|i| i.data.clone()) else {
            let msg = format!("unknown-type `{}`: not a data type", self.cx.names.text(s));
            self.err(Code::UnknownType, *name_node, &msg);
            return Ok((Ref(NONE), Ty::NEVER));
        };
        let mut vals = vec![Ref(NONE); fields.len()];
        for a in &kids[1..] {
            let fname = self.sym(*a);
            let Some(i) = fields.iter().position(|f| f.name == fname) else {
                let msg = format!("unknown-data-field `{}`", self.cx.names.text(fname));
                self.err(Code::UnknownDataField, *a, &msg);
                continue;
            };
            let Some(e) = a.children().next() else {
                continue;
            };
            let (r, t) = self.expr(e)?;
            self.expect(t, fields[i].ty, e, "field");
            vals[i] = r;
        }
        if vals.contains(&Ref(NONE)) {
            let msg = format!("missing-required-field in `{}`", self.cx.names.text(s));
            self.err(Code::MissingRequiredField, n, &msg);
        }
        let ty = pool.intern_ty(&TyData::Adt {
            def,
            args: TyList::EMPTY,
        });
        let rec = self.b.refs_record(&vals);
        Ok((self.b.emit(Tag::NewData, NONE, rec, ty, n.index()), ty))
    }

    fn call(&mut self, n: NodeRef<'_>, kids: &[NodeRef<'_>]) -> StageResult<(Ref, Ty)> {
        let pool = self.cx.names.pool;
        let (Some(callee), al) = (kids.first().copied(), kids.get(1).copied()) else {
            return unsupported("a call shape");
        };
        if callee.kind() == SyntaxKind::FieldExpr {
            return self.method_call(n, callee, al);
        }
        if callee.kind() != SyntaxKind::NameExpr {
            return unsupported("a call of a computed value");
        }
        let s = self.sym(callee);
        let arg_nodes: Vec<NodeRef<'_>> = al
            .iter()
            .flat_map(|l| l.children())
            .filter_map(|a| a.children().next())
            .collect();
        match self.cx.scope.lookup(s) {
            Some(b) if b.kind == BindingKind::Prelude => {
                let Some(p) = hd_host_abi::PRELUDE_IMPORTS.get(b.value as usize) else {
                    return unsupported("a prelude function without an import");
                };
                if arg_nodes.len() != p.params.len() {
                    self.err(
                        Code::ArgumentCount,
                        n,
                        &format!(
                            "argument-count: `{}` takes {} arguments",
                            p.name,
                            p.params.len()
                        ),
                    );
                }
                let mut refs = Vec::new();
                for e in &arg_nodes {
                    let (r, t) = self.expr(*e)?;
                    self.expect(t, Ty::I32, *e, "argument");
                    refs.push(r);
                }
                let rec = self.b.refs_record(&refs);
                Ok((
                    self.b
                        .emit(Tag::CallHost, b.value, rec, Ty::VOID, n.index()),
                    Ty::VOID,
                ))
            }
            Some(b) if b.kind == BindingKind::Item => {
                let def = DefId::from_raw(b.value);
                let Some(ItemData::Fn(sig)) = self.cx.lookup.item(def).map(|i| i.data.clone())
                else {
                    let msg = format!("unknown-name `{}`: not a function", self.cx.names.text(s));
                    self.err(Code::UnknownName, callee, &msg);
                    return Ok((Ref(NONE), Ty::NEVER));
                };
                let vars: Vec<Ty> = sig
                    .generics
                    .iter()
                    .map(|_| self.infer.fresh(pool, VarKind::General))
                    .collect();
                let inst = |t: Ty| {
                    pool.subst(t, &|p: ParamRef| {
                        (p.owner == def)
                            .then(|| vars.get(p.index as usize).copied())
                            .flatten()
                    })
                };
                if arg_nodes.len() != sig.params.len() {
                    let msg = format!(
                        "argument-count: `{}` takes {} arguments",
                        self.cx.names.text(s),
                        sig.params.len()
                    );
                    self.err(Code::ArgumentCount, n, &msg);
                }
                let mut refs = Vec::new();
                for (i, e) in arg_nodes.iter().enumerate() {
                    let (r, t) = self.expr(*e)?;
                    if let Some((_, pt)) = sig.params.get(i) {
                        self.expect(t, inst(*pt), *e, "argument");
                    }
                    refs.push(r);
                }
                for (i, g) in sig.generics.iter().enumerate() {
                    if let Some(tr) = g.bound {
                        self.require(tr, vars[i], n)?;
                    }
                }
                let ret = inst(sig.ret);
                let targs = pool.list(&vars);
                let c = Callee::Item { def, targs };
                Ok((self.b.call(&c, &refs, Providers::None, ret, n.index()), ret))
            }
            _ => {
                let msg = format!("unknown-name `{}`", self.cx.names.text(s));
                self.err(Code::UnknownName, callee, &msg);
                Ok((Ref(NONE), Ty::NEVER))
            }
        }
    }

    /// A bound at a use site: `ty: trait_` must hold (trait-solver.md §1.2).
    fn require(&mut self, trait_: DefId, ty: Ty, at: NodeRef<'_>) -> StageResult<()> {
        let pool = self.cx.names.pool;
        let ty = self.infer.resolve(pool, ty);
        if pool.has_infer(ty) {
            return unsupported("a bound on a type that is not yet known");
        }
        let goal = Goal::Implements {
            tref: TraitRef {
                trait_,
                self_ty: ty,
                args: TyList::EMPTY,
            },
            bindings: vec![],
            mut_: false,
        };
        let mut scx = SolveCx {
            pool,
            env: &self.env,
            universe: self.cx.universe,
            tables: self.cx.impls,
            body_memo: &mut self.memo,
            global: self.cx.global,
        };
        match self.cx.solver.solve(&mut scx, &goal, &mut self.fuel)? {
            Answer::Holds { .. } => Ok(()),
            Answer::Fails(_) => {
                let msg = format!(
                    "unsatisfied-trait-bound: {} does not implement {}",
                    self.show(ty),
                    self.cx.names.path(trait_)
                );
                self.err(Code::UnsatisfiedTraitBound, at, &msg);
                Ok(())
            }
            Answer::OutOfFuel => unsupported("the solver's fuel ran out (limit diagnostic)"),
            other => unsupported(format!("solver answer {other:?}")),
        }
    }

    fn method_call(
        &mut self,
        n: NodeRef<'_>,
        fe: NodeRef<'_>,
        al: Option<NodeRef<'_>>,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.cx.names.pool;
        let Some(recv_node) = fe.children().next() else {
            return unsupported("a method call without a receiver");
        };
        let mname = self.cx.src.text(self.cx.src.last(fe)).to_owned();
        let msym = self.cx.names.syms.intern(&mname);
        let (recv, rt) = self.expr(recv_node)?;
        let rt = self.infer.resolve(pool, rt);
        let has_method = |d: DefId| -> Option<DefId> {
            match self.cx.lookup.item(d).map(|i| &i.data) {
                Some(ItemData::Trait(ms)) => ms.iter().find(|(m, _)| *m == msym).map(|(_, d)| *d),
                _ => None,
            }
        };
        let found = match pool.get(rt) {
            TyData::Param(p) if p.owner == self.def => self
                .sig
                .generics
                .get(p.index as usize)
                .and_then(|g| g.bound)
                .and_then(|tr| {
                    has_method(tr).map(|m| (tr, m, (ChoiceKind::Bound, u32::from(p.index))))
                }),
            TyData::Adt { .. } => {
                let mut hits = Vec::new();
                for imp in self.cx.lookup.impls() {
                    if let ItemData::Impl {
                        trait_, self_ty, ..
                    } = &imp.data
                        && *self_ty == rt
                        && let Some(m) = has_method(*trait_)
                    {
                        hits.push((*trait_, m, (ChoiceKind::Impl, imp.def.raw())));
                    }
                }
                match hits.as_slice() {
                    [one] => Some(*one),
                    [] => None,
                    _ => return unsupported("method selection among several traits"),
                }
            }
            _ => None,
        };
        let Some((trait_, method, choice)) = found else {
            let msg = format!("unknown-method `{mname}` on {}", self.show(rt));
            self.err(Code::UnknownMethod, fe, &msg);
            return Ok((Ref(NONE), Ty::NEVER));
        };
        self.require(trait_, rt, n)?;
        let Some(sig) = self.cx.lookup.item(method).and_then(|i| i.sig().cloned()) else {
            return unsupported("a trait method without a signature");
        };
        if !sig.generics.is_empty() {
            return unsupported("generic trait methods");
        }
        let inst = |t: Ty| {
            pool.subst(t, &|p: ParamRef| {
                (p.owner == trait_ && p.index == 0).then_some(rt)
            })
        };
        let arg_nodes: Vec<NodeRef<'_>> = al
            .iter()
            .flat_map(|l| l.children())
            .filter_map(|a| a.children().next())
            .collect();
        let rest = sig.params.get(1..).unwrap_or(&[]);
        if arg_nodes.len() != rest.len() {
            self.err(
                Code::ArgumentCount,
                n,
                &format!("argument-count: `{mname}` takes {} arguments", rest.len()),
            );
        }
        let mut refs = vec![recv];
        for (i, e) in arg_nodes.iter().enumerate() {
            let (r, t) = self.expr(*e)?;
            if let Some((_, pt)) = rest.get(i) {
                self.expect(t, inst(*pt), *e, "argument");
            }
            refs.push(r);
        }
        let result = inst(sig.ret);
        let c = Callee::TraitMethod {
            trait_,
            method,
            self_ty: rt,
            targs: TyList::EMPTY,
            choice,
        };
        Ok((
            self.b.call(&c, &refs, Providers::None, result, n.index()),
            result,
        ))
    }

    /// Resolves every type the body recorded; integer literals with no
    /// other constraint are `i32` (type-checking.md §3.6).
    fn zonk(&mut self, t: Ty) -> Ty {
        let pool = self.cx.names.pool;
        let r = self.infer.resolve(pool, t);
        if !pool.has_infer(r) {
            return r;
        }
        if let TyData::Infer(_) = pool.get(r) {
            if self.infer.unify(pool, r, Ty::I32).is_ok() {
                return Ty::I32;
            }
            return Ty::POISON;
        }
        let d = match pool.get(r) {
            TyData::Adt { def, args } => {
                let items: Vec<Ty> = pool
                    .list_items(args)
                    .into_iter()
                    .map(|x| self.zonk(x))
                    .collect();
                TyData::Adt {
                    def,
                    args: pool.list(&items),
                }
            }
            _ => return Ty::POISON,
        };
        pool.intern_ty(&d)
    }

    fn finish(mut self, root: Ref) -> StageResult<Body> {
        let pool = self.cx.names.pool;
        // Literal kinds first, so `x := +0` then `x = y` resolves both.
        let n = self.b.body_mut().ty.len();
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
        for i in 0..self.b.body_mut().consts.len() {
            let t = self.b.body_mut().consts[i].0;
            let z = self.zonk(t);
            self.b.body_mut().consts[i].0 = z;
        }
        for i in 0..n {
            if self.b.body_mut().tags[i] != Tag::Call {
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
                Callee::Item { def, targs } => {
                    let items: Vec<Ty> = pool
                        .list_items(targs)
                        .into_iter()
                        .map(|x| self.zonk(x))
                        .collect();
                    Callee::Item {
                        def,
                        targs: pool.list(&items),
                    }
                }
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
                    targs,
                    choice,
                },
            };
            let nw = z.words();
            self.b.body_mut().extra[at..at + nw.len()].copy_from_slice(&nw);
        }
        self.b
            .finish(root, &[])
            .map_err(|e| NotImplemented::new(Stage::Body, format!("TIR verifier: {e:?}")))
    }
}
