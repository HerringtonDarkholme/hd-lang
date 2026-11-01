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
    let Kind::Ref {
        target, fat: false, ..
    } = kind(pool, env, t)
    else {
        return None;
    };
    if crate::layout::ref_mutable(pool, env, t)
        || hd_mono::is_open(pool, target)
        || !matches!(kind(pool, env, target), Kind::Value)
        || crate::layout::walk(pool, env, target).is_some()
    {
        return None;
    }
    let n = crate::layout::vals(pool, env, target).ok()?.len();
    (1..=4).contains(&n).then_some(target)
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

impl Em<'_, '_, '_> {
    /// Types each promoted argument ([`promoted_refs`]) as its referent.
    pub(super) fn type_promoted(&mut self, promoted: HashSet<u32>) -> StageResult<()> {
        for &a in &promoted {
            let Some((target, false)) = self.ref_target(self.inst_ty[a as usize]) else {
                return unsupported("a promoted argument whose type is no thin reference");
            };
            self.inst_ty[a as usize] = target;
        }
        self.promoted = promoted;
        Ok(())
    }
}
