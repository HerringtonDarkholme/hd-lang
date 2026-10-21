//! std's `list_steps` and `map_steps` (`crates/hd_wasm_linear/DESIGN.md`
//! §23.19, job 11): the step closures of `List.iter()`'s `ListIter` and
//! `Map.iter()`'s `MapEntries`, which keep their position themselves and
//! pass it in. A list's environment holds its header, with a count of its
//! own, and the length at creation; its code is `rt:list_step`, which
//! panics `iterator-invalidated` when the length has changed
//! (`flow.for.invalidate.panic`) and otherwise returns the element at the
//! index, with its own count, or `.None`. A map's holds its header and, at
//! creation, the table's written, live and rebuild counts; its code
//! returns the first live entry from the one passed in, with its number
//! (`rt:map_step`, `map.rs`).

use hd_base::StageResult;
use hd_types::{Prim, TyData};
use wasm_encoder::Instruction as I;

use super::Em;
use crate::layout::{Mem, Part, fields_walk, payload_mem};
use crate::map::{GROWN, LIVE, USED};
use crate::module::{HEAP, Ins, Sym, mem};
use crate::rt::{Helper, LIST_LEN, OptVal};
use crate::unsupported;

/// Pushes one `i32` field of an iterator environment, given the local
/// holding the collection's header.
type IntField<'f, E> = dyn Fn(&mut E, u32) -> StageResult<()> + 'f;

impl Em<'_, '_, '_> {
    /// Where each value of an optional `item` comes from, given the
    /// values of `item` itself; and the optional's values.
    fn opt_of(&self, item: hd_types::Ty) -> StageResult<(Vec<OptVal>, Vec<crate::module::Val>)> {
        let ot = self.pool().intern_ty(&TyData::Option(item));
        let (om, positions) = self.flat(ot)?;
        let Some(some) = positions.get(1).and_then(|fs| fs.first()) else {
            return unsupported("an optional without its payload");
        };
        let opt = (0..om.leaves.len())
            .map(|p| match some.iter().position(|q| *q == p) {
                _ if p == 0 => OptVal::Tag,
                Some(k) => OptVal::Elem(u32::try_from(k).expect("leaves")),
                None => OptVal::Zero,
            })
            .collect();
        Ok((opt, om.vals()))
    }

    /// A new iterator environment holding collection `coll`'s header,
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
        let mut tys = vec![self.strip_mut(self.ty_of(coll))];
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
        let env = self.object(&m, walk.as_ref(), cyclic);
        let h = self.header(coll)?;
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
        self.retain_fields(env, &m, walk.as_ref());
        self.f.push(Ins::TableSlot(Sym::Helper(code)));
        self.f.get(env);
        // The new environment arrives with its count.
        self.store_owned(i)
    }

    /// `list_steps` of list `list`: the cursor's step closure.
    pub(super) fn list_iter(&mut self, i: u32, list: u32) -> StageResult<()> {
        let (et, elem) = self.elem(self.ty_of(list))?;
        let (opt, results) = self.opt_of(et)?;
        let len = |em: &mut Self, h: u32| {
            em.f.get(h);
            em.f.op(I::I32Load(mem(HEAP, 0, 2)));
            em.f.op(I::I32Load(mem(HEAP, LIST_LEN, 2)));
            Ok(())
        };
        let code = Helper::ListStep { elem, opt, results };
        self.iter_env(i, list, &[&len], code)
    }

    /// `map_steps` of map `map`: the cursor's step closure over its
    /// entries, each as its entry number and a `(K, V)` tuple.
    pub(super) fn map_iter(&mut self, i: u32, map: u32) -> StageResult<()> {
        let (_, _, entry) = self.entry(self.ty_of(map))?;
        let TyData::Fn { result, .. } = self.pool().get(self.strip_mut(self.ty(i))) else {
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
        let (opt, results) = self.opt_of(item)?;
        let count = |at: u64| {
            move |em: &mut Self, h: u32| {
                em.f.get(h);
                em.f.op(I::I32Load(mem(HEAP, 0, 2)));
                em.f.op(I::I32Load(mem(HEAP, at, 2)));
                Ok(())
            }
        };
        let (used, live, grown) = (count(USED), count(LIVE), count(GROWN));
        let code = Helper::MapStep {
            entry,
            opt,
            results,
        };
        self.iter_env(i, map, &[&used, &live, &grown], code)
    }
}
