//! Maps (`crates/hd_wasm_linear/DESIGN.md` §23.7, §23.18, job 10). A map value
//! is the address of a counted table (`crate::map`), so a copy counts one
//! more holder of the same table and a change makes the table the map's
//! own first (`rt:map_unique`). Keys
//! hash and compare through the operations collection chose
//! (`crates/hd_mono/DESIGN.md` §13.12).

use crate::op::Op as I;
use hd_base::StageResult;
use hd_mono::layout::StdKind;
use hd_tir::ir::count::Part;
use hd_tir::ir::{Callee, IntrinsicOp, NONE, PlaceRoot, Tag};
use hd_types::Ty;
use wasm_encoder::BlockType;

use super::value::{Dst, Src};
use super::{Ctl, Em, MapProbe};
use crate::layout::{coll_kind, payload_mem, strip};
use crate::map::{Entry, KeyOps, LIVE, USED, entry_address, hash_address};
use crate::module::{Global, HEADER, HEAP, Import, Sym, Val, mem};
use crate::rt::{Helper, empty_buffer};

impl Em<'_, '_, '_> {
    /// The value type of map type `t` and a local holding the address of
    /// the value of entry `e`, a `MapEntry`'s number, in the map table in
    /// local `m`: the place `[%m [%e]]` (`ref.elem.map`), laid out as the
    /// value type is.
    pub(super) fn map_value_at(&mut self, m: u32, t: Ty, e: u32) -> StageResult<(Ty, u32)> {
        self.map_value_from(m, t, &Src::Ref(e))
    }

    /// The value of entry `e`, where `e` is either a TIR value or a local.
    fn map_value_from(&mut self, m: u32, t: Ty, e: &Src) -> StageResult<(Ty, u32)> {
        let (kt, vt, entry) = self.entry(t)?;
        let (_, parts) = payload_mem(self.pool(), self.env(), &[kt, vt])?;
        let value = &parts[1];
        let (ix, at) = (self.f.local(Val::I32), self.f.local(Val::I32));
        self.src(e, 0)?;
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
        // ruled out: the driver roots std runtime items before collecting map operations.
        // A map operation's operand is a `Map[K, V]`.
        let (_, args) = (self.pool().adt(strip(self.pool(), self.env(), t))).expect("a map");
        let (kt, vt) = (
            self.pool().list_items(args)[0],
            self.pool().list_items(args)[1],
        );
        Ok((kt, vt, Entry::of(self.input, t, kt, vt)?))
    }

    /// How instruction `i`'s map hashes and compares keys whose values
    /// are `key`: an integer, `bool`, `char` or `string` through the keyed
    /// bucket hash collection chose and an inline comparison, any other
    /// key through the `key_bucket_hash` and `Eq.eq` instances it chose.
    /// Collection selects them for every map operation (`hd_mono`
    /// `map_key`): std's `int_bucket_hash(u64)` or
    /// `string_bucket_hash(string)` by the key's primitive type, else
    /// `key_bucket_hash[K](K) -> u32` and `key_equal[K](K, K) -> bool`, so
    /// each takes the key's values.
    fn key_ops(&self, i: u32, key: &[Val]) -> KeyOps {
        let ops = self
            .calls
            .get(&i)
            .map(|t| KeyOps::of(self.input, self.m, t, key));
        // ruled out: hd_mono::map_key collects every map operation's key ops.
        super::required(ops, "collection selects a map operation's key operations")
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
        let kl = self.map_key_locals(r, e)?;
        let h = self.map_hash(&kl, e, ops);
        Ok((kl, h))
    }

    /// The bucket hash of the key in locals `kl`, in a local.
    fn map_hash(&mut self, kl: &[u32], e: &Entry, ops: &KeyOps) -> u32 {
        let key = e.key_vals();
        for l in kl {
            self.f.get(*l);
        }
        self.f.helper(Helper::MapHash {
            key,
            ops: ops.clone(),
        });
        let h = self.f.local(Val::I32);
        self.f.set(h);
        h
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

    /// Puts a key whose count the table takes as it stands, and a value
    /// whose copy or transfer follows its TIR consumer, into the table.
    fn map_put_taking_key(
        &mut self,
        m: u32,
        vt: Ty,
        (e, ops): (&Entry, &KeyOps),
        (kl, h, probe): (&[u32], u32, u32),
        v: &Src,
    ) -> StageResult<()> {
        let s = self.retain_src(v, vt)?;
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
    /// literal shares the static empty table. Each key's count moves in as
    /// a value's does: the operand is consumed, and a repeated key's put
    /// releases the count it was given.
    pub(super) fn new_map(&mut self, i: u32, items: u32) -> StageResult<()> {
        let items = self.rec(items);
        let (_, vt, e) = self.entry(self.ty(i))?;
        if items.is_empty() {
            let at = empty_buffer(self.m);
            self.f.static_addr(at);
            return self.store_owned(i);
        }
        let ops = self.key_ops(i, &e.key_vals());
        let cap = u32::try_from((items.len() / 2).next_power_of_two().max(4)).expect("entries");
        let m = self.f.local(Val::I32);
        self.f.u32(cap);
        self.f.helper(Helper::MapTable { entry: e.clone() });
        self.f.set(m);
        for pair in items.chunks(2) {
            let key = self.consumed(pair[0])?;
            let kl: Vec<u32> = e.key_vals().iter().map(|v| self.f.local(*v)).collect();
            for (k, l) in kl.iter().enumerate() {
                self.src(&key, k)?;
                self.f.set(*l);
            }
            let h = self.map_hash(&kl, &e, &ops);
            let probe = self.map_find(m, h, &kl, &e, &ops);
            self.map_put_taking_key(m, vt, (&e, &ops), (&kl, h, probe), &Src::Ref(pair[1]))?;
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

    /// Entry references lent by the loop's table. A mutable entry's map
    /// place is made unique by the ownership pass before this operation.
    /// The source reference holds its pointer loan for the whole loop;
    /// entry bindings only lend slots within that protected table.
    pub(super) fn map_slot_entry(&mut self, i: u32, map: u32, slot: u32) -> StageResult<()> {
        // The checker types the entry `(&K, &V)`, or `(&K, &mut V)` for
        // a mutable one (`map_ref_loop`).
        let (key, value, entry) = self.entry(self.ty_of(map))?;
        let (_, parts) = payload_mem(self.pool(), self.env(), &[key, value])?;
        let m = self.buffer(map)?;
        let index = self.f.local(Val::I32);
        self.comp(slot, 0)?;
        self.f.set(index);
        let at = self.f.local(Val::I32);
        entry_address(&mut self.f, m, index, entry.size);
        self.f.set(at);
        for part in parts {
            self.f.get(at);
            self.f.u32(part.offset);
            self.f.op(I::I32Add);
        }
        self.store_owned(i)
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
        // Boundary counts lower at their target, not at this metadata row.
        if hd_tir::ir::count::boundary(self.b, k) {
            return true;
        }
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
            | Tag::LoanEnd
            | Tag::Release
            | Tag::IsUnique => true,
            Tag::Call => Callee::from_words(self.b.record(self.b.data[k][0]))
                .is_some_and(|callee| !self.input.own_cx.may_write(&callee)),
            Tag::Block => self.block_preserves_map_probe(i, map),
            // An inlined body's scope, which runs no cleanup suite.
            Tag::Scope => {
                let [body, suites] = self.b.data[k];
                (suites == NONE || self.b.record(suites).is_empty())
                    && self.block_preserves_map_probe(body, map)
            }
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
            | Tag::Break
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

    /// The preceding `MapGet` or `MapEntry`
    /// whose hash and entry a `MapSet` may reuse. Both operations must be statements of
    /// one block, name equivalent reads of the map and key, and have no
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
        let ops = self.key_ops(i, &e.key_vals());
        let m = self.buffer(args[0])?;
        if op == IntrinsicOp::MapSet {
            let source = self.map_probe_source(i, args[0], args[1]);
            let cached = source.and_then(|source| self.map_probes.get(&source).copied());
            if let Some(probe) = cached
                && source.is_some_and(|source| {
                    self.b.tags[source as usize] == Tag::Intrinsic
                        && IntrinsicOp::from_u32(self.b.data[source as usize][0])
                            == Some(IntrinsicOp::MapEntry)
                })
            {
                // MapEntry already rejected a missing key, and the probe
                // proof excludes intervening changes. A uniqueness copy
                // preserves its entry number. Replace only the value:
                // map_put would retain and then discard the same key.
                let at = self.f.local(Val::I32);
                entry_address(&mut self.f, m, probe.entry, e.size);
                self.f.set(at);
                return self.replace(Dst::Mem(at, 0, &e.leaves[e.keys..]), vt, args[2]);
            }
            let (kl, h, probe) = if let Some(probe) = cached {
                (self.map_key_locals(args[1], &e)?, probe.hash, probe.entry)
            } else {
                let (kl, h) = self.map_key(args[1], &e, &ops)?;
                let probe = self.map_find(m, h, &kl, &e, &ops);
                (kl, h, probe)
            };
            if let Some(reason) = self.operation_count(i, Tag::Retain, Part::Operand(1)) {
                self.because(reason, |s| s.retain_locals(&kl, kt))?;
            }
            self.map_put_taking_key(m, vt, (&e, &ops), (&kl, h, probe), &Src::Ref(args[2]))?;
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
        let hit = [Src::Locals(vl.clone())];
        match self.operation_count(i, Tag::Retain, Part::Operand(0)) {
            Some(copy) => self.because(copy, |em| em.new_variant(i, 1, &hit))?,
            None => self.new_variant(i, 1, &hit)?,
        }
        if op == IntrinsicOp::MapRemove {
            // The entry stays in place, marked removed: probes pass it and
            // iteration skips it, so the survivors keep their order. The
            // table's counts of it go: the value's after the result took
            // its own, and the key's.
            self.release_map_part(i, Part::Operand(0), &vl, vt)?;
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
            self.release_map_part(i, Part::Operand(1), &kl, kt)?;
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

    /// A hit ends this table-owned part after its last use by the result,
    /// where its statement says so.
    fn release_map_part(&mut self, i: u32, part: Part, ls: &[u32], ty: Ty) -> StageResult<()> {
        match self.operation_count(i, Tag::Release, part) {
            Some(reason) => self.because(reason, |em| em.release_locals(ls, ty)),
            None => Ok(()),
        }
    }

    /// Makes the table in `m` unique before returning a mutable value
    /// reference. Entry numbers and a miss probe survive this copy.
    fn map_probe_unique(&mut self, i: u32, m: u32, entry: &Entry) {
        // ruled out: own::map_probe_unique states every mutable probe's action.
        let reason = self.required_operation_count(i, Tag::MakeUnique, Part::Operand(0));
        self.because(reason, |em| {
            em.f.get(m);
            em.f.u32(HEADER);
            em.f.op(I::I32Sub);
            em.f.op(I::I32Load(mem(HEAP, 0, 2)));
            em.f.i32(1);
            em.f.op(I::I32Ne);
            em.f.op(I::If(BlockType::Empty));
            em.f.get(m);
            em.f.helper(Helper::MapUnique {
                entry: entry.clone(),
            });
            em.f.set(m);
            em.f.op(I::End);
        });
    }

    /// Stores a probe's changed table through its map reference.
    fn map_probe_store(&mut self, at: u32, m: u32) {
        self.f.get(at);
        self.f.get(m);
        self.f.op(I::I32Store(mem(HEAP, 0, 2)));
    }

    /// Releases a directly consumed probe result through its known table,
    /// avoiding the generic interior-reference page lookup.
    pub(super) fn drop_map_probe_ref(&mut self, i: u32) -> bool {
        let Some(place) = self.b.decode_place(self.b.data[i as usize][0]) else {
            return false;
        };
        let PlaceRoot::Value(value) = place.root else {
            return false;
        };
        let Some(at) = value.as_inst() else {
            return false;
        };
        if !place.proj.is_empty() {
            return false;
        }
        let Some((owner, counted)) = self.map_probe_refs.remove(&at.0) else {
            return false;
        };
        if counted {
            self.f.get(owner);
            self.f.helper(Helper::ReleaseKnown);
        }
        if let Some(locals) = self.vals.get(&at.0).cloned() {
            for local in locals {
                self.f.zero(local);
            }
        }
        true
    }

    /// `map_probe(map, key, default)`: hash and find the key once, reuse
    /// a miss probe to insert `default`, and return the value reference.
    pub(super) fn map_probe(&mut self, i: u32, args: &[u32]) -> StageResult<()> {
        // `Map.get_or_insert(&mut self, key, default)`.
        let (map, key, default) = (args[0], args[1], args[2]);
        let (mt, _) = self.ref_target(self.ty_of(map)).expect("`&mut self`");
        let (kt, vt, entry) = self.entry(mt)?;
        let ops = self.key_ops(i, &entry.key_vals());
        let at = self.f.local(Val::I32);
        self.comp(map, 0)?;
        self.f.set(at);
        let value = self.ref_layout(mt)?;
        // A map is one header word.
        let m = crate::rt::ref_lend(&mut self.f, at, &value)[0];
        // A local reference takes no count of its table (`ref.count.local`):
        // the map's root lives until its last use.
        self.map_probe_refs.insert(i, (m, false));
        let (kl, hash) = self.map_key(key, &entry, &ops)?;
        let found = self.map_find(m, hash, &kl, &entry, &ops);
        self.map_probe_unique(i, m, &entry);

        self.f.get(found);
        self.f.i32(0);
        self.f.op(I::I32GeS);
        self.f.op(I::If(BlockType::Empty));
        self.ctl.push(Ctl::Plain);
        self.map_probe_store(at, m);
        let (_, value_at) = self.map_value_from(m, mt, &Src::Locals(vec![found]))?;
        self.f.get(value_at);
        self.store_owned(i)?;

        self.f.op(I::Else);
        let inserted = self.f.local(Val::I32);
        if let Some(reason) = self.operation_count(i, Tag::Retain, Part::Operand(1)) {
            self.because(reason, |s| s.retain_locals(&kl, kt))?;
        }
        let copy = self.operation_count(i, Tag::Retain, Part::Operand(2));
        let was = std::mem::replace(&mut self.f.reason, copy);
        self.map_put_taking_key(
            m,
            vt,
            (&entry, &ops),
            (&kl, hash, found),
            &Src::Ref(default),
        )?;
        self.f.reason = was;
        self.f.get(m);
        self.f.op(I::I32Load(mem(HEAP, USED, 2)));
        self.f.i32(1);
        self.f.op(I::I32Sub);
        self.f.set(inserted);
        self.map_probe_store(at, m);
        let (_, value_at) = self.map_value_from(m, mt, &Src::Locals(vec![inserted]))?;
        self.f.get(value_at);
        self.store_owned(i)?;
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
