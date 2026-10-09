//! Patterns, `let`, `match` and exhaustiveness (type-checking.md §6;
//! checking-and-tir.md "Matching"). The checker declares each arm's
//! bindings, emits the arm blocks, then a decision block of `SwitchTag`,
//! `If`, `Payload`, `Unwrap`, `Guard` and `ToArm` that tries the rows in
//! order. A failed test continues with the next row; the next row's tests
//! are emitted again on each failing path, which keeps every block owned
//! by exactly one instruction. Exhaustiveness is Maranget's usefulness
//! test over the arm patterns.

use std::collections::HashMap;

use hd_base::{LocalId, NodeIdx, StageResult};
use hd_diag::Code;
use hd_resolve::{ItemData, Src};
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_tir::ir::{IntrinsicOp, NONE, PrimOp, Ref, Tag, TirSink};
use hd_types::{Prim, Ty, TyData, VarKind};

use crate::body::{Ck, unsupported};
use crate::call::subst_owner;

/// One row of a decision: its pattern (none: matches anything), whether
/// the pattern is under `.Some`, its binding locals, guard and arm.
pub(crate) struct Row<'t> {
    pub pat: Option<NodeRef<'t>>,
    pub wrap_some: bool,
    pub binds: Vec<(NodeIdx, LocalId)>,
    pub guard: Option<Ref>,
    pub arm: u32,
}

impl<'t> Row<'t> {
    pub(crate) fn some(pat: NodeRef<'t>, binds: Vec<(NodeIdx, LocalId)>) -> Self {
        Row {
            pat: Some(pat),
            wrap_some: true,
            binds,
            guard: None,
            arm: 0,
        }
    }
    pub(crate) fn rest() -> Self {
        Row {
            pat: None,
            wrap_some: false,
            binds: vec![],
            guard: None,
            arm: 1,
        }
    }
}

/// A pattern for the exhaustiveness test.
#[derive(Clone, Debug)]
enum P {
    Wild,
    Ctor(String, Vec<P>),
    Lit,
}

impl Ck<'_, '_> {
    /// Declares the bindings of a pattern against a type, checking the
    /// pattern's shape. `binds` maps binding nodes to their locals.
    pub(crate) fn declare_pattern(
        &mut self,
        p: NodeRef<'_>,
        t: Ty,
        binds: &mut Vec<(NodeIdx, LocalId)>,
    ) -> StageResult<()> {
        let pool = self.pool();
        let tr = self.infer.resolve(pool, t);
        let inner = match pool.get(tr) {
            TyData::Mut(i) => i,
            _ => tr,
        };
        match p.kind() {
            SyntaxKind::WildcardPattern => {}
            SyntaxKind::BindingPattern => {
                let name_tok = p.direct_tokens().find(|t| {
                    matches!(
                        self.cx.src.tkind(*t),
                        Some(TokenKind::Ident | TokenKind::RawIdent)
                    )
                });
                let Some(nt) = name_tok else {
                    return unsupported("a binding pattern without a name");
                };
                let name = self.cx.names.syms.intern(self.cx.src.text(nt));
                // `flow.match.bare-variant`: a bare name of a variant of
                // the subject's enum is not a catch-all binding.
                if self.variant_fields(t, self.cx.src.text(nt)).is_some() {
                    let msg = format!("write `.{}` or a qualified name", self.cx.src.text(nt));
                    self.err(Code::BareVariantPattern, p, &msg);
                    return Ok(());
                }
                if binds
                    .iter()
                    .any(|(_, l)| self.b.body_mut().local_name[l.idx()] == name)
                {
                    let msg = format!(
                        "`{}` is bound twice in one pattern",
                        self.cx.names.text(name)
                    );
                    self.err(Code::DuplicateBinding, p, &msg);
                }
                if let Some(sub) = p.children().next() {
                    // `name @ pattern`-like forms.
                    let _ = sub;
                    return unsupported("a binding with a sub-pattern");
                }
                // A `let` pattern's names follow the binding forms
                // (types.bind.let-pattern-mut, types.bind.let-mut-pattern.annotated).
                let t = match self.let_view {
                    Some(annotated)
                        if p.direct_tokens()
                            .any(|t| self.cx.src.tkind(t) == Some(TokenKind::KwMut)) =>
                    {
                        self.let_mut_name(t, annotated, p);
                        t
                    }
                    Some(false) => self.readonly_view(t),
                    _ => t,
                };
                let l = self.bind_local(name, t, p);
                binds.push((p.index(), l));
            }
            SyntaxKind::LiteralPattern => {
                let (_, lt) = self.pattern_literal(p, Some(t))?;
                self.expect(lt, t, p, "pattern");
            }
            SyntaxKind::RangePattern => {
                for c in p.children() {
                    let (_, lt) = self.pattern_literal(c, Some(t))?;
                    self.expect(lt, t, c, "pattern");
                }
            }
            SyntaxKind::TuplePattern if p.children().next().is_none() => {
                self.expect(t, Ty::VOID, p, "pattern");
            }
            SyntaxKind::TuplePattern => {
                let subs: Vec<NodeRef<'_>> = p.children().collect();
                if subs.iter().any(|s| s.kind() == SyntaxKind::SpreadPattern) {
                    return unsupported("a tuple pattern with a rest");
                }
                let elems = match pool.get(inner) {
                    TyData::Tuple { elems, .. } => pool.list_items(elems).to_vec(),
                    TyData::Infer(_) => {
                        let vs: Vec<Ty> = subs
                            .iter()
                            .map(|_| self.infer.fresh(pool, VarKind::General))
                            .collect();
                        let tt = pool.intern_ty(&TyData::Tuple {
                            elems: pool.list(&vs),
                            rest: None,
                        });
                        self.expect(tt, t, p, "pattern");
                        vs
                    }
                    _ => {
                        let msg = format!("in pattern: {} is not a tuple", self.show(t));
                        self.err(Code::TypeMismatch, p, &msg);
                        return Ok(());
                    }
                };
                if elems.len() != subs.len() {
                    self.err(Code::PatternArity, p, "the tuple has another size");
                    return Ok(());
                }
                for (s, et) in subs.iter().zip(elems) {
                    self.declare_pattern(*s, et, binds)?;
                }
            }
            SyntaxKind::VariantPattern => {
                let name = self.variant_pattern_name(p);
                // `flow.match.bare-payload`: `Some(x)` for `.Some(x)`.
                if !p
                    .direct_tokens()
                    .any(|t| self.cx.src.tkind(t) == Some(TokenKind::Dot))
                {
                    let msg = format!("write `.{name}` or a qualified name");
                    self.err(Code::BareVariantPattern, p, &msg);
                    return Ok(());
                }
                let Some((_, fields)) = self.variant_fields(t, &name) else {
                    if matches!(pool.get(inner), TyData::Infer(_)) {
                        return unsupported("a variant pattern on a value whose type is not known");
                    }
                    let msg = format!("no variant `.{name}` of {}", self.show(t));
                    self.err(Code::UnknownVariant, p, &msg);
                    return Ok(());
                };
                let subs = self.variant_subpatterns(p, t, &name)?;
                if subs.len() != fields.len() {
                    self.err(
                        Code::PatternArity,
                        p,
                        "the variant has another number of fields",
                    );
                    return Ok(());
                }
                for (s, ft) in subs.iter().zip(fields) {
                    if let Some(s) = s {
                        self.declare_pattern(*s, ft, binds)?;
                    }
                }
            }
            SyntaxKind::DataPattern => {
                let mut seen: Vec<String> = Vec::new();
                for f in p
                    .children()
                    .filter(|c| c.kind() == SyntaxKind::DataPatternField)
                {
                    let fname = self.cx.src.text(self.cx.src.first(f)).to_owned();
                    if seen.contains(&fname) {
                        let msg = format!("`{fname}` appears twice in this pattern");
                        self.err(Code::DuplicateDataPatternField, f, &msg);
                        continue;
                    }
                    seen.push(fname.clone());
                    let Some((_, ft)) = self.field_of(t, &fname) else {
                        let msg = format!("no field `{fname}` on {}", self.show(t));
                        self.err(Code::UnknownDataField, f, &msg);
                        continue;
                    };
                    self.check_field_visible(t, &fname, f);
                    if let Some(sub) = f.children().next() {
                        self.declare_pattern(sub, ft, binds)?;
                    } else {
                        let name = self.cx.names.syms.intern(&fname);
                        let l = self.bind_local(name, ft, f);
                        binds.push((f.index(), l));
                    }
                }
            }
            other => return unsupported(format!("the pattern {other:?}")),
        }
        Ok(())
    }

    fn variant_pattern_name(&self, p: NodeRef<'_>) -> String {
        p.direct_tokens()
            .filter(|t| {
                matches!(
                    self.cx.src.tkind(*t),
                    Some(TokenKind::Ident | TokenKind::RawIdent)
                )
            })
            .last()
            .map(|t| self.cx.src.text(t).to_owned())
            .unwrap_or_default()
    }

    /// The sub-patterns of a variant pattern, by field position (`None`
    /// for an omitted named field).
    fn variant_subpatterns<'t>(
        &self,
        p: NodeRef<'t>,
        t: Ty,
        name: &str,
    ) -> StageResult<Vec<Option<NodeRef<'t>>>> {
        let Some(al) = Src::child(p, SyntaxKind::PatternArgumentList) else {
            return Ok(vec![]);
        };
        let kids: Vec<NodeRef<'t>> = al.children().collect();
        if kids.iter().any(|k| k.kind() == SyntaxKind::SpreadPattern) {
            return unsupported("a variant pattern with a rest");
        }
        if kids.iter().all(|k| k.kind() != SyntaxKind::NamedPattern) {
            return Ok(kids.into_iter().map(Some).collect());
        }
        // Named fields: by the variant's field names.
        let pool = self.pool();
        let tt = self.infer.resolve(pool, t);
        let tt = match pool.get(tt) {
            TyData::Mut(i) => i,
            _ => tt,
        };
        let TyData::Adt { def, .. } = pool.get(tt) else {
            return unsupported("named fields in an optional pattern");
        };
        let Some(ItemData::Enum { variants, .. }) = self.cx.lookup.item(def).map(|i| &i.data)
        else {
            return unsupported("named fields of a non-enum");
        };
        let sym = self.cx.names.syms.intern(name);
        let Some(v) = variants.iter().find(|v| v.name == sym) else {
            return Ok(vec![]);
        };
        let mut out = vec![None; v.fields.len()];
        for k in kids {
            if k.kind() != SyntaxKind::NamedPattern {
                return unsupported("mixed positional and named variant fields");
            }
            let fname = self
                .cx
                .names
                .syms
                .intern(self.cx.src.text(self.cx.src.first(k)));
            if let Some(i) = v.fields.iter().position(|f| f.name == fname) {
                out[i] = k.children().next().or(Some(k));
            }
        }
        Ok(out)
    }

    /// A literal pattern's value.
    fn pattern_literal(&mut self, p: NodeRef<'_>, want: Option<Ty>) -> StageResult<(Ref, Ty)> {
        match p.kind() {
            SyntaxKind::LiteralPattern => {
                let neg = self.cx.src.tkind(self.cx.src.first(p)) == Some(TokenKind::Minus);
                let inner = p.children().next();
                let (r, t) = if let Some(e) = inner {
                    self.expr(e, want)?
                } else {
                    let tok = self.cx.src.first(p);
                    let tok = if neg {
                        hd_base::TokenIdx::from_raw(tok.raw() + 1)
                    } else {
                        tok
                    };
                    match self.cx.src.tkind(tok) {
                        Some(TokenKind::String) => (self.string_expr(p)?, Ty::STRING),
                        Some(TokenKind::StrHead) => {
                            return unsupported("an interpolated string pattern");
                        }
                        _ => self.literal_at(tok)?,
                    }
                };
                // A literal pattern takes the scrutinee's type.
                if let Some(w) = want
                    && self.b.const_of(r).is_some()
                    && self
                        .infer
                        .kind_of(self.pool(), t)
                        .is_some_and(|k| k != VarKind::General)
                {
                    let pool = self.pool();
                    let _ = self.infer.unify(pool, t, w);
                }
                if neg && let Some((ct, bits)) = self.b.const_of(r) {
                    return Ok((
                        self.b
                            .const_value(ct, bits.cast_signed().wrapping_neg().cast_unsigned()),
                        t,
                    ));
                }
                Ok((r, t))
            }
            _ => self.expr(p, want),
        }
    }

    // ------------------------------------------------------------ let

    pub(crate) fn let_stmt(&mut self, s: NodeRef<'_>, kids: &[NodeRef<'_>]) -> StageResult<()> {
        let Some(pat) = kids.first().copied() else {
            return unsupported("a `let` without a pattern");
        };
        let ty_node = kids.iter().copied().find(|k| k.kind().is_type());
        let els = Src::child(s, SyntaxKind::ElseClause);
        let Some(rhs) = kids
            .iter()
            .copied()
            .skip(1)
            .find(|k| !k.kind().is_type() && k.kind() != SyntaxKind::ElseClause)
        else {
            return unsupported("a `let` without a value");
        };
        self.let_pattern(pat, ty_node, rhs, els, s)
    }

    /// `let p[: T] = e [else: ...]` and `p := e`.
    pub(crate) fn let_pattern(
        &mut self,
        pat: NodeRef<'_>,
        ty_node: Option<NodeRef<'_>>,
        rhs: NodeRef<'_>,
        els: Option<NodeRef<'_>>,
        at: NodeRef<'_>,
    ) -> StageResult<()> {
        let pool = self.pool();
        let annot = match ty_node {
            Some(t) => Some(self.ty_node(t)?),
            None => None,
        };
        let (r, t) = self.expr(rhs, annot)?;
        let (r, t) = match annot {
            Some(a) => (self.coerce(r, t, a, rhs, "binding"), a),
            None => (r, t),
        };
        // `let mut` on a primitive or a tuple (types.bind.let-mut-primitive,
        // types.bind.let-mut-tuple).
        let mut t = t;
        if pat.kind() == SyntaxKind::BindingPattern
            && pat
                .direct_tokens()
                .any(|t| self.cx.src.tkind(t) == Some(TokenKind::KwMut))
        {
            let lit = matches!(
                self.infer.kind_of(pool, t),
                Some(VarKind::IntLit | VarKind::SignedIntLit | VarKind::FloatLit)
            );
            match pool.get(self.strip_mut(t)) {
                _ if lit => self.err(Code::MutOnPrimitive, pat, "`let mut` on a primitive value"),
                TyData::Prim(_) => {
                    self.err(Code::MutOnPrimitive, pat, "`let mut` on a primitive value");
                }
                TyData::Tuple { .. } => {
                    self.err(Code::MutOnTuple, pat, "`let mut` on a tuple");
                }
                _ => self.let_mut_name(t, annot.is_some(), pat),
            }
        } else if pat.kind() == SyntaxKind::BindingPattern && annot.is_none() {
            // `x := e` and `let x = e` bind the readonly view
            // (types.bind.short, types.bind.let-readonly).
            t = self.readonly_view(t);
        }
        // A plain name: a new local.
        if pat.kind() == SyntaxKind::BindingPattern && els.is_none() {
            let name_tok = pat.direct_tokens().find(|t| {
                matches!(
                    self.cx.src.tkind(*t),
                    Some(TokenKind::Ident | TokenKind::RawIdent)
                )
            });
            let Some(nt) = name_tok else {
                return unsupported("a binding pattern without a name");
            };
            let name = self.cx.names.syms.intern(self.cx.src.text(nt));
            let l = self.bind_local(name, t, pat);
            self.b.set(l, r, at.index());
            return Ok(());
        }
        // A pattern: a two-arm (or one-arm) match whose bindings stay in
        // scope after it.
        let mut binds = Vec::new();
        let outer = self.scopes.last().cloned().unwrap_or_default();
        let before = self.diags.len();
        let saved = self.let_view.replace(annot.is_some());
        let declared = self.declare_pattern(pat, t, &mut binds);
        self.let_view = saved;
        declared?;
        // `flow.let.refutable.else`, `flow.let.else.unreachable`. A pattern
        // that does not fit `t` already reported; skip the cascade.
        let refutable = self.refutable(pat, t);
        match els {
            _ if self.diags.len() != before => {}
            None if refutable => self.err(
                Code::RefutableLetPattern,
                pat,
                "this pattern can fail to match; add an `else` block",
            ),
            Some(e) if !refutable => self.err(
                Code::UnreachableMatchArm,
                e,
                "the pattern always matches, so `else` never runs",
            ),
            _ => {}
        }
        let open = self.b.open_block();
        let first = self.b.close_block(open, None, Ty::VOID, pat.index());
        let mut arms = vec![first];
        let mut rows = vec![Row {
            pat: Some(pat),
            wrap_some: false,
            binds,
            guard: None,
            arm: 0,
        }];
        if let Some(e) = els {
            let Some(blk) = Src::child(e, SyntaxKind::Block) else {
                return unsupported("a `let ... else` without a suite");
            };
            let eb = self.b.open_block();
            // `names.let-else.not-in-else`: the pattern's names are not
            // visible in the `else` block.
            let inner = self.scopes.pop().unwrap_or_default();
            self.scopes.push(outer);
            let checked = self.block_value(blk, Some(Ty::VOID));
            self.scopes.pop();
            self.scopes.push(inner);
            let (_, ety) = checked?;
            if ety != Ty::NEVER {
                self.err(
                    Code::LetElseFallsThrough,
                    e,
                    "the `else` suite must leave the block",
                );
            }
            arms.push(self.b.close_block(eb, None, Ty::NEVER, e.index()));
            rows.push(Row {
                pat: None,
                wrap_some: false,
                binds: vec![],
                guard: None,
                arm: 1,
            });
        }
        let db = self.b.open_block();
        self.decide(&rows, 0, r, t)?;
        let dec = self.b.close_block(db, None, Ty::NEVER, at.index());
        let mut rec = vec![dec];
        rec.extend(arms);
        let rec = self.b.refs_record(&rec);
        let _ = pool;
        self.b.emit(Tag::Match, r.0, rec, Ty::VOID, at.index());
        Ok(())
    }

    // ------------------------------------------------------------ match

    pub(crate) fn match_expr(
        &mut self,
        n: NodeRef<'_>,
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let Some(scrut) = n.children().next() else {
            return unsupported("a `match` without a scrutinee");
        };
        let (sr, st) = self.expr(scrut, None)?;
        let result = self.join_target(want);
        let mut rows = Vec::new();
        let mut arms = Vec::new();
        let mut all_never = true;
        let arm_nodes: Vec<NodeRef<'_>> = n
            .children()
            .filter(|c| c.kind() == SyntaxKind::MatchArm)
            .collect();
        for (i, arm) in arm_nodes.iter().enumerate() {
            let Some(pat) = arm.children().next() else {
                return unsupported("an arm without a pattern");
            };
            self.scopes.push(HashMap::new());
            let mut binds = Vec::new();
            self.declare_pattern(pat, st, &mut binds)?;
            let guard = match Src::child(*arm, SyntaxKind::MatchGuard) {
                Some(g) => {
                    let Some(ge) = g.children().next() else {
                        return unsupported("an empty guard");
                    };
                    let gb = self.b.open_block();
                    let (gr, gt) = self.expr(ge, Some(Ty::BOOL))?;
                    self.expect(gt, Ty::BOOL, ge, "guard");
                    Some(self.b.close_block(gb, Some(gr), Ty::BOOL, g.index()))
                }
                None => None,
            };
            let Some(body) = Src::child(*arm, SyntaxKind::Block) else {
                return unsupported("an arm without a body");
            };
            let ab = self.b.open_block();
            let (tail, ty) = self.block_value(body, Some(result))?;
            if ty != Ty::NEVER {
                all_never = false;
            }
            arms.push(self.b.close_block(ab, tail, ty, body.index()));
            self.scopes.pop();
            rows.push(Row {
                pat: Some(pat),
                wrap_some: false,
                binds,
                guard,
                arm: u32::try_from(i).unwrap_or(u32::MAX),
            });
        }
        self.check_exhaustive(n, &rows, st);
        self.check_reachable(&rows, st);
        let db = self.b.open_block();
        self.decide(&rows, 0, sr, st)?;
        let dec = self.b.close_block(db, None, Ty::NEVER, n.index());
        let mut rec = vec![dec];
        rec.extend(arms);
        let rec = self.b.refs_record(&rec);
        let ty = if all_never && !arm_nodes.is_empty() {
            Ty::NEVER
        } else {
            result
        };
        Ok((self.b.emit(Tag::Match, sr.0, rec, ty, n.index()), ty))
    }

    // ------------------------------------------------------------ decisions

    /// Emits, into the open block, the tests of rows `i..` against the
    /// scrutinee; every path ends in `ToArm` or `Unreachable`.
    pub(crate) fn decide(&mut self, rows: &[Row<'_>], i: usize, v: Ref, t: Ty) -> StageResult<()> {
        self.charge()?;
        let Some(row) = rows.get(i) else {
            self.b.emit(
                Tag::Unreachable,
                NONE,
                NONE,
                Ty::NEVER,
                hd_base::NodeIdx::NONE,
            );
            return Ok(());
        };
        if row.wrap_some {
            let pool = self.pool();
            let inner = match pool.get(self.infer.resolve(pool, t)) {
                TyData::Option(x) => x,
                _ => Ty::POISON,
            };
            let ok = self.b.open_block();
            let u = self.b.emit(Tag::Unwrap, v.0, NONE, inner, NodeIdx::NONE);
            let work = row.pat.map(|p| vec![(p, u, inner)]).unwrap_or_default();
            self.row_tests(rows, i, work, v, t)?;
            let okb = self.b.close_block(ok, None, Ty::NEVER, NodeIdx::NONE);
            let fb = self.b.open_block();
            self.decide(rows, i + 1, v, t)?;
            let fail = self.b.close_block(fb, None, Ty::NEVER, NodeIdx::NONE);
            let rec = self.b.refs_record(&[Ref(1), okb, fail]);
            self.b
                .emit(Tag::SwitchTag, v.0, rec, Ty::NEVER, NodeIdx::NONE);
            return Ok(());
        }
        let work = row.pat.map(|p| vec![(p, v, t)]).unwrap_or_default();
        self.row_tests(rows, i, work, v, t)
    }

    /// The remaining tests of row `i` (a stack of pattern, value, type),
    /// then its leaf.
    fn row_tests(
        &mut self,
        rows: &[Row<'_>],
        i: usize,
        mut work: Vec<(NodeRef<'_>, Ref, Ty)>,
        root: Ref,
        root_t: Ty,
    ) -> StageResult<()> {
        let pool = self.pool();
        let row = &rows[i];
        while let Some((p, v, t)) = work.pop() {
            match p.kind() {
                SyntaxKind::WildcardPattern => {}
                SyntaxKind::BindingPattern => {
                    if let Some(&(_, l)) = row.binds.iter().find(|(n, _)| *n == p.index()) {
                        self.b.set(l, v, p.index());
                    }
                }
                SyntaxKind::LiteralPattern | SyntaxKind::RangePattern => {
                    let cond = self.literal_test(p, v, t)?;
                    let then = self.b.open_block();
                    self.row_tests(rows, i, std::mem::take(&mut work), root, root_t)?;
                    let tb = self.b.close_block(then, None, Ty::NEVER, p.index());
                    let fb = self.b.open_block();
                    self.decide(rows, i + 1, root, root_t)?;
                    let eb = self.b.close_block(fb, None, Ty::NEVER, p.index());
                    let rec = self.b.refs_record(&[tb, eb]);
                    self.b.emit(Tag::If, cond.0, rec, Ty::NEVER, p.index());
                    return Ok(());
                }
                SyntaxKind::TuplePattern if p.children().next().is_none() => {}
                SyntaxKind::TuplePattern => {
                    let elems = match pool.get(self.infer.resolve(pool, t)) {
                        TyData::Tuple { elems, .. } => pool.list_items(elems),
                        TyData::Mut(x) => match pool.get(self.infer.resolve(pool, x)) {
                            TyData::Tuple { elems, .. } => pool.list_items(elems),
                            _ => &[],
                        },
                        _ => &[],
                    };
                    let subs: Vec<NodeRef<'_>> = p.children().collect();
                    for (k, s) in subs.iter().enumerate().rev() {
                        let et = elems.get(k).copied().unwrap_or(Ty::POISON);
                        let g = self.b.emit(
                            Tag::TupleGet,
                            v.0,
                            u32::try_from(k).unwrap_or(0),
                            et,
                            s.index(),
                        );
                        work.push((*s, g, et));
                    }
                }
                SyntaxKind::DataPattern => {
                    for f in p
                        .children()
                        .filter(|c| c.kind() == SyntaxKind::DataPatternField)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                    {
                        let fname = self.cx.src.text(self.cx.src.first(f)).to_owned();
                        let Some((idx, ft)) = self.field_of(t, &fname) else {
                            continue;
                        };
                        let g = self.b.emit(Tag::Field, v.0, idx, ft, f.index());
                        match f.children().next() {
                            Some(sub) => work.push((sub, g, ft)),
                            None => {
                                if let Some(&(_, l)) =
                                    row.binds.iter().find(|(n, _)| *n == f.index())
                                {
                                    self.b.set(l, g, f.index());
                                }
                            }
                        }
                    }
                }
                SyntaxKind::VariantPattern => {
                    let name = self.variant_pattern_name(p);
                    let Some((idx, fields)) = self.variant_fields(t, &name) else {
                        // Reported when the pattern was declared.
                        continue;
                    };
                    let subs = self.variant_subpatterns(p, t, &name)?;
                    let ok = self.b.open_block();
                    let is_option = matches!(
                        pool.get(match pool.get(self.infer.resolve(pool, t)) {
                            TyData::Mut(x) => x,
                            _ => self.infer.resolve(pool, t),
                        }),
                        TyData::Option(_)
                    );
                    let mut more = std::mem::take(&mut work);
                    for (k, s) in subs.iter().enumerate().rev() {
                        let Some(s) = s else { continue };
                        let ft = fields.get(k).copied().unwrap_or(Ty::POISON);
                        let g = if is_option {
                            self.b.emit(Tag::Unwrap, v.0, NONE, ft, s.index())
                        } else {
                            let rec = self
                                .b
                                .refs_record(&[Ref(idx), Ref(u32::try_from(k).unwrap_or(0))]);
                            self.b.emit(Tag::Payload, v.0, rec, ft, s.index())
                        };
                        more.push((*s, g, ft));
                    }
                    self.row_tests(rows, i, more, root, root_t)?;
                    let okb = self.b.close_block(ok, None, Ty::NEVER, p.index());
                    let fb = self.b.open_block();
                    self.decide(rows, i + 1, root, root_t)?;
                    let fail = self.b.close_block(fb, None, Ty::NEVER, p.index());
                    let rec = self.b.refs_record(&[Ref(idx), okb, fail]);
                    self.b.emit(Tag::SwitchTag, v.0, rec, Ty::NEVER, p.index());
                    return Ok(());
                }
                other => return unsupported(format!("the pattern {other:?} in a decision")),
            }
        }
        // The leaf: the guard, then the arm.
        match row.guard {
            Some(g) => {
                let to = self.b.open_block();
                self.b
                    .emit(Tag::ToArm, row.arm, NONE, Ty::NEVER, NodeIdx::NONE);
                let tob = self.b.close_block(to, None, Ty::NEVER, NodeIdx::NONE);
                let fb = self.b.open_block();
                self.decide(rows, i + 1, root, root_t)?;
                let fail = self.b.close_block(fb, None, Ty::NEVER, NodeIdx::NONE);
                let rec = self.b.refs_record(&[tob, fail]);
                self.b.emit(Tag::Guard, g.0, rec, Ty::NEVER, NodeIdx::NONE);
            }
            None => {
                self.b
                    .emit(Tag::ToArm, row.arm, NONE, Ty::NEVER, NodeIdx::NONE);
            }
        }
        Ok(())
    }

    /// `v == literal`, or a range test, as a `bool` value.
    fn literal_test(&mut self, p: NodeRef<'_>, v: Ref, t: Ty) -> StageResult<Ref> {
        if p.kind() == SyntaxKind::RangePattern {
            let ends: Vec<NodeRef<'_>> = p.children().collect();
            let [lo, hi] = ends.as_slice() else {
                return unsupported("an open range pattern");
            };
            let inclusive = p
                .direct_token(&self.cx.src.parse.tokens, TokenKind::DotDotEq)
                .is_some();
            let (l, _) = self.pattern_literal(*lo, Some(t))?;
            let (h, _) = self.pattern_literal(*hi, Some(t))?;
            let a = self.b.prim(PrimOp::Ge as u32, &[v, l], Ty::BOOL, p.index());
            let op = if inclusive { PrimOp::Le } else { PrimOp::Lt };
            let c = self.b.prim(op as u32, &[v, h], Ty::BOOL, p.index());
            return Ok(self
                .b
                .prim(PrimOp::And as u32, &[a, c], Ty::BOOL, p.index()));
        }
        let (lit, _) = self.pattern_literal(p, Some(t))?;
        let pool = self.pool();
        let tt = self.infer.resolve(pool, t);
        if tt == Ty::STRING {
            let rec = self.b.refs_record(&[v, lit]);
            return Ok(self.b.emit(
                Tag::Intrinsic,
                IntrinsicOp::StrEq as u32,
                rec,
                Ty::BOOL,
                p.index(),
            ));
        }
        Ok(self
            .b
            .prim(PrimOp::Eq as u32, &[v, lit], Ty::BOOL, p.index()))
    }

    // ------------------------------------------------------------ exhaustiveness

    fn to_p(&self, p: Option<NodeRef<'_>>, t: Ty) -> P {
        let Some(p) = p else { return P::Wild };
        let pool = self.pool();
        match p.kind() {
            SyntaxKind::WildcardPattern | SyntaxKind::BindingPattern => P::Wild,
            SyntaxKind::LiteralPattern => match self.cx.src.tkind(self.cx.src.first(p)) {
                Some(TokenKind::KwTrue) => P::Ctor("true".into(), vec![]),
                Some(TokenKind::KwFalse) => P::Ctor("false".into(), vec![]),
                _ => P::Lit,
            },
            SyntaxKind::TuplePattern if p.children().next().is_none() => P::Wild,
            SyntaxKind::TuplePattern => {
                let elems = match pool.get(self.strip(t)) {
                    TyData::Tuple { elems, .. } => pool.list_items(elems),
                    _ => &[],
                };
                P::Ctor(
                    "()".into(),
                    p.children()
                        .enumerate()
                        .map(|(i, c)| {
                            self.to_p(Some(c), elems.get(i).copied().unwrap_or(Ty::POISON))
                        })
                        .collect(),
                )
            }
            SyntaxKind::VariantPattern => {
                let name = self.variant_pattern_name(p);
                let fields = self
                    .variant_fields(t, &name)
                    .map(|f| f.1)
                    .unwrap_or_default();
                let subs = self.variant_subpatterns(p, t, &name).unwrap_or_default();
                let args = (0..fields.len())
                    .map(|k| self.to_p(subs.get(k).copied().flatten(), fields[k]))
                    .collect();
                P::Ctor(name, args)
            }
            SyntaxKind::DataPattern => {
                let Some(ctors) = self.ctors(t) else {
                    return P::Lit;
                };
                let Some((_, ftys)) = ctors.first() else {
                    return P::Wild;
                };
                let pool = self.pool();
                let st = self.strip(t);
                let TyData::Adt { def, .. } = pool.get(st) else {
                    return P::Lit;
                };
                let Some(ItemData::Data(fs)) = self.cx.lookup.item(def).map(|i| &i.data) else {
                    return P::Lit;
                };
                let mut args = vec![P::Wild; fs.len()];
                for f in p
                    .children()
                    .filter(|c| c.kind() == SyntaxKind::DataPatternField)
                {
                    let fname = self
                        .cx
                        .names
                        .syms
                        .intern(self.cx.src.text(self.cx.src.first(f)));
                    if let Some(k) = fs.iter().position(|x| x.name == fname) {
                        args[k] = self.to_p(
                            f.children().next(),
                            ftys.get(k).copied().unwrap_or(Ty::POISON),
                        );
                    }
                }
                P::Ctor("{}".into(), args)
            }
            _ => P::Lit,
        }
    }

    fn strip(&self, t: Ty) -> Ty {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        match pool.get(t) {
            TyData::Mut(i) => self.infer.resolve(pool, i),
            _ => t,
        }
    }

    /// Every constructor of a type with its field types; `None` for an
    /// open set (numbers, strings).
    fn ctors(&self, t: Ty) -> Option<Vec<(String, Vec<Ty>)>> {
        let pool = self.pool();
        let st = self.strip(t);
        match pool.get(st) {
            TyData::Prim(Prim::Bool) => {
                Some(vec![("true".into(), vec![]), ("false".into(), vec![])])
            }
            TyData::Tuple { elems, .. } => {
                Some(vec![("()".into(), pool.list_items(elems).to_vec())])
            }
            TyData::Option(i) => Some(vec![("None".into(), vec![]), ("Some".into(), vec![i])]),
            TyData::Adt { def, args } => match &self.cx.lookup.item(def)?.data {
                ItemData::Enum { variants, .. } => {
                    let argv = pool.list_items(args);
                    Some(
                        variants
                            .iter()
                            .map(|v| {
                                (
                                    self.cx.names.text(v.name).to_owned(),
                                    v.fields
                                        .iter()
                                        .map(|f| subst_owner(pool, def, argv, f.ty))
                                        .collect(),
                                )
                            })
                            .collect(),
                    )
                }
                ItemData::Data(fs) => {
                    let argv = pool.list_items(args);
                    Some(vec![(
                        "{}".into(),
                        fs.iter()
                            .map(|f| subst_owner(pool, def, argv, f.ty))
                            .collect(),
                    )])
                }
                _ => None,
            },
            _ => None,
        }
    }

    fn useful(&self, m: &[Vec<P>], q: &[P], tys: &[Ty], depth: u32) -> bool {
        if depth > 64 {
            return false;
        }
        let Some(head) = q.first() else {
            return m.is_empty();
        };
        let t = tys.first().copied().unwrap_or(Ty::POISON);
        let spec = |c: &str, arity: usize, rows: &[Vec<P>]| -> Vec<Vec<P>> {
            rows.iter()
                .filter_map(|r| match &r[0] {
                    P::Ctor(d, a) if d == c => {
                        let mut v = a.clone();
                        v.resize(arity, P::Wild);
                        v.extend(r[1..].iter().cloned());
                        Some(v)
                    }
                    P::Ctor(..) | P::Lit => None,
                    P::Wild => {
                        let mut v = vec![P::Wild; arity];
                        v.extend(r[1..].iter().cloned());
                        Some(v)
                    }
                })
                .collect()
        };
        match head {
            P::Ctor(c, args) => {
                let ftys = self
                    .ctors(t)
                    .and_then(|cs| cs.into_iter().find(|(n, _)| n == c).map(|x| x.1))
                    .unwrap_or_else(|| vec![Ty::POISON; args.len()]);
                let mut nq = args.clone();
                nq.resize(ftys.len(), P::Wild);
                nq.extend(q[1..].iter().cloned());
                let mut nt = ftys.clone();
                nt.extend(tys[1..].iter().copied());
                self.useful(&spec(c, ftys.len(), m), &nq, &nt, depth + 1)
            }
            P::Lit => true,
            P::Wild => {
                let ctors = self.ctors(t);
                let present: Vec<&String> = m
                    .iter()
                    .filter_map(|r| match &r[0] {
                        P::Ctor(c, _) => Some(c),
                        _ => None,
                    })
                    .collect();
                if let Some(cs) = ctors
                    && !cs.is_empty()
                    && cs.iter().all(|(c, _)| present.contains(&c))
                {
                    return cs.iter().any(|(c, ftys)| {
                        let mut nq = vec![P::Wild; ftys.len()];
                        nq.extend(q[1..].iter().cloned());
                        let mut nt = ftys.clone();
                        nt.extend(tys[1..].iter().copied());
                        self.useful(&spec(c, ftys.len(), m), &nq, &nt, depth + 1)
                    });
                }
                let d: Vec<Vec<P>> = m
                    .iter()
                    .filter(|r| matches!(r[0], P::Wild))
                    .map(|r| r[1..].to_vec())
                    .collect();
                self.useful(&d, &q[1..], &tys[1..], depth + 1)
            }
        }
    }

    /// `unreachable-match-arm` for an arm whose values the unguarded arms
    /// before it already match.
    /// Whether some value of `t` fails to match `pat`.
    pub(crate) fn refutable(&mut self, pat: NodeRef<'_>, t: Ty) -> bool {
        let p = self.to_p(Some(pat), t);
        self.useful(&[vec![p]], &[P::Wild], &[t], 0)
    }

    fn check_reachable(&mut self, rows: &[Row<'_>], t: Ty) {
        let mut before: Vec<Vec<P>> = Vec::new();
        for r in rows {
            let p = self.to_p(r.pat, t);
            if !self.useful(&before, std::slice::from_ref(&p), &[t], 0)
                && let Some(node) = r.pat
            {
                self.err(
                    Code::UnreachableMatchArm,
                    node,
                    "the arms before this one match all of its values",
                );
            }
            if r.guard.is_none() {
                before.push(vec![p]);
            }
        }
    }

    /// `nonexhaustive-match` when some value matches no unguarded arm.
    fn check_exhaustive(&mut self, n: NodeRef<'_>, rows: &[Row<'_>], t: Ty) {
        let m: Vec<Vec<P>> = rows
            .iter()
            .filter(|r| r.guard.is_none())
            .map(|r| vec![self.to_p(r.pat, t)])
            .collect();
        if self.useful(&m, &[P::Wild], &[t], 0) {
            let missing = self.ctors(t).map_or_else(
                || "_".into(),
                |cs| {
                    cs.into_iter()
                        .filter(|(c, f)| {
                            let q = vec![P::Ctor(c.clone(), vec![P::Wild; f.len()])];
                            self.useful(&m, &q, &[t], 0)
                        })
                        .map(|(c, _)| c)
                        .collect::<Vec<_>>()
                        .join(", ")
                },
            );
            let msg = format!("no arm matches {missing}");
            self.err(Code::NonexhaustiveMatch, n, &msg);
        }
    }
}
