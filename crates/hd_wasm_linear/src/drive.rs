//! Fused call-and-drive slots (`crates/hd_wasm_linear/DESIGN.md` §23.39).
//!
//! A bang method called through a trait value and awaited at once goes
//! through a second slot of its vtable, its drive slot: the cold call and
//! its first poll in one, so a host provider makes a frame only for a call
//! the host leaves pending. This module decides where a call is awaited at
//! once, which methods therefore get drive slots, and builds those slots.

use hd_base::{DefId, HashMap, StageResult};
use hd_mono::{ProgramEnv, Target, TargetKind};
use hd_tir::ir::{Body, Callee, NONE, PlaceRoot, ReadMode, Tag, Topology};

use crate::module::{Module, Sym, Val};
use crate::rt::Helper;
use crate::{Input, unsupported};

/// Whether a dynamic call of `method`, of trait `trait_`, may use a drive
/// slot: a bang method that is not generic, of a trait with no type
/// arguments and no supertraits, so every vtable of the trait has the same
/// shape (its slots, then its drive slots) and the slot's signature is
/// the method's own.
pub fn drivable(env: &dyn ProgramEnv, trait_: DefId, method: DefId) -> bool {
    env.suspends(method)
        && env.trait_arity(trait_) == 0
        && env.decls().supertraits(trait_).is_empty()
        && hd_mono::own_count(env, method) == 0
}

/// The trait and method of the callee record at word `a`.
pub fn callee(b: &Body, a: u32) -> Option<(DefId, DefId)> {
    match Callee::from_words(b.record(a)) {
        Some(Callee::TraitMethod { trait_, method, .. }) => Some((trait_, method)),
        _ => None,
    }
}

/// The arguments of call or await `i`, without the trailing words.
fn args(b: &Body, i: u32) -> &[u32] {
    let r = b.record(b.data[i as usize][1]);
    &r[..r.len().saturating_sub(3)]
}

fn is_block_on(calls: &HashMap<u32, Target>, i: u32) -> bool {
    matches!(
        calls.get(&i),
        Some(Target::Call(ct)) if matches!(&ct.kind, TargetKind::Intrinsic(k) if k == "block_on")
    )
}

/// The `block_on` that drives cold dynamic call `c` at once, if one does:
/// `block_on(x.m(args))`, or `let s = x.m(args)` and then `block_on(s)`
/// with the local moved in. Between the two only drops of other values,
/// that store and that read may run, so polling at the call instead of at
/// the `block_on` changes nothing a program can see. Execution is
/// followed through the rest of `c`'s block and into tail blocks only.
pub fn block_on_of(b: &Body, topo: &Topology, calls: &HashMap<u32, Target>, c: u32) -> Option<u32> {
    let mut local = None;
    let mut moved = None;
    let mut blk = topo.blk[c as usize];
    let mut pos = topo.pos[c as usize] + 1;
    loop {
        if blk == NONE || b.tags[blk as usize] != Tag::Block {
            return None;
        }
        let [list, tail] = b.data[blk as usize];
        let items = b.record(list);
        while let Some(&x) = items.get(pos as usize) {
            pos += 1;
            let xi = x as usize;
            match b.tags[xi] {
                // A block runs where the instruction that owns it runs.
                Tag::Block => {}
                Tag::Drop => match b.decode_place(b.data[xi][0]).map(|p| p.root) {
                    Some(PlaceRoot::Value(v))
                        if v.as_inst().is_none_or(|v| v.0 != c && Some(v.0) != moved) => {}
                    Some(PlaceRoot::Local(l)) if Some(l) != local => {}
                    _ => return None,
                },
                Tag::Store if local.is_none() && b.data[xi][1] == c => {
                    let (l, true) = b.stored_local(xi)? else {
                        return None;
                    };
                    local = Some(l);
                }
                Tag::Read
                    if moved.is_none()
                        && local.is_some()
                        && b.local_read(xi) == local
                        && ReadMode::from_u32(b.data[xi][1]) == Some(ReadMode::Move) =>
                {
                    moved = Some(x);
                }
                Tag::Call if is_block_on(calls, x) => {
                    let s = args(b, x).first().copied()?;
                    let ok = match (local, moved) {
                        (None, None) => s == c,
                        (Some(_), Some(r)) => s == r,
                        _ => false,
                    };
                    return ok.then_some(x);
                }
                _ => return None,
            }
        }
        if tail == NONE {
            return None;
        }
        blk = tail;
        pos = 0;
    }
}

/// Fills `m.driven` before any vtable is built: every drivable method that
/// some body awaits at once through a trait value. Emission uses a drive
/// slot only for a method in this set, so a vtable never lacks one a call
/// reads.
pub fn collect(input: &Input<'_>, m: &mut Module) {
    let env = input.env;
    for id in input.order {
        let Some(b) = (input.body)(*id) else {
            continue;
        };
        let calls = &input.collected.calls[id.idx()];
        let mut topo = None;
        for i in 0..b.len() {
            let tag = b.tags[i];
            if !matches!(tag, Tag::Await | Tag::Call) {
                continue;
            }
            let x = u32::try_from(i).expect("instructions");
            if !matches!(calls.get(&x), Some(Target::Dyn)) {
                continue;
            }
            let Some((trait_, method)) = callee(b, b.data[i][0]) else {
                continue;
            };
            if !drivable(env, trait_, method) || m.driven.contains(&method) {
                continue;
            }
            let fused = tag == Tag::Await || {
                let t = topo.get_or_insert_with(|| Topology::new(b));
                block_on_of(b, t, calls, x).is_some()
            };
            if fused {
                m.driven.insert(method);
            }
        }
    }
}

/// The vtable word index of `method`'s drive slot: after the trait's
/// method slots (a drivable trait has no supertrait words), in method
/// order among the driven methods.
pub fn index(env: &dyn ProgramEnv, m: &Module, trait_: DefId, method: DefId) -> u32 {
    let methods = env.trait_methods(trait_);
    let rank = methods
        .iter()
        .take_while(|d| **d != method)
        .filter(|d| m.driven.contains(*d))
        .count();
    u32::try_from(methods.len() + rank).expect("slots")
}

/// The vtable of trait `trait_` with `slots` and `parents`, followed by a
/// drive slot for each of its driven methods.
pub fn vtable(
    input: &Input<'_>,
    m: &mut Module,
    trait_: DefId,
    slots: Vec<Sym>,
    parents: Vec<usize>,
) -> StageResult<usize> {
    let mut drives = Vec::new();
    if !m.driven.is_empty() {
        let methods = input.env.trait_methods(trait_);
        for (method, cold) in methods.iter().zip(&slots) {
            if m.driven.contains(method) {
                drives.push(slot(input, *method, cold)?);
            }
        }
        // `index` counts on the method slots alone before the drive slots.
        if !drives.is_empty() && (!parents.is_empty() || slots.len() != methods.len()) {
            return unsupported("drive slots in a vtable of another shape");
        }
    }
    Ok(m.vtable_full(slots, parents, drives))
}

/// The drive slot of `method` beside its cold slot `cold`.
fn slot(input: &Input<'_>, method: DefId, cold: &Sym) -> StageResult<Sym> {
    let (pool, env) = (input.pool, input.env);
    let (ptys, ret) = crate::boundary::signature(pool, env, method, None);
    let mut params = vec![Val::I32];
    for p in ptys {
        params.extend(crate::layout::vals(pool, env, p)?);
    }
    if crate::emit::slot_ctx(env, method) {
        params.push(Val::I32);
    }
    let results = crate::layout::vals(pool, env, ret)?;
    let Sym::Helper(h) = cold else {
        return unsupported("a vtable slot that is not a helper");
    };
    Ok(Sym::Helper(match h {
        Helper::HostCold {
            module,
            method,
            args,
            area,
            res,
        } => Helper::HostDrive {
            module: module.clone(),
            method: method.clone(),
            args: args.clone(),
            area: area.clone(),
            res: res.clone(),
        },
        Helper::DynTrap { .. } => Helper::DynTrap {
            params,
            results: [vec![Val::I32], results].concat(),
        },
        other => Helper::Drive {
            cold: Box::new(other.clone()),
            params,
            results,
        },
    }))
}
