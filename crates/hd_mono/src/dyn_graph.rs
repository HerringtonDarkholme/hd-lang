//! Instantiated supertrait graph queries (#1987).
//!
//! Follow rustc's `prepare_vtable_segments`: an explicit DFS stack and a
//! visited set of instantiated traits, not a recursive walk of inheritance
//! paths. Unlike rustc's segmented vtable, hd keeps its direct-parent ABI.
//! <https://github.com/rust-lang/rust/blob/main/compiler/rustc_trait_selection/src/traits/vtable.rs>

use hd_base::{DefId, HashMap, HashSet, NotImplemented, Stage, StageResult};
use hd_types::{InternPool, Ty, TyData, TyList};

use crate::{Collected, ProgramEnv, subst};

/// Direct supertraits in declaration order, with `Self` and arguments filled.
/// One substitution per edge; consumers retain this list while walking it.
pub fn direct_supers(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    owner: DefId,
    args: TyList,
    self_ty: Ty,
) -> Vec<Ty> {
    let full = pool.list(
        &std::iter::once(self_ty)
            .chain(pool.list_items(args).iter().copied())
            .collect::<Vec<_>>(),
    );
    env.decls()
        .supertraits(owner)
        .iter()
        .map(|&sup| subst(pool, env, owner, full, sup))
        .collect()
}

/// First DFS paths from one erased source. Keep predecessor links, not a
/// copied path per node: storage/build work is O(V + E + method slots), and
/// each lookup costs the length of the emitted path. Exact arguments win;
/// otherwise preserve the former first-definition fallback.
#[derive(Default)]
pub struct DynPaths {
    predecessors: Vec<Option<(usize, u32)>>,
    exact: HashMap<(DefId, TyList), usize>,
    whole: HashMap<Ty, usize>,
    bindings: HashMap<(DefId, TyList), HashMap<DefId, Ty>>,
    fallback: HashMap<DefId, usize>,
}

impl DynPaths {
    /// A table's parent pointers follow the slots its trait keeps in
    /// `program` ([`Collected::slots`]).
    pub fn new(
        pool: &InternPool,
        env: &dyn ProgramEnv,
        program: &Collected,
        root: Ty,
    ) -> StageResult<Self> {
        let mut out = Self::default();
        let mut seen = HashSet::default();
        let mut stack = vec![(root, None)];
        while let Some((at, predecessor)) = stack.pop() {
            if !seen.insert(at) {
                continue;
            }
            let TyData::TraitValue {
                def,
                args,
                bindings,
            } = pool.get(at)
            else {
                continue;
            };
            crate::work::dyn_path(1);
            let index = out.predecessors.len();
            out.predecessors.push(predecessor);
            out.exact.entry((def, args)).or_insert(index);
            out.whole.entry(at).or_insert(index);
            out.bindings
                .entry((def, args))
                .or_insert_with(|| bindings.into_iter().collect());
            out.fallback.entry(def).or_insert(index);
            let own = program.slots(env, def).len();
            let supers = direct_supers(pool, env, def, args, at);
            crate::work::dyn_path(supers.len());
            // Push rightmost first so popping visits declared parents leftmost
            // first, exactly as the old recursive path query did.
            for (k, sup) in supers.into_iter().enumerate().rev() {
                // Parent addresses are Wasm32 byte offsets, not an arbitrary
                // inheritance limit. Propagate an unrepresentable layout.
                let offset = own
                    .checked_add(k)
                    .and_then(|slot| u32::try_from(slot).ok())
                    .and_then(|slot| slot.checked_mul(4))
                    .ok_or_else(|| {
                        NotImplemented::bug(
                            Stage::Emit,
                            "a vtable parent offset exceeds Wasm32 memory",
                        )
                    })?;
                stack.push((sup, Some((index, offset))));
            }
        }
        Ok(out)
    }

    /// A checked whole key, without the legacy definition-only fallback.
    #[must_use]
    pub fn exact_path(&self, to: Ty) -> Option<Vec<u32>> {
        Some(self.path_from_index(*self.whole.get(&to)?))
    }

    /// Whole arguments and every requested associated binding must match.
    /// A supertrait's binding to `Self` counts as bound but stays implicit
    /// in an erased key (`trait.dyn.binding.self-fixed`). An explicitly
    /// bound supplier may therefore serve that implicit form. Weakening
    /// the constraint does not change its vtable path. Valid trait
    /// declarations cannot give one trait instance contradictory bindings.
    #[must_use]
    pub fn key_path(&self, pool: &InternPool, to: Ty) -> Option<Vec<u32>> {
        if let Some(path) = self.exact_path(to) {
            return Some(path);
        }
        let TyData::TraitValue {
            def,
            args,
            bindings,
        } = pool.get(to)
        else {
            return None;
        };
        let have = self.bindings.get(&(def, args))?;
        if bindings.iter().any(|(name, ty)| have.get(name) != Some(ty)) {
            return None;
        }
        Some(self.path_from_index(*self.exact.get(&(def, args))?))
    }

    #[must_use]
    pub fn path(&self, to: (DefId, TyList)) -> Option<Vec<u32>> {
        Some(self.path_from_index(*self.exact.get(&to).or_else(|| self.fallback.get(&to.0))?))
    }

    fn path_from_index(&self, mut index: usize) -> Vec<u32> {
        let mut path = Vec::new();
        while let Some((parent, offset)) = self.predecessors[index] {
            crate::work::dyn_path(1);
            path.push(offset);
            index = parent;
        }
        path.reverse();
        path
    }
}
