  (func $hd.string_transform
    (param $value (ref null $hd.bytes))
    (param $operation i32)
    (result (ref null $hd.bytes))
    (local $source (ref $hd.bytes))
    (local $result (ref $hd.bytes))
    (local $index i32)
    (local $length i32)
    (local.set $source (ref.as_non_null (local.get $value)))
    (call $hd.string_transform_begin (local.get $operation))
    (block $input-done
      (loop $input-next
        (br_if $input-done
          (i32.ge_u (local.get $index) (array.len (local.get $source))))
        (call $hd.string_transform_input
          (array.get_u $hd.bytes (local.get $source) (local.get $index)))
        (local.set $index (i32.add (local.get $index) (i32.const 1)))
        (br $input-next)))
    (local.set $length (call $hd.string_transform_output (i32.const -1)))
    (local.set $result (array.new_default $hd.bytes (local.get $length)))
    (local.set $index (i32.const 0))
    (block $output-done
      (loop $output-next
        (br_if $output-done (i32.ge_u (local.get $index) (local.get $length)))
        (array.set $hd.bytes
          (local.get $result)
          (local.get $index)
          (call $hd.string_transform_output (local.get $index)))
        (local.set $index (i32.add (local.get $index) (i32.const 1)))
        (br $output-next)))
    (local.get $result))

  (func $hd.string_trim
    (param $value (ref null $hd.bytes))
    (result (ref null $hd.bytes))
    (call $hd.string_transform (local.get $value) (i32.const 0)))

  (func $hd.string_lower
    (param $value (ref null $hd.bytes))
    (result (ref null $hd.bytes))
    (call $hd.string_transform (local.get $value) (i32.const 1)))
