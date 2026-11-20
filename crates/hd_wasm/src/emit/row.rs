//! Function-row adapters consume checked evidence, never ambient providers.
use super::{Em, Instruction as I, Sym, Target, Val};
use crate::layout::strip_newtype;
use crate::module::{HEAP, mem};
use crate::rt::{Helper, RowAdapter, RowEntry};
use hd_base::StageResult;
use hd_types::{Ty, TyData};

impl Em<'_, '_, '_> {
    /// One ordered predecessor index per whole H, then exact whole K lookup.
    /// Shared with ordinary dynamic dispatch's production DAG algorithm.
    pub(super) fn whole_key_steps(&mut self, supplier: Ty, required: Ty) -> StageResult<Vec<u32>> {
        if supplier == required {
            return Ok(Vec::new());
        }
        if !self.m.row_key_graphs.contains_key(&supplier) {
            let graph = hd_mono::dyn_graph::DynPaths::new(self.pool(), self.env(), supplier)?;
            self.m.row_key_graphs.insert(supplier, graph);
        }
        self.m.row_key_graphs[&supplier]
            .key_path(self.pool(), required)
            .ok_or_else(|| {
                hd_base::NotImplemented::bug(
                    hd_base::Stage::Emit,
                    "checked whole-key supplier has no projection",
                )
            })
    }

    pub(super) fn row_adapt(&mut self, i: u32, original: u32) -> StageResult<()> {
        let evidence = self.b.row_adaptation(i as usize).ok_or_else(|| {
            hd_base::NotImplemented::bug(hd_base::Stage::Emit, "row adapter has no recipe")
        })?;
        let pairs = self.pool().list_items(evidence).to_vec();
        let source_ty = self.ty_of(original);
        let TyData::Fn {
            params,
            result,
            suspends,
            ..
        } = self
            .pool()
            .get(strip_newtype(self.pool(), self.env(), source_ty))
        else {
            return Err(hd_base::NotImplemented::bug(
                hd_base::Stage::Emit,
                "a row adapter's original is not a function",
            ));
        };
        let mut abi = vec![Val::I32];
        for ty in self.pool().list_items(params).to_vec() {
            abi.extend(self.vals(ty)?);
        }
        abi.push(Val::I32);
        let results = if suspends {
            vec![Val::I32]
        } else {
            self.vals(result)?
        };
        let (source_keys, open) = self.fn_row(source_ty);
        // Substitution can collapse distinct generic keys into one row key.
        // Preserve the first checked recipe, like row-set canonicalization.
        let mut seen = hd_base::HashSet::default();
        let mut entries = Vec::new();
        for pair in pairs.chunks_exact(2) {
            let (required, supplier) = (pair[0], pair[1]);
            if !seen.insert(required) {
                continue;
            }
            entries.push(RowEntry {
                required: super::ctx::key_id(self.input, required),
                supplier: super::ctx::key_id(self.input, supplier),
                projection: self.whole_key_steps(supplier, required)?,
            });
        }
        // Inlining may close a generic row tail after the recipe was checked.
        // Its newly concrete keys are still tail entries, not absent providers.
        // The explicit recipe stays first, including when that tail overlaps it.
        let open = open || source_keys.iter().any(|key| !seen.contains(key));
        let code = match self.calls.get(&original) {
            Some(Target::Closure(key)) => Some(*key),
            _ => None,
        };
        let helper = Helper::RowAdapter(RowAdapter {
            params: abi,
            results,
            entries,
            open,
            code,
        });
        // Consume the original function exactly once; only its environment
        // owns a count. No provider from this conversion site is captured.
        let original_value = self.consumed(original)?;
        let environment = if code.is_some() {
            let local = self.f.local(Val::I32);
            self.src(&original_value, 1)?;
            self.f.set(local);
            local
        } else {
            // Ordinary typed function payload (code, old env), 8 bytes. Its
            // existing count walk/drop/cycle traversal owns only old env.
            let layout = crate::layout::mem(self.pool(), self.env(), source_ty)?;
            let walk = crate::layout::count_walk(self.pool(), self.env(), source_ty);
            let cyclic = self.cyclic_holding(&[source_ty]);
            let at = self.object(&layout, walk.as_ref(), cyclic, false, None);
            #[cfg(feature = "checked-report")]
            crate::rt::checked_kind(&mut self.f, self.m, 1);
            for component in 0..2 {
                self.f.get(at);
                self.src(&original_value, component)?;
                self.f.op(I::I32Store(mem(HEAP, component as u64 * 4, 2)));
            }
            at
        };
        self.f.table_slot(Sym::Helper(helper));
        self.f.get(environment);
        self.store(i)
    }
}
