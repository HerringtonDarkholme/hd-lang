//! Dependent results (spec/lang/13-values-and-references.md, "Dependent
//! Results"): a function's result may hold a local reference, which
//! depends on the source its signature elides ([`source_of`]).
//!
//! Once a body is checked, [`Ck::check_dependent_results`] solves the
//! origins of its local-holding values over the body's TIR, once and per
//! binding, as Rust's NLL does for regions (`ref.dep.origin.*`). An
//! origin is a storage root a local reference may point into: what a
//! parameter refers to, a local's or a parameter's own storage, the
//! hidden local of `&e` on a value, or a top-level binding. A returned
//! value may depend only on the source (`ref.dep.return`), a call's result
//! depends on its source argument (`ref.dep.call`), a store through a
//! reference lands in a place of the body (`ref.dep.store`). An element
//! reference takes the origin of its collection's root place
//! (`ref.dep.origin.element`).

use hd_base::SliceSort as _;
use hd_base::{DefId, HashMap, HashSet, NodeIdx, Symbol};
use hd_diag::Code;
use hd_syntax::{GreenTree, NodeRef, SyntaxKind};
use hd_tir::ir::{Body, BodyKind, Callee, NONE, PlaceRoot, Proj, Role, Tag, Word, slots};
use hd_types::{Ty, TyData, Types};

use crate::body::{BodyCx, Ck};
use crate::local_refs::holds_local_ref;

/// What a function's result depends on (`ref.dep.elide`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Source {
    /// The result holds no local reference.
    Owned,
    /// Parameter `k`, a receiver first.
    Param(usize),
    /// No reference position: `reference-result-without-source`.
    Missing,
    /// Two or more, and no reference receiver: `ambiguous-reference-source`.
    Ambiguous,
}

/// The source of a result `ret` under the parameters `params`: a `&self`
/// or `&mut self` receiver, or else the parameter of the one local
/// reference position (`ref.dep.position`).
pub(crate) fn source_of(
    pool: Types<'_>,
    params: &[(Symbol, Ty)],
    is_self: impl Fn(Symbol) -> bool,
    ret: Ty,
) -> Source {
    if !holds_local_ref(pool, ret) {
        return Source::Owned;
    }
    if params
        .first()
        .is_some_and(|&(s, t)| is_self(s) && local_ref(pool, t).is_some_and(|(c, _)| !c))
    {
        return Source::Param(0);
    }
    let mut found = Source::Missing;
    for (k, &(s, t)) in params.iter().enumerate() {
        match (pool.ref_slots(t), found) {
            (0, _) => {}
            _ if is_self(s) => {}
            (1, Source::Missing) => found = Source::Param(k),
            _ => return Source::Ambiguous,
        }
    }
    found
}

/// `Some((counted, to a trait value))` when `t` is a reference type,
/// under any `mut`.
fn local_ref(pool: Types<'_>, t: Ty) -> Option<(bool, bool)> {
    match pool.get(t) {
        TyData::Ref { counted, inner, .. } => Some((counted, is_dyn(pool, inner))),
        TyData::Mut(i) => local_ref(pool, i),
        _ => None,
    }
}

fn is_dyn(pool: Types<'_>, t: Ty) -> bool {
    match pool.get(t) {
        TyData::TraitValue { .. } => true,
        TyData::Mut(i) => is_dyn(pool, i),
        _ => false,
    }
}

/// Whether a value of type `t` may have origins: it holds a local
/// reference, or it is a trait value, which may hold one
/// (`trait.dyn.ref.convert`).
fn may_hold(pool: Types<'_>, t: Ty) -> bool {
    holds_local_ref(pool, t) || is_dyn(pool, t)
}

/// A storage root a local reference may point into (`ref.dep.origin.set`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
enum Origin {
    /// What parameter `$p` refers to, which the caller owns: `p*`.
    Param(u32),
    /// The own storage of local `$l`, a parameter's included.
    Local(u32),
    /// The hidden local of `&e` on a value, at the `Ref`.
    Hidden(u32),
    /// A top-level binding.
    Global(u32),
}

/// An instruction's, a local's or a parameter's index as a word.
fn w(i: usize) -> u32 {
    u32::try_from(i).expect("an index")
}

/// An origin set, sorted.
type Set = Vec<Origin>;

fn union(into: &mut Set, from: &[Origin]) -> bool {
    let n = into.len();
    for o in from {
        if let Err(at) = into.binary_search(o) {
            into.insert(at, *o);
        }
    }
    into.len() != n
}

/// The origin sets of one body, the least solution of its constraints.
struct Solve<'a> {
    b: &'a Body,
    pool: Types<'a>,
    /// Each instruction's value.
    val: Vec<Set>,
    /// What each local's or hidden local's storage holds: `O(b)`.
    held: HashMap<Origin, Set>,
    /// The dependent calls: the parameter index of each one's source.
    deps: &'a HashMap<u32, usize>,
    /// Stores of a local-holding value into a place the caller owns.
    out: HashSet<u32>,
    changed: bool,
}

impl Solve<'_> {
    fn of(&self, r: u32) -> &[Origin] {
        if r == NONE || r & hd_tir::ir::Ref::CONST_BIT != 0 {
            return &[];
        }
        self.val.get(r as usize).map_or(&[], Vec::as_slice)
    }

    fn held(&self, o: Origin) -> Set {
        match o {
            Origin::Param(_) => vec![o],
            Origin::Local(_) | Origin::Hidden(_) => self.held.get(&o).cloned().unwrap_or_default(),
            Origin::Global(_) => vec![],
        }
    }

    fn hold(&mut self, o: Origin, from: &[Origin]) {
        if from.is_empty() || !matches!(o, Origin::Local(_) | Origin::Hidden(_)) {
            return;
        }
        if union(self.held.entry(o).or_default(), from) {
            self.changed = true;
        }
    }

    /// The storage the place `word` of instruction `i` lies in, and the
    /// origins of the value it holds (`ref.dep.origin.place`).
    fn place(&self, i: u32, word: u32) -> (Set, Set) {
        let Some(p) = self.b.decode_place(word) else {
            return (vec![], vec![]);
        };
        match p.root {
            PlaceRoot::Local(l) => {
                let o = Origin::Local(l.raw());
                (vec![o], self.held(o))
            }
            // An element's origin is the collection's root place
            // (`ref.dep.origin.element`). The checker has already read
            // that collection value, so follow the read back to its
            // place; a collection made as a value has a hidden local.
            PlaceRoot::Value(v) if matches!(p.proj.first(), Some(Proj::Elem(_))) => {
                let mut v = v.0;
                while v & hd_tir::ir::Ref::CONST_BIT == 0 && self.b.tags[v as usize] == Tag::Coerce
                {
                    v = self.b.data[v as usize][0];
                }
                if v & hd_tir::ir::Ref::CONST_BIT == 0 && self.b.tags[v as usize] == Tag::Read {
                    (self.place(v, self.b.data[v as usize][0]).0, vec![])
                } else {
                    (vec![Origin::Hidden(i)], vec![])
                }
            }
            PlaceRoot::Value(v) if p.proj.first() == Some(&Proj::Deref) => {
                let t = self.b.ty.get(v.0 as usize).copied().unwrap_or(Ty::VOID);
                let kind = (v.0 & hd_tir::ir::Ref::CONST_BIT == 0)
                    .then(|| local_ref(self.pool, t))
                    .flatten();
                match kind {
                    // A trait value through a reference is a view of the
                    // reference's data, with its origins.
                    Some((false, true)) => (self.of(v.0).to_vec(), self.of(v.0).to_vec()),
                    // A reborrow: what the reference points into.
                    Some((false, false)) => {
                        let s = self.of(v.0).to_vec();
                        let mut h = Set::new();
                        for &o in &s {
                            union(&mut h, &self.held(o));
                        }
                        (s, h)
                    }
                    // Crossing a counted reference changes nothing: the
                    // place lies where the reference was read from.
                    _ => {
                        let s = match self.b.tags.get(v.0 as usize) {
                            Some(Tag::Read) => self.place(v.0, self.b.data[v.0 as usize][0]).0,
                            _ => vec![Origin::Hidden(v.0)],
                        };
                        let dyn_ = kind.is_some_and(|(_, d)| d);
                        (s.clone(), if dyn_ { s } else { vec![] })
                    }
                }
            }
            PlaceRoot::Value(v) => (vec![Origin::Hidden(i)], self.of(v.0).to_vec()),
            PlaceRoot::Global(d, _) => (vec![Origin::Global(d.raw())], vec![]),
            _ => (vec![], vec![]),
        }
    }

    /// The origins of the source argument `a`: a local-holding value's,
    /// or for a counted reference lent as a local one, the storage it was
    /// read from (`ref.dep.origin.lend`).
    fn lent(&self, a: u32) -> Set {
        if a & hd_tir::ir::Ref::CONST_BIT != 0 {
            return vec![];
        }
        let b = self.b;
        let mut v = a as usize;
        if b.tags[v] == Tag::Ref || may_hold(self.pool, b.ty[v]) {
            return self.of(a).to_vec();
        }
        while b.tags[v] == Tag::Coerce && b.data[v][0] & hd_tir::ir::Ref::CONST_BIT == 0 {
            v = b.data[v][0] as usize;
        }
        match b.tags[v] {
            Tag::Read => self.place(w(v), b.data[v][0]).0,
            _ => vec![Origin::Hidden(w(v))],
        }
    }

    fn step(&mut self, i: usize) {
        let b = self.b;
        let tag = b.tags[i];
        if tag == Tag::Store {
            let [word, v] = b.data[i];
            let value = self.of(v).to_vec();
            if value.is_empty() {
                return;
            }
            let p = b.decode_place(word);
            let through = p
                .as_ref()
                .is_some_and(|p| p.proj.first() == Some(&Proj::Deref));
            let (storage, _) = self.place(w(i), word);
            for s in storage {
                match s {
                    Origin::Param(_) if through => {
                        let t = b.ty.get(v as usize).copied().unwrap_or(Ty::VOID);
                        if holds_local_ref(self.pool, t) {
                            self.out.insert(w(i));
                        }
                    }
                    Origin::Local(_) | Origin::Hidden(_) => self.hold(s, &value),
                    _ => {}
                }
            }
            return;
        }
        if tag == Tag::Break {
            let [label, v] = b.data[i];
            let value = self.of(v).to_vec();
            if let Some(&t) = b.label_inst.get(label as usize)
                && union(&mut self.val[t as usize], &value)
            {
                self.changed = true;
            }
            return;
        }
        let new = match tag {
            Tag::Ref => {
                let (s, v) = self.place(w(i), b.data[i][0]);
                if s == [Origin::Hidden(w(i))] {
                    self.hold(s[0], &v);
                }
                s
            }
            // Only a local-holding value has origins.
            _ if !may_hold(self.pool, b.ty[i]) => return,
            Tag::Read => self.place(w(i), b.data[i][0]).1,
            Tag::Call | Tag::Await | Tag::CallDyn => {
                let k = self.deps.get(&w(i));
                k.and_then(|&k| source_arg(b, i, k))
                    .map(|a| self.lent(a))
                    .unwrap_or_default()
            }
            Tag::Coerce => self.of(b.data[i][0]).to_vec(),
            Tag::Prim | Tag::Intrinsic | Tag::CallValue | Tag::CallHost | Tag::Return => vec![],
            _ => {
                let mut s = Set::new();
                slots(b, i, |_, w, word| {
                    if matches!(word, Word::Block | Word::Value(Role::Tail | Role::Consume)) {
                        union(&mut s, self.of(w));
                    }
                });
                s
            }
        };
        if union(&mut self.val[i], &new) {
            self.changed = true;
        }
    }
}

/// The value call `i` passes for parameter `k`, a trait value's receiver
/// included.
fn source_arg(b: &Body, i: usize, k: usize) -> Option<u32> {
    let k = match (b.tags[i], k) {
        (Tag::CallDyn, 0) => return Some(b.data[i][0]),
        (Tag::CallDyn, k) => k - 1,
        (_, k) => k,
    };
    let mut found = None;
    let want = Word::Value(Role::Arg(w(k)));
    slots(b, i, |_, v, word| {
        if word == want {
            found = Some(v);
        }
    });
    found
}

impl Ck<'_, '_> {
    /// The source of a callee's result (`ref.dep.call`, `ref.dep.trait`):
    /// a trait method's from the trait's signature.
    fn callee_source(&self, def: DefId) -> Source {
        let Ok(sig) = self.sig_of(def) else {
            return Source::Owned;
        };
        let names = &self.cx.names;
        source_of(
            self.pool(),
            &sig.params,
            |s| names.text(s) == "self",
            sig.ret,
        )
    }

    /// The dependent results of the finished body, its result `ret`
    /// (module docs). `fun` is the function's declaration.
    pub(crate) fn check_dependent_results(
        &mut self,
        root: hd_tir::ir::Ref,
        ret: Ty,
        fun: Option<NodeRef<'_>>,
    ) {
        let pool = self.pool();
        let body = self.b.body();
        // The body's own source: parameter k is local k.
        let fn_body = body.kind == BodyKind::Fn && fun.is_some() && holds_local_ref(pool, ret);
        let own = match fn_body.then(|| self.sig_of(body.item)) {
            Some(Ok(sig)) => {
                let names = &self.cx.names;
                Some(source_of(
                    pool,
                    &sig.params,
                    |s| names.text(s) == "self",
                    ret,
                ))
            }
            _ => None,
        };
        let mut deps: HashMap<u32, usize> = HashMap::default();
        let mut through = false;
        for i in 0..body.tags.len() {
            match body.tags[i] {
                // A block's or a loop's value may leave a local's scope.
                Tag::Block | Tag::Loop if w(i) != root.0 && holds_local_ref(pool, body.ty[i]) => {
                    through = true;
                }
                Tag::Call | Tag::Await | Tag::CallDyn if holds_local_ref(pool, body.ty[i]) => {
                    let def = match body.tags[i] {
                        Tag::CallDyn => body.record(body.data[i][1]).first().copied(),
                        _ => match Callee::from_words(body.record(body.data[i][0])) {
                            Some(Callee::Item { def, .. }) => Some(def.raw()),
                            Some(Callee::TraitMethod { method, .. }) => Some(method.raw()),
                            None => None,
                        },
                    };
                    if let Some(d) = def
                        && let Source::Param(k) = self.callee_source(DefId::from_raw(d))
                    {
                        deps.insert(w(i), k);
                    }
                }
                Tag::Store => {
                    let [_, v] = body.data[i];
                    through |= v & hd_tir::ir::Ref::CONST_BIT == 0
                        && body
                            .ty
                            .get(v as usize)
                            .is_some_and(|&t| holds_local_ref(pool, t));
                }
                _ => {}
            }
        }
        if let (Some(Source::Missing | Source::Ambiguous), Some(f)) = (own, fun)
            && self.result_omitted
        {
            self.source_error(own == Some(Source::Missing), f);
        }
        let src = match own {
            Some(Source::Param(k)) => Some(w(k)),
            _ => None,
        };
        if src.is_none() && !through {
            return;
        }
        let body = self.b.body();
        let mut s = Solve {
            b: body,
            pool,
            val: vec![Set::new(); body.tags.len()],
            held: HashMap::default(),
            deps: &deps,
            out: HashSet::default(),
            changed: true,
        };
        for l in 0..body.local_ty.len() {
            if body.local_flags[l] & hd_tir::ir::local_flags::PARAM != 0
                && holds_local_ref(pool, body.local_ty[l])
            {
                s.held
                    .insert(Origin::Local(w(l)), vec![Origin::Param(w(l))]);
            }
        }
        while s.changed {
            s.changed = false;
            for i in 0..body.tags.len() {
                s.step(i);
            }
        }
        let mut stores: Vec<NodeIdx> = s.out.iter().map(|&i| body.syn[i as usize]).collect();
        stores.sort_stable_by_key(|n| n.raw());
        let mut bad: Vec<(NodeIdx, Origin, u32)> = Vec::new();
        if let Some(k) = src {
            for r in returned(body, root.0) {
                if let Some(&o) = s.of(r).iter().find(|&&o| o != Origin::Param(k)) {
                    bad.push((site(&s, r, o, 0), o, k));
                }
            }
        }
        // A returned value's origins are `ref.dep.return`'s.
        let mut skip: HashSet<u32> = HashSet::default();
        if src.is_some() {
            skip.extend(returned(body, root.0));
        }
        skip.insert(root.0);
        let outlives = scoped_uses(&s, &skip, self.cx);
        for at in stores {
            if at == NodeIdx::NONE {
                continue;
            }
            let node = self.cx.src.parse.tree.node(at);
            self.err(
                Code::LocalReferenceEscapes,
                node,
                "a local reference never escapes to a place the caller owns; store it only in a place of this function",
            );
        }
        let mut seen = HashSet::default();
        for (at, o, k) in bad {
            if at == NodeIdx::NONE || !seen.insert(at) {
                continue;
            }
            let msg = self.origin_message(o, k, fun);
            let node = self.cx.src.parse.tree.node(at);
            self.err(Code::LocalReferenceEscapes, node, &msg);
        }
        for (at, msg) in outlives {
            let node = self.cx.src.parse.tree.node(at);
            self.err(Code::LocalReferenceOutlivesLocal, node, &msg);
        }
    }

    /// `reference-result-without-source` or `ambiguous-reference-source`
    /// for an omitted result type that holds a local reference.
    fn source_error(&mut self, missing: bool, f: NodeRef<'_>) {
        if missing {
            self.err(
                Code::ReferenceResultWithoutSource,
                f,
                "a reference result needs a reference parameter to borrow from",
            );
        } else {
            self.err(
                Code::AmbiguousReferenceSource,
                f,
                "the result may borrow from more than one reference parameter; naming the source comes in a later stage",
            );
        }
    }

    /// `ref.dep.return.message`: the root the result must not depend on.
    fn origin_message(&self, o: Origin, src: u32, fun: Option<NodeRef<'_>>) -> String {
        let body = self.b.body();
        let name = |l: u32| self.cx.names.text(body.local_name[l as usize]).to_owned();
        let f = fun
            .and_then(|f| f.name(&self.cx.src.parse.tokens))
            .map_or_else(
                || "this function".to_owned(),
                |t| format!("`{}`", self.cx.src.text(t)),
            );
        let what = match o {
            Origin::Local(l)
                if body.local_flags[l as usize] & hd_tir::ir::local_flags::PARAM != 0 =>
            {
                format!(
                    "`{}` is a parameter, whose own storage the call owns",
                    name(l)
                )
            }
            Origin::Local(l) if name(l).starts_with('$') => {
                format!("the value refers to a temporary of {f}")
            }
            Origin::Local(l) => format!("`{}` is a local of {f}", name(l)),
            Origin::Param(p) => format!("`{}` is another parameter", name(p)),
            Origin::Hidden(_) => format!("the value refers to a temporary of {f}"),
            Origin::Global(_) => "the value refers to a top-level binding".to_owned(),
        };
        format!("{what}; the result depends only on `{}`", name(src))
    }
}

/// The values the body returns: its root block's value and each `Return`
/// operand outside its closures.
fn returned(b: &Body, root: u32) -> Vec<u32> {
    let mut out = Vec::new();
    let mut seen = HashSet::default();
    let mut stack = vec![root];
    while let Some(i) = stack.pop() {
        if i == NONE || i as usize >= b.tags.len() || !seen.insert(i) {
            continue;
        }
        match b.tags[i as usize] {
            Tag::Return => out.push(b.data[i as usize][0]),
            Tag::Closure => continue,
            _ => {}
        }
        if i == root && b.tags[i as usize] == Tag::Block {
            out.push(b.data[i as usize][1]);
        }
        slots(b, i as usize, |_, w, word| {
            if matches!(word, Word::Block | Word::Inst) {
                stack.push(w);
            }
        });
    }
    out.retain(|&v| v != NONE);
    out
}

/// The innermost part of value `v` whose origins hold `o`, for the
/// diagnostic's position: an arm, a block's value, a construction's part.
fn site(s: &Solve<'_>, v: u32, o: Origin, depth: u32) -> NodeIdx {
    let b = s.b;
    let here = b.syn.get(v as usize).copied().unwrap_or(NodeIdx::NONE);
    if depth > 64 || v & hd_tir::ir::Ref::CONST_BIT != 0 {
        return here;
    }
    let mut inner = None;
    if matches!(b.tags[v as usize], Tag::Coerce) {
        inner = Some(b.data[v as usize][0]);
    } else if !matches!(
        b.tags[v as usize],
        Tag::Read | Tag::Ref | Tag::Call | Tag::Await | Tag::CallDyn
    ) {
        slots(b, v as usize, |_, w, word| {
            if inner.is_none()
                && matches!(word, Word::Block | Word::Value(Role::Tail | Role::Consume))
                && s.of(w).contains(&o)
            {
                inner = Some(w);
            }
        });
    }
    match inner {
        Some(w) => {
            let at = site(s, w, o, depth + 1);
            if at == NodeIdx::NONE { here } else { at }
        }
        None => here,
    }
}

/// Where a local of the body is in scope (`ref.dep.scope`): the preorder
/// node range `[lo, hi)` that its uses lie in, and the node range of the
/// loop each of whose iterations makes it anew.
#[derive(Clone, Copy)]
struct Scope {
    lo: u32,
    hi: u32,
    lp: Option<(u32, u32)>,
}

/// The node that declares origin `o`, a binding's pattern or a compiler
/// local's or a temporary's expression, and whether it is a temporary or
/// a compiler local (`$cur`). `None` for what the caller owns and for a
/// parameter, which is in scope in the whole body.
fn decl_node(b: &Body, cx: &BodyCx<'_>, o: Origin) -> Option<(NodeIdx, bool)> {
    let (n, temp) = match o {
        Origin::Local(l) if b.local_flags[l as usize] & hd_tir::ir::local_flags::PARAM == 0 => (
            b.local_syn[l as usize],
            cx.names.text(b.local_name[l as usize]).starts_with('$'),
        ),
        Origin::Hidden(h) => (b.syn[h as usize], true),
        _ => return None,
    };
    (n != NodeIdx::NONE).then_some((n, temp))
}

/// The preorder range of node `n`: its index and one past its last
/// descendant.
fn range(n: NodeRef<'_>) -> (u32, u32) {
    (n.index().raw(), n.end().raw())
}

/// The first suite of node `p`: a loop's body.
fn first_block(p: NodeRef<'_>) -> Option<NodeRef<'_>> {
    p.children().find(|c| c.kind() == SyntaxKind::Block)
}

/// The scope of origin `o`, declared at node `n`. A `let`, `:=` or
/// pattern binding is in scope from the end of its statement or binding
/// to the end of its suite, a `match` arm's in its arm, a `for` pattern's
/// in the loop body, and a comprehension's in the comprehension. A
/// temporary or a compiler local is in scope from the start of its
/// statement to the end of its suite (`ref.dep.scope.temporary`).
fn scope_of(tree: &GreenTree, n: NodeIdx, temp: bool) -> Option<Scope> {
    let x = n.raw();
    let mut path = vec![tree.root()];
    while let Some(c) = path.last().and_then(|p| {
        p.children().find(|c| {
            let (lo, hi) = range(*c);
            lo <= x && x < hi
        })
    }) {
        path.push(c);
    }
    let mut lo = temp.then_some(x);
    let mut region = None;
    let mut lp = None;
    let mut looped = false;
    for k in (0..path.len()).rev() {
        let p = path[k];
        let (pl, ph) = range(p);
        let up = k.checked_sub(1).map(|j| path[j]);
        // The loop whose every iteration runs the declaration.
        if !looped {
            let per_iteration = |p: NodeRef<'_>| {
                let head = p.children().next().map(range);
                let body = first_block(p).map(range);
                [head, body].iter().flatten().any(|&(l, h)| l <= x && x < h)
            };
            match p.kind() {
                SyntaxKind::ForExpr | SyntaxKind::WhileExpr if per_iteration(p) => {
                    lp = Some((pl, ph));
                    looped = true;
                }
                SyntaxKind::ComprehensionExpr => {
                    lp = Some((pl, ph));
                    looped = true;
                }
                SyntaxKind::ClosureExpr | SyntaxKind::FnDecl => looped = true,
                _ => {}
            }
        }
        if region.is_some() {
            continue;
        }
        match p.kind() {
            SyntaxKind::LetStmt | SyntaxKind::BindingExpr if lo.is_none() => lo = Some(ph),
            SyntaxKind::MatchArm if lo.is_none() => region = Some((pl, ph)),
            SyntaxKind::ForExpr if lo.is_none() => region = first_block(p).map(range),
            SyntaxKind::ComprehensionFor if lo.is_none() => region = up.map(range),
            SyntaxKind::Block | SyntaxKind::ClosureExpr | SyntaxKind::FnDecl => {
                // A temporary's statement uses it from its start.
                let stmt = path.get(k + 1).map(|c| c.index().raw());
                let lo = lo?;
                region = Some((if temp { stmt.unwrap_or(lo) } else { lo }, ph));
            }
            _ => {}
        }
    }
    let (lo, hi) = region?;
    Some(Scope { lo, hi, lp })
}

/// `ref.dep.scope`: each use of a value with a local's origin lies in
/// that local's scope and, for a loop-body local, reads no binding
/// that outlives the iteration.
fn scoped_uses(s: &Solve<'_>, skip: &HashSet<u32>, cx: &BodyCx<'_>) -> Vec<(NodeIdx, String)> {
    let b = s.b;
    let tree = &cx.src.parse.tree;
    let mut scopes: HashMap<Origin, Option<Scope>> = HashMap::default();
    let mut bad: Vec<(NodeIdx, Origin, Option<Origin>, bool, u32)> = Vec::new();
    for i in 0..b.tags.len() {
        let at = b.syn[i];
        if at == NodeIdx::NONE || skip.contains(&w(i)) {
            continue;
        }
        // The value's origins, and those of each binding a place reads.
        let mut uses: Vec<(Option<Origin>, Origin)> = s.val[i].iter().map(|&o| (None, o)).collect();
        let deref = |word| {
            b.decode_place(word)
                .is_some_and(|p| p.proj.contains(&Proj::Deref))
        };
        let tag = b.tags[i];
        if matches!(tag, Tag::Read | Tag::Ref) || tag == Tag::Store && deref(b.data[i][0]) {
            for st in s.place(w(i), b.data[i][0]).0 {
                if matches!(st, Origin::Local(_) | Origin::Hidden(_)) {
                    uses.extend(s.held(st).into_iter().map(|o| (Some(st), o)));
                }
            }
        }
        for (st, o) in uses {
            if st == Some(o) {
                continue;
            }
            let Some(sc) = *scopes
                .entry(o)
                .or_insert_with(|| decl_node(b, cx, o).and_then(|(n, t)| scope_of(tree, n, t)))
            else {
                continue;
            };
            let u = at.raw();
            let in_loop = |n: u32| sc.lp.is_some_and(|(lo, hi)| lo <= n && n < hi);
            let earlier = if sc.lo <= u && u < sc.hi {
                // In scope, but a binding declared outside the local's
                // loop may hold an earlier iteration's reference.
                let kept = st
                    .is_some_and(|st| decl_node(b, cx, st).is_none_or(|(n, _)| !in_loop(n.raw())));
                if !(sc.lp.is_some() && kept) {
                    continue;
                }
                true
            } else {
                in_loop(u) && u < sc.lo
            };
            bad.push((at, o, st, earlier, w(i)));
        }
    }
    bad.sort_stable_by_key(|x| x.0.raw());
    let mut seen = HashSet::default();
    bad.into_iter()
        .filter(|x| seen.insert(x.1))
        .map(|(at, o, st, earlier, i)| (at, scope_message(b, cx, o, st, earlier, i)))
        .collect()
}

/// `ref.dep.scope.message`: the binding or value that may point into
/// local `o` out of its scope, read at instruction `i` from `st`.
fn scope_message(
    b: &Body,
    cx: &BodyCx<'_>,
    o: Origin,
    st: Option<Origin>,
    earlier: bool,
    i: u32,
) -> String {
    let named = |l: u32| {
        let n = cx.names.text(b.local_name[l as usize]);
        (!n.starts_with('$')).then(|| format!("`{n}`"))
    };
    let holder = match st {
        Some(Origin::Local(l)) => named(l),
        _ => b.local_read(i as usize).and_then(|l| named(l.raw())),
    }
    .unwrap_or_else(|| "this value".to_owned());
    let target = match o {
        Origin::Local(l) => named(l),
        _ => None,
    }
    .unwrap_or_else(|| "a temporary".to_owned());
    if earlier {
        format!("{holder} may point into {target} from an earlier iteration of its loop")
    } else {
        format!("{holder} may point into {target}, whose block has ended")
    }
}
