//! Patterns, `let`, `match` and exhaustiveness (`crates/hd_check/DESIGN.md` (type checking) §7;
//! `crates/hd_tir/DESIGN.md` "Matching"). The checker declares each arm's
//! bindings, emits the arm blocks, then a decision block of `SwitchTag`,
//! `If`, payload reads (`Read [%v as .V .k]`), `Guard` and `ToArm`: a decision tree over
//! the pattern matrix, where each test runs once on a path and each
//! outcome keeps the rows, in order, that may still match. Exhaustiveness
//! is Maranget's usefulness test over the arm patterns.

use std::collections::BTreeMap;

use hd_base::SliceSort as _;
use hd_base::{HashMap, HashSet};

use hd_base::{LocalId, NodeIdx, StageResult};
use hd_diag::Code;
use hd_resolve::{ItemData, Src};
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_tir::ir::{
    BlockMark, IntrinsicOp, NONE, Place as TirPlace, PlaceRoot, PrimOp, Proj, Ref, Tag, TirSink,
};
use hd_types::{Prim, Ty, TyData, VarKind};

use crate::body::{Ck, ExpectPosition};
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

impl Cell<'_> {
    /// The syntax a test of this cell is at: its pattern, or none for a
    /// `.Some` row's test, which no pattern of its own writes.
    fn at(self) -> NodeIdx {
        match self {
            Cell::Pat(p) => p.index(),
            Cell::Some(_) | Cell::Wild => NodeIdx::NONE,
        }
    }
}

/// The work a match's usefulness pass may still spend
/// (`flow.match.limit`): a class of values the pass splits costs one plus
/// its rows, as rustc's pattern complexity counts. A match's pass has a
/// budget of `hd_base::limits::MATCH_WORK`, or less when the body's fuel
/// has less left.
struct Work {
    left: u64,
    spent: u64,
}

/// A usefulness pass ran past its [`Work`] budget.
struct Exceeded;

impl Work {
    fn new(left: u64) -> Self {
        Self { left, spent: 0 }
    }

    /// Takes a class of `rows` rows; `Exceeded` once its cost is more
    /// than is left.
    fn take(&mut self, rows: usize) -> Result<(), Exceeded> {
        let cost = 1 + rows as u64;
        if cost > self.left {
            return Err(Exceeded);
        }
        self.left -= cost;
        self.spent += cost;
        Ok(())
    }
}

/// The wildcard an omitted field stands for, and each field a wildcard
/// row expands to under a constructor.
static WILD: P = P::Wild;

/// The end of a list in a [`Pass`].
const NIL: usize = usize::MAX;

/// The lists of one usefulness pass: each row's patterns for the columns
/// left, and each class's column types, as linked cells. Specializing a
/// row pushes its head's fields onto the rest of its list, so a class
/// costs its rows and the fields they add, never a copy of each row.
struct Pass<'p> {
    pats: Vec<(&'p P, usize)>,
    tys: Vec<(Ty, usize)>,
}

impl<'p> Pass<'p> {
    fn cell(&mut self, p: &'p P, next: usize) -> usize {
        self.pats.push((p, next));
        self.pats.len() - 1
    }

    /// `rest` under the first `arity` of `args`, a wildcard for each
    /// missing one: the list of a row whose head is specialized.
    fn fields(&mut self, args: &'p [P], arity: usize, rest: usize) -> usize {
        let mut at = rest;
        for k in (0..arity).rev() {
            at = self.cell(args.get(k).unwrap_or(&WILD), at);
        }
        at
    }

    /// `rest` under `tys`, the first on top.
    fn types(&mut self, tys: &[Ty], rest: usize) -> usize {
        let mut at = rest;
        for &t in tys.iter().rev() {
            self.tys.push((t, at));
            at = self.tys.len() - 1;
        }
        at
    }
}

/// One class of values of a usefulness pass: the rows that match each of
/// its values, in arm order, each with its list of patterns left; its
/// list of column types; and the group of the first column it lies in
/// (`NIL` for the class of every value).
struct Class {
    rows: Vec<(usize, usize)>,
    tys: usize,
    top: usize,
}

/// One class a column splits a class into: the values of a constructor,
/// an integer segment, a string or a float the rows name, or the values
/// none names; the constructor's name and fields, and its rows.
struct Group<'p> {
    name: Option<&'p str>,
    arity: usize,
    tys: usize,
    rows: Vec<(usize, usize)>,
}

impl Group<'_> {
    fn plain(tys: usize) -> Self {
        Group {
            name: None,
            arity: 0,
            tys,
            rows: Vec::new(),
        }
    }
}

/// The constructors of a column type, by name, as [`Ck::ctors`] lists
/// them; computed once per type in a pass.
struct Ctors {
    list: Vec<(String, Vec<Ty>)>,
    at: HashMap<String, usize>,
}

/// What a usefulness pass must decide. It stops splitting a class once
/// the class can decide nothing more: its rows are useful already and its
/// group of the first column already holds an unmatched value.
#[derive(Clone, Copy)]
struct Need {
    /// Which rows are useful.
    rows: bool,
    /// Which groups of the first column hold a value no unguarded row
    /// matches; without `rows`, only whether one does, so the pass stops
    /// at the first.
    unmatched: bool,
}

/// What a usefulness pass decided.
struct Verdict {
    /// Per row: some value reaches it before any unguarded row above it
    /// matches that value.
    useful: Vec<bool>,
    /// Some value matches no unguarded row.
    unmatched: bool,
    /// The groups of the first column: a constructor's name (`None` for
    /// the other groups), and whether a value in it matches no unguarded
    /// row.
    top: Vec<(Option<String>, bool)>,
}

impl Verdict {
    /// Whether the group `top` of the first column holds an unmatched
    /// value; never for the class of every value, which is split.
    fn unmatched_in(&self, top: usize) -> bool {
        self.top.get(top).is_some_and(|g| g.1)
    }

    fn unmatch(&mut self, top: usize) {
        self.unmatched = true;
        if let Some(g) = self.top.get_mut(top) {
            g.1 = true;
        }
    }
}

/// A tag switch's rows by the variant each names in the switched
/// column (row positions in order), and the rows with no test there.
struct Tags {
    by_tag: HashMap<u32, Vec<usize>>,
    wild: Vec<usize>,
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

/// A step left on [`Ck::tree`]'s work stack.
enum Step<'t> {
    /// The tree of a matrix over its columns, into the open block.
    Tree(Vec<(Ref, Ty)>, Vec<DRow<'t>>),
    /// A guarded leaf whose fail side is built: close that side and emit
    /// the `Guard` choosing between the arm's block `to` and it.
    Guard { g: Ref, to: Ref, fail: BlockMark },
    /// The next step of a test chain.
    Chain(Box<Chain<'t>>),
}

/// A chain of tests on one value, each in the fail block of the one
/// before: a `SwitchTag` per variant a column names, or an `If` per
/// literal or range ([`Ck::chain`]). Each case's tree, then the rest's
/// tree in the last fail block, is built from the work stack.
struct Chain<'t> {
    /// The value tested.
    v: Ref,
    /// The cases left.
    links: Links<'t>,
    /// The columns of the matrix the chain splits.
    occs: Vec<(Ref, Ty)>,
    /// The case whose tree is being built: its test, its block, its
    /// syntax.
    open: Option<(Head, BlockMark, NodeIdx)>,
    /// Each case built: its test, its block, its syntax and its fail
    /// block, still open.
    fails: Vec<(Head, Ref, NodeIdx, BlockMark)>,
    /// What the last fail block holds.
    rest: Rest<'t>,
}

/// The cases a [`Chain`] has left.
enum Links<'t> {
    /// Each literal or range test and the rows its passing leaves, over
    /// the chain's columns; the value has type `t`.
    Lit {
        t: Ty,
        cases: std::vec::IntoIter<(NodeRef<'t>, Vec<DRow<'t>>)>,
    },
    /// Each variant column `j` names, with its field types and syntax;
    /// a variant's rows are specialized from `m` when its case starts.
    Tag {
        named: std::vec::IntoIter<(u32, Vec<Ty>, NodeIdx)>,
        tags: Tags,
        m: Vec<DRow<'t>>,
        j: usize,
        is_option: bool,
    },
}

/// The test of a [`Chain`] case: an `If` on a condition, or a
/// `SwitchTag` case on a variant.
enum Head {
    If(Ref),
    Tag(u32),
}

/// What a [`Chain`]'s last fail block holds.
enum Rest<'t> {
    /// The tree of these rows over these columns.
    Tree(Vec<(Ref, Ty)>, Vec<DRow<'t>>),
    /// `Unreachable`: the cases cover every value.
    Unreachable,
    /// Already built.
    Built,
}

/// The one value a literal pattern names ([`Ck::lit_key`]).
#[derive(Clone, PartialEq, Eq, Hash)]
enum LitKey {
    Int(i128),
    Str(String),
    Ctor(String),
    Flt(u64),
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

/// A literal column split once for its test chain ([`Ck::literal_chain`]).
struct LitSplit<'t> {
    /// Each test, in chain order, with the rows naming a known value in
    /// the column that may match when it passes, in order, each with
    /// whether its passing implies the row.
    tests: Vec<(NodeRef<'t>, Vec<(usize, bool)>)>,
    /// The rows naming no known value in the column, in order.
    other: Vec<usize>,
    /// Per row, whether it names known values the chain tests all of,
    /// so the last `else` side leaves it out.
    gone: Vec<bool>,
}

/// A row with no test left and no guard: it matches every value, so the
/// rows after it are never tried.
fn done(rows: &[Row<'_>], r: &DRow<'_>) -> bool {
    rows[r.row].guard.is_none() && r.cells.iter().all(|c| matches!(c, Cell::Wild))
}

/// The literal or range pattern a cell tests, if it tests one.
fn literal_cell(c: Cell<'_>) -> Option<NodeRef<'_>> {
    let Cell::Pat(p) = c else { return None };
    let kind = p.kind();
    (kind == SyntaxKind::LiteralPattern || kind == SyntaxKind::RangePattern).then_some(p)
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

/// `flow.match.limit.error`: a match whose coverage and reachability
/// checks need more than `hd_base::limits::MATCH_WORK`.
const MATCH_TOO_COMPLEX: &str = "checking this match's arms needs more steps than the compiler allows; split it into smaller matches";

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
            // `ref.match.borrow`: below a local reference a binding of a
            // payload that is not copied takes a reference of its kind;
            // a `&` crossed keeps every binding below it shared.
            let saved = self.pattern_below_ref;
            if self.ref_counted(t) == Some(false) {
                self.pattern_below_ref = Some(saved.map_or(mutable, |m| m && mutable));
            }
            let r = self.declare_pattern(p, self.deref_ty(referent), binds);
            self.pattern_below_ref = saved;
            return r;
        }
        let tr = self.infer.resolve(pool, t);
        let inner = tr;
        match p.kind() {
            SyntaxKind::WildcardPattern => {}
            SyntaxKind::BindingPattern => {
                let name_tok = p.direct_tokens().find(|t| {
                    matches!(
                        self.cx.src.tkind(*t),
                        Some(TokenKind::Ident | TokenKind::RawIdent)
                    )
                });
                // The parser's one binding pattern without a name is a
                // bare `Self`: a type, not a binding (`flow.match.self-pattern`).
                let Some(nt) = name_tok else {
                    self.err(
                        Code::TypeUsedAsValue,
                        p,
                        "`Self` names a type, not a binding: write `_`, a name, or a variant such as `Self.Variant`",
                    );
                    return Ok(());
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
                self.bind_pattern_name(p, name, t, binds);
                return Ok(());
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
                    self.bind_pattern_name(p, name, Ty::POISON, binds);
                }
            }
            SyntaxKind::LiteralPattern => {
                let (r, lt) = self.pattern_literal(p, Some(t))?;
                self.expect_ty(lt, t, p, "pattern", ExpectPosition::General);
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
                    let (r, lt) = self.bound_literal(b, t, p);
                    self.expect_ty(lt, t, p, "pattern", ExpectPosition::General);
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
                self.expect_ty(t, Ty::VOID, p, "pattern", ExpectPosition::General);
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
                    TyData::Infer(_) => {
                        // The pattern fixes only the tuple shape. Its element
                        // variables still need context; without it, the final
                        // check reports the empty source literal once.
                        let vs: Vec<Ty> = fixed
                            .iter()
                            .map(|_| self.infer.fresh(pool, VarKind::General))
                            .collect();
                        let rest = spread.map(|_| {
                            let elem = self.infer.fresh(pool, VarKind::General);
                            pool.intern_ty(&TyData::Adt {
                                def: self.cx.names.known.list,
                                args: pool.list(&[elem]),
                            })
                        });
                        self.row_argument_vars(hd_base::DefId::NONE, &vs, p);
                        let tt = pool.intern_ty(&TyData::Tuple {
                            elems: pool.list(&vs),
                            rest,
                        });
                        self.expect_ty(tt, t, p, "pattern", ExpectPosition::General);
                        (vs, rest)
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
                            self.bind_pattern_name(sp, name, rest_ty, binds);
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
                        // A tuple pattern needs a tuple of the same arity,
                        // wherever it stands (`flow.match.tuple.arity`).
                        if elems.len() != subs.len() {
                            let noun = if elems.len() == 1 {
                                "element"
                            } else {
                                "elements"
                            };
                            let msg = format!(
                                "{} has {} {noun}, but the pattern has {}",
                                self.show(t),
                                elems.len(),
                                subs.len()
                            );
                            self.err(Code::TypeMismatch, p, &msg);
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
                        TyData::Adt { .. } | TyData::Union(_)
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
                let (subs, unknown) = self.variant_subpatterns(p, t, &name);
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
                // Preserve resolved field positions for tooling.
                if self.cx.source_facts.is_some() {
                    self.tool_facts
                        .patterns
                        .push(crate::comparison::VariantPattern {
                            node: p.index(),
                            variant: name.clone(),
                            fields: self.variant_field_names(t, &name),
                            subs: subs.iter().map(|sub| sub.map(NodeRef::index)).collect(),
                        });
                }
                for (s, ft) in subs.iter().zip(fields) {
                    if let Some(s) = s {
                        self.declare_pattern(*s, ft, binds)?;
                    }
                }
            }
            // A data pattern: the parser builds no other pattern.
            _ => {
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
                            self.bind_pattern_name(f, name, Ty::POISON, binds);
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
                    if let Some(sub) = f.children().next() {
                        self.declare_pattern(sub, ft, binds)?;
                    } else {
                        let name = self.cx.names.syms.intern(&fname);
                        self.bind_pattern_name(f, name, ft, binds);
                    }
                }
            }
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
    /// (a `let` pattern's names follow the binding forms,
    /// `types.bind.let-pattern-mut`).
    fn bind_pattern_name(
        &mut self,
        p: NodeRef<'_>,
        name: hd_base::Symbol,
        t: Ty,
        binds: &mut Vec<(NodeIdx, LocalId)>,
    ) {
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
        let borrow = self.pattern_below_ref.filter(|_| !self.copies(t));
        let t = match borrow {
            Some(mutable) => {
                let pool = self.pool();
                pool.intern_ty(&TyData::Ref {
                    inner: self.resolved(t),
                    mutable,
                    counted: false,
                })
            }
            None => t,
        };
        let l = self.bind_local(name, t, p);
        if let Some(mutable) = borrow {
            self.ref_bound.insert(l, mutable);
        }
        if self.let_view.is_some()
            && p.direct_tokens()
                .any(|t| self.cx.src.tkind(t) == Some(TokenKind::KwMut))
        {
            self.mut_locals.push(l);
        }
        binds.push((p.index(), l));
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
    fn variant_subpatterns<'t>(&self, p: NodeRef<'t>, t: Ty, name: &str) -> VariantSubs<'t> {
        let Some(al) = Src::child(p, SyntaxKind::PatternArgumentList) else {
            return (vec![], vec![]);
        };
        let kids: Vec<NodeRef<'t>> = al.children().collect();
        if kids.iter().all(|k| k.kind() != SyntaxKind::NamedPattern) {
            return (kids.into_iter().map(Some).collect(), vec![]);
        }
        // Positional sub-patterns fill fields by position, named ones by
        // the variant's field names (data.enum.payload.order).
        let fields = self.variant_field_names(t, name);
        let positional = kids
            .iter()
            .take_while(|k| k.kind() != SyntaxKind::NamedPattern)
            .count();
        // Extra positional sub-patterns stay in the list, so the caller
        // reports the arity.
        let mut out = vec![None; fields.len().max(positional)];
        let mut unknown: Vec<(String, NodeRef<'t>, bool)> = Vec::new();
        let mut filled = vec![false; out.len()];
        for (i, k) in kids.into_iter().enumerate() {
            let (slot, sub) = if i < positional {
                (Some(i), Some(k))
            } else if k.kind() == SyntaxKind::NamedPattern {
                let fname = self.cx.src.text(self.cx.src.first(k));
                let slot = fields.iter().position(|f| f == fname);
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
        (out, unknown)
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
                    // An interpolated string is the pattern's child.
                    match self.cx.src.tkind(tok) {
                        Some(TokenKind::String) => (self.string_expr(p)?, Ty::STRING),
                        _ => self.literal_at(tok, false, p),
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
    fn bound_literal(&mut self, b: Bound, t: Ty, at: NodeRef<'_>) -> (Ref, Ty) {
        let (r, lt) = self.literal_at(b.tok, false, at);
        self.pattern_value(r, lt, Some(t), b.neg)
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

    pub(crate) fn let_stmt(&mut self, s: NodeRef<'_>) -> StageResult<()> {
        let (pat, ty_node, rhs) = binding_parts(s);
        let els = Src::child(s, SyntaxKind::ElseClause);
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
                let mut parts = self.closure_parts(rhs);
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
        let let_mut = pat.kind() == SyntaxKind::BindingPattern
            && pat
                .direct_tokens()
                .any(|t| self.cx.src.tkind(t) == Some(TokenKind::KwMut));
        let mut deferred = None;
        if let_mut {
            deferred = self.let_mut_binding(t, true, pat);
        }
        // A plain name: a new local. A bare `Self` has none and is
        // reported as a pattern (`flow.match.self-pattern`).
        if pat.kind() == SyntaxKind::BindingPattern
            && els.is_none()
            && let Some(name) = name
        {
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
                    self.user_locals.push((l, pat.index()));
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
        let refutable = self.refutable(pat, t)?;
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
            let [blk] = e.parts();
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
        let [scrut] = n.parts();
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
                // The parser builds an arm as its pattern, an optional
                // guard, and its body.
                let [pat, second] = arm.parts();
                let body = if second.kind() == SyntaxKind::MatchGuard {
                    let [_, _, body] = arm.parts();
                    body
                } else {
                    second
                };
                this.scopes.push(HashMap::default());
                let mut binds = Vec::new();
                let outer_rejected = std::mem::replace(&mut this.pattern_rejected, false);
                this.declare_pattern(pat, st, &mut binds)?;
                let rejected = std::mem::replace(&mut this.pattern_rejected, outer_rejected);
                rejected_arms.push(rejected);
                let guard = if second.kind() == SyntaxKind::MatchGuard {
                    let [ge] = second.parts();
                    let gb = this.b.open_block();
                    let (gr, gt) = this.expr(ge, Some(Ty::BOOL))?;
                    this.expect_ty(gt, Ty::BOOL, ge, "guard", ExpectPosition::General);
                    Some(this.b.close_block(gb, Some(gr), Ty::BOOL, second.index()))
                } else {
                    None
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
            // A match past its work limit has reported; its decision tree,
            // as large as its usefulness work, is a poison value.
            let too_complex = !failed && !this.check_match(n, &rows, &rejected_arms, st)?;
            let db = this.b.open_block();
            if too_complex {
                this.b.emit(Tag::Poison, NONE, NONE, Ty::POISON, n.index());
            } else {
                this.decide(&rows, sr, st)?;
            }
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
        self.tree(rows, vec![(v, t)], m, &mut lits)
    }

    /// The decision tree of matrix `m`, whose columns test the values
    /// `occs` (Maranget's scheme, `crates/hd_check/DESIGN.md` §7.2): the
    /// first row's first column with a test decides, and each outcome
    /// continues with the rows it leaves, so no test repeats on a path.
    /// A row with no test left is a leaf: its bindings, its guard, its arm.
    ///
    /// The tree is built from a work stack, not native recursion, so a
    /// match of many arms, which nests one test per arm, does not deepen
    /// the stack. Each step costs the rows it splits: a literal column is
    /// partitioned by value once ([`Self::literal_chain`]), not filtered
    /// again for each test.
    fn tree<'t>(
        &mut self,
        rows: &[Row<'t>],
        occs: Vec<(Ref, Ty)>,
        m: Vec<DRow<'t>>,
        lits: &mut HashMap<NodeIdx, P>,
    ) -> StageResult<()> {
        let mut stack = vec![Step::Tree(occs, m)];
        while let Some(step) = stack.pop() {
            match step {
                Step::Tree(occs, m) => self.matrix(rows, occs, m, lits, &mut stack)?,
                Step::Guard { g, to, fail } => {
                    let fail = self.b.close_block(fail, None, Ty::NEVER, NodeIdx::NONE);
                    let rec = self.b.refs_record(&[to, fail]);
                    self.b.emit(Tag::Guard, g.0, rec, Ty::NEVER, NodeIdx::NONE);
                }
                Step::Chain(c) => self.chain(rows, c, &mut stack)?,
            }
        }
        Ok(())
    }

    /// One matrix of [`Self::tree`]: its tree goes into the open block,
    /// the trees of its outcomes onto `stack`. Rows from `start` on are
    /// the matrix left: a guarded leaf's fail side is the rows below it,
    /// already settled, so a chain of guarded arms costs one step each.
    fn matrix<'t>(
        &mut self,
        rows: &[Row<'t>],
        mut occs: Vec<(Ref, Ty)>,
        mut m: Vec<DRow<'t>>,
        lits: &mut HashMap<NodeIdx, P>,
        stack: &mut Vec<Step<'t>>,
    ) -> StageResult<()> {
        let mut start = 0;
        let mut settled = false;
        loop {
            self.charge()?;
            if !settled {
                crate::work::decision_rows(m.len());
                for r in &mut m {
                    self.settle(&rows[r.row], &occs, r);
                }
                // Rows after one that matches everything are never tried.
                if let Some(k) = m.iter().position(|r| done(rows, r)) {
                    m.truncate(k + 1);
                }
                settled = true;
            }
            let Some(first) = m.get(start) else {
                self.b
                    .emit(Tag::Unreachable, NONE, NONE, Ty::NEVER, NodeIdx::NONE);
                return Ok(());
            };
            let Some(j) = first.cells.iter().position(|c| !matches!(c, Cell::Wild)) else {
                match self.leaf(rows, first) {
                    Some(guard) => {
                        stack.push(guard);
                        start += 1;
                        continue;
                    }
                    None => return Ok(()),
                }
            };
            m.drain(..start);
            start = 0;
            let first = &m[0];
            // Struct and tuple patterns project their places directly. They
            // need no whole-value read merely to find their fields.
            if let Cell::Pat(p) = first.cells[j] {
                match p.kind() {
                    SyntaxKind::TuplePattern => {
                        (occs, m) = self.expand_tuple(rows, &occs, m, j);
                        settled = false;
                        continue;
                    }
                    SyntaxKind::DataPattern => {
                        (occs, m) = self.expand_data(rows, &occs, m, j);
                        settled = false;
                        continue;
                    }
                    _ => {}
                }
            }
            // `ref.match.read`: every name bound to this column was set by
            // `settle`, so the test reads the value the reference refers to.
            if self.ref_of(occs[j].1).is_some() {
                let at = first.cells[j].at();
                let (mut v, mut t) = occs[j];
                while self.ref_of(t).is_some() {
                    (v, t) = self.emit_deref(v, t, at);
                }
                occs[j] = (v, t);
                settled = false;
                continue;
            }
            let chain = match first.cells[j] {
                Cell::Pat(p) => match p.kind() {
                    SyntaxKind::VariantPattern => self.switch(occs, m, j),
                    SyntaxKind::LiteralPattern | SyntaxKind::RangePattern => {
                        self.literal_chain(rows, occs, m, j, p, lits)
                    }
                    other => {
                        // ruled out: `settle` turns every other pattern a
                        // column holds into `Wild`, and tuple and data
                        // patterns are expanded above.
                        unreachable!("settle and tuple/data expansion leave only tests: {other:?}")
                    }
                },
                Cell::Some(_) | Cell::Wild => self.switch(occs, m, j),
            };
            stack.push(Step::Chain(Box::new(chain)));
            return Ok(());
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

    /// The row `first` has no test left: set its bindings, then its
    /// guard chooses between its arm and the rows below, whose tree goes
    /// into the fail block this leaves open; the step that closes it is
    /// returned.
    fn leaf<'t>(&mut self, rows: &[Row<'t>], first: &DRow<'t>) -> Option<Step<'t>> {
        for &(l, v, at) in &first.sets {
            let v = match self.ref_bound.get(&l) {
                Some(&mutable) => self.payload_ref(l, v, mutable, at).unwrap_or(v),
                None => v,
            };
            self.set_local(l, v, at);
        }
        let row = &rows[first.row];
        let Some(g) = row.guard else {
            self.b
                .emit(Tag::ToArm, row.arm, NONE, Ty::NEVER, NodeIdx::NONE);
            return None;
        };
        let to = self.b.open_block();
        self.b
            .emit(Tag::ToArm, row.arm, NONE, Ty::NEVER, NodeIdx::NONE);
        let to = self.b.close_block(to, None, Ty::NEVER, NodeIdx::NONE);
        let fail = self.b.open_block();
        Some(Step::Guard { g, to, fail })
    }

    /// `ref.match.borrow`: the reference a binding below a local reference
    /// takes to its payload. The column's value `v` reads the payload out
    /// of a read through the reference; the reference is to the same place,
    /// read through the reference itself: `[*r as #1 .0]`. `None` when `v`
    /// reads no such place.
    fn payload_ref(&mut self, l: LocalId, v: Ref, mutable: bool, at: NodeIdx) -> Option<Ref> {
        let b = self.b.body();
        let i = v.as_inst()?.0 as usize;
        if b.tags[i] != Tag::Read {
            return None;
        }
        let place = b.decode_place(b.data[i][0])?;
        self.pattern_place_ref(place, self.b.local_ty(l), mutable, at)
    }

    /// Make the reference directly from the projected place. A matrix
    /// column used only by borrowed bindings never needs a payload read.
    fn pattern_place_ref(
        &mut self,
        mut place: TirPlace,
        ty: Ty,
        mutable: bool,
        at: NodeIdx,
    ) -> Option<Ref> {
        let b = self.b.body();
        // Prefix each read the place is rooted at, until the one that
        // dereferences the reference.
        while !place.proj.contains(&Proj::Deref) {
            let PlaceRoot::Value(x) = place.root else {
                return None;
            };
            let x = x.0 as usize;
            if x >= b.len() || b.tags[x] != Tag::Read {
                return None;
            }
            let inner = b.decode_place(b.data[x][0])?;
            let mut proj = inner.proj;
            proj.extend(place.proj);
            place = TirPlace {
                root: inner.root,
                proj,
            };
        }
        let word = self.b.place(&place);
        Some(self.b.emit(Tag::Ref, word, u32::from(mutable), ty, at))
    }

    /// A column whose reading patterns all bind references can carry
    /// that reference itself. Value tests or value bindings still need
    /// the ordinary read, shared by the rows that use it.
    fn borrowed_column<'t>(
        &self,
        rows: &[Row<'t>],
        patterns: impl Iterator<Item = (usize, NodeRef<'t>)>,
    ) -> Option<(Ty, bool)> {
        let mut found = None;
        for (row, pattern) in patterns.filter(|(_, p)| reads(*p)) {
            let &(_, local) = rows[row]
                .binds
                .iter()
                .find(|(node, _)| *node == pattern.index())?;
            let mutable = *self.ref_bound.get(&local)?;
            let kind = (self.b.local_ty(local), mutable);
            if found.is_some_and(|previous| previous != kind) {
                return None;
            }
            found = Some(kind);
        }
        found
    }

    /// Project a struct or tuple through references without reading the
    /// whole value first. Individual columns decide whether to read or borrow.
    fn pattern_fields(&self, v: Ref, mut t: Ty) -> (TirPlace, Ty) {
        let mut proj = Vec::new();
        while let Some((inner, _)) = self.ref_of(t) {
            proj.push(Proj::Deref);
            t = self.deref_ty(inner);
        }
        (
            TirPlace {
                root: PlaceRoot::Value(v),
                proj,
            },
            t,
        )
    }

    /// Replaces tuple column `j` by a column per member that some row
    /// tests or binds, read once with a `Read` of the element.
    fn expand_tuple<'t>(
        &mut self,
        rows: &[Row<'t>],
        occs: &[(Ref, Ty)],
        m: Vec<DRow<'t>>,
        j: usize,
    ) -> (Vec<(Ref, Ty)>, Vec<DRow<'t>>) {
        let (base, t) = self.pattern_fields(occs[j].0, occs[j].1);
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
            let field = u32::try_from(k).unwrap_or(0);
            let borrowed = self.borrowed_column(
                rows,
                m.iter()
                    .zip(&subs)
                    .filter_map(|(r, subs)| subs.get(k).map(|&pattern| (r.row, pattern))),
            );
            let mut place = base.clone();
            place.proj.push(Proj::Field(field));
            let reference = borrowed.and_then(|(ty, mutable)| {
                self.pattern_place_ref(place.clone(), ty, mutable, s.index())
                    .map(|r| (r, ty))
            });
            let value = reference.unwrap_or_else(|| {
                let word = self.b.place(&place);
                (self.b.emit(Tag::Read, word, 0, et, s.index()), et)
            });
            cols.push((k, value));
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
        let (base, t) = self.pattern_fields(occs[j].0, occs[j].1);
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
            let borrowed = self.borrowed_column(
                rows,
                m.iter().zip(&fields).filter_map(|(r, fields)| {
                    fields
                        .iter()
                        .find(|field| field.0 == idx)
                        .map(|&(_, _, node, sub)| (r.row, sub.unwrap_or(node)))
                }),
            );
            let mut place = base.clone();
            place.proj.push(Proj::Field(idx));
            let reference = borrowed.and_then(|(ty, mutable)| {
                self.pattern_place_ref(place.clone(), ty, mutable, f.index())
                    .map(|r| (r, ty))
            });
            let value = reference.unwrap_or_else(|| {
                let word = self.b.place(&place);
                (self.b.emit(Tag::Read, word, 0, ft, f.index()), ft)
            });
            cols.push((idx, value));
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
    fn switch<'t>(&mut self, occs: Vec<(Ref, Ty)>, m: Vec<DRow<'t>>, j: usize) -> Chain<'t> {
        crate::work::decision_rows(m.len());
        let (v, t) = occs[j];
        let mut named: Vec<(u32, Vec<Ty>, NodeIdx)> = Vec::new();
        let mut seen = HashSet::default();
        // Each row's variant, looked up once: by variant, the rows that
        // name it, and the rows with no test in this column.
        let mut by_tag: HashMap<u32, Vec<usize>> = HashMap::default();
        let mut wild = Vec::new();
        for (i, r) in m.iter().enumerate() {
            let name = match r.cells[j] {
                Cell::Wild => {
                    wild.push(i);
                    continue;
                }
                Cell::Some(_) => "Some".to_owned(),
                Cell::Pat(p) if p.kind() == SyntaxKind::VariantPattern => {
                    self.variant_pattern_name(p)
                }
                Cell::Pat(_) => continue,
            };
            let at = r.cells[j].at();
            // `Some(..)` is the variant at index 1 whether or not the
            // column's type names it.
            let found = self.variant_fields(t, &name);
            let tag = match (&r.cells[j], &found) {
                (Cell::Some(_), _) => Some(1),
                (_, found) => found.as_ref().map(|f| f.0),
            };
            if let Some(tag) = tag {
                by_tag.entry(tag).or_default().push(i);
            }
            if let Some((idx, fs)) = found
                && seen.insert(idx)
            {
                named.push((idx, fs, at));
            }
        }
        let all = self.ctors(t).is_some_and(|c| c.len() == named.len());
        let rest = if all {
            Rest::Unreachable
        } else {
            let rest = wild
                .iter()
                .map(|&i| m[i].clone().with_column(j, []))
                .collect();
            Rest::Tree(splice(&occs, j, []), rest)
        };
        let is_option = self.option_arg(self.strip(t)).is_some();
        Chain {
            v,
            links: Links::Tag {
                named: named.into_iter(),
                tags: Tags { by_tag, wild },
                m,
                j,
                is_option,
            },
            occs,
            open: None,
            fails: Vec::new(),
            rest,
        }
    }

    /// One step of a test chain ([`Chain`]): close the case whose tree is
    /// built and open the fail block after it, then start the next case,
    /// or the rest once every case is started, or close the fail blocks,
    /// innermost first, once the rest is built. The chain is a loop over
    /// its cases, so a match over many variants or values does not
    /// recurse once per case.
    fn chain<'t>(
        &mut self,
        rows: &[Row<'t>],
        mut c: Box<Chain<'t>>,
        stack: &mut Vec<Step<'t>>,
    ) -> StageResult<()> {
        if let Some((head, mark, at)) = c.open.take() {
            let ok = self.b.close_block(mark, None, Ty::NEVER, at);
            c.fails.push((head, ok, at, self.b.open_block()));
        }
        let case = match &mut c.links {
            Links::Lit { t, cases } => match cases.next() {
                Some((p, yes)) => {
                    if !c.fails.is_empty() {
                        // The `else` side's tree, which a chain enters
                        // directly.
                        self.charge()?;
                    }
                    let cond = self.literal_test(p, c.v, *t)?;
                    let mark = self.b.open_block();
                    Some((Head::If(cond), mark, p.index(), c.occs.clone(), yes))
                }
                None => None,
            },
            Links::Tag {
                named,
                tags,
                m,
                j,
                is_option,
            } => match named.next() {
                Some((idx, fs, at)) => {
                    let mark = self.b.open_block();
                    let (occs, m) =
                        self.variant_case(rows, &c.occs, m, *j, (idx, &fs), tags, *is_option);
                    Some((Head::Tag(idx), mark, at, occs, m))
                }
                None => None,
            },
        };
        if let Some((head, mark, at, occs, m)) = case {
            c.open = Some((head, mark, at));
            stack.push(Step::Chain(c));
            stack.push(Step::Tree(occs, m));
            return Ok(());
        }
        match std::mem::replace(&mut c.rest, Rest::Built) {
            Rest::Tree(occs, m) => {
                stack.push(Step::Chain(c));
                stack.push(Step::Tree(occs, m));
                return Ok(());
            }
            Rest::Unreachable => {
                self.b
                    .emit(Tag::Unreachable, NONE, NONE, Ty::NEVER, NodeIdx::NONE);
            }
            Rest::Built => {}
        }
        while let Some((head, ok, at, fb)) = c.fails.pop() {
            let fail = self.b.close_block(fb, None, Ty::NEVER, at);
            match head {
                Head::If(cond) => {
                    let rec = self.b.refs_record(&[ok, fail]);
                    self.b.emit(Tag::If, cond.0, rec, Ty::NEVER, at);
                }
                Head::Tag(idx) => {
                    let rec = self.b.refs_record(&[Ref(idx), ok, fail]);
                    self.b.emit(Tag::SwitchTag, c.v.0, rec, Ty::NEVER, at);
                }
            }
        }
        Ok(())
    }

    /// The rows the variant `idx` of switch column `j` leaves, with its
    /// fields `fs` as new columns, each read once in the open block.
    #[expect(
        clippy::too_many_arguments,
        reason = "one case of a switch: its columns, rows, variant and tags"
    )]
    fn variant_case<'t>(
        &mut self,
        rows: &[Row<'t>],
        occs: &[(Ref, Ty)],
        m: &[DRow<'t>],
        j: usize,
        (idx, fs): (u32, &[Ty]),
        tags: &Tags,
        is_option: bool,
    ) -> (Vec<(Ref, Ty)>, Vec<DRow<'t>>) {
        let (v, t) = occs[j];
        let mut picked = tags.by_tag.get(&idx).cloned().unwrap_or_default();
        picked.extend_from_slice(&tags.wild);
        picked.sort_stable();
        crate::work::decision_rows(picked.len());
        let mut spec: Vec<(DRow<'t>, Vec<Option<NodeRef<'t>>>)> = Vec::new();
        for i in picked {
            let r = &m[i];
            let subs = match r.cells[j] {
                Cell::Wild => Vec::new(),
                Cell::Some(p) => vec![p],
                Cell::Pat(p) => {
                    let name = self.variant_pattern_name(p);
                    self.variant_subpatterns(p, t, &name).0
                }
            };
            spec.push((r.clone(), subs));
        }
        let mut cols = Vec::new();
        for (f, &ft) in fs.iter().enumerate() {
            let Some(s) = spec
                .iter()
                .filter_map(|(_, s)| s.get(f).copied().flatten())
                .find(|s| reads(*s))
            else {
                continue;
            };
            let (variant, field) = if is_option {
                (1, 0)
            } else {
                (idx, u32::try_from(f).unwrap_or(0))
            };
            let borrowed = self.borrowed_column(
                rows,
                spec.iter().filter_map(|(r, subs)| {
                    subs.get(f)
                        .copied()
                        .flatten()
                        .map(|pattern| (r.row, pattern))
                }),
            );
            let reference = borrowed.and_then(|(ty, mutable)| {
                self.pattern_place_ref(
                    TirPlace {
                        root: PlaceRoot::Value(v),
                        proj: vec![Proj::Variant(variant), Proj::Field(field)],
                    },
                    ty,
                    mutable,
                    s.index(),
                )
                .map(|r| (r, ty))
            });
            let value =
                reference.unwrap_or_else(|| (self.b.payload(v, variant, field, ft, s.index()), ft));
            cols.push((f, value));
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
        (occs_in, m_in)
    }

    /// The literal or range tests of column `j`, starting with the first
    /// row's pattern `p0`, as a chain of `If`s, each in the `else` side of
    /// the one before. A test's `then` side keeps the rows that may still
    /// match, with no test left where its passing implies theirs; its
    /// `else` side drops the rows its failing decides.
    ///
    /// The column is split once ([`Self::value_split`],
    /// [`Self::span_split`]): the chain tests the head rows' patterns in
    /// order while each names values no test before it names. A test's
    /// `then` side is the rows the split gives it merged in order with the
    /// rows naming no known value there, up to the first its passing
    /// settles; rows whose values the chain
    /// tests are gone from the last `else` side. So a flat match of n
    /// literal or range arms costs n plus the sides it builds, not n² / 2.
    /// An opaque literal or an empty range at the head is one test over
    /// every row.
    fn literal_chain<'t>(
        &mut self,
        rows: &[Row<'t>],
        occs: Vec<(Ref, Ty)>,
        m: Vec<DRow<'t>>,
        j: usize,
        p0: NodeRef<'t>,
        lits: &mut HashMap<NodeIdx, P>,
    ) -> Chain<'t> {
        let (v, t) = occs[j];
        crate::work::decision_rows(m.len());
        let LitSplit { tests, other, gone } = if self.int_range(t).is_some() {
            self.span_split(rows, &m, j, p0, t, lits)
        } else {
            self.value_split(&m, j, p0, t, lits)
        };
        if tests.is_empty() {
            // `p0` is an opaque literal or an empty range: one test.
            let (yes, no) = self.literal_sides(rows, m, j, p0, t, lits);
            return Chain {
                v,
                links: Links::Lit {
                    t,
                    cases: vec![(p0, yes)].into_iter(),
                },
                occs: occs.clone(),
                open: None,
                fails: Vec::new(),
                rest: Rest::Tree(occs, no),
            };
        }
        let mut cases = Vec::with_capacity(tests.len());
        for (p, keyed) in tests {
            // The rows the split gives this test, merged in order with the
            // rows naming no known value, up to the first its passing
            // settles. A test tells nothing of a row naming no known
            // value: a wildcard, an opaque literal, or an empty range,
            // which matches nothing and is reported.
            let mut yes = Vec::new();
            let mut named = keyed.into_iter().peekable();
            let mut rest = other.iter().copied().peekable();
            loop {
                let next = match (named.peek(), rest.peek()) {
                    (Some(&(i, implied)), Some(&o)) if i < o => {
                        named.next();
                        (i, implied)
                    }
                    (_, Some(&o)) => {
                        rest.next();
                        (o, false)
                    }
                    (Some(&(i, implied)), None) => {
                        named.next();
                        (i, implied)
                    }
                    (None, None) => break,
                };
                let mut r = m[next.0].clone();
                if next.1 {
                    r.cells[j] = Cell::Wild;
                }
                let settled = done(rows, &r);
                yes.push(r);
                if settled {
                    break;
                }
            }
            crate::work::decision_rows(yes.len());
            cases.push((p, yes));
        }
        let no = m
            .into_iter()
            .enumerate()
            .filter(|&(i, _)| !gone[i])
            .map(|(_, r)| r)
            .collect();
        Chain {
            v,
            links: Links::Lit {
                t,
                cases: cases.into_iter(),
            },
            occs: occs.clone(),
            open: None,
            fails: Vec::new(),
            rest: Rest::Tree(occs, no),
        }
    }

    /// The split of a literal column of strings, floats, chars or `bool`s,
    /// whose known values are equal or apart: partitioned by value in one
    /// pass. The chain tests each value in the order the rows first name
    /// it, while the next row left names a new value and tests nothing
    /// before column `j`. A value's test passing implies every row naming
    /// it, and excludes the rows naming another.
    fn value_split<'t>(
        &self,
        m: &[DRow<'t>],
        j: usize,
        p0: NodeRef<'t>,
        t: Ty,
        lits: &mut HashMap<NodeIdx, P>,
    ) -> LitSplit<'t> {
        // Each row's known value in this column, by the value's class.
        let mut keys: Vec<Option<usize>> = Vec::with_capacity(m.len());
        let mut class: HashMap<LitKey, usize> = HashMap::default();
        let mut groups: Vec<Vec<(usize, bool)>> = Vec::new();
        let mut other: Vec<usize> = Vec::new();
        for (i, r) in m.iter().enumerate() {
            let key = literal_cell(r.cells[j]).and_then(|p| self.lit_key(p, t, lits));
            let k = key.map(|key| {
                let n = class.len();
                let k = *class.entry(key).or_insert(n);
                if k == groups.len() {
                    groups.push(Vec::new());
                }
                groups[k].push((i, true));
                k
            });
            if k.is_none() {
                other.push(i);
            }
            keys.push(k);
        }
        // The chain: the classes of the rows that head each `else` side,
        // the first row's first. A row of a tested class is gone there.
        let mut tested = vec![false; groups.len()];
        let mut tests = Vec::new();
        for (r, &key) in m.iter().zip(&keys) {
            match key {
                Some(k) if tested[k] => {}
                Some(k) if r.cells[..j].iter().all(|c| matches!(c, Cell::Wild)) => {
                    tested[k] = true;
                    let p = literal_cell(r.cells[j]).unwrap_or(p0);
                    tests.push((p, std::mem::take(&mut groups[k])));
                }
                _ => break,
            }
        }
        let gone = keys.iter().map(|k| k.is_some_and(|k| tested[k])).collect();
        LitSplit { tests, other, gone }
    }

    /// The split of an integer column, whose literals and ranges are
    /// spans of values: the chain tests each head row's span while it is
    /// apart from every span tested before it; a span within a tested
    /// one is passed over, and one that overlaps a tested span ends the
    /// chain. The tested spans are disjoint, so one ordered map finds, for
    /// each row in one pass, the tests its span meets: a test's passing
    /// implies the row when the row's span holds the test's. A test takes
    /// no row after one its passing settles, so a run of such rows meets
    /// each test once.
    fn span_split<'t>(
        &self,
        rows: &[Row<'t>],
        m: &[DRow<'t>],
        j: usize,
        p0: NodeRef<'t>,
        t: Ty,
        lits: &mut HashMap<NodeIdx, P>,
    ) -> LitSplit<'t> {
        let mut spans: Vec<Option<(i128, i128)>> = Vec::with_capacity(m.len());
        let mut other = Vec::new();
        for (i, r) in m.iter().enumerate() {
            let span = literal_cell(r.cells[j]).and_then(|p| {
                match lits
                    .entry(p.index())
                    .or_insert_with(|| self.to_p(Some(p), t))
                {
                    &mut P::Int(lo, hi) if lo <= hi => Some((lo, hi)),
                    _ => None,
                }
            });
            if span.is_none() {
                other.push(i);
            }
            spans.push(span);
        }
        // The tested spans by their low end: the high end and the test.
        let mut tested: BTreeMap<i128, (i128, usize)> = BTreeMap::new();
        let within = |tested: &BTreeMap<i128, (i128, usize)>, (lo, hi): (i128, i128)| {
            tested
                .range(..=lo)
                .next_back()
                .is_some_and(|(_, &(h, _))| hi <= h)
        };
        let mut tests: Vec<(NodeRef<'t>, Vec<(usize, bool)>)> = Vec::new();
        for (r, &span) in m.iter().zip(&spans) {
            let Some((lo, hi)) = span else { break };
            if within(&tested, (lo, hi)) {
                continue;
            }
            let meets = tested
                .range(..=lo)
                .next_back()
                .is_some_and(|(_, &(h, _))| lo <= h)
                || tested.range(lo..=hi).next().is_some();
            if meets || !r.cells[..j].iter().all(|c| matches!(c, Cell::Wild)) {
                break;
            }
            tested.insert(lo, (hi, tests.len()));
            tests.push((literal_cell(r.cells[j]).unwrap_or(p0), Vec::new()));
        }
        // The tests still taking rows: a test whose passing settles a row
        // takes none after it.
        let mut open = tested.clone();
        let mut gone = vec![false; m.len()];
        let mut met = Vec::new();
        for (i, (r, &span)) in m.iter().zip(&spans).enumerate() {
            let Some((lo, hi)) = span else { continue };
            gone[i] = within(&tested, (lo, hi));
            met.clear();
            if let Some((&l, &(h, _))) = open.range(..lo).next_back()
                && lo <= h
            {
                met.push(l);
            }
            met.extend(open.range(lo..=hi).map(|(&l, _)| l));
            let settles = rows[r.row].guard.is_none()
                && r.cells
                    .iter()
                    .enumerate()
                    .all(|(c, cell)| c == j || matches!(cell, Cell::Wild));
            for &l in &met {
                let (h, k) = open[&l];
                let implied = lo <= l && h <= hi;
                tests[k].1.push((i, implied));
                if implied && settles {
                    open.remove(&l);
                }
            }
        }
        LitSplit { tests, other, gone }
    }

    /// The sides of one literal or range test of `p0` in column `j`: the
    /// rows that may still match when it passes, with no test left where
    /// its passing implies theirs, and the rows its failing leaves.
    fn literal_sides<'t>(
        &self,
        rows: &[Row<'t>],
        m: Vec<DRow<'t>>,
        j: usize,
        p0: NodeRef<'t>,
        t: Ty,
        lits: &mut HashMap<NodeIdx, P>,
    ) -> (Vec<DRow<'t>>, Vec<DRow<'t>>) {
        crate::work::decision_rows(m.len());
        let mut yes = Vec::new();
        let mut no = Vec::new();
        let mut yes_done = false;
        for r in m {
            let rel = match literal_cell(r.cells[j]) {
                Some(p) => self.lit_rel(p, p0, t, lits),
                None => Rel::default(),
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
        (yes, no)
    }

    /// The one value a literal pattern names, when known: two patterns
    /// with one key match the same values, two with different keys no
    /// common value. A float's key is its bits, `-0.0` taken as `0.0`,
    /// since the two compare equal; a range is no key.
    fn lit_key(&self, p: NodeRef<'_>, t: Ty, lits: &mut HashMap<NodeIdx, P>) -> Option<LitKey> {
        if p.kind() != SyntaxKind::LiteralPattern {
            return None;
        }
        let value = lits
            .entry(p.index())
            .or_insert_with(|| self.to_p(Some(p), t));
        match value {
            P::Int(lo, hi) if lo == hi => Some(LitKey::Int(*lo)),
            P::Str(s) => Some(LitKey::Str(s.clone())),
            P::Ctor(name, args) if args.is_empty() => Some(LitKey::Ctor(name.clone())),
            P::Flt(bits) => Some(LitKey::Flt(if f64::from_bits(*bits) == 0.0 {
                0
            } else {
                *bits
            })),
            _ => None,
        }
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
                let (l, _) = self.bound_literal(lo, t, p);
                tests.push(self.b.prim(PrimOp::Ge as u32, &[v, l], Ty::BOOL, p.index()));
            }
            if let Some(hi) = bounds.hi {
                let (h, _) = self.bound_literal(hi, t, p);
                let op = if bounds.inclusive {
                    PrimOp::Le
                } else {
                    PrimOp::Lt
                };
                tests.push(self.b.prim(op as u32, &[v, h], Ty::BOOL, p.index()));
            }
            // The parser builds a range pattern with a bound.
            let test = tests.into_iter().reduce(|a, c| {
                self.b
                    .prim(PrimOp::And as u32, &[a, c], Ty::BOOL, p.index())
            });
            return Ok(test.expect("a range pattern's bound"));
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
                let subs = self.variant_subpatterns(p, t, &name).0;
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
                let Some(ItemData::Struct(fs, _)) = self.cx.lookup.item(def).map(|i| &i.data)
                else {
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

    /// `t` with its references taken off: the type whose values
    /// the exhaustiveness and reachability checks split, which a test
    /// reads through any reference (`ref.match.read`).
    fn strip(&self, t: Ty) -> Ty {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        match pool.get(t) {
            TyData::Ref { inner: i, .. } => self.strip(i),
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
            TyData::Union(members) => Some(
                pool.list_items(members)
                    .iter()
                    .enumerate()
                    .map(|(i, member)| (format!("_{i}"), vec![*member]))
                    .collect(),
            ),
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
                ItemData::Struct(fs, _) => {
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

    /// The constructors of the column type `t`, from the pass's `cache`.
    fn ctors_of(
        &self,
        cache: &mut HashMap<Ty, Option<std::rc::Rc<Ctors>>>,
        t: Ty,
    ) -> Option<std::rc::Rc<Ctors>> {
        cache
            .entry(t)
            .or_insert_with(|| {
                self.ctors(t).map(|list| {
                    let at = list
                        .iter()
                        .enumerate()
                        .map(|(i, (c, _))| (c.clone(), i))
                        .collect();
                    std::rc::Rc::new(Ctors { list, at })
                })
            })
            .clone()
    }

    /// Which of `rows` (a pattern, and whether its arm has a guard) some
    /// value of `t` reaches before an unguarded row above it, and which
    /// values match no unguarded row (`flow.match.unreachable`,
    /// `flow.match.cover`), in one pass over the matrix, as rustc's
    /// `compute_exhaustiveness_and_usefulness` does. A class of values
    /// splits on its first column: one class per constructor, integer
    /// segment, string or float that its rows name, each holding the rows
    /// that match it, and one class for the values none names, holding
    /// the wildcard rows. A class with no column left reaches its rows up
    /// to the first unguarded one, and a class with no row is a value no
    /// arm matches. An opaque literal is useful once a value reaches it,
    /// and matches no value of a split.
    ///
    /// Classes are taken from a work stack, not by recursion: a data
    /// pattern adds a column per field, so the depth would grow with a
    /// type's width. Each class takes its cost from `work`, and the pass
    /// stops with `Exceeded` past it. A row is in one class per value
    /// its head names, so a flat match of n arms over c columns costs
    /// about n · c, and a wildcard row costs one place in each class of
    /// its column.
    fn usefulness<'p>(
        &self,
        rows: &'p [(P, bool)],
        t: Ty,
        need: Need,
        work: &mut Work,
    ) -> Result<Verdict, Exceeded> {
        let mut pass = Pass {
            pats: Vec::with_capacity(rows.len()),
            tys: vec![(t, NIL)],
        };
        let first = (0..rows.len())
            .map(|i| (i, pass.cell(&rows[i].0, NIL)))
            .collect();
        let mut v = Verdict {
            useful: vec![false; rows.len()],
            unmatched: false,
            top: Vec::new(),
        };
        let mut cache = HashMap::default();
        let mut stack = vec![Class {
            rows: first,
            tys: 0,
            top: NIL,
        }];
        while let Some(class) = stack.pop() {
            work.take(class.rows.len())?;
            if (!need.unmatched || v.unmatched_in(class.top))
                && (!need.rows || class.rows.iter().all(|&(r, _)| v.useful[r]))
            {
                continue;
            }
            // The class of every value is split even with no row, so
            // that an unmatched value has a group of the first column.
            if class.tys == NIL || (class.rows.is_empty() && class.top != NIL) {
                let covered = class.rows.iter().any(|&(r, _)| {
                    v.useful[r] = true;
                    !rows[r].1
                });
                if !covered {
                    v.unmatch(class.top);
                    if !need.rows {
                        return Ok(v);
                    }
                }
                continue;
            }
            let (t, rest_tys) = pass.tys[class.tys];
            let mut live: Vec<(usize, &'p P, usize)> = Vec::with_capacity(class.rows.len());
            for &(r, s) in &class.rows {
                let (p, rest) = pass.pats[s];
                v.useful[r] |= matches!(p, P::Lit);
                live.push((r, p, rest));
            }
            let groups = self.split(&mut pass, &mut cache, &live, t, rest_tys);
            for g in groups.into_iter().rev() {
                let top = if class.top == NIL {
                    v.top.push((g.name.map(str::to_owned), false));
                    v.top.len() - 1
                } else {
                    class.top
                };
                stack.push(Class {
                    rows: g.rows,
                    tys: g.tys,
                    top,
                });
            }
        }
        Ok(v)
    }

    /// The classes the head column of `live` (a row, its head pattern
    /// and the rest of its list) splits into, of type `t` with the rest of
    /// the columns `rest`. Each row is in the classes of the values its
    /// head matches, in row order. Integers split at the bounds the
    /// rows name, within `t`'s values (`flow.match.cover.integer`); the
    /// segments no row names share the class of wildcard rows.
    fn split<'p>(
        &self,
        pass: &mut Pass<'p>,
        cache: &mut HashMap<Ty, Option<std::rc::Rc<Ctors>>>,
        live: &[(usize, &'p P, usize)],
        t: Ty,
        rest: usize,
    ) -> Vec<Group<'p>> {
        let mut groups: Vec<Group<'p>> = Vec::new();
        let mut by_ctor: HashMap<&str, usize> = HashMap::default();
        let mut by_str: HashMap<&str, usize> = HashMap::default();
        let mut by_flt: HashMap<u64, usize> = HashMap::default();
        // The values no row names have a class of their own.
        let mut other = false;
        let mut ints = false;
        let mut cs: Option<Option<std::rc::Rc<Ctors>>> = None;
        let mut named = 0;
        for &(_, p, _) in live {
            match p {
                P::Ctor(c, args) if !by_ctor.contains_key(c.as_str()) => {
                    let cs = cs.get_or_insert_with(|| self.ctors_of(cache, t));
                    let fields = cs
                        .as_ref()
                        .and_then(|cs| cs.at.get(c.as_str()).map(|&i| &cs.list[i].1));
                    named += usize::from(fields.is_some());
                    let ftys = fields
                        .cloned()
                        .unwrap_or_else(|| vec![Ty::POISON; args.len()]);
                    let tys = pass.types(&ftys, rest);
                    by_ctor.insert(c.as_str(), groups.len());
                    groups.push(Group {
                        name: Some(c.as_str()),
                        arity: ftys.len(),
                        tys,
                        rows: Vec::new(),
                    });
                }
                P::Str(s) if !by_str.contains_key(s.as_str()) => {
                    other = true;
                    by_str.insert(s.as_str(), groups.len());
                    groups.push(Group::plain(rest));
                }
                P::Flt(b) if !by_flt.contains_key(b) => {
                    other = true;
                    by_flt.insert(*b, groups.len());
                    groups.push(Group::plain(rest));
                }
                P::Int(..) => ints = true,
                // A wildcard or an opaque literal names no value, and a
                // constructor, string or float seen before has its class.
                P::Wild | P::Lit | P::Ctor(..) | P::Str(_) | P::Flt(_) => {}
            }
        }
        if let Some(cs) = &cs {
            other |= cs.as_ref().is_none_or(|cs| named < cs.list.len());
        }
        // The integer segments: `starts[j]` begins segment `j`, which ends
        // before the next; `seg[j]` is its group, `NIL` for one no row
        // names.
        let mut starts: Vec<i128> = Vec::new();
        let mut seg: Vec<usize> = Vec::new();
        let mut domain = (0, -1);
        if ints {
            domain = self.int_range(t).unwrap_or_else(|| {
                // An open set of integers (a `char`): the hull of the
                // rows' values, and the class of the rest.
                other = true;
                live.iter()
                    .fold((i128::MAX, i128::MIN), |(lo, hi), x| match *x.1 {
                        P::Int(l, h) if l <= h => (lo.min(l), hi.max(h)),
                        _ => (lo, hi),
                    })
            });
            let (lo, hi) = domain;
            starts.push(lo);
            for &(_, p, _) in live {
                if let P::Int(l, h) = *p
                    && l <= h
                {
                    starts.extend(
                        [l, h.saturating_add(1)]
                            .into_iter()
                            .filter(|c| *c > lo && *c <= hi),
                    );
                }
            }
            starts.sort_stable();
            starts.dedup();
            // The rows naming each segment, as a difference count.
            let mut count = vec![0i64; starts.len() + 1];
            for &(_, p, _) in live {
                if let P::Int(l, h) = *p {
                    let (a, b) = segments(&starts, domain, (l, h));
                    if a < b {
                        count[a] += 1;
                        count[b] -= 1;
                    }
                }
            }
            let mut open = 0;
            for d in &count[..starts.len()] {
                open += d;
                if open > 0 {
                    seg.push(groups.len());
                    groups.push(Group::plain(rest));
                } else {
                    other = true;
                    seg.push(NIL);
                }
            }
        }
        if other || groups.is_empty() {
            groups.push(Group::plain(rest));
        }
        for &(r, p, at) in live {
            match p {
                P::Wild => {
                    for g in &mut groups {
                        let s = pass.fields(&[], g.arity, at);
                        g.rows.push((r, s));
                    }
                }
                P::Ctor(c, args) => {
                    let g = &mut groups[by_ctor[c.as_str()]];
                    let s = pass.fields(args, g.arity, at);
                    g.rows.push((r, s));
                }
                P::Str(s) => groups[by_str[s.as_str()]].rows.push((r, at)),
                P::Flt(b) => groups[by_flt[b]].rows.push((r, at)),
                P::Int(l, h) => {
                    let (a, b) = segments(&starts, domain, (*l, *h));
                    for &g in &seg[a..b] {
                        groups[g].rows.push((r, at));
                    }
                }
                P::Lit => {}
            }
        }
        groups
    }

    /// Whether some value of `t` fails to match `pat`. The question's
    /// work charges the body's fuel, and stops the check when it runs
    /// out; one pattern's work grows with its size alone.
    pub(crate) fn refutable(&mut self, pat: NodeRef<'_>, t: Ty) -> StageResult<bool> {
        let row = [(self.to_p(Some(pat), t), false)];
        let mut work = Work::new(self.fuel.left());
        let need = Need {
            rows: false,
            unmatched: true,
        };
        match self.usefulness(&row, t, need, &mut work) {
            Ok(v) if self.fuel.charge(work.spent) => Ok(v.unmatched),
            _ => crate::body::out_of_fuel(),
        }
    }

    /// `flow.match.limit`: the coverage and reachability checks of the
    /// match `n`, one usefulness pass with a [`Work`] budget of
    /// `MATCH_WORK`. Past it the match is `match-too-complex`, neither
    /// check reports, and the answer is false. The work charges the
    /// body's fuel; a body with less fuel left than the limit stops at its
    /// fuel, as `item-too-complex`.
    ///
    /// A rejected pattern has reported its error: its arm is neither
    /// reported nor hides the arms after it, and an unguarded one leaves
    /// no value unmatched.
    fn check_match(
        &mut self,
        n: NodeRef<'_>,
        rows: &[Row<'_>],
        rejected: &[bool],
        t: Ty,
    ) -> StageResult<bool> {
        let limit = u64::from(hd_base::limits::MATCH_WORK);
        let fuel = self.fuel.left();
        let mut work = Work::new(limit.min(fuel));
        let kept: Vec<&Row<'_>> = rows
            .iter()
            .zip(rejected)
            .filter(|(_, rej)| !**rej)
            .map(|(r, _)| r)
            .collect();
        let matrix: Vec<(P, bool)> = kept
            .iter()
            .map(|r| (self.to_p(r.pat, t), r.guard.is_some()))
            .collect();
        let need = Need {
            rows: true,
            unmatched: !rows
                .iter()
                .zip(rejected)
                .any(|(r, rej)| *rej && r.guard.is_none()),
        };
        let checked = self.usefulness(&matrix, t, need, &mut work);
        if let Ok(v) = &checked {
            if need.unmatched && v.unmatched {
                let msg = format!("no arm matches {}", self.missing(v, t));
                self.err(Code::NonexhaustiveMatch, n, &msg);
            }
            for (r, useful) in kept.iter().zip(&v.useful) {
                // An arm with an out-of-type bound is already an error.
                if !useful
                    && let Some(node) = r.pat
                    && !self.bound_outside(node, t)
                {
                    self.err(
                        Code::UnreachableMatchArm,
                        node,
                        "the arms before this one match all of its values",
                    );
                }
            }
        } else {
            if fuel < limit {
                return crate::body::out_of_fuel();
            }
            self.err(Code::MatchTooComplex, n, MATCH_TOO_COMPLEX);
        }
        if self.fuel.charge(work.spent) {
            Ok(checked.is_ok())
        } else {
            crate::body::out_of_fuel()
        }
    }

    /// The top-level constructors of `t` with a value no unguarded row
    /// matches, as `nonexhaustive-match` names them; `_` for a type with
    /// an open set of values.
    fn missing(&self, v: &Verdict, t: Ty) -> String {
        let Some(cs) = self.ctors(t) else {
            return "_".into();
        };
        let mut named: HashMap<&str, bool> = HashMap::default();
        let mut other = false;
        for (name, unmatched) in &v.top {
            match name {
                Some(c) => {
                    named.insert(c.as_str(), *unmatched);
                }
                None => other |= *unmatched,
            }
        }
        cs.into_iter()
            .filter(|(c, _)| named.get(c.as_str()).copied().unwrap_or(other))
            .map(|(c, _)| c)
            .collect::<Vec<_>>()
            .join(", ")
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
}

/// The segments `a..b` of `starts` that the integers `l..=h` cover within
/// `domain`.
fn segments(starts: &[i128], (lo, hi): (i128, i128), (l, h): (i128, i128)) -> (usize, usize) {
    let (l, h) = (l.max(lo), h.min(hi));
    if l > h {
        return (0, 0);
    }
    (
        starts.partition_point(|&s| s < l),
        starts.partition_point(|&s| s <= h),
    )
}

/// A `let` or `using` statement's pattern, optional type and value, as
/// the parser builds them (an optional `else` clause follows).
pub(crate) fn binding_parts(s: NodeRef<'_>) -> (NodeRef<'_>, Option<NodeRef<'_>>, NodeRef<'_>) {
    let [pat, second] = s.parts();
    if second.kind().is_type() {
        let [_, _, rhs] = s.parts();
        (pat, Some(second), rhs)
    } else {
        (pat, None, second)
    }
}
