//! Literal functions (spec/lang/05-expressions.md "Literal Suffixes" and
//! "Prefixed Strings"): a suffixed number `250ms` is the call `ms(250)` of
//! a function marked `@num_suffix`, and a prefixed string `sql"..."` is the
//! call `sql(t)` of a function marked `@str_prefix`, with a
//! `std.ops.Template` value `t`.

use hd_base::{DefId, StageResult};
use hd_diag::Code;
use hd_resolve::iface::{LITERAL_PREFIX, LITERAL_SUFFIX};
use hd_resolve::{BindingKind, ItemData};
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_tir::ir::{NONE, Ref, Tag, TirSink};
use hd_types::{Prim, Ty, TyData, VarKind};

use crate::body::{Ck, ExpectPosition, unsupported};
use crate::call::Args;
use crate::expr::{normalize_line_endings, strip_piece};

pub(crate) use hd_resolve::literal::{decimal_digits, radix_digits, split_suffix};

/// Built-in duration units (`types.literal.duration.fixed`).
fn duration_scale(name: &str) -> Option<u64> {
    Some(match name {
        "ms" => 1,
        "s" => 1_000,
        "min" => 60_000,
        "h" => 3_600_000,
        _ => return None,
    })
}

/// An exact, whole-millisecond replacement for a decimal fraction.
/// Integer arithmetic keeps the suggested spelling independent of float rounding.
fn duration_fraction(digits: &str, scale: u64) -> Option<u128> {
    let digits = digits.replace('_', "");
    let (whole, fraction) = digits.split_once('.')?;
    let denominator = 10u128.checked_pow(u32::try_from(fraction.len()).ok()?)?;
    let numerator = whole
        .parse::<u128>()
        .ok()?
        .checked_mul(denominator)?
        .checked_add(fraction.parse::<u128>().ok()?)?
        .checked_mul(u128::from(scale))?;
    (numerator % denominator == 0).then_some(numerator / denominator)
}

/// Primitive names are reserved for type suffixes, never user suffix functions.
pub(crate) fn primitive_suffix(name: &str) -> Option<Prim> {
    Prim::ALL.into_iter().find(|p| p.name() == name)
}

/// A piece of a prefixed string's raw text: text kept exactly as written
/// (`expr.prefix.raw-parts`), or a `$name` value.
enum Raw {
    Text(String),
    Name(String),
}

/// Splits raw text at its `$name` references. A backslash keeps the next
/// character as text, so `\$x` is not a reference, and `$5` is text.
fn split_raw(raw: &str, out: &mut Vec<Raw>) {
    let raw = &normalize_line_endings(raw);
    let mut text = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                text.push('\\');
                if let Some(n) = chars.next() {
                    text.push(n);
                }
            }
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
                out.push(Raw::Text(std::mem::take(&mut text)));
                out.push(Raw::Name(name));
            }
            _ => text.push(c),
        }
    }
    out.push(Raw::Text(text));
}

impl Ck<'_, '_> {
    fn poison(&mut self, n: NodeRef<'_>) -> (Ref, Ty) {
        (
            self.b.emit(Tag::Poison, NONE, NONE, Ty::POISON, n.index()),
            Ty::POISON,
        )
    }

    /// The call of the literal function `name` with the literal at `n` as
    /// its one argument (`expr.suffix.user-call`, `expr.prefix.fn-call`).
    pub(crate) fn literal_fn_call(
        &mut self,
        n: NodeRef<'_>,
        name: &str,
        kind: u8,
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let duration = (kind == LITERAL_SUFFIX)
            .then(|| duration_scale(name))
            .flatten();
        let def = if let Some(scale) = duration {
            let text = self.cx.src.text(self.cx.src.first(n));
            let digits = split_suffix(text).0;
            if !decimal_digits(digits) {
                let mut msg = format!("duration literal `{text}` needs a whole decimal integer");
                if let Some(millis) = duration_fraction(digits, scale)
                    && millis <= i64::MAX as u128
                {
                    msg = format!("{msg}; write `{millis}ms`");
                }
                self.err(Code::TypeMismatch, n, &msg);
                return Ok(self.poison(n));
            }
            let count = digits.replace('_', "").parse::<u64>().ok();
            if count
                .and_then(|v| v.checked_mul(scale))
                .is_none_or(|v| i64::try_from(v).is_err())
            {
                self.err(
                    Code::IntegerLiteralRange,
                    n,
                    "duration literal exceeds 9223372036854775807 milliseconds",
                );
                return Ok(self.poison(n));
            }
            self.cx.names.item("std.time", name)
        } else {
            let sym = self.cx.names.syms.intern(name);
            if self.is_poison_name(name) {
                return Ok(self.poison(n));
            }
            let Some(b) = self.cx.scope_at(self.at).lookup(sym) else {
                let msg = format!("`{name}` is not defined");
                self.err(Code::UnknownName, n, &msg);
                return Ok(self.poison(n));
            };
            let def = DefId::from_raw(b.value);
            let marked = b.kind == BindingKind::Item
                && self
                    .cx
                    .lookup
                    .item(def)
                    .is_some_and(|i| matches!(i.data, ItemData::Fn(_)) && i.literal_fn == kind);
            if !marked {
                let (code, name_of, what) = if kind == LITERAL_SUFFIX {
                    (
                        Code::InvalidLiteralSuffix,
                        "invalid-literal-suffix",
                        "suffix",
                    )
                } else {
                    (Code::InvalidStringPrefix, "invalid-string-prefix", "prefix")
                };
                let msg =
                    format!("{name_of}: `{name}` is not a function marked for a literal {what}");
                self.err(code, n, &msg);
                return Ok(self.poison(n));
            }
            def
        };
        let args = Args {
            positional: vec![n],
            ..Args::empty()
        };
        self.lit_arg = Some(n.index());
        let r = self.call_item(def, &[], &args, n, false, want);
        self.lit_arg = None;
        r
    }

    /// A number literal's type meeting a trait value type is its default.
    fn default_literal(&mut self, got: Ty, want: Ty) -> Ty {
        let pool = self.pool();
        let g = self.strip_mut(self.infer.resolve(pool, got));
        if matches!(pool.get(self.strip_mut(want)), TyData::TraitValue { .. })
            && matches!(pool.get(g), TyData::Infer(_))
            && matches!(
                self.infer.kind_of(pool, g),
                Some(VarKind::IntLit | VarKind::FloatLit)
            )
        {
            return self.zonk(g);
        }
        got
    }

    /// Interpolation owns a fresh object when its erased element is a reference.
    fn template_value(&mut self, r: Ref, ty: Ty, elem: Ty, n: NodeRef<'_>) -> Ref {
        let (r, ty) = if self.dyn_ref_of(elem).is_some() && self.ref_of(ty).is_none() {
            self.new_object_ref(r, ty, false, n)
        } else {
            (r, ty)
        };
        self.coerce(r, ty, elem, n, "argument", ExpectPosition::General)
    }

    /// The `Template[T]` argument of a prefix function: the raw text
    /// pieces and the interpolated values (`expr.prefix.template`).
    /// `want` is the parameter type, whose `T` checks each value.
    pub(crate) fn template_arg(
        &mut self,
        n: NodeRef<'_>,
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
        let template = self.cx.names.known.template;
        let want = want.map(|w| self.infer.resolve(pool, w));
        let Some(elem) = want.and_then(|w| match pool.get(self.strip_mut(w)) {
            TyData::Adt { def, args } if def == template => pool.list_items(args).first().copied(),
            _ => None,
        }) else {
            let found = want.map_or_else(|| "a value".into(), |w| self.show(w));
            let msg = format!("in argument: expected Template[T], found {found}");
            self.err(Code::TypeMismatch, n, &msg);
            return Ok(self.poison(n));
        };
        let mut pieces: Vec<Ref> = Vec::new();
        let mut values: Vec<Ref> = Vec::new();
        let mut cur = String::new();
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
            // A piece after an interpolation: its expression first.
            if matches!(k, TokenKind::StrMid | TokenKind::StrTail)
                && let Some(ip) = kids.next()
                && let Some(e) = ip.children().next()
            {
                pieces.push(self.b.const_str(&std::mem::take(&mut cur)));
                let (r, ty) = self.expr(e, Some(elem))?;
                let ty = self.default_literal(ty, elem);
                values.push(self.template_value(r, ty, elem, e));
            }
            let raw = strip_piece(self.cx.src.text(t), k).to_owned();
            let mut parts = Vec::new();
            split_raw(&raw, &mut parts);
            for p in parts {
                match p {
                    Raw::Text(s) => cur.push_str(&s),
                    Raw::Name(name) => {
                        let sym = self.cx.names.syms.intern(&name);
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
                        pieces.push(self.b.const_str(&std::mem::take(&mut cur)));
                        let ty = self.default_literal(ty, elem);
                        values.push(self.template_value(r, ty, elem, n));
                    }
                }
            }
        }
        pieces.push(self.b.const_str(&cur));
        let list = self.cx.names.known.list;
        let list_of = |me: &mut Self, items: &[Ref], el: Ty| {
            let lt = pool.intern_ty(&TyData::Adt {
                def: list,
                args: pool.list(&[el]),
            });
            let lt = pool.intern_ty(&TyData::Mut(lt));
            let rec = me.b.refs_record(items);
            me.b.emit(Tag::NewList, NONE, rec, lt, n.index())
        };
        let raw_parts = list_of(self, &pieces, Ty::STRING);
        let vals = list_of(self, &values, elem);
        let tt = pool.intern_ty(&TyData::Adt {
            def: template,
            args: pool.list(&[elem]),
        });
        let rec = self.b.refs_record(&[raw_parts, vals]);
        Ok((self.b.emit(Tag::NewStruct, NONE, rec, tt, n.index()), tt))
    }

    /// A prefixed string `x"..."`: a call of the prefix function `x`.
    pub(crate) fn prefixed_string(
        &mut self,
        n: NodeRef<'_>,
        prefix: &str,
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        if self.lit_arg == Some(n.index()) {
            self.lit_arg = None;
            return self.template_arg(n, want);
        }
        if prefix.is_empty() {
            return unsupported("a prefixed string");
        }
        self.literal_fn_call(n, prefix, LITERAL_PREFIX, want)
    }
}

impl Ck<'_, '_> {
    /// The check of `@num_suffix` and `@str_prefix` before the function
    /// `def`, reported at the decorator: the marker's signature is
    /// `fn(N) -> R` with `N < Num`, or `fn(Template[T]) -> R`, and a
    /// literal function never suspends (`annot.typed-fact.check`).
    pub(crate) fn check_literal_marker(&mut self, def: DefId, node: NodeRef<'_>) {
        let pool = self.pool();
        let Some(item) = self.cx.lookup.item(def) else {
            return;
        };
        let kind = item.literal_fn;
        let Some(sig) = item.sig().cloned() else {
            return;
        };
        let wanted = if kind == LITERAL_SUFFIX {
            "num_suffix"
        } else if kind == LITERAL_PREFIX {
            "str_prefix"
        } else {
            return;
        };
        let Some(dec) = node.children().find(|c| {
            c.kind() == SyntaxKind::Decorator
                && crate::facts::literal_marker(self.cx, *c) == Some(kind)
        }) else {
            return;
        };
        let mismatch = |me: &mut Self, why: &str| {
            let msg = format!("`@{wanted}` {why}");
            me.err(Code::TypeMismatch, dec, &msg);
        };
        if sig.suspends {
            return mismatch(self, "does not fit a suspending function");
        }
        let [(_, ty)] = sig.params.as_slice() else {
            return mismatch(self, "needs a function of exactly one parameter");
        };
        let ty = self.infer.resolve(pool, *ty);
        if kind == LITERAL_SUFFIX {
            if !self.numeric(ty) {
                let msg = format!(
                    "`@num_suffix` needs a `Num` parameter, found {}",
                    self.show(ty)
                );
                self.err(Code::UnsatisfiedTraitBound, dec, &msg);
            }
            return;
        }
        let template = self.cx.names.known.template;
        if !matches!(pool.get(ty), TyData::Adt { def: d, .. } if d == template) {
            mismatch(self, "needs a `Template` parameter");
        }
    }
}
