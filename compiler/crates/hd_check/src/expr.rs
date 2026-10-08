//! Expressions (type-checking.md §2.2): literals, strings and
//! interpolation, names, operators, fields, indexing, collection and data
//! literals, `if`, loops, closures, `?`, ranges and providers.

use std::collections::HashMap;

use hd_base::DefId;
use hd_base::StageResult;
use hd_diag::Code;
use hd_intern::PathKind;
use hd_resolve::{ItemData, Src};
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_tir::ir::{IntrinsicOp, NONE, PrimOp, Ref, Tag, TirSink, local_flags};
use hd_types::solver::{Answer, TraitRef};
use hd_types::{ParamRef, Prim, Ty, TyData, TyList, VarKind};

use crate::body::{Ck, OpenSub, unsupported};

/// Decodes a string piece's text: escapes, and `$name` references.
pub(crate) enum Piece {
    Text(String),
    Name(String),
}

fn decode_piece(raw: &str, out: &mut Vec<Piece>) {
    let mut text = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('n') => text.push('\n'),
                Some('t') => text.push('\t'),
                Some('r') => text.push('\r'),
                Some('0') => text.push('\0'),
                Some('u') => {
                    let mut hex = String::new();
                    if chars.peek() == Some(&'{') {
                        chars.next();
                        while let Some(&h) = chars.peek() {
                            chars.next();
                            if h == '}' {
                                break;
                            }
                            hex.push(h);
                        }
                    }
                    if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                        text.push(ch);
                    }
                }
                Some(o) => text.push(o),
                None => {}
            },
            '$' if chars.peek().is_some_and(|n| n.is_alphabetic() || *n == '_') => {
                let mut name = String::new();
                while let Some(&n) = chars.peek() {
                    if n.is_alphanumeric() || n == '_' {
                        name.push(n);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if !text.is_empty() {
                    out.push(Piece::Text(std::mem::take(&mut text)));
                }
                out.push(Piece::Name(name));
            }
            _ => text.push(c),
        }
    }
    if !text.is_empty() {
        out.push(Piece::Text(text));
    }
}

/// The text of a string token without its delimiters and interpolation
/// openers and closers.
fn strip_piece(t: &str, kind: TokenKind) -> &str {
    let t = match kind {
        TokenKind::StrHead | TokenKind::String => {
            let t = t.trim_start_matches(|c: char| c.is_alphanumeric() || c == '_');
            t.strip_prefix("\"\"\"")
                .or_else(|| t.strip_prefix('"'))
                .unwrap_or(t)
        }
        _ => t.strip_prefix('}').unwrap_or(t),
    };
    match kind {
        TokenKind::StrHead | TokenKind::StrMid => t.strip_suffix("${").unwrap_or(t),
        _ => t
            .strip_suffix("\"\"\"")
            .or_else(|| t.strip_suffix('"'))
            .unwrap_or(t),
    }
}

/// The text of a plain string literal without interpolation, as a test
/// name or option (`module.testing.it.name`); `None` for anything else.
pub(crate) fn literal_text(src: &hd_resolve::Src<'_>, n: NodeRef<'_>) -> Option<String> {
    if n.kind() != SyntaxKind::StringExpr {
        return None;
    }
    let toks: Vec<_> = src.tokens(n).collect();
    let [t] = toks.as_slice() else {
        return None;
    };
    let text = src.text(*t);
    if src.tkind(*t) != Some(TokenKind::String) || !text.starts_with('"') {
        return None;
    }
    let mut pieces = Vec::new();
    decode_piece(strip_piece(text, TokenKind::String), &mut pieces);
    let mut out = String::new();
    for p in pieces {
        match p {
            Piece::Text(s) => out.push_str(&s),
            Piece::Name(_) => return None,
        }
    }
    Some(out)
}

impl Ck<'_, '_> {
    pub(crate) fn expr(&mut self, n: NodeRef<'_>, want: Option<Ty>) -> StageResult<(Ref, Ty)> {
        let span = self.cx.src.span(n);
        self.expr_node(n, want).map_err(|e| e.at(span))
    }

    fn expr_node(&mut self, n: NodeRef<'_>, want: Option<Ty>) -> StageResult<(Ref, Ty)> {
        self.charge()?;
        let pool = self.cx.names.pool;
        let kids: Vec<NodeRef<'_>> = n.children().collect();
        Ok(match n.kind() {
            SyntaxKind::LiteralExpr => self.literal(n, want)?,
            SyntaxKind::StringExpr => (self.string_expr(n)?, Ty::STRING),
            SyntaxKind::UnaryExpr => self.unary(n, &kids, want)?,
            SyntaxKind::ParenExpr => match kids.as_slice() {
                [e] => self.expr(*e, want)?,
                _ => return unsupported("a parenthesized expression shape"),
            },
            SyntaxKind::TupleExpr => {
                let wants: Vec<Option<Ty>> =
                    match want.map(|w| pool.get(self.infer.shallow(pool, w))) {
                        Some(TyData::Tuple { elems, .. }) => {
                            pool.list_items(elems).into_iter().map(Some).collect()
                        }
                        _ => vec![],
                    };
                if kids.is_empty() {
                    return Ok((Ref(NONE), Ty::VOID));
                }
                let mut refs = Vec::new();
                let mut tys = Vec::new();
                for (i, e) in kids.iter().enumerate() {
                    if e.kind() == SyntaxKind::SpreadExpr {
                        return unsupported("a tuple spread");
                    }
                    let w = wants.get(i).copied().flatten();
                    let (r, t) = self.expr(*e, w)?;
                    let r = match w {
                        Some(w) => self.coerce(r, t, w, *e, "tuple element"),
                        None => r,
                    };
                    refs.push(r);
                    tys.push(w.unwrap_or(t));
                }
                let t = pool.intern_ty(&TyData::Tuple {
                    elems: pool.list(&tys),
                    rest: None,
                });
                let rec = self.b.refs_record(&refs);
                (self.b.emit(Tag::NewTuple, NONE, rec, t, n.index()), t)
            }
            SyntaxKind::NameExpr => self.name_expr(n, want)?,
            SyntaxKind::VariantExpr => {
                self.variant_value(n, &crate::call::Args::empty(), want, None)?
            }
            SyntaxKind::AdditiveExpr
            | SyntaxKind::MultiplicativeExpr
            | SyntaxKind::ComparisonExpr
            | SyntaxKind::LogicalAndExpr
            | SyntaxKind::LogicalOrExpr
            | SyntaxKind::BitwiseAndExpr
            | SyntaxKind::BitwiseOrExpr
            | SyntaxKind::BitwiseXorExpr
            | SyntaxKind::ShiftExpr => self.binary(n, &kids)?,
            SyntaxKind::FieldExpr => self.field_expr(n, &kids)?,
            SyntaxKind::IndexExpr => self.index_expr(n, &kids)?,
            SyntaxKind::CallExpr => self.call(n, &kids, want)?,
            SyntaxKind::DataExpr => self.data_expr(n, &kids, want)?,
            SyntaxKind::ListExpr => self.list_expr(n, &kids, want)?,
            SyntaxKind::MapExpr => self.map_expr(n, &kids, want)?,
            SyntaxKind::IfExpr => self.if_expr(n, want)?,
            SyntaxKind::WhileExpr => self.while_expr(n, want)?,
            SyntaxKind::ForExpr => self.for_expr(n, want)?,
            SyntaxKind::MatchExpr => self.match_expr(n, want)?,
            SyntaxKind::ClosureExpr => self.closure(n, want)?,
            SyntaxKind::PipeExpr => self.pipe(&kids, want)?,
            SyntaxKind::PlaceholderExpr => self.placeholder_value(n),
            SyntaxKind::ComprehensionExpr => self.comprehension(n, &kids, want)?,
            SyntaxKind::TryExpr => self.try_expr(n, &kids)?,
            SyntaxKind::RangeExpr => self.range_expr(n, &kids, want)?,
            SyntaxKind::ContextExpr => self.context_expr(n, want)?,
            SyntaxKind::BindingExpr => {
                let [pat, rhs] = kids.as_slice() else {
                    return unsupported("this binding form");
                };
                if rhs.kind() == SyntaxKind::BindingExpr {
                    return unsupported("a binding chain");
                }
                // `names.bind.no-redeclare`, `names.scope.duplicate`.
                let name = (pat.kind() == SyntaxKind::BindingPattern).then(|| self.sym_of(*pat));
                if let Some(name) = name
                    && self.scopes.last().is_some_and(|s| s.contains_key(&name))
                {
                    let msg = format!(
                        "duplicate-binding: `{}` is already bound in this scope",
                        self.cx.names.text(name)
                    );
                    self.err(Code::DuplicateBinding, *pat, &msg);
                }
                self.let_pattern(*pat, None, *rhs, None, n)?;
                // `expr.bind.one-name`: the binding is not reassignable,
                // and the expression's value is the initializer's.
                match name.and_then(|name| self.find_local(name)) {
                    Some((l, _)) => {
                        self.b.body_mut().local_flags[l.idx()] |= local_flags::SHORT;
                        if want == Some(Ty::VOID) {
                            (Ref(NONE), Ty::VOID)
                        } else {
                            let t = self.b.local_ty(l);
                            (self.b.get(l, t, n.index()), t)
                        }
                    }
                    None => (Ref(NONE), Ty::VOID),
                }
            }
            SyntaxKind::TypeArgsExpr => self.explicit_item_value(n)?,
            SyntaxKind::PathExpr => return self.gap(n, "a path used as a value"),
            other => return unsupported(format!("expression {other:?}")),
        })
    }

    fn literal(&mut self, n: NodeRef<'_>, want: Option<Ty>) -> StageResult<(Ref, Ty)> {
        let (r, t) = self.literal_at(self.cx.src.first(n), want)?;
        if self.b.const_of(r).is_some() {
            self.lit_nodes.push((r, n.index()));
        }
        Ok((r, t))
    }

    /// The literal token at `t`.
    pub(crate) fn literal_at(
        &mut self,
        t: hd_base::TokenIdx,
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.cx.names.pool;
        Ok(match self.cx.src.tkind(t) {
            Some(TokenKind::KwTrue) => (self.b.const_value(Ty::BOOL, 1), Ty::BOOL),
            Some(TokenKind::KwFalse) => (self.b.const_value(Ty::BOOL, 0), Ty::BOOL),
            Some(TokenKind::KwPass) => (Ref(NONE), Ty::VOID),
            Some(TokenKind::Char) => {
                let raw = self.cx.src.text(t);
                let inner = raw.trim_start_matches('\'').trim_end_matches('\'');
                let mut pieces = Vec::new();
                decode_piece(inner, &mut pieces);
                let ch = match pieces.as_slice() {
                    [Piece::Text(s)] => s.chars().next().unwrap_or('\0'),
                    _ => inner.chars().next().unwrap_or('\0'),
                };
                let ct = Ty::prim(Prim::Char);
                (self.b.const_value(ct, u64::from(u32::from(ch))), ct)
            }
            Some(TokenKind::Number) => {
                let text = self.cx.src.text(t).replace('_', "");
                let int = if let Some(h) = text.strip_prefix("0x") {
                    u64::from_str_radix(h, 16).ok()
                } else if let Some(b) = text.strip_prefix("0b") {
                    u64::from_str_radix(b, 2).ok()
                } else if let Some(o) = text.strip_prefix("0o") {
                    u64::from_str_radix(o, 8).ok()
                } else {
                    text.parse::<u64>().ok()
                };
                if let Some(v) = int {
                    let float_wanted = want
                        .map(|w| self.infer.shallow(pool, w))
                        .is_some_and(|w| matches!(pool.get(w), TyData::Prim(p) if p.is_float()));
                    if float_wanted && let Ok(f) = text.parse::<f64>() {
                        let ft = self.infer.fresh(pool, VarKind::FloatLit);
                        return Ok((self.b.const_value(ft, f.to_bits()), ft));
                    }
                    let lt = self.infer.fresh(pool, VarKind::IntLit);
                    (self.b.const_value(lt, v), lt)
                } else if let Ok(f) = text.parse::<f64>() {
                    let ft = self.infer.fresh(pool, VarKind::FloatLit);
                    (self.b.const_value(ft, f.to_bits()), ft)
                } else {
                    return unsupported(format!("the literal `{text}`"));
                }
            }
            _ => return unsupported("this literal form"),
        })
    }

    /// A string literal: one constant, or `Interp` over its parts, each
    /// converted with `Display.to_string` (lowering-catalog "String
    /// Interpolation And Concatenation").
    pub(crate) fn string_expr(&mut self, n: NodeRef<'_>) -> StageResult<Ref> {
        let first = self.cx.src.first(n);
        let ftext = self.cx.src.text(first);
        if !ftext.starts_with('"') {
            return unsupported("a prefixed string");
        }
        let mut parts: Vec<Ref> = Vec::new();
        let mut kids = n.children();
        for t in self.cx.src.tokens(n).collect::<Vec<_>>() {
            let Some(k) = self.cx.src.tkind(t) else {
                continue;
            };
            if !matches!(
                k,
                TokenKind::String | TokenKind::StrHead | TokenKind::StrMid | TokenKind::StrTail
            ) {
                continue;
            }
            // A piece after an interpolation: the interpolated expression first.
            if matches!(k, TokenKind::StrMid | TokenKind::StrTail)
                && let Some(ip) = kids.next()
                && let Some(e) = ip.children().next()
            {
                let (r, ty) = self.expr(e, None)?;
                let s = self.display_str(r, ty, e)?;
                parts.push(s);
            }
            let raw = strip_piece(self.cx.src.text(t), k).to_owned();
            let mut pieces = Vec::new();
            decode_piece(&raw, &mut pieces);
            for p in pieces {
                match p {
                    Piece::Text(s) => parts.push(self.b.const_str(&s)),
                    Piece::Name(name) => {
                        let sym = self.cx.names.syms.intern(&name);
                        // A local, else a top-level binding of the module.
                        let read = match self.find_local(sym) {
                            Some((l, d)) => Some(self.read_local(l, d, n)),
                            None => self.global_get(sym, n),
                        };
                        let Some((r, ty)) = read else {
                            let msg = format!("unknown-name `{name}`");
                            self.err(Code::UnknownName, n, &msg);
                            continue;
                        };
                        let s = self.display_str(r, ty, n)?;
                        parts.push(s);
                    }
                }
            }
        }
        Ok(match parts.as_slice() {
            [] => self.b.const_str(""),
            [one] if self.b.const_of(*one).is_some() => *one,
            _ => {
                let rec = self.b.refs_record(&parts);
                self.b.emit(Tag::Interp, NONE, rec, Ty::STRING, n.index())
            }
        })
    }

    /// A value's `Display` text: itself for a string, else `to_string`.
    pub(crate) fn display_str(&mut self, r: Ref, t: Ty, n: NodeRef<'_>) -> StageResult<Ref> {
        let pool = self.cx.names.pool;
        let tt = self.infer.shallow(pool, t);
        let tt = match pool.get(tt) {
            TyData::Mut(i) => self.infer.shallow(pool, i),
            _ => tt,
        };
        if tt == Ty::STRING {
            return Ok(r);
        }
        let display = self.cx.names.item("std.format", "Display");
        let r2 = self.trait_call(display, "to_string", r, t, &[], n)?;
        Ok(r2.0)
    }

    fn unary(
        &mut self,
        n: NodeRef<'_>,
        kids: &[NodeRef<'_>],
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let op = self.cx.src.tkind(self.cx.src.first(n));
        let Some(e) = kids.first() else {
            return unsupported("an operand-less unary");
        };
        let (r, t) = self.expr(*e, want)?;
        let pool = self.cx.names.pool;
        let st = self.strip_mut(t);
        let numeric = self.numeric(t);
        let prim = match pool.get(st) {
            TyData::Prim(p) => Some(p),
            _ => None,
        };
        // `expr.arith.unary-plus`, `expr.arith.unary-minus`: unary `+` on
        // numbers only, `-` on signed numbers; `-` and `~` on other types
        // are the `Neg` and `Not` traits (`expr.op.desugar`).
        let mismatch = |me: &mut Self, what: &str| {
            let msg = format!("type-mismatch: {what} does not apply to {}", me.show(t));
            me.err(Code::TypeMismatch, n, &msg);
            Ok((
                me.b.emit(Tag::Poison, NONE, NONE, Ty::POISON, n.index()),
                Ty::POISON,
            ))
        };
        match op {
            Some(TokenKind::Plus) if !numeric || prim == Some(Prim::Char) => {
                return mismatch(self, "unary `+`");
            }
            Some(TokenKind::Minus) if prim.is_some_and(Prim::is_unsigned) => {
                return mismatch(self, "unary `-`");
            }
            Some(TokenKind::Tilde) if prim.is_some_and(|p| p.is_float() || p == Prim::Char) => {
                return mismatch(self, "`~`");
            }
            Some(TokenKind::Minus | TokenKind::Tilde) if !numeric => {
                let (trait_name, method) = if op == Some(TokenKind::Minus) {
                    ("Neg", "neg")
                } else {
                    ("Not", "not")
                };
                let tr = self.cx.names.item("std.ops", trait_name);
                if !self.op_fits(tr, t, None)? {
                    return mismatch(self, &format!("`{trait_name}`"));
                }
                return self.trait_call_args(tr, method, r, t, &[], n);
            }
            _ => {}
        }
        Ok(match op {
            Some(TokenKind::Plus) => (r, t),
            Some(TokenKind::Minus) => {
                if let Some((ct, bits)) = self.b.const_of(r) {
                    self.lit_nodes.retain(|x| x.0 != r);
                    let pool = self.pool();
                    let is_float = matches!(self.infer.kind_of(pool, ct), Some(VarKind::FloatLit))
                        || matches!(pool.get(self.infer.shallow(pool, ct)), TyData::Prim(p) if p.is_float());
                    let v = if is_float {
                        (-f64::from_bits(bits)).to_bits()
                    } else {
                        bits.cast_signed().wrapping_neg().cast_unsigned()
                    };
                    let nr = self.b.const_value(ct, v);
                    self.lit_nodes.push((nr, n.index()));
                    (nr, t)
                } else {
                    (self.b.prim(PrimOp::Neg as u32, &[r], t, n.index()), t)
                }
            }
            Some(TokenKind::Bang | TokenKind::Tilde) => {
                if op == Some(TokenKind::Bang) {
                    self.expect(t, Ty::BOOL, *e, "operand");
                }
                (self.b.prim(PrimOp::Not as u32, &[r], t, n.index()), t)
            }
            _ => return unsupported("this unary operator"),
        })
    }

    /// Whether a type is a primitive number (or a parameter of a numeric
    /// family, or a literal variable), so operators are `Prim`.
    pub(crate) fn numeric(&self, t: Ty) -> bool {
        let pool = self.pool();
        let t = self.infer.shallow(pool, t);
        match pool.get(t) {
            TyData::Prim(p) => p.is_integer() || p.is_float() || p == Prim::Char,
            TyData::Infer(_) => self
                .infer
                .kind_of(pool, t)
                .is_some_and(|k| k != VarKind::General),
            TyData::Mut(i) => self.numeric(i),
            TyData::Param(_) => (0..self.env.clause_self.len()).any(|i| {
                self.env.clause_self[i] == t
                    && matches!(
                        self.cx.names.path(self.env.clause_trait[i]).as_str(),
                        "std/num/Num" | "std/num/Integer" | "std/num/Float"
                    )
            }),
            TyData::Never | TyData::Poison => true,
            _ => false,
        }
    }

    fn binary(&mut self, n: NodeRef<'_>, kids: &[NodeRef<'_>]) -> StageResult<(Ref, Ty)> {
        let [l, r] = kids else {
            return unsupported("a binary expression shape");
        };
        let op_tok = hd_base::TokenIdx::from_raw(self.cx.src.last(*l).raw() + 1);
        let Some(op) = self.cx.src.tkind(op_tok) else {
            return unsupported("a binary operator");
        };
        if matches!(op, TokenKind::AndAnd | TokenKind::OrOr) {
            let (a, at) = self.expr(*l, Some(Ty::BOOL))?;
            self.expect(at, Ty::BOOL, *l, "operand");
            let m = self.b.open_block();
            let (c, ct) = self.expr(*r, Some(Ty::BOOL))?;
            self.expect(ct, Ty::BOOL, *r, "operand");
            let blk = self.b.close_block(m, Some(c), Ty::BOOL, r.index());
            let tag = if op == TokenKind::AndAnd {
                Tag::And
            } else {
                Tag::Or
            };
            return Ok((self.b.emit(tag, a.0, blk.0, Ty::BOOL, n.index()), Ty::BOOL));
        }
        let (a, at) = self.expr(*l, None)?;
        let (c, ct) = self.expr(*r, Some(at))?;
        let v = self.binary_values(op, (a, at), (c, ct), n, *r)?;
        let t = self.b.ty_of(v);
        Ok((v, t))
    }

    /// Applies a binary operator to two checked operands.
    pub(crate) fn binary_values(
        &mut self,
        op: TokenKind,
        (a, at): (Ref, Ty),
        (c, ct): (Ref, Ty),
        n: NodeRef<'_>,
        rn: NodeRef<'_>,
    ) -> StageResult<Ref> {
        let pool = self.cx.names.pool;
        let prim = match op {
            TokenKind::Plus => Some(PrimOp::Add),
            TokenKind::Minus => Some(PrimOp::Sub),
            TokenKind::Star => Some(PrimOp::Mul),
            TokenKind::Slash => Some(PrimOp::Div),
            TokenKind::Percent => Some(PrimOp::Rem),
            TokenKind::EqEq => Some(PrimOp::Eq),
            TokenKind::NotEq => Some(PrimOp::Ne),
            TokenKind::Lt => Some(PrimOp::Lt),
            TokenKind::LtEq => Some(PrimOp::Le),
            TokenKind::Gt => Some(PrimOp::Gt),
            TokenKind::GtEq => Some(PrimOp::Ge),
            TokenKind::Amp => Some(PrimOp::BitAnd),
            TokenKind::Pipe => Some(PrimOp::BitOr),
            TokenKind::Caret => Some(PrimOp::BitXor),
            TokenKind::Shl => Some(PrimOp::Shl),
            TokenKind::Shr => Some(PrimOp::Shr),
            _ => None,
        };
        if op == TokenKind::KwIs {
            // `expr.is.value-operand`: both operands have identity.
            for (t, at) in [(at, n), (ct, rn)] {
                if !self.has_identity(t) {
                    let msg = format!(
                        "identity-requires-references: {} has no identity to compare with `is`",
                        self.show(t)
                    );
                    self.err(Code::IdentityRequiresReferences, at, &msg);
                }
            }
            return Ok(self.b.emit(Tag::Is, a.0, c.0, Ty::BOOL, n.index()));
        }
        let Some(prim) = prim else {
            return unsupported("this binary operator");
        };
        let cmp = matches!(
            prim,
            PrimOp::Eq | PrimOp::Ne | PrimOp::Lt | PrimOp::Le | PrimOp::Gt | PrimOp::Ge
        );
        let shift = matches!(prim, PrimOp::Shl | PrimOp::Shr);
        let bitwise = matches!(prim, PrimOp::BitAnd | PrimOp::BitOr | PrimOp::BitXor);
        let bool_ops = matches!(prim, PrimOp::Eq | PrimOp::Ne);
        let ats = self.infer.shallow(pool, at);
        let ats = match pool.get(ats) {
            TyData::Mut(i) => self.infer.shallow(pool, i),
            _ => ats,
        };
        let general = |me: &Self, x: Ty| {
            let x = me.infer.shallow(pool, x);
            matches!(pool.get(x), TyData::Infer(_))
                && me.infer.kind_of(pool, x) == Some(VarKind::General)
        };
        if general(self, at) && self.numeric(ct) && !shift {
            self.expect(at, ct, n, "operand");
        }
        if self.numeric(at) || (ats == Ty::BOOL && bool_ops) {
            if shift {
                self.shift_count(ct, rn);
            } else {
                self.expect(ct, at, rn, "operand");
            }
            // `expr.bit.non-integer-no-impl`: bitwise operators and shifts
            // take integers.
            if (bitwise || shift) && self.float_like(at) {
                let msg = format!(
                    "type-mismatch: bitwise operators take integers, not {}",
                    self.show(at)
                );
                self.err(Code::TypeMismatch, n, &msg);
            }
            let result = if cmp { Ty::BOOL } else { at };
            return Ok(self.b.prim(prim as u32, &[a, c], result, n.index()));
        }
        // Strings: `+` and `==` are built in; others go through traits.
        if ats == Ty::STRING && matches!(prim, PrimOp::Add | PrimOp::Eq | PrimOp::Ne) {
            self.expect(ct, Ty::STRING, rn, "operand");
            let (iop, t) = if prim == PrimOp::Add {
                (IntrinsicOp::StrConcat, Ty::STRING)
            } else {
                (IntrinsicOp::StrEq, Ty::BOOL)
            };
            let rec = self.b.refs_record(&[a, c]);
            let v = self.b.emit(Tag::Intrinsic, iop as u32, rec, t, n.index());
            if prim == PrimOp::Ne {
                return Ok(self.b.prim(PrimOp::Not as u32, &[v], Ty::BOOL, n.index()));
            }
            return Ok(v);
        }
        // Everything else is the operator trait's method.
        let (module, trait_name, method) = match prim {
            PrimOp::Add => ("std.ops", "Add", "add"),
            PrimOp::Sub => ("std.ops", "Sub", "sub"),
            PrimOp::Mul => ("std.ops", "Mul", "mul"),
            PrimOp::Div => ("std.ops", "Div", "div"),
            PrimOp::Rem => ("std.ops", "Rem", "rem"),
            PrimOp::BitAnd => ("std.ops", "BitAnd", "bit_and"),
            PrimOp::BitOr => ("std.ops", "BitOr", "bit_or"),
            PrimOp::BitXor => ("std.ops", "BitXor", "bit_xor"),
            PrimOp::Shl => ("std.ops", "Shl", "shl"),
            PrimOp::Shr => ("std.ops", "Shr", "shr"),
            PrimOp::Eq | PrimOp::Ne => ("std.cmp", "Eq", "eq"),
            _ => ("std.cmp", "Ord", "cmp"),
        };
        let tr = self.cx.names.item(module, trait_name);
        if matches!(prim, PrimOp::Lt | PrimOp::Le | PrimOp::Gt | PrimOp::Ge) {
            return self.ordering((a, at), (c, ct), prim, n, rn);
        }
        if !self.op_fits(tr, at, Some(ct))? {
            let msg = format!(
                "type-mismatch: {} does not implement `{trait_name}` for this operand",
                self.show(at)
            );
            self.err(Code::TypeMismatch, n, &msg);
            return Ok(self.b.emit(Tag::Poison, NONE, NONE, Ty::POISON, n.index()));
        }
        let (v, t) = self.trait_call_args(tr, method, a, at, &[(c, ct, rn)], n)?;
        if prim == PrimOp::Ne {
            return Ok(self.b.prim(PrimOp::Not as u32, &[v], Ty::BOOL, n.index()));
        }
        let _ = t;
        Ok(v)
    }

    /// Whether a type is floating-point, or a floating literal's variable.
    fn float_like(&self, t: Ty) -> bool {
        let pool = self.cx.names.pool;
        let t = self.strip_mut(t);
        match pool.get(t) {
            TyData::Prim(p) => p.is_float(),
            TyData::Infer(_) => self.infer.kind_of(pool, t) == Some(VarKind::FloatLit),
            _ => false,
        }
    }

    /// A shift count (`expr.shift.count-unsigned`): any unsigned integer
    /// type; an unsuffixed literal takes `u32`.
    fn shift_count(&mut self, ct: Ty, rn: NodeRef<'_>) {
        let pool = self.cx.names.pool;
        let c = self.strip_mut(ct);
        let ok = match pool.get(c) {
            TyData::Infer(_) if self.infer.kind_of(pool, c) == Some(VarKind::IntLit) => {
                self.infer.unify(pool, c, Ty::prim(Prim::U32)).is_ok()
            }
            TyData::Prim(p) => p.is_unsigned(),
            TyData::Never | TyData::Poison => true,
            _ => false,
        };
        if !ok {
            let msg = format!(
                "type-mismatch: a shift count must be unsigned, found {}",
                self.show(ct)
            );
            self.err(Code::TypeMismatch, rn, &msg);
        }
    }

    /// Whether some implementation of operator trait `tr` fits a left
    /// operand of type `at` and a right operand of type `ct`
    /// (`expr.op.no-impl`).
    pub(crate) fn op_fits(&mut self, tr: DefId, at: Ty, ct: Option<Ty>) -> StageResult<bool> {
        let pool = self.cx.names.pool;
        let t = self.strip_mut(at);
        if matches!(
            pool.get(t),
            TyData::Infer(_) | TyData::Poison | TyData::Never
        ) {
            return Ok(true);
        }
        let n_trait = self.cx.lookup.item(tr).map_or(0, |i| i.generics.len());
        let mut args: Vec<Ty> = (0..n_trait)
            .map(|_| self.infer.fresh(pool, VarKind::General))
            .collect();
        if let (Some(first), Some(c)) = (args.first_mut(), ct) {
            let c = self.strip_mut(c);
            if !matches!(pool.get(c), TyData::Infer(_)) {
                *first = c;
            }
        }
        let tref = TraitRef {
            trait_: tr,
            self_ty: t,
            args: pool.list(&args),
        };
        if self.builtin_holds(tref).is_some() {
            return Ok(true);
        }
        Ok(!matches!(self.solve(tref)?, Answer::Fails(_)))
    }

    /// `a < b` and the other relational operators on non-primitive
    /// operands: `PartialOrd.partial_cmp`, then a test of its answer
    /// (`expr.ord.partial-cmp`; an unordered answer makes all four false).
    fn ordering(
        &mut self,
        (a, at): (Ref, Ty),
        (c, ct): (Ref, Ty),
        prim: PrimOp,
        n: NodeRef<'_>,
        rn: NodeRef<'_>,
    ) -> StageResult<Ref> {
        let pool = self.cx.names.pool;
        let tr = self.cx.names.item("std.cmp", "PartialOrd");
        if !self.op_fits(tr, at, Some(ct))? {
            let msg = format!(
                "type-mismatch: {} does not implement `PartialOrd`",
                self.show(at)
            );
            self.err(Code::TypeMismatch, n, &msg);
            return Ok(self.b.emit(Tag::Poison, NONE, NONE, Ty::POISON, n.index()));
        }
        let (v, vt) = self.trait_call_args(tr, "partial_cmp", a, at, &[(c, ct, rn)], n)?;
        let ord = self.cx.names.item("std.cmp", "Ordering");
        let ord_ty = pool.intern_ty(&TyData::Adt {
            def: ord,
            args: hd_types::TyList::EMPTY,
        });
        let sym = self.cx.names.syms.intern("order");
        let l = self.b.local(vt, sym, local_flags::ASSIGNED, n.index());
        self.b.set(l, v, n.index());
        let eq = self.cx.names.item("std.cmp", "Eq");
        // Ordering's variants: Less, Equal, Greater.
        let (first, second) = match prim {
            PrimOp::Lt => (0, None),
            PrimOp::Le => (0, Some(1)),
            PrimOp::Gt => (2, None),
            _ => (2, Some(1)),
        };
        let is = |me: &mut Self, k: u32| -> StageResult<Ref> {
            let cur = me.b.get(l, vt, n.index());
            let empty = me.b_empty_rec();
            let o = me.b.emit(Tag::NewVariant, k, empty, ord_ty, n.index());
            let some =
                me.b.coerce(hd_tir::ir::Coercion::WrapSome, NONE, o, vt, n.index());
            Ok(me
                .trait_call_args(eq, "eq", cur, vt, &[(some, vt, rn)], n)?
                .0)
        };
        let x = is(self, first)?;
        let Some(k) = second else {
            return Ok(x);
        };
        let m = self.b.open_block();
        let y = is(self, k)?;
        let blk = self.b.close_block(m, Some(y), Ty::BOOL, n.index());
        Ok(self.b.emit(Tag::Or, x.0, blk.0, Ty::BOOL, n.index()))
    }

    /// Whether a type's values have identity (`AnyRef`): data, lists,
    /// maps, trait values and `AnyRef`-bounded parameters.
    fn has_identity(&self, t: Ty) -> bool {
        let pool = self.cx.names.pool;
        let t = self.strip_mut(t);
        match pool.get(t) {
            TyData::Adt { def, .. } => !matches!(
                self.cx.lookup.item(def).map(|i| &i.data),
                Some(ItemData::Enum { .. })
            ),
            TyData::TraitValue { .. } | TyData::Never | TyData::Poison | TyData::Infer(_) => true,
            TyData::Param(_) => {
                let any_ref = self.cx.names.item("std.core", "AnyRef");
                (0..self.env.clause_self.len()).any(|i| {
                    self.env.clause_self[i] == t
                        && (self.env.clause_trait[i] == any_ref
                            || self.trait_extends(self.env.clause_trait[i], any_ref, 0))
                })
            }
            _ => false,
        }
    }

    pub(crate) fn b_empty_rec(&mut self) -> u32 {
        self.b.refs_record(&[])
    }

    /// A data field by name: its index and type under the value's
    /// arguments.
    pub(crate) fn field_of(&self, t: Ty, name: &str) -> Option<(u32, Ty)> {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        let t = match pool.get(t) {
            TyData::Mut(i) => i,
            _ => t,
        };
        let TyData::Adt { def, args } = pool.get(t) else {
            return None;
        };
        let ItemData::Data(fields) = &self.cx.lookup.item(def)?.data else {
            return None;
        };
        let sym = self.cx.names.syms.intern(name);
        let i = fields.iter().position(|f| f.name == sym)?;
        let argv = pool.list_items(args);
        let ft = pool.subst(fields[i].ty, &|p: ParamRef| {
            (p.owner == def)
                .then(|| argv.get(p.index as usize).copied())
                .flatten()
        });
        Some((u32::try_from(i).ok()?, ft))
    }

    fn field_expr(&mut self, n: NodeRef<'_>, kids: &[NodeRef<'_>]) -> StageResult<(Ref, Ty)> {
        let pool = self.cx.names.pool;
        let Some(base) = kids.first() else {
            return unsupported("a field without a base");
        };
        let name = self.cx.src.text(self.cx.src.last(n)).to_owned();
        // `Type.Variant`, `module.item`.
        if base.kind() == SyntaxKind::NameExpr
            && self.find_local(self.sym_of(*base)).is_none()
            && let Some(v) = self.static_member_value(*base, &name, n)?
        {
            return Ok(v);
        }
        let (r, bt) = self.expr(*base, None)?;
        let bt = self.infer.resolve(pool, bt);
        let inner = match pool.get(bt) {
            TyData::Mut(i) => i,
            _ => bt,
        };
        if let TyData::Tuple { elems, .. } = pool.get(inner)
            && let Ok(i) = name.trim_start_matches('_').parse::<usize>()
        {
            let items = pool.list_items(elems);
            let Some(t) = items.get(i).copied() else {
                let msg = format!("unknown-data-field `{name}` on {}", self.show(bt));
                self.err(Code::UnknownDataField, n, &msg);
                return Ok((Ref(NONE), Ty::NEVER));
            };
            let idx = u32::try_from(i).unwrap_or(0);
            return Ok((self.b.emit(Tag::TupleGet, r.0, idx, t, n.index()), t));
        }
        if let Some((idx, ft)) = self.field_of(bt, &name) {
            let ft = self.field_access(bt, &name, ft);
            return Ok((self.b.emit(Tag::Field, r.0, idx, ft, n.index()), ft));
        }
        if let Some(v) = self.shared_field(r, inner, &name, n)? {
            return Ok(v);
        }
        if matches!(pool.get(inner), TyData::Infer(_)) {
            return self.gap(n, "a field of a value whose type is not yet known");
        }
        let msg = format!("unknown-data-field `{name}` on {}", self.show(bt));
        self.err(Code::UnknownDataField, n, &msg);
        Ok((Ref(NONE), Ty::NEVER))
    }

    /// The built-in index operations of a receiver type: get, set, key
    /// type, value type.
    pub(crate) fn index_kind(&mut self, t: Ty) -> Option<(IntrinsicOp, IntrinsicOp, Ty, Ty)> {
        let pool = self.cx.names.pool;
        let t = self.infer.resolve(pool, t);
        let t = match pool.get(t) {
            TyData::Mut(i) => i,
            _ => t,
        };
        if t == Ty::STRING {
            return Some((
                IntrinsicOp::StrIndex,
                IntrinsicOp::StrIndex,
                Ty::prim(Prim::Usize),
                Ty::prim(Prim::U8),
            ));
        }
        let TyData::Adt { def, args } = pool.get(t) else {
            return None;
        };
        let argv = pool.list_items(args);
        match self.cx.names.path(def).as_str() {
            "std/core/List" => Some((
                IntrinsicOp::ListIndex,
                IntrinsicOp::ListSet,
                Ty::prim(Prim::Usize),
                *argv.first()?,
            )),
            "std/core/Map" => Some((
                IntrinsicOp::MapIndex,
                IntrinsicOp::MapSet,
                *argv.first()?,
                *argv.get(1)?,
            )),
            _ => None,
        }
    }

    fn index_expr(&mut self, n: NodeRef<'_>, kids: &[NodeRef<'_>]) -> StageResult<(Ref, Ty)> {
        let [base, key] = kids else {
            return unsupported("an index shape");
        };
        let (br, bt) = self.expr(*base, None)?;
        if key.kind() != SyntaxKind::RangeExpr
            && let Some((get, _, kt, vt)) = self.index_kind(bt)
        {
            let kr = self.index_key(*key, kt)?;
            let rec = self.b.refs_record(&[br, kr]);
            return Ok((
                self.b.emit(Tag::Intrinsic, get as u32, rec, vt, n.index()),
                vt,
            ));
        }
        let (kr, kt) = self.expr(*key, None)?;
        let index = self.cx.names.item("std.ops", "Index");
        // `expr.index.trait.no-read`.
        if !self.op_fits(index, bt, Some(kt))? {
            let msg = format!(
                "type-mismatch: {} has no `Index` implementation for this key",
                self.show(bt)
            );
            self.err(Code::TypeMismatch, n, &msg);
            return Ok((
                self.b.emit(Tag::Poison, NONE, NONE, Ty::POISON, n.index()),
                Ty::POISON,
            ));
        }
        self.trait_call_args(index, "index", br, bt, &[(kr, kt, *key)], n)
    }

    /// A built-in index key of type `kt`. A `usize` key
    /// (`expr.index.list.unsigned`) admits every unsigned type: a narrower
    /// one widens, and a `u64` beyond the `usize` range saturates, so it
    /// fails the bounds check (`expr.index.list.range`).
    pub(crate) fn index_key(&mut self, key: NodeRef<'_>, kt: Ty) -> StageResult<Ref> {
        let pool = self.cx.names.pool;
        let usize_t = Ty::prim(Prim::Usize);
        if kt != usize_t {
            let (kr, ktt) = self.expr(key, Some(kt))?;
            return Ok(self.coerce(kr, ktt, kt, key, "index"));
        }
        let (kr, ktt) = self.expr(key, Some(kt))?;
        let got = self.strip_mut(ktt);
        let p = match pool.get(got) {
            TyData::Prim(p) if p.is_unsigned() && p != Prim::Usize => p,
            _ => return Ok(self.coerce(kr, ktt, kt, key, "index")),
        };
        let at = key.index();
        if p != Prim::U64 {
            return Ok(self.b.prim(PrimOp::Conv as u32, &[kr], usize_t, at));
        }
        let max = self.b.const_value(got, u64::from(u32::MAX));
        let over = self.b.prim(PrimOp::Gt as u32, &[kr, max], Ty::BOOL, at);
        let tb = self.b.open_block();
        let big = self.b.const_value(usize_t, u64::from(u32::MAX));
        let then = self.b.close_block(tb, Some(big), usize_t, at);
        let eb = self.b.open_block();
        let small = self.b.prim(PrimOp::Conv as u32, &[kr], usize_t, at);
        let els = self.b.close_block(eb, Some(small), usize_t, at);
        let rec = self.b.refs_record(&[then, els]);
        Ok(self.b.emit(Tag::If, over.0, rec, usize_t, at))
    }

    fn list_expr(
        &mut self,
        n: NodeRef<'_>,
        kids: &[NodeRef<'_>],
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.cx.names.pool;
        let list = self.cx.names.item("std.core", "List");
        let elem = match want
            .map(|w| self.infer.resolve(pool, w))
            .map(|w| match pool.get(w) {
                TyData::Mut(i) => pool.get(i),
                d => d,
            }) {
            Some(TyData::Adt { def, args }) if def == list => {
                pool.list_items(args).first().copied()
            }
            _ => None,
        };
        let inferred = elem.is_none();
        let elem = elem.unwrap_or_else(|| self.infer.fresh(pool, VarKind::General));
        let mut refs = Vec::new();
        for e in kids {
            if e.kind() == SyntaxKind::SpreadExpr {
                return unsupported("a list spread");
            }
            if e.kind() == SyntaxKind::ComprehensionFor {
                return unsupported("a list comprehension");
            }
            let (r, t) = self.expr(*e, Some(elem))?;
            refs.push(self.coerce(r, t, elem, *e, "list element"));
        }
        // An element type solved from the elements alone is their readonly
        // view, as before permissions were tracked in inference. This keeps
        // `typing/invalid/orphan-impl-nested-trait-argument.hd` at one error;
        // types.fresh.element-permission would keep `mut` (reported).
        if inferred && let TyData::Mut(x) = pool.get(self.infer.shallow(pool, elem)) {
            self.infer.rebind(pool, elem, x);
        }
        // A fresh list has mutable access to its new outer object
        // (types.fresh.mutable-outer).
        let t = pool.intern_ty(&TyData::Adt {
            def: list,
            args: pool.list(&[elem]),
        });
        let t = pool.intern_ty(&TyData::Mut(t));
        let rec = self.b.refs_record(&refs);
        Ok((self.b.emit(Tag::NewList, NONE, rec, t, n.index()), t))
    }

    fn map_expr(
        &mut self,
        n: NodeRef<'_>,
        kids: &[NodeRef<'_>],
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.cx.names.pool;
        let map = self.cx.names.item("std.core", "Map");
        let (k, v, expected) =
            match want
                .map(|w| self.infer.resolve(pool, w))
                .map(|w| match pool.get(w) {
                    TyData::Mut(i) => pool.get(i),
                    d => d,
                }) {
                Some(TyData::Adt { def, args }) if def == map => {
                    let a = pool.list_items(args);
                    (a[0], a[1], true)
                }
                _ => (
                    self.infer.fresh(pool, VarKind::General),
                    self.infer.fresh(pool, VarKind::General),
                    false,
                ),
            };
        let mut refs = Vec::new();
        for e in kids {
            let ek: Vec<NodeRef<'_>> = e.children().collect();
            let [ke, ve] = ek.as_slice() else {
                return unsupported("a map entry shape");
            };
            let (kr, kt) = self.expr(*ke, Some(k))?;
            refs.push(self.coerce(kr, kt, k, *ke, "map key"));
            let (vr, vt) = self.expr(*ve, Some(v))?;
            refs.push(self.coerce(vr, vt, v, *ve, "map value"));
        }
        // A key type an expected map type did not fix meets the key bound
        // here (types.map-key.declared-bound).
        if !kids.is_empty() && !expected {
            self.check_map_key(k, n)?;
        }
        let t = pool.intern_ty(&TyData::Adt {
            def: map,
            args: pool.list(&[k, v]),
        });
        let t = pool.intern_ty(&TyData::Mut(t));
        let rec = self.b.refs_record(&refs);
        Ok((self.b.emit(Tag::NewMap, NONE, rec, t, n.index()), t))
    }

    /// `Name { field: value, ... }`, with `Name::[T]` or inferred arguments.
    fn data_expr(
        &mut self,
        n: NodeRef<'_>,
        kids: &[NodeRef<'_>],
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.cx.names.pool;
        let Some(name_node) = kids.first() else {
            return unsupported("a data literal without a name");
        };
        let (path_node, targ_nodes): (NodeRef<'_>, Vec<NodeRef<'_>>) =
            if name_node.kind() == SyntaxKind::TypeArgsExpr {
                let Some(inner) = name_node.children().next() else {
                    return unsupported("a data literal name");
                };
                let tl: Vec<NodeRef<'_>> = Src::child(*name_node, SyntaxKind::TypeArgumentList)
                    .map(|l| l.children().filter(|c| c.kind().is_type()).collect())
                    .unwrap_or_default();
                (inner, tl)
            } else {
                (*name_node, vec![])
            };
        let segs = self.segments_expr(path_node);
        let Some(def) = self.resolve_path(&segs).or_else(|| {
            (segs.len() == 1 && segs[0] == "Self")
                .then(|| match self.self_ty.map(|t| pool.get(t)) {
                    Some(TyData::Adt { def, .. }) => Some(def),
                    _ => None,
                })
                .flatten()
        }) else {
            let msg = format!("unknown-type `{}`", segs.join("."));
            self.err(Code::UnknownType, *name_node, &msg);
            return Ok((Ref(NONE), Ty::NEVER));
        };
        let Some(item) = self.cx.lookup.item(def) else {
            return unsupported("a data literal of an unknown item");
        };
        let ItemData::Data(fields) = item.data.clone() else {
            let msg = format!("unknown-type `{}`: not a data type", segs.join("."));
            self.err(Code::UnknownType, *name_node, &msg);
            return Ok((Ref(NONE), Ty::NEVER));
        };
        let mut targs = Vec::new();
        for t in &targ_nodes {
            targs.push(self.ty_node(*t)?);
        }
        let explicit = targs.len();
        while targs.len() < item.generics.len() {
            targs.push(self.infer.fresh(pool, VarKind::General));
        }
        // An expected type of the same declaration solves the inferred
        // arguments first, so a field sees its substituted type (`mut U`
        // included).
        if let Some(w) = want
            && let TyData::Adt { def: wd, args: wa } = pool.get(self.strip_mut(w))
            && wd == def
        {
            for (v, a) in targs.iter().zip(pool.list_items(wa)).skip(explicit) {
                let _ = self.infer.unify(pool, *v, a);
            }
        }
        let inst = |t: Ty| {
            pool.subst(t, &|p: ParamRef| {
                (p.owner == def)
                    .then(|| targs.get(p.index as usize).copied())
                    .flatten()
            })
        };
        let mut vals = vec![Ref(NONE); fields.len()];
        let mut seen = vec![false; fields.len()];
        let mut fresh_mut = true;
        for a in &kids[1..] {
            if a.kind() == SyntaxKind::SpreadExpr {
                return unsupported("a data literal spread");
            }
            let fname = self.sym_of(*a);
            let Some(i) = fields.iter().position(|f| f.name == fname) else {
                let msg = format!("unknown-data-field `{}`", self.cx.names.text(fname));
                self.err(Code::UnknownDataField, *a, &msg);
                continue;
            };
            let ft = inst(fields[i].ty);
            let (r, t) = if let Some(e) = a.children().next() {
                self.expr(e, Some(ft))?
            } else {
                // Shorthand `Name { x }`.
                let Some((l, d)) = self.find_local(fname) else {
                    let msg = format!("unknown-name `{}`", self.cx.names.text(fname));
                    self.err(Code::UnknownName, *a, &msg);
                    continue;
                };
                self.read_local(l, d, *a)
            };
            // A readonly value in a direct `mut U` field makes the literal
            // readonly (types.fresh.readonly-field).
            let ft = if matches!(pool.get(fields[i].ty), TyData::Mut(_))
                && !fields[i].embedded
                && self.is_composite(t)
                && !self.has_mut_access(t)
            {
                fresh_mut = false;
                self.readonly_view(ft)
            } else {
                ft
            };
            let ft = self.infer.resolve(pool, ft);
            vals[i] = self.coerce(r, t, ft, *a, "field");
            seen[i] = true;
        }
        let targ_list = pool.list(&targs);
        for (i, f) in fields.iter().enumerate() {
            if !seen[i] {
                if f.has_default {
                    // The field's default body, at each construction that
                    // omits it (checking-and-tir.md "Default calls").
                    let fname = self.cx.names.text(f.name).to_owned();
                    let body = self.cx.names.member(def, PathKind::Hidden, &fname);
                    let a = self.b.refs_record(&[Ref(body.raw()), Ref(targ_list.0)]);
                    let bw = self.b.refs_record(&[]);
                    let ft = inst(f.ty);
                    vals[i] = self.b.emit(Tag::DefaultCall, a, bw, ft, n.index());
                    continue;
                }
                let msg = format!(
                    "missing-required-field `{}` in `{}`",
                    self.cx.names.text(f.name),
                    segs.join(".")
                );
                self.err(Code::MissingRequiredField, n, &msg);
            }
        }
        let ty = pool.intern_ty(&TyData::Adt {
            def,
            args: pool.list(&targs),
        });
        let ty = if fresh_mut {
            pool.intern_ty(&TyData::Mut(ty))
        } else {
            ty
        };
        let rec = self.b.refs_record(&vals);
        Ok((self.b.emit(Tag::NewData, NONE, rec, ty, n.index()), ty))
    }

    /// The identifier segments of a name or field-path expression.
    pub(crate) fn segments_expr(&self, n: NodeRef<'_>) -> Vec<String> {
        match n.kind() {
            SyntaxKind::NameExpr => vec![self.cx.src.text(self.cx.src.first(n)).to_owned()],
            SyntaxKind::FieldExpr | SyntaxKind::PathExpr => {
                let mut v = n
                    .children()
                    .next()
                    .map(|b| self.segments_expr(b))
                    .unwrap_or_default();
                v.push(self.cx.src.text(self.cx.src.last(n)).to_owned());
                v
            }
            _ => vec![],
        }
    }

    /// `if c: a else: b` as a statement or a value.
    pub(crate) fn if_expr(&mut self, e: NodeRef<'_>, want: Option<Ty>) -> StageResult<(Ref, Ty)> {
        let pool = self.cx.names.pool;
        let Some(cond) = e.children().next() else {
            return unsupported("an `if` without a condition");
        };
        let Some(then_node) = Src::child(e, SyntaxKind::Block) else {
            return unsupported("an `if` without a block");
        };
        let clause = Src::child(e, SyntaxKind::ElseClause);
        let (c, ct) = self.expr(cond, Some(Ty::BOOL))?;
        self.expect(ct, Ty::BOOL, cond, "condition");
        // A value `if` needs an `else`; without one it is a statement.
        let want = if clause.is_none() {
            Some(Ty::VOID)
        } else {
            want
        };
        let result = match want {
            Some(w) => w,
            None => self.infer.fresh(pool, VarKind::General),
        };
        let tb = self.b.open_block();
        let (tt, tty) = self.block_value(then_node, Some(result))?;
        let then = self.b.close_block(tb, tt, tty, then_node.index());
        let (els, ety) = if let Some(clause) = clause {
            let eb = self.b.open_block();
            let (tail, ty) = if let Some(nested) = Src::child(clause, SyntaxKind::IfExpr) {
                let (r, t) = self.if_expr(nested, Some(result))?;
                let t = self.infer.shallow(pool, t);
                if t == Ty::VOID || r.0 == NONE {
                    (None, t)
                } else {
                    (Some(r), t)
                }
            } else {
                let Some(blk) = Src::child(clause, SyntaxKind::Block) else {
                    return unsupported("an `else` without a block");
                };
                self.block_value(blk, Some(result))?
            };
            (self.b.close_block(eb, tail, ty, clause.index()), ty)
        } else {
            (Ref(NONE), Ty::VOID)
        };
        let both_never = tty == Ty::NEVER && ety == Ty::NEVER;
        let ty = if both_never { Ty::NEVER } else { result };
        let rec = self.b.refs_record(&[then, els]);
        Ok((self.b.emit(Tag::If, c.0, rec, ty, e.index()), ty))
    }

    /// `while c: body` is `Loop { Block { if c { body } else { break } } }`.
    fn while_expr(&mut self, e: NodeRef<'_>, want: Option<Ty>) -> StageResult<(Ref, Ty)> {
        let Some(cond) = e.children().next() else {
            return unsupported("a `while` without a condition");
        };
        let Some(body) = Src::child(e, SyntaxKind::Block) else {
            return unsupported("a `while` without a block");
        };
        let els = self.loop_else_of(e, want)?;
        let result = els.map(|(_, t)| t);
        let lp = self.b.open_loop();
        let lb = self.b.open_block();
        let (c, ct) = self.expr(cond, Some(Ty::BOOL))?;
        self.expect(ct, Ty::BOOL, cond, "condition");
        self.loops.push((lp, result));
        let tb = self.b.open_block();
        self.block_value(body, Some(Ty::VOID))?;
        let then = self.b.close_block(tb, None, Ty::VOID, body.index());
        self.loops.pop();
        let eb = self.b.open_block();
        self.loop_exit(lp, els, e.index())?;
        let exit = self.b.close_block(eb, None, Ty::VOID, e.index());
        let rec = self.b.refs_record(&[then, exit]);
        self.b.emit(Tag::If, c.0, rec, Ty::VOID, e.index());
        let lbody = self.b.close_block(lb, None, Ty::VOID, e.index());
        // `flow.while.infinite`: `while true`, or `while (true)`, without
        // a `break` never completes.
        let mut lit = cond;
        while lit.kind() == SyntaxKind::ParenExpr
            && let Some(inner) = lit.children().next()
        {
            lit = inner;
        }
        let infinite = lit.kind() == SyntaxKind::LiteralExpr
            && self.cx.src.tkind(self.cx.src.first(lit)) == Some(TokenKind::KwTrue)
            && !Self::has_break(body);
        let ty = if infinite {
            Ty::NEVER
        } else {
            result.unwrap_or(Ty::VOID)
        };
        Ok((self.b.close_loop(lp, lbody, ty, e.index()), ty))
    }

    /// A loop's `else` suite (`flow.loop.else.value`) and the loop's
    /// result type.
    fn loop_else_of(
        &mut self,
        e: NodeRef<'_>,
        want: Option<Ty>,
    ) -> StageResult<Option<(hd_base::NodeIdx, Ty)>> {
        let Some(clause) = Src::child(e, SyntaxKind::ElseClause) else {
            return Ok(None);
        };
        let Some(blk) = Src::child(clause, SyntaxKind::Block) else {
            return unsupported("a loop `else` without a block");
        };
        let pool = self.cx.names.pool;
        let t = match want {
            Some(w) => w,
            None => self.infer.fresh(pool, VarKind::General),
        };
        Ok(Some((blk.index(), t)))
    }

    /// `flow.for.pattern.irrefutable`: a `for` pattern must always match.
    fn irrefutable_loop_pattern(&mut self, pat: NodeRef<'_>, t: Ty) {
        if self.refutable(pat, t) {
            self.err(
                Code::RefutableLetPattern,
                pat,
                "refutable-let-pattern: a `for` pattern must match every item",
            );
        }
    }

    /// A loop's normal exit: its `else` suite's value, then `break`.
    pub(crate) fn loop_exit(
        &mut self,
        lp: hd_tir::ir::LoopMark,
        els: Option<(hd_base::NodeIdx, Ty)>,
        at: hd_base::NodeIdx,
    ) -> StageResult<()> {
        let v = match els {
            None => NONE,
            Some((blk, t)) => {
                let node = self.cx.src.parse.tree.node(blk);
                let (tail, _) = self.block_value(node, Some(t))?;
                tail.map_or(NONE, |r| r.0)
            }
        };
        self.b.emit(Tag::Break, lp.0.raw(), v, Ty::NEVER, at);
        Ok(())
    }

    fn has_break(n: NodeRef<'_>) -> bool {
        n.children().any(|c| match c.kind() {
            SyntaxKind::BreakStmt => true,
            SyntaxKind::WhileExpr | SyntaxKind::ForExpr | SyntaxKind::ClosureExpr => false,
            _ => Self::has_break(c),
        })
    }

    /// `for p in e: body` over the iterator protocol: `iter` (unless `e`
    /// is already an `Iterator`), then a loop around `next` and a match on
    /// its option (checking-and-tir.md "What The Checker Desugars").
    fn for_expr(&mut self, e: NodeRef<'_>, want: Option<Ty>) -> StageResult<(Ref, Ty)> {
        self.loop_else = self.loop_else_of(e, want)?;
        let kids: Vec<NodeRef<'_>> = e.children().collect();
        let (Some(pat), Some(src), Some(body)) =
            (kids.first(), kids.get(1), Src::child(e, SyntaxKind::Block))
        else {
            return unsupported("a `for` shape");
        };
        self.iterate(*pat, *src, e, &mut |me: &mut Self| {
            me.block_value(body, Some(Ty::VOID)).map(|_| ())
        })
    }

    /// The loop of a `for` or a comprehension clause: `pat` binds each item
    /// of `src` while `body` runs.
    fn iterate(
        &mut self,
        pat: NodeRef<'_>,
        src: NodeRef<'_>,
        e: NodeRef<'_>,
        body: &mut dyn FnMut(&mut Self) -> StageResult<()>,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.cx.names.pool;
        let els = self.loop_else.take();
        let (sr, st) = self.expr(src, None)?;
        let iterator = self.cx.names.item("std.iter", "Iterator");
        let st_r = self.infer.resolve(pool, st);
        let st_i = match pool.get(st_r) {
            TyData::Mut(i) => i,
            _ => st_r,
        };
        if let TyData::Adt { def, args } = pool.get(st_i)
            && self.cx.names.path(def) == "std/ops/Range"
            && matches!(
                pat.kind(),
                SyntaxKind::BindingPattern | SyntaxKind::WildcardPattern
            )
        {
            let et = pool.list_items(args).first().copied().unwrap_or(Ty::POISON);
            if self.numeric(et) {
                return self.range_loop(pat, (sr, et), els, e, body);
            }
        }
        let (it, item_ty) = match pool.get(st_i) {
            TyData::Adt { def, args } if def == iterator => {
                // The loop advances the iterator itself (flow.for.iterator-mut).
                if !self.has_mut_access(st) {
                    self.err(
                        Code::MutableReceiverRequired,
                        src,
                        "mutable-receiver-required: a loop advances its iterator, which needs `mut` access",
                    );
                }
                (
                    sr,
                    pool.list_items(args).first().copied().unwrap_or(Ty::POISON),
                )
            }
            TyData::Infer(_) => return unsupported("a `for` over a value whose type is not known"),
            // Every std range is `Iterable[T] for Range[T]` (and `RangeFrom`).
            TyData::Adt { def, args }
                if matches!(
                    self.cx.names.path(def).as_str(),
                    "std/ops/Range" | "std/ops/RangeFrom"
                ) =>
            {
                let et = pool.list_items(args).first().copied().unwrap_or(Ty::POISON);
                let iterable = self.cx.names.item("std.iter", "Iterable");
                let tref = hd_types::solver::TraitRef {
                    trait_: iterable,
                    self_ty: st_i,
                    args: pool.list(&[et]),
                };
                // The element is often a literal still open here: its
                // impl is chosen when the loop's uses fix it.
                if !pool.has_infer(self.infer.resolve(pool, et)) {
                    self.require_ref(tref, src)?;
                }
                let method = self.cx.names.member(iterable, PathKind::Member, "iter");
                let it_t = pool.intern_ty(&TyData::Adt {
                    def: iterator,
                    args: pool.list(&[et]),
                });
                let c = hd_tir::ir::Callee::TraitMethod {
                    trait_: iterable,
                    method,
                    self_ty: st_i,
                    targs: pool.list(&[et]),
                    choice: (hd_tir::ir::ChoiceKind::Builtin, 0),
                };
                let r = self
                    .b
                    .call(&c, &[sr], hd_tir::ir::Providers::None, it_t, src.index());
                (r, et)
            }
            _ => {
                let iterable = self.cx.names.item("std.iter", "Iterable");
                let (r, t) = self.trait_call(iterable, "iter", sr, st, &[], src)?;
                let t = self.infer.resolve(pool, t);
                let t = match pool.get(t) {
                    TyData::Mut(i) => i,
                    _ => t,
                };
                match pool.get(t) {
                    TyData::Adt { def, args } if def == iterator => (
                        r,
                        pool.list_items(args).first().copied().unwrap_or(Ty::POISON),
                    ),
                    _ => return unsupported("an `iter` that does not return an `Iterator`"),
                }
            }
        };
        let it_ty = pool.intern_ty(&TyData::Adt {
            def: iterator,
            args: pool.list(&[item_ty]),
        });
        let it_sym = self.cx.names.syms.intern("$iter");
        let itl = self
            .b
            .local(it_ty, it_sym, hd_tir::ir::local_flags::ASSIGNED, e.index());
        self.b.set(itl, it, e.index());
        let lp = self.b.open_loop();
        let lb = self.b.open_block();
        let cur = self.b.get(itl, it_ty, e.index());
        let (next, nt) = self.inherent_call(it_ty, "next", cur, &[], e)?;
        let _ = nt;
        let opt = pool.intern_ty(&TyData::Option(item_ty));
        let result = els.map(|(_, t)| t);
        self.loops.push((lp, result));
        self.scopes.push(HashMap::new());
        let mut binds = Vec::new();
        let before = self.diags.len();
        self.declare_pattern(pat, item_ty, &mut binds)?;
        // A pattern that does not fit the item type already reported; its
        // refutability would only repeat that error.
        if self.diags.len() == before {
            self.irrefutable_loop_pattern(pat, item_ty);
        }
        let ab = self.b.open_block();
        body(self)?;
        let arm0 = self.b.close_block(ab, None, Ty::VOID, e.index());
        self.scopes.pop();
        self.loops.pop();
        let bb = self.b.open_block();
        self.loop_exit(lp, els, e.index())?;
        let arm1 = self.b.close_block(bb, None, Ty::VOID, e.index());
        // Decision: `.Some(p)` to arm 0, `.None` to arm 1.
        let rows = vec![crate::pat::Row::some(pat, binds), crate::pat::Row::rest()];
        let db = self.b.open_block();
        self.decide(&rows, 0, next, opt)?;
        let dec = self.b.close_block(db, None, Ty::NEVER, e.index());
        let rec = self.b.refs_record(&[dec, arm0, arm1]);
        self.b.emit(Tag::Match, next.0, rec, Ty::VOID, e.index());
        let lbody = self.b.close_block(lb, None, Ty::VOID, e.index());
        let ty = result.unwrap_or(Ty::VOID);
        Ok((self.b.close_loop(lp, lbody, ty, e.index()), ty))
    }

    /// A `for` over a numeric `Range` as a counting loop (lowering-catalog
    /// "for over a range"): no iterator value. Each round tests
    /// `cur < end || (inclusive && cur == end)`, binds the item, then
    /// advances before the body runs, so `continue` needs no step of its
    /// own. At the inclusive end it clears `inclusive` instead of adding,
    /// so `0..=255u8` cannot overflow.
    fn range_loop(
        &mut self,
        pat: NodeRef<'_>,
        (sr, et): (Ref, Ty),
        loop_else: Option<(hd_base::NodeIdx, Ty)>,
        e: NodeRef<'_>,
        body: &mut dyn FnMut(&mut Self) -> StageResult<()>,
    ) -> StageResult<(Ref, Ty)> {
        use hd_tir::ir::local_flags::ASSIGNED;
        let result = loop_else.map(|(_, t)| t);
        let at = e.index();
        let sym = |me: &Self, s: &str| me.cx.names.syms.intern(s);
        let (cur_s, end_s, inc_s) = (sym(self, "$cur"), sym(self, "$end"), sym(self, "$incl"));
        let cur_l = self.b.local(et, cur_s, ASSIGNED, at);
        let end_l = self.b.local(et, end_s, ASSIGNED, at);
        let inc_l = self.b.local(Ty::BOOL, inc_s, ASSIGNED, at);
        let start = self.b.emit(Tag::Field, sr.0, 0, et, at);
        self.b.set(cur_l, start, at);
        let end = self.b.emit(Tag::Field, sr.0, 1, et, at);
        self.b.set(end_l, end, at);
        let inc = self.b.emit(Tag::Field, sr.0, 2, Ty::BOOL, at);
        self.b.set(inc_l, inc, at);
        let lt = PrimOp::Lt as u32;
        let lp = self.b.open_loop();
        let lb = self.b.open_block();
        // The test.
        let (c, d) = (self.b.get(cur_l, et, at), self.b.get(end_l, et, at));
        let below = self.b.prim(lt, &[c, d], Ty::BOOL, at);
        let ob = self.b.open_block();
        let incv = self.b.get(inc_l, Ty::BOOL, at);
        let ab = self.b.open_block();
        let (c, d) = (self.b.get(cur_l, et, at), self.b.get(end_l, et, at));
        let at_end = self.b.prim(PrimOp::Eq as u32, &[c, d], Ty::BOOL, at);
        let ablk = self.b.close_block(ab, Some(at_end), Ty::BOOL, at);
        let both = self.b.emit(Tag::And, incv.0, ablk.0, Ty::BOOL, at);
        let oblk = self.b.close_block(ob, Some(both), Ty::BOOL, at);
        let go = self.b.emit(Tag::Or, below.0, oblk.0, Ty::BOOL, at);
        // The round: bind, advance, then the body.
        let tb = self.b.open_block();
        self.loops.push((lp, result));
        self.scopes.push(HashMap::new());
        let item = self.b.get(cur_l, et, at);
        let mut binds = Vec::new();
        self.declare_pattern(pat, et, &mut binds)?;
        for (_, l) in &binds {
            self.b.set(*l, item, at);
        }
        let (c, d) = (self.b.get(cur_l, et, at), self.b.get(end_l, et, at));
        let more = self.b.prim(lt, &[c, d], Ty::BOOL, at);
        let ib = self.b.open_block();
        let c = self.b.get(cur_l, et, at);
        let one = self.b.const_value(et, 1);
        let next = self.b.prim(PrimOp::Add as u32, &[c, one], et, at);
        self.b.set(cur_l, next, at);
        let iblk = self.b.close_block(ib, None, Ty::VOID, at);
        let fb = self.b.open_block();
        let no = self.b.const_value(Ty::BOOL, 0);
        self.b.set(inc_l, no, at);
        let fblk = self.b.close_block(fb, None, Ty::VOID, at);
        let rec = self.b.refs_record(&[iblk, fblk]);
        self.b.emit(Tag::If, more.0, rec, Ty::VOID, at);
        body(self)?;
        self.scopes.pop();
        self.loops.pop();
        let then = self.b.close_block(tb, None, Ty::VOID, at);
        let eb = self.b.open_block();
        self.loop_exit(lp, loop_else, at)?;
        let els = self.b.close_block(eb, None, Ty::VOID, at);
        let rec = self.b.refs_record(&[then, els]);
        self.b.emit(Tag::If, go.0, rec, Ty::VOID, at);
        let lbody = self.b.close_block(lb, None, Ty::VOID, at);
        let ty = result.unwrap_or(Ty::VOID);
        Ok((self.b.close_loop(lp, lbody, ty, at), ty))
    }

    /// `_` in a pipe step: the piped value.
    fn placeholder_value(&mut self, n: NodeRef<'_>) -> (Ref, Ty) {
        if let Some(v) = self.placeholder.take() {
            return v;
        }
        self.err(
            Code::PlaceholderOutsidePipe,
            n,
            "placeholder-outside-pipe: `_` stands for a pipe's value only in its step",
        );
        (Ref(NONE), Ty::NEVER)
    }

    /// `a |> f(_, b)`: `a` first, then the step with `a` in the
    /// placeholder's slot (checking-and-tir.md "What The Checker Desugars").
    fn pipe(&mut self, kids: &[NodeRef<'_>], want: Option<Ty>) -> StageResult<(Ref, Ty)> {
        let [lhs, step] = kids else {
            return unsupported("a pipe shape");
        };
        let v = self.expr(*lhs, None)?;
        let saved = self.placeholder.replace(v);
        let r = self.expr(*step, want);
        let unused = self.placeholder.take().is_some();
        self.placeholder = saved;
        if unused {
            self.err(
                Code::PipeStepNeedsPlaceholder,
                *step,
                "pipe-step-needs-placeholder: write `_` where the piped value goes",
            );
        }
        r
    }

    /// `[for p in xs if c => e]` and `{for p in xs => k: v}`: an empty
    /// list or map, the clauses' loops and tests, a push or an insert.
    fn comprehension(
        &mut self,
        n: NodeRef<'_>,
        kids: &[NodeRef<'_>],
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.cx.names.pool;
        let is_map = self.cx.src.tkind(self.cx.src.first(n)) == Some(TokenKind::LBrace);
        let Some((element, clauses)) = kids.split_last() else {
            return unsupported("an empty comprehension");
        };
        let want = want.map(|w| self.strip_mut(w));
        let (def, args) = if is_map {
            let map = self.cx.names.item("std.core", "Map");
            let a = match want.map(|w| pool.get(w)) {
                Some(TyData::Adt { def, args }) if def == map => pool.list_items(args),
                _ => vec![
                    self.infer.fresh(pool, VarKind::General),
                    self.infer.fresh(pool, VarKind::General),
                ],
            };
            (map, a)
        } else {
            let list = self.cx.names.item("std.core", "List");
            let a = match want.map(|w| pool.get(w)) {
                Some(TyData::Adt { def, args }) if def == list => pool.list_items(args),
                _ => vec![self.infer.fresh(pool, VarKind::General)],
            };
            (list, a)
        };
        let t = pool.intern_ty(&TyData::Adt {
            def,
            args: pool.list(&args),
        });
        let mt = pool.intern_ty(&TyData::Mut(t));
        let acc_sym = self.cx.names.syms.intern("$acc");
        let acc = self
            .b
            .local(mt, acc_sym, hd_tir::ir::local_flags::ASSIGNED, n.index());
        let empty = self.b.refs_record(&[]);
        let tag = if is_map { Tag::NewMap } else { Tag::NewList };
        let fresh = self.b.emit(tag, NONE, empty, mt, n.index());
        self.b.set(acc, fresh, n.index());
        self.scopes.push(HashMap::new());
        let r = self.clauses(clauses, *element, (acc, mt), &args, n);
        self.scopes.pop();
        r?;
        Ok((self.b.get(acc, mt, n.index()), mt))
    }

    fn clauses(
        &mut self,
        clauses: &[NodeRef<'_>],
        last: NodeRef<'_>,
        acc: (hd_base::LocalId, Ty),
        args: &[Ty],
        n: NodeRef<'_>,
    ) -> StageResult<()> {
        let Some((c, rest)) = clauses.split_first() else {
            // The element: a push, or a map insert.
            let a = self.b.get(acc.0, acc.1, n.index());
            if last.kind() == SyntaxKind::MapEntry {
                let ek: Vec<NodeRef<'_>> = last.children().collect();
                let [k, v] = ek.as_slice() else {
                    return unsupported("a map comprehension entry");
                };
                let (kr, kt) = self.expr(*k, Some(args[0]))?;
                let kr = self.coerce(kr, kt, args[0], *k, "map key");
                let (vr, vt) = self.expr(*v, Some(args[1]))?;
                let vr = self.coerce(vr, vt, args[1], *v, "map value");
                let rec = self.b.refs_record(&[a, kr, vr]);
                self.b.emit(
                    Tag::Intrinsic,
                    IntrinsicOp::MapSet as u32,
                    rec,
                    Ty::VOID,
                    n.index(),
                );
            } else {
                let (r, t) = self.expr(last, Some(args[0]))?;
                let r = self.coerce(r, t, args[0], last, "element");
                let rec = self.b.refs_record(&[a, r]);
                self.b.emit(
                    Tag::Intrinsic,
                    IntrinsicOp::ListPush as u32,
                    rec,
                    Ty::VOID,
                    n.index(),
                );
            }
            return Ok(());
        };
        match c.kind() {
            SyntaxKind::ComprehensionFor => {
                let ck: Vec<NodeRef<'_>> = c.children().collect();
                let [pat, src] = ck.as_slice() else {
                    return unsupported("a comprehension `for` shape");
                };
                self.iterate(*pat, *src, *c, &mut |me: &mut Self| {
                    me.clauses(rest, last, acc, args, n)
                })?;
            }
            SyntaxKind::ComprehensionIf => {
                let Some(cond) = c.children().next() else {
                    return unsupported("a comprehension `if` shape");
                };
                let (cr, ct) = self.expr(cond, Some(Ty::BOOL))?;
                self.expect(ct, Ty::BOOL, cond, "condition");
                let tb = self.b.open_block();
                self.clauses(rest, last, acc, args, n)?;
                let then = self.b.close_block(tb, None, Ty::VOID, c.index());
                let rec = self.b.refs_record(&[then, Ref(NONE)]);
                self.b.emit(Tag::If, cr.0, rec, Ty::VOID, c.index());
            }
            _ => return unsupported("this comprehension clause"),
        }
        Ok(())
    }

    /// `fn(params) -> R: body`: a sub-body with its captures.
    fn closure(&mut self, n: NodeRef<'_>, want: Option<Ty>) -> StageResult<(Ref, Ty)> {
        let pool = self.cx.names.pool;
        let wanted = want
            .map(|w| self.infer.resolve(pool, w))
            .map(|w| match pool.get(w) {
                TyData::Mut(i) => pool.get(i),
                d => d,
            });
        let (wparams, wret, wrow) = match wanted {
            Some(TyData::Fn {
                params,
                result,
                row,
                ..
            }) => (pool.list_items(params), Some(result), row),
            _ => (vec![], None, hd_types::RowId::EMPTY),
        };
        let suspends = n
            .direct_token(&self.cx.src.parse.tokens, TokenKind::Bang)
            .is_some();
        let mut params = Vec::new();
        let mut ptys = Vec::new();
        self.scopes.push(HashMap::new());
        if let Some(pl) = Src::child(n, SyntaxKind::ParameterList) {
            for (i, p) in pl
                .children()
                .filter(|c| c.kind() == SyntaxKind::Parameter)
                .enumerate()
            {
                let toks = &self.cx.src.parse.tokens;
                let name = p
                    .name(toks)
                    .map_or_else(|| format!("${i}"), |t| self.cx.src.text(t).to_owned());
                let t = match Src::type_child(p) {
                    Some(tn) => self.ty_node(tn)?,
                    None => wparams
                        .get(i)
                        .copied()
                        .unwrap_or_else(|| self.infer.fresh(pool, VarKind::General)),
                };
                let sym = self.cx.names.syms.intern(&name);
                let l = self
                    .b
                    .local(t, sym, hd_tir::ir::local_flags::PARAM, p.index());
                self.scopes.last_mut().expect("scope").insert(sym, l);
                params.push(l);
                ptys.push(t);
            }
        }
        let ret = match n
            .children()
            .find(|c| c.kind().is_type() && c.kind() != SyntaxKind::RequirementRow)
        {
            Some(rt) => self.ty_node(rt)?,
            None => wret.unwrap_or_else(|| self.infer.fresh(pool, VarKind::General)),
        };
        let row = match Src::child(n, SyntaxKind::RequirementRow) {
            Some(r) => self.row_of(r)?,
            None => wrow,
        };
        let mark = self.b.open_sub(&params);
        self.subs.push(OpenSub {
            mark,
            depth: self.scopes.len() - 1,
        });
        self.rets.push(ret);
        self.rows.push(row);
        self.suspends.push(suspends);
        let saved_loops = std::mem::take(&mut self.loops);
        let blk = self.b.open_block();
        let body = Src::child(n, SyntaxKind::Block);
        let (tail, _) = if let Some(bn) = body {
            self.block_value(bn, Some(ret))?
        } else {
            // An inline suite: one expression.
            let Some(e) = n.children().filter(|c| !c.kind().is_type()).last() else {
                return unsupported("a closure without a body");
            };
            let (r, t) = self.expr(e, Some(ret))?;
            (Some(self.coerce(r, t, ret, e, "result")), ret)
        };
        let root = self.b.close_block(blk, tail, ret, n.index());
        self.loops = saved_loops;
        self.rows.pop();
        self.rets.pop();
        self.suspends.pop();
        self.subs.pop();
        self.scopes.pop();
        let ft = pool.intern_ty(&TyData::Fn {
            params: pool.list(&ptys),
            result: ret,
            row,
            suspends,
        });
        Ok((self.b.close_sub(mark, root, ft, n.index()), ft))
    }

    /// `e?` on a `Result` or an `Option`: the success payload, or an early
    /// return of the failure (converted by `From` when the error types
    /// differ).
    fn try_expr(&mut self, n: NodeRef<'_>, kids: &[NodeRef<'_>]) -> StageResult<(Ref, Ty)> {
        let pool = self.cx.names.pool;
        let Some(e) = kids.first() else {
            return unsupported("a `?` without an operand");
        };
        let (r, t) = self.expr(*e, None)?;
        let t = self.infer.resolve(pool, t);
        let ret = self.infer.resolve(pool, *self.rets.last().expect("ret"));
        let result = self.cx.names.item("std.core", "Result");
        match (pool.get(t), pool.get(ret)) {
            (TyData::Option(inner), TyData::Option(_)) => {
                let ok_b = self.b.open_block();
                let v = self.b.emit(Tag::Unwrap, r.0, NONE, inner, n.index());
                let ok = self.b.close_block(ok_b, Some(v), inner, n.index());
                let none_b = self.b.open_block();
                let empty = self.b_empty_rec();
                let nv = self.b.emit(Tag::NewVariant, 0, empty, ret, n.index());
                self.b.emit(Tag::Return, nv.0, NONE, Ty::NEVER, n.index());
                let none = self.b.close_block(none_b, None, Ty::NEVER, n.index());
                let rec = self.b.refs_record(&[Ref(1), ok, none]);
                Ok((
                    self.b.emit(Tag::SwitchTag, r.0, rec, inner, n.index()),
                    inner,
                ))
            }
            (TyData::Adt { def: d1, args: a1 }, TyData::Adt { def: d2, args: a2 })
                if d1 == result && d2 == result =>
            {
                let a1 = pool.list_items(a1);
                let a2 = pool.list_items(a2);
                let (okt, et, ret_e) = (a1[0], a1[1], a2[1]);
                let ok_b = self.b.open_block();
                let pr = self.b.refs_record(&[Ref(0), Ref(0)]);
                let v = self.b.emit(Tag::Payload, r.0, pr, okt, n.index());
                let ok = self.b.close_block(ok_b, Some(v), okt, n.index());
                let err_b = self.b.open_block();
                let pr = self.b.refs_record(&[Ref(1), Ref(0)]);
                let ev = self.b.emit(Tag::Payload, r.0, pr, et, n.index());
                let ev = if self.can_unify(et, ret_e) {
                    self.expect(et, ret_e, *e, "error");
                    ev
                } else if matches!(
                    pool.get(self.infer.resolve(pool, ret_e)),
                    TyData::TraitValue { .. }
                ) {
                    // Into an erased error such as `dyn Error`: a trait-value
                    // coercion (`expr.try.test.converts`).
                    self.coerce(ev, et, ret_e, *e, "error")
                } else {
                    let from = self.cx.names.item("std.convert", "From");
                    let tref = hd_types::solver::TraitRef {
                        trait_: from,
                        self_ty: ret_e,
                        args: pool.list(&[et]),
                    };
                    let _ = self.require_ref(tref, n)?;
                    let method = self.cx.names.member(from, PathKind::Member, "from");
                    let c = hd_tir::ir::Callee::TraitMethod {
                        trait_: from,
                        method,
                        self_ty: ret_e,
                        targs: pool.list(&[et]),
                        choice: (hd_tir::ir::ChoiceKind::Builtin, u32::MAX),
                    };
                    self.b
                        .call(&c, &[ev], hd_tir::ir::Providers::None, ret_e, n.index())
                };
                let rec = self.b.refs_record(&[ev]);
                let errv = self.b.emit(Tag::NewVariant, 1, rec, ret, n.index());
                self.b.emit(Tag::Return, errv.0, NONE, Ty::NEVER, n.index());
                let err = self.b.close_block(err_b, None, Ty::NEVER, n.index());
                let rec = self.b.refs_record(&[Ref(0), ok, err]);
                Ok((self.b.emit(Tag::SwitchTag, r.0, rec, okt, n.index()), okt))
            }
            _ => {
                if pool.has_infer(t) || pool.has_infer(ret) {
                    return unsupported("`?` on a value whose type is not yet known");
                }
                self.err(
                    Code::InvalidResultPropagation,
                    n,
                    "invalid-result-propagation: `?` needs a Result or an optional in a function returning one",
                );
                Ok((Ref(NONE), Ty::NEVER))
            }
        }
    }

    /// `a..b`, `a..=b`, `a..`, `..b`: a value of std's range types.
    fn range_expr(
        &mut self,
        n: NodeRef<'_>,
        kids: &[NodeRef<'_>],
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.cx.names.pool;
        let inclusive = n
            .direct_token(&self.cx.src.parse.tokens, TokenKind::DotDotEq)
            .is_some();
        let _ = want;
        let first_tok = self.cx.src.first(n);
        let starts_open = matches!(
            self.cx.src.tkind(first_tok),
            Some(TokenKind::DotDot | TokenKind::DotDotEq)
        );
        let (name, fields): (&str, Vec<NodeRef<'_>>) = match (kids.len(), starts_open) {
            (2, _) => ("Range", kids.to_vec()),
            (1, true) => ("RangeTo", kids.to_vec()),
            (1, false) => ("RangeFrom", kids.to_vec()),
            _ => return unsupported("a full range `..`"),
        };
        let def = self.cx.names.item("std.ops", name);
        let et = self.infer.fresh(pool, VarKind::General);
        let mut refs = Vec::new();
        for f in &fields {
            let (r, t) = self.expr(*f, Some(et))?;
            refs.push(self.coerce(r, t, et, *f, "range bound"));
        }
        if name != "RangeFrom" {
            refs.push(self.b.const_value(Ty::BOOL, u64::from(inclusive)));
        }
        let t = pool.intern_ty(&TyData::Adt {
            def,
            args: pool.list(&[et]),
        });
        let rec = self.b.refs_record(&refs);
        Ok((self.b.emit(Tag::NewData, NONE, rec, t, n.index()), t))
    }

    /// `$.use(K)`: the covering provider of `K`, which the body's row must
    /// name (spec/lang/11-requirements-and-suspension.md).
    /// `$.with(K = p, ...): block` (`req.with`): each provider is
    /// evaluated, must implement its key, and covers the key in the block.
    fn with_expr(&mut self, n: NodeRef<'_>, want: Option<Ty>) -> StageResult<(Ref, Ty)> {
        let pool = self.cx.names.pool;
        let Some(al) = Src::child(n, SyntaxKind::ArgumentList) else {
            return unsupported("a `$.with` without providers");
        };
        let mut pairs = Vec::new();
        let mut keys = Vec::new();
        for entry in al
            .children()
            .filter(|c| c.kind() == SyntaxKind::ContextEntry)
        {
            let kids: Vec<NodeRef<'_>> = entry.children().collect();
            let [kn, vn] = kids.as_slice() else {
                return unsupported("a `$.with` spread of a context");
            };
            let key = self.ty_node(*kn)?;
            let TyData::TraitValue { def, args, .. } = pool.get(key) else {
                return unsupported("a `$.with` key that is not a trait");
            };
            let (r, t) = self.expr(*vn, None)?;
            // A mutable requirement trait needs a `mut` provider.
            if self.trait_is_mutable(def, 0) && self.is_composite(t) && !self.has_mut_access(t) {
                let msg = format!(
                    "mutable-upgrade: `{}` has `mut self` methods, so its provider needs `mut` access, but this is a readonly {}",
                    self.cx.names.path(def),
                    self.show(t)
                );
                self.err(Code::MutableUpgrade, *vn, &msg);
            }
            let tref = hd_types::solver::TraitRef {
                trait_: def,
                self_ty: self.strip_mut(t),
                args,
            };
            self.require_ref(tref, *vn)?;
            pairs.push(Ref(key.0));
            pairs.push(r);
            keys.push(key);
        }
        let Some(body) = Src::child(n, SyntaxKind::Block) else {
            return unsupported("a `$.with` without a block");
        };
        let row = pool.row(&hd_types::RowData {
            keys,
            params: vec![],
        });
        self.rows.push(row);
        let bm = self.b.open_block();
        let r = self.block_value(body, want);
        self.rows.pop();
        let (tail, ty) = r?;
        let blk = self.b.close_block(bm, tail, ty, body.index());
        let rec = self.b.refs_record(&pairs);
        Ok((self.b.emit(Tag::With, rec, blk.0, ty, n.index()), ty))
    }

    fn context_expr(&mut self, n: NodeRef<'_>, want: Option<Ty>) -> StageResult<(Ref, Ty)> {
        let pool = self.cx.names.pool;
        let toks: Vec<String> = self
            .cx
            .src
            .tokens(n)
            .take(4)
            .map(|t| self.cx.src.text(t).to_owned())
            .collect();
        if toks.get(2).map(String::as_str) == Some("with") {
            return self.with_expr(n, want);
        }
        if toks.get(2).map(String::as_str) != Some("use") {
            return unsupported("`$.context` and context types");
        }
        let Some(kn) = n.children().find(|c| c.kind().is_type()) else {
            return unsupported("`$.use` without a key");
        };
        let key = self.ty_node(kn)?;
        let TyData::TraitValue { def: key_trait, .. } = pool.get(key) else {
            return unsupported("a `$.use` key that is not a trait");
        };
        self.check_row_has(key_trait, n);
        let at = self.b.refs_record(&[Ref(key.0)]);
        // `mut K` for a mutable requirement trait, else `K`.
        let pt = if self.trait_is_mutable(key_trait, 0) {
            pool.intern_ty(&TyData::Mut(key))
        } else {
            key
        };
        Ok((self.b.emit(Tag::ProviderGet, at, NONE, pt, n.index()), pt))
    }

    /// A requirement the body uses must be in its row
    /// (`req.row.set.call`): `missing-requirement` otherwise.
    pub(crate) fn check_row_has(&mut self, key: hd_base::DefId, n: NodeRef<'_>) {
        let pool = self.cx.names.pool;
        let covered = self.rows.iter().any(|r| {
            let d = pool.row_data(*r);
            !d.params.is_empty()
                || d.keys.iter().any(|k| match pool.get(*k) {
                    TyData::TraitValue { def, .. } => def == key || self.trait_extends(def, key, 0),
                    _ => false,
                })
        });
        if !covered {
            let msg = format!(
                "missing-requirement: this needs `$ {}`, which the enclosing function's row does not name",
                hd_resolve::show_ty(
                    &self.cx.names,
                    pool.intern_ty(&TyData::TraitValue {
                        def: key,
                        args: TyList::EMPTY,
                        bindings: vec![]
                    })
                )
            );
            self.err(Code::MissingRequirement, n, &msg);
        }
    }

    /// Whether trait `a` has `b` among its supertraits.
    pub(crate) fn trait_extends(&self, a: hd_base::DefId, b: hd_base::DefId, depth: u32) -> bool {
        if depth > 16 {
            return false;
        }
        let Some(ItemData::Trait(t)) = self.cx.lookup.item(a).map(|i| &i.data) else {
            return false;
        };
        t.supers.iter().any(|s| match self.pool().get(*s) {
            TyData::TraitValue { def, .. } => def == b || self.trait_extends(def, b, depth + 1),
            _ => false,
        })
    }
}
