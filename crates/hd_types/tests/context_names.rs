//! Persistent impl-directory method names, including recovery declarations.
use hd_base::{DefId, Symbol, UnitId};
use hd_intern::PathTable;
use hd_types::lookup::{ImplView, OwnerMap, TableAt, UnitImpls, UniverseImpls, Unseen};
use hd_types::solver::{HeadKey, ImplTable, ImplUniverseId, Impls};

fn contribution(trait_: u32) -> UnitImpls {
    // Only the directory's row identity, trait and head columns are queried.
    UnitImpls {
        table: ImplTable {
            def: vec![DefId::from_raw(trait_ + 100)],
            trait_: vec![DefId::from_raw(trait_)],
            head_key: vec![HeadKey::Any],
            ..ImplTable::default()
        },
        arg_impls: vec![0],
        unowned: vec![0],
        ..UnitImpls::default()
    }
}

fn indexed_names(extra: &UniverseImpls, at: TableAt, name: Symbol) -> Vec<DefId> {
    let paths = PathTable::new();
    let owners = OwnerMap::default();
    let view = ImplView {
        paths: &paths,
        owners: &owners,
        own: None,
        units: &(),
        universe: ImplUniverseId(0),
        extra,
        arity: &|_| 0,
        hidden: Unseen::default(),
        plans: None,
    };
    let codes = [Some(HeadKey::Any.code()), None, None];
    // No own table and no unit's: only the universe's tables have rows.
    assert_eq!(
        Impls::Owned(&view).tables_at(codes),
        [TableAt::Unowned, TableAt::Directory]
    );
    let mut found = Vec::new();
    Impls::Owned(&view).traits_named(
        at,
        codes,
        name,
        &|_, _| panic!("indexed contributions must not rebuild a name index"),
        &mut found,
    );
    found
}

#[test]
fn indexed_directory_unions_preserve_names_and_unnamed_recovery() {
    let first = contribution(1);
    let second = contribution(2);
    let a = Symbol::from_raw(11);
    let b = Symbol::from_raw(12);
    let known = UniverseImpls::unit(UnitId::from_raw(1), &first, &|_, emit| {
        emit(a);
        true
    });
    let unnamed = UniverseImpls::unit(UnitId::from_raw(2), &second, &|_, _| false);
    for merged in [known.union(&unnamed), unnamed.union(&known)] {
        for at in [TableAt::Directory, TableAt::Unowned] {
            assert_eq!(
                indexed_names(&merged, at, a),
                [DefId::from_raw(1), DefId::from_raw(2)]
            );
            assert_eq!(indexed_names(&merged, at, b), [DefId::from_raw(2)]);
        }
    }
    assert_eq!(indexed_names(&known, TableAt::Directory, b), []);
    assert_eq!(
        indexed_names(&known, TableAt::Directory, a),
        [DefId::from_raw(1)]
    );
}

#[test]
fn indexed_directory_unions_with_empty_contexts_keep_the_index() {
    let rows = contribution(1);
    let a = Symbol::from_raw(11);
    let known = UniverseImpls::unit(UnitId::from_raw(1), &rows, &|_, emit| {
        emit(a);
        true
    });
    for merged in [
        known.union(&UniverseImpls::default()),
        UniverseImpls::default().union(&known),
    ] {
        assert_eq!(
            indexed_names(&merged, TableAt::Directory, a),
            [DefId::from_raw(1)]
        );
    }
}
