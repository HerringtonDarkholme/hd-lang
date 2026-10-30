//! Names, calls and member lookup (`crates/hd_check/DESIGN.md` (type checking) §2.3, §5): local and
//! item names, variant constructors, generic calls, inherent, built-in and
//! trait methods, static members, conversions, function values, bang calls
//! and the row check of a callee's requirements.

use hd_base::SliceSort as _;
use hd_base::{HashMap, HashSet};

use hd_base::{DefId, PathId, StageResult, Symbol};
use hd_diag::Code;
use hd_intern::PathKind;
use hd_resolve::{BindingKind, FnSig, HeadKind, ItemData, Src};
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_tir::ir::{
    Callee, ChoiceKind, IntrinsicOp, NONE, PrimOp, Providers, Ref, Tag, TirSink, local_flags,
};
use hd_types::solver::{Answer, Candidate, Evidence, Goal, TraitRef};
use hd_types::{Inputs, ParamRef, Prim, Ty, TyData, TyList, VarKind, with_assoc_args};

use crate::body::{Ck, ExpectPosition, unsupported};
use crate::ty::Named;

/// Own methods followed by the shared methods of the ordered visible interfaces.
#[derive(Default)]
pub struct MethodIndex {
    /// Inherent methods: (impl, method).
    pub inherent: HashMap<Symbol, Vec<(DefId, DefId)>>,
    /// Trait methods: (trait, method).
    pub traits: HashMap<Symbol, Vec<(DefId, DefId)>>,
    seen: HashSet<DefId>,
    shared: Option<std::sync::Arc<Self>>,
}

fn name_distance(left: &str, right: &str) -> usize {
    let right: Vec<_> = right.chars().collect();
    let mut row: Vec<_> = (0..=right.len()).collect();
    for (i, left) in left.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, right) in right.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (row[j] + 1)
                .min(above + 1)
                .min(diagonal + usize::from(left != *right));
            diagonal = above;
        }
    }
    row[right.len()]
}

impl MethodIndex {
    #[must_use]
    pub fn build(lookup: &hd_resolve::Lookup<'_>) -> Self {
        let mut ix = Self::default();
        ix.extend(lookup.own.iter());
        ix.extend(
            lookup
                .ifaces
                .iter()
                .flat_map(|f| f.items.impls().chain(f.items.traits())),
        );
        ix
    }

    /// An interface closure has one index, independent of each module's private items.
    #[must_use]
    pub fn interfaces(lookup: &hd_resolve::Lookup<'_>) -> Self {
        let mut ix = Self::default();
        ix.extend(
            lookup
                .ifaces
                .iter()
                .flat_map(|f| f.items.impls().chain(f.items.traits())),
        );
        ix
    }

    /// Own declarations win, including declarations also held by an interface (#346).
    #[must_use]
    pub fn with_own(lookup: &hd_resolve::Lookup<'_>, shared: std::sync::Arc<Self>) -> Self {
        let mut ix = Self::default();
        ix.extend(lookup.own.iter());
        ix.shared = Some(shared);
        ix
    }

    fn extend<'a>(&mut self, items: impl Iterator<Item = &'a hd_resolve::Item>) {
        for it in items {
            if !self.seen.insert(it.def) {
                continue;
            }
            match &it.data {
                ItemData::Impl {
                    trait_, methods, ..
                } if *trait_ == DefId::NONE => {
                    for (n, m) in methods {
                        self.inherent.entry(*n).or_default().push((it.def, *m));
                    }
                }
                ItemData::Trait(t) => {
                    for (n, m) in &t.methods {
                        self.traits.entry(*n).or_default().push((it.def, *m));
                    }
                }
                _ => {}
            }
        }
    }

    fn map(&self, inherent: bool) -> &HashMap<Symbol, Vec<(DefId, DefId)>> {
        if inherent {
            &self.inherent
        } else {
            &self.traits
        }
    }

    fn candidates(&self, sym: Symbol, inherent: bool) -> Option<Vec<(DefId, DefId)>> {
        let mut out = self.map(inherent).get(&sym).cloned().unwrap_or_default();
        if let Some(shared) = &self.shared {
            out.extend(
                shared
                    .map(inherent)
                    .get(&sym)
                    .into_iter()
                    .flatten()
                    .filter(|(def, _)| !self.seen.contains(def))
                    .copied(),
            );
        }
        (!out.is_empty()).then_some(out)
    }

    fn names(&self) -> impl Iterator<Item = Symbol> + '_ {
        self.inherent
            .keys()
            .chain(self.traits.keys())
            .copied()
            .chain(
                self.shared
                    .iter()
                    .flat_map(|ix| ix.inherent.iter().chain(&ix.traits))
                    .filter(|(_, methods)| methods.iter().any(|(def, _)| !self.seen.contains(def)))
                    .map(|(name, _)| *name),
            )
    }
}

/// A resolved method.
pub(crate) enum Hit {
    /// An inherent method with its impl's arguments.
    Inherent {
        method: DefId,
        impl_def: DefId,
        impl_args: Vec<Ty>,
    },
    /// A language-tier built-in method.
    Builtin {
        op: IntrinsicOp,
        params: Vec<Ty>,
        ret: Ty,
    },
    /// A trait method: the trait's arguments and the evidence choice.
    Trait {
        trait_: DefId,
        method: DefId,
        args: Vec<Ty>,
        choice: (ChoiceKind, u32),
    },
    /// A method of one trait that the receiver implements through these
    /// impl candidates (an `Instantiations` answer, in content order).
    Choice {
        trait_: DefId,
        method: DefId,
        cands: Vec<Candidate>,
    },
    /// Methods of two or more traits (`trait.resolve.ambiguous`).
    Ambiguous(Vec<DefId>),
    /// A method of the one trait the receiver may implement, whose impl
    /// waits for inference to fix the receiver: the call is typed by the
    /// trait's signature and its impl is chosen once the receiver is known.
    Pending { trait_: DefId, method: DefId },
}

/// The choice of a trait method call whose impl waits for inference,
/// replaced when the body is finished.
pub(crate) const PENDING_CHOICE: (ChoiceKind, u32) = (ChoiceKind::Builtin, u32::MAX - 1);

/// A trait method with its `Self` and the trait's arguments.
pub(crate) struct TraitTarget {
    pub trait_: DefId,
    pub method: DefId,
    pub self_ty: Ty,
    pub args: Vec<Ty>,
}

/// Argument nodes of a call: positional values, then named ones, then a
/// trailing block (`fn.trailing.form`), which supplies the final parameter.
pub(crate) struct Args<'t> {
    pub positional: Vec<NodeRef<'t>>,
    /// The operand of a positional spread `x...`, which stands after every
    /// plain positional value (`expr.call.spread`).
    pub spread: Option<NodeRef<'t>>,
    /// Positional arguments written after the spread
    /// (`expr.call.spread.position`).
    pub after_spread: Vec<NodeRef<'t>>,
    pub named: Vec<(String, NodeRef<'t>)>,
    pub trailing: Option<NodeRef<'t>>,
}

/// The parameters a call's arguments are checked against.
pub(crate) struct Formals<'a> {
    pub params: &'a [(Symbol, Ty)],
    pub defaults: &'a [bool],
    /// Whether the final parameter is a vararg (`fn.vararg.form`).
    pub variadic: bool,
    /// Per parameter: the inferred type parameters it names, by generic
    /// index, so the inference join checks arguments that share one
    /// (`types.generic.infer.join.other-conflict`).
    pub inferred: &'a [Vec<usize>],
    /// A concrete function-type view of a bare generic parameter's one
    /// `Fn` or `FnMut` bound. It supplies closure parameter types without
    /// fixing the parameter to the dynamic `fn` representation.
    pub closure_wants: Option<&'a [Option<Ty>]>,
}

/// Whether a pre-instantiation formal type is a bare occurrence of type
/// parameter `i` of `owner`, with at most an outer permission on it.
/// Wrapped occurrences (`Tree[T]`) unify through the parameter instead and
/// never join here.
fn bare_param(pool: hd_types::Types<'_>, t: Ty, owner: DefId, i: usize) -> bool {
    let t = match pool.get(t) {
        TyData::Mut(inner) => inner,
        _ => t,
    };
    matches!(
        pool.get(t),
        TyData::Param(p) if p.owner == owner && p.index as usize == i
    )
}

/// Per-formal inferred generic indices for the join check: a formal joins
/// every group for a type parameter of `owner` it names bare, unless an
/// explicit argument was written for it. `sig` must align with `formals`;
/// any extra formal joins nothing. `_` placeholders count as explicit and
/// keep today's behavior.
fn inferred_formals(
    ck: &Ck,
    owner: DefId,
    sig: &FnSig,
    explicit: &[Ty],
    formals: &[(Symbol, Ty)],
) -> Vec<Vec<usize>> {
    let pool = ck.pool();
    formals
        .iter()
        .zip(sig.params.iter())
        .map(|(_, (_, s))| {
            (0..sig.generics.len())
                .filter(|&i| explicit.get(i).is_none() && bare_param(pool, *s, owner, i))
                .collect()
        })
        .chain(std::iter::repeat_with(Vec::new))
        .take(formals.len())
        .collect()
}

/// Per-formal expected function types supplied by a bare generic
/// parameter's `Fn` or `FnMut` bound. The argument still infers the
/// concrete `F`; this view only gives an unannotated closure its input
/// types.
fn fn_bound_formals(
    ck: &Ck,
    owner: DefId,
    sig: &FnSig,
    formals: &[(Symbol, Ty)],
    inst: &dyn Fn(Ty) -> Ty,
) -> Vec<Option<Ty>> {
    let pool = ck.pool();
    formals
        .iter()
        .zip(&sig.params)
        .map(|(_, (_, raw))| {
            let index = (0..sig.generics.len()).find(|&i| bare_param(pool, *raw, owner, i))?;
            let bound = sig.generics[index].bounds.iter().find_map(|b| {
                let bound = inst(*b);
                match pool.get(bound) {
                    TyData::TraitValue { def, args, .. }
                        if ck.cx.names.known.fn_trait_name(def).is_some() =>
                    {
                        Some(args)
                    }
                    _ => None,
                }
            })?;
            let [input, output, row] = pool.list_items(bound) else {
                return None;
            };
            let TyData::Row(row) = pool.get(*row) else {
                return None;
            };
            Some(pool.intern_ty(&TyData::Fn {
                params: pool.list(&[*input]),
                result: *output,
                row,
                suspends: false,
                inputs: Inputs::Tuple,
                deps: hd_types::FnDeps::NONE,
            }))
        })
        .collect()
}

/// Records this argument's type for each inferred generic its formal names
/// and reports whether it conflicts with an earlier one for the same
/// generic (`types.generic.infer.join.other-conflict`).
fn join_arg(
    ck: &mut Ck,
    groups: Option<&[usize]>,
    first: &mut HashMap<usize, Ty>,
    mark: hd_diag::DiagMark,
    t: Ty,
    e: NodeRef<'_>,
    what: &str,
) -> bool {
    // Literals take the solved type (`types.generic.infer.join.literal`,
    // and string injection): they never constrain the join either way.
    // Parentheses do not change the value.
    let mut bare = e;
    while bare.kind() == SyntaxKind::ParenExpr {
        let Some(inner) = bare.children().next() else {
            break;
        };
        bare = inner;
    }
    if matches!(
        bare.kind(),
        SyntaxKind::LiteralExpr | SyntaxKind::StringExpr | SyntaxKind::PlaceholderExpr
    ) {
        return false;
    }
    // One failed expression is one diagnostic: once this call has an error,
    // later arguments keep their own checking but join no further.
    if ck.diags.errors_since(mark) > 0 {
        return false;
    }
    let Some(gs) = groups else {
        return false;
    };
    for g in gs {
        if let Some(&prev) = first.get(g) {
            if ck.infer_other_conflict(prev, t, e, what) {
                return true;
            }
        } else {
            first.insert(*g, t);
        }
    }
    false
}

/// Where a positional spread stands among the parameters not yet given.
#[derive(Clone, Copy)]
struct SpreadAt<'a> {
    /// Whether the final of those parameters is a vararg.
    variadic: bool,
    /// How many plain positional arguments precede the spread.
    given: usize,
    /// The parameters a named argument supplies; the spread skips them.
    named: &'a [bool],
    /// The separate arguments a rest tuple vararg still holds, which the
    /// spread then ends (`fn.vararg.collect.tuple-expr`).
    items: &'a [(Ref, Ty)],
}

impl Args<'_> {
    pub(crate) fn empty() -> Self {
        Args {
            positional: Vec::new(),
            spread: None,
            after_spread: Vec::new(),
            named: Vec::new(),
            trailing: None,
        }
    }
}

impl Ck<'_, '_> {
    pub(crate) fn args_of<'t>(&mut self, al: Option<NodeRef<'t>>) -> Args<'t> {
        let mut a = Args::empty();
        for c in al.iter().flat_map(|l| l.children()) {
            match c.kind() {
                SyntaxKind::Argument => {
                    let Some(e) = c.children().next() else {
                        continue;
                    };
                    let operand = if e.kind() == SyntaxKind::SpreadExpr {
                        e.children().next()
                    } else {
                        None
                    };
                    match (operand, a.spread) {
                        (Some(o), None) => a.spread = Some(o),
                        (_, Some(_)) => a.after_spread.push(operand.unwrap_or(e)),
                        (None, None) => a.positional.push(e),
                    }
                }
                SyntaxKind::NamedArgument => {
                    let name = self.cx.src.text(self.cx.src.first(c)).to_owned();
                    let Some(e) = c.children().next() else {
                        continue;
                    };
                    a.named.push((name, e));
                }
                _ => {}
            }
        }
        // A direct element-reference argument participates in the call's
        // static conflict check (`ref.elem.conflict`).
        let all: Vec<NodeRef<'t>> = a
            .positional
            .iter()
            .chain(&a.after_spread)
            .chain(a.named.iter().map(|(_, e)| e))
            .copied()
            .collect();
        for e in all {
            self.elem_may_stand(e, crate::elem_refs::ElemUse::Arg);
        }
        a
    }

    fn method_index(&self) -> &crate::MethodIndex {
        self.cx
            .methods
            .get_or_init(|| std::sync::Arc::new(crate::MethodIndex::build(self.cx.lookup)))
    }

    /// The traits declaring a method `sym`, with their method, in the
    /// program: what the per-folder method index maps the name to.
    fn traits_declaring(&self, sym: Symbol) -> Vec<(DefId, DefId)> {
        self.method_index()
            .candidates(sym, false)
            .unwrap_or_default()
    }

    /// Whether the trait's name is bound at the call: declared in the
    /// module or in an enclosing block, introduced by a `use`, or supplied
    /// by the prelude (`trait.avail.module`, `trait.avail.scope`,
    /// `trait.avail.prelude`).
    fn trait_available(&self, tr: DefId) -> bool {
        self.cx
            .locals
            .iter()
            .any(|l| l.def == tr && l.covers(self.at))
            || self
                .cx
                .scope_at(self.at)
                .binding
                .iter()
                .any(|b| b.kind == BindingKind::Item && b.value == tr.raw())
    }

    /// The traits declaring `sym` that are available to a dot call or a
    /// type-qualified call here. A trait that is not available is not a
    /// candidate (`trait.avail.not-candidate`).
    fn available_traits(&self, sym: Symbol) -> Vec<(DefId, DefId)> {
        let mut traits = self.traits_declaring(sym);
        traits.retain(|&(tr, _)| self.trait_available(tr));
        traits
    }

    // ------------------------------------------------------------ names

    pub(crate) fn import_hint(&self, name: &str) -> String {
        hd_resolve::iface::std_import_hint(
            &self.cx.names,
            self.cx.lookup.ifaces.iter().flat_map(|f| &f.exports),
            name,
        )
    }

    fn name_hint(&self, name: &str) -> String {
        if name == "print" {
            "; did you mean `println`?".to_owned()
        } else {
            self.import_hint(name)
        }
    }

    pub(crate) fn name_expr(&mut self, n: NodeRef<'_>, want: Option<Ty>) -> StageResult<(Ref, Ty)> {
        let s = self.sym_of(n);
        if let Some((l, d)) = self.find_local(s) {
            return Ok(self.read_local(l, d, n));
        }
        if let Some(v) = self.global_get(s, n) {
            return Ok(v);
        }
        let text = self.cx.names.text(s).to_owned();
        if self.is_poison_name(&text) {
            return Ok(self.poison_value(n));
        }
        if let Some(Named::Item(def)) = self.scope_name(&text)
            && let Some(v) = self.declaration_value(def, &text, want, n)?
        {
            return Ok(v);
        }
        let msg = format!("`{text}` is not defined{}", self.name_hint(&text));
        self.err(Code::UnknownName, n, &msg);
        Ok((Ref(NONE), Ty::POISON))
    }

    /// The declaration `def`, written as `name`, used as a value: a
    /// function, or a data type without fields. A type, alias or trait is
    /// an error (`names.type-as-value`); `None` for any other declaration.
    pub(crate) fn declaration_value(
        &mut self,
        def: DefId,
        name: &str,
        want: Option<Ty>,
        n: NodeRef<'_>,
    ) -> StageResult<Option<(Ref, Ty)>> {
        if let Some(item) = self.cx.lookup.item(def) {
            if let ItemData::Fn(sig) = &item.data {
                let sig = sig.clone();
                return self.item_value(def, &sig, &[], want, n).map(Some);
            }
            if let ItemData::Data(fields) = &item.data
                && fields.is_empty()
            {
                let t = self.ctor(def, &[])?;
                let rec = self.b.refs_record(&[]);
                return Ok(Some((
                    self.b.emit(Tag::NewStruct, NONE, rec, t, n.index()),
                    t,
                )));
            }
        }
        let kind = self.kind_of_item(def);
        if kind.is_some_and(|k| k.is_type() || k == HeadKind::Trait) {
            let what = if kind == Some(HeadKind::Trait) {
                "a trait"
            } else {
                "a type"
            };
            let msg = format!("`{name}` is {what}, not a value");
            self.err(Code::TypeUsedAsValue, n, &msg);
            return Ok(Some((Ref(NONE), Ty::POISON)));
        }
        Ok(None)
    }

    /// `f::[T]` as a value, or a member's `Owner::name::[T]`.
    pub(crate) fn explicit_item_value(
        &mut self,
        n: NodeRef<'_>,
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let Some(inner) = n.children().next() else {
            return unsupported("a type-argument value shape");
        };
        let mut explicit = Vec::new();
        if let Some(tl) = Src::child(n, SyntaxKind::TypeArgumentList) {
            let gens = self.explicit_generics(inner);
            explicit = self.explicit_targs(tl, &gens)?;
        }
        if inner.kind() == SyntaxKind::PathExpr {
            return self.path_value(inner, want, &explicit);
        }
        if let Some(l) = self.local_generic_named(inner) {
            return Ok(self.local_generic_value(l, &explicit, n));
        }
        let segs = self.segments_expr(inner);
        if segs.first().is_some_and(|s| self.is_poison_name(s)) {
            return Ok(self.poison_value(n));
        }
        let Some(def) = self.resolve_path(&segs) else {
            let hint = match segs.as_slice() {
                [name] => self.name_hint(name),
                _ => String::new(),
            };
            let msg = format!("`{}` is not defined{hint}", segs.join("."));
            self.err(Code::UnknownName, n, &msg);
            return Ok((Ref(NONE), Ty::POISON));
        };
        if let Some(ItemData::Fn(sig)) = self.cx.lookup.item(def).map(|i| &i.data) {
            let sig = sig.clone();
            return self.item_value(def, &sig, &explicit, want, n);
        }
        unsupported("a type with arguments used as a value")
    }

    /// The generic parameters of the function or type a type-argument
    /// callee names by path, for the kinds of its slots: a bare key in a
    /// row slot is a kind error (`fn.generic.explicit.row-dollar`,
    /// `req.row.slot.data`), and a const slot takes a const argument.
    /// Empty for any other callee.
    pub(crate) fn explicit_generics(&self, callee: NodeRef<'_>) -> Vec<hd_resolve::Generic> {
        if self.local_generic_named(callee).is_some() {
            return vec![];
        }
        self.resolve_path(&self.segments_expr(callee))
            .and_then(|d| self.cx.lookup.item(d))
            .map(|item| match item.sig() {
                Some(sig) => sig.generics.clone(),
                None => item.generics.clone(),
            })
            .unwrap_or_default()
    }

    /// The generic local function a name expression names, if it does.
    fn local_generic_named(&self, e: NodeRef<'_>) -> Option<hd_base::LocalId> {
        if e.kind() != SyntaxKind::NameExpr {
            return None;
        }
        let (l, _) = self.find_local(self.sym_of(e))?;
        self.local_generics.contains_key(&l).then_some(l)
    }

    /// The member's own type arguments: the written ones, the trailing
    /// slots fresh (`fn.generic.explicit.trailing`). A list longer than
    /// the member's parameters is an error at `n`
    /// (`fn.generic.explicit.too-long-count`).
    pub(crate) fn fresh_generics(
        &mut self,
        sig: &FnSig,
        explicit: &[Ty],
        n: NodeRef<'_>,
    ) -> Vec<Ty> {
        let pool = self.pool();
        if explicit.len() > sig.generics.len() {
            let msg = format!(
                "{} type arguments are written, but the function takes {}",
                explicit.len(),
                sig.generics.len()
            );
            self.err(Code::ArgumentCount, n, &msg);
        }
        // `fn.generic.explicit.row-dollar`: the slot of a row parameter
        // takes a row type argument written after `$`. A bare key or row
        // alias there is a kind error; an inferred or poisoned slot stays
        // to inference or its own error.
        for (i, g) in sig.generics.iter().enumerate() {
            let Some(t) = explicit.get(i) else {
                continue;
            };
            if !g.row {
                continue;
            }
            let row_like = matches!(
                pool.get(*t),
                TyData::Row(_) | TyData::Infer(_) | TyData::Poison
            );
            if !row_like {
                let msg = format!(
                    "the slot of row parameter `${}` takes a row type argument written after `$`, as in `::[$ ...]`",
                    self.cx.names.text(g.name)
                );
                self.err(Code::GenericKindMismatch, n, &msg);
            }
        }
        let vars: Vec<Ty> = sig
            .generics
            .iter()
            .enumerate()
            .map(|(i, _)| {
                explicit
                    .get(i)
                    .copied()
                    .unwrap_or_else(|| self.infer.fresh(pool, VarKind::General))
            })
            .collect();
        vars
    }

    /// A const parameter of a call that no explicit argument fills and no
    /// parameter type names: only argument types solve one, so neither a
    /// runtime value nor the expected result can
    /// (`types.generic.const.infer-unsolved`). Its variable becomes poison,
    /// so the call reports nothing more about it.
    pub(crate) fn unsolvable_consts(
        &mut self,
        sig: &FnSig,
        owner: DefId,
        explicit: usize,
        vars: &mut [Ty],
        n: NodeRef<'_>,
    ) {
        let pool = self.pool();
        for (i, g) in sig.generics.iter().enumerate().skip(explicit) {
            if g.const_type.is_none() {
                continue;
            }
            let index = u16::try_from(i).unwrap_or(u16::MAX);
            let named = std::cell::Cell::new(false);
            for (_, t) in &sig.params {
                pool.subst(*t, &|p| {
                    named.set(named.get() || (p.owner == owner && p.index == index));
                    None
                });
            }
            if !named.get() {
                let name = self.cx.names.text(g.name);
                let msg = format!(
                    "no argument's type names const parameter `{name}`, so it cannot be inferred: write it in the call, as in `f::[4](...)`"
                );
                self.err(Code::CannotInferType, n, &msg);
                if let Some(v) = vars.get_mut(i) {
                    *v = Ty::POISON;
                }
            }
        }
    }

    /// Records a call's type variables for the end of the body: one
    /// that is still unsolved then, with its default already filled in,
    /// is `cannot-infer-type` (`types.generic.default.unsolved`).
    /// A call with an argument that is already an error, or has type
    /// `never` because of one, reports nothing more: that error stands
    /// for the whole call.
    pub(crate) fn open_call_params(&mut self, vars: &[Ty], refs: &[Ref], n: NodeRef<'_>) {
        let pool = self.pool();
        let poisoned = refs.iter().any(|r| {
            r.as_inst().is_none_or(|i| {
                let t = self.b.body_mut().ty[i.0 as usize];
                t == Ty::NEVER || pool.has_poison(t)
            })
        });
        if poisoned {
            return;
        }
        for &v in vars {
            self.call_params.push((v, n.index()));
        }
    }

    // ------------------------------------------------------------ calls

    pub(crate) fn call(
        &mut self,
        n: NodeRef<'_>,
        kids: &[NodeRef<'_>],
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let (Some(callee), al) = (
            kids.first().copied(),
            Src::child(n, SyntaxKind::ArgumentList),
        ) else {
            return unsupported("a call shape");
        };
        let args = self.args_of(al);
        self.call_args(n, callee, &args, want)
    }

    /// `f(a): block` and `f: block` (`fn.trailing.form`,
    /// `fn.trailing.empty-parentheses`): the call with one more final
    /// argument, the block, which `check_args` checks as a zero-argument
    /// closure typed by the callee's final parameter (`crates/hd_tir/DESIGN.md`,
    /// "What The Checker Desugars").
    pub(crate) fn trailing_call(
        &mut self,
        n: NodeRef<'_>,
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let (Some(head), Some(block)) = (n.children().next(), Src::child(n, SyntaxKind::Block))
        else {
            return unsupported("a trailing-block call shape");
        };
        let (call, callee, al) = if head.kind() == SyntaxKind::CallExpr {
            let Some(callee) = head.children().next() else {
                return unsupported("a call shape");
            };
            (head, callee, Src::child(head, SyntaxKind::ArgumentList))
        } else {
            (n, head, None)
        };
        let mut args = self.args_of(al);
        args.trailing = Some(block);
        self.call_args(call, callee, &args, want)
    }

    /// A call of `callee` with `args`; `n` is the call node. The call is
    /// one exclusivity frame: the accesses of its element reference
    /// arguments and receiver end with it (`ref.elem.access.live`).
    pub(crate) fn call_args(
        &mut self,
        n: NodeRef<'_>,
        callee: NodeRef<'_>,
        args: &Args<'_>,
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        self.open_access_frame();
        let r = self.call_args_in_frame(n, callee, args, want);
        self.close_access_frame();
        r
    }

    fn call_args_in_frame(
        &mut self,
        n: NodeRef<'_>,
        callee: NodeRef<'_>,
        args: &Args<'_>,
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let (callee, bang) = if callee.kind() == SyntaxKind::SuspendExpr {
            match callee.children().next() {
                Some(c) => (c, true),
                None => return unsupported("a bang call shape"),
            }
        } else {
            (callee, false)
        };
        match callee.kind() {
            SyntaxKind::VariantExpr => self.variant_value(callee, args, want, Some(n)),
            SyntaxKind::NameExpr => {
                let s = self.sym_of(callee);
                if let Some((l, d)) = self.find_local(s) {
                    let form = self.local_forms.get(&l).cloned();
                    if let Some(form) = form {
                        let vars = self.local_generic_vars(l, &[], callee);
                        let (f, ft) = match &vars {
                            Some(v) => self.local_generic_inst(l, v, callee),
                            None => self.read_local(l, d, callee),
                        };
                        return self.call_value_form(f, ft, args, n, bang, Some((&form, vars)));
                    }
                    let (f, ft) = self.read_local(l, d, callee);
                    return self.call_value(f, ft, args, n, bang);
                }
                // A top-level binding holding a function value
                // (`names.module.declarations`, `module.init.binding`).
                if let Some((f, ft)) = self.global_get(s, callee) {
                    return self.call_value(f, ft, args, n, bang);
                }
                let text = self.cx.names.text(s).to_owned();
                if let Some(p) = Prim::ALL.iter().find(|p| p.name() == text) {
                    return self.conversion(Ty::prim(*p), args, n);
                }
                if let Some(Named::Item(def)) = self.scope_name(&text) {
                    return self.call_item(def, &[], args, n, bang, want);
                }
                if self.is_poison_name(&text) {
                    return Ok(self.poison_value(n));
                }
                let msg = if text == "loop" && n.kind() == SyntaxKind::TrailingCallExpr {
                    "`loop` is not defined; write 'while true:' for an endless loop".to_owned()
                } else {
                    format!("`{text}` is not defined{}", self.name_hint(&text))
                };
                self.err(Code::UnknownName, callee, &msg);
                if text == "loop"
                    && !bang
                    && n.kind() == SyntaxKind::TrailingCallExpr
                    && self.cx.src.tkind(self.cx.src.first(callee)) == Some(TokenKind::Ident)
                {
                    self.diags.add_fix(
                        "replace `loop:` with `while true:`",
                        hd_diag::FixSafety::Exact,
                        &[(self.cx.src.span(callee), "while true")],
                    );
                }
                Ok((Ref(NONE), Ty::NEVER))
            }
            SyntaxKind::TypeArgsExpr => {
                let Some(inner) = callee.children().next() else {
                    return unsupported("a type-argument call shape");
                };
                // `f!::[T](..)`: the bang sits before the type arguments,
                // inside the type-argument node (`grammar.expr.bang-suffix`).
                let (inner, bang) = match inner.kind() {
                    SyntaxKind::SuspendExpr => match inner.children().next() {
                        Some(c) => (c, true),
                        None => return unsupported("a bang call shape"),
                    },
                    _ => (inner, bang),
                };
                let mut explicit = Vec::new();
                if let Some(tl) = Src::child(callee, SyntaxKind::TypeArgumentList) {
                    let gens = self.explicit_generics(inner);
                    explicit = self.explicit_targs(tl, &gens)?;
                }
                // `f::[T](..)` of a generic local function.
                if let Some(l) = self.local_generic_named(inner) {
                    let vars = self.local_generic_vars(l, &explicit, callee);
                    let (f, ft) = match &vars {
                        Some(v) => self.local_generic_inst(l, v, callee),
                        None => (Ref(NONE), Ty::NEVER),
                    };
                    return match self.local_forms.get(&l).cloned() {
                        Some(form) => {
                            self.call_value_form(f, ft, args, n, bang, Some((&form, vars)))
                        }
                        None => self.call_value(f, ft, args, n, bang),
                    };
                }
                let segs = self.segments_expr(inner);
                if segs.first().is_some_and(|s| self.is_poison_name(s)) {
                    return Ok(self.poison_value(n));
                }
                match self.resolve_path(&segs) {
                    Some(def)
                        if matches!(
                            self.cx.lookup.item(def).map(|i| &i.data),
                            Some(
                                ItemData::Fn(_)
                                    | ItemData::Data(_)
                                    | ItemData::Enum { .. }
                                    | ItemData::Newtype(_)
                                    | ItemData::Alias(_)
                            )
                        ) =>
                    {
                        self.call_item(def, &explicit, args, n, bang, want)
                    }
                    _ if matches!(inner.kind(), SyntaxKind::FieldExpr | SyntaxKind::PathExpr) => {
                        self.member_call(n, inner, args, bang, want, explicit)
                    }
                    _ => unsupported("a call through explicit type arguments of a type"),
                }
            }
            SyntaxKind::FieldExpr | SyntaxKind::PathExpr => {
                self.member_call(n, callee, args, bang, want, vec![])
            }
            _ => {
                let (f, ft) = self.expr(callee, None)?;
                self.call_value(f, ft, args, n, bang)
            }
        }
    }

    /// The type a newtype value wraps, with the newtype's arguments
    /// applied; `None` for a value of any other type.
    fn newtype_base(&self, t: Ty) -> Option<Ty> {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        let TyData::Adt { def, args } = pool.get(match pool.get(t) {
            TyData::Mut(i) => i,
            _ => t,
        }) else {
            return None;
        };
        let Some(ItemData::Newtype(inner)) = self.cx.lookup.item(def).map(|i| &i.data) else {
            return None;
        };
        Some(subst_owner(pool, def, pool.list_items(args), *inner))
    }

    /// The base type's constructor unwraps a newtype value `r` of type `t`
    /// (`types.newtype.construct`). A composite base keeps the newtype
    /// value's outer `mut`, which only a `mut Suspend[T]` base can have.
    fn unwrap_newtype(
        &mut self,
        r: Ref,
        t: Ty,
        to: Ty,
        arg: NodeRef<'_>,
        n: NodeRef<'_>,
    ) -> Option<(Ref, Ty)> {
        let inner = self.newtype_base(t)?;
        if !self.can_unify(inner, to) {
            return None;
        }
        self.expect(inner, to, arg, "conversion", ExpectPosition::General);
        let result = if self.is_composite(to) && self.has_mut_access(t) {
            self.pool().intern_ty(&TyData::Mut(to))
        } else {
            to
        };
        Some((self.b.field(r, 0, result, n.index()), result))
    }

    /// A call of the data type, enum or alias `name`, which expands to
    /// `to`. Only the unwrapping of a newtype is a call of a type name
    /// (`types.newtype.construct`); any other is a type used as a value
    /// (`names.type-as-value`).
    fn type_name_call(
        &mut self,
        name: &str,
        to: Ty,
        args: &Args<'_>,
        n: NodeRef<'_>,
    ) -> StageResult<(Ref, Ty)> {
        if let [e] = args.positional.as_slice()
            && args.named.is_empty()
            && args.spread.is_none()
            && args.trailing.is_none()
        {
            let (r, t) = self.expr(*e, None)?;
            if let Some(v) = self.unwrap_newtype(r, t, to, *e, n) {
                return Ok(v);
            }
            if matches!(self.infer.resolve(self.pool(), t), Ty::POISON | Ty::NEVER) {
                return Ok((Ref(NONE), self.infer.resolve(self.pool(), t)));
            }
        }
        let msg = format!("`{name}` is a type, not a value");
        self.err(Code::TypeUsedAsValue, n, &msg);
        Ok((Ref(NONE), Ty::POISON))
    }

    /// Whether `t` is a type the numeric cast table lists: an integer or
    /// floating-point type, a numeric literal, or a numeric type parameter.
    /// `char` and `bool` are not in the table.
    fn cast_numeric(&self, t: Ty) -> bool {
        let pool = self.pool();
        let t = self.infer.shallow(pool, t);
        match pool.get(t) {
            TyData::Prim(p) => p.is_integer() || p.is_float(),
            TyData::Mut(i) => self.cast_numeric(i),
            _ => self.numeric(t),
        }
    }

    /// `i64(x)`: a numeric conversion.
    fn conversion(&mut self, to: Ty, args: &Args<'_>, n: NodeRef<'_>) -> StageResult<(Ref, Ty)> {
        plain_args(args, "a conversion")?;
        let [e] = args.positional.as_slice() else {
            return unsupported("a conversion shape");
        };
        let (r, t) = self.expr(*e, None)?;
        // A newtype converts to the type it wraps (`string(path)`).
        if let Some(v) = self.unwrap_newtype(r, t, to, *e, n) {
            return Ok(v);
        }
        if let Some(inner) = self.newtype_base(t)
            && self.cast_numeric(inner)
            && self.cast_numeric(to)
        {
            let v = self.b.field(r, 0, inner, n.index());
            return Ok((self.b.prim(PrimOp::Conv as u32, &[v], to, n.index()), to));
        }
        // Only a numeric-to-numeric call is a cast (`types.cast.nonnumeric`);
        // any other constructor-style call of a primitive type is a call of
        // something that is not a function.
        if !self.cast_numeric(t) || !self.cast_numeric(to) {
            let msg = format!(
                "`{}` is not a function: only numeric types convert by a constructor-style call",
                self.show(to)
            );
            self.err(Code::NotCallable, n, &msg);
            return Ok((Ref(NONE), Ty::POISON));
        }
        // A literal argument takes the target's type directly.
        if self.b.const_of(r).is_some()
            && matches!(
                self.infer.kind_of(self.pool(), t),
                Some(VarKind::IntLit | VarKind::SignedIntLit | VarKind::FloatLit)
            )
            && self.can_unify(t, to)
        {
            self.expect(t, to, *e, "conversion", ExpectPosition::General);
            return Ok((r, to));
        }
        Ok((self.b.prim(PrimOp::Conv as u32, &[r], to, n.index()), to))
    }

    /// A call of a function value.
    pub(crate) fn call_value(
        &mut self,
        f: Ref,
        ft: Ty,
        args: &Args<'_>,
        n: NodeRef<'_>,
        bang: bool,
    ) -> StageResult<(Ref, Ty)> {
        self.call_value_form(f, ft, args, n, bang, None)
    }

    /// `fn.trait.mut.call`: a call of a value whose bound is `FnMut` only
    /// changes the value, so the callee must be a writable place (a `let
    /// mut` binding, a `mut` parameter, a field reached through `&mut`) or
    /// a temporary. The place's binding counts as changed.
    fn fn_mut_callee(&mut self, f: Ref, n: NodeRef<'_>) {
        if self.is_fresh(f) || self.place_writable_quiet(f) {
            if let Some(l) = self.place_root_local(f) {
                self.changed_locals.push(l);
            }
            return;
        }
        self.err(
            Code::NotWritable,
            n,
            "calling an `FnMut` value may change it, so the callee must be writable: a `let mut` binding, a `mut` parameter, or a field reached through `&mut`",
        );
    }

    /// The concrete function type exposed by a value's `Fn[I, O, $R]` or
    /// `FnMut[I, O, $R]` bound, and whether only `FnMut` holds. The solver
    /// teaches the three fresh arguments from either a parameter's
    /// environment clause or a concrete function type's compiler-supplied
    /// implementation.
    fn fn_bound_type(&mut self, self_ty: Ty) -> StageResult<Option<(Ty, bool)>> {
        let pool = self.pool();
        let mut args: Vec<Ty> = (0..3)
            .map(|_| self.infer.fresh(pool, VarKind::General))
            .collect();
        let mut only_mut = false;
        let mut answer = Answer::OutOfFuel;
        for trait_ in [
            self.cx.names.known.fn_trait,
            self.cx.names.known.fn_mut_trait,
        ] {
            let tref = TraitRef {
                trait_,
                self_ty,
                args: pool.list(&args),
            };
            answer = self.solve(tref)?;
            if matches!(answer, Answer::Holds { .. }) {
                break;
            }
            only_mut = true;
        }
        let Answer::Holds { learned, .. } = answer else {
            return Ok(None);
        };
        for (v, t) in learned {
            let var = pool.intern_ty(&TyData::Infer(v));
            let _ = self.infer.unify(pool, var, t);
        }
        for t in &mut args {
            *t = self.infer.resolve(pool, *t);
        }
        let [input, output, row] = args.as_slice() else {
            return Ok(None);
        };
        let TyData::Row(row) = pool.get(*row) else {
            return Ok(None);
        };
        let ft = pool.intern_ty(&TyData::Fn {
            params: pool.list(&[*input]),
            result: *output,
            row,
            suspends: false,
            inputs: Inputs::Tuple,
            deps: hd_types::FnDeps::NONE,
        });
        Ok(Some((ft, only_mut)))
    }

    /// A call of a function value, or of a local function by its name
    /// (`form`, with its type arguments if generic): the call then takes
    /// named arguments and omits parameters that have defaults.
    pub(crate) fn call_value_form(
        &mut self,
        f: Ref,
        ft: Ty,
        args: &Args<'_>,
        n: NodeRef<'_>,
        bang: bool,
        form: Option<(&crate::local::LocalForm, Option<Vec<Ty>>)>,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
        // A callee that already failed has been reported.
        if f.0 == NONE && ft == Ty::NEVER {
            return Ok((f, ft));
        }
        // A callee of a type that failed to resolve has been reported; its
        // arguments are still checked.
        if self.infer.resolve(pool, ft) == Ty::POISON {
            for e in &args.positional {
                self.expr(*e, Some(Ty::POISON))?;
            }
            return Ok((Ref(NONE), Ty::POISON));
        }
        let full = ft;
        let ft = self.infer.resolve(pool, ft);
        let mut ft = match pool.get(ft) {
            TyData::Mut(i) => i,
            _ => ft,
        };
        // `s!()` on a stored suspension: a suspension point.
        let suspend = self.cx.names.known.suspend;
        if let TyData::Adt { def, args: sa } = pool.get(ft)
            && def == suspend
            && bang
            && args.positional.is_empty()
            && args.trailing.is_none()
        {
            // Driving a suspension advances it (`mut self`).
            if !self.has_mut_access(full) {
                self.err(
                    Code::NotWritable,
                    n,
                    "driving a suspension needs `mut` access to it",
                );
            }
            let t = pool.list_items(sa).first().copied().unwrap_or(Ty::POISON);
            self.check_bang(true, n);
            return Ok((self.b.emit(Tag::AwaitValue, f.0, NONE, t, n.index()), t));
        }
        if !matches!(pool.get(ft), TyData::Fn { .. })
            && let Some((bound, only_mut)) = self.fn_bound_type(ft)?
        {
            if only_mut {
                self.fn_mut_callee(f, n);
            }
            ft = bound;
        }
        let TyData::Fn {
            params,
            result,
            row,
            suspends,
            inputs,
            deps: _,
        } = pool.get(ft)
        else {
            if matches!(pool.get(ft), TyData::Infer(_)) {
                return unsupported("a call of a value whose type is not yet known");
            }
            if !bang && self.is_applicable(ft)? {
                return self.apply_call((f, full), args, n);
            }
            let msg = format!("{} is not a function", self.show(ft));
            self.err(Code::NotCallable, n, &msg);
            return Ok((Ref(NONE), Ty::NEVER));
        };
        if let Some((_, Some(vars))) = &form {
            let explicit = n
                .children()
                .next()
                .filter(|callee| callee.kind() == SyntaxKind::TypeArgsExpr)
                .and_then(|callee| Src::child(callee, SyntaxKind::TypeArgumentList))
                .map_or(0, |list| {
                    list.children().filter(|arg| arg.kind().is_type()).count()
                });
            self.local_ref_call_targs
                .push((n.index(), vars.clone(), explicit));
        }
        let form = form.filter(|_| inputs == Inputs::Fixed);
        if !args.named.is_empty() && form.is_none() {
            // A function value has no parameter names (`fn.ref.value`), so
            // every named argument names no parameter (`fn.arg.unknown-name`).
            for (pname, e) in &args.named {
                let msg = format!(
                    "a function value has no parameter names, so `{pname}=` names none; pass it by position"
                );
                self.err(Code::UnknownNamedArgument, *e, &msg);
            }
            return Ok((Ref(NONE), Ty::NEVER));
        }
        let ps = pool.list_items(params);
        let refs = if let Some((form, vars)) = form {
            let formals: Vec<(Symbol, Ty)> =
                form.names.iter().copied().zip(ps.iter().copied()).collect();
            let defaults: Vec<bool> = form.defaults.iter().map(Option::is_some).collect();
            // Reached only through explicit type arguments: every formal is
            // fixed, and the join check stays off.
            let inferred: Vec<Vec<usize>> = vec![Vec::new(); formals.len()];
            let formals = Formals {
                params: &formals,
                defaults: &defaults,
                variadic: false,
                inferred: &inferred,
                closure_wants: None,
            };
            self.local_defaults = Some(crate::local::LocalDefaults {
                defaults: form.defaults.clone(),
                vars,
            });
            self.check_args(&formals, 0, args, n, &form.name)?
        } else if inputs == Inputs::Tuple {
            match self.tuple_inputs_args(ft, ps[0], args, n)? {
                Some(refs) => refs,
                None => return Ok((Ref(NONE), Ty::NEVER)),
            }
        } else if inputs == Inputs::Rest {
            // The inputs end in a rest element: the arguments are
            // collected as a vararg call collects them
            // (`fn.type.rest-call`).
            let formals: Vec<(Symbol, Ty)> = ps
                .iter()
                .enumerate()
                .map(|(i, t)| (self.cx.names.syms.intern(&format!("${i}")), *t))
                .collect();
            let defaults = vec![false; ps.len()];
            // Function values keep today's behavior: without a declaration
            // there is no explicit/inferred split to read.
            let inferred: Vec<Vec<usize>> = vec![Vec::new(); formals.len()];
            let formals = Formals {
                params: &formals,
                defaults: &defaults,
                variadic: true,
                inferred: &inferred,
                closure_wants: None,
            };
            self.check_args(&formals, 0, args, n, "the function value")?
        } else if args.spread.is_some() {
            self.spread_value_args(ps, args, n)?
        } else {
            // A trailing block fills the final parameter, so it counts
            // as an argument only when the callee can take it.
            let takes =
                args.trailing.is_some() && self.trailing_takes(ps.last().copied()).is_some();
            let given = args.positional.len() + usize::from(takes);
            if ps.len() != given {
                let msg = format!("the function takes {} arguments", ps.len());
                self.err(Code::ArgumentCount, n, &msg);
            }
            let mut refs = Vec::new();
            for (i, e) in args.positional.iter().enumerate() {
                let w = ps.get(i).copied();
                let (r, t) = self.arg_value(*e, w, true)?;
                let r = match w {
                    Some(w) => self.coerce(r, t, w, *e, "argument", ExpectPosition::General),
                    None => r,
                };
                refs.push(r);
            }
            if let Some(block) = args.trailing {
                // A block the callee cannot take is still checked, against
                // the poison type, but fills nothing.
                let w = self.trailing_param(
                    if takes { ps.last().copied() } else { None },
                    "the function value",
                    n,
                );
                let (r, t) = self.trailing_closure(block, w, takes)?;
                if takes {
                    refs.push(self.coerce(r, t, w, block, "argument", ExpectPosition::General));
                }
            }
            refs
        };
        self.check_row(row, n);
        let rec = self.b.refs_record(&refs);
        if !suspends {
            if bang {
                self.check_bang(false, n);
            }
            return Ok((
                self.b.emit(Tag::CallValue, f.0, rec, result, n.index()),
                result,
            ));
        }
        // A suspending function value: a plain call is cold, a bang call
        // awaits it.
        if !bang && self.bare_step {
            return Ok(self.suspending_pipe_step(n));
        }
        let st = pool.intern_ty(&TyData::Mut(pool.intern_ty(&TyData::Adt {
            def: suspend,
            args: pool.list(&[result]),
        })));
        let cold = self.b.emit(Tag::CallValue, f.0, rec, st, n.index());
        if bang {
            self.check_bang(true, n);
            return Ok((
                self.b
                    .emit(Tag::AwaitValue, cold.0, NONE, result, n.index()),
                result,
            ));
        }
        Ok((cold, st))
    }

    /// The arguments of a call through a function value whose inputs are
    /// the tuple `input`, of an arity not known here, as in
    /// `f: Fn[Args, O, $ R]`: one positional spread of that same type
    /// (`expr.call.spread.inputs`). Where its type then solves `input` as
    /// a tuple, the spread fills each parameter with one element; else
    /// the call passes the tuple, which instantiation spreads.
    fn tuple_inputs_args(
        &mut self,
        ft: Ty,
        input: Ty,
        args: &Args<'_>,
        n: NodeRef<'_>,
    ) -> StageResult<Option<Vec<Ref>>> {
        let pool = self.pool();
        let spread = match args.spread {
            Some(s)
                if args.positional.is_empty()
                    && args.after_spread.is_empty()
                    && args.trailing.is_none() =>
            {
                s
            }
            _ => {
                let msg = format!(
                    "{} takes its inputs `{}` as one spread, as in `f(args...)`: their arity is not known here",
                    self.show(ft),
                    self.show(input)
                );
                self.err(Code::TypeMismatch, n, &msg);
                let given = args.positional.iter().chain(&args.spread);
                for e in given.chain(&args.after_spread) {
                    self.expr(*e, None)?;
                }
                return Ok(None);
            }
        };
        let (r, t) = self.arg_value(spread, Some(input), false)?;
        let r = self.coerce(r, t, input, spread, "argument", ExpectPosition::General);
        let solved = self.infer.resolve(pool, input);
        match pool.get(solved) {
            TyData::Tuple { elems, rest } => {
                let mut refs = Vec::new();
                for (k, et) in pool
                    .list_items(elems)
                    .iter()
                    .copied()
                    .chain(rest)
                    .enumerate()
                {
                    let idx = u32::try_from(k).unwrap_or(0);
                    refs.push(self.b.field(r, idx, et, spread.index()));
                }
                Ok(Some(refs))
            }
            TyData::Infer(_) => unsupported("a call of a value whose inputs are not yet known"),
            _ => Ok(Some(vec![r])),
        }
    }

    /// The final parameter a trailing block fills, when it is a
    /// zero-argument function type (`fn.trailing.form`,
    /// `grammar.call.trailing-block.eligible`).
    fn trailing_takes(&self, last: Option<Ty>) -> Option<Ty> {
        let pool = self.pool();
        let t = last?;
        let r = self.infer.resolve(pool, t);
        if let TyData::Fn { params, .. } = pool.get(self.strip_mut(r))
            && pool.list_items(params).is_empty()
        {
            Some(t)
        } else {
            None
        }
    }

    /// The type of the final parameter `last` that a trailing block fills
    /// (see [`Self::trailing_takes`]). Any other callee is
    /// `trailing-block-ineligible`, reported at the call; checking
    /// continues against the poison type.
    fn trailing_param(&mut self, last: Option<Ty>, name: &str, n: NodeRef<'_>) -> Ty {
        if let Some(t) = self.trailing_takes(last) {
            return t;
        }
        let msg = format!(
            "a trailing block for `{name}`, whose final parameter is not a zero-argument function"
        );
        self.err(Code::TrailingBlockIneligible, n, &msg);
        Ty::POISON
    }

    /// Each named argument that names a parameter of `rest`, with that
    /// parameter's type.
    fn named_params<'t>(
        &self,
        rest: &[(Symbol, Ty)],
        named: &[(String, NodeRef<'t>)],
    ) -> Vec<(NodeRef<'t>, Ty)> {
        named
            .iter()
            .filter_map(|(pname, e)| {
                rest.iter()
                    .find(|p| self.cx.names.text(p.0) == pname.as_str())
                    .map(|p| (*e, p.1))
            })
            .collect()
    }

    /// Checks arguments against parameters (positional, then named by
    /// parameter name), skipping `skip` leading parameters already given.
    fn check_args(
        &mut self,
        formals: &Formals<'_>,
        skip: usize,
        args: &Args<'_>,
        n: NodeRef<'_>,
        name: &str,
    ) -> StageResult<Vec<Ref>> {
        // The callee's default bodies, taken before any argument is checked.
        let owner = self.default_owner.take();
        let local = self.local_defaults.take();
        let rest = formals.params.get(skip..).unwrap_or(&[]);
        let variadic = formals.variadic && !rest.is_empty();
        if variadic && args.trailing.is_some() {
            return unsupported("a trailing block for a vararg function");
        }
        // A vararg is the final parameter; the fixed ones come before it.
        let nfixed = rest.len() - usize::from(variadic);
        let mut slots: Vec<Option<Ref>> = vec![None; rest.len()];
        // The first argument type seen for each inferred generic: later
        // arguments for the same generic must meet it by the join.
        let mut join_first: HashMap<usize, Ty> = HashMap::default();
        let join_mark = self.diags.mark();
        // A trailing block supplies the final parameter, so positional
        // arguments fill the ones before it (`fn.trailing.arguments`).
        // A block the callee cannot take fills nothing.
        let takes = args.trailing.is_some()
            && !rest.is_empty()
            && self.trailing_takes(rest.last().map(|p| p.1)).is_some();
        let room = rest.len() - usize::from(takes);
        if !variadic && args.positional.len() > room {
            let msg = format!("`{name}` takes {} arguments", rest.len());
            self.err(Code::ArgumentCount, n, &msg);
        }
        // Bare variable parameters still open: several arguments solving
        // one join at the readonly view (types.generic.infer.join).
        let pool = self.pool();
        let open: Vec<bool> = rest
            .iter()
            .map(|p| {
                matches!(pool.get(p.1), TyData::Infer(_))
                    && matches!(pool.get(self.infer.shallow(pool, p.1)), TyData::Infer(_))
            })
            .collect();
        // The separate arguments a vararg collects, with the types each
        // contributes to a tuple (`fn.vararg.collect`).
        let mut items: Vec<(Ref, Ty)> = Vec::new();
        let mut item_wants = Vec::new();
        for (i, e) in args.positional.iter().enumerate() {
            if variadic && i >= nfixed {
                // Asked once the fixed arguments have solved what they can.
                if i == nfixed {
                    item_wants =
                        self.vararg_wants(rest[nfixed].1, args.positional.len() - nfixed)?;
                }
                let w = item_wants[i - nfixed];
                let (r, t) = self.arg_value(*e, w, false)?;
                let r = match w {
                    Some(w) => self.coerce(r, t, w, *e, "argument", ExpectPosition::General),
                    None => r,
                };
                items.push((r, w.unwrap_or(t)));
                continue;
            }
            let mut w = match rest.get(i) {
                Some(p) => Some(self.normalize_deep(p.1)?),
                None => None,
            };
            // An earlier argument solved this bare parameter: the two join
            // (types.generic.infer.join).
            let rejoin = open.get(i).copied().unwrap_or(false)
                && w.is_some_and(|w| {
                    !matches!(pool.get(self.infer.shallow(pool, w)), TyData::Infer(_))
                });
            // A closure whose parameter types a later argument solves is
            // checked against them (§2.4 step 4).
            let mut bounded = None;
            let expect = match w {
                Some(want) if e.kind() == SyntaxKind::ClosureExpr => {
                    bounded = formals
                        .closure_wants
                        .and_then(|xs| xs.get(skip + i))
                        .copied()
                        .flatten();
                    let later: Vec<(NodeRef<'_>, Ty)> = args.positional[i + 1..]
                        .iter()
                        .zip(rest.get(i + 1..nfixed).unwrap_or(&[]))
                        .map(|(a, p)| (*a, p.1))
                        .chain(self.named_params(rest, &args.named))
                        .collect();
                    bounded
                        .or_else(|| {
                            self.open_inputs(want)
                                .then(|| self.solved_by_later(want, &later))
                                .flatten()
                        })
                        .or(w)
                }
                _ => w,
            };
            let scoped_capture = e.kind() == SyntaxKind::ClosureExpr
                && bounded.is_none()
                && formals.inferred.get(skip + i).is_none_or(Vec::is_empty);
            let (r, t) = self.arg_value(*e, expect, scoped_capture)?;
            if let Some(p) = rest.get(i)
                && open.get(i).copied().unwrap_or(false)
                && self.infer.resolve(pool, t) == Ty::NEVER
                && !self.diags.has_errors()
            {
                self.never_args.push(p.1);
            }
            if let Some(p) = rest.get(i)
                && open.get(i).copied().unwrap_or(false)
                && self.join_down(p.1, t)
            {
                w = Some(self.normalize_deep(p.1)?);
            }
            let what = if rest
                .get(i)
                .is_some_and(|p| matches!(pool.get(p.1), TyData::Infer(_)))
            {
                "inferred argument"
            } else {
                "argument"
            };
            let r = match w {
                Some(w) if rejoin && self.infer_join_conflict(w, t, *e) => r,
                Some(w) => {
                    if join_arg(
                        self,
                        formals.inferred.get(skip + i).map(Vec::as_slice),
                        &mut join_first,
                        join_mark,
                        t,
                        *e,
                        what,
                    ) {
                        r
                    } else {
                        self.coerce(r, t, w, *e, what, ExpectPosition::General)
                    }
                }
                None => r,
            };
            if i < room
                && let Some(s) = slots.get_mut(i)
            {
                *s = Some(r);
            }
        }
        // A list spread that ends a rest tuple vararg's arguments is its
        // rest list, so the arguments wait for it.
        let tail_pending = args.spread.is_some()
            && variadic
            && !items.is_empty()
            && self.rest_of(rest[nfixed].1)?.is_some();
        if variadic && !items.is_empty() && !tail_pending {
            slots[nfixed] = Some(self.pack_vararg(&items, rest[nfixed].1, n)?);
        }
        if let Some(s) = args.spread {
            let tys: Vec<Ty> = rest.iter().map(|p| p.1).collect();
            let named: Vec<bool> = rest
                .iter()
                .map(|p| {
                    let text = self.cx.names.text(p.0);
                    args.named.iter().any(|(nm, _)| nm == text)
                })
                .collect();
            let at = SpreadAt {
                variadic,
                given: args.positional.len(),
                named: &named,
                items: if tail_pending { &items } else { &[] },
            };
            self.spread_arg(s, &tys, at, &mut slots, args)?;
        }
        // A bad named argument already put the call in error, so a slot
        // it was meant to fill is not also an arity error.
        let mut named_bad = false;
        for (k, (pname, e)) in args.named.iter().enumerate() {
            let Some(i) = rest
                .iter()
                .position(|p| self.cx.names.text(p.0) == pname.as_str())
            else {
                let msg = format!("`{name}` has no parameter `{pname}`");
                self.err(Code::UnknownNamedArgument, *e, &msg);
                named_bad = true;
                continue;
            };
            if slots[i].is_some() {
                let msg = format!("`{pname}` is given twice");
                self.err(Code::DuplicateArgument, *e, &msg);
                named_bad = true;
                continue;
            }
            let mut w = self.normalize_deep(rest[i].1)?;
            let rejoin =
                open[i] && !matches!(pool.get(self.infer.shallow(pool, w)), TyData::Infer(_));
            let mut bounded = None;
            let expect = if e.kind() == SyntaxKind::ClosureExpr {
                bounded = formals
                    .closure_wants
                    .and_then(|xs| xs.get(skip + i))
                    .copied()
                    .flatten();
                let later = self.named_params(rest, &args.named[k + 1..]);
                bounded
                    .or_else(|| {
                        self.open_inputs(w)
                            .then(|| self.solved_by_later(w, &later))
                            .flatten()
                    })
                    .unwrap_or(w)
            } else {
                w
            };
            let scoped_capture = e.kind() == SyntaxKind::ClosureExpr
                && bounded.is_none()
                && formals.inferred.get(skip + i).is_none_or(Vec::is_empty);
            let (r, t) = self.arg_value(*e, Some(expect), scoped_capture)?;
            if open[i] && self.infer.resolve(pool, t) == Ty::NEVER && !self.diags.has_errors() {
                self.never_args.push(rest[i].1);
            }
            if open[i] && self.join_down(rest[i].1, t) {
                w = self.normalize_deep(rest[i].1)?;
            }
            if rejoin && self.infer_join_conflict(w, t, *e) {
                slots[i] = Some(r);
                continue;
            }
            let what2 = if matches!(pool.get(rest[i].1), TyData::Infer(_)) {
                "inferred argument"
            } else {
                "argument"
            };
            if join_arg(
                self,
                formals.inferred.get(skip + i).map(Vec::as_slice),
                &mut join_first,
                join_mark,
                t,
                *e,
                what2,
            ) {
                slots[i] = Some(r);
                continue;
            }
            let r = self.coerce(r, t, w, *e, what2, ExpectPosition::General);
            slots[i] = Some(r);
        }
        if let Some(block) = args.trailing {
            // A block the callee cannot take is still checked, against
            // the poison type, but fills nothing (and cannot collide
            // with a positional argument).
            let takes = self.trailing_takes(rest.last().map(|p| p.1)).is_some();
            if takes {
                let w = match rest.last() {
                    Some(p) => Some(self.normalize_deep(p.1)?),
                    None => None,
                };
                let w = self.trailing_param(w, name, block);
                let last = rest.len() - 1;
                if slots[last].is_some() {
                    let pname = self.cx.names.text(rest[last].0).to_owned();
                    let msg = format!("`{pname}` is given twice");
                    self.err(Code::DuplicateArgument, block, &msg);
                } else {
                    let last_is_generic = formals
                        .inferred
                        .get(skip + last)
                        .is_some_and(|xs| !xs.is_empty())
                        || formals
                            .closure_wants
                            .and_then(|xs| xs.get(skip + last))
                            .is_some_and(Option::is_some);
                    let (r, t) = self.trailing_closure(block, w, !last_is_generic)?;
                    slots[last] =
                        Some(self.coerce(r, t, w, block, "argument", ExpectPosition::General));
                }
            } else {
                let w = self.trailing_param(None, name, n);
                self.trailing_closure(block, w, false)?;
            }
        }
        // A vararg nothing supplied collects no arguments.
        if variadic && slots[nfixed].is_none() {
            slots[nfixed] = Some(self.pack_vararg(&[], rest[nfixed].1, n)?);
        }
        let mut out = Vec::new();
        for (i, s) in slots.into_iter().enumerate() {
            match s {
                Some(r) => out.push(r),
                None if formals.defaults.get(skip + i).copied().unwrap_or(false) => {
                    // A local function's default is a closure of the
                    // earlier values, called after every argument.
                    if let Some(ld) = &local {
                        let Some(Some(hidden)) = ld.defaults.get(skip + i) else {
                            return unsupported("a default argument of this callee");
                        };
                        let Some((l, d)) = self.find_local(*hidden) else {
                            return unsupported("a default argument of this callee");
                        };
                        let (c, _) = match &ld.vars {
                            Some(v) => self.local_generic_inst(l, v, n),
                            None => self.read_local(l, d, n),
                        };
                        let t = self.normalize_deep(rest[i].1)?;
                        let earlier = self.b.refs_record(&out);
                        out.push(self.b.emit(Tag::CallValue, c.0, earlier, t, n.index()));
                        continue;
                    }
                    // `DefaultCall` of the parameter's default body, after
                    // every explicit argument, over the earlier values
                    // (`crates/hd_tir/DESIGN.md` "Default calls").
                    let Some((def, targs, prefix)) = &owner else {
                        return unsupported("a default argument of this callee");
                    };
                    let pname = self.cx.names.text(rest[i].0).to_owned();
                    let body = self.cx.names.member(*def, PathKind::Hidden, &pname);
                    let a = self.b.refs_record(&[Ref(body.raw()), Ref(targs.0)]);
                    let mut earlier = prefix.clone();
                    earlier.extend(&out);
                    let bw = self.b.refs_record(&earlier);
                    let t = self.normalize_deep(rest[i].1)?;
                    out.push(self.b.emit(Tag::DefaultCall, a, bw, t, n.index()));
                }
                None => {
                    if !named_bad {
                        let msg = format!("`{name}` takes {} arguments", rest.len());
                        self.err(Code::ArgumentCount, n, &msg);
                    }
                    break;
                }
            }
        }
        Ok(out)
    }

    /// The rest list type of a vararg type that is a tuple with a rest
    /// element.
    fn rest_of(&mut self, lt: Ty) -> StageResult<Option<Ty>> {
        let pool = self.pool();
        let lt = self.normalize_deep(lt)?;
        Ok(match pool.get(self.strip_mut(lt)) {
            TyData::Tuple { rest, .. } => rest,
            _ => None,
        })
    }

    /// The expected type of each of `count` separate arguments a vararg of
    /// type `lt` collects: the list's element, or the tuple's elements
    /// when the type is already a tuple of that many.
    fn vararg_wants(&mut self, lt: Ty, count: usize) -> StageResult<Vec<Option<Ty>>> {
        let pool = self.pool();
        let lt = self.normalize_deep(lt)?;
        let list = self.cx.names.known.list;
        Ok(match pool.get(self.strip_mut(lt)) {
            TyData::Adt { def, args } if def == list => {
                let et = pool.list_items(args).first().copied().unwrap_or(Ty::POISON);
                vec![Some(et); count]
            }
            TyData::Tuple { elems, rest: None } if pool.list_items(elems).len() == count => {
                pool.list_items(elems).iter().copied().map(Some).collect()
            }
            // The fixed elements one each, then the rest's item type
            // (`expr.tuple.rest.collect`).
            TyData::Tuple {
                elems,
                rest: Some(rest),
            } => {
                let item = self.list_item(rest);
                let fixed = pool.list_items(elems).iter().copied().map(Some);
                fixed.chain(std::iter::repeat(item)).take(count).collect()
            }
            _ => vec![None; count],
        })
    }

    /// The collected value of a vararg of type `lt` from separate
    /// arguments: a list, or the tuple expression of them
    /// (`fn.vararg.collect.list`, `fn.vararg.collect.tuple-expr`,
    /// `fn.vararg.tuple-param.infer`).
    fn pack_vararg(&mut self, items: &[(Ref, Ty)], lt: Ty, n: NodeRef<'_>) -> StageResult<Ref> {
        let pool = self.pool();
        let lt = self.normalize_deep(lt)?;
        let list = self.cx.names.known.list;
        let refs: Vec<Ref> = items.iter().map(|i| i.0).collect();
        let shape = pool.get(self.strip_mut(lt));
        if matches!(shape, TyData::Adt { def, .. } if def == list) {
            let rec = self.b.refs_record(&refs);
            return Ok(self.b.emit(Tag::NewList, NONE, rec, lt, n.index()));
        }
        if matches!(shape, TyData::Tuple { rest: Some(_), .. }) {
            let stripped = self.strip_mut(lt);
            let refs: Vec<Ref> = items.iter().map(|i| i.0).collect();
            return Ok(self.rest_tuple_value(stripped, &refs, None, n));
        }
        let tys: Vec<Ty> = items.iter().map(|i| i.1).collect();
        let tt = pool.intern_ty(&TyData::Tuple {
            elems: pool.list(&tys),
            rest: None,
        });
        self.expect(tt, lt, n, "argument", ExpectPosition::General);
        let rec = self.b.refs_record(&refs);
        Ok(self.b.emit(Tag::NewTuple, NONE, rec, tt, n.index()))
    }

    /// The positional spread `e...` of a call (`expr.call.spread.fills`).
    /// `tys` are the parameters not yet given, `at` where the spread
    /// stands; the spread fills `slots`.
    fn spread_arg(
        &mut self,
        e: NodeRef<'_>,
        tys: &[Ty],
        at: SpreadAt,
        slots: &mut [Option<Ref>],
        args: &Args<'_>,
    ) -> StageResult<()> {
        let pool = self.pool();
        let SpreadAt {
            variadic,
            given,
            named,
            items,
        } = at;
        let nfixed = tys.len() - usize::from(variadic);
        // The parameters the spread fills: those after the given ones that
        // no named argument supplies (`expr.call.spread.fills`).
        let free: Vec<usize> = (given..tys.len())
            .filter(|&i| !named.get(i).copied().unwrap_or(false))
            .collect();
        let at_vararg = variadic && (given >= nfixed || free.first() == Some(&nfixed));
        // A positional after the spread is a second supply of a vararg, or
        // else the spread is not final (`expr.call.spread.position`,
        // `expr.call.spread.vararg-duplicate`).
        let mut failed = false;
        if let Some(late) = args.after_spread.first() {
            let (code, msg) = if at_vararg {
                (
                    Code::DuplicateArgument,
                    "the vararg is given by the spread and by a value",
                )
            } else {
                (
                    Code::NonfinalPositionalSpread,
                    "a positional spread must be the final positional argument",
                )
            };
            self.err(code, *late, msg);
            failed = true;
        }
        let lt = if at_vararg {
            Some(self.normalize_deep(tys[nfixed])?)
        } else {
            None
        };
        // After separate arguments, the spread is the rest list.
        let tail_ty = match lt {
            Some(lt) if !items.is_empty() => self.rest_of(lt)?,
            _ => None,
        };
        let (r, t) = self.arg_value(e, tail_ty.or(lt), false)?;
        let poison = |slots: &mut [Option<Ref>], free: &[usize]| {
            for &i in free {
                if let Some(s) = slots.get_mut(i) {
                    s.get_or_insert(Ref(NONE));
                }
            }
        };
        if failed {
            poison(slots, &free);
            return Ok(());
        }
        let t = self.infer.resolve(pool, t);
        if matches!(self.strip_mut(t), Ty::NEVER | Ty::POISON) {
            poison(slots, &free);
            return Ok(());
        }
        // At the vararg: the operand is its collected value
        // (`expr.call.spread.at-vararg`), passed through without a copy.
        if let Some(lt) = lt {
            if let Some(rest) = tail_ty {
                let tail = self.coerce(r, t, rest, e, "argument", ExpectPosition::General);
                let refs: Vec<Ref> = items.iter().map(|i| i.0).collect();
                let tuple = self.strip_mut(lt);
                slots[nfixed] = Some(self.rest_tuple_value(tuple, &refs, Some(tail), e));
                return Ok(());
            }
            if slots[nfixed].is_some() {
                // Separate arguments already supplied it.
                let list = self.cx.names.known.list;
                if matches!(pool.get(self.strip_mut(lt)), TyData::Adt { def, .. } if def == list) {
                    self.err(
                        Code::DuplicateArgument,
                        e,
                        "the vararg is given by separate values and by a spread",
                    );
                    return Ok(());
                }
                self.err(
                    Code::TypeMismatch,
                    e,
                    "in spread: the vararg's tuple type has no rest element for a list",
                );
                return Ok(());
            }
            slots[nfixed] = Some(self.coerce(r, t, lt, e, "argument", ExpectPosition::General));
            return Ok(());
        }
        // Before fixed parameters: a tuple of the remaining inputs
        // (`expr.call.spread.inputs`).
        let list = self.cx.names.known.list;
        match pool.get(self.strip_mut(t)) {
            TyData::Tuple { elems, rest } => {
                let elems = pool.list_items(elems);
                // A rest element fills the vararg; a tuple without one
                // fills the remaining inputs exactly (`expr.call.spread.inputs`).
                let fits = if rest.is_some() {
                    variadic && elems.len() + 1 == free.len() && free.last() == Some(&nfixed)
                } else {
                    !variadic && elems.len() == free.len()
                };
                if !fits {
                    let msg = format!(
                        "in spread: the remaining inputs are {} values, found {}",
                        free.len(),
                        self.show(t)
                    );
                    self.err(Code::TypeMismatch, e, &msg);
                    poison(slots, &free);
                    return Ok(());
                }
                for (k, (&et, &i)) in elems.iter().zip(&free).enumerate() {
                    let idx = u32::try_from(k).unwrap_or(0);
                    let g = self.b.field(r, idx, et, e.index());
                    let w = self.normalize_deep(tys[i])?;
                    slots[i] = Some(self.coerce(g, et, w, e, "argument", ExpectPosition::General));
                }
                if let Some(rest) = rest {
                    let idx = u32::try_from(elems.len()).unwrap_or(0);
                    let g = self.b.field(r, idx, rest, e.index());
                    let w = self.normalize_deep(tys[nfixed])?;
                    slots[nfixed] =
                        Some(self.coerce(g, rest, w, e, "argument", ExpectPosition::General));
                }
            }
            TyData::Adt { def, .. } if def == list => {
                self.err(
                    Code::PositionalSpreadNeedsVararg,
                    e,
                    "a list spreads only at a vararg parameter",
                );
                poison(slots, &free);
            }
            TyData::Infer(_) => {
                return unsupported("a spread of a value whose type is not yet known");
            }
            _ => {
                let msg = format!("in spread: expected a tuple, found {}", self.show(t));
                self.err(Code::TypeMismatch, e, &msg);
                poison(slots, &free);
            }
        }
        Ok(())
    }

    /// The arguments of a function-value call with a positional spread.
    fn spread_value_args(
        &mut self,
        ps: &[Ty],
        args: &Args<'_>,
        n: NodeRef<'_>,
    ) -> StageResult<Vec<Ref>> {
        if args.trailing.is_some() {
            return unsupported("a trailing block with a spread argument");
        }
        let mut slots: Vec<Option<Ref>> = vec![None; ps.len()];
        if args.positional.len() > ps.len() {
            let msg = format!("the function takes {} arguments", ps.len());
            self.err(Code::ArgumentCount, n, &msg);
        }
        for (i, e) in args.positional.iter().enumerate() {
            let w = ps.get(i).copied();
            let (r, t) = self.expr(*e, w)?;
            let r = match w {
                Some(w) => self.coerce(r, t, w, *e, "argument", ExpectPosition::General),
                None => r,
            };
            if let Some(s) = slots.get_mut(i) {
                *s = Some(r);
            }
        }
        if let Some(s) = args.spread {
            let at = SpreadAt {
                variadic: false,
                given: args.positional.len(),
                named: &[],
                items: &[],
            };
            self.spread_arg(s, ps, at, &mut slots, args)?;
        }
        if slots.iter().any(Option::is_none) {
            let msg = format!("the function takes {} arguments", ps.len());
            self.err(Code::ArgumentCount, n, &msg);
        }
        Ok(slots.into_iter().flatten().collect())
    }

    /// A call of a function item, generic or not.
    pub(crate) fn call_item(
        &mut self,
        def: DefId,
        explicit: &[Ty],
        args: &Args<'_>,
        n: NodeRef<'_>,
        bang: bool,
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
        let Some(item) = self.cx.lookup.item(def) else {
            return unsupported("a call of an item outside the closure");
        };
        if !matches!(&item.data, ItemData::Fn(_)) {
            let generics = item.generics.clone();
            self.check_data_targ_kinds(def, &generics, explicit, n);
        }
        self.note_call(def);
        if def == self.cx.names.known.dbg {
            self.note_dbg(n, true);
        }
        // `std-testing.hd-run.integration-only`: a call of `hd_run!`
        // outside an integration test module is an error, reported here
        // rather than as the requirement its row would need.
        if def == self.cx.names.item("std.testing", "hd_run") && !self.in_integration_test() {
            self.err(
                Code::TestOnlyUse,
                n,
                "`hd_run!` runs a package executable, so only an integration test module may call it",
            );
            return Ok((Ref(NONE), Ty::POISON));
        }
        let name = self.cx.names.text(item.name).to_owned();
        self.check_forbidden_call(def, &name, n);
        // A registration function is called only by a test registration
        // call in test position (`module.testing.direct-call`).
        let registering = std::mem::take(&mut self.registering);
        let runner = crate::tests::case_runner(self.cx.names.known, def);
        if !registering && self.is_registration_fn(def) {
            let msg = format!("`{name}` is called only as a test registration in test position");
            self.err(Code::MisplacedTestCase, n, &msg);
            return Ok((Ref(NONE), Ty::POISON));
        }
        if def == self.cx.names.known.snapshot {
            self.check_snapshot_expect(args);
        }
        match &item.data {
            ItemData::Fn(sig) => {
                if item
                    .intrinsic
                    .is_some_and(|k| self.cx.names.text(k) == "task_all")
                    && !explicit.is_empty()
                {
                    self.err(
                        Code::TypeMismatch,
                        n,
                        "`all` takes no type arguments; its result type is the tuple of its tasks",
                    );
                    return Ok((Ref(NONE), Ty::POISON));
                }
                if def == self.cx.names.known.race && race_has_no_tasks(args) {
                    self.err(
                        Code::ArgumentCount,
                        n,
                        "`race` needs at least one task, and this call passes none",
                    );
                    return Ok((Ref(NONE), Ty::POISON));
                }
                let sig = self.with_result(def, sig.clone());
                let mut vars = self.fresh_generics(&sig, explicit, n);
                self.unsolvable_consts(&sig, def, explicit.len(), &mut vars, n);
                let inst = |t: Ty| subst_owner(pool, def, &vars, t);
                let params: Vec<(Symbol, Ty)> =
                    sig.params.iter().map(|(s, t)| (*s, inst(*t))).collect();
                if runner.is_some() {
                    self.registration_body_result(&params, args)?;
                }
                if item
                    .intrinsic
                    .is_some_and(|k| self.cx.names.text(k) == "task_all")
                {
                    return self.await_all(args, n, bang);
                }
                if item
                    .intrinsic
                    .is_some_and(|k| self.cx.names.text(k) == "facts_of")
                {
                    return self.facts_of_call(args, n);
                }
                // A direct `dbg` call prints with its call site.
                if def == self.cx.names.known.dbg && crate::dbg::is_direct(args, explicit) {
                    return self.dbg_call(args, n);
                }
                // The result first, so an expected type guides literals.
                let ret = self.normalize_deep(inst(sig.ret))?;
                if let Some(w) = want
                    && !sig.suspends
                    && pool.has_infer(ret)
                    && w != Ty::VOID
                {
                    let snap = self.infer.snapshot();
                    if self.infer.unify(pool, ret, w).is_err() {
                        self.infer.rollback(snap);
                        // An expected result still guides each result
                        // argument that fits when another one does not. In
                        // particular, the operand of `?` may return a
                        // different error type that `From` converts while
                        // its success type solves a callee parameter
                        // (`expr.try.expected.result`).
                        if let (
                            TyData::Adt {
                                def: ret_def,
                                args: ret_args,
                            },
                            TyData::Adt {
                                def: want_def,
                                args: want_args,
                            },
                        ) = (pool.get(ret), pool.get(w))
                            && ret_def == want_def
                        {
                            let ret_args = pool.list_items(ret_args).to_vec();
                            let want_args = pool.list_items(want_args).to_vec();
                            for (ret_arg, want_arg) in ret_args.into_iter().zip(want_args) {
                                let snap = self.infer.snapshot();
                                if self.infer.unify(pool, ret_arg, want_arg).is_err() {
                                    self.infer.rollback(snap);
                                }
                            }
                        }
                    } else if vars.iter().any(|&v| {
                        matches!(
                            pool.get(self.infer.resolve(pool, v)),
                            TyData::TraitValue { .. }
                        )
                    }) {
                        // A trait value is no type argument
                        // (types.trait.value.dyn-required.reference-values),
                        // so an expected `*mut dyn Tr` leaves `box(e)`'s `T`
                        // to its argument, and the result converts after the
                        // call (ref.box.weaken).
                        self.infer.rollback(snap);
                    }
                }
                self.default_owner = Some((def, pool.list(&vars), vec![]));
                let inferred = inferred_formals(self, def, &sig, explicit, &params);
                let inst = |x| subst_owner(pool, def, &vars, x);
                let closure_wants = fn_bound_formals(self, def, &sig, &params, &inst);
                let formals = Formals {
                    params: &params,
                    defaults: &sig.defaults,
                    variadic: sig.variadic,
                    inferred: &inferred,
                    closure_wants: Some(&closure_wants),
                };
                let refs = self.check_args(&formals, 0, args, n, &name)?;
                // A rejected argument makes the call an error value, so
                // its context reports nothing more.
                if refs.iter().any(|r| {
                    r.as_inst()
                        .is_some_and(|i| self.b.body_mut().tags[i.0 as usize] == Tag::Poison)
                }) {
                    return Ok(self.poison_value(n));
                }
                // Parameters named only in bounds, then defaults (§2.4
                // steps 7 and 8); a bound naming one still open waits.
                let open = self.infer_through_bounds(def, &sig, &vars, n)?;
                self.open_call_params(&vars, &refs, n);
                let waits = |b: Ty| {
                    open.iter()
                        .any(|&k| crate::trial::names_param(pool, b, def, k))
                };
                for (i, g) in sig.generics.iter().enumerate() {
                    for b in &g.bounds {
                        let waiting = waits(*b);
                        let b = inst(*b);
                        if let TyData::TraitValue {
                            def: tr, args: ta, ..
                        } = pool.get(b)
                        {
                            let tref = TraitRef {
                                trait_: tr,
                                self_ty: vars[i],
                                args: ta,
                            };
                            if waiting {
                                self.note_impl_use(tr, None);
                                self.pending.push(crate::body::PendingGoal {
                                    tref,
                                    at: n.index(),
                                    kind: crate::body::Waiting::Bound {
                                        code: Code::UnsatisfiedTraitBound,
                                    },
                                });
                            } else {
                                self.require_ref(tref, n)?;
                            }
                        }
                    }
                }
                for (i, g) in sig.generics.iter().enumerate() {
                    for b in &g.bounds {
                        self.check_bindings(vars[i], inst(*b), n)?;
                    }
                }
                // The arguments fixed the parameters a projection waited on.
                let ret = self.norm_ty(ret);
                let row = self.call_row(def, &sig, &vars, 0);
                self.callee_row(def, row, vec![(def, vars.clone())], n)?;
                // A registration call's test case runs its std body with
                // the call's arguments, and returns the result of the
                // registration's body (`crates/hd_tir/DESIGN.md` §4.13.9).
                if let Some(body_fn) = runner {
                    if refs.len() != params.len() {
                        return Ok((Ref(NONE), Ty::NEVER));
                    }
                    let last = match params.last() {
                        Some((_, t)) => self.normalize_deep(*t)?,
                        None => Ty::VOID,
                    };
                    let result = match pool.get(self.strip_mut(last)) {
                        TyData::Fn { result, .. } => self.norm_ty(result),
                        _ => Ty::VOID,
                    };
                    // A `timeout` is evaluated and reported before the body
                    // runs (`std-testing.option.timeout-at-run`).
                    if args.named.iter().any(|(name, _)| name == "timeout")
                        && let Some(r) = params
                            .iter()
                            .position(|p| self.cx.names.text(p.0) == "timeout")
                            .and_then(|i| refs.get(i))
                    {
                        self.report_timeout(*r, n);
                    }
                    let c = Callee::Item {
                        def: body_fn,
                        targs: pool.list(&vars),
                    };
                    return Ok(self.emit_call(&c, &refs, result, true, true, n));
                }
                // std's `identity_of(r)` (lib/std/cmp.hd): the identity
                // fields of the reference (`crate::refs`), as the result.
                if item
                    .intrinsic
                    .is_some_and(|k| self.cx.names.text(k) == "ref_identity")
                    && let [r] = refs[..]
                {
                    return Ok((self.ref_identity_value(r, ret, n), ret));
                }
                // `box(e)` (ref.box.new): a new object holding the
                // argument's value, which the argument already copied
                // (ref.box.copy). Its `T < 'static` keeps a local
                // reference out (ref.box.no-local).
                if item
                    .intrinsic
                    .is_some_and(|k| self.cx.names.text(k) == "box")
                    && let ([v], [t]) = (&refs[..], &vars[..])
                {
                    let t = self.infer.resolve(pool, *t);
                    return Ok(self.new_object_ref(*v, t, true, n));
                }
                // A checked `assert_equal` call runs std's `check_equal`
                // (lib/std/testing.hd), which has the same signature.
                let def = if item
                    .intrinsic
                    .is_some_and(|k| self.cx.names.text(k) == "assert_equal")
                {
                    self.cx.names.known.check_equal
                } else {
                    def
                };
                if def == self.cx.names.known.dbg
                    && let (Some(&tuple), Some(&args_ty)) = (refs.first(), vars.first())
                    && let Some(r) = self.dbg_bare(tuple, args_ty, n)
                {
                    return Ok((r, Ty::VOID));
                }
                let c = Callee::Item {
                    def,
                    targs: pool.list(&vars),
                };
                Ok(self.emit_call(&c, &refs, ret, sig.suspends, bang, n))
            }
            ItemData::Newtype(inner) => {
                let inner = *inner;
                plain_args(args, "a newtype constructor")?;
                let [e] = args.positional.as_slice() else {
                    return unsupported("a newtype constructor shape");
                };
                let t = self.ctor(def, explicit)?;
                let TyData::Adt { args: targs, .. } = pool.get(t) else {
                    return unsupported("a newtype's type");
                };
                let it = subst_owner(pool, def, pool.list_items(targs), inner);
                let (r, rt) = self.expr(*e, Some(it))?;
                let r = self.coerce(r, rt, it, *e, "argument", ExpectPosition::General);
                // A newtype over a composite keeps its base value's outer
                // `mut`, which only a `mut Suspend[T]` base can have.
                let t = if self.is_composite(rt) && self.has_mut_access(rt) {
                    pool.intern_ty(&TyData::Mut(t))
                } else {
                    t
                };
                let rec = self.b.refs_record(&[r]);
                Ok((self.b.emit(Tag::NewStruct, NONE, rec, t, n.index()), t))
            }
            ItemData::Data(_) | ItemData::Enum { .. } | ItemData::Alias(_) => {
                let to = self.ctor(def, explicit)?;
                match pool.get(to) {
                    // An alias calls the type it expands to.
                    TyData::Adt { def: base, args: a } if base != def => {
                        self.call_item(base, pool.list_items(a), args, n, bang, want)
                    }
                    TyData::Prim(_) => self.conversion(to, args, n),
                    _ => self.type_name_call(&name, to, args, n),
                }
            }
            _ => {
                let msg = format!("`{name}` is not a function");
                self.err(Code::NotCallable, n, &msg);
                Ok((Ref(NONE), Ty::NEVER))
            }
        }
    }

    /// `req.drive.block-on.direct`: a call of `block_on` written directly
    /// in a `defer` suite, a default or fact expression, or a non-entry
    /// module's initialization.
    /// A closure or local function declared there is not direct.
    pub(crate) fn check_forbidden_call(&mut self, def: DefId, name: &str, n: NodeRef<'_>) {
        let known = self.cx.names.known;
        if def != known.block_on {
            return;
        }
        let context = if self.defer_base.is_some() && self.subs.len() == self.defer_subs {
            "a `defer` suite"
        } else if self.subs.is_empty()
            && let Some(c) = self.forbidden
        {
            c
        } else {
            return;
        };
        let msg = format!("`{name}` cannot be called directly in {context}");
        self.err(Code::SuspensionForbiddenContext, n, &msg);
    }

    /// When a cold suspension is expected, show the actual call with
    /// only its callee's bang removed, retaining bangs in its arguments.
    pub(crate) fn cold_call_hint(&self, want: Ty, n: NodeRef<'_>) -> String {
        if !matches!(self.pool().get(self.strip_mut(want)), TyData::Adt { def, .. } if def == self.cx.names.known.suspend)
            || n.kind() != SyntaxKind::CallExpr
        {
            return String::new();
        }
        let Some(mut callee) = n.children().next() else {
            return String::new();
        };
        if callee.kind() == SyntaxKind::TypeArgsExpr {
            let Some(inner) = callee.children().next() else {
                return String::new();
            };
            callee = inner;
        }
        if callee.kind() != SyntaxKind::SuspendExpr {
            return String::new();
        }
        let Some(bang) = callee.direct_token(&self.cx.src.parse.tokens, TokenKind::Bang) else {
            return String::new();
        };
        let span = self.cx.src.span(n);
        if self
            .diags
            .code
            .iter()
            .zip(&self.diags.primary)
            .any(|(code, at)| *code == Code::NotSuspending && *at == span)
        {
            return String::new();
        }
        let bang = self.cx.src.token_span(bang);
        let text = self.cx.src.text;
        let cold = format!(
            "{}{}",
            &text[span.lo as usize..bang.lo as usize],
            &text[bang.hi as usize..span.hi as usize]
        );
        format!("; pass the cold call `{cold}` without awaiting it first")
    }

    /// A bang call needs a suspending callee (`not-suspending`) and a
    /// driver context (`req.bang.driver-forms`).
    pub(crate) fn check_bang(&mut self, callee_suspends: bool, n: NodeRef<'_>) {
        if !callee_suspends {
            self.err(
                Code::NotSuspending,
                n,
                "a bang call of a function that does not suspend",
            );
        }
        if self.defer_base.is_some() {
            // `flow.defer.suspend`: a `defer` suite cannot suspend.
            self.err(
                Code::SuspensionForbiddenContext,
                n,
                "a `defer` suite cannot suspend; collect the value in the scope and act on it after the scope ends",
            );
        } else if !self.suspends.last().copied().unwrap_or(false) {
            if self.forbidden == Some("a default or fact expression")
                && self.suspends.len() == self.default_depth
            {
                self.err(
                    Code::SuspensionForbiddenContext,
                    n,
                    "a default cannot suspend",
                );
                return;
            }
            self.err(
                Code::BangCallOutsideSuspension,
                n,
                "a bang call needs a suspending function or closure",
            );
        }
    }

    /// `all!(a(), b())`: cold suspensions, then one `AwaitAll` whose
    /// value is the tuple of their results (`crates/hd_wasm_linear/DESIGN.md` §14.5).
    fn await_all(&mut self, args: &Args<'_>, n: NodeRef<'_>, bang: bool) -> StageResult<(Ref, Ty)> {
        if let Some(s) = args.spread {
            self.err(
                Code::TypeMismatch,
                s,
                "`all` takes its tasks as direct arguments, not as a spread",
            );
            return Ok((Ref(NONE), Ty::NEVER));
        }
        if let Some((_, e)) = args.named.first() {
            self.err(
                Code::TypeMismatch,
                *e,
                "`all` takes its tasks as positional arguments, not named ones",
            );
            return Ok((Ref(NONE), Ty::NEVER));
        }
        plain_args(args, "`all`")?;
        let pool = self.pool();
        if !bang {
            self.err(Code::NotSuspending, n, "`all` is called as `all!(...)`");
        }
        if bang {
            self.check_bang(true, n);
        }
        let suspend = self.cx.names.known.suspend;
        let mut refs = Vec::new();
        let mut tys = Vec::new();
        for e in &args.positional {
            let (r, t) = self.expr(*e, None)?;
            let st = self.strip_mut(t);
            let x = match pool.get(st) {
                TyData::Adt { def, args } if def == suspend => {
                    pool.list_items(args).first().copied().unwrap_or(Ty::POISON)
                }
                _ => {
                    let msg = format!(
                        "in argument: expected a cold suspension, found {}",
                        self.show(t)
                    );
                    self.err(Code::TypeMismatch, *e, &msg);
                    Ty::POISON
                }
            };
            refs.push(r);
            tys.push(x);
        }
        let t = pool.intern_ty(&TyData::Tuple {
            elems: pool.list(&tys),
            rest: None,
        });
        let rec = self.b.refs_record(&refs);
        Ok((self.b.emit(Tag::AwaitAll, NONE, rec, t, n.index()), t))
    }

    /// `expr.pipe.bare.no-suspend`: a bare step would suspend with no `!`.
    fn suspending_pipe_step(&mut self, n: NodeRef<'_>) -> (Ref, Ty) {
        self.err(
            Code::SuspendingPipeStep,
            n,
            "a bare pipe step cannot suspend; write `_` and `!`",
        );
        (Ref(NONE), Ty::NEVER)
    }

    /// Emits a `Call`, or an `Await` for a bang call of a suspending
    /// callee; a plain call of one is a cold `mut Suspend[T]`.
    pub(crate) fn emit_call(
        &mut self,
        c: &Callee,
        refs: &[Ref],
        ret: Ty,
        suspends: bool,
        bang: bool,
        n: NodeRef<'_>,
    ) -> (Ref, Ty) {
        let pool = self.pool();
        // A call of a known item reaches its body: a method call, inherent
        // or associated, reads what that body reads
        // (`names.init.transitive`, `names.exec.methods`).
        if let Callee::Item { def, .. } = c {
            self.note_call(*def);
            // `Ptr[T]`'s `read` and `write` (lib/std/rt/memory.hd) are a
            // read and a store of the place at the pointer's target.
            let key = self.cx.lookup.item(*def).and_then(|it| it.intrinsic);
            let key = key.map(|k| self.cx.names.text(k));
            if let (Some(k @ ("ptr_read" | "ptr_write")), [p, rest @ ..]) = (key, refs) {
                let root = hd_tir::ir::PlaceRoot::Ptr(*p);
                let word = self.b.place(&hd_tir::ir::Place { root, proj: vec![] });
                let (tag, w, ty) = match rest {
                    [v] if k == "ptr_write" => (Tag::Store, v.0, Ty::VOID),
                    _ => (Tag::Read, hd_tir::ir::ReadMode::Use as u32, ret),
                };
                return (self.b.emit(tag, word, w, ty, n.index()), ty);
            }
        }
        let ret = self.open_projections(ret, n);
        if bang {
            self.check_bang(suspends, n);
        }
        if suspends && !bang && self.bare_step {
            return self.suspending_pipe_step(n);
        }
        if suspends && !bang {
            let suspend = self.cx.names.known.suspend;
            let st = pool.intern_ty(&TyData::Adt {
                def: suspend,
                args: pool.list(&[ret]),
            });
            let st = pool.intern_ty(&TyData::Mut(st));
            return (self.b.call(c, refs, Providers::None, st, n.index()), st);
        }
        if !suspends
            && !bang
            && let Some(r) = self.push_interpolation(c, refs, ret, n)
        {
            return (r, ret);
        }
        let r = self.b.call(c, refs, Providers::None, ret, n.index());
        if suspends && bang {
            self.b.body_mut().tags[r.0 as usize] = Tag::Await;
        }
        (r, ret)
    }

    /// Append an interpolation's already evaluated strings directly. All
    /// expressions (and their Display calls) finish before any push, just
    /// as with one concatenated argument; the receiver is evaluated once.
    fn push_interpolation(
        &mut self,
        c: &Callee,
        refs: &[Ref],
        ret: Ty,
        n: NodeRef<'_>,
    ) -> Option<Ref> {
        let Callee::Item { def, .. } = c else {
            return None;
        };
        let [recv, text] = refs else { return None };
        let item = self.cx.lookup.item(*def)?;
        if self.cx.names.text(item.name) != "push" {
            return None;
        }
        let paths = self.cx.names.paths;
        let owner = paths.parent(PathId::from_raw(def.raw()));
        if paths.parent(owner) != self.cx.names.module("std.text") {
            return None;
        }
        let (inner, true) = self.ref_of(self.b.ty_of(*recv))? else {
            return None;
        };
        if !matches!(self.pool().get(inner), TyData::Adt { def, .. }
            if def == self.cx.names.item("std.text", "StringBuilder"))
        {
            return None;
        }
        let i = text.as_inst()?.0 as usize;
        let body = self.b.body_mut();
        if body.tags[i] != Tag::Interp {
            return None;
        }
        let parts: Vec<Ref> = body
            .record(body.data[i][1])
            .iter()
            .copied()
            .map(Ref)
            .collect();
        if parts.is_empty() {
            return None;
        }
        // Keep the instruction position as an empty constant-valued block;
        // only concatenation goes away, never any part's evaluation.
        let empty = self.b.const_str("");
        let block = self.b.words_record(&[]);
        let body = self.b.body_mut();
        body.tags[i] = Tag::Block;
        body.data[i] = [block, empty.0];
        let mut result = None;
        for part in parts {
            if let Some((Ty::STRING, at)) = self.b.const_of(part)
                && self.b.body_mut().strings[usize::try_from(at).expect("string constant index")]
                    .is_empty()
            {
                continue;
            }
            result = Some(
                self.b
                    .call(c, &[*recv, part], Providers::None, ret, n.index()),
            );
        }
        Some(result.unwrap_or_else(|| {
            self.b
                .call(c, &[*recv, empty], Providers::None, ret, n.index())
        }))
    }

    /// `t` with each projection whose self type inference has not fixed
    /// replaced by a fresh variable: a pending goal fixes it once that
    /// type is known, so the rest of the statement can use the result.
    fn open_projections(&mut self, t: Ty, n: NodeRef<'_>) -> Ty {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        if !pool.has_assoc(t) || !pool.has_infer(t) {
            return t;
        }
        let list = |me: &mut Self, l: TyList| {
            let v: Vec<Ty> = pool
                .list_items(l)
                .iter()
                .map(|x| me.open_projections(*x, n))
                .collect();
            pool.list(&v)
        };
        let d = match pool.get(t) {
            TyData::Assoc {
                assoc,
                trait_,
                self_ty,
                args,
            } => {
                let self_ty = self.strip_mut(self_ty);
                if !pool.has_infer(self_ty) {
                    return t;
                }
                let n_trait = self.cx.lookup.item(trait_).map_or(0, |i| i.generics.len());
                let mut av = pool.list_items(args).to_vec();
                while av.len() < n_trait {
                    av.push(self.infer.fresh(pool, VarKind::General));
                }
                let var = self.infer.fresh(pool, VarKind::General);
                self.pending.push(crate::body::PendingGoal {
                    tref: TraitRef {
                        trait_,
                        self_ty,
                        args: pool.list(&av),
                    },
                    at: n.index(),
                    kind: crate::body::Waiting::Project { assoc, var },
                });
                return var;
            }
            TyData::Adt { def, args } => TyData::Adt {
                def,
                args: list(self, args),
            },
            TyData::Tuple { elems, rest } => TyData::Tuple {
                elems: list(self, elems),
                rest,
            },
            TyData::Option(i) => TyData::Option(self.open_projections(i, n)),
            TyData::Mut(i) => TyData::Mut(self.open_projections(i, n)),
            TyData::Ref {
                inner,
                mutable,
                counted,
            } => TyData::Ref {
                inner: self.open_projections(inner, n),
                mutable,
                counted,
            },
            TyData::Fn {
                params,
                result,
                row,
                suspends,
                inputs,
                deps,
            } => TyData::Fn {
                params: list(self, params),
                result: self.open_projections(result, n),
                row,
                suspends,
                inputs,
                deps,
            },
            _ => return t,
        };
        pool.intern_ty(&d)
    }

    // ------------------------------------------------------------ variants

    /// `.V`, `.V(args)`: a variant of the expected enum, `Option` or
    /// `Result`.
    pub(crate) fn variant_value(
        &mut self,
        v: NodeRef<'_>,
        args: &Args<'_>,
        want: Option<Ty>,
        call: Option<NodeRef<'_>>,
    ) -> StageResult<(Ref, Ty)> {
        plain_args(args, "a variant")?;
        let pool = self.pool();
        let at = call.unwrap_or(v);
        let name = self.cx.src.text(self.cx.src.last(v)).to_owned();
        let w = want
            .map(|w| self.infer.shallow(pool, w))
            .map(|w| match pool.get(w) {
                TyData::Mut(i) => i,
                _ => w,
            });
        let enum_ty = match w.map(|w| pool.get(w)) {
            Some(TyData::Option(_) | TyData::Adt { .. }) => w.unwrap_or(Ty::POISON),
            // A poisoned expectation already reported its error.
            Some(TyData::Poison) => return Ok((Ref(NONE), Ty::POISON)),
            // An open expectation may still resolve to an optional or a
            // `Result` from the context around the call. A join site's own
            // variable is no expectation (`types.lct.contextual.error`).
            Some(TyData::Infer(_))
                if !want.is_some_and(|w| self.joins.is_site(w))
                    && matches!(name.as_str(), "Some" | "None" | "Ok" | "Err") =>
            {
                match name.as_str() {
                    "Some" | "None" => {
                        let t = self.infer.fresh(pool, VarKind::General);
                        pool.intern_ty(&TyData::Option(t))
                    }
                    _ => {
                        let a = self.infer.fresh(pool, VarKind::General);
                        let b = self.infer.fresh(pool, VarKind::General);
                        pool.intern_ty(&TyData::Adt {
                            def: self.cx.names.known.result,
                            args: pool.list(&[a, b]),
                        })
                    }
                }
            }
            _ => {
                let msg = format!("`.{name}` needs an expected enum type");
                self.err(Code::MissingContextualEnumType, v, &msg);
                return Ok((Ref(NONE), Ty::POISON));
            }
        };
        // `expr.name.contextual.rules`: with no call, `.Variant` is what
        // `Enum.Variant` is, so a payload variant is a function value or,
        // with two or more fields, `unsaturated-enum-constructor`
        // (`data.enum.fn-value`).
        if call.is_none()
            && let Some(r) = self.variant_fn_value(enum_ty, &name, want, v)
        {
            return Ok(r);
        }
        self.variant_by_name(enum_ty, &name, args, at)
    }

    /// A variant's index and payload field types under the enum's
    /// arguments; `Option`'s are `None` 0 and `Some` 1.
    pub(crate) fn variant_fields(&self, enum_ty: Ty, name: &str) -> Option<(u32, Vec<Ty>)> {
        let pool = self.pool();
        let t = self.infer.shallow(pool, enum_ty);
        let t = match pool.get(t) {
            TyData::Mut(i) => i,
            _ => t,
        };
        match pool.get(t) {
            TyData::Option(inner) => match name {
                "None" => Some((0, vec![])),
                "Some" => Some((1, vec![inner])),
                _ => None,
            },
            TyData::Adt { def, args } => {
                let ItemData::Enum { variants, .. } = &self.cx.lookup.item(def)?.data else {
                    return None;
                };
                let sym = self.cx.names.syms.intern(name);
                let i = variants.iter().position(|v| v.name == sym)?;
                let argv = pool.list_items(args);
                let fs = variants[i]
                    .fields
                    .iter()
                    .map(|f| subst_owner(pool, def, argv, f.ty))
                    .collect();
                Some((u32::try_from(i).ok()?, fs))
            }
            _ => None,
        }
    }

    // ------------------------------------------------------------ members

    /// `Type.Variant`, `module.item` and other static member values, or
    /// `None` when `base` is a value.
    pub(crate) fn static_member_value(
        &mut self,
        base: NodeRef<'_>,
        name: &str,
        want: Option<Ty>,
        n: NodeRef<'_>,
    ) -> StageResult<Option<(Ref, Ty)>> {
        let named = if base.kind() == SyntaxKind::FieldExpr {
            self.module_item(base)
        } else {
            let text = self.cx.src.text(self.cx.src.first(base)).to_owned();
            self.scope_name(&text)
        };
        match named {
            Some(Named::Poison) => Ok(Some(self.poison_value(n))),
            Some(Named::Missing) => Ok(Some((Ref(NONE), Ty::POISON))),
            Some(Named::Module(m)) => {
                let Some(def) = self.export(m, name) else {
                    self.missing_export(m, name, n);
                    return Ok(Some((Ref(NONE), Ty::POISON)));
                };
                if let Some(v) = self.declaration_value(def, name, want, n)? {
                    return Ok(Some(v));
                }
                let msg = format!("`{name}` is not a value");
                self.err(Code::UnknownName, n, &msg);
                Ok(Some((Ref(NONE), Ty::POISON)))
            }
            Some(Named::Item(def)) if self.is_enum_head(def) => {
                let t = self.ctor(def, &[])?;
                if self.kind_of_item(def) == Some(HeadKind::Alias)
                    && self.variant_fields(t, name).is_none()
                {
                    return Ok(None);
                }
                if let Some(v) = self.variant_fn_value(t, name, want, n) {
                    return Ok(Some(v));
                }
                Ok(Some(self.variant_by_name(t, name, &Args::empty(), n)?))
            }
            _ => Ok(None),
        }
    }

    /// `Enum.Variant` without an argument clause as a function value
    /// (`data.enum.fn-value`): a variant with one payload field is the
    /// closure that builds it, typed from the expected function type
    /// where there is one (`data.enum.fn-value.generic-argument`). One
    /// with two or more fields is `unsaturated-enum-constructor`. `None`
    /// for a payload-free variant, an enum value, and for an unknown one.
    fn variant_fn_value(
        &mut self,
        t: Ty,
        name: &str,
        want: Option<Ty>,
        n: NodeRef<'_>,
    ) -> Option<(Ref, Ty)> {
        let pool = self.pool();
        let (index, fields) = self.variant_fields(t, name)?;
        match fields.len() {
            0 => return None,
            1 => {}
            _ => {
                let msg = format!(
                    "`{name}` takes {} values, so it is not a function value; call it",
                    fields.len()
                );
                self.err(Code::UnsaturatedEnumConstructor, n, &msg);
                return Some((Ref(NONE), Ty::POISON));
            }
        }
        let payload = fields[0];
        // `ref.lifetime.enum.construct`: an enum with lifetimes takes them
        // from the payload, as an elided result would.
        let deps = if pool.ref_slots(t) > 0 {
            pool.fn_deps_written(&[payload], t, &[])
        } else {
            hd_types::FnDeps::NONE
        };
        let ft = pool.intern_ty(&TyData::Fn {
            params: pool.list(&[payload]),
            result: t,
            row: hd_types::RowId::EMPTY,
            suspends: false,
            inputs: hd_types::Inputs::Fixed,
            deps,
        });
        self.fit_expected(ft, want);
        // The enum's own type arguments are decided like a reference's.
        let targs = match pool.get(self.infer.resolve(pool, t)) {
            TyData::Adt { args, .. } => pool.list_items(args).to_vec(),
            TyData::Option(inner) => vec![inner],
            _ => vec![],
        };
        for a in targs {
            self.ref_params.push((a, None, n.index()));
        }
        let syn = n.index();
        let sym = self.cx.names.syms.intern("$0");
        let param = self.b.local(payload, sym, local_flags::PARAM, syn);
        let mark = self.b.open_sub(&[param]);
        let blk = self.b.open_block();
        let value = self.b.get(param, payload, syn);
        let rec = self.b.refs_record(&[value]);
        let made = self.b.emit(Tag::NewVariant, index, rec, t, syn);
        let root = self.b.close_block(blk, Some(made), t, syn);
        Some((self.b.close_sub(mark, root, ft, syn), ft))
    }

    /// Whether `def` names an enum, or an alias that may expand to one
    /// (`Option` is an alias of the built-in optional): `Name.Variant` can
    /// then name a variant.
    fn is_enum_head(&self, def: DefId) -> bool {
        matches!(
            self.kind_of_item(def),
            Some(HeadKind::Enum | HeadKind::Alias)
        )
    }

    /// A variant value of `t` from positional or named payload values.
    fn variant_by_name(
        &mut self,
        t: Ty,
        name: &str,
        args: &Args<'_>,
        n: NodeRef<'_>,
    ) -> StageResult<(Ref, Ty)> {
        let Some((index, fields)) = self.variant_fields(t, name) else {
            let msg = format!("no variant `{name}` of {}", self.show(t));
            self.err(Code::UnknownVariant, n, &msg);
            return Ok((Ref(NONE), Ty::NEVER));
        };
        let names = self.variant_field_names(t, name);
        let mut slots: Vec<Option<Ref>> = vec![None; fields.len()];
        let mut extra = false;
        let mut named_bad = false;
        for (i, e) in args.positional.iter().enumerate() {
            let ft = fields.get(i).copied();
            let (r, rt) = self.expr(*e, ft)?;
            match (ft, slots.get_mut(i)) {
                (Some(ft), Some(slot)) => {
                    *slot = Some(self.coerce(r, rt, ft, *e, "payload", ExpectPosition::General));
                }
                _ => extra = true,
            }
        }
        for (fname, e) in &args.named {
            let Some(i) = names.iter().position(|x| x == fname) else {
                let msg = format!("no field `{fname}` of variant `{name}`");
                self.err(Code::UnknownField, *e, &msg);
                named_bad = true;
                continue;
            };
            let ft = fields[i];
            if slots[i].is_some() {
                let msg = format!("`{fname}` is given twice");
                self.err(Code::DuplicateArgument, *e, &msg);
                named_bad = true;
                continue;
            }
            let (r, rt) = self.expr(*e, Some(ft))?;
            slots[i] = Some(self.coerce(r, rt, ft, *e, "payload", ExpectPosition::General));
        }
        if extra || (!named_bad && slots.iter().any(Option::is_none)) {
            let msg = format!("`{name}` takes {} values", fields.len());
            self.err(Code::ArgumentCount, n, &msg);
        }
        let refs: Vec<Ref> = slots.into_iter().map(|r| r.unwrap_or(Ref(NONE))).collect();
        let rec = self.b.refs_record(&refs);
        Ok((self.b.emit(Tag::NewVariant, index, rec, t, n.index()), t))
    }

    /// A variant's payload field names (`0`, `1`... when positional).
    pub(crate) fn variant_field_names(&self, t: Ty, name: &str) -> Vec<String> {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        let t = match pool.get(t) {
            TyData::Mut(i) => i,
            _ => t,
        };
        let TyData::Adt { def, .. } = pool.get(t) else {
            return vec!["0".into()];
        };
        let Some(ItemData::Enum { variants, .. }) = self.cx.lookup.item(def).map(|i| &i.data)
        else {
            return vec![];
        };
        let sym = self.cx.names.syms.intern(name);
        variants
            .iter()
            .find(|v| v.name == sym)
            .map(|v| {
                v.fields
                    .iter()
                    .map(|f| self.cx.names.text(f.name).to_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// `base.name(args)` and `Base::name(args)`.
    fn member_call(
        &mut self,
        n: NodeRef<'_>,
        callee: NodeRef<'_>,
        args: &Args<'_>,
        bang: bool,
        want: Option<Ty>,
        method_targs: Vec<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let Some(base) = callee.children().next() else {
            return unsupported("a member call without a base");
        };
        self.elem_may_stand(base, crate::elem_refs::ElemUse::Receiver);
        let name = self.cx.src.text(self.cx.src.last(callee)).to_owned();
        // Static forms: a type, a type parameter, a trait or a module,
        // each also through a module (`text.StringBuilder::new()`).
        if matches!(base.kind(), SyntaxKind::NameExpr | SyntaxKind::TypeArgsExpr)
            && (base.kind() == SyntaxKind::TypeArgsExpr
                || self.find_local(self.sym_of(base)).is_none())
            || base.kind() == SyntaxKind::FieldExpr
        {
            let (bnode, explicit) = if base.kind() == SyntaxKind::TypeArgsExpr {
                let Some(inner) = base.children().next() else {
                    return unsupported("a type-argument base");
                };
                let mut ex = Vec::new();
                if let Some(tl) = Src::child(base, SyntaxKind::TypeArgumentList) {
                    let gens = self.explicit_generics(inner);
                    ex = self.explicit_targs(tl, &gens)?;
                }
                (inner, ex)
            } else {
                (base, vec![])
            };
            let text = self.cx.src.text(self.cx.src.first(bnode)).to_owned();
            let named = if bnode.kind() == SyntaxKind::FieldExpr {
                self.module_item(bnode)
            } else {
                if let Some(p) = self.type_param(&text) {
                    self.method_targs = method_targs;
                    return self.static_on_type(p, &name, args, (n, bang), true, want);
                }
                if let Some(p) = Prim::ALL.iter().find(|p| p.name() == text) {
                    self.method_targs = method_targs;
                    return self.static_on_type(Ty::prim(*p), &name, args, (n, bang), false, want);
                }
                self.scope_name(&text)
            };
            match named {
                Some(Named::Poison) => return Ok(self.poison_value(n)),
                Some(Named::Missing) => return Ok((Ref(NONE), Ty::POISON)),
                Some(Named::Module(m)) => {
                    let Some(def) = self.export(m, &name) else {
                        self.missing_export(m, &name, callee);
                        return Ok((Ref(NONE), Ty::POISON));
                    };
                    return self.call_item(def, &explicit, args, n, bang, want);
                }
                Some(Named::Item(def)) => match self.kind_of_item(def) {
                    Some(HeadKind::Enum | HeadKind::Alias)
                        if {
                            let et = self.ctor(def, &explicit)?;
                            self.variant_fields(et, &name).is_some()
                        } =>
                    {
                        let t = self.ctor(def, &explicit)?;
                        let t = match want {
                            Some(w) if self.can_unify(w, t) => {
                                self.expect(t, w, n, "variant", ExpectPosition::General);
                                t
                            }
                            _ => t,
                        };
                        return self.variant_by_name(t, &name, args, n);
                    }
                    Some(HeadKind::Trait) => {
                        self.method_targs = method_targs;
                        return self.trait_static(def, explicit, &name, args, n, bang);
                    }
                    Some(k) if k.is_type() => {
                        let t = self.ctor(def, &explicit)?;
                        self.method_targs = method_targs;
                        return self.static_on_type(t, &name, args, (n, bang), false, want);
                    }
                    _ => {}
                },
                None => {}
            }
        }
        // A `&self` or `&mut self` method on a place takes its address
        // (`fn.method.auto-address`).
        if let Some((recv, rt)) = self.auto_address(base, &name, true)? {
            self.method_targs = method_targs;
            return self.method_on(recv, rt, &name, args, (n, bang), want);
        }
        let (recv, rt) = self.expr(base, None)?;
        // A method call through a reference calls it on the referenced
        // value (`ref.read.auto`), unless the method takes a reference.
        if let Some(passed) = self.pass_reference_receiver(recv, rt, &name, base)? {
            self.method_targs = method_targs;
            return self.method_on(passed.0, passed.1, &name, args, (n, bang), want);
        }
        let via_dyn = self.dyn_ref_of(rt).map(|(_, m)| (m, base.index()));
        let (recv, rt) = match self.ref_of(rt) {
            Some(_) => self.emit_deref(recv, rt, base.index()),
            None => (recv, rt),
        };
        self.method_targs = method_targs;
        // Through a trait value reference, `&self` and `&mut self` methods
        // are available (`trait.dyn.ref.member`).
        self.dyn_recv = via_dyn;
        self.method_on(recv, rt, &name, args, (n, bang), want)
    }

    /// `T.name(args)` on a type: an inherent associated function, or a
    /// trait's static method through a bound of a parameter.
    fn static_on_type(
        &mut self,
        t: Ty,
        name: &str,
        args: &Args<'_>,
        (n, bang): (NodeRef<'_>, bool),
        is_param: bool,
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
        if !is_param && let Some((method, impl_def, impl_args)) = self.find_inherent(t, name) {
            let sig = self.sig_of(method)?;
            let explicit = std::mem::take(&mut self.method_targs);
            let mut vars = self.fresh_generics(&sig, &explicit, n);
            self.unsolvable_consts(&sig, method, explicit.len(), &mut vars, n);
            let inst = |x: Ty| {
                subst_owner(
                    pool,
                    method,
                    &vars,
                    subst_owner(pool, impl_def, &impl_args, x),
                )
            };
            let params: Vec<(Symbol, Ty)> =
                sig.params.iter().map(|(s, x)| (*s, inst(*x))).collect();
            let mut all = impl_args.clone();
            all.extend(&vars);
            self.default_owner = Some((method, pool.list(&all), vec![]));
            let inferred = inferred_formals(self, method, &sig, &explicit, &params);
            let closure_wants = fn_bound_formals(self, method, &sig, &params, &inst);
            let formals = Formals {
                params: &params,
                defaults: &sig.defaults,
                variadic: sig.variadic,
                inferred: &inferred,
                closure_wants: Some(&closure_wants),
            };
            let refs = self.check_args(&formals, 0, args, n, name)?;
            self.fill_method_defaults(&sig, &vars, &inst, want)?;
            self.open_call_params(&vars, &refs, n);
            self.bounds_of(&sig, &vars, &inst, n)?;
            self.inherent_row(method, &sig, (impl_def, &impl_args), &vars, n)?;
            let mut targs = impl_args.clone();
            targs.extend(&vars);
            let c = Callee::Item {
                def: method,
                targs: pool.list(&targs),
            };
            let ret = self.normalize_deep(inst(sig.ret))?;
            return Ok(self.emit_call(&c, &refs, ret, sig.suspends, bang, n));
        }
        // A trait method with no receiver through a bound (`N::zero()`).
        let sym = self.cx.names.syms.intern(name);
        let traits = self.traits_declaring(sym);
        // Through a type, the trait part of `Methods`
        // (`trait.assoc-call.type.traits`): `X::from(v)` chooses among
        // `X`'s instantiations of `From` like a dot call.
        if !is_param {
            match self.trait_part(t, sym, &self.available_traits(sym))? {
                Some(Hit::Ambiguous(found)) => {
                    let names: Vec<String> = self
                        .cx
                        .names
                        .display_names(&found)
                        .iter()
                        .map(|d| format!("`{d}`"))
                        .collect();
                    let msg = format!(
                        "`{}::{name}` is ambiguous: the traits {} each supply it; write `Trait::{name}(..)`",
                        self.show(t),
                        names.join(" and ")
                    );
                    self.err(Code::AmbiguousMethod, n, &msg);
                    return Ok((Ref(NONE), Ty::POISON));
                }
                Some(Hit::Choice {
                    trait_,
                    method,
                    cands,
                }) => {
                    return self.choose_method(
                        (trait_, method, t),
                        &cands,
                        None,
                        args,
                        (n, bang),
                        want,
                    );
                }
                Some(Hit::Trait {
                    trait_,
                    method,
                    args: targs,
                    ..
                }) => {
                    return self.trait_method_call(
                        TraitTarget {
                            trait_,
                            method,
                            self_ty: t,
                            args: targs,
                        },
                        None,
                        args,
                        n,
                        (bang, want),
                    );
                }
                _ => {
                    let msg = format!("no method `{name}` on {}", self.show(t));
                    if self.no_method(t, name, n, &msg) && name == "new" && !bang {
                        self.empty_map_fix(t, args, n, want);
                    }
                    return Ok((Ref(NONE), Ty::POISON));
                }
            }
        }
        // `T::f(args)`, where `T` is a type parameter, resolves `f`
        // through the bounds of `T`: exactly one trait among the bounds
        // and their supertraits may declare it
        // (`trait.assoc-call.parameter`, `trait.assoc-call.parameter.one`).
        let mut bound_hits = Vec::new();
        for (tr, m) in traits {
            let tref = TraitRef {
                trait_: tr,
                self_ty: t,
                args: TyList::EMPTY,
            };
            let holds = if is_param {
                (0..self.env.clause_self.len())
                    .any(|i| self.env.clause_self[i] == t && self.env.clause_trait[i] == tr)
            } else {
                matches!(self.solve(tref)?, hd_types::solver::Answer::Holds { .. })
            };
            if holds {
                if is_param {
                    bound_hits.push((tr, m));
                    continue;
                }
                return self.trait_method_call(
                    TraitTarget {
                        trait_: tr,
                        method: m,
                        self_ty: t,
                        args: vec![],
                    },
                    None,
                    args,
                    n,
                    (bang, want),
                );
            }
        }
        if is_param {
            match bound_hits.as_slice() {
                // Exactly one bound declares the method: statically
                // resolved through it (`trait.assoc-call.parameter`).
                [(tr, m)] => {
                    let (tr, m) = (*tr, *m);
                    return self.trait_method_call(
                        TraitTarget {
                            trait_: tr,
                            method: m,
                            self_ty: t,
                            args: vec![],
                        },
                        None,
                        args,
                        n,
                        (bang, want),
                    );
                }
                // Two or more do: an error (`trait.assoc-call.parameter.one`).
                [_, _, ..] => {
                    let names: Vec<String> = bound_hits
                        .iter()
                        .map(|(tr, _)| format!("`{}`", self.cx.names.display_name(*tr)))
                        .collect();
                    let msg = format!(
                        "`{}::{name}` is ambiguous: the bounds {} each declare it; write one bound's trait explicitly",
                        self.show(t),
                        names.join(" and ")
                    );
                    self.err(Code::AmbiguousMethod, n, &msg);
                    return Ok((Ref(NONE), Ty::POISON));
                }
                [] => {}
            }
        }
        let msg = format!("no method `{name}` on {}", self.show(t));
        self.no_method(t, name, n, &msg);
        Ok((Ref(NONE), Ty::POISON))
    }

    /// `Trait::method(args)`: the self type comes from the arguments; the
    /// trait's arguments are the written ones (`Combine::[i32]::combine`,
    /// `trait.qualified.trait-arguments`), the rest fresh.
    fn trait_static(
        &mut self,
        tr: DefId,
        trait_args: Vec<Ty>,
        name: &str,
        args: &Args<'_>,
        n: NodeRef<'_>,
        bang: bool,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
        let Some(ItemData::Trait(td)) = self.cx.lookup.item(tr).map(|i| &i.data) else {
            return unsupported("a trait path");
        };
        let sym = self.cx.names.syms.intern(name);
        let Some(&(_, m)) = td.methods.iter().find(|(s, _)| *s == sym) else {
            let msg = format!(
                "no method `{name}` on trait {}",
                self.cx.names.display_name(tr)
            );
            self.err(Code::UnknownMethod, n, &msg);
            return Ok((Ref(NONE), Ty::POISON));
        };
        // Inside a template, `Structure::f(..)` and `Trait::f(..)` of the
        // derived trait have the template's `T` as `Self`
        // (`annot.template.qualified-self`).
        let (self_ty, track_self) = match self.template_self(tr) {
            Some(t) => (t, false),
            None => (self.infer.fresh(pool, VarKind::General), true),
        };
        let out = self.trait_method_call(
            TraitTarget {
                trait_: tr,
                method: m,
                self_ty,
                args: trait_args,
            },
            None,
            args,
            n,
            (bang, None),
        );
        // A `Self` nothing determines reports at the end of the body,
        // once the arguments and the expected type had their chance
        // (`trait.assoc-call.trait.undetermined`).
        if track_self {
            self.assoc_self.push((self_ty, n.index()));
        }
        out
    }

    pub(crate) fn sig_of(&self, d: DefId) -> StageResult<FnSig> {
        match self.cx.lookup.item(d).and_then(|i| i.sig().cloned()) {
            Some(s) => Ok(self.with_result(d, s)),
            None => unsupported("a method without a signature"),
        }
    }

    /// The bounds of a callee's own parameters, under its full
    /// instantiation (`Self` and the owner's parameters included).
    pub(crate) fn bounds_of(
        &mut self,
        sig: &FnSig,
        vars: &[Ty],
        inst: &dyn Fn(Ty) -> Ty,
        n: NodeRef<'_>,
    ) -> StageResult<()> {
        let pool = self.pool();
        for (i, g) in sig.generics.iter().enumerate() {
            for b in &g.bounds {
                let b = inst(*b);
                if let TyData::TraitValue {
                    def: tr, args: ta, ..
                } = pool.get(b)
                {
                    let tref = TraitRef {
                        trait_: tr,
                        self_ty: vars[i],
                        args: ta,
                    };
                    self.require_ref(tref, n)?;
                }
            }
        }
        for (i, g) in sig.generics.iter().enumerate() {
            for b in &g.bounds {
                self.check_bindings(vars[i], inst(*b), n)?;
            }
        }
        Ok(())
    }

    /// An inherent method (`names.method-lookup.inherent`): method, impl and arguments.
    pub(crate) fn find_inherent(&mut self, t: Ty, name: &str) -> Option<(DefId, DefId, Vec<Ty>)> {
        self.inherent_where(t, name, true)
    }

    /// An inherent method of a type, whatever its visibility: what the
    /// compiler's own desugarings call.
    pub(crate) fn find_inherent_any(
        &mut self,
        t: Ty,
        name: &str,
    ) -> Option<(DefId, DefId, Vec<Ty>)> {
        self.inherent_where(t, name, false)
    }

    /// No member was selected for `t.name(..)`: `private-member` when `t`
    /// itself has an inherent method `name` that is not visible here
    /// (`names.method-lookup.private`), else `unknown-method` with `msg`
    /// (`names.method-lookup.unknown`).
    /// Returns true only for an unknown method with no hidden candidate.
    pub(crate) fn no_method(&mut self, t: Ty, name: &str, n: NodeRef<'_>, msg: &str) -> bool {
        // A parameter bound by a trait that does not exist has the methods
        // of that trait, whatever they are; the bound is the one error.
        let bare = self.strip_mut(t);
        if self
            .env
            .clause_self
            .iter()
            .zip(&self.env.clause_trait)
            .any(|(s, tr)| *s == bare && *tr == DefId::NONE)
        {
            return false;
        }
        let snap = self.infer.snapshot();
        let private = self.find_inherent_any(t, name).is_some();
        self.infer.rollback(snap);
        if private {
            let msg = format!(
                "the method `{name}` of {} is private to its module",
                self.show(t)
            );
            self.err(Code::PrivateMember, n, &msg);
            false
        } else {
            let hidden = self.unavailable_traits_of(t, name);
            let snap = self.infer.snapshot();
            let unmet = self.unmet_inherent_bound(t, name);
            self.infer.rollback(snap);
            if let Some(unmet) = unmet {
                self.err(Code::UnsatisfiedTraitBound, n, &unmet);
                false
            } else if hidden.is_empty() {
                let hint = self.method_hint(t, name);
                self.err(Code::UnknownMethod, n, &format!("{msg}{hint}"));
                true
            } else {
                let names = self.cx.names.display_names(&hidden).join("`, `");
                let msg = format!(
                    "{msg}; `{names}` supplies it but is not available here, add a `use` for it"
                );
                self.err(Code::UnknownMethod, n, &msg);
                false
            }
        }
    }

    /// `expr.collection.empty.map-constructor-fix`: only a type-qualified
    /// failed constructor with a known expected map type can become `{}`.
    fn empty_map_fix(&mut self, ty: Ty, args: &Args<'_>, n: NodeRef<'_>, want: Option<Ty>) {
        let pool = self.pool();
        if n.kind() != SyntaxKind::CallExpr
            || !args.positional.is_empty()
            || args.spread.is_some()
            || !args.after_spread.is_empty()
            || !args.named.is_empty()
            || args.trailing.is_some()
            || !self.method_targs.is_empty()
            || !self
                .available_traits(self.cx.names.syms.intern("new"))
                .is_empty()
            || !matches!(pool.get(self.strip_mut(ty)), TyData::Adt { def, .. }
                if def == self.cx.names.known.map)
        {
            return;
        }
        let Some(want) = self.literal_want(want) else {
            return;
        };
        let want = self.infer.resolve(pool, want);
        if !matches!(pool.get(want), TyData::Adt { def, .. } if def == self.cx.names.known.map)
            || pool.has_infer(want)
            || pool.has_poison(want)
            || !self.can_unify(ty, want)
        {
            return;
        }
        let span = self.cx.src.span(n);
        // A whole-call edit must not discard a comment within the call.
        if self.cx.src.text[span.lo as usize..span.hi as usize].contains('#') {
            return;
        }
        self.diags.add_fix(
            "use `{}` for an empty map",
            hd_diag::FixSafety::Exact,
            &[(span, "{}")],
        );
    }

    fn append_fix(&mut self, receiver: (Ref, Ty), args: &Args<'_>, n: NodeRef<'_>) {
        let (value, ty) = receiver;
        if n.kind() != SyntaxKind::CallExpr
            || args.positional.len() != 1
            || args.spread.is_some()
            || !args.after_spread.is_empty()
            || !args.named.is_empty()
            || args.trailing.is_some()
            || self.builtin_method(ty, "push").is_none()
            || !self.place_writable_quiet(value)
        {
            return;
        }
        let Some(field) = n
            .children()
            .next()
            .filter(|head| head.kind() == SyntaxKind::FieldExpr)
        else {
            return;
        };
        let member = self.cx.src.last(field);
        if self.cx.src.tkind(member) == Some(TokenKind::Ident)
            && self.cx.src.text(member) == "append"
        {
            self.diags.add_fix(
                "rename `append` to `push`",
                hd_diag::FixSafety::Exact,
                &[(self.cx.src.token_span(member), "push")],
            );
        }
    }

    fn method_hint(&mut self, t: Ty, name: &str) -> String {
        let bare = self.strip_mut(self.infer.resolve(self.pool(), t));
        let data = self.pool().get(bare);
        if name == "new"
            && matches!(data, TyData::Adt { def, .. } if def == self.cx.names.known.map)
        {
            return "; write `{}` for an empty map".to_owned();
        }
        if name == "unwrap" && matches!(data, TyData::Option(_)) {
            return "; use `expect(message)` or `unwrap_or(fallback)`".to_owned();
        }
        if name == "append" && self.builtin_method(t, "push").is_some() {
            return "; did you mean `push`?".to_owned();
        }
        let index = self.method_index();
        let mut names: Vec<String> = index
            .names()
            .map(|s| self.cx.names.text(s).to_owned())
            .chain(["len", "push", "get", "remove"].map(str::to_owned))
            .collect();
        names.sort_stable();
        names.dedup();
        let mut ranked: Vec<_> = names
            .into_iter()
            .map(|candidate| (name_distance(name, &candidate), candidate))
            .filter(|(distance, candidate)| {
                candidate != name && *distance <= name.chars().count().div_ceil(2).max(3)
            })
            .collect();
        ranked.sort_stable();
        for (_, candidate) in ranked {
            let checkpoint = self.checkpoint();
            let memo = std::mem::take(&mut self.memo);
            let fuel = std::mem::replace(&mut self.fuel, hd_base::Fuel::new(4096));
            let inherent = self.find_inherent(bare, &candidate).is_some()
                || self.builtin_method(bare, &candidate).is_some();
            let sym = self.cx.names.syms.intern(&candidate);
            let traits = self
                .available_traits(sym)
                .into_iter()
                .map(|(tr, _)| tr)
                .collect();
            let applicable = inherent
                || matches!(self.solve_goal(&Goal::Methods { receiver: bare, name: sym, traits }), Ok(Answer::Candidates(c)) if !c.is_empty());
            self.rollback(checkpoint);
            self.memo = memo;
            self.fuel = fuel;
            if applicable {
                return format!("; did you mean `{candidate}`?");
            }
        }
        String::new()
    }

    /// The traits with a method `name` that `t` implements and whose
    /// names are not bound here, for the `use` suggestion
    /// (`names.method-lookup.hint.use`).
    fn unavailable_traits_of(&mut self, t: Ty, name: &str) -> Vec<DefId> {
        let sym = self.cx.names.syms.intern(name);
        let asked: Vec<DefId> = self
            .traits_declaring(sym)
            .into_iter()
            .map(|(tr, _)| tr)
            .filter(|&tr| !self.trait_available(tr))
            .collect();
        if asked.is_empty() {
            return asked;
        }
        let snap = self.infer.snapshot();
        let goal = Goal::Methods {
            receiver: self.strip_mut(t),
            name: sym,
            traits: asked,
        };
        let mut found: Vec<DefId> = Vec::new();
        if let Ok(Answer::Candidates(cands)) = self.solve_goal(&goal) {
            for c in cands {
                if let Some(table) = self.cx.impl_table(c.row.module) {
                    let tr = table.trait_[c.row.row as usize];
                    if !found.contains(&tr) {
                        found.push(tr);
                    }
                }
            }
        }
        self.infer.rollback(snap);
        found
    }

    /// The first bound of `impl_def`'s generics, at `vars`, that fails:
    /// the type it constrains and the bound.
    fn unmet_impl_bound(&mut self, impl_def: DefId, vars: &[Ty]) -> Option<(Ty, Ty)> {
        let pool = self.pool();
        let item = self.cx.lookup.item(impl_def)?;
        for (i, g) in item.generics.iter().enumerate() {
            for b in &g.bounds {
                let b = subst_owner(pool, impl_def, vars, *b);
                if let TyData::TraitValue { def: tr, args, .. } = pool.get(b) {
                    let ty = self.infer.resolve(pool, vars[i]);
                    let tref = TraitRef {
                        trait_: tr,
                        self_ty: ty,
                        args,
                    };
                    if matches!(self.solve(tref), Ok(hd_types::solver::Answer::Fails(_))) {
                        return Some((ty, b));
                    }
                }
            }
        }
        None
    }

    /// The one inherent method `name` whose impl matches `t` but whose
    /// bound fails, described for `unsatisfied-trait-bound`; `None` when
    /// there is no such method or several.
    fn unmet_inherent_bound(&mut self, t: Ty, name: &str) -> Option<String> {
        let pool = self.pool();
        let sym = self.cx.names.syms.intern(name);
        let cands = self.method_index().candidates(sym, true)?;
        let t = match pool.get(self.infer.shallow(pool, t)) {
            TyData::Mut(i) => i,
            _ => t,
        };
        let mut found = None;
        for (impl_def, method) in cands {
            if self.hidden.contains(&impl_def) || !self.method_visible(method) {
                continue;
            }
            let item = self.cx.lookup.item(impl_def)?;
            let ItemData::Impl { self_ty, .. } = &item.data else {
                continue;
            };
            let self_ty = *self_ty;
            let n = item.generics.len();
            let vars: Vec<Ty> = (0..n)
                .map(|_| self.infer.fresh(pool, VarKind::General))
                .collect();
            let head = subst_owner(pool, impl_def, &vars, self_ty);
            let snap = self.infer.snapshot();
            if self.infer.unify(pool, head, t).is_ok()
                && let Some((ty, bound)) = self.unmet_impl_bound(impl_def, &vars)
            {
                if found.is_some() {
                    self.infer.rollback(snap);
                    return None;
                }
                found = Some(format!(
                    "the method `{name}` of {} needs {} to satisfy {}",
                    self.show(t),
                    self.show(ty),
                    self.show(bound)
                ));
            }
            self.infer.rollback(snap);
        }
        found
    }

    fn inherent_where(
        &mut self,
        t: Ty,
        name: &str,
        visible_only: bool,
    ) -> Option<(DefId, DefId, Vec<Ty>)> {
        let pool = self.pool();
        let sym = self.cx.names.syms.intern(name);
        let cands = self.method_index().candidates(sym, true)?;
        let t = match pool.get(self.infer.shallow(pool, t)) {
            TyData::Mut(i) => i,
            _ => t,
        };
        for (impl_def, method) in cands {
            // A local impl's methods are found only in its extent
            // (`trait.impl.local.lookup`).
            if self.hidden.contains(&impl_def) || (visible_only && !self.method_visible(method)) {
                continue;
            }
            let Some(item) = self.cx.lookup.item(impl_def) else {
                continue;
            };
            let ItemData::Impl { self_ty, .. } = &item.data else {
                continue;
            };
            let n = item.generics.len();
            let vars: Vec<Ty> = (0..n)
                .map(|_| self.infer.fresh(pool, VarKind::General))
                .collect();
            let head = subst_owner(pool, impl_def, &vars, *self_ty);
            let snap = self.infer.snapshot();
            if self.infer.unify(pool, head, t).is_ok() {
                // The impl's bounds must hold (`impl[T < Eq] List[T]`).
                let ok = self.unmet_impl_bound(impl_def, &vars).is_none();
                if ok {
                    return Some((method, impl_def, vars));
                }
            }
            self.infer.rollback(snap);
        }
        None
    }

    /// Emits the built-in method `op` on `refs`, whose intrinsic gives
    /// `ret`.
    pub(crate) fn emit_builtin(
        &mut self,
        op: IntrinsicOp,
        refs: &[Ref],
        ret: Ty,
        n: NodeRef<'_>,
    ) -> (Ref, Ty) {
        let rec = self.b.refs_record(refs);
        let r = self.b.emit(Tag::Intrinsic, op as u32, rec, ret, n.index());
        (r, ret)
    }

    /// A built-in method of `List`, `Map` (spec/lang/10-modules.md#built-in-methods).
    fn builtin_method(&mut self, t: Ty, name: &str) -> Option<(IntrinsicOp, Vec<Ty>, Ty)> {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        let t = match pool.get(t) {
            TyData::Mut(i) => i,
            _ => t,
        };
        let TyData::Adt { def, args } = pool.get(t) else {
            return None;
        };
        let a = pool.list_items(args);
        let usize_t = Ty::prim(Prim::Usize);
        let known = self.cx.names.known;
        let which = if def == known.list {
            1
        } else if def == known.map {
            2
        } else {
            0
        };
        match (which, name) {
            (1, "len") => Some((IntrinsicOp::ListLen, vec![], usize_t)),
            (1, "push") => Some((IntrinsicOp::ListPush, vec![a[0]], Ty::VOID)),
            (2, "len") => Some((IntrinsicOp::MapLen, vec![], usize_t)),
            (2, "get") => Some((
                IntrinsicOp::MapGet,
                vec![a[0]],
                pool.intern_ty(&TyData::Option(a[1])),
            )),
            (2, "remove") => Some((
                IntrinsicOp::MapRemove,
                vec![a[0]],
                pool.intern_ty(&TyData::Option(a[1])),
            )),
            _ => None,
        }
    }

    pub(crate) fn resolve_method(&mut self, rt: Ty, name: &str) -> StageResult<Option<Hit>> {
        let pool = self.pool();
        let t = self.infer.resolve(pool, rt);
        self.add_opaque_bounds(t);
        let t = match pool.get(t) {
            TyData::Mut(i) => i,
            _ => t,
        };
        if matches!(pool.get(t), TyData::Infer(_)) && self.learn_waiting(t)? {
            return self.resolve_method(rt, name);
        }
        if let Some((method, impl_def, impl_args)) = self.find_inherent(t, name) {
            return Ok(Some(Hit::Inherent {
                method,
                impl_def,
                impl_args,
            }));
        }
        if let Some((op, params, ret)) = self.builtin_method(t, name) {
            return Ok(Some(Hit::Builtin { op, params, ret }));
        }
        let sym = self.cx.names.syms.intern(name);
        let traits = self.traits_declaring(sym);
        // A parameter: its bounds, supertraits included. A rigid
        // projection such as `I::IntoIter` has the bounds its trait
        // declares on the associated type (`trait.assoc.bound-use`).
        if matches!(
            pool.get(t),
            TyData::Param(_) | TyData::Opaque { .. } | TyData::Assoc { .. }
        ) {
            for i in 0..self.env.clause_self.len() {
                if self.env.clause_self[i] != t {
                    continue;
                }
                let tr = self.env.clause_trait[i];
                if let Some(&(_, m)) = traits.iter().find(|(x, _)| *x == tr) {
                    return Ok(Some(Hit::Trait {
                        trait_: tr,
                        method: m,
                        args: pool.list_items(self.env.clause_args[i]).to_vec(),
                        choice: (ChoiceKind::Bound, u32::try_from(i).unwrap_or(0)),
                    }));
                }
            }
            return Ok(None);
        }
        // A trait value: its trait and supertraits, a supertrait at the
        // arguments the value's trait gives it (trait.dyn.value-methods).
        if let TyData::TraitValue { def, args, .. } = pool.get(t) {
            for (tr, m) in &traits {
                if let Some(a) = self.super_args(def, t, args, *tr, 0) {
                    return Ok(Some(Hit::Trait {
                        trait_: *tr,
                        method: *m,
                        args: pool.list_items(a).to_vec(),
                        choice: (ChoiceKind::TraitValue, tr.raw()),
                    }));
                }
            }
            return Ok(None);
        }
        // A literal's type is its default once a member is asked of it.
        if matches!(pool.get(t), TyData::Infer(_))
            && let Some(k @ (VarKind::IntLit | VarKind::SignedIntLit | VarKind::FloatLit)) =
                self.infer.kind_of(pool, t)
        {
            let d = match k {
                VarKind::SignedIntLit => Ty::I32,
                VarKind::IntLit => Ty::prim(Prim::Usize),
                _ => Ty::prim(Prim::F64),
            };
            let _ = self.infer.unify(pool, t, d);
            return self.resolve_method(d, name);
        }
        if matches!(pool.get(t), TyData::Infer(_)) {
            return Err(hd_base::NotImplemented::new(
                hd_base::Stage::Body,
                "a method on a value whose type is not yet known",
            ));
        }
        self.trait_part(t, sym, &self.available_traits(sym))
    }

    /// The trait methods named `sym` of a concrete type: the
    /// compiler-answered traits, then the trait part of `Methods`
    /// (`crates/hd_check/DESIGN.md` (trait solver) §6.5), each other trait with the method by the
    /// impl candidates the type has. Two traits are ambiguous.
    fn trait_part(
        &mut self,
        t: Ty,
        sym: Symbol,
        traits: &[(DefId, DefId)],
    ) -> StageResult<Option<Hit>> {
        let pool = self.pool();
        let mut builtin = Vec::new();
        let mut asked = Vec::new();
        for &(tr, m) in traits {
            let n_args = self.cx.lookup.item(tr).map_or(0, |i| i.generics.len());
            let fresh: Vec<Ty> = (0..n_args)
                .map(|_| self.infer.fresh(pool, VarKind::General))
                .collect();
            let tref = TraitRef {
                trait_: tr,
                self_ty: t,
                args: pool.list(&fresh),
            };
            if self.supplied_holds(tref)?.is_some() {
                builtin.push((tr, m, fresh));
            } else {
                asked.push(tr);
            }
        }
        let goal = Goal::Methods {
            receiver: t,
            name: sym,
            traits: asked,
        };
        let cands = match self.solve_goal(&goal)? {
            Answer::Candidates(c) => c,
            Answer::Stalled { .. } => return self.waiting_trait(t, &builtin, traits),
            other => return unsupported(format!("solver answer {other:?} to a method lookup")),
        };
        // The candidates by trait, in content order of their first one.
        let mut groups: Vec<(DefId, Vec<Candidate>)> = Vec::new();
        for c in cands {
            let Some(table) = self.cx.impl_table(c.row.module) else {
                continue;
            };
            let tr = table.trait_[c.row.row as usize];
            match groups.iter_mut().find(|g| g.0 == tr) {
                Some(g) => g.1.push(c),
                None => groups.push((tr, vec![c])),
            }
        }
        if builtin.len() + groups.len() > 1 {
            let mut found: Vec<DefId> = builtin.iter().map(|b| b.0).collect();
            found.extend(groups.iter().map(|g| g.0));
            return Ok(Some(Hit::Ambiguous(found)));
        }
        if let Some((tr, m, args)) = builtin.pop() {
            return Ok(Some(Hit::Trait {
                trait_: tr,
                method: m,
                args,
                choice: (ChoiceKind::Builtin, 0),
            }));
        }
        Ok(groups.pop().and_then(|(tr, cands)| {
            let &(_, method) = traits.iter().find(|(x, _)| *x == tr)?;
            Some(Hit::Choice {
                trait_: tr,
                method,
                cands,
            })
        }))
    }

    /// The method of a receiver whose impl waits for inference: the one
    /// trait among `traits` the receiver may still implement. Its impl is
    /// chosen once the receiver is known, by a pending call goal.
    fn waiting_trait(
        &mut self,
        t: Ty,
        builtin: &[(DefId, DefId, Vec<Ty>)],
        traits: &[(DefId, DefId)],
    ) -> StageResult<Option<Hit>> {
        let mut open: Vec<DefId> = builtin.iter().map(|b| b.0).collect();
        for &(tr, _) in traits {
            if open.contains(&tr) {
                continue;
            }
            let goal = Goal::Instantiations {
                self_ty: t,
                trait_: tr,
                mut_: false,
            };
            match self.solve_goal(&goal)? {
                Answer::Fails(_) => {}
                Answer::Candidates(c) if c.is_empty() => {}
                _ => open.push(tr),
            }
        }
        match open.as_slice() {
            [tr] => Ok(traits
                .iter()
                .find(|(x, _)| x == tr)
                .map(|&(trait_, method)| Hit::Pending { trait_, method })),
            _ => unsupported("a method of several traits whose impl waits for inference"),
        }
    }

    /// `recv.name(args)`.
    pub(crate) fn method_on(
        &mut self,
        recv: Ref,
        rt: Ty,
        name: &str,
        args: &Args<'_>,
        (n, bang): (NodeRef<'_>, bool),
        want: Option<Ty>,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
        let dyn_recv = self.dyn_recv.take();
        let rt = self.norm_ty(rt);
        // A receiver whose type an earlier error left unknown adds nothing.
        if matches!(self.strip_mut(rt), Ty::NEVER | Ty::POISON) {
            for e in &args.positional {
                self.expr(*e, None)?;
            }
            return Ok((Ref(NONE), Ty::NEVER));
        }
        // A receiver whose address the call took (`fn.method.auto-address`).
        let addressed = self.addressed(recv, rt);
        let hit = self.resolve_method(rt, name)?;
        if matches!(
            hit,
            None | Some(
                Hit::Trait { .. } | Hit::Choice { .. } | Hit::Pending { .. } | Hit::Ambiguous(_)
            )
        ) && let Some(path) = self.promoted_method_path(rt, name)
        {
            // A trait candidate beside the promoted one: neither wins
            // (names.method-lookup.ambiguous).
            if hit.is_some() {
                let msg = format!(
                    "`{name}` on {} is ambiguous: a trait method and the promoted method `{}.{name}`; write `Trait::{name}(..)` or the explicit path",
                    self.show(rt),
                    path.join(".")
                );
                self.err(Code::AmbiguousMethod, n, &msg);
                return Ok((Ref(NONE), Ty::POISON));
            }
            let (recv, rt) = self.walk_path(recv, rt, &path, n);
            return self.method_on(recv, rt, name, args, (n, bang), want);
        }
        match hit {
            Some(Hit::Ambiguous(traits)) => {
                let names: Vec<String> = self
                    .cx
                    .names
                    .display_names(&traits)
                    .iter()
                    .map(|t| format!("`{t}`"))
                    .collect();
                let msg = format!(
                    "`{name}` on {} is ambiguous: the traits {} each supply it; write `Trait::{name}(..)`",
                    self.show(rt),
                    names.join(" and ")
                );
                self.err(Code::AmbiguousMethod, n, &msg);
                Ok((Ref(NONE), Ty::POISON))
            }
            Some(Hit::Choice {
                trait_,
                method,
                cands,
            }) => {
                let t = self.strip_mut(self.infer.resolve(pool, rt));
                if let Some(&(s, st)) = self.sig_of(method)?.params.first()
                    && self.cx.names.text(s) == "self"
                    && !addressed
                {
                    self.check_receiver(st, recv, rt, n);
                }
                let recv = match self.ref_receiver_kind(method)? {
                    Some(k) if !addressed => self.fit_call_receiver(recv, rt, k, name, n),
                    _ => recv,
                };
                self.choose_method(
                    (trait_, method, t),
                    &cands,
                    Some(recv),
                    args,
                    (n, bang),
                    want,
                )
            }
            Some(Hit::Pending { trait_, method }) => {
                let t = self.strip_mut(self.infer.resolve(pool, rt));
                if let Some(&(s, st)) = self.sig_of(method)?.params.first()
                    && self.cx.names.text(s) == "self"
                    && !addressed
                {
                    self.check_receiver(st, recv, rt, n);
                }
                let recv = match self.ref_receiver_kind(method)? {
                    Some(k) if !addressed => self.fit_call_receiver(recv, rt, k, name, n),
                    _ => recv,
                };
                self.trait_method_call(
                    TraitTarget {
                        trait_,
                        method,
                        self_ty: t,
                        args: vec![],
                    },
                    Some((recv, PENDING_CHOICE)),
                    args,
                    n,
                    (bang, want),
                )
            }
            Some(Hit::Inherent {
                method,
                impl_def,
                impl_args,
            }) => {
                let sig = self.sig_of(method)?;
                let explicit = std::mem::take(&mut self.method_targs);
                let mut vars = self.fresh_generics(&sig, &explicit, n);
                self.unsolvable_consts(&sig, method, explicit.len(), &mut vars, n);
                let inst = |x: Ty| {
                    subst_owner(
                        pool,
                        method,
                        &vars,
                        subst_owner(pool, impl_def, &impl_args, x),
                    )
                };
                let params: Vec<(Symbol, Ty)> =
                    sig.params.iter().map(|(s, x)| (*s, inst(*x))).collect();
                let has_self = sig
                    .params
                    .first()
                    .is_some_and(|p| self.cx.names.text(p.0) == "self");
                if !has_self {
                    let msg = format!("`{name}` is an associated function, not a method");
                    self.err(Code::UnknownMethod, n, &msg);
                    return Ok((Ref(NONE), Ty::POISON));
                }
                let recv = match self.ref_receiver_kind(method)? {
                    Some(k) if !addressed => self.fit_call_receiver(recv, rt, k, name, n),
                    Some(_) => recv,
                    None => {
                        self.check_receiver(params[0].1, recv, rt, n);
                        recv
                    }
                };
                let mut refs = vec![recv];
                let mut all = impl_args.clone();
                all.extend(&vars);
                self.default_owner = Some((method, pool.list(&all), vec![refs[0]]));
                let inferred = inferred_formals(self, method, &sig, &explicit, &params);
                let closure_wants = fn_bound_formals(self, method, &sig, &params, &inst);
                let formals = Formals {
                    params: &params,
                    defaults: &sig.defaults,
                    variadic: sig.variadic,
                    inferred: &inferred,
                    closure_wants: Some(&closure_wants),
                };
                refs.extend(self.check_args(&formals, 1, args, n, name)?);
                self.fill_method_defaults(&sig, &vars, &inst, want)?;
                self.open_call_params(&vars, &refs, n);
                // `h.fact::[D]()` on a structure handle reads a typed fact
                // (annot.handle.fact.typed): `D` is matched against the
                // member's type, not bounded by `Inspectable`.
                let handle_fact = name == "fact"
                    && matches!(
                        pool.get(self.strip_mut(rt)),
                        TyData::Adt { def, .. } if def == self.cx.names.known.field
                    );
                if handle_fact {
                    if let Some(&m) = vars.first() {
                        self.handle_fact_type(rt, m, n)?;
                    }
                } else {
                    self.bounds_of(&sig, &vars, &inst, n)?;
                }
                self.inherent_row(method, &sig, (impl_def, &impl_args), &vars, n)?;
                let mut targs = impl_args.clone();
                targs.extend(&vars);
                let c = Callee::Item {
                    def: method,
                    targs: pool.list(&targs),
                };
                let ret = self.normalize_deep(inst(sig.ret))?;
                Ok(self.emit_call(&c, &refs, ret, sig.suspends, bang, n))
            }
            Some(Hit::Builtin { op, params, ret }) => {
                if matches!(op, IntrinsicOp::ListPush | IntrinsicOp::MapRemove) {
                    // `push` and `remove` change the receiver in place
                    // (`types.coll.writable.call`).
                    self.coll_change(recv, rt, "this call", n);
                    // An element receiver's access begins before the
                    // arguments run (`fn.method.eval-order`,
                    // `ref.elem.access.live`).
                    if let Some((coll, _)) = self.element_of(recv) {
                        self.begin_access(coll);
                    }
                }
                // The spec's declared parameter name, so a named argument
                // matches (`module.method.normative`).
                let pname = match op {
                    IntrinsicOp::MapGet | IntrinsicOp::MapRemove => "key",
                    _ => "value",
                };
                let ps: Vec<(Symbol, Ty)> = params
                    .iter()
                    .map(|t| (self.cx.names.syms.intern(pname), *t))
                    .collect();
                let mut refs = vec![recv];
                let mut full = vec![(self.cx.names.syms.intern("self"), rt)];
                full.extend(ps);
                // Builtin signatures are fixed: the join check stays off.
                let inferred: Vec<Vec<usize>> = vec![Vec::new(); full.len()];
                // A builtin's parameters are the collection's type
                // arguments, never references.
                let formals = Formals {
                    params: &full,
                    defaults: &[],
                    variadic: false,
                    inferred: &inferred,
                    closure_wants: None,
                };
                refs.extend(self.check_args(&formals, 1, args, n, name)?);
                if matches!(op, IntrinsicOp::ListPush | IntrinsicOp::MapRemove) {
                    self.length_changed(recv, n.index());
                }
                Ok(self.emit_builtin(op, &refs, ret, n))
            }
            Some(Hit::Trait {
                trait_,
                method,
                args: targs,
                choice,
            }) => {
                let t = self.infer.resolve(pool, rt);
                let t = match pool.get(t) {
                    TyData::Mut(i) => i,
                    _ => t,
                };
                // `annot.walker.generic-member-hook-call`: a generic
                // structure member hook is for generated code only. The
                // protocol's own `Self` still calls it, as a default hook must.
                if crate::derive::structure_member_role(
                    self.cx.names,
                    self.cx.lookup,
                    trait_,
                    method,
                )
                .is_some()
                    && let TyData::Param(p) = pool.get(t)
                    && (p.owner != trait_ || p.index != 0)
                {
                    let msg = format!(
                        "`{name}` through a generic `{}` may only be called by generated code",
                        self.cx.names.display_name(trait_)
                    );
                    self.err(Code::GenericMemberCall, n, &msg);
                }
                // A trait value read through `&dyn` or `&mut dyn`
                // (`trait.dyn.ref.member`).
                let via_dyn = dyn_recv.filter(|_| choice.0 == ChoiceKind::TraitValue);
                if choice.0 == ChoiceKind::TraitValue && pool.has_opaque(self.sig_of(method)?.ret) {
                    for e in &args.positional {
                        self.expr(*e, Some(Ty::POISON))?;
                    }
                    self.err(Code::DynMemberUnavailable, n, &format!(
                        "`{name}` returns `impl Trait`, which is unavailable on a trait value; use a generic bound instead"
                    ));
                    return Ok((Ref(NONE), Ty::NEVER));
                }
                if let Some(&(s, st)) = self.sig_of(method)?.params.first()
                    && self.cx.names.text(s) == "self"
                {
                    let by_ref = matches!(pool.get(st), TyData::Ref { .. });
                    if let Some((mutable, at)) = via_dyn {
                        let changes = match pool.get(st) {
                            TyData::Ref { mutable, .. } => mutable,
                            TyData::Mut(_) => true,
                            _ => false,
                        };
                        if changes && !mutable {
                            for e in &args.positional {
                                self.expr(*e, Some(Ty::POISON))?;
                            }
                            self.dyn_ref_mut_receiver(name, at, n);
                            return Ok((Ref(NONE), Ty::NEVER));
                        }
                    }
                    // A trait value reference passes its payload as the
                    // reference receiver.
                    let passes_payload = by_ref && via_dyn.is_some();
                    if !(addressed || passes_payload) {
                        self.check_receiver(st, recv, rt, n);
                    }
                    if choice.0 == ChoiceKind::TraitValue
                        && self.names_self_beyond_receiver(method, trait_)?
                    {
                        // The receiver is the only place a trait value can
                        // supply `Self` (trait.dyn.member.self).
                        for e in &args.positional {
                            self.expr(*e, Some(Ty::POISON))?;
                        }
                        let msg = format!(
                            "`{name}` names `Self` outside its receiver, which is unavailable on a trait value"
                        );
                        self.err(Code::DynMemberUnavailable, n, &msg);
                        return Ok((Ref(NONE), Ty::NEVER));
                    }
                } else if choice.0 == ChoiceKind::TraitValue {
                    // An associated function has no receiver for a trait
                    // value to dispatch on (trait.dyn.member.assoc-function).
                    for e in &args.positional {
                        self.expr(*e, Some(Ty::POISON))?;
                    }
                    let msg = format!(
                        "`{name}` is an associated function, which is unavailable on a trait value"
                    );
                    self.err(Code::DynMemberUnavailable, n, &msg);
                    return Ok((Ref(NONE), Ty::NEVER));
                }
                // A trait value passes its payload to a reference receiver
                // itself (`hd_mono::SlotRecv`).
                let recv = match self.ref_receiver_kind(method)? {
                    Some(k) if !addressed && choice.0 != ChoiceKind::TraitValue => {
                        self.fit_call_receiver(recv, rt, k, name, n)
                    }
                    _ => recv,
                };
                self.trait_method_call(
                    TraitTarget {
                        trait_,
                        method,
                        self_ty: t,
                        args: targs,
                    },
                    Some((recv, choice)),
                    args,
                    n,
                    (bang, want),
                )
            }
            None => {
                // A method call never reads a field
                // (`expr.member.no-field-call`); a field of that name only
                // earns the hint (`expr.member.stored-fn.hint`).
                let advice = if self.field_of(rt, name).is_some() {
                    format!("; to call the field, write `(x.{name})(..)`")
                } else {
                    String::new()
                };
                let msg = format!("no method `{name}` on {}{advice}", self.show(rt));
                if self.no_method(rt, name, n, &msg) && name == "append" {
                    self.append_fix((recv, rt), args, n);
                }
                Ok((Ref(NONE), Ty::POISON))
            }
        }
    }

    /// Whether a trait method's signature names `Self` anywhere but its
    /// receiver (trait.dyn.member.self). `Self::Assoc` projections do not
    /// count: the associated type is bound by the `dyn` type.
    fn names_self_beyond_receiver(&self, method: DefId, trait_: DefId) -> StageResult<bool> {
        let pool = self.pool();
        let sig = self.sig_of(method)?;
        let has_receiver = sig
            .params
            .first()
            .is_some_and(|p| self.cx.names.text(p.0) == "self");
        let rest = sig.params.iter().skip(usize::from(has_receiver));
        let tys = rest
            .map(|p| p.1)
            .chain([sig.ret])
            .chain(sig.generics.iter().flat_map(|g| g.bounds.iter().copied()));
        let mut found = tys.into_iter().any(|t| pool.names_trait_self(t, trait_));
        found |= pool.row_names_trait_self(sig.row, trait_);
        Ok(found)
    }

    /// A trait method call: `Self` is `self_ty`, the trait's parameters
    /// are `targs` (fresh when empty), the receiver is `recv` when given.
    pub(crate) fn trait_method_call(
        &mut self,
        target: TraitTarget,
        recv: Option<(Ref, (ChoiceKind, u32))>,
        args: &Args<'_>,
        n: NodeRef<'_>,
        (bang, want): (bool, Option<Ty>),
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
        let TraitTarget {
            trait_,
            method,
            self_ty,
            args: mut targs,
        } = target;
        self.note_call(method);
        let sig = self.sig_of(method)?;
        let n_trait = self.cx.lookup.item(trait_).map_or(0, |i| i.generics.len());
        while targs.len() < n_trait {
            targs.push(self.infer.fresh(pool, VarKind::General));
        }
        let explicit = std::mem::take(&mut self.method_targs);
        let mut vars = self.fresh_generics(&sig, &explicit, n);
        self.unsolvable_consts(&sig, method, explicit.len(), &mut vars, n);
        let tv = targs.clone();
        let tl = pool.list(&tv);
        let inst = |x: Ty| {
            let x = subst_owner(pool, method, &vars, x);
            let x = pool.subst(x, &|p: ParamRef| {
                if p.owner != trait_ {
                    return None;
                }
                if p.index == 0 {
                    Some(self_ty)
                } else {
                    tv.get(p.index as usize - 1).copied()
                }
            });
            with_assoc_args(pool, x, trait_, tl)
        };
        let params: Vec<(Symbol, Ty)> = sig.params.iter().map(|(s, x)| (*s, inst(*x))).collect();
        let has_self = sig
            .params
            .first()
            .is_some_and(|p| self.cx.names.text(p.0) == "self");
        let mut refs = Vec::new();
        let skip = match recv {
            Some((r, _)) if has_self => {
                refs.push(r);
                1
            }
            _ => 0,
        };
        let inferred = inferred_formals(self, method, &sig, &explicit, &params);
        let closure_wants = fn_bound_formals(self, method, &sig, &params, &inst);
        let formals = Formals {
            params: &params,
            defaults: &sig.defaults,
            variadic: sig.variadic,
            inferred: &inferred,
            closure_wants: Some(&closure_wants),
        };
        let method_name = self
            .cx
            .names
            .text(
                self.cx
                    .lookup
                    .item(method)
                    .map_or(Symbol::from_raw(0), |i| i.name),
            )
            .to_owned();
        // An omitted argument takes the trait declaration's default
        // (`trait.impl.param-default`), whose body shares the trait
        // method's arguments: `Self`, the trait's, then the method's.
        let mut owner_args = vec![self_ty];
        owner_args.extend(&targs);
        owner_args.extend(&vars);
        self.default_owner = Some((method, pool.list(&owner_args), refs.clone()));
        refs.extend(self.check_args(&formals, skip, args, n, &method_name)?);
        if pool.has_opaque(sig.ret) {
            let self_ty = self.strip_mut(self.infer.resolve(pool, self_ty));
            let inner = self.ref_of(self_ty).map_or(self_ty, |(t, _)| t);
            if matches!(pool.get(inner), TyData::TraitValue { .. }) {
                self.err(Code::DynMemberUnavailable, n, &format!(
                    "`{method_name}` returns `impl Trait`, which is unavailable on a trait value; use a generic bound instead"
                ));
                return Ok((Ref(NONE), Ty::NEVER));
            }
        }
        self.fill_method_defaults(&sig, &vars, &inst, want)?;
        self.open_call_params(&vars, &refs, n);
        self.bounds_of(&sig, &vars, &inst, n)?;
        // At an opt-in, or in a tuple template's plan, a `walk`,
        // `describe` or `build` call's walker, describer or source type
        // carries the member obligations.
        if let Some(imp) = self.in_template
            && trait_ == self.cx.names.known.structure
            && matches!(method_name.as_str(), "walk" | "describe" | "build")
        {
            crate::results::note_structure_read(self.cx, imp);
        }
        if (self.opt_in.is_some() || self.plan_sink.is_some())
            && trait_ == self.cx.names.known.structure
            && let Some(&w) = vars.first()
        {
            self.structure_calls.push((method, w));
        }
        let row = self.call_row(method, &sig, &vars, 0);
        let row = pool.subst_row(row, &|p| {
            (p.owner == trait_ && p.index > 0)
                .then(|| tv.get(p.index as usize - 1).copied())
                .flatten()
        });
        self.check_row(self.infer.resolve_row(pool, row), n);
        let choice = match recv {
            Some((_, c)) if c != PENDING_CHOICE => c,
            _ => {
                let st = self.infer.resolve(pool, self_ty);
                let tref = TraitRef {
                    trait_,
                    self_ty: st,
                    args: pool.list(&targs),
                };
                self.call_choice(tref, method, n)?
            }
        };
        let ret = self.normalize_deep(inst(sig.ret))?;
        let known = &self.cx.names.known;
        if ((trait_ == known.fn_trait && method == known.fn_call)
            || (trait_ == known.fn_mut_trait && method == known.fn_mut_call))
            && choice != PENDING_CHOICE
        {
            let Some(&receiver) = refs.first() else {
                return unsupported("`Fn.call` without a receiver");
            };
            let receiver_ty = self.b.ty_of(receiver);
            let (function, _) = self.emit_deref(receiver, receiver_ty, n.index());
            let rec = self.b.refs_record(&refs[1..]);
            if bang {
                self.check_bang(false, n);
            }
            return Ok((
                self.b.emit(Tag::CallValue, function.0, rec, ret, n.index()),
                ret,
            ));
        }
        let mut all = targs.clone();
        all.extend(&vars);
        let c = Callee::TraitMethod {
            trait_,
            method,
            self_ty: self.infer.resolve(pool, self_ty),
            targs: pool.list(&all),
            choice,
        };
        Ok(self.emit_call(&c, &refs, ret, sig.suspends, bang, n))
    }

    /// `trait_.name(recv, args...)` for operators, `Display` and `iter`.
    pub(crate) fn trait_call(
        &mut self,
        trait_: DefId,
        name: &str,
        recv: Ref,
        rt: Ty,
        args: &[(Ref, Ty, NodeRef<'_>)],
        n: NodeRef<'_>,
    ) -> StageResult<(Ref, Ty)> {
        self.trait_call_args(trait_, name, recv, rt, args, n)
    }

    pub(crate) fn trait_call_args(
        &mut self,
        trait_: DefId,
        name: &str,
        recv: Ref,
        rt: Ty,
        args: &[(Ref, Ty, NodeRef<'_>)],
        n: NodeRef<'_>,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
        let method = self.cx.names.member(trait_, PathKind::Member, name);
        let sig = self.sig_of(method)?;
        let t = self.infer.resolve(pool, rt);
        let t = match pool.get(t) {
            TyData::Mut(i) => i,
            _ => t,
        };
        let n_trait = self.cx.lookup.item(trait_).map_or(0, |i| i.generics.len());
        let mut targs: Vec<Ty> = (0..n_trait)
            .map(|_| self.infer.fresh(pool, VarKind::General))
            .collect();
        // An operator's right operand fixes `Rhs` when the impl is generic.
        if let (Some(first), Some((_, at, _))) = (targs.first().copied(), args.first())
            && sig.params.get(1).is_some_and(
                |p| matches!(pool.get(p.1), TyData::Param(q) if q.owner == trait_ && q.index == 1),
            )
        {
            let _ = self.infer.unify(pool, first, *at);
        }
        let tref = TraitRef {
            trait_,
            self_ty: t,
            args: pool.list(&targs),
        };
        let code = if trait_ == self.cx.names.known.index && n.kind() == SyntaxKind::IndexExpr {
            Code::TypeMismatch
        } else {
            Code::UnsatisfiedTraitBound
        };
        let ev = self.require_ref_noting(tref, n, code)?;
        let choice = self.choice_of(ev.as_ref());
        targs = targs
            .into_iter()
            .map(|x| self.infer.resolve(pool, x))
            .collect();
        let tv = targs.clone();
        let tl = pool.list(&tv);
        let inst = |x: Ty| {
            let x = pool.subst(x, &|p: ParamRef| {
                if p.owner != trait_ {
                    return None;
                }
                if p.index == 0 {
                    Some(t)
                } else {
                    tv.get(p.index as usize - 1).copied()
                }
            });
            with_assoc_args(pool, x, trait_, tl)
        };
        let mut refs = vec![recv];
        for (i, (r, at, an)) in args.iter().enumerate() {
            let w = sig.params.get(i + 1).map(|p| inst(p.1));
            refs.push(match w {
                Some(w) => self.coerce(*r, *at, w, *an, "operand", ExpectPosition::Operand),
                None => *r,
            });
        }
        let ret = self.normalize_deep(inst(sig.ret))?;
        let c = Callee::TraitMethod {
            trait_,
            method,
            self_ty: t,
            targs: pool.list(&targs),
            choice,
        };
        Ok(self.emit_call(&c, &refs, ret, sig.suspends, false, n))
    }

    // ------------------------------------------------------------ projections

    /// The trait that declares `name` for a projection on `base`.
    pub(crate) fn assoc_owner(&self, base: Ty, name: &str) -> Option<(DefId, TyList)> {
        let sym = self.cx.names.syms.intern(name);
        let has = |tr: DefId| match self.cx.lookup.item(tr).map(|i| &i.data) {
            Some(ItemData::Trait(t)) => t.assoc.iter().any(|(s, _)| *s == sym),
            _ => false,
        };
        (0..self.env.clause_self.len())
            .find(|&i| self.env.clause_self[i] == base && has(self.env.clause_trait[i]))
            .map(|i| (self.env.clause_trait[i], self.env.clause_args[i]))
    }

    /// A bound's associated-type bindings at a use site
    /// (`trait.binding.use-site`): each projection of the argument
    /// normalizes and unifies with the bound type, which infers a
    /// parameter named there (`trait.binding.inference`). A projection
    /// still waiting on inference is skipped.
    pub(crate) fn check_bindings(
        &mut self,
        self_ty: Ty,
        bound: Ty,
        n: NodeRef<'_>,
    ) -> StageResult<()> {
        let pool = self.pool();
        let TyData::TraitValue {
            def,
            args,
            bindings,
        } = pool.get(bound)
        else {
            return Ok(());
        };
        let st = self.strip_mut(self.infer.resolve(pool, self_ty));
        if matches!(st, Ty::NEVER | Ty::POISON) {
            return Ok(());
        }
        for (key, want) in bindings {
            let proj = pool.intern_ty(&TyData::Assoc {
                assoc: key,
                trait_: def,
                self_ty: st,
                args,
            });
            let got = self.normalize_deep(proj)?;
            // Waiting on inference, or no impl (the bound itself reported it).
            if matches!(pool.get(got), TyData::Assoc { self_ty, .. }
                if !matches!(pool.get(self.strip_mut(self_ty)), TyData::Param(_)))
                || pool.has_poison(got)
            {
                continue;
            }
            let snap = self.infer.snapshot();
            let unified = self.infer.unify(pool, got, want).is_ok();
            if unified {
                // An associated binding is type equality, including function
                // rows: ordinary unification deliberately leaves rows alone.
                self.solve_nested_rows(got, want, 1);
            }
            if !unified || !self.perm_fits(got, want, false, 1) {
                self.infer.rollback(snap);
                let msg = format!(
                    "{} does not satisfy {}: its `{}` is {}, not {}",
                    self.show(st),
                    self.cx.names.display_name(def),
                    self.assoc_name(key),
                    self.show(got),
                    self.show(want)
                );
                self.err(Code::UnsatisfiedTraitBound, n, &msg);
            }
        }
        Ok(())
    }

    /// The name of an associated item or binding key.
    fn assoc_name(&self, d: DefId) -> &str {
        self.cx
            .names
            .paths
            .segment(hd_base::PathId::from_raw(d.raw()))
    }

    /// The trait that declares the associated type `name`: `trait_` or
    /// one of its supertraits, with its arguments over `self_ty`, and the
    /// associated item (`trait.binding.name-reach`).
    fn assoc_decl(
        &self,
        trait_: DefId,
        self_ty: Ty,
        args: TyList,
        name: &str,
        depth: u32,
    ) -> Option<(DefId, TyList, DefId)> {
        let pool = self.pool();
        let Some(ItemData::Trait(t)) = self.cx.lookup.item(trait_).map(|i| &i.data) else {
            return None;
        };
        if let Some(&(_, d)) = t.assoc.iter().find(|(s, _)| self.cx.names.text(*s) == name) {
            return Some((trait_, args, d));
        }
        if depth > 16 {
            return None;
        }
        let known = pool.list_items(args);
        for s in &t.supers {
            let s = pool.subst(*s, &|p: ParamRef| {
                if p.owner != trait_ {
                    None
                } else if p.index == 0 {
                    Some(self_ty)
                } else {
                    known.get(p.index as usize - 1).copied()
                }
            });
            if let TyData::TraitValue { def, args, .. } = pool.get(s)
                && let Some(x) = self.assoc_decl(def, self_ty, args, name, depth + 1)
            {
                return Some(x);
            }
        }
        None
    }

    /// The arguments of `target` as `trait_[args]` or one of its
    /// transitive supertraits, `Self` replaced by `self_ty`: the first
    /// found depth first in declared order.
    fn super_args(
        &self,
        trait_: DefId,
        self_ty: Ty,
        args: TyList,
        target: DefId,
        depth: u32,
    ) -> Option<TyList> {
        if trait_ == target {
            return Some(args);
        }
        let pool = self.pool();
        let Some(ItemData::Trait(t)) = self.cx.lookup.item(trait_).map(|i| &i.data) else {
            return None;
        };
        if depth > 16 {
            return None;
        }
        let known = pool.list_items(args);
        for s in &t.supers {
            let s = pool.subst(*s, &|p: ParamRef| {
                if p.owner != trait_ {
                    None
                } else if p.index == 0 {
                    Some(self_ty)
                } else {
                    known.get(p.index as usize - 1).copied()
                }
            });
            if let TyData::TraitValue { def, args, .. } = pool.get(s)
                && let Some(x) = self.super_args(def, self_ty, args, target, depth + 1)
            {
                return Some(x);
            }
        }
        None
    }

    /// A projection's normal form, itself normalized once more; a cycle of
    /// bindings stops at the depth limit.
    fn normalized(&mut self, x: Ty) -> StageResult<Ty> {
        // A binding in error (a header's unknown binding name) is poison:
        // its poison types come from that error, as a poisoned name's do.
        if self.pool().has_poison(x) {
            self.read_poison_name = true;
        }
        if self.norm_depth > 32 {
            return Ok(x);
        }
        self.norm_depth += 1;
        let r = self.normalize_deep(x);
        self.norm_depth -= 1;
        r
    }

    /// `t` with its projections normalized as far as inference allows;
    /// a type without projections is returned as is.
    /// Make only the declared bounds of opaque results available. This reads
    /// type descriptors, never a defining body or its hidden representation.
    pub(crate) fn add_opaque_bounds(&mut self, t: Ty) {
        let pool = self.pool();
        if !pool.has_opaque(t) {
            return;
        }
        let before = self.env.clause_self.len();
        let mut todo = vec![t];
        let mut seen = Vec::new();
        while let Some(t) = todo.pop() {
            if !pool.has_opaque(t) || seen.contains(&t) {
                continue;
            }
            seen.push(t);
            match pool.get(t) {
                TyData::Opaque { bound, args, .. } => {
                    self.add_bound(t, bound, 0);
                    todo.extend_from_slice(pool.list_items(args));
                }
                TyData::Adt { args, .. } => todo.extend_from_slice(pool.list_items(args)),
                TyData::Tuple { elems, rest } => {
                    todo.extend_from_slice(pool.list_items(elems));
                    todo.extend(rest);
                }
                TyData::Option(i) | TyData::Mut(i) | TyData::Ref { inner: i, .. } => todo.push(i),
                TyData::Fn {
                    params,
                    result,
                    row,
                    ..
                } => {
                    todo.extend_from_slice(pool.list_items(params));
                    todo.push(result);
                    todo.extend(pool.row_data(row).keys);
                }
                TyData::TraitValue { args, bindings, .. } => {
                    todo.extend_from_slice(pool.list_items(args));
                    todo.extend(bindings.into_iter().map(|(_, t)| t));
                }
                TyData::Assoc { self_ty, args, .. } => {
                    todo.push(self_ty);
                    todo.extend_from_slice(pool.list_items(args));
                }
                TyData::Row(r) | TyData::Context(r) => todo.extend(pool.row_data(r).keys),
                _ => {}
            }
        }
        if self.env.clause_self.len() != before {
            self.env.key = Some(self.cx.global.env_key(&self.env));
        }
    }

    pub(crate) fn norm_ty(&mut self, t: Ty) -> Ty {
        let pool = self.pool();
        let r = self.infer.resolve(pool, t);
        self.add_opaque_bounds(r);
        if !pool.has_assoc(r) {
            return t;
        }
        self.normalize_deep(r).unwrap_or(r)
    }

    /// Normalizes projections whose self type is known (`crates/hd_check/DESIGN.md` (trait solver)
    /// §4.3): a bound's binding, a trait value's binding, or an impl's
    /// binding. A projection on a parameter with no binding stays rigid,
    /// named by the trait that declares it.
    pub(crate) fn normalize(&mut self, t: Ty) -> StageResult<Ty> {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        let TyData::Assoc {
            assoc,
            trait_,
            self_ty,
            args,
        } = pool.get(t)
        else {
            return Ok(t);
        };
        let st = match pool.get(self_ty) {
            TyData::Mut(i) => i,
            _ => self_ty,
        };
        let name = self.assoc_name(assoc).to_owned();
        let (trait_, args, assoc) = self
            .assoc_decl(trait_, st, args, &name, 0)
            .unwrap_or((trait_, args, assoc));
        let bound = |bs: &[(DefId, Ty)], me: &Self| {
            bs.iter()
                .find(|(d, _)| me.assoc_name(*d) == name)
                .map(|(_, b)| *b)
        };
        match pool.get(st) {
            TyData::Ref { inner, .. } if matches!(pool.get(inner), TyData::TraitValue { .. }) => {
                let TyData::TraitValue { bindings, .. } = pool.get(inner) else {
                    unreachable!()
                };
                return match bound(&bindings, self) {
                    Some(binding) => self.normalized(binding),
                    None => Ok(t),
                };
            }
            TyData::Param(_) | TyData::Opaque { .. } => {
                // A binding on the parameter's bound or on a supertrait
                // clause it reaches (`trait.binding.inside`).
                let found = (0..self.env.clause_self.len())
                    .filter(|&i| self.env.clause_self[i] == st)
                    .find_map(|i| bound(&self.env.clause_bindings[i], self));
                if let Some(b) = found {
                    return self.normalized(b);
                }
                // Implicit trait arguments (`Self::Error` in a generic
                // trait) are the bound's own, so the projection is the
                // one a call of a sibling method returns.
                let n_trait = self.cx.lookup.item(trait_).map_or(0, |i| i.generics.len());
                let args = if pool.list_items(args).len() < n_trait {
                    (0..self.env.clause_self.len())
                        .find(|&i| {
                            self.env.clause_self[i] == st && self.env.clause_trait[i] == trait_
                        })
                        .map_or(args, |i| self.env.clause_args[i])
                } else {
                    args
                };
                return Ok(pool.intern_ty(&TyData::Assoc {
                    assoc,
                    trait_,
                    self_ty: st,
                    args,
                }));
            }
            // `trait.dyn.bound.reference-projection`: the trait value's binding.
            TyData::TraitValue { bindings, .. } => {
                return match bound(&bindings, self) {
                    Some(b) => self.normalized(b),
                    None => Ok(t),
                };
            }
            _ => {}
        }
        // A self type that inference has not finished still normalizes
        // when exactly one impl fits whatever its variables become.
        // `P::Error` under a bound `P < Walker[Self]` leaves the trait's
        // arguments implicit: the impl that matches supplies them.
        let n_trait = self.cx.lookup.item(trait_).map_or(0, |i| i.generics.len());
        let mut av = pool.list_items(args).to_vec();
        while av.len() < n_trait {
            av.push(self.infer.fresh(pool, VarKind::General));
        }
        let tref = TraitRef {
            trait_,
            self_ty: st,
            args: pool.list(&av),
        };
        let answer = self.solve(tref)?;
        let stalled = matches!(answer, Answer::Stalled { .. });
        // A rejected trait has no associated value. Its bound reports the
        // error; consumers of this projection must not add follow-ons.
        if matches!(answer, Answer::Fails(_)) {
            return Ok(Ty::POISON);
        }
        if let hd_types::solver::Answer::Holds {
            evidence:
                Evidence::Impl {
                    row,
                    args: impl_args,
                },
            learned,
        } = answer
            && let Some(table) = self.cx.impl_table(row.module)
        {
            // The one impl that fits fixes the trait's arguments, which
            // the solver learned from its head; the self type's own
            // variables stay open (`List[?]::IntoIter` is `ListIter[?]`).
            for (v, x) in learned {
                let var = pool.intern_ty(&TyData::Infer(v));
                let _ = self.infer.unify(pool, var, x);
            }
            let r = row.row as usize;
            let impl_def = table.def[r];
            if let Some((_, b)) = table.assoc[r].iter().find(|(d, _)| *d == assoc) {
                let ia = pool.list_items(impl_args);
                let x = subst_owner(pool, impl_def, ia, *b);
                return self.normalized(x);
            }
        }
        // An impl whose head alone fits, with bounds that wait on the self
        // type's literals, still names the projection: under
        // `impl[T < Integer] Iterable[T] for Range[T]`, `Range[?]::IntoIter`
        // is `RangeIter[?]`. The bounds are the trait goal's to check.
        if stalled
            && let Answer::Candidates(cands) =
                self.solve_goal(&hd_types::solver::Goal::Instantiations {
                    self_ty: st,
                    trait_,
                    mut_: false,
                })?
            && let [c] = &cands[..]
            && let Some(table) = self.cx.impl_table(c.row.module)
        {
            let r = c.row.row as usize;
            let impl_def = table.def[r];
            let ia = pool.list_items(c.impl_args);
            // A binding that reads no parameter the self type left open,
            // as `never` under `impl[S] Walker[S] for Encoder`.
            let open = |i: usize| {
                let index = u16::try_from(i).unwrap_or(u16::MAX);
                ia[i]
                    == pool.intern_ty(&TyData::Param(ParamRef {
                        owner: impl_def,
                        index,
                    }))
            };
            if let Some((_, b)) = table.assoc[r].iter().find(|(d, _)| *d == assoc)
                && (0..ia.len())
                    .all(|i| !open(i) || !crate::trial::names_param(pool, *b, impl_def, i))
            {
                let x = subst_owner(pool, impl_def, ia, *b);
                return self.normalized(x);
            }
        }
        // A trait's default for the associated type, once the implementing
        // type is known.
        if pool.has_infer(st) {
            return Ok(t);
        }
        if let Some(ItemData::AssocType {
            default: Some(d), ..
        }) = self.cx.lookup.item(assoc).map(|i| &i.data)
        {
            let x = pool.subst(*d, &|p: ParamRef| {
                (p.owner == trait_ && p.index == 0).then_some(st)
            });
            return self.normalized(x);
        }
        Ok(t)
    }
}

impl Ck<'_, '_> {
    /// Normalizes every projection inside a type.
    pub(crate) fn normalize_deep(&mut self, t: Ty) -> StageResult<Ty> {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        let list = |me: &mut Self, l: TyList| -> StageResult<TyList> {
            let mut v = Vec::new();
            for x in pool.list_items(l).iter().copied() {
                v.push(me.normalize_deep(x)?);
            }
            Ok(pool.list(&v))
        };
        let d = match pool.get(t) {
            TyData::Assoc {
                assoc,
                trait_,
                self_ty,
                args,
            } => {
                let s = self.normalize_deep(self_ty)?;
                let a = list(self, args)?;
                let x = pool.intern_ty(&TyData::Assoc {
                    assoc,
                    trait_,
                    self_ty: s,
                    args: a,
                });
                return self.normalize(x);
            }
            TyData::Adt { def, args } => TyData::Adt {
                def,
                args: list(self, args)?,
            },
            TyData::Tuple { elems, rest } => TyData::Tuple {
                elems: list(self, elems)?,
                rest,
            },
            TyData::Option(i) => TyData::Option(self.normalize_deep(i)?),
            TyData::Mut(i) => TyData::Mut(self.normalize_deep(i)?),
            TyData::Ref {
                inner,
                mutable,
                counted,
            } => TyData::Ref {
                inner: self.normalize_deep(inner)?,
                mutable,
                counted,
            },
            TyData::Fn {
                params,
                result,
                row,
                suspends,
                inputs,
                deps,
            } => TyData::Fn {
                params: list(self, params)?,
                result: self.normalize_deep(result)?,
                row,
                suspends,
                inputs,
                deps,
            },
            _ => return Ok(t),
        };
        Ok(pool.intern_ty(&d))
    }
}

impl Ck<'_, '_> {
    /// Inspect a constructor expectation without erasing inference-variable
    /// identities in its arguments (including opaque return occurrences).
    pub(crate) fn expected_shape(&self, ty: Ty) -> Ty {
        let pool = self.pool();
        let ty = self.infer.shallow(pool, ty);
        match pool.get(ty) {
            TyData::Mut(inner) => self.infer.shallow(pool, inner),
            _ => ty,
        }
    }

    /// A type with its outer `mut` view removed, variables resolved.
    pub(crate) fn strip_mut(&self, t: Ty) -> Ty {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        match pool.get(t) {
            TyData::Mut(i) => i,
            _ => t,
        }
    }

    /// The template's `T` when the body is a template's method and `tr`
    /// is `Structure` or the template's own trait
    /// (`annot.template.qualified-self`).
    pub(crate) fn template_self(&mut self, tr: DefId) -> Option<Ty> {
        let item = self.b.body_mut().item;
        let ItemData::Method { owner, .. } = &self.cx.lookup.item(item)?.data else {
            return None;
        };
        let ItemData::Impl {
            trait_,
            self_ty,
            kind: hd_resolve::ImplKind::Template | hd_resolve::ImplKind::TupleTemplate,
            ..
        } = &self.cx.lookup.item(*owner)?.data
        else {
            return None;
        };
        (tr == *trait_ || tr == self.cx.names.known.structure).then_some(*self_ty)
    }

    /// The impl choice of a trait method call: the evidence of `tref`, or,
    /// when that waits for inference, [`PENDING_CHOICE`] and a pending
    /// call goal that learns the trait's arguments once the receiver is
    /// known.
    pub(crate) fn call_choice(
        &mut self,
        tref: TraitRef,
        method: DefId,
        n: NodeRef<'_>,
    ) -> StageResult<(ChoiceKind, u32)> {
        let before = self.pending.len();
        let ev = self.require_ref(tref, n)?;
        if let Some(p) = self.pending.get_mut(before) {
            p.kind = crate::body::Waiting::Call { method };
            return Ok(PENDING_CHOICE);
        }
        Ok(self.choice_of(ev.as_ref()))
    }

    /// The callee choice a solver answer names (`crates/hd_tir/DESIGN.md`,
    /// "callee record").
    pub(crate) fn choice_of(&self, ev: Option<&Evidence>) -> (ChoiceKind, u32) {
        match ev {
            Some(Evidence::Impl { row, .. }) => {
                let def = self
                    .cx
                    .impl_table(row.module)
                    .map_or(DefId::NONE, |t| t.def[row.row as usize]);
                (ChoiceKind::Impl, def.raw())
            }
            Some(Evidence::Bound { index, .. }) => (ChoiceKind::Bound, u32::from(*index)),
            Some(Evidence::TraitValue { trait_ }) => (ChoiceKind::TraitValue, trait_.raw()),
            // The compiler-supplied implementation plus one, which the TIR
            // verifier checks against the callee's trait (invariant 13).
            Some(Evidence::Builtin(b)) => (ChoiceKind::Builtin, 1 + *b as u32),
            _ => (ChoiceKind::Builtin, 0),
        }
    }
}

/// Substitutes one owner's parameters.
/// A callee with no final function parameter takes no trailing block
/// (`grammar.call.trailing-block.eligible`); no diagnostic code names that
/// yet (docs/audit/diagnostic-notes.md), so it stays a stop.
/// The arguments of a form that takes neither a trailing block nor a
/// positional spread.
impl Ck<'_, '_> {
    /// Whether `def` is a test registration function: `it`, `it_each`,
    /// `it_prop` or `it_prop_with` (`module.testing.direct-call`).
    pub(crate) fn is_registration_fn(&self, def: DefId) -> bool {
        crate::tests::case_runner(self.cx.names.known, def).is_some()
            || self
                .cx
                .lookup
                .item(def)
                .and_then(|i| i.intrinsic)
                .is_some_and(|k| self.cx.names.text(k) == "test_case")
    }

    /// `module.testing.snapshot.literal`: a `snapshot` call's `expect`,
    /// named or second, is a string literal without interpolation.
    fn check_snapshot_expect(&mut self, args: &Args<'_>) {
        let expect = args
            .named
            .iter()
            .find(|(name, _)| name == "expect")
            .map(|(_, e)| *e)
            .or_else(|| args.positional.get(1).copied());
        if let Some(e) = expect
            && crate::expr::literal_text(&self.cx.src, e).is_none()
        {
            self.err(
                Code::NonLiteralTestArgument,
                e,
                "a snapshot's `expect` is a string literal",
            );
        }
    }

    /// `module.testing.reg.body-closure-result.reference-values`: the body closure of a
    /// registration call that writes no result type returns `void`, or
    /// `Result[void, dyn Error]` when it uses `?`
    /// (`expr.try.test.with-try.reference-values`, `expr.try.test.without-try`). The body
    /// is the final parameter, given by name, as a trailing block or last
    /// in position.
    /// Reports a test case's `timeout`, a `Duration?` value, to the
    /// runner: std's `case_timeout` when it holds a duration
    /// (`std-testing.runner.timeout`), nothing for `.None`.
    pub(crate) fn report_timeout(&mut self, timeout: Ref, n: NodeRef<'_>) {
        let pool = self.pool();
        let duration = match pool.get(crate::tests::timeout_ty(self.cx)) {
            TyData::Option(inner) => inner,
            _ => Ty::POISON,
        };
        let some = self.b.open_block();
        let d = self.b.payload(timeout, 1, 0, duration, n.index());
        let c = Callee::Item {
            def: self.cx.names.item("std.testing", "case_timeout"),
            targs: TyList::EMPTY,
        };
        self.b.call(&c, &[d], Providers::None, Ty::VOID, n.index());
        let some = self.b.close_block(some, None, Ty::VOID, n.index());
        let none = self.b.open_block();
        let none = self.b.close_block(none, None, Ty::VOID, n.index());
        let rec = self.b.refs_record(&[Ref(1), some, none]);
        self.b
            .emit(Tag::SwitchTag, timeout.0, rec, Ty::VOID, n.index());
    }

    fn registration_body_result(
        &mut self,
        params: &[(Symbol, Ty)],
        args: &Args<'_>,
    ) -> StageResult<()> {
        let Some((pname, pty)) = params.last() else {
            return Ok(());
        };
        let pname = self.cx.names.text(*pname).to_owned();
        let body = args
            .named
            .iter()
            .find(|(name, _)| *name == pname)
            .map(|(_, e)| *e)
            .or_else(|| args.positional.get(params.len() - 1).copied());
        let Some(body) = body.filter(|b| b.kind() == SyntaxKind::ClosureExpr) else {
            return Ok(());
        };
        let written = body
            .children()
            .any(|c| c.kind().is_type() && c.kind() != SyntaxKind::RequirementRow);
        if written {
            return Ok(());
        }
        let want = crate::tests::case_result(self.cx, crate::tests::has_try(body));
        let pool = self.pool();
        let pt = self.normalize_deep(*pty)?;
        if let TyData::Fn { result, .. } = pool.get(self.strip_mut(pt)) {
            let _ = self.infer.unify(pool, result, want);
        }
        Ok(())
    }
}

/// A `race!` call written with no task: no argument at all, or the
/// task list passed by name as an empty list literal
/// (`req.combinator.race-empty`, `.race-empty-literal`).
fn race_has_no_tasks(args: &Args<'_>) -> bool {
    if args.spread.is_some() || !args.positional.is_empty() || args.trailing.is_some() {
        return false;
    }
    match args.named.as_slice() {
        [] => true,
        [(name, e)] => {
            name == "tasks" && e.kind() == SyntaxKind::ListExpr && e.children().next().is_none()
        }
        _ => false,
    }
}

fn plain_args(args: &Args<'_>, what: &str) -> StageResult<()> {
    if args.trailing.is_some() {
        return unsupported(format!("a trailing block for {what}"));
    }
    if args.spread.is_some() {
        return unsupported(format!("a spread argument for {what}"));
    }
    Ok(())
}

pub(crate) fn subst_owner(pool: hd_types::Types<'_>, owner: DefId, args: &[Ty], t: Ty) -> Ty {
    pool.subst(t, &|p: ParamRef| {
        (p.owner == owner)
            .then(|| args.get(p.index as usize).copied())
            .flatten()
    })
}
