;; commit bd985d7; hd build --wat b4-suspend.hd; 2026-09-25T20:54:16.142Z
(module
  (import "hd" "trace" (func $hd.trace (param i32 i32)))
  (import "hd" "pending" (func $hd.pending (param i32 i32) (result i32)))
  (import "hd" "panic" (func $hd.panic (param i32)))
  (rec
    (type $sig0 (func (param anyref) (param i32) (result (ref null $hd.suspension))))
    (type $sig1 (func (param anyref) (result (ref null $hd.suspension))))
    (type $tsig0_0 (func (param anyref) (param anyref) (result (ref null $hd.bytes))))
    (type $tsig1_0 (func (param anyref) (param anyref) (param anyref) (result i32)))
    (type $tsig2_0 (func (param anyref) (param anyref) (param anyref) (result (ref null $hd.variant))))
    (type $tsig3_0 (func (param anyref) (param anyref)))
    (type $tsig4_0 (func (param anyref) (param anyref) (result (ref null $hd.variant))))
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
    (type $s0 (struct
      (field $s0state (mut i32))
      (field $s0polls (mut i32))
      (field $s0a0 i32)
      (field $s0result (mut i32))))
    (type $s1 (struct
      (field $s1state (mut i32))
      (field $s1polls (mut i32))
      (field $s1l0 (mut i32))
      (field $s1l1 (mut i32))
      (field $s1l2 (mut i32))
      (field $s1l3 (mut i32))
      (field $s1l4 (mut i32))
      (field $s1l5 (mut i32))
      (field $s1l6 (mut i32))
      (field $s1l7 (mut i32))
      (field $s1l8 (mut i32))
      (field $s1l9 (mut i32))
      (field $s1l10 (mut i32))
      (field $s1l11 (mut i32))
      (field $s1l12 (mut i32))
      (field $s1l13 (mut i32))
      (field $s1l14 (mut i32))
      (field $s1child0 (mut (ref null $s0)))
      (field $s1result (mut i32))))


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

  (elem declare func $fv0 $fv1 $swpoll0 $swcancel0 $swresult0 $swpoll1 $swcancel1 $swresult1)

  (func $body0 (param $l0 i32) (result i32)
    (call $hd.add_i32 (local.get $l0) (i32.const 1))
  )

  (func $f0 (param $l0 i32) (result (ref null $s0))
    (call $hd.trace (i32.const 0) (i32.const 0))
    (struct.new $s0 (i32.const 0) (i32.const 0) (local.get $l0) (i32.const 0))
  )
  
  (func $poll0 (param $frame (ref null $s0)) (result i32)
    (call $hd.trace (i32.const 0) (i32.const 1))
    (if (i32.eq (struct.get $s0 $s0state (local.get $frame)) (i32.const 1))
      (then (call $hd.trace (i32.const 0) (i32.const 4)) (call $hd.panic (i32.const 5)) unreachable))
    (if (i32.or
          (i32.eq (struct.get $s0 $s0state (local.get $frame)) (i32.const 2))
          (i32.eq (struct.get $s0 $s0state (local.get $frame)) (i32.const 3)))
      (then (call $hd.trace (i32.const 0) (i32.const 5)) (call $hd.panic (i32.const 4)) unreachable))
    (struct.set $s0 $s0polls (local.get $frame)
      (i32.add (struct.get $s0 $s0polls (local.get $frame)) (i32.const 1)))
    (struct.set $s0 $s0state (local.get $frame) (i32.const 1))
    (if (call $hd.pending
          (i32.const 0)
          (struct.get $s0 $s0polls (local.get $frame)))
      (then
        (struct.set $s0 $s0state (local.get $frame) (i32.const 4))
        (call $hd.trace (i32.const 0) (i32.const 6))
        (return (i32.const 0))))
    (struct.set $s0 $s0state (local.get $frame) (i32.const 1))
    (struct.set $s0 $s0result (local.get $frame) (call $body0 (struct.get $s0 $s0a0 (local.get $frame))))
    (struct.set $s0 $s0state (local.get $frame) (i32.const 2))
    (call $hd.trace (i32.const 0) (i32.const 2))
    (i32.const 1)
  )
  
  (func $drive0 (param $frame (ref null $s0)) (result i32)
    (if (global.get $hd.driver-active) (then (call $hd.panic (i32.const 6)) unreachable))
    (global.set $hd.driver-active (i32.const 1))
    (block $ready
      (loop $drive
        (br_if $ready (i32.eq (call $poll0 (local.get $frame)) (i32.const 1)))
        (br $drive)))
    (global.set $hd.driver-active (i32.const 0))
    (struct.get $s0 $s0result (local.get $frame))
  )
  
  (func $cancel0 (param $frame (ref null $s0))
    (call $hd.trace (i32.const 0) (i32.const 3))
    (if (i32.eq (struct.get $s0 $s0state (local.get $frame)) (i32.const 1))
      (then (call $hd.trace (i32.const 0) (i32.const 4)) (call $hd.panic (i32.const 5)) unreachable))
    (if (i32.or
          (i32.eq (struct.get $s0 $s0state (local.get $frame)) (i32.const 0))
          (i32.eq (struct.get $s0 $s0state (local.get $frame)) (i32.const 4)))
      (then (struct.set $s0 $s0state (local.get $frame) (i32.const 3))))
  )

  (func $fv0 (type $sig0) (param $env anyref) (param $l0 i32) (result (ref null $hd.suspension))
    (struct.new $hd.suspension (call $f0 (local.get $l0)) (ref.func $swpoll0) (ref.func $swcancel0) (ref.func $swresult0))
  )

  (func $f1 (result (ref null $s1))
    (call $hd.trace (i32.const 1) (i32.const 0))
    (struct.new $s1 (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (ref.null $s0) (i32.const 0))
  )
  
  (func $poll1 (param $frame (ref null $s1)) (result i32)
    (local $resume-state i32)
    (local $pc i32)
    (local $l0 i32)
    (local $l1 i32)
    (local $l2 i32)
    (local $l3 i32)
    (local $l4 i32)
    (local $l5 i32)
    (local $l6 i32)
    (local $l7 i32)
    (local $l8 i32)
    (local $l9 i32)
    (local $l10 i32)
    (local $l11 i32)
    (local $l12 i32)
    (local $l13 i32)
    (local $l14 i32)
    (local $tmp0 i32)
    (local $tmp1 i32)
    (local $tmp2 i32)
    (local $tmp3 i32)
    (local $tmp4 i32)
    (call $hd.trace (i32.const 1) (i32.const 1))
    (local.set $resume-state (struct.get $s1 $s1state (local.get $frame)))
    (if (i32.eq (local.get $resume-state) (i32.const 1))
      (then (call $hd.trace (i32.const 1) (i32.const 4)) (call $hd.panic (i32.const 5)) unreachable))
    (if (i32.or (i32.eq (local.get $resume-state) (i32.const 2)) (i32.eq (local.get $resume-state) (i32.const 3)))
      (then (call $hd.trace (i32.const 1) (i32.const 5)) (call $hd.panic (i32.const 4)) unreachable))
    (struct.set $s1 $s1polls (local.get $frame)
      (i32.add (struct.get $s1 $s1polls (local.get $frame)) (i32.const 1)))
    (struct.set $s1 $s1state (local.get $frame) (i32.const 1))
    (if (call $hd.pending (i32.const 1) (struct.get $s1 $s1polls (local.get $frame)))
      (then
        (struct.set $s1 $s1state (local.get $frame)
          (if (result i32) (i32.eq (local.get $resume-state) (i32.const 0))
            (then (i32.const 4))
            (else (local.get $resume-state))))
        (call $hd.trace (i32.const 1) (i32.const 6))
        (return (i32.const 0))))
    (local.set $l0 (struct.get $s1 $s1l0 (local.get $frame)))
    (local.set $l1 (struct.get $s1 $s1l1 (local.get $frame)))
    (local.set $l2 (struct.get $s1 $s1l2 (local.get $frame)))
    (local.set $l3 (struct.get $s1 $s1l3 (local.get $frame)))
    (local.set $l4 (struct.get $s1 $s1l4 (local.get $frame)))
    (local.set $l5 (struct.get $s1 $s1l5 (local.get $frame)))
    (local.set $l6 (struct.get $s1 $s1l6 (local.get $frame)))
    (local.set $l7 (struct.get $s1 $s1l7 (local.get $frame)))
    (local.set $l8 (struct.get $s1 $s1l8 (local.get $frame)))
    (local.set $l9 (struct.get $s1 $s1l9 (local.get $frame)))
    (local.set $l10 (struct.get $s1 $s1l10 (local.get $frame)))
    (local.set $l11 (struct.get $s1 $s1l11 (local.get $frame)))
    (local.set $l12 (struct.get $s1 $s1l12 (local.get $frame)))
    (local.set $l13 (struct.get $s1 $s1l13 (local.get $frame)))
    (local.set $l14 (struct.get $s1 $s1l14 (local.get $frame)))
    (struct.set $s1 $s1state (local.get $frame) (i32.const 1))
    (if (i32.le_u (local.get $resume-state) (i32.const 4))
      (then (local.set $pc (i32.const 20)))
      (else
        (if (i32.eq (local.get $resume-state) (i32.const 5))
          (then
            (if (i32.eqz (call $poll0 (struct.get $s1 $s1child0 (local.get $frame))))
              (then
                (struct.set $s1 $s1l0 (local.get $frame) (local.get $l0))
                (struct.set $s1 $s1l1 (local.get $frame) (local.get $l1))
                (struct.set $s1 $s1l2 (local.get $frame) (local.get $l2))
                (struct.set $s1 $s1l3 (local.get $frame) (local.get $l3))
                (struct.set $s1 $s1l4 (local.get $frame) (local.get $l4))
                (struct.set $s1 $s1l5 (local.get $frame) (local.get $l5))
                (struct.set $s1 $s1l6 (local.get $frame) (local.get $l6))
                (struct.set $s1 $s1l7 (local.get $frame) (local.get $l7))
                (struct.set $s1 $s1l8 (local.get $frame) (local.get $l8))
                (struct.set $s1 $s1l9 (local.get $frame) (local.get $l9))
                (struct.set $s1 $s1l10 (local.get $frame) (local.get $l10))
                (struct.set $s1 $s1l11 (local.get $frame) (local.get $l11))
                (struct.set $s1 $s1l12 (local.get $frame) (local.get $l12))
                (struct.set $s1 $s1l13 (local.get $frame) (local.get $l13))
                (struct.set $s1 $s1l14 (local.get $frame) (local.get $l14))
                (struct.set $s1 $s1state (local.get $frame) (local.get $resume-state))
                (call $hd.trace (i32.const 1) (i32.const 6))
                (return (i32.const 0))))
            (local.set $l7 (struct.get $s0 $s0result (struct.get $s1 $s1child0 (local.get $frame))))
            (local.set $pc (i32.const 10))
          )
          (else
            (local.set $pc (i32.const 20))
          ))
      ))
    (loop $cfg
      (if (i32.eq (local.get $pc) (i32.const 0))
        (then
          (local.set $l2 (local.get $l1))
          (struct.set $s1 $s1result (local.get $frame) (local.get $l2))
          (struct.set $s1 $s1state (local.get $frame) (i32.const 2))
          (call $hd.trace (i32.const 1) (i32.const 2))
          (return (i32.const 1))
        ))
      (if (i32.eq (local.get $pc) (i32.const 1))
        (then
          (local.set $pc (i32.const 0))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 2))
        (then
          (local.set $pc (i32.const 18))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 3))
        (then
          (local.set $pc (i32.const 2))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 4))
        (then
          (local.set $l0 (call $hd.add_i32 (local.get $l11) (local.get $l12)))
          (local.set $pc (i32.const 3))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 5))
        (then
          (local.set $l12 (i32.const 1))
          (local.set $pc (i32.const 4))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 6))
        (then
          (local.set $l11 (local.get $l0))
          (local.set $pc (i32.const 5))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 7))
        (then
          (local.set $l1 (block (result i32)
            (local.set $tmp0 (local.get $l9))
            (local.set $tmp1 (local.get $l10))
            (if (i32.eqz (local.get $tmp1))
              (then (call $hd.panic (i32.const 1)) unreachable))
            (i32.rem_s (local.get $tmp0) (local.get $tmp1))
          ))
          (local.set $pc (i32.const 6))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 8))
        (then
          (local.set $l10 (i32.const 1000003))
          (local.set $pc (i32.const 7))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 9))
        (then
          (local.set $l9 (call $hd.add_i32 (local.get $l3) (local.get $l8)))
          (local.set $pc (i32.const 8))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 10))
        (then
          (local.set $l8 (local.get $l7))
          (local.set $pc (i32.const 9))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 11))
        (then
          (struct.set $s1 $s1child0 (local.get $frame) (call $f0 (local.get $l6)))
          (if (i32.eqz (call $poll0 (struct.get $s1 $s1child0 (local.get $frame))))
            (then
              (struct.set $s1 $s1l0 (local.get $frame) (local.get $l0))
              (struct.set $s1 $s1l1 (local.get $frame) (local.get $l1))
              (struct.set $s1 $s1l2 (local.get $frame) (local.get $l2))
              (struct.set $s1 $s1l3 (local.get $frame) (local.get $l3))
              (struct.set $s1 $s1l4 (local.get $frame) (local.get $l4))
              (struct.set $s1 $s1l5 (local.get $frame) (local.get $l5))
              (struct.set $s1 $s1l6 (local.get $frame) (local.get $l6))
              (struct.set $s1 $s1l7 (local.get $frame) (local.get $l7))
              (struct.set $s1 $s1l8 (local.get $frame) (local.get $l8))
              (struct.set $s1 $s1l9 (local.get $frame) (local.get $l9))
              (struct.set $s1 $s1l10 (local.get $frame) (local.get $l10))
              (struct.set $s1 $s1l11 (local.get $frame) (local.get $l11))
              (struct.set $s1 $s1l12 (local.get $frame) (local.get $l12))
              (struct.set $s1 $s1l13 (local.get $frame) (local.get $l13))
              (struct.set $s1 $s1l14 (local.get $frame) (local.get $l14))
              (struct.set $s1 $s1state (local.get $frame) (i32.const 5))
              (call $hd.trace (i32.const 1) (i32.const 6))
              (return (i32.const 0))))
          (local.set $l7 (struct.get $s0 $s0result (struct.get $s1 $s1child0 (local.get $frame))))
          (local.set $pc (i32.const 10))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 12))
        (then
          (local.set $l6 (block (result i32)
            (local.set $tmp2 (local.get $l4))
            (local.set $tmp3 (local.get $l5))
            (if (i32.eqz (local.get $tmp3))
              (then (call $hd.panic (i32.const 1)) unreachable))
            (i32.rem_s (local.get $tmp2) (local.get $tmp3))
          ))
          (local.set $pc (i32.const 11))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 13))
        (then
          (local.set $l5 (i32.const 7))
          (local.set $pc (i32.const 12))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 14))
        (then
          (local.set $l4 (local.get $l0))
          (local.set $pc (i32.const 13))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 15))
        (then
          (local.set $l3 (local.get $l1))
          (local.set $pc (i32.const 14))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 16))
        (then
          (if (block (result i32) (local.set $tmp4 (if (result i32) (i32.lt_s (local.get $l13) (local.get $l14)) (then (i32.const -1)) (else (if (result i32) (i32.gt_s (local.get $l13) (local.get $l14)) (then (i32.const 1)) (else (i32.const 0)))))) (i32.eq (local.get $tmp4) (i32.const -1)))
            (then (local.set $pc (i32.const 15)))
            (else (local.set $pc (i32.const 1))))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 17))
        (then
          (local.set $l14 (i32.const 200000))
          (local.set $pc (i32.const 16))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 18))
        (then
          (local.set $l13 (local.get $l0))
          (local.set $pc (i32.const 17))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 19))
        (then
          (local.set $l1 (i32.const 0))
          (local.set $pc (i32.const 2))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 20))
        (then
          (local.set $l0 (i32.const 0))
          (local.set $pc (i32.const 19))
          (br $cfg)
        ))
      unreachable
    )
  )
  
  (func $drive1 (param $frame (ref null $s1)) (result i32)
    (if (global.get $hd.driver-active) (then (call $hd.panic (i32.const 6)) unreachable))
    (global.set $hd.driver-active (i32.const 1))
    (block $ready
      (loop $drive
        (br_if $ready (i32.eq (call $poll1 (local.get $frame)) (i32.const 1)))
        (br $drive)))
    (global.set $hd.driver-active (i32.const 0))
    (struct.get $s1 $s1result (local.get $frame))
  )
  
  (func $cancel1 (param $frame (ref null $s1))
    (local $resume-state i32)
    (local $l0 i32)
    (local $l1 i32)
    (local $l2 i32)
    (local $l3 i32)
    (local $l4 i32)
    (local $l5 i32)
    (local $l6 i32)
    (local $l7 i32)
    (local $l8 i32)
    (local $l9 i32)
    (local $l10 i32)
    (local $l11 i32)
    (local $l12 i32)
    (local $l13 i32)
    (local $l14 i32)
    (local $tmp0 i32)
    (local $tmp1 i32)
    (local $tmp2 i32)
    (local $tmp3 i32)
    (local $tmp4 i32)
    (call $hd.trace (i32.const 1) (i32.const 3))
    (local.set $resume-state (struct.get $s1 $s1state (local.get $frame)))
    (if (i32.eq (local.get $resume-state) (i32.const 1))
      (then (call $hd.trace (i32.const 1) (i32.const 4)) (call $hd.panic (i32.const 5)) unreachable))
    (local.set $l0 (struct.get $s1 $s1l0 (local.get $frame)))
    (local.set $l1 (struct.get $s1 $s1l1 (local.get $frame)))
    (local.set $l2 (struct.get $s1 $s1l2 (local.get $frame)))
    (local.set $l3 (struct.get $s1 $s1l3 (local.get $frame)))
    (local.set $l4 (struct.get $s1 $s1l4 (local.get $frame)))
    (local.set $l5 (struct.get $s1 $s1l5 (local.get $frame)))
    (local.set $l6 (struct.get $s1 $s1l6 (local.get $frame)))
    (local.set $l7 (struct.get $s1 $s1l7 (local.get $frame)))
    (local.set $l8 (struct.get $s1 $s1l8 (local.get $frame)))
    (local.set $l9 (struct.get $s1 $s1l9 (local.get $frame)))
    (local.set $l10 (struct.get $s1 $s1l10 (local.get $frame)))
    (local.set $l11 (struct.get $s1 $s1l11 (local.get $frame)))
    (local.set $l12 (struct.get $s1 $s1l12 (local.get $frame)))
    (local.set $l13 (struct.get $s1 $s1l13 (local.get $frame)))
    (local.set $l14 (struct.get $s1 $s1l14 (local.get $frame)))
    (if (i32.eq (local.get $resume-state) (i32.const 5))
      (then
        (call $cancel0 (struct.get $s1 $s1child0 (local.get $frame)))
        (struct.set $s1 $s1state (local.get $frame) (i32.const 3))
        (return)))
    (if (i32.or (i32.eq (local.get $resume-state) (i32.const 0)) (i32.eq (local.get $resume-state) (i32.const 4)))
      (then (struct.set $s1 $s1state (local.get $frame) (i32.const 3))))
  )
  
  (func $entry1 (export "main") (result i32)
    (call $drive1 (call $f1))
  )
  
  (global $hd.dev-frame1 (mut (ref null $s1)) (ref.null $s1))
  (func (export "__hd_start")
    (if (global.get $hd.driver-active) (then (call $hd.panic (i32.const 6)) unreachable))
    (global.set $hd.driver-active (i32.const 1))
    (global.set $hd.dev-frame1 (call $f1))
  )
  (func (export "__hd_poll") (result i32)
    (local $ready i32)
    (local.set $ready (call $poll1 (global.get $hd.dev-frame1)))
    (if (local.get $ready) (then (global.set $hd.driver-active (i32.const 0))))
    (local.get $ready)
  )
  (func (export "__hd_cancel")
    (call $cancel1 (global.get $hd.dev-frame1))
    (global.set $hd.driver-active (i32.const 0))
  )
  (func (export "__hd_result") (result i32) (struct.get $s1 $s1result (global.get $hd.dev-frame1)))

  (func $fv1 (type $sig1) (param $env anyref)  (result (ref null $hd.suspension))
    (struct.new $hd.suspension (call $f1) (ref.func $swpoll1) (ref.func $swcancel1) (ref.func $swresult1))
  )

  (func $swpoll0 (type $hd.suspension-poll-sig) (param $inner anyref) (result i32)
    (call $poll0 (ref.cast (ref $s0) (local.get $inner))))
  
  (func $swcancel0 (type $hd.suspension-cancel-sig) (param $inner anyref)
    (call $cancel0 (ref.cast (ref $s0) (local.get $inner))))
  
  (func $swresult0 (type $hd.suspension-result-sig) (param $inner anyref) (result anyref)
    (struct.new $hd.box-i32 (struct.get $s0 $s0result (ref.cast (ref $s0) (local.get $inner)))))
  
  (func $swpoll1 (type $hd.suspension-poll-sig) (param $inner anyref) (result i32)
    (call $poll1 (ref.cast (ref $s1) (local.get $inner))))
  
  (func $swcancel1 (type $hd.suspension-cancel-sig) (param $inner anyref)
    (call $cancel1 (ref.cast (ref $s1) (local.get $inner))))
  
  (func $swresult1 (type $hd.suspension-result-sig) (param $inner anyref) (result anyref)
    (struct.new $hd.box-i32 (struct.get $s1 $s1result (ref.cast (ref $s1) (local.get $inner)))))
)