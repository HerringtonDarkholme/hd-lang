;; commit bd985d7; command: hd build --wat audit/probes/arch/requirements/constructs.hd; date: 2026-09-25T20:50:10Z
(module
  (import "hd" "trace" (func $hd.trace (param i32 i32)))
  (import "hd" "pending" (func $hd.pending (param i32 i32) (result i32)))
  (import "hd" "panic" (func $hd.panic (param i32)))
  (rec
    (type $sig0 (func (param anyref) (param (ref null $trait0)) (result i32)))
    (type $sig1 (func (param anyref) (param (ref null $trait1)) (param (ref null $trait0)) (result i32)))
    (type $sig2 (func (param anyref) (param (ref null $trait0)) (result i32)))
    (type $sig3 (func (param anyref) (result i32)))
    (type $sig4 (func (param anyref) (param (ref null $trait1)) (param (ref null $trait0)) (result (ref null $context0))))
    (type $sig5 (func (param anyref) (param (ref null $hd.providers)) (result i32)))
    (type $sig6 (func (param anyref) (param (ref null $closure5)) (param (ref null $trait0)) (param (ref null $hd.providers)) (result i32)))
    (type $sig7 (func (param anyref) (param (ref null $trait0)) (param (ref null $trait1)) (result i32)))
    (type $sig8 (func (param anyref) (param (ref null $trait0)) (result (ref null $closure3))))
    (type $sig9 (func (param anyref) (param (ref null $trait0)) (result (ref null $hd.suspension))))
    (type $sig10 (func (param anyref) (param (ref null $trait1)) (param (ref null $trait0)) (result (ref null $hd.suspension))))
    (type $sig11 (func (param anyref) (result (ref null $hd.suspension))))
    (type $sig12 (func (param anyref) (param (ref null $d0)) (result i32)))
    (type $tsig0_0 (func (param anyref) (param anyref) (result i32)))
    (type $tsig1_0 (func (param anyref) (param anyref) (result i32)))
    (type $tsig2_0 (func (param anyref) (param anyref) (result (ref null $hd.bytes))))
    (type $tsig3_0 (func (param anyref) (param anyref) (param anyref) (result i32)))
    (type $tsig4_0 (func (param anyref) (param anyref) (param anyref) (result (ref null $hd.variant))))
    (type $tsig5_0 (func (param anyref) (param anyref)))
    (type $tsig6_0 (func (param anyref) (param anyref) (result (ref null $hd.variant))))
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
    (type $closure2 (struct
      (field $closure2fn (ref $sig2))
      (field $closure2env anyref)))
    (type $closure3 (struct
      (field $closure3fn (ref $sig3))
      (field $closure3env anyref)))
    (type $closure4 (struct
      (field $closure4fn (ref $sig4))
      (field $closure4env anyref)))
    (type $closure5 (struct
      (field $closure5fn (ref $sig5))
      (field $closure5env anyref)))
    (type $closure6 (struct
      (field $closure6fn (ref $sig6))
      (field $closure6env anyref)))
    (type $closure7 (struct
      (field $closure7fn (ref $sig7))
      (field $closure7env anyref)))
    (type $closure8 (struct
      (field $closure8fn (ref $sig8))
      (field $closure8env anyref)))
    (type $closure9 (struct
      (field $closure9fn (ref $sig9))
      (field $closure9env anyref)))
    (type $closure10 (struct
      (field $closure10fn (ref $sig10))
      (field $closure10env anyref)))
    (type $closure11 (struct
      (field $closure11fn (ref $sig11))
      (field $closure11env anyref)))
    (type $closure12 (struct
      (field $closure12fn (ref $sig12))
      (field $closure12env anyref)))
    (type $context0 (struct
      (field $context0f0 (ref null $trait1))
      (field $context0f1 (ref null $trait0))))
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
    (type $trait6 (struct
      (field $trait6value anyref)
      (field $trait6bounds (ref null $hd.list))
      (field $trait6m0 (ref $tsig6_0))))
    (type $s9 (struct
      (field $s9state (mut i32))
      (field $s9polls (mut i32))
      (field $s9p0 (ref null $trait0))
      (field $s9result (mut i32))))
    (type $s10 (struct
      (field $s10state (mut i32))
      (field $s10polls (mut i32))
      (field $s10p0 (ref null $trait1))
      (field $s10p1 (ref null $trait0))
      (field $s10l0 (mut i32))
      (field $s10child0 (mut (ref null $s9)))
      (field $s10result (mut i32))))
    (type $s11 (struct
      (field $s11state (mut i32))
      (field $s11polls (mut i32))
      (field $s11l0 (mut (ref null $trait0)))
      (field $s11l1 (mut (ref null $trait1)))
      (field $s11l2 (mut (ref null $d0)))
      (field $s11l3 (mut (ref null $closure3)))
      (field $s11l4 (mut i32))
      (field $s11l5 (mut i32))
      (field $s11l6 (mut i32))
      (field $s11l7 (mut (ref null $trait0)))
      (field $s11l8 (mut (ref null $trait0)))
      (field $s11l9 (mut i32))
      (field $s11l10 (mut i32))
      (field $s11l11 (mut i32))
      (field $s11l12 (mut (ref null $trait1)))
      (field $s11l13 (mut (ref null $trait0)))
      (field $s11l14 (mut i32))
      (field $s11l15 (mut i32))
      (field $s11l16 (mut (ref null $trait0)))
      (field $s11l17 (mut (ref null $trait1)))
      (field $s11l18 (mut i32))
      (field $s11l19 (mut i32))
      (field $s11l20 (mut (ref null $closure3)))
      (field $s11l21 (mut i32))
      (field $s11l22 (mut i32))
      (field $s11l23 (mut (ref null $trait1)))
      (field $s11l24 (mut (ref null $trait0)))
      (field $s11l25 (mut i32))
      (field $s11l26 (mut i32))
      (field $s11l27 (mut i32))
      (field $s11child0 (mut (ref null $s10)))
      (field $s11result (mut i32))))
    (type $env0 (struct
      (field $env0f0 (ref null $trait0))))
    (type $d0 (struct
      (field $d0f0 (mut i32))))
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

  (elem declare func $c0 $fv0 $fv1 $fv2 $fv3 $fv4 $fv5 $fv6 $fv7 $fv8 $fv9 $fv10 $fv11 $fv12 $fv13 $adapt0 $tadapt0_0 $tadapt1_0 $swpoll9 $swcancel9 $swresult9 $swpoll10 $swcancel10 $swresult10 $swpoll11 $swcancel11 $swresult11)

  (func $f0 (export "read") (param $provider0 (ref null $trait0)) (result i32)
    (local $tmp0 (ref null $trait0))
    (block (result i32)
      (local.set $tmp0 (local.get $provider0))
      (call_ref $tsig0_0
      (struct.get $trait0 $trait0value (local.get $tmp0))
      (local.get $tmp0)
      (struct.get $trait0 $trait0m0 (local.get $tmp0)))
    )
  )

  (func $fv0 (type $sig0) (param $env anyref) (param $provider0 (ref null $trait0)) (result i32)
    (call $f0 (local.get $provider0))
  )

  (func $f1 (export "read_both") (param $provider0 (ref null $trait1)) (param $provider1 (ref null $trait0)) (result i32)
    (local $tmp0 (ref null $trait0))
    (local $tmp1 (ref null $trait1))
    (call $hd.add_i32 (block (result i32)
      (local.set $tmp0 (local.get $provider1))
      (call_ref $tsig0_0
      (struct.get $trait0 $trait0value (local.get $tmp0))
      (local.get $tmp0)
      (struct.get $trait0 $trait0m0 (local.get $tmp0)))
    ) (block (result i32)
      (local.set $tmp1 (local.get $provider0))
      (call_ref $tsig1_0
      (struct.get $trait1 $trait1value (local.get $tmp1))
      (local.get $tmp1)
      (struct.get $trait1 $trait1m0 (local.get $tmp1)))
    ))
  )

  (func $fv1 (type $sig1) (param $env anyref) (param $provider0 (ref null $trait1)) (param $provider1 (ref null $trait0)) (result i32)
    (call $f1 (local.get $provider0) (local.get $provider1))
  )

  (func $f2 (param $l0 (ref null $trait0)) (result i32)
    (local $l1 (ref null $trait0))
    (block (result i32)
      (local.set $l1 (local.get $l0))
      (call $f0 (local.get $l1))
    )
  )

  (func $fv2 (type $sig2) (param $env anyref) (param $l0 (ref null $trait0)) (result i32)
    (call $f2 (local.get $l0))
  )

  (func $f3 (export "with_construct") (result i32)
    (local $l0 (ref null $trait0))
    (local $tmp0 (ref null $d0))
    (local $tmp1 i32)
    (block (result i32)
      (local.set $l0 (block (result (ref null $trait0))
      (local.set $tmp0 (block (result (ref null $d0))
      (local.set $tmp1 (i32.const 1))
      (struct.new $d0 (local.get $tmp1))
    ))
      (struct.new $trait0 (local.get $tmp0) (ref.null $hd.list) (ref.func $tadapt0_0))
    ))
      (call $f0 (local.get $l0))
    )
  )

  (func $fv3 (type $sig3) (param $env anyref)  (result i32)
    (call $f3)
  )

  (func $f4 (param $provider0 (ref null $trait1)) (param $provider1 (ref null $trait0)) (result (ref null $context0))
    (local $l0 (ref null $trait0))
    (local $l1 (ref null $trait1))
    (block (result (ref null $context0))
      (local.set $l0 (local.get $provider1))
      (local.set $l1 (local.get $provider0))
      (struct.new $context0 (local.get $l1) (local.get $l0))
    )
  )

  (func $fv4 (type $sig4) (param $env anyref) (param $provider0 (ref null $trait1)) (param $provider1 (ref null $trait0)) (result (ref null $context0))
    (call $f4 (local.get $provider0) (local.get $provider1))
  )

  (func $f5 (export "with_spread") (param $provider0 (ref null $trait1)) (param $provider1 (ref null $trait0)) (result i32)
    (local $l0 (ref null $context0))
    (local $l1 (ref null $context0))
    (local $l2 (ref null $trait1))
    (local $l3 (ref null $trait0))
    (local.set $l0 (call $f4 (local.get $provider0) (local.get $provider1)))
    (block (result i32)
      (local.set $l1 (local.get $l0))
      (local.set $l2 (struct.get $context0 $context0f0 (local.get $l1)))
      (local.set $l3 (struct.get $context0 $context0f1 (local.get $l1)))
      (call $f1 (local.get $l2) (local.get $l3))
    )
  )

  (func $fv5 (type $sig1) (param $env anyref) (param $provider0 (ref null $trait1)) (param $provider1 (ref null $trait0)) (result i32)
    (call $f5 (local.get $provider0) (local.get $provider1))
  )

  (func $f6 (param $l0 (ref null $closure5)) (param $l1 (ref null $trait0)) (param $provider0 (ref null $hd.providers)) (result i32)
    (local $l2 (ref null $trait0))
    (local $tmp0 (ref null $closure5))
    (block (result i32)
      (local.set $l2 (local.get $l1))
      (block (result i32)
        (local.set $tmp0 (local.get $l0))
        (call_ref $sig5
          (struct.get $closure5 $closure5env (local.get $tmp0))
          (struct.new $hd.providers (i32.const 0) (local.get $l2) (call $hd.provider_concat (local.get $provider0) (ref.null $hd.providers)))
          (struct.get $closure5 $closure5fn (local.get $tmp0)))
      )
    )
  )

  (func $fv6 (type $sig6) (param $env anyref) (param $l0 (ref null $closure5)) (param $l1 (ref null $trait0)) (param $provider0 (ref null $hd.providers)) (result i32)
    (call $f6 (local.get $l0) (local.get $l1) (local.get $provider0))
  )

  (func $f7 (param $l0 (ref null $trait0)) (param $provider0 (ref null $trait1)) (result i32)
    (call $f6 (struct.new $closure5 (ref.func $adapt0) (struct.new $closure1 (ref.func $fv1) (ref.null any))) (local.get $l0) (struct.new $hd.providers (i32.const 1) (local.get $provider0) (ref.null $hd.providers)))
  )

  (func $fv7 (type $sig7) (param $env anyref) (param $l0 (ref null $trait0)) (param $provider0 (ref null $trait1)) (result i32)
    (call $f7 (local.get $l0) (local.get $provider0))
  )

  (func $f8 (param $l0 (ref null $trait0)) (result (ref null $closure3))
    (local $l1 (ref null $trait0))
    (block (result (ref null $closure3))
      (local.set $l1 (local.get $l0))
      (struct.new $closure3 (ref.func $c0) (struct.new $env0 (local.get $l1)))
    )
  )

  (func $fv8 (type $sig8) (param $env anyref) (param $l0 (ref null $trait0)) (result (ref null $closure3))
    (call $f8 (local.get $l0))
  )

  (func $body9 (param $provider0 (ref null $trait0)) (result i32)
    (local $tmp0 (ref null $trait0))
    (block (result i32)
      (local.set $tmp0 (local.get $provider0))
      (call_ref $tsig0_0
      (struct.get $trait0 $trait0value (local.get $tmp0))
      (local.get $tmp0)
      (struct.get $trait0 $trait0m0 (local.get $tmp0)))
    )
  )

  (func $f9 (param $provider0 (ref null $trait0)) (result (ref null $s9))
    (call $hd.trace (i32.const 9) (i32.const 0))
    (struct.new $s9 (i32.const 0) (i32.const 0) (local.get $provider0) (i32.const 0))
  )
  
  (func $poll9 (param $frame (ref null $s9)) (result i32)
    (call $hd.trace (i32.const 9) (i32.const 1))
    (if (i32.eq (struct.get $s9 $s9state (local.get $frame)) (i32.const 1))
      (then (call $hd.trace (i32.const 9) (i32.const 4)) (call $hd.panic (i32.const 5)) unreachable))
    (if (i32.or
          (i32.eq (struct.get $s9 $s9state (local.get $frame)) (i32.const 2))
          (i32.eq (struct.get $s9 $s9state (local.get $frame)) (i32.const 3)))
      (then (call $hd.trace (i32.const 9) (i32.const 5)) (call $hd.panic (i32.const 4)) unreachable))
    (struct.set $s9 $s9polls (local.get $frame)
      (i32.add (struct.get $s9 $s9polls (local.get $frame)) (i32.const 1)))
    (struct.set $s9 $s9state (local.get $frame) (i32.const 1))
    (if (call $hd.pending
          (i32.const 9)
          (struct.get $s9 $s9polls (local.get $frame)))
      (then
        (struct.set $s9 $s9state (local.get $frame) (i32.const 4))
        (call $hd.trace (i32.const 9) (i32.const 6))
        (return (i32.const 0))))
    (struct.set $s9 $s9state (local.get $frame) (i32.const 1))
    (struct.set $s9 $s9result (local.get $frame) (call $body9 (struct.get $s9 $s9p0 (local.get $frame))))
    (struct.set $s9 $s9state (local.get $frame) (i32.const 2))
    (call $hd.trace (i32.const 9) (i32.const 2))
    (i32.const 1)
  )
  
  (func $drive9 (param $frame (ref null $s9)) (result i32)
    (if (global.get $hd.driver-active) (then (call $hd.panic (i32.const 6)) unreachable))
    (global.set $hd.driver-active (i32.const 1))
    (block $ready
      (loop $drive
        (br_if $ready (i32.eq (call $poll9 (local.get $frame)) (i32.const 1)))
        (br $drive)))
    (global.set $hd.driver-active (i32.const 0))
    (struct.get $s9 $s9result (local.get $frame))
  )
  
  (func $cancel9 (param $frame (ref null $s9))
    (call $hd.trace (i32.const 9) (i32.const 3))
    (if (i32.eq (struct.get $s9 $s9state (local.get $frame)) (i32.const 1))
      (then (call $hd.trace (i32.const 9) (i32.const 4)) (call $hd.panic (i32.const 5)) unreachable))
    (if (i32.or
          (i32.eq (struct.get $s9 $s9state (local.get $frame)) (i32.const 0))
          (i32.eq (struct.get $s9 $s9state (local.get $frame)) (i32.const 4)))
      (then (struct.set $s9 $s9state (local.get $frame) (i32.const 3))))
  )

  (func $fv9 (type $sig9) (param $env anyref) (param $provider0 (ref null $trait0)) (result (ref null $hd.suspension))
    (struct.new $hd.suspension (call $f9 (local.get $provider0)) (ref.func $swpoll9) (ref.func $swcancel9) (ref.func $swresult9))
  )

  (func $f10 (param $provider0 (ref null $trait1)) (param $provider1 (ref null $trait0)) (result (ref null $s10))
    (call $hd.trace (i32.const 10) (i32.const 0))
    (struct.new $s10 (i32.const 0) (i32.const 0) (local.get $provider0) (local.get $provider1) (i32.const 0) (ref.null $s9) (i32.const 0))
  )
  
  (func $poll10 (param $frame (ref null $s10)) (result i32)
    (local $resume-state i32)
    (local $l0 i32)
    (local $provider0 (ref null $trait1))
    (local $provider1 (ref null $trait0))
    (local $tmp0 (ref null $trait1))
    (local $tmp1 (ref null $trait1))
    (call $hd.trace (i32.const 10) (i32.const 1))
    (local.set $resume-state (struct.get $s10 $s10state (local.get $frame)))
    (if (i32.eq (local.get $resume-state) (i32.const 1))
      (then (call $hd.trace (i32.const 10) (i32.const 4)) (call $hd.panic (i32.const 5)) unreachable))
    (if (i32.or (i32.eq (local.get $resume-state) (i32.const 2)) (i32.eq (local.get $resume-state) (i32.const 3)))
      (then (call $hd.trace (i32.const 10) (i32.const 5)) (call $hd.panic (i32.const 4)) unreachable))
    (struct.set $s10 $s10polls (local.get $frame)
      (i32.add (struct.get $s10 $s10polls (local.get $frame)) (i32.const 1)))
    (struct.set $s10 $s10state (local.get $frame) (i32.const 1))
    (if (call $hd.pending (i32.const 10) (struct.get $s10 $s10polls (local.get $frame)))
      (then
        (struct.set $s10 $s10state (local.get $frame)
          (if (result i32) (i32.eq (local.get $resume-state) (i32.const 0))
            (then (i32.const 4))
            (else (local.get $resume-state))))
        (call $hd.trace (i32.const 10) (i32.const 6))
        (return (i32.const 0))))
    (local.set $provider0 (struct.get $s10 $s10p0 (local.get $frame)))
    (local.set $provider1 (struct.get $s10 $s10p1 (local.get $frame)))
    (local.set $l0 (struct.get $s10 $s10l0 (local.get $frame)))
    (struct.set $s10 $s10state (local.get $frame) (i32.const 1))
    (if (i32.le_u (local.get $resume-state) (i32.const 4))
      (then
        (struct.set $s10 $s10child0 (local.get $frame) (call $f9 (local.get $provider1)))
        (if (i32.eqz (call $poll9 (struct.get $s10 $s10child0 (local.get $frame))))
          (then
            (struct.set $s10 $s10l0 (local.get $frame) (local.get $l0))
            (struct.set $s10 $s10state (local.get $frame) (i32.const 5))
            (call $hd.trace (i32.const 10) (i32.const 6))
            (return (i32.const 0))))
        (local.set $l0 (struct.get $s9 $s9result (struct.get $s10 $s10child0 (local.get $frame))))
        (struct.set $s10 $s10result (local.get $frame) (call $hd.add_i32 (local.get $l0) (block (result i32)
          (local.set $tmp0 (local.get $provider0))
          (call_ref $tsig1_0
          (struct.get $trait1 $trait1value (local.get $tmp0))
          (local.get $tmp0)
          (struct.get $trait1 $trait1m0 (local.get $tmp0)))
        )))
        (struct.set $s10 $s10state (local.get $frame) (i32.const 2))
        (call $hd.trace (i32.const 10) (i32.const 2))
        (return (i32.const 1))
      )
      (else
        (if (i32.eq (local.get $resume-state) (i32.const 5))
          (then
            (if (i32.eqz (call $poll9 (struct.get $s10 $s10child0 (local.get $frame))))
              (then
                (struct.set $s10 $s10l0 (local.get $frame) (local.get $l0))
                (struct.set $s10 $s10state (local.get $frame) (i32.const 5))
                (call $hd.trace (i32.const 10) (i32.const 6))
                (return (i32.const 0))))
            (local.set $l0 (struct.get $s9 $s9result (struct.get $s10 $s10child0 (local.get $frame))))
            (struct.set $s10 $s10result (local.get $frame) (call $hd.add_i32 (local.get $l0) (block (result i32)
              (local.set $tmp1 (local.get $provider0))
              (call_ref $tsig1_0
              (struct.get $trait1 $trait1value (local.get $tmp1))
              (local.get $tmp1)
              (struct.get $trait1 $trait1m0 (local.get $tmp1)))
            )))
            (struct.set $s10 $s10state (local.get $frame) (i32.const 2))
            (call $hd.trace (i32.const 10) (i32.const 2))
            (return (i32.const 1))
          )
          (else
            (unreachable)
          ))
      ))
  )
  
  (func $drive10 (param $frame (ref null $s10)) (result i32)
    (if (global.get $hd.driver-active) (then (call $hd.panic (i32.const 6)) unreachable))
    (global.set $hd.driver-active (i32.const 1))
    (block $ready
      (loop $drive
        (br_if $ready (i32.eq (call $poll10 (local.get $frame)) (i32.const 1)))
        (br $drive)))
    (global.set $hd.driver-active (i32.const 0))
    (struct.get $s10 $s10result (local.get $frame))
  )
  
  (func $cancel10 (param $frame (ref null $s10))
    (local $resume-state i32)
    (local $l0 i32)
    (local $provider0 (ref null $trait1))
    (local $provider1 (ref null $trait0))
    (local $tmp0 (ref null $trait1))
    (local $tmp1 (ref null $trait1))
    (call $hd.trace (i32.const 10) (i32.const 3))
    (local.set $resume-state (struct.get $s10 $s10state (local.get $frame)))
    (if (i32.eq (local.get $resume-state) (i32.const 1))
      (then (call $hd.trace (i32.const 10) (i32.const 4)) (call $hd.panic (i32.const 5)) unreachable))
    (local.set $provider0 (struct.get $s10 $s10p0 (local.get $frame)))
    (local.set $provider1 (struct.get $s10 $s10p1 (local.get $frame)))
    (local.set $l0 (struct.get $s10 $s10l0 (local.get $frame)))
    (if (i32.eq (local.get $resume-state) (i32.const 5))
      (then
        (call $cancel9 (struct.get $s10 $s10child0 (local.get $frame)))
        (struct.set $s10 $s10state (local.get $frame) (i32.const 3))
        (return)))
    (if (i32.or (i32.eq (local.get $resume-state) (i32.const 0)) (i32.eq (local.get $resume-state) (i32.const 4)))
      (then (struct.set $s10 $s10state (local.get $frame) (i32.const 3))))
  )

  (func $fv10 (type $sig10) (param $env anyref) (param $provider0 (ref null $trait1)) (param $provider1 (ref null $trait0)) (result (ref null $hd.suspension))
    (struct.new $hd.suspension (call $f10 (local.get $provider0) (local.get $provider1)) (ref.func $swpoll10) (ref.func $swcancel10) (ref.func $swresult10))
  )

  (func $f11 (result (ref null $s11))
    (call $hd.trace (i32.const 11) (i32.const 0))
    (struct.new $s11 (i32.const 0) (i32.const 0) (ref.null $trait0) (ref.null $trait1) (ref.null $d0) (ref.null $closure3) (i32.const 0) (i32.const 0) (i32.const 0) (ref.null $trait0) (ref.null $trait0) (i32.const 0) (i32.const 0) (i32.const 0) (ref.null $trait1) (ref.null $trait0) (i32.const 0) (i32.const 0) (ref.null $trait0) (ref.null $trait1) (i32.const 0) (i32.const 0) (ref.null $closure3) (i32.const 0) (i32.const 0) (ref.null $trait1) (ref.null $trait0) (i32.const 0) (i32.const 0) (i32.const 0) (ref.null $s10) (i32.const 0))
  )
  
  (func $poll11 (param $frame (ref null $s11)) (result i32)
    (local $resume-state i32)
    (local $pc i32)
    (local $l0 (ref null $trait0))
    (local $l1 (ref null $trait1))
    (local $l2 (ref null $d0))
    (local $l3 (ref null $closure3))
    (local $l4 i32)
    (local $l5 i32)
    (local $l6 i32)
    (local $l7 (ref null $trait0))
    (local $l8 (ref null $trait0))
    (local $l9 i32)
    (local $l10 i32)
    (local $l11 i32)
    (local $l12 (ref null $trait1))
    (local $l13 (ref null $trait0))
    (local $l14 i32)
    (local $l15 i32)
    (local $l16 (ref null $trait0))
    (local $l17 (ref null $trait1))
    (local $l18 i32)
    (local $l19 i32)
    (local $l20 (ref null $closure3))
    (local $l21 i32)
    (local $l22 i32)
    (local $l23 (ref null $trait1))
    (local $l24 (ref null $trait0))
    (local $l25 i32)
    (local $l26 i32)
    (local $l27 i32)
    (local $tmp0 (ref null $closure3))
    (local $tmp1 (ref null $d0))
    (local $tmp2 (ref null $d0))
    (local $tmp3 (ref null $d0))
    (local $tmp4 i32)
    (local $tmp5 (ref null $d0))
    (local $tmp6 i32)
    (local $tmp7 (ref null $d0))
    (local $tmp8 i32)
    (call $hd.trace (i32.const 11) (i32.const 1))
    (local.set $resume-state (struct.get $s11 $s11state (local.get $frame)))
    (if (i32.eq (local.get $resume-state) (i32.const 1))
      (then (call $hd.trace (i32.const 11) (i32.const 4)) (call $hd.panic (i32.const 5)) unreachable))
    (if (i32.or (i32.eq (local.get $resume-state) (i32.const 2)) (i32.eq (local.get $resume-state) (i32.const 3)))
      (then (call $hd.trace (i32.const 11) (i32.const 5)) (call $hd.panic (i32.const 4)) unreachable))
    (struct.set $s11 $s11polls (local.get $frame)
      (i32.add (struct.get $s11 $s11polls (local.get $frame)) (i32.const 1)))
    (struct.set $s11 $s11state (local.get $frame) (i32.const 1))
    (if (call $hd.pending (i32.const 11) (struct.get $s11 $s11polls (local.get $frame)))
      (then
        (struct.set $s11 $s11state (local.get $frame)
          (if (result i32) (i32.eq (local.get $resume-state) (i32.const 0))
            (then (i32.const 4))
            (else (local.get $resume-state))))
        (call $hd.trace (i32.const 11) (i32.const 6))
        (return (i32.const 0))))
    (local.set $l0 (struct.get $s11 $s11l0 (local.get $frame)))
    (local.set $l1 (struct.get $s11 $s11l1 (local.get $frame)))
    (local.set $l2 (struct.get $s11 $s11l2 (local.get $frame)))
    (local.set $l3 (struct.get $s11 $s11l3 (local.get $frame)))
    (local.set $l4 (struct.get $s11 $s11l4 (local.get $frame)))
    (local.set $l5 (struct.get $s11 $s11l5 (local.get $frame)))
    (local.set $l6 (struct.get $s11 $s11l6 (local.get $frame)))
    (local.set $l7 (struct.get $s11 $s11l7 (local.get $frame)))
    (local.set $l8 (struct.get $s11 $s11l8 (local.get $frame)))
    (local.set $l9 (struct.get $s11 $s11l9 (local.get $frame)))
    (local.set $l10 (struct.get $s11 $s11l10 (local.get $frame)))
    (local.set $l11 (struct.get $s11 $s11l11 (local.get $frame)))
    (local.set $l12 (struct.get $s11 $s11l12 (local.get $frame)))
    (local.set $l13 (struct.get $s11 $s11l13 (local.get $frame)))
    (local.set $l14 (struct.get $s11 $s11l14 (local.get $frame)))
    (local.set $l15 (struct.get $s11 $s11l15 (local.get $frame)))
    (local.set $l16 (struct.get $s11 $s11l16 (local.get $frame)))
    (local.set $l17 (struct.get $s11 $s11l17 (local.get $frame)))
    (local.set $l18 (struct.get $s11 $s11l18 (local.get $frame)))
    (local.set $l19 (struct.get $s11 $s11l19 (local.get $frame)))
    (local.set $l20 (struct.get $s11 $s11l20 (local.get $frame)))
    (local.set $l21 (struct.get $s11 $s11l21 (local.get $frame)))
    (local.set $l22 (struct.get $s11 $s11l22 (local.get $frame)))
    (local.set $l23 (struct.get $s11 $s11l23 (local.get $frame)))
    (local.set $l24 (struct.get $s11 $s11l24 (local.get $frame)))
    (local.set $l25 (struct.get $s11 $s11l25 (local.get $frame)))
    (local.set $l26 (struct.get $s11 $s11l26 (local.get $frame)))
    (local.set $l27 (struct.get $s11 $s11l27 (local.get $frame)))
    (struct.set $s11 $s11state (local.get $frame) (i32.const 1))
    (if (i32.le_u (local.get $resume-state) (i32.const 4))
      (then (local.set $pc (i32.const 27)))
      (else
        (if (i32.eq (local.get $resume-state) (i32.const 5))
          (then
            (if (i32.eqz (call $poll10 (struct.get $s11 $s11child0 (local.get $frame))))
              (then
                (struct.set $s11 $s11l0 (local.get $frame) (local.get $l0))
                (struct.set $s11 $s11l1 (local.get $frame) (local.get $l1))
                (struct.set $s11 $s11l2 (local.get $frame) (local.get $l2))
                (struct.set $s11 $s11l3 (local.get $frame) (local.get $l3))
                (struct.set $s11 $s11l4 (local.get $frame) (local.get $l4))
                (struct.set $s11 $s11l5 (local.get $frame) (local.get $l5))
                (struct.set $s11 $s11l6 (local.get $frame) (local.get $l6))
                (struct.set $s11 $s11l7 (local.get $frame) (local.get $l7))
                (struct.set $s11 $s11l8 (local.get $frame) (local.get $l8))
                (struct.set $s11 $s11l9 (local.get $frame) (local.get $l9))
                (struct.set $s11 $s11l10 (local.get $frame) (local.get $l10))
                (struct.set $s11 $s11l11 (local.get $frame) (local.get $l11))
                (struct.set $s11 $s11l12 (local.get $frame) (local.get $l12))
                (struct.set $s11 $s11l13 (local.get $frame) (local.get $l13))
                (struct.set $s11 $s11l14 (local.get $frame) (local.get $l14))
                (struct.set $s11 $s11l15 (local.get $frame) (local.get $l15))
                (struct.set $s11 $s11l16 (local.get $frame) (local.get $l16))
                (struct.set $s11 $s11l17 (local.get $frame) (local.get $l17))
                (struct.set $s11 $s11l18 (local.get $frame) (local.get $l18))
                (struct.set $s11 $s11l19 (local.get $frame) (local.get $l19))
                (struct.set $s11 $s11l20 (local.get $frame) (local.get $l20))
                (struct.set $s11 $s11l21 (local.get $frame) (local.get $l21))
                (struct.set $s11 $s11l22 (local.get $frame) (local.get $l22))
                (struct.set $s11 $s11l23 (local.get $frame) (local.get $l23))
                (struct.set $s11 $s11l24 (local.get $frame) (local.get $l24))
                (struct.set $s11 $s11l25 (local.get $frame) (local.get $l25))
                (struct.set $s11 $s11l26 (local.get $frame) (local.get $l26))
                (struct.set $s11 $s11l27 (local.get $frame) (local.get $l27))
                (struct.set $s11 $s11state (local.get $frame) (local.get $resume-state))
                (call $hd.trace (i32.const 11) (i32.const 6))
                (return (i32.const 0))))
            (local.set $l25 (struct.get $s10 $s10result (struct.get $s11 $s11child0 (local.get $frame))))
            (local.set $pc (i32.const 1))
          )
          (else
            (local.set $pc (i32.const 27))
          ))
      ))
    (loop $cfg
      (if (i32.eq (local.get $pc) (i32.const 0))
        (then
          (local.set $l27 (call $hd.add_i32 (local.get $l22) (local.get $l26)))
          (struct.set $s11 $s11result (local.get $frame) (local.get $l27))
          (struct.set $s11 $s11state (local.get $frame) (i32.const 2))
          (call $hd.trace (i32.const 11) (i32.const 2))
          (return (i32.const 1))
        ))
      (if (i32.eq (local.get $pc) (i32.const 1))
        (then
          (local.set $l26 (local.get $l25))
          (local.set $pc (i32.const 0))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 2))
        (then
          (struct.set $s11 $s11child0 (local.get $frame) (call $f10 (local.get $l23) (local.get $l24)))
          (if (i32.eqz (call $poll10 (struct.get $s11 $s11child0 (local.get $frame))))
            (then
              (struct.set $s11 $s11l0 (local.get $frame) (local.get $l0))
              (struct.set $s11 $s11l1 (local.get $frame) (local.get $l1))
              (struct.set $s11 $s11l2 (local.get $frame) (local.get $l2))
              (struct.set $s11 $s11l3 (local.get $frame) (local.get $l3))
              (struct.set $s11 $s11l4 (local.get $frame) (local.get $l4))
              (struct.set $s11 $s11l5 (local.get $frame) (local.get $l5))
              (struct.set $s11 $s11l6 (local.get $frame) (local.get $l6))
              (struct.set $s11 $s11l7 (local.get $frame) (local.get $l7))
              (struct.set $s11 $s11l8 (local.get $frame) (local.get $l8))
              (struct.set $s11 $s11l9 (local.get $frame) (local.get $l9))
              (struct.set $s11 $s11l10 (local.get $frame) (local.get $l10))
              (struct.set $s11 $s11l11 (local.get $frame) (local.get $l11))
              (struct.set $s11 $s11l12 (local.get $frame) (local.get $l12))
              (struct.set $s11 $s11l13 (local.get $frame) (local.get $l13))
              (struct.set $s11 $s11l14 (local.get $frame) (local.get $l14))
              (struct.set $s11 $s11l15 (local.get $frame) (local.get $l15))
              (struct.set $s11 $s11l16 (local.get $frame) (local.get $l16))
              (struct.set $s11 $s11l17 (local.get $frame) (local.get $l17))
              (struct.set $s11 $s11l18 (local.get $frame) (local.get $l18))
              (struct.set $s11 $s11l19 (local.get $frame) (local.get $l19))
              (struct.set $s11 $s11l20 (local.get $frame) (local.get $l20))
              (struct.set $s11 $s11l21 (local.get $frame) (local.get $l21))
              (struct.set $s11 $s11l22 (local.get $frame) (local.get $l22))
              (struct.set $s11 $s11l23 (local.get $frame) (local.get $l23))
              (struct.set $s11 $s11l24 (local.get $frame) (local.get $l24))
              (struct.set $s11 $s11l25 (local.get $frame) (local.get $l25))
              (struct.set $s11 $s11l26 (local.get $frame) (local.get $l26))
              (struct.set $s11 $s11l27 (local.get $frame) (local.get $l27))
              (struct.set $s11 $s11state (local.get $frame) (i32.const 5))
              (call $hd.trace (i32.const 11) (i32.const 6))
              (return (i32.const 0))))
          (local.set $l25 (struct.get $s10 $s10result (struct.get $s11 $s11child0 (local.get $frame))))
          (local.set $pc (i32.const 1))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 3))
        (then
          (local.set $l24 (local.get $l0))
          (local.set $pc (i32.const 2))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 4))
        (then
          (local.set $l23 (local.get $l1))
          (local.set $pc (i32.const 3))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 5))
        (then
          (local.set $l22 (call $hd.add_i32 (local.get $l19) (local.get $l21)))
          (local.set $pc (i32.const 4))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 6))
        (then
          (local.set $l21 (block (result i32)
            (local.set $tmp0 (local.get $l20))
            (call_ref $sig3
              (struct.get $closure3 $closure3env (local.get $tmp0))
              (struct.get $closure3 $closure3fn (local.get $tmp0)))
          ))
          (local.set $pc (i32.const 5))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 7))
        (then
          (local.set $l20 (local.get $l3))
          (local.set $pc (i32.const 6))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 8))
        (then
          (local.set $l19 (call $hd.add_i32 (local.get $l15) (local.get $l18)))
          (local.set $pc (i32.const 7))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 9))
        (then
          (local.set $l18 (call $f7 (local.get $l16) (local.get $l17)))
          (local.set $pc (i32.const 8))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 10))
        (then
          (local.set $l17 (local.get $l1))
          (local.set $pc (i32.const 9))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 11))
        (then
          (local.set $l16 (block (result (ref null $trait0))
            (local.set $tmp1 (local.get $l2))
            (struct.new $trait0 (local.get $tmp1) (ref.null $hd.list) (ref.func $tadapt0_0))
          ))
          (local.set $pc (i32.const 10))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 12))
        (then
          (local.set $l15 (call $hd.add_i32 (local.get $l11) (local.get $l14)))
          (local.set $pc (i32.const 11))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 13))
        (then
          (local.set $l14 (call $f5 (local.get $l12) (local.get $l13)))
          (local.set $pc (i32.const 12))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 14))
        (then
          (local.set $l13 (local.get $l0))
          (local.set $pc (i32.const 13))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 15))
        (then
          (local.set $l12 (local.get $l1))
          (local.set $pc (i32.const 14))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 16))
        (then
          (local.set $l11 (call $hd.add_i32 (local.get $l9) (local.get $l10)))
          (local.set $pc (i32.const 15))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 17))
        (then
          (local.set $l10 (call $f3))
          (local.set $pc (i32.const 16))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 18))
        (then
          (local.set $l9 (call $f2 (local.get $l8)))
          (local.set $pc (i32.const 17))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 19))
        (then
          (local.set $l8 (block (result (ref null $trait0))
            (local.set $tmp2 (local.get $l2))
            (struct.new $trait0 (local.get $tmp2) (ref.null $hd.list) (ref.func $tadapt0_0))
          ))
          (local.set $pc (i32.const 18))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 20))
        (then
          (local.set $l3 (call $f8 (local.get $l7)))
          (local.set $pc (i32.const 19))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 21))
        (then
          (local.set $l7 (block (result (ref null $trait0))
            (local.set $tmp3 (local.get $l2))
            (struct.new $trait0 (local.get $tmp3) (ref.null $hd.list) (ref.func $tadapt0_0))
          ))
          (local.set $pc (i32.const 20))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 22))
        (then
          (local.set $l2 (block (result (ref null $d0))
            (local.set $tmp4 (local.get $l6))
            (struct.new $d0 (local.get $tmp4))
          ))
          (local.set $pc (i32.const 21))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 23))
        (then
          (local.set $l6 (i32.const 3))
          (local.set $pc (i32.const 22))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 24))
        (then
          (local.set $l1 (block (result (ref null $trait1))
            (local.set $tmp5 (block (result (ref null $d0))
            (local.set $tmp6 (local.get $l5))
            (struct.new $d0 (local.get $tmp6))
          ))
            (struct.new $trait1 (local.get $tmp5) (ref.null $hd.list) (ref.func $tadapt1_0))
          ))
          (local.set $pc (i32.const 23))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 25))
        (then
          (local.set $l5 (i32.const 2))
          (local.set $pc (i32.const 24))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 26))
        (then
          (local.set $l0 (block (result (ref null $trait0))
            (local.set $tmp7 (block (result (ref null $d0))
            (local.set $tmp8 (local.get $l4))
            (struct.new $d0 (local.get $tmp8))
          ))
            (struct.new $trait0 (local.get $tmp7) (ref.null $hd.list) (ref.func $tadapt0_0))
          ))
          (local.set $pc (i32.const 25))
          (br $cfg)
        ))
      (if (i32.eq (local.get $pc) (i32.const 27))
        (then
          (local.set $l4 (i32.const 1))
          (local.set $pc (i32.const 26))
          (br $cfg)
        ))
      unreachable
    )
  )
  
  (func $drive11 (param $frame (ref null $s11)) (result i32)
    (if (global.get $hd.driver-active) (then (call $hd.panic (i32.const 6)) unreachable))
    (global.set $hd.driver-active (i32.const 1))
    (block $ready
      (loop $drive
        (br_if $ready (i32.eq (call $poll11 (local.get $frame)) (i32.const 1)))
        (br $drive)))
    (global.set $hd.driver-active (i32.const 0))
    (struct.get $s11 $s11result (local.get $frame))
  )
  
  (func $cancel11 (param $frame (ref null $s11))
    (local $resume-state i32)
    (local $l0 (ref null $trait0))
    (local $l1 (ref null $trait1))
    (local $l2 (ref null $d0))
    (local $l3 (ref null $closure3))
    (local $l4 i32)
    (local $l5 i32)
    (local $l6 i32)
    (local $l7 (ref null $trait0))
    (local $l8 (ref null $trait0))
    (local $l9 i32)
    (local $l10 i32)
    (local $l11 i32)
    (local $l12 (ref null $trait1))
    (local $l13 (ref null $trait0))
    (local $l14 i32)
    (local $l15 i32)
    (local $l16 (ref null $trait0))
    (local $l17 (ref null $trait1))
    (local $l18 i32)
    (local $l19 i32)
    (local $l20 (ref null $closure3))
    (local $l21 i32)
    (local $l22 i32)
    (local $l23 (ref null $trait1))
    (local $l24 (ref null $trait0))
    (local $l25 i32)
    (local $l26 i32)
    (local $l27 i32)
    (local $tmp0 (ref null $closure3))
    (local $tmp1 (ref null $d0))
    (local $tmp2 (ref null $d0))
    (local $tmp3 (ref null $d0))
    (local $tmp4 i32)
    (local $tmp5 (ref null $d0))
    (local $tmp6 i32)
    (local $tmp7 (ref null $d0))
    (local $tmp8 i32)
    (call $hd.trace (i32.const 11) (i32.const 3))
    (local.set $resume-state (struct.get $s11 $s11state (local.get $frame)))
    (if (i32.eq (local.get $resume-state) (i32.const 1))
      (then (call $hd.trace (i32.const 11) (i32.const 4)) (call $hd.panic (i32.const 5)) unreachable))
    (local.set $l0 (struct.get $s11 $s11l0 (local.get $frame)))
    (local.set $l1 (struct.get $s11 $s11l1 (local.get $frame)))
    (local.set $l2 (struct.get $s11 $s11l2 (local.get $frame)))
    (local.set $l3 (struct.get $s11 $s11l3 (local.get $frame)))
    (local.set $l4 (struct.get $s11 $s11l4 (local.get $frame)))
    (local.set $l5 (struct.get $s11 $s11l5 (local.get $frame)))
    (local.set $l6 (struct.get $s11 $s11l6 (local.get $frame)))
    (local.set $l7 (struct.get $s11 $s11l7 (local.get $frame)))
    (local.set $l8 (struct.get $s11 $s11l8 (local.get $frame)))
    (local.set $l9 (struct.get $s11 $s11l9 (local.get $frame)))
    (local.set $l10 (struct.get $s11 $s11l10 (local.get $frame)))
    (local.set $l11 (struct.get $s11 $s11l11 (local.get $frame)))
    (local.set $l12 (struct.get $s11 $s11l12 (local.get $frame)))
    (local.set $l13 (struct.get $s11 $s11l13 (local.get $frame)))
    (local.set $l14 (struct.get $s11 $s11l14 (local.get $frame)))
    (local.set $l15 (struct.get $s11 $s11l15 (local.get $frame)))
    (local.set $l16 (struct.get $s11 $s11l16 (local.get $frame)))
    (local.set $l17 (struct.get $s11 $s11l17 (local.get $frame)))
    (local.set $l18 (struct.get $s11 $s11l18 (local.get $frame)))
    (local.set $l19 (struct.get $s11 $s11l19 (local.get $frame)))
    (local.set $l20 (struct.get $s11 $s11l20 (local.get $frame)))
    (local.set $l21 (struct.get $s11 $s11l21 (local.get $frame)))
    (local.set $l22 (struct.get $s11 $s11l22 (local.get $frame)))
    (local.set $l23 (struct.get $s11 $s11l23 (local.get $frame)))
    (local.set $l24 (struct.get $s11 $s11l24 (local.get $frame)))
    (local.set $l25 (struct.get $s11 $s11l25 (local.get $frame)))
    (local.set $l26 (struct.get $s11 $s11l26 (local.get $frame)))
    (local.set $l27 (struct.get $s11 $s11l27 (local.get $frame)))
    (if (i32.eq (local.get $resume-state) (i32.const 5))
      (then
        (call $cancel10 (struct.get $s11 $s11child0 (local.get $frame)))
        (struct.set $s11 $s11state (local.get $frame) (i32.const 3))
        (return)))
    (if (i32.or (i32.eq (local.get $resume-state) (i32.const 0)) (i32.eq (local.get $resume-state) (i32.const 4)))
      (then (struct.set $s11 $s11state (local.get $frame) (i32.const 3))))
  )
  
  (func $entry11 (export "main") (result i32)
    (call $drive11 (call $f11))
  )
  
  (global $hd.dev-frame11 (mut (ref null $s11)) (ref.null $s11))
  (func (export "__hd_start")
    (if (global.get $hd.driver-active) (then (call $hd.panic (i32.const 6)) unreachable))
    (global.set $hd.driver-active (i32.const 1))
    (global.set $hd.dev-frame11 (call $f11))
  )
  (func (export "__hd_poll") (result i32)
    (local $ready i32)
    (local.set $ready (call $poll11 (global.get $hd.dev-frame11)))
    (if (local.get $ready) (then (global.set $hd.driver-active (i32.const 0))))
    (local.get $ready)
  )
  (func (export "__hd_cancel")
    (call $cancel11 (global.get $hd.dev-frame11))
    (global.set $hd.driver-active (i32.const 0))
  )
  (func (export "__hd_result") (result i32) (struct.get $s11 $s11result (global.get $hd.dev-frame11)))

  (func $fv11 (type $sig11) (param $env anyref)  (result (ref null $hd.suspension))
    (struct.new $hd.suspension (call $f11) (ref.func $swpoll11) (ref.func $swcancel11) (ref.func $swresult11))
  )

  (func $f12 (param $l0 (ref null $d0)) (result i32)
    (struct.get $d0 $d0f0 (local.get $l0))
  )

  (func $fv12 (type $sig12) (param $env anyref) (param $l0 (ref null $d0)) (result i32)
    (call $f12 (local.get $l0))
  )

  (func $f13 (param $l0 (ref null $d0)) (result i32)
    (struct.get $d0 $d0f0 (local.get $l0))
  )

  (func $fv13 (type $sig12) (param $env anyref) (param $l0 (ref null $d0)) (result i32)
    (call $f13 (local.get $l0))
  )

  (func $c0 (type $sig3) (param $env anyref) (result i32)
    (local $tmp0 (ref null $trait0))
    (block (result i32)
      (local.set $tmp0 (struct.get $env0 $env0f0 (ref.cast (ref $env0) (local.get $env))))
      (call_ref $tsig0_0
      (struct.get $trait0 $trait0value (local.get $tmp0))
      (local.get $tmp0)
      (struct.get $trait0 $trait0m0 (local.get $tmp0)))
    )
  )

  (func $swpoll9 (type $hd.suspension-poll-sig) (param $inner anyref) (result i32)
    (call $poll9 (ref.cast (ref $s9) (local.get $inner))))
  
  (func $swcancel9 (type $hd.suspension-cancel-sig) (param $inner anyref)
    (call $cancel9 (ref.cast (ref $s9) (local.get $inner))))
  
  (func $swresult9 (type $hd.suspension-result-sig) (param $inner anyref) (result anyref)
    (struct.new $hd.box-i32 (struct.get $s9 $s9result (ref.cast (ref $s9) (local.get $inner)))))
  
  (func $swpoll10 (type $hd.suspension-poll-sig) (param $inner anyref) (result i32)
    (call $poll10 (ref.cast (ref $s10) (local.get $inner))))
  
  (func $swcancel10 (type $hd.suspension-cancel-sig) (param $inner anyref)
    (call $cancel10 (ref.cast (ref $s10) (local.get $inner))))
  
  (func $swresult10 (type $hd.suspension-result-sig) (param $inner anyref) (result anyref)
    (struct.new $hd.box-i32 (struct.get $s10 $s10result (ref.cast (ref $s10) (local.get $inner)))))
  
  (func $swpoll11 (type $hd.suspension-poll-sig) (param $inner anyref) (result i32)
    (call $poll11 (ref.cast (ref $s11) (local.get $inner))))
  
  (func $swcancel11 (type $hd.suspension-cancel-sig) (param $inner anyref)
    (call $cancel11 (ref.cast (ref $s11) (local.get $inner))))
  
  (func $swresult11 (type $hd.suspension-result-sig) (param $inner anyref) (result anyref)
    (struct.new $hd.box-i32 (struct.get $s11 $s11result (ref.cast (ref $s11) (local.get $inner)))))

  (func $adapt0 (type $sig5) (param $env anyref) (param $p0 (ref null $hd.providers)) (result i32)
    (call_ref $sig1 (struct.get $closure1 $closure1env (ref.cast (ref $closure1) (local.get $env))) (ref.cast (ref null $trait1) (call $hd.provider_get (call $hd.provider_concat (local.get $p0) (ref.null $hd.providers)) (i32.const 1))) (ref.cast (ref null $trait0) (call $hd.provider_get (call $hd.provider_concat (local.get $p0) (ref.null $hd.providers)) (i32.const 0))) (struct.get $closure1 $closure1fn (ref.cast (ref $closure1) (local.get $env))))
  )

  (func $tadapt0_0 (type $tsig0_0) (param $self anyref) (param $dictionary anyref)  (result i32)
    (call $f12 (ref.cast (ref null $d0) (local.get $self)))
  )
  
  (func $tadapt1_0 (type $tsig1_0) (param $self anyref) (param $dictionary anyref)  (result i32)
    (call $f13 (ref.cast (ref null $d0) (local.get $self)))
  )
)
