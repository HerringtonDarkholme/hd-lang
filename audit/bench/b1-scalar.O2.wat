;; commit bd985d7; binaryen -O2 of b1-scalar.hd; 2026-09-25T20:54:02.822Z
(module
 (type $0 (func (param i32)))
 (type $1 (func (result i32)))
 (type $2 (func (param i32 i32) (result i32)))
 (type $3 (func (param i32) (result i32)))
 (import "hd" "panic" (func $hd.panic (type $0) (param i32)))
 (export "fib" (func $f0))
 (export "main" (func $f1))
 (func $hd.sub_i32 (type $2) (param $0 i32) (param $1 i32) (result i32)
  (local $2 i64)
  (if
   (i32.or
    (i64.lt_s
     (local.tee $2
      (i64.sub
       (i64.extend_i32_s
        (local.get $0)
       )
       (i64.extend_i32_s
        (local.get $1)
       )
      )
     )
     (i64.const -2147483648)
    )
    (i64.gt_s
     (local.get $2)
     (i64.const 2147483647)
    )
   )
   (then
    (call $hd.panic
     (i32.const 2)
    )
    (unreachable)
   )
  )
  (i32.wrap_i64
   (local.get $2)
  )
 )
 (func $f0 (type $3) (param $0 i32) (result i32)
  (local $1 i64)
  (if (result i32)
   (i32.eq
    (select
     (i32.const -1)
     (i32.gt_s
      (local.get $0)
      (i32.const 2)
     )
     (i32.lt_s
      (local.get $0)
      (i32.const 2)
     )
    )
    (i32.const -1)
   )
   (then
    (local.get $0)
   )
   (else
    (if
     (i32.or
      (i64.lt_s
       (local.tee $1
        (i64.add
         (i64.extend_i32_s
          (call $f0
           (call $hd.sub_i32
            (local.get $0)
            (i32.const 1)
           )
          )
         )
         (i64.extend_i32_s
          (call $f0
           (call $hd.sub_i32
            (local.get $0)
            (i32.const 2)
           )
          )
         )
        )
       )
       (i64.const -2147483648)
      )
      (i64.gt_s
       (local.get $1)
       (i64.const 2147483647)
      )
     )
     (then
      (call $hd.panic
       (i32.const 2)
      )
      (unreachable)
     )
    )
    (i32.wrap_i64
     (local.get $1)
    )
   )
  )
 )
 (func $f1 (type $1) (result i32)
  (call $f0
   (i32.const 27)
  )
 )
)
