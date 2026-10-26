//! Fused call-and-drive slots (`crates/hd_wasm_linear/DESIGN.md` §23.39).
//!
//! An immediately driven trait call uses an extra vtable slot. Async await
//! slots return an owned frame when Pending; synchronous `block_on` slots
//! receive caller-local storage for host leaves. Keeping the sets separate
//! avoids linking an async constructor merely for a synchronous call.

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

/// Classifies the async and blocking drive slots before any vtable is built.
/// Each set contains exactly the methods whose corresponding call form appears.
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
            if !drivable(env, trait_, method) {
                continue;
            }
            if tag == Tag::Await {
                m.driven.insert(method);
            } else {
                let t = topo.get_or_insert_with(|| Topology::new(b));
                if block_on_of(b, t, calls, x).is_some() {
                    m.blocking_driven.insert(method);
                }
            }
        }
    }
}

/// The vtable word index of a drive slot: normal methods, async drive
/// slots, then blocking drive slots, each group in declaration order.
pub fn index(
    env: &dyn ProgramEnv,
    m: &Module,
    trait_: DefId,
    method: DefId,
    blocking: bool,
) -> u32 {
    let methods = env.trait_methods(trait_);
    let selected = |candidate, blocking| needs_slot(env, m, trait_, candidate, blocking);
    let preceding = if blocking {
        methods.iter().filter(|d| selected(**d, false)).count()
    } else {
        0
    };
    let rank = methods
        .iter()
        .take_while(|d| **d != method)
        .filter(|d| selected(**d, blocking))
        .count();
    u32::try_from(methods.len() + preceding + rank).expect("slots")
}

/// Persistent vtable values can be called by later inputs in either mode.
/// Give those interfaces a fixed layout, even for methods not yet driven.
fn needs_slot(
    env: &dyn ProgramEnv,
    m: &Module,
    trait_: DefId,
    method: DefId,
    blocking: bool,
) -> bool {
    if m.open_world {
        return drivable(env, trait_, method);
    }
    if blocking {
        m.blocking_driven.contains(&method)
    } else {
        m.driven.contains(&method)
    }
}

/// The vtable of trait `trait_`, with async and then blocking drive slots
/// after its normal slots and parents.
pub fn vtable(
    input: &Input<'_>,
    m: &mut Module,
    trait_: DefId,
    slots: Vec<Sym>,
    parents: Vec<usize>,
) -> StageResult<usize> {
    let mut drives = Vec::new();
    if m.open_world || !m.driven.is_empty() || !m.blocking_driven.is_empty() {
        let methods = input.env.trait_methods(trait_);
        for (method, cold) in methods.iter().zip(&slots) {
            if needs_slot(input.env, m, trait_, *method, false) {
                drives.push(slot(input, *method, cold, false)?);
            }
        }
        for (method, cold) in methods.iter().zip(&slots) {
            if needs_slot(input.env, m, trait_, *method, true) {
                drives.push(slot(input, *method, cold, true)?);
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
fn slot(input: &Input<'_>, method: DefId, cold: &Sym, blocking: bool) -> StageResult<Sym> {
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
            blocking,
            module: module.clone(),
            method: method.clone(),
            args: args.clone(),
            area: area.clone(),
            res: res.clone(),
        },
        Helper::DynTrap { .. } => Helper::DynTrap {
            params: if blocking {
                [params, vec![Val::I32]].concat()
            } else {
                params
            },
            results: [vec![Val::I32], results].concat(),
        },
        other => Helper::Drive {
            blocking,
            cold: Box::new(other.clone()),
            params,
            results,
        },
    }))
}
