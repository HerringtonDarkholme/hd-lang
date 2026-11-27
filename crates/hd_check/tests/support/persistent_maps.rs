pub type PersistentMapContents = Vec<(u128, u128)>;

/// Exercise the actual persistent method-map implementation and retain an
/// ancestor while extending it. Only final and ancestor contents are read.
#[must_use]
pub fn persistent_method_map_history(
    layers: &[Vec<(u128, u128)>],
) -> (PersistentMapContents, PersistentMapContents) {
    let mut map = hd_types::context_map::Map::default();
    let mut first = None;
    for layer in layers {
        let mut own = hd_types::context_map::Map::default();
        for &(k, v) in layer {
            own.insert(k, v);
        }
        map = map.min_union(&own, |v| *v);
        if first.is_none() {
            first = Some(map.clone());
        }
    }
    let contents = |m: &hd_types::context_map::Map<u128>| m.iter().map(|(k, v)| (k, *v)).collect();
    (
        contents(&map),
        first.as_ref().map_or_else(Vec::new, contents),
    )
}
