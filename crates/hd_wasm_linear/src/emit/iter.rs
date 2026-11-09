//! std's `map_steps` (`crates/hd_wasm_linear/DESIGN.md` §23.19): the
//! step closure of `Map.iter()`'s `MapEntries`. The cursor keeps its own
//! position and passes it in. Its environment holds the map's table, a
//! snapshot by its count, and the written entry count at creation. The code
//! returns the first live entry at or after the given number
//! (`rt:map_step`, `map.rs`). List cursors are ordinary std code.

use crate::op::Op as I;
use hd_base::StageResult;
use hd_types::{Prim, TyData};

use super::Em;
use crate::layout::{Mem, Part, fields_walk, payload_mem};
use crate::map::USED;
use crate::module::{HEAP, Sym, mem};
use crate::rt::Helper;
use crate::unsupported;

/// Pushes one `i32` field of an iterator environment, given the local
/// holding the collection's buffer.
type IntField<'f, E> = dyn Fn(&mut E, u32) -> StageResult<()> + 'f;

impl Em<'_, '_, '_> {
    /// A new iterator environment holding collection `coll`'s buffer,
    /// then `ints` `i32` values, each pushed by its closure; and the code
    /// that steps it, pushed as the closure's code.
    fn iter_env(
        &mut self,
        i: u32,
        coll: u32,
        ints: &[&IntField<'_, Self>],
        code: Helper,
    ) -> StageResult<()> {
        let int = self.pool().intern_ty(&TyData::Prim(Prim::I32));
        let mut tys = vec![self.ty_of(coll)];
        tys.extend(ints.iter().map(|_| int));
        let (m, parts) = payload_mem(self.pool(), self.env(), &tys)?;
        // The step code reads the fields at these offsets.
        let want: Vec<u32> = (0..tys.len())
            .map(|k| 4 * u32::try_from(k).expect("fields"))
            .collect();
        if m.leaves.iter().map(|l| l.offset).collect::<Vec<_>>() != want {
            return unsupported("an iterator environment of another layout");
        }
        let walk = fields_walk(self.pool(), self.env(), &tys);
        let cyclic = self.cyclic_holding(&tys);
        let env = self.object(&m, walk.as_ref(), cyclic, false, None);
        let h = self.buffer(coll)?;
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
        for (p, v) in parts[1..].iter().zip(ints) {
            store(self, &m, *p, &|em| v(em, h))?;
        }
        let colls = self.part_colls(&tys, &parts);
        self.retain_fields(env, &m, walk.as_ref(), &colls, colls.len());
        self.f.table_slot(Sym::Helper(code));
        self.f.get(env);
        // The new environment arrives with its count.
        self.store_owned(i)
    }

    /// `map_steps` of map `map`: the cursor's step closure over its
    /// entries, each as its entry number and a `(K, V)` tuple.
    pub(super) fn map_iter(&mut self, i: u32, map: u32) -> StageResult<()> {
        let (_, _, entry) = self.entry(self.ty_of(map))?;
        let TyData::Fn { result, .. } = self.pool().get(self.ty(i)) else {
            return unsupported("a map step that is not a function");
        };
        let TyData::Option(item) = self.pool().get(result) else {
            return unsupported("a map step whose result is not optional");
        };
        let mut want = vec![crate::module::Val::I32];
        want.extend(entry.leaves.iter().map(|l| l.val));
        if self.vals(item)? != want {
            return unsupported("a map entry that lays out unlike its tuple");
        }
        let ot = self.pool().intern_ty(&TyData::Option(item));
        // `ot` is the optional of the entry type.
        let opt = self.enum_flat(ot)?;
        let count = |at: u64| {
            move |em: &mut Self, h: u32| {
                em.f.get(h);
                em.f.op(I::I32Load(mem(HEAP, at, 2)));
                Ok(())
            }
        };
        let used = count(USED);
        let code = Helper::MapStep {
            entry,
            item: Box::new((*opt).clone()),
        };
        self.iter_env(i, map, &[&used], code)
    }
}
