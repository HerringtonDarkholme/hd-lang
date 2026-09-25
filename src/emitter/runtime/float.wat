  (func $hd.f64_to_string
    (param $value f64)
    (result (ref null $hd.bytes))
    (local $length i32)
    (local $index i32)
    (local $result (ref $hd.bytes))
    (local.set $length
      (call $hd.format_f64 (local.get $value) (i32.const -1)))
    (local.set $result
      (array.new_default $hd.bytes (local.get $length)))
    (block $done
      (loop $write
        (br_if $done
          (i32.ge_u (local.get $index) (local.get $length)))
        (array.set $hd.bytes
          (local.get $result)
          (local.get $index)
          (call $hd.format_f64
            (local.get $value)
            (local.get $index)))
        (local.set $index
          (i32.add (local.get $index) (i32.const 1)))
        (br $write)))
    (local.get $result))
