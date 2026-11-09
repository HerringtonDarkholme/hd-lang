//! Whole-program scans of every instance's TIR before emission, whose
//! results a release link's backend consumes: [`mutation_targets`], the
//! concrete types whose existing heap objects the program can write
//! (`crates/hd_wasm_linear/DESIGN.md` §23.28). The scan reads the
//! backend's layout through [`ObjectLayout`].

use hd_base::{DefId, HashSet, InstId};
use hd_mono::{InstanceTable, ProgramEnv, Sub, subst};
use hd_tir::ir::{Body, PlaceRoot, Tag, ref_word};
use hd_types::{InternPool, Ty, TyData, TyList};

/// What the scan reads of the backend's type layout.
pub trait ObjectLayout {
    /// `t` with newtypes erased: what its layout reads.
    fn strip(&self, t: Ty) -> Ty;
    /// The stripped target of a mutable reference type of either kind, or
    /// none.
    fn mutable_counted_ref_target(&self, t: Ty) -> Option<Ty>;
}

/// The concrete types whose existing heap objects the program can write.
/// `None` means an instance body was unavailable, so every reference edge
/// must remain cycle-capable.
pub fn mutation_targets(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    layout: &dyn ObjectLayout,
    instances: &Instances<'_>,
) -> Option<HashSet<Ty>> {
    let table = instances.table;
    let mut targets = HashSet::default();
    let mut seen = HashSet::default();
    for id in instances.order {
        if matches!(table.sub[id.idx()], Sub::Adapter) {
            continue;
        }
        let b = (instances.body)(*id)?;
        let (item, args) = (table.item[id.idx()], table.args[id.idx()]);
        if !seen.insert((std::ptr::from_ref(b) as usize, item, args)) {
            continue;
        }
        let scan = Scan {
            pool,
            env,
            layout,
            item,
            args,
            b,
        };
        scan.mutation_targets(&mut targets);
    }
    Some(targets)
}

/// The program's instances as the backend emits them: collection's table,
/// the content order, and each instance's TIR body.
pub struct Instances<'a> {
    pub table: &'a InstanceTable,
    pub order: &'a [InstId],
    pub body: &'a dyn Fn(InstId) -> Option<&'a Body>,
}

struct Scan<'a> {
    pool: &'a InternPool,
    env: &'a dyn ProgramEnv,
    layout: &'a dyn ObjectLayout,
    item: DefId,
    args: TyList,
    b: &'a Body,
}

impl Scan<'_> {
    fn sub(&self, t: Ty) -> Ty {
        subst(self.pool, self.env, self.item, self.args, t)
    }

    /// Adds every type whose existing heap object this body can write: a
    /// mutable counted reference's target, and a reassignable local whose
    /// cell a counted reference can keep.
    fn mutation_targets(&self, out: &mut HashSet<Ty>) {
        let b = self.b;
        let mut writes = vec![0u32; b.local_ty.len()];
        for i in 0..b.len() {
            if b.tags[i] == Tag::Store
                && let Some((local, true)) = b.stored_local(i)
            {
                writes[local.idx()] += 1;
            }
        }
        let mut seen = HashSet::default();
        for &t in b.ty.iter().chain(&b.local_ty) {
            self.mutation_type(self.sub(t), &mut seen, out);
        }
        for (i, (&tag, &[place, word])) in b.tags.iter().zip(&b.data).enumerate() {
            if tag != Tag::Ref
                || word & ref_word::LOCAL != 0
                || self
                    .layout
                    .mutable_counted_ref_target(self.sub(b.ty[i]))
                    .is_some()
            {
                continue;
            }
            let Some(PlaceRoot::Local(local)) = b.decode_place(place).map(|p| p.root) else {
                continue;
            };
            let flags = b.local_flags[local.idx()];
            let ty = self.sub(b.local_ty[local.idx()]);
            // A parameter's cell holds the caller's value.
            let reassignable = flags & hd_tir::ir::local_flags::PARAM == 0
                && (flags & hd_tir::ir::local_flags::MUTATED != 0 || writes[local.idx()] > 1);
            if reassignable {
                out.insert(self.layout.strip(ty));
            }
        }
    }

    /// Adds mutable counted references nested anywhere in concrete `t`.
    fn mutation_type(&self, t: Ty, seen: &mut HashSet<Ty>, out: &mut HashSet<Ty>) {
        if !seen.insert(t) {
            return;
        }
        if let Some(target) = self.layout.mutable_counted_ref_target(t) {
            out.insert(target);
        }
        let mut children = Vec::new();
        match self.pool.get(t) {
            TyData::Adt { def, args } => {
                children.extend(self.pool.list_items(args).iter().copied());
                if let Some(fields) = self.env.data_fields(def) {
                    children.extend(
                        fields
                            .into_iter()
                            .map(|field| subst(self.pool, self.env, def, args, field)),
                    );
                }
            }
            TyData::Tuple { elems, rest } => {
                children.extend(self.pool.list_items(elems).iter().copied());
                children.extend(rest);
            }
            TyData::Union(members) => {
                children.extend(self.pool.list_items(members).iter().copied());
            }
            TyData::Ref { inner, .. } => {
                children.push(inner);
            }
            TyData::Fn { params, result, .. } => {
                children.extend(self.pool.list_items(params).iter().copied());
                children.push(result);
            }
            TyData::TraitValue { args, bindings, .. } => {
                children.extend(self.pool.list_items(args).iter().copied());
                children.extend(bindings.into_iter().map(|(_, ty)| ty));
            }
            TyData::Opaque { args, bound, .. } => {
                children.extend(self.pool.list_items(args).iter().copied());
                children.push(bound);
            }
            TyData::Assoc { self_ty, args, .. } => {
                children.push(self_ty);
                children.extend(self.pool.list_items(args).iter().copied());
            }
            TyData::Prim(_)
            | TyData::Never
            | TyData::Poison
            | TyData::Param(_)
            | TyData::Const(_)
            | TyData::Infer(_)
            | TyData::Canon(_)
            | TyData::Row(_)
            | TyData::Context(_) => {}
        }
        for child in children {
            self.mutation_type(child, seen, out);
        }
    }
}

/// The instances whose call can re-enter themselves and that the
/// coinductive flag rule covers (`annot.bound.recursive.flag`): a static
/// method with no parameters, no requirement row and a `bool` result, on a
/// cycle of the static call graph. Such a method's code answers a call made
/// while the same instance is evaluating with `true`, so the cycle
/// evaluates to its greatest fixpoint instead of recursing without end.
///
/// The graph is collection's: each instance's call targets, plus one
/// virtual node for the calls it cannot name (a trait-value call, a call
/// of a function value). Every dynamic call site points at the virtual
/// node, and the virtual node at every flag method, so a flag that may
/// reach itself through such a call is on a cycle too. Cost: one
/// iterative Tarjan walk over that graph, O(instances + targets + flags),
/// each instance visited once and no native recursion
/// ([`coinductive_cycles`]).
#[must_use]
pub fn coinductive_flags(
    env: &dyn ProgramEnv,
    instances: &Instances<'_>,
    calls: &[hd_base::HashMap<u32, hd_mono::Target>],
) -> HashSet<hd_base::Hash128> {
    let table = instances.table;
    let n = table.item.len();
    let by_key: hd_base::HashMap<hd_base::Hash128, u32> = table
        .key
        .iter()
        .enumerate()
        .map(|(i, &k)| (k, word(i)))
        .collect();
    let flag: Vec<bool> = (0..n).map(|i| flag_method(env, table, i)).collect();
    let mut edges = |i: u32, out: &mut Vec<u32>| {
        let i = i as usize;
        let id = InstId::from_raw(word(i));
        // A body that is unavailable may call anything.
        let mut dynamic = (instances.body)(id).is_none_or(|b| {
            b.tags
                .iter()
                .enumerate()
                .any(|(at, &t)| t == Tag::CallValue && !known_call_value(calls.get(i), at))
        });
        for target in calls.get(i).into_iter().flat_map(|c| c.values()) {
            let mut call = |t: &hd_mono::CallTarget| {
                if t.kind == hd_mono::TargetKind::Instance
                    && let Some(&j) = by_key.get(&t.key)
                {
                    out.push(j);
                }
            };
            match target {
                hd_mono::Target::Call(t)
                | hd_mono::Target::Default { call: t, .. }
                | hd_mono::Target::MapInline(t) => call(t),
                hd_mono::Target::MapKey { hash, eq } => {
                    call(hash);
                    call(eq);
                }
                hd_mono::Target::KnownClosure(key) => {
                    if let Some(&j) = by_key.get(key) {
                        out.push(j);
                    }
                }
                hd_mono::Target::Dyn => dynamic = true,
                // Making a closure or a trait value calls nothing; a later
                // call of it is a dynamic call site.
                hd_mono::Target::Closure(_)
                | hd_mono::Target::VTable(_)
                | hd_mono::Target::ConstDefault(_) => {}
            }
        }
        if dynamic {
            out.push(word(n));
        }
    };
    let cyclic = coinductive_cycles(n, &flag, &mut edges, &mut 0);
    (0..n)
        .filter(|&i| cyclic[i])
        .map(|i| table.key[i])
        .collect()
}

/// Whether instruction `at` of instance `calls`' body is a function value
/// call that collection made direct (`Target::KnownClosure`).
fn known_call_value(calls: Option<&hd_base::HashMap<u32, hd_mono::Target>>, at: usize) -> bool {
    matches!(
        calls.and_then(|c| c.get(&word(at))),
        Some(hd_mono::Target::KnownClosure(_))
    )
}

/// `i`, an instance index, a body position or the instance count, as a
/// `u32` word.
fn word(i: usize) -> u32 {
    // ruled out: instance ids and TIR positions are u32 words, so every
    // index the table or a body has fits, and so does their count.
    u32::try_from(i).unwrap_or(u32::MAX)
}

/// Whether instance `i` is a flag method of `annot.bound.recursive.flag`:
/// an item body (no closure) of a method with no parameters, a `bool`
/// result, no requirement row, and no suspension.
fn flag_method(env: &dyn ProgramEnv, table: &InstanceTable, i: usize) -> bool {
    table.sub[i] == Sub::Body(0) && is_flag_method(env, table.item[i], table.args[i])
}

/// Whether `item` at `args` is a flag method of
/// `annot.bound.recursive.flag`: a method with no parameters, a `bool`
/// result, no requirement row, and no suspension.
#[must_use]
pub fn is_flag_method(env: &dyn ProgramEnv, item: DefId, args: TyList) -> bool {
    env.parent(item).is_some()
        && env.params(item).is_some_and(|p| p.is_empty())
        && env.ret(item) == Some(Ty::BOOL)
        && !env.suspends(item)
        && !hd_mono::has_row_params(env, item)
        && env.row_keys(item, args).is_empty()
}

/// The flag nodes of `0..n` on a cycle of `edges`, where node `n` is the
/// virtual node of dynamic calls, which reaches every flag. Only a flag
/// can be on a reported cycle, and a walk from it finds every cycle
/// through it, so the walk ([`components`]) starts at flags alone.
/// `work` counts the nodes entered and the edges followed, for the cost
/// test.
fn coinductive_cycles(
    n: usize,
    flag: &[bool],
    edges: &mut dyn FnMut(u32, &mut Vec<u32>),
    work: &mut u64,
) -> Vec<bool> {
    let virt = word(n);
    let flags: Vec<u32> = (0..virt).filter(|&i| flag[i as usize]).collect();
    let mut cyclic = vec![false; n];
    let mut succ = |v: u32, out: &mut Vec<u32>| {
        if v == virt {
            out.extend_from_slice(&flags);
        } else {
            edges(v, out);
        }
    };
    components(n + 1, &flags, &mut succ, work, &mut |members, looped| {
        for &m in members {
            if m != virt {
                cyclic[m as usize] = looped && flag[m as usize];
            }
        }
    });
    cyclic
}

/// The strongly connected components of the graph `edges` gives over
/// nodes `0..n` that `roots` reach, each passed to `emit` with whether it
/// is a cycle (more than one member, or a member calling itself). A
/// component is emitted after every component it reaches, callees first.
/// An iterative Tarjan walk (Tarjan 1972, in the shape of rustc's
/// `rustc_data_structures::graph::scc`): dense per-node arrays, one
/// explicit frame stack whose successor lists share one buffer, each node
/// entered once and each edge followed once, no native recursion. `work`
/// counts the nodes entered and the edges followed.
fn components(
    n: usize,
    roots: &[u32],
    edges: &mut dyn FnMut(u32, &mut Vec<u32>),
    work: &mut u64,
    emit: &mut dyn FnMut(&[u32], bool),
) {
    const UNSEEN: u32 = u32::MAX;
    let mut index = vec![UNSEEN; n];
    let mut low = vec![0u32; n];
    let mut on_stack = vec![false; n];
    let mut stack: Vec<u32> = Vec::new();
    // Per frame: its node, its next successor, and the range of its
    // successors in `succ`; a child's range follows its parent's.
    let mut frames: Vec<(u32, usize, usize, usize)> = Vec::new();
    let mut succ: Vec<u32> = Vec::new();
    let mut next = 0u32;
    for &root in roots {
        if index[root as usize] != UNSEEN {
            continue;
        }
        let mut pending = Some(root);
        loop {
            if let Some(v) = pending.take() {
                *work += 1;
                index[v as usize] = next;
                low[v as usize] = next;
                next += 1;
                stack.push(v);
                on_stack[v as usize] = true;
                let from = succ.len();
                edges(v, &mut succ);
                frames.push((v, from, from, succ.len()));
            }
            let Some(&mut (v, ref mut at, from, end)) = frames.last_mut() else {
                break;
            };
            if *at < end {
                let w = succ[*at];
                *at += 1;
                *work += 1;
                if index[w as usize] == UNSEEN {
                    pending = Some(w);
                } else if on_stack[w as usize] {
                    low[v as usize] = low[v as usize].min(index[w as usize]);
                }
                continue;
            }
            if low[v as usize] == index[v as usize] {
                // ruled out: Tarjan's invariant keeps a node on the stack
                // until its component closes, and `v` closes it here.
                let at = stack.iter().rposition(|&m| m == v).unwrap_or(0);
                let looped = stack.len() - at > 1 || succ[from..end].contains(&v);
                for &m in &stack[at..] {
                    on_stack[m as usize] = false;
                }
                emit(&stack[at..], looped);
                stack.truncate(at);
            }
            frames.pop();
            succ.truncate(from);
            if let Some(&(parent, ..)) = frames.last() {
                low[parent as usize] = low[parent as usize].min(low[v as usize]);
            }
        }
    }
}

/// [`solve_flags`]'s evaluation of one node's body, where a call of node
/// `j` returns `seed(j)`.
pub type FlagEval<'a> = dyn FnMut(u32, &dyn Fn(u32) -> Option<bool>) -> Option<bool> + 'a;

/// The value of each flag method of a call graph that the rule
/// `annot.bound.recursive.flag` decides at compile time, or `None` where
/// the call stays a call whose re-entry the backend answers at runtime.
/// Node `i`'s body calls `callees[from[i]..from[i + 1]]`, and `local[i]`
/// says whether it has no effect a caller sees, those calls aside.
/// `eval(i, seed)` evaluates node `i`'s body where a call of node `j`
/// returns `seed(j)`, `None` meaning unknown (any value), and gives its
/// value where that decides it (`hd_mono::defaults::value_given`, whose
/// unknowns read as Kleene's strong three-valued logic).
///
/// The runtime rule is co-SLD resolution's (Simon, Gupta et al., ICLP
/// 2006): a call made while the same flag evaluates reads `true`. A
/// folded value must equal what that rule gives in every context the
/// call can run in, so each component of the graph (callees first) is
/// solved in two passes, each a worklist in which a member changes once:
///
/// 1. Coinductive: the greatest set of members that evaluate to `true`
///    when every member of the set reads `true` and every other member
///    is unknown. All start in the set; a member that is not then surely
///    `true` leaves it, and its callers in the set are evaluated again.
///    By induction on the members not yet evaluating, each member left
///    is `true` in every context, as a cycle of `&&` and `||` is at its
///    greatest fixpoint.
/// 2. Inductive: each other member whose own re-entry reads `true`, the
///    set's members `true`, the members already settled their values, and
///    the rest unknown, surely evaluates to one value is settled to it;
///    its callers are evaluated again. Once settled, a member's whole
///    evaluation has that value wherever it starts, so it folds.
///
/// A member left unsettled (`A::on()` asking `!B::on()` and back, whose
/// answer depends on which one the program asks first) keeps its runtime
/// flag, and so does every member whose value depends on it. A member
/// whose body has an effect, or calls one that does, never folds.
///
/// Cost: purity is one walk over the reversed edges, O(n + e); the
/// components are one Tarjan walk, O(n + e). Each pass evaluates a member
/// once, then once more per change of a member it calls, at most once per
/// edge: at most 2(n + e) evaluations, each linear in its body, so
/// O(sum of bodies x component size) in the worst case and linear when
/// bodies call a bounded number of flags. `work` counts the evaluations
/// and the walk's nodes and edges. The release inliner solves a component
/// on its first call, without a lock, so once per thread that reaches it
/// before the first result is recorded: at most the thread count
/// (`HD_JOBS`, at most 8) times this bound, and once in the common case.
pub fn solve_flags(
    from: &[u32],
    callees: &[u32],
    local: &[bool],
    eval: &mut FlagEval<'_>,
    work: &mut u64,
) -> Vec<Option<bool>> {
    let n = local.len();
    let out_of = |i: u32| &callees[from[i as usize] as usize..from[i as usize + 1] as usize];
    // The reversed edges, as one counting sort: `callers[rfrom[j]..rfrom[j
    // + 1]]` call `j`.
    let mut rfrom = vec![0u32; n + 1];
    for &j in callees {
        rfrom[j as usize + 1] += 1;
    }
    for j in 0..n {
        rfrom[j + 1] += rfrom[j];
    }
    let mut fill = rfrom.clone();
    let mut by_callee = vec![0u32; callees.len()];
    for i in 0..word(n) {
        for &j in out_of(i) {
            by_callee[fill[j as usize] as usize] = i;
            fill[j as usize] += 1;
        }
    }
    let callers_of =
        |j: u32| &by_callee[rfrom[j as usize] as usize..rfrom[j as usize + 1] as usize];
    // A body is pure when it is local and calls only pure bodies: the
    // effects spread to every caller.
    let mut pure = local.to_vec();
    let mut spread: Vec<u32> = (0..word(n)).filter(|&i| !pure[i as usize]).collect();
    while let Some(j) = spread.pop() {
        for &i in callers_of(j) {
            if pure[i as usize] {
                pure[i as usize] = false;
                spread.push(i);
            }
        }
    }
    let mut value: Vec<Option<bool>> = vec![None; n];
    let mut comp = vec![u32::MAX; n];
    let mut in_set = vec![false; n];
    let mut settled: Vec<Option<bool>> = vec![None; n];
    let roots: Vec<u32> = (0..word(n)).collect();
    let mut walk = 0;
    let mut evals = 0;
    let mut succ = |v: u32, out: &mut Vec<u32>| out.extend_from_slice(out_of(v));
    let mut next_comp = 0u32;
    components(n, &roots, &mut succ, &mut walk, &mut |members, _| {
        let c = next_comp;
        next_comp += 1;
        for &m in members {
            comp[m as usize] = c;
            in_set[m as usize] = pure[m as usize];
        }
        // Pass 1: the greatest set that is surely `true`.
        let mut list: Vec<u32> = members
            .iter()
            .copied()
            .filter(|&m| pure[m as usize])
            .collect();
        while let Some(x) = list.pop() {
            if !in_set[x as usize] {
                continue;
            }
            let seed = |j: u32| {
                if comp[j as usize] == c {
                    (j == x || in_set[j as usize]).then_some(true)
                } else {
                    value[j as usize]
                }
            };
            evals += 1;
            if eval(x, &seed) != Some(true) {
                in_set[x as usize] = false;
                list.extend(
                    callers_of(x)
                        .iter()
                        .filter(|&&i| comp[i as usize] == c && in_set[i as usize]),
                );
            }
        }
        // Pass 2: settle what the set and the settled members decide.
        let open = |m: u32| pure[m as usize] && !in_set[m as usize];
        let mut list: Vec<u32> = members.iter().copied().filter(|&m| open(m)).collect();
        while let Some(x) = list.pop() {
            if settled[x as usize].is_some() {
                continue;
            }
            let seed = |j: u32| {
                if comp[j as usize] != c {
                    value[j as usize]
                } else if j == x || in_set[j as usize] {
                    Some(true)
                } else {
                    settled[j as usize]
                }
            };
            evals += 1;
            if let Some(v) = eval(x, &seed) {
                settled[x as usize] = Some(v);
                list.extend(callers_of(x).iter().filter(|&&i| {
                    comp[i as usize] == c && open(i) && settled[i as usize].is_none()
                }));
            }
        }
        for &m in members {
            value[m as usize] = if in_set[m as usize] {
                Some(true)
            } else {
                settled[m as usize]
            };
        }
    });
    *work += walk + evals;
    value
}

#[cfg(test)]
mod tests {
    use super::{coinductive_cycles, solve_flags};
    use hd_base::SliceSort;

    fn cycles(n: usize, flag: &[bool], edges: &[(u32, u32)], dynamic: &[u32]) -> (Vec<bool>, u64) {
        let mut work = 0;
        let mut succ = |v: u32, out: &mut Vec<u32>| {
            out.extend(edges.iter().filter(|e| e.0 == v).map(|e| e.1));
            if dynamic.contains(&v) {
                out.push(u32::try_from(n).expect("small"));
            }
        };
        let out = coinductive_cycles(n, flag, &mut succ, &mut work);
        (out, work)
    }

    /// A self call, a two-flag cycle, a cycle through a non-flag, a flag
    /// that only calls a cyclic one, and a flag that reaches itself only
    /// through a dynamic call.
    #[test]
    fn flags_on_a_cycle_and_only_those() {
        // 0 calls itself; 1 <-> 2; 3 -> 4 (not a flag) -> 3; 5 -> 1;
        // 6 makes a dynamic call; 7 is a non-flag on its own cycle.
        let flag = [true, true, true, true, false, true, true, false];
        let edges = [(0, 0), (1, 2), (2, 1), (3, 4), (4, 3), (5, 1), (7, 7)];
        let (out, _) = cycles(8, &flag, &edges, &[6]);
        assert_eq!(out, vec![true, true, true, true, false, false, true, false]);
    }

    /// The cost check (`.agents/skills/shared-rules.md` "Cost Model"): 20
    /// layers of 1,000 flags, each calling two flags of the next layer,
    /// the last calling back to the first. The walk enters each node once
    /// and follows each edge once: 20,000 nodes and 40,000 edges, exactly.
    #[test]
    fn the_walk_is_linear_at_width_1000_and_depth_20() {
        const WIDTH: u32 = 1_000;
        const DEPTH: u32 = 20;
        let n = (WIDTH * DEPTH) as usize;
        let mut edges = Vec::new();
        for d in 0..DEPTH {
            for j in 0..WIDTH {
                let to = (d + 1) % DEPTH * WIDTH;
                edges.push((d * WIDTH + j, to + j));
                edges.push((d * WIDTH + j, to + (j + 1) % WIDTH));
            }
        }
        let flag = vec![true; n];
        let mut work = 0;
        let mut succ = |v: u32, out: &mut Vec<u32>| {
            let at = v as usize * 2;
            out.extend([edges[at].1, edges[at + 1].1]);
        };
        let out = coinductive_cycles(n, &flag, &mut succ, &mut work);
        assert!(out.iter().all(|&c| c));
        assert_eq!(work, 20_000 + 40_000);
        // Acyclic: drop the back edges; nothing is a cycle, same bound.
        let mut work = 0;
        let mut succ = |v: u32, out: &mut Vec<u32>| {
            if v < (DEPTH - 1) * WIDTH {
                let at = v as usize * 2;
                out.extend([edges[at].1, edges[at + 1].1]);
            }
        };
        let out = coinductive_cycles(n, &flag, &mut succ, &mut work);
        assert!(out.iter().all(|&c| !c));
        assert_eq!(work, 20_000 + 2 * 19_000);
    }

    /// A flag body for the solver tests: constants, calls of other flags,
    /// `!`, `&&` and `||`.
    #[derive(Clone, Debug)]
    enum Flag {
        Is(bool),
        Call(u32),
        Not(Box<Flag>),
        And(Box<Flag>, Box<Flag>),
        Or(Box<Flag>, Box<Flag>),
    }

    impl Flag {
        /// Kleene's strong three-valued reading, as `Values` reads an
        /// unknown call result.
        fn eval(&self, seed: &dyn Fn(u32) -> Option<bool>) -> Option<bool> {
            match self {
                Flag::Is(b) => Some(*b),
                Flag::Call(j) => seed(*j),
                Flag::Not(a) => a.eval(seed).map(|b| !b),
                Flag::And(a, b) => match (a.eval(seed), b.eval(seed)) {
                    (Some(false), _) | (_, Some(false)) => Some(false),
                    (Some(true), Some(true)) => Some(true),
                    _ => None,
                },
                Flag::Or(a, b) => match (a.eval(seed), b.eval(seed)) {
                    (Some(true), _) | (_, Some(true)) => Some(true),
                    (Some(false), Some(false)) => Some(false),
                    _ => None,
                },
            }
        }

        /// The runtime answer, `annot.bound.recursive.flag` itself: a call
        /// of a flag on `stack` (a bit set) reads `true`, any other
        /// evaluates its body with itself on the stack.
        fn run(&self, bodies: &[Flag], stack: u32) -> bool {
            match self {
                Flag::Is(b) => *b,
                Flag::Call(j) if stack & (1 << j) != 0 => true,
                Flag::Call(j) => bodies[*j as usize].run(bodies, stack | (1 << j)),
                Flag::Not(a) => !a.run(bodies, stack),
                Flag::And(a, b) => a.run(bodies, stack) && b.run(bodies, stack),
                Flag::Or(a, b) => a.run(bodies, stack) || b.run(bodies, stack),
            }
        }

        fn calls(&self, out: &mut Vec<u32>) {
            match self {
                Flag::Is(_) => {}
                Flag::Call(j) => out.push(*j),
                Flag::Not(a) => a.calls(out),
                Flag::And(a, b) | Flag::Or(a, b) => {
                    a.calls(out);
                    b.calls(out);
                }
            }
        }
    }

    /// The solver over `bodies`, with its counted work.
    fn solve(bodies: &[Flag]) -> (Vec<Option<bool>>, u64) {
        let mut from = vec![0u32];
        let mut callees = Vec::new();
        for b in bodies {
            let mut out = Vec::new();
            b.calls(&mut out);
            out.sort_stable();
            out.dedup();
            callees.extend(out);
            from.push(u32::try_from(callees.len()).expect("small"));
        }
        let local = vec![true; bodies.len()];
        let mut work = 0;
        let mut eval = |i: u32, seed: &dyn Fn(u32) -> Option<bool>| bodies[i as usize].eval(seed);
        let out = solve_flags(&from, &callees, &local, &mut eval, &mut work);
        (out, work)
    }

    /// A random flag body over `n` flags, `depth` operators deep.
    fn body(next: &mut dyn FnMut(u64) -> u64, n: u64, depth: u32) -> Flag {
        match if depth == 0 { next(2) } else { next(6) } {
            0 => Flag::Is(next(4) != 0),
            1 => Flag::Call(u32::try_from(next(n)).expect("small")),
            2 => Flag::Not(Box::new(body(next, n, depth - 1))),
            3 => Flag::And(
                Box::new(body(next, n, depth - 1)),
                Box::new(body(next, n, depth - 1)),
            ),
            _ => Flag::Or(
                Box::new(body(next, n, depth - 1)),
                Box::new(body(next, n, depth - 1)),
            ),
        }
    }

    /// The program with each folded flag replaced by its value answers
    /// every flag's call as the runtime rule does: over small random
    /// systems with negation, against the rule run as written.
    #[test]
    fn folding_keeps_every_flags_runtime_answer() {
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = move |m: u64| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state % m
        };
        let (mut folded, mut kept) = (0, 0);
        for _ in 0..20_000 {
            let n = 1 + next(5);
            let bodies: Vec<Flag> = (0..n).map(|_| body(&mut next, n, 3)).collect();
            let (values, _) = solve(&bodies);
            // The release program: each folded flag is its constant.
            let release: Vec<Flag> = (bodies.iter().zip(&values))
                .map(|(b, v)| v.map_or_else(|| b.clone(), Flag::Is))
                .collect();
            for x in 0..n {
                let x = u32::try_from(x).expect("small");
                match values[x as usize] {
                    Some(_) => folded += 1,
                    None => kept += 1,
                }
                let dev = bodies[x as usize].run(&bodies, 1 << x);
                let rel = release[x as usize].run(&release, 1 << x);
                assert_eq!(dev, rel, "flag {x}: {bodies:?} {values:?}");
            }
        }
        assert!(folded > kept, "{folded} folded, {kept} kept");
    }

    /// The cases the rule's tests name: a flag that negates itself is
    /// `false`; two that negate each other answer by which is asked first,
    /// so both stay runtime; a cycle of `&&` and `||` is `true`; one false
    /// member makes its callers false.
    #[test]
    fn the_rules_cases() {
        let a = [Flag::Not(Box::new(Flag::Call(0)))];
        assert_eq!(solve(&a).0, vec![Some(false)]);
        let bc = [
            Flag::Not(Box::new(Flag::Call(1))),
            Flag::Not(Box::new(Flag::Call(0))),
        ];
        assert_eq!(solve(&bc).0, vec![None, None]);
        let even_odd = [
            Flag::And(Box::new(Flag::Call(1)), Box::new(Flag::Is(true))),
            Flag::Or(Box::new(Flag::Call(0)), Box::new(Flag::Call(2))),
            Flag::And(Box::new(Flag::Is(true)), Box::new(Flag::Call(2))),
        ];
        assert_eq!(solve(&even_odd).0, vec![Some(true); 3]);
        let falls = [
            Flag::And(Box::new(Flag::Call(1)), Box::new(Flag::Call(2))),
            Flag::And(Box::new(Flag::Call(0)), Box::new(Flag::Call(2))),
            Flag::Is(false),
        ];
        assert_eq!(solve(&falls).0, vec![Some(false); 3]);
        let through = [
            Flag::And(Box::new(Flag::Call(1)), Box::new(Flag::Call(1))),
            Flag::And(Box::new(Flag::Call(0)), Box::new(Flag::Is(false))),
        ];
        assert_eq!(solve(&through).0, vec![Some(false); 2]);
    }

    /// The cost check (`.agents/skills/shared-rules.md` "Cost Model"): 20
    /// layers of 1,000 flags, each the `&&` of two flags of the next
    /// layer, the last calling back to the first: one component of 20,000
    /// members and 40,000 edges. All `true`: one evaluation per member.
    /// With one member `false`: each member leaves the set once and
    /// settles once, each change evaluating its callers again, within
    /// 2(n + e).
    #[test]
    fn the_solve_is_linear_at_width_1000_and_depth_20() {
        const WIDTH: u32 = 1_000;
        const DEPTH: u32 = 20;
        let node = |d: u32, j: u32| d % DEPTH * WIDTH + j % WIDTH;
        let layered = |first: Flag| -> Vec<Flag> {
            (0..DEPTH * WIDTH)
                .map(|i| {
                    let (d, j) = (i / WIDTH, i % WIDTH);
                    if i == 0 {
                        return first.clone();
                    }
                    Flag::And(
                        Box::new(Flag::Call(node(d + 1, j))),
                        Box::new(Flag::Call(node(d + 1, j + 1))),
                    )
                })
                .collect()
        };
        let n = u64::from(DEPTH * WIDTH);
        let e = 2 * n;
        let walk = n + e;
        let regular = layered(Flag::And(
            Box::new(Flag::Call(node(1, 0))),
            Box::new(Flag::Call(node(1, 1))),
        ));
        let (values, work) = solve(&regular);
        assert!(values.iter().all(|&v| v == Some(true)));
        assert_eq!(work, walk + n);
        let one_false = layered(Flag::And(
            Box::new(Flag::Is(false)),
            Box::new(Flag::And(
                Box::new(Flag::Call(node(1, 0))),
                Box::new(Flag::Call(node(1, 1))),
            )),
        ));
        let (values, work) = solve(&one_false);
        assert!(values.iter().all(|&v| v == Some(false)));
        assert!(work - walk <= 2 * (n + e), "{work}");
        // 79,998 evaluations, under 4n: linear.
        assert_eq!(work, walk + 79_998);
    }
}
