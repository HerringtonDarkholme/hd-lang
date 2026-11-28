//! Declared variance (spec 04 "Variance", "Polarity", "Variance On A
//! Non-Bare Target"): where each marked parameter occurs on a type's
//! readonly surface, and whether that agrees with its marker.

use hd_base::DefId;
use hd_types::{InternPool, ParamRef, Ty, TyData};

/// The polarities a parameter was seen at: positive, negative, invariant.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Seen {
    pub pos: bool,
    pub neg: bool,
    pub inv: bool,
}

impl Seen {
    fn add(&mut self, polarity: i8) {
        match polarity {
            1 => self.pos = true,
            -1 => self.neg = true,
            _ => self.inv = true,
        }
    }

    /// Whether the occurrences break a declared marker
    /// (types.variance.covariant-check, types.variance.contravariant-check).
    #[must_use]
    pub fn breaks(self, marker: i8) -> bool {
        match marker {
            1 => self.neg || self.inv,
            -1 => self.pos || self.inv,
            _ => false,
        }
    }

    /// The variance these occurrences give a parameter
    /// (types.variance.target.derived): 0 when mixed or invariant.
    #[must_use]
    pub fn derived(self) -> i8 {
        match (self.pos, self.neg, self.inv) {
            (true, false, false) => 1,
            (false, true, false) => -1,
            _ => 0,
        }
    }
}

/// Walks `t` at polarity `polarity`, recording each occurrence of a parameter
/// of `owner` in `seen` by index. `var_of` gives a declaration's declared
/// parameter variances.
///
/// Cost: each (type, polarity) pair once, from an explicit stack, and no
/// type without a parameter (`InternPool::has_param`): a second visit of a
/// pair records nothing new, since [`Seen::add`] only sets flags, so a
/// type doubled through n aliases costs n entries, not its 2^n leaves, and
/// no native recursion follows its depth.
pub fn walk(
    pool: &InternPool,
    t: Ty,
    polarity: i8,
    owner: DefId,
    var_of: &dyn Fn(DefId) -> Vec<i8>,
    seen: &mut [Seen],
) {
    let mut visited = hd_base::HashSet::default();
    hd_base::stack::all((t, polarity), |(t, polarity), stack| {
        if !pool.has_param(t) || !visited.insert((t, polarity)) {
            return true;
        }
        crate::work::declaration_type_visit();
        let mut go = |x: Ty, p: i8| stack.push((x, p));
        match pool.get(t) {
            TyData::Param(ParamRef { owner: o, index }) if o == owner => {
                if let Some(s) = seen.get_mut(usize::from(index)) {
                    s.add(polarity);
                }
            }
            // Beneath `mut` is invariant (types.polarity.mut).
            // A reference's operand is invariant too (ref.type.distinct).
            TyData::Ref { inner: i, .. } => go(i, 0),
            TyData::Adt(def, args) => {
                let vs = def.get().map(&var_of);
                for (j, a) in pool.list_items(args).iter().copied().enumerate() {
                    let variance = vs.as_ref().map_or(1, |vs| vs.get(j).copied().unwrap_or(0));
                    go(a, polarity * variance);
                }
            }
            TyData::Opaque { args, bound, .. } => {
                for &a in pool.list_items(args) {
                    go(a, 0);
                }
                go(bound, 0);
            }
            // A closure's identity is invariant in its owner's arguments.
            TyData::Closure {
                args,
                sig,
                captures,
                ..
            } => {
                for &a in pool
                    .list_items(args)
                    .iter()
                    .chain(pool.list_items(captures))
                {
                    go(a, 0);
                }
                go(sig, 0);
            }

            TyData::Fn {
                params,
                result,
                row,
                ..
            } => {
                for p in pool.list_items(params).iter().copied() {
                    go(p, -polarity);
                }
                go(result, polarity);
                // The requirement row is invariant (types.variance.function).
                for k in pool.row_data(row).keys {
                    go(k, 0);
                }
            }
            // A row is invariant wherever it occurs, as a function type's is
            // (types.variance.function): a row argument's keys and a
            // context's.
            TyData::Row(row) | TyData::Context(row) => {
                for k in pool.row_data(row).keys {
                    go(k, 0);
                }
            }
            TyData::TraitValue { args, bindings, .. } => {
                for a in pool.list_items(args).iter().copied() {
                    go(a, 0);
                }
                for (_, b) in bindings {
                    go(b, 0);
                }
            }
            TyData::Assoc { self_ty, args, .. } => {
                go(self_ty, 0);
                for a in pool.list_items(args).iter().copied() {
                    go(a, 0);
                }
            }
            // No parameter of `owner` inside.
            TyData::Param(_)
            | TyData::Prim(_)
            | TyData::Never
            | TyData::Poison
            | TyData::Const(_)
            | TyData::Infer(_)
            | TyData::Canon(_) => {}
        }
        true
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variance_at_depth_1000_fits_a_small_stack() {
        std::thread::Builder::new()
            // cost: one 96 KiB thread for this test.
            .stack_size(96 << 10)
            .spawn(|| {
                let pool = InternPool::new();
                let owner = DefId::from_raw(1);
                let param = pool.intern_ty(&TyData::Param(ParamRef { owner, index: 0 }));
                for depth in [100, 1_000] {
                    let mut t = param;
                    for _ in 0..depth {
                        t = pool.product(pool.list(&[t, t]), None);
                    }
                    for polarity in [-1, 0, 1] {
                        let mut seen = [Seen::default()];
                        let ((), work) = crate::work::count_declaration_type_visits(|| {
                            walk(&pool, t, polarity, owner, &|_| Vec::new(), &mut seen);
                        });
                        assert_eq!(
                            seen[0],
                            Seen {
                                pos: polarity == 1,
                                neg: polarity == -1,
                                inv: polarity == 0
                            }
                        );
                        assert_eq!(work, depth + 1);
                    }
                    let both = pool.intern_ty(&TyData::Fn {
                        params: pool.list(&[t]),
                        result: t,
                        row: hd_types::RowId::EMPTY,
                        suspends: false,
                        inputs: hd_types::Inputs::Fixed,
                        deps: hd_types::FnDeps::NONE,
                    });
                    let mut seen = [Seen::default()];
                    let ((), work) = crate::work::count_declaration_type_visits(|| {
                        walk(&pool, both, 1, owner, &|_| Vec::new(), &mut seen);
                    });
                    assert_eq!(
                        seen[0],
                        Seen {
                            pos: true,
                            neg: true,
                            inv: false
                        }
                    );
                    assert_eq!(work, 2 * (depth + 1) + 1, "one visit per type and polarity");
                }
            })
            .expect("thread")
            .join()
            .expect("no stack overflow");
    }
}
