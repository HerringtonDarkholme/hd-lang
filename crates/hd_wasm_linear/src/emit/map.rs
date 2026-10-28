//! Maps (`crates/hd_wasm_linear/DESIGN.md` §23.7, §23.18, job 10). A map value
//! is the address of a counted table (`crate::map`), so a copy counts one
//! more holder of the same table and a change makes the table the map's
//! own first (`rt:map_unique`). Keys
//! hash and compare through the operations collection chose
//! (`crates/hd_mono/DESIGN.md` §13.12).

use crate::op::Op as I;
use hd_base::StageResult;
use hd_mono::layout::{StdKind, inline_map_key, strip_newtype};
use hd_mono::{CallTarget, Target, TargetKind, subst};
use hd_tir::ir::{Callee, IntrinsicOp, NONE, PlaceRoot, Tag};
use hd_types::{Ty, TyData};
use wasm_encoder::BlockType;

use super::value::Src;
use super::{Ctl, Em, MapProbe};
use crate::layout::{coll_kind, coll_leaves, payload_mem, strip};
use crate::map::{Entry, KeyOps, LIVE, MAP_PINNED, USED, entry_address, hash_address};
use crate::module::{Global, HEAP, Import, Sym, Val, mem};
use crate::rt::{Helper, empty_buffer};
use crate::unsupported;

impl Em<'_, '_, '_> {
    /// The value type of map type `t` and a local holding the address of
    /// the value of entry `e`, a `MapEntry`'s number, in the map table in
    /// local `m`: the place `[%m [%e]]` (`ref.elem.map`), laid out as the
    /// value type is.
    pub(super) fn map_value_at(&mut self, m: u32, t: Ty, e: u32) -> StageResult<(Ty, u32)> {
        let (kt, vt, entry) = self.entry(t)?;
        let (_, parts) = payload_mem(self.pool(), self.env(), &[kt, vt])?;
        let Some(value) = parts.get(1) else {
            return unsupported("a map entry without its value");
        };
        let (ix, at) = (self.f.local(Val::I32), self.f.local(Val::I32));
        self.comp(e, 0)?;
        self.f.set(ix);
        entry_address(&mut self.f, m, ix, entry.size);
        self.f.u32(value.offset);
        self.f.op(I::I32Add);
        self.f.set(at);
        Ok((vt, at))
    }

    /// Whether `t` is a map type.
    pub(super) fn is_map(&self, t: Ty) -> bool {
        coll_kind(self.pool(), self.env(), t) == Some(StdKind::Map)
    }

    /// The key and value types of map type `t`, and its entry as the glue
    /// sees it.
    pub(super) fn entry(&self, t: Ty) -> StageResult<(Ty, Ty, Entry)> {
        if self.input.runtime.is_none() {
            return unsupported("maps without std's allocator");
        }
        let TyData::Adt { args, .. } = self.pool().get(strip(self.pool(), self.env(), t)) else {
            return unsupported("a map operation on a value that is not a map");
        };
        let [kt, vt] = self.pool().list_items(args)[..] else {
            return unsupported("a map without its key and value types");
        };
        let (m, parts) = payload_mem(self.pool(), self.env(), &[kt, vt])?;
        let keys = parts[0].count;
        let shift = u32::try_from(keys).expect("values");
        let entry = Entry {
            size: m.size,
            leaves: m.leaves,
            keys,
            key_walk: self.counted(kt),
            val_walk: self.counted(vt).map(|w| w.remap(&|k| k + shift)),
            colls: coll_leaves(self.pool(), self.env(), vt)
                .into_iter()
                .map(|k| k + shift)
                .collect(),
            cyclic: self.cyclic_object(t),
        };
        Ok((kt, vt, entry))
    }

    /// How instruction `i`'s map hashes and compares keys of type `kt`:
    /// an integer, `bool`, `char` or `string` through the keyed bucket
    /// hash collection chose and an inline comparison, any other key
    /// through the `key_bucket_hash` and `Eq.eq` instances it chose.
    fn key_ops(&self, i: u32, kt: Ty, key: &[Val]) -> StageResult<KeyOps> {
        match self.calls.get(&i) {
            Some(Target::MapInline(hash)) => {
                if !inline_map_key(self.pool(), self.env(), kt)
                    || self.vals(hash.ret)? != [Val::I32]
                {
                    return unsupported(
                        "an inline map key's bucket hash that returns other values",
                    );
                }
                let string = strip_newtype(self.pool(), self.env(), kt) == Ty::STRING;
                Ok(match (key, self.key_params(hash, 0)?.as_slice()) {
                    ([Val::I32], [Val::I64]) if !string => KeyOps::I32(hash.key),
                    ([Val::I64], [Val::I64]) => KeyOps::I64(hash.key),
                    ([Val::I32, Val::I32], [Val::I32, Val::I32]) if string => {
                        KeyOps::Str(hash.key, self.key_own(hash, 0)?)
                    }
                    _ => return unsupported("an inline map key of another layout"),
                })
            }
            Some(Target::MapKey { hash, eq }) => {
                let params = self.key_params(hash, 0)?;
                if params != key
                    || self.key_params(eq, 0)? != params
                    || self.key_params(eq, 1)? != params
                    || self.vals(hash.ret)? != [Val::I32]
                    || self.vals(eq.ret)? != [Val::I32]
                {
                    return unsupported("a map key whose bucket hash or `eq` takes other values");
                }
                Ok(KeyOps::Call {
                    hash: hash.key,
                    eq: eq.key,
                    own: [
                        self.key_own(hash, 0)?,
                        self.key_own(eq, 0)?,
                        self.key_own(eq, 1)?,
                    ],
                })
            }
            _ => unsupported("a map key whose operations collection did not select"),
        }
    }

    /// The walk of parameter `k` of a key operation's instance when the
    /// instance owns it (`layout::Pass`): the map glue that lends it the
    /// key then counts a copy for it.
    fn key_own(&self, t: &CallTarget, k: usize) -> StageResult<Option<crate::layout::Walk>> {
        let ps = super::callee_params(self.input, self.m, t.key, t.item, t.args);
        let Some(&p) = ps.get(k) else {
            return unsupported("a map key operation without its parameter");
        };
        Ok(match self.pass_at(self.env().lends(t.item), k, p) {
            crate::layout::Pass::Owned(w) => Some(w),
            _ => None,
        })
    }

    /// The values parameter `k` of a key operation's instance takes.
    fn key_params(&self, t: &CallTarget, k: usize) -> StageResult<Vec<Val>> {
        if t.kind != TargetKind::Instance
            || self.env().suspends(t.item)
            || !self.env().row_keys(t.item, t.args).is_empty()
        {
            return unsupported("a map key whose `Eq` or `Hash` is not a plain function");
        }
        let ps = self.env().params(t.item).unwrap_or_default();
        let Some(&p) = ps.get(k) else {
            return unsupported("a map key operation without its parameter");
        };
        self.vals(subst(self.pool(), self.env(), t.item, t.args, p))
    }

    /// Key `r` in fresh locals.
    fn map_key_locals(&mut self, r: u32, e: &Entry) -> StageResult<Vec<u32>> {
        let key = e.key_vals();
        let kl: Vec<u32> = key.iter().map(|v| self.f.local(*v)).collect();
        for (k, l) in kl.iter().enumerate() {
            self.comp(r, k)?;
            self.f.set(*l);
        }
        Ok(kl)
    }

    /// Key `r` in fresh locals, and its bucket hash in one.
    fn map_key(&mut self, r: u32, e: &Entry, ops: &KeyOps) -> StageResult<(Vec<u32>, u32)> {
        let key = e.key_vals();
        let kl = self.map_key_locals(r, e)?;
        for l in &kl {
            self.f.get(*l);
        }
        self.f.helper(Helper::MapHash {
            key,
            ops: ops.clone(),
        });
        let h = self.f.local(Val::I32);
        self.f.set(h);
        Ok((kl, h))
    }

    /// Probes map table `m` for key `kl` with hash `h`, returning the
    /// matching entry or the complemented empty index slot.
    fn map_find(&mut self, m: u32, h: u32, kl: &[u32], e: &Entry, ops: &KeyOps) -> u32 {
        self.f.get(m);
        self.f.get(h);
        for l in kl {
            self.f.get(*l);
        }
        self.f.helper(Helper::MapFind {
            entry: e.clone(),
            ops: ops.clone(),
        });
        let probe = self.f.local(Val::I32);
        self.f.set(probe);
        probe
    }

    /// Puts the key in locals `kl` (hash `h`) with value `v` into the map
    /// table in local `m`, which then holds the table, moved when it grew:
    /// the table takes a count of each, a collection's through its copy
    /// glue (`types.coll.value`).
    fn map_put(
        &mut self,
        m: u32,
        (kt, vt): (Ty, Ty),
        (e, ops): (&Entry, &KeyOps),
        (kl, h, probe): (&[u32], u32, u32),
        v: u32,
    ) -> StageResult<()> {
        self.retain_locals(kl, kt)?;
        let s = self.retain_src(&Src::Ref(v), vt)?;
        self.f.get(m);
        self.f.get(h);
        self.f.get(probe);
        for l in kl {
            self.f.get(*l);
        }
        for k in 0..e.leaves.len() - e.keys {
            self.src(&s, k)?;
        }
        self.f.helper(Helper::MapPut {
            entry: e.clone(),
            ops: ops.clone(),
        });
        self.f.set(m);
        Ok(())
    }

    /// `NewMap`: a table with room for the entries, each put in source
    /// order (`expr.map.order`), so a later equal key's value replaces the
    /// earlier one in its place (`expr.map.duplicate.last`). An empty
    /// literal shares the static empty table.
    pub(super) fn new_map(&mut self, i: u32, items: u32) -> StageResult<()> {
        let items = self.rec(items);
        let (kt, vt, e) = self.entry(self.ty(i))?;
        if items.is_empty() {
            let at = empty_buffer(self.m);
            self.f.static_addr(at);
            return self.store_owned(i);
        }
        let ops = self.key_ops(i, kt, &e.key_vals())?;
        let Ok(cap) = u32::try_from((items.len() / 2).next_power_of_two().max(4)) else {
            return unsupported("a map literal too long for one table");
        };
        let m = self.f.local(Val::I32);
        self.f.u32(cap);
        self.f.helper(Helper::MapTable { entry: e.clone() });
        self.f.set(m);
        for pair in items.chunks(2) {
            let (kl, h) = self.map_key(pair[0], &e, &ops)?;
            let probe = self.map_find(m, h, &kl, &e, &ops);
            self.map_put(m, (kt, vt), (&e, &ops), (&kl, h, probe), pair[1])?;
        }
        self.f.get(m);
        self.store_owned(i)
    }

    /// Pushes field `at` of the table of the map value `r`.
    fn table_field(&mut self, r: u32, at: u64) -> StageResult<()> {
        self.comp(r, 0)?;
        self.f.op(I::I32Load(mem(HEAP, at, 2)));
        Ok(())
    }

    /// `MapLen`: the live entries.
    pub(super) fn map_len(&mut self, i: u32, map: u32) -> StageResult<()> {
        self.table_field(map, LIVE)?;
        self.store(i)
    }

    /// `MapSlots`: the entries written, removed ones included.
    pub(super) fn map_slots(&mut self, i: u32, map: u32) -> StageResult<()> {
        self.table_field(map, USED)?;
        self.store(i)
    }

    /// `MapSlotNext`: the first live entry in `pos..end`, or `end`.
    pub(super) fn map_slot_next(&mut self, i: u32, args: &[u32]) -> StageResult<()> {
        for r in &args[..3] {
            self.comp(*r, 0)?;
        }
        self.f.helper(Helper::MapSlotNext);
        self.store(i)
    }

    /// `MapSlotEntry`: the `(K, V)` of a live entry, which lays out as the
    /// entry does.
    pub(super) fn map_slot_entry(&mut self, i: u32, map: u32, slot: u32) -> StageResult<()> {
        let (_, _, e) = self.entry(self.ty_of(map))?;
        if self.vals(self.ty(i))? != e.leaves.iter().map(|l| l.val).collect::<Vec<_>>() {
            return unsupported("a map entry that lays out unlike its table's");
        }
        let m = self.buffer(map)?;
        let s = self.f.local(Val::I32);
        self.comp(slot, 0)?;
        self.f.set(s);
        let at = self.f.local(Val::I32);
        entry_address(&mut self.f, m, s, e.size);
        self.f.set(at);
        for l in &e.leaves {
            self.f.get(at);
            self.f.op(l.load(0));
        }
        self.store(i)
    }

    /// Whether two TIR values are the same map or key value for this
    /// optimization: the same SSA value, or two reads of the same place.
    /// The latter is how the checker represents repeated reads of a
    /// parameter such as `counts` or `word`.
    fn same_map_value(&self, a: u32, b: u32) -> bool {
        if a == b {
            return true;
        }
        let (Some(&Tag::Read), Some(&Tag::Read)) =
            (self.b.tags.get(a as usize), self.b.tags.get(b as usize))
        else {
            return false;
        };
        self.b.decode_place(self.b.data[a as usize][0])
            == self.b.decode_place(self.b.data[b as usize][0])
    }

    /// Whether instruction `i` cannot invalidate a map probe. A plain
    /// uniqueness copy is safe because map entry numbers survive it.
    /// Calls use the ownership summary; every indirect call, possibly
    /// aliasing store, mutable reference and collection change is rejected
    /// conservatively.
    fn preserves_map_probe(&self, i: u32, map: u32) -> bool {
        let k = i as usize;
        match self.b.tags[k] {
            Tag::ItemRef
            | Tag::Prim
            | Tag::Is
            | Tag::Coerce
            | Tag::NewStruct
            | Tag::CopyStruct
            | Tag::NewVariant
            | Tag::NewTuple
            | Tag::NewList
            | Tag::NewMap
            | Tag::RefIdentity
            | Tag::Read
            | Tag::Drop
            | Tag::IsUnique => true,
            Tag::Call => Callee::from_words(self.b.record(self.b.data[k][0]))
                .is_some_and(|callee| !self.input.own_cx.may_write(&callee)),
            Tag::Block => self.block_preserves_map_probe(i, map),
            Tag::Store => {
                let map_place = self
                    .b
                    .tags
                    .get(map as usize)
                    .filter(|&&tag| tag == Tag::Read)
                    .and_then(|_| self.b.decode_place(self.b.data[map as usize][0]));
                let stored = self.b.decode_place(self.b.data[k][0]);
                matches!((map_place, stored), (Some(map), Some(stored))
                    if matches!((map.root, stored.root),
                        (PlaceRoot::Local(a), PlaceRoot::Local(b)) if a != b)
                    && stored.proj.is_empty())
            }
            Tag::Match => self
                .b
                .record(self.b.data[k][1])
                .iter()
                .all(|&block| self.block_preserves_map_probe(block, map)),
            Tag::SwitchTag
            | Tag::SwitchInt
            | Tag::SwitchChar
            | Tag::SwitchStr
            | Tag::ToArm
            | Tag::Unreachable => true,
            Tag::Intrinsic => matches!(
                IntrinsicOp::from_u32(self.b.data[k][0]),
                Some(
                    IntrinsicOp::ListLen
                        | IntrinsicOp::MapLen
                        | IntrinsicOp::MapSlots
                        | IntrinsicOp::MapSlotNext
                        | IntrinsicOp::MapSlotEntry
                        | IntrinsicOp::StrIndex
                        | IntrinsicOp::StrConcat
                        | IntrinsicOp::StrEq
                )
            ),
            _ => false,
        }
    }

    fn block_preserves_map_probe(&self, block: u32, map: u32) -> bool {
        self.b.tags.get(block as usize) == Some(&Tag::Block)
            && self
                .b
                .record(self.b.data[block as usize][0])
                .iter()
                .all(|&i| self.preserves_map_probe(i, map))
    }

    /// The preceding `MapGet` or `MapEntry` whose hash and entry a
    /// `MapSet` may reuse. Both operations must be statements of one
    /// block, name equivalent reads of the map and key, and have no
    /// possibly-changing statement between them.
    fn map_probe_source(&self, i: u32, map: u32, key: u32) -> Option<u32> {
        let block = *self.topo.blk.get(i as usize)?;
        let pos = *self.topo.pos.get(i as usize)? as usize;
        if block == NONE || self.b.tags.get(block as usize) != Some(&Tag::Block) {
            return None;
        }
        let items = self.b.record(self.b.data[block as usize][0]);
        for &candidate in items.get(..pos)?.iter().rev() {
            let k = candidate as usize;
            if self.b.tags.get(k) == Some(&Tag::Intrinsic)
                && matches!(
                    IntrinsicOp::from_u32(self.b.data[k][0]),
                    Some(IntrinsicOp::MapGet | IntrinsicOp::MapEntry)
                )
            {
                let args = self.b.record(self.b.data[k][1]);
                if args.len() >= 2
                    && self.same_map_value(args[0], map)
                    && self.same_map_value(args[1], key)
                {
                    return Some(candidate);
                }
            }
            if !self.preserves_map_probe(candidate, map) {
                return None;
            }
        }
        None
    }

    /// `m.get(k)`, `m[k] = v`, `m.remove(k)` and a map value place's
    /// entry (`MapEntry`): the key is hashed, then its entry probed
    /// for. The passes' `IsUnique` made the table the map's own before a
    /// change, and a put stores the table, moved when it grew, into the
    /// map's place.
    pub(super) fn map_op(&mut self, i: u32, op: IntrinsicOp, args: &[u32]) -> StageResult<()> {
        let (kt, vt, e) = self.entry(self.ty_of(args[0]))?;
        let ops = self.key_ops(i, kt, &e.key_vals())?;
        let m = self.buffer(args[0])?;
        if op == IntrinsicOp::MapSet {
            let cached = self
                .map_probe_source(i, args[0], args[1])
                .and_then(|source| self.map_probes.get(&source).copied());
            let (kl, h, probe) = if let Some(probe) = cached {
                (self.map_key_locals(args[1], &e)?, probe.hash, probe.entry)
            } else {
                let (kl, h) = self.map_key(args[1], &e, &ops)?;
                let probe = self.map_find(m, h, &kl, &e, &ops);
                (kl, h, probe)
            };
            self.map_put(m, (kt, vt), (&e, &ops), (&kl, h, probe), args[2])?;
            return self.put_back(args[0], m);
        }
        let (kl, h) = self.map_key(args[1], &e, &ops)?;
        let ent = self.map_find(m, h, &kl, &e, &ops);
        if matches!(op, IntrinsicOp::MapGet | IntrinsicOp::MapEntry) {
            self.map_probes.insert(
                i,
                MapProbe {
                    hash: h,
                    entry: ent,
                },
            );
        }
        // The entry's address and its leaves `from..to` in fresh locals.
        let read = |em: &mut Self, from: usize, to: usize| -> (u32, Vec<u32>) {
            let at = em.f.local(Val::I32);
            entry_address(&mut em.f, m, ent, e.size);
            em.f.set(at);
            let ls = e.leaves[from..to]
                .iter()
                .map(|l| {
                    let x = em.f.local(l.val);
                    em.f.get(at);
                    em.f.op(l.load(0));
                    em.f.set(x);
                    x
                })
                .collect();
            (at, ls)
        };
        let n = e.leaves.len();
        if op == IntrinsicOp::MapEntry {
            // expr.index.map.read-value, types.coll.map-value.missing,
            // ref.elem.map.key
            self.f.get(ent);
            self.f.i32(0);
            self.f.op(I::I32LtS);
            self.f.op(I::If(BlockType::Empty));
            self.f.helper(Helper::Panic(
                "index-out-of-bounds: map key not found".into(),
            ));
            self.f.op(I::End);
            self.f.get(ent);
            return self.store(i);
        }
        // `get` and `remove` answer `V?`.
        self.f.get(ent);
        self.f.i32(0);
        self.f.op(I::I32GeS);
        self.f.op(I::If(BlockType::Empty));
        self.ctl.push(Ctl::Plain);
        let (at, vl) = read(self, e.keys, n);
        self.new_variant(i, 1, &[Src::Locals(vl.clone())])?;
        if op == IntrinsicOp::MapRemove {
            // A held key of a pinned map stays (`ref.elem.pinned.map`).
            self.f.get(m);
            self.f.helper(Helper::ListPinned);
            self.f.op(I::If(BlockType::Empty));
            self.f.helper(Helper::Panic(MAP_PINNED.into()));
            self.f.op(I::End);
            // The entry stays in place, marked removed: probes pass it and
            // iteration skips it, so the survivors keep their order. The
            // table's counts of it go: the value's after the result took
            // its own, and the key's.
            self.release_locals(&vl, vt)?;
            let kl: Vec<u32> = e.leaves[..e.keys]
                .iter()
                .map(|l| {
                    let x = self.f.local(l.val);
                    self.f.get(at);
                    self.f.op(l.load(0));
                    self.f.set(x);
                    x
                })
                .collect();
            self.release_locals(&kl, kt)?;
            hash_address(&mut self.f, m, ent);
            self.f.i32(-1);
            self.f.op(I::I32Store(mem(HEAP, 0, 2)));
            self.f.get(m);
            self.f.get(m);
            self.f.op(I::I32Load(mem(HEAP, LIVE, 2)));
            self.f.i32(1);
            self.f.op(I::I32Sub);
            self.f.op(I::I32Store(mem(HEAP, LIVE, 2)));
        }
        self.f.op(I::Else);
        self.new_variant(i, 0, &[])?;
        self.close();
        Ok(())
    }

    /// `map_hash_seed`: the run's map hash key, the runtime global filled
    /// from the host's `hash_seed` import on first use. A host that draws
    /// 0 is asked again, and answers 0 again.
    pub(super) fn hash_seed(&mut self, i: u32) -> StageResult<()> {
        let seed = self.f.local(Val::I64);
        self.f.global_get(Global::HashSeed);
        self.f.set(seed);
        self.f.get(seed);
        self.f.op(I::I64Eqz);
        self.f.op(I::If(BlockType::Empty));
        self.f.call(Sym::Import(Import::new(
            "hd:rt",
            "hash_seed",
            &[],
            &[Val::I64],
        )));
        self.f.set(seed);
        self.f.get(seed);
        self.f.global_set(Global::HashSeed);
        self.f.op(I::End);
        self.f.get(seed);
        self.store(i)
    }
}
