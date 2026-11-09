//! Promoted arguments (§23.40, #1151): a read-only reference to a
//! caller's local that a call takes by value instead. Such a `Ref` is a
//! read of its place, of the referent's type, and its local needs no
//! memory for it.

use hd_base::{DefId, HashMap, HashSet, StageResult};
use hd_mono::{Target, TargetKind, subst};
use hd_tir::ir::{Body, Ref, Tag};
use hd_types::{Ty, TyList};

use super::Em;
use crate::layout::{Kind, kind};
use crate::module::Module;
use crate::{Input, unsupported};

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

/// The `Ref`s of body `b` (instance `item` at `args`, with call targets
/// `targets`) that are promoted arguments: each argument a direct call
/// passes to a parameter its callee's promoted copy takes by value
/// (collection chose that copy, `hd_tir::own::promote`),
/// and, where `own` holds, each read-only reference to a local that a
/// primitive comparison only reads (`eq`, `cmp`, `partial_cmp`).
pub(super) fn promoted_refs(
    input: &Input<'_>,
    m: &Module,
    b: &Body,
    (item, args): (DefId, TyList),
    targets: &HashMap<u32, Target>,
    own: bool,
) -> StageResult<HashSet<u32>> {
    let (pool, env) = (input.pool, input.env);
    let mut out = HashSet::default();
    for i in (0..b.len()).filter(|&i| b.tags[i] == Tag::Call) {
        let Some(Target::Call(ct)) = targets.get(&u32::try_from(i).expect("insts")) else {
            continue;
        };
        let words = b.record(b.data[i][1]);
        let operands = &words[..words.len().saturating_sub(3)];
        let arg_ty = |a: usize| subst(pool, env, item, args, b.ty[a]);
        match &ct.kind {
            TargetKind::Instance => {
                let Some(&id) = m.inst_of.get(&ct.key) else {
                    continue;
                };
                let mask =
                    hd_mono::defaults::promoted_mask(&input.collected.table.defaults[id.idx()]);
                for (k, &w) in operands.iter().enumerate().take(64) {
                    if mask & (1 << k) == 0 {
                        continue;
                    }
                    let a = Ref(w).as_inst().map_or(usize::MAX, |r| r.0 as usize);
                    if a == usize::MAX || !hd_tir::own::promote::promotable_ref(b, a, i) {
                        return unsupported("a promoted argument that is no local reference");
                    }
                    out.insert(u32::try_from(a).expect("insts"));
                }
            }
            TargetKind::Builtin { .. } if own => {
                let path = (input.path)(ct.item);
                let name = path.rsplit(['.', '/']).next().unwrap_or("");
                if !matches!(name, "eq" | "cmp" | "partial_cmp") {
                    continue;
                }
                for &w in operands {
                    let Some(a) = Ref(w).as_inst().map(|r| r.0 as usize) else {
                        continue;
                    };
                    if hd_tir::own::promote::promotable_ref(b, a, i)
                        && promotable_referent(pool, env, arg_ty(a)).is_some()
                    {
                        out.insert(u32::try_from(a).expect("insts"));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(out)
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
    /// Types each promoted argument ([`promoted_refs`]) as its referent.
    /// A counted one is a loan: its read takes no count, and its `Drop`
    /// releases none, as the callee only borrows it. One that takes its
    /// local's count (`ref_word::TAKES`) moves the value out of the local
    /// instead ([`Em::place_ref`]), and its `Drop`, after the call,
    /// releases it.
    pub(super) fn type_promoted(&mut self, promoted: HashSet<u32>) -> StageResult<()> {
        for &a in &promoted {
            let Some((target, false)) = self.ref_target(self.inst_ty[a as usize]) else {
                return unsupported("a promoted argument whose type is no thin reference");
            };
            self.inst_ty[a as usize] = target;
            let takes = self.b.data[a as usize][1] & hd_tir::ir::ref_word::TAKES != 0;
            if self.tracked(target) && !takes {
                self.loans.insert(a);
            }
        }
        self.promoted = promoted;
        Ok(())
    }
}
