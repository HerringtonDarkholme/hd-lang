  (func $hd.string_slice
    (param $source (ref $hd.bytes))
    (param $start i32)
    (param $end i32)
    (result (ref null $hd.bytes))
    (local $result (ref $hd.bytes))
    (local $index i32)
    (local $length i32)
    (local.set $length (i32.sub (local.get $end) (local.get $start)))
    (local.set $result (array.new_default $hd.bytes (local.get $length)))
    (block $done
      (loop $next
        (br_if $done (i32.ge_u (local.get $index) (local.get $length)))
        (array.set $hd.bytes
          (local.get $result)
          (local.get $index)
          (array.get_u $hd.bytes
            (local.get $source)
            (i32.add (local.get $start) (local.get $index))))
        (local.set $index (i32.add (local.get $index) (i32.const 1)))
        (br $next)))
    (local.get $result))

  (func $hd.string_match_at
    (param $value (ref $hd.bytes))
    (param $separator (ref $hd.bytes))
    (param $position i32)
    (result i32)
    (local $index i32)
    (local $separator-length i32)
    (local.set $separator-length (array.len (local.get $separator)))
    (if
      (i32.gt_u
        (local.get $separator-length)
        (i32.sub (array.len (local.get $value)) (local.get $position)))
      (then (return (i32.const 0))))
    (block $matched
      (loop $next
        (br_if $matched (i32.ge_u (local.get $index) (local.get $separator-length)))
        (if
          (i32.ne
            (array.get_u $hd.bytes
              (local.get $value)
              (i32.add (local.get $position) (local.get $index)))
            (array.get_u $hd.bytes (local.get $separator) (local.get $index)))
          (then (return (i32.const 0))))
        (local.set $index (i32.add (local.get $index) (i32.const 1)))
        (br $next)))
    (i32.const 1))

  (func $hd.string_split_empty
    (param $value (ref $hd.bytes))
    (result (ref null $hd.vector))
    (local $result (ref $hd.vector))
    (local $values (ref $hd.list))
    (local $length i32)
    (local $position i32)
    (local $start i32)
    (local $piece i32)
    (local.set $length (array.len (local.get $value)))
    (local.set $values
      (array.new_default $hd.list (call $hd.string_len (local.get $value))))
    (local.set $result
      (struct.new $hd.vector
        (array.len (local.get $values))
        (local.get $values)
        (i32.const 0)))
    (block $done
      (loop $next-scalar
        (br_if $done (i32.ge_u (local.get $position) (local.get $length)))
        (local.set $start (local.get $position))
        (local.set $position (i32.add (local.get $position) (i32.const 1)))
        (block $scalar-done
          (loop $next-byte
            (br_if $scalar-done (i32.ge_u (local.get $position) (local.get $length)))
            (br_if $scalar-done
              (i32.ne
                (i32.and
                  (array.get_u $hd.bytes (local.get $value) (local.get $position))
                  (i32.const 192))
                (i32.const 128)))
            (local.set $position (i32.add (local.get $position) (i32.const 1)))
            (br $next-byte)))
        (array.set $hd.list
          (local.get $values)
          (local.get $piece)
          (call $hd.string_slice
            (local.get $value)
            (local.get $start)
            (local.get $position)))
        (local.set $piece (i32.add (local.get $piece) (i32.const 1)))
        (br $next-scalar)))
    (local.get $result))

  (func $hd.string_split
    (param $value-source (ref null $hd.bytes))
    (param $separator-source (ref null $hd.bytes))
    (result (ref null $hd.vector))
    (local $value (ref $hd.bytes))
    (local $separator (ref $hd.bytes))
    (local $result (ref $hd.vector))
    (local $values (ref $hd.list))
    (local $value-length i32)
    (local $separator-length i32)
    (local $position i32)
    (local $start i32)
    (local $piece i32)
    (local $piece-count i32)
    (local.set $value (ref.as_non_null (local.get $value-source)))
    (local.set $separator (ref.as_non_null (local.get $separator-source)))
    (local.set $value-length (array.len (local.get $value)))
    (local.set $separator-length (array.len (local.get $separator)))
    (if (i32.eqz (local.get $separator-length))
      (then (return (call $hd.string_split_empty (local.get $value)))))
    (local.set $piece-count (i32.const 1))
    (block $counted
      (loop $count
        (br_if $counted
          (i32.gt_u (local.get $separator-length)
            (i32.sub (local.get $value-length) (local.get $position))))
        (if (call $hd.string_match_at
              (local.get $value) (local.get $separator) (local.get $position))
          (then
            (local.set $piece-count
              (i32.add (local.get $piece-count) (i32.const 1)))
            (local.set $position
              (i32.add (local.get $position) (local.get $separator-length))))
          (else
            (local.set $position (i32.add (local.get $position) (i32.const 1)))))
        (br $count)))
    (local.set $values (array.new_default $hd.list (local.get $piece-count)))
    (local.set $result
      (struct.new $hd.vector
        (local.get $piece-count)
        (local.get $values)
        (i32.const 0)))
    (local.set $position (i32.const 0))
    (block $split-done
      (loop $split
        (br_if $split-done
          (i32.gt_u (local.get $separator-length)
            (i32.sub (local.get $value-length) (local.get $position))))
        (if (call $hd.string_match_at
              (local.get $value) (local.get $separator) (local.get $position))
          (then
            (array.set $hd.list
              (local.get $values)
              (local.get $piece)
              (call $hd.string_slice
                (local.get $value) (local.get $start) (local.get $position)))
            (local.set $piece (i32.add (local.get $piece) (i32.const 1)))
            (local.set $position
              (i32.add (local.get $position) (local.get $separator-length)))
            (local.set $start (local.get $position)))
          (else
            (local.set $position (i32.add (local.get $position) (i32.const 1)))))
        (br $split)))
    (array.set $hd.list
      (local.get $values)
      (local.get $piece)
      (call $hd.string_slice
        (local.get $value) (local.get $start) (local.get $value-length)))
    (local.get $result))

  ;; Copies `source` into `target` at `offset` and returns the offset after it.
  (func $hd.string_copy_into
    (param $target (ref $hd.bytes))
    (param $offset i32)
    (param $source (ref $hd.bytes))
    (result i32)
    (local $index i32)
    (block $done
      (loop $next
        (br_if $done (i32.ge_u (local.get $index) (array.len (local.get $source))))
        (array.set $hd.bytes
          (local.get $target)
          (i32.add (local.get $offset) (local.get $index))
          (array.get_u $hd.bytes (local.get $source) (local.get $index)))
        (local.set $index (i32.add (local.get $index) (i32.const 1)))
        (br $next)))
    (i32.add (local.get $offset) (array.len (local.get $source))))

  ;; `replace` substitutes non-overlapping matches from left to right; an empty
  ;; `old` inserts the replacement at every scalar boundary. A valid UTF-8
  ;; needle can only match at a scalar boundary, so matching compares bytes.
  (func $hd.string_replace
    (param $value-source (ref null $hd.bytes))
    (param $old-source (ref null $hd.bytes))
    (param $replacement-source (ref null $hd.bytes))
    (result (ref null $hd.bytes))
    (local $value (ref $hd.bytes))
    (local $old (ref $hd.bytes))
    (local $replacement (ref $hd.bytes))
    (local $result (ref $hd.bytes))
    (local $value-length i32)
    (local $old-length i32)
    (local $replacement-length i32)
    (local $position i32)
    (local $matches i32)
    (local $offset i32)
    (local.set $value (ref.as_non_null (local.get $value-source)))
    (local.set $old (ref.as_non_null (local.get $old-source)))
    (local.set $replacement (ref.as_non_null (local.get $replacement-source)))
    (local.set $value-length (array.len (local.get $value)))
    (local.set $old-length (array.len (local.get $old)))
    (local.set $replacement-length (array.len (local.get $replacement)))
    (if (i32.eqz (local.get $old-length))
      (then
        (local.set $result
          (array.new_default $hd.bytes
            (i32.add
              (local.get $value-length)
              (i32.mul
                (i32.add (call $hd.string_len (local.get $value)) (i32.const 1))
                (local.get $replacement-length)))))
        (local.set $offset
          (call $hd.string_copy_into
            (local.get $result) (i32.const 0) (local.get $replacement)))
        (block $empty-done
          (loop $empty-next
            (br_if $empty-done
              (i32.ge_u (local.get $position) (local.get $value-length)))
            (array.set $hd.bytes
              (local.get $result)
              (local.get $offset)
              (array.get_u $hd.bytes (local.get $value) (local.get $position)))
            (local.set $offset (i32.add (local.get $offset) (i32.const 1)))
            (local.set $position (i32.add (local.get $position) (i32.const 1)))
            (if
              (if (result i32)
                (i32.ge_u (local.get $position) (local.get $value-length))
                (then (i32.const 1))
                (else
                  (i32.ne
                    (i32.and
                      (array.get_u $hd.bytes (local.get $value) (local.get $position))
                      (i32.const 192))
                    (i32.const 128))))
              (then
                (local.set $offset
                  (call $hd.string_copy_into
                    (local.get $result) (local.get $offset) (local.get $replacement)))))
            (br $empty-next)))
        (return (local.get $result))))
    (block $counted
      (loop $count
        (br_if $counted
          (i32.gt_u (local.get $old-length)
            (i32.sub (local.get $value-length) (local.get $position))))
        (if (call $hd.string_match_at
              (local.get $value) (local.get $old) (local.get $position))
          (then
            (local.set $matches (i32.add (local.get $matches) (i32.const 1)))
            (local.set $position
              (i32.add (local.get $position) (local.get $old-length))))
          (else
            (local.set $position (i32.add (local.get $position) (i32.const 1)))))
        (br $count)))
    (if (i32.eqz (local.get $matches))
      (then (return (local.get $value))))
    (local.set $result
      (array.new_default $hd.bytes
        (i32.add
          (local.get $value-length)
          (i32.mul
            (local.get $matches)
            (i32.sub (local.get $replacement-length) (local.get $old-length))))))
    (local.set $position (i32.const 0))
    (block $replace-done
      (loop $replace
        (br_if $replace-done
          (i32.ge_u (local.get $position) (local.get $value-length)))
        (if
          (i32.and
            (i32.le_u (local.get $old-length)
              (i32.sub (local.get $value-length) (local.get $position)))
            (call $hd.string_match_at
              (local.get $value) (local.get $old) (local.get $position)))
          (then
            (local.set $offset
              (call $hd.string_copy_into
                (local.get $result) (local.get $offset) (local.get $replacement)))
            (local.set $position
              (i32.add (local.get $position) (local.get $old-length))))
          (else
            (array.set $hd.bytes
              (local.get $result)
              (local.get $offset)
              (array.get_u $hd.bytes (local.get $value) (local.get $position)))
            (local.set $offset (i32.add (local.get $offset) (i32.const 1)))
            (local.set $position (i32.add (local.get $position) (i32.const 1)))))
        (br $replace)))
    (local.get $result))
