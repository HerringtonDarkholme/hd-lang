;; commit bd985d7; emitted by audit/scripts/arch/req-probes.ts for chain-10.hd; 2026-09-25T20:54:24.919Z
(module
  (import "hd" "panic" (func $hd.panic (param i32)))
  (rec
    (type $sig0 (func (param anyref) (param i32) (param (ref null $trait0)) (param (ref null $trait9)) (param (ref null $trait1)) (param (ref null $trait2)) (param (ref null $trait3)) (param (ref null $trait4)) (param (ref null $trait5)) (param (ref null $trait6)) (param (ref null $trait7)) (param (ref null $trait8)) (result i32)))
    (type $sig1 (func (param anyref) (result i32)))
    (type $sig2 (func (param anyref) (param (ref null $d0)) (result i32)))
    (type $sig3 (func (param anyref) (param (ref null $d1)) (result i32)))
    (type $sig4 (func (param anyref) (param (ref null $d2)) (result i32)))
    (type $sig5 (func (param anyref) (param (ref null $d3)) (result i32)))
    (type $sig6 (func (param anyref) (param (ref null $d4)) (result i32)))
    (type $sig7 (func (param anyref) (param (ref null $d5)) (result i32)))
    (type $sig8 (func (param anyref) (param (ref null $d6)) (result i32)))
    (type $sig9 (func (param anyref) (param (ref null $d7)) (result i32)))
    (type $sig10 (func (param anyref) (param (ref null $d8)) (result i32)))
    (type $sig11 (func (param anyref) (param (ref null $d9)) (result i32)))
    (type $tsig0_0 (func (param anyref) (param anyref) (result i32)))
    (type $tsig1_0 (func (param anyref) (param anyref) (result i32)))
    (type $tsig2_0 (func (param anyref) (param anyref) (result i32)))
    (type $tsig3_0 (func (param anyref) (param anyref) (result i32)))
    (type $tsig4_0 (func (param anyref) (param anyref) (result i32)))
    (type $tsig5_0 (func (param anyref) (param anyref) (result i32)))
    (type $tsig6_0 (func (param anyref) (param anyref) (result i32)))
    (type $tsig7_0 (func (param anyref) (param anyref) (result i32)))
    (type $tsig8_0 (func (param anyref) (param anyref) (result i32)))
    (type $tsig9_0 (func (param anyref) (param anyref) (result i32)))
    (type $tsig10_0 (func (param anyref) (param anyref) (result (ref null $hd.bytes))))
    (type $tsig11_0 (func (param anyref) (param anyref) (param anyref) (result i32)))
    (type $tsig12_0 (func (param anyref) (param anyref) (param anyref) (result (ref null $hd.variant))))
    (type $tsig13_0 (func (param anyref) (param anyref)))
    (type $tsig14_0 (func (param anyref) (param anyref) (result (ref null $hd.variant))))
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
    (type $trait7 (struct
      (field $trait7value anyref)
      (field $trait7bounds (ref null $hd.list))
      (field $trait7m0 (ref $tsig7_0))))
    (type $trait8 (struct
      (field $trait8value anyref)
      (field $trait8bounds (ref null $hd.list))
      (field $trait8m0 (ref $tsig8_0))))
    (type $trait9 (struct
      (field $trait9value anyref)
      (field $trait9bounds (ref null $hd.list))
      (field $trait9m0 (ref $tsig9_0))))
    (type $trait10 (struct
      (field $trait10value anyref)
      (field $trait10bounds (ref null $hd.list))
      (field $trait10m0 (ref $tsig10_0))))
    (type $trait11 (struct
      (field $trait11value anyref)
      (field $trait11bounds (ref null $hd.list))
      (field $trait11m0 (ref $tsig11_0))))
    (type $trait12 (struct
      (field $trait12value anyref)
      (field $trait12bounds (ref null $hd.list))
      (field $trait12m0 (ref $tsig12_0))))
    (type $trait13 (struct
      (field $trait13value anyref)
      (field $trait13bounds (ref null $hd.list))
      (field $trait13m0 (ref $tsig13_0))))
    (type $trait14 (struct
      (field $trait14value anyref)
      (field $trait14bounds (ref null $hd.list))
      (field $trait14m0 (ref $tsig14_0))))


    (type $d0 (struct
      (field $d0f0 (mut i32))))
    (type $d1 (struct
      (field $d1f0 (mut i32))))
    (type $d2 (struct
      (field $d2f0 (mut i32))))
    (type $d3 (struct
      (field $d3f0 (mut i32))))
    (type $d4 (struct
      (field $d4f0 (mut i32))))
    (type $d5 (struct
      (field $d5f0 (mut i32))))
    (type $d6 (struct
      (field $d6f0 (mut i32))))
    (type $d7 (struct
      (field $d7f0 (mut i32))))
    (type $d8 (struct
      (field $d8f0 (mut i32))))
    (type $d9 (struct
      (field $d9f0 (mut i32))))
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

  (elem declare func $fv0 $fv1 $fv2 $fv3 $fv4 $fv5 $fv6 $fv7 $fv8 $fv9 $fv10 $fv11 $fv12 $fv13 $fv14 $fv15 $fv16 $fv17 $fv18 $fv19 $fv20 $tadapt0_0 $tadapt1_0 $tadapt2_0 $tadapt3_0 $tadapt4_0 $tadapt5_0 $tadapt6_0 $tadapt7_0 $tadapt8_0 $tadapt9_0)

  (func $f0 (export "c10") (param $l0 i32) (param $provider0 (ref null $trait0)) (param $provider1 (ref null $trait9)) (param $provider2 (ref null $trait1)) (param $provider3 (ref null $trait2)) (param $provider4 (ref null $trait3)) (param $provider5 (ref null $trait4)) (param $provider6 (ref null $trait5)) (param $provider7 (ref null $trait6)) (param $provider8 (ref null $trait7)) (param $provider9 (ref null $trait8)) (result i32)
    (local $tmp0 (ref null $trait0))
    (call $hd.add_i32 (local.get $l0) (block (result i32)
      (local.set $tmp0 (local.get $provider0))
      (call_ref $tsig0_0
      (struct.get $trait0 $trait0value (local.get $tmp0))
      (local.get $tmp0)
      (struct.get $trait0 $trait0m0 (local.get $tmp0)))
    ))
  )

  (func $fv0 (type $sig0) (param $env anyref) (param $l0 i32) (param $provider0 (ref null $trait0)) (param $provider1 (ref null $trait9)) (param $provider2 (ref null $trait1)) (param $provider3 (ref null $trait2)) (param $provider4 (ref null $trait3)) (param $provider5 (ref null $trait4)) (param $provider6 (ref null $trait5)) (param $provider7 (ref null $trait6)) (param $provider8 (ref null $trait7)) (param $provider9 (ref null $trait8)) (result i32)
    (call $f0 (local.get $l0) (local.get $provider0) (local.get $provider1) (local.get $provider2) (local.get $provider3) (local.get $provider4) (local.get $provider5) (local.get $provider6) (local.get $provider7) (local.get $provider8) (local.get $provider9))
  )

  (func $f1 (export "c9") (param $l0 i32) (param $provider0 (ref null $trait0)) (param $provider1 (ref null $trait9)) (param $provider2 (ref null $trait1)) (param $provider3 (ref null $trait2)) (param $provider4 (ref null $trait3)) (param $provider5 (ref null $trait4)) (param $provider6 (ref null $trait5)) (param $provider7 (ref null $trait6)) (param $provider8 (ref null $trait7)) (param $provider9 (ref null $trait8)) (result i32)
    (call $f0 (local.get $l0) (local.get $provider0) (local.get $provider1) (local.get $provider2) (local.get $provider3) (local.get $provider4) (local.get $provider5) (local.get $provider6) (local.get $provider7) (local.get $provider8) (local.get $provider9))
  )

  (func $fv1 (type $sig0) (param $env anyref) (param $l0 i32) (param $provider0 (ref null $trait0)) (param $provider1 (ref null $trait9)) (param $provider2 (ref null $trait1)) (param $provider3 (ref null $trait2)) (param $provider4 (ref null $trait3)) (param $provider5 (ref null $trait4)) (param $provider6 (ref null $trait5)) (param $provider7 (ref null $trait6)) (param $provider8 (ref null $trait7)) (param $provider9 (ref null $trait8)) (result i32)
    (call $f1 (local.get $l0) (local.get $provider0) (local.get $provider1) (local.get $provider2) (local.get $provider3) (local.get $provider4) (local.get $provider5) (local.get $provider6) (local.get $provider7) (local.get $provider8) (local.get $provider9))
  )

  (func $f2 (export "c8") (param $l0 i32) (param $provider0 (ref null $trait0)) (param $provider1 (ref null $trait9)) (param $provider2 (ref null $trait1)) (param $provider3 (ref null $trait2)) (param $provider4 (ref null $trait3)) (param $provider5 (ref null $trait4)) (param $provider6 (ref null $trait5)) (param $provider7 (ref null $trait6)) (param $provider8 (ref null $trait7)) (param $provider9 (ref null $trait8)) (result i32)
    (call $f1 (local.get $l0) (local.get $provider0) (local.get $provider1) (local.get $provider2) (local.get $provider3) (local.get $provider4) (local.get $provider5) (local.get $provider6) (local.get $provider7) (local.get $provider8) (local.get $provider9))
  )

  (func $fv2 (type $sig0) (param $env anyref) (param $l0 i32) (param $provider0 (ref null $trait0)) (param $provider1 (ref null $trait9)) (param $provider2 (ref null $trait1)) (param $provider3 (ref null $trait2)) (param $provider4 (ref null $trait3)) (param $provider5 (ref null $trait4)) (param $provider6 (ref null $trait5)) (param $provider7 (ref null $trait6)) (param $provider8 (ref null $trait7)) (param $provider9 (ref null $trait8)) (result i32)
    (call $f2 (local.get $l0) (local.get $provider0) (local.get $provider1) (local.get $provider2) (local.get $provider3) (local.get $provider4) (local.get $provider5) (local.get $provider6) (local.get $provider7) (local.get $provider8) (local.get $provider9))
  )

  (func $f3 (export "c7") (param $l0 i32) (param $provider0 (ref null $trait0)) (param $provider1 (ref null $trait9)) (param $provider2 (ref null $trait1)) (param $provider3 (ref null $trait2)) (param $provider4 (ref null $trait3)) (param $provider5 (ref null $trait4)) (param $provider6 (ref null $trait5)) (param $provider7 (ref null $trait6)) (param $provider8 (ref null $trait7)) (param $provider9 (ref null $trait8)) (result i32)
    (call $f2 (local.get $l0) (local.get $provider0) (local.get $provider1) (local.get $provider2) (local.get $provider3) (local.get $provider4) (local.get $provider5) (local.get $provider6) (local.get $provider7) (local.get $provider8) (local.get $provider9))
  )

  (func $fv3 (type $sig0) (param $env anyref) (param $l0 i32) (param $provider0 (ref null $trait0)) (param $provider1 (ref null $trait9)) (param $provider2 (ref null $trait1)) (param $provider3 (ref null $trait2)) (param $provider4 (ref null $trait3)) (param $provider5 (ref null $trait4)) (param $provider6 (ref null $trait5)) (param $provider7 (ref null $trait6)) (param $provider8 (ref null $trait7)) (param $provider9 (ref null $trait8)) (result i32)
    (call $f3 (local.get $l0) (local.get $provider0) (local.get $provider1) (local.get $provider2) (local.get $provider3) (local.get $provider4) (local.get $provider5) (local.get $provider6) (local.get $provider7) (local.get $provider8) (local.get $provider9))
  )

  (func $f4 (export "c6") (param $l0 i32) (param $provider0 (ref null $trait0)) (param $provider1 (ref null $trait9)) (param $provider2 (ref null $trait1)) (param $provider3 (ref null $trait2)) (param $provider4 (ref null $trait3)) (param $provider5 (ref null $trait4)) (param $provider6 (ref null $trait5)) (param $provider7 (ref null $trait6)) (param $provider8 (ref null $trait7)) (param $provider9 (ref null $trait8)) (result i32)
    (call $f3 (local.get $l0) (local.get $provider0) (local.get $provider1) (local.get $provider2) (local.get $provider3) (local.get $provider4) (local.get $provider5) (local.get $provider6) (local.get $provider7) (local.get $provider8) (local.get $provider9))
  )

  (func $fv4 (type $sig0) (param $env anyref) (param $l0 i32) (param $provider0 (ref null $trait0)) (param $provider1 (ref null $trait9)) (param $provider2 (ref null $trait1)) (param $provider3 (ref null $trait2)) (param $provider4 (ref null $trait3)) (param $provider5 (ref null $trait4)) (param $provider6 (ref null $trait5)) (param $provider7 (ref null $trait6)) (param $provider8 (ref null $trait7)) (param $provider9 (ref null $trait8)) (result i32)
    (call $f4 (local.get $l0) (local.get $provider0) (local.get $provider1) (local.get $provider2) (local.get $provider3) (local.get $provider4) (local.get $provider5) (local.get $provider6) (local.get $provider7) (local.get $provider8) (local.get $provider9))
  )

  (func $f5 (export "c5") (param $l0 i32) (param $provider0 (ref null $trait0)) (param $provider1 (ref null $trait9)) (param $provider2 (ref null $trait1)) (param $provider3 (ref null $trait2)) (param $provider4 (ref null $trait3)) (param $provider5 (ref null $trait4)) (param $provider6 (ref null $trait5)) (param $provider7 (ref null $trait6)) (param $provider8 (ref null $trait7)) (param $provider9 (ref null $trait8)) (result i32)
    (call $f4 (local.get $l0) (local.get $provider0) (local.get $provider1) (local.get $provider2) (local.get $provider3) (local.get $provider4) (local.get $provider5) (local.get $provider6) (local.get $provider7) (local.get $provider8) (local.get $provider9))
  )

  (func $fv5 (type $sig0) (param $env anyref) (param $l0 i32) (param $provider0 (ref null $trait0)) (param $provider1 (ref null $trait9)) (param $provider2 (ref null $trait1)) (param $provider3 (ref null $trait2)) (param $provider4 (ref null $trait3)) (param $provider5 (ref null $trait4)) (param $provider6 (ref null $trait5)) (param $provider7 (ref null $trait6)) (param $provider8 (ref null $trait7)) (param $provider9 (ref null $trait8)) (result i32)
    (call $f5 (local.get $l0) (local.get $provider0) (local.get $provider1) (local.get $provider2) (local.get $provider3) (local.get $provider4) (local.get $provider5) (local.get $provider6) (local.get $provider7) (local.get $provider8) (local.get $provider9))
  )

  (func $f6 (export "c4") (param $l0 i32) (param $provider0 (ref null $trait0)) (param $provider1 (ref null $trait9)) (param $provider2 (ref null $trait1)) (param $provider3 (ref null $trait2)) (param $provider4 (ref null $trait3)) (param $provider5 (ref null $trait4)) (param $provider6 (ref null $trait5)) (param $provider7 (ref null $trait6)) (param $provider8 (ref null $trait7)) (param $provider9 (ref null $trait8)) (result i32)
    (call $f5 (local.get $l0) (local.get $provider0) (local.get $provider1) (local.get $provider2) (local.get $provider3) (local.get $provider4) (local.get $provider5) (local.get $provider6) (local.get $provider7) (local.get $provider8) (local.get $provider9))
  )

  (func $fv6 (type $sig0) (param $env anyref) (param $l0 i32) (param $provider0 (ref null $trait0)) (param $provider1 (ref null $trait9)) (param $provider2 (ref null $trait1)) (param $provider3 (ref null $trait2)) (param $provider4 (ref null $trait3)) (param $provider5 (ref null $trait4)) (param $provider6 (ref null $trait5)) (param $provider7 (ref null $trait6)) (param $provider8 (ref null $trait7)) (param $provider9 (ref null $trait8)) (result i32)
    (call $f6 (local.get $l0) (local.get $provider0) (local.get $provider1) (local.get $provider2) (local.get $provider3) (local.get $provider4) (local.get $provider5) (local.get $provider6) (local.get $provider7) (local.get $provider8) (local.get $provider9))
  )

  (func $f7 (export "c3") (param $l0 i32) (param $provider0 (ref null $trait0)) (param $provider1 (ref null $trait9)) (param $provider2 (ref null $trait1)) (param $provider3 (ref null $trait2)) (param $provider4 (ref null $trait3)) (param $provider5 (ref null $trait4)) (param $provider6 (ref null $trait5)) (param $provider7 (ref null $trait6)) (param $provider8 (ref null $trait7)) (param $provider9 (ref null $trait8)) (result i32)
    (call $f6 (local.get $l0) (local.get $provider0) (local.get $provider1) (local.get $provider2) (local.get $provider3) (local.get $provider4) (local.get $provider5) (local.get $provider6) (local.get $provider7) (local.get $provider8) (local.get $provider9))
  )

  (func $fv7 (type $sig0) (param $env anyref) (param $l0 i32) (param $provider0 (ref null $trait0)) (param $provider1 (ref null $trait9)) (param $provider2 (ref null $trait1)) (param $provider3 (ref null $trait2)) (param $provider4 (ref null $trait3)) (param $provider5 (ref null $trait4)) (param $provider6 (ref null $trait5)) (param $provider7 (ref null $trait6)) (param $provider8 (ref null $trait7)) (param $provider9 (ref null $trait8)) (result i32)
    (call $f7 (local.get $l0) (local.get $provider0) (local.get $provider1) (local.get $provider2) (local.get $provider3) (local.get $provider4) (local.get $provider5) (local.get $provider6) (local.get $provider7) (local.get $provider8) (local.get $provider9))
  )

  (func $f8 (export "c2") (param $l0 i32) (param $provider0 (ref null $trait0)) (param $provider1 (ref null $trait9)) (param $provider2 (ref null $trait1)) (param $provider3 (ref null $trait2)) (param $provider4 (ref null $trait3)) (param $provider5 (ref null $trait4)) (param $provider6 (ref null $trait5)) (param $provider7 (ref null $trait6)) (param $provider8 (ref null $trait7)) (param $provider9 (ref null $trait8)) (result i32)
    (call $f7 (local.get $l0) (local.get $provider0) (local.get $provider1) (local.get $provider2) (local.get $provider3) (local.get $provider4) (local.get $provider5) (local.get $provider6) (local.get $provider7) (local.get $provider8) (local.get $provider9))
  )

  (func $fv8 (type $sig0) (param $env anyref) (param $l0 i32) (param $provider0 (ref null $trait0)) (param $provider1 (ref null $trait9)) (param $provider2 (ref null $trait1)) (param $provider3 (ref null $trait2)) (param $provider4 (ref null $trait3)) (param $provider5 (ref null $trait4)) (param $provider6 (ref null $trait5)) (param $provider7 (ref null $trait6)) (param $provider8 (ref null $trait7)) (param $provider9 (ref null $trait8)) (result i32)
    (call $f8 (local.get $l0) (local.get $provider0) (local.get $provider1) (local.get $provider2) (local.get $provider3) (local.get $provider4) (local.get $provider5) (local.get $provider6) (local.get $provider7) (local.get $provider8) (local.get $provider9))
  )

  (func $f9 (export "c1") (param $l0 i32) (param $provider0 (ref null $trait0)) (param $provider1 (ref null $trait9)) (param $provider2 (ref null $trait1)) (param $provider3 (ref null $trait2)) (param $provider4 (ref null $trait3)) (param $provider5 (ref null $trait4)) (param $provider6 (ref null $trait5)) (param $provider7 (ref null $trait6)) (param $provider8 (ref null $trait7)) (param $provider9 (ref null $trait8)) (result i32)
    (call $f8 (local.get $l0) (local.get $provider0) (local.get $provider1) (local.get $provider2) (local.get $provider3) (local.get $provider4) (local.get $provider5) (local.get $provider6) (local.get $provider7) (local.get $provider8) (local.get $provider9))
  )

  (func $fv9 (type $sig0) (param $env anyref) (param $l0 i32) (param $provider0 (ref null $trait0)) (param $provider1 (ref null $trait9)) (param $provider2 (ref null $trait1)) (param $provider3 (ref null $trait2)) (param $provider4 (ref null $trait3)) (param $provider5 (ref null $trait4)) (param $provider6 (ref null $trait5)) (param $provider7 (ref null $trait6)) (param $provider8 (ref null $trait7)) (param $provider9 (ref null $trait8)) (result i32)
    (call $f9 (local.get $l0) (local.get $provider0) (local.get $provider1) (local.get $provider2) (local.get $provider3) (local.get $provider4) (local.get $provider5) (local.get $provider6) (local.get $provider7) (local.get $provider8) (local.get $provider9))
  )

  (func $f10 (export "main") (result i32)
    (local $l0 (ref null $trait0))
    (local $l1 (ref null $trait1))
    (local $l2 (ref null $trait2))
    (local $l3 (ref null $trait3))
    (local $l4 (ref null $trait4))
    (local $l5 (ref null $trait5))
    (local $l6 (ref null $trait6))
    (local $l7 (ref null $trait7))
    (local $l8 (ref null $trait8))
    (local $l9 (ref null $trait9))
    (local $l10 i32)
    (local $l11 i32)
    (local $tmp0 (ref null $d0))
    (local $tmp1 i32)
    (local $tmp2 (ref null $d1))
    (local $tmp3 i32)
    (local $tmp4 (ref null $d2))
    (local $tmp5 i32)
    (local $tmp6 (ref null $d3))
    (local $tmp7 i32)
    (local $tmp8 (ref null $d4))
    (local $tmp9 i32)
    (local $tmp10 (ref null $d5))
    (local $tmp11 i32)
    (local $tmp12 (ref null $d6))
    (local $tmp13 i32)
    (local $tmp14 (ref null $d7))
    (local $tmp15 i32)
    (local $tmp16 (ref null $d8))
    (local $tmp17 i32)
    (local $tmp18 (ref null $d9))
    (local $tmp19 i32)
    (local $tmp20 i32)
    (local $tmp21 i32)
    (local $tmp22 i32)
    (local $tmp23 i32)
    (local $tmp24 i32)
    (block (result i32)
      (local.set $l0 (block (result (ref null $trait0))
      (local.set $tmp0 (block (result (ref null $d0))
      (local.set $tmp1 (i32.const 1))
      (struct.new $d0 (local.get $tmp1))
    ))
      (struct.new $trait0 (local.get $tmp0) (ref.null $hd.list) (ref.func $tadapt0_0))
    ))
      (local.set $l1 (block (result (ref null $trait1))
      (local.set $tmp2 (block (result (ref null $d1))
      (local.set $tmp3 (i32.const 1))
      (struct.new $d1 (local.get $tmp3))
    ))
      (struct.new $trait1 (local.get $tmp2) (ref.null $hd.list) (ref.func $tadapt1_0))
    ))
      (local.set $l2 (block (result (ref null $trait2))
      (local.set $tmp4 (block (result (ref null $d2))
      (local.set $tmp5 (i32.const 1))
      (struct.new $d2 (local.get $tmp5))
    ))
      (struct.new $trait2 (local.get $tmp4) (ref.null $hd.list) (ref.func $tadapt2_0))
    ))
      (local.set $l3 (block (result (ref null $trait3))
      (local.set $tmp6 (block (result (ref null $d3))
      (local.set $tmp7 (i32.const 1))
      (struct.new $d3 (local.get $tmp7))
    ))
      (struct.new $trait3 (local.get $tmp6) (ref.null $hd.list) (ref.func $tadapt3_0))
    ))
      (local.set $l4 (block (result (ref null $trait4))
      (local.set $tmp8 (block (result (ref null $d4))
      (local.set $tmp9 (i32.const 1))
      (struct.new $d4 (local.get $tmp9))
    ))
      (struct.new $trait4 (local.get $tmp8) (ref.null $hd.list) (ref.func $tadapt4_0))
    ))
      (local.set $l5 (block (result (ref null $trait5))
      (local.set $tmp10 (block (result (ref null $d5))
      (local.set $tmp11 (i32.const 1))
      (struct.new $d5 (local.get $tmp11))
    ))
      (struct.new $trait5 (local.get $tmp10) (ref.null $hd.list) (ref.func $tadapt5_0))
    ))
      (local.set $l6 (block (result (ref null $trait6))
      (local.set $tmp12 (block (result (ref null $d6))
      (local.set $tmp13 (i32.const 1))
      (struct.new $d6 (local.get $tmp13))
    ))
      (struct.new $trait6 (local.get $tmp12) (ref.null $hd.list) (ref.func $tadapt6_0))
    ))
      (local.set $l7 (block (result (ref null $trait7))
      (local.set $tmp14 (block (result (ref null $d7))
      (local.set $tmp15 (i32.const 1))
      (struct.new $d7 (local.get $tmp15))
    ))
      (struct.new $trait7 (local.get $tmp14) (ref.null $hd.list) (ref.func $tadapt7_0))
    ))
      (local.set $l8 (block (result (ref null $trait8))
      (local.set $tmp16 (block (result (ref null $d8))
      (local.set $tmp17 (i32.const 1))
      (struct.new $d8 (local.get $tmp17))
    ))
      (struct.new $trait8 (local.get $tmp16) (ref.null $hd.list) (ref.func $tadapt8_0))
    ))
      (local.set $l9 (block (result (ref null $trait9))
      (local.set $tmp18 (block (result (ref null $d9))
      (local.set $tmp19 (i32.const 1))
      (struct.new $d9 (local.get $tmp19))
    ))
      (struct.new $trait9 (local.get $tmp18) (ref.null $hd.list) (ref.func $tadapt9_0))
    ))
      (local.set $l10 (i32.const 0))
      (local.set $l11 (i32.const 0))
      (block $break0
        (loop $loop0
          (br_if $break0 (i32.eqz (block (result i32) (local.set $tmp24 (if (result i32) (i32.lt_s (local.get $l10) (i32.const 200000)) (then (i32.const -1)) (else (if (result i32) (i32.gt_s (local.get $l10) (i32.const 200000)) (then (i32.const 1)) (else (i32.const 0)))))) (i32.eq (local.get $tmp24) (i32.const -1)))))
          (local.set $l11 (block (result i32)
            (local.set $tmp22 (call $hd.add_i32 (local.get $l11) (call $f9 (block (result i32)
            (local.set $tmp20 (local.get $l10))
            (local.set $tmp21 (i32.const 7))
            (if (i32.eqz (local.get $tmp21))
              (then (call $hd.panic (i32.const 1)) unreachable))
            (i32.rem_s (local.get $tmp20) (local.get $tmp21))
          ) (local.get $l0) (local.get $l9) (local.get $l1) (local.get $l2) (local.get $l3) (local.get $l4) (local.get $l5) (local.get $l6) (local.get $l7) (local.get $l8))))
            (local.set $tmp23 (i32.const 1000003))
            (if (i32.eqz (local.get $tmp23))
              (then (call $hd.panic (i32.const 1)) unreachable))
            (i32.rem_s (local.get $tmp22) (local.get $tmp23))
          ))
          (local.set $l10 (call $hd.add_i32 (local.get $l10) (i32.const 1)))
          (br $loop0)
        )
      )
      (local.get $l11)
    )
  )

  (func $fv10 (type $sig1) (param $env anyref)  (result i32)
    (call $f10)
  )

  (func $f11 (param $l0 (ref null $d0)) (result i32)
    (struct.get $d0 $d0f0 (local.get $l0))
  )

  (func $fv11 (type $sig2) (param $env anyref) (param $l0 (ref null $d0)) (result i32)
    (call $f11 (local.get $l0))
  )

  (func $f12 (param $l0 (ref null $d1)) (result i32)
    (struct.get $d1 $d1f0 (local.get $l0))
  )

  (func $fv12 (type $sig3) (param $env anyref) (param $l0 (ref null $d1)) (result i32)
    (call $f12 (local.get $l0))
  )

  (func $f13 (param $l0 (ref null $d2)) (result i32)
    (struct.get $d2 $d2f0 (local.get $l0))
  )

  (func $fv13 (type $sig4) (param $env anyref) (param $l0 (ref null $d2)) (result i32)
    (call $f13 (local.get $l0))
  )

  (func $f14 (param $l0 (ref null $d3)) (result i32)
    (struct.get $d3 $d3f0 (local.get $l0))
  )

  (func $fv14 (type $sig5) (param $env anyref) (param $l0 (ref null $d3)) (result i32)
    (call $f14 (local.get $l0))
  )

  (func $f15 (param $l0 (ref null $d4)) (result i32)
    (struct.get $d4 $d4f0 (local.get $l0))
  )

  (func $fv15 (type $sig6) (param $env anyref) (param $l0 (ref null $d4)) (result i32)
    (call $f15 (local.get $l0))
  )

  (func $f16 (param $l0 (ref null $d5)) (result i32)
    (struct.get $d5 $d5f0 (local.get $l0))
  )

  (func $fv16 (type $sig7) (param $env anyref) (param $l0 (ref null $d5)) (result i32)
    (call $f16 (local.get $l0))
  )

  (func $f17 (param $l0 (ref null $d6)) (result i32)
    (struct.get $d6 $d6f0 (local.get $l0))
  )

  (func $fv17 (type $sig8) (param $env anyref) (param $l0 (ref null $d6)) (result i32)
    (call $f17 (local.get $l0))
  )

  (func $f18 (param $l0 (ref null $d7)) (result i32)
    (struct.get $d7 $d7f0 (local.get $l0))
  )

  (func $fv18 (type $sig9) (param $env anyref) (param $l0 (ref null $d7)) (result i32)
    (call $f18 (local.get $l0))
  )

  (func $f19 (param $l0 (ref null $d8)) (result i32)
    (struct.get $d8 $d8f0 (local.get $l0))
  )

  (func $fv19 (type $sig10) (param $env anyref) (param $l0 (ref null $d8)) (result i32)
    (call $f19 (local.get $l0))
  )

  (func $f20 (param $l0 (ref null $d9)) (result i32)
    (struct.get $d9 $d9f0 (local.get $l0))
  )

  (func $fv20 (type $sig11) (param $env anyref) (param $l0 (ref null $d9)) (result i32)
    (call $f20 (local.get $l0))
  )

  (func $tadapt0_0 (type $tsig0_0) (param $self anyref) (param $dictionary anyref)  (result i32)
    (call $f11 (ref.cast (ref null $d0) (local.get $self)))
  )
  
  (func $tadapt1_0 (type $tsig1_0) (param $self anyref) (param $dictionary anyref)  (result i32)
    (call $f12 (ref.cast (ref null $d1) (local.get $self)))
  )
  
  (func $tadapt2_0 (type $tsig2_0) (param $self anyref) (param $dictionary anyref)  (result i32)
    (call $f13 (ref.cast (ref null $d2) (local.get $self)))
  )
  
  (func $tadapt3_0 (type $tsig3_0) (param $self anyref) (param $dictionary anyref)  (result i32)
    (call $f14 (ref.cast (ref null $d3) (local.get $self)))
  )
  
  (func $tadapt4_0 (type $tsig4_0) (param $self anyref) (param $dictionary anyref)  (result i32)
    (call $f15 (ref.cast (ref null $d4) (local.get $self)))
  )
  
  (func $tadapt5_0 (type $tsig5_0) (param $self anyref) (param $dictionary anyref)  (result i32)
    (call $f16 (ref.cast (ref null $d5) (local.get $self)))
  )
  
  (func $tadapt6_0 (type $tsig6_0) (param $self anyref) (param $dictionary anyref)  (result i32)
    (call $f17 (ref.cast (ref null $d6) (local.get $self)))
  )
  
  (func $tadapt7_0 (type $tsig7_0) (param $self anyref) (param $dictionary anyref)  (result i32)
    (call $f18 (ref.cast (ref null $d7) (local.get $self)))
  )
  
  (func $tadapt8_0 (type $tsig8_0) (param $self anyref) (param $dictionary anyref)  (result i32)
    (call $f19 (ref.cast (ref null $d8) (local.get $self)))
  )
  
  (func $tadapt9_0 (type $tsig9_0) (param $self anyref) (param $dictionary anyref)  (result i32)
    (call $f20 (ref.cast (ref null $d9) (local.get $self)))
  )
)