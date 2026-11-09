//! std's `map_steps` (`crates/hd_wasm_linear/DESIGN.md` §23.19): the
//! step closure of `Map.iter()`'s `MapEntries`. The cursor keeps its own
//! position and passes it in. Its environment holds the map's table, a
//! snapshot by its count, and the written entry count at creation. The code
//! returns the first live entry at or after the given number
//! (`rt:map_step`, `map.rs`). List cursors are ordinary std code.

use crate::op::Op as I;
use hd_base::StageResult;
use hd_types::{Prim, Ty, TyData};

use super::Em;
use crate::layout::{Mem, Part, fields_walk, payload_mem};
use crate::map::USED;
use crate::module::{HEAP, Sym, mem};
use crate::rt::Helper;

impl Em<'_, '_, '_> {
    /// `map_steps` of map `map`: the cursor's step closure over its
    /// entries, each as its entry number and a `(K, V)` tuple.
    pub(super) fn map_iter(&mut self, i: u32, map: u32) -> StageResult<()> {
        let (key, value, entry) = self.entry(self.ty_of(map))?;
        let pool = self.pool();
        let tuple = |fields: &[Ty]| {
            pool.intern_ty(&TyData::Tuple {
                elems: pool.list(fields),
                rest: None,
            })
        };
        // ruled out: std's sole map_steps signature returns the step
        // function fn(usize) -> (usize, (K, V))?. Its payload leaves are
        // exactly the index followed by the map entry's leaves.
        let index = pool.intern_ty(&TyData::Prim(Prim::Usize));
        let item = tuple(&[index, tuple(&[key, value])]);
        let result = pool.intern_ty(&TyData::Adt {
            def: self.env().option(),
            args: pool.list(&[item]),
        });
        let opt = self.enum_flat(result)?;
        let code = Helper::MapStep {
            entry,
            item: Box::new((*opt).clone()),
        };
        // ruled out: the environment has only a Map's i32 table pointer
        // and an i32 used count, at offsets 0 and 4, for every K and V.
        let int = pool.intern_ty(&TyData::Prim(Prim::I32));
        let tys = [self.ty_of(map), int];
        let (m, parts) = payload_mem(pool, self.env(), &tys)?;
        let walk = fields_walk(pool, self.env(), &tys);
        let cyclic = self.cyclic_holding(&tys);
        let env = self.object(&m, walk.as_ref(), cyclic, false, None);
        let h = self.buffer(map)?;
        let store = |em: &mut Self, m: &Mem, p: Part, v: &dyn Fn(&mut Self) -> StageResult<()>| {
            em.store_leaf(
                &|em: &mut Self| {
                    em.f.get(env);
                    Ok(())
                },
                0,
                m.leaves[p.first],
                &mut |em: &mut Self| v(em),
            )
        };
        store(self, &m, parts[0], &|em| {
            em.f.get(h);
            Ok(())
        })?;
        store(self, &m, parts[1], &|em| {
            em.f.get(h);
            em.f.op(I::I32Load(mem(HEAP, USED, 2)));
            Ok(())
        })?;
        let colls = self.part_colls(&tys, &parts);
        self.retain_fields(env, &m, walk.as_ref(), &colls, colls.len());
        self.f.table_slot(Sym::Helper(code));
        self.f.get(env);
        // The new environment arrives with its count.
        self.store_owned(i)
    }
}
