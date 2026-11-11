//! Sparse closure copies. Index an enclosing body once, then copy only a
//! selected sub-body's instructions, operand records and referenced locals.

use super::{Site, inlinable, inline_bound_calls};
use crate::ir::{CaptureMode, ReadMode, Topology, Word};
use crate::own::splice::{self, Tables};
use hd_base::{HashMap, HashSet, Range32, SliceSort as _, SubId};

use crate::ir::{
    Body, BodyKind, INLINED, Loc, NONE, Op, Ref, Root, Tag, local_flags, place, slots, u32_of,
};

// Test-only work accounting: absent from the compiler used by benchmarks.
macro_rules! work {
    ($amount:expr) => {
        #[cfg(test)]
        measurements::add($amount);
    };
}
#[cfg(test)]
mod measurements {
    use std::cell::Cell;
    thread_local! { static WORK: Cell<usize> = const { Cell::new(0) }; }
    pub(super) fn add(n: usize) {
        WORK.with(|work| work.set(work.get() + n));
    }
    pub(super) fn reset() {
        WORK.with(|work| work.set(0));
    }
    pub(super) fn total() -> usize {
        WORK.with(Cell::get)
    }
}

/// Shared instruction and side-row groups for all closure copies of a body.
struct Index {
    rows: Vec<Vec<usize>>,
    fallback: Vec<Vec<[u32; 2]>>,
    held: Vec<Vec<(u32, u32)>>,
    shared: Vec<Vec<u32>>,
    roots: HashSet<u32>,
    params: Vec<Vec<u32>>,
    recursive: crate::own::RecursiveBindings,
    copy_roots: Vec<Vec<(hd_base::LocalId, hd_base::LocalId)>>,
}

impl Index {
    /// O(instructions + side rows) once, independent of the number of calls.
    #[must_use]
    pub fn new(body: &Body) -> Self {
        work!(body.sub_root.len());
        let subs = body.inst_subs();
        let mut rows = vec![Vec::new(); body.sub_root.len()];
        for (i, &sub) in subs.iter().enumerate() {
            work!(1);
            rows[sub as usize].push(i);
        }
        let mut fallback = vec![Vec::new(); rows.len()];
        for &row in &body.fallback {
            work!(1);
            fallback[subs[row[0] as usize] as usize].push(row);
        }
        let mut held = vec![Vec::new(); rows.len()];
        for &row in &body.held_args {
            work!(1);
            held[subs[row.0 as usize] as usize].push(row);
        }
        let mut shared = vec![Vec::new(); rows.len()];
        for &(sub, capture) in &body.inlined_shared {
            work!(1);
            shared[sub.idx()].push(capture);
        }
        work!(body.local_ty.len());
        let mut params = vec![crate::own::params(body)];
        for r in body.sub_params.iter().skip(1) {
            work!(r.len as usize);
            params
                .push(body.extra[(r.start + 1) as usize..(r.start + 1 + r.len) as usize].to_vec());
        }
        let mut copy_roots = vec![Vec::new(); rows.len()];
        for &(sub, snapshot, creator) in &body.inlined_capture_origins {
            work!(1);
            copy_roots[sub.idx()].push((snapshot, creator));
        }
        let recursive = crate::own::RecursiveBindings::new(body);
        work!(recursive.work_units());
        Self {
            copy_roots,
            recursive,
            params,
            roots: body.sub_root.iter().copied().collect(),
            shared,
            rows,
            fallback,
            held,
        }
    }

    fn extract(
        &self,
        body: &Body,
        sub: usize,
        captures: &[(u32, crate::ir::CaptureMode)],
        mut budget: usize,
    ) -> Option<(Body, HashMap<u32, u32>)> {
        let order = self.rows.get(sub)?;
        let params = self.params.get(sub)?;
        budget = budget.checked_sub(
            order.len()
                + params.len()
                + captures.len()
                + self.fallback.get(sub)?.len() * 2
                + self.held.get(sub)?.len() * 2
                + self.shared.get(sub)?.len() * 4
                + self.copy_roots.get(sub)?.len() * 3,
        )?;
        let values: HashMap<_, _> = order
            .iter()
            .enumerate()
            .map(|(new, &old)| (u32_of(old), u32_of(new)))
            .collect();
        let parameters: HashSet<_> = params.iter().copied().collect();
        let mut locals: HashSet<_> = params
            .iter()
            .copied()
            .chain(captures.iter().map(|c| c.0))
            .collect();
        for &capture in self.shared.get(sub)? {
            work!(1);
            locals.insert(body.cap_local[capture as usize].raw());
        }
        for &(snapshot, creator) in self.copy_roots.get(sub)? {
            work!(1);
            locals.insert(snapshot.raw());
            locals.insert(creator.raw());
        }
        let mut records = HashMap::default();
        let mut labels = HashMap::default();
        let mut places = HashSet::default();
        let mut out = Body::new(body.item, BodyKind::Fn);
        out.fallback.clone_from(self.fallback.get(sub)?);
        out.held_args.clone_from(self.held.get(sub)?);
        for &[_, local] in &out.fallback {
            work!(1);
            if local != 0 {
                locals.insert(local - 1);
            }
        }
        for &old in order {
            work!(1);
            let tag = body.tags[old];
            let mut data = body.data[old];
            let ops = tag.operands();
            for (word, op) in data.iter_mut().zip([ops.0, ops.1]) {
                work!(1);
                if *word == NONE {
                    continue;
                }
                match op {
                    Op::Values | Op::Blocks | Op::Record | Op::Place => {
                        if op == Op::Place && *word & place::BARE_LOCAL != 0 {
                            locals.insert(*word & !place::BARE_LOCAL);
                            continue;
                        }
                        if !records.contains_key(word) {
                            budget = budget.checked_sub(body.record(*word).len() + 1)?;
                        }
                        let at = *records.entry(*word).or_insert_with(|| {
                            let at = u32_of(out.extra.len());
                            let words = body.record(*word);
                            // A block list may declare another sub-body. It
                            // has no execution in this selected sub-body.
                            let copied: Vec<_> = words
                                .iter()
                                .copied()
                                .filter(|x| {
                                    !(op == Op::Blocks || tag == Tag::Block)
                                        || values.contains_key(x)
                                        || !self.roots.contains(x)
                                })
                                .collect();
                            out.extra.push(u32_of(copied.len()));
                            work!(words.len());
                            out.extra.extend_from_slice(&copied);
                            at
                        });
                        *word = at;
                        if op == Op::Place {
                            places.insert(at);
                            if let Root::Local(local) = out.view(at).root {
                                locals.insert(local);
                            }
                        }
                    }
                    Op::Meta if matches!(tag, Tag::ItemRef | Tag::ProviderGet) => {
                        if !records.contains_key(word) {
                            budget = budget.checked_sub(body.record(*word).len() + 1)?;
                        }
                        let at = *records.entry(*word).or_insert_with(|| {
                            let at = u32_of(out.extra.len());
                            out.extra.push(u32_of(body.record(*word).len()));
                            work!(body.record(*word).len());
                            out.extra.extend_from_slice(body.record(*word));
                            at
                        });
                        *word = at;
                    }
                    Op::Label => {
                        let next = u32_of(labels.len());
                        let label = *labels.entry(*word).or_insert(next);
                        if label as usize == out.label_inst.len() {
                            out.label_inst
                                .push(*values.get(body.label_inst.get(*word as usize)?)?);
                        }
                        *word = label;
                    }
                    // Scalar metadata is unchanged; value/block identities
                    // are relocated after their records have been copied.
                    Op::None | Op::Value | Op::Inst | Op::Block | Op::Meta => {}
                }
            }
            out.tags.push(tag);
            out.data.push(data);
            out.ty.push(body.ty[old]);
            out.syn.push(body.syn[old]);
            out.anchors.push(body.anchor(u32_of(old)));
        }
        let mut locals: Vec<_> = locals.into_iter().collect();
        locals.sort_stable();
        let local_map: HashMap<_, _> = locals
            .iter()
            .enumerate()
            .map(|(new, &old)| (old, u32_of(new)))
            .collect();
        for &local in &locals {
            work!(1);
            let l = local as usize;
            out.local_ty.push(body.local_ty[l]);
            out.local_name.push(body.local_name[l]);
            out.local_syn.push(body.local_syn[l]);
            out.local_flags.push(
                (body.local_flags[l] & !local_flags::PARAM)
                    | if parameters.contains(&local) {
                        local_flags::PARAM
                    } else {
                        0
                    },
            );
        }
        for &capture in self.shared.get(sub)? {
            work!(1);
            out.inlined_shared
                .push((SubId::from_raw(0), u32_of(out.cap_local.len())));
            out.cap_local.push(hd_base::LocalId::from_raw(
                local_map[&body.cap_local[capture as usize].raw()],
            ));
            out.cap_mode.push(body.cap_mode[capture as usize]);
        }
        for &(snapshot, creator) in self.copy_roots.get(sub)? {
            work!(1);
            out.inlined_capture_origins.push((
                SubId::from_raw(0),
                hd_base::LocalId::from_raw(local_map[&snapshot.raw()]),
                hd_base::LocalId::from_raw(local_map[&creator.raw()]),
            ));
        }
        for at in places {
            work!(1);
            if out.extra[at as usize + 1] == place::ROOT_LOCAL {
                let local = &mut out.extra[at as usize + 2];
                *local = *local_map.get(local)?;
            }
        }
        let mut constants = HashMap::default();
        let mut mapped = HashSet::default();
        for i in 0..out.len() {
            work!(1);
            let mut operands = Vec::new();
            slots(&out, i, |loc, value, _| {
                if value != NONE {
                    operands.push((loc, value));
                }
            });
            for (loc, value) in operands {
                work!(1);
                if let Loc::Extra(at) = loc
                    && !mapped.insert(at)
                {
                    continue;
                }
                let value = if value & Ref::CONST_BIT != 0 {
                    let old = value & !Ref::CONST_BIT;
                    let new = if let Some(&new) = constants.get(&old) {
                        new
                    } else {
                        budget = budget.checked_sub(1)?;
                        let mut constant = body.consts[old as usize];
                        if constant.0 == hd_types::Ty::STRING {
                            let string = &body.strings[usize::try_from(constant.1).ok()?];
                            budget = budget.checked_sub(string.len())?;
                            work!(string.len());
                            constant.1 = out.strings.len() as u64;
                            out.strings.push(string.clone());
                        }
                        let new = u32_of(out.consts.len());
                        out.consts.push(constant);
                        constants.insert(old, new);
                        new
                    };
                    Ref::CONST_BIT | new
                } else {
                    *values.get(&value)?
                };
                match loc {
                    Loc::Data(k) => out.data[i][usize::from(k)] = value,
                    Loc::Extra(at) => out.extra[at as usize] = value,
                }
            }
            let ops = out.tags[i].operands();
            for (word, op) in out.data[i].iter_mut().zip([ops.0, ops.1]) {
                work!(1);
                if op == Op::Place && *word & place::BARE_LOCAL != 0 {
                    *word = local_map.get(&(*word & !place::BARE_LOCAL))? | place::BARE_LOCAL;
                }
            }
        }
        for row in &mut out.fallback {
            work!(1);
            row[0] = *values.get(&row[0])?;
            if row[1] != 0 {
                row[1] = local_map.get(&(row[1] - 1))? + 1;
            }
        }
        for row in &mut out.held_args {
            work!(1);
            *row = (*values.get(&row.0)?, *values.get(&row.1)?);
        }
        out.sub_root.push(*values.get(body.sub_root.get(sub)?)?);
        out.sub_params.push(Range32::default());
        out.sub_parent.push(SubId::from_raw(0));
        out.sub_flags.push(0);
        copy_frames(body, &mut out, budget)?;
        Some((out, local_map))
    }
}

fn copy_frames(source: &Body, out: &mut Body, mut budget: usize) -> Option<()> {
    let frame = |anchor: u32| {
        if anchor == NONE || anchor & INLINED == 0 {
            return None;
        }
        let at = anchor & !INLINED;
        let k = source
            .inlined
            .partition_point(|f| f[0] <= at)
            .checked_sub(1)?;
        (at - source.inlined[k][0] < source.inlined[k][1]).then_some((k, at))
    };
    let mut todo = out.anchors.clone();
    let mut used = HashSet::default();
    while let Some(anchor) = todo.pop() {
        work!(1);
        if let Some((k, at)) = frame(anchor)
            && used.insert((k, at))
        {
            // Four frame words and at most two origin words per projection.
            // Reject before walking the rest of an arbitrarily deep ancestry.
            budget = budget.checked_sub(6)?;
            todo.push(source.inlined[k][3]);
        }
    }
    let mut used: Vec<_> = used.into_iter().collect();
    used.sort_stable();
    let positions: HashMap<_, _> = used
        .iter()
        .enumerate()
        .map(|(i, &key)| (key, u32_of(i)))
        .collect();
    let remap = |anchor| frame(anchor).map_or(anchor, |key| INLINED | positions[&key]);
    for anchor in &mut out.anchors {
        work!(1);
        *anchor = remap(*anchor);
    }
    let mut cursor = 0;
    while cursor < used.len() {
        work!(1);
        let k = used[cursor].0;
        let first = cursor;
        while cursor < used.len() && used[cursor].0 == k {
            work!(1);
            let at = used[cursor].1;
            let original = source
                .inlined_origins
                .binary_search_by_key(&at, |row| row[0])
                .map_or(at - source.inlined[k][0], |i| source.inlined_origins[i][1]);
            if original != u32_of(cursor - first) {
                out.inlined_origins.push([u32_of(cursor), original]);
            }
            cursor += 1;
        }
        let f = source.inlined[k];
        out.inlined
            .push([u32_of(first), u32_of(cursor - first), f[2], remap(f[3])]);
    }
    Some(())
}

struct Flow {
    values: Vec<u32>,
    locals: Vec<u32>,
    calls: Vec<Vec<usize>>,
    escapes: Vec<bool>,
}

impl Flow {
    fn new(body: &Body) -> Self {
        Self::build(body, false)
    }

    fn build(body: &Body, parameters: bool) -> Self {
        let n = body.len();
        let unresolved = u32::MAX;
        let mut stores = vec![(0u32, unresolved); body.local_ty.len()];
        for i in 0..n {
            work!(1);
            if let Some((local, whole)) = body.stored_local(i) {
                let s = &mut stores[local.idx()];
                *s = (s.0 + 1, if whole { body.data[i][1] } else { unresolved });
            }
            if body.tags[i] == Tag::Ref
                && let Some(local) = body.local_in_place(body.data[i][0])
            {
                stores[local.idx()].0 = 2;
            }
        }
        let mut edges = vec![Vec::new(); n + stores.len()];
        let mut origins = vec![unresolved; edges.len()];
        let mut queue = Vec::new();
        let mut link = |from: u32, to: usize| {
            if let Some(value) = Ref(from).as_inst() {
                edges[value.0 as usize].push(to);
            }
        };
        for (local, &(count, value)) in stores.iter().enumerate() {
            work!(1);
            if count == 1
                && body.local_flags[local]
                    & (local_flags::PARAM | local_flags::MUTATED | local_flags::CAPTURED_ASSIGNED)
                    == 0
            {
                link(value, n + local);
            }
        }
        for (i, origin) in origins.iter_mut().enumerate().take(n) {
            work!(1);
            if body.tags[i] == Tag::Closure && !body.is_instantiation(i) {
                *origin = u32_of(i);
                queue.push(i);
            } else if body.tags[i] == Tag::Coerce && body.same_value_coercion(i) {
                link(body.data[i][0], i);
            }
        }
        for i in 0..n {
            work!(1);
            if body.tags[i] == Tag::Read
                && let Some(local) = body.local_read(i)
            {
                if parameters
                    && stores[local.idx()].0 == 0
                    && body.local_flags[local.idx()] & local_flags::PARAM != 0
                    && body.local_flags[local.idx()] & local_flags::MUTATED == 0
                    && origins[n + local.idx()] == unresolved
                {
                    // A parameter's first whole read is its identity, so
                    // origin IDs still name real same-sub-body instructions.
                    origins[n + local.idx()] = u32_of(i);
                    queue.push(n + local.idx());
                }
                edges[n + local.idx()].push(i);
            }
        }
        let mut cursor = 0;
        while cursor < queue.len() {
            work!(1);
            let node = queue[cursor];
            cursor += 1;
            for &user in &edges[node] {
                work!(1);
                if origins[user] == unresolved {
                    origins[user] = origins[node];
                    queue.push(user);
                }
            }
        }
        let locals = origins.split_off(n);
        let mut out = Self {
            values: origins,
            locals,
            calls: vec![Vec::new(); n],
            escapes: vec![false; n],
        };
        let subs = body.inst_subs();
        for i in 0..n {
            work!(1);
            slots(body, i, |loc, value, kind| {
                if !matches!(kind, Word::Value(_) | Word::Provider) {
                    return;
                }
                let Some(value) = Ref(value).as_inst() else {
                    return;
                };
                let made = out.values[value.0 as usize];
                if made == unresolved {
                    return;
                }
                let alias =
                    out.values[i] == made && matches!(body.tags[i], Tag::Read | Tag::Coerce);
                let binding_used = body
                    .stored_local(i)
                    .is_some_and(|(l, whole)| whole && out.locals[l.idx()] == made);
                let called = body.tags[i] == Tag::CallValue
                    && matches!(loc, Loc::Data(0))
                    && subs[i] == subs[made as usize];
                if called {
                    out.calls[made as usize].push(i);
                }
                if !(alias || binding_used || called) {
                    out.escapes[made as usize] = true;
                }
            });
            if body.tags[i] == Tag::Closure {
                let captures = body.record(body.data[i][1]);
                if let [start, count, ..] = captures {
                    for local in &body.cap_local[*start as usize..(*start + *count) as usize] {
                        work!(1);
                        let made = out.locals[local.idx()];
                        if made != unresolved {
                            out.escapes[made as usize] = true;
                        }
                    }
                }
            }
            if body.tags[i].operands().0 == Op::Place
                && let Some(local) = body.local_in_place(body.data[i][0])
            {
                let made = out.locals[local.idx()];
                // A cleanup borrow is an ownership lifetime use, not another
                // function-value alias we may erase with its constructor.
                if made != unresolved
                    && (!matches!(body.tags[i], Tag::Read | Tag::Store)
                        || body.tags[i] == Tag::Read && body.data[i][1] == ReadMode::Borrow as u32)
                {
                    out.escapes[made as usize] = true;
                }
            }
        }
        for &(_, capture) in &body.inlined_shared {
            work!(1);
            let made = out.locals[body.cap_local[capture as usize].idx()];
            if made != unresolved {
                out.escapes[made as usize] = true;
            }
        }
        out
    }
}

/// Known literal argument positions for each direct call. Build once per
/// immutable caller round, using the same def-use worklist as literal copies.
/// Cost O(instructions + locals + operands + side rows), no per-call rescan.
#[must_use]
pub fn literal_arguments(body: &Body) -> Vec<Vec<usize>> {
    if !body.tags.contains(&Tag::Closure) {
        return Vec::new();
    }
    let flow = Flow::new(body);
    let mut out = vec![Vec::new(); body.len()];
    for (i, args) in out.iter_mut().enumerate() {
        work!(1);
        if super::known_call(body, i).is_none() {
            continue;
        }
        for (k, &value) in super::arguments(body, i).iter().enumerate() {
            work!(1);
            if Ref(value)
                .as_inst()
                .is_some_and(|v| flow.values[v.0 as usize] != NONE)
            {
                args.push(k);
            }
        }
    }
    out
}

/// For each parameter, whether its identity is only called or immutably
/// aliased in this consumer. Returned/stored/captured/reassigned callbacks
/// keep their existing out-of-line eligibility and environment contract.
/// Call only on the bounded consumer copy, not once per input on the caller.
#[must_use]
pub fn called_parameters(body: &Body) -> Vec<bool> {
    let flow = Flow::build(body, true);
    crate::own::params(body)
        .into_iter()
        .map(|local| {
            work!(1);
            let origin = flow.locals[local as usize];
            origin != NONE
                && !flow.escapes[origin as usize]
                && !flow.calls[origin as usize].is_empty()
        })
        .collect()
}

struct Template {
    body: Body,
    captures: Vec<(u32, u32, CaptureMode)>,
}

fn template(body: &Body, index: &Index, made: usize, limit: usize) -> Option<Template> {
    let [start, count] = body.record(body.data[made][1]) else {
        return None;
    };
    // Capture lifetime uses alone already consume the instruction allowance.
    if *count as usize >= limit {
        return None;
    }
    let captures: Vec<_> = (*start..*start + *count)
        .map(|c| {
            let local = body.cap_local[c as usize];
            (
                local.raw(),
                index
                    .recursive
                    .capture_mode(local, body.cap_mode[c as usize]),
            )
        })
        .collect();
    let sub = body.data[made][0] as usize;
    if sub == 0 || body.sub_is_generic(sub) {
        return None;
    }
    // Independently cap projected IR and source metadata. The instruction
    // heuristic does not count operand records or inline ancestry; without
    // this bound many tiny literals could each scan the same deep trace.
    let (copy, locals) = index.extract(body, sub, &captures, limit.saturating_mul(16))?;
    let captures: Vec<_> = captures
        .into_iter()
        .map(|(outer, mode)| (locals[&outer], outer, mode))
        .collect();
    // Each captured lifetime contributes one cleanup use, plus its suite.
    let capture_cost = captures.len() + usize::from(!captures.is_empty());
    inlinable(&copy, limit.saturating_sub(capture_cost)).then_some(Template {
        body: copy,
        captures,
    })
}

/// Copy ordinary small literals only when every use of their function value
/// is a visible call or an immutable alias. Escaping values retain their
/// environment and its existing ownership contract.
pub fn inline_local_closures(
    body: &mut Body,
    limit: usize,
    fits: &dyn Fn(&Body, usize, &mut Body) -> bool,
) -> Result<bool, String> {
    if !body.tags.contains(&Tag::Closure) {
        return Ok(false);
    }
    if (0..body.len()).any(|i| {
        matches!(body.tags[i], Tag::Call | Tag::Await)
            && body
                .record(body.data[i][1])
                .iter()
                .rev()
                .nth(2)
                .is_some_and(|&p| p > 2)
    }) {
        return Ok(false);
    }
    let flow = Flow::new(body);
    let index = Index::new(body);
    let mut selected = Vec::new();
    for made in 0..body.len() {
        work!(1);
        if flow.calls[made].is_empty() || flow.escapes[made] {
            continue;
        }
        let Some(template) = template(body, &index, made, limit) else {
            continue;
        };
        if flow.calls[made]
            .iter()
            .any(|&call| !fits(body, call, &mut template.body.clone()))
        {
            continue;
        }
        selected.push((body.data[made], made, template));
    }
    if selected.is_empty() {
        return Ok(false);
    }
    let topo = Topology::new(body);
    let mut edits = Tables::default();
    let mut environments = HashMap::default();
    for &(key, made, ref template) in &selected {
        work!(1);
        let mut bindings = Vec::new();
        for &(copy, outer, mode) in &template.captures {
            work!(1);
            let local = if mode.shares() {
                outer
            } else {
                let local = u32_of(body.local_ty.len());
                body.local_ty.push(body.local_ty[outer as usize]);
                body.local_name.push(body.local_name[outer as usize]);
                body.local_syn.push(body.syn[made]);
                body.local_flags
                    .push(local_flags::READ | local_flags::ASSIGNED);
                let read = edits.insert(
                    topo.blk[made],
                    topo.pos[made],
                    Tag::Read,
                    [place::BARE_LOCAL | outer, ReadMode::Use as u32],
                    body.local_ty[outer as usize],
                    body.syn[made],
                );
                body.inlined_capture_origins.push((
                    SubId::from_raw(topo.sub[made]),
                    hd_base::LocalId::from_raw(local),
                    hd_base::LocalId::from_raw(outer),
                ));
                let value = u32_of(body.len()) + read;
                edits.insert(
                    topo.blk[made],
                    topo.pos[made],
                    Tag::Store,
                    [place::BARE_LOCAL | local, value],
                    hd_types::Ty::VOID,
                    body.syn[made],
                );
                local
            };
            bindings.push((copy, local));
        }
        bindings.sort_stable();
        environments.insert(key, (template.body.clone(), bindings));
    }
    splice::apply(body, &topo, &mut edits)?;
    let flow = Flow::new(body);
    let mut sites = Vec::new();
    for made in 0..body.len() {
        work!(1);
        let Some((copy, bindings)) = environments
            .get(&body.data[made])
            .filter(|_| body.tags[made] == Tag::Closure)
        else {
            continue;
        };
        for &call in &flow.calls[made] {
            work!(1);
            let mut copy = copy.clone();
            if fits(body, call, &mut copy) {
                sites.push((
                    Site {
                        call,
                        copy,
                        by_value: 0,
                        by_ref: 0,
                    },
                    bindings.clone(),
                ));
            }
        }
    }
    sites.sort_stable_by_key(|(site, _)| std::cmp::Reverse(site.call));
    let copied = inline_bound_calls(body, sites)?;
    if !copied.contains(&true) {
        return Ok(false);
    }
    // Calls no longer reference the function value. Remove only closed alias
    // graphs with no observable use; capture snapshots remain ordinary locals.
    let flow = Flow::new(body);
    let topo = Topology::new(body);
    let mut edits = Tables::default();
    let dead = |made: u32| {
        made != NONE
            && environments.contains_key(&body.data[made as usize])
            && !flow.escapes[made as usize]
            && flow.calls[made as usize].is_empty()
    };
    let mut definitions = HashSet::default();
    for i in 0..body.len() {
        work!(1);
        let made = flow.values[i];
        if dead(made) {
            if body.tags[i] == Tag::Closure {
                definitions.insert(u32_of(i));
            }
            edits.remove(u32_of(i));
        } else if let Some((local, whole)) = body.stored_local(i)
            && whole
            && dead(flow.locals[local.idx()])
        {
            edits.remove(u32_of(i));
        }
    }
    erase_definitions(body, &topo, &definitions, &mut edits, &index.recursive);
    splice::apply(body, &topo, &mut edits)?;
    // With no live function constructor, dead sub IDs have no code identity
    // left to preserve. A bounded root projection lets ordinary callers copy
    // the resulting small body, including its logical shared-cell facts.
    if !body.tags.contains(&Tag::Closure) && body.label.is_empty() && body.susp.is_empty() {
        let index = Index::new(body);
        if let Some((mut root, _)) = index.extract(body, 0, &[], limit.saturating_mul(16)) {
            root.kind = body.kind;
            if super::copyable(&root) {
                *body = root;
            }
        }
    }
    Ok(true)
}

/// Sub IDs are code identities used by collection. Keep those identities,
/// but replace a fully eliminated literal's unreachable body by a trap, not
/// its old accesses to captures that no longer exist. No live constructor
/// may name that body. This is dead-sub-body elimination, not a new fallback.
fn erase_definitions(
    body: &mut Body,
    topo: &Topology,
    removed: &HashSet<u32>,
    edits: &mut Tables,
    recursive: &crate::own::RecursiveBindings,
) {
    let mut remaining = vec![false; body.sub_root.len()];
    for i in 0..body.len() {
        work!(1);
        if body.tags[i] == Tag::Closure && !removed.contains(&u32_of(i)) {
            remaining[body.data[i][0] as usize] = true;
        }
    }
    let mut dead_sub = vec![false; remaining.len()];
    for &made in removed {
        work!(1);
        let sub = body.data[made as usize][0] as usize;
        if !remaining[sub] {
            dead_sub[sub] = true;
        }
    }
    body.inlined_capture_origins
        .retain(|&(sub, _, _)| !dead_sub[sub.idx()]);
    let mut dead_inst = vec![false; body.len()];
    for (i, &sub) in topo.sub.iter().enumerate() {
        work!(1);
        if dead_sub[sub as usize] && u32_of(i) != body.sub_root[sub as usize] {
            dead_inst[i] = true;
            edits.remove(u32_of(i));
        }
    }
    for (sub, &dead) in dead_sub.iter().enumerate() {
        work!(1);
        if !dead {
            continue;
        }
        let root = body.sub_root[sub];
        body.data[root as usize] = [super::push_record(body, &[]), NONE];
        body.ty[root as usize] = hd_types::Ty::NEVER;
        let trap = edits.insert(
            root,
            0,
            Tag::Unreachable,
            [NONE, NONE],
            hd_types::Ty::NEVER,
            body.syn[root as usize],
        );
        edits.rewrite(root, Loc::Data(1), trap);
    }
    body.fallback.retain(|row| !dead_inst[row[0] as usize]);
    body.held_args.retain(|row| !dead_inst[row.0 as usize]);
    let mut labels = vec![NONE; body.label_inst.len()];
    let mut kept = Vec::new();
    for (old, &inst) in body.label_inst.iter().enumerate() {
        work!(1);
        if !dead_inst[inst as usize] {
            labels[old] = u32_of(kept.len());
            kept.push(inst);
        }
    }
    for (i, &dead) in dead_inst.iter().enumerate() {
        work!(1);
        if dead {
            continue;
        }
        let ops = body.tags[i].operands();
        for (word, op) in body.data[i].iter_mut().zip([ops.0, ops.1]) {
            work!(1);
            if op == Op::Label {
                *word = labels[*word as usize];
            }
        }
    }
    body.label_inst = kept;
    // The function value disappears, but a shared capture remains a logical
    // alias of its cell, including when a parent writes after a reader call.
    let mut logical = std::mem::take(&mut body.inlined_shared);
    for (made, &tag) in body.tags.iter().enumerate() {
        work!(1);
        if tag != Tag::Closure || !removed.contains(&u32_of(made)) {
            continue;
        }
        let cap = body.record(body.data[made][1]);
        for c in cap[0]..cap[0] + cap[1] {
            work!(1);
            if recursive
                .capture_mode(body.cap_local[c as usize], body.cap_mode[c as usize])
                .shares()
            {
                logical.push((SubId::from_raw(topo.sub[made]), c));
            }
        }
    }
    let old_locals = std::mem::take(&mut body.cap_local);
    let old_modes = std::mem::take(&mut body.cap_mode);
    let mut spans = HashMap::default();
    let mut records = HashSet::default();
    for (i, &dead) in dead_inst.iter().enumerate() {
        work!(1);
        if body.tags[i] != Tag::Closure || removed.contains(&u32_of(i)) || dead {
            continue;
        }
        let record = body.data[i][1];
        if !records.insert(record) {
            continue;
        }
        let cap = body.record(record);
        let key = (cap[0], cap[1]);
        let next = if let Some(&next) = spans.get(&key) {
            next
        } else {
            let next = u32_of(body.cap_local.len());
            let range = key.0 as usize..(key.0 + key.1) as usize;
            body.cap_local.extend_from_slice(&old_locals[range.clone()]);
            body.cap_mode.extend_from_slice(&old_modes[range]);
            spans.insert(key, next);
            next
        };
        body.extra[record as usize + 1] = next;
    }
    let mut seen = HashSet::default();
    for (sub, capture) in logical {
        work!(1);
        let c = capture as usize;
        if dead_sub[sub.idx()] || !seen.insert((sub, old_locals[c])) {
            continue;
        }
        let next = u32_of(body.cap_local.len());
        body.cap_local.push(old_locals[c]);
        body.cap_mode.push(old_modes[c]);
        body.inlined_shared.push((sub, next));
    }
    let mut captured = vec![false; body.local_ty.len()];
    for local in &body.cap_local {
        work!(1);
        captured[local.idx()] = true;
    }
    for (local, flag) in body.local_flags.iter_mut().enumerate() {
        work!(1);
        if !captured[local] {
            if *flag & local_flags::CAPTURED_ASSIGNED != 0 {
                *flag |= local_flags::MUTATED;
            }
            *flag &= !(local_flags::CAPTURED | local_flags::CAPTURED_ASSIGNED);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{inline_local_closures, measurements};
    use crate::ir::{
        Body, BodyKind, CaptureMode, INLINED, NONE, Tag, TirBuilder, TirSink, Topology,
        local_flags, u32_of, verify_structure,
    };
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::Ty;

    /// Independent literals, each reached through `aliases` immutable locals,
    /// all sharing the same `frames`-deep source ancestry. Generation itself
    /// is iterative, including the deep rejection control.
    fn generated(width: usize, aliases: usize, frames: usize) -> Body {
        let n = NodeIdx::from_raw(1);
        let fn_ty = Ty(1_000_000);
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let block = b.open_block();
        for _ in 0..width {
            let x = b.local(Ty::I32, Symbol::from_raw(0), local_flags::CAPTURED, n);
            let one = b.const_value(Ty::I32, 1);
            b.set(x, one, n);
            let sub = b.open_sub(&[]);
            let inner = b.open_block();
            b.capture(sub, x);
            let value = b.get(x, Ty::I32, n);
            let root = b.close_block(inner, Some(value), Ty::I32, n);
            let mut value = b.close_sub(sub, root, fn_ty, n);
            for _ in 0..aliases {
                let local = b.local(fn_ty, Symbol::from_raw(0), 0, n);
                b.set(local, value, n);
                value = b.get(local, fn_ty, n);
            }
            let args = b.refs_record(&[]);
            b.emit(Tag::CallValue, value.0, args, Ty::I32, n);
        }
        let root = b.close_block(block, None, Ty::VOID, n);
        let mut body = b
            .finish(root, &vec![CaptureMode::Copy; width])
            .expect("generated TIR");
        if frames != 0 {
            body.anchors = (0..body.len()).map(u32_of).collect();
            for k in 0..frames {
                body.inlined.push([
                    u32_of(k),
                    1,
                    body.item.raw(),
                    if k == 0 { 0 } else { INLINED | u32_of(k - 1) },
                ]);
            }
            let subs = body.inst_subs();
            for (i, sub) in subs.into_iter().enumerate() {
                if sub != 0 {
                    body.anchors[i] = INLINED | u32_of(frames - 1);
                }
            }
        }
        let errors = verify_structure(&body, &Topology::new(&body));
        assert!(errors.is_empty(), "{errors:?}");
        body
    }

    fn size(b: &Body) -> usize {
        // Side tables are input/output, not free work hidden by instruction count.
        b.len()
            + b.extra.len()
            + b.local_ty.len()
            + b.cap_local.len()
            + b.inlined.len() * 4
            + b.inlined_origins.len() * 2
            + b.anchors.len()
            + b.inlined_shared.len() * 2
            + b.inlined_capture_origins.len() * 3
    }

    fn run(
        width: usize,
        aliases: usize,
        frames: usize,
        expected: bool,
        shared: bool,
    ) -> (usize, usize) {
        let mut body = generated(width, aliases, frames);
        if shared {
            body.cap_mode.fill(CaptureMode::Shared);
        }
        let input = size(&body);
        measurements::reset();
        assert_eq!(
            inline_local_closures(&mut body, 16, &|_, _, _| true),
            Ok(expected)
        );
        let work = measurements::total();
        let output = size(&body);
        let errors = verify_structure(&body, &Topology::new(&body));
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(body.tags.contains(&Tag::CallValue), !expected);
        assert!(
            work < 80 * (input + output),
            "work={work}, input={input}, output={output}"
        );
        if expected {
            assert!(!body.tags.contains(&Tag::Closure));
            assert!(body.inlined.iter().all(|f| f[0] != NONE));
        }
        println!(
            "width={width} aliases={aliases} frames={frames}: input={input} output={output} work={work}"
        );
        (work, output)
    }

    #[test]
    fn counted_width_and_alias_depth_include_shared_frame_projection() {
        for shared in [false, true] {
            let small = run(100, 20, 20, true, shared);
            let large = run(1000, 20, 20, true, shared);
            assert!(large.0 <= 11 * small.0);
            assert!(large.1 <= 11 * small.1);
        }
    }

    #[test]
    fn provider_metadata_copies_stay_linear_in_width_and_source_depth() {
        let mut costs = Vec::new();
        for width in [1000, 10000] {
            let mut body = generated(width, 1, 20);
            let subs = body.inst_subs();
            for (i, sub) in subs.into_iter().enumerate() {
                if sub != 0 && body.tags[i] == Tag::Read {
                    let at = u32_of(body.extra.len());
                    body.extra.extend([1, Ty::I32.0]);
                    body.tags[i] = Tag::ProviderGet;
                    body.data[i] = [at, NONE];
                }
            }
            measurements::reset();
            assert_eq!(
                inline_local_closures(&mut body, 16, &|_, _, _| true),
                Ok(true)
            );
            costs.push(measurements::total());
            let keys: Vec<_> = (0..body.len())
                .filter(|&i| body.tags[i] == Tag::ProviderGet)
                .map(|i| body.record(body.data[i][0]))
                .collect();
            assert_eq!(keys.len(), width);
            assert!(keys.iter().all(|&key| key == [Ty::I32.0]));
        }
        assert!(costs[1] <= 11 * costs[0], "{costs:?}");
    }

    #[test]
    fn counted_deep_ancestry_stops_at_metadata_budget() {
        let shallow = run(1000, 1, 1000, false, false);
        let deep = run(1000, 1, 10000, false, false);
        // Tenfold unused source ancestry does not increase projection work.
        assert_eq!(deep.0, shallow.0);
    }
    #[test]
    fn logical_shared_capture_rows_relocate_through_an_ordinary_copy() {
        use crate::inline::{Site, copyable, inline_calls};
        use crate::ir::{Callee, Providers};
        use hd_types::TyList;

        let mut copy = generated(1, 1, 20);
        copy.cap_mode.fill(CaptureMode::Shared);
        assert_eq!(
            inline_local_closures(&mut copy, 16, &|_, _, _| true),
            Ok(true)
        );
        assert_eq!(copy.inlined_shared.len(), 1);
        assert!(copyable(&copy), "{}", crate::ir::print(&copy));
        let original = copy.cap_local[0].raw();
        let n = NodeIdx::from_raw(1);
        let mut b = TirBuilder::new(DefId::from_raw(2), BodyKind::Fn);
        let block = b.open_block();
        // A caller local makes every copied local change its identity.
        b.local(Ty::I32, Symbol::from_raw(0), 0, n);
        let call = b.call(
            &Callee::Item {
                def: copy.item,
                targs: TyList::EMPTY,
            },
            &[],
            Providers::None,
            Ty::VOID,
            n,
        );
        let root = b.close_block(block, Some(call), Ty::VOID, n);
        let mut body = b.finish(root, &[]).expect("caller TIR");
        assert_eq!(
            inline_calls(
                &mut body,
                vec![Site {
                    call: call.0 as usize,
                    copy,
                    by_value: 0,
                    by_ref: 0
                }]
            ),
            Ok(vec![true])
        );
        let (sub, capture) = body.inlined_shared[0];
        assert_eq!(sub.raw(), 0);
        assert_eq!(body.cap_local[capture as usize].raw(), original + 1);
        assert_eq!(body.cap_mode[capture as usize], CaptureMode::Shared);
        let errors = verify_structure(&body, &Topology::new(&body));
        assert!(errors.is_empty(), "{errors:?}");
    }
    #[test]
    fn copy_capture_provenance_relocates_with_its_creator() {
        use crate::inline::{Site, inline_calls};
        use crate::ir::{Callee, Providers};
        use hd_types::TyList;

        let mut copy = generated(1, 1, 20);
        assert_eq!(
            inline_local_closures(&mut copy, 16, &|_, _, _| true),
            Ok(true)
        );
        let (scope, snapshot, creator) = copy.inlined_capture_origins[0];
        assert_eq!(scope.raw(), 0);
        assert!(creator.idx() < snapshot.idx());
        let n = NodeIdx::from_raw(1);
        let mut b = TirBuilder::new(DefId::from_raw(2), BodyKind::Fn);
        let block = b.open_block();
        b.local(Ty::I32, Symbol::from_raw(0), 0, n);
        let call = b.call(
            &Callee::Item {
                def: copy.item,
                targs: TyList::EMPTY,
            },
            &[],
            Providers::None,
            Ty::VOID,
            n,
        );
        let root = b.close_block(block, Some(call), Ty::VOID, n);
        let mut body = b.finish(root, &[]).expect("caller TIR");
        assert_eq!(
            inline_calls(
                &mut body,
                vec![Site {
                    call: call.0 as usize,
                    copy,
                    by_value: 0,
                    by_ref: 0
                }]
            ),
            Ok(vec![true])
        );
        let (scope, moved_snapshot, moved_creator) = body.inlined_capture_origins[0];
        assert_eq!(scope.raw(), 0);
        assert_eq!(moved_snapshot.raw(), snapshot.raw() + 1);
        assert_eq!(moved_creator.raw(), creator.raw() + 1);
        let errors = verify_structure(&body, &Topology::new(&body));
        assert!(errors.is_empty(), "{errors:?}");
    }
    fn consumer(aliases: usize, control: usize) -> Body {
        let n = NodeIdx::from_raw(1);
        let fn_ty = Ty(1_000_000);
        let mut b = TirBuilder::new(DefId::from_raw(2), BodyKind::Fn);
        let block = b.open_block();
        let parameter = b.local(fn_ty, Symbol::NONE, local_flags::PARAM, n);
        let mut value = b.get(parameter, fn_ty, n);
        for _ in 0..aliases {
            let local = b.local(fn_ty, Symbol::NONE, 0, n);
            b.set(local, value, n);
            value = b.get(local, fn_ty, n);
        }
        if control == 1 {
            b.set(parameter, value, n);
        }
        if control == 2 {
            let values = b.refs_record(&[value]);
            b.emit(Tag::NewStruct, NONE, values, fn_ty, n);
        }
        if control != 4 {
            let args = b.refs_record(&[]);
            b.emit(Tag::CallValue, value.0, args, Ty::VOID, n);
        }
        let root = b.close_block(
            block,
            (control == 3).then_some(value),
            if control == 3 { fn_ty } else { Ty::VOID },
            n,
        );
        b.finish(root, &[]).expect("generated consumer")
    }

    #[test]
    fn consumer_admission_preserves_reassigned_stored_and_returned_callbacks() {
        for control in 0..5 {
            let body = consumer(20, control);
            assert_eq!(
                super::called_parameters(&body),
                [control == 0],
                "control {control}"
            );
        }
    }

    #[test]
    fn literal_argument_and_consumer_work_is_linear_in_width_and_alias_depth() {
        for (width, depth) in [(1, 1), (1000, 1), (1, 20), (1000, 20)] {
            let mut body = generated(width, depth, 20);
            for i in 0..body.len() {
                if body.tags[i] == Tag::CallValue {
                    let literal = body.data[i][0];
                    let callee = super::super::push_record(&mut body, &[0, 2, 0]);
                    let args = super::super::push_record(&mut body, &[literal, 0, 0, 0]);
                    body.tags[i] = Tag::Call;
                    body.data[i] = [callee, args];
                }
            }
            let copy = consumer(depth, 0);
            measurements::reset();
            let inputs = super::literal_arguments(&body);
            let parameters = super::called_parameters(&copy);
            let mut admitted = 0;
            for input in inputs.iter().filter(|input| !input.is_empty()) {
                assert_eq!(input, &[0]);
                assert!(input.iter().any(|&k| parameters.get(k) == Some(&true)));
                admitted += 1;
            }
            assert_eq!(admitted, width);
            let work = measurements::total();
            let rows = body.len()
                + body.extra.len()
                + body.local_ty.len()
                + copy.len()
                + copy.extra.len()
                + copy.local_ty.len();
            assert!(work <= 32 * rows, "{width}x{depth}: {work} for {rows} rows");
        }
    }
}
