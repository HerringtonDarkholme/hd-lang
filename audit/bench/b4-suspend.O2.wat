;; commit bd985d7; binaryen -O2 of b4-suspend.hd; 2026-09-25T20:54:16.084Z
(module
 (rec
  (type $sig0 (func (param anyref i32) (result (ref null $hd.suspension))))
  (type $sig1 (func (param anyref) (result (ref null $hd.suspension))))
  (type $tsig0_0 (func (param anyref anyref) (result (ref null $hd.bytes))))
  (type $tsig1_0 (func (param anyref anyref anyref) (result i32)))
  (type $tsig2_0 (func (param anyref anyref anyref) (result (ref null $hd.variant))))
  (type $tsig3_0 (func (param anyref anyref)))
  (type $tsig4_0 (func (param anyref anyref) (result (ref null $hd.variant))))
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
  (type $s0 (struct (field $s0state (mut i32)) (field $s0polls (mut i32)) (field $s0a0 i32) (field $s0result (mut i32))))
  (type $s1 (struct (field $s1state (mut i32)) (field $s1polls (mut i32)) (field $s1l0 (mut i32)) (field $s1l1 (mut i32)) (field $s1l2 (mut i32)) (field $s1l3 (mut i32)) (field $s1l4 (mut i32)) (field $s1l5 (mut i32)) (field $s1l6 (mut i32)) (field $s1l7 (mut i32)) (field $s1l8 (mut i32)) (field $s1l9 (mut i32)) (field $s1l10 (mut i32)) (field $s1l11 (mut i32)) (field $s1l12 (mut i32)) (field $s1l13 (mut i32)) (field $s1l14 (mut i32)) (field $s1child0 (mut (ref null $s0))) (field $s1result (mut i32))))
  (type $e0 (struct (field $e0tag i32)))
  (type $hd.runtime (struct (field $status (mut i32)) (field $scratch (mut (ref null $hd.bytes)))))
 )
 (type $32 (func (param i32 i32)))
 (type $33 (func (param i32 i32) (result i32)))
 (type $34 (func (param i32)))
 (type $35 (func))
 (type $36 (func (result i32)))
 (type $37 (func (param (ref null $s0)) (result i32)))
 (type $38 (func (param (ref null $s1)) (result i32)))
 (type $39 (func (result (ref (exact $s1)))))
 (import "hd" "trace" (func $hd.trace (type $32) (param i32 i32)))
 (import "hd" "pending" (func $hd.pending (type $33) (param i32 i32) (result i32)))
 (import "hd" "panic" (func $hd.panic (type $34) (param i32)))
 (global $hd.driver-active (mut i32) (i32.const 0))
 (global $hd.dev-frame1 (mut (ref null $s1)) (ref.null none))
 (export "main" (func $entry1))
 (export "__hd_start" (func $0))
 (export "__hd_poll" (func $1))
 (export "__hd_cancel" (func $2))
 (export "__hd_result" (func $3))
 (func $hd.add_i32 (type $33) (param $0 i32) (param $1 i32) (result i32)
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
 (func $poll0 (type $37) (param $0 (ref null $s0)) (result i32)
  (call $hd.trace
   (i32.const 0)
   (i32.const 1)
  )
  (if
   (i32.eq
    (struct.get $s0 $s0state
     (local.get $0)
    )
    (i32.const 1)
   )
   (then
    (call $hd.trace
     (i32.const 0)
     (i32.const 4)
    )
    (call $hd.panic
     (i32.const 5)
    )
    (unreachable)
   )
  )
  (if
   (i32.or
    (i32.eq
     (struct.get $s0 $s0state
      (local.get $0)
     )
     (i32.const 2)
    )
    (i32.eq
     (struct.get $s0 $s0state
      (local.get $0)
     )
     (i32.const 3)
    )
   )
   (then
    (call $hd.trace
     (i32.const 0)
     (i32.const 5)
    )
    (call $hd.panic
     (i32.const 4)
    )
    (unreachable)
   )
  )
  (struct.set $s0 $s0polls
   (local.get $0)
   (i32.add
    (struct.get $s0 $s0polls
     (local.get $0)
    )
    (i32.const 1)
   )
  )
  (struct.set $s0 $s0state
   (local.get $0)
   (i32.const 1)
  )
  (if
   (call $hd.pending
    (i32.const 0)
    (struct.get $s0 $s0polls
     (local.get $0)
    )
   )
   (then
    (struct.set $s0 $s0state
     (local.get $0)
     (i32.const 4)
    )
    (call $hd.trace
     (i32.const 0)
     (i32.const 6)
    )
    (return
     (i32.const 0)
    )
   )
  )
  (struct.set $s0 $s0state
   (local.get $0)
   (i32.const 1)
  )
  (struct.set $s0 $s0result
   (local.get $0)
   (call $hd.add_i32
    (struct.get $s0 $s0a0
     (local.get $0)
    )
    (i32.const 1)
   )
  )
  (struct.set $s0 $s0state
   (local.get $0)
   (i32.const 2)
  )
  (call $hd.trace
   (i32.const 0)
   (i32.const 2)
  )
  (i32.const 1)
 )
 (func $f1 (type $39) (result (ref (exact $s1)))
  (call $hd.trace
   (i32.const 1)
   (i32.const 0)
  )
  (struct.new_default $s1)
 )
 (func $poll1 (type $38) (param $0 (ref null $s1)) (result i32)
  (local $1 i32)
  (local $2 i32)
  (local $3 i32)
  (local $4 i32)
  (local $5 i32)
  (local $6 i32)
  (local $7 i32)
  (local $8 i32)
  (local $9 i32)
  (local $10 i32)
  (local $11 i32)
  (local $12 i32)
  (local $13 i32)
  (local $14 i32)
  (local $15 i32)
  (local $16 i32)
  (call $hd.trace
   (i32.const 1)
   (i32.const 1)
  )
  (if
   (i32.eq
    (local.tee $1
     (struct.get $s1 $s1state
      (local.get $0)
     )
    )
    (i32.const 1)
   )
   (then
    (call $hd.trace
     (i32.const 1)
     (i32.const 4)
    )
    (call $hd.panic
     (i32.const 5)
    )
    (unreachable)
   )
  )
  (if
   (i32.or
    (i32.eq
     (local.get $1)
     (i32.const 2)
    )
    (i32.eq
     (local.get $1)
     (i32.const 3)
    )
   )
   (then
    (call $hd.trace
     (i32.const 1)
     (i32.const 5)
    )
    (call $hd.panic
     (i32.const 4)
    )
    (unreachable)
   )
  )
  (struct.set $s1 $s1polls
   (local.get $0)
   (i32.add
    (struct.get $s1 $s1polls
     (local.get $0)
    )
    (i32.const 1)
   )
  )
  (struct.set $s1 $s1state
   (local.get $0)
   (i32.const 1)
  )
  (if
   (call $hd.pending
    (i32.const 1)
    (struct.get $s1 $s1polls
     (local.get $0)
    )
   )
   (then
    (struct.set $s1 $s1state
     (local.get $0)
     (select
      (local.get $1)
      (i32.const 4)
      (local.get $1)
     )
    )
    (call $hd.trace
     (i32.const 1)
     (i32.const 6)
    )
    (return
     (i32.const 0)
    )
   )
  )
  (local.set $3
   (struct.get $s1 $s1l0
    (local.get $0)
   )
  )
  (local.set $2
   (struct.get $s1 $s1l1
    (local.get $0)
   )
  )
  (local.set $16
   (struct.get $s1 $s1l2
    (local.get $0)
   )
  )
  (local.set $8
   (struct.get $s1 $s1l3
    (local.get $0)
   )
  )
  (local.set $9
   (struct.get $s1 $s1l4
    (local.get $0)
   )
  )
  (local.set $4
   (struct.get $s1 $s1l5
    (local.get $0)
   )
  )
  (local.set $10
   (struct.get $s1 $s1l6
    (local.get $0)
   )
  )
  (local.set $5
   (struct.get $s1 $s1l7
    (local.get $0)
   )
  )
  (local.set $11
   (struct.get $s1 $s1l8
    (local.get $0)
   )
  )
  (local.set $12
   (struct.get $s1 $s1l9
    (local.get $0)
   )
  )
  (local.set $13
   (struct.get $s1 $s1l10
    (local.get $0)
   )
  )
  (local.set $14
   (struct.get $s1 $s1l11
    (local.get $0)
   )
  )
  (local.set $15
   (struct.get $s1 $s1l12
    (local.get $0)
   )
  )
  (local.set $6
   (struct.get $s1 $s1l13
    (local.get $0)
   )
  )
  (local.set $7
   (struct.get $s1 $s1l14
    (local.get $0)
   )
  )
  (struct.set $s1 $s1state
   (local.get $0)
   (i32.const 1)
  )
  (local.set $1
   (if (result i32)
    (i32.le_u
     (local.get $1)
     (i32.const 4)
    )
    (then
     (i32.const 20)
    )
    (else
     (if (result i32)
      (i32.eq
       (local.get $1)
       (i32.const 5)
      )
      (then
       (if
        (i32.eqz
         (call $poll0
          (struct.get $s1 $s1child0
           (local.get $0)
          )
         )
        )
        (then
         (struct.set $s1 $s1l0
          (local.get $0)
          (local.get $3)
         )
         (struct.set $s1 $s1l1
          (local.get $0)
          (local.get $2)
         )
         (struct.set $s1 $s1l2
          (local.get $0)
          (local.get $16)
         )
         (struct.set $s1 $s1l3
          (local.get $0)
          (local.get $8)
         )
         (struct.set $s1 $s1l4
          (local.get $0)
          (local.get $9)
         )
         (struct.set $s1 $s1l5
          (local.get $0)
          (local.get $4)
         )
         (struct.set $s1 $s1l6
          (local.get $0)
          (local.get $10)
         )
         (struct.set $s1 $s1l7
          (local.get $0)
          (local.get $5)
         )
         (struct.set $s1 $s1l8
          (local.get $0)
          (local.get $11)
         )
         (struct.set $s1 $s1l9
          (local.get $0)
          (local.get $12)
         )
         (struct.set $s1 $s1l10
          (local.get $0)
          (local.get $13)
         )
         (struct.set $s1 $s1l11
          (local.get $0)
          (local.get $14)
         )
         (struct.set $s1 $s1l12
          (local.get $0)
          (local.get $15)
         )
         (struct.set $s1 $s1l13
          (local.get $0)
          (local.get $6)
         )
         (struct.set $s1 $s1l14
          (local.get $0)
          (local.get $7)
         )
         (struct.set $s1 $s1state
          (local.get $0)
          (local.get $1)
         )
         (call $hd.trace
          (i32.const 1)
          (i32.const 6)
         )
         (return
          (i32.const 0)
         )
        )
       )
       (local.set $5
        (struct.get $s0 $s0result
         (struct.get $s1 $s1child0
          (local.get $0)
         )
        )
       )
       (i32.const 10)
      )
      (else
       (i32.const 20)
      )
     )
    )
   )
  )
  (loop $cfg
   (if
    (i32.eqz
     (local.get $1)
    )
    (then
     (struct.set $s1 $s1result
      (local.get $0)
      (local.get $2)
     )
     (struct.set $s1 $s1state
      (local.get $0)
      (i32.const 2)
     )
     (call $hd.trace
      (i32.const 1)
      (i32.const 2)
     )
     (return
      (i32.const 1)
     )
    )
   )
   (if
    (i32.eq
     (local.get $1)
     (i32.const 1)
    )
    (then
     (local.set $1
      (i32.const 0)
     )
     (br $cfg)
    )
   )
   (if
    (i32.eq
     (local.get $1)
     (i32.const 2)
    )
    (then
     (local.set $1
      (i32.const 18)
     )
     (br $cfg)
    )
   )
   (if
    (i32.eq
     (local.get $1)
     (i32.const 3)
    )
    (then
     (local.set $1
      (i32.const 2)
     )
     (br $cfg)
    )
   )
   (if
    (i32.eq
     (local.get $1)
     (i32.const 4)
    )
    (then
     (local.set $3
      (call $hd.add_i32
       (local.get $14)
       (local.get $15)
      )
     )
     (local.set $1
      (i32.const 3)
     )
     (br $cfg)
    )
   )
   (if
    (i32.eq
     (local.get $1)
     (i32.const 5)
    )
    (then
     (local.set $15
      (i32.const 1)
     )
     (local.set $1
      (i32.const 4)
     )
     (br $cfg)
    )
   )
   (if
    (i32.eq
     (local.get $1)
     (i32.const 6)
    )
    (then
     (local.set $14
      (local.get $3)
     )
     (local.set $1
      (i32.const 5)
     )
     (br $cfg)
    )
   )
   (if
    (i32.eq
     (local.get $1)
     (i32.const 7)
    )
    (then
     (if
      (i32.eqz
       (local.tee $2
        (local.get $13)
       )
      )
      (then
       (call $hd.panic
        (i32.const 1)
       )
       (unreachable)
      )
     )
     (local.set $2
      (i32.rem_s
       (local.get $12)
       (local.get $2)
      )
     )
     (local.set $1
      (i32.const 6)
     )
     (br $cfg)
    )
   )
   (if
    (i32.eq
     (local.get $1)
     (i32.const 8)
    )
    (then
     (local.set $13
      (i32.const 1000003)
     )
     (local.set $1
      (i32.const 7)
     )
     (br $cfg)
    )
   )
   (if
    (i32.eq
     (local.get $1)
     (i32.const 9)
    )
    (then
     (local.set $12
      (call $hd.add_i32
       (local.get $8)
       (local.get $11)
      )
     )
     (local.set $1
      (i32.const 8)
     )
     (br $cfg)
    )
   )
   (if
    (i32.eq
     (local.get $1)
     (i32.const 10)
    )
    (then
     (local.set $11
      (local.get $5)
     )
     (local.set $1
      (i32.const 9)
     )
     (br $cfg)
    )
   )
   (if
    (i32.eq
     (local.get $1)
     (i32.const 11)
    )
    (then
     (call $hd.trace
      (i32.const 0)
      (i32.const 0)
     )
     (struct.set $s1 $s1child0
      (local.get $0)
      (struct.new $s0
       (i32.const 0)
       (i32.const 0)
       (local.get $10)
       (i32.const 0)
      )
     )
     (if
      (i32.eqz
       (call $poll0
        (struct.get $s1 $s1child0
         (local.get $0)
        )
       )
      )
      (then
       (struct.set $s1 $s1l0
        (local.get $0)
        (local.get $3)
       )
       (struct.set $s1 $s1l1
        (local.get $0)
        (local.get $2)
       )
       (struct.set $s1 $s1l2
        (local.get $0)
        (local.get $16)
       )
       (struct.set $s1 $s1l3
        (local.get $0)
        (local.get $8)
       )
       (struct.set $s1 $s1l4
        (local.get $0)
        (local.get $9)
       )
       (struct.set $s1 $s1l5
        (local.get $0)
        (local.get $4)
       )
       (struct.set $s1 $s1l6
        (local.get $0)
        (local.get $10)
       )
       (struct.set $s1 $s1l7
        (local.get $0)
        (local.get $5)
       )
       (struct.set $s1 $s1l8
        (local.get $0)
        (local.get $11)
       )
       (struct.set $s1 $s1l9
        (local.get $0)
        (local.get $12)
       )
       (struct.set $s1 $s1l10
        (local.get $0)
        (local.get $13)
       )
       (struct.set $s1 $s1l11
        (local.get $0)
        (local.get $14)
       )
       (struct.set $s1 $s1l12
        (local.get $0)
        (local.get $15)
       )
       (struct.set $s1 $s1l13
        (local.get $0)
        (local.get $6)
       )
       (struct.set $s1 $s1l14
        (local.get $0)
        (local.get $7)
       )
       (struct.set $s1 $s1state
        (local.get $0)
        (i32.const 5)
       )
       (call $hd.trace
        (i32.const 1)
        (i32.const 6)
       )
       (return
        (i32.const 0)
       )
      )
     )
     (local.set $5
      (struct.get $s0 $s0result
       (struct.get $s1 $s1child0
        (local.get $0)
       )
      )
     )
     (local.set $1
      (i32.const 10)
     )
     (br $cfg)
    )
   )
   (if
    (i32.eq
     (local.get $1)
     (i32.const 12)
    )
    (then
     (if
      (i32.eqz
       (local.get $4)
      )
      (then
       (call $hd.panic
        (i32.const 1)
       )
       (unreachable)
      )
     )
     (local.set $10
      (i32.rem_s
       (local.get $9)
       (local.get $4)
      )
     )
     (local.set $1
      (i32.const 11)
     )
     (br $cfg)
    )
   )
   (if
    (i32.eq
     (local.get $1)
     (i32.const 13)
    )
    (then
     (local.set $4
      (i32.const 7)
     )
     (local.set $1
      (i32.const 12)
     )
     (br $cfg)
    )
   )
   (if
    (i32.eq
     (local.get $1)
     (i32.const 14)
    )
    (then
     (local.set $9
      (local.get $3)
     )
     (local.set $1
      (i32.const 13)
     )
     (br $cfg)
    )
   )
   (if
    (i32.eq
     (local.get $1)
     (i32.const 15)
    )
    (then
     (local.set $8
      (local.get $2)
     )
     (local.set $1
      (i32.const 14)
     )
     (br $cfg)
    )
   )
   (if
    (i32.eq
     (local.get $1)
     (i32.const 16)
    )
    (then
     (local.set $1
      (select
       (i32.const 15)
       (i32.const 1)
       (i32.eq
        (select
         (i32.const -1)
         (i32.gt_s
          (local.get $6)
          (local.get $7)
         )
         (i32.lt_s
          (local.get $6)
          (local.get $7)
         )
        )
        (i32.const -1)
       )
      )
     )
     (br $cfg)
    )
   )
   (if
    (i32.eq
     (local.get $1)
     (i32.const 17)
    )
    (then
     (local.set $7
      (i32.const 200000)
     )
     (local.set $1
      (i32.const 16)
     )
     (br $cfg)
    )
   )
   (if
    (i32.eq
     (local.get $1)
     (i32.const 18)
    )
    (then
     (local.set $6
      (local.get $3)
     )
     (local.set $1
      (i32.const 17)
     )
     (br $cfg)
    )
   )
   (if
    (i32.eq
     (local.get $1)
     (i32.const 19)
    )
    (then
     (local.set $2
      (i32.const 0)
     )
     (local.set $1
      (i32.const 2)
     )
     (br $cfg)
    )
   )
   (if
    (i32.eq
     (local.get $1)
     (i32.const 20)
    )
    (then
     (local.set $3
      (i32.const 0)
     )
     (local.set $1
      (i32.const 19)
     )
     (br $cfg)
    )
   )
  )
  (unreachable)
 )
 (func $entry1 (type $36) (result i32)
  (local $0 (ref (exact $s1)))
  (local.set $0
   (call $f1)
  )
  (if
   (global.get $hd.driver-active)
   (then
    (call $hd.panic
     (i32.const 6)
    )
    (unreachable)
   )
  )
  (global.set $hd.driver-active
   (i32.const 1)
  )
  (loop $drive
   (br_if $drive
    (i32.ne
     (call $poll1
      (local.get $0)
     )
     (i32.const 1)
    )
   )
  )
  (global.set $hd.driver-active
   (i32.const 0)
  )
  (struct.get $s1 $s1result
   (local.get $0)
  )
 )
 (func $0 (type $35)
  (if
   (global.get $hd.driver-active)
   (then
    (call $hd.panic
     (i32.const 6)
    )
    (unreachable)
   )
  )
  (global.set $hd.driver-active
   (i32.const 1)
  )
  (global.set $hd.dev-frame1
   (call $f1)
  )
 )
 (func $1 (type $36) (result i32)
  (local $0 i32)
  (if
   (local.tee $0
    (call $poll1
     (global.get $hd.dev-frame1)
    )
   )
   (then
    (global.set $hd.driver-active
     (i32.const 0)
    )
   )
  )
  (local.get $0)
 )
 (func $2 (type $35)
  (local $0 (ref null $s1))
  (local $1 (ref null $s0))
  (local $2 i32)
  (local.set $0
   (global.get $hd.dev-frame1)
  )
  (call $hd.trace
   (i32.const 1)
   (i32.const 3)
  )
  (if
   (i32.eq
    (local.tee $2
     (struct.get $s1 $s1state
      (local.get $0)
     )
    )
    (i32.const 1)
   )
   (then
    (call $hd.trace
     (i32.const 1)
     (i32.const 4)
    )
    (call $hd.panic
     (i32.const 5)
    )
    (unreachable)
   )
  )
  (drop
   (struct.get $s1 $s1l0
    (local.get $0)
   )
  )
  (drop
   (struct.get $s1 $s1l1
    (local.get $0)
   )
  )
  (drop
   (struct.get $s1 $s1l2
    (local.get $0)
   )
  )
  (drop
   (struct.get $s1 $s1l3
    (local.get $0)
   )
  )
  (drop
   (struct.get $s1 $s1l4
    (local.get $0)
   )
  )
  (drop
   (struct.get $s1 $s1l5
    (local.get $0)
   )
  )
  (drop
   (struct.get $s1 $s1l6
    (local.get $0)
   )
  )
  (drop
   (struct.get $s1 $s1l7
    (local.get $0)
   )
  )
  (drop
   (struct.get $s1 $s1l8
    (local.get $0)
   )
  )
  (drop
   (struct.get $s1 $s1l9
    (local.get $0)
   )
  )
  (drop
   (struct.get $s1 $s1l10
    (local.get $0)
   )
  )
  (drop
   (struct.get $s1 $s1l11
    (local.get $0)
   )
  )
  (drop
   (struct.get $s1 $s1l12
    (local.get $0)
   )
  )
  (drop
   (struct.get $s1 $s1l13
    (local.get $0)
   )
  )
  (drop
   (struct.get $s1 $s1l14
    (local.get $0)
   )
  )
  (block $__inlined_func$cancel1$4
   (if
    (i32.eq
     (local.get $2)
     (i32.const 5)
    )
    (then
     (local.set $1
      (struct.get $s1 $s1child0
       (local.get $0)
      )
     )
     (call $hd.trace
      (i32.const 0)
      (i32.const 3)
     )
     (if
      (i32.eq
       (struct.get $s0 $s0state
        (local.get $1)
       )
       (i32.const 1)
      )
      (then
       (call $hd.trace
        (i32.const 0)
        (i32.const 4)
       )
       (call $hd.panic
        (i32.const 5)
       )
       (unreachable)
      )
     )
     (if
      (i32.or
       (i32.eqz
        (struct.get $s0 $s0state
         (local.get $1)
        )
       )
       (i32.eq
        (struct.get $s0 $s0state
         (local.get $1)
        )
        (i32.const 4)
       )
      )
      (then
       (struct.set $s0 $s0state
        (local.get $1)
        (i32.const 3)
       )
      )
     )
     (struct.set $s1 $s1state
      (local.get $0)
      (i32.const 3)
     )
     (br $__inlined_func$cancel1$4)
    )
   )
   (if
    (i32.or
     (i32.eqz
      (local.get $2)
     )
     (i32.eq
      (local.get $2)
      (i32.const 4)
     )
    )
    (then
     (struct.set $s1 $s1state
      (local.get $0)
      (i32.const 3)
     )
    )
   )
  )
  (global.set $hd.driver-active
   (i32.const 0)
  )
 )
 (func $3 (type $36) (result i32)
  (struct.get $s1 $s1result
   (global.get $hd.dev-frame1)
  )
 )
)
