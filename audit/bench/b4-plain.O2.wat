;; commit bd985d7; binaryen -O2 of b4-plain.hd; 2026-09-25T20:54:19.259Z
(module
 (type $0 (func (param i32)))
 (type $1 (func (result i32)))
 (type $2 (func (param i32 i32) (result i32)))
 (type $3 (func (param i32) (result i32)))
 (import "hd" "panic" (func $hd.panic (type $0) (param i32)))
 (export "step" (func $f0))
 (export "main" (func $f1))
 (func $hd.add_i32 (type $2) (param $0 i32) (param $1 i32) (result i32)
  (local $2 i64)
  (if
   (i32.or
    (i64.lt_s
     (local.tee $2
      (i64.add
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
  (call $hd.add_i32
   (local.get $0)
   (i32.const 1)
  )
 )
 (func $f1 (type $1) (result i32)
  (local $0 i32)
  (local $1 i32)
  (local $2 i32)
  (loop $loop0
   (if
    (i32.eq
     (select
      (i32.const -1)
      (i32.gt_s
       (local.get $1)
       (i32.const 200000)
      )
      (i32.lt_s
       (local.get $1)
       (i32.const 200000)
      )
     )
     (i32.const -1)
    )
    (then
     (local.set $0
      (call $hd.add_i32
       (local.get $0)
       (block (result i32)
        (if
         (block (result i32)
          (local.set $0
           (i32.const 7)
          )
          (i32.const 0)
         )
         (then
          (call $hd.panic
           (i32.const 1)
          )
          (unreachable)
         )
        )
        (call $f0
         (i32.rem_s
          (local.get $1)
          (local.get $0)
         )
        )
       )
      )
     )
     (if
      (block (result i32)
       (local.set $2
        (i32.const 1000003)
       )
       (i32.const 0)
      )
      (then
       (call $hd.panic
        (i32.const 1)
       )
       (unreachable)
      )
      (else
       (local.set $0
        (i32.rem_s
         (local.get $0)
         (local.get $2)
        )
       )
       (local.set $1
        (call $hd.add_i32
         (local.get $1)
         (i32.const 1)
        )
       )
       (br $loop0)
      )
     )
    )
   )
  )
  (local.get $0)
 )
)
