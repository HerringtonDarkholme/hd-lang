//! The map helpers (codegen.md §13.12, representation-runtime.md §7.2):
//! entries in the order they were added, a removed entry marked by hash
//! -1, and a linear-probing index of entry numbers plus one (0 empty),
//! twice the entry capacity, so a probe always meets an empty slot.

use hd_base::StageResult;

use crate::asm::Asm;
use crate::layout::{M_HASHES, M_INDEX, M_KEYS, M_LIVE, M_USED, storage};
use crate::rt::{Helper, KeyOps, OptForm};
use crate::{Code, Sym, VT, WTy, unsupported};

fn n32(i: usize) -> u32 {
    u32::try_from(i).expect("map fields")
}

fn ints() -> WTy {
    WTy::Array(VT::I32)
}

/// The array that holds a stored component of type `v`.
fn comp_arr(v: &VT) -> WTy {
    WTy::Array(storage(v).dflt())
}

/// `p = h & mask`, then the first empty slot of `idx` from `p`, set to the
/// value `slot` pushes.
fn index_insert(a: &mut Asm, idx: u32, mask: u32, h: u32, p: u32, slot: impl Fn(&mut Asm)) {
    a.get(h);
    a.get(mask);
    a.s().i32_and();
    a.set(p);
    a.block();
    a.loop_();
    a.get(idx);
    a.get(p);
    a.array_get(&ints());
    a.s().i32_eqz();
    a.br_if(1);
    a.get(p);
    a.i32(1);
    a.s().i32_add();
    a.get(mask);
    a.s().i32_and();
    a.set(p);
    a.br(0);
    a.end();
    a.end();
    a.get(idx);
    a.get(p);
    slot(a);
    a.array_set(&ints());
}

/// `local = len(field) - 1`: an index's probe mask.
fn mask_of(a: &mut Asm, idx: u32, mask: u32) {
    a.get(idx);
    a.array_len();
    a.i32(1);
    a.s().i32_sub();
    a.set(mask);
}

/// `(key...) -> i32`: the bucket hash, the key's 32 bits mixed into a
/// non-negative `i32`. Integers and strings hash inline; any other key
/// through its `hash_of` instance.
pub fn hash_code(key: &[VT], ops: &KeyOps) -> StageResult<Code> {
    let mut a = Asm::new(key.to_vec());
    let hv = a.local(VT::I32);
    match ops {
        KeyOps::I32 => a.get(0),
        KeyOps::I64 => {
            a.get(0);
            a.get(0);
            a.i64(32);
            a.s().i64_shr_u().i64_xor().i32_wrap_i64();
        }
        KeyOps::Str => {
            // FNV-1a over the viewed bytes.
            let (start, len, k) = (a.local(VT::I32), a.local(VT::I32), a.local(VT::I32));
            a.get(1);
            a.s().i32_wrap_i64();
            a.set(start);
            a.get(1);
            a.i64(32);
            a.s().i64_shr_u().i32_wrap_i64();
            a.set(len);
            a.i32(0x811C_9DC5_u32.cast_signed());
            a.set(hv);
            a.block();
            a.loop_();
            a.get(k);
            a.get(len);
            a.s().i32_ge_u();
            a.br_if(1);
            a.get(hv);
            a.get(0);
            a.get(start);
            a.get(k);
            a.s().i32_add();
            a.array_get(&WTy::Bytes);
            a.s().i32_xor();
            a.i32(0x0100_0193);
            a.s().i32_mul();
            a.set(hv);
            a.get(k);
            a.i32(1);
            a.s().i32_add();
            a.set(k);
            a.br(0);
            a.end();
            a.end();
            a.get(hv);
        }
        KeyOps::Call { hash, params, .. } => {
            if params.len() != key.len() {
                return unsupported("a map key whose hash takes other values");
            }
            let x = a.local(VT::I64);
            for (k, (have, want)) in key.iter().zip(params).enumerate() {
                a.get(n32(k));
                a.conv(have, want);
            }
            a.call((**hash).clone());
            a.set(x);
            a.get(x);
            a.get(x);
            a.i64(32);
            a.s().i64_shr_u().i64_xor().i32_wrap_i64();
        }
    }
    a.i32(0x9E37_79B1_u32.cast_signed());
    a.s().i32_mul();
    a.set(hv);
    a.get(hv);
    a.get(hv);
    a.i32(16);
    a.s().i32_shr_u().i32_xor();
    a.i32(0x7FFF_FFFF);
    a.s().i32_and();
    Ok(a.finish(vec![VT::I32]))
}

/// Pushes whether entry `e`'s key equals the key in locals `k0..`.
fn key_eq(a: &mut Asm, map: &WTy, key: &[VT], ops: &KeyOps, e: u32, k0: u32) {
    let stored = |a: &mut Asm, k: usize| {
        a.get(0);
        a.struct_get(map, M_KEYS + n32(k));
        a.get(e);
        a.array_get(&comp_arr(&key[k]));
    };
    match ops {
        KeyOps::I32 => {
            stored(a, 0);
            a.get(k0);
            a.s().i32_eq();
        }
        KeyOps::I64 => {
            stored(a, 0);
            a.get(k0);
            a.s().i64_eq();
        }
        KeyOps::Str => {
            stored(a, 0);
            a.s().ref_as_non_null();
            stored(a, 1);
            a.get(k0);
            a.get(k0 + 1);
            a.call(Sym::Helper(Helper::StrEq));
        }
        KeyOps::Call { eq, params, .. } => {
            for (k, want) in params.iter().enumerate() {
                stored(a, k);
                a.conv(&storage(&key[k]).dflt(), want);
            }
            for (k, want) in params.iter().enumerate() {
                a.get(k0 + n32(k));
                a.conv(&key[k], want);
            }
            a.call((**eq).clone());
        }
    }
}

/// `(map, hash, key...) -> entry or -1`: probes the index from the hash
/// until an empty slot. A removed entry holds hash -1, which matches no
/// key's hash, so probing passes it.
pub fn find_code(map: &WTy, key: &[VT], ops: &KeyOps) -> StageResult<Code> {
    if let KeyOps::Call { params, .. } = ops
        && params.len() != key.len()
    {
        return unsupported("a map key whose `eq` takes other values");
    }
    let mut a = Asm::new([vec![VT::r(map.clone()), VT::I32], key.to_vec()].concat());
    let idx = a.local(VT::r(ints()));
    let (mask, p, slot, e) = (
        a.local(VT::I32),
        a.local(VT::I32),
        a.local(VT::I32),
        a.local(VT::I32),
    );
    // A map that never held an entry has no index yet.
    a.get(0);
    a.struct_get(map, M_USED);
    a.s().i32_eqz();
    a.if_();
    a.i32(-1);
    a.s().return_();
    a.end();
    a.get(0);
    a.struct_get(map, M_INDEX);
    a.set(idx);
    mask_of(&mut a, idx, mask);
    a.get(1);
    a.get(mask);
    a.s().i32_and();
    a.set(p);
    a.loop_();
    a.get(idx);
    a.get(p);
    a.array_get(&ints());
    a.set(slot);
    a.get(slot);
    a.s().i32_eqz();
    a.if_();
    a.i32(-1);
    a.s().return_();
    a.end();
    a.get(slot);
    a.i32(1);
    a.s().i32_sub();
    a.set(e);
    a.get(0);
    a.struct_get(map, M_HASHES);
    a.get(e);
    a.array_get(&ints());
    a.get(1);
    a.s().i32_eq();
    a.if_();
    key_eq(&mut a, map, key, ops, e, 2);
    a.if_();
    a.get(e);
    a.s().return_();
    a.end();
    a.end();
    a.get(p);
    a.i32(1);
    a.s().i32_add();
    a.get(mask);
    a.s().i32_and();
    a.set(p);
    a.br(0);
    a.end();
    a.i32(-1);
    Ok(a.finish(vec![VT::I32]))
}

/// `(map, hash, key..., value...)`: an equal key's entry keeps its place
/// and takes the value (`types.map.replace.in-place`,
/// `expr.map.duplicate.last`); a new key appends an entry.
#[must_use]
pub fn put_code(map: &WTy, key: &[VT], val: &[VT], ops: &KeyOps) -> Code {
    let nk = n32(key.len());
    let mut a = Asm::new(
        [
            vec![VT::r(map.clone()), VT::I32],
            key.to_vec(),
            val.to_vec(),
        ]
        .concat(),
    );
    let idx = a.local(VT::r(ints()));
    let (e, u, mask, p) = (
        a.local(VT::I32),
        a.local(VT::I32),
        a.local(VT::I32),
        a.local(VT::I32),
    );
    for k in 0..2 + nk {
        a.get(k);
    }
    a.call(Sym::Helper(Helper::MapFind {
        map: map.clone(),
        key: key.to_vec(),
        ops: ops.clone(),
    }));
    a.set(e);
    a.get(e);
    a.i32(0);
    a.s().i32_ge_s();
    a.if_();
    for (k, v) in val.iter().enumerate() {
        a.get(0);
        a.struct_get(map, M_KEYS + nk + n32(k));
        a.get(e);
        a.get(2 + nk + n32(k));
        a.array_set(&comp_arr(v));
    }
    a.s().return_();
    a.end();
    a.get(0);
    a.struct_get(map, M_USED);
    a.get(0);
    a.struct_get(map, M_HASHES);
    a.array_len();
    a.s().i32_eq();
    a.if_();
    a.get(0);
    a.call(Sym::Helper(Helper::MapGrow {
        map: map.clone(),
        comps: [key, val].concat(),
    }));
    a.end();
    a.get(0);
    a.struct_get(map, M_USED);
    a.set(u);
    a.get(0);
    a.struct_get(map, M_HASHES);
    a.get(u);
    a.get(1);
    a.array_set(&ints());
    for (k, v) in key.iter().chain(val).enumerate() {
        a.get(0);
        a.struct_get(map, M_KEYS + n32(k));
        a.get(u);
        a.get(2 + n32(k));
        a.array_set(&comp_arr(v));
    }
    a.get(0);
    a.struct_get(map, M_INDEX);
    a.set(idx);
    mask_of(&mut a, idx, mask);
    index_insert(&mut a, idx, mask, 1, p, |a| {
        a.get(u);
        a.i32(1);
        a.s().i32_add();
    });
    a.get(0);
    a.get(u);
    a.i32(1);
    a.s().i32_add();
    a.struct_set(map, M_USED);
    a.get(0);
    a.get(0);
    a.struct_get(map, M_LIVE);
    a.i32(1);
    a.s().i32_add();
    a.struct_set(map, M_LIVE);
    a.finish(vec![])
}

/// `(map)`, when the entry arrays are full: copies the live entries, in
/// order, into a capacity that doubles (at least 4) when at least half of
/// them are live and stays otherwise, and rebuilds the index from the
/// stored hashes. Amortized O(1) per insertion either way.
#[must_use]
pub fn grow_code(map: &WTy, comps: &[VT]) -> Code {
    let mut a = Asm::new(vec![VT::r(map.clone())]);
    let (cap, ncap, used, j, e, h, p, mask) = (
        a.local(VT::I32),
        a.local(VT::I32),
        a.local(VT::I32),
        a.local(VT::I32),
        a.local(VT::I32),
        a.local(VT::I32),
        a.local(VT::I32),
        a.local(VT::I32),
    );
    let nidx = a.local(VT::r(ints()));
    let nh = a.local(VT::r(ints()));
    let fresh: Vec<u32> = comps.iter().map(|v| a.local(VT::r(comp_arr(v)))).collect();
    a.get(0);
    a.struct_get(map, M_HASHES);
    a.array_len();
    a.set(cap);
    a.get(0);
    a.struct_get(map, M_USED);
    a.set(used);
    a.get(cap);
    a.set(ncap);
    a.get(0);
    a.struct_get(map, M_LIVE);
    a.i32(1);
    a.s().i32_shl();
    a.get(cap);
    a.s().i32_ge_u();
    a.if_();
    a.get(cap);
    a.i32(1);
    a.s().i32_shl();
    a.set(ncap);
    a.get(ncap);
    a.i32(4);
    a.s().i32_lt_u();
    a.if_();
    a.i32(4);
    a.set(ncap);
    a.end();
    a.end();
    a.get(ncap);
    a.i32(1);
    a.s().i32_shl();
    a.array_new_default(&ints());
    a.set(nidx);
    a.get(ncap);
    a.array_new_default(&ints());
    a.set(nh);
    for (k, v) in comps.iter().enumerate() {
        a.get(ncap);
        a.array_new_default(&comp_arr(v));
        a.set(fresh[k]);
    }
    mask_of(&mut a, nidx, mask);
    a.block();
    a.loop_();
    a.get(e);
    a.get(used);
    a.s().i32_ge_u();
    a.br_if(1);
    a.get(0);
    a.struct_get(map, M_HASHES);
    a.get(e);
    a.array_get(&ints());
    a.set(h);
    a.get(h);
    a.i32(0);
    a.s().i32_ge_s();
    a.if_();
    a.get(nh);
    a.get(j);
    a.get(h);
    a.array_set(&ints());
    for (k, v) in comps.iter().enumerate() {
        let arr = comp_arr(v);
        a.get(fresh[k]);
        a.get(j);
        a.get(0);
        a.struct_get(map, M_KEYS + n32(k));
        a.get(e);
        a.array_get(&arr);
        a.array_set(&arr);
    }
    index_insert(&mut a, nidx, mask, h, p, |a| {
        a.get(j);
        a.i32(1);
        a.s().i32_add();
    });
    a.get(j);
    a.i32(1);
    a.s().i32_add();
    a.set(j);
    a.end();
    a.get(e);
    a.i32(1);
    a.s().i32_add();
    a.set(e);
    a.br(0);
    a.end();
    a.end();
    a.get(0);
    a.get(nidx);
    a.struct_set(map, M_INDEX);
    a.get(0);
    a.get(nh);
    a.struct_set(map, M_HASHES);
    for (k, f) in fresh.iter().enumerate() {
        a.get(0);
        a.get(*f);
        a.struct_set(map, M_KEYS + n32(k));
    }
    a.get(0);
    a.get(j);
    a.struct_set(map, M_USED);
    a.finish(vec![])
}

/// The iterator environment's fields after the code: the map, the next
/// entry, the entry count written at creation, the live count then.
pub const IT_MAP: u32 = 1;
pub const IT_POS: u32 = 2;
pub const IT_END: u32 = 3;
pub const IT_LEN: u32 = 4;

/// The cursor of `Map.iter()`. A live count other than the captured one
/// is the `iterator-invalidated` panic (`flow.for.length`,
/// `flow.for.invalidate.panic`), checked on every call, so an exhausted
/// iterator panics too. Entries at or past the captured end stay unseen
/// and removed entries are skipped (Q-R7); a value replaced in place is
/// seen (`flow.for.replace.observed`).
pub fn step_code(env: &WTy, comps: &[VT], tuple: Option<&WTy>, opt: &OptForm) -> StageResult<Code> {
    let WTy::Struct { fields, .. } = env else {
        return unsupported("a map cursor without its environment");
    };
    let (Some(VT::Ref(code, _)), Some(VT::Ref(map, _))) = (fields.first(), fields.get(1)) else {
        return unsupported("a map cursor environment without its code and map");
    };
    let WTy::Func(params, results) = &**code else {
        return unsupported("a map cursor without a code type");
    };
    let map: &WTy = map;
    let mut a = Asm::new(params.clone());
    let e = a.local(VT::r(env.clone()));
    let m = a.local(VT::r(map.clone()));
    let (i, end) = (a.local(VT::I32), a.local(VT::I32));
    a.get(0);
    a.ref_cast(env, false);
    a.set(e);
    a.get(e);
    a.struct_get(env, IT_MAP);
    a.set(m);
    a.get(m);
    a.struct_get(map, M_LIVE);
    a.get(e);
    a.struct_get(env, IT_LEN);
    a.s().i32_ne();
    a.if_();
    a.call(Sym::Helper(Helper::Panic(
        "iterator-invalidated: map changed during iteration".into(),
    )));
    a.s().unreachable();
    a.end();
    a.get(e);
    a.struct_get(env, IT_POS);
    a.set(i);
    a.get(e);
    a.struct_get(env, IT_END);
    a.set(end);
    // Removals and insertions that restore the live count go undetected;
    // a growth among them compacts the entries, so the end is clamped to
    // what is written now.
    a.get(m);
    a.struct_get(map, M_USED);
    a.get(end);
    a.s().i32_lt_u();
    a.if_();
    a.get(m);
    a.struct_get(map, M_USED);
    a.set(end);
    a.end();
    a.block();
    a.loop_();
    a.get(i);
    a.get(end);
    a.s().i32_ge_u();
    a.br_if(1);
    a.get(m);
    a.struct_get(map, M_HASHES);
    a.get(i);
    a.array_get(&ints());
    a.i32(0);
    a.s().i32_ge_s();
    a.if_();
    a.get(e);
    a.get(i);
    a.i32(1);
    a.s().i32_add();
    a.struct_set(env, IT_POS);
    if *opt == OptForm::Tagged {
        a.i32(1);
    }
    for (k, v) in comps.iter().enumerate() {
        let st = storage(v).dflt();
        a.get(m);
        a.struct_get(map, M_KEYS + n32(k));
        a.get(i);
        a.array_get(&WTy::Array(st.clone()));
        let want = if tuple.is_none() && *opt == OptForm::NullRef {
            v.dflt()
        } else {
            v.clone()
        };
        a.conv(&st, &want);
    }
    if let Some(t) = tuple {
        a.struct_new(t);
    }
    if let OptForm::Boxed(b) = opt {
        a.struct_new(b);
    }
    a.s().return_();
    a.end();
    a.get(i);
    a.i32(1);
    a.s().i32_add();
    a.set(i);
    a.br(0);
    a.end();
    a.end();
    a.get(e);
    a.get(i);
    a.struct_set(env, IT_POS);
    for v in results {
        a.zero(v);
    }
    Ok(a.finish(results.clone()))
}
