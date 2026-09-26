;; commit bd985d7; hd build --wat b2-generic.hd; 2026-09-25T20:54:04.728Z
(module
  (import "hd" "panic" (func $hd.panic (param i32)))
  (rec
    (type $sig0 (func (param anyref) (result i32)))
    (type $sig1 (func (param anyref) (param i32) (param i32) (result i32)))
    (type $tsig0_0 (func (param anyref) (param anyref) (param i32) (result i32)))
    (type $tsig1_0 (func (param anyref) (param anyref) (result (ref null $hd.bytes))))
    (type $tsig2_0 (func (param anyref) (param anyref) (param anyref) (result i32)))
    (type $tsig3_0 (func (param anyref) (param anyref) (param anyref) (result (ref null $hd.variant))))
    (type $tsig4_0 (func (param anyref) (param anyref)))
    (type $tsig5_0 (func (param anyref) (param anyref) (result (ref null $hd.variant))))
    (type $hd.suspension-poll-sig (func (param anyref) (result i32)))
    (type $hd.suspension-cancel-sig (func (param anyref)))
    (type $hd.suspension-result-sig (func (param anyref) (result anyref)))
    (type $hd.suspension (struct
      (field $hd.suspension-inner anyref)
      (field $hd.suspension-poll (ref $hd.suspension-poll-sig))
      (field $hd.suspension-cancel (ref $hd.suspension-cancel-sig))
      (field $hd.suspension-result (ref $hd.suspension-result-sig))))
    (type $hd.bytes (array (mut i8)))
    (type $hd.list (array (mut anyref)))
    (type $hd.vector (struct
      (field $hd.vector-size (mut i32))
      (field $hd.vector-values (mut (ref $hd.list)))
      (field $hd.vector-version (mut i32))))
    (type $hd.iterator (struct
      (field $hd.iterator-list (ref null $hd.vector))
      (field $hd.iterator-map (ref null $hd.map))
      (field $hd.iterator-index (mut i32))
      (field $hd.iterator-version i32)))
    (type $hd.map (struct
      (field $hd.map-key-kind i32)
      (field $hd.map-size (mut i32))
      (field $hd.map-keys (mut (ref $hd.list)))
      (field $hd.map-values (mut (ref $hd.list)))
      (field $hd.map-version (mut i32))))
    (type $hd.providers (struct
      (field $hd.provider-key i32)
      (field $hd.provider-value anyref)
      (field $hd.provider-parent (ref null $hd.providers))))
    (type $hd.box-i32 (struct
      (field $hd.box-i32-value i32)))
    (type $hd.box-f64 (struct
      (field $hd.box-f64-value f64)))
    (type $hd.box-extern (struct
      (field $hd.box-extern-value externref)))
    (type $hd.variant (struct
      (field $hd.variant-tag i32)
      (field $hd.variant-payload (mut anyref))))
    (type $closure0 (struct
      (field $closure0fn (ref $sig0))
      (field $closure0env anyref)))
    (type $closure1 (struct
      (field $closure1fn (ref $sig1))
      (field $closure1env anyref)))

    (type $trait0 (struct
      (field $trait0value anyref)
      (field $trait0bounds (ref null $hd.list))
      (field $trait0m0 (ref $tsig0_0))))
    (type $trait1 (struct
      (field $trait1value anyref)
      (field $trait1bounds (ref null $hd.list))
      (field $trait1m0 (ref $tsig1_0))))
    (type $trait2 (struct
      (field $trait2value anyref)
      (field $trait2bounds (ref null $hd.list))
      (field $trait2m0 (ref $tsig2_0))))
    (type $trait3 (struct
      (field $trait3value anyref)
      (field $trait3bounds (ref null $hd.list))
      (field $trait3m0 (ref $tsig3_0))))
    (type $trait4 (struct
      (field $trait4value anyref)
      (field $trait4bounds (ref null $hd.list))
      (field $trait4m0 (ref $tsig4_0))))
    (type $trait5 (struct
      (field $trait5value anyref)
      (field $trait5bounds (ref null $hd.list))
      (field $trait5m0 (ref $tsig5_0))))



    (type $e0 (struct
      (field $e0tag i32)))
    (type $hd.runtime (struct
      (field $status (mut i32))
      (field $scratch (mut (ref null $hd.bytes)))))
  )

  (global $e0v0 (ref $e0)
    (struct.new $e0 (i32.const 0)))
  (global $e0v1 (ref $e0)
    (struct.new $e0 (i32.const 1)))
  (global $e0v2 (ref $e0)
    (struct.new $e0 (i32.const 2)))
  (global $hd.runtime-root
    (mut (ref null $hd.runtime))
    (ref.null $hd.runtime))

  (global $hd.driver-active (mut i32) (i32.const 0))
  (global $hd.host-call-site (mut i32) (i32.const -1))

  ;; Stable numeric tags for runtime-panic.ts. The host reports their names.
  (global $hd.panic-integer-overflow i32 (i32.const 2))
  (global $hd.panic-invalid-shift i32 (i32.const 9))
  (global $hd.panic-index-out-of-bounds i32 (i32.const 10))

  (func $hd.provider_get
    (param $providers (ref null $hd.providers))
    (param $key i32)
    (result anyref)
    (local $cursor (ref null $hd.providers))
    (local.set $cursor (local.get $providers))
    (block $missing
      (loop $search
        (br_if $missing (ref.is_null (local.get $cursor)))
        (if
          (i32.eq
            (struct.get $hd.providers $hd.provider-key (ref.as_non_null (local.get $cursor)))
            (local.get $key))
          (then
            (return
              (struct.get $hd.providers $hd.provider-value (ref.as_non_null (local.get $cursor))))))
        (local.set $cursor
          (struct.get $hd.providers $hd.provider-parent (ref.as_non_null (local.get $cursor))))
        (br $search)))
    unreachable)

  (func $hd.provider_concat
    (param $left (ref null $hd.providers))
    (param $right (ref null $hd.providers))
    (result (ref null $hd.providers))
    (if (result (ref null $hd.providers))
      (ref.is_null (local.get $left))
      (then (local.get $right))
      (else
        (struct.new $hd.providers
          (struct.get $hd.providers $hd.provider-key (ref.as_non_null (local.get $left)))
          (struct.get $hd.providers $hd.provider-value (ref.as_non_null (local.get $left)))
          (call $hd.provider_concat
            (struct.get $hd.providers $hd.provider-parent (ref.as_non_null (local.get $left)))
            (local.get $right))))))

  (func $hd.vector_get
    (param $vector (ref $hd.vector))
    (param $index i32)
    (result anyref)
    (if (i32.ge_u (local.get $index) (struct.get $hd.vector $hd.vector-size (local.get $vector)))
      (then (call $hd.panic (global.get $hd.panic-index-out-of-bounds)) unreachable))
    (array.get $hd.list
      (struct.get $hd.vector $hd.vector-values (local.get $vector))
      (local.get $index)))

  (func $hd.vector_set
    (param $vector (ref $hd.vector))
    (param $index i32)
    (param $value anyref)
    (if (i32.ge_u (local.get $index) (struct.get $hd.vector $hd.vector-size (local.get $vector)))
      (then (call $hd.panic (global.get $hd.panic-index-out-of-bounds)) unreachable))
    (array.set $hd.list
      (struct.get $hd.vector $hd.vector-values (local.get $vector))
      (local.get $index)
      (local.get $value)))

  (func $hd.vector_append
    (param $vector (ref $hd.vector))
    (param $value anyref)
    (local $size i32)
    (local $capacity i32)
    (local $next-capacity i32)
    (local $index i32)
    (local $old-values (ref $hd.list))
    (local $new-values (ref $hd.list))
    (local.set $size (struct.get $hd.vector $hd.vector-size (local.get $vector)))
    (local.set $old-values (struct.get $hd.vector $hd.vector-values (local.get $vector)))
    (local.set $capacity (array.len (local.get $old-values)))
    (if (i32.ge_u (local.get $size) (local.get $capacity))
      (then
        (local.set $next-capacity
          (if (result i32) (i32.eqz (local.get $capacity))
            (then (i32.const 4))
            (else (i32.mul (local.get $capacity) (i32.const 2)))))
        (local.set $new-values (array.new_default $hd.list (local.get $next-capacity)))
        (block $copied
          (loop $copy
            (br_if $copied (i32.ge_u (local.get $index) (local.get $size)))
            (array.set $hd.list
              (local.get $new-values)
              (local.get $index)
              (array.get $hd.list (local.get $old-values) (local.get $index)))
            (local.set $index (i32.add (local.get $index) (i32.const 1)))
            (br $copy)))
        (struct.set $hd.vector $hd.vector-values (local.get $vector) (local.get $new-values))))
    (array.set $hd.list
      (struct.get $hd.vector $hd.vector-values (local.get $vector))
      (local.get $size)
      (local.get $value))
    (struct.set $hd.vector $hd.vector-size
      (local.get $vector)
      (i32.add (local.get $size) (i32.const 1)))
    (struct.set $hd.vector $hd.vector-version
      (local.get $vector)
      (i32.add
        (struct.get $hd.vector $hd.vector-version (local.get $vector))
        (i32.const 1))))

  (func $hd.add_i32 (param $left i32) (param $right i32) (result i32)
    (local $wide i64)
    (local.set $wide
      (i64.add (i64.extend_i32_s (local.get $left)) (i64.extend_i32_s (local.get $right))))
    (if (i32.or
      (i64.lt_s (local.get $wide) (i64.const -2147483648))
      (i64.gt_s (local.get $wide) (i64.const 2147483647)))
      (then (call $hd.panic (global.get $hd.panic-integer-overflow)) unreachable))
    (i32.wrap_i64 (local.get $wide)))

  (func $hd.sub_i32 (param $left i32) (param $right i32) (result i32)
    (local $wide i64)
    (local.set $wide
      (i64.sub (i64.extend_i32_s (local.get $left)) (i64.extend_i32_s (local.get $right))))
    (if (i32.or
      (i64.lt_s (local.get $wide) (i64.const -2147483648))
      (i64.gt_s (local.get $wide) (i64.const 2147483647)))
      (then (call $hd.panic (global.get $hd.panic-integer-overflow)) unreachable))
    (i32.wrap_i64 (local.get $wide)))

  (func $hd.mul_i32 (param $left i32) (param $right i32) (result i32)
    (local $wide i64)
    (local.set $wide
      (i64.mul (i64.extend_i32_s (local.get $left)) (i64.extend_i32_s (local.get $right))))
    (if (i32.or
      (i64.lt_s (local.get $wide) (i64.const -2147483648))
      (i64.gt_s (local.get $wide) (i64.const 2147483647)))
      (then (call $hd.panic (global.get $hd.panic-integer-overflow)) unreachable))
    (i32.wrap_i64 (local.get $wide)))

  (func $hd.pow_i32 (param $base i32) (param $exponent i32) (result i32)
    (local $result i32)
    (if (i32.lt_s (local.get $exponent) (i32.const 0)) (then unreachable))
    (local.set $result (i32.const 1))
    (block $done
      (loop $next
        (br_if $done (i32.eqz (local.get $exponent)))
        (if (i32.and (local.get $exponent) (i32.const 1))
          (then (local.set $result (call $hd.mul_i32 (local.get $result) (local.get $base)))))
        (local.set $exponent (i32.shr_u (local.get $exponent) (i32.const 1)))
        (if (local.get $exponent)
          (then (local.set $base (call $hd.mul_i32 (local.get $base) (local.get $base)))))
        (br $next)))
    (local.get $result))

  (func $hd.neg_i32 (param $value i32) (result i32)
    (if (i32.eq (local.get $value) (i32.const -2147483648))
      (then (call $hd.panic (global.get $hd.panic-integer-overflow)) unreachable))
    (i32.sub (i32.const 0) (local.get $value)))

  (func $hd.shl_i32 (param $value i32) (param $count i32) (result i32)
    (if (i32.ge_u (local.get $count) (i32.const 32))
      (then (call $hd.panic (global.get $hd.panic-invalid-shift)) unreachable))
    (i32.shl (local.get $value) (local.get $count)))

  (func $hd.shr_i32 (param $value i32) (param $count i32) (result i32)
    (if (i32.ge_u (local.get $count) (i32.const 32))
      (then (call $hd.panic (global.get $hd.panic-invalid-shift)) unreachable))
    (i32.shr_s (local.get $value) (local.get $count)))

  (func $hd.string_len (param $value (ref null $hd.bytes)) (result i32)
    (local $bytes (ref $hd.bytes))
    (local $index i32)
    (local $length i32)
    (local $scalars i32)
    (local.set $bytes (ref.as_non_null (local.get $value)))
    (local.set $length (array.len (local.get $bytes)))
    (block $done
      (loop $next
        (br_if $done (i32.ge_u (local.get $index) (local.get $length)))
        (if
          (i32.ne
            (i32.and
              (array.get_u $hd.bytes (local.get $bytes) (local.get $index))
              (i32.const 192))
            (i32.const 128))
          (then
            (local.set $scalars
              (i32.add (local.get $scalars) (i32.const 1)))))
        (local.set $index (i32.add (local.get $index) (i32.const 1)))
        (br $next)))
    (local.get $scalars))

  (func $hd.string_concat
    (param $left-value (ref null $hd.bytes))
    (param $right-value (ref null $hd.bytes))
    (result (ref null $hd.bytes))
    (local $left (ref $hd.bytes))
    (local $right (ref $hd.bytes))
    (local $result (ref $hd.bytes))
    (local $left-length i32)
    (local $length i32)
    (local $index i32)
    (local.set $left (ref.as_non_null (local.get $left-value)))
    (local.set $right (ref.as_non_null (local.get $right-value)))
    (local.set $left-length (array.len (local.get $left)))
    (local.set $length
      (i32.add (local.get $left-length) (array.len (local.get $right))))
    (if (i32.lt_u (local.get $length) (local.get $left-length)) (then unreachable))
    (local.set $result (array.new_default $hd.bytes (local.get $length)))
    (block $left-done
      (loop $copy-left
        (br_if $left-done (i32.ge_u (local.get $index) (local.get $left-length)))
        (array.set $hd.bytes
          (local.get $result)
          (local.get $index)
          (array.get_u $hd.bytes (local.get $left) (local.get $index)))
        (local.set $index (i32.add (local.get $index) (i32.const 1)))
        (br $copy-left)))
    (block $right-done
      (loop $copy-right
        (br_if $right-done (i32.ge_u (local.get $index) (local.get $length)))
        (array.set $hd.bytes
          (local.get $result)
          (local.get $index)
          (array.get_u $hd.bytes
            (local.get $right)
            (i32.sub (local.get $index) (local.get $left-length))))
        (local.set $index (i32.add (local.get $index) (i32.const 1)))
        (br $copy-right)))
    (local.get $result))

  (func $hd.i32_to_string (param $value i32) (result (ref null $hd.bytes))
    (local $magnitude i64)
    (local $remaining i64)
    (local $negative i32)
    (local $digits i32)
    (local $length i32)
    (local $index i32)
    (local $result (ref $hd.bytes))
    (local.set $magnitude (i64.extend_i32_s (local.get $value)))
    (if (i64.lt_s (local.get $magnitude) (i64.const 0))
      (then
        (local.set $negative (i32.const 1))
        (local.set $magnitude (i64.sub (i64.const 0) (local.get $magnitude)))))
    (local.set $remaining (local.get $magnitude))
    (local.set $digits (i32.const 1))
    (block $counted
      (loop $count
        (br_if $counted (i64.lt_u (local.get $remaining) (i64.const 10)))
        (local.set $remaining (i64.div_u (local.get $remaining) (i64.const 10)))
        (local.set $digits (i32.add (local.get $digits) (i32.const 1)))
        (br $count)))
    (local.set $length (i32.add (local.get $digits) (local.get $negative)))
    (local.set $index (local.get $length))
    (local.set $result (array.new_default $hd.bytes (local.get $length)))
    (local.set $remaining (local.get $magnitude))
    (block $written
      (loop $write
        (local.set $index (i32.sub (local.get $index) (i32.const 1)))
        (array.set $hd.bytes
          (local.get $result)
          (local.get $index)
          (i32.add (i32.wrap_i64 (i64.rem_u (local.get $remaining) (i64.const 10))) (i32.const 48)))
        (local.set $remaining (i64.div_u (local.get $remaining) (i64.const 10)))
        (br_if $write (i32.gt_u (local.get $index) (local.get $negative)))))
    (if (local.get $negative)
      (then (array.set $hd.bytes (local.get $result) (i32.const 0) (i32.const 45))))
    (local.get $result))

  (func $hd.char_to_string (param $value i32) (result (ref null $hd.bytes))
    (local $result (ref $hd.bytes))
    (if (result (ref null $hd.bytes)) (i32.le_u (local.get $value) (i32.const 127))
      (then (array.new_fixed $hd.bytes 1 (local.get $value)))
      (else
        (if (result (ref null $hd.bytes)) (i32.le_u (local.get $value) (i32.const 2047))
          (then
            (array.new_fixed $hd.bytes 2
              (i32.or (i32.const 192) (i32.shr_u (local.get $value) (i32.const 6)))
              (i32.or (i32.const 128) (i32.and (local.get $value) (i32.const 63)))))
          (else
            (if (result (ref null $hd.bytes)) (i32.le_u (local.get $value) (i32.const 65535))
              (then
                (array.new_fixed $hd.bytes 3
                  (i32.or (i32.const 224) (i32.shr_u (local.get $value) (i32.const 12)))
                  (i32.or (i32.const 128) (i32.and (i32.shr_u (local.get $value) (i32.const 6)) (i32.const 63)))
                  (i32.or (i32.const 128) (i32.and (local.get $value) (i32.const 63)))))
              (else
                (array.new_fixed $hd.bytes 4
                  (i32.or (i32.const 240) (i32.shr_u (local.get $value) (i32.const 18)))
                  (i32.or (i32.const 128) (i32.and (i32.shr_u (local.get $value) (i32.const 12)) (i32.const 63)))
                  (i32.or (i32.const 128) (i32.and (i32.shr_u (local.get $value) (i32.const 6)) (i32.const 63)))
                  (i32.or (i32.const 128) (i32.and (local.get $value) (i32.const 63)))))))))))

  (func $hd.string_starts_with
    (param $value-source (ref null $hd.bytes))
    (param $prefix-source (ref null $hd.bytes))
    (result i32)
    (local $value (ref $hd.bytes))
    (local $prefix (ref $hd.bytes))
    (local $index i32)
    (local.set $value (ref.as_non_null (local.get $value-source)))
    (local.set $prefix (ref.as_non_null (local.get $prefix-source)))
    (if (i32.gt_u (array.len (local.get $prefix)) (array.len (local.get $value)))
      (then (return (i32.const 0))))
    (block $matched
      (loop $next
        (br_if $matched (i32.ge_u (local.get $index) (array.len (local.get $prefix))))
        (if
          (i32.ne
            (array.get_u $hd.bytes (local.get $value) (local.get $index))
            (array.get_u $hd.bytes (local.get $prefix) (local.get $index)))
          (then (return (i32.const 0))))
        (local.set $index (i32.add (local.get $index) (i32.const 1)))
        (br $next)))
    (i32.const 1))

  (func $hd.string_compare
    (param $left-value (ref null $hd.bytes))
    (param $right-value (ref null $hd.bytes))
    (result i32)
    (local $left (ref $hd.bytes))
    (local $right (ref $hd.bytes))
    (local $index i32)
    (local $limit i32)
    (local $left-byte i32)
    (local $right-byte i32)
    (local.set $left (ref.as_non_null (local.get $left-value)))
    (local.set $right (ref.as_non_null (local.get $right-value)))
    (local.set $limit
      (if (result i32)
        (i32.lt_u (array.len (local.get $left)) (array.len (local.get $right)))
        (then (array.len (local.get $left)))
        (else (array.len (local.get $right)))))
    (block $different
      (loop $next-byte
        (br_if $different (i32.ge_u (local.get $index) (local.get $limit)))
        (local.set $left-byte (array.get_u $hd.bytes (local.get $left) (local.get $index)))
        (local.set $right-byte (array.get_u $hd.bytes (local.get $right) (local.get $index)))
        (if (i32.lt_u (local.get $left-byte) (local.get $right-byte)) (then (return (i32.const -1))))
        (if (i32.gt_u (local.get $left-byte) (local.get $right-byte)) (then (return (i32.const 1))))
        (local.set $index (i32.add (local.get $index) (i32.const 1)))
        (br $next-byte)))
    (if (result i32)
      (i32.lt_u (array.len (local.get $left)) (array.len (local.get $right)))
      (then (i32.const -1))
      (else
        (if (result i32)
          (i32.gt_u (array.len (local.get $left)) (array.len (local.get $right)))
          (then (i32.const 1))
          (else (i32.const 0))))))


(func $hd.suspension_poll (param $frame (ref null $hd.suspension)) (result i32)
  (call_ref $hd.suspension-poll-sig
    (struct.get $hd.suspension $hd.suspension-inner (local.get $frame))
    (struct.get $hd.suspension $hd.suspension-poll (local.get $frame))))

(func $hd.suspension_cancel (param $frame (ref null $hd.suspension))
  (call_ref $hd.suspension-cancel-sig
    (struct.get $hd.suspension $hd.suspension-inner (local.get $frame))
    (struct.get $hd.suspension $hd.suspension-cancel (local.get $frame))))

(func $hd.suspension_result (param $frame (ref null $hd.suspension)) (result anyref)
  (call_ref $hd.suspension-result-sig
    (struct.get $hd.suspension $hd.suspension-inner (local.get $frame))
    (struct.get $hd.suspension $hd.suspension-result (local.get $frame))))

(func $hd.suspension_drive (param $frame (ref null $hd.suspension)) (result anyref)
  (if (global.get $hd.driver-active)
    (then (call $hd.panic (i32.const 6)) unreachable))
  (global.set $hd.driver-active (i32.const 1))
  (block $ready
    (loop $drive
      (br_if $ready (i32.eq (call $hd.suspension_poll (local.get $frame)) (i32.const 1)))
      (br $drive)))
  (global.set $hd.driver-active (i32.const 0))
  (call $hd.suspension_result (local.get $frame)))

  (func $hd.map_key_equal
    (param $kind i32)
    (param $left anyref)
    (param $right anyref)
    (result i32)
    (if (result i32)
      (i32.eqz (local.get $kind))
      (then
        (i32.eq
          (struct.get $hd.box-i32 $hd.box-i32-value
            (ref.cast (ref $hd.box-i32) (local.get $left)))
          (struct.get $hd.box-i32 $hd.box-i32-value
            (ref.cast (ref $hd.box-i32) (local.get $right)))))
      (else
        (i32.eqz
          (call $hd.string_compare
            (ref.cast (ref $hd.bytes) (local.get $left))
            (ref.cast (ref $hd.bytes) (local.get $right)))))))

  (func $hd.map_insert
    (param $map (ref $hd.map))
    (param $key anyref)
    (param $value anyref)
    (local $index i32)
    (local $size i32)
    (local $capacity i32)
    (local $new-keys (ref $hd.list))
    (local $new-values (ref $hd.list))
    (local.set $size
      (struct.get $hd.map $hd.map-size (local.get $map)))
    (block $append
      (loop $scan
        (br_if $append
          (i32.ge_u (local.get $index) (local.get $size)))
        (if
          (call $hd.map_key_equal
            (struct.get $hd.map $hd.map-key-kind (local.get $map))
            (array.get $hd.list
              (struct.get $hd.map $hd.map-keys (local.get $map))
              (local.get $index))
            (local.get $key))
          (then
            (array.set $hd.list
              (struct.get $hd.map $hd.map-values (local.get $map))
              (local.get $index)
              (local.get $value))
            (return)))
        (local.set $index
          (i32.add (local.get $index) (i32.const 1)))
        (br $scan)))
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
          (local.get $new-values))))
    (array.set $hd.list
      (struct.get $hd.map $hd.map-keys (local.get $map))
      (local.get $size)
      (local.get $key))
    (array.set $hd.list
      (struct.get $hd.map $hd.map-values (local.get $map))
      (local.get $size)
      (local.get $value))
    (struct.set $hd.map $hd.map-size
      (local.get $map)
      (i32.add (local.get $size) (i32.const 1)))
    (struct.set $hd.map $hd.map-version
      (local.get $map)
      (i32.add
        (struct.get $hd.map $hd.map-version (local.get $map))
        (i32.const 1))))

  (func $hd.map_get
    (param $map (ref $hd.map))
    (param $key anyref)
    (result (ref $hd.variant))
    (local $index i32)
    (local.set $index
      (struct.get $hd.map $hd.map-size (local.get $map)))
    (block $missing
      (loop $scan
        (br_if $missing (i32.eqz (local.get $index)))
        (local.set $index
          (i32.sub (local.get $index) (i32.const 1)))
        (if
          (call $hd.map_key_equal
            (struct.get $hd.map $hd.map-key-kind (local.get $map))
            (array.get $hd.list
              (struct.get $hd.map $hd.map-keys (local.get $map))
              (local.get $index))
            (local.get $key))
          (then
            (return
              (struct.new $hd.variant
                (i32.const 1)
                (array.get $hd.list
                  (struct.get $hd.map $hd.map-values (local.get $map))
                  (local.get $index))))))
        (br $scan)))
    (struct.new $hd.variant (i32.const 0) (ref.null any)))

  (func $hd.map_remove
    (param $map (ref $hd.map))
    (param $key anyref)
    (result (ref $hd.variant))
    (local $index i32)
    (local $size i32)
    (local $removed anyref)
    (local.set $size
      (struct.get $hd.map $hd.map-size (local.get $map)))
    (block $missing
      (loop $scan
        (br_if $missing
          (i32.ge_u (local.get $index) (local.get $size)))
        (if
          (call $hd.map_key_equal
            (struct.get $hd.map $hd.map-key-kind (local.get $map))
            (array.get $hd.list
              (struct.get $hd.map $hd.map-keys (local.get $map))
              (local.get $index))
            (local.get $key))
          (then
            (local.set $removed
              (array.get $hd.list
                (struct.get $hd.map $hd.map-values (local.get $map))
                (local.get $index)))
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
            (struct.set $hd.map $hd.map-version
              (local.get $map)
              (i32.add
                (struct.get $hd.map $hd.map-version (local.get $map))
                (i32.const 1)))
            (return
              (struct.new $hd.variant
                (i32.const 1)
                (local.get $removed)))))
        (local.set $index
          (i32.add (local.get $index) (i32.const 1)))
        (br $scan)))
    (struct.new $hd.variant (i32.const 0) (ref.null any)))

  (elem declare func $fv1 $fv2 $tadapt0_0)

  (func $f0 (param $l0 (ref null $hd.vector)) (param $bound0 (ref null $trait0)) (result i32)
    (local $l1 i32)
    (local $l2 i32)
    (local $tmp0 (ref null $trait0))
    (local $tmp1 i32)
    (local.set $l1 (i32.const 0))
    (local.set $l2 (i32.const 0))
    (block $break0
      (loop $loop0
        (br_if $break0 (i32.eqz (block (result i32) (local.set $tmp1 (if (result i32) (i32.lt_s (local.get $l2) (struct.get $hd.vector $hd.vector-size (ref.as_non_null (local.get $l0)))) (then (i32.const -1)) (else (if (result i32) (i32.gt_s (local.get $l2) (struct.get $hd.vector $hd.vector-size (ref.as_non_null (local.get $l0)))) (then (i32.const 1)) (else (i32.const 0)))))) (i32.eq (local.get $tmp1) (i32.const -1)))))
        (local.set $l1 (block (result i32)
          (local.set $tmp0 (struct.new $trait0 (call $hd.vector_get (ref.as_non_null (local.get $l0)) (local.get $l2)) (struct.get $trait0 $trait0bounds (local.get $bound0)) (struct.get $trait0 $trait0m0 (local.get $bound0))))
          (call_ref $tsig0_0
          (struct.get $trait0 $trait0value (local.get $tmp0))
          (local.get $tmp0)
          (local.get $l1)
          (struct.get $trait0 $trait0m0 (local.get $tmp0)))
        ))
        (local.set $l2 (call $hd.add_i32 (local.get $l2) (i32.const 1)))
        (br $loop0)
      )
    )
    (local.get $l1)
  )

  (func $f1 (export "main") (result i32)
    (local $l0 (ref null $hd.vector))
    (local $l1 i32)
    (local $l2 i32)
    (local $l3 i32)
    (local $tmp0 i32)
    (local $tmp1 i32)
    (local $tmp2 i32)
    (local $tmp3 i32)
    (local.set $l0 (struct.new $hd.vector (i32.const 0) (array.new_default $hd.list (i32.const 0)) (i32.const 0)))
    (local.set $l1 (i32.const 0))
    (block $break1
      (loop $loop1
        (br_if $break1 (i32.eqz (block (result i32) (local.set $tmp0 (if (result i32) (i32.lt_s (local.get $l1) (i32.const 1000)) (then (i32.const -1)) (else (if (result i32) (i32.gt_s (local.get $l1) (i32.const 1000)) (then (i32.const 1)) (else (i32.const 0)))))) (i32.eq (local.get $tmp0) (i32.const -1)))))
        (call $hd.vector_append (ref.as_non_null (local.get $l0)) (struct.new $hd.box-i32 (local.get $l1)))
        (local.set $l1 (call $hd.add_i32 (local.get $l1) (i32.const 1)))
        (br $loop1)
      )
    )
    (local.set $l2 (i32.const 0))
    (local.set $l3 (i32.const 0))
    (block $break2
      (loop $loop2
        (br_if $break2 (i32.eqz (block (result i32) (local.set $tmp3 (if (result i32) (i32.lt_s (local.get $l2) (i32.const 2000)) (then (i32.const -1)) (else (if (result i32) (i32.gt_s (local.get $l2) (i32.const 2000)) (then (i32.const 1)) (else (i32.const 0)))))) (i32.eq (local.get $tmp3) (i32.const -1)))))
        (local.set $l3 (block (result i32)
          (local.set $tmp1 (call $hd.add_i32 (local.get $l3) (call $f0 (local.get $l0) (struct.new $trait0 (ref.null any) (ref.null $hd.list) (ref.func $tadapt0_0)))))
          (local.set $tmp2 (i32.const 1000003))
          (if (i32.eqz (local.get $tmp2))
            (then (call $hd.panic (i32.const 1)) unreachable))
          (i32.rem_s (local.get $tmp1) (local.get $tmp2))
        ))
        (local.set $l2 (call $hd.add_i32 (local.get $l2) (i32.const 1)))
        (br $loop2)
      )
    )
    (local.get $l3)
  )

  (func $fv1 (type $sig0) (param $env anyref)  (result i32)
    (call $f1)
  )

  (func $f2 (param $l0 i32) (param $l1 i32) (result i32)
    (local $tmp0 i32)
    (local $tmp1 i32)
    (block (result i32)
      (local.set $tmp0 (call $hd.add_i32 (local.get $l0) (local.get $l1)))
      (local.set $tmp1 (i32.const 1000003))
      (if (i32.eqz (local.get $tmp1))
        (then (call $hd.panic (i32.const 1)) unreachable))
      (i32.rem_s (local.get $tmp0) (local.get $tmp1))
    )
  )

  (func $fv2 (type $sig1) (param $env anyref) (param $l0 i32) (param $l1 i32) (result i32)
    (call $f2 (local.get $l0) (local.get $l1))
  )

  (func $tadapt0_0 (type $tsig0_0) (param $self anyref) (param $dictionary anyref) (param $a0 i32) (result i32)
    (call $f2 (struct.get $hd.box-i32 $hd.box-i32-value (ref.cast (ref $hd.box-i32) (local.get $self))) (local.get $a0))
  )
)