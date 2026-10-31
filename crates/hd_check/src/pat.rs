//! Patterns, `let`, `match` and exhaustiveness (`crates/hd_check/DESIGN.md` (type checking) §7;
//! `crates/hd_tir/DESIGN.md` "Matching"). The checker declares each arm's
//! bindings, emits the arm blocks, then a decision block of `SwitchTag`,
//! `If`, payload reads (`Read [%v as .V .k]`), `Guard` and `ToArm`: a decision tree over
//! the pattern matrix, where each test runs once on a path and each
//! outcome keeps the rows, in order, that may still match. Exhaustiveness
//! is Maranget's usefulness test over the arm patterns.

use hd_base::HashMap;
use hd_base::SliceSort as _;

use hd_base::{LocalId, NodeIdx, StageResult};
use hd_diag::Code;
use hd_resolve::{ItemData, Src};
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_tir::ir::{IntrinsicOp, NONE, PrimOp, Ref, Tag, TirSink};
use hd_types::{Prim, Ty, TyData, VarKind};

use crate::body::{Ck, ExpectPosition, unsupported};
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

/// A cell of a decision matrix: no test left, a pattern, or `.Some`
/// around a row's pattern (a [`Row::some`] row).
#[derive(Clone, Copy)]
enum Cell<'t> {
    Wild,
    Pat(NodeRef<'t>),
    Some(Option<NodeRef<'t>>),
}

/// A row of a decision matrix: a cell per column, the stores its
/// bindings need at its leaf, and its index in the [`Row`]s.
#[derive(Clone)]
struct DRow<'t> {
    cells: Vec<Cell<'t>>,
    sets: Vec<(LocalId, Ref, NodeIdx)>,
    row: usize,
}

impl<'t> DRow<'t> {
    /// The row with column `j` replaced by `cells`.
    fn with_column(mut self, j: usize, cells: impl IntoIterator<Item = Cell<'t>>) -> Self {
        self.cells.splice(j..=j, cells);
        self
    }
}

/// What a literal test passing or failing tells of another literal or
/// range pattern on the same value: passing implies it matches
/// (`implied`) or that it does not (`excluded`); failing implies it does
/// not (`covered`).
#[derive(Default)]
struct Rel {
    implied: bool,
    excluded: bool,
    covered: bool,
}

/// A row with no test left and no guard: it matches every value, so the
/// rows after it are never tried.
fn done(rows: &[Row<'_>], r: &DRow<'_>) -> bool {
    rows[r.row].guard.is_none() && r.cells.iter().all(|c| matches!(c, Cell::Wild))
}

/// Records the store into the local that the pattern node `at` binds.
fn bind(row: &Row<'_>, sets: &mut Vec<(LocalId, Ref, NodeIdx)>, at: NodeIdx, v: Ref) {
    if let Some(&(_, l)) = row.binds.iter().find(|(n, _)| *n == at) {
        sets.push((l, v, at));
    }
}

/// Whether a pattern tests its value, so a reference is read through to
/// test the value it refers to (`ref.match.read`): every pattern but a
/// name, `_`, `()` and a spread.
fn tests_value(p: NodeRef<'_>) -> bool {
    match p.kind() {
        SyntaxKind::WildcardPattern | SyntaxKind::BindingPattern | SyntaxKind::SpreadPattern => {
            false
        }
        SyntaxKind::TuplePattern => p.children().next().is_some(),
        _ => true,
    }
}

/// Whether a sub-pattern tests or binds its value: all but `_` and `()`.
fn reads(p: NodeRef<'_>) -> bool {
    match p.kind() {
        SyntaxKind::WildcardPattern => false,
        SyntaxKind::TuplePattern => p.children().next().is_some(),
        _ => true,
    }
}

/// The columns `occs` with column `j` replaced by `new`.
fn splice(
    occs: &[(Ref, Ty)],
    j: usize,
    new: impl IntoIterator<Item = (Ref, Ty)>,
) -> Vec<(Ref, Ty)> {
    let mut o = occs.to_vec();
    o.splice(j..=j, new);
    o
}

/// A pattern for the exhaustiveness test.
#[derive(Clone, Debug)]
enum P {
    Wild,
    Ctor(String, Vec<P>),
    /// An integer literal or range pattern: the values `lo..=hi`, none
    /// when `lo > hi` (`flow.match.cover.integer`).
    Int(i128, i128),
    /// A string literal's cooked value: equal strings match the same
    /// values (`flow.match.unreachable`).
    Str(String),
    /// A float literal's bits: equal bits match the same values.
    Flt(u64),
    Lit,
}

/// A numeric bound of a range pattern.
#[derive(Clone, Copy)]
struct Bound {
    neg: bool,
    tok: hd_base::TokenIdx,
}

/// The bounds of a range pattern (`flow.match.range.*`).
struct RangeBounds {
    lo: Option<Bound>,
    hi: Option<Bound>,
    inclusive: bool,
}

/// A variant pattern's sub-patterns by field position (`None` for an
/// omitted named field), with the named fields the variant does not
/// declare, and the named fields that repeat a field already matched:
/// their names and sub-patterns, and whether the field is a repeat, for the
/// caller to report and bind to the poison type.
type VariantSubs<'t> = (Vec<Option<NodeRef<'t>>>, Vec<(String, NodeRef<'t>, bool)>);

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
        // `ref.match.read`: a pattern that tests its value reads through a
        // reference; a name or `_` takes the reference itself.
        if tests_value(p)
            && let Some((referent, mutable)) = self.ref_of(t)
        {
            return self.declare_pattern(p, self.deref_ty(referent, mutable), binds);
        }
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
                let name = self.cx.src.symbol(nt, self.cx.names.syms);
                // `flow.match.bare-variant`: a bare name of a variant of
                // the subject's enum is not a catch-all binding.
                if self
                    .variant_fields(self.strip(t), self.cx.src.text(nt))
                    .is_some()
                {
                    let msg = format!("write `.{}` or a qualified name", self.cx.src.text(nt));
                    self.err(Code::BareVariantPattern, p, &msg);
                    self.pattern_rejected = true;
                    return Ok(());
                }
                return self.bind_pattern_name(p, name, t, binds);
            }
            // A spread pattern is checked by its tuple pattern; here it
            // stands where a tuple pattern has no rest element.
            SyntaxKind::SpreadPattern => {
                if inner != Ty::POISON {
                    let msg = format!(
                        "a spread pattern needs a tuple with a rest element, not {}",
                        self.show(t)
                    );
                    self.err(Code::TypeMismatch, p, &msg);
                }
                if let Some(nt) = p.direct_tokens().find(|t| {
                    matches!(
                        self.cx.src.tkind(*t),
                        Some(TokenKind::Ident | TokenKind::RawIdent)
                    )
                }) {
                    let name = self.cx.src.symbol(nt, self.cx.names.syms);
                    self.bind_pattern_name(p, name, Ty::POISON, binds)?;
                }
            }
            SyntaxKind::LiteralPattern => {
                let (r, lt) = self.pattern_literal(p, Some(t))?;
                self.expect(lt, t, p, "pattern", ExpectPosition::General);
                // `types.literal.int-range`.
                if self.b.const_of(r).is_some() && p.children().next().is_none() {
                    let neg = self.cx.src.tkind(self.cx.src.first(p)) == Some(TokenKind::Minus);
                    self.lit_nodes.push((r, p.index(), neg));
                }
            }
            SyntaxKind::RangePattern => {
                let bounds = self.range_bounds(p);
                for b in [bounds.lo, bounds.hi].into_iter().flatten() {
                    let before = self.diags.len();
                    let (r, lt) = self.bound_literal(b, t)?;
                    self.expect(lt, t, p, "pattern", ExpectPosition::General);
                    if self.diags.len() != before {
                        // One `type-mismatch` per pattern.
                        break;
                    }
                    // `flow.match.range.bound-type`.
                    if self.b.const_of(r).is_some() {
                        self.lit_nodes.push((r, p.index(), b.neg));
                    }
                }
            }
            SyntaxKind::TuplePattern if p.children().next().is_none() => {
                self.expect(t, Ty::VOID, p, "pattern", ExpectPosition::General);
            }
            SyntaxKind::TuplePattern => {
                let subs: Vec<NodeRef<'_>> = p.children().collect();
                // A spread pattern ends the pattern (grammar.pattern.tuple-spread).
                let spread = subs
                    .last()
                    .copied()
                    .filter(|s| s.kind() == SyntaxKind::SpreadPattern);
                let fixed = &subs[..subs.len() - usize::from(spread.is_some())];
                let (elems, rest) = match pool.get(inner) {
                    TyData::Tuple { elems, rest } => (pool.list_items(elems).to_vec(), rest),
                    TyData::Poison => return self.declare_poisoned(&subs, binds),
                    TyData::Infer(_) if spread.is_some() => {
                        return unsupported("a spread pattern on a value whose type is not known");
                    }
                    TyData::Infer(_) => {
                        let vs: Vec<Ty> = subs
                            .iter()
                            .map(|_| self.infer.fresh(pool, VarKind::General))
                            .collect();
                        let tt = pool.intern_ty(&TyData::Tuple {
                            elems: pool.list(&vs),
                            rest: None,
                        });
                        self.expect(tt, t, p, "pattern", ExpectPosition::General);
                        (vs, None)
                    }
                    _ => {
                        let msg = format!("in pattern: {} is not a tuple", self.show(t));
                        self.err(Code::TypeMismatch, p, &msg);
                        return self.declare_poisoned(&subs, binds);
                    }
                };
                match (rest, spread) {
                    (Some(rest_ty), Some(sp)) => {
                        // `flow.match.spread.arity`.
                        if fixed.len() != elems.len() {
                            let msg = format!(
                                "the tuple takes {} patterns before its spread, found {}",
                                elems.len(),
                                fixed.len()
                            );
                            self.err(Code::TypeMismatch, p, &msg);
                            return self.declare_poisoned(&subs, binds);
                        }
                        for (s, et) in fixed.iter().zip(elems) {
                            self.declare_pattern(*s, et, binds)?;
                        }
                        // `flow.match.spread.bind`: `_...` binds nothing.
                        if let Some(nt) = sp.direct_tokens().find(|t| {
                            matches!(
                                self.cx.src.tkind(*t),
                                Some(TokenKind::Ident | TokenKind::RawIdent)
                            )
                        }) {
                            let name = self.cx.src.symbol(nt, self.cx.names.syms);
                            self.bind_pattern_name(sp, name, rest_ty, binds)?;
                        }
                    }
                    // `flow.match.spread.required`.
                    (Some(_), None) => {
                        let msg = format!(
                            "{} has a rest element, so its pattern ends in a spread pattern",
                            self.show(t)
                        );
                        self.err(Code::TypeMismatch, p, &msg);
                        return self.declare_poisoned(&subs, binds);
                    }
                    // `flow.match.spread.fixed-tuple`.
                    (None, Some(_)) => {
                        let msg = format!("{} has no rest element to spread", self.show(t));
                        self.err(Code::TypeMismatch, p, &msg);
                        return self.declare_poisoned(&subs, binds);
                    }
                    (None, None) => {
                        if elems.len() != subs.len() {
                            // A `let` tuple pattern needs a tuple of the
                            // same arity (`flow.let.tuple-arity`).
                            if self.let_view.is_some() {
                                let msg = format!(
                                    "the pattern binds {} names but the tuple has {} elements",
                                    subs.len(),
                                    elems.len()
                                );
                                self.err(Code::TypeMismatch, p, &msg);
                            } else {
                                self.err(Code::PatternArity, p, "the tuple has another size");
                            }
                            return self.declare_poisoned(&subs, binds);
                        }
                        for (s, et) in subs.iter().zip(elems) {
                            self.declare_pattern(*s, et, binds)?;
                        }
                    }
                }
            }
            // Against a poisoned type the names still bind, to the poison
            // type, so their uses report nothing further.
            SyntaxKind::VariantPattern | SyntaxKind::DataPattern if inner == Ty::POISON => {
                return self.declare_poisoned_fields(p, binds);
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
                    self.pattern_rejected = true;
                    return self.declare_poisoned_fields(p, binds);
                }
                let Some((_, fields)) = self.variant_fields(t, &name) else {
                    if matches!(pool.get(inner), TyData::Infer(_)) {
                        let msg = format!(
                            "the type of the matched value is not known here; annotate it so `.{name}` can be checked"
                        );
                        self.err(Code::CannotInferType, p, &msg);
                        // The value's type is now poisoned: later arms and
                        // the exhaustiveness check stay silent.
                        self.infer.rebind(pool, inner, Ty::POISON);
                        return self.declare_poisoned_fields(p, binds);
                    }
                    // `flow.match.contextual`: a non-enum subject has no
                    // variants to name.
                    let enum_like = matches!(
                        pool.get(self.infer.resolve(pool, inner)),
                        TyData::Option(_) | TyData::Adt { .. }
                    );
                    if !enum_like {
                        let msg = format!("`.{name}` needs an enum subject, not {}", self.show(t));
                        self.err(Code::MissingContextualEnumType, p, &msg);
                        return self.declare_poisoned_fields(p, binds);
                    }
                    let msg = format!("no variant `.{name}` of {}", self.show(t));
                    self.err(Code::UnknownVariant, p, &msg);
                    return Ok(());
                };
                let (subs, unknown) = self.variant_subpatterns(p, t, &name)?;
                for (fname, sub, repeated) in unknown {
                    if repeated {
                        let msg = format!("`{fname}` appears twice in this pattern");
                        self.err(Code::DuplicateDataPatternField, sub, &msg);
                        self.pattern_rejected = true;
                    } else {
                        let msg = format!("no field `{fname}` of variant `{name}`");
                        self.err(Code::UnknownField, sub, &msg);
                    }
                    self.declare_pattern(sub, Ty::POISON, binds)?;
                }
                if subs.len() != fields.len() {
                    self.err(
                        Code::PatternArity,
                        p,
                        "the variant has another number of fields",
                    );
                    return Ok(());
                }
                // Preserve resolved field positions and source binding forms for tooling.
                if self.cx.source_facts.is_some() {
                    let fields = self.variant_field_names(t, &name);
                    let bindings = subs
                        .iter()
                        .enumerate()
                        .filter_map(|(position, sub)| {
                            let sub = (*sub)?;
                            if sub.kind() != SyntaxKind::BindingPattern {
                                return None;
                            }
                            let token = sub.direct_tokens().find(|token| {
                                matches!(
                                    self.cx.src.tkind(*token),
                                    Some(TokenKind::Ident | TokenKind::RawIdent)
                                )
                            })?;
                            Some(crate::comparison::PayloadBinding {
                                span: self.cx.src.span(sub),
                                position,
                                name: self.cx.src.text(token).to_owned(),
                                bare: sub.children().next().is_none(),
                            })
                        })
                        .collect();
                    self.variant_patterns
                        .push(crate::comparison::VariantPattern {
                            span: self.cx.src.span(p),
                            variant: name.clone(),
                            fields,
                            bindings,
                        });
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
                    let fname = self.data_field_name(f);
                    if seen.contains(&fname) {
                        let msg = format!("`{fname}` appears twice in this pattern");
                        self.err(Code::DuplicateDataPatternField, f, &msg);
                        self.pattern_rejected = true;
                        if let Some(sub) = f.children().next() {
                            self.declare_pattern(sub, Ty::POISON, binds)?;
                        } else {
                            let name = self.cx.names.syms.intern(&fname);
                            self.bind_pattern_name(f, name, Ty::POISON, binds)?;
                        }
                        continue;
                    }
                    seen.push(fname.clone());
                    let Some((_, ft)) = self.field_of(t, &fname) else {
                        let msg = format!("no field `{fname}` on {}", self.show(t));
                        self.err(Code::UnknownField, f, &msg);
                        continue;
                    };
                    self.check_field_visible(t, &fname, f);
                    // A subject that is not writable binds a field declared
                    // `mut Suspend[T]` as `Suspend[T]`.
                    let ft = self.field_access(t, &fname, ft);
                    if let Some(sub) = f.children().next() {
                        self.declare_pattern(sub, ft, binds)?;
                    } else {
                        let name = self.cx.names.syms.intern(&fname);
                        self.bind_pattern_name(f, name, ft, binds)?;
                    }
                }
            }
            other => return unsupported(format!("the pattern {other:?}")),
        }
        Ok(())
    }

    /// The names under a variant or data pattern whose type is poisoned
    /// bind to the poison type.
    fn declare_poisoned_fields(
        &mut self,
        p: NodeRef<'_>,
        binds: &mut Vec<(NodeIdx, LocalId)>,
    ) -> StageResult<()> {
        let fields: Vec<NodeRef<'_>> = match p.kind() {
            SyntaxKind::DataPattern => p
                .children()
                .filter(|c| c.kind() == SyntaxKind::DataPatternField)
                .collect(),
            _ => Src::child(p, SyntaxKind::PatternArgumentList)
                .map(|al| al.children().collect())
                .unwrap_or_default(),
        };
        for f in fields {
            match f.kind() {
                SyntaxKind::DataPatternField if f.children().next().is_none() => {
                    let name = self.cx.names.syms.intern(&self.data_field_name(f));
                    let l = self.bind_local(name, Ty::POISON, f);
                    binds.push((f.index(), l));
                }
                SyntaxKind::DataPatternField | SyntaxKind::NamedPattern => {
                    if let Some(sub) = f.children().next() {
                        self.declare_pattern(sub, Ty::POISON, binds)?;
                    }
                }
                _ => self.declare_pattern(f, Ty::POISON, binds)?,
            }
        }
        Ok(())
    }

    /// After an error in a tuple pattern its names still bind, to the
    /// poison type, so their uses report nothing further.
    fn declare_poisoned(
        &mut self,
        subs: &[NodeRef<'_>],
        binds: &mut Vec<(NodeIdx, LocalId)>,
    ) -> StageResult<()> {
        for s in subs {
            self.declare_pattern(*s, Ty::POISON, binds)?;
        }
        Ok(())
    }

    /// Binds a name of a binding or spread pattern to a local of type `t`
    /// (`names.bind.pattern`; a `let` pattern's names follow the binding
    /// forms, types.bind.let-pattern-mut).
    fn bind_pattern_name(
        &mut self,
        p: NodeRef<'_>,
        name: hd_base::Symbol,
        t: Ty,
        binds: &mut Vec<(NodeIdx, LocalId)>,
    ) -> StageResult<()> {
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
        if p.kind() == SyntaxKind::BindingPattern && p.children().next().is_some() {
            // `name @ pattern`-like forms.
            return unsupported("a binding with a sub-pattern");
        }
        let t = match self.let_view {
            Some(annotated)
                if p.direct_tokens()
                    .any(|t| self.cx.src.tkind(t) == Some(TokenKind::KwMut)) =>
            {
                let _ = annotated;
                if self.is_coll_ty(t) {
                    self.readonly_view(t)
                } else {
                    t
                }
            }
            Some(false) => self.readonly_view(t),
            _ => t,
        };
        let l = self.bind_local(name, t, p);
        if self.let_view.is_some()
            && p.direct_tokens()
                .any(|t| self.cx.src.tkind(t) == Some(TokenKind::KwMut))
        {
            self.mut_locals.push(l);
        }
        binds.push((p.index(), l));
        Ok(())
    }

    /// The field a data pattern field names: its first name token, past
    /// the `mut` of `Point { mut tags }` (`grammar.stmt.let-pattern.mut`).
    fn data_field_name(&self, f: NodeRef<'_>) -> String {
        f.direct_tokens()
            .find(|t| {
                matches!(
                    self.cx.src.tkind(*t),
                    Some(TokenKind::Ident | TokenKind::RawIdent)
                )
            })
            .map(|t| self.cx.src.text(t).to_owned())
            .unwrap_or_default()
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
    /// for an omitted named field), with the named fields the variant
    /// does not declare (see [`VariantSubs`]).
    fn variant_subpatterns<'t>(
        &self,
        p: NodeRef<'t>,
        t: Ty,
        name: &str,
    ) -> StageResult<VariantSubs<'t>> {
        let Some(al) = Src::child(p, SyntaxKind::PatternArgumentList) else {
            return Ok((vec![], vec![]));
        };
        let kids: Vec<NodeRef<'t>> = al.children().collect();
        if kids.iter().any(|k| k.kind() == SyntaxKind::SpreadPattern) {
            return unsupported("a variant pattern with a rest");
        }
        if kids.iter().all(|k| k.kind() != SyntaxKind::NamedPattern) {
            return Ok((kids.into_iter().map(Some).collect(), vec![]));
        }
        // Positional sub-patterns fill fields by position, named ones by
        // the variant's field names (data.enum.payload.order).
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
            return Ok((vec![], vec![]));
        };
        let positional = kids
            .iter()
            .take_while(|k| k.kind() != SyntaxKind::NamedPattern)
            .count();
        // Extra positional sub-patterns stay in the list, so the caller
        // reports the arity.
        let mut out = vec![None; v.fields.len().max(positional)];
        let mut unknown: Vec<(String, NodeRef<'t>, bool)> = Vec::new();
        let mut filled = vec![false; out.len()];
        for (i, k) in kids.into_iter().enumerate() {
            let (slot, sub) = if i < positional {
                (Some(i), Some(k))
            } else if k.kind() == SyntaxKind::NamedPattern {
                let fname = self.cx.src.text(self.cx.src.first(k));
                let slot = v
                    .fields
                    .iter()
                    .position(|f| f.name == self.cx.names.syms.intern(fname));
                // A named field the variant does not declare is reported
                // by the caller; its sub-pattern binds to the poison type
                // so its uses stay silent.
                if slot.is_none() {
                    unknown.push((fname.to_owned(), k.children().next().unwrap_or(k), false));
                }
                (slot, k.children().next().or(Some(k)))
            } else {
                // A positional pattern after a named one: `pattern-order`
                // is already reported by the parser.
                (None, None)
            };
            let Some(slot) = slot else { continue };
            if std::mem::replace(&mut filled[slot], true) {
                // `flow.match.data.fields.duplicate`: the repeat reports
                // and binds to the poison type.
                if let Some(sub) = sub {
                    let fname = self.cx.src.text(self.cx.src.first(k));
                    unknown.push((fname.to_owned(), sub, true));
                }
                continue;
            }
            out[slot] = sub;
        }
        Ok((out, unknown))
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
                Ok(self.pattern_value(r, t, want, neg))
            }
            _ => self.expr(p, want),
        }
    }

    /// A literal pattern's constant `r` of type `t`: it takes the
    /// scrutinee's type `want`, and `neg` negates it.
    fn pattern_value(&mut self, r: Ref, t: Ty, want: Option<Ty>, neg: bool) -> (Ref, Ty) {
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
            let v = self.negated_bits(ct, bits);
            return (self.b.const_value(ct, v), t);
        }
        (r, t)
    }

    /// The bounds of a range pattern, read from its tokens: `[-]a`, the
    /// `..` or `..=`, then `[-]b`; either bound may be missing.
    fn range_bounds(&self, p: NodeRef<'_>) -> RangeBounds {
        let mut out = RangeBounds {
            lo: None,
            hi: None,
            inclusive: false,
        };
        let mut neg = false;
        let mut after = false;
        for t in p.direct_tokens() {
            match self.cx.src.tkind(t) {
                Some(TokenKind::Minus) => neg = true,
                Some(k @ (TokenKind::DotDot | TokenKind::DotDotEq)) => {
                    after = true;
                    out.inclusive = k == TokenKind::DotDotEq;
                }
                Some(TokenKind::Number) => {
                    let b = Some(Bound { neg, tok: t });
                    if after {
                        out.hi = b;
                    } else {
                        out.lo = b;
                    }
                    neg = false;
                }
                _ => {}
            }
        }
        out
    }

    /// A range bound's constant, checked against the subject type `t`.
    fn bound_literal(&mut self, b: Bound, t: Ty) -> StageResult<(Ref, Ty)> {
        let (r, lt) = self.literal_at(b.tok)?;
        Ok(self.pattern_value(r, lt, Some(t), b.neg))
    }

    /// A pattern bound's integer value.
    fn bound_value(&self, b: Bound) -> Option<i128> {
        let text = self.cx.src.text(b.tok);
        let text = crate::literals::split_suffix(text).0.replace('_', "");
        let v = if let Some((radix, digits)) = crate::literals::radix_digits(&text) {
            i128::from_str_radix(digits, radix).ok()?
        } else {
            text.parse::<i128>().ok()?
        };
        Some(if b.neg { -v } else { v })
    }

    // ------------------------------------------------------------ let

    pub(crate) fn let_stmt(&mut self, s: NodeRef<'_>, kids: &[NodeRef<'_>]) -> StageResult<()> {
        let kids: Vec<NodeRef<'_>> = kids.to_vec();
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
        // A `using` binding keeps its initializer's access (flow.using.access.v2).
        let using = std::mem::take(&mut self.using_bind);
        let annot = match ty_node {
            Some(t) => Some(self.ty_node(t)?),
            None => None,
        };
        let name = (pat.kind() == SyntaxKind::BindingPattern && els.is_none())
            .then(|| {
                pat.direct_tokens().find(|t| {
                    matches!(
                        self.cx.src.tkind(*t),
                        Some(TokenKind::Ident | TokenKind::RawIdent)
                    )
                })
            })
            .flatten()
            .map(|nt| self.cx.src.symbol(nt, self.cx.names.syms));
        let initializer_diags = self.diags.mark();
        // `names.scope.recursive-closure`: a closure initializer's body may
        // call the name being bound, which is a local before the body.
        let (r, t, own) = match name {
            // `fn.recursion.closure.binding`: only a statement-form binding
            // grants the self-name; a chain member does not.
            Some(name)
                if rhs.kind() == SyntaxKind::ClosureExpr
                    && (at.kind() == SyntaxKind::LetStmt
                        || self.stmt_binding == Some(at.index())) =>
            {
                self.check_local_name(name, pat);
                self.check_prelude_shadow(name, pat);
                let mut parts = self.closure_parts(rhs)?;
                parts.name = Some(name);
                parts.reassignable = at.kind() != SyntaxKind::BindingExpr;
                let (r, t, own) = self.closure_with(rhs, &parts, annot)?;
                // `fn.closure.recursive-result`
                if let Some(l) = own
                    && parts.ret.is_none()
                    && self.b.body_mut().local_flags[l.idx()] & hd_tir::ir::local_flags::CAPTURED
                        != 0
                {
                    self.err(
                        Code::RecursiveFunctionNeedsResultType,
                        rhs,
                        "a recursive closure must write its result type",
                    );
                }
                (r, t, own)
            }
            _ => {
                let (r, t) = self.expr(rhs, annot)?;
                (r, t, None)
            }
        };
        let initializer_failed = self.diags.errors_since(initializer_diags) != 0;
        if !using && !initializer_failed && self.implements_auto_closing(t)? {
            self.err(
                Code::UnregisteredAutoClosing,
                rhs,
                "an AutoClosing value cannot be bound with `:=` or `let`; call its `@auto_closing` constructor as a statement",
            );
        }
        let (r, t) = match annot {
            Some(a) => (
                self.coerce(r, t, a, rhs, "binding", ExpectPosition::General),
                a,
            ),
            None => (r, t),
        };
        // `let mut` on a primitive or a tuple (types.bind.let-mut-primitive-local,
        // types.bind.let-mut-tuple-valid).
        let mut t = t;
        let let_mut = pat.kind() == SyntaxKind::BindingPattern
            && pat
                .direct_tokens()
                .any(|t| self.cx.src.tkind(t) == Some(TokenKind::KwMut));
        let mut deferred = None;
        if let_mut {
            deferred = self.let_mut_binding(t, annot.is_some(), true, pat);
            // A collection in a `let mut` is a value that changes in place
            // (`types.coll.writable`): its type is the value type.
            if annot.is_none() && self.is_coll_ty(t) {
                t = self.readonly_view(t);
            }
        } else if pat.kind() == SyntaxKind::BindingPattern && annot.is_none() && !using {
            // `x := e` and `let x = e` drop an outer `mut` from the type
            // (types.mut.weaken).
            t = self.readonly_view(t);
        }
        // A plain name: a new local.
        if pat.kind() == SyntaxKind::BindingPattern && els.is_none() {
            let Some(name) = name else {
                return unsupported("a binding pattern without a name");
            };
            // `types.bind.never`: a binding without an annotation whose
            // initializer has type `never` could never be read. A failed
            // initializer may recover as `never`; it needs no follow-on.
            if annot.is_none()
                && !initializer_failed
                && self.infer.resolve(self.pool(), t) == Ty::NEVER
            {
                let msg = format!(
                    "`{}` has no value to bind: the initializer has type `never`, which has no value",
                    self.cx.names.text(name)
                );
                self.err(Code::UninhabitedBinding, pat, &msg);
            }
            let l = match own {
                Some(l) => {
                    self.b.body_mut().local_ty[l.idx()] = t;
                    self.user_locals.push(l);
                    l
                }
                None => self.bind_local(name, t, pat),
            };
            if let_mut {
                self.mut_locals.push(l);
            }
            if let_mut && self.cx.source_facts.is_some() {
                self.mut_bindings.push((l, pat.index(), deferred));
            }
            self.note_ref_origin(l, at, rhs);
            if annot.is_none() {
                self.local_inits.insert(l, rhs.index());
            }
            self.set_local(l, r, at.index());
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
        self.decide(&rows, r, t)?;
        let dec = self.b.close_block(db, None, Ty::NEVER, at.index());
        let mut rec = vec![dec];
        rec.extend(arms);
        let rec = self.b.refs_record(&rec);
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
        // A scrutinee that failed to check has reported its error (`sr` is
        // the empty ref) or is poisoned: its arms bind poisoned names and
        // the exhaustiveness and reachability checks stay silent, so one
        // failed expression is one diagnostic.
        let failed = sr.0 == NONE || self.infer.resolve(self.pool(), st) == Ty::POISON;
        let st = if failed { Ty::POISON } else { st };
        self.join_site(want, |this, result| {
            let mut rows = Vec::new();
            let mut arms = Vec::new();
            let mut rejected_arms: Vec<bool> = Vec::new();
            let mut all_never = true;
            let arm_nodes: Vec<NodeRef<'_>> = n
                .children()
                .filter(|c| c.kind() == SyntaxKind::MatchArm)
                .collect();
            for (i, arm) in arm_nodes.iter().enumerate() {
                let Some(pat) = arm.children().next() else {
                    return unsupported("an arm without a pattern");
                };
                this.scopes.push(HashMap::default());
                let mut binds = Vec::new();
                let outer_rejected = std::mem::replace(&mut this.pattern_rejected, false);
                this.declare_pattern(pat, st, &mut binds)?;
                let rejected = std::mem::replace(&mut this.pattern_rejected, outer_rejected);
                rejected_arms.push(rejected);
                let guard = match Src::child(*arm, SyntaxKind::MatchGuard) {
                    Some(g) => {
                        let Some(ge) = g.children().next() else {
                            return unsupported("an empty guard");
                        };
                        let gb = this.b.open_block();
                        let (gr, gt) = this.expr(ge, Some(Ty::BOOL))?;
                        this.expect(gt, Ty::BOOL, ge, "guard", ExpectPosition::General);
                        Some(this.b.close_block(gb, Some(gr), Ty::BOOL, g.index()))
                    }
                    None => None,
                };
                let Some(body) = Src::child(*arm, SyntaxKind::Block) else {
                    return unsupported("an arm without a body");
                };
                let ab = this.b.open_block();
                let (tail, ty) = this.block_value(body, Some(result))?;
                if ty != Ty::NEVER {
                    all_never = false;
                }
                arms.push(this.b.close_block(ab, tail, ty, body.index()));
                this.scopes.pop();
                rows.push(Row {
                    pat: Some(pat),
                    wrap_some: false,
                    binds,
                    guard,
                    arm: u32::try_from(i).unwrap_or(u32::MAX),
                });
            }
            // An arm that could not name its variants poisoned the type.
            let failed = failed || this.infer.resolve(this.pool(), st) == Ty::POISON;
            if !failed {
                this.check_exhaustive(n, &rows, &rejected_arms, st);
                this.check_reachable(&rows, &rejected_arms, st);
            }
            let db = this.b.open_block();
            this.decide(&rows, sr, st)?;
            let dec = this.b.close_block(db, None, Ty::NEVER, n.index());
            let mut rec = vec![dec];
            rec.extend(arms);
            let rec = this.b.refs_record(&rec);
            let ty = if all_never && !arm_nodes.is_empty() {
                Ty::NEVER
            } else {
                result
            };
            Ok((this.b.emit(Tag::Match, sr.0, rec, ty, n.index()), ty))
        })
    }

    // ------------------------------------------------------------ decisions

    /// Emits, into the open block, the decision tree of `rows` against the
    /// scrutinee `v`; every path ends in `ToArm` or `Unreachable`.
    pub(crate) fn decide(&mut self, rows: &[Row<'_>], v: Ref, t: Ty) -> StageResult<()> {
        let m = rows
            .iter()
            .enumerate()
            .map(|(i, r)| DRow {
                cells: vec![match (r.pat, r.wrap_some) {
                    (p, true) => Cell::Some(p),
                    (Some(p), false) => Cell::Pat(p),
                    (None, false) => Cell::Wild,
                }],
                sets: Vec::new(),
                row: i,
            })
            .collect();
        let mut lits = HashMap::default();
        self.tree(rows, &[(v, t)], m, &mut lits)
    }

    /// The decision tree of matrix `m`, whose columns test the values
    /// `occs` (Maranget's scheme, `crates/hd_check/DESIGN.md` §7.2): the
    /// first row's first column with a test decides, and each outcome
    /// continues with the rows it leaves, so no test repeats on a path.
    /// A row with no test left is a leaf: its bindings, its guard, its arm.
    fn tree<'t>(
        &mut self,
        rows: &[Row<'t>],
        occs: &[(Ref, Ty)],
        mut m: Vec<DRow<'t>>,
        lits: &mut HashMap<NodeIdx, P>,
    ) -> StageResult<()> {
        self.charge()?;
        for r in &mut m {
            self.settle(&rows[r.row], occs, r);
        }
        // Rows after one that matches everything are never tried.
        if let Some(k) = m.iter().position(|r| done(rows, r)) {
            m.truncate(k + 1);
        }
        let Some(first) = m.first() else {
            self.b
                .emit(Tag::Unreachable, NONE, NONE, Ty::NEVER, NodeIdx::NONE);
            return Ok(());
        };
        let Some(j) = first.cells.iter().position(|c| !matches!(c, Cell::Wild)) else {
            return self.leaf(rows, occs, m, lits);
        };
        // `ref.match.read`: every name bound to this column was set by
        // `settle`, so the test reads the value the reference refers to.
        if self.ref_of(occs[j].1).is_some() {
            let at = match first.cells[j] {
                Cell::Pat(p) => p.index(),
                Cell::Some(_) | Cell::Wild => NodeIdx::NONE,
            };
            let (mut v, mut t) = occs[j];
            while self.ref_of(t).is_some() {
                (v, t) = self.emit_deref(v, t, at);
            }
            let occs = splice(occs, j, [(v, t)]);
            return self.tree(rows, &occs, m, lits);
        }
        match first.cells[j] {
            Cell::Pat(p) => match p.kind() {
                SyntaxKind::TuplePattern => {
                    let (occs, m) = self.expand_tuple(occs, m, j);
                    self.tree(rows, &occs, m, lits)
                }
                SyntaxKind::DataPattern => {
                    let (occs, m) = self.expand_data(rows, occs, m, j);
                    self.tree(rows, &occs, m, lits)
                }
                SyntaxKind::VariantPattern => self.switch(rows, occs, &m, j, lits),
                SyntaxKind::LiteralPattern | SyntaxKind::RangePattern => {
                    self.test(rows, occs, m, j, p, lits)
                }
                other => unsupported(format!("the pattern {other:?} in a decision")),
            },
            Cell::Some(_) | Cell::Wild => self.switch(rows, occs, &m, j, lits),
        }
    }

    /// Turns the cells of `r` that test nothing into `Wild`, recording the
    /// store into each name they bind.
    fn settle(&self, row: &Row<'_>, occs: &[(Ref, Ty)], r: &mut DRow<'_>) {
        for (j, c) in r.cells.iter_mut().enumerate() {
            let Cell::Pat(p) = *c else { continue };
            let (v, t) = occs[j];
            let wild = match p.kind() {
                SyntaxKind::WildcardPattern => true,
                SyntaxKind::BindingPattern | SyntaxKind::SpreadPattern => {
                    bind(row, &mut r.sets, p.index(), v);
                    true
                }
                SyntaxKind::TuplePattern => p.children().next().is_none(),
                // An unknown variant was reported when the pattern was
                // declared.
                SyntaxKind::VariantPattern => self
                    .variant_fields(self.strip(t), &self.variant_pattern_name(p))
                    .is_none(),
                _ => false,
            };
            if wild {
                *c = Cell::Wild;
            }
        }
    }

    /// The first row has no test left: set its bindings, then its guard
    /// chooses between its arm and the rows below.
    fn leaf<'t>(
        &mut self,
        rows: &[Row<'t>],
        occs: &[(Ref, Ty)],
        mut m: Vec<DRow<'t>>,
        lits: &mut HashMap<NodeIdx, P>,
    ) -> StageResult<()> {
        let first = m.remove(0);
        for &(l, v, at) in &first.sets {
            self.set_local(l, v, at);
        }
        let row = &rows[first.row];
        match row.guard {
            Some(g) => {
                let to = self.b.open_block();
                self.b
                    .emit(Tag::ToArm, row.arm, NONE, Ty::NEVER, NodeIdx::NONE);
                let tob = self.b.close_block(to, None, Ty::NEVER, NodeIdx::NONE);
                let fb = self.b.open_block();
                self.tree(rows, occs, m, lits)?;
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

    /// Replaces tuple column `j` by a column per member that some row
    /// tests or binds, read once with a `Read` of the element.
    fn expand_tuple<'t>(
        &mut self,
        occs: &[(Ref, Ty)],
        m: Vec<DRow<'t>>,
        j: usize,
    ) -> (Vec<(Ref, Ty)>, Vec<DRow<'t>>) {
        let (v, t) = occs[j];
        let members = self.tuple_members(t);
        let subs: Vec<Vec<NodeRef<'t>>> = m
            .iter()
            .map(|r| match r.cells[j] {
                Cell::Pat(p) if p.kind() == SyntaxKind::TuplePattern => p.children().collect(),
                _ => Vec::new(),
            })
            .collect();
        let width = subs.iter().map(Vec::len).max().unwrap_or(0);
        let mut cols = Vec::new();
        for k in 0..width {
            let Some(s) = subs.iter().filter_map(|s| s.get(k)).find(|s| reads(**s)) else {
                continue;
            };
            let et = members.get(k).copied().unwrap_or(Ty::POISON);
            let g = self
                .b
                .field(v, u32::try_from(k).unwrap_or(0), et, s.index());
            cols.push((k, (g, et)));
        }
        let occs = splice(occs, j, cols.iter().map(|c| c.1));
        let m = m
            .into_iter()
            .zip(subs)
            .map(|(r, s)| {
                r.with_column(
                    j,
                    cols.iter()
                        .map(|(k, _)| s.get(*k).map_or(Cell::Wild, |p| Cell::Pat(*p))),
                )
            })
            .collect();
        (occs, m)
    }

    /// Replaces data column `j` by a column per field that some row tests
    /// or binds, read once with a `Read` of the field; a shorthand field binds it.
    fn expand_data<'t>(
        &mut self,
        rows: &[Row<'t>],
        occs: &[(Ref, Ty)],
        m: Vec<DRow<'t>>,
        j: usize,
    ) -> (Vec<(Ref, Ty)>, Vec<DRow<'t>>) {
        // Each row's fields: index, type, the field node, its sub-pattern.
        type Fields<'t> = Vec<(u32, Ty, NodeRef<'t>, Option<NodeRef<'t>>)>;
        let (v, t) = occs[j];
        let fields: Vec<Fields<'t>> = m
            .iter()
            .map(|r| match r.cells[j] {
                Cell::Pat(p) if p.kind() == SyntaxKind::DataPattern => p
                    .children()
                    .filter(|c| c.kind() == SyntaxKind::DataPatternField)
                    .filter_map(|f| {
                        let (idx, ft) = self.field_of(t, &self.data_field_name(f))?;
                        Some((idx, ft, f, f.children().next()))
                    })
                    .collect(),
                _ => Vec::new(),
            })
            .collect();
        let mut cols: Vec<(u32, (Ref, Ty))> = Vec::new();
        for &(idx, ft, f, sub) in fields.iter().flatten() {
            if cols.iter().any(|c| c.0 == idx) || sub.is_some_and(|s| !reads(s)) {
                continue;
            }
            let g = self.field_value(v, idx, ft, f.index());
            cols.push((idx, (g, ft)));
        }
        let occs = splice(occs, j, cols.iter().map(|c| c.1));
        let m = m
            .into_iter()
            .zip(fields)
            .map(|(mut r, fs)| {
                let mut cells = Vec::with_capacity(cols.len());
                for &(idx, (g, _)) in &cols {
                    cells.push(match fs.iter().find(|f| f.0 == idx) {
                        Some(&(_, _, _, Some(sub))) => Cell::Pat(sub),
                        Some(&(_, _, f, None)) => {
                            bind(&rows[r.row], &mut r.sets, f.index(), g);
                            Cell::Wild
                        }
                        None => Cell::Wild,
                    });
                }
                r.with_column(j, cells)
            })
            .collect();
        (occs, m)
    }

    /// A tag switch on column `j`: a `SwitchTag` per variant the rows
    /// name, in the order they first name it, each over the rows that
    /// variant leaves, with the variant's fields as new columns. The
    /// last default holds the rows with no test there, or is
    /// `Unreachable` when the rows name every variant.
    fn switch<'t>(
        &mut self,
        rows: &[Row<'t>],
        occs: &[(Ref, Ty)],
        m: &[DRow<'t>],
        j: usize,
        lits: &mut HashMap<NodeIdx, P>,
    ) -> StageResult<()> {
        let t = occs[j].1;
        let mut named: Vec<(u32, Vec<Ty>, NodeIdx)> = Vec::new();
        for r in m {
            let (name, at) = match r.cells[j] {
                Cell::Some(_) => ("Some".to_owned(), NodeIdx::NONE),
                Cell::Pat(p) if p.kind() == SyntaxKind::VariantPattern => {
                    (self.variant_pattern_name(p), p.index())
                }
                _ => continue,
            };
            let Some((idx, fs)) = self.variant_fields(t, &name) else {
                continue;
            };
            if !named.iter().any(|n| n.0 == idx) {
                named.push((idx, fs, at));
            }
        }
        let all = self.ctors(t).is_some_and(|c| c.len() == named.len());
        self.switch_from(rows, occs, m, j, &named, all, lits)
    }

    /// The `SwitchTag` chain of [`Self::switch`] from `named[0]` on.
    fn switch_from<'t>(
        &mut self,
        rows: &[Row<'t>],
        occs: &[(Ref, Ty)],
        m: &[DRow<'t>],
        j: usize,
        named: &[(u32, Vec<Ty>, NodeIdx)],
        all: bool,
        lits: &mut HashMap<NodeIdx, P>,
    ) -> StageResult<()> {
        let Some((idx, fs, at)) = named.first() else {
            if all {
                self.b
                    .emit(Tag::Unreachable, NONE, NONE, Ty::NEVER, NodeIdx::NONE);
                return Ok(());
            }
            let rest = m
                .iter()
                .filter(|r| matches!(r.cells[j], Cell::Wild))
                .map(|r| r.clone().with_column(j, []))
                .collect();
            return self.tree(rows, &splice(occs, j, []), rest, lits);
        };
        let (idx, at) = (*idx, *at);
        let (v, t) = occs[j];
        let ok = self.b.open_block();
        let mut spec: Vec<(DRow<'t>, Vec<Option<NodeRef<'t>>>)> = Vec::new();
        for r in m {
            let subs = match r.cells[j] {
                Cell::Wild => Vec::new(),
                Cell::Some(p) if idx == 1 => vec![p],
                Cell::Pat(p) if p.kind() == SyntaxKind::VariantPattern => {
                    let name = self.variant_pattern_name(p);
                    if self.variant_fields(t, &name).map(|f| f.0) != Some(idx) {
                        continue;
                    }
                    self.variant_subpatterns(p, t, &name)?.0
                }
                _ => continue,
            };
            spec.push((r.clone(), subs));
        }
        let is_option = matches!(self.pool().get(self.strip(t)), TyData::Option(_));
        let mut cols = Vec::new();
        for (f, &ft) in fs.iter().enumerate() {
            let Some(s) = spec
                .iter()
                .filter_map(|(_, s)| s.get(f).copied().flatten())
                .find(|s| reads(*s))
            else {
                continue;
            };
            let g = if is_option {
                self.b.payload(v, 1, 0, ft, s.index())
            } else {
                let k = u32::try_from(f).unwrap_or(0);
                self.b.payload(v, idx, k, ft, s.index())
            };
            cols.push((f, (g, ft)));
        }
        let occs_in = splice(occs, j, cols.iter().map(|c| c.1));
        let m_in = spec
            .into_iter()
            .map(|(r, s)| {
                r.with_column(
                    j,
                    cols.iter()
                        .map(|(f, _)| s.get(*f).copied().flatten().map_or(Cell::Wild, Cell::Pat)),
                )
            })
            .collect();
        self.tree(rows, &occs_in, m_in, lits)?;
        let okb = self.b.close_block(ok, None, Ty::NEVER, at);
        let fb = self.b.open_block();
        self.switch_from(rows, occs, m, j, &named[1..], all, lits)?;
        let fail = self.b.close_block(fb, None, Ty::NEVER, at);
        let rec = self.b.refs_record(&[Ref(idx), okb, fail]);
        self.b.emit(Tag::SwitchTag, v.0, rec, Ty::NEVER, at);
        Ok(())
    }

    /// A literal or range test of the first row's pattern `p0` in column
    /// `j`: the `then` side keeps the rows that may still match, with no
    /// test left where `p0` passing implies theirs; the `else` side drops
    /// the rows whose test `p0` failing decides.
    fn test<'t>(
        &mut self,
        rows: &[Row<'t>],
        occs: &[(Ref, Ty)],
        m: Vec<DRow<'t>>,
        j: usize,
        p0: NodeRef<'t>,
        lits: &mut HashMap<NodeIdx, P>,
    ) -> StageResult<()> {
        let (v, t) = occs[j];
        let cond = self.literal_test(p0, v, t)?;
        let mut yes = Vec::new();
        let mut no = Vec::new();
        let mut yes_done = false;
        for r in m {
            let rel = match r.cells[j] {
                Cell::Pat(p)
                    if matches!(
                        p.kind(),
                        SyntaxKind::LiteralPattern | SyntaxKind::RangePattern
                    ) =>
                {
                    self.lit_rel(p, p0, t, lits)
                }
                _ => Rel::default(),
            };
            let to_yes = !yes_done && (rel.implied || !rel.excluded);
            if !rel.covered {
                if to_yes {
                    no.push(r.clone());
                } else {
                    no.push(r);
                    continue;
                }
            }
            if to_yes {
                let mut r = r;
                if rel.implied {
                    r.cells[j] = Cell::Wild;
                }
                yes_done = done(rows, &r);
                yes.push(r);
            }
        }
        let then = self.b.open_block();
        self.tree(rows, occs, yes, lits)?;
        let tb = self.b.close_block(then, None, Ty::NEVER, p0.index());
        let fb = self.b.open_block();
        self.tree(rows, occs, no, lits)?;
        let eb = self.b.close_block(fb, None, Ty::NEVER, p0.index());
        let rec = self.b.refs_record(&[tb, eb]);
        self.b.emit(Tag::If, cond.0, rec, Ty::NEVER, p0.index());
        Ok(())
    }

    /// What `p0` passing or failing tells of pattern `p` over the same
    /// value of type `t`; unknown unless both have known values.
    fn lit_rel(
        &self,
        p: NodeRef<'_>,
        p0: NodeRef<'_>,
        t: Ty,
        lits: &mut HashMap<NodeIdx, P>,
    ) -> Rel {
        let same = Rel {
            implied: true,
            excluded: false,
            covered: true,
        };
        if p.index() == p0.index() {
            return same;
        }
        let mut value = |n: NodeRef<'_>| {
            lits.entry(n.index())
                .or_insert_with(|| self.to_p(Some(n), t))
                .clone()
        };
        let (a, b) = (value(p), value(p0));
        let apart = Rel {
            implied: false,
            excluded: true,
            covered: false,
        };
        match (a, b) {
            (P::Int(lo, hi), P::Int(lo0, hi0)) => Rel {
                implied: lo <= lo0 && hi0 <= hi,
                excluded: hi < lo0 || hi0 < lo,
                covered: lo0 <= lo && hi <= hi0,
            },
            (P::Str(x), P::Str(y)) => {
                if x == y {
                    same
                } else {
                    apart
                }
            }
            (P::Ctor(x, _), P::Ctor(y, _)) => {
                if x == y {
                    same
                } else {
                    apart
                }
            }
            // `0.0` and `-0.0` differ in bits and compare equal.
            (P::Flt(x), P::Flt(y)) if x == y => same,
            _ => Rel::default(),
        }
    }

    /// `v == literal`, or a range test, as a `bool` value.
    fn literal_test(&mut self, p: NodeRef<'_>, v: Ref, t: Ty) -> StageResult<Ref> {
        if p.kind() == SyntaxKind::RangePattern {
            // `flow.match.range.*`: bound comparisons, no range value.
            let bounds = self.range_bounds(p);
            let mut tests = Vec::new();
            if let Some(lo) = bounds.lo {
                let (l, _) = self.bound_literal(lo, t)?;
                tests.push(self.b.prim(PrimOp::Ge as u32, &[v, l], Ty::BOOL, p.index()));
            }
            if let Some(hi) = bounds.hi {
                let (h, _) = self.bound_literal(hi, t)?;
                let op = if bounds.inclusive {
                    PrimOp::Le
                } else {
                    PrimOp::Lt
                };
                tests.push(self.b.prim(op as u32, &[v, h], Ty::BOOL, p.index()));
            }
            return match tests.as_slice() {
                [only] => Ok(*only),
                [a, c] => Ok(self
                    .b
                    .prim(PrimOp::And as u32, &[*a, *c], Ty::BOOL, p.index())),
                _ => unsupported("a range pattern without a bound"),
            };
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

    /// The cooked value of a plain string literal pattern: `None` for an
    /// interpolated or prefixed string, which stays opaque.
    fn string_value(&self, p: NodeRef<'_>) -> Option<String> {
        let raw = self.cx.src.text(self.cx.src.first(p));
        if !raw.starts_with('"') {
            return None;
        }
        let mut pieces = Vec::new();
        crate::expr::decode_piece(
            crate::expr::strip_piece(raw, TokenKind::String),
            &mut pieces,
        );
        let mut out = String::new();
        for piece in pieces {
            match piece {
                crate::expr::Piece::Text(s) => out.push_str(&s),
                crate::expr::Piece::Name(_) => return None,
            }
        }
        Some(out)
    }

    /// The bits of a float literal pattern, parsed as expressions parse
    /// one: an integer text or anything unparsable stays opaque (`None`).
    fn float_value(&self, p: NodeRef<'_>) -> Option<u64> {
        let text = self.cx.src.text(self.cx.src.first(p)).replace('_', "");
        if text.parse::<u64>().is_ok() {
            return None;
        }
        text.parse::<f64>().ok().map(f64::to_bits)
    }

    /// The scalar value of a char literal pattern, decoding escapes.
    fn char_value(&self, p: NodeRef<'_>) -> Option<u32> {
        let raw = self.cx.src.text(self.cx.src.first(p));
        let inner = raw.strip_prefix('\'').unwrap_or(raw);
        let inner = inner.strip_suffix('\'').unwrap_or(inner);
        let mut pieces = Vec::new();
        crate::expr::decode_piece(inner, &mut pieces);
        let ch = match pieces.as_slice() {
            [crate::expr::Piece::Text(s)] => s.chars().next(),
            _ => inner.chars().next(),
        };
        ch.map(u32::from)
    }

    fn to_p(&self, p: Option<NodeRef<'_>>, t: Ty) -> P {
        let Some(p) = p else { return P::Wild };
        match p.kind() {
            // A spread pattern covers every list (`flow.match.spread.cover`).
            SyntaxKind::WildcardPattern
            | SyntaxKind::BindingPattern
            | SyntaxKind::SpreadPattern => P::Wild,
            SyntaxKind::LiteralPattern => match self.cx.src.tkind(self.cx.src.first(p)) {
                Some(TokenKind::KwTrue) => P::Ctor("true".into(), vec![]),
                Some(TokenKind::KwFalse) => P::Ctor("false".into(), vec![]),
                // A char literal against a char subject is one scalar
                // value, so a repeated one is unreachable; against any
                // other subject it stays opaque (`flow.match.cover`).
                Some(TokenKind::Char)
                    if matches!(self.pool().get(self.strip(t)), TyData::Prim(Prim::Char)) =>
                {
                    self.char_value(p)
                        .map_or(P::Lit, |v| P::Int(i128::from(v), i128::from(v)))
                }
                Some(first) if self.int_range(t).is_some() => {
                    let neg = first == TokenKind::Minus;
                    let tok = if neg {
                        hd_base::TokenIdx::from_raw(self.cx.src.first(p).raw() + 1)
                    } else {
                        self.cx.src.first(p)
                    };
                    match self.cx.src.tkind(tok) {
                        Some(TokenKind::Number) => self
                            .bound_value(Bound { neg, tok })
                            .map_or(P::Lit, |v| P::Int(v, v)),
                        _ => P::Lit,
                    }
                }
                Some(TokenKind::String) if self.strip(t) == Ty::STRING => {
                    self.string_value(p).map_or(P::Lit, P::Str)
                }
                Some(TokenKind::Number) if matches!(self.pool().get(self.strip(t)), TyData::Prim(p) if p.is_float()) => {
                    self.float_value(p).map_or(P::Lit, P::Flt)
                }
                _ => P::Lit,
            },
            SyntaxKind::RangePattern => self.range_p(p, t),
            SyntaxKind::TuplePattern if p.children().next().is_none() => P::Wild,
            SyntaxKind::TuplePattern => {
                let members = self.tuple_members(t);
                P::Ctor(
                    "()".into(),
                    p.children()
                        .enumerate()
                        .map(|(i, c)| {
                            self.to_p(Some(c), members.get(i).copied().unwrap_or(Ty::POISON))
                        })
                        .collect(),
                )
            }
            SyntaxKind::VariantPattern => {
                let t = self.strip(t);
                let name = self.variant_pattern_name(p);
                let fields = self
                    .variant_fields(t, &name)
                    .map(|f| f.1)
                    .unwrap_or_default();
                let subs = self
                    .variant_subpatterns(p, t, &name)
                    .map(|s| s.0)
                    .unwrap_or_default();
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
                    let fname = self.cx.names.syms.intern(&self.data_field_name(f));
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

    /// The values of an integer type, smallest and largest; `None` for
    /// any other type. `usize` is 32 bits, as its literals are checked.
    fn int_range(&self, t: Ty) -> Option<(i128, i128)> {
        let TyData::Prim(p) = self.pool().get(self.strip(t)) else {
            return None;
        };
        Some(match p {
            Prim::I8 => (i128::from(i8::MIN), i128::from(i8::MAX)),
            Prim::I16 => (i128::from(i16::MIN), i128::from(i16::MAX)),
            Prim::I32 => (i128::from(i32::MIN), i128::from(i32::MAX)),
            Prim::I64 => (i128::from(i64::MIN), i128::from(i64::MAX)),
            Prim::U8 => (0, i128::from(u8::MAX)),
            Prim::U16 => (0, i128::from(u16::MAX)),
            Prim::U32 | Prim::Usize => (0, i128::from(u32::MAX)),
            Prim::U64 => (0, i128::from(u64::MAX)),
            _ => return None,
        })
    }

    /// A range pattern as the integers it matches
    /// (`flow.match.range.cover`); an empty range is `Int(lo, hi)` with
    /// `lo > hi` (`flow.match.range.empty`).
    fn range_p(&self, p: NodeRef<'_>, t: Ty) -> P {
        let Some((min, max)) = self.int_range(t) else {
            return P::Lit;
        };
        let b = self.range_bounds(p);
        let lo = b.lo.map_or(Some(min), |x| self.bound_value(x));
        let hi = b.hi.map_or(Some(max), |x| {
            self.bound_value(x)
                .map(|v| if b.inclusive { v } else { v - 1 })
        });
        match (lo, hi) {
            (Some(lo), Some(hi)) => P::Int(lo, hi),
            _ => P::Lit,
        }
    }

    /// The members of a tuple type, its rest element last; none for a
    /// type that is not a tuple.
    fn tuple_members(&self, t: Ty) -> Vec<Ty> {
        let pool = self.pool();
        match pool.get(self.strip(t)) {
            TyData::Tuple { elems, rest } => {
                pool.list_items(elems).iter().copied().chain(rest).collect()
            }
            _ => Vec::new(),
        }
    }

    /// `t` with its views and references taken off: the type whose values
    /// the exhaustiveness and reachability checks split, which a test
    /// reads through any reference (`ref.match.read`).
    fn strip(&self, t: Ty) -> Ty {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        match pool.get(t) {
            TyData::Mut(i) | TyData::Ref { inner: i, .. } => self.strip(i),
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
            // The rest element is the tuple's last member.
            TyData::Tuple { .. } => Some(vec![("()".into(), self.tuple_members(st))]),
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
                    P::Ctor(..) | P::Lit | P::Int(..) | P::Str(_) | P::Flt(_) => None,
                    P::Wild => {
                        let mut v = vec![P::Wild; arity];
                        v.extend(r[1..].iter().cloned());
                        Some(v)
                    }
                })
                .collect()
        };
        // An integer column: the values split at the bounds of its
        // patterns (`flow.match.cover.integer`).
        let ints = m.iter().any(|r| matches!(r[0], P::Int(..)));
        match head {
            P::Int(lo, hi) => self.useful_int(m, q, tys, (*lo, *hi), depth),
            // A string or float column: only the same value matches, so
            // the arm is useful when no earlier row matches it
            // (`flow.match.unreachable`).
            P::Str(s) => !m.iter().any(|r| match &r[0] {
                P::Wild => true,
                P::Str(t) => t == s,
                _ => false,
            }),
            P::Flt(b) => !m.iter().any(|r| match &r[0] {
                P::Wild => true,
                P::Flt(c) => c == b,
                _ => false,
            }),
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
                if ints && let Some(all) = self.int_range(t) {
                    return self.useful_int(m, q, tys, all, depth);
                }
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

    /// Whether some integer in `lo..=hi` matches `q` and no row of `m`.
    /// The values split into segments at the rows' bounds, and each
    /// segment is tried with the rows that hold it.
    fn useful_int(
        &self,
        m: &[Vec<P>],
        q: &[P],
        tys: &[Ty],
        (lo, hi): (i128, i128),
        depth: u32,
    ) -> bool {
        if lo > hi {
            return false;
        }
        let mut starts = vec![lo];
        for r in m {
            if let P::Int(l, h) = r[0]
                && l <= h
            {
                starts.extend([l, h + 1].into_iter().filter(|c| *c > lo && *c <= hi));
            }
        }
        starts.sort_stable();
        starts.dedup();
        starts.iter().enumerate().any(|(i, &s)| {
            let e = starts.get(i + 1).map_or(hi, |n| n - 1);
            let rows: Vec<Vec<P>> = m
                .iter()
                .filter(|r| match r[0] {
                    P::Wild => true,
                    P::Int(l, h) => l <= s && e <= h,
                    _ => false,
                })
                .map(|r| r[1..].to_vec())
                .collect();
            self.useful(&rows, &q[1..], &tys[1..], depth + 1)
        })
    }

    /// `unreachable-match-arm` for an arm whose values the unguarded arms
    /// before it already match.
    /// Whether some value of `t` fails to match `pat`.
    pub(crate) fn refutable(&mut self, pat: NodeRef<'_>, t: Ty) -> bool {
        let p = self.to_p(Some(pat), t);
        self.useful(&[vec![p]], &[P::Wild], &[t], 0)
    }

    /// Whether a range pattern has a bound outside the integer type `t`,
    /// which `integer-literal-range` reports (`flow.match.range.bound-type`).
    fn bound_outside(&self, p: NodeRef<'_>, t: Ty) -> bool {
        let Some((min, max)) = self.int_range(t) else {
            return false;
        };
        let b = self.range_bounds(p);
        [b.lo, b.hi]
            .into_iter()
            .flatten()
            .filter_map(|x| self.bound_value(x))
            .any(|v| v < min || v > max)
    }

    fn check_reachable(&mut self, rows: &[Row<'_>], rejected: &[bool], t: Ty) {
        let mut before: Vec<Vec<P>> = Vec::new();
        for (r, rej) in rows.iter().zip(rejected) {
            // A rejected pattern has reported its error: its arm neither
            // is reported nor hides the arms after it.
            if *rej {
                continue;
            }
            let p = self.to_p(r.pat, t);
            // An arm with an out-of-type bound is already an error.
            if !self.useful(&before, std::slice::from_ref(&p), &[t], 0)
                && !r.pat.is_some_and(|n| self.bound_outside(n, t))
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
    fn check_exhaustive(&mut self, n: NodeRef<'_>, rows: &[Row<'_>], rejected: &[bool], t: Ty) {
        let m: Vec<Vec<P>> = rows
            .iter()
            .zip(rejected)
            .filter(|(r, _)| r.guard.is_none())
            .map(|(r, rej)| vec![if *rej { P::Wild } else { self.to_p(r.pat, t) }])
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
