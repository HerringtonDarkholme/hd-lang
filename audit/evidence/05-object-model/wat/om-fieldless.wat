;; commit bd985d7; command: om-build.ts (emitWat of audit/probes/arch/object-model/om-fieldless.hd); date: 2026-09-25T20:54:06.500Z
;; excerpt: rec types, user globals, user functions (runtime $hd.* functions omitted)
(rec
    (type $sig0 (func (param anyref) (param (ref null $d0)) (result i32)))
    (type $sig1 (func (param anyref) (param (ref null $e0)) (result i32)))
    (type $sig2 (func (param anyref) (param (ref null $e1)) (result i32)))
    (type $sig3 (func (param anyref) (param i32) (result i32)))
    (type $sig4 (func (param anyref) (result i32)))
    (type $tsig0_0 (func (param anyref) (param anyref) (result (ref null $hd.bytes))))
    (type $tsig1_0 (func (param anyref) (param anyref) (param anyref) (result i32)))
    (type $tsig2_0 (func (param anyref) (param anyref) (param anyref) (result (ref null $hd.variant))))
    (type $tsig3_0 (func (param anyref) (param anyref)))
    (type $tsig4_0 (func (param anyref) (param anyref) (result (ref null $hd.variant))))
    (type $hd.suspension-poll-sig (func (param anyref) (result i32)))
    (type $hd.suspension-cancel-sig (func (param anyref)))
    (type $hd.suspension-result-sig (func (param anyref) (result anyref)))
    (type $hd.suspension (struct
      (field $hd.suspension-inner anyref)
      (field $hd.suspension-poll (ref $hd.suspension-poll-sig))
      (field $hd.suspension-cancel (ref $hd.suspension-cancel-sig))
      (field $hd.suspension-result (ref $hd.suspension-result-sig))))
    (type $hd.bytes (array (mut i8)))
    (type $hd.list (array (mut anyref)))
    (type $hd.vector (struct
      (field $hd.vector-size (mut i32))
      (field $hd.vector-values (mut (ref $hd.list)))
      (field $hd.vector-version (mut i32))))
    (type $hd.iterator (struct
      (field $hd.iterator-list (ref null $hd.vector))
      (field $hd.iterator-map (ref null $hd.map))
      (field $hd.iterator-index (mut i32))
      (field $hd.iterator-version i32)))
    (type $hd.map (struct
      (field $hd.map-key-kind i32)
      (field $hd.map-size (mut i32))
      (field $hd.map-keys (mut (ref $hd.list)))
      (field $hd.map-values (mut (ref $hd.list)))
      (field $hd.map-version (mut i32))))
    (type $hd.providers (struct
      (field $hd.provider-key i32)
      (field $hd.provider-value anyref)
      (field $hd.provider-parent (ref null $hd.providers))))
    (type $hd.box-i32 (struct
      (field $hd.box-i32-value i32)))
    (type $hd.box-f64 (struct
      (field $hd.box-f64-value f64)))
    (type $hd.box-extern (struct
      (field $hd.box-extern-value externref)))
    (type $hd.variant (struct
      (field $hd.variant-tag i32)
      (field $hd.variant-payload (mut anyref))))
    (type $closure0 (struct
      (field $closure0fn (ref $sig0))
      (field $closure0env anyref)))
    (type $closure1 (struct
      (field $closure1fn (ref $sig1))
      (field $closure1env anyref)))
    (type $closure2 (struct
      (field $closure2fn (ref $sig2))
      (field $closure2env anyref)))
    (type $closure3 (struct
      (field $closure3fn (ref $sig3))
      (field $closure3env anyref)))
    (type $closure4 (struct
      (field $closure4fn (ref $sig4))
      (field $closure4env anyref)))
    (type $trait0 (struct
      (field $trait0value anyref)
      (field $trait0bounds (ref null $hd.list))
      (field $trait0m0 (ref $tsig0_0))))
    (type $trait1 (struct
      (field $trait1value anyref)
      (field $trait1bounds (ref null $hd.list))
      (field $trait1m0 (ref $tsig1_0))))
    (type $trait2 (struct
      (field $trait2value anyref)
      (field $trait2bounds (ref null $hd.list))
      (field $trait2m0 (ref $tsig2_0))))
    (type $trait3 (struct
      (field $trait3value anyref)
      (field $trait3bounds (ref null $hd.list))
      (field $trait3m0 (ref $tsig3_0))))
    (type $trait4 (struct
      (field $trait4value anyref)
      (field $trait4bounds (ref null $hd.list))
      (field $trait4m0 (ref $tsig4_0))))
    (type $d0 (struct
))
    (type $e0 (struct
      (field $e0tag i32)))
    (type $e1 (struct
      (field $e1tag i32)
      (field $e1f0 (mut anyref))))
    (type $e2 (struct
      (field $e2tag i32)))
    (type $hd.runtime (struct
      (field $status (mut i32))
      (field $scratch (mut (ref null $hd.bytes)))))
  )

(global $e0v0 (ref $e0)
    (struct.new $e0 (i32.const 0)))

(global $e0v1 (ref $e0)
    (struct.new $e0 (i32.const 1)))

(global $e1v1 (ref $e1)
    (struct.new $e1 (i32.const 1) (ref.null any)))

(global $e2v0 (ref $e2)
    (struct.new $e2 (i32.const 0)))

(global $e2v1 (ref $e2)
    (struct.new $e2 (i32.const 1)))

(global $e2v2 (ref $e2)
    (struct.new $e2 (i32.const 2)))

(func $f0 (param $l0 (ref null $d0)) (result i32)
    (i32.const 1)
  )

(func $fv0 (type $sig0) (param $env anyref) (param $l0 (ref null $d0)) (result i32)
    (call $f0 (local.get $l0))
  )

(func $f1 (param $l0 (ref null $e0)) (result i32)
    (local $tmp0 (ref null $e0))
    (block (result i32)
      (local.set $tmp0 (local.get $l0))
      (if (result i32)
        (i32.eq (struct.get $e0 $e0tag (local.get $tmp0)) (i32.const 0))
        (then
          (i32.const 1)
        )
        (else
          (if (result i32)
            (i32.eq (struct.get $e0 $e0tag (local.get $tmp0)) (i32.const 1))
            (then
              (i32.const 0)
            )
            (else
              (unreachable)
            )
          )
        )
      )
    )
  )

(func $fv1 (type $sig1) (param $env anyref) (param $l0 (ref null $e0)) (result i32)
    (call $f1 (local.get $l0))
  )

(func $f2 (param $l0 (ref null $e1)) (result i32)
    (local $l1 i32)
    (local $tmp0 (ref null $e1))
    (block (result i32)
      (local.set $tmp0 (local.get $l0))
      (if (result i32)
        (i32.eq (struct.get $e1 $e1tag (local.get $tmp0)) (i32.const 0))
        (then
          (local.set $l1 (struct.get $hd.box-i32 $hd.box-i32-value (ref.cast (ref $hd.box-i32) (struct.get $e1 $e1f0 (local.get $tmp0)))))
          (i32.const 0)
        )
        (else
          (if (result i32)
            (i32.eq (struct.get $e1 $e1tag (local.get $tmp0)) (i32.const 1))
            (then
              (i32.const 1)
            )
            (else
              (unreachable)
            )
          )
        )
      )
    )
  )

(func $fv2 (type $sig2) (param $env anyref) (param $l0 (ref null $e1)) (result i32)
    (call $f2 (local.get $l0))
  )

(func $f3 (export "loop_units") (param $l0 i32) (result i32)
    (local $l1 i32)
    (local $l2 i32)
    (local $l3 (ref null $e1))
    (local $tmp0 i32)
    (local.set $l1 (i32.const 0))
    (local.set $l2 (i32.const 0))
    (block $break0
      (loop $loop0
        (br_if $break0 (i32.eqz (block (result i32) (local.set $tmp0 (if (result i32) (i32.lt_s (local.get $l2) (local.get $l0)) (then (i32.const -1)) (else (if (result i32) (i32.gt_s (local.get $l2) (local.get $l0)) (then (i32.const 1)) (else (i32.const 0)))))) (i32.eq (local.get $tmp0) (i32.const -1)))))
        (local.set $l3 (global.get $e1v1))
        (local.set $l1 (call $hd.add_i32 (call $hd.add_i32 (call $hd.add_i32 (local.get $l1) (call $f0 (struct.new $d0))) (call $f1 (global.get $e0v0))) (call $f2 (local.get $l3))))
        (local.set $l2 (call $hd.add_i32 (local.get $l2) (i32.const 1)))
        (br $loop0)
      )
    )
    (local.get $l1)
  )

(func $fv3 (type $sig3) (param $env anyref) (param $l0 i32) (result i32)
    (call $f3 (local.get $l0))
  )

(func $f4 (export "main") (result i32)
    (call $f3 (i32.const 14))
  )

(func $fv4 (type $sig4) (param $env anyref)  (result i32)
    (call $f4)
  )
