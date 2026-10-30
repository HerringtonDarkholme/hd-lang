//! Closed const evaluation of const generic arguments
//! (`types.generic.const.argument` to `types.generic.const.nonconstant`).
//! It reads syntax only: the parameter's declared type types the literals,
//! so header lowering and body checks share it.

use hd_diag::Code;
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_types::{ConstValue, ParamRef, Prim};

use crate::Src;
use crate::literal::{Piece, decode_piece, radix_digits, split_suffix, strip_piece};

/// What a name in a const argument denotes, as the caller's scope sees it.
pub enum ConstName<'a> {
    /// A const parameter in scope (`types.generic.const.type-position`).
    Param(ParamRef),
    /// A top-level `:=` binding (`types.generic.const.named`): its
    /// initializer in its module's source.
    Binding { src: Src<'a>, init: NodeRef<'a> },
    /// A value known only at run time: a local, a parameter, a function.
    Runtime,
    /// No value of that name is in scope.
    Unknown,
    /// A type, which no const slot takes.
    Type,
    /// A name already reported, or one this evaluator cannot read yet.
    Poison,
}

/// A const argument: an evaluated value, or a bare const parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstArg {
    Value(ConstValue),
    Param(ParamRef),
    /// An error already reported elsewhere.
    Poison,
}

/// Why an argument is not a const value, as a diagnostic on the argument.
#[derive(Debug, PartialEq, Eq)]
pub struct ConstError {
    pub code: Code,
    pub message: String,
}

/// A value while evaluating; an integer is held wide and checked against
/// its type after each step.
#[derive(Clone, Debug, PartialEq)]
enum V {
    Int(i128, Prim),
    Bool(bool),
    Char(char),
    Str(String),
}

impl V {
    fn prim(&self) -> Prim {
        match self {
            V::Int(_, p) => *p,
            V::Bool(_) => Prim::Bool,
            V::Char(_) => Prim::Char,
            V::Str(_) => Prim::String,
        }
    }
}

enum Fail {
    Error(ConstError),
    Poison,
}

fn failed(what: &str) -> Fail {
    Fail::Error(ConstError {
        code: Code::ConstEvaluationFailed,
        message: format!("const evaluation failed: {what}"),
    })
}

fn nonconstant(why: &str) -> Fail {
    Fail::Error(ConstError {
        code: Code::NonconstantGenericArgument,
        message: format!("a const generic argument must be a closed const expression: {why}"),
    })
}

fn mismatch_error(want: Prim, found: Prim) -> ConstError {
    ConstError {
        code: Code::TypeMismatch,
        message: format!(
            "this const argument has type `{}`, but `{}` is expected",
            found.name(),
            want.name()
        ),
    }
}

fn mismatch(want: Prim, found: Prim) -> Fail {
    Fail::Error(mismatch_error(want, found))
}

/// Named constants nest at most this deep; a deeper chain is a cycle.
const DEPTH: u32 = 64;

/// The range of an integer type.
fn range(p: Prim) -> (i128, i128) {
    match p {
        Prim::I8 => (i128::from(i8::MIN), i128::from(i8::MAX)),
        Prim::I16 => (i128::from(i16::MIN), i128::from(i16::MAX)),
        Prim::I32 => (i128::from(i32::MIN), i128::from(i32::MAX)),
        Prim::I64 => (i128::from(i64::MIN), i128::from(i64::MAX)),
        Prim::U8 => (0, i128::from(u8::MAX)),
        Prim::U16 => (0, i128::from(u16::MAX)),
        Prim::U32 | Prim::Usize => (0, i128::from(u32::MAX)),
        _ => (0, i128::from(u64::MAX)),
    }
}

fn width(p: Prim) -> u32 {
    match p {
        Prim::I8 | Prim::U8 => 8,
        Prim::I16 | Prim::U16 => 16,
        Prim::I32 | Prim::U32 | Prim::Usize => 32,
        _ => 64,
    }
}

fn int(v: i128, p: Prim) -> Result<V, Fail> {
    let (lo, hi) = range(p);
    if (lo..=hi).contains(&v) {
        Ok(V::Int(v, p))
    } else {
        Err(failed(&format!(
            "overflow: the result does not fit `{}`",
            p.name()
        )))
    }
}

/// `v` wrapped to the bits of `p` (a shift discards its high bits,
/// `expr.shift.fixed-width`).
fn wrap(v: i128, p: Prim) -> i128 {
    let w = width(p);
    let bits = u128::from_ne_bytes(v.to_ne_bytes()) & ((1u128 << w) - 1);
    // At most 64 bits, so it fits.
    let bits = i128::try_from(bits).unwrap_or_default();
    if p.is_unsigned() || bits >> (w - 1) == 0 {
        bits
    } else {
        bits - (1i128 << w)
    }
}

/// Evaluates `arg`, the argument of a const parameter of type `want`
/// (`types.generic.const.evaluate`). `want` is `None` when the parameter's
/// declared type is itself an error. `lookup` reads a name in the
/// argument's scope.
///
/// # Errors
///
/// The diagnostic for the argument when it is not a closed const
/// expression of type `want`, or its evaluation fails.
pub fn const_argument<'a>(
    src: &Src<'a>,
    arg: NodeRef<'a>,
    want: Option<Prim>,
    syms: &hd_intern::ShardedInterner,
    lookup: &mut dyn FnMut(&str) -> ConstName<'a>,
) -> Result<ConstArg, ConstError> {
    let arg = match arg.kind() {
        SyntaxKind::ConstArgument => match arg.children().next() {
            Some(e) => e,
            None => return Ok(ConstArg::Poison),
        },
        _ => arg,
    };
    if let Some(name) = bare_name(src, arg)
        && let ConstName::Param(p) = lookup(name)
    {
        return Ok(ConstArg::Param(p));
    }
    let mut ev = Eval { lookup, depth: 0 };
    let v = match ev.eval(src, arg, want) {
        Ok(v) => v,
        Err(Fail::Poison) => return Ok(ConstArg::Poison),
        Err(Fail::Error(e)) => return Err(e),
    };
    let Some(want) = want else {
        return Ok(ConstArg::Poison);
    };
    if v.prim() != want {
        return Err(mismatch_error(want, v.prim()));
    }
    Ok(ConstArg::Value(match v {
        // In range for `prim`, so a negative value fits `i64` and any
        // other `u64`.
        V::Int(i, prim) => ConstValue::Int {
            prim,
            bits: match i64::try_from(i) {
                Ok(v) if v < 0 => u64::from_ne_bytes(v.to_ne_bytes()),
                _ => u64::try_from(i).unwrap_or_default(),
            },
        },
        V::Bool(b) => ConstValue::Bool(b),
        V::Char(c) => ConstValue::Char(c),
        V::Str(s) => ConstValue::Str(syms.intern(&s)),
    }))
}

/// The name an argument is when it is one bare name.
fn bare_name<'a>(src: &Src<'a>, n: NodeRef<'_>) -> Option<&'a str> {
    if !matches!(n.kind(), SyntaxKind::NameExpr | SyntaxKind::NamedType)
        || n.children().next().is_some()
    {
        return None;
    }
    let toks: Vec<_> = src.tokens(n).collect();
    match toks.as_slice() {
        [t] if matches!(src.tkind(*t), Some(TokenKind::Ident | TokenKind::RawIdent)) => {
            Some(src.text(*t).trim_start_matches("r#"))
        }
        _ => None,
    }
}

struct Eval<'l, 'a> {
    lookup: &'l mut dyn FnMut(&str) -> ConstName<'a>,
    depth: u32,
}

impl<'a> Eval<'_, 'a> {
    /// The type `n` has on its own, before a context types its literals:
    /// a suffix, a non-integer literal, an operator's result, or a named
    /// constant's. `None` for an unsuffixed integer.
    fn natural(&mut self, src: &Src<'a>, n: NodeRef<'a>) -> Option<Prim> {
        let kids: Vec<_> = n.children().collect();
        match n.kind() {
            SyntaxKind::LiteralExpr => {
                let t = src.first(n);
                match src.tkind(t)? {
                    TokenKind::KwTrue | TokenKind::KwFalse => Some(Prim::Bool),
                    TokenKind::Char => Some(Prim::Char),
                    TokenKind::Number => {
                        let (_, suffix) = split_suffix(src.text(t));
                        Prim::ALL.iter().copied().find(|p| p.name() == suffix)
                    }
                    _ => None,
                }
            }
            SyntaxKind::StringExpr => Some(Prim::String),
            SyntaxKind::ParenExpr => self.natural(src, *kids.first()?),
            SyntaxKind::UnaryExpr => match src.tkind(src.first(n))? {
                TokenKind::Bang => Some(Prim::Bool),
                _ => self.natural(src, *kids.first()?),
            },
            SyntaxKind::ComparisonExpr | SyntaxKind::LogicalAndExpr | SyntaxKind::LogicalOrExpr => {
                Some(Prim::Bool)
            }
            SyntaxKind::ShiftExpr | SyntaxKind::PowerExpr => self.natural(src, *kids.first()?),
            SyntaxKind::AdditiveExpr
            | SyntaxKind::MultiplicativeExpr
            | SyntaxKind::BitwiseAndExpr
            | SyntaxKind::BitwiseOrExpr
            | SyntaxKind::BitwiseXorExpr => {
                let l = self.natural(src, *kids.first()?);
                l.or_else(|| self.natural(src, *kids.get(1)?))
            }
            SyntaxKind::NameExpr | SyntaxKind::NamedType => {
                let name = bare_name(src, n)?;
                match (self.lookup)(name) {
                    ConstName::Binding { src, init } => {
                        if self.depth >= DEPTH {
                            return None;
                        }
                        self.depth += 1;
                        let t = self.natural(&src, init);
                        self.depth -= 1;
                        t
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// The common type of two operands, or `fallback` when neither has one.
    fn common(
        &mut self,
        src: &Src<'a>,
        (l, r): (NodeRef<'a>, NodeRef<'a>),
        want: Option<Prim>,
        fallback: Prim,
    ) -> Prim {
        want.or_else(|| self.natural(src, l))
            .or_else(|| self.natural(src, r))
            .unwrap_or(fallback)
    }

    fn eval(&mut self, src: &Src<'a>, n: NodeRef<'a>, want: Option<Prim>) -> Result<V, Fail> {
        let kids: Vec<_> = n.children().collect();
        match n.kind() {
            SyntaxKind::LiteralExpr => literal(src, n, want, false),
            SyntaxKind::StringExpr => string(src, n),
            SyntaxKind::ParenExpr => match kids.as_slice() {
                [e] => self.eval(src, *e, want),
                _ => Err(nonconstant("a tuple is not a scalar")),
            },
            SyntaxKind::NameExpr | SyntaxKind::NamedType => self.name(src, n, want),
            SyntaxKind::UnaryExpr => self.unary(src, n, &kids, want),
            SyntaxKind::LogicalAndExpr | SyntaxKind::LogicalOrExpr => {
                let [l, r] = kids.as_slice() else {
                    return Err(Fail::Poison);
                };
                let and = n.kind() == SyntaxKind::LogicalAndExpr;
                let a = self.bool(src, *l)?;
                // `types.generic.const.short-circuit`.
                if a != and {
                    return Ok(V::Bool(a));
                }
                Ok(V::Bool(self.bool(src, *r)?))
            }
            SyntaxKind::ComparisonExpr => self.comparison(src, &kids),
            SyntaxKind::AdditiveExpr
            | SyntaxKind::MultiplicativeExpr
            | SyntaxKind::BitwiseAndExpr
            | SyntaxKind::BitwiseOrExpr
            | SyntaxKind::BitwiseXorExpr
            | SyntaxKind::ShiftExpr
            | SyntaxKind::PowerExpr => self.binary(src, n, &kids, want),
            SyntaxKind::CallExpr => Err(nonconstant("a call or cast is not a const operation")),
            _ => Err(nonconstant(
                "this operation is not available to const evaluation",
            )),
        }
    }

    fn bool(&mut self, src: &Src<'a>, n: NodeRef<'a>) -> Result<bool, Fail> {
        match self.eval(src, n, Some(Prim::Bool))? {
            V::Bool(b) => Ok(b),
            v => Err(mismatch(Prim::Bool, v.prim())),
        }
    }

    fn name(&mut self, src: &Src<'a>, n: NodeRef<'a>, want: Option<Prim>) -> Result<V, Fail> {
        let Some(name) = bare_name(src, n) else {
            return Err(nonconstant(
                "this operation is not available to const evaluation",
            ));
        };
        match (self.lookup)(name) {
            ConstName::Param(_) => Err(nonconstant(&format!(
                "`{name}` is a const parameter, and stage 1 has no symbolic const arithmetic"
            ))),
            ConstName::Runtime => Err(nonconstant(&format!("`{name}` depends on runtime data"))),
            ConstName::Poison => Err(Fail::Poison),
            ConstName::Unknown => Err(Fail::Error(ConstError {
                code: Code::UnknownName,
                message: format!("no value named `{name}` is in scope"),
            })),
            ConstName::Type => Err(Fail::Error(ConstError {
                code: Code::GenericKindMismatch,
                message: format!("`{name}` is a type, but a const parameter takes a value"),
            })),
            // The use's type types the initializer (`types.generic.const.evaluate`).
            ConstName::Binding { src: at, init } => {
                if self.depth >= DEPTH {
                    return Err(nonconstant(&format!("`{name}` refers to itself")));
                }
                self.depth += 1;
                let v = self.eval(&at, init, want);
                self.depth -= 1;
                v
            }
        }
    }

    fn unary(
        &mut self,
        src: &Src<'a>,
        n: NodeRef<'a>,
        kids: &[NodeRef<'a>],
        want: Option<Prim>,
    ) -> Result<V, Fail> {
        let [e] = kids else {
            return Err(Fail::Poison);
        };
        match src.tkind(src.first(n)) {
            Some(TokenKind::Bang) => Ok(V::Bool(!self.bool(src, *e)?)),
            Some(TokenKind::Minus) => {
                let p = want.or_else(|| self.natural(src, *e)).unwrap_or(Prim::I32);
                if e.kind() == SyntaxKind::LiteralExpr
                    && src.tkind(src.first(*e)) == Some(TokenKind::Number)
                {
                    return literal(src, *e, Some(p), true);
                }
                match self.eval(src, *e, Some(p))? {
                    // `expr.arith.unary-minus`.
                    V::Int(_, p) if p.is_unsigned() => Err(mismatch(Prim::I64, p)),
                    V::Int(v, p) => int(-v, p),
                    v => Err(mismatch(p, v.prim())),
                }
            }
            Some(TokenKind::Plus) => {
                let p = want.or_else(|| self.natural(src, *e)).unwrap_or(Prim::I32);
                match self.eval(src, *e, Some(p))? {
                    v @ V::Int(..) => Ok(v),
                    v => Err(mismatch(p, v.prim())),
                }
            }
            Some(TokenKind::Tilde) => {
                let p = want.or_else(|| self.natural(src, *e)).unwrap_or(Prim::I32);
                match self.eval(src, *e, Some(p))? {
                    V::Int(v, p) => Ok(V::Int(wrap(!v, p), p)),
                    v => Err(mismatch(p, v.prim())),
                }
            }
            _ => Err(nonconstant(
                "this operation is not available to const evaluation",
            )),
        }
    }

    fn comparison(&mut self, src: &Src<'a>, kids: &[NodeRef<'a>]) -> Result<V, Fail> {
        let [l, r] = kids else {
            return Err(Fail::Poison);
        };
        let op = op_after(src, *l);
        let p = self.common(src, (*l, *r), None, Prim::I32);
        let (a, b) = (self.eval(src, *l, Some(p))?, self.eval(src, *r, Some(p))?);
        if a.prim() != b.prim() {
            return Err(mismatch(a.prim(), b.prim()));
        }
        let ord = match (&a, &b) {
            (V::Int(x, _), V::Int(y, _)) => x.cmp(y),
            (V::Char(x), V::Char(y)) => x.cmp(y),
            (V::Str(x), V::Str(y)) => x.cmp(y),
            (V::Bool(x), V::Bool(y)) if matches!(op, Some(TokenKind::EqEq | TokenKind::NotEq)) => {
                x.cmp(y)
            }
            _ => return Err(mismatch(Prim::I32, a.prim())),
        };
        Ok(V::Bool(match op {
            Some(TokenKind::EqEq) => ord.is_eq(),
            Some(TokenKind::NotEq) => ord.is_ne(),
            Some(TokenKind::Lt) => ord.is_lt(),
            Some(TokenKind::LtEq) => ord.is_le(),
            Some(TokenKind::Gt) => ord.is_gt(),
            Some(TokenKind::GtEq) => ord.is_ge(),
            _ => {
                return Err(nonconstant(
                    "this operation is not available to const evaluation",
                ));
            }
        }))
    }

    fn binary(
        &mut self,
        src: &Src<'a>,
        n: NodeRef<'a>,
        kids: &[NodeRef<'a>],
        want: Option<Prim>,
    ) -> Result<V, Fail> {
        let [l, r] = kids else {
            return Err(Fail::Poison);
        };
        let op = op_after(src, *l);
        // A shift count and an exponent type apart from the left operand
        // (`expr.shift.count-literal`, `expr.power.int.literal`).
        if matches!(n.kind(), SyntaxKind::ShiftExpr | SyntaxKind::PowerExpr) {
            let p = want.or_else(|| self.natural(src, *l)).unwrap_or(Prim::I32);
            let count = self.natural(src, *r).unwrap_or(Prim::U32);
            let (a, b) = (
                self.eval(src, *l, Some(p))?,
                self.eval(src, *r, Some(count))?,
            );
            let (V::Int(x, p), V::Int(c, cp)) = (&a, &b) else {
                let bad = if matches!(a, V::Int(..)) { &b } else { &a };
                return Err(mismatch(p, bad.prim()));
            };
            let (x, p, c) = (*x, *p, *c);
            if n.kind() == SyntaxKind::PowerExpr {
                if !cp.is_unsigned() {
                    return Err(mismatch(Prim::U32, *cp));
                }
                // `expr.power.checked`: a base of magnitude two or more
                // overflows every type within 64 steps.
                let acc = match x {
                    0 => i128::from(c == 0),
                    1 => 1,
                    -1 => 1 - 2 * (c % 2),
                    _ => {
                        let mut acc: i128 = 1;
                        for _ in 0..c {
                            acc = int(acc * x, p).map(|_| acc * x)?;
                        }
                        acc
                    }
                };
                return int(acc, p);
            }
            // `types.arith.shift-count`.
            if c < 0 || c >= i128::from(width(p)) {
                return Err(failed(&format!(
                    "invalid shift: the count {c} is outside 0 to {} for `{}`",
                    width(p) - 1,
                    p.name()
                )));
            }
            if !cp.is_unsigned() {
                return Err(mismatch(Prim::U32, *cp));
            }
            let c = u32::try_from(c).unwrap_or(0);
            return Ok(match op {
                Some(TokenKind::Shl) => V::Int(wrap(x << c, p), p),
                _ => V::Int(wrap(x, p) >> c, p),
            });
        }
        let fallback = if want == Some(Prim::String) {
            Prim::String
        } else {
            Prim::I32
        };
        let p = self.common(src, (*l, *r), want, fallback);
        let (a, b) = (self.eval(src, *l, Some(p))?, self.eval(src, *r, Some(p))?);
        match (a, b) {
            (V::Str(x), V::Str(y)) if op == Some(TokenKind::Plus) => Ok(V::Str(x + &y)),
            (V::Int(x, p), V::Int(y, q)) => {
                if p != q {
                    return Err(mismatch(p, q));
                }
                match op {
                    Some(TokenKind::Plus) => int(x + y, p),
                    Some(TokenKind::Minus) => int(x - y, p),
                    Some(TokenKind::Star) => int(x * y, p),
                    Some(TokenKind::Slash | TokenKind::Percent) if y == 0 => {
                        Err(failed("division by zero"))
                    }
                    // `expr.arith.int.divide`: truncation toward zero.
                    Some(TokenKind::Slash) => int(x / y, p),
                    Some(TokenKind::Percent) => int(x % y, p),
                    Some(TokenKind::Amp) => Ok(V::Int(x & y, p)),
                    Some(TokenKind::Pipe) => Ok(V::Int(x | y, p)),
                    Some(TokenKind::Caret) => Ok(V::Int(x ^ y, p)),
                    _ => Err(nonconstant(
                        "this operation is not available to const evaluation",
                    )),
                }
            }
            (a, b) => Err(mismatch(a.prim(), b.prim())),
        }
    }
}

/// The operator token after a binary node's left operand.
fn op_after(src: &Src<'_>, l: NodeRef<'_>) -> Option<TokenKind> {
    src.tkind(hd_base::TokenIdx::from_raw(src.last(l).raw() + 1))
}

/// A literal typed by `want`: an unsuffixed integer takes it, or `i32`.
/// With `negated`, the literal is the operand of a unary `-` and is
/// negated before its range check, so `-128i8` fits.
fn literal(src: &Src<'_>, n: NodeRef<'_>, want: Option<Prim>, negated: bool) -> Result<V, Fail> {
    let t = src.first(n);
    match src.tkind(t) {
        Some(TokenKind::KwTrue) => Ok(V::Bool(true)),
        Some(TokenKind::KwFalse) => Ok(V::Bool(false)),
        Some(TokenKind::Char) => {
            let raw = src.text(t);
            let inner = raw.strip_prefix('\'').unwrap_or(raw);
            let inner = inner.strip_suffix('\'').unwrap_or(inner);
            let mut pieces = Vec::new();
            decode_piece(inner, &mut pieces);
            match pieces.as_slice() {
                [Piece::Text(s)] => s.chars().next().map(V::Char).ok_or(Fail::Poison),
                _ => Err(Fail::Poison),
            }
        }
        Some(TokenKind::Number) => {
            let (digits, suffix) = split_suffix(src.text(t));
            let p = if suffix.is_empty() {
                match want {
                    Some(p) if !matches!(p, Prim::Bool | Prim::Char | Prim::String) => p,
                    _ => Prim::I32,
                }
            } else {
                match Prim::ALL.iter().copied().find(|p| p.name() == suffix) {
                    Some(p) => p,
                    None => {
                        return Err(nonconstant(&format!(
                            "the suffix `{suffix}` calls a function, not a const operation"
                        )));
                    }
                }
            };
            if p.is_float() {
                return Err(mismatch(want.unwrap_or(Prim::I32), p));
            }
            let digits = digits.replace('_', "");
            let v = match radix_digits(&digits) {
                Some((radix, d)) => u128::from_str_radix(d, radix).ok(),
                None => digits.parse::<u128>().ok(),
            };
            match v.and_then(|v| i128::try_from(v).ok()) {
                Some(_) if negated && p.is_unsigned() => Err(mismatch(Prim::I64, p)),
                Some(v) => int(if negated { -v } else { v }, p),
                // A float literal, which no const type holds.
                None if digits.contains(['.', 'e', 'E']) => Err(mismatch(p, Prim::F64)),
                None => Err(failed("overflow: the literal is too large")),
            }
        }
        _ => Err(Fail::Poison),
    }
}

/// A plain string literal; interpolation and prefixes are not const
/// operations.
fn string(src: &Src<'_>, n: NodeRef<'_>) -> Result<V, Fail> {
    let toks: Vec<_> = src.tokens(n).collect();
    let unavailable =
        || nonconstant("string interpolation or a string prefix is not a const operation");
    let [t] = toks.as_slice() else {
        return Err(unavailable());
    };
    let text = src.text(*t);
    if src.tkind(*t) != Some(TokenKind::String) || !text.starts_with('"') {
        return Err(unavailable());
    }
    let mut pieces = Vec::new();
    decode_piece(strip_piece(text, TokenKind::String), &mut pieces);
    let mut out = String::new();
    for p in pieces {
        match p {
            Piece::Text(s) => out.push_str(&s),
            Piece::Name(_) => return Err(unavailable()),
        }
    }
    Ok(V::Str(out))
}
