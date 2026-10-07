//! `hd_mono`: monomorphizing collection (codegen.md §13.1 to §13.3), simplest
//! form. Reads TIR only.

pub mod layout;
pub mod passes;
pub mod suspend;

use std::collections::{BTreeSet, HashMap, HashSet};

use hd_base::Hash128;
use hd_iface::KeyHasher;
use hd_tir::world::{DefId, DefKind, Ty, TyKind, World};
use hd_tir::{CALLEE_ITEM, CHOICE_BOUND, CHOICE_IMPL, TirBody, TirTag};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Instance {
    pub item: DefId,
    pub ty_args: Vec<Ty>,
}

/// `instance_key = H("inst", item stable path, sub-body index, [canon(arg)])`.
#[must_use]
pub fn instance_key(w: &World, inst: &Instance) -> Hash128 {
    let mut h = KeyHasher::new("inst").str(w.path(inst.item)).str("0");
    for t in &inst.ty_args {
        let mut b = Vec::new();
        w.canon(*t).encode(&mut b);
        h = h.bytes(&b);
    }
    h.finish()
}

pub struct InstanceSet {
    /// Sorted by instance key bytes.
    pub instances: Vec<(Hash128, Instance)>,
    /// Per instance key: the hash of its callees' representation summaries,
    /// in call order (walking skeleton, SK-3). Each code key holds it.
    pub callee_reps: HashMap<Hash128, Hash128>,
    pub imports: BTreeSet<u32>,
    pub types: BTreeSet<String>,
}

/// `select` (§13.2): head match of a concrete trait reference.
#[must_use]
pub fn select(w: &World, trait_: DefId, self_ty: Ty) -> Option<DefId> {
    w.impls
        .get(&trait_)?
        .iter()
        .copied()
        .find(|i| matches!(w.defs.get(i), Some(DefKind::Impl { target, .. }) if *target == self_ty))
}

/// Resolves a trait-method callee at an instance to the impl method.
pub fn resolve_method(w: &mut World, rec: &[u32], args: &[Ty]) -> Result<Instance, String> {
    let trait_ = DefId(rec[1]);
    let index = rec[2] as usize;
    let self_ty = w.subst(Ty(rec[3]), args, None);
    let imp = match rec[4] {
        CHOICE_IMPL => DefId(rec[5]),
        CHOICE_BOUND => select(w, trait_, self_ty).ok_or_else(|| {
            format!(
                "select: no impl of {} at {}",
                w.path(trait_),
                w.display(self_ty)
            )
        })?,
        _ => unreachable!(),
    };
    let Some(DefKind::Impl { methods, .. }) = w.defs.get(&imp) else {
        panic!("impl")
    };
    let method = methods
        .iter()
        .find(|(_, m)| matches!(w.defs.get(m), Some(DefKind::ImplMethod { index: i, .. }) if *i as usize == index))
        .map(|(_, m)| *m)
        .expect("impl method");
    Ok(Instance {
        item: method,
        ty_args: Vec::new(),
    })
}

/// A1 at collection: a move-only type argument with a one-reference layout is
/// replaced by its class `REF`.
pub fn classify(w: &mut World, bodies: &HashMap<DefId, TirBody>, mut inst: Instance) -> Instance {
    let Some(b) = bodies.get(&inst.item) else {
        return inst;
    };
    for (i, t) in inst.ty_args.iter_mut().enumerate() {
        let move_only = b.rep_exact.get(i).copied() == Some(0);
        if move_only && matches!(w.kind(*t), TyKind::Adt(_) | TyKind::ClassRef) {
            *t = w.ty(TyKind::ClassRef);
        }
    }
    inst
}

pub fn collect(
    w: &mut World,
    bodies: &HashMap<DefId, TirBody>,
    root: DefId,
) -> Result<InstanceSet, String> {
    let mut seen: HashSet<Instance> = HashSet::new();
    let mut work = vec![Instance {
        item: root,
        ty_args: Vec::new(),
    }];
    let mut out = Vec::new();
    let mut imports = BTreeSet::new();
    let mut types = BTreeSet::new();
    let mut callee_reps = HashMap::new();
    while let Some(inst) = work.pop() {
        if !seen.insert(inst.clone()) {
            continue;
        }
        let body = bodies
            .get(&inst.item)
            .unwrap_or_else(|| panic!("no TIR for {}", w.path(inst.item)));
        let mut reps = KeyHasher::new("callee-reps");
        for i in 0..body.tags.len() {
            let ty = w.subst(body.ty[i], &inst.ty_args, None);
            if let TyKind::Adt(d) = *w.kind(ty) {
                types.insert(w.path(d).to_owned());
            }
            match body.tags[i] {
                TirTag::Call => {
                    let rec = body.get_list(body.data[i][0]).to_vec();
                    let callee = if rec[0] == CALLEE_ITEM {
                        let args: Vec<Ty> = rec[2..]
                            .iter()
                            .map(|&t| w.subst(Ty(t), &inst.ty_args, None))
                            .collect();
                        Instance {
                            item: DefId(rec[1]),
                            ty_args: args,
                        }
                    } else {
                        resolve_method(w, &rec, &inst.ty_args)?
                    };
                    let rep = bodies
                        .get(&callee.item)
                        .map_or(&[][..], |b| &b.rep_exact[..]);
                    reps = reps.str(w.path(callee.item)).bytes(rep);
                    let callee = classify(w, bodies, callee);
                    work.push(callee);
                }
                TirTag::Intrinsic => {
                    imports.insert(body.data[i][0]);
                }
                _ => {}
            }
        }
        let key = instance_key(w, &inst);
        callee_reps.insert(key, reps.finish());
        out.push((key, inst));
    }
    out.sort_by_key(|(k, _)| k.0);
    Ok(InstanceSet {
        instances: out,
        callee_reps,
        imports,
        types,
    })
}
