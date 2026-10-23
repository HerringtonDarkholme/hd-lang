//! The map glue (`crates/hd_wasm_linear/DESIGN.md` §23.7, §23.18, job 10): a map
//! is a list's kind of header over a counted table that holds its entries
//! in the order they were added, a removed entry marked by hash -1, and a
//! linear-probing index of entry numbers plus one (0 empty), twice the
//! entry capacity, so a probe always meets an empty slot. It is the Wasm
//! GC backend's map (`crates/hd_wasm/src/map.rs`) in one block of memory.

use crate::op::Op as I;
use hd_base::Hash128;
use wasm_encoder::BlockType;

use crate::layout::{Leaf, Walk};
use crate::module::{Func, HEADER, HEAP, Module, Sym, Val, mem};
use crate::rt::{Helper, LIST_OWN, OptVal, own_arg, retain_call, walk_code};

/// A table's payload: the live entry count, the written entry count
/// (removed ones included), how many times growth rebuilt the entries,
/// the entry capacity (0 or a power of two), then the index, the hashes
/// and the entries.
pub const LIVE: u64 = 0;
pub const USED: u64 = 4;
pub const GROWN: u64 = 8;
pub const CAP: u64 = 12;
pub const INDEX: u32 = 16;

/// The panic of a change of a pinned map's length (`ref.elem.pinned.map`).
pub const MAP_PINNED: &str = "collection-pinned: map length changed while pinned";

/// How a map hashes and compares its key (`crates/hd_mono/DESIGN.md`
/// §13.12): an inline key through the keyed bucket hash collection chose
/// and an inline comparison, any other through the key type's
/// `key_bucket_hash` and `Eq.eq` instances.
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
    /// The key type's `key_bucket_hash` and `Eq.eq`. With the walk of
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

/// [`Helper::MapTable`]: `(cap) -> table`, empty, its index cleared.
pub fn table(m: &Module, entry: &Entry) -> Func {
    let mut f = m.func("rt:map_table", vec![Val::I32], vec![Val::I32]);
    let t = f.local(Val::I32);
    if let Some(most) =
        (u32::MAX - INDEX - HEADER - crate::module::CYCLE_WORD).checked_div(12 + entry.size)
    {
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
    for at in [LIVE, USED, GROWN] {
        store_u32(&mut f, t, at, &|f| f.i32(0));
    }
    store_u32(&mut f, t, CAP, &|f| f.get(0));
    f.get(t);
    f.u32(INDEX);
    f.op(I::I32Add);
    f.i32(0);
    f.get(0);
    f.i32(8);
    f.op(I::I32Mul);
    f.op(I::MemoryFill(HEAP));
    f.get(t);
    f
}

/// [`Helper::DropEntries`]: one count less on each live entry's counted
/// references.
pub fn drop_entries(m: &Module, entry: &Entry) -> Func {
    let mut f = m.func("rt:drop_entries", vec![Val::I32], vec![]);
    let (cap, e) = (f.local(Val::I32), f.local(Val::I32));
    let Some(walk) = entry.walk() else {
        return f;
    };
    load_u32(&mut f, 0, CAP);
    f.set(cap);
    let n = entry.leaves.len();
    each_live(&mut f, 0, cap, e, &|f| {
        entry_at(f, 0, cap, e, entry.size);
        let ls = load_entry(f, entry, 0, n);
        walk_code(f, &walk, &|f, k| f.get(ls[k as usize]), &|f| {
            f.helper(Helper::ReleaseRef);
        });
    });
    f
}

/// [`Helper::MapHash`]: `(key...) -> i32`, the keyed bucket hash, made
/// non-negative.
pub fn hash(m: &Module, key: &[Val], ops: &KeyOps) -> Func {
    let mut f = m.func("rt:map_hash", key.to_vec(), vec![Val::I32]);
    match ops {
        KeyOps::I32(h) => {
            f.get(0);
            f.op(I::I64ExtendI32U);
            f.call(Sym::Inst(*h));
        }
        KeyOps::I64(h) => {
            f.get(0);
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
            f.call(Sym::Inst(*h));
        }
    }
    f.i32(0x7FFF_FFFF);
    f.op(I::I32And);
    f
}

/// [`Helper::MapFind`]: `(table, hash, key...) -> entry or -1`. A removed
/// entry holds hash -1, which no key's hash matches, so a probe passes it.
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
    load_u32(&mut f, 0, USED);
    f.op(I::I32Eqz);
    f.op(I::If(BlockType::Empty));
    f.i32(-1);
    f.op(I::Return);
    f.op(I::End);
    load_u32(&mut f, 0, CAP);
    f.set(cap);
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
    f.i32(-1);
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

/// [`Helper::MapPut`]: `(map, hash, key..., value...)`, taking over one
/// count of the key and of the value. An equal key's entry keeps its
/// place and takes the value (`types.map.replace.in-place`,
/// `expr.map.duplicate.last`), and the key given is released; a new key
/// appends an entry, after growth when the entries are full, and panics
/// `collection-pinned` while an element reference pins the map
/// (`ref.elem.pinned.map`). The map's table is its own already.
pub fn put(m: &Module, entry: &Entry, ops: &KeyOps) -> Func {
    let nk = entry.keys;
    let n = entry.leaves.len();
    let params = [vec![Val::I32, Val::I32], entry.key_vals(), entry.val_vals()].concat();
    let mut f = m.func("rt:map_put", params, vec![]);
    let (t, cap, e, at) = (
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
    );
    let arg = |k: usize| 2 + n32(k);
    let table_of = |f: &mut Func| {
        load_u32(f, 0, 0);
        f.set(t);
        load_u32(f, t, CAP);
        f.set(cap);
    };
    table_of(&mut f);
    f.get(t);
    f.get(1);
    for k in 0..nk {
        f.get(arg(k));
    }
    f.helper(Helper::MapFind {
        entry: entry.clone(),
        ops: ops.clone(),
    });
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
        walk_code(&mut f, w, &|f, k| f.get(old[k as usize]), &|f| {
            f.helper(Helper::ReleaseRef);
        });
    }
    for k in nk..n {
        f.get(at);
        f.get(arg(k));
        f.op(store_op(entry.leaves[k], 0));
    }
    if let Some(w) = &entry.key_walk {
        walk_code(&mut f, w, &|f, k| f.get(arg(k as usize)), &|f| {
            f.helper(Helper::ReleaseRef);
        });
    }
    f.op(I::Return);
    f.op(I::End);
    // A new key changes a pinned map's length (`ref.elem.pinned.map`).
    f.get(0);
    f.helper(Helper::ListPinned);
    f.op(I::If(BlockType::Empty));
    f.helper(Helper::Panic(MAP_PINNED.into()));
    f.op(I::End);
    load_u32(&mut f, t, USED);
    f.get(cap);
    f.op(I::I32Eq);
    f.op(I::If(BlockType::Empty));
    f.get(0);
    f.helper(Helper::MapGrow {
        entry: entry.clone(),
    });
    table_of(&mut f);
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
    index_insert(&mut f, t, cap, 1, slot);
    store_u32(&mut f, t, USED, &|f| f.get(slot));
    store_u32(&mut f, t, LIVE, &|f| {
        load_u32(f, t, LIVE);
        f.i32(1);
        f.op(I::I32Add);
    });
    f
}

/// [`Helper::MapGrow`]: `(map)`, its own table full. The live entries move
/// in order into a capacity that doubles (at least 4) when at least half
/// of them are live and stays otherwise, the index is rebuilt from their
/// hashes, the rebuild is counted, and the old block is freed with no
/// release, since its entries moved.
pub fn grow(entry: &Entry, m: &Module) -> Func {
    let mut f = m.func("rt:map_grow", vec![Val::I32], vec![]);
    let (t, cap, ncap, nt, e, j, h, slot) = (
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
    );
    load_u32(&mut f, 0, 0);
    f.set(t);
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
        f.op(I::MemoryCopy {
            src_mem: HEAP,
            dst_mem: HEAP,
        });
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
    if let Some(r) = m.runtime {
        f.get(t);
        f.u32(HEADER);
        f.op(I::I32Sub);
        f.call(Sym::Inst(r.free));
    }
    store_u32(&mut f, 0, 0, &|f| f.get(nt));
    f
}

/// [`Helper::MapUnique`]: `(map)`: a shared table is copied whole, its
/// live entries take their own counts, the collections among their values
/// become copies, and the old table loses one owner.
pub fn unique(entry: &Entry, m: &Module) -> Func {
    let mut f = m.func("rt:map_unique", vec![Val::I32], vec![]);
    let (t, cap, nt, e, at) = (
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
    );
    load_u32(&mut f, 0, 0);
    f.set(t);
    f.get(t);
    f.u32(HEADER);
    f.op(I::I32Sub);
    f.op(I::I32Load(mem(HEAP, 0, 2)));
    f.i32(1);
    f.op(I::I32Ne);
    // A header with an owner word is its table's one owner; the other
    // counts are element references, which see the change in place.
    f.get(0);
    f.op(I::I32Load(mem(HEAP, LIST_OWN, 2)));
    f.op(I::I32Eqz);
    f.op(I::I32And);
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
    f.op(I::MemoryCopy {
        src_mem: HEAP,
        dst_mem: HEAP,
    });
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
                f.helper(Helper::ListCopy);
                f.op(I::I32Store(mem(HEAP, u64::from(l.offset), 2)));
            }
        });
    }
    f.get(t);
    f.helper(Helper::ReleaseKnown);
    store_u32(&mut f, 0, 0, &|f| f.get(nt));
    f.op(I::End);
    f
}

/// [`Helper::MapClone`]: `(map) -> map`: a new header over the map's
/// table, which takes one more count, then made unique, so the table is
/// copied whole and the original keeps its own (`ref.elem.copy`).
pub fn clone(m: &Module, entry: &Entry) -> Func {
    let mut f = m.func("rt:map_clone", vec![Val::I32], vec![Val::I32]);
    let (t, h) = (f.local(Val::I32), f.local(Val::I32));
    load_u32(&mut f, 0, 0);
    f.set(t);
    f.get(t);
    f.helper(Helper::RetainKnown);
    f.get(t);
    f.helper(Helper::ListHeader {
        cyclic: entry.cyclic,
    });
    f.set(h);
    f.get(h);
    f.helper(Helper::MapUnique {
        entry: entry.clone(),
    });
    f.get(h);
    f
}

/// [`Helper::MapSlotNext`]: `(map, pos, end) -> slot`: the first live
/// entry in `pos..end` among those written, or `end`.
pub fn slot_next(m: &Module) -> Func {
    let mut f = m.func("rt:map_slot_next", vec![Val::I32; 3], vec![Val::I32]);
    let (t, cap, lim) = (f.local(Val::I32), f.local(Val::I32), f.local(Val::I32));
    load_u32(&mut f, 0, 0);
    f.set(t);
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
/// header at 0, and at creation the written entry count at 4, the live
/// count at 8 and the rebuild count at 12. A changed live or rebuild count
/// is the `iterator-invalidated` panic; with neither changed, the entries
/// before the written count at creation are where they were. The entry
/// leaves with its own counts. In `opt`, the item's leaf 0 is the entry
/// number and leaf `k` the entry's leaf `k - 1`.
pub fn step(m: &Module, entry: &Entry, opt: &[OptVal], results: &[Val]) -> Func {
    // A closure's code: the environment, the first entry to look at, then
    // the context it ignores.
    let mut f = m.func(
        "rt:map_step",
        vec![Val::I32, Val::I32, Val::I32],
        results.to_vec(),
    );
    let (h, t, cap) = (f.local(Val::I32), f.local(Val::I32), f.local(Val::I32));
    let (i, end, at) = (f.local(Val::I32), f.local(Val::I32), f.local(Val::I32));
    let out: Vec<u32> = results.iter().map(|v| f.local(*v)).collect();
    load_u32(&mut f, 0, 0);
    f.set(h);
    load_u32(&mut f, h, 0);
    f.set(t);
    load_u32(&mut f, t, LIVE);
    load_u32(&mut f, 0, 8);
    f.op(I::I32Ne);
    load_u32(&mut f, t, GROWN);
    load_u32(&mut f, 0, 12);
    f.op(I::I32Ne);
    f.op(I::I32Or);
    f.op(I::If(BlockType::Empty));
    f.helper(Helper::Panic(
        "iterator-invalidated: map changed during iteration".into(),
    ));
    f.op(I::End);
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
    let mut vals = Vec::new();
    for (p, o) in opt.iter().enumerate() {
        match o {
            OptVal::Tag => {
                f.i32(1);
                f.set(out[p]);
            }
            OptVal::Elem(0) => {
                f.get(i);
                f.set(out[p]);
            }
            OptVal::Elem(k) => {
                f.get(at);
                f.op(entry.leaves[*k as usize - 1].load(0));
                f.set(out[p]);
                vals.push((*k, out[p]));
            }
            OptVal::Zero => {}
        }
    }
    if let Some(w) = entry.walk() {
        vals.sort_unstable();
        for (_, l) in &vals {
            f.get(*l);
        }
        f.helper(Helper::Retain {
            vals: entry.leaves.iter().map(|l| l.val).collect(),
            walk: w,
        });
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

/// Pushes the address of entry `e` (a local) of map header `h` (a local),
/// for the emitter.
pub fn entry_address(f: &mut Func, h: u32, e: u32, size: u32) {
    let (t, cap) = (f.local(Val::I32), f.local(Val::I32));
    load_u32(f, h, 0);
    f.set(t);
    load_u32(f, t, CAP);
    f.set(cap);
    entry_at(f, t, cap, e, size);
}

/// Pushes `hashes[e]`'s address of map header `h`, for removal.
pub fn hash_address(f: &mut Func, h: u32, e: u32) {
    let (t, cap) = (f.local(Val::I32), f.local(Val::I32));
    load_u32(f, h, 0);
    f.set(t);
    load_u32(f, t, CAP);
    f.set(cap);
    hashes(f, t, cap);
    f.get(e);
    f.i32(4);
    f.op(I::I32Mul);
    f.op(I::I32Add);
}
