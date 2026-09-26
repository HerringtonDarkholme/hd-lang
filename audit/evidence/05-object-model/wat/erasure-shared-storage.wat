;; commit bd985d7; command: erasure-excerpts.ts (emitWat of audit/probes/arch/erasure/erasure-shared-storage.hd); date: 2026-09-25T20:54:17.628Z
(rec
    (type $sig0 (func (param anyref) (result i32)))
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

(func $f0 (param $l0 (ref null $hd.vector)) (result anyref)
    (call $hd.vector_set (ref.as_non_null (local.get $l0)) (i32.const 0) (array.new_fixed $hd.list 2 (array.get $hd.list (ref.as_non_null (ref.cast (ref null $hd.list) (call $hd.vector_get (ref.as_non_null (local.get $l0)) (i32.const 1)))) (i32.const 0)) (struct.new $hd.box-i32 (i32.const 0))))
    (array.get $hd.list (ref.as_non_null (ref.cast (ref null $hd.list) (call $hd.vector_get (ref.as_non_null (local.get $l0)) (i32.const 0)))) (i32.const 0))
  )

(func $f1 (param $l0 (ref null $hd.vector)) (result (ref null $hd.variant))
    (ref.cast (ref null $hd.variant) (call $hd.vector_get (ref.as_non_null (local.get $l0)) (i32.const 0)))
  )

(func $f2 (export "tuples") (result i32)
    (local $l0 (ref null $hd.vector))
    (local $l1 i32)
    (local.set $l0 (struct.new $hd.vector (i32.const 2) (array.new_fixed $hd.list 2 (array.new_fixed $hd.list 2 (struct.new $hd.box-i32 (i32.const 1)) (struct.new $hd.box-i32 (i32.const 2))) (array.new_fixed $hd.list 2 (struct.new $hd.box-i32 (i32.const 40)) (struct.new $hd.box-i32 (i32.const 3)))) (i32.const 0)))
    (local.set $l1 (struct.get $hd.box-i32 $hd.box-i32-value (ref.cast (ref $hd.box-i32) (call $f0 (local.get $l0)))))
    (call $hd.sub_i32 (call $hd.add_i32 (struct.get $hd.box-i32 $hd.box-i32-value (ref.cast (ref $hd.box-i32) (array.get $hd.list (ref.as_non_null (ref.cast (ref null $hd.list) (call $hd.vector_get (ref.as_non_null (local.get $l0)) (i32.const 0)))) (i32.const 0)))) (local.get $l1)) (i32.const 40))
  )

(func $fv2 (type $sig0) (param $env anyref)  (result i32)
    (call $f2)
  )

(func $f3 (export "optionals") (result i32)
    (local $l0 (ref null $hd.vector))
    (local $l1 i32)
    (local $tmp0 (ref null $hd.variant))
    (local.set $l0 (struct.new $hd.vector (i32.const 2) (array.new_fixed $hd.list 2 (struct.new $hd.variant (i32.const 1) (struct.new $hd.box-i32 (i32.const 41))) (struct.new $hd.variant (i32.const 0) (ref.null any))) (i32.const 0)))
    (block (result i32)
      (local.set $tmp0 (call $f1 (local.get $l0)))
      (if (result i32)
        (i32.eq (struct.get $hd.variant $hd.variant-tag (local.get $tmp0)) (i32.const 0))
        (then
          (i32.const 0)
        )
        (else
          (if (result i32)
            (i32.eq (struct.get $hd.variant $hd.variant-tag (local.get $tmp0)) (i32.const 1))
            (then
              (local.set $l1 (struct.get $hd.box-i32 $hd.box-i32-value (ref.cast (ref $hd.box-i32) (struct.get $hd.variant $hd.variant-payload (local.get $tmp0)))))
              (local.get $l1)
            )
            (else
              (unreachable)
            )
          )
        )
      )
    )
  )

(func $fv3 (type $sig0) (param $env anyref)  (result i32)
    (call $f3)
  )

(func $f4 (export "main") (result i32)
    (call $hd.add_i32 (call $hd.add_i32 (call $f2) (call $f3)) (i32.const 1))
  )

(func $fv4 (type $sig0) (param $env anyref)  (result i32)
    (call $f4)
  )
