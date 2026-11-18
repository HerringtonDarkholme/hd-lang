//! Promoted arguments (§23.40, #1151): a read-only reference to a
//! caller's local that a call takes by value instead. Such a `Ref` is a
//! read of its place, of the referent's type, and its local needs no
//! memory for it.

use hd_base::{HashMap, InstId};
use hd_mono::{Sub, Target, TargetKind};
use hd_tir::ir::{Body, Ref, Tag};
use hd_types::Ty;

use super::Em;
use crate::Input;
use crate::layout::{Kind, kind};
use crate::module::Module;

/// The referent of `t` when a reference of type `t` may be passed by
/// value (§23.40): a thin, read-only reference to a value held in
/// locals with no counted leaf and at most four Wasm values, such as a
/// number, a `bool`, a `char`, or a small struct or tuple of them.
#[must_use]
pub fn promotable_referent(
    pool: &hd_types::InternPool,
    env: &dyn hd_mono::ProgramEnv,
    t: Ty,
) -> Option<Ty> {
    lent_referent(pool, env, t).filter(|&target| crate::layout::walk(pool, env, target).is_none())
}

/// [`promotable_referent`], counted parts allowed: the referent of a
/// reference a promoted copy may take by value, lent with no count
/// (§23.40), such as a `string` or a small struct holding one.
#[must_use]
pub fn lent_referent(
    pool: &hd_types::InternPool,
    env: &dyn hd_mono::ProgramEnv,
    t: Ty,
) -> Option<Ty> {
    small_referent(pool, env, t).filter(|&r| !crate::layout::holds_drop(pool, env, r))
}

/// The small referent of `t`, as [`lent_referent`] gives it, that holds
/// a value with a `std.rt.Drop` (a `Buffer`, a `UniqList`): a value no
/// copy may take but where nothing can drop it while the copy reads it,
/// through its caller's reference parameter (`hd_opt::inline`).
#[must_use]
pub fn rooted_referent(
    pool: &hd_types::InternPool,
    env: &dyn hd_mono::ProgramEnv,
    t: Ty,
) -> Option<Ty> {
    small_referent(pool, env, t).filter(|&r| crate::layout::holds_drop(pool, env, r))
}

fn small_referent(pool: &hd_types::InternPool, env: &dyn hd_mono::ProgramEnv, t: Ty) -> Option<Ty> {
    let Kind::Ref {
        target, fat: false, ..
    } = kind(pool, env, t)
    else {
        return None;
    };
    if crate::layout::ref_mutable(pool, env, t) || !matches!(kind(pool, env, target), Kind::Value) {
        return None;
    }
    let n = crate::layout::vals(pool, env, target).ok()?.len();
    if !(1..=4).contains(&n) {
        return None;
    }
    // The layout's target has its newtypes erased; the promoted value keeps
    // the declared referent, so a path through it steps into a newtype's
    // one field, as `i32(*m)` with `m: &Meters` does.
    Some(declared_referent(pool, env, t).unwrap_or(target))
}

/// The type a reference type `t` refers to, as declared: newtypes kept.
fn declared_referent(
    pool: &hd_types::InternPool,
    env: &dyn hd_mono::ProgramEnv,
    t: Ty,
) -> Option<Ty> {
    let t = crate::layout::strip(pool, env, t);
    match pool.get(t) {
        hd_types::TyData::Ref { inner, .. } => Some(inner),
        _ => None,
    }
}

/// Each instance body whose storage collection's choices change, with
/// them written (`hd_tir::own::settle`): the arguments a call passes to
/// its callee's promoted copy (`hd_tir::own::promote`), each local
/// reference a primitive comparison (`eq`, `cmp`, `partial_cmp`) only
/// reads in a plain root of the optimized pipeline, and the root's locals
/// an await of no known body takes. The instances of a body's sub-bodies
/// settle it together, in one walk; an adapter's call body has no `Ref`.
#[must_use]
pub fn settled(input: &Input<'_>) -> HashMap<InstId, std::sync::Arc<Body>> {
    let (pool, env, t) = (input.pool, input.env, &input.collected.table);
    let masks: HashMap<_, _> = (t.key.iter().zip(&t.defaults))
        .map(|(&k, d)| (k, hd_mono::defaults::promoted_mask(d)))
        .filter(|&(_, m)| m != 0)
        .collect();
    // A second instance of one sub-body starts a group of its own.
    let mut groups = Vec::<(&Body, hd_base::DefId, Vec<Option<InstId>>)>::new();
    let mut at = HashMap::<*const Body, usize>::default();
    for &id in input.order {
        let (Sub::Body(sub), Some(b)) = (t.sub[id.idx()], (input.body)(id)) else {
            continue;
        };
        let g = at.entry(std::ptr::from_ref(b)).or_insert(usize::MAX);
        if groups
            .get(*g)
            .is_none_or(|(_, _, s)| s[usize::from(sub)].is_some())
        {
            *g = groups.len();
            groups.push((b, t.item[id.idx()], vec![None; b.sub_root.len()]));
        }
        groups[*g].2[usize::from(sub)] = Some(id);
    }
    let mut out = HashMap::default();
    for (b, item, insts) in groups {
        // A suspending entry root with no suspension point is plain.
        let plain = !env.suspends(item)
            || input.entry.root(item) && super::susp::plan(b, b.sub_root[0]).0.is_empty();
        let (mut args, mut ops, mut counted) = (Vec::new(), Vec::new(), Vec::new());
        for (i, sub) in b.inst_subs().into_iter().enumerate() {
            let (Some(Some(id)), Ok(sub)) = (insts.get(sub as usize), u16::try_from(sub)) else {
                continue;
            };
            let target = match input.collected.calls[id.idx()].get(&hd_tir::ir::u32_of(i)) {
                Some(Target::Call(ct)) => Some(ct),
                _ => None,
            };
            let instance = target.filter(|ct| ct.kind == TargetKind::Instance);
            if b.tags[i] == Tag::Await && sub == 0 && instance.is_none() {
                hd_tir::own::operands(b, i, &mut ops);
                counted.resize(b.local_ty.len(), false);
                for l in ops
                    .iter()
                    .filter_map(|op| super::shape::ref_local(b, op.value as usize))
                {
                    counted[l] = true;
                }
            } else if b.tags[i] == Tag::Call
                && let Some(ct) = instance
            {
                // ruled out: collection's promoted_target installs this mask
                // only after promoted_call proves a local Ref. Its operand
                // word is therefore an instruction ID, not a constant.
                let mask = masks.get(&ct.key).copied().unwrap_or(0);
                let ks = b.call_args(i).iter().take(64).enumerate();
                args.extend(
                    ks.filter(|&(k, _)| mask & (1 << k) != 0)
                        .map(|(_, &w)| (sub, w)),
                );
            } else if b.tags[i] == Tag::Call
                && let Some(ct) = target.filter(|_| input.promote_args && sub == 0 && plain)
            {
                let path = (input.path)(ct.item);
                let compares = matches!(
                    path.rsplit(['.', '/']).next(),
                    Some("eq" | "cmp" | "partial_cmp")
                );
                let refs = b.call_args(i).iter().filter_map(|&w| Ref(w).as_inst());
                args.extend(refs.map(|a| (sub, a.0)).filter(|&(_, a)| {
                    compares
                        && matches!(ct.kind, TargetKind::Builtin { .. })
                        && hd_tir::own::promote::promotable_ref(b, a as usize, i)
                        && promotable_referent(pool, env, b.ty[a as usize]).is_some()
                }));
            }
        }
        let changed = !(args.is_empty() && counted.is_empty());
        if let Some(b) = changed
            .then(|| hd_tir::own::settle(b, &args, &counted, input.own_cx))
            .flatten()
        {
            let b = std::sync::Arc::new(b);
            out.extend(
                insts
                    .into_iter()
                    .flatten()
                    .map(|id| (id, std::sync::Arc::clone(&b))),
            );
        }
    }
    out
}

/// The parameters instance `key`, a promoted copy, takes by value: the
/// mask of its mark (`defaults::promoted`), 0 for any other instance.
/// Both sides of a call read them as lent: the caller passes its local's
/// value with no count, and the copy owns none.
pub(super) fn promoted_params(input: &Input<'_>, m: &Module, key: hd_base::Hash128) -> u64 {
    m.inst_of.get(&key).map_or(0, |id| {
        hd_mono::defaults::promoted_mask(&input.collected.table.defaults[id.idx()])
    })
}

impl Em<'_, '_, '_> {
    /// Whether `Ref` `i` is a promoted argument (`ref_word::PROMOTED`): a
    /// read of its place's value, typed as its referent.
    pub(super) fn promoted(&self, i: u32) -> bool {
        let i = i as usize;
        self.b.tags[i] == Tag::Ref && self.b.data[i][1] & hd_tir::ir::ref_word::PROMOTED != 0
    }

    /// Types each promoted argument of this sub-body as its referent.
    /// A counted one is a loan: its read takes no count, and its `Drop`
    /// releases none, as the callee only borrows it. One that takes its
    /// local's count (`ref_word::TAKES`) moves the value out of the local
    /// instead ([`Em::place_ref`]), and its `Drop`, after the call,
    /// releases it.
    pub(super) fn type_promoted(&mut self) {
        let rows = std::sync::Arc::clone(&self.rows);
        for a in rows.iter().map(|&a| hd_tir::ir::u32_of(a)) {
            // ruled out: promoted copies select only lent_referent types;
            // primitive comparisons use promotable_referent. Both are thin.
            if self.promoted(a)
                && let Some((target, false)) = self.ref_target(self.ty(a))
            {
                self.retyped.insert(a, target);
                let takes = self.b.data[a as usize][1] & hd_tir::ir::ref_word::TAKES != 0;
                if self.tracked(target) && !takes {
                    self.loans.insert(a);
                }
            }
        }
    }
}
