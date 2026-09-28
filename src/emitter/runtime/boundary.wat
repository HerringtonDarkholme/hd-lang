  ;; The string half of the generic host-function boundary (emitter/intrinsics.ts):
  ;; a `string` crosses as a host handle whose UTF-8 bytes are copied one by one.
  (func $hd.string_to_host (param $value (ref null $hd.bytes)) (result externref)
    (local $bytes (ref $hd.bytes))
    (local $handle externref)
    (local $index i32)
    (local.set $bytes (ref.as_non_null (local.get $value)))
    (local.set $handle (call $hd.host_string_new (array.len (local.get $bytes))))
    (block $done
      (loop $next
        (br_if $done (i32.ge_u (local.get $index) (array.len (local.get $bytes))))
        (call $hd.host_string_set
          (local.get $handle)
          (local.get $index)
          (array.get_u $hd.bytes (local.get $bytes) (local.get $index)))
        (local.set $index (i32.add (local.get $index) (i32.const 1)))
        (br $next)))
    (local.get $handle))

  (func $hd.string_from_host (param $handle externref) (result (ref null $hd.bytes))
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
    (local.get $result))
