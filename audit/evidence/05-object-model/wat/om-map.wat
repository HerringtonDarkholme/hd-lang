;; commit bd985d7; command: om-build.ts (emitWat of audit/probes/arch/object-model/om-map.hd); date: 2026-09-25T20:54:06.500Z
;; excerpt: rec types, user globals, user functions (runtime $hd.* functions omitted)
(rec
    (type $sig0 (func (param anyref) (param i32) (result (ref null $hd.map))))
    (type $sig1 (func (param anyref) (param i32) (param i32) (result i32)))
    (type $sig2 (func (param anyref) (result i32)))
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

(func $f0 (param $l0 i32) (result (ref null $hd.map))
    (local $l1 (ref null $hd.map))
    (local $l2 i32)
    (local $tmp0 (ref null $hd.map))
    (local $tmp1 i32)
    (local.set $l1 (block (result (ref null $hd.map))
      (local.set $tmp0
        (struct.new $hd.map
          (i32.const 0)
          (i32.const 0)
          (array.new_default $hd.list (i32.const 0))
          (array.new_default $hd.list (i32.const 0))
          (i32.const 0)))
      (local.get $tmp0)
    ))
    (local.set $l2 (i32.const 0))
    (block $break0
      (loop $loop0
        (br_if $break0 (i32.eqz (block (result i32) (local.set $tmp1 (if (result i32) (i32.lt_s (local.get $l2) (local.get $l0)) (then (i32.const -1)) (else (if (result i32) (i32.gt_s (local.get $l2) (local.get $l0)) (then (i32.const 1)) (else (i32.const 0)))))) (i32.eq (local.get $tmp1) (i32.const -1)))))
        (call $hd.map_insert (ref.as_non_null (local.get $l1)) (struct.new $hd.box-i32 (local.get $l2)) (struct.new $hd.box-i32 (local.get $l2)))
        (local.set $l2 (call $hd.add_i32 (local.get $l2) (i32.const 1)))
        (br $loop0)
      )
    )
    (local.get $l1)
  )

(func $fv0 (type $sig0) (param $env anyref) (param $l0 i32) (result (ref null $hd.map))
    (call $f0 (local.get $l0))
  )

(func $f1 (export "lookup") (param $l0 i32) (param $l1 i32) (result i32)
    (local $l2 (ref null $hd.map))
    (local $l3 i32)
    (local $l4 i32)
    (local $l5 i32)
    (local $l6 i32)
    (local $tmp0 (ref null $hd.variant))
    (local $tmp1 i32)
    (local $tmp2 i32)
    (local $tmp3 i32)
    (local.set $l2 (call $f0 (local.get $l0)))
    (local.set $l3 (i32.const 0))
    (local.set $l4 (i32.const 0))
    (block $break1
      (loop $loop1
        (br_if $break1 (i32.eqz (block (result i32) (local.set $tmp3 (if (result i32) (i32.lt_s (local.get $l4) (local.get $l1)) (then (i32.const -1)) (else (if (result i32) (i32.gt_s (local.get $l4) (local.get $l1)) (then (i32.const 1)) (else (i32.const 0)))))) (i32.eq (local.get $tmp3) (i32.const -1)))))
        (local.set $l6 (block (result i32)
          (local.set $tmp0 (call $hd.map_get (ref.as_non_null (local.get $l2)) (struct.new $hd.box-i32 (block (result i32)
          (local.set $tmp1 (local.get $l4))
          (local.set $tmp2 (local.get $l0))
          (if (i32.eqz (local.get $tmp2))
            (then (call $hd.panic (i32.const 1)) unreachable))
          (i32.rem_s (local.get $tmp1) (local.get $tmp2))
        ))))
          (if (result i32)
            (i32.eq (struct.get $hd.variant $hd.variant-tag (local.get $tmp0)) (i32.const 1))
            (then
              (local.set $l5 (struct.get $hd.box-i32 $hd.box-i32-value (ref.cast (ref $hd.box-i32) (struct.get $hd.variant $hd.variant-payload (local.get $tmp0)))))
              (local.get $l5)
            )
            (else
              (if (result i32)
                (i32.eq (struct.get $hd.variant $hd.variant-tag (local.get $tmp0)) (i32.const 0))
                (then
                  (i32.const 0)
                )
                (else
                  (unreachable)
                )
              )
            )
          )
        ))
        (local.set $l3 (call $hd.add_i32 (local.get $l3) (local.get $l6)))
        (local.set $l4 (call $hd.add_i32 (local.get $l4) (i32.const 1)))
        (br $loop1)
      )
    )
    (local.get $l3)
  )

(func $fv1 (type $sig1) (param $env anyref) (param $l0 i32) (param $l1 i32) (result i32)
    (call $f1 (local.get $l0) (local.get $l1))
  )

(func $f2 (export "string_keys") (result i32)
    (local $l0 (ref null $hd.map))
    (local $l1 i32)
    (local $tmp0 (ref null $hd.map))
    (local $tmp1 (ref null $hd.variant))
    (local.set $l0 (block (result (ref null $hd.map))
      (local.set $tmp0
        (struct.new $hd.map
          (i32.const 1)
          (i32.const 0)
          (array.new_default $hd.list (i32.const 0))
          (array.new_default $hd.list (i32.const 0))
          (i32.const 0)))
      (local.get $tmp0)
    ))
    (call $hd.map_insert (ref.as_non_null (local.get $l0)) (array.new_fixed $hd.bytes 1 (i32.const 97)) (struct.new $hd.box-i32 (i32.const 1)))
    (call $hd.map_insert (ref.as_non_null (local.get $l0)) (array.new_fixed $hd.bytes 1 (i32.const 98)) (struct.new $hd.box-i32 (i32.const 2)))
    (block (result i32)
      (local.set $tmp1 (call $hd.map_get (ref.as_non_null (local.get $l0)) (array.new_fixed $hd.bytes 1 (i32.const 98))))
      (if (result i32)
        (i32.eq (struct.get $hd.variant $hd.variant-tag (local.get $tmp1)) (i32.const 1))
        (then
          (local.set $l1 (struct.get $hd.box-i32 $hd.box-i32-value (ref.cast (ref $hd.box-i32) (struct.get $hd.variant $hd.variant-payload (local.get $tmp1)))))
          (local.get $l1)
        )
        (else
          (if (result i32)
            (i32.eq (struct.get $hd.variant $hd.variant-tag (local.get $tmp1)) (i32.const 0))
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

(func $fv2 (type $sig2) (param $env anyref)  (result i32)
    (call $f2)
  )

(func $f3 (export "main") (result i32)
    (call $hd.add_i32 (call $f1 (i32.const 10) (i32.const 10)) (call $f2))
  )

(func $fv3 (type $sig2) (param $env anyref)  (result i32)
    (call $f3)
  )
