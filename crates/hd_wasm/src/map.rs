//! The map glue (`crates/hd_wasm/DESIGN.md` §23.7, §23.18, job 10): a map
//! value is the address of a counted table that holds its entries
//! in the order they were added, a removed entry marked by hash -1, and a
//! linear-probing index of entry numbers plus one (0 empty), twice the
//! entry capacity, so a probe always meets an empty slot. It is the Wasm
//! GC backend's map (`crates/hd_wasm/src/map.rs`) in one block of memory.

use crate::op::Op as I;
use hd_base::Hash128;
use wasm_encoder::BlockType;

use hd_base::StageResult;
use hd_mono::{CallTarget, Target};
use hd_types::Ty;

use crate::Input;
use crate::layout::{Leaf, Pass, Walk};
use crate::module::{Func, Global, HEADER, HEAP, Import, Module, Sym, Val, mem};
use crate::rt::{Helper, LIST_OWN, own_arg, retain_call, walk_code};

/// A table's payload: the live entry count, the written entry count
/// (removed ones included), the owner word at a list buffer's
/// ([`LIST_OWN`]), the entry capacity (0 or a power of two), how many
/// times growth rebuilt the entries, a 64-bit per-map seed, then the index,
/// the hashes and the entries.
pub const LIVE: u64 = 0;
pub const USED: u64 = 4;
pub const CAP: u64 = 12;
pub const GROWN: u64 = 16;
pub const SEED: u64 = 24;
pub const INDEX: u32 = 32;

/// How a map hashes and compares its key (`crates/hd_mono/DESIGN.md`
/// §13.12): an inline key through the keyed bucket hash collection chose
/// and an inline comparison, any other through the key type's
/// `hash_key` and `Eq.eq` instances.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum KeyOps {
    /// An integer, `bool` or `char` in one `i32`, hashed as its bits
    /// zero-extended to a `u64`; equal by `i32.eq`.
    I32(Hash128),
    /// A 64-bit integer; equal by `i64.eq`.
    I64(Hash128),
    /// A string, hashed over its bytes; equal by its bytes. With the walk
    /// of the hash's parameter when the hash owns it (`layout::Pass`).
    Str(Hash128, Option<Walk>),
    /// The key type's `hash_key` and `Eq.eq`. With the walk of
    /// each of their parameters that the callee owns: the hash's key, then
    /// `eq`'s two.
    Call {
        hash: Hash128,
        eq: Hash128,
        own: [Option<Walk>; 3],
    },
}

/// A map's entry as the glue sees it: a `(K, V)` record of `size` bytes
/// whose first `keys` leaves are the key's; the counted references of
/// the key and of the value among the leaves; and the value's
/// collections, which a copy of the entry copies.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Entry {
    pub size: u32,
    pub leaves: Vec<Leaf>,
    pub keys: usize,
    pub key_walk: Option<Walk>,
    pub val_walk: Option<Walk>,
    pub colls: Vec<u32>,
    /// Whether the map's header and tables take part in cycle collection
    /// (`crate::cycle`).
    pub cyclic: bool,
}

impl Entry {
    fn vals(&self, from: usize, to: usize) -> Vec<Val> {
        self.leaves[from..to].iter().map(|l| l.val).collect()
    }

    pub fn key_vals(&self) -> Vec<Val> {
        self.vals(0, self.keys)
    }

    pub fn val_vals(&self) -> Vec<Val> {
        self.vals(self.keys, self.leaves.len())
    }

    /// The walk of the whole entry.
    pub(crate) fn walk(&self) -> Option<Walk> {
        match (&self.key_walk, &self.val_walk) {
            (Some(k), Some(v)) => Some(Walk::Seq(vec![k.clone(), v.clone()])),
            (Some(w), None) | (None, Some(w)) => Some(w.clone()),
            (None, None) => None,
        }
    }
}

impl Entry {
    /// The entry of a `Map[kt, vt]` of type `t`, for a map operation or a
    /// boundary map's codec.
    pub(crate) fn of(input: &Input<'_>, t: Ty, kt: Ty, vt: Ty) -> StageResult<Self> {
        let (pool, env, counts) = (input.pool, input.env, true);
        let (m, parts) = crate::layout::payload_mem(pool, env, &[kt, vt])?;
        let keys = parts[0].count;
        let shift = n32(keys);
        let counted = |t: Ty| {
            counts
                .then(|| crate::layout::count_walk(pool, env, t))
                .flatten()
        };
        Ok(Entry {
            size: m.size,
            leaves: m.leaves,
            keys,
            key_walk: counted(kt),
            val_walk: counted(vt).map(|w| w.remap(&|k| k + shift)),
            colls: crate::layout::coll_leaves(pool, env, vt)
                .into_iter()
                .map(|k| k + shift)
                .collect(),
            cyclic: counts && crate::cycle::object(pool, env, t),
        })
    }
}

impl KeyOps {
    /// The key operations collection selected as `target` (`hd_mono`
    /// `map_key`), for keys whose values are `key`: std's
    /// `hash_integer(u64, u64)` or `hash_string(string, u64)` by the key's
    /// primitive type, else `hash_key[K](K, u64) -> u32` and
    /// `key_equal[K](K, K) -> bool`, so each takes the key's values.
    pub(crate) fn of(input: &Input<'_>, m: &Module, target: &Target, key: &[Val]) -> Self {
        // The walk of parameter `k` of a key operation's instance when
        // the instance owns it (`layout::Pass`): the map glue that lends
        // it the key then counts a copy for it.
        let own = |t: &CallTarget, k: usize| -> Option<Walk> {
            let p = crate::emit::callee_params(input, m, t.key, t.item, t.args)[k];
            let lent = k < 64 && input.env.lends(t.item, t.args) & (1 << k) != 0;
            match crate::layout::pass(input.pool, input.env, true, p) {
                Pass::Owned(w) if !lent => Some(w),
                _ => None,
            }
        };
        match target {
            Target::MapInline(hash) => match key {
                [Val::I32] => KeyOps::I32(hash.key),
                [Val::I64] => KeyOps::I64(hash.key),
                _ => KeyOps::Str(hash.key, own(hash, 0)),
            },
            Target::MapKey { hash, eq } => KeyOps::Call {
                hash: hash.key,
                eq: eq.key,
                own: [own(hash, 0), own(eq, 0), own(eq, 1)],
            },
            // ruled out: map_key and note_boundary_value collect only these
            // two target forms before the map emitters call this accessor.
            _ => crate::emit::required(None, "collection must select map key operations"),
        }
    }
}

/// Emits a loop over the live entries of the map table in local `t`,
/// in the order they were added: `body` runs for each with a local that
/// holds the entry's address.
pub(crate) fn each_entry(f: &mut Func, t: u32, size: u32, body: &dyn Fn(&mut Func, u32)) {
    let (cap, e, at) = (f.local(Val::I32), f.local(Val::I32), f.local(Val::I32));
    load_u32(f, t, CAP);
    f.set(cap);
    each_live(f, t, cap, e, &|f| {
        entry_at(f, t, cap, e, size);
        f.set(at);
        body(f, at);
    });
}

fn n32(n: usize) -> u32 {
    u32::try_from(n).expect("map values")
}

/// Pushes the address of the hashes of the table in local `t`, whose
/// capacity is in local `cap`.
fn hashes(f: &mut Func, t: u32, cap: u32) {
    f.get(t);
    f.get(cap);
    f.i32(8);
    f.op(I::I32Mul);
    f.op(I::I32Add);
    f.u32(INDEX);
    f.op(I::I32Add);
}

/// Pushes the address of entry `e` (a local) of the table in local `t`.
fn entry_at(f: &mut Func, t: u32, cap: u32, e: u32, size: u32) {
    f.get(t);
    f.get(cap);
    f.i32(12);
    f.op(I::I32Mul);
    f.op(I::I32Add);
    f.u32(INDEX);
    f.op(I::I32Add);
    f.get(e);
    f.u32(size);
    f.op(I::I32Mul);
    f.op(I::I32Add);
}

/// Pushes `hashes[e]` of the table in local `t`.
fn hash_of(f: &mut Func, t: u32, cap: u32, e: u32) {
    hashes(f, t, cap);
    f.get(e);
    f.i32(4);
    f.op(I::I32Mul);
    f.op(I::I32Add);
    f.op(I::I32Load(mem(HEAP, 0, 2)));
}

fn load_u32(f: &mut Func, t: u32, offset: u64) {
    f.get(t);
    f.op(I::I32Load(mem(HEAP, offset, 2)));
}

fn store_u32(f: &mut Func, t: u32, offset: u64, value: &dyn Fn(&mut Func)) {
    f.get(t);
    value(f);
    f.op(I::I32Store(mem(HEAP, offset, 2)));
}

fn store_op(l: Leaf, offset: u64) -> I {
    let m = mem(
        HEAP,
        offset + u64::from(l.offset),
        u32::from(l.bytes).trailing_zeros(),
    );
    match (l.val, l.bytes) {
        (Val::I32, 1) => I::I32Store8(m),
        (Val::I32, 2) => I::I32Store16(m),
        (Val::I32, _) => I::I32Store(m),
        (Val::I64, _) => I::I64Store(m),
        (Val::F32, _) => I::F32Store(m),
        (Val::F64, _) => I::F64Store(m),
    }
}

/// The table's byte size for the capacity in local `cap`, on the stack.
fn table_bytes(f: &mut Func, cap: u32, size: u32) {
    f.get(cap);
    f.u32(12 + size);
    f.op(I::I32Mul);
    f.u32(INDEX);
    f.op(I::I32Add);
}

/// Pushes a table's descriptor: its release glue's table slot plus one,
/// or 0 when an entry holds nothing counted, with the collector's bit when
/// the map takes part in cycle collection.
fn descriptor(f: &mut Func, entry: &Entry) {
    let glue = entry.walk().map(|_| {
        Sym::Helper(Helper::DropEntries {
            entry: entry.clone(),
        })
    });
    crate::rt::push_desc(f, glue, entry.cyclic);
}

/// Emits a loop over the written entries `0..used` of the table in local
/// `t`: `body` runs for each live one with `e` its number.
fn each_live(f: &mut Func, t: u32, cap: u32, e: u32, body: &dyn Fn(&mut Func)) {
    let used = f.local(Val::I32);
    load_u32(f, t, USED);
    f.set(used);
    f.i32(0);
    f.set(e);
    f.op(I::Block(BlockType::Empty));
    f.op(I::Loop(BlockType::Empty));
    f.get(e);
    f.get(used);
    f.op(I::I32GeU);
    f.op(I::BrIf(1));
    hash_of(f, t, cap, e);
    f.i32(0);
    f.op(I::I32GeS);
    f.op(I::If(BlockType::Empty));
    body(f);
    f.op(I::End);
    f.get(e);
    f.i32(1);
    f.op(I::I32Add);
    f.set(e);
    f.op(I::Br(0));
    f.op(I::End);
    f.op(I::End);
}

/// Loads the leaves `from..to` of the entry at the address on the stack
/// into fresh locals.
fn load_entry(f: &mut Func, entry: &Entry, from: usize, to: usize) -> Vec<u32> {
    let at = f.local(Val::I32);
    f.set(at);
    let ls: Vec<u32> = entry.leaves.iter().map(|l| f.local(l.val)).collect();
    for (l, x) in entry.leaves[from..to].iter().zip(&ls[from..to]) {
        f.get(at);
        f.op(l.load(0));
        f.set(*x);
    }
    ls
}

/// [`Helper::MapSeed`]: bijective `SplitMix64` finalization of the run
/// seed plus an odd counter stride. Distinct counter values yield distinct
/// seeds within a run. Constant work and one counter increment per seed;
/// replay supplies the same run seed and replays the same creation order.
/// Prior art: Vigna's public-domain `SplitMix64`, <https://prng.di.unimi.it/splitmix.c>.
pub fn seed(m: &Module) -> Func {
    let mut f = m.func("rt:map_seed", vec![], vec![Val::I64]);
    let value = f.local(Val::I64);
    f.global_get(Global::HashSeedReady);
    f.op(I::I32Eqz);
    f.op(I::If(BlockType::Empty));
    f.call(Sym::Import(Import::new(
        "hd:rt",
        "hash_seed",
        &[],
        &[Val::I64],
    )));
    f.global_set(Global::HashSeed);
    f.i32(1);
    f.global_set(Global::HashSeedReady);
    f.op(I::End);
    f.global_get(Global::MapSeedCounter);
    f.op(I::I64Const(0x9e37_79b9_7f4a_7c15_u64.cast_signed()));
    f.op(I::I64Add);
    f.op(I::LocalTee(value));
    f.global_set(Global::MapSeedCounter);
    f.get(value);
    f.global_get(Global::HashSeed);
    f.op(I::I64Add);
    f.set(value);
    for (shift, factor) in [
        (30, 0xbf58_476d_1ce4_e5b9_u64),
        (27, 0x94d0_49bb_1331_11eb_u64),
    ] {
        f.get(value);
        f.get(value);
        f.op(I::I64Const(shift));
        f.op(I::I64ShrU);
        f.op(I::I64Xor);
        f.op(I::I64Const(factor.cast_signed()));
        f.op(I::I64Mul);
        f.set(value);
    }
    f.get(value);
    f.get(value);
    f.op(I::I64Const(31));
    f.op(I::I64ShrU);
    f.op(I::I64Xor);
    f
}

/// [`Helper::MapTable`]: `(cap, seed) -> table`, empty, its index cleared.
pub fn table(m: &Module, entry: &Entry) -> Func {
    let mut f = m.func("rt:map_table", vec![Val::I32, Val::I64], vec![Val::I32]);
    let t = f.local(Val::I32);
    if let Some(most) = (u32::MAX - INDEX - HEADER).checked_div(12 + entry.size) {
        f.get(0);
        f.u32(most);
        f.op(I::I32GtU);
        f.op(I::If(BlockType::Empty));
        f.helper(Helper::Panic(
            "heap-exhausted: a map does not fit the heap".into(),
        ));
        f.op(I::End);
    }
    table_bytes(&mut f, 0, entry.size);
    descriptor(&mut f, entry);
    f.helper(Helper::New);
    f.set(t);
    for at in [LIVE, USED, LIST_OWN, GROWN] {
        store_u32(&mut f, t, at, &|f| f.i32(0));
    }
    store_u32(&mut f, t, CAP, &|f| f.get(0));
    f.get(t);
    f.get(1);
    f.op(I::I64Store(mem(HEAP, SEED, 3)));
    f.get(t);
    f.u32(INDEX);
    f.op(I::I32Add);
    f.i32(0);
    f.get(0);
    f.i32(8);
    f.op(I::I32Mul);
    f.push(crate::bulk::fill(HEAP));
    f.get(t);
    f
}

/// [`Helper::DropEntries`]: one count less on each live entry's counted
/// references.
pub fn drop_entries(m: &Module, entry: &Entry) -> Func {
    let mut f = m.func("rt:drop_entries", vec![Val::I32], vec![]);
    let (cap, e) = (f.local(Val::I32), f.local(Val::I32));
    let walk = entry.walk().unwrap();
    load_u32(&mut f, 0, CAP);
    f.set(cap);
    let n = entry.leaves.len();
    each_live(&mut f, 0, cap, e, &|f| {
        entry_at(f, 0, cap, e, entry.size);
        let ls = load_entry(f, entry, 0, n);
        walk_code(
            f,
            &walk,
            &|f, k| f.get(ls[k as usize]),
            &crate::rt::release_call,
        );
    });
    f
}

/// [`Helper::MapHash`]: `(key..., seed) -> i32`, the keyed bucket hash, made
/// non-negative.
pub fn hash(m: &Module, key: &[Val], ops: &KeyOps) -> Func {
    let mut params = key.to_vec();
    params.push(Val::I64);
    let mut f = m.func("rt:map_hash", params, vec![Val::I32]);
    match ops {
        KeyOps::I32(h) => {
            f.get(0);
            f.op(I::I64ExtendI32U);
            f.get(n32(key.len()));
            f.call(Sym::Inst(*h));
        }
        KeyOps::I64(h) => {
            f.get(0);
            f.get(n32(key.len()));
            f.call(Sym::Inst(*h));
        }
        KeyOps::Str(h, own)
        | KeyOps::Call {
            hash: h,
            own: [own, ..],
            ..
        } => {
            // The map lends the key; a hash that owns it takes a count.
            own_arg(&mut f, key, own.as_ref(), &|f| {
                for k in 0..key.len() {
                    f.get(n32(k));
                }
            });
            for k in 0..key.len() {
                f.get(n32(k));
            }
            f.get(n32(key.len()));
            f.call(Sym::Inst(*h));
        }
    }
    f.i32(0x7FFF_FFFF);
    f.op(I::I32And);
    f
}

/// [`Helper::MapFind`]: `(table, hash, key...) -> probe`. A matching key's
/// probe is its nonnegative entry number; a missing key's is the bitwise
/// complement of the empty index slot. A zero-capacity table returns -1;
/// its subsequent put grows before it uses the slot. A removed entry holds
/// hash -1, which no key's hash matches, so a probe passes it.
pub fn find(m: &Module, entry: &Entry, ops: &KeyOps) -> Func {
    let params = [vec![Val::I32, Val::I32], entry.key_vals()].concat();
    let mut f = m.func("rt:map_find", params, vec![Val::I32]);
    let (cap, mask, p, slot, e) = (
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
    );
    load_u32(&mut f, 0, CAP);
    f.set(cap);
    f.get(cap);
    f.op(I::I32Eqz);
    f.op(I::If(BlockType::Empty));
    f.i32(-1);
    f.op(I::Return);
    f.op(I::End);
    f.get(cap);
    f.i32(1);
    f.op(I::I32Shl);
    f.i32(1);
    f.op(I::I32Sub);
    f.set(mask);
    f.get(1);
    f.get(mask);
    f.op(I::I32And);
    f.set(p);
    f.op(I::Loop(BlockType::Empty));
    f.get(0);
    f.get(p);
    f.i32(4);
    f.op(I::I32Mul);
    f.op(I::I32Add);
    f.op(I::I32Load(mem(HEAP, u64::from(INDEX), 2)));
    f.set(slot);
    f.get(slot);
    f.op(I::I32Eqz);
    f.op(I::If(BlockType::Empty));
    f.get(p);
    f.i32(-1);
    f.op(I::I32Xor);
    f.op(I::Return);
    f.op(I::End);
    f.get(slot);
    f.i32(1);
    f.op(I::I32Sub);
    f.set(e);
    hash_of(&mut f, 0, cap, e);
    f.get(1);
    f.op(I::I32Eq);
    f.op(I::If(BlockType::Empty));
    entry_at(&mut f, 0, cap, e, entry.size);
    let stored = load_entry(&mut f, entry, 0, entry.keys);
    match ops {
        KeyOps::I32(_) => {
            f.get(stored[0]);
            f.get(2);
            f.op(I::I32Eq);
        }
        KeyOps::I64(_) => {
            f.get(stored[0]);
            f.get(2);
            f.op(I::I64Eq);
        }
        KeyOps::Str(..) => {
            f.get(stored[0]);
            f.get(stored[1]);
            f.get(2);
            f.get(3);
            f.helper(Helper::StrEq);
        }
        KeyOps::Call {
            eq,
            own: [_, own_self, own_other],
            ..
        } => {
            // Both keys are lent; an `eq` that owns them takes counts.
            let kv: Vec<Val> = entry.leaves[..entry.keys].iter().map(|l| l.val).collect();
            own_arg(&mut f, &kv, own_self.as_ref(), &|f| {
                for s in &stored[..entry.keys] {
                    f.get(*s);
                }
            });
            own_arg(&mut f, &kv, own_other.as_ref(), &|f| {
                for k in 0..entry.keys {
                    f.get(2 + n32(k));
                }
            });
            for s in &stored[..entry.keys] {
                f.get(*s);
            }
            for k in 0..entry.keys {
                f.get(2 + n32(k));
            }
            f.call(Sym::Inst(*eq));
        }
    }
    f.op(I::If(BlockType::Empty));
    f.get(e);
    f.op(I::Return);
    f.op(I::End);
    f.op(I::End);
    f.get(p);
    f.i32(1);
    f.op(I::I32Add);
    f.get(mask);
    f.op(I::I32And);
    f.set(p);
    f.op(I::Br(0));
    f.op(I::End);
    f.i32(-1);
    f
}

/// Sets the first empty index slot from `hash & mask` of the table in
/// local `t` to the value in local `slot`.
fn index_insert(f: &mut Func, t: u32, cap: u32, h: u32, slot: u32) {
    let (mask, p) = (f.local(Val::I32), f.local(Val::I32));
    f.get(cap);
    f.i32(1);
    f.op(I::I32Shl);
    f.i32(1);
    f.op(I::I32Sub);
    f.set(mask);
    f.get(h);
    f.get(mask);
    f.op(I::I32And);
    f.set(p);
    let at = |f: &mut Func| {
        f.get(t);
        f.get(p);
        f.i32(4);
        f.op(I::I32Mul);
        f.op(I::I32Add);
    };
    f.op(I::Block(BlockType::Empty));
    f.op(I::Loop(BlockType::Empty));
    at(f);
    f.op(I::I32Load(mem(HEAP, u64::from(INDEX), 2)));
    f.op(I::I32Eqz);
    f.op(I::BrIf(1));
    f.get(p);
    f.i32(1);
    f.op(I::I32Add);
    f.get(mask);
    f.op(I::I32And);
    f.set(p);
    f.op(I::Br(0));
    f.op(I::End);
    f.op(I::End);
    at(f);
    f.get(slot);
    f.op(I::I32Store(mem(HEAP, u64::from(INDEX), 2)));
}

/// [`Helper::MapPut`]: `(map, hash, probe, key..., value...) -> map`, taking
/// over one count of the key and of the value. `probe` is a matching entry
/// number, or the bitwise complement of the empty index slot found by
/// [`Helper::MapFind`]. An equal key's entry keeps its
/// place and takes the value (`types.map.replace.in-place`,
/// `expr.map.duplicate.last`), and the key given is released; a new key
/// appends an entry, after growth when the entries are full. A live
/// element reference makes that a compile-time or a pointer-loan conflict
/// (`ref.elem.access.length`, `ref.excl.alias.conflict`). The map's table
/// is its own already.
pub fn put(m: &Module, entry: &Entry, _ops: &KeyOps) -> Func {
    let nk = entry.keys;
    let n = entry.leaves.len();
    let params = [
        vec![Val::I32, Val::I32, Val::I32],
        entry.key_vals(),
        entry.val_vals(),
    ]
    .concat();
    let mut f = m.func("rt:map_put", params, vec![Val::I32]);
    let (cap, e, at, grew) = (
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
    );
    let (t, arg) = (0, |k: usize| 3 + n32(k));
    load_u32(&mut f, t, CAP);
    f.set(cap);
    f.get(2);
    f.set(e);
    f.get(e);
    f.i32(0);
    f.op(I::I32GeS);
    f.op(I::If(BlockType::Empty));
    entry_at(&mut f, t, cap, e, entry.size);
    f.set(at);
    if let Some(w) = &entry.val_walk {
        f.get(at);
        let old = load_entry(&mut f, entry, nk, n);
        walk_code(
            &mut f,
            w,
            &|f, k| f.get(old[k as usize]),
            &crate::rt::release_call,
        );
    }
    for k in nk..n {
        f.get(at);
        f.get(arg(k));
        f.op(store_op(entry.leaves[k], 0));
    }
    if let Some(w) = &entry.key_walk {
        walk_code(
            &mut f,
            w,
            &|f, k| f.get(arg(k as usize)),
            &crate::rt::release_call,
        );
    }
    f.get(t);
    f.op(I::Return);
    f.op(I::End);
    load_u32(&mut f, t, USED);
    f.get(cap);
    f.op(I::I32Eq);
    f.op(I::If(BlockType::Empty));
    f.get(t);
    f.helper(Helper::MapGrow {
        entry: entry.clone(),
    });
    f.set(t);
    f.i32(1);
    f.set(grew);
    load_u32(&mut f, t, CAP);
    f.set(cap);
    f.op(I::End);
    load_u32(&mut f, t, USED);
    f.set(e);
    hashes(&mut f, t, cap);
    f.get(e);
    f.i32(4);
    f.op(I::I32Mul);
    f.op(I::I32Add);
    f.get(1);
    f.op(I::I32Store(mem(HEAP, 0, 2)));
    entry_at(&mut f, t, cap, e, entry.size);
    f.set(at);
    for k in 0..n {
        f.get(at);
        f.get(arg(k));
        f.op(store_op(entry.leaves[k], 0));
    }
    let slot = f.local(Val::I32);
    f.get(e);
    f.i32(1);
    f.op(I::I32Add);
    f.set(slot);
    f.get(grew);
    f.op(I::If(BlockType::Empty));
    index_insert(&mut f, t, cap, 1, slot);
    f.op(I::Else);
    f.get(t);
    f.get(2);
    f.i32(-1);
    f.op(I::I32Xor);
    f.i32(4);
    f.op(I::I32Mul);
    f.op(I::I32Add);
    f.get(slot);
    f.op(I::I32Store(mem(HEAP, u64::from(INDEX), 2)));
    f.op(I::End);
    store_u32(&mut f, t, USED, &|f| f.get(slot));
    store_u32(&mut f, t, LIVE, &|f| {
        load_u32(f, t, LIVE);
        f.i32(1);
        f.op(I::I32Add);
    });
    f.get(t);
    f
}

/// [`Helper::MapGrow`]: `(map) -> map`, its own table full. The live entries move
/// in order into a capacity that doubles (at least 4) when at least half
/// of them are live and stays otherwise, the index is rebuilt from their
/// hashes, the rebuild is counted, and the old table, its entries moved
/// and none written, loses the count the map held.
pub fn grow(entry: &Entry, m: &Module) -> Func {
    let mut f = m.func("rt:map_grow", vec![Val::I32], vec![Val::I32]);
    let t = 0;
    let (cap, ncap, nt, e, j, h, slot) = (
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
    );
    load_u32(&mut f, t, CAP);
    f.set(cap);
    f.get(cap);
    f.set(ncap);
    load_u32(&mut f, t, LIVE);
    f.i32(1);
    f.op(I::I32Shl);
    f.get(cap);
    f.op(I::I32GeU);
    f.op(I::If(BlockType::Empty));
    f.i32(4);
    f.get(cap);
    f.i32(1);
    f.op(I::I32Shl);
    f.get(cap);
    f.i32(2);
    f.op(I::I32LtU);
    f.op(I::Select);
    f.set(ncap);
    f.op(I::End);
    f.get(ncap);
    f.get(t);
    f.op(I::I64Load(mem(HEAP, SEED, 3)));
    f.helper(Helper::MapTable {
        entry: entry.clone(),
    });
    f.set(nt);
    each_live(&mut f, t, cap, e, &|f| {
        hash_of(f, t, cap, e);
        f.set(h);
        hashes(f, nt, ncap);
        f.get(j);
        f.i32(4);
        f.op(I::I32Mul);
        f.op(I::I32Add);
        f.get(h);
        f.op(I::I32Store(mem(HEAP, 0, 2)));
        entry_at(f, nt, ncap, j, entry.size);
        entry_at(f, t, cap, e, entry.size);
        f.u32(entry.size);
        f.push(crate::bulk::copy(HEAP, HEAP));
        f.get(j);
        f.i32(1);
        f.op(I::I32Add);
        f.set(slot);
        index_insert(f, nt, ncap, h, slot);
        f.get(slot);
        f.set(j);
    });
    store_u32(&mut f, nt, USED, &|f| f.get(j));
    store_u32(&mut f, nt, LIVE, &|f| f.get(j));
    store_u32(&mut f, nt, GROWN, &|f| {
        load_u32(f, t, GROWN);
        f.i32(1);
        f.op(I::I32Add);
    });
    store_u32(&mut f, t, USED, &|f| f.i32(0));
    f.get(t);
    f.helper(Helper::ReleaseKnown);
    f.get(nt);
    f
}

/// [`Helper::MapUnique`]: `(map) -> map`: a shared table is copied whole, its
/// live entries take their own counts, the collections among their values
/// become copies, and the old table loses one holder.
pub fn unique(entry: &Entry, m: &Module) -> Func {
    let name = "rt:map_unique";
    let mut f = m.func(name, vec![Val::I32], vec![Val::I32]);
    let (t, cap, nt, e, at) = (
        0,
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
    );
    f.get(t);
    f.u32(HEADER);
    f.op(I::I32Sub);
    crate::rt::unique_count(&mut f, m);
    f.i32(1);
    f.op(I::I32Ne);
    f.op(I::If(BlockType::Empty));
    load_u32(&mut f, t, CAP);
    f.set(cap);
    table_bytes(&mut f, cap, entry.size);
    descriptor(&mut f, entry);
    f.helper(Helper::New);
    f.set(nt);
    f.get(nt);
    f.get(t);
    table_bytes(&mut f, cap, entry.size);
    f.push(crate::bulk::copy(HEAP, HEAP));
    store_u32(&mut f, nt, LIST_OWN, &|f| f.i32(0));
    let walk = entry.walk().and_then(|w| w.without(&entry.colls));
    let retain = retain_call(m);
    if walk.is_some() || !entry.colls.is_empty() {
        let n = entry.leaves.len();
        each_live(&mut f, nt, cap, e, &|f| {
            entry_at(f, nt, cap, e, entry.size);
            f.set(at);
            f.get(at);
            let ls = load_entry(f, entry, 0, n);
            if let Some(w) = &walk {
                walk_code(f, w, &|f, k| f.get(ls[k as usize]), &retain);
            }
            for c in &entry.colls {
                let l = entry.leaves[*c as usize];
                f.get(at);
                f.get(ls[*c as usize]);
                f.helper(Helper::CollCopy);
                f.op(I::I32Store(mem(HEAP, u64::from(l.offset), 2)));
            }
        });
    }
    f.get(t);
    f.helper(Helper::ReleaseKnown);
    f.get(nt);
    f.set(t);
    f.op(I::End);
    f.get(t);
    f
}

/// [`Helper::MapSlotNext`]: `(map, pos, end) -> slot`: the first live
/// entry in `pos..end` among those written, or `end`.
pub fn slot_next(m: &Module) -> Func {
    let mut f = m.func("rt:map_slot_next", vec![Val::I32; 3], vec![Val::I32]);
    let (t, cap, lim) = (0, f.local(Val::I32), f.local(Val::I32));
    load_u32(&mut f, t, CAP);
    f.set(cap);
    load_u32(&mut f, t, USED);
    f.get(2);
    load_u32(&mut f, t, USED);
    f.get(2);
    f.op(I::I32LtU);
    f.op(I::Select);
    f.set(lim);
    f.op(I::Block(BlockType::Empty));
    f.op(I::Loop(BlockType::Empty));
    f.get(1);
    f.get(lim);
    f.op(I::I32GeU);
    f.op(I::If(BlockType::Empty));
    f.get(2);
    f.op(I::Return);
    f.op(I::End);
    hash_of(&mut f, t, cap, 1);
    f.i32(0);
    f.op(I::I32GeS);
    f.op(I::BrIf(1));
    f.get(1);
    f.i32(1);
    f.op(I::I32Add);
    f.set(1);
    f.op(I::Br(0));
    f.op(I::End);
    f.op(I::End);
    f.get(1);
    f
}

/// [`Helper::MapStep`]: `(env, at) -> (usize, (K, V))?`, the step closure
/// code of std's `map_steps` (`emit/iter.rs`): the first live entry from
/// entry `at` on, with its entry number. The environment holds the map's
/// table at 0, a count of it that keeps it as `iter` found it
/// (`flow.for.cursor.snapshot`), and the written entry count at 4. The
/// entry leaves with its own counts. `item` is the optional's layout: its
/// `.Some` payload's leaf 0 is the entry number and leaf `k` the entry's
/// leaf `k - 1`. No entry leaves the results `.None`'s values, set first.
pub fn step(m: &Module, entry: &Entry, item: &crate::layout::Flat) -> Func {
    // A closure's code: the environment, the first entry to look at, then
    // the context it ignores.
    let results = item.mem.vals();
    let mut f = m.func(
        "rt:map_step",
        vec![Val::I32, Val::I32, Val::I32],
        results.clone(),
    );
    let (t, cap) = (f.local(Val::I32), f.local(Val::I32));
    let (i, end, at) = (f.local(Val::I32), f.local(Val::I32), f.local(Val::I32));
    let out: Vec<u32> = results.iter().map(|v| f.local(*v)).collect();
    let elems: Vec<u32> = entry.leaves.iter().map(|l| f.local(l.val)).collect();
    let values = crate::layout::variant_steps(item, 1);
    // `.None`'s values: its niche value or tag where it is not zero, as
    // the locals start.
    for (steps, x) in crate::layout::variant_steps(item, 0).iter().zip(&out) {
        if let Some(steps) = steps {
            crate::layout::run_steps(&mut f, steps, &|_, _, _| {});
            f.set(*x);
        }
    }
    load_u32(&mut f, 0, 0);
    f.set(t);
    load_u32(&mut f, t, CAP);
    f.set(cap);
    f.get(1);
    f.set(i);
    load_u32(&mut f, 0, 4);
    f.set(end);
    f.op(I::Block(BlockType::Empty));
    f.op(I::Loop(BlockType::Empty));
    f.get(i);
    f.get(end);
    f.op(I::I32GeU);
    f.op(I::BrIf(1));
    hash_of(&mut f, t, cap, i);
    f.i32(0);
    f.op(I::I32GeS);
    f.op(I::If(BlockType::Empty));
    entry_at(&mut f, t, cap, i, entry.size);
    f.set(at);
    for (l, x) in entry.leaves.iter().zip(&elems) {
        f.get(at);
        f.op(l.load(0));
        f.set(*x);
    }
    if let Some(w) = entry.walk() {
        for x in &elems {
            f.get(*x);
        }
        f.helper(Helper::Retain {
            vals: entry.leaves.iter().map(|l| l.val).collect(),
            walk: w,
        });
    }
    // The payload's one field, the `(usize, (K, V))` tuple.
    let leaf = |f: &mut Func, _: usize, k: usize| f.get(if k == 0 { i } else { elems[k - 1] });
    // A value `.Some` leaves zero stays zero, as its local starts.
    for (steps, x) in values.iter().zip(&out) {
        if let Some(steps) = steps {
            crate::layout::run_steps(&mut f, steps, &leaf);
            f.set(*x);
        }
    }
    f.op(I::Br(2));
    f.op(I::End);
    f.get(i);
    f.i32(1);
    f.op(I::I32Add);
    f.set(i);
    f.op(I::Br(0));
    f.op(I::End);
    f.op(I::End);
    for l in &out {
        f.get(*l);
    }
    f
}

/// Pushes the address of entry `e` (a local) of the map table in local
/// `t`, for the emitter.
pub fn entry_address(f: &mut Func, t: u32, e: u32, size: u32) {
    let cap = f.local(Val::I32);
    load_u32(f, t, CAP);
    f.set(cap);
    entry_at(f, t, cap, e, size);
}

/// Pushes `hashes[e]`'s address of the map table in local `t`, for
/// removal.
pub fn hash_address(f: &mut Func, t: u32, e: u32) {
    let cap = f.local(Val::I32);
    load_u32(f, t, CAP);
    f.set(cap);
    hashes(f, t, cap);
    f.get(e);
    f.i32(4);
    f.op(I::I32Mul);
    f.op(I::I32Add);
}

#[cfg(test)]
mod seed_tests {
    use super::*;

    #[test]
    fn map_headers_keep_seeds_across_copies_growth_and_replay()
    -> Result<(), Box<dyn std::error::Error>> {
        // Reuse the assembler harness's bounded bump allocator and
        // non-freeing count stubs; this oracle inspects real map helpers'
        // header/seed behavior, without testing the allocator again.
        let mut m = Module::default();
        let entry = Entry {
            size: 0,
            leaves: vec![],
            keys: 0,
            key_walk: None,
            val_walk: None,
            colls: vec![],
            cyclic: false,
        };
        let mut make = m.func("test:map", vec![Val::I32], vec![Val::I32]);
        make.get(0);
        make.helper(Helper::MapSeed);
        make.helper(Helper::MapTable {
            entry: entry.clone(),
        });
        let constructor = Sym::Inst(Hash128(1));
        m.define(constructor.clone(), make);
        m.export("make", constructor);
        let mut get = m.func("test:seed", vec![Val::I32], vec![Val::I64]);
        get.get(0);
        get.op(I::I64Load(mem(HEAP, SEED, 3)));
        let key = Sym::Inst(Hash128(2));
        m.define(key.clone(), get);
        m.export("seed", key);
        let mut copy = m.func("test:copy", vec![Val::I32], vec![Val::I32]);
        copy.get(0);
        copy.u32(HEADER);
        copy.op(I::I32Sub);
        copy.i32(2);
        copy.op(I::I32Store(mem(HEAP, 0, 2)));
        copy.get(0);
        copy.helper(Helper::MapUnique {
            entry: entry.clone(),
        });
        let key = Sym::Inst(Hash128(3));
        m.define(key.clone(), copy);
        m.export("copy", key);
        m.export("grow", Sym::Helper(Helper::MapGrow { entry }));
        m.export("next", Sym::Helper(Helper::MapSeed));
        m.export("reset", Sym::Helper(Helper::EntryReset));
        let bytes = m.finish().map_err(|e| format!("{e:?}"))?;
        wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
            .validate_all(&bytes)?;
        let script = format!(
            r"
import assert from 'node:assert/strict';
const module = new WebAssembly.Module(new Uint8Array({bytes:?}));
function run(seed, maintain = true) {{
  let calls = 0;
  const imports = {{}};
  for (const item of WebAssembly.Module.imports(module)) {{
    (imports[item.module] ??= {{}})[item.name] = item.name === 'hash_seed'
      ? () => {{ calls++; return seed; }}
      : () => {{ throw Error('unexpected host import ' + item.name); }};
  }}
  const map = new WebAssembly.Instance(module, imports).exports;
  const first = map.make(0), second = map.make(0);
  const a = map.seed(first), b = map.seed(second);
  assert.notEqual(a, b);
  if (maintain) {{
    const copy = map.copy(first);
    assert.notEqual(copy, first);
    assert.equal(map.seed(copy), a);
    const grown = map.grow(copy);
    assert.equal(map.seed(grown), a);
    // Existing tables and the per-instance sequence survive an entry reset.
    map.reset();
  }}
  const c = map.next();
  const values = [a, b, c];
  for (let i = 0; i < 61; i++) values.push(map.next());
  assert.equal(new Set(values).size, values.length);
  assert.equal(calls, 1, 'zero is a valid run seed, not an initialization sentinel');
  return values;
}}
assert.deepEqual(run(0n), run(0n));
assert.deepEqual(run(0n), run(0n, false), 'copy/grow/reset must consume no seed');
assert.deepEqual(run(19n), run(19n));
assert.notDeepEqual(run(0n), run(19n));
console.log('ok');
"
        );
        let output = std::process::Command::new("node")
            .args(["--input-type=module", "-e", &script])
            .output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8(output.stdout)?.trim(), "ok");
        Ok(())
    }
}
