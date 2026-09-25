  (func $hd.console_print
    (param $provider externref)
    (param $value-source (ref null $hd.bytes))
    (local $value (ref $hd.bytes))
    (local $index i32)
    (local.set $value (ref.as_non_null (local.get $value-source)))
    (block $done
      (loop $next
        (br_if $done (i32.ge_u (local.get $index) (array.len (local.get $value))))
        (call $hd.console_byte
          (local.get $provider)
          (array.get_u $hd.bytes (local.get $value) (local.get $index)))
        (local.set $index (i32.add (local.get $index) (i32.const 1)))
        (br $next)))
    (call $hd.console_byte (local.get $provider) (i32.const -1)))