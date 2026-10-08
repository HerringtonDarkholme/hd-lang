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
pub fn walk(
    pool: &InternPool,
    t: Ty,
    polarity: i8,
    owner: DefId,
    var_of: &dyn Fn(DefId) -> Vec<i8>,
    seen: &mut [Seen],
) {
    let go = |x: Ty, p: i8, seen: &mut [Seen]| walk(pool, x, p, owner, var_of, seen);
    match pool.get(t) {
        TyData::Param(ParamRef { owner: o, index }) if o == owner => {
            if let Some(s) = seen.get_mut(usize::from(index)) {
                s.add(polarity);
            }
        }
        // Beneath `mut` is invariant (types.polarity.mut); `Option` is
        // unmarked (types.option.invariant).
        TyData::Mut(i) | TyData::Option(i) => go(i, 0, seen),
        TyData::Adt { def, args } => {
            let vs = var_of(def);
            for (j, a) in pool.list_items(args).into_iter().enumerate() {
                go(a, polarity * vs.get(j).copied().unwrap_or(0), seen);
            }
        }
        TyData::Tuple { elems, rest } => {
            for e in pool.list_items(elems) {
                go(e, polarity, seen);
            }
            if let Some(r) = rest {
                go(r, polarity, seen);
            }
        }
        TyData::Fn {
            params,
            result,
            row,
            ..
        } => {
            for p in pool.list_items(params) {
                go(p, -polarity, seen);
            }
            go(result, polarity, seen);
            // The requirement row is invariant (types.variance.function).
            for k in pool.row_data(row).keys {
                go(k, 0, seen);
            }
        }
        TyData::TraitValue { args, bindings, .. } => {
            for a in pool.list_items(args) {
                go(a, 0, seen);
            }
            for (_, b) in bindings {
                go(b, 0, seen);
            }
        }
        TyData::Assoc { self_ty, args, .. } => {
            go(self_ty, 0, seen);
            for a in pool.list_items(args) {
                go(a, 0, seen);
            }
        }
        _ => {}
    }
}
