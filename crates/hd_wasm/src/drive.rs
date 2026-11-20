//! Fused call-and-drive slots (`crates/hd_wasm/DESIGN.md` §23.39).
//!
//! An immediately driven trait call uses an extra vtable slot. Async await
//! slots return an owned frame when Pending; synchronous `block_on` slots
//! receive caller-local storage for host leaves. Keeping the sets separate
//! avoids linking an async constructor merely for a synchronous call.

use hd_base::{DefId, HashMap, InstId, StageResult};
use hd_mono::{ProgramEnv, Sub, Target, TargetKind};
use hd_tir::ir::{Body, Callee, NONE, PlaceRoot, ReadMode, Ref, Tag, Topology};
use hd_types::{Ty, TyData};

use crate::Input;
use crate::module::{Module, Sym, Val};
use crate::rt::Helper;

/// Whether a dynamic call of `method`, of trait `trait_`, may use a drive
/// slot: a bang method that is not generic, of a trait with no type
/// arguments, so the slot's signature is the method's own. Parent vtable
/// pointers precede the drive slots, just as in ordinary trait vtables.
pub fn drivable(env: &dyn ProgramEnv, trait_: DefId, method: DefId) -> bool {
    env.suspends(method)
        && env.trait_arity(trait_) == 0
        && env.bounded(method).is_none_or(|b| b.is_empty())
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
    b.call_args(i as usize)
}

fn is_block_on(calls: &HashMap<u32, Target>, i: u32) -> bool {
    matches!(
        calls.get(&i),
        Some(Target::Call(ct)) if matches!(&ct.kind, TargetKind::Intrinsic(k) if k == "block_on")
    )
}

/// The `block_on` that drives cold call `c` at once, if one does:
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
                // A loan's statements run no program code; a fused drive
                // runs the closes between the call and its `block_on` first
                // (`Em::call_known_drive`), so the cold call's argument loans
                // end before the drive, as without fusion.
                Tag::Release | Tag::FlagSet | Tag::FlagClear | Tag::LoanBegin | Tag::LoanClose => {}
                Tag::Drop | Tag::LoanEnd => match b.decode_place(b.data[xi][0]).map(|p| p.root) {
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

/// A known body's first drive always finishes: its own sub-body has no
/// suspension point, and a runtime scenario does not hold its polls.
/// Nested closure bodies do not run merely because their maker runs.
pub fn completes(input: &Input<'_>, id: InstId) -> bool {
    if input.collected.table.sub[id.idx()] != Sub::Body(0)
        || input
            .entry
            .held(input.collected.table.item[id.idx()])
            .is_some()
    {
        return false;
    }
    let Some(body) = (input.body)(id) else {
        return false;
    };
    let topo = Topology::new(body);
    !(0..body.len()).any(|i| {
        topo.sub[i] == 0
            && matches!(
                body.tags[i],
                Tag::Await | Tag::AwaitValue | Tag::AwaitAll | Tag::AwaitRace
            )
    })
}

/// An unresolved function value can name a cold constructor only when its
/// actual lowered ABI returns a suspension. Ordinary callbacks (including
/// collector visitors) cannot discard a cold frame through return variance.
/// Their separately collected bodies still contribute all normal slot uses.
fn raw_cold_call(input: &Input<'_>, id: InstId, body: &Body, i: usize) -> bool {
    if !matches!(input.collected.table.sub[id.idx()], Sub::Body(_)) {
        return true;
    }
    let reference = Ref(body.data[i][0]);
    let ty = match reference.as_inst() {
        Some(index) => body.ty.get(index.0 as usize).copied(),
        None => body
            .consts
            .get((reference.0 & !Ref::CONST_BIT) as usize)
            .map(|c| c.0),
    }
    .unwrap_or(Ty::POISON);
    let table = &input.collected.table;
    let ty = hd_mono::subst(
        input.pool,
        input.env,
        table.item[id.idx()],
        table.args[id.idx()],
        ty,
    );
    let TyData::Fn {
        result: ret,
        suspends,
        ..
    } = input
        .pool
        .get(crate::layout::strip_newtype(input.pool, input.env, ty))
    else {
        return true;
    };
    if suspends {
        return true;
    }
    match input
        .pool
        .get(crate::layout::strip_newtype(input.pool, input.env, ret))
    {
        TyData::Prim(_) | TyData::Tuple { .. } => false,
        TyData::Adt { def, .. } => def == input.env.suspend_def(),
        _ => true,
    }
}

/// Classifies the async and blocking drive slots before any vtable is built.
/// Each set contains exactly the methods whose corresponding call form appears.
pub fn collect(input: &Input<'_>, m: &mut Module) {
    struct BodyFacts {
        topo: std::sync::OnceLock<Topology>,
        raw: Vec<Vec<usize>>,
    }
    // Bodies remain borrowed by Input for this preparation. Cache only
    // immutable topology and original-order raw calls, never call targets.
    let mut bodies: hd_base::HashMap<*const Body, BodyFacts> = hd_base::HashMap::default();
    let env = input.env;
    let mut immediate = hd_base::HashSet::default();
    let mut cold = hd_base::HashSet::default();
    let mut unknown = false;
    for id in input.order {
        let calls = &input.collected.calls[id.idx()];
        let Some(b) = (input.body)(*id) else {
            unknown |= calls.values().any(|target| matches!(target, Target::Dyn));
            continue;
        };
        let facts = bodies.entry(std::ptr::from_ref(b)).or_insert_with(|| {
            let topo = std::sync::OnceLock::new();
            let mut raw = vec![Vec::new(); b.sub_root.len()];
            if b.tags
                .iter()
                .any(|tag| matches!(tag, Tag::CallValue | Tag::AwaitValue))
            {
                let topology = topo.get_or_init(|| Topology::new(b));
                for (i, tag) in b.tags.iter().enumerate() {
                    if matches!(tag, Tag::CallValue | Tag::AwaitValue) {
                        raw[topology.sub[i] as usize].push(i);
                    }
                }
            }
            crate::work::use_facts(b.len());
            BodyFacts { topo, raw }
        });
        if let Sub::Body(sub) = input.collected.table.sub[id.idx()] {
            unknown |= facts.raw[usize::from(sub)]
                .iter()
                .any(|&i| match b.tags[i] {
                    Tag::AwaitValue => true,
                    Tag::CallValue => {
                        // ruled out: verified TIR instruction indices fit u32.
                        !matches!(
                            calls.get(&u32::try_from(i).expect("instructions")),
                            Some(Target::KnownClosure(_))
                        ) && raw_cold_call(input, *id, b, i)
                    }
                    _ => false, // ruled out: only the two raw call tags are indexed.
                });
        }
        for (&x, target) in calls {
            if !matches!(target, Target::Dyn) {
                continue;
            }
            let i = x as usize;
            let tag = b.tags[i];
            if !matches!(tag, Tag::Await | Tag::Call) {
                // A CallValue has no method identity. Keep every normal
                // representation rather than interpreting its Ref as a callee.
                unknown = true;
                continue;
            }
            let Some((trait_, method)) = callee(b, b.data[i][0]) else {
                unknown = true;
                continue;
            };
            if !drivable(env, trait_, method) {
                continue;
            }
            if tag == Tag::Await {
                m.driven.insert(method);
                // An await in a shielded dispose suite uses the normal slot.
                // Retain it conservatively for every asynchronous invocation.
                cold.insert(method);
            } else {
                let t = facts.topo.get_or_init(|| Topology::new(b));
                if block_on_of(b, t, calls, x).is_some() {
                    m.blocking_driven.insert(method);
                    immediate.insert(method);
                } else {
                    cold.insert(method);
                }
            }
        }
    }
    if !m.open_world && !unknown {
        m.blocking_only = immediate.difference(&cold).copied().collect();
    }
}

/// The vtable word index of a drive slot: dispatchable methods, parent
/// pointers, async drives, then blocking drives, each in declaration order.
pub fn index(
    env: &dyn ProgramEnv,
    m: &Module,
    trait_: DefId,
    method: DefId,
    blocking: bool,
) -> u32 {
    let methods = hd_mono::slot_methods(env, trait_);
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
    // ruled out: a vtable fits Wasm32 memory, with four bytes per slot.
    u32::try_from(methods.len() + env.decls().supertraits(trait_).len() + preceding + rank)
        .expect("slots")
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
/// after its normal slots and parents. Every slot is a runtime helper (a
/// trap included), so its drive slot wraps or replaces that helper.
pub fn vtable(
    input: &Input<'_>,
    m: &mut Module,
    trait_: DefId,
    mut slots: Vec<Helper>,
    parents: Vec<usize>,
) -> StageResult<usize> {
    let mut drives = Vec::new();
    if m.open_world || !m.driven.is_empty() || !m.blocking_driven.is_empty() {
        let methods = hd_mono::slot_methods(input.env, trait_);
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
    }
    // Construct drive helpers from the real cold symbols first. Replacing
    // an unused table entry leaves their direct constructor calls as real
    // reachability edges, including a host drive's owned Pending fallback.
    if !m.open_world {
        for (method, normal) in hd_mono::slot_methods(input.env, trait_)
            .iter()
            .zip(&mut slots)
        {
            if m.blocking_only.contains(method) && matches!(normal, Helper::HostCold { .. }) {
                *normal = crate::boundary::uncalled_slot(input, *method)?;
            }
        }
    }
    let slots = slots.into_iter().map(Sym::Helper).collect();
    Ok(m.vtable_full(slots, parents, drives))
}

/// The drive slot of `method` beside its cold slot `cold`.
fn slot(input: &Input<'_>, method: DefId, cold: &Helper, blocking: bool) -> StageResult<Sym> {
    let (pool, env) = (input.pool, input.env);
    let (ptys, ret) = crate::boundary::signature(pool, env, method, None);
    let mut params = vec![Val::I32];
    for p in ptys {
        params.extend(crate::layout::vals(pool, env, p)?);
    }
    if crate::emit::slot_ctx(pool, env, method) {
        params.push(Val::I32);
    }
    let results = crate::layout::vals(pool, env, ret)?;
    Ok(Sym::Helper(match cold {
        Helper::HostCold {
            module,
            method,
            args,
            area,
            counts,
            res,
        } => Helper::HostDrive {
            blocking,
            module: module.clone(),
            method: method.clone(),
            args: args.clone(),
            area: area.clone(),
            counts: counts.clone(),
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
