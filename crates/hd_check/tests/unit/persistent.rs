use hd_check::{testing::persistent_method_map_history, work::count_method_context_steps};
use std::collections::BTreeMap;

#[test]
fn persistent_method_maps_keep_order_collisions_and_ancestors() {
    let mut layers = vec![vec![(0, 9), (u128::MAX, 8), (1 << 127, 7)]];
    for n in 1..50u128 {
        layers.push(vec![
            (n * 37, n),
            (u128::MAX, 50 - n),
            ((n << 64) | n, n + 1),
        ]);
    }
    let (actual, first) = persistent_method_map_history(&layers);
    let mut expected = BTreeMap::new();
    for layer in &layers {
        for &(k, v) in layer {
            expected
                .entry(k)
                .and_modify(|old: &mut u128| *old = (*old).min(v))
                .or_insert(v);
        }
    }
    assert_eq!(actual, expected.into_iter().collect::<Vec<_>>());
    assert_eq!(first, [(0, 9), (1 << 127, 7), (u128::MAX, 8)]);
}

#[test]
fn extending_method_maps_counts_key_paths_not_prior_members() {
    for count in [100u32, 1000] {
        let layers: Vec<_> = (0..u128::from(count))
            .map(|n| vec![((n << 64) | n, n)])
            .collect();
        let ((out, _), work) =
            count_method_context_steps(|| persistent_method_map_history(&layers));
        assert_eq!(out.len(), layers.len());
        // One inserted key follows at most all128 bits, including an
        // operand swap at each branch; the map never enumerates ancestors.
        assert!(work > 0 && work <= (2 * u64::from(u128::BITS) + 1) * u64::from(count));
    }
}
