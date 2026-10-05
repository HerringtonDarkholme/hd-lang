  (global $hd.runtime-root
    (mut (ref null $hd.runtime))
    (ref.null $hd.runtime))

  (global $hd.driver-active (mut i32) (i32.const 0))
  (global $hd.host-call-function (mut i32) (i32.const -1))
  (global $hd.host-call-site (mut i32) (i32.const -1))

  ;; Stable numeric tags for runtime-panic.ts. The host reports their names.
  (global $hd.panic-integer-overflow i32 (i32.const 2))
  (global $hd.panic-invalid-shift i32 (i32.const 8))
  (global $hd.panic-index-out-of-bounds i32 (i32.const 9))

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

  ;; A u64 index is checked before narrowing, so 2^32 cannot wrap to 0.
  (func $hd.vector_get_wide
    (param $vector (ref $hd.vector))
    (param $index i64)
    (result anyref)
    (if (i64.ge_u
      (local.get $index)
      (i64.extend_i32_u (struct.get $hd.vector $hd.vector-size (local.get $vector))))
      (then (call $hd.panic (global.get $hd.panic-index-out-of-bounds)) unreachable))
    (call $hd.vector_get (local.get $vector) (i32.wrap_i64 (local.get $index))))

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

  ;; A u64 index is checked before narrowing, so 2^32 cannot wrap to 0.
  (func $hd.vector_set_wide
    (param $vector (ref $hd.vector))
    (param $index i64)
    (param $value anyref)
    (if (i64.ge_u
      (local.get $index)
      (i64.extend_i32_u (struct.get $hd.vector $hd.vector-size (local.get $vector))))
      (then (call $hd.panic (global.get $hd.panic-index-out-of-bounds)) unreachable))
    (call $hd.vector_set
      (local.get $vector)
      (i32.wrap_i64 (local.get $index))
      (local.get $value)))

  (func $hd.vector_push
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
      (i32.add (local.get $size) (i32.const 1))))

  ;; Drops the elements from `length` on and clears their slots. A length past the size is a bug in the caller.
  (func $hd.vector_truncate
    (param $vector (ref $hd.vector))
    (param $length i32)
    (local $size i32)
    (local.set $size (struct.get $hd.vector $hd.vector-size (local.get $vector)))
    (if (i32.gt_u (local.get $length) (local.get $size))
      (then (call $hd.panic (global.get $hd.panic-index-out-of-bounds)) unreachable))
    (array.fill $hd.list
      (struct.get $hd.vector $hd.vector-values (local.get $vector))
      (local.get $length)
      (ref.null none)
      (i32.sub (local.get $size) (local.get $length)))
    (struct.set $hd.vector $hd.vector-size (local.get $vector) (local.get $length)))

  (func $hd.add_i32 (param $left i32) (param $right i32) (result i32)
    (local $wide i64)
    (local.set $wide
      (i64.add (i64.extend_i32_s (local.get $left)) (i64.extend_i32_s (local.get $right))))
    (if (i32.or
      (i64.lt_s (local.get $wide) (i64.const -2147483648))
      (i64.gt_s (local.get $wide) (i64.const 2147483647)))
      (then (call $hd.panic (global.get $hd.panic-integer-overflow)) unreachable))
    (i32.wrap_i64 (local.get $wide)))

  ;; A u8 result computed as an i32: panics unless it is in 0..255.
  (func $hd.check_u8 (param $value i32) (result i32)
    (if (i32.gt_u (local.get $value) (i32.const 255))
      (then (call $hd.panic (global.get $hd.panic-integer-overflow)) unreachable))
    (local.get $value))

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

  ;; Checked i64 arithmetic: a sum or difference overflows when its sign
  ;; differs from both operands' (for a sum) or from the left one's.
  (func $hd.add_i64 (param $left i64) (param $right i64) (result i64)
    (local $sum i64)
    (local.set $sum (i64.add (local.get $left) (local.get $right)))
    (if (i64.lt_s
      (i64.and
        (i64.xor (local.get $left) (local.get $sum))
        (i64.xor (local.get $right) (local.get $sum)))
      (i64.const 0))
      (then (call $hd.panic (global.get $hd.panic-integer-overflow)) unreachable))
    (local.get $sum))

  (func $hd.sub_i64 (param $left i64) (param $right i64) (result i64)
    (local $difference i64)
    (local.set $difference (i64.sub (local.get $left) (local.get $right)))
    (if (i64.lt_s
      (i64.and
        (i64.xor (local.get $left) (local.get $right))
        (i64.xor (local.get $left) (local.get $difference)))
      (i64.const 0))
      (then (call $hd.panic (global.get $hd.panic-integer-overflow)) unreachable))
    (local.get $difference))

  (func $hd.mul_i64 (param $left i64) (param $right i64) (result i64)
    (local $product i64)
    (if (i32.or
      (i32.and
        (i64.eq (local.get $left) (i64.const -1))
        (i64.eq (local.get $right) (i64.const -9223372036854775808)))
      (i32.and
        (i64.eq (local.get $right) (i64.const -1))
        (i64.eq (local.get $left) (i64.const -9223372036854775808))))
      (then (call $hd.panic (global.get $hd.panic-integer-overflow)) unreachable))
    (local.set $product (i64.mul (local.get $left) (local.get $right)))
    ;; `i32.and` evaluates both operands, so a zero `left` must not reach the
    ;; division: test it in its own `if`.
    (if (i64.ne (local.get $left) (i64.const 0))
      (then
        (if (i64.ne (i64.div_s (local.get $product) (local.get $left)) (local.get $right))
          (then (call $hd.panic (global.get $hd.panic-integer-overflow)) unreachable))))
    (local.get $product))

  (func $hd.pow_i64 (param $base i64) (param $exponent i32) (result i64)
    (local $result i64)
    (if (i32.lt_s (local.get $exponent) (i32.const 0)) (then unreachable))
    (local.set $result (i64.const 1))
    (block $done
      (loop $next
        (br_if $done (i32.eqz (local.get $exponent)))
        (if (i32.and (local.get $exponent) (i32.const 1))
          (then (local.set $result (call $hd.mul_i64 (local.get $result) (local.get $base)))))
        (local.set $exponent (i32.shr_u (local.get $exponent) (i32.const 1)))
        (if (local.get $exponent)
          (then (local.set $base (call $hd.mul_i64 (local.get $base) (local.get $base)))))
        (br $next)))
    (local.get $result))

  (func $hd.neg_i64 (param $value i64) (result i64)
    (if (i64.eq (local.get $value) (i64.const -9223372036854775808))
      (then (call $hd.panic (global.get $hd.panic-integer-overflow)) unreachable))
    (i64.sub (i64.const 0) (local.get $value)))

  ;; Sized integers (src/numeric.ts). An integer of at most 16 bits is an
  ;; i32 whose result is range-checked; u32 is an i32 and u64 an i64, both
  ;; read as unsigned.
  (func $hd.check_range_i32 (param $value i32) (param $minimum i32) (param $maximum i32) (result i32)
    (if (i32.or
      (i32.lt_s (local.get $value) (local.get $minimum))
      (i32.gt_s (local.get $value) (local.get $maximum)))
      (then (call $hd.panic (global.get $hd.panic-integer-overflow)) unreachable))
    (local.get $value))

  (func $hd.check_range_i64 (param $value i64) (param $minimum i64) (param $maximum i64) (result i64)
    (if (i32.or
      (i64.lt_s (local.get $value) (local.get $minimum))
      (i64.gt_s (local.get $value) (local.get $maximum)))
      (then (call $hd.panic (global.get $hd.panic-integer-overflow)) unreachable))
    (local.get $value))

  (func $hd.check_shift (param $count i32) (param $bits i32) (result i32)
    (if (i32.ge_u (local.get $count) (local.get $bits))
      (then (call $hd.panic (global.get $hd.panic-invalid-shift)) unreachable))
    (local.get $count))

  (func $hd.check_shift_i64 (param $count i64) (result i64)
    (if (i64.ge_u (local.get $count) (i64.const 64))
      (then (call $hd.panic (global.get $hd.panic-invalid-shift)) unreachable))
    (local.get $count))

  (func $hd.check_u32 (param $value i64) (result i32)
    (if (i64.gt_u (local.get $value) (i64.const 4294967295))
      (then (call $hd.panic (global.get $hd.panic-integer-overflow)) unreachable))
    (i32.wrap_i64 (local.get $value)))

  (func $hd.add_u64 (param $left i64) (param $right i64) (result i64)
    (local $sum i64)
    (local.set $sum (i64.add (local.get $left) (local.get $right)))
    (if (i64.lt_u (local.get $sum) (local.get $left))
      (then (call $hd.panic (global.get $hd.panic-integer-overflow)) unreachable))
    (local.get $sum))

  (func $hd.sub_u64 (param $left i64) (param $right i64) (result i64)
    (if (i64.lt_u (local.get $left) (local.get $right))
      (then (call $hd.panic (global.get $hd.panic-integer-overflow)) unreachable))
    (i64.sub (local.get $left) (local.get $right)))

  (func $hd.mul_u64 (param $left i64) (param $right i64) (result i64)
    (local $product i64)
    (local.set $product (i64.mul (local.get $left) (local.get $right)))
    ;; `i32.and` evaluates both operands, so a zero `left` must not reach the
    ;; division: test it in its own `if`.
    (if (i64.ne (local.get $left) (i64.const 0))
      (then
        (if (i64.ne (i64.div_u (local.get $product) (local.get $left)) (local.get $right))
          (then (call $hd.panic (global.get $hd.panic-integer-overflow)) unreachable))))
    (local.get $product))

  (func $hd.pow_u64 (param $base i64) (param $exponent i32) (result i64)
    (local $result i64)
    (local.set $result (i64.const 1))
    (block $done
      (loop $next
        (br_if $done (i32.eqz (local.get $exponent)))
        (if (i32.and (local.get $exponent) (i32.const 1))
          (then (local.set $result (call $hd.mul_u64 (local.get $result) (local.get $base)))))
        (local.set $exponent (i32.shr_u (local.get $exponent) (i32.const 1)))
        (if (local.get $exponent)
          (then (local.set $base (call $hd.mul_u64 (local.get $base) (local.get $base)))))
        (br $next)))
    (local.get $result))

  (func $hd.shl_i32 (param $value i32) (param $count i32) (result i32)
    (if (i32.ge_u (local.get $count) (i32.const 32))
      (then (call $hd.panic (global.get $hd.panic-invalid-shift)) unreachable))
    (i32.shl (local.get $value) (local.get $count)))

  (func $hd.shr_i32 (param $value i32) (param $count i32) (result i32)
    (if (i32.ge_u (local.get $count) (i32.const 32))
      (then (call $hd.panic (global.get $hd.panic-invalid-shift)) unreachable))
    (i32.shr_s (local.get $value) (local.get $count)))

  ;; A string is a `$hd.string`: `length` bytes of an immutable `$hd.bytes`
  ;; array from offset `start`, so a slice shares its source's bytes
  ;; (spec/lang/10-modules.md#r-module.string.slice.shared).

  ;; `text[index]`, and std's `bytes_at`: the byte at a byte offset, or an
  ;; index-out-of-bounds panic (spec/lang/05-expressions.md#string-indexing).
  ;; A narrow index is read as unsigned, so a negative one is out of range too.
  (func $hd.string_get (param $text (ref $hd.string)) (param $index i32) (result i32)
    (if (i32.ge_u (local.get $index) (struct.get $hd.string $hd.string-length (local.get $text)))
      (then (call $hd.panic (global.get $hd.panic-index-out-of-bounds)) unreachable))
    (array.get_u $hd.bytes
      (struct.get $hd.string $hd.string-bytes (local.get $text))
      (i32.add (struct.get $hd.string $hd.string-start (local.get $text)) (local.get $index))))

  (func $hd.string_get_wide (param $text (ref $hd.string)) (param $index i64) (result i32)
    (if (i64.ge_u
      (local.get $index)
      (i64.extend_i32_u (struct.get $hd.string $hd.string-length (local.get $text))))
      (then (call $hd.panic (global.get $hd.panic-index-out-of-bounds)) unreachable))
    (call $hd.string_get (local.get $text) (i32.wrap_i64 (local.get $index))))

  ;; std's `bytes_concat`: `left`'s bytes, then `right`'s, in a new array.
  (func $hd.bytes_concat
    (param $left (ref $hd.string))
    (param $right (ref $hd.string))
    (result (ref $hd.string))
    (local $left-length i32)
    (local $length i32)
    (local $bytes (ref $hd.bytes))
    (local.set $left-length (struct.get $hd.string $hd.string-length (local.get $left)))
    (if (i32.eqz (struct.get $hd.string $hd.string-length (local.get $right)))
      (then (return (local.get $left))))
    (if (i32.eqz (local.get $left-length)) (then (return (local.get $right))))
    (local.set $length
      (i32.add
        (local.get $left-length)
        (struct.get $hd.string $hd.string-length (local.get $right))))
    (if (i32.lt_u (local.get $length) (local.get $left-length)) (then unreachable))
    (local.set $bytes (array.new_default $hd.bytes (local.get $length)))
    (array.copy $hd.bytes $hd.bytes
      (local.get $bytes)
      (i32.const 0)
      (struct.get $hd.string $hd.string-bytes (local.get $left))
      (struct.get $hd.string $hd.string-start (local.get $left))
      (local.get $left-length))
    (array.copy $hd.bytes $hd.bytes
      (local.get $bytes)
      (local.get $left-length)
      (struct.get $hd.string $hd.string-bytes (local.get $right))
      (struct.get $hd.string $hd.string-start (local.get $right))
      (i32.sub (local.get $length) (local.get $left-length)))
    (struct.new $hd.string (local.get $bytes) (i32.const 0) (local.get $length)))

  ;; std's `string_from_bytes`: a new string of a `List[u8]`'s bytes, each
  ;; boxed as `$hd.box-i32`.
  (func $hd.string_from_bytes (param $items (ref $hd.vector)) (result (ref $hd.string))
    (local $length i32)
    (local $index i32)
    (local $bytes (ref $hd.bytes))
    (local.set $length (struct.get $hd.vector $hd.vector-size (local.get $items)))
    (local.set $bytes (array.new_default $hd.bytes (local.get $length)))
    (block $done
      (loop $next
        (br_if $done (i32.ge_u (local.get $index) (local.get $length)))
        (array.set $hd.bytes
          (local.get $bytes)
          (local.get $index)
          (struct.get $hd.box-i32 $hd.box-i32-value
            (ref.cast (ref $hd.box-i32)
              (array.get $hd.list
                (struct.get $hd.vector $hd.vector-values (local.get $items))
                (local.get $index)))))
        (local.set $index (i32.add (local.get $index) (i32.const 1)))
        (br $next)))
    (struct.new $hd.string (local.get $bytes) (i32.const 0) (local.get $length)))
