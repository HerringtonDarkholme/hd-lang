//! The body of `std.rt.unique[T]` at an exact `T`
//! (`fn.crossing.mode.unique`), which a `Send` crossing calls on each
//! capture. A `string`, `List`, `Map`, `UniqList` or `UniqMap` calls std's
//! walk; a struct, enum, tuple, union or newtype calls `unique` on each
//! member that may hold a buffer, in place; any other body is empty.
//! Rust's `drop_in_place::<T>` glue has this shape: one body per type, a
//! switch on the variant, a call per field. Cost: O(the type's members).

use hd_base::{DefId, HashMap, StageResult};
use hd_resolve::ItemData;
use hd_tir::Body;
use hd_tir::ir::{
    BodyKind, Callee, Coercion, NONE, Place, PlaceRoot, Proj, Providers, Ref, Tag, TirBuilder,
    TirSink, local_flags, ref_word,
};
use hd_types::{AdtId, Prim, Ty, TyData};

use super::{Cx, Gen, Handles, SYN, Shape, Var};

use hd_resolve::known::{
    CROSSING_MODULE as CROSSING, RUNTIME_MODULE as RUNTIME, UNIQ_LIST_MODULE, UNIQ_MAP_MODULE,
};

/// Whether a `ty` may hold a counted buffer (`module.marker.sync.leaf`):
/// a `string`, `List` or `Map`, a `UniqList` or `UniqMap` of one, or a
/// composite with such a member. A type reaches itself only through a
/// collection, which holds one. `memo` reads each type's members once.
fn holds(cx: &Cx<'_, '_>, ty: Ty, memo: &mut HashMap<Ty, bool>) -> bool {
    if let Some(&h) = memo.get(&ty) {
        return h;
    }
    memo.insert(ty, false);
    let known = &cx.names.known;
    let held = match cx.names.pool.get(ty) {
        TyData::Prim(Prim::String) => true,
        TyData::Adt(AdtId::Declared(d), _) if d == known.list || d == known.map => true,
        TyData::Adt(AdtId::Declared(d), args) if uniq(cx, d).is_some() => {
            (cx.names.pool.list_items(args).iter()).any(|&t| holds(cx, t, memo))
        }
        _ => super::dbg::target(cx, ty).is_some_and(|t| {
            (t.vars.iter().flat_map(|v| &v.members)).any(|m| holds(cx, m.ty, memo))
        }),
    };
    memo.insert(ty, held);
    held
}

/// std's walk of `UniqList` or `UniqMap` `d`, whose raw buffer the type's
/// members do not show: the module declaring both, and the walk.
fn uniq(cx: &Cx<'_, '_>, d: DefId) -> Option<(&'static str, &'static str)> {
    [
        (UNIQ_LIST_MODULE, "UniqList", "unique_uniq_list"),
        (UNIQ_MAP_MODULE, "UniqMap", "unique_uniq_map"),
    ]
    .into_iter()
    .find(|(m, n, _)| cx.names.item(m, n) == d)
    .map(|(m, _, walk)| (m, walk))
}

/// `unique[ty]`'s body: its parameter `value: &mut ty`.
pub fn unique_body(cx: &Cx<'_, '_>, item: DefId, ty: Ty) -> StageResult<Body> {
    let names = cx.names;
    let pool = names.pool;
    let mut g = Gen::new(cx, ty, item, Handles::Supplied);
    g.b = TirBuilder::new(item, BodyKind::Fn);
    let value_ty = mut_ref(cx, ty);
    let name = names.syms.intern("value");
    let value = g.b.local(value_ty, name, local_flags::PARAM, SYN);
    let block = g.b.open_block();
    let s = g.b.get(value, value_ty, SYN);
    let mut memo = HashMap::default();
    let mut holds = |t: Ty| holds(cx, t, &mut memo);
    let (list, map) = (names.known.list, names.known.map);
    // ruled out: a `List` has one type argument and a `Map` two.
    match pool.get(ty) {
        TyData::Prim(Prim::String) => call(&mut g, (CROSSING, "unique_string"), &[], s),
        TyData::Adt(AdtId::Declared(d), args) if d == list || d == map => {
            let parts = pool.list_items(args);
            let deep = parts.iter().any(|&t| holds(t));
            let name = match (d == list, deep) {
                (true, true) => "unique_list",
                (true, false) => "own_list",
                (false, true) => "unique_map",
                (false, false) => "own_map",
            };
            call(&mut g, (CROSSING, name), parts, s);
        }
        TyData::Adt(AdtId::Declared(d), args) if let Some(walk) = uniq(cx, d) => {
            if holds(ty) {
                call(&mut g, walk, pool.list_items(args), s);
            }
        }
        // A newtype has its base's representation, and so its reference.
        TyData::Adt(AdtId::Declared(d), _) if newtype(cx, d) => {
            let base = super::dbg::target(cx, ty).map(|t| t.vars[0].members[0].ty);
            if let Some(base) = base.filter(|&b| holds(b)) {
                let r =
                    g.b.coerce(Coercion::Weaken, NONE, s, mut_ref(cx, base), SYN);
                call(&mut g, (RUNTIME, "unique"), &[base], r);
            }
        }
        _ => {
            let mut vars: Vec<Var> =
                (super::dbg::target(cx, ty).map(|t| t.vars)).unwrap_or_default();
            for v in &mut vars {
                v.members.retain(|m| holds(m.ty));
            }
            let mut each = |g: &mut Gen<'_, '_>, v: &Var| {
                for m in &v.members {
                    let variant = (v.shape == Shape::Enum).then_some(Proj::Variant(v.index));
                    let proj = [Proj::Deref].into_iter().chain(variant);
                    let place = g.b.place(&Place {
                        root: PlaceRoot::Value(s),
                        proj: proj.chain([Proj::Field(m.field)]).collect(),
                    });
                    let r =
                        g.b.emit(Tag::Ref, place, ref_word::MUT, mut_ref(cx, m.ty), SYN);
                    call(g, (RUNTIME, "unique"), &[m.ty], r);
                }
            };
            match &vars[..] {
                _ if vars.iter().all(|v| v.members.is_empty()) => {}
                [v] if v.of_data() => each(&mut g, v),
                _ => g.dispatch(&vars, s, &mut each),
            }
        }
    }
    let root = g.b.close_block(block, None, Ty::VOID, SYN);
    g.finish(root).map(|(body, _)| body)
}

fn newtype(cx: &Cx<'_, '_>, d: DefId) -> bool {
    matches!(
        cx.items.item(d).map(|i| &i.data),
        Some(ItemData::Newtype(..))
    )
}

/// A call of `module`'s `name` at `targs` on reference `r`.
fn call(g: &mut Gen<'_, '_>, (module, name): (&str, &str), targs: &[Ty], r: Ref) {
    let callee = Callee::Item {
        def: g.names.item(module, name),
        targs: g.names.pool.list(targs),
    };
    g.b.call(&callee, &[r], Providers::None, Ty::VOID, SYN);
}

fn mut_ref(cx: &Cx<'_, '_>, inner: Ty) -> Ty {
    cx.names.pool.intern_ty(&TyData::Ref {
        inner,
        mutable: true,
        counted: false,
    })
}
