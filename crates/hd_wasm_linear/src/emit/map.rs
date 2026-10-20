//! Maps (`crates/hd_wasm_linear/DESIGN.md` §23.7, §23.18, job 10). A map value
//! is a list's kind of header (`list.rs`) over a counted table
//! (`crate::map`), so a copy is a new header over the same table and a
//! change makes the table the map's own first (`rt:map_unique`). Keys
//! hash and compare through the operations collection chose
//! (`crates/hd_mono/DESIGN.md` §13.12).

use hd_base::StageResult;
use hd_mono::layout::{inline_map_key, strip_newtype};
use hd_mono::{CallTarget, Target, TargetKind, subst};
use hd_tir::ir::IntrinsicOp;
use hd_types::{Ty, TyData};
use wasm_encoder::{BlockType, Instruction as I};

use super::value::Src;
use super::{Ctl, Em};
use crate::layout::{coll_leaves, payload_mem, strip};
use crate::map::{Entry, KeyOps, LIVE, USED, entry_address, hash_address};
use crate::module::{Global, HEAP, Import, Sym, Val, mem};
use crate::rt::{Helper, empty_buffer};
use crate::unsupported;

impl Em<'_, '_, '_> {
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
        Ok(match self.pass(p) {
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

    /// Key `r` in fresh locals, and its bucket hash in one.
    fn map_key(&mut self, r: u32, e: &Entry, ops: &KeyOps) -> StageResult<(Vec<u32>, u32)> {
        let key = e.key_vals();
        let kl: Vec<u32> = key.iter().map(|v| self.f.local(*v)).collect();
        for (k, l) in kl.iter().enumerate() {
            self.comp(r, k)?;
            self.f.set(*l);
        }
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

    /// Puts the key in locals `kl` (hash `h`) with value `v` into the map
    /// header in local `m`: the table takes a count of each, and a value
    /// holding a collection that some place holds is stored as its copy
    /// (`types.coll.value`).
    fn map_put(
        &mut self,
        m: u32,
        (kt, vt): (Ty, Ty),
        (e, ops): (&Entry, &KeyOps),
        (kl, h): (&[u32], u32),
        v: u32,
    ) -> StageResult<()> {
        self.retain_locals(kl, kt)?;
        let s = self.slot_src(v, vt, false)?;
        self.retain_src(&s, vt)?;
        self.f.get(m);
        self.f.get(h);
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
            self.f.u32(at);
            self.f.helper(Helper::ListHeader { cyclic: e.cyclic });
            return self.store_owned(i);
        }
        let ops = self.key_ops(i, kt, &e.key_vals())?;
        let Ok(cap) = u32::try_from((items.len() / 2).next_power_of_two().max(4)) else {
            return unsupported("a map literal too long for one table");
        };
        let m = self.f.local(Val::I32);
        self.f.u32(cap);
        self.f.helper(Helper::MapTable { entry: e.clone() });
        self.f.helper(Helper::ListHeader { cyclic: e.cyclic });
        self.f.set(m);
        for pair in items.chunks(2) {
            let (kl, h) = self.map_key(pair[0], &e, &ops)?;
            self.map_put(m, (kt, vt), (&e, &ops), (&kl, h), pair[1])?;
        }
        self.f.get(m);
        self.store_owned(i)
    }

    /// Pushes field `at` of the table of the map value `r`.
    fn table_field(&mut self, r: u32, at: u64) -> StageResult<()> {
        self.comp(r, 0)?;
        self.f.op(I::I32Load(mem(HEAP, 0, 2)));
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
        let h = self.header(map)?;
        let s = self.f.local(Val::I32);
        self.comp(slot, 0)?;
        self.f.set(s);
        let at = self.f.local(Val::I32);
        entry_address(&mut self.f, h, s, e.size);
        self.f.set(at);
        for l in &e.leaves {
            self.f.get(at);
            self.f.op(l.load(0));
        }
        self.store(i)
    }

    /// `m[k]`, `m.get(k)`, `m[k] = v` and `m.remove(k)`: the key is
    /// hashed, then its entry probed for. A change makes the table the
    /// map's own first.
    pub(super) fn map_op(&mut self, i: u32, op: IntrinsicOp, args: &[u32]) -> StageResult<()> {
        let (kt, vt, e) = self.entry(self.ty_of(args[0]))?;
        let ops = self.key_ops(i, kt, &e.key_vals())?;
        let m = self.header(args[0])?;
        if matches!(
            op,
            IntrinsicOp::MapSet | IntrinsicOp::MapRemove | IntrinsicOp::MapIndexMut
        ) {
            self.f.get(m);
            self.f.helper(Helper::MapUnique { entry: e.clone() });
        }
        let (kl, h) = self.map_key(args[1], &e, &ops)?;
        if op == IntrinsicOp::MapSet {
            return self.map_put(m, (kt, vt), (&e, &ops), (&kl, h), args[2]);
        }
        let ent = self.f.local(Val::I32);
        self.f.get(m);
        self.f.op(I::I32Load(mem(HEAP, 0, 2)));
        self.f.get(h);
        for l in &kl {
            self.f.get(*l);
        }
        self.f.helper(Helper::MapFind {
            entry: e.clone(),
            ops,
        });
        self.f.set(ent);
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
        if matches!(op, IntrinsicOp::MapIndex | IntrinsicOp::MapIndexMut) {
            // expr.index.map.read-value
            self.f.get(ent);
            self.f.i32(0);
            self.f.op(I::I32LtS);
            self.f.op(I::If(BlockType::Empty));
            self.f.helper(Helper::Panic(
                "index-out-of-bounds: map key not found".into(),
            ));
            self.f.op(I::End);
            let (_, vl) = read(self, e.keys, n);
            for l in vl {
                self.f.get(l);
            }
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
            let t = self.f.local(Val::I32);
            self.f.get(m);
            self.f.op(I::I32Load(mem(HEAP, 0, 2)));
            self.f.set(t);
            self.f.get(t);
            self.f.get(t);
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
