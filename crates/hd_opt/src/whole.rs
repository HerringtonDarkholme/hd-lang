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
    let (item, args) = (table.item[i], table.args[i]);
    table.sub[i] == Sub::Body(0)
        && env.parent(item).is_some()
        && env.params(item).is_some_and(|p| p.is_empty())
        && env.ret(item) == Some(Ty::BOOL)
        && !env.suspends(item)
        && !hd_mono::has_row_params(env, item)
        && env.row_keys(item, args).is_empty()
}

/// The flag nodes of `0..n` on a cycle of `edges`, where node `n` is the
/// virtual node of dynamic calls, which reaches every flag. An iterative
/// Tarjan walk (Tarjan 1972, in the shape of rustc's
/// `rustc_data_structures::graph::scc`): dense per-node arrays, one
/// explicit frame stack whose successor lists share one buffer, each node
/// entered once and each edge followed once. Only a flag can be on a
/// reported cycle, and a walk from it finds every cycle through it, so the
/// walk starts at flags alone. `work` counts the nodes entered and the
/// edges followed, for the cost test.
fn coinductive_cycles(
    n: usize,
    flag: &[bool],
    edges: &mut dyn FnMut(u32, &mut Vec<u32>),
    work: &mut u64,
) -> Vec<bool> {
    const UNSEEN: u32 = u32::MAX;
    let virt = word(n);
    let flags: Vec<u32> = (0..virt).filter(|&i| flag[i as usize]).collect();
    let mut index = vec![UNSEEN; n + 1];
    let mut low = vec![0u32; n + 1];
    let mut on_stack = vec![false; n + 1];
    let mut stack: Vec<u32> = Vec::new();
    let mut cyclic = vec![false; n];
    // Per frame: its node, its next successor, and the range of its
    // successors in `succ`; a child's range follows its parent's.
    let mut frames: Vec<(u32, usize, usize, usize)> = Vec::new();
    let mut succ: Vec<u32> = Vec::new();
    let mut next = 0u32;
    for &root in &flags {
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
                if v == virt {
                    succ.extend_from_slice(&flags);
                } else {
                    edges(v, &mut succ);
                }
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
                for m in stack.drain(at..) {
                    on_stack[m as usize] = false;
                    if m != virt {
                        cyclic[m as usize] = looped && flag[m as usize];
                    }
                }
            }
            frames.pop();
            succ.truncate(from);
            if let Some(&(parent, ..)) = frames.last() {
                low[parent as usize] = low[parent as usize].min(low[v as usize]);
            }
        }
    }
    cyclic
}

#[cfg(test)]
mod tests {
    use super::coinductive_cycles;

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
}
