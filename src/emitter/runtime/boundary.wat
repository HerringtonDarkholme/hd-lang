  ;; The string half of the generic host-function boundary (emitter/intrinsics.ts):
  ;; a `string` crosses as a host handle whose UTF-8 bytes are copied one by one.
  (func $hd.string_to_host (param $value (ref null $hd.string)) (result externref)
    (local $text (ref $hd.string))
    (local $handle externref)
    (local $index i32)
    (local.set $text (ref.as_non_null (local.get $value)))
    (local.set $handle
      (call $hd.host_string_new (struct.get $hd.string $hd.string-length (local.get $text))))
    (block $done
      (loop $next
        (br_if $done
          (i32.ge_u (local.get $index) (struct.get $hd.string $hd.string-length (local.get $text))))
        (call $hd.host_string_set
          (local.get $handle)
          (local.get $index)
          (call $hd.string_get (local.get $text) (local.get $index)))
        (local.set $index (i32.add (local.get $index) (i32.const 1)))
        (br $next)))
    (local.get $handle))

  (func $hd.string_from_host (param $handle externref) (result (ref null $hd.string))
    (local $length i32)
    (local $index i32)
    (local $result (ref $hd.bytes))
    (local.set $length (call $hd.host_string_length (local.get $handle)))
    (local.set $result (array.new_default $hd.bytes (local.get $length)))
    (block $done
      (loop $next
        (br_if $done (i32.ge_u (local.get $index) (local.get $length)))
        (array.set $hd.bytes
          (local.get $result)
          (local.get $index)
          (call $hd.host_string_get (local.get $handle) (local.get $index)))
        (local.set $index (i32.add (local.get $index) (i32.const 1)))
        (br $next)))
    (struct.new $hd.string (local.get $result) (i32.const 0) (local.get $length)))
