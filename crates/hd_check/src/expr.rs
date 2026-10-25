//! Expressions (`crates/hd_check/DESIGN.md` (type checking) §2.2): literals, strings and
//! interpolation, names, operators, fields, indexing, collection and data
//! literals, `if`, loops, closures, `?`, ranges and providers.

use hd_base::HashMap;
use std::fmt::Write as _;

use hd_base::DefId;
use hd_base::StageResult;
use hd_diag::{Code, FixSafety, Severity};
use hd_intern::PathKind;
use hd_resolve::{ItemData, Src, show_ty_rows_in};
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_tir::ir::{Coercion, IntrinsicOp, NONE, PrimOp, Ref, Tag, TirSink, local_flags};
use hd_types::solver::{Answer, Evidence, TraitRef};
use hd_types::{ParamRef, Prim, Ty, TyData, VarKind};

use crate::body::{Ck, ExpectPosition, OpenSub, unsupported};

/// What the base of a `**` is (`expr.power.*`).
#[derive(Clone, Copy)]
enum PowerBase {
    Int,
    Float,
    /// Not a number: an error.
    Other,
    /// Already an error, or a divergent value.
    Poison,
}

/// What a built-in index reads ([`Ck::index_kind`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Indexed {
    /// A list's element: a place of the list's buffer, `[%xs [%i]]`.
    List,
    /// A map's value: a place of its entry, `[%m [%e]]` at the number
    /// `MapEntry` finds.
    Map,
    /// A string's byte: `StrIndex`.
    Str,
}

/// Decodes a string piece's text: escapes, and `$name` references.
pub(crate) enum Piece {
    Text(String),
    Name(String),
}

pub(crate) fn decode_piece(raw: &str, out: &mut Vec<Piece>) {
    let raw = &normalize_line_endings(raw);
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

/// The text with each CRLF line ending as one line feed
/// (`lex.multiline.line-feed`). A bare carriage return is a lexical error
/// and never reaches here.
pub(crate) fn normalize_line_endings(raw: &str) -> std::borrow::Cow<'_, str> {
    if raw.contains("\r\n") {
        std::borrow::Cow::Owned(raw.replace("\r\n", "\n"))
    } else {
        std::borrow::Cow::Borrowed(raw)
    }
}

/// The text of a string token without its delimiters and interpolation
/// openers and closers.
pub(crate) fn strip_piece(t: &str, kind: TokenKind) -> &str {
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

/// Whether an explicit field of a copy-update may run code when it is
/// evaluated: anything but shorthand, a plain name or a scalar literal.
fn may_run_code(field: NodeRef<'_>) -> bool {
    field.kind() != SyntaxKind::SpreadExpr
        && field
            .children()
            .next()
            .is_some_and(|e| !matches!(e.kind(), SyntaxKind::NameExpr | SyntaxKind::LiteralExpr))
}

/// A bare step: identifiers joined by `.`, or a path (`expr.pipe.bare.form`).
fn is_bare_path(n: NodeRef<'_>) -> bool {
    match n.kind() {
        SyntaxKind::NameExpr | SyntaxKind::PathExpr => true,
        SyntaxKind::FieldExpr => n.children().next().is_some_and(is_bare_path),
        _ => false,
    }
}

/// The `_` nodes a pipe step holds, split into its own and those inside a
/// closure nested in it. A nested pipe's step belongs to that pipe, so
/// only its left side is searched (`expr.pipe.slot.nested`).
fn step_slots<'t>(
    n: NodeRef<'t>,
    in_closure: bool,
    own: &mut Vec<NodeRef<'t>>,
    closed: &mut Vec<NodeRef<'t>>,
) {
    match n.kind() {
        SyntaxKind::PlaceholderExpr => {
            if in_closure {
                closed.push(n);
            } else {
                own.push(n);
            }
        }
        SyntaxKind::PipeExpr => {
            if let Some(lhs) = n.children().next() {
                step_slots(lhs, in_closure, own, closed);
            }
        }
        SyntaxKind::ClosureExpr => {
            for c in n.children() {
                step_slots(c, true, own, closed);
            }
        }
        _ => {
            for c in n.children() {
                step_slots(c, in_closure, own, closed);
            }
        }
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
        // An expectation poisoned by an error already reported absorbs the
        // expression: its unsolved types are not a second error. A block
        // is left out, so its unrelated statements still report.
        if let Some(w) = want
            && self.infer.resolve(self.pool(), w) == Ty::POISON
            && !matches!(n.kind(), SyntaxKind::Block)
        {
            self.absorbed.push(span);
        }
        let (r, t) = self.expr_node(n, want).map_err(|e| e.at(span))?;
        // A projection whose base inference has since fixed (a generic
        // call's `S::Item`) is used by its normal form.
        Ok((r, self.norm_ty(t)))
    }

    fn expr_node(&mut self, n: NodeRef<'_>, want: Option<Ty>) -> StageResult<(Ref, Ty)> {
        self.charge()?;
        if let Some((at, v)) = self.pipe_arg
            && at == n.index()
        {
            return Ok(v);
        }
        let kids: Vec<NodeRef<'_>> = n.children().collect();
        Ok(match n.kind() {
            SyntaxKind::LiteralExpr => self.literal(n, want)?,
            SyntaxKind::StringExpr => {
                let first = self.cx.src.text(self.cx.src.first(n));
                match first.find('"') {
                    Some(0) | None => (self.string_expr(n)?, Ty::STRING),
                    Some(at) => {
                        let prefix = first[..at].to_owned();
                        self.prefixed_string(n, &prefix, want)?
                    }
                }
            }
            SyntaxKind::UnaryExpr => {
                let v = self.unary(n, &kids, want)?;
                // `!c` swaps the paths of `c`.
                let negation = self.cx.src.tkind(self.cx.src.first(n)) == Some(TokenKind::Bang);
                if negation
                    && let Some(f) = self.cond_flow.as_mut()
                    && kids.first().is_some_and(|k| f.node == k.index())
                {
                    f.node = n.index();
                    std::mem::swap(&mut f.on_true, &mut f.on_false);
                }
                v
            }
            SyntaxKind::RefExpr => self.ref_expr(n, want)?,
            SyntaxKind::DerefExpr => self.deref_expr(n)?,
            SyntaxKind::ParenExpr => match kids.as_slice() {
                [e] => {
                    let v = self.expr(*e, want)?;
                    if let Some(f) = self.cond_flow.as_mut()
                        && f.node == e.index()
                    {
                        f.node = n.index();
                    }
                    v
                }
                _ => return unsupported("a parenthesized expression shape"),
            },
            SyntaxKind::TupleExpr => self.tuple_expr(n, &kids, want)?,
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
            SyntaxKind::PowerExpr => self.power(n, &kids)?,
            SyntaxKind::FieldExpr => self.field_expr(n, &kids, want)?,
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
            SyntaxKind::TrailingCallExpr => self.trailing_call(n, want)?,
            SyntaxKind::PipeExpr => self.pipe(&kids, want)?,
            SyntaxKind::PlaceholderExpr => self.placeholder_value(n),
            SyntaxKind::ComprehensionExpr => self.comprehension(n, &kids, want)?,
            SyntaxKind::TryExpr => self.try_expr(n, &kids, want)?,
            SyntaxKind::RangeExpr => self.range_expr(n, &kids, want)?,
            SyntaxKind::ContextExpr => self.context_expr(n, want)?,
            SyntaxKind::BindingExpr => {
                let [pat, rhs] = kids.as_slice() else {
                    return unsupported("this binding form");
                };
                // `grammar.stmt.chain`: a chain `a := b := e` is `a := (b := e)`;
                // `let_pattern` checks the inner binding through this same arm.
                // `names.bind.no-redeclare`, `names.scope.duplicate`.
                let name = (pat.kind() == SyntaxKind::BindingPattern).then(|| self.sym_of(*pat));
                if let Some(name) = name
                    && self.scopes.last().is_some_and(|s| s.contains_key(&name))
                {
                    let msg = format!(
                        "`{}` is already bound in this scope",
                        self.cx.names.text(name)
                    );
                    self.err(Code::DuplicateBinding, *pat, &msg);
                }
                self.let_pattern(*pat, None, *rhs, None, n)?;
                if let Some((l, depth)) = name.and_then(|name| self.find_local(name)) {
                    self.bound.push((l, depth));
                }
                // `expr.bind.one-name`: the binding is not reassignable,
                // and the expression's value is the initializer's.
                match name.and_then(|name| self.find_local(name)) {
                    Some((l, _)) => {
                        self.add_flags(l, local_flags::SHORT);
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
            SyntaxKind::TypeArgsExpr => self.explicit_item_value(n, want)?,
            SyntaxKind::PathExpr => self.path_value(n, want, &[])?,
            other => return unsupported(format!("expression {other:?}")),
        })
    }

    fn literal(&mut self, n: NodeRef<'_>, want: Option<Ty>) -> StageResult<(Ref, Ty)> {
        let first = self.cx.src.first(n);
        let mut strip = false;
        let mut primitive = None;
        if self.cx.src.tkind(first) == Some(TokenKind::Number) {
            let text = self.cx.src.text(first).to_owned();
            let suffix = crate::literals::split_suffix(&text).1;
            if !suffix.is_empty() {
                if let Some(p) = crate::literals::primitive_suffix(suffix)
                    && (p.is_integer() || p.is_float())
                {
                    primitive = Some(Ty::prim(p));
                    strip = true;
                } else if self.lit_arg == Some(n.index()) {
                    self.lit_arg = None;
                    strip = true;
                } else {
                    return self.literal_fn_call(
                        n,
                        suffix,
                        hd_resolve::iface::LITERAL_SUFFIX,
                        want,
                    );
                }
            }
        }
        let (r, mut t) = self.literal_tok(first, strip)?;
        if let Some(fixed) = primitive {
            self.expect(t, fixed, n, "literal", ExpectPosition::General);
            t = fixed;
        }
        if self.b.const_of(r).is_some() {
            self.lit_nodes.push((r, n.index(), false));
        }
        Ok((r, t))
    }

    /// The literal token at `t`.
    pub(crate) fn literal_at(&mut self, t: hd_base::TokenIdx) -> StageResult<(Ref, Ty)> {
        self.literal_tok(t, false)
    }

    /// The literal token at `t`; `strip` drops a number's literal suffix.
    /// An integer literal is an integer class even where a float is
    /// expected (`types.literal.int-not-float`).
    fn literal_tok(&mut self, t: hd_base::TokenIdx, strip: bool) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
        Ok(match self.cx.src.tkind(t) {
            Some(TokenKind::KwTrue) => (self.b.const_value(Ty::BOOL, 1), Ty::BOOL),
            Some(TokenKind::KwFalse) => (self.b.const_value(Ty::BOOL, 0), Ty::BOOL),
            Some(TokenKind::KwPass) => (Ref(NONE), Ty::VOID),
            Some(TokenKind::Char) => {
                let raw = self.cx.src.text(t);
                let inner = raw.strip_prefix('\'').unwrap_or(raw);
                let inner = inner.strip_suffix('\'').unwrap_or(inner);
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
                let text = self.cx.src.text(t);
                let text = if strip {
                    crate::literals::split_suffix(text).0
                } else {
                    text
                }
                .replace('_', "");
                let int = if let Some((radix, digits)) = crate::literals::radix_digits(&text) {
                    u64::from_str_radix(digits, radix).ok()
                } else {
                    text.parse::<u64>().ok()
                };
                if let Some(v) = int {
                    let lt = self.infer.fresh(pool, VarKind::IntLit);
                    (self.b.const_value(lt, v), lt)
                } else if crate::literals::decimal_digits(&text) {
                    // An integer too large for u64 stays an integer,
                    // clamped to the maximum; the overflow bit travels
                    // with the constant so `check_literal_ranges`
                    // reports `integer-literal-range`
                    // (`types.literal.int-range`).
                    let lt = self.infer.fresh(pool, VarKind::IntLit);
                    let r = self.b.const_value(lt, u64::MAX);
                    self.overflow_lits.push(r);
                    (r, lt)
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
        for t in n.direct_tokens().collect::<Vec<_>>() {
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
                            if !self.is_poison_name(&name) {
                                let msg = format!("`{name}` is not defined");
                                self.err(Code::UnknownName, n, &msg);
                            }
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
        let pool = self.pool();
        let tt = self.infer.shallow(pool, t);
        let tt = match pool.get(tt) {
            TyData::Mut(i) => self.infer.shallow(pool, i),
            _ => tt,
        };
        if tt == Ty::STRING {
            return Ok(r);
        }
        let display = self.cx.names.known.display;
        let r2 = self.trait_call(display, "to_string", r, t, &[], n)?;
        Ok(r2.0)
    }

    /// The bits of the constant `bits` of type `ct`, negated: a float
    /// literal flips its sign, an integer is two's complement.
    pub(crate) fn negated_bits(&self, ct: Ty, bits: u64) -> u64 {
        let pool = self.pool();
        let is_float = matches!(self.infer.kind_of(pool, ct), Some(VarKind::FloatLit))
            || matches!(pool.get(self.infer.shallow(pool, ct)), TyData::Prim(p) if p.is_float());
        if is_float {
            (-f64::from_bits(bits)).to_bits()
        } else {
            bits.cast_signed().wrapping_neg().cast_unsigned()
        }
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
        let pool = self.pool();
        // A negated literal is signed (`expr.power.negated-literal`,
        // `types.literal.local.signed`): parentheses are transparent, so
        // `-5` and `-(5)` mark the literal alike, and so is `**`, whose
        // value has its base's type, so `-2 ** 2` marks the `2`. Only an
        // unbound literal class is marked (`Infer::mark_signed`), so a
        // concrete unsigned operand still mismatches below.
        let mut core = *e;
        while core.kind() == SyntaxKind::ParenExpr
            && let Some(inner) = core.children().next()
        {
            core = inner;
        }
        let mut powered = false;
        if core.kind() == SyntaxKind::PowerExpr
            && let Some(base) = core.children().next()
        {
            let mut b = base;
            while b.kind() == SyntaxKind::ParenExpr
                && let Some(inner) = b.children().next()
            {
                b = inner;
            }
            core = b;
            powered = true;
        }
        if matches!(op, Some(TokenKind::Plus | TokenKind::Minus))
            && (self.b.const_of(r).is_some() || powered)
            && self.cx.src.tkind(self.cx.src.first(core)) == Some(TokenKind::Number)
        {
            self.infer.mark_signed(pool, t);
        }
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
            let msg = format!("{what} does not apply to {}", me.show(t));
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
                let tr = if op == Some(TokenKind::Minus) {
                    self.cx.names.known.neg
                } else {
                    self.cx.names.known.not
                };
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
                    let overflowed = self.overflow_lits.contains(&r);
                    self.lit_nodes.retain(|x| x.0 != r);
                    self.overflow_lits.retain(|x| *x != r);
                    let v = self.negated_bits(ct, bits);
                    let nr = self.b.const_value(ct, v);
                    self.lit_nodes.push((nr, n.index(), true));
                    if overflowed {
                        self.overflow_lits.push(nr);
                    }
                    (nr, t)
                } else {
                    self.neg_nodes.push((t, n.index()));
                    (self.b.prim(PrimOp::Neg as u32, &[r], t, n.index()), t)
                }
            }
            Some(TokenKind::Bang | TokenKind::Tilde) => {
                if op == Some(TokenKind::Bang) {
                    self.expect(t, Ty::BOOL, *e, "operand", ExpectPosition::Operand);
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
                let known = self.cx.names.known;
                self.env.clause_self[i] == t
                    && [known.num, known.integer, known.float].contains(&self.env.clause_trait[i])
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
            self.expect(at, Ty::BOOL, *l, "operand", ExpectPosition::Operand);
            let m = self.b.open_block();
            let mut rhs = None;
            self.short_circuit(n, (*l, *r), op == TokenKind::AndAnd, |ck| {
                let (c, ct) = ck.expr(*r, Some(Ty::BOOL))?;
                ck.expect(ct, Ty::BOOL, *r, "operand", ExpectPosition::Operand);
                rhs = Some(c);
                Ok(())
            })?;
            let c = rhs.unwrap_or(Ref(NONE));
            let blk = self.b.close_block(m, Some(c), Ty::BOOL, r.index());
            let tag = if op == TokenKind::AndAnd {
                Tag::And
            } else {
                Tag::Or
            };
            return Ok((self.b.emit(tag, a.0, blk.0, Ty::BOOL, n.index()), Ty::BOOL));
        }
        // `expr.eq.contextual-operand`: a contextual variant on the left of
        // `==` or `!=` takes the right operand's type; `expr.eq.contextual-both`:
        // with a contextual variant on both sides neither has one.
        let contextual = |e: NodeRef<'_>| {
            e.kind() == SyntaxKind::VariantExpr
                || (e.kind() == SyntaxKind::CallExpr
                    && e.children()
                        .next()
                        .is_some_and(|c| c.kind() == SyntaxKind::VariantExpr))
        };
        let eq_op = matches!(op, TokenKind::EqEq | TokenKind::NotEq);
        let left_want = if eq_op && contextual(*l) && !contextual(*r) {
            Some(self.infer.fresh(self.pool(), VarKind::General))
        } else {
            None
        };
        let (a, at) = self.expr(*l, left_want)?;
        let right_want = if left_want.is_none() && eq_op && contextual(*l) {
            Ty::POISON
        } else {
            at
        };
        let (c, ct) = self.expr(*r, Some(right_want))?;
        if matches!(
            op,
            TokenKind::Lt | TokenKind::LtEq | TokenKind::Gt | TokenKind::GtEq
        ) {
            self.warn_unsigned_zero(op, *l, at, *r, ct, n);
        }
        // `ref.identity.no-equality`: `==` between two references is
        // ambiguous, so it is an error with a fix-it for each meaning.
        if eq_op && self.ref_of(at).is_some() && self.ref_of(ct).is_some() {
            self.reference_equality(op == TokenKind::NotEq, op_tok, (*l, *r), n);
            let v = self.b.emit(Tag::Poison, NONE, NONE, Ty::BOOL, n.index());
            return Ok((v, Ty::BOOL));
        }
        let v = self.binary_values(op, (a, at), (c, ct), n, *r)?;
        let t = self.b.ty_of(v);
        Ok((v, t))
    }

    /// Reports `l == r` or `l != r` on two references
    /// (`ref.identity.no-equality`), with the fix-its `l is r`, which
    /// compares places, and `*l == *r`, which compares values.
    fn reference_equality(
        &mut self,
        negated: bool,
        op_tok: hd_base::TokenIdx,
        (l, r): (NodeRef<'_>, NodeRef<'_>),
        n: NodeRef<'_>,
    ) {
        let (op, cmp) = if negated {
            ("!=", "`!(a is b)`")
        } else {
            ("==", "`a is b`")
        };
        let msg = format!(
            "`{op}` between two references is ambiguous: write {cmp} to compare the places they refer to, or `*a {op} *b` to compare the values"
        );
        self.err(Code::ReferenceEquality, n, &msg);
        let file = self.cx.src.file;
        let at = |lo: u32, hi: u32| hd_base::Span { file, lo, hi };
        let (ls, rs) = (self.cx.src.span(l), self.cx.src.span(r));
        let os = self.cx.src.token_span(op_tok);
        if negated {
            self.diags.add_fix(
                "compare the places with `is`",
                FixSafety::Suggestion,
                &[
                    (at(ls.lo, ls.lo), "!("),
                    (at(os.lo, os.hi), "is"),
                    (at(rs.hi, rs.hi), ")"),
                ],
            );
        } else {
            self.diags.add_fix(
                "compare the places with `is`",
                FixSafety::Suggestion,
                &[(at(os.lo, os.hi), "is")],
            );
        }
        let wrap = |e: NodeRef<'_>| {
            !matches!(
                e.kind(),
                SyntaxKind::NameExpr
                    | SyntaxKind::FieldExpr
                    | SyntaxKind::CallExpr
                    | SyntaxKind::ParenExpr
                    | SyntaxKind::IndexExpr
                    | SyntaxKind::RefExpr
                    | SyntaxKind::DerefExpr
            )
        };
        let mut edits = Vec::new();
        for (e, sp) in [(l, ls), (r, rs)] {
            if wrap(e) {
                edits.push((at(sp.lo, sp.lo), "*("));
                edits.push((at(sp.hi, sp.hi), ")"));
            } else {
                edits.push((at(sp.lo, sp.lo), "*"));
            }
        }
        self.diags
            .add_fix("compare the values with `*`", FixSafety::Suggestion, &edits);
    }

    /// Whether a node is an unsuffixed integer literal with the value
    /// zero, in any radix (`expr.ord.unsigned-zero.literal`).
    fn zero_literal(&self, n: NodeRef<'_>) -> bool {
        if n.kind() != SyntaxKind::LiteralExpr {
            return false;
        }
        let first = self.cx.src.first(n);
        if self.cx.src.tkind(first) != Some(TokenKind::Number) {
            return false;
        }
        let text = self.cx.src.text(first);
        let (digits, suffix) = crate::literals::split_suffix(text);
        if !suffix.is_empty() {
            return false;
        }
        let digits = digits.replace('_', "");
        if let Some((radix, rest)) = crate::literals::radix_digits(&digits) {
            return u64::from_str_radix(rest, radix).ok() == Some(0);
        }
        crate::literals::decimal_digits(&digits) && digits.parse::<u64>().ok() == Some(0)
    }

    /// Whether a type is an unsigned integer type at the comparison
    /// (`expr.ord.unsigned-zero.binding-type`).
    fn unsigned_now(&self, t: Ty) -> Option<Prim> {
        let pool = self.pool();
        match pool.get(self.strip_mut(t)) {
            TyData::Prim(p) if p.is_unsigned() => Some(p),
            _ => None,
        }
    }

    /// `expr.ord.unsigned-zero`: `t >= 0`, `0 <= t`, `t < 0` and `0 > t`
    /// with an unsigned `t` warn `unsigned-comparison-always`; the
    /// program still compiles.
    fn warn_unsigned_zero(
        &mut self,
        op: TokenKind,
        l: NodeRef<'_>,
        at: Ty,
        r: NodeRef<'_>,
        ct: Ty,
        n: NodeRef<'_>,
    ) {
        // Whether `t` is the left operand, and whether the form is
        // always true.
        let (t_left, t_node, t_ty, always) = match op {
            TokenKind::GtEq if self.zero_literal(r) => (true, l, at, true),
            TokenKind::LtEq if self.zero_literal(l) => (false, r, ct, true),
            TokenKind::Lt if self.zero_literal(r) => (true, l, at, false),
            TokenKind::Gt if self.zero_literal(l) => (false, r, ct, false),
            _ => return,
        };
        let Some(p) = self.unsigned_now(t_ty) else {
            return;
        };
        let span = self.cx.src.span(t_node);
        let name = self
            .cx
            .src
            .text
            .get(span.lo as usize..span.hi as usize)
            .unwrap_or("the value");
        let op_text = match op {
            TokenKind::GtEq => ">=",
            TokenKind::LtEq => "<=",
            TokenKind::Lt => "<",
            _ => ">",
        };
        let ordering = if always {
            "always true"
        } else {
            "always false"
        };
        let form = if t_left {
            format!("{name} {op_text} 0")
        } else {
            format!("0 {op_text} {name}")
        };
        let mut msg = format!("`{name}` is unsigned, so `{form}` is {ordering}");
        if p == Prim::Usize {
            msg.push_str("; consider a signed literal such as `+10` or another condition");
        }
        self.diags.push(
            Code::UnsignedComparisonAlways,
            Severity::Warning,
            self.cx.src.span(n),
            &msg,
            None,
        );
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
        let pool = self.pool();
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
            // `ref.identity.same-place`: a reference compares its place.
            if self.ref_of(at).is_some() || self.ref_of(ct).is_some() {
                return self.ref_is((a, at), (c, ct), n, rn);
            }
            // `expr.is.value-operand`: both operands have identity. The
            // first that has none is the one error, so the second operand
            // adds no knock-on report.
            for (t, at) in [(at, n), (ct, rn)] {
                if !self.has_identity(t)? {
                    let msg = format!("{} has no identity to compare with `is`", self.show(t));
                    self.err(Code::IdentityRequiresReferences, at, &msg);
                    return Ok(self.b.emit(Tag::Is, a.0, c.0, Ty::BOOL, n.index()));
                }
            }
            // `expr.is.compatible.referent`, `expr.is.compatible.trait-value`,
            // `expr.is.incompatible.data`.
            if !self.identity_compatible(at, ct)? {
                let msg = format!(
                    "`is` compares {} with {}, which are not compatible reference types",
                    self.show(at),
                    self.show(ct)
                );
                self.err(Code::IncompatibleIdentityOperands, rn, &msg);
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
            self.expect(at, ct, n, "operand", ExpectPosition::Operand);
        }
        // `expr.op.primitive.types`: a primitive left operand with a
        // non-primitive right operand is the operator trait's call, so
        // `impl Mul[Money] for i64` fits `i64(3) * price`. A literal left
        // operand takes the class's width first
        // (`expr.op.left-literal.join`), which only its default can be
        // here, as an implementation alone never names a width.
        let trait_left = !cmp && self.primitive_left(at) && self.nominal(ct);
        if trait_left {
            let at = self.zonk(at);
            return self.operator_trait_call(prim, (a, at), (c, ct), n, rn);
        }
        if self.numeric(at) || (ats == Ty::BOOL && bool_ops) {
            let mut mixed = false;
            if shift {
                let _ = self.unsigned_count(ct, rn, "a shift count", true);
            } else {
                mixed = self.expect(ct, at, rn, "operand", ExpectPosition::Operand);
            }
            // `expr.bit.non-integer-no-impl`: bitwise operators and shifts
            // take integers.
            if (bitwise || shift) && self.float_like(at) {
                let msg = format!("bitwise operators take integers, not {}", self.show(at));
                self.err(Code::TypeMismatch, n, &msg);
            }
            // A mismatched pair is reported at its operand; the result is not
            // checked again, so it adds no knock-on report.
            let result = if mixed {
                Ty::POISON
            } else if cmp {
                Ty::BOOL
            } else {
                at
            };
            return Ok(self.b.prim(prim as u32, &[a, c], result, n.index()));
        }
        // Strings: `+` and `==` are built in; others go through traits.
        if ats == Ty::STRING && matches!(prim, PrimOp::Add | PrimOp::Eq | PrimOp::Ne) {
            self.expect(ct, Ty::STRING, rn, "operand", ExpectPosition::Operand);
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
        self.operator_trait_call(prim, (a, at), (c, ct), n, rn)
    }

    /// Whether an operand is a primitive number or character, or a
    /// number literal whose width is still open.
    fn primitive_left(&self, t: Ty) -> bool {
        let pool = self.pool();
        let t = self.strip_mut(t);
        match pool.get(t) {
            TyData::Prim(p) => p.is_integer() || p.is_float() || p == Prim::Char,
            TyData::Infer(_) => self
                .infer
                .kind_of(pool, t)
                .and_then(VarKind::literal_default)
                .is_some(),
            _ => false,
        }
    }

    /// Whether a type is a declared or structural type rather than a
    /// primitive, an open variable, or a type parameter.
    fn nominal(&self, t: Ty) -> bool {
        let pool = self.pool();
        let t = self.strip_mut(t);
        matches!(
            pool.get(t),
            TyData::Adt { .. }
                | TyData::Tuple { .. }
                | TyData::Option(_)
                | TyData::Fn { .. }
                | TyData::TraitValue { .. }
        )
    }

    /// The instantiation choice for an unsuffixed literal right operand or
    /// index key (`expr.op.left-dispatch`, `expr.index.trait.choice`,
    /// `trait.resolve.literal-arg`). The first of `args` is the operand
    /// or key; a store adds its value. When several implementations of the
    /// trait for the receiver apply, the literal joins the choice and
    /// takes its default type. None when the operand is no literal or
    /// fewer than two implementations exist. The trials are effect-free;
    /// the already-evaluated receiver is shared, so evaluation still runs
    /// left, right, call.
    pub(crate) fn literal_arg_choice(
        &mut self,
        tr: DefId,
        method: &str,
        (a, at): (Ref, Ty),
        (args, ct): (&[NodeRef<'_>], Ty),
        n: NodeRef<'_>,
    ) -> StageResult<Option<(Ref, Ty)>> {
        let pool = self.pool();
        let literal = args
            .first()
            .is_some_and(|k| k.kind() == SyntaxKind::LiteralExpr)
            && matches!(
                self.infer.kind_of(pool, self.strip_mut(ct)),
                Some(VarKind::IntLit | VarKind::SignedIntLit | VarKind::FloatLit)
            );
        if !literal {
            return Ok(None);
        }
        let t = self.strip_mut(at);
        // A parameter or trait value has bounds, not implementations; its
        // clauses answer the call as they do without a literal.
        if matches!(
            pool.get(self.infer.resolve(pool, t)),
            TyData::Param(_) | TyData::TraitValue { .. }
        ) {
            return Ok(None);
        }
        let goal = hd_types::solver::Goal::Instantiations {
            self_ty: t,
            trait_: tr,
            mut_: false,
        };
        let Answer::Candidates(cands) = self.solve_goal(&goal)? else {
            return Ok(None);
        };
        if cands.len() < 2 {
            return Ok(None);
        }
        let method = self.cx.names.member(tr, PathKind::Member, method);
        let mut call = crate::call::Args::empty();
        call.positional.extend_from_slice(args);
        self.choose_method((tr, method, t), &cands, Some(a), &call, (n, false), None)
            .map(Some)
    }

    /// The instantiation choice for a tuple key with unsuffixed literal
    /// elements (`expr.index.trait.choice`, `trait.resolve.literal-arg`),
    /// made element by element, so the key's type is known before the
    /// chosen implementation's `Out` is projected
    /// (`expr.index.trait.read-other`). A single implementation whose key
    /// fits gives the literals their types by context. Signed literals,
    /// written as `i32` values, narrow the candidates next. The other
    /// literals take their default type only to break a remaining tie.
    /// `tr` is `Index` or `IndexSet`, whose first argument is the key.
    pub(crate) fn tuple_key_choice(&mut self, tr: DefId, at: Ty, kt: Ty) -> StageResult<()> {
        let pool = self.pool();
        let key = self.infer.resolve(pool, self.strip_mut(kt));
        if !matches!(pool.get(key), TyData::Tuple { .. }) {
            return Ok(());
        }
        let mut lits = Vec::new();
        self.open_tuple_literals(key, &mut lits);
        if lits.is_empty() {
            return Ok(());
        }
        let t = self.strip_mut(at);
        // A parameter or trait value has bounds, not implementations.
        if matches!(
            pool.get(self.infer.resolve(pool, t)),
            TyData::Param(_) | TyData::TraitValue { .. }
        ) {
            return Ok(());
        }
        let goal = hd_types::solver::Goal::Instantiations {
            self_ty: t,
            trait_: tr,
            mut_: false,
        };
        let Answer::Candidates(cands) = self.solve_goal(&goal)? else {
            return Ok(());
        };
        let all: Vec<usize> = (0..cands.len()).collect();
        let mut fits = self.fitting_keys(&cands, &all, key);
        for signed in [true, false] {
            if fits.len() < 2 {
                break;
            }
            for &l in &lits {
                let Some(k) = self.infer.kind_of(pool, l) else {
                    continue;
                };
                if (k == VarKind::SignedIntLit) != signed {
                    continue;
                }
                if let Some(d) = k.literal_default() {
                    self.note_fallback(l, k);
                    let _ = self.infer.unify(pool, l, d);
                }
            }
            fits = self.fitting_keys(&cands, &fits, key);
        }
        if let [one] = fits[..]
            && let Some(k) = self.candidate_key(&cands[one])
        {
            let _ = self.infer.unify(pool, k, key);
        }
        Ok(())
    }

    /// The open literal classes among a tuple's elements, nested tuples
    /// included.
    fn open_tuple_literals(&self, t: Ty, out: &mut Vec<Ty>) {
        let pool = self.pool();
        let t = self.infer.shallow(pool, t);
        match pool.get(t) {
            TyData::Infer(_) => {
                let open = self
                    .infer
                    .kind_of(pool, t)
                    .and_then(VarKind::literal_default)
                    .is_some();
                if open && !out.contains(&t) {
                    out.push(t);
                }
            }
            TyData::Tuple { elems, .. } => {
                for &e in pool.list_items(elems) {
                    self.open_tuple_literals(e, out);
                }
            }
            _ => {}
        }
    }

    /// The candidates among `which` whose key unifies with `key`, each
    /// tried without a trace left behind.
    fn fitting_keys(
        &mut self,
        cands: &[hd_types::solver::Candidate],
        which: &[usize],
        key: Ty,
    ) -> Vec<usize> {
        let pool = self.pool();
        let mut fits = Vec::new();
        for &i in which {
            let mark = self.infer.trial_mark();
            let fit = self
                .candidate_key(&cands[i])
                .is_some_and(|k| self.infer.unify(pool, k, key).is_ok());
            self.infer.rollback_trial(mark);
            if fit {
                fits.push(i);
            }
        }
        fits
    }

    /// A candidate's key, the trait's first argument, with a fresh
    /// variable for each implementation parameter the receiver left open.
    fn candidate_key(&mut self, c: &hd_types::solver::Candidate) -> Option<Ty> {
        let pool = self.pool();
        let def = self.cx.impl_table(c.row.module)?.def[c.row.row as usize];
        let first = *pool.list_items(c.args).first()?;
        let vars: Vec<Ty> = (0..pool.list_items(c.impl_args).len())
            .map(|_| self.infer.fresh(pool, VarKind::General))
            .collect();
        Some(crate::call::subst_owner(pool, def, &vars, first))
    }

    /// `a op b` as the operator trait's method (`expr.op.desugar`), once
    /// the built-in rules do not type it.
    fn operator_trait_call(
        &mut self,
        prim: PrimOp,
        (a, at): (Ref, Ty),
        (c, ct): (Ref, Ty),
        n: NodeRef<'_>,
        rn: NodeRef<'_>,
    ) -> StageResult<Ref> {
        let k = self.cx.names.known;
        let (tr, trait_name, method) = match prim {
            PrimOp::Add => (k.add, "Add", "add"),
            PrimOp::Sub => (k.sub, "Sub", "sub"),
            PrimOp::Mul => (k.mul, "Mul", "mul"),
            PrimOp::Div => (k.div, "Div", "div"),
            PrimOp::Rem => (k.rem, "Rem", "rem"),
            PrimOp::BitAnd => (k.bit_and, "BitAnd", "bit_and"),
            PrimOp::BitOr => (k.bit_or, "BitOr", "bit_or"),
            PrimOp::BitXor => (k.bit_xor, "BitXor", "bit_xor"),
            PrimOp::Shl => (k.shl, "Shl", "shl"),
            PrimOp::Shr => (k.shr, "Shr", "shr"),
            PrimOp::Eq | PrimOp::Ne => (k.eq, "Eq", "eq"),
            _ => (k.ord, "Ord", "cmp"),
        };
        if matches!(prim, PrimOp::Lt | PrimOp::Le | PrimOp::Gt | PrimOp::Ge) {
            return self.ordering((a, at), (c, ct), prim, n, rn);
        }
        if let Some((v, _)) = self.literal_arg_choice(tr, method, (a, at), (&[rn], ct), n)? {
            if prim == PrimOp::Ne {
                return Ok(self.b.prim(PrimOp::Not as u32, &[v], Ty::BOOL, n.index()));
            }
            return Ok(v);
        }
        if let Some(info) = self.op_failure(tr, at, Some(ct))? {
            if let Some(msg) = self.unmet_derived_bound(&info) {
                self.err(Code::MissingDerivedBound, n, &msg);
                return Ok(self.b.emit(Tag::Poison, NONE, NONE, Ty::POISON, n.index()));
            }
            let mut msg = format!(
                "{} does not implement `{trait_name}` for this operand",
                self.show(at)
            );
            let pool = self.pool();
            if tr == k.add
                && matches!(pool.get(self.strip_mut(self.infer.resolve(pool, at))), TyData::Adt { def, .. } if def == k.list)
                && matches!(pool.get(self.strip_mut(self.infer.resolve(pool, ct))), TyData::Adt { def, .. } if def == k.list)
                && let Some(left) = n.children().next()
            {
                let left = self.cx.src.span(left);
                let right = self.cx.src.span(rn);
                let text = self.cx.src.text;
                let _ = write!(
                    msg,
                    "; concatenate with `[{}..., {}...]`",
                    &text[left.lo as usize..left.hi as usize],
                    &text[right.lo as usize..right.hi as usize]
                );
            }
            self.err(Code::TypeMismatch, n, &msg);
            return Ok(self.b.emit(Tag::Poison, NONE, NONE, Ty::POISON, n.index()));
        }
        // A function-typed left operand selects implementations by its
        // own type: row subsumption does not apply
        // (`expr.op.left-dispatch.exact-function`), so a winner for a
        // wider row does not fit. The message suggests the
        // implementation's function type
        // (`expr.op.left-dispatch.exact-function.message`).
        let pool = self.pool();
        let operand = self.infer.resolve(pool, self.strip_mut(at));
        if matches!(pool.get(operand), TyData::Fn { .. })
            && let Some(winner) = self.op_impl_winner(tr, at, Some(ct))?
            && self.infer.resolve(pool, winner) != operand
        {
            // The suggestion names the implementation's function type
            // with its row: the rows are the whole difference.
            let winner = show_ty_rows_in(&self.cx.names, pool, self.infer.resolve(pool, winner));
            let msg = format!(
                "{} does not implement `{trait_name}` for this operand; a function operand selects by its own type, so bind it with the implementation's type, as in `let handler: {winner} = ...`",
                self.show(at),
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

    /// Whether a type is an integer type, an integer literal's variable,
    /// or a parameter bounded by `Integer`. A still-open variable passes.
    fn integer_like(&self, t: Ty) -> bool {
        let pool = self.pool();
        let t = self.infer.shallow(pool, self.strip_mut(t));
        match pool.get(t) {
            TyData::Prim(p) => p.is_integer(),
            TyData::Infer(_) => !matches!(self.infer.kind_of(pool, t), Some(VarKind::FloatLit)),
            TyData::Param(_) => (0..self.env.clause_self.len()).any(|i| {
                self.env.clause_self[i] == t
                    && self.env.clause_trait[i] == self.cx.names.known.integer
            }),
            TyData::Never | TyData::Poison => true,
            _ => false,
        }
    }

    /// Whether a type may be a range bound (`expr.range.bound.numeric`):
    /// integer-like, floating-point, or a parameter bounded by `Float`.
    fn range_bound_like(&self, t: Ty) -> bool {
        let pool = self.pool();
        let t = self.infer.shallow(pool, self.strip_mut(t));
        self.integer_like(t)
            || self.float_like(t)
            || matches!(pool.get(t), TyData::Param(_))
                && (0..self.env.clause_self.len()).any(|i| {
                    self.env.clause_self[i] == t
                        && self.env.clause_trait[i] == self.cx.names.known.float
                })
    }

    /// Whether a type is floating-point, or a floating literal's variable.
    fn float_like(&self, t: Ty) -> bool {
        let pool = self.pool();
        let t = self.strip_mut(t);
        match pool.get(t) {
            TyData::Prim(p) => p.is_float(),
            TyData::Infer(_) => self.infer.kind_of(pool, t) == Some(VarKind::FloatLit),
            _ => false,
        }
    }

    /// A shift count (`expr.shift.count-unsigned`) or an integer exponent
    /// (`expr.power.int.exponent`): any unsigned integer type; an
    /// unsuffixed literal takes `u32`. A negated literal is signed. A
    /// shift count takes `u32` anyway and is checked at run time, but an
    /// exponent rejects it here (`expr.power.negated-literal`).
    fn unsigned_count(&mut self, ct: Ty, rn: NodeRef<'_>, what: &str, negated_ok: bool) -> bool {
        let pool = self.pool();
        let c = self.strip_mut(ct);
        let ok = match pool.get(c) {
            TyData::Infer(_) => match self.infer.kind_of(pool, c) {
                Some(VarKind::IntLit) => self.infer.unify(pool, c, Ty::prim(Prim::U32)).is_ok(),
                Some(VarKind::SignedIntLit) if negated_ok => {
                    self.infer.unify(pool, c, Ty::prim(Prim::U32)).is_ok()
                }
                _ => false,
            },
            TyData::Prim(p) => p.is_unsigned(),
            TyData::Never | TyData::Poison => true,
            _ => false,
        };
        if !ok {
            let msg = format!("{what} must be unsigned, found {}", self.show(ct));
            self.err(Code::TypeMismatch, rn, &msg);
        }
        ok
    }

    /// What a `**` base is (`expr.power.*`).
    fn power_base(&self, t: Ty) -> PowerBase {
        let pool = self.pool();
        let t = self.strip_mut(t);
        match pool.get(t) {
            TyData::Prim(p) if p.is_integer() => PowerBase::Int,
            TyData::Prim(p) if p.is_float() => PowerBase::Float,
            TyData::Infer(_) => match self.infer.kind_of(pool, t) {
                Some(VarKind::IntLit | VarKind::SignedIntLit) => PowerBase::Int,
                Some(VarKind::FloatLit) => PowerBase::Float,
                _ => PowerBase::Other,
            },
            TyData::Param(_) => {
                let known = self.cx.names.known;
                let bound = |tr: DefId| {
                    (0..self.env.clause_self.len())
                        .any(|i| self.env.clause_self[i] == t && self.env.clause_trait[i] == tr)
                };
                if bound(known.integer) {
                    PowerBase::Int
                } else if bound(known.float) {
                    PowerBase::Float
                } else {
                    PowerBase::Other
                }
            }
            TyData::Never | TyData::Poison => PowerBase::Poison,
            _ => PowerBase::Other,
        }
    }

    /// `a ** b` (`expr.power.*`). `**` has no operator trait
    /// (`expr.op.not-overloaded`). An integer base takes an unsigned
    /// exponent, a float base an exponent of its own type, and the result
    /// has the base's type.
    fn power(&mut self, n: NodeRef<'_>, kids: &[NodeRef<'_>]) -> StageResult<(Ref, Ty)> {
        let [l, r] = kids else {
            return unsupported("a power expression shape");
        };
        let (a, at) = self.expr(*l, None)?;
        let base = self.power_base(at);
        let want = match base {
            PowerBase::Int => Some(Ty::prim(Prim::U32)),
            PowerBase::Float => Some(at),
            PowerBase::Poison | PowerBase::Other => None,
        };
        let (c, ct) = self.expr(*r, want)?;
        // A rejected operand makes the whole expression an error value, so
        // its context reports nothing more.
        let rejected = match base {
            PowerBase::Int => !self.unsigned_count(ct, *r, "an exponent", false),
            PowerBase::Float => {
                self.expect(ct, at, *r, "operand", ExpectPosition::Operand);
                false
            }
            PowerBase::Poison => false,
            PowerBase::Other => {
                let msg = format!("`**` takes integers or floats, found {}", self.show(at));
                self.err(Code::TypeMismatch, n, &msg);
                true
            }
        };
        if rejected {
            let p = self.b.emit(Tag::Poison, NONE, NONE, Ty::POISON, n.index());
            return Ok((p, Ty::POISON));
        }
        let v = self.b.prim(PrimOp::Pow as u32, &[a, c], at, n.index());
        Ok((v, at))
    }

    /// Whether some implementation of operator trait `tr` fits a left
    /// operand of type `at` and a right operand of type `ct`
    /// (`expr.op.no-impl`).
    pub(crate) fn op_fits(&mut self, tr: DefId, at: Ty, ct: Option<Ty>) -> StageResult<bool> {
        Ok(self.op_failure(tr, at, ct)?.is_none())
    }

    /// Why no implementation of operator trait `tr` fits, or `None` when
    /// one does or the answer waits.
    pub(crate) fn op_failure(
        &mut self,
        tr: DefId,
        at: Ty,
        ct: Option<Ty>,
    ) -> StageResult<Option<Box<hd_types::solver::FailInfo>>> {
        let pool = self.pool();
        let t = self.strip_mut(at);
        if matches!(
            pool.get(t),
            TyData::Infer(_) | TyData::Poison | TyData::Never
        ) {
            return Ok(None);
        }
        let tref = self.op_tref(tr, at, ct);
        Ok(match self.solve(tref)? {
            Answer::Fails(info) => Some(info),
            _ => None,
        })
    }

    /// An operator goal's trait reference: fresh variables for the
    /// trait's arguments, with a known right operand fixing the first.
    /// `op_failure` and `op_impl_winner` solve the same goal, so the
    /// same implementation wins.
    fn op_tref(&mut self, tr: DefId, at: Ty, ct: Option<Ty>) -> TraitRef {
        let pool = self.pool();
        let t = self.strip_mut(at);
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
        TraitRef {
            trait_: tr,
            self_ty: t,
            args: pool.list(&args),
        }
    }

    /// The winning implementation's instantiated self type for an
    /// operator goal, when the winner is a written implementation.
    /// `None` when the goal fails, stalls, or holds by another
    /// evidence, so callers keep their own failure path.
    fn op_impl_winner(&mut self, tr: DefId, at: Ty, ct: Option<Ty>) -> StageResult<Option<Ty>> {
        let pool = self.pool();
        let tref = self.op_tref(tr, at, ct);
        let Answer::Holds {
            evidence:
                Evidence::Impl {
                    row,
                    args: impl_args,
                },
            ..
        } = self.solve(tref)?
        else {
            return Ok(None);
        };
        let def = self
            .cx
            .impl_table(row.module)
            .map_or(DefId::NONE, |t| t.def[row.row as usize]);
        if def == DefId::NONE {
            return Ok(None);
        }
        let Some(item) = self.cx.lookup.item(def) else {
            return Ok(None);
        };
        let ItemData::Impl { self_ty, .. } = &item.data else {
            return Ok(None);
        };
        let ia = pool.list_items(impl_args);
        Ok(Some(crate::call::subst_owner(pool, def, ia, *self_ty)))
    }

    /// The type argument and bound that a derived implementation's
    /// failure lacks (`trait.derive.bound-unmet`), when the outermost
    /// implementation tried was derived.
    pub(crate) fn unmet_derived_bound(&self, info: &hd_types::solver::FailInfo) -> Option<String> {
        let first = info.chain.first()?;
        if !matches!(first.origin, hd_types::solver::ImplOrigin::Derived { .. }) {
            return None;
        }
        Some(format!(
            "{} does not implement {}, which the derived implementation requires of its type argument",
            self.show(info.leaf.self_ty),
            self.cx.names.display_name(info.leaf.trait_)
        ))
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
        let pool = self.pool();
        let tr = self.cx.names.known.partial_ord;
        if let Some(info) = self.op_failure(tr, at, Some(ct))? {
            if let Some(msg) = self.unmet_derived_bound(&info) {
                self.err(Code::MissingDerivedBound, n, &msg);
                return Ok(self.b.emit(Tag::Poison, NONE, NONE, Ty::POISON, n.index()));
            }
            let msg = format!("{} does not implement `PartialOrd`", self.show(at));
            self.err(Code::TypeMismatch, n, &msg);
            return Ok(self.b.emit(Tag::Poison, NONE, NONE, Ty::POISON, n.index()));
        }
        let (v, vt) = self.trait_call_args(tr, "partial_cmp", a, at, &[(c, ct, rn)], n)?;
        let ord = self.cx.names.known.ordering;
        let ord_ty = pool.intern_ty(&TyData::Adt {
            def: ord,
            args: hd_types::TyList::EMPTY,
        });
        let sym = self.cx.names.syms.intern("order");
        let l = self.b.local(vt, sym, local_flags::ASSIGNED, n.index());
        self.set_local(l, v, n.index());
        let eq = self.cx.names.known.eq;
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

    /// Whether a type's values have identity: the solver's `AnyRef` row
    /// holds or waits (`crates/hd_check/DESIGN.md` (trait solver) §3.9). A diverging operand has no
    /// value to compare and passes.
    pub(crate) fn has_identity(&mut self, t: Ty) -> StageResult<bool> {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        if self.strip_mut(t) == Ty::NEVER {
            return Ok(true);
        }
        // An open literal class is a number, which has no identity.
        if matches!(pool.get(t), TyData::Infer(_))
            && self
                .infer
                .kind_of(pool, t)
                .and_then(VarKind::literal_default)
                .is_some()
        {
            return Ok(false);
        }
        let tref = TraitRef {
            trait_: self.cx.names.known.any_ref,
            self_ty: t,
            args: hd_types::TyList::EMPTY,
        };
        Ok(!matches!(self.solve(tref)?, Answer::Fails(_)))
    }

    /// Whether two `is` operands, both with identity, have compatible
    /// types (`expr.is.compatible.referent`,
    /// `expr.is.compatible.trait-value`): equal once `mut` is removed at every
    /// level, which unification does, or one is a trait value or `Any` type
    /// that the other converts to. A match fixes the types' open variables.
    fn identity_compatible(&mut self, l: Ty, r: Ty) -> StageResult<bool> {
        let pool = self.pool();
        let (l, r) = (self.strip_mut(l), self.strip_mut(r));
        if l == Ty::NEVER || r == Ty::NEVER {
            return Ok(true);
        }
        let snap = self.infer.snapshot();
        if self.infer.unify(pool, l, r).is_ok() {
            return Ok(true);
        }
        self.infer.rollback(snap);
        Ok(self.erases_into(l, r)? || self.erases_into(r, l)?)
    }

    /// Whether `t` coerces into the trait value `target` because it
    /// implements the trait (`types.assign.trait-value.ref`), or the answer waits for
    /// inference; `false` when `target` is no trait value.
    fn erases_into(&mut self, t: Ty, target: Ty) -> StageResult<bool> {
        let pool = self.pool();
        let TyData::TraitValue { def, args, .. } = pool.get(self.infer.resolve(pool, target))
        else {
            return Ok(false);
        };
        let tref = TraitRef {
            trait_: def,
            self_ty: self.infer.resolve(pool, t),
            args,
        };
        Ok(!matches!(self.solve(tref)?, Answer::Fails(_)))
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

    /// Whether `name` selects a tuple element (rest list included) or a
    /// shared enum parameter of `t`: a member that reads but is no place.
    pub(crate) fn is_read_only_member(&self, t: Ty, name: &str) -> bool {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        let t = match pool.get(t) {
            TyData::Mut(i) => i,
            _ => t,
        };
        if let TyData::Tuple { elems, rest } = pool.get(t) {
            let n = pool.list_items(elems).len();
            return name
                .strip_prefix('_')
                .and_then(|d| d.parse::<usize>().ok())
                .is_some_and(|i| i < n || (i == n && rest.is_some()));
        }
        self.shared_param(t, name).is_some()
    }

    fn field_expr(
        &mut self,
        n: NodeRef<'_>,
        kids: &[NodeRef<'_>],
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let Some(base) = kids.first() else {
            return unsupported("a field without a base");
        };
        let name = self.cx.src.text(self.cx.src.last(n)).to_owned();
        // `Type.Variant`, `module.item`, `module.Type.Variant`.
        if (base.kind() == SyntaxKind::NameExpr && self.find_local(self.sym_of(*base)).is_none()
            || base.kind() == SyntaxKind::FieldExpr)
            && let Some(v) = self.static_member_value(*base, &name, want, n)?
        {
            return Ok(v);
        }
        self.elem_may_stand(*base, crate::elem_refs::ElemUse::Through);
        let based = self.expr(*base, None)?;
        self.field_of_value(based, &name, (*base, n))
    }

    /// The field `name` of the value `r` of type `bt`, which `base`
    /// evaluated, as the field expression `n` reads it.
    pub(crate) fn field_of_value(
        &mut self,
        (r, bt): (Ref, Ty),
        name: &str,
        (base, n): (NodeRef<'_>, NodeRef<'_>),
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
        let name = name.to_owned();
        let mut bt = self.infer.resolve(pool, bt);
        if matches!(pool.get(bt), TyData::Infer(_)) && self.learn_waiting(bt)? {
            bt = self.infer.resolve(pool, bt);
        }
        // Shallow readonly references preserve the declared field types.
        let (r, bt) = match self.ref_of(bt) {
            Some(_) => self.emit_deref(r, bt, base.index()),
            None => (r, bt),
        };
        let inner = match pool.get(bt) {
            TyData::Mut(i) => i,
            _ => bt,
        };
        // A base whose type an earlier error left unknown adds nothing.
        if matches!(inner, Ty::NEVER | Ty::POISON) {
            return Ok((Ref(NONE), Ty::NEVER));
        }
        if let TyData::Tuple { elems, rest } = pool.get(inner)
            && let Ok(i) = name.trim_start_matches('_').parse::<usize>()
        {
            // `types.tuple.rest.value`: the rest list follows the fixed
            // elements and is selected like any other element.
            let items = pool.list_items(elems);
            let Some(t) = items
                .get(i)
                .copied()
                .or_else(|| rest.filter(|_| i == items.len()))
            else {
                // A selection past the end is a missing field read
                // (`expr.tuple.select-underscore`).
                let msg = format!("no field `{name}` on {}", self.show(bt));
                self.err(Code::UnknownField, n, &msg);
                return Ok((Ref(NONE), Ty::NEVER));
            };
            let idx = u32::try_from(i).unwrap_or(0);
            return Ok((self.b.field(r, idx, t, n.index()), t));
        }
        let (r, bt) = self.promote_base(r, bt, &name, n);
        if let Some((idx, ft)) = self.field_of(bt, &name) {
            self.check_field_visible(bt, &name, n);
            let ft = self.field_access(bt, &name, ft);
            return Ok((self.field_read(r, idx, ft, n.index()), ft));
        }
        if let Some(v) = self.shared_field(r, inner, &name, n) {
            return Ok(v);
        }
        if matches!(pool.get(inner), TyData::Infer(_)) {
            return self.gap(n, "a field of a value whose type is not yet known");
        }
        // In a trait's default body, `self` is the trait itself, whose
        // members resolve as methods: naming one there that neither the
        // trait nor a supertrait declares is `unknown-method`
        // (`trait.default.no-self-members`).
        let trait_self = match pool.get(inner) {
            TyData::Param(p) => {
                p.index == 0
                    && self
                        .cx
                        .lookup
                        .item(p.owner)
                        .is_some_and(|i| matches!(i.data, ItemData::Trait(_)))
            }
            _ => false,
        };
        if trait_self {
            let msg = format!("no method `{name}` on {}", self.show(bt));
            self.err(Code::UnknownMethod, n, &msg);
        } else {
            let msg = format!("no field `{name}` on {}", self.show(bt));
            self.err(Code::UnknownField, n, &msg);
        }
        Ok((Ref(NONE), Ty::NEVER))
    }

    /// The built-in index of a receiver type, its key type and its value
    /// type. A list element is read and written as a place; a map value
    /// is replaced with `MapSet`, and read, referred to or written through
    /// as a place at its entry (`Ck::elem_index`).
    pub(crate) fn index_kind(&mut self, t: Ty) -> Option<(Indexed, Ty, Ty)> {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        let t = match pool.get(t) {
            TyData::Mut(i) => i,
            _ => t,
        };
        if t == Ty::STRING {
            return Some((Indexed::Str, Ty::prim(Prim::Usize), Ty::prim(Prim::U8)));
        }
        let TyData::Adt { def, args } = pool.get(t) else {
            return None;
        };
        let argv = pool.list_items(args);
        let known = self.cx.names.known;
        match def {
            d if d == known.list => Some((Indexed::List, Ty::prim(Prim::Usize), *argv.first()?)),
            d if d == known.map => Some((Indexed::Map, *argv.first()?, *argv.get(1)?)),
            _ => None,
        }
    }

    /// The read of built-in index `get` of `base` at the key `key`, of
    /// value type `vt`: a list's element place, `Read [%base [%key]]`
    /// (`docs/compiler/tir-v2.md`, "Element reads and writes"), a map's
    /// value place at the key's entry, or the `StrIndex` intrinsic.
    pub(crate) fn index_read(
        &mut self,
        get: Indexed,
        (base, key): (Ref, Ref),
        vt: Ty,
        at: hd_base::NodeIdx,
    ) -> Ref {
        let op = match get {
            Indexed::List | Indexed::Map => {
                let index = self.elem_index(base, key, self.b.ty_of(base), at);
                return self.b.element(base, index, vt, at);
            }
            Indexed::Str => IntrinsicOp::StrIndex,
        };
        let rec = self.b.refs_record(&[base, key]);
        self.b.emit(Tag::Intrinsic, op as u32, rec, vt, at)
    }

    fn index_expr(&mut self, n: NodeRef<'_>, kids: &[NodeRef<'_>]) -> StageResult<(Ref, Ty)> {
        let [base, key] = kids else {
            return unsupported("an index shape");
        };
        let (br, bt) = self.expr(*base, None)?;
        // A base that already failed, such as a type name written where a
        // value is needed, leaves nothing to index.
        if br.0 == NONE && matches!(bt, Ty::NEVER | Ty::POISON) {
            return Ok((br, bt));
        }
        let (br, bt) = self.deref_index_base(br, bt, base.index());
        let builtin = self.index_kind(bt);
        let (kr, kt) = self.index_key_value(*key, builtin)?;
        self.index_of_value((br, bt), builtin, (kr, kt), (*key, n))
    }

    /// The index expression `n` on the evaluated base `br` of type `bt`,
    /// whose built-in index operations are `builtin`, at the evaluated key
    /// `kr` of type `kt` that `key` wrote.
    pub(crate) fn index_of_value(
        &mut self,
        (br, bt): (Ref, Ty),
        builtin: Option<(Indexed, Ty, Ty)>,
        (kr, kt): (Ref, Ty),
        (key, n): (NodeRef<'_>, NodeRef<'_>),
    ) -> StageResult<(Ref, Ty)> {
        let key = &key;
        // `expr.index.slice.call`: a list or string indexed by a range
        // type is the call of `Index`; every other built-in index reads
        // by its key type.
        if let Some((get, bkt, vt)) = builtin
            && !(get != Indexed::Map && self.is_range_ty(kt))
        {
            let kr = self.index_key((kr, kt), *key, bkt);
            return Ok((self.index_read(get, (br, kr), vt, n.index()), vt));
        }
        let index = self.cx.names.known.index;
        self.tuple_key_choice(index, bt, kt)?;
        // `expr.index.trait.no-read`.
        if !self.op_fits(index, bt, Some(kt))? {
            let msg = format!(
                "{} has no `Index` implementation for this key",
                self.show(bt)
            );
            self.err(Code::TypeMismatch, n, &msg);
            return Ok((
                self.b.emit(Tag::Poison, NONE, NONE, Ty::POISON, n.index()),
                Ty::POISON,
            ));
        }
        if let Some(chosen) = self.literal_arg_choice(index, "index", (br, bt), (&[*key], kt), n)? {
            return Ok(chosen);
        }
        self.trait_call_args(index, "index", br, bt, &[(kr, kt, *key)], n)
    }

    /// An index key, checked with the expected type its built-in receiver
    /// gives it: `usize` for a list or string, or `Range[usize]` for a
    /// range expression, so a range's literal bounds are `usize`
    /// (`expr.index.expected`, `expr.index.expected.range`).
    pub(crate) fn index_key_value(
        &mut self,
        key: NodeRef<'_>,
        builtin: Option<(Indexed, Ty, Ty)>,
    ) -> StageResult<(Ref, Ty)> {
        let want = builtin.map(|(get, kt, _)| {
            if key.kind() == SyntaxKind::RangeExpr && get != Indexed::Map {
                let known = self.cx.names.known;
                self.pool().intern_ty(&TyData::Adt {
                    def: known.range,
                    args: self.pool().list(&[kt]),
                })
            } else {
                kt
            }
        });
        self.expr(key, want)
    }

    /// Whether `t` is one of std's range types.
    pub(crate) fn is_range_ty(&self, t: Ty) -> bool {
        let pool = self.pool();
        let known = self.cx.names.known;
        match pool.get(self.strip_mut(t)) {
            TyData::Adt { def, .. } => [
                known.range,
                known.range_from,
                known.range_to,
                known.range_full,
            ]
            .contains(&def),
            _ => false,
        }
    }

    /// A built-in index key of type `kt`. A `usize` key
    /// (`expr.index.list.unsigned`) admits every unsigned type: a narrower
    /// one widens, and a `u64` beyond the `usize` range saturates, so it
    /// fails the bounds check (`expr.index.list.range`).
    pub(crate) fn index_key(&mut self, (kr, ktt): (Ref, Ty), key: NodeRef<'_>, kt: Ty) -> Ref {
        let pool = self.pool();
        let usize_t = Ty::prim(Prim::Usize);
        let got = self.strip_mut(ktt);
        let p = match pool.get(got) {
            TyData::Prim(p) if kt == usize_t && p.is_unsigned() && p != Prim::Usize => p,
            _ => return self.coerce(kr, ktt, kt, key, "index", ExpectPosition::General),
        };
        let at = key.index();
        if p != Prim::U64 {
            return self.b.prim(PrimOp::Conv as u32, &[kr], usize_t, at);
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
        self.b.emit(Tag::If, over.0, rec, usize_t, at)
    }

    /// A tuple expression (`expr.tuple.comma`). Against an expected tuple
    /// type with a rest element, its elements fill the fixed elements and
    /// the rest are collected into the rest list, or a final spread
    /// supplies it (`expr.tuple.rest.*`).
    fn tuple_expr(
        &mut self,
        n: NodeRef<'_>,
        kids: &[NodeRef<'_>],
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
        let want = self.literal_want(want);
        let (wants, want_rest) = match want.map(|w| pool.get(self.infer.shallow(pool, w))) {
            Some(TyData::Tuple { elems, rest }) => (
                pool.list_items(elems).iter().copied().map(Some).collect(),
                rest,
            ),
            _ => (Vec::<Option<Ty>>::new(), None),
        };
        // The element type each collected rest value is checked against.
        let rest_item = want_rest.and_then(|r| self.list_item(r));
        let spread = kids
            .last()
            .copied()
            .filter(|e| e.kind() == SyntaxKind::SpreadExpr);
        let plain = &kids[..kids.len() - usize::from(spread.is_some())];
        if kids.is_empty() && want_rest.is_none() {
            return Ok((Ref(NONE), Ty::VOID));
        }
        let mut refs = Vec::new();
        let mut tys = Vec::new();
        for (i, e) in plain.iter().enumerate() {
            if e.kind() == SyntaxKind::SpreadExpr {
                return unsupported("a tuple spread before the last element");
            }
            let w = wants.get(i).copied().flatten().or(rest_item);
            let (r, t) = self.expr(*e, w)?;
            let r = match w {
                Some(w) => self.coerce(r, t, w, *e, "tuple element", ExpectPosition::General),
                None => r,
            };
            refs.push(r);
            tys.push(w.unwrap_or(t));
        }
        let tail = match spread {
            Some(sp) => {
                let Some(operand) = sp.children().next() else {
                    return unsupported("a spread without an operand");
                };
                let (r, t) = self.expr(operand, want_rest)?;
                Some((r, t, operand))
            }
            None => None,
        };
        let tt = match (want_rest, &tail) {
            (Some(_), _) => want.map_or(Ty::POISON, |w| self.infer.shallow(pool, w)),
            (None, Some((_, t, operand))) => {
                // `expr.tuple.rest.spread.list`: the spread operand is a
                // list, which gives the rest element.
                let t = self.strip_mut(*t);
                let list = self.cx.names.known.list;
                if !matches!(pool.get(t), TyData::Adt { def, .. } if def == list) {
                    if !matches!(t, Ty::NEVER | Ty::POISON) {
                        let msg = format!("in spread: expected a list, found {}", self.show(t));
                        self.err(Code::TypeMismatch, *operand, &msg);
                    }
                    return Ok((Ref(NONE), Ty::NEVER));
                }
                pool.intern_ty(&TyData::Tuple {
                    elems: pool.list(&tys),
                    rest: Some(t),
                })
            }
            (None, None) => {
                let t = pool.intern_ty(&TyData::Tuple {
                    elems: pool.list(&tys),
                    rest: None,
                });
                let rec = self.b.refs_record(&refs);
                return Ok((self.b.emit(Tag::NewTuple, NONE, rec, t, n.index()), t));
            }
        };
        let tail = match (&tail, want_rest) {
            (Some((r, t, operand)), Some(rest)) => Some(self.coerce(
                *r,
                *t,
                rest,
                *operand,
                "tuple spread",
                ExpectPosition::General,
            )),
            (Some((r, _, _)), None) => Some(*r),
            (None, _) => None,
        };
        let r = self.rest_tuple_value(tt, &refs, tail, n);
        Ok((r, tt))
    }

    /// The item type of a list type; none for any other type.
    pub(crate) fn list_item(&self, list_ty: Ty) -> Option<Ty> {
        let pool = self.pool();
        let list = self.cx.names.known.list;
        match pool.get(self.expected_shape(list_ty)) {
            TyData::Adt { def, args } if def == list => pool.list_items(args).first().copied(),
            _ => None,
        }
    }

    /// The value type of the map type `map_ty`, `mut` or not.
    pub(crate) fn map_value_item(&self, map_ty: Ty) -> Option<Ty> {
        let pool = self.pool();
        let map = self.cx.names.known.map;
        match pool.get(self.expected_shape(map_ty)) {
            TyData::Adt { def, args } if def == map => pool.list_items(args).get(1).copied(),
            _ => None,
        }
    }

    /// The value of the rest tuple type `tt`: `items` fill the fixed
    /// elements and the rest are collected into a new list, or `tail` is
    /// the rest list (`expr.tuple.rest.collect`, `expr.tuple.rest.spread`,
    /// `expr.tuple.rest.value`). The items are already checked against
    /// their element types.
    pub(crate) fn rest_tuple_value(
        &mut self,
        tt: Ty,
        items: &[Ref],
        tail: Option<Ref>,
        at: NodeRef<'_>,
    ) -> Ref {
        let pool = self.pool();
        let TyData::Tuple {
            elems,
            rest: Some(list_ty),
        } = pool.get(tt)
        else {
            return Ref(NONE);
        };
        let fixed = pool.list_items(elems).len();
        if items.len() < fixed || (tail.is_some() && items.len() != fixed) {
            let msg = format!(
                "{} takes {fixed} elements before its rest, found {}",
                self.show(tt),
                items.len()
            );
            self.err(Code::TypeMismatch, at, &msg);
            return Ref(NONE);
        }
        let rest = tail.unwrap_or_else(|| {
            let rec = self.b.refs_record(&items[fixed..]);
            self.b.emit(Tag::NewList, NONE, rec, list_ty, at.index())
        });
        let rec = self.b.refs_record(&items[..fixed]);
        self.b.emit(Tag::NewTuple, rest.0, rec, tt, at.index())
    }

    fn list_expr(
        &mut self,
        n: NodeRef<'_>,
        kids: &[NodeRef<'_>],
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
        let list = self.cx.names.known.list;
        let elem = match self
            .literal_want(want)
            .map(|w| self.infer.shallow(pool, w))
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
        self.join_site(elem, |this, elem| {
            let mut refs = Vec::new();
            let mut spreads = Vec::new();
            for (k, e) in kids.iter().enumerate() {
                if e.kind() == SyntaxKind::ComprehensionFor {
                    return unsupported("a list comprehension");
                }
                if e.kind() == SyntaxKind::SpreadExpr {
                    let Some(operand) = e.children().next() else {
                        return unsupported("a spread without an operand");
                    };
                    let list_ty = pool.intern_ty(&TyData::Adt {
                        def: list,
                        args: pool.list(&[elem]),
                    });
                    let (r, t) = this.expr(operand, Some(list_ty))?;
                    // `expr.list.spread.type`: the operand is a list.
                    let Some(item) = this.list_item(this.infer.resolve(pool, t)) else {
                        if !matches!(this.strip_mut(t), Ty::NEVER | Ty::POISON) {
                            let msg = format!("in spread: expected a list, found {}", this.show(t));
                            this.err(Code::TypeMismatch, operand, &msg);
                        }
                        return Ok((Ref(NONE), Ty::NEVER));
                    };
                    // `expr.list.spread.expected`, `expr.list.spread.inferred`.
                    // Each element converts; a conversion with no run-time
                    // meaning leaves the list as it is.
                    if this.joins.is_site(elem) {
                        this.join_spread(item, elem, operand, "list spread");
                    } else {
                        let cp = this.b.checkpoint();
                        let c = this.coerce(
                            r,
                            item,
                            elem,
                            operand,
                            "list spread",
                            ExpectPosition::General,
                        );
                        if c != r {
                            let body = this.b.body_mut();
                            let kind = c
                                .as_inst()
                                .filter(|i| body.tags[i.0 as usize] == Tag::Coerce)
                                .and_then(|i| {
                                    body.record(body.data[i.0 as usize][1]).first().copied()
                                });
                            let static_kinds =
                                [Coercion::Weaken, Coercion::Variance, Coercion::RowSubsume];
                            if !kind.is_some_and(|k| static_kinds.iter().any(|s| *s as u32 == k)) {
                                return unsupported("a list spread that converts its elements");
                            }
                            this.b.rollback(cp);
                        }
                    }
                    spreads.push(u32::try_from(k).unwrap_or(0));
                    refs.push(r);
                    continue;
                }
                let (r, t) = this.expr(*e, Some(elem))?;
                refs.push(this.coerce(r, t, elem, *e, "list element", ExpectPosition::General));
            }
            // An element type solved from the elements alone drops an outer
            // `mut`. This keeps
            // `typing/invalid/orphan-impl-nested-trait-argument.hd` at one error.
            if inferred && let TyData::Mut(x) = pool.get(this.infer.shallow(pool, elem)) {
                this.infer.rebind(pool, elem, x);
            }
            let t = pool.intern_ty(&TyData::Adt {
                def: list,
                args: pool.list(&[elem]),
            });
            let rec = this.b.refs_record(&refs);
            let marks = if spreads.is_empty() {
                NONE
            } else {
                this.b.words_record(&spreads)
            };
            Ok((this.b.emit(Tag::NewList, marks, rec, t, n.index()), t))
        })
    }

    fn map_expr(
        &mut self,
        n: NodeRef<'_>,
        kids: &[NodeRef<'_>],
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
        let map = self.cx.names.known.map;
        let (kw, vw) = match self
            .literal_want(want)
            .map(|w| self.infer.shallow(pool, w))
            .map(|w| match pool.get(w) {
                TyData::Mut(i) => pool.get(i),
                d => d,
            }) {
            Some(TyData::Adt { def, args }) if def == map => {
                let a = pool.list_items(args);
                (Some(a[0]), Some(a[1]))
            }
            _ => (None, None),
        };
        // `types.lct.sites`: the keys join, and so do the values.
        self.join_site(kw, |this, k| {
            this.join_site(vw, |this, v| {
                let mut refs = Vec::new();
                for e in kids {
                    let ek: Vec<NodeRef<'_>> = e.children().collect();
                    let [ke, ve] = ek.as_slice() else {
                        return unsupported("a map entry shape");
                    };
                    let (kr, kt) = this.expr(*ke, Some(k))?;
                    refs.push(this.coerce(kr, kt, k, *ke, "map key", ExpectPosition::General));
                    let (vr, vt) = this.expr(*ve, Some(v))?;
                    refs.push(this.coerce(vr, vt, v, *ve, "map value", ExpectPosition::General));
                }
                // A key type an expected map type did not fix meets the key
                // bound here (types.map-key.declared-bound).
                if !kids.is_empty() && kw.is_none() {
                    this.check_map_key(k, n)?;
                }
                let t = pool.intern_ty(&TyData::Adt {
                    def: map,
                    args: pool.list(&[k, v]),
                });
                let rec = this.b.refs_record(&refs);
                Ok((this.b.emit(Tag::NewMap, NONE, rec, t, n.index()), t))
            })
        })
    }

    /// `Name { field: value, ... }`, with `Name::[T]` or inferred arguments.
    fn data_expr(
        &mut self,
        n: NodeRef<'_>,
        kids: &[NodeRef<'_>],
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
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
            let hint = match segs.as_slice() {
                [name] => self.import_hint(name),
                _ => String::new(),
            };
            let msg = format!("no type named `{}`{hint}", segs.join("."));
            self.err(Code::UnknownType, *name_node, &msg);
            return Ok((Ref(NONE), Ty::NEVER));
        };
        let Some(item) = self.cx.lookup.item(def) else {
            return unsupported("a data literal of an unknown item");
        };
        let ItemData::Data(fields) = item.data.clone() else {
            let msg = format!("no type named `{}`: not a data type", segs.join("."));
            self.err(Code::UnknownType, *name_node, &msg);
            return Ok((Ref(NONE), Ty::NEVER));
        };
        self.check_literal_visible(def, n);
        let mut targs = Vec::new();
        for t in &targ_nodes {
            targs.push(self.explicit_targ(*t, false)?);
        }
        let explicit = targs.len();
        while targs.len() < item.generics.len() {
            targs.push(self.infer.fresh(pool, VarKind::General));
        }
        // An expected type of the same declaration solves the inferred
        // arguments first, so a field sees its substituted type (`mut U`
        // included).
        if let Some(w) = want
            && let TyData::Adt { def: wd, args: wa } = pool.get(self.expected_shape(w))
            && wd == def
        {
            for (v, a) in targs
                .iter_mut()
                .zip(pool.list_items(wa).iter().copied())
                .skip(explicit)
            {
                if self.strict_opaque_return(a) {
                    // Preserve a rigid return occurrence, even after it has
                    // a concrete choice; a fresh variable would forget its origin.
                    *v = a;
                } else {
                    let _ = self.infer.unify(pool, *v, a);
                }
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
        // `expr.update.form`: a leading spread supplies every field the
        // literal does not name. The fields it names are replaced.
        let spread = kids[1..]
            .iter()
            .find(|a| a.kind() == SyntaxKind::SpreadExpr)
            .copied();
        let replaced: Vec<bool> = fields
            .iter()
            .map(|f| {
                kids[1..]
                    .iter()
                    .any(|a| a.kind() != SyntaxKind::SpreadExpr && self.sym_of(*a) == f.name)
            })
            .collect();
        let mut source = None;
        // Whether a field of the literal already failed to resolve: an
        // unknown field, an unknown shorthand name, or a duplicate. The
        // literal's given/missing accounting cannot be trusted then, so
        // no missing-required-field follows.
        let mut failed = false;
        for a in &kids[1..] {
            if a.kind() == SyntaxKind::SpreadExpr {
                if source.is_some() {
                    continue;
                }
                let target = pool.intern_ty(&TyData::Adt {
                    def,
                    args: pool.list(&targs),
                });
                let Some(inner) = a.children().next() else {
                    return unsupported("a spread without a source");
                };
                // `expr.update.spread-first`, `expr.update.exact-type`.
                let (sr, st) = self.expr(inner, Some(target))?;
                if matches!(self.infer.resolve(pool, st), Ty::NEVER | Ty::POISON) {
                    return Ok((Ref(NONE), Ty::NEVER));
                }
                if !self.copy_source_fits(st, target, inner) {
                    return Ok((Ref(NONE), Ty::NEVER));
                }
                // `expr.update.copy-time`: the copied fields are read now,
                // when an explicit field expression could change the
                // source before construction.
                if kids[1..].iter().any(|f| may_run_code(*f)) {
                    self.copy_fields_now((sr, st), &replaced, &mut vals, *a);
                }
                source = Some((sr, st));
                continue;
            }
            let fname = self.sym_of(*a);
            let Some(i) = fields.iter().position(|f| f.name == fname) else {
                let msg = format!("no field `{}`", self.cx.names.text(fname));
                self.err(Code::UnknownField, *a, &msg);
                failed = true;
                continue;
            };
            // `data.literal.duplicate`: a field is given once.
            if seen[i] {
                let msg = format!("the field `{}` is given twice", self.cx.names.text(fname));
                self.err(Code::DuplicateField, *a, &msg);
                failed = true;
            }
            // A copy-update has its source's concrete type independently
            // of the result expectation; its fields keep ordinary conversions.
            let ft = source
                .and_then(|(_, ty)| match pool.get(self.expected_shape(ty)) {
                    TyData::Adt { args, .. } => Some(crate::call::subst_owner(
                        pool,
                        def,
                        pool.list_items(args),
                        fields[i].ty,
                    )),
                    _ => None,
                })
                .unwrap_or_else(|| inst(fields[i].ty));
            let (r, t) = if let Some(e) = a.children().next() {
                self.expr(e, Some(ft))?
            } else {
                // Shorthand `Name { x }`.
                let Some((l, d)) = self.find_local(fname) else {
                    let text = self.cx.names.text(fname).to_owned();
                    if !self.is_poison_name(&text) {
                        let msg = format!("`{text}` is not defined");
                        self.err(Code::UnknownName, *a, &msg);
                    }
                    failed = true;
                    continue;
                };
                self.read_local(l, d, *a)
            };
            vals[i] = self.coerce(r, t, ft, *a, "field", ExpectPosition::General);
            seen[i] = true;
        }
        let targ_list = pool.list(&targs);
        for (i, f) in fields.iter().enumerate() {
            // `data.update.supplies-all`, `data.update.no-defaults`.
            if !seen[i] && source.is_none() {
                if failed {
                    continue;
                }
                if f.has_default {
                    // The field's default body, at each construction that
                    // omits it (`crates/hd_tir/DESIGN.md` "Default calls").
                    let fname = self.cx.names.text(f.name).to_owned();
                    let body = self.cx.names.member(def, PathKind::Hidden, &fname);
                    let a = self.b.refs_record(&[Ref(body.raw()), Ref(targ_list.0)]);
                    let bw = self.b.refs_record(&[]);
                    let ft = inst(f.ty);
                    vals[i] = self.b.emit(Tag::DefaultCall, a, bw, ft, n.index());
                    continue;
                }
                let msg = format!(
                    "`{}` is required in `{}`",
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
        if let Some((sr, _)) = source {
            let rec = self.copy_record(&vals);
            let at = spread.map_or(n, |s| s);
            return Ok((self.b.emit(Tag::CopyStruct, sr.0, rec, ty, at.index()), ty));
        }
        let rec = self.b.refs_record(&vals);
        Ok((self.b.emit(Tag::NewStruct, NONE, rec, ty, n.index()), ty))
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

    /// `if c: a else if d: b else: e` as a statement or a value. The
    /// chain is one node whose branches are siblings (`flow.if.else-if`).
    /// It lowers to nested `If`s, each later branch in the `else` block of
    /// the one before, built by a loop so a long chain takes no stack per
    /// branch.
    pub(crate) fn if_expr(&mut self, e: NodeRef<'_>, want: Option<Ty>) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
        let Some(cond) = e.children().next() else {
            return unsupported("an `if` without a condition");
        };
        let Some(then_node) = Src::child(e, SyntaxKind::Block) else {
            return unsupported("an `if` without a block");
        };
        // Each branch: its node, condition, and suite.
        let mut branches = vec![(e, cond, then_node)];
        let mut last_else = None;
        for clause in e.children() {
            match clause.kind() {
                SyntaxKind::ElseIfClause => {
                    let (Some(cond), Some(blk)) = (
                        clause.children().next(),
                        Src::child(clause, SyntaxKind::Block),
                    ) else {
                        return unsupported("an `else if` without a condition or a block");
                    };
                    branches.push((clause, cond, blk));
                }
                SyntaxKind::ElseClause => {
                    let Some(blk) = Src::child(clause, SyntaxKind::Block) else {
                        return unsupported("an `else` without a block");
                    };
                    last_else = Some((clause, blk));
                }
                _ => {}
            }
        }
        let (c, ct) = self.expr(cond, Some(Ty::BOOL))?;
        self.expect(ct, Ty::BOOL, cond, "condition", ExpectPosition::General);
        let first_flow = self.take_cond_flow(cond);
        // `flow.if.value.else`: an `if` used where a value is required
        // must have an `else`. A function-body tail is exempt: there the
        // fallthrough is `missing-return-value`.
        if last_else.is_none() && want != Some(Ty::VOID) && self.fn_body_tail != Some(e.index()) {
            self.err(
                Code::TypeMismatch,
                e,
                "an `if` used as a value needs an `else`, for the value when the condition is false",
            );
        }
        // A value `if` needs an `else`; without one the chain is a
        // statement.
        let want = if last_else.is_none() {
            Some(Ty::VOID)
        } else {
            want
        };
        let last = branches.len() - 1;
        self.join_site(want, |this, site| {
            struct Frame {
                node: hd_base::NodeIdx,
                c: Ref,
                then: Ref,
                tty: Ty,
                after_then: hd_base::HashSet<hd_base::LocalId>,
                els: Option<hd_tir::ir::BlockMark>,
            }
            let mut frames: Vec<Frame> = Vec::with_capacity(branches.len());
            let mut flow = Some((c, first_flow));
            for (k, &(node, cond, then_node)) in branches.iter().enumerate() {
                // A later branch's condition is checked in the `else`
                // block of the branch before it.
                let (c, (when_true, when_false)) = if let Some(first) = flow.take() {
                    first
                } else {
                    let (c, ct) = this.expr(cond, Some(Ty::BOOL))?;
                    this.expect(ct, Ty::BOOL, cond, "condition", ExpectPosition::General);
                    (c, this.take_cond_flow(cond))
                };
                // Each branch starts from the condition's `when_true` or
                // `when_false`.
                let tb = this.b.open_block();
                this.set_uninit(when_true);
                let (tt, tty) = this.block_value(then_node, Some(site))?;
                let then = this.b.close_block(tb, tt, tty, then_node.index());
                let after_then = this.replace_uninit(when_false);
                let els = (k < last || last_else.is_some()).then(|| this.b.open_block());
                frames.push(Frame {
                    node: node.index(),
                    c,
                    then,
                    tty,
                    after_then,
                    els,
                });
            }
            let (mut els, mut ety) = match (last_else, frames.last().and_then(|f| f.els)) {
                (Some((clause, blk)), Some(eb)) => {
                    let (tail, ty) = this.block_value(blk, Some(site))?;
                    (this.b.close_block(eb, tail, ty, clause.index()), ty)
                }
                _ => (Ref(NONE), Ty::VOID),
            };
            // From the last branch back to the first: each `If` is the
            // value of the `else` block of the branch before it.
            while let Some(f) = frames.pop() {
                let both_never = f.tty == Ty::NEVER && ety == Ty::NEVER;
                // `names.definite.merge`, `names.definite.diverging`: a
                // branch that diverges is not an incoming path.
                let merged = [(f.tty, &f.after_then), (ety, &this.uninit)]
                    .into_iter()
                    .filter(|(t, _)| *t != Ty::NEVER)
                    .flat_map(|(_, s)| s.iter().copied())
                    .collect();
                this.set_uninit(merged);
                let ty = if both_never { Ty::NEVER } else { site };
                let rec = this.b.refs_record(&[f.then, els]);
                let r = this.b.emit(Tag::If, f.c.0, rec, ty, f.node);
                let Some(outer) = frames.last() else {
                    return Ok((r, ty));
                };
                let Some(eb) = outer.els else {
                    return unsupported("an `else if` without an `else` block");
                };
                let t = this.infer.shallow(pool, ty);
                let tail = if t == Ty::VOID || r.0 == NONE {
                    None
                } else {
                    // A later branch's `If` has a type of its own, which
                    // then joins this site's.
                    let node = this.cx.src.parse.tree.node(f.node);
                    Some(this.coerce(r, t, site, node, "result", ExpectPosition::General))
                };
                els = this.b.close_block(eb, tail, t, f.node);
                ety = t;
            }
            unsupported("an `if` without a branch")
        })
    }

    /// `while c: body` is `Loop { Block { if c { body } else { break } } }`.
    fn while_expr(&mut self, e: NodeRef<'_>, want: Option<Ty>) -> StageResult<(Ref, Ty)> {
        self.loop_site(e, want, |this, els| this.while_loop(e, els))
    }

    /// A `while` loop whose `else` suite, if any, is `els`.
    fn while_loop(
        &mut self,
        e: NodeRef<'_>,
        els: Option<(hd_base::NodeIdx, Ty)>,
    ) -> StageResult<(Ref, Ty)> {
        let Some(cond) = e.children().next() else {
            return unsupported("a `while` without a condition");
        };
        let Some(body) = Src::child(e, SyntaxKind::Block) else {
            return unsupported("a `while` without a block");
        };
        let result = els.map(|(_, t)| t);
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
        let lp = self.b.open_loop();
        let lb = self.b.open_block();
        let (c, ct) = self.expr(cond, Some(Ty::BOOL))?;
        self.expect(ct, Ty::BOOL, cond, "condition", ExpectPosition::General);
        let (when_true, when_false) = self.take_cond_flow(cond);
        self.loops.push((lp, result));
        let tb = self.b.open_block();
        self.set_uninit(when_true);
        self.block_value(body, Some(Ty::VOID))?;
        for l in when_false {
            self.uninit_insert(l);
        }
        let then = self.b.close_block(tb, None, Ty::VOID, body.index());
        self.loops.pop();
        let eb = self.b.open_block();
        // The `else` suite of a `never` loop is still checked, but its
        // value reaches no loop result (`flow.while.infinite.else`).
        self.loop_exit(lp, els, e.index(), !infinite)?;
        let exit = self.b.close_block(eb, None, Ty::VOID, e.index());
        let rec = self.b.refs_record(&[then, exit]);
        self.b.emit(Tag::If, c.0, rec, Ty::VOID, e.index());
        let lbody = self.b.close_block(lb, None, Ty::VOID, e.index());
        let ty = if infinite {
            Ty::NEVER
        } else {
            result.unwrap_or(Ty::VOID)
        };
        Ok((self.b.close_loop(lp, lbody, ty, e.index()), ty))
    }

    /// Checks loop `e` with `run`, which gets the loop's `else` suite and
    /// result type (`flow.loop.else.value`), or none for a loop without
    /// `else`. A loop with `else` is a least-common-type site like a value
    /// `if`: its `break` values and its `else` value join by the same rules
    /// and take the same conversions (`flow.loop.else.type`, `types.lct`).
    fn loop_site(
        &mut self,
        e: NodeRef<'_>,
        want: Option<Ty>,
        run: impl FnOnce(&mut Self, Option<(hd_base::NodeIdx, Ty)>) -> StageResult<(Ref, Ty)>,
    ) -> StageResult<(Ref, Ty)> {
        let Some(clause) = Src::child(e, SyntaxKind::ElseClause) else {
            return run(self, None);
        };
        let Some(blk) = Src::child(clause, SyntaxKind::Block) else {
            return unsupported("a loop `else` without a block");
        };
        self.join_site(want, |this, result| run(this, Some((blk.index(), result))))
    }

    /// `flow.for.pattern.irrefutable`: a `for` pattern must always match.
    pub(crate) fn irrefutable_loop_pattern(&mut self, pat: NodeRef<'_>, t: Ty) {
        if self.refutable(pat, t) {
            self.err(
                Code::RefutableLetPattern,
                pat,
                "a `for` pattern must match every item",
            );
        }
    }

    /// A loop's normal exit: its `else` suite's value, then `break`; the
    /// `break` carries the value only when `valued`.
    pub(crate) fn loop_exit(
        &mut self,
        lp: hd_tir::ir::LoopMark,
        els: Option<(hd_base::NodeIdx, Ty)>,
        at: hd_base::NodeIdx,
        valued: bool,
    ) -> StageResult<()> {
        let v = match els {
            None => NONE,
            Some((blk, t)) => {
                let node = self.cx.src.parse.tree.node(blk);
                let (tail, _) = self.block_value(node, Some(t))?;
                tail.filter(|_| valued).map_or(NONE, |r| r.0)
            }
        };
        self.b.emit(Tag::Break, lp.0.raw(), v, Ty::NEVER, at);
        Ok(())
    }

    pub(crate) fn has_break(n: NodeRef<'_>) -> bool {
        n.children().any(|c| match c.kind() {
            SyntaxKind::BreakStmt => true,
            SyntaxKind::WhileExpr | SyntaxKind::ForExpr | SyntaxKind::ClosureExpr => false,
            _ => Self::has_break(c),
        })
    }

    /// `for p in e: body` over the iterator protocol: `iter` (unless `e`
    /// is already an `Iterator`), then a loop around `next` and a match on
    /// its option (`crates/hd_tir/DESIGN.md` "What The Checker Desugars").
    fn for_expr(&mut self, e: NodeRef<'_>, want: Option<Ty>) -> StageResult<(Ref, Ty)> {
        let kids: Vec<NodeRef<'_>> = e.children().collect();
        let (Some(pat), Some(src), Some(body)) =
            (kids.first(), kids.get(1), Src::child(e, SyntaxKind::Block))
        else {
            return unsupported("a `for` shape");
        };
        self.loop_site(e, want, |this, els| {
            this.loop_else = els;
            this.iterate(*pat, *src, e, &mut |me: &mut Self| {
                me.block_value(body, Some(Ty::VOID)).map(|_| ())
            })
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
        let pool = self.pool();
        let els = self.loop_else.take();
        // `for p in &xs` and `for p in &mut xs` over a list bind element
        // references (`flow.for.ref`).
        let refsrc = crate::elem_refs::strip_parens(src);
        if refsrc.kind() == SyntaxKind::RefExpr
            && let Some(done) = self.ref_loop(pat, refsrc, els, e, body)?
        {
            return Ok(done);
        }
        // The source is evaluated once. A cursor value becomes the loop's
        // own writable copy; an explicit reference keeps sharing its target
        // (`flow.for.iterator.owned-binding`).
        let (sr, st) = self.expr(src, None)?;
        // A named reference to a collection is a value source; the
        // syntactic `&xs` element-reference loop was handled above.
        let (sr, st) = if self
            .ref_of(st)
            .is_some_and(|(inner, _)| self.is_coll_ty(inner))
        {
            self.emit_deref(sr, st, src.index())
        } else {
            (sr, st)
        };
        let st_r = self.infer.resolve(pool, st);
        let st_i = match pool.get(st_r) {
            TyData::Mut(i) => i,
            _ => st_r,
        };
        // A source whose type a waiting call or projection fixes, such as
        // `(1..4).iter()`'s cursor, is learned now, as a receiver's is.
        let st_i = if matches!(pool.get(st_i), TyData::Infer(_))
            && self.infer.kind_of(pool, st_i) == Some(VarKind::General)
            && self.learn_waiting(st_i)?
        {
            self.strip_mut(self.infer.resolve(pool, st_i))
        } else {
            st_i
        };
        // A literal source takes its default type, as a literal does once
        // a member is asked of it; no number is iterable
        // (`flow.for.protocol-required`).
        let st_i = match self.infer.kind_of(pool, st_i) {
            Some(k @ (VarKind::IntLit | VarKind::SignedIntLit | VarKind::FloatLit))
                if matches!(pool.get(st_i), TyData::Infer(_)) =>
            {
                let d = match k {
                    VarKind::SignedIntLit => Ty::I32,
                    VarKind::IntLit => Ty::prim(Prim::Usize),
                    _ => Ty::prim(Prim::F64),
                };
                let _ = self.infer.unify(pool, st_i, d);
                d
            }
            _ => st_i,
        };
        if let TyData::Adt { def, args } = pool.get(st_i)
            && def == self.cx.names.known.range
            && matches!(
                pat.kind(),
                SyntaxKind::BindingPattern | SyntaxKind::WildcardPattern
            )
        {
            let et = pool.list_items(args).first().copied().unwrap_or(Ty::POISON);
            // A float range is not iterable (`flow.for.range.float`), so it
            // goes through the general `Iterable` call below and fails there.
            if self.numeric(et) && self.integer_like(et) {
                return self.range_loop(pat, (sr, et), els, e, body);
            }
        }
        if let TyData::Adt { def, args } = pool.get(st_i)
            && (def == self.cx.names.known.list || def == self.cx.names.known.map)
        {
            let items = pool.list_items(args);
            let map = def == self.cx.names.known.map;
            let item = match items {
                [t] if !map => *t,
                [k, v] if map => pool.intern_ty(&TyData::Tuple {
                    elems: pool.list(&[*k, *v]),
                    rest: None,
                }),
                _ => return unsupported("a collection type's arguments"),
            };
            return self.collection_loop(pat, (sr, st, item, map), els, e, body);
        }
        let (referent, writable) = self.ref_of(st_i).unwrap_or((st_i, false));
        let mut direct = self.iterator_item(referent)?;
        // A cursor over open literals, such as `(1..4).iter()`'s
        // `RangeIter[?]`, whose `Iterator` impl waits on their type: its
        // literals take their defaults, as a literal does once a member is
        // asked of it.
        if direct.is_none() && self.default_open_literals(referent) {
            direct = self.iterator_item(referent)?;
        }
        let (it, it_ty, item_ty) = match pool.get(st_i) {
            _ if direct.is_some() => {
                let cursor = if self.ref_of(st_i).is_some() {
                    if !writable {
                        self.err(
                            Code::NotWritable,
                            src,
                            "a loop cannot advance an iterator through a shared reference; use `&mut`",
                        );
                        return Ok(self.poison_value(e));
                    }
                    sr
                } else {
                    self.new_object_ref(sr, st_i, true, src).0
                };
                (cursor, referent, direct.unwrap_or(Ty::POISON))
            }
            // An infer-typed source, such as an open numeric literal, goes
            // through the general `Iterable` call below: the solver waits
            // while the literal is open, and reports `unsatisfied-trait-bound`
            // when nothing fixes it to an iterable type.
            // Every std range is `Iterable[T] for Range[T]` (and `RangeFrom`).
            TyData::Adt { def, args }
                if def == self.cx.names.known.range || def == self.cx.names.known.range_from =>
            {
                let et = pool.list_items(args).first().copied().unwrap_or(Ty::POISON);
                let iterable = self.cx.names.known.iterable;
                let tref = hd_types::solver::TraitRef {
                    trait_: iterable,
                    self_ty: st_i,
                    args: pool.list(&[et]),
                };
                // The element is often a literal still open here: its
                // impl is chosen when the loop's uses fix it. A float
                // literal's range has no impl whatever type it takes
                // (`flow.for.range.float`), so it is required now.
                let open = pool.has_infer(self.infer.resolve(pool, et));
                if (!open || self.float_like(self.infer.resolve(pool, et)))
                    && self.require_ref(tref, src)?.is_none()
                {
                    return Ok(self.poison_value(e));
                }
                let method = self.cx.names.member(iterable, PathKind::Member, "iter");
                // The one impl over every integer type names the std
                // cursor, so its type is known before the element type is
                // (`flow.for.range.cursor`).
                let cursor = if def == self.cx.names.known.range {
                    "RangeIter"
                } else {
                    "RangeFromIter"
                };
                let it_t = pool.intern_ty(&TyData::Adt {
                    def: self.cx.names.item("std.ops", cursor),
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
                (self.new_object_ref(r, it_t, true, src).0, it_t, et)
            }
            // `flow.for.iterable.cursor`: `iter` gives the type's own
            // cursor, `Self::IntoIter`, which the loop owns as a writable
            // copy and advances with no trait value.
            _ => {
                let iterable = self.cx.names.known.iterable;
                let (r, t) = self.trait_call(iterable, "iter", sr, st, &[], src)?;
                let t = self.normalize_deep(t)?;
                let t = self.strip_mut(self.infer.resolve(pool, t));
                // A source already reported, or one with no `Iterable`
                // impl, whose bound reported it.
                if pool.has_poison(t) || pool.has_poison(self.infer.resolve(pool, st)) {
                    return Ok(self.poison_value(e));
                }
                let Some(item) = self.iterator_item(t)? else {
                    return unsupported("an `iter` result that does not implement `Iterator`");
                };
                (self.new_object_ref(r, t, true, src).0, t, item)
            }
        };
        let it_ref = self.b.ty_of(it);
        let it_sym = self.cx.names.syms.intern("$iter");
        let itl = self
            .b
            .local(it_ref, it_sym, hd_tir::ir::local_flags::ASSIGNED, e.index());
        self.set_local(itl, it, e.index());
        let lp = self.b.open_loop();
        let lb = self.b.open_block();
        let cur = self.b.get(itl, it_ref, e.index());
        if matches!(pool.get(self.strip_mut(it_ty)), TyData::TraitValue { .. }) {
            self.dyn_recv = Some((true, e.index()));
        }
        let (next, nt) = self.method_on(
            cur,
            it_ty,
            "next",
            &crate::call::Args::empty(),
            (e, false),
            None,
        )?;
        let _ = nt;
        let opt = pool.intern_ty(&TyData::Option(item_ty));
        let result = els.map(|(_, t)| t);
        self.loops.push((lp, result));
        self.scopes.push(HashMap::default());
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
        self.loop_exit(lp, els, e.index(), true)?;
        let arm1 = self.b.close_block(bb, None, Ty::VOID, e.index());
        // Decision: `.Some(p)` to arm 0, `.None` to arm 1.
        let rows = vec![crate::pat::Row::some(pat, binds), crate::pat::Row::rest()];
        let db = self.b.open_block();
        self.decide(&rows, next, opt)?;
        let dec = self.b.close_block(db, None, Ty::NEVER, e.index());
        let rec = self.b.refs_record(&[dec, arm0, arm1]);
        self.b.emit(Tag::Match, next.0, rec, Ty::VOID, e.index());
        let lbody = self.b.close_block(lb, None, Ty::VOID, e.index());
        let ty = result.unwrap_or(Ty::VOID);
        Ok((self.b.close_loop(lp, lbody, ty, e.index()), ty))
    }

    /// Gives each open literal variable in `t` its default type; whether
    /// there was one.
    fn default_open_literals(&mut self, t: Ty) -> bool {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        if !pool.has_infer(t) {
            return false;
        }
        let parts: Vec<Ty> = match pool.get(t) {
            TyData::Infer(_) => {
                return match self
                    .infer
                    .kind_of(pool, t)
                    .and_then(VarKind::literal_default)
                {
                    Some(d) => {
                        let k = self.infer.kind_of(pool, t).unwrap_or(VarKind::General);
                        self.note_fallback(t, k);
                        self.infer.unify(pool, t, d).is_ok()
                    }
                    None => false,
                };
            }
            TyData::Adt { args, .. } => pool.list_items(args).to_vec(),
            TyData::Tuple { elems, .. } => pool.list_items(elems).to_vec(),
            TyData::Option(i) | TyData::Mut(i) | TyData::Ref { inner: i, .. } => vec![i],
            _ => vec![],
        };
        let mut any = false;
        for p in parts {
            any |= self.default_open_literals(p);
        }
        any
    }

    /// Probes the cursor trait without reporting when the source is an
    /// iterable instead; a successful answer fixes its item type.
    fn iterator_item(&mut self, ty: Ty) -> StageResult<Option<Ty>> {
        let pool = self.pool();
        let ty = self.strip_mut(self.infer.resolve(pool, ty));
        if matches!(
            pool.get(ty),
            TyData::Infer(_) | TyData::Poison | TyData::Never
        ) {
            return Ok(None);
        }
        if let TyData::TraitValue { def, args, .. } = pool.get(ty)
            && def == self.cx.names.known.iterator
        {
            return Ok(pool.list_items(args).first().copied());
        }
        let snap = self.infer.snapshot();
        let item = self.infer.fresh(pool, VarKind::General);
        let goal = TraitRef {
            trait_: self.cx.names.known.iterator,
            self_ty: ty,
            args: pool.list(&[item]),
        };
        if let Answer::Holds { learned, .. } = self.solve(goal)? {
            for (v, t) in learned {
                let var = pool.intern_ty(&TyData::Infer(v));
                let _ = self.infer.unify(pool, var, t);
            }
            Ok(Some(self.infer.resolve(pool, item)))
        } else {
            self.infer.rollback(snap);
            Ok(None)
        }
    }

    /// A `for` over a numeric `Range` as a counting loop (lowering-catalog
    /// "for over a range"): no iterator value. Each round tests
    /// `cur < end || (inclusive && cur == end)`, binds the item, then
    /// advances before the body runs, so `continue` needs no step of its
    /// own. A literal exclusive range uses only the less-than test and
    /// increments directly. Other ranges reuse that test when advancing.
    /// At the inclusive end it clears `inclusive` instead of adding,
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
        // A literal exclusive range cannot reach its end in the body.
        // Its one top-of-loop comparison proves that incrementing fits.
        let exclusive = sr.as_inst().is_some_and(|i| {
            let b = self.b.body_mut();
            if b.tags[i.0 as usize] != Tag::NewStruct {
                return false;
            }
            b.record(b.data[i.0 as usize][1])
                .get(2)
                .copied()
                .filter(|r| *r != NONE && Ref(*r).as_inst().is_none())
                .is_some_and(|r| b.consts[(r & !Ref::CONST_BIT) as usize].1 == 0)
        });
        let result = loop_else.map(|(_, t)| t);
        let at = e.index();
        let sym = |me: &Self, s: &str| me.cx.names.syms.intern(s);
        let (cur_s, end_s, inc_s) = (sym(self, "$cur"), sym(self, "$end"), sym(self, "$incl"));
        let cur_l = self.b.local(et, cur_s, ASSIGNED, at);
        let end_l = self.b.local(et, end_s, ASSIGNED, at);
        let inc_l = self.b.local(Ty::BOOL, inc_s, ASSIGNED, at);
        let start = self.b.field(sr, 0, et, at);
        self.set_local(cur_l, start, at);
        let end = self.b.field(sr, 1, et, at);
        self.set_local(end_l, end, at);
        let inc = self.b.field(sr, 2, Ty::BOOL, at);
        self.set_local(inc_l, inc, at);
        let lt = PrimOp::Lt as u32;
        let lp = self.b.open_loop();
        let lb = self.b.open_block();
        // The test.
        let (c, d) = (self.b.get(cur_l, et, at), self.b.get(end_l, et, at));
        let below = self.b.prim(lt, &[c, d], Ty::BOOL, at);
        let go = if exclusive {
            below
        } else {
            let ob = self.b.open_block();
            let incv = self.b.get(inc_l, Ty::BOOL, at);
            let ab = self.b.open_block();
            let (c, d) = (self.b.get(cur_l, et, at), self.b.get(end_l, et, at));
            let at_end = self.b.prim(PrimOp::Eq as u32, &[c, d], Ty::BOOL, at);
            let ablk = self.b.close_block(ab, Some(at_end), Ty::BOOL, at);
            let both = self.b.emit(Tag::And, incv.0, ablk.0, Ty::BOOL, at);
            let oblk = self.b.close_block(ob, Some(both), Ty::BOOL, at);
            self.b.emit(Tag::Or, below.0, oblk.0, Ty::BOOL, at)
        };
        // The round: bind, advance, then the body.
        let tb = self.b.open_block();
        self.loops.push((lp, result));
        self.scopes.push(HashMap::default());
        let item = self.b.get(cur_l, et, at);
        let mut binds = Vec::new();
        self.declare_pattern(pat, et, &mut binds)?;
        for (_, l) in &binds {
            self.set_local(*l, item, at);
        }
        if exclusive {
            let c = self.b.get(cur_l, et, at);
            let one = self.b.const_value(et, 1);
            let next = self.b.prim(PrimOp::Add as u32, &[c, one], et, at);
            self.set_local(cur_l, next, at);
        } else {
            let ib = self.b.open_block();
            let c = self.b.get(cur_l, et, at);
            let one = self.b.const_value(et, 1);
            let next = self.b.prim(PrimOp::Add as u32, &[c, one], et, at);
            self.set_local(cur_l, next, at);
            let iblk = self.b.close_block(ib, None, Ty::VOID, at);
            let fb = self.b.open_block();
            let no = self.b.const_value(Ty::BOOL, 0);
            self.set_local(inc_l, no, at);
            let fblk = self.b.close_block(fb, None, Ty::VOID, at);
            let rec = self.b.refs_record(&[iblk, fblk]);
            self.b.emit(Tag::If, below.0, rec, Ty::VOID, at);
        }
        body(self)?;
        self.scopes.pop();
        self.loops.pop();
        let then = self.b.close_block(tb, None, Ty::VOID, at);
        let eb = self.b.open_block();
        self.loop_exit(lp, loop_else, at, true)?;
        let els = self.b.close_block(eb, None, Ty::VOID, at);
        let rec = self.b.refs_record(&[then, els]);
        self.b.emit(Tag::If, go.0, rec, Ty::VOID, at);
        let lbody = self.b.close_block(lb, None, Ty::VOID, at);
        let ty = result.unwrap_or(Ty::VOID);
        Ok((self.b.close_loop(lp, lbody, ty, at), ty))
    }

    /// A `for` over a built-in `List` or `Map` as a counted loop
    /// (lowering-catalog "Loops"): no iterator value. The loop iterates a
    /// snapshot of the collection as it was when the loop began
    /// (`flow.for.snapshot`): `SnapBegin` shares its buffer, which a change
    /// to the collection during the loop copies first, and `SnapEnd`
    /// releases it when the loop ends. A fresh source value needs no
    /// snapshot: the loop owns that value and ordinary local lifetime
    /// handling releases it. Each round reads the next element or live
    /// entry, advances, and binds it.
    fn collection_loop(
        &mut self,
        pat: NodeRef<'_>,
        (sr, st, item_ty, map): (Ref, Ty, Ty, bool),
        loop_else: Option<(hd_base::NodeIdx, Ty)>,
        e: NodeRef<'_>,
        body: &mut dyn FnMut(&mut Self) -> StageResult<()>,
    ) -> StageResult<(Ref, Ty)> {
        use hd_tir::ir::local_flags::ASSIGNED;
        let result = loop_else.map(|(_, t)| t);
        let at = e.index();
        let us = Ty::prim(Prim::Usize);
        let intr = |me: &mut Self, op: IntrinsicOp, args: &[Ref], t: Ty| {
            let rec = me.b.refs_record(args);
            me.b.emit(Tag::Intrinsic, op as u32, rec, t, at)
        };
        let sym = |me: &Self, s: &str| me.cx.names.syms.intern(s);
        let (src_s, len_s, pos_s, end_s) = (
            sym(self, "$coll"),
            sym(self, "$len"),
            sym(self, "$pos"),
            sym(self, "$end"),
        );
        // A value the source expression made has no other place the body
        // can change. Let the loop own it directly, as an ordinary stored
        // value does; only a source read out of a place needs a snapshot.
        let orig_l = if self.is_fresh(sr) {
            None
        } else {
            let l = self.b.local(st, sym(self, "$orig"), ASSIGNED, at);
            self.set_local(l, sr, at);
            Some(l)
        };
        let src_l = self.b.local(st, src_s, ASSIGNED, at);
        let source = if let Some(orig_l) = orig_l {
            let o = self.b.get(orig_l, st, at);
            intr(self, IntrinsicOp::SnapBegin, &[o], st)
        } else {
            sr
        };
        self.set_local(src_l, source, at);
        let len_l = self.b.local(us, len_s, ASSIGNED, at);
        let pos_l = self.b.local(us, pos_s, ASSIGNED, at);
        let end_l = self.b.local(us, end_s, ASSIGNED, at);
        let c = self.b.get(src_l, st, at);
        let len_op = if map {
            IntrinsicOp::MapLen
        } else {
            IntrinsicOp::ListLen
        };
        let n = intr(self, len_op, &[c], us);
        self.set_local(len_l, n, at);
        // A map's loop visits the slots written when it began; a list's,
        // its captured length.
        let end = if map {
            let c = self.b.get(src_l, st, at);
            intr(self, IntrinsicOp::MapSlots, &[c], us)
        } else {
            self.b.get(len_l, us, at)
        };
        self.set_local(end_l, end, at);
        let zero = self.b.const_value(us, 0);
        self.set_local(pos_l, zero, at);
        let lp = self.b.open_loop();
        let lb = self.b.open_block();
        if map {
            let (c, p, d) = (
                self.b.get(src_l, st, at),
                self.b.get(pos_l, us, at),
                self.b.get(end_l, us, at),
            );
            let next = intr(self, IntrinsicOp::MapSlotNext, &[c, p, d], us);
            self.set_local(pos_l, next, at);
        }
        let (p, d) = (self.b.get(pos_l, us, at), self.b.get(end_l, us, at));
        let go = self.b.prim(PrimOp::Lt as u32, &[p, d], Ty::BOOL, at);
        // The round: read, advance, then bind and run the body.
        let tb = self.b.open_block();
        self.loops.push((lp, result));
        self.scopes.push(HashMap::default());
        let (c, p) = (self.b.get(src_l, st, at), self.b.get(pos_l, us, at));
        // A map's live entry, or a list's element place `[%c [%p]]`.
        let item = if map {
            intr(self, IntrinsicOp::MapSlotEntry, &[c, p], item_ty)
        } else {
            self.b.element(c, p, item_ty, at)
        };
        let p = self.b.get(pos_l, us, at);
        let one = self.b.const_value(us, 1);
        let next = self.b.prim(PrimOp::Add as u32, &[p, one], us, at);
        self.set_local(pos_l, next, at);
        let mut binds = Vec::new();
        let before = self.diags.len();
        self.declare_pattern(pat, item_ty, &mut binds)?;
        if self.diags.len() == before {
            self.irrefutable_loop_pattern(pat, item_ty);
        }
        let ab = self.b.open_block();
        body(self)?;
        let arm = self.b.close_block(ab, None, Ty::VOID, at);
        let rows = vec![crate::pat::Row {
            pat: Some(pat),
            wrap_some: false,
            binds,
            guard: None,
            arm: 0,
        }];
        let db = self.b.open_block();
        self.decide(&rows, item, item_ty)?;
        let dec = self.b.close_block(db, None, Ty::NEVER, at);
        let rec = self.b.refs_record(&[dec, arm]);
        self.b.emit(Tag::Match, item.0, rec, Ty::VOID, at);
        self.scopes.pop();
        self.loops.pop();
        let then = self.b.close_block(tb, None, Ty::VOID, at);
        let eb = self.b.open_block();
        self.loop_exit(lp, loop_else, at, true)?;
        let els = self.b.close_block(eb, None, Ty::VOID, at);
        let rec = self.b.refs_record(&[then, els]);
        self.b.emit(Tag::If, go.0, rec, Ty::VOID, at);
        let lbody = self.b.close_block(lb, None, Ty::VOID, at);
        let ty = result.unwrap_or(Ty::VOID);
        let lr = self.b.close_loop(lp, lbody, ty, at);
        if let Some(orig_l) = orig_l {
            let (o, c) = (self.b.get(orig_l, st, at), self.b.get(src_l, st, at));
            intr(self, IntrinsicOp::SnapEnd, &[o, c], Ty::VOID);
        }
        Ok((lr, ty))
    }

    /// `_` in a pipe step: the piped value.
    fn placeholder_value(&mut self, n: NodeRef<'_>) -> (Ref, Ty) {
        if let Some(v) = self.placeholder.take() {
            return v;
        }
        self.err(
            Code::PlaceholderOutsidePipe,
            n,
            "`_` stands for a pipe's value only in its step",
        );
        self.poison_value(n)
    }

    /// `a |> f(_, b)`: `a` first, then the step with `a` in the
    /// placeholder's slot (`crates/hd_tir/DESIGN.md` "What The Checker Desugars").
    /// Whether the pipe step holds an indented suite: a `Block` starting
    /// on its own line (`expr.pipe.single-line`). Only blanks lie
    /// between such a block and its line break. A nested pipe's step
    /// belongs to that pipe, so only its left side is searched.
    fn pipe_step_suite(&self, step: NodeRef<'_>) -> bool {
        if step.kind() == SyntaxKind::Block {
            let span = self.cx.src.span(step);
            let bytes = self.cx.src.text.as_bytes();
            let mut start = usize::try_from(span.lo).unwrap_or(0).min(bytes.len());
            while start > 0 && matches!(bytes[start - 1], b' ' | b'\t' | b'\r') {
                start -= 1;
            }
            if start > 0 && bytes[start - 1] == b'\n' {
                return true;
            }
        }
        let mut kids = step.children();
        if step.kind() == SyntaxKind::PipeExpr {
            return kids.next().is_some_and(|lhs| self.pipe_step_suite(lhs));
        }
        kids.any(|child| self.pipe_step_suite(child))
    }

    fn pipe(&mut self, kids: &[NodeRef<'_>], want: Option<Ty>) -> StageResult<(Ref, Ty)> {
        let [lhs, step] = kids else {
            return unsupported("a pipe shape");
        };
        if self.pipe_step_suite(*step) {
            self.err(
                Code::MultiLinePipeStep,
                *step,
                "a pipe step must not contain an indented suite",
            );
            return Ok(self.poison_value(*step));
        }
        let v = self.expr(*lhs, None)?;
        if is_bare_path(*step) {
            // `expr.pipe.bare.call`: `value |> path` is the call
            // `path(value)`, the value already evaluated.
            let mut args = crate::call::Args::empty();
            args.positional.push(*lhs);
            let saved = self.pipe_arg.replace((lhs.index(), v));
            let saved_bare = std::mem::replace(&mut self.bare_step, true);
            let r = self.call_args(*step, *step, &args, want);
            self.bare_step = saved_bare;
            self.pipe_arg = saved;
            return r;
        }
        let (mut own, mut closed) = (Vec::new(), Vec::new());
        step_slots(*step, false, &mut own, &mut closed);
        let mut rejected = false;
        for slot in &closed {
            self.err(
                Code::PipePlaceholderInClosure,
                *slot,
                "a closure cannot use the pipe's `_`; bind the value first",
            );
            rejected = true;
        }
        if let Some(second) = own.get(1) {
            self.err(
                Code::DuplicatePipePlaceholder,
                *second,
                "a pipe step takes exactly one `_`",
            );
            rejected = true;
        }
        if rejected {
            return Ok(self.poison_value(*step));
        }
        if own.is_empty() {
            // `expr.pipe.bare.needs-placeholder`: not a bare step, no `_`.
            self.err(
                Code::PipeStepNeedsPlaceholder,
                *step,
                "write `_` where the piped value goes",
            );
            return Ok((Ref(NONE), Ty::NEVER));
        }
        let saved = self.placeholder.replace(v);
        let r = self.expr(*step, want);
        let unused = self.placeholder.take().is_some();
        self.placeholder = saved;
        if unused {
            self.err(
                Code::PipeStepNeedsPlaceholder,
                *step,
                "write `_` where the piped value goes",
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
        let pool = self.pool();
        let is_map = self.cx.src.tkind(self.cx.src.first(n)) == Some(TokenKind::LBrace);
        let Some((element, clauses)) = kids.split_last() else {
            return unsupported("an empty comprehension");
        };
        let want = want.map(|w| self.strip_mut(w));
        let (def, args) = if is_map {
            let map = self.cx.names.known.map;
            let a = match want.map(|w| pool.get(w)) {
                Some(TyData::Adt { def, args }) if def == map => pool.list_items(args).to_vec(),
                _ => vec![
                    self.infer.fresh(pool, VarKind::General),
                    self.infer.fresh(pool, VarKind::General),
                ],
            };
            (map, a)
        } else {
            let list = self.cx.names.known.list;
            let a = match want.map(|w| pool.get(w)) {
                Some(TyData::Adt { def, args }) if def == list => pool.list_items(args).to_vec(),
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
        self.set_local(acc, fresh, n.index());
        self.scopes.push(HashMap::default());
        let r = self.clauses(clauses, *element, (acc, mt), &args, n);
        self.scopes.pop();
        r?;
        let v = self.b.get(acc, mt, n.index());
        Ok((self.retype(v, t, n.index()), t))
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
                let kr = self.coerce(kr, kt, args[0], *k, "map key", ExpectPosition::General);
                let (vr, vt) = self.expr(*v, Some(args[1]))?;
                let vr = self.coerce(vr, vt, args[1], *v, "map value", ExpectPosition::General);
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
                let r = self.coerce(r, t, args[0], last, "element", ExpectPosition::General);
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
                self.expect(ct, Ty::BOOL, cond, "condition", ExpectPosition::General);
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
        let parts = self.closure_parts(n)?;
        self.closure_of(n, &parts, want)
    }

    /// The source parts of the `fn(params) -> R: body` expression `n`.
    pub(crate) fn closure_parts<'t>(&self, n: NodeRef<'t>) -> StageResult<ClosureParts<'t>> {
        let body = match Src::child(n, SyntaxKind::Block) {
            Some(b) => b,
            // An inline suite: one expression.
            None => match n.children().filter(|c| !c.kind().is_type()).last() {
                Some(e) => e,
                None => return unsupported("a closure without a body"),
            },
        };
        Ok(ClosureParts {
            params: Src::child(n, SyntaxKind::ParameterList),
            ret: n
                .children()
                .find(|c| c.kind().is_type() && c.kind() != SyntaxKind::RequirementRow),
            row: Src::child(n, SyntaxKind::RequirementRow),
            body,
            suspends: n
                .direct_token(&self.cx.src.parse.tokens, TokenKind::Bang)
                .is_some(),
            name: None,
            reassignable: false,
            generic: None,
            upto: None,
            form: None,
        })
    }

    /// A trailing block (`fn.trailing.closure`): a zero-argument closure
    /// whose result and row come from `want`, the callee's final
    /// parameter, and which suspends when that parameter does
    /// (`fn.trailing.suspending`, `fn.trailing.suspending.body`).
    pub(crate) fn trailing_closure(
        &mut self,
        block: NodeRef<'_>,
        want: Ty,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
        let w = self.infer.resolve(pool, want);
        let suspends = match pool.get(self.strip_mut(w)) {
            TyData::Fn { suspends, .. } => suspends,
            _ => false,
        };
        let parts = ClosureParts {
            params: None,
            ret: None,
            row: None,
            body: block,
            suspends,
            name: None,
            reassignable: false,
            generic: None,
            upto: None,
            form: None,
        };
        self.closure_of(block, &parts, Some(want))
    }

    /// A closure from its parts; `n` is the node its `Closure` names.
    fn closure_of(
        &mut self,
        n: NodeRef<'_>,
        parts: &ClosureParts<'_>,
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let (r, t, _) = self.closure_with(n, parts, want)?;
        Ok((r, t))
    }

    /// [`Self::closure_of`]; a local function's name (`parts.name`) is a
    /// local of the enclosing scope, bound before the body so the body can
    /// call it (`names.local-fn.visible`), and returned with it. The
    /// caller sets it to the closure.
    pub(crate) fn closure_with(
        &mut self,
        n: NodeRef<'_>,
        parts: &ClosureParts<'_>,
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty, Option<hd_base::LocalId>)> {
        let pool = self.pool();
        let wanted = want
            .map(|w| self.infer.resolve(pool, w))
            .map(|w| match pool.get(w) {
                TyData::Mut(i) => pool.get(i),
                d => d,
            });
        // Expected inputs of an arity not known here give no parameter
        // its type (`fn.type.ctor.inputs`).
        let poisoned = want.is_some_and(|w| self.infer.resolve(pool, w) == Ty::POISON);
        let (wparams, wret) = match wanted {
            Some(TyData::Fn {
                params,
                result,
                inputs,
                ..
            }) => {
                let ps = if inputs == hd_types::Inputs::Tuple {
                    vec![]
                } else {
                    pool.list_items(params).to_vec()
                };
                (ps, Some(result))
            }
            _ => (vec![], None),
        };
        // `types.lct.sites`: a closure whose result type is inferred joins
        // its final value and `return` operands, and widens in place when
        // an optional one follows plain ones (`join_site`).
        if parts.ret.is_some() {
            return self.closure_body(n, parts, (&wparams, poisoned), None);
        }
        self.join_site(wret, |this, ret| {
            this.closure_body(n, parts, (&wparams, poisoned), Some(ret))
        })
    }

    /// [`Self::closure_with`] from its parameters on: `ret` is the joined
    /// result type, or `None` for the written one.
    fn closure_body(
        &mut self,
        n: NodeRef<'_>,
        parts: &ClosureParts<'_>,
        (wparams, poisoned): (&[Ty], bool),
        ret: Option<Ty>,
    ) -> StageResult<(Ref, Ty, Option<hd_base::LocalId>)> {
        let pool = self.pool();
        let suspends = parts.suspends;
        let mut params = Vec::new();
        let mut ptys = Vec::new();
        self.scopes.push(HashMap::default());
        if let Some(pl) = parts.params {
            let upto = parts.upto.unwrap_or(usize::MAX);
            for (i, p) in pl
                .children()
                .filter(|c| c.kind() == SyntaxKind::Parameter)
                .enumerate()
            {
                let toks = &self.cx.src.parse.tokens;
                let name = p
                    .name(toks)
                    .map_or_else(|| format!("${i}"), |t| self.cx.src.text(t).to_owned());
                // A default sees the earlier parameters only. The others
                // are named, so an enclosing local of the same name is not
                // read by mistake (`fn.default.later-parameter`).
                if i >= upto {
                    let t = match Src::type_child(p) {
                        Some(tn) => self.ty_node(tn)?,
                        None => Ty::POISON,
                    };
                    let sym = self.cx.names.syms.intern(&name);
                    let l = self.b.local(t, sym, local_flags::NOT_YET, p.index());
                    self.scopes.last_mut().expect("scope").insert(sym, l);
                    continue;
                }
                let t = match Src::type_child(p) {
                    Some(tn) => self.ty_node(tn)?,
                    None => {
                        if let Some(t) = wparams.get(i).copied() {
                            t
                        } else {
                            // `fn.closure.needs-annotation`: the poisoned
                            // type keeps the body from reporting more. A
                            // poisoned expectation already has its error.
                            if !poisoned {
                                self.err(
                                    Code::ClosureParameterNeedsAnnotation,
                                    p,
                                    "a closure parameter needs a type annotation here",
                                );
                            }
                            Ty::POISON
                        }
                    }
                };
                let sym = self.cx.names.syms.intern(&name);
                self.check_local_name(sym, p);
                // `names.fn.params.scope`: value parameters have distinct
                // names. A default's closure re-binds the earlier ones.
                if parts.upto.is_none() && self.scopes.last().is_some_and(|s| s.contains_key(&sym))
                {
                    let msg = format!("`{name}` names an earlier parameter");
                    self.err(Code::DuplicateBinding, p, &msg);
                }
                self.check_prelude_shadow(sym, p);
                let l = self
                    .b
                    .local(t, sym, hd_tir::ir::local_flags::PARAM, p.index());
                self.scopes.last_mut().expect("scope").insert(sym, l);
                // `fn.param.mut`: a `mut` closure parameter is its own
                // changeable copy.
                if p.direct_token(&self.cx.src.parse.tokens, TokenKind::KwMut)
                    .is_some()
                    && !matches!(pool.get(t), TyData::Mut(_))
                {
                    self.mut_params.push(l);
                    self.mut_locals.push(l);
                }
                params.push(l);
                ptys.push(t);
            }
        }
        let written_ret = if let Some(annotation) = parts.ret
            && parts.name.is_some()
            && parts.upto.is_none()
        {
            Some(self.local_return_annotation(annotation, n)?)
        } else {
            None
        };
        let opaque = written_ret.filter(|&ty| pool.has_opaque(ty));
        let strict_before = self.strict_rets.len();
        let occurrences_before = self.opaque_rets.len();
        let ret = if let Some(opaque) = opaque {
            self.open_opaque_return(opaque)
        } else {
            match (ret, written_ret, parts.ret) {
                (Some(r), _, _) => r,
                (None, Some(r), _) => r,
                (None, None, Some(rt)) => self.ty_node(rt)?,
                (None, None, None) => self.join_target(None),
            }
        };
        let occurrences = occurrences_before..self.opaque_rets.len();
        let public_ret = opaque.unwrap_or(ret);
        // A written row, or the least row of the keys the body uses
        // (`req.row.omitted.closure-row`); the expected row is matched
        // against it afterwards, by row subsumption.
        let written = match parts.row {
            Some(r) => Some(self.row_of(r)?),
            None => None,
        };
        // The name's type before the body: the written row, or none yet.
        let own_row = written.unwrap_or(hd_types::RowId::EMPTY);
        let own = parts.name.map(|name| {
            let t = pool.intern_ty(&TyData::Fn {
                params: pool.list(&ptys),
                result: public_ret,
                row: own_row,
                suspends,
                inputs: hd_types::Inputs::Fixed,
            });
            let flags = if parts.reassignable {
                local_flags::ASSIGNED
            } else {
                local_flags::ASSIGNED | local_flags::SHORT
            };
            let l = self.b.local(t, name, flags, n.index());
            if let Some(f) = &parts.form {
                self.local_forms.insert(l, f.clone());
            }
            // The scope around the parameters' is the declaration's.
            let at = self.scopes.len() - 2;
            self.scopes[at].insert(name, l);
            l
        });
        let mark = self.b.open_sub(&params);
        // A generic local function's name: what its uses instantiate, over
        // its type so far (the written result and row), for a recursive
        // call in its body.
        if let (Some(l), Some(g)) = (own, &parts.generic) {
            let mut g = g.clone();
            g.sub = mark.sub().raw();
            g.fn_ty = self.b.local_ty(l);
            g.open = true;
            self.local_generics.insert(l, g);
        }
        self.subs.push(OpenSub {
            mark,
            depth: self.scopes.len() - 1,
        });
        self.rets.push(ret);
        self.rows.push(crate::body::RowFrame::Closure {
            written,
            used: hd_types::RowData::default(),
        });
        self.suspends.push(suspends);
        let saved_loops = std::mem::take(&mut self.loops);
        let blk = self.b.open_block();
        let e = parts.body;
        let (tail, _) = if e.kind() == SyntaxKind::Block {
            self.block_value(e, Some(ret))?
        } else {
            let (r, t) = self.expr(e, Some(ret))?;
            (
                Some(self.coerce(r, t, ret, e, "result", ExpectPosition::General)),
                ret,
            )
        };
        if opaque.is_some() {
            self.check_opaque_occurrences(occurrences, n)?;
            self.strict_rets.truncate(strict_before);
        }
        let root = self.b.close_block(blk, tail, ret, n.index());
        self.loops = saved_loops;
        let row = match self.rows.pop() {
            Some(crate::body::RowFrame::Closure {
                written: Some(r), ..
            }) => r,
            Some(crate::body::RowFrame::Closure { used, .. }) => pool.row(&used),
            _ => hd_types::RowId::EMPTY,
        };
        self.rets.pop();
        self.suspends.pop();
        self.subs.pop();
        self.scopes.pop();
        let ft = pool.intern_ty(&TyData::Fn {
            params: pool.list(&ptys),
            result: public_ret,
            row,
            suspends,
            inputs: hd_types::Inputs::Fixed,
        });
        // A local function that calls itself captures its own name: the
        // name is declared, a fresh shared cell, before the closure exists,
        // and set once it does (`fn.local.capture-rules`).
        if let Some(l) = own
            && self.b.body_mut().local_flags[l.idx()] & local_flags::CAPTURED != 0
        {
            if row != own_row {
                return unsupported(
                    "a recursive local function whose requirements are inferred; write its `$` clause",
                );
            }
            self.set_local(l, Ref(NONE), n.index());
            self.add_flags(l, local_flags::CAPTURED_ASSIGNED);
        }
        self.elem_ref_captures(mark, n);
        self.local_ref_captures(mark, n);
        Ok((self.b.close_sub(mark, root, ft, n.index()), ft, own))
    }

    /// `e?` on a `Result` or an `Option`: the success payload, or an early
    /// return of the failure (converted by `From` when the error types
    /// differ).
    fn try_expr(
        &mut self,
        n: NodeRef<'_>,
        kids: &[NodeRef<'_>],
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
        let Some(e) = kids.first() else {
            return unsupported("a `?` without an operand");
        };
        let ret = self.infer.resolve(pool, *self.rets.last().expect("ret"));
        let result = self.cx.names.known.result;
        // `expr.try.expected`: the operand's hint is `Result[T, E]` or `T?`,
        // by the nearest function's result kind.
        let hint = want
            .map(|w| self.strip_mut(w))
            .and_then(|w| match pool.get(ret) {
                TyData::Option(_) => Some(pool.intern_ty(&TyData::Option(w))),
                TyData::Adt { def, args } if def == result => {
                    let err = pool.list_items(args)[1];
                    Some(pool.intern_ty(&TyData::Adt {
                        def,
                        args: pool.list(&[w, err]),
                    }))
                }
                _ => None,
            });
        let (r, t) = self.expr(*e, hint)?;
        let t = self.infer.resolve(pool, t);
        if self.defer_base.is_some() && self.subs.len() == self.defer_subs {
            self.err(
                Code::DeferControlFlow,
                n,
                "a `defer` suite cannot leave through `?`",
            );
        }
        match (pool.get(t), pool.get(ret)) {
            (TyData::Option(inner), TyData::Option(_)) => {
                let ok_b = self.b.open_block();
                let v = self.b.payload(r, 1, 0, inner, n.index());
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
                let v = self.b.payload(r, 0, 0, okt, n.index());
                let ok = self.b.close_block(ok_b, Some(v), okt, n.index());
                let err_b = self.b.open_block();
                let ev = self.b.payload(r, 1, 0, et, n.index());
                let ev = if self.can_unify(et, ret_e) {
                    self.expect(et, ret_e, *e, "error", ExpectPosition::General);
                    ev
                } else if let Some((tv, mutable)) = self.dyn_ref_of(ret_e)
                    && self.dyn_fits(self.ref_of(et).map_or(et, |(inner, _)| inner), tv)
                {
                    let (ev, source) = if self.ref_of(et).is_none() {
                        self.new_object_ref(ev, et, mutable, *e)
                    } else {
                        (ev, et)
                    };
                    self.coerce(ev, source, ret_e, *e, "error", ExpectPosition::General)
                } else {
                    let from = self.cx.names.known.from;
                    let tref = hd_types::solver::TraitRef {
                        trait_: from,
                        self_ty: ret_e,
                        args: pool.list(&[et]),
                    };
                    // An error that neither is assignable nor converts is
                    // `invalid-result-propagation` (`expr.try.convert.none`).
                    let _ = self.require_ref_noting(tref, n, Code::InvalidResultPropagation)?;
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
                    "`?` needs a Result or an optional in a function returning one",
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
        let pool = self.pool();
        let inclusive = n
            .direct_token(&self.cx.src.parse.tokens, TokenKind::DotDotEq)
            .is_some();
        // The expected type of the bounds: the element of an expected
        // range type (`expr.index.expected.range`).
        let known = self.cx.names.known;
        let bound_want = want.and_then(|w| {
            let w = self.strip_mut(w);
            match pool.get(w) {
                TyData::Adt { def, args }
                    if [known.range, known.range_from, known.range_to].contains(&def) =>
                {
                    pool.list_items(args).first().copied()
                }
                _ => None,
            }
        });
        let first_tok = self.cx.src.first(n);
        let starts_open = matches!(
            self.cx.src.tkind(first_tok),
            Some(TokenKind::DotDot | TokenKind::DotDotEq)
        );
        let (def, fields): (DefId, Vec<NodeRef<'_>>) = match (kids.len(), starts_open) {
            (2, _) => (known.range, kids.to_vec()),
            (1, true) => (known.range_to, kids.to_vec()),
            (1, false) => (known.range_from, kids.to_vec()),
            // `expr.range.form.full`: `RangeFull {}`, with no bound and
            // no element type.
            (0, _) => {
                let t = pool.intern_ty(&TyData::Adt {
                    def: known.range_full,
                    args: pool.list(&[]),
                });
                let rec = self.b.refs_record(&[]);
                return Ok((self.b.emit(Tag::NewStruct, NONE, rec, t, n.index()), t));
            }
            _ => return unsupported("a range with more than two bounds"),
        };
        let et = self.infer.fresh(pool, VarKind::General);
        let mut refs = Vec::new();
        let mut bound_tys = Vec::new();
        for f in &fields {
            let (r, t) = self.expr(*f, Some(et))?;
            bound_tys.push(t);
            refs.push(self.coerce(r, t, et, *f, "range bound", ExpectPosition::Operand));
        }
        // Bounds that are all literals take the expected element type.
        if let Some(bw) = bound_want
            && let e = self.infer.resolve(pool, et)
            && matches!(pool.get(e), TyData::Infer(_))
            && self
                .infer
                .kind_of(pool, e)
                .is_some_and(|k| k.literal_default().is_some())
        {
            self.expect(e, bw, fields[0], "range bound", ExpectPosition::Operand);
        }
        // `expr.range.bound.numeric`: the element type is an integer or a
        // floating-point type. The one error lands on the first bound that
        // is neither.
        if !self.range_bound_like(et)
            && let Some(i) = bound_tys.iter().position(|t| !self.range_bound_like(*t))
        {
            let msg = format!(
                "a range bound must have an integer or floating-point type, found {}",
                self.show(bound_tys[i])
            );
            self.err(Code::TypeMismatch, fields[i], &msg);
        }
        if def != known.range_from {
            refs.push(self.b.const_value(Ty::BOOL, u64::from(inclusive)));
        }
        let t = pool.intern_ty(&TyData::Adt {
            def,
            args: pool.list(&[et]),
        });
        let rec = self.b.refs_record(&refs);
        Ok((self.b.emit(Tag::NewStruct, NONE, rec, t, n.index()), t))
    }

    /// The entries of `$.with(...)` or `$.context(...)` in source order
    /// (`req.context.order`), as `(key, value)` pairs, and every key they
    /// bind (`crates/hd_tir/DESIGN.md` §4.13.4, "Context values"). The keys are
    /// `None` when an entry's keys are unknown because it is already in
    /// error, so the caller reports nothing more about them.
    fn provider_entries(
        &mut self,
        al: NodeRef<'_>,
        lexical: bool,
    ) -> StageResult<(Vec<Ref>, Option<Vec<Ty>>)> {
        let pool = self.pool();
        // A `$.with` block also compares with the keys its scopes show
        // (`req.with.collision.compared`).
        let visible = if lexical {
            self.visible_keys()
        } else {
            Vec::new()
        };
        let mut pairs = Vec::new();
        let mut keys = Vec::new();
        let mut known = true;
        for entry in al
            .children()
            .filter(|c| c.kind() == SyntaxKind::ContextEntry)
        {
            let kids: Vec<NodeRef<'_>> = entry.children().collect();
            let before = keys.len();
            let (kn, vn) = match kids.as_slice() {
                [kn, vn] => (kn, vn),
                [sn] if sn.kind() == SyntaxKind::SpreadExpr => {
                    if let Some(vn) = sn.children().next() {
                        known &= self.context_spread(vn, &mut pairs, &mut keys)?;
                    }
                    self.check_key_collision(entry, &keys[before..], &keys[..before], &visible);
                    continue;
                }
                // The parser reported the malformed entry.
                _ => {
                    known = false;
                    continue;
                }
            };
            let key = self.key_node(*kn)?;
            if key == Ty::POISON {
                known = false;
                continue;
            }
            let TyData::TraitValue { def, .. } = pool.get(key) else {
                return unsupported("a provider key that is not a trait");
            };
            // A provider outlives its scope (`req.use.value.outlives-scope`),
            // so a `$.use` provider installed again is counted
            // (`req.use.value.counted`).
            let hint = (crate::elem_refs::strip_parens(*vn).kind() == SyntaxKind::ContextExpr)
                .then(|| self.ref_ty(key, self.trait_is_mutable(def, 0), true));
            let (r, t) = self.expr(*vn, hint)?;
            let r = self.install_provider(r, t, (key, def), *vn);
            pairs.push(Ref(key.0));
            pairs.push(r);
            self.check_key_collision(entry, &[key], &keys, &visible);
            keys.push(key);
        }
        Ok((pairs, known.then_some(keys)))
    }

    /// A spread entry `ctx...` (`req.context.spread`): the pair keeps the
    /// context value under its own context type, and the entry binds
    /// that context's keys. Anything but a context value is
    /// `type-mismatch`. Returns whether the entry's keys are known.
    fn context_spread(
        &mut self,
        vn: NodeRef<'_>,
        pairs: &mut Vec<Ref>,
        keys: &mut Vec<Ty>,
    ) -> StageResult<bool> {
        let pool = self.pool();
        let (r, t) = self.expr(vn, None)?;
        let ct = self.strip_mut(self.infer.resolve(pool, t));
        match pool.get(ct) {
            TyData::Context(row) => {
                pairs.push(Ref(ct.0));
                pairs.push(r);
                keys.extend(pool.row_data(row).keys);
                return Ok(true);
            }
            TyData::Poison | TyData::Never => {}
            TyData::Infer(_)
                if self
                    .infer
                    .kind_of(pool, ct)
                    .is_none_or(|k| k.literal_default().is_none()) =>
            {
                return unsupported("a provider spread whose type is not yet known");
            }
            _ => {
                let msg = format!(
                    "a provider spread needs a context value `$.Context[...]`, found {}",
                    self.show(t)
                );
                self.err(Code::TypeMismatch, vn, &msg);
            }
        }
        Ok(false)
    }

    /// `$.with(K = p, ctx..., ...): block` (`req.with`): each provider is
    /// evaluated, must implement its key, and covers the key in the block.
    fn with_expr(&mut self, n: NodeRef<'_>, want: Option<Ty>) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
        let Some(al) = Src::child(n, SyntaxKind::ArgumentList) else {
            return unsupported("a `$.with` without providers");
        };
        let (pairs, keys) = self.provider_entries(al, true)?;
        let Some(body) = Src::child(n, SyntaxKind::Block) else {
            return unsupported("a `$.with` without a block");
        };
        // An entry in error covers every key, so its block reports no
        // missing key on its account.
        let frame = match keys {
            Some(keys) => crate::body::RowFrame::With(pool.row(&hd_types::RowData {
                keys,
                params: vec![],
            })),
            None => crate::body::RowFrame::Any,
        };
        self.rows.push(frame);
        let bm = self.b.open_block();
        let r = self.block_value(body, want);
        self.rows.pop();
        let (tail, ty) = r?;
        let blk = self.b.close_block(bm, tail, ty, body.index());
        let rec = self.b.refs_record(&pairs);
        Ok((self.b.emit(Tag::With, rec, blk.0, ty, n.index()), ty))
    }

    /// `$.context(K = p, ctx..., ...)` (`req.context.create`): a context
    /// value over every key its entries bind, one provider per key
    /// (`req.context.one-per-key`).
    fn context_new(&mut self, n: NodeRef<'_>) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
        let Some(al) = Src::child(n, SyntaxKind::ArgumentList) else {
            return unsupported("a `$.context` without providers");
        };
        let (pairs, keys) = self.provider_entries(al, false)?;
        // An entry in error leaves the context's row unknown.
        let t = match keys {
            Some(keys) => pool.intern_ty(&TyData::Context(pool.row(&hd_types::RowData {
                keys,
                params: vec![],
            }))),
            None => Ty::POISON,
        };
        let rec = self.b.refs_record(&pairs);
        Ok((self.b.emit(Tag::ContextNew, rec, NONE, t, n.index()), t))
    }

    /// `$.use(K, ...)`: the covering provider of each `K`, which the body's
    /// row must name (spec/lang/11-requirements-and-suspension.md); `$.with` and
    /// `$.context` go to their own checks.
    fn context_expr(&mut self, n: NodeRef<'_>, want: Option<Ty>) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
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
        if toks.get(2).map(String::as_str) == Some("context") {
            return self.context_new(n);
        }
        if toks.get(2).map(String::as_str) != Some("use") {
            return unsupported("this `$.` form in an expression");
        }
        let mut refs = Vec::new();
        let mut tys = Vec::new();
        let keys: Vec<NodeRef<'_>> = n.children().filter(|c| c.kind().is_type()).collect();
        // `req.use.value.counted`: a provider is counted where a counted
        // reference is expected, for one key or for its tuple element.
        let want = want.map(|w| self.infer.resolve(pool, w));
        let counted_at = |me: &Self, i: usize| -> bool {
            let Some(w) = want else { return false };
            let w = if keys.len() == 1 {
                Some(w)
            } else if let TyData::Tuple { elems, .. } = pool.get(w) {
                pool.list_items(elems).get(i).copied()
            } else {
                None
            };
            w.and_then(|w| me.ref_counted(w)).unwrap_or(false)
        };
        for (i, &kn) in keys.iter().enumerate() {
            let counted = counted_at(self, i);
            let (r, t) = self.provider_get(kn, n, counted)?;
            if t == Ty::POISON {
                return Ok((Ref(NONE), Ty::POISON));
            }
            refs.push(r);
            tys.push(t);
        }
        match tys[..] {
            [] => unsupported("`$.use` without a key"),
            [t] => Ok((refs[0], t)),
            // `$.use(A, B)` is the tuple of the providers in the requested
            // key order (`req.use.order`).
            _ => {
                let t = pool.intern_ty(&TyData::Tuple {
                    elems: pool.list(&tys),
                    rest: None,
                });
                let rec = self.b.refs_record(&refs);
                Ok((self.b.emit(Tag::NewTuple, NONE, rec, t, n.index()), t))
            }
        }
    }

    /// The covering provider of the one key `kn` of `$.use` node `n`.
    fn provider_get(
        &mut self,
        kn: NodeRef<'_>,
        n: NodeRef<'_>,
        counted: bool,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
        let key = self.key_node(kn)?;
        if key == Ty::POISON {
            return Ok((Ref(NONE), Ty::POISON));
        }
        let TyData::TraitValue { def: key_trait, .. } = pool.get(key) else {
            return unsupported("a `$.use` key that is not a trait");
        };
        self.require_key(key, n);
        let at = self.b.refs_record(&[Ref(key.0)]);
        let (pt, by_ref) = self.provider_ty(key, key_trait, counted);
        if by_ref {
            // The provider holds the trait value reference itself
            // (`Coercion::RefAsProvider`).
            let p = self.b.emit(Tag::ProviderGet, at, NONE, key, n.index());
            let r = self
                .b
                .coerce(Coercion::ProviderAsRef, NONE, p, pt, n.index());
            return Ok((r, pt));
        }
        Ok((self.b.emit(Tag::ProviderGet, at, NONE, pt, n.index()), pt))
    }
}

/// The source parts of a closure: a `fn(...)` expression's, or a trailing
/// block's, which has only a body.
pub(crate) struct ClosureParts<'t> {
    pub params: Option<NodeRef<'t>>,
    pub ret: Option<NodeRef<'t>>,
    pub row: Option<NodeRef<'t>>,
    /// A `Block`, or an inline suite's one expression.
    pub body: NodeRef<'t>,
    pub suspends: bool,
    /// The name of the local the closure initializes, visible in its own
    /// body: a local function's (`names.local-fn.visible`) or a binding's
    /// (`names.scope.recursive-closure`).
    pub name: Option<hd_base::Symbol>,
    /// Whether that local is a `let` binding, which `=` may reassign
    /// (`fn.recursion.closure.let`), not a `:=` or a local function.
    pub reassignable: bool,
    /// A generic local function's own type parameters: its name is
    /// instantiated at each use, never read as one closure value.
    pub generic: Option<crate::local::LocalGeneric>,
    /// Only the first this many parameters are the closure's: a local
    /// function's default is a closure of the earlier parameters.
    pub upto: Option<usize>,
    /// A local function's parameter names and default closures, kept for
    /// its calls (`fn.default.omit`, named arguments).
    pub form: Option<crate::local::LocalForm>,
}
