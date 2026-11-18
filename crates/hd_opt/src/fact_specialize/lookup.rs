//! Read-only nullable searches, proved from checked code rather than names.
//!
//! The proof follows return values to a dynamic downcast of a collection
//! element. Local bindings and projections share memoized provenance. Every
//! loop must have a finite, unchanged-length induction; writes and calls that
//! the proof cannot account for keep the original call.

use hd_base::{DefId, Hash128, HashMap, HashSet, LocalId, StableHasher};
use hd_tir::ir::{
    Body, Callee, Coercion, IntrinsicOp, PlaceRoot, PrimOp, Proj, Ref, Tag, Topology,
};
use hd_types::{Prim, Ty, TyList};

use super::{FactEnv, FactLookup};

type Path = usize;

#[derive(Clone, Copy)]
struct Summary {
    key: Ty,
    parameter: usize,
    projection: Path,
    digest: Hash128,
}

/// Declaration summaries share projection suffixes instead of copying the
/// growing path at each wrapper. Recursive components remain unknown: their
/// termination is not established by a finite collection search.
#[derive(Default)]
pub(crate) struct Memo {
    definitions: HashMap<DefId, Option<Summary>>,
    paths: Paths,
}

pub(super) fn lookup(env: &dyn FactEnv, def: DefId, args: TyList) -> Option<FactLookup> {
    if env.intrinsic(def).is_some() {
        return None;
    }
    let memo = env.memo()?;
    let mut guard = memo.fact_lookups.lock().ok()?;
    {
        let state = std::cell::RefCell::new(&mut *guard);
        crate::inline::bottom_up(
            def,
            &|def| state.borrow().definitions.contains_key(&def),
            &mut |def| {
                env.checked_body(def)
                    .filter(|(body, _)| candidate(body))
                    .map_or_else(Vec::new, |(body, _)| {
                        (0..body.len())
                            .filter_map(|i| match hd_tir::inline::plain_call(body, i) {
                                Some(Callee::Item { def, .. }) if env.intrinsic(def).is_none() => {
                                    Some(def)
                                }
                                _ => None,
                            })
                            .collect()
                    })
            },
            &mut |members, recursive| {
                let mut state = state.borrow_mut();
                for &def in members.iter() {
                    let result = if recursive {
                        None
                    } else {
                        env.checked_body(def)
                            .and_then(|(body, hash)| analyze(env, body, *hash, &mut state))
                    };
                    state.definitions.insert(def, result);
                }
            },
        );
    }
    let summary = guard.definitions.get(&def).copied().flatten()?;
    let key = hd_mono::subst(env.pool(), env, def, args, summary.key);
    if key == Ty::POISON
        || env.pool().has_param(key)
        || env.pool().has_assoc(key)
        || env.pool().has_opaque(key)
    {
        return None;
    }
    Some(FactLookup {
        key,
        parameter: summary.parameter,
        projection: guard.paths.flatten(summary.projection),
        digest: summary.digest,
    })
}

/// Flat storage avoids recursive destruction of input-deep projection paths.
#[derive(Default)]
pub(super) struct Paths {
    nodes: Vec<PathNode>,
    append: HashMap<(Path, Proj), Path>,
    joins: HashMap<(Path, Path), Path>,
}

enum PathNode {
    Step(Path, Proj),
    Join(Path, Path),
}

impl Paths {
    fn step(&mut self, path: Path, step: Proj) -> Path {
        if let Some(&known) = self.append.get(&(path, step)) {
            return known;
        }
        self.nodes.push(PathNode::Step(path, step));
        let out = self.nodes.len();
        self.append.insert((path, step), out);
        out
    }

    fn join(&mut self, first: Path, last: Path) -> Path {
        if first == 0 {
            return last;
        }
        if last == 0 {
            return first;
        }
        if let Some(&path) = self.joins.get(&(first, last)) {
            return path;
        }
        self.nodes.push(PathNode::Join(first, last));
        let path = self.nodes.len();
        self.joins.insert((first, last), path);
        path
    }

    pub(super) fn flatten(&self, path: Path) -> Vec<Proj> {
        enum Work {
            Path(Path),
            Step(Proj),
        }
        let mut work = vec![Work::Path(path)];
        let mut out = Vec::new();
        while let Some(next) = work.pop() {
            match next {
                Work::Path(0) => {}
                Work::Path(path) => match self.nodes[path - 1] {
                    PathNode::Step(prefix, step) => {
                        work.push(Work::Step(step));
                        work.push(Work::Path(prefix));
                    }
                    PathNode::Join(first, last) => {
                        work.push(Work::Path(last));
                        work.push(Work::Path(first));
                    }
                },
                Work::Step(step) => out.push(step),
            }
        }
        out
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Root {
    Parameter(LocalId),
    Value(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Source {
    root: Root,
    path: Path,
}

/// Do not expand a callee's receiver projection for an argument which only
/// comes from this body's parameters. Declaration-fact provenance requires a
/// literal record or factory; the shared origin cache makes alias chains cheap.
pub(super) fn potential_inputs(body: &Body) -> Vec<bool> {
    let mut sources = Sources::new(body);
    let mut paths = Paths::default();
    (0..body.len())
        .map(|i| {
            body.tags[i] == Tag::Call
                && body.call_args(i).iter().any(|&value| {
                    sources
                        .source(value, &mut paths, &mut || {})
                        .is_some_and(|source| {
                            matches!(source.root, Root::Value(value)
                if matches!(body.tags[value as usize],
                    Tag::NewStruct | Tag::Call | Tag::DefaultCall | Tag::Ref))
                        })
                })
        })
        .collect()
}

/// Each value and shared place is resolved once. A local with more than one
/// assignment has no immutable origin. The explicit stack also handles aliases
/// whose declaration order differs from instruction order, without recursion.
struct Sources<'a> {
    body: &'a Body,
    sets: Vec<Vec<(usize, u32)>>,
    parameters: Vec<bool>,
    values: Vec<Option<Source>>,
    seen: Vec<bool>,
    places: HashMap<u32, Option<Source>>,
}

fn candidate(body: &Body) -> bool {
    if body.sub_root.len() != 1 {
        return false;
    }
    let root = body.sub_root[0] as usize;
    let tail = body.data[root][1];
    (tail == hd_tir::ir::NONE && body.ty[root] == Ty::NEVER
        || body.tags.get(tail as usize).is_some_and(|tag| {
            matches!(
                tag,
                Tag::Call | Tag::Read | Tag::Coerce | Tag::NewVariant | Tag::Block | Tag::Return
            )
        }))
        && body.tags.iter().all(|tag| {
            matches!(
                tag,
                Tag::Block
                    | Tag::If
                    | Tag::Match
                    | Tag::SwitchTag
                    | Tag::ToArm
                    | Tag::Return
                    | Tag::Break
                    | Tag::Continue
                    | Tag::Loop
                    | Tag::NewVariant
                    | Tag::Store
                    | Tag::Read
                    | Tag::Prim
                    | Tag::Intrinsic
                    | Tag::Coerce
                    | Tag::Call
            )
        })
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct ResultSource {
    key: Ty,
    parameter: LocalId,
    projection: Path,
}

fn analyze(env: &dyn FactEnv, body: &Body, hash: Hash128, memo: &mut Memo) -> Option<Summary> {
    if !candidate(body)
        || hd_tir::own::params(body)
            .iter()
            .any(|&local| !env.search_input_copies(body.local_ty[local as usize]))
    {
        return None;
    }
    let mut sources = Sources::new(body);
    let mut queries = vec![None; body.len()];
    let mut premise = None;
    for (i, query) in queries.iter_mut().enumerate() {
        if body.tags[i] != Tag::Call {
            continue;
        }
        *query = call_source(env, body, i, &mut sources, memo);
        premise = premise.or(*query);
    }
    let premise = premise?;
    let (value, reachable) = hd_mono::defaults::returned_given(body, &|i| {
        (queries[i] == Some(premise)).then_some(hd_mono::defaults::Value::Variant(0))
    })?;
    if value != hd_mono::defaults::Value::Variant(0) {
        return None;
    }
    let topo = Topology::new(body);
    let mut additions = vec![false; body.len()];
    let mut indices = vec![None; body.len()];
    let mut positives = vec![None; body.len()];
    for i in 0..body.len() {
        if reachable[i] && body.tags[i] == Tag::Loop {
            positives[i] = Some(finite_loop(
                body,
                &topo,
                i,
                &mut sources,
                &mut memo.paths,
                &mut additions,
                &mut indices,
            )?);
        }
    }
    let mut digest = StableHasher::new("nullable-collection-search-v1");
    digest.hash(hash);
    let mut checked_places = HashSet::default();
    for i in 0..body.len() {
        if !reachable[i] {
            continue;
        }
        let a = body.data[i][0];
        match body.tags[i] {
            Tag::Block
            | Tag::If
            | Tag::Match
            | Tag::SwitchTag
            | Tag::ToArm
            | Tag::Return
            | Tag::Break
            | Tag::Loop => {}
            Tag::NewVariant if body.record(body.data[i][1]).is_empty() => {}
            Tag::Continue => {
                let target = *body.label_inst.get(a as usize)? as usize;
                let positive = positives.get(target).copied().flatten()?;
                if !(topo.first[positive]..=u32::try_from(positive).ok()?)
                    .contains(&u32::try_from(i).ok()?)
                {
                    return None;
                }
            }
            Tag::Store => {
                let (local, whole) = body.stored_local(i)?;
                if !whole || sources.parameters[local.idx()] {
                    return None;
                }
            }
            Tag::Read => {
                if !checked_places.insert(a) {
                    continue;
                }
                let place = body.decode_place(a)?;
                let mut source = match place.root {
                    PlaceRoot::Local(local) if sources.parameters[local.idx()] => Some(Source {
                        root: Root::Parameter(local),
                        path: 0,
                    }),
                    PlaceRoot::Local(local) => sources
                        .stored(local)
                        .and_then(|v| sources.source(v, &mut memo.paths, &mut || {})),
                    PlaceRoot::Value(value) => sources.source(value.0, &mut memo.paths, &mut || {}),
                    _ => None,
                };
                for step in place.proj {
                    if let Proj::Elem(index) = step {
                        let expected = indices.get(index.as_inst()?.0 as usize)?.as_ref()?;
                        if source.as_ref() != Some(expected) {
                            return None;
                        }
                    }
                    source = source.map(|mut source| {
                        source.path = memo.paths.step(source.path, step);
                        source
                    });
                }
            }
            Tag::Prim
                if additions[i]
                    || matches!(
                        PrimOp::from_u32(a),
                        Some(
                            PrimOp::Eq
                                | PrimOp::Ne
                                | PrimOp::Lt
                                | PrimOp::Le
                                | PrimOp::Gt
                                | PrimOp::Ge
                                | PrimOp::And
                                | PrimOp::Or
                                | PrimOp::Not
                                | PrimOp::BitAnd
                                | PrimOp::BitOr
                                | PrimOp::BitXor
                        )
                    ) => {}
            Tag::Intrinsic if IntrinsicOp::from_u32(a) == Some(IntrinsicOp::ListLen) => {}
            Tag::Coerce if body.same_value_coercion(i) => {}
            Tag::Call => {
                if queries[i] != Some(premise) {
                    return None;
                }
                let Callee::Item { def, .. } = hd_tir::inline::plain_call(body, i)? else {
                    return None;
                };
                if env.intrinsic(def).as_deref() != Some("downcast_ref") {
                    digest.hash(memo.definitions.get(&def).copied().flatten()?.digest);
                }
            }
            _ => return None,
        }
    }
    Some(Summary {
        key: premise.key,
        parameter: hd_tir::own::params(body)
            .iter()
            .position(|&local| local == premise.parameter.raw())?,
        projection: premise.projection,
        digest: digest.finish(),
    })
}

fn call_source(
    env: &dyn FactEnv,
    body: &Body,
    call: usize,
    sources: &mut Sources<'_>,
    memo: &mut Memo,
) -> Option<ResultSource> {
    let Callee::Item { def, targs } = hd_tir::inline::plain_call(body, call)? else {
        return None;
    };
    let arguments = body.call_args(call);
    let cast = env.intrinsic(def).as_deref() == Some("downcast_ref");
    let (key, parameter, suffix) = if cast {
        (*env.pool().list_items(targs).first()?, 0, 0)
    } else {
        let summary = memo.definitions.get(&def).copied().flatten()?;
        (
            hd_mono::subst(env.pool(), env, def, targs, summary.key),
            summary.parameter,
            summary.projection,
        )
    };
    let mut source = sources.source(*arguments.get(parameter)?, &mut memo.paths, &mut || {})?;
    let Root::Parameter(parameter) = source.root else {
        return None;
    };
    if cast {
        let PathNode::Step(prefix, Proj::Elem(_)) =
            *memo.paths.nodes.get(source.path.checked_sub(1)?)?
        else {
            return None;
        };
        source.path = prefix;
    }
    Some(ResultSource {
        key,
        parameter,
        projection: memo.paths.join(source.path, suffix),
    })
}

/// The usual unsigned, zero-based, unit-stride induction under `i < len`.
/// Both initializers dominate the loop; its true suite increments before any
/// control transfer, and its false suite exits. The length and collection
/// cannot change: the enclosing read-only proof admits only local stores.
fn finite_loop(
    body: &Body,
    topo: &Topology,
    lp: usize,
    sources: &mut Sources<'_>,
    paths: &mut Paths,
    additions: &mut [bool],
    indices: &mut [Option<Source>],
) -> Option<usize> {
    let header = body.data[lp][0] as usize;
    let guards: Vec<_> = body
        .record(body.data[header][0])
        .iter()
        .copied()
        .filter(|&i| body.tags[i as usize] == Tag::If)
        .collect();
    let &[guard] = guards.as_slice() else {
        return None;
    };
    let condition = Ref(body.data[guard as usize][0]).as_inst()?.0 as usize;
    if body.tags[condition] != Tag::Prim || body.data[condition][0] != PrimOp::Lt as u32 {
        return None;
    }
    let &[cur, end] = body.record(body.data[condition][1]) else {
        return None;
    };
    let (current, bound) = (
        body.local_read(Ref(cur).as_inst()?.0 as usize)?,
        body.local_read(Ref(end).as_inst()?.0 as usize)?,
    );
    if body.ty[Ref(cur).as_inst()?.0 as usize] != Ty::prim(Prim::Usize)
        || topo.blk[Ref(cur).as_inst()?.0 as usize] != u32::try_from(header).ok()?
    {
        return None;
    }
    let &[positive, negative] = body.record(body.data[guard as usize][1]) else {
        return None;
    };
    let exits: Vec<_> = body
        .record(body.data[negative as usize][0])
        .iter()
        .copied()
        .filter(|&i| body.tags[i as usize] != Tag::Block)
        .collect();
    let &[exit] = exits.as_slice() else {
        return None;
    };
    if body.tags[exit as usize] != Tag::Break
        || body
            .label_inst
            .get(body.data[exit as usize][0] as usize)
            .copied()
            != Some(u32::try_from(lp).ok()?)
    {
        return None;
    }
    let &[first, second] = sources.sets[current.idx()].as_slice() else {
        return None;
    };
    let (init, step) = if topo.loop_of[first.0] == u32::try_from(lp).ok()? {
        (second, first)
    } else {
        (first, second)
    };
    let before_loop = |i: usize| topo.blk[i] == topo.blk[lp] && topo.pos[i] < topo.pos[lp];
    let constant = |value: u32, wanted| {
        value & Ref::CONST_BIT != 0
            && body
                .consts
                .get((value & !Ref::CONST_BIT) as usize)
                .is_some_and(|&(_, n)| n == wanted)
    };
    if !before_loop(init.0) || !constant(init.1, 0) || topo.blk[step.0] != positive {
        return None;
    }
    let add = Ref(step.1).as_inst()?.0 as usize;
    if body.tags[add] != Tag::Prim || body.data[add][0] != PrimOp::Add as u32 {
        return None;
    }
    let &[before, one] = body.record(body.data[add][1]) else {
        return None;
    };
    if body.local_read(Ref(before).as_inst()?.0 as usize) != Some(current)
        || topo.blk[Ref(before).as_inst()?.0 as usize] != positive
        || topo.pos[Ref(before).as_inst()?.0 as usize] >= topo.pos[step.0]
        || topo.blk[add] != positive
        || topo.pos[add] >= topo.pos[step.0]
        || !constant(one, 1)
    {
        return None;
    }
    let &[(set, length)] = sources.sets[bound.idx()].as_slice() else {
        return None;
    };
    if !before_loop(set) {
        return None;
    }
    let length = sources.source(length, paths, &mut || {})?;
    let Root::Value(length_inst) = length.root else {
        return None;
    };
    let length_inst = length_inst as usize;
    if length.path != 0
        || body.tags[length_inst] != Tag::Intrinsic
        || body.data[length_inst][0] != IntrinsicOp::ListLen as u32
        || !before_loop(length_inst)
    {
        return None;
    }
    let list = sources.source(
        *body.record(body.data[length_inst][1]).first()?,
        paths,
        &mut || {},
    )?;
    for &i in body.record(body.data[positive as usize][0]) {
        let i = i as usize;
        if topo.pos[i] >= topo.pos[step.0] {
            break;
        }
        match body.tags[i] {
            Tag::Read => {
                if body.local_read(i) == Some(current) {
                    indices[i] = Some(list);
                }
            }
            Tag::Prim | Tag::Block => {}
            _ => return None,
        }
    }
    additions[add] = true;
    Some(positive as usize)
}

impl<'a> Sources<'a> {
    fn new(body: &'a Body) -> Self {
        let mut out = Self {
            body,
            sets: vec![Vec::new(); body.local_ty.len()],
            parameters: vec![false; body.local_ty.len()],
            values: vec![None; body.len()],
            seen: vec![false; body.len()],
            places: HashMap::default(),
        };
        for local in hd_tir::own::params(body) {
            out.parameters[local as usize] = true;
        }
        for i in 0..body.len() {
            if body.tags[i] != Tag::Store {
                continue;
            }
            if let Some((local, _)) = body.stored_local(i) {
                let local = local.idx();
                out.sets[local].push((i, body.data[i][1]));
            }
        }
        out
    }

    fn stored(&self, local: LocalId) -> Option<u32> {
        let &[(at, value)] = self.sets[local.idx()].as_slice() else {
            return None;
        };
        self.body.stored_local(at)?.1.then_some(value)
    }

    fn source(
        &mut self,
        value: u32,
        paths: &mut Paths,
        visit: &mut impl FnMut(),
    ) -> Option<Source> {
        let mut work = vec![(value, false)];
        while let Some((value, ready)) = work.pop() {
            let i = Ref(value).as_inst()?.0 as usize;
            if *self.seen.get(i)? && !ready {
                continue;
            }
            visit();
            self.seen[i] = true;
            let [a, b] = self.body.data[i];
            let mut projection = None;
            let dependency = match self.body.tags[i] {
                Tag::Read => {
                    if let Some(&known) = self.places.get(&a) {
                        self.values[i] = known;
                        continue;
                    }
                    let place = self.body.decode_place(a)?;
                    projection = Some(place.proj);
                    match place.root {
                        PlaceRoot::Local(local)
                            if self.parameters[local.idx()]
                                && self.sets[local.idx()].is_empty() =>
                        {
                            let known = Source {
                                root: Root::Parameter(local),
                                path: 0,
                            };
                            let result = projection.as_ref()?.iter().fold(known, |mut s, &p| {
                                s.path = paths.step(s.path, p);
                                s
                            });
                            self.places.insert(a, Some(result));
                            self.values[i] = Some(result);
                            continue;
                        }
                        PlaceRoot::Local(local) => self.stored(local),
                        PlaceRoot::Value(value) => Some(value.0),
                        _ => None,
                    }
                }
                Tag::Block => Some(b),
                Tag::Coerce
                    if self.body.record(b).first() == Some(&(Coercion::Identity as u32)) =>
                {
                    Some(a)
                }
                _ => {
                    self.values[i] = Some(Source {
                        root: Root::Value(value),
                        path: 0,
                    });
                    continue;
                }
            };
            let (seen, known) = dependency
                .and_then(|d| Ref(d).as_inst())
                .map_or((true, None), |d| {
                    (self.seen[d.0 as usize], self.values[d.0 as usize])
                });
            if !seen && !ready {
                work.push((value, true));
                if let Some(dependency) = dependency {
                    work.push((dependency, false));
                }
                continue;
            }
            let result = known.map(|mut source| {
                if let Some(projection) = projection {
                    for step in projection {
                        source.path = paths.step(source.path, step);
                    }
                }
                source
            });
            if self.body.tags[i] == Tag::Read {
                self.places.insert(a, result);
            }
            self.values[i] = result;
        }
        self.values[Ref(value).as_inst()?.0 as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_tir::ir::{BodyKind, TirBuilder, TirSink as _, local_flags};
    use hd_types::Ty;

    #[test]
    fn immutable_origins_resolve_aliases_once_at_width_and_depth() {
        let mut counts = Vec::new();
        for (width, depth) in [(100, 20), (1000, 20), (20, 1000)] {
            let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
            let n = NodeIdx::from_raw(0);
            let block = b.open_block();
            let parameter = b.local(Ty::I32, Symbol::NONE, local_flags::PARAM, n);
            let mut ends = Vec::new();
            for _ in 0..width {
                let mut previous = parameter;
                for _ in 0..depth {
                    let local = b.local(Ty::I32, Symbol::NONE, 0, n);
                    let read = b.get(previous, Ty::I32, n);
                    b.set(local, read, n);
                    previous = local;
                }
                ends.push(b.get(previous, Ty::I32, n).0);
            }
            let root = b.close_block(block, None, Ty::VOID, n);
            let body = b.finish(root, &[]).expect("valid alias body");
            let mut sources = Sources::new(&body);
            let mut paths = Paths::default();
            let mut work = 0;
            for end in ends {
                assert_eq!(
                    sources.source(end, &mut paths, &mut || work += 1),
                    Some(Source {
                        root: Root::Parameter(parameter),
                        path: 0,
                    })
                );
            }
            assert!(work <= 2 * body.len(), "{width}/{depth}: {work}");
            counts.push(work);
        }
        assert!(counts[1] <= 11 * counts[0], "tenfold width: {counts:?}");
    }

    #[test]
    fn projection_composition_and_destruction_use_flat_storage() {
        let mut paths = Paths::default();
        let mut path = 0;
        for field in 0..1000 {
            let suffix = paths.step(0, Proj::Field(field));
            path = paths.join(path, suffix);
        }
        assert_eq!(
            paths.flatten(path),
            (0..1000).map(Proj::Field).collect::<Vec<_>>()
        );
    }
}
