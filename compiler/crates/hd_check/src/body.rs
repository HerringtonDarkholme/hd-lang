//! Body checking that emits TIR (checking-and-tir.md §4.13), for the subset.

use std::collections::{BTreeMap, HashMap};

use hd_base::Hash128;
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};

use crate::A1Rule;
use crate::resolve::{Cst, Scope};
use hd_tir::world::{DefId, DefKind, FnSig, Ty, TyKind, World};
use hd_tir::{
    CALLEE_ITEM, CALLEE_TRAIT_METHOD, CHOICE_BOUND, CHOICE_IMPL, CONST_BIT, INTRINSIC_PRINTLN_I32,
    Inst, LOCAL_PARAM, NONE, PrimOp, TirBody, TirTag,
};

pub struct Checked {
    pub body: TirBody,
    pub errors: Vec<String>,
    /// `deps`: (stable path, per-item interface hash), sorted by path.
    pub deps: Vec<(String, Hash128)>,
}

struct Ck<'a, 'w> {
    w: &'w mut World,
    cst: &'a Cst<'a>,
    scope: &'a Scope,
    b: TirBody,
    lists: Vec<Vec<u32>>,
    locals: Vec<HashMap<String, u32>>,
    bounds: Vec<Option<DefId>>,
    ret: Ty,
    loops: Vec<u32>,
    errors: Vec<String>,
    deps: BTreeMap<String, Hash128>,
}

pub fn check_fn(
    w: &mut World,
    cst: &Cst<'_>,
    scope: &Scope,
    def: DefId,
    f: NodeRef<'_>,
    rule: A1Rule,
) -> Checked {
    let sig: FnSig = match w.defs.get(&def) {
        Some(DefKind::Fn(s) | DefKind::ImplMethod { sig: s, .. }) => s.clone(),
        _ => panic!("check_fn on a non-function"),
    };
    let mut ck = Ck {
        w,
        cst,
        scope,
        b: TirBody {
            item: Some(def),
            ..TirBody::default()
        },
        lists: vec![Vec::new()],
        locals: vec![HashMap::new()],
        bounds: sig.generics.iter().map(|g| g.1).collect(),
        ret: sig.ret,
        loops: Vec::new(),
        errors: Vec::new(),
        deps: BTreeMap::new(),
    };
    for (name, ty) in &sig.params {
        let text = ck.w.text(*name).to_owned();
        let l = ck.b.local(*name, *ty, LOCAL_PARAM);
        ck.locals[0].insert(text, l);
    }
    for (_, bound) in &sig.generics {
        if let Some(t) = bound {
            ck.dep(*t);
        }
    }
    let body = Cst::child(f, SyntaxKind::Block).expect("fn body");
    let ret_void = matches!(ck.w.kind(sig.ret), TyKind::Void);
    let root = ck.block(body, !ret_void);
    ck.b.sub_root.push(root);
    ck.b.rep_exact = rep_summary(ck.w, &ck.b, &sig, rule);
    Checked {
        body: ck.b,
        errors: ck.errors,
        deps: ck.deps.into_iter().collect(),
    }
}

impl Ck<'_, '_> {
    fn ty(&mut self, k: TyKind) -> Ty {
        self.w.ty(k)
    }
    fn dep(&mut self, d: DefId) {
        let h = self.w.item_hash.get(&d).copied().unwrap_or_default();
        self.deps.insert(self.w.path(d).to_owned(), h);
    }
    fn emit(&mut self, tag: TirTag, a: u32, b: u32, ty: Ty, n: NodeRef<'_>) -> u32 {
        let i = self.b.push(tag, a, b, ty, self.cst.span(n));
        self.lists.last_mut().expect("block").push(i.0);
        i.0
    }
    /// Checks a `Block` node; the block instruction is owned by its parent.
    fn block(&mut self, n: NodeRef<'_>, want_tail: bool) -> Inst {
        self.lists.push(Vec::new());
        self.locals.push(HashMap::new());
        let stmts: Vec<_> = n.children().collect();
        let mut tail = NONE;
        for (i, s) in stmts.iter().enumerate() {
            if want_tail && i + 1 == stmts.len() && s.kind() == SyntaxKind::ExprStmt {
                let e = s.children().next().expect("expr");
                let expected = self.ret;
                let (r, t) = self.expr(e, Some(expected));
                self.expect_ty(t, expected, "result");
                tail = r;
            } else {
                self.stmt(*s);
            }
        }
        self.locals.pop();
        let items = self.lists.pop().expect("block");
        let list = self.b.list(&items);
        let void = self.ty(TyKind::Void);
        self.b
            .push(TirTag::Block, list, tail, void, self.cst.span(n))
    }

    fn stmt(&mut self, s: NodeRef<'_>) {
        let void = self.ty(TyKind::Void);
        let never = self.ty(TyKind::Never);
        let kids: Vec<_> = s.children().collect();
        match s.kind() {
            SyntaxKind::LetStmt => {
                let name = self.cst.text(self.cst.first(kids[0])).to_owned();
                let (r, t) = self.expr(kids[1], None);
                let sym = self.w.sym(&name);
                let l = self.b.local(sym, t, 0);
                self.locals.last_mut().expect("scope").insert(name, l);
                self.emit(TirTag::LocalSet, l, r, void, s);
            }
            SyntaxKind::AssignmentStmt => {
                let name = self.cst.text(self.cst.first(kids[0])).to_owned();
                let Some(l) = self.find_local(&name) else {
                    self.errors.push(format!("unknown-name `{name}`"));
                    return;
                };
                let lt = self.b.local_ty[l as usize];
                let (r, t) = self.expr(kids[1], Some(lt));
                self.expect_ty(t, lt, "assignment");
                self.emit(TirTag::LocalSet, l, r, void, s);
            }
            SyntaxKind::ReturnStmt => {
                let r = if let Some(e) = kids.first() {
                    let ret = self.ret;
                    let (r, t) = self.expr(*e, Some(ret));
                    self.expect_ty(t, ret, "return");
                    r
                } else {
                    NONE
                };
                self.emit(TirTag::Return, r, NONE, never, s);
            }
            SyntaxKind::ExprStmt => {
                let _ = self.expr(kids[0], None);
            }
            SyntaxKind::IfExpr => {
                let bool_ = self.ty(TyKind::Bool);
                let (c, t) = self.expr(kids[0], Some(bool_));
                self.expect_ty(t, bool_, "condition");
                let then = self.block(kids[1], false);
                let els = kids.get(2).map_or(NONE, |e| self.block(*e, false).0);
                let rec = self.b.list(&[then.0, els]);
                self.emit(TirTag::If, c, rec, void, s);
            }
            SyntaxKind::WhileExpr => {
                // while c: body  ==>  Loop { If c { body } else { Break } }
                let label = u32::try_from(self.b.label_inst.len()).expect("labels");
                self.b.label_inst.push(Inst(NONE));
                self.loops.push(label);
                self.lists.push(Vec::new());
                let bool_ = self.ty(TyKind::Bool);
                let (c, t) = self.expr(kids[0], Some(bool_));
                self.expect_ty(t, bool_, "condition");
                let then = self.block(kids[1], false);
                self.lists.push(Vec::new());
                self.emit(TirTag::Break, label, NONE, never, s);
                let items = self.lists.pop().expect("else");
                let list = self.b.list(&items);
                let els = self
                    .b
                    .push(TirTag::Block, list, NONE, void, self.cst.span(s));
                let rec = self.b.list(&[then.0, els.0]);
                self.emit(TirTag::If, c, rec, void, s);
                let items = self.lists.pop().expect("loop body");
                let list = self.b.list(&items);
                let body = self
                    .b
                    .push(TirTag::Block, list, NONE, void, self.cst.span(s));
                let lp = self.emit(TirTag::Loop, body.0, NONE, void, s);
                self.b.label_inst[label as usize] = Inst(lp);
                self.loops.pop();
            }
            other => self
                .errors
                .push(format!("statement {other:?} is outside the subset")),
        }
    }

    fn find_local(&self, name: &str) -> Option<u32> {
        self.locals.iter().rev().find_map(|s| s.get(name).copied())
    }

    fn expect_ty(&mut self, got: Ty, want: Ty, what: &str) {
        let never = self.ty(TyKind::Never);
        if got != want && got != never {
            let msg = format!(
                "type-mismatch in {what}: expected {}, found {}",
                self.w.display(want),
                self.w.display(got)
            );
            self.errors.push(msg);
        }
    }

    fn konst(&mut self, ty: Ty, bits: u64) -> u32 {
        self.w.konst(ty, bits) | CONST_BIT
    }

    #[expect(
        clippy::only_used_in_recursion,
        reason = "the expected-type hint is threaded for bidirectional checking; the subset does not read it yet"
    )]
    fn expr(&mut self, n: NodeRef<'_>, expected: Option<Ty>) -> (u32, Ty) {
        let i32_ = self.ty(TyKind::I32);
        let bool_ = self.ty(TyKind::Bool);
        let kids: Vec<_> = n.children().collect();
        match n.kind() {
            SyntaxKind::LiteralExpr => {
                let t = self.cst.first(n);
                match self.cst.tkind(t) {
                    TokenKind::KwTrue => (self.konst(bool_, 1), bool_),
                    TokenKind::KwFalse => (self.konst(bool_, 0), bool_),
                    _ => {
                        let text = self.cst.text(t).replace('_', "");
                        let v: i64 = text.parse().unwrap_or_else(|_| {
                            self.errors.push(format!("bad literal `{text}`"));
                            0
                        });
                        // The low 32 bits: an i32 constant's bit pattern.
                        let bits = u32::try_from(v & 0xffff_ffff).expect("32 bits");
                        (self.konst(i32_, u64::from(bits)), i32_)
                    }
                }
            }
            SyntaxKind::UnaryExpr => {
                let op = self.cst.tkind(self.cst.first(n));
                let (r, t) = self.expr(kids[0], expected);
                if op == TokenKind::Plus {
                    return (r, t);
                }
                if r & CONST_BIT != 0 && r != NONE {
                    let (ct, bits) = self.w.const_value(r & !CONST_BIT);
                    let neg = u32::try_from(bits)
                        .expect("i32 bits")
                        .cast_signed()
                        .wrapping_neg();
                    return (self.konst(ct, u64::from(neg.cast_unsigned())), t);
                }
                let list = self.b.list(&[r]);
                (self.emit(TirTag::Prim, PrimOp::Neg as u32, list, t, n), t)
            }
            SyntaxKind::TupleExpr => self.expr(kids[0], expected),
            SyntaxKind::NameExpr => {
                let name = self.cst.text(self.cst.first(n));
                if let Some(l) = self.find_local(name) {
                    let t = self.b.local_ty[l as usize];
                    (self.emit(TirTag::LocalGet, l, NONE, t, n), t)
                } else {
                    self.errors.push(format!("unknown-name `{name}`"));
                    (NONE, self.ty(TyKind::Never))
                }
            }
            SyntaxKind::AdditiveExpr
            | SyntaxKind::MultiplicativeExpr
            | SyntaxKind::ComparisonExpr
            | SyntaxKind::LogicalAndExpr
            | SyntaxKind::LogicalOrExpr => {
                let op_tok = Cst::next(self.cst.last(kids[0]));
                let op = match self.cst.tkind(op_tok) {
                    TokenKind::Plus => PrimOp::Add,
                    TokenKind::Minus => PrimOp::Sub,
                    TokenKind::Star => PrimOp::Mul,
                    TokenKind::Slash => PrimOp::Div,
                    TokenKind::Percent => PrimOp::Rem,
                    TokenKind::EqEq => PrimOp::Eq,
                    TokenKind::NotEq => PrimOp::Ne,
                    TokenKind::Lt => PrimOp::Lt,
                    TokenKind::LtEq => PrimOp::Le,
                    TokenKind::Gt => PrimOp::Gt,
                    TokenKind::GtEq => PrimOp::Ge,
                    TokenKind::AndAnd => PrimOp::And,
                    _ => PrimOp::Or,
                };
                let operand = if matches!(op, PrimOp::And | PrimOp::Or) {
                    bool_
                } else {
                    i32_
                };
                let (l, lt) = self.expr(kids[0], Some(operand));
                let (r, rt) = self.expr(kids[1], Some(operand));
                self.expect_ty(lt, operand, "operand");
                self.expect_ty(rt, operand, "operand");
                let result = if matches!(
                    op,
                    PrimOp::Add | PrimOp::Sub | PrimOp::Mul | PrimOp::Div | PrimOp::Rem
                ) {
                    i32_
                } else {
                    bool_
                };
                let list = self.b.list(&[l, r]);
                (self.emit(TirTag::Prim, op as u32, list, result, n), result)
            }
            SyntaxKind::FieldExpr => {
                let (base, bt) = self.expr(kids[0], None);
                let name = self.cst.text(self.cst.last(n)).to_owned();
                let TyKind::Adt(d) = *self.w.kind(bt) else {
                    self.errors
                        .push(format!("no-field `{name}` on {}", self.w.display(bt)));
                    return (NONE, self.ty(TyKind::Never));
                };
                self.dep(d);
                let Some(DefKind::Data(fields)) = self.w.defs.get(&d) else {
                    return (NONE, self.ty(TyKind::Never));
                };
                let found = fields.iter().position(|(f, _)| self.w.text(*f) == name);
                let Some(idx) = found else {
                    self.errors.push(format!("no-field `{name}`"));
                    return (NONE, self.ty(TyKind::Never));
                };
                let ft = fields[idx].1;
                (
                    self.emit(TirTag::Field, base, u32::try_from(idx).expect("f"), ft, n),
                    ft,
                )
            }
            SyntaxKind::DataExpr => {
                let name = self.cst.text(self.cst.first(n)).to_owned();
                let Some(d) = self.scope.names.get(&name).and_then(|p| self.w.lookup(p)) else {
                    self.errors.push(format!("unknown-type `{name}`"));
                    return (NONE, self.ty(TyKind::Never));
                };
                self.dep(d);
                let Some(DefKind::Data(fields)) = self.w.defs.get(&d).cloned() else {
                    self.errors.push(format!("`{name}` is not data"));
                    return (NONE, self.ty(TyKind::Never));
                };
                let mut vals = vec![NONE; fields.len()];
                for a in kids.iter().skip(1) {
                    let fname = self.cst.text(self.cst.first(*a)).to_owned();
                    let Some(idx) = fields.iter().position(|(f, _)| self.w.text(*f) == fname)
                    else {
                        self.errors.push(format!("no-field `{fname}`"));
                        continue;
                    };
                    let e = a.children().next().expect("value");
                    let (r, t) = self.expr(e, Some(fields[idx].1));
                    self.expect_ty(t, fields[idx].1, "field");
                    vals[idx] = r;
                }
                if vals.contains(&NONE) {
                    self.errors.push(format!("missing-field in `{name}`"));
                }
                let ty = self.ty(TyKind::Adt(d));
                let list = self.b.list(&vals);
                (self.emit(TirTag::NewData, NONE, list, ty, n), ty)
            }
            SyntaxKind::CallExpr => self.call(n, &kids),
            other => {
                self.errors
                    .push(format!("expression {other:?} is outside the subset"));
                (NONE, self.ty(TyKind::Never))
            }
        }
    }

    fn args(&mut self, al: NodeRef<'_>, params: &[Ty]) -> Vec<(u32, Ty)> {
        al.children()
            .enumerate()
            .map(|(i, a)| {
                let e = a.children().next().expect("arg");
                let p = params.get(i).copied();
                let hint = p.filter(|t| !matches!(self.w.kind(*t), TyKind::Param(_)));
                self.expr(e, hint)
            })
            .collect()
    }

    fn call(&mut self, n: NodeRef<'_>, kids: &[NodeRef<'_>]) -> (u32, Ty) {
        let void = self.ty(TyKind::Void);
        let callee = kids[0];
        let al = kids[1];
        if callee.kind() == SyntaxKind::FieldExpr {
            return self.method_call(n, callee, al);
        }
        let name = self.cst.text(self.cst.first(callee)).to_owned();
        if name == "println" && !self.scope.names.contains_key("println") {
            let i32_ = self.ty(TyKind::I32);
            let args = self.args(al, &[i32_]);
            if args.len() != 1 {
                self.errors.push("println takes one i32".into());
            }
            for (_, t) in &args {
                self.expect_ty(*t, i32_, "println argument");
            }
            let refs: Vec<u32> = args.iter().map(|a| a.0).collect();
            let list = self.b.list(&refs);
            return (
                self.emit(TirTag::Intrinsic, INTRINSIC_PRINTLN_I32, list, void, n),
                void,
            );
        }
        let Some(d) = self.scope.names.get(&name).and_then(|p| self.w.lookup(p)) else {
            self.errors.push(format!("unknown-name `{name}`"));
            return (NONE, self.ty(TyKind::Never));
        };
        let Some(DefKind::Fn(sig)) = self.w.defs.get(&d).cloned() else {
            self.errors.push(format!("`{name}` is not a function"));
            return (NONE, self.ty(TyKind::Never));
        };
        self.dep(d);
        let ptys: Vec<Ty> = sig.params.iter().map(|p| p.1).collect();
        let args = self.args(al, &ptys);
        if args.len() != ptys.len() {
            self.errors
                .push(format!("arity: `{name}` takes {} arguments", ptys.len()));
        }
        // Use-site type argument inference: first-order matching.
        let mut targs: Vec<Option<Ty>> = vec![None; sig.generics.len()];
        for (p, (_, at)) in ptys.iter().zip(&args) {
            if let TyKind::Param(i) = *self.w.kind(*p) {
                let slot = &mut targs[i as usize];
                match slot {
                    None => *slot = Some(*at),
                    Some(prev) if prev != at => {
                        self.errors
                            .push("type-mismatch: conflicting type arguments".into());
                    }
                    _ => {}
                }
            }
        }
        let never = self.ty(TyKind::Never);
        let targs: Vec<Ty> = targs.into_iter().map(|t| t.unwrap_or(never)).collect();
        // Bounds: each bounded type argument must have an impl (static check).
        for (i, (_, bound)) in sig.generics.iter().enumerate() {
            if let Some(tr) = bound
                && self.find_impl(*tr, targs[i]).is_none()
                && !matches!(self.w.kind(targs[i]), TyKind::Param(_))
            {
                let msg = format!(
                    "unsatisfied-bound: {} < {}",
                    self.w.display(targs[i]),
                    self.w.path(*tr)
                );
                self.errors.push(msg);
            }
        }
        for (p, (_, at)) in ptys.iter().zip(&args) {
            let want = self.w.subst(*p, &targs, None);
            self.expect_ty(*at, want, "argument");
        }
        let ret = self.w.subst(sig.ret, &targs, None);
        let mut rec = vec![CALLEE_ITEM, d.0];
        rec.extend(targs.iter().map(|t| t.0));
        let a = self.b.list(&rec);
        let refs: Vec<u32> = args.iter().map(|x| x.0).collect();
        let b = self.b.list(&refs);
        (self.emit(TirTag::Call, a, b, ret, n), ret)
    }

    fn find_impl(&self, tr: DefId, target: Ty) -> Option<DefId> {
        self.w.impls.get(&tr)?.iter().copied().find(
            |i| matches!(self.w.defs.get(i), Some(DefKind::Impl { target: t, .. }) if *t == target),
        )
    }

    fn method_call(&mut self, n: NodeRef<'_>, fe: NodeRef<'_>, al: NodeRef<'_>) -> (u32, Ty) {
        let recv_node = fe.children().next().expect("receiver");
        let mname = self.cst.text(self.cst.last(fe)).to_owned();
        let (recv, rt) = self.expr(recv_node, None);
        // Candidate traits: the bound of a parameter, or every trait with an
        // impl for the receiver's type.
        let (trait_, choice, choice_val) = if let TyKind::Param(i) = *self.w.kind(rt) {
            if let Some(tr) = self.bounds.get(i as usize).copied().flatten() {
                (tr, CHOICE_BOUND, i)
            } else {
                self.errors
                    .push(format!("no-method `{mname}` on an unbounded parameter"));
                return (NONE, self.ty(TyKind::Never));
            }
        } else {
            let mut found = None;
            let mut traits: Vec<DefId> = self.w.impls.keys().copied().collect();
            traits.sort();
            for tr in traits {
                let has = matches!(self.w.defs.get(&tr), Some(DefKind::Trait(ms)) if ms.iter().any(|(m, _)| self.w.text(*m) == mname));
                if has && let Some(imp) = self.find_impl(tr, rt) {
                    found = Some((tr, CHOICE_IMPL, imp.0));
                }
            }
            let Some(f) = found else {
                self.errors
                    .push(format!("no-method `{mname}` on {}", self.w.display(rt)));
                return (NONE, self.ty(TyKind::Never));
            };
            self.dep(DefId(f.2));
            f
        };
        self.dep(trait_);
        let Some(DefKind::Trait(ms)) = self.w.defs.get(&trait_).cloned() else {
            return (NONE, self.ty(TyKind::Never));
        };
        let Some(index) = ms.iter().position(|(m, _)| self.w.text(*m) == mname) else {
            self.errors.push(format!("no-method `{mname}`"));
            return (NONE, self.ty(TyKind::Never));
        };
        let sig = ms[index].1.clone();
        let rest: Vec<Ty> = sig
            .params
            .iter()
            .skip(1)
            .map(|p| self.w.subst(p.1, &[], Some(rt)))
            .collect();
        let args = self.args(al, &rest);
        for (want, (_, got)) in rest.iter().zip(&args) {
            self.expect_ty(*got, *want, "argument");
        }
        let out_ty = self.w.subst(sig.ret, &[], Some(rt));
        let rec = [
            CALLEE_TRAIT_METHOD,
            trait_.0,
            u32::try_from(index).expect("i"),
            rt.0,
            choice,
            choice_val,
        ];
        let a = self.b.list(&rec);
        let mut refs = vec![recv];
        refs.extend(args.iter().map(|x| x.0));
        let b = self.b.list(&refs);
        (self.emit(TirTag::Call, a, b, out_ty, n), out_ty)
    }
}

/// The representation summary (codegen.md §13.2, A1): per type parameter,
/// 1 when instances need the exact representation, 0 when the parameter may
/// share the `REF` class.
///
/// `A1Rule::Bounded` (walking skeleton, SK-2): a parameter with a bound is
/// always exact, and only an unbounded parameter may share `REF`. The summary
/// is then a property of the signature, which the interface hash covers.
///
/// `A1Rule::Literal`: the rule as first written, read from the body alone:
/// exact only when the body makes a trait call on the parameter. It is unsound
/// when a bounded parameter is passed on to a bounded callee; a test keeps
/// that reproduction.
fn rep_summary(w: &World, b: &TirBody, sig: &FnSig, rule: A1Rule) -> Vec<u8> {
    match rule {
        A1Rule::Bounded => sig
            .generics
            .iter()
            .map(|(_, bound)| u8::from(bound.is_some()))
            .collect(),
        A1Rule::Literal => {
            let mut exact = vec![0; sig.generics.len()];
            for i in 0..b.tags.len() {
                if b.tags[i] == TirTag::Call {
                    let rec = b.get_list(b.data[i][0]);
                    if rec[0] == CALLEE_TRAIT_METHOD
                        && let TyKind::Param(p) = *w.kind(Ty(rec[3]))
                    {
                        exact[p as usize] = 1;
                    }
                }
            }
            exact
        }
    }
}
