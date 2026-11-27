//! The keys of stored emitted units (#2423, `crates/hd_wasm/DESIGN.md`
//! §23.41): what a unit's emission reads, hashed, as Go's action ID
//! hashes a compile's inputs, and the reads it records, checked again on
//! reuse, as rustc's `try_mark_green` checks a work product's.
//!
//! A unit's emission is a function of [`Input`] and the link's
//! [`Facts`]. Its key holds the run (the store's fingerprint), the
//! link's program-wide facts, its instance and that instance's content;
//! the other instances it looks up by key ([`crate::module::Module::inst`])
//! are its reads. Cost per link: one content hash per instance a unit
//! keys or reads, each at most once, and one pass over the facts.

use std::sync::OnceLock;

use hd_base::{DefId, Hash128, HashMap, InstId, SliceSort as _, StableHasher};
use hd_mono::Target;
use hd_mono::layout::CanonMemo;
use hd_types::Ty;

use crate::module::Facts;
use crate::{Entry, Input, UnitStore};

/// One link's unit keys.
pub(crate) struct Keys<'i, 'a> {
    input: &'i Input<'a>,
    pub(crate) store: &'a dyn UnitStore,
    /// The run's and the link's program-wide part of every key.
    link: Hash128,
    /// Each instance's content, by id, made on first use; `None` for an
    /// instance whose body has no wire form, whose unit is not stored.
    contents: Vec<OnceLock<Option<Hash128>>>,
    /// Each instance's body, by number: the instances of one item's
    /// sub-bodies share their body, which is hashed once
    /// ([`Keys::body_hash`]), so n closures of one body hash it once, not
    /// n times.
    body_of: Vec<u32>,
    /// Each distinct body's hash, made on first use.
    bodies: Vec<OnceLock<Option<Hash128>>>,
    /// Each instance's kept loans and crossed objects (`Facts::kept`,
    /// `Facts::crossed`), hashed.
    loans: HashMap<InstId, Hash128>,
    /// The entry's roots and held item ([`Entry::root`], [`Entry::held`]).
    roots: hd_base::HashSet<DefId>,
}

impl<'i, 'a> Keys<'i, 'a> {
    /// The keys of a link whose prepared facts are `facts`.
    /// Cost: one pass over the program-wide facts and the loan facts.
    pub(crate) fn new(input: &'i Input<'a>, store: &'a dyn UnitStore, facts: &Facts) -> Self {
        // No `..`: adding a link fact must audit its effect on stored units.
        let Facts {
            track_origins,
            #[cfg(feature = "checked-report")]
            count_audit,
            #[cfg(feature = "checked-report")]
            allocation_kind_reporting,
            // ruled out: initializer order is used only by finish-time folding and helpers.
            init_insts: _,
            open_world,
            driven,
            blocking_driven,
            blocking_only,
            runtime,
            // ruled out: Module::inst records keyed reads; numeric indices belong to this link.
            inst_of: _,
            // ruled out: derived from hashed current/callee bodies and types, read via Module::inst.
            indirect_params: _,
            addressed,
            objects,
            binding_slots,
            release,
            optimized,
            // ruled out: only finish-time reachability/helpers select collector initializers.
            cycle_insts: _,
            // ruled out: only the finish-time ThreadStart helper selects runtime initializers.
            rt_insts: _,
            // ruled out: only merged-module optimization removes std initializer releases.
            std_insts: _,
            // ruled out: only Module::finish rewrites cold direct calls through the function table.
            cold_insts: _,
            inline_counts,
            threads,
            lent,
            kept,
            // ruled out: collect_loans projects these sites from the per-instance hashed kept triples.
            kept_sites: _,
            crossed,
            dyn_types,
        } = facts;
        let mut memo = CanonMemo::default();
        let env = input.env;
        let canon = |memo: &mut CanonMemo, t: Ty| {
            hd_mono::layout::canon(input.pool, &|d| env.path_hash(d), memo, t)
        };
        let mut h = StableHasher::new("emit-unit-link-v1");
        h.hash(store.fingerprint());
        #[cfg(feature = "checked-report")]
        {
            h.u8(u8::from(*count_audit));
            h.u8(u8::from(*allocation_kind_reporting));
        }
        for flag in [
            *track_origins,
            *open_world,
            *release,
            *optimized,
            *inline_counts,
            *threads,
            input.promote_args,
        ] {
            h.u8(u8::from(flag));
        }
        h.str(&format!("{:?} {:?}", runtime, input.float_ops));
        for set in [driven, blocking_driven, blocking_only] {
            sorted(&mut h, set.iter().map(|&d| env.path_hash(d)));
        }
        let mut addressed: Vec<_> = addressed.iter().collect();
        addressed.sort_stable_by_key(|(k, _)| **k);
        h.len(addressed.len());
        for (k, a) in addressed {
            h.hash(*k);
            h.str(&format!("{a:?}"));
        }
        sorted(&mut h, objects.iter().copied());
        sorted(
            &mut h,
            binding_slots
                .iter()
                .map(|(k, &at)| pair(*k, Hash128(u128::from(at)))),
        );
        sorted(
            &mut h,
            lent.iter()
                .map(|(&t, &excl)| pair(canon(&mut memo, t), Hash128(u128::from(excl)))),
        );
        sorted(
            &mut h,
            dyn_types.iter().map(|(&d, tys)| {
                let mut k = StableHasher::new("dyn-types");
                k.hash(env.path_hash(d));
                for &t in tys {
                    k.hash(canon(&mut memo, t));
                }
                k.finish()
            }),
        );
        sorted(&mut h, input.coinductive_flags.iter().copied());
        let link = h.finish();

        // Each instance's loan rows, in a stable order.
        let mut rows: HashMap<InstId, Vec<Hash128>> = HashMap::default();
        for &(id, site, step) in kept {
            let mut k = StableHasher::new("kept");
            k.u32(site);
            k.u32(step);
            rows.entry(id).or_default().push(k.finish());
        }
        for (&(id, at), crossed) in crossed {
            let mut k = StableHasher::new("crossed");
            k.u32(at);
            match crossed.root {
                None => k.u8(0),
                Some(root) => {
                    place_root(&mut k, root, |t| canon(&mut memo, t), |d| env.path_hash(d));
                }
            }
            k.len(crossed.steps.len());
            for &(step, t) in &crossed.steps {
                k.len(step);
                k.hash(canon(&mut memo, t));
            }
            rows.entry(id).or_default().push(k.finish());
        }
        let loans = rows
            .into_iter()
            .map(|(id, mut rows)| {
                rows.sort_stable();
                let mut k = StableHasher::new("loans");
                for &r in &rows {
                    k.hash(r);
                }
                (id, k.finish())
            })
            .collect();
        let roots = match &input.entry {
            Entry::Main { main, .. } | Entry::Scenario { main, .. } => {
                [*main].into_iter().collect()
            }
            Entry::Tests { case_items, .. } => case_items.clone(),
            Entry::Script { .. } | Entry::Inputs(_) | Entry::Fixture { .. } => {
                hd_base::HashSet::default()
            }
        };
        // Number the distinct bodies: one pass over the instances.
        let mut body_of = vec![u32::MAX; input.collected.table.key.len()];
        let mut seen: HashMap<usize, u32> = HashMap::default();
        for &id in input.order {
            if let Some(b) = (input.body)(id) {
                let next = u32::try_from(seen.len()).unwrap_or(u32::MAX);
                body_of[id.idx()] = *seen.entry(std::ptr::from_ref(b).addr()).or_insert(next);
            }
        }
        let bodies = (0..seen.len()).map(|_| OnceLock::new()).collect();
        Keys {
            body_of,
            bodies,
            input,
            store,
            link,
            contents: (0..input.collected.table.key.len())
                .map(|_| OnceLock::new())
                .collect(),
            loans,
            roots,
        }
    }

    /// Instance `id`'s content: the body emission reads, without its node
    /// indices; its sub-body, default mask and lends; whether it is an
    /// entry root or the held item; its call targets; its loan rows; its
    /// slot facts.
    /// Cost: one wire encoding of its body, on first use.
    pub(crate) fn content(&self, id: InstId) -> Option<Hash128> {
        *self.contents[id.idx()].get_or_init(|| {
            let input = self.input;
            let t = &input.collected.table;
            let env = input.env;
            let item = t.item[id.idx()];
            let mut h = StableHasher::new("emit-unit-content-v1");
            h.hash(self.body_hash(id)?);
            h.str(&format!("{:?}", t.sub[id.idx()]));
            h.u64(hd_mono::defaults::promoted_mask(&t.defaults[id.idx()]));
            h.u64(env.lends(item, t.args[id.idx()]));
            h.u8(u8::from(self.roots.contains(&item)));
            h.str(&format!("{:?}", input.entry.held(item)));
            let mut memo = CanonMemo::default();
            let mut canon =
                |t: Ty| hd_mono::layout::canon(input.pool, &|d| env.path_hash(d), &mut memo, t);
            let mut calls: Vec<_> = input.collected.calls[id.idx()].iter().collect();
            calls.sort_stable_by_key(|(at, _)| **at);
            h.len(calls.len());
            for (&at, target) in calls {
                h.u32(at);
                target_hash(&mut h, target, &mut canon, env);
            }
            h.hash(self.loans.get(&id).copied().unwrap_or_default());
            // The kept slots of each trait whose table layout its code
            // reads and the final digest of each table it builds
            // (`Collected::slot_facts`, #2415): a table keeps only the
            // slots some trait-value call names, so these depend on the
            // whole program's calls.
            h.hash(
                input
                    .collected
                    .slot_facts
                    .get(id.idx())
                    .copied()
                    .unwrap_or_default(),
            );
            Some(h.finish())
        })
    }

    /// The hash of instance `id`'s body, once per distinct body.
    fn body_hash(&self, id: InstId) -> Option<Hash128> {
        let at = *self.body_of.get(id.idx())? as usize;
        *self
            .bodies
            .get(at)?
            .get_or_init(|| self.store.body_hash(id))
    }

    /// The content of the instance keyed `key`; for a key the link has
    /// no instance of, the zero hash, which no content hash is.
    fn content_of(&self, facts: &Facts, key: Hash128) -> Option<Hash128> {
        facts
            .instance(key)
            .map_or(Some(Hash128::default()), |id| self.content(id))
    }

    /// The key of instance `id`'s unit, `None` when it is not stored.
    pub(crate) fn key(&self, id: InstId) -> Option<Hash128> {
        let mut h = StableHasher::new("emit-unit-v1");
        h.hash(self.link);
        h.hash(self.input.collected.table.key[id.idx()]);
        h.hash(self.content(id)?);
        Some(h.finish())
    }

    /// Reads `keys` of instance `own`'s emission, each once with its
    /// content now, sorted by key; `own` is in its key. `None` when one
    /// has no content: the unit is not stored.
    pub(crate) fn reads(
        &self,
        facts: &Facts,
        own: Hash128,
        mut keys: Vec<Hash128>,
    ) -> Option<Vec<(Hash128, Hash128)>> {
        keys.sort_stable();
        keys.dedup();
        keys.into_iter()
            .filter(|&k| k != own)
            .map(|k| Some((k, self.content_of(facts, k)?)))
            .collect()
    }

    /// Whether every read of a stored unit still finds the content it
    /// read.
    pub(crate) fn fresh(&self, facts: &Facts, reads: &[(Hash128, Hash128)]) -> bool {
        reads
            .iter()
            .all(|&(k, c)| self.content_of(facts, k) == Some(c))
    }
}

fn pair(a: Hash128, b: Hash128) -> Hash128 {
    let mut h = StableHasher::new("pair");
    h.hash(a);
    h.hash(b);
    h.finish()
}

/// `items`, sorted, into `h`.
fn sorted(h: &mut StableHasher, items: impl Iterator<Item = Hash128>) {
    let mut items: Vec<_> = items.collect();
    items.sort_stable();
    h.len(items.len());
    for k in items {
        h.hash(k);
    }
}

pub fn place_root(
    h: &mut StableHasher,
    root: hd_tir::ir::PlaceRoot,
    mut canon: impl FnMut(Ty) -> Hash128,
    path_hash: impl Fn(hd_base::DefId) -> Hash128,
) {
    use hd_tir::ir::PlaceRoot;
    match root {
        PlaceRoot::Local(l) => {
            h.u8(1);
            h.u32(l.raw());
        }
        PlaceRoot::Global(d, t) => {
            h.u8(2);
            h.hash(path_hash(d));
            h.hash(canon(t));
        }
        PlaceRoot::Value(r) => {
            h.u8(3);
            h.u32(r.0);
        }
        PlaceRoot::Frame(k) => {
            h.u8(4);
            h.u32(k);
        }
        PlaceRoot::Ptr(r) => {
            h.u8(5);
            h.u32(r.0);
        }
    }
}

fn call_target_hash(
    h: &mut StableHasher,
    ct: &hd_mono::CallTarget,
    canon: &mut impl FnMut(Ty) -> Hash128,
    env: &dyn hd_mono::ProgramEnv,
) {
    h.hash(ct.key);
    h.hash(env.path_hash(ct.item));
    h.hash(canon(ct.ret));
    match &ct.kind {
        hd_mono::TargetKind::Instance => h.u8(0),
        hd_mono::TargetKind::Intrinsic(name) => {
            h.u8(1);
            h.str(name);
        }
        hd_mono::TargetKind::Builtin { method, self_ty } => {
            h.u8(2);
            h.str(method);
            h.hash(canon(*self_ty));
        }
    }
}

/// A call target, by stable names: each type by its `canon` digest, each
/// item by its path hash. Arguments are in the callee's key.
fn target_hash(
    h: &mut StableHasher,
    target: &Target,
    canon: &mut impl FnMut(Ty) -> Hash128,
    env: &dyn hd_mono::ProgramEnv,
) {
    match target {
        Target::Call(ct) => {
            h.u8(0);
            call_target_hash(h, ct, canon, env);
        }
        Target::CrossingNumber { call, number } => {
            h.u8(1);
            call_target_hash(h, call, canon, env);
            h.u64(*number);
        }
        Target::Default {
            call,
            bracket,
            owned,
        } => {
            h.u8(2);
            call_target_hash(h, call, canon, env);
            h.u8(u8::from(*bracket) | u8::from(*owned) << 1);
        }
        Target::ConstDefault(bits) => {
            h.u8(3);
            h.u64(*bits);
        }
        Target::VTable(v) => {
            // A unit that makes a vtable is emitted in order, never
            // stored; its number keys it within the link.
            h.u8(4);
            h.len(v.0);
        }
        Target::Dyn => h.u8(5),
        Target::Closure(k) => {
            h.u8(6);
            h.hash(*k);
        }
        Target::KnownClosure(k) => {
            h.u8(7);
            h.hash(*k);
        }
        Target::MapKey { hash, eq } => {
            h.u8(8);
            call_target_hash(h, hash, canon, env);
            call_target_hash(h, eq, canon, env);
        }
        Target::MapInline(ct) => {
            h.u8(9);
            call_target_hash(h, ct, canon, env);
        }
    }
}
