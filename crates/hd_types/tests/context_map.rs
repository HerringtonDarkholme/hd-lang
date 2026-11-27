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
