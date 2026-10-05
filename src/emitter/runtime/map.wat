  ;; Kind 0 compares boxed i32 scalars, kind 1 strings (through std's
  ;; `string_equal`, emitter/emitter.ts), and kinds 2 and 3
  ;; call the key type's Eq through the map's `$equal` wrapper, with the map's
  ;; key context: kind 3, a type-parameter key, passes its Eq dictionary.
  (func $hd.map_key_equal
    (param $kind i32)
    (param $left anyref)
    (param $right anyref)
    (param $map (ref $hd.map))
    (result i32)
    (if (i32.ge_u (local.get $kind) (i32.const 2))
      (then
        (return
          (call_ref $hd.key-eq
            (local.get $left)
            (local.get $right)
            (struct.get $hd.map $hd.map-key-context (local.get $map))
            (struct.get $hd.map $hd.map-key-eq (local.get $map))))))
    (if (result i32)
      (i32.eqz (local.get $kind))
      (then
        (i32.eq
          (struct.get $hd.box-i32 $hd.box-i32-value
            (ref.cast (ref $hd.box-i32) (local.get $left)))
          (struct.get $hd.box-i32 $hd.box-i32-value
            (ref.cast (ref $hd.box-i32) (local.get $right)))))
      (else
        (call $hd.string_key_equal (local.get $left) (local.get $right)))))

  ;; The key's `Hash` (spec `types.map.eq-hash`): the `$hd.key-hash` wrapper
  ;; the emitter stored in the map calls the key type's `Hash` implementation
  ;; over a fresh `DefaultHasher`, as `hash_of` does.
  (func $hd.map_hash
    (param $map (ref $hd.map))
    (param $key anyref)
    (result i64)
    (call_ref $hd.key-hash
      (local.get $key)
      (struct.get $hd.map $hd.map-key-hash-context (local.get $map))
      (struct.get $hd.map $hd.map-key-hash (local.get $map))))

  ;; Entries stay in insertion order in `map-keys`/`map-values`
  ;; (spec `types.map.order`): iteration walks those arrays directly.
  ;; `map-buckets` maps each hash bucket to its first entry, 1-based with 0
  ;; for empty; `map-chain` links each entry to the next in its bucket,
  ;; 1-based with 0 for the end. A constant hash (kind 3) degrades to one
  ;; chain: still a correct linear scan, with `Eq` verifying every step.
  (func $hd.map_bucket
    (param $map (ref $hd.map))
    (param $hash i64)
    (result i32)
    (i32.wrap_i64
      (i64.rem_u (local.get $hash)
        (i64.extend_i32_u
          (array.len (struct.get $hd.map $hd.map-buckets (local.get $map)))))))

  ;; Rebuilds the bucket index from the entries in order, over fresh tables.
  ;; Callers grow or compact the entry arrays first.
  (func $hd.map_reindex
    (param $map (ref $hd.map))
    (local $capacity i32)
    (local $index i32)
    (local $bucket i32)
    (local.set $capacity
      (array.len (struct.get $hd.map $hd.map-keys (local.get $map))))
    (struct.set $hd.map $hd.map-buckets
      (local.get $map)
      (array.new_default $hd.map-index (local.get $capacity)))
    (struct.set $hd.map $hd.map-chain
      (local.get $map)
      (array.new_default $hd.map-index (local.get $capacity)))
    (local.set $index (i32.const 0))
    (block $done
      (loop $rehash
        (br_if $done
          (i32.ge_u (local.get $index)
            (struct.get $hd.map $hd.map-size (local.get $map))))
        (local.set $bucket
          (call $hd.map_bucket (local.get $map)
            (call $hd.map_hash (local.get $map)
              (array.get $hd.list
                (struct.get $hd.map $hd.map-keys (local.get $map))
                (local.get $index)))))
        (array.set $hd.map-index
          (struct.get $hd.map $hd.map-chain (local.get $map))
          (local.get $index)
          (array.get $hd.map-index
            (struct.get $hd.map $hd.map-buckets (local.get $map))
            (local.get $bucket)))
        (array.set $hd.map-index
          (struct.get $hd.map $hd.map-buckets (local.get $map))
          (local.get $bucket)
          (i32.add (local.get $index) (i32.const 1)))
        (local.set $index (i32.add (local.get $index) (i32.const 1)))
        (br $rehash))))

  (func $hd.map_insert
    (param $map (ref $hd.map))
    (param $key anyref)
    (param $value anyref)
    (local $index i32)
    (local $size i32)
    (local $capacity i32)
    (local $bucket i32)
    (local $entry i32)
    (local $new-keys (ref $hd.list))
    (local $new-values (ref $hd.list))
    (local.set $size
      (struct.get $hd.map $hd.map-size (local.get $map)))
    ;; Grow first, so the bucket table below is never empty.
    (if
      (i32.ge_u
        (local.get $size)
        (array.len (struct.get $hd.map $hd.map-keys (local.get $map))))
      (then
        (local.set $capacity
          (if (result i32)
            (i32.eqz (local.get $size))
            (then (i32.const 4))
            (else (i32.mul (local.get $size) (i32.const 2)))))
        (local.set $new-keys
          (array.new_default $hd.list (local.get $capacity)))
        (local.set $new-values
          (array.new_default $hd.list (local.get $capacity)))
        (local.set $index (i32.const 0))
        (block $copied
          (loop $copy
            (br_if $copied
              (i32.ge_u (local.get $index) (local.get $size)))
            (array.set $hd.list
              (local.get $new-keys)
              (local.get $index)
              (array.get $hd.list
                (struct.get $hd.map $hd.map-keys (local.get $map))
                (local.get $index)))
            (array.set $hd.list
              (local.get $new-values)
              (local.get $index)
              (array.get $hd.list
                (struct.get $hd.map $hd.map-values (local.get $map))
                (local.get $index)))
            (local.set $index
              (i32.add (local.get $index) (i32.const 1)))
            (br $copy)))
        (struct.set $hd.map $hd.map-keys
          (local.get $map)
          (local.get $new-keys))
        (struct.set $hd.map $hd.map-values
          (local.get $map)
          (local.get $new-values))
        (call $hd.map_reindex (local.get $map))))
    (local.set $bucket
      (call $hd.map_bucket (local.get $map)
        (call $hd.map_hash (local.get $map) (local.get $key))))
    ;; Walk the bucket's chain; a match replaces the value in place, so the
    ;; entry keeps its insertion order (spec `types.map.replace`).
    (local.set $entry
      (array.get $hd.map-index
        (struct.get $hd.map $hd.map-buckets (local.get $map))
        (local.get $bucket)))
    (block $append
      (loop $scan
        (br_if $append (i32.eqz (local.get $entry)))
        (local.set $index
          (i32.sub (local.get $entry) (i32.const 1)))
        (if
          (call $hd.map_key_equal
            (struct.get $hd.map $hd.map-key-kind (local.get $map))
            (array.get $hd.list
              (struct.get $hd.map $hd.map-keys (local.get $map))
              (local.get $index))
            (local.get $key)
            (local.get $map))
          (then
            (array.set $hd.list
              (struct.get $hd.map $hd.map-values (local.get $map))
              (local.get $index)
              (local.get $value))
            (return)))
        (local.set $entry
          (array.get $hd.map-index
            (struct.get $hd.map $hd.map-chain (local.get $map))
            (local.get $index)))
        (br $scan)))
    (array.set $hd.list
      (struct.get $hd.map $hd.map-keys (local.get $map))
      (local.get $size)
      (local.get $key))
    (array.set $hd.list
      (struct.get $hd.map $hd.map-values (local.get $map))
      (local.get $size)
      (local.get $value))
    (array.set $hd.map-index
      (struct.get $hd.map $hd.map-chain (local.get $map))
      (local.get $size)
      (array.get $hd.map-index
        (struct.get $hd.map $hd.map-buckets (local.get $map))
        (local.get $bucket)))
    (array.set $hd.map-index
      (struct.get $hd.map $hd.map-buckets (local.get $map))
      (local.get $bucket)
      (i32.add (local.get $size) (i32.const 1)))
    (struct.set $hd.map $hd.map-size
      (local.get $map)
      (i32.add (local.get $size) (i32.const 1))))

  (func $hd.map_get
    (param $map (ref $hd.map))
    (param $key anyref)
    (result (ref $hd.variant))
    (local $index i32)
    (local $bucket i32)
    (local $entry i32)
    (if (i32.eqz (struct.get $hd.map $hd.map-size (local.get $map)))
      (then (return (struct.new $hd.variant (i32.const 0) (ref.null any)))))
    (local.set $bucket
      (call $hd.map_bucket (local.get $map)
        (call $hd.map_hash (local.get $map) (local.get $key))))
    (local.set $entry
      (array.get $hd.map-index
        (struct.get $hd.map $hd.map-buckets (local.get $map))
        (local.get $bucket)))
    (block $missing
      (loop $scan
        (br_if $missing (i32.eqz (local.get $entry)))
        (local.set $index
          (i32.sub (local.get $entry) (i32.const 1)))
        (if
          (call $hd.map_key_equal
            (struct.get $hd.map $hd.map-key-kind (local.get $map))
            (array.get $hd.list
              (struct.get $hd.map $hd.map-keys (local.get $map))
              (local.get $index))
            (local.get $key)
            (local.get $map))
          (then
            (return
              (struct.new $hd.variant
                (i32.const 1)
                (array.get $hd.list
                  (struct.get $hd.map $hd.map-values (local.get $map))
                  (local.get $index))))))
        (local.set $entry
          (array.get $hd.map-index
            (struct.get $hd.map $hd.map-chain (local.get $map))
            (local.get $index)))
        (br $scan)))
    (struct.new $hd.variant (i32.const 0) (ref.null any)))

;; The read of `m[k] op= v`: the value, or an index-out-of-bounds panic
  ;; when the key is missing (05-expressions.md#r-expr.assign.compound.map-missing).
  (func $hd.map_get_required
    (param $map (ref $hd.map))
    (param $key anyref)
    (result anyref)
    (local $found (ref null $hd.variant))
    (local.set $found (call $hd.map_get (local.get $map) (local.get $key)))
    (if (i32.eqz (struct.get $hd.variant $hd.variant-tag (local.get $found)))
      (then (call $hd.panic (global.get $hd.panic-index-out-of-bounds)) unreachable))
    (struct.get $hd.variant $hd.variant-payload (local.get $found)))

  (func $hd.map_remove
    (param $map (ref $hd.map))
    (param $key anyref)
    (result (ref $hd.variant))
    (local $index i32)
    (local $size i32)
    (local $bucket i32)
    (local $entry i32)
    (local $previous i32)
    (local $removed anyref)
    (local.set $size
      (struct.get $hd.map $hd.map-size (local.get $map)))
    (if (i32.eqz (local.get $size))
      (then (return (struct.new $hd.variant (i32.const 0) (ref.null any)))))
    (local.set $bucket
      (call $hd.map_bucket (local.get $map)
        (call $hd.map_hash (local.get $map) (local.get $key))))
    (local.set $entry
      (array.get $hd.map-index
        (struct.get $hd.map $hd.map-buckets (local.get $map))
        (local.get $bucket)))
    (local.set $previous (i32.const 0))
    (block $missing
      (loop $scan
        (br_if $missing (i32.eqz (local.get $entry)))
        (local.set $index
          (i32.sub (local.get $entry) (i32.const 1)))
        (if
          (call $hd.map_key_equal
            (struct.get $hd.map $hd.map-key-kind (local.get $map))
            (array.get $hd.list
              (struct.get $hd.map $hd.map-keys (local.get $map))
              (local.get $index))
            (local.get $key)
            (local.get $map))
          (then
            (local.set $removed
              (array.get $hd.list
                (struct.get $hd.map $hd.map-values (local.get $map))
                (local.get $index)))
            ;; Unlink the entry from its chain.
            (if (i32.eqz (local.get $previous))
              (then
                (array.set $hd.map-index
                  (struct.get $hd.map $hd.map-buckets (local.get $map))
                  (local.get $bucket)
                  (array.get $hd.map-index
                    (struct.get $hd.map $hd.map-chain (local.get $map))
                    (local.get $index))))
              (else
                (array.set $hd.map-index
                  (struct.get $hd.map $hd.map-chain (local.get $map))
                  (i32.sub (local.get $previous) (i32.const 1))
                  (array.get $hd.map-index
                    (struct.get $hd.map $hd.map-chain (local.get $map))
                    (local.get $index)))))
            ;; Compact the entries, so iteration keeps insertion order.
            (block $shifted
              (loop $shift
                (br_if $shifted
                  (i32.ge_u
                    (i32.add (local.get $index) (i32.const 1))
                    (local.get $size)))
                (array.set $hd.list
                  (struct.get $hd.map $hd.map-keys (local.get $map))
                  (local.get $index)
                  (array.get $hd.list
                    (struct.get $hd.map $hd.map-keys (local.get $map))
                    (i32.add (local.get $index) (i32.const 1))))
                (array.set $hd.list
                  (struct.get $hd.map $hd.map-values (local.get $map))
                  (local.get $index)
                  (array.get $hd.list
                    (struct.get $hd.map $hd.map-values (local.get $map))
                    (i32.add (local.get $index) (i32.const 1))))
                (local.set $index
                  (i32.add (local.get $index) (i32.const 1)))
                (br $shift)))
            (array.set $hd.list
              (struct.get $hd.map $hd.map-keys (local.get $map))
              (i32.sub (local.get $size) (i32.const 1))
              (ref.null any))
            (array.set $hd.list
              (struct.get $hd.map $hd.map-values (local.get $map))
              (i32.sub (local.get $size) (i32.const 1))
              (ref.null any))
            (struct.set $hd.map $hd.map-size
              (local.get $map)
              (i32.sub (local.get $size) (i32.const 1)))
            ;; The entry indices moved, so rebuild the index from the entries.
            (call $hd.map_reindex (local.get $map))
            (return
              (struct.new $hd.variant
                (i32.const 1)
                (local.get $removed)))))
        (local.set $previous (local.get $entry))
        (local.set $entry
          (array.get $hd.map-index
            (struct.get $hd.map $hd.map-chain (local.get $map))
            (local.get $index)))
        (br $scan)))
    (struct.new $hd.variant (i32.const 0) (ref.null any)))
