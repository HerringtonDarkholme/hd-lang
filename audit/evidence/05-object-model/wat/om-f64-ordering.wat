;; UNBALANCED WAT (Error: unbalanced form at 0); tail of emitted module:
(elem declare func $fv0 $fv1)

  (func $f0 (export "greater") (param $l0 f64) (param $l1 f64) (result i32)
    (local $tmp0 i32)
    (block (result i32) (local.set $tmp0 (if (result i32) (f64.lt (local.get $l0) (local.get $l1)) (then (i32.const -1)) (else (if (result i32) (f64.gt (local.get $l0) (local.get $l1)) (then (i32.const 1)) (else (if (result i32) (f64.eq (local.get $l0) (local.get $l1)) (then (i32.const 0)) (else (i32.const 2))))))) (i32.eq (local.get $tmp0) (i32.const 1)))
  )

  (func $fv0 (type $sig0) (param $env anyref) (param $l0 f64) (param $l1 f64) (result i32)
    (call $f0 (local.get $l0) (local.get $l1))
  )

  (func $f1 (export "main") (result i32)
    (if (result i32) (call $f0 (f64.const 2) (f64.const 1))
      (then
        (i32.const 1)
      )
      (else
        (i32.const 0)
      )
    )
  )

  (func $fv1 (type $sig1) (param $env anyref)  (result i32)
    (call $f1)
  )
)