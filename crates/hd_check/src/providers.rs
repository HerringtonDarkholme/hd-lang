//! Freeze call-site provider evidence after row inference, before publication.
//! Lexical scopes are walked once with an undoable key index: no search of the
//! ambient runtime context, and no walk through enclosing scopes per call.

use hd_base::{DefId, HashMap, NotImplemented, PathId, Range32, Stage, StageResult, Symbol};
use hd_resolve::ItemData;
use hd_tir::ir::{
    Body, Callee, Coercion, NONE, Providers, ReadMode, Ref, Tag, Topology, place, u32_of,
};
use hd_tir::own::{BatchInsert, insert_batch};
use hd_types::{RowId, Ty, TyData, TyList};

use crate::BodyCx;
use crate::rows::{key_is, supertrait_key_index};

#[derive(Default)]
struct Scope {
    keys: Vec<Ty>,
    children: Vec<usize>,
    calls: Vec<usize>,
}

#[derive(Clone, Copy)]
struct Candidate {
    supplier: Ty,
    order: usize,
}

type Instance = (DefId, TyList);

#[derive(Default)]
struct Active {
    exact: HashMap<Ty, Vec<Candidate>>,
    plain: HashMap<Instance, Vec<Candidate>>,
    direct: HashMap<Instance, Vec<Candidate>>,
    // Immutable predecessor-independent entailment indexes, once per whole H.
    graphs: HashMap<Ty, std::sync::Arc<hd_types::solver::SupertraitKeyIndex>>,
    ordered: std::collections::BTreeSet<(usize, u32)>,
    clock: usize,
}

fn instance(pool: hd_types::Types<'_>, key: Ty) -> Option<Instance> {
    match pool.get(key) {
        TyData::TraitValue { def, args, .. } => Some((def, args)),
        _ => None,
    }
}

impl Active {
    fn index(&mut self, cx: &BodyCx<'_>, keys: &[Ty]) {
        let pool = cx.names.pool.types();
        for &supplier in keys {
            crate::work::scan_work(1);
            if self.graphs.contains_key(&supplier) {
                continue;
            }
            let graph = supertrait_key_index(cx, pool, supplier);
            self.graphs.insert(supplier, graph);
        }
    }

    fn change(&mut self, cx: &BodyCx<'_>, keys: &[Ty], enter: bool) {
        let pool = cx.names.pool.types();
        for &supplier in keys.iter().rev() {
            crate::work::scan_work(1);
            if let Some(old) = self.exact.get(&supplier).and_then(|v| v.last()) {
                self.ordered.remove(&(old.order, supplier.0));
            }
            self.clock += 1;
            let direct = Candidate {
                supplier,
                order: self.clock,
            };
            change(&mut self.exact, supplier, direct, enter);
            if let Some(current) = self.exact.get(&supplier).and_then(|v| v.last()) {
                self.ordered.insert((current.order, supplier.0));
            }
            if let Some(key) = instance(pool, supplier) {
                change(&mut self.direct, key, direct, enter);
                if matches!(pool.get(supplier), TyData::TraitValue { bindings, .. } if bindings.is_empty())
                {
                    change(&mut self.plain, key, direct, enter);
                }
            }
        }
    }

    fn supplier(&self, cx: &BodyCx<'_>, key: Ty) -> Option<Ty> {
        let pool = cx.names.pool.types();
        let exact = self.exact.get(&key).and_then(|v| v.last()).copied();
        let Some(inst) = instance(pool, key) else {
            return exact.map(|c| c.supplier);
        };
        let unconstrained =
            matches!(pool.get(key), TyData::TraitValue { bindings, .. } if bindings.is_empty());
        let other = if unconstrained {
            &self.direct
        } else {
            &self.plain
        }
        .get(&inst)
        .and_then(|v| v.last())
        .copied();
        if let Some(direct) = exact.into_iter().chain(other).max_by_key(|c| c.order) {
            return Some(direct.supplier);
        }
        if let Some(&(_, supplier)) = self.ordered.iter().next_back() {
            let supplier = Ty(supplier);
            crate::work::scan_work(1);
            if self.graphs[&supplier]
                .get(&inst)
                .is_some_and(|instances| instances.iter().any(|&i| key_is(pool, supplier, i, key)))
            {
                return Some(supplier);
            }
        }
        let bindings: Vec<_> = pool
            .trait_value_bindings(key)
            .into_iter()
            .flatten()
            .collect();
        crate::work::scan_work(bindings.len());
        cx.global
            .with_supertrait_key_suppliers(inst, &bindings, |buckets| {
                let (mut group, mut offset) = (0, 0);
                // Borrow and lock only the candidate actually visited. Taking
                // every bucket lock would itself rescan all groups per call.
                let mut next_candidate = || loop {
                    let bucket = buckets.get(group)?;
                    let values = bucket
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if let Some(&candidate) = values.get(offset) {
                        offset += 1;
                        return Some(candidate);
                    }
                    group += 1;
                    offset = 0;
                };
                // Advance both indexes together. The nearest active match wins;
                // exhausting the exact-instance bucket also proves its maximum.
                // This avoids scanning a large bucket when the nearest match is
                // right behind one unrelated lexical binding.
                let mut nearest = self.ordered.iter().rev().skip(1);
                let mut best: Option<Candidate> = None;
                loop {
                    let &(_, supplier) = nearest.next()?;
                    let supplier = Ty(supplier);
                    crate::work::scan_work(1);
                    if self.graphs[&supplier].get(&inst).is_some_and(|instances| {
                        instances.iter().any(|&i| key_is(pool, supplier, i, key))
                    }) {
                        return Some(supplier);
                    }
                    let Some((supplier, instance)) = next_candidate() else {
                        return best.map(|c| c.supplier);
                    };
                    crate::work::scan_work(1);
                    if let Some(current) = self.exact.get(&supplier).and_then(|v| v.last())
                        && key_is(pool, supplier, instance, key)
                        && best.is_none_or(|old| current.order > old.order)
                    {
                        best = Some(*current);
                    }
                }
            })
    }
}

fn change<K: std::hash::Hash + Eq>(
    map: &mut HashMap<K, Vec<Candidate>>,
    key: K,
    value: Candidate,
    enter: bool,
) {
    if enter {
        map.entry(key).or_default().push(value);
    } else if let Some(values) = map.get_mut(&key) {
        values.pop();
    }
}

fn declared_row(cx: &BodyCx<'_>, def: DefId) -> RowId {
    let written = cx
        .lookup
        .item(def)
        .and_then(|i| i.sig())
        .map_or(RowId::EMPTY, |s| s.row);
    crate::results::row_now(cx, def, written)
}

fn item_row(cx: &BodyCx<'_>, def: DefId, targs: TyList, default: bool) -> RowId {
    let pool = cx.names.pool.types();
    let owner = if default {
        DefId::from_raw(cx.names.paths.parent(PathId::from_raw(def.raw())).raw())
    } else {
        def
    };
    let parent = cx.lookup.item(owner).and_then(|i| match i.data {
        ItemData::Method { owner, .. } => cx.lookup.item(owner).map(|i| {
            (
                owner,
                i.generics.len() + usize::from(matches!(i.data, ItemData::Trait(_))),
            )
        }),
        _ => None,
    });
    let args = pool.list_items(targs);
    let skip = parent.map_or(0, |(_, n)| n);
    pool.subst_row(declared_row(cx, def), &|p| {
        if p.owner == owner {
            args.get(skip + p.index as usize).copied()
        } else if parent.is_some_and(|(owner, _)| owner == p.owner) {
            args.get(p.index as usize).copied()
        } else {
            None
        }
    })
}

fn needed_row(cx: &BodyCx<'_>, b: &Body, i: usize) -> RowId {
    let pool = cx.names.pool.types();
    match b.tags[i] {
        Tag::Call | Tag::Await => match Callee::from_words(b.record(b.data[i][0])) {
            Some(Callee::Item { def, targs }) => item_row(cx, def, targs, false),
            Some(Callee::TraitMethod {
                trait_,
                method,
                self_ty,
                targs,
                ..
            }) => {
                let n = cx.lookup.item(trait_).map_or(0, |i| i.generics.len());
                let args = pool.list_items(targs);
                pool.subst_row(declared_row(cx, method), &|p| {
                    if p.owner == method {
                        args.get(n + p.index as usize).copied()
                    } else if p.owner == trait_ {
                        if p.index == 0 {
                            Some(self_ty)
                        } else {
                            args.get(p.index as usize - 1).copied()
                        }
                    } else {
                        None
                    }
                })
            }
            None => RowId::EMPTY,
        },
        Tag::DefaultCall => match b.record(b.data[i][0]) {
            &[def, args] => item_row(cx, DefId::from_raw(def), TyList(args), true),
            _ => RowId::EMPTY,
        },
        Tag::CallValue | Tag::AwaitValue => {
            let value = Ref(b.data[i][0]);
            let ty = value.as_inst().map_or_else(
                || b.consts[(value.0 & !Ref::CONST_BIT) as usize].0,
                |v| b.ty[v.0 as usize],
            );
            match pool.get(ty) {
                TyData::Fn { row, .. } => row,
                _ => RowId::EMPTY,
            }
        }
        _ => RowId::EMPTY,
    }
}

/// Finish checked evidence once rows are settled. Identity-only bodies add no
/// instructions, contexts or runtime calls. Existing explicit packets survive.
pub(crate) fn finalize(cx: &BodyCx<'_>, b: &mut Body) -> StageResult<()> {
    let pool = cx.names.pool.types();
    let mut scopes: Vec<Scope> = (0..b.sub_root.len()).map(|_| Scope::default()).collect();
    if scopes.is_empty() {
        return Ok(());
    }
    scopes[0].keys = pool.row_data(declared_row(cx, b.item)).keys;
    let mut with_keys = HashMap::default();
    for i in 0..b.len() {
        crate::work::scan_work(1);
        if b.tags[i] == Tag::Closure && !b.is_instantiation(i) {
            if let TyData::Fn { row, .. } = pool.get(b.ty[i]) {
                scopes[b.data[i][0] as usize].keys = pool.row_data(row).keys;
            }
        } else if b.tags[i] == Tag::With {
            let keys: Vec<_> = b
                .record(b.data[i][0])
                .chunks_exact(2)
                .map(|p| Ty(p[0]))
                .collect();
            with_keys.insert(i, keys);
        }
    }
    let may_project = scopes.iter().flat_map(|s| s.keys.iter()).chain(with_keys.values().flatten()).any(|&key| {
        match pool.get(key) {
            TyData::TraitValue { def, bindings, .. } => !bindings.is_empty() || cx.lookup.item(def).is_some_and(|i| {
                matches!(&i.data, ItemData::Trait(t) if !t.supers.is_empty() || !t.assoc.is_empty())
            }),
            _ => false,
        }
    });
    if !may_project {
        return Ok(());
    }
    let topo = Topology::new(b);
    let mut at_scope = vec![0; b.len()];
    for i in (0..b.len()).rev() {
        crate::work::scan_work(1);
        if topo.is_root[i] {
            at_scope[i] = topo.sub[i] as usize;
            continue;
        }
        let parent = if topo.owner[i] == NONE {
            topo.blk[i]
        } else {
            topo.owner[i]
        };
        if parent == NONE {
            continue;
        }
        let parent = parent as usize;
        let outer = at_scope[parent];
        if b.tags[i] == Tag::Block && b.tags[parent] == Tag::With && b.data[parent][1] == u32_of(i)
        {
            let scope = scopes.len();
            scopes.push(Scope {
                keys: with_keys.remove(&parent).unwrap_or_default(),
                ..Scope::default()
            });
            scopes[outer].children.push(scope);
            at_scope[i] = scope;
        } else {
            at_scope[i] = outer;
        }
    }
    for i in 0..b.len() {
        crate::work::scan_work(1);
        if matches!(
            b.tags[i],
            Tag::Call | Tag::Await | Tag::DefaultCall | Tag::CallValue | Tag::AwaitValue
        ) && b.call_providers(i) == Providers::None
        {
            scopes[at_scope[i]].calls.push(i);
        }
    }
    let mut active = Active::default();
    for scope in &scopes {
        active.index(cx, &scope.keys);
    }
    let mut inserts = Vec::new();
    let mut mutable = HashMap::default();
    let roots = b.sub_root.len();
    for root in 0..roots {
        let mut walk = vec![(root, true)];
        while let Some((s, enter)) = walk.pop() {
            crate::work::scan_work(1);
            active.change(cx, &scopes[s].keys, enter);
            if !enter {
                continue;
            }
            walk.push((s, false));
            walk.extend(scopes[s].children.iter().rev().map(|&c| (c, true)));
            let mut selected = HashMap::default();
            for &call in &scopes[s].calls {
                let mut pairs = Vec::new();
                for key in pool.row_data(needed_row(cx, b, call)).keys {
                    crate::work::scan_work(1);
                    let supplier = *selected
                        .entry(key)
                        .or_insert_with(|| active.supplier(cx, key).unwrap_or(key));
                    if supplier != key {
                        let value = project(
                            cx,
                            b,
                            &topo,
                            call,
                            key,
                            supplier,
                            &mut mutable,
                            &mut inserts,
                        );
                        pairs.extend([key.0, value]);
                    }
                }
                if !pairs.is_empty() {
                    let range = Range32 {
                        start: u32_of(b.extra.len()),
                        len: u32_of(pairs.len()),
                    };
                    b.extra.extend(pairs);
                    if !b.set_call_providers(call, Providers::Pairs(range)) {
                        return Err(NotImplemented::bug(
                            Stage::Body,
                            "checked call has no provider footer",
                        ));
                    }
                }
            }
        }
    }
    insert_batch(b, &topo, &inserts)
        .map_err(|e| NotImplemented::bug(Stage::Body, format!("provider evidence splice: {e}")))?;
    Ok(())
}

fn record(b: &mut Body, words: &[u32]) -> u32 {
    let at = u32_of(b.extra.len());
    b.extra.push(u32_of(words.len()));
    b.extra.extend_from_slice(words);
    at
}

fn project(
    cx: &BodyCx<'_>,
    b: &mut Body,
    topo: &Topology,
    call: usize,
    key: Ty,
    supplier: Ty,
    mutable: &mut HashMap<DefId, bool>,
    edits: &mut Vec<BatchInsert>,
) -> u32 {
    let pool = cx.names.pool.types();
    let mut permission = |key| match pool.get(key) {
        TyData::TraitValue { def, .. } => *mutable
            .entry(def)
            .or_insert_with(|| crate::access::trait_is_mutable(cx, def)),
        _ => false,
    };
    let source = pool.intern_ty(&TyData::Ref {
        inner: supplier,
        mutable: permission(supplier),
        counted: false,
    });
    let target = pool.intern_ty(&TyData::Ref {
        inner: key,
        mutable: permission(key),
        counted: true,
    });
    let base_len = b.len();
    let syn = b.syn[call];
    let mut push = |tag, data, ty| {
        crate::work::scan_work(1);
        let value = u32_of(base_len + edits.len());
        edits.push(BatchInsert {
            block: topo.blk[call],
            pos: topo.pos[call],
            tag,
            data,
            ty,
            syn,
        });
        value
    };
    let get = record(b, &[supplier.0]);
    let provider = push(Tag::ProviderGet, [get, NONE], supplier);
    let as_ref = record(b, &[Coercion::ProviderAsRef as u32, NONE]);
    let reference = push(Tag::Coerce, [provider, as_ref], source);
    let local = u32_of(b.local_ty.len());
    b.local_ty.push(source);
    b.local_name.push(Symbol::from_raw(0));
    b.local_syn.push(b.syn[call]);
    b.local_flags.push(0);
    push(Tag::Store, [place::BARE_LOCAL | local, reference], Ty::VOID);
    let place = record(b, &[place::ROOT_LOCAL, local, place::PROJ_DEREF]);
    let value = push(Tag::Read, [place, ReadMode::Use as u32], supplier);
    let widening = record(b, &[Coercion::Supertrait as u32, NONE]);
    let value = push(Tag::Coerce, [value, widening], key);
    let empty = record(b, &[]);
    let block = push(Tag::Block, [empty, value], key);
    let reference = push(Tag::RefDyn, [block, NONE], target);
    let as_provider = record(b, &[Coercion::RefAsProvider as u32, NONE]);
    push(Tag::Coerce, [reference, as_provider], key)
}
