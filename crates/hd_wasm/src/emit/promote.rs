//! Promoted arguments (§23.40, #1151): a read-only reference to a
//! caller's local that a call takes by value instead. Such a `Ref` is a
//! read of its place, of the referent's type, and its local needs no
//! memory for it.

use hd_base::{HashMap, InstId};
use hd_mono::{Sub, Target, TargetKind};
use hd_tir::ir::{Body, Ref, Tag};

use super::Em;
use crate::Input;
use crate::module::Module;

/// Final instance-TIR records; boundary annotations do not clone a body's
/// instruction columns. Sibling sub-bodies share their one TIR plan.
#[derive(Default)]
pub struct Settled {
    pub bodies: HashMap<InstId, std::sync::Arc<Body>>,
    pub boundaries: HashMap<InstId, std::sync::Arc<hd_tir::own::boundary::Boundary>>,
}

/// Collection facts selecting the TIR boundary pass, shared with generated
/// call bodies. Type ownership and loan decisions belong to that TIR pass.
pub(super) fn boundary_inputs(
    input: &Input<'_>,
    b: &Body,
    item: hd_base::DefId,
    cold: bool,
    lends: u64,
) -> Vec<hd_tir::own::boundary::Input> {
    let mut closures = vec![None; b.sub_root.len()];
    for i in 0..b.len() {
        if b.tags[i] == Tag::Closure {
            closures[b.data[i][0] as usize].get_or_insert(i);
        }
    }
    (0..b.sub_root.len())
        .map(|sub| {
            let closure = closures[sub];
            let suspending = closure.map_or_else(
                || {
                    !cold
                        && input.env.suspends(item)
                        && !(input.entry.root(item)
                            && super::susp::plan(b, b.sub_root[sub]).0.is_empty())
                },
                |ci| input.own_cx.function_suspends(b.ty[ci]),
            );
            hd_tir::own::boundary::Input {
                sub: hd_tir::ir::u32_of(sub),
                closure,
                suspending,
                by_type: cold || !suspending && (sub != 0 || !input.env.suspends(item)),
                lends: if sub == 0 { lends } else { 0 },
            }
        })
        .collect()
}

/// Each instance body whose storage collection's choices change, with
/// them written (`hd_tir::own::settle`): the arguments a call passes to
/// its callee's promoted copy (`hd_tir::own::promote`), each local
/// reference a primitive comparison (`eq`, `cmp`, `partial_cmp`) only
/// reads in a plain root of the optimized pipeline, and the root's locals
/// an await of no known body takes. The instances of a body's sub-bodies
/// settle it together, in one walk; an adapter's call body has no `Ref`.
#[must_use]
pub fn settled(input: &Input<'_>) -> Settled {
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
    let mut out = Settled::default();
    for (b, item, insts) in groups {
        // A suspending entry root with no suspension point is plain.
        let plain = !env.suspends(item)
            || input.entry.root(item) && super::susp::plan(b, b.sub_root[0]).0.is_empty();
        let (mut args, mut ops, mut counted) = (Vec::new(), Vec::new(), Vec::new());
        let topo = hd_tir::ir::Topology::new(b);
        for (i, &sub) in topo.sub.iter().enumerate() {
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
                        && hd_layout::facts::promotable_referent(pool, env, b.ty[a as usize])
                            .is_some()
                }));
            }
        }
        let changed = !(args.is_empty() && counted.is_empty());
        let changed = changed
            .then(|| hd_tir::own::settle(b, &args, &counted, input.own_cx))
            .flatten()
            .map(std::sync::Arc::new);
        let body = changed.as_deref().unwrap_or(b);
        let lends = insts.first().copied().flatten().map_or(0, |id| {
            input.env.lends(item, t.args[id.idx()])
                | hd_mono::defaults::promoted_mask(&t.defaults[id.idx()])
        });
        let inputs = boundary_inputs(input, body, item, false, lends);
        let boundary = std::sync::Arc::new(hd_tir::own::boundary::plan(
            body,
            &topo,
            input.own_cx,
            &inputs,
            &mut crate::work::loan_queries,
        ));
        for id in insts.into_iter().flatten() {
            out.boundaries.insert(id, std::sync::Arc::clone(&boundary));
            if let Some(body) = &changed {
                out.bodies.insert(id, std::sync::Arc::clone(body));
            }
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
}
