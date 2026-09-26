;; commit bd985d7; binaryen -O2 of b2-generic.hd; 2026-09-25T20:54:04.693Z
(module
 (rec
  (type $sig0 (func (param anyref) (result i32)))
  (type $sig1 (func (param anyref i32 i32) (result i32)))
  (type $tsig0_0 (func (param anyref anyref i32) (result i32)))
  (type $tsig1_0 (func (param anyref anyref) (result (ref null $hd.bytes))))
  (type $tsig2_0 (func (param anyref anyref anyref) (result i32)))
  (type $tsig3_0 (func (param anyref anyref anyref) (result (ref null $hd.variant))))
  (type $tsig4_0 (func (param anyref anyref)))
  (type $tsig5_0 (func (param anyref anyref) (result (ref null $hd.variant))))
  (type $hd.suspension-poll-sig (func (param anyref) (result i32)))
  (type $hd.suspension-cancel-sig (func (param anyref)))
  (type $hd.suspension-result-sig (func (param anyref) (result anyref)))
  (type $hd.suspension (struct (field $hd.suspension-inner anyref) (field $hd.suspension-poll (ref $hd.suspension-poll-sig)) (field $hd.suspension-cancel (ref $hd.suspension-cancel-sig)) (field $hd.suspension-result (ref $hd.suspension-result-sig))))
  (type $hd.bytes (array (mut i8)))
  (type $hd.list (array (mut anyref)))
  (type $hd.vector (struct (field $hd.vector-size (mut i32)) (field $hd.vector-values (mut (ref $hd.list))) (field $hd.vector-version (mut i32))))
  (type $hd.iterator (struct (field $hd.iterator-list (ref null $hd.vector)) (field $hd.iterator-map (ref null $hd.map)) (field $hd.iterator-index (mut i32)) (field $hd.iterator-version i32)))
  (type $hd.map (struct (field $hd.map-key-kind i32) (field $hd.map-size (mut i32)) (field $hd.map-keys (mut (ref $hd.list))) (field $hd.map-values (mut (ref $hd.list))) (field $hd.map-version (mut i32))))
  (type $hd.providers (struct (field $hd.provider-key i32) (field $hd.provider-value anyref) (field $hd.provider-parent (ref null $hd.providers))))
  (type $hd.box-i32 (struct (field $hd.box-i32-value i32)))
  (type $hd.box-f64 (struct (field $hd.box-f64-value f64)))
  (type $hd.box-extern (struct (field $hd.box-extern-value externref)))
  (type $hd.variant (struct (field $hd.variant-tag i32) (field $hd.variant-payload (mut anyref))))
  (type $closure0 (struct (field $closure0fn (ref $sig0)) (field $closure0env anyref)))
  (type $closure1 (struct (field $closure1fn (ref $sig1)) (field $closure1env anyref)))
  (type $trait0 (struct (field $trait0value anyref) (field $trait0bounds (ref null $hd.list)) (field $trait0m0 (ref $tsig0_0))))
  (type $trait1 (struct (field $trait1value anyref) (field $trait1bounds (ref null $hd.list)) (field $trait1m0 (ref $tsig1_0))))
  (type $trait2 (struct (field $trait2value anyref) (field $trait2bounds (ref null $hd.list)) (field $trait2m0 (ref $tsig2_0))))
  (type $trait3 (struct (field $trait3value anyref) (field $trait3bounds (ref null $hd.list)) (field $trait3m0 (ref $tsig3_0))))
  (type $trait4 (struct (field $trait4value anyref) (field $trait4bounds (ref null $hd.list)) (field $trait4m0 (ref $tsig4_0))))
  (type $trait5 (struct (field $trait5value anyref) (field $trait5bounds (ref null $hd.list)) (field $trait5m0 (ref $tsig5_0))))
  (type $e0 (struct (field $e0tag i32)))
  (type $hd.runtime (struct (field $status (mut i32)) (field $scratch (mut (ref null $hd.bytes)))))
 )
 (type $32 (func (param i32)))
 (type $33 (func (result i32)))
 (type $34 (func (param i32 i32) (result i32)))
 (import "hd" "panic" (func $hd.panic (type $32) (param i32)))
 (elem declare func $tadapt0_0)
 (export "main" (func $f1))
 (func $hd.add_i32 (type $34) (param $0 i32) (param $1 i32) (result i32)
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
 (func $f1 (type $33) (result i32)
  (local $0 i32)
  (local $1 i32)
  (local $2 i32)
  (local $3 i32)
  (local $4 (ref (exact $hd.list)))
  (local $5 i32)
  (local $6 i32)
  (local $7 (ref (exact $trait0)))
  (local $8 (ref (exact $hd.list)))
  (local.set $4
   (array.new_default $hd.list
    (i32.const 0)
   )
  )
  (loop $loop1
   (if
    (i32.eq
     (select
      (i32.const -1)
      (i32.gt_s
       (local.get $1)
       (i32.const 1000)
      )
      (i32.lt_s
       (local.get $1)
       (i32.const 1000)
      )
     )
     (i32.const -1)
    )
    (then
     (local.set $2
      (i32.const 0)
     )
     (if
      (i32.le_u
       (local.tee $5
        (array.len
         (local.tee $8
          (local.get $4)
         )
        )
       )
       (local.get $3)
      )
      (then
       (local.set $4
        (array.new_default $hd.list
         (select
          (i32.shl
           (local.get $5)
           (i32.const 1)
          )
          (i32.const 4)
          (local.get $5)
         )
        )
       )
       (loop $copy
        (if
         (i32.lt_u
          (local.get $2)
          (local.get $3)
         )
         (then
          (array.set $hd.list
           (local.get $4)
           (local.get $2)
           (array.get $hd.list
            (local.get $8)
            (local.get $2)
           )
          )
          (local.set $2
           (i32.add
            (local.get $2)
            (i32.const 1)
           )
          )
          (br $copy)
         )
        )
       )
      )
     )
     (array.set $hd.list
      (local.get $4)
      (local.get $3)
      (struct.new $hd.box-i32
       (local.get $1)
      )
     )
     (local.set $3
      (i32.add
       (local.get $3)
       (i32.const 1)
      )
     )
     (local.set $0
      (i32.add
       (local.get $0)
       (i32.const 1)
      )
     )
     (local.set $1
      (call $hd.add_i32
       (local.get $1)
       (i32.const 1)
      )
     )
     (br $loop1)
    )
   )
  )
  (local.set $1
   (i32.const 0)
  )
  (loop $loop2
   (if
    (i32.eq
     (select
      (i32.const -1)
      (i32.gt_s
       (local.get $1)
       (i32.const 2000)
      )
      (i32.lt_s
       (local.get $1)
       (i32.const 2000)
      )
     )
     (i32.const -1)
    )
    (then
     (local.set $0
      (i32.const 0)
     )
     (local.set $2
      (i32.const 0)
     )
     (loop $loop0
      (if
       (i32.eq
        (select
         (i32.const -1)
         (i32.gt_s
          (local.get $0)
          (local.get $3)
         )
         (i32.lt_s
          (local.get $0)
          (local.get $3)
         )
        )
        (i32.const -1)
       )
       (then
        (if
         (i32.ge_u
          (local.get $0)
          (local.get $3)
         )
         (then
          (call $hd.panic
           (i32.const 10)
          )
          (unreachable)
         )
        )
        (local.set $2
         (call_ref $tsig0_0
          (struct.get $trait0 $trait0value
           (local.tee $7
            (struct.new $trait0
             (array.get $hd.list
              (local.get $4)
              (local.get $0)
             )
             (ref.null none)
             (ref.func $tadapt0_0)
            )
           )
          )
          (local.get $7)
          (local.get $2)
          (struct.get $trait0 $trait0m0
           (local.get $7)
          )
         )
        )
        (local.set $0
         (call $hd.add_i32
          (local.get $0)
          (i32.const 1)
         )
        )
        (br $loop0)
       )
      )
     )
     (local.set $6
      (i32.rem_s
       (call $hd.add_i32
        (local.get $6)
        (local.get $2)
       )
       (i32.const 1000003)
      )
     )
     (local.set $1
      (call $hd.add_i32
       (local.get $1)
       (i32.const 1)
      )
     )
     (br $loop2)
    )
   )
  )
  (local.get $6)
 )
 (func $tadapt0_0 (type $tsig0_0) (param $0 anyref) (param $1 anyref) (param $2 i32) (result i32)
  (i32.rem_s
   (call $hd.add_i32
    (struct.get $hd.box-i32 $hd.box-i32-value
     (ref.cast (ref $hd.box-i32)
      (local.get $0)
     )
    )
    (local.get $2)
   )
   (i32.const 1000003)
  )
 )
)
