//! Declaration queries over real local and decoded interface items.

use std::sync::Arc;

use hd_base::{DefId, UnitId};
use hd_intern::{PathTable, ShardedInterner};
use hd_resolve::assoc::{
    Fault, LookupView, TraitView, assoc_decls, bound_clashes, reached_assocs, resolve_bindings,
};
use hd_resolve::iface::IfaceIndex;
use hd_resolve::{
    Items, KnownItems, Lookup, LookupDecls, Names, declares_assoc_function, decode_items,
    encode_items, unit_iface,
};
use hd_types::sealed::Declarations;
use hd_types::{InternPool, Ty};

fn with_lookups(mut test: impl FnMut(Names<'_>, &Lookup<'_>)) {
    let pool = InternPool::new();
    let paths = PathTable::new();
    let syms = ShardedInterner::default();
    let known = KnownItems::new(&paths);
    let names = Names {
        pool: &pool,
        paths: &paths,
        syms: &syms,
        known: &known,
    };
    let (items, exports) =
        super::const_generics::resolve_in(names, include_str!("inputs/trait_queries.hd"));
    let bytes = encode_items(&names, &items, &exports).expect("valid source interface");
    let (decoded, exports) = decode_items(&names, &bytes).expect("interface roundtrip");
    let iface = unit_iface(
        "app",
        Items::new(decoded),
        exports,
        Arc::from(bytes),
        None,
        &[],
        Vec::new(),
    );
    let unit = UnitId::from_raw(0);
    let index = IfaceIndex::default();
    index.insert(unit, &iface);
    let local = Lookup::new(&items, [], &index);
    let cached = Lookup::new(&[], [(unit, &iface)], &index);
    test(names, &local);
    test(names, &cached);
}

#[test]
fn associated_queries_deduplicate_diamonds_and_recover_bad_bindings() {
    with_lookups(|names, lookup| {
        let pool = names.pool.types();
        let view = LookupView {
            lookup,
            names: &names,
        };
        let base = names.item("app", "Base");
        let diamond = names.item("app", "Diamond");
        let item = view.own_assoc(base, "Item").expect("Base declares Item");
        let args = pool.list(&[Ty::STRING]);
        assert_eq!(view.own_assocs(base), [item]);
        assert!(view.own_assoc(diamond, "Item").is_none());
        assert_eq!(reached_assocs(pool, &view, diamond), [item]);
        assert_eq!(
            assoc_decls(pool, &view, diamond, args, Some(Ty::I32), "Item"),
            [(base, args, item)]
        );
        let (kept, bad) = resolve_bindings(
            pool,
            &view,
            (diamond, args, Some(Ty::I32)),
            vec![
                ("Item".into(), Ty::prim(hd_types::Prim::I64)),
                ("Item".into(), Ty::BOOL),
            ],
        );
        assert_eq!(kept, [(item, Ty::prim(hd_types::Prim::I64))]);
        assert_eq!(bad.len(), 1);
        assert!(matches!(bad[0].fault, Fault::Duplicate));
        let (recovered, bad) = resolve_bindings(
            pool,
            &view,
            (diamond, args, Some(Ty::I32)),
            vec![("Missing".into(), Ty::I32)],
        );
        assert_eq!(recovered, [(item, Ty::POISON)]);
        assert_eq!(bad.len(), 1);
        assert!(matches!(bad[0].fault, Fault::Unknown));
        // Poison is a normal recovery value after an earlier diagnostic.
        assert!(bound_clashes(pool, &view, Ty::I32, &[Ty::POISON]).is_empty());
    });
}

#[test]
fn declaration_queries_keep_misses_neutral_and_check_reference_capabilities() {
    with_lookups(|names, lookup| {
        let pool = names.pool.types();
        let view = LookupView {
            lookup,
            names: &names,
        };
        let decls = LookupDecls {
            lookup,
            pool: names.pool,
            sealed: names.known.sealed(),
            paths: names.paths,
            syms: names.syms,
        };
        for missing in [
            names.item("app", "Record"),
            names.item("app", "Absent"),
            DefId::NONE,
        ] {
            assert!(view.supers(missing).is_empty());
            assert!(view.own_assocs(missing).is_empty());
            assert!(view.own_assoc(missing, "Item").is_none());
            assert!(reached_assocs(pool, &view, missing).is_empty());
            assert!(!declares_assoc_function(
                names.syms,
                |d| lookup.item(d),
                missing
            ));
            assert!(!decls.has_unavailable_member(missing));
            for mutable in [false, true] {
                assert!(!decls.has_unavailable_reference_member(missing, mutable));
            }
        }
        let base = names.item("app", "Base");
        assert!(declares_assoc_function(
            names.syms,
            |d| lookup.item(d),
            base
        ));
        assert!(decls.has_unavailable_member(base));
        for mutable in [false, true] {
            assert!(decls.has_unavailable_reference_member(base, mutable));
            assert!(!decls.has_unavailable_reference_member(names.item("app", "Empty"), mutable));
            assert!(
                !decls.has_unavailable_reference_member(names.item("app", "ReadOnly"), mutable)
            );
        }
        let edit = names.item("app", "Edit");
        assert!(!decls.has_unavailable_member(edit));
        assert!(decls.has_unavailable_reference_member(edit, false));
        assert!(!decls.has_unavailable_reference_member(edit, true));
    });
}
