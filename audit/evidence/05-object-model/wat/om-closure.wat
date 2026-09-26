;; commit bd985d7; command: om-build.ts (emitWat of audit/probes/arch/object-model/om-closure.hd); date: 2026-09-25T20:54:06.500Z
;; excerpt: rec types, user globals, user functions (runtime $hd.* functions omitted)
(rec
    (type $sig0 (func (param anyref) (param i32) (result i32)))
    (type $sig1 (func (param anyref) (param (ref null $closure0)) (param i32) (result i32)))
    (type $sig2 (func (param anyref) (result i32)))
    (type $sig3 (func (param anyref) (param f64) (param f64) (result i32)))
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
    (type $env0 (struct
      (field $env0f0 i32)
      (field $env0f1 f64)))
    (type $env1 (struct
      (field $env1f0 i32)))
    (type $env2 (struct))
    (type $e0 (struct
      (field $e0tag i32)))
    (type $hd.runtime (struct
      (field $status (mut i32))
      (field $scratch (mut (ref null $hd.bytes)))))
  )

(global $e0v0 (ref $e0)
    (struct.new $e0 (i32.const 0)))

(global $e0v1 (ref $e0)
    (struct.new $e0 (i32.const 1)))

(global $e0v2 (ref $e0)
    (struct.new $e0 (i32.const 2)))

(func $f0 (param $l0 (ref null $closure0)) (param $l1 i32) (result i32)
    (local $tmp0 (ref null $closure0))
    (block (result i32)
      (local.set $tmp0 (local.get $l0))
      (call_ref $sig0
        (struct.get $closure0 $closure0env (local.get $tmp0))
        (local.get $l1)
        (struct.get $closure0 $closure0fn (local.get $tmp0)))
    )
  )

(func $fv0 (type $sig1) (param $env anyref) (param $l0 (ref null $closure0)) (param $l1 i32) (result i32)
    (call $f0 (local.get $l0) (local.get $l1))
  )

(func $f1 (export "loop_create") (param $l0 i32) (result i32)
    (local $l1 i32)
    (local $l2 f64)
    (local $l3 i32)
    (local $l4 i32)
    (local $l5 (ref null $closure0))
    (local $tmp0 i32)
    (local.set $l1 (i32.const 1))
    (local.set $l2 (f64.const 2))
    (local.set $l3 (i32.const 0))
    (local.set $l4 (i32.const 0))
    (block $break0
      (loop $loop0
        (br_if $break0 (i32.eqz (block (result i32) (local.set $tmp0 (if (result i32) (i32.lt_s (local.get $l4) (local.get $l0)) (then (i32.const -1)) (else (if (result i32) (i32.gt_s (local.get $l4) (local.get $l0)) (then (i32.const 1)) (else (i32.const 0)))))) (i32.eq (local.get $tmp0) (i32.const -1)))))
        (local.set $l5 (struct.new $closure0 (ref.func $c0) (struct.new $env0 (local.get $l1) (local.get $l2))))
        (local.set $l3 (call $hd.add_i32 (local.get $l3) (call $f0 (local.get $l5) (local.get $l4))))
        (local.set $l4 (call $hd.add_i32 (local.get $l4) (i32.const 1)))
        (br $loop0)
      )
    )
    (local.get $l3)
  )

(func $fv1 (type $sig0) (param $env anyref) (param $l0 i32) (result i32)
    (call $f1 (local.get $l0))
  )

(func $f2 (export "loop_reuse") (param $l0 i32) (result i32)
    (local $l1 i32)
    (local $l2 (ref null $closure0))
    (local $l3 i32)
    (local $l4 i32)
    (local $tmp0 (ref null $closure0))
    (local $tmp1 i32)
    (local.set $l1 (i32.const 1))
    (local.set $l2 (struct.new $closure0 (ref.func $c1) (struct.new $env1 (local.get $l1))))
    (local.set $l3 (i32.const 0))
    (local.set $l4 (i32.const 0))
    (block $break1
      (loop $loop1
        (br_if $break1 (i32.eqz (block (result i32) (local.set $tmp1 (if (result i32) (i32.lt_s (local.get $l4) (local.get $l0)) (then (i32.const -1)) (else (if (result i32) (i32.gt_s (local.get $l4) (local.get $l0)) (then (i32.const 1)) (else (i32.const 0)))))) (i32.eq (local.get $tmp1) (i32.const -1)))))
        (local.set $l3 (call $hd.add_i32 (local.get $l3) (block (result i32)
          (local.set $tmp0 (local.get $l2))
          (call_ref $sig0
            (struct.get $closure0 $closure0env (local.get $tmp0))
            (local.get $l4)
            (struct.get $closure0 $closure0fn (local.get $tmp0)))
        )))
        (local.set $l4 (call $hd.add_i32 (local.get $l4) (i32.const 1)))
        (br $loop1)
      )
    )
    (local.get $l3)
  )

(func $fv2 (type $sig0) (param $env anyref) (param $l0 i32) (result i32)
    (call $f2 (local.get $l0))
  )

(func $f3 (export "noncapturing") (param $l0 i32) (result i32)
    (local $l1 (ref null $closure0))
    (local.set $l1 (struct.new $closure0 (ref.func $c2) (struct.new $env2)))
    (call $f0 (local.get $l1) (local.get $l0))
  )

(func $fv3 (type $sig0) (param $env anyref) (param $l0 i32) (result i32)
    (call $f3 (local.get $l0))
  )

(func $f4 (export "main") (result i32)
    (call $hd.add_i32 (call $hd.add_i32 (call $f1 (i32.const 3)) (call $f2 (i32.const 3))) (call $f3 (i32.const 1)))
  )

(func $fv4 (type $sig2) (param $env anyref)  (result i32)
    (call $f4)
  )

(func $f5 (export "above") (param $l0 f64) (param $l1 f64) (result i32)
    (if (result i32) (f64.eq (local.get $l0) (local.get $l1))
      (then
        (i32.const 1)
      )
      (else
        (i32.const 0)
      )
    )
  )

(func $fv5 (type $sig3) (param $env anyref) (param $l0 f64) (param $l1 f64) (result i32)
    (call $f5 (local.get $l0) (local.get $l1))
  )

(func $c0 (type $sig0) (param $env anyref) (param $l0 i32) (result i32)
    (call $hd.add_i32 (call $hd.add_i32 (local.get $l0) (struct.get $env0 $env0f0 (ref.cast (ref $env0) (local.get $env)))) (call $hd.sub_i32 (i32.const 1) (call $f5 (struct.get $env0 $env0f1 (ref.cast (ref $env0) (local.get $env))) (f64.const 1))))
  )

(func $c1 (type $sig0) (param $env anyref) (param $l0 i32) (result i32)
    (call $hd.add_i32 (local.get $l0) (struct.get $env1 $env1f0 (ref.cast (ref $env1) (local.get $env))))
  )

(func $c2 (type $sig0) (param $env anyref) (param $l0 i32) (result i32)
    (call $hd.add_i32 (local.get $l0) (i32.const 1))
  )
