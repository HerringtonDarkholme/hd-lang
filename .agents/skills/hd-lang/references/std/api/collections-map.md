# `std.collections.map`

Generated from `hd doc std.collections.map`. Run `hd doc std.collections.map.NAME` in a package for one item's full text.

## `MapKeys`

```text
pub struct MapKeys[K < Eq & Hash, V]
```

A snapshot cursor over a map's keys.

## `MapValues`

```text
pub struct MapValues[K < Eq & Hash, V]
```

A snapshot cursor over a map's values.

## `MapDrain`

```text
pub struct MapDrain[K < Eq & Hash, V, 'a]
```

A borrowed cursor over the entries removed by `Map.drain`.

## `Map`

- `pub fn retain[F < ?Copy & FnMut(&K, &mut V) -> bool](&mut self, mut keep: F) -> void`: Keeps entries selected by `keep`, which may change their values.
- `pub fn extend[I < ?Copy & Iterable[(K, V)]](&mut self, other: I) -> void`: Inserts entries in order, replacing values of equal keys.
- `pub fn drain['a](&'a mut self) -> MapDrain[K, V, 'a]`: Empties this map and yields its entries while borrowing it.
- `pub fn get_or_insert(&mut self, key: K, default: V) -> &mut V`: Returns a mutable reference to the value for `key`; when the map has no equal key, inserts `default` first.
- `pub fn contains_key(&self, key: K) -> bool`: True when the map has an entry for `key`.
- `pub fn keys(&self) -> MapKeys[K, V]`: A lazy cursor over a snapshot of the map's keys, in iteration order.
- `pub fn values(&self) -> MapValues[K, V]`: A lazy cursor over a snapshot of the map's values, in iteration order.
- `pub fn is_empty(&self) -> bool`: True exactly when the map has no entries.
- `pub fn entries(&self) -> List[(K, V)]`: A new list of the `(key, value)` pairs, in the map's iteration order.
- `pub fn most_common(&self, n: usize) -> List[(K, usize)]`: Returns the `n` pairs with the largest counts, largest first; equal counts keep the map's order.
- `pub fn insert(&mut self, key: K, value: V) -> V?`: Inserts a value and returns the previous value, if any, without replacing an equal stored key.
- `pub fn clear(&mut self) -> void`: Removes all entries from this map, leaving other copies unchanged.
