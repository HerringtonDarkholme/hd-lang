//! Prefix queries retain exactly the keys of a packed secondary index.
use hd_types::context_map::Map;

#[test]
fn packed_index_prefixes_match_exact_filtering_at_every_key_width() {
    let keys = [0, 1, 63, 1 << 32, (1 << 64) | 7, 1 << 127, u128::MAX];
    let mut map = Map::default();
    for key in keys {
        map.insert(key, key);
    }
    for low in [0, 1, 32, 64, 96, 127, 128] {
        let mask = u128::MAX.checked_shl(low).unwrap_or(0);
        for prefix in keys.into_iter().chain([17, u128::MAX - 1]) {
            let expected: Vec<_> = keys
                .into_iter()
                .filter(|k| k & mask == prefix & mask)
                .collect();
            assert_eq!(
                map.prefix(prefix, low).map(|(k, _)| k).collect::<Vec<_>>(),
                expected
            );
        }
    }
}

#[test]
fn pointer_equality_tells_shared_roots_and_empty_maps_apart() {
    let empty = Map::<u32>::default();
    let mut one = Map::default();
    one.insert(5, 50);
    let mut other = Map::default();
    other.insert(5, 50);
    assert!(empty.ptr_eq(&Map::default()));
    assert!(!empty.ptr_eq(&one) && !one.ptr_eq(&empty));
    assert!(one.ptr_eq(&one.clone()));
    assert!(!one.ptr_eq(&other), "equal contents, separate roots");
}

/// A nested union whose inner map on one side is empty takes the other
/// side's inner map, root and all.
#[test]
fn nested_unions_keep_the_nonempty_inner_root() {
    let mut inner = Map::default();
    inner.insert(1, 10u32);
    let mut full = Map::default();
    full.insert(7, inner.clone());
    let mut hollow = Map::default();
    hollow.insert(7, Map::default());
    for merged in [full.nested_union(&hollow), hollow.nested_union(&full)] {
        assert!(merged.get(7).is_some_and(|m| m.ptr_eq(&inner)));
    }
}

#[test]
fn debug_and_iteration_list_the_entries_in_key_order() {
    let mut map = Map::default();
    for (key, value) in [(9u128, 'b'), (2, 'a'), (1 << 70, 'c')] {
        map.insert(key, value);
    }
    assert_eq!(
        format!("{map:?}"),
        "{2: 'a', 9: 'b', 1180591620717411303424: 'c'}"
    );
    let mut values = String::new();
    for (_, value) in &map {
        values.push(*value);
    }
    assert_eq!(values, "abc");
}
