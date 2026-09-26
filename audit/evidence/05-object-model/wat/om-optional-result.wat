;; commit bd985d7; command: om-build.ts (emitWat of audit/probes/arch/object-model/om-optional-result.hd); date: 2026-09-25T20:54:06.500Z
;; excerpt: rec types, user globals, user functions (runtime $hd.* functions omitted)
(rec
    (type $sig0 (func (param anyref) (param i32) (result (ref null $hd.variant))))
    (type $sig1 (func (param anyref) (param i32) (result (ref null $hd.variant))))
    (type $sig2 (func (param anyref) (param i32) (result (ref null $hd.variant))))
    (type $sig3 (func (param anyref) (param i32) (result (ref null $hd.variant))))
    (type $sig4 (func (param anyref) (param i32) (result (ref null $hd.variant))))
    (type $sig5 (func (param anyref) (param (ref null $hd.variant)) (result i32)))
    (type $sig6 (func (param anyref) (param (ref null $hd.variant)) (result i32)))
    (type $sig7 (func (param anyref) (param (ref null $hd.variant)) (result i32)))
    (type $sig8 (func (param anyref) (result i32)))
    (type $sig9 (func (param anyref) (param f64) (param f64) (result i32)))
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
    (type $closure5 (struct
      (field $closure5fn (ref $sig5))
      (field $closure5env anyref)))
    (type $closure6 (struct
      (field $closure6fn (ref $sig6))
      (field $closure6env anyref)))
    (type $closure7 (struct
      (field $closure7fn (ref $sig7))
      (field $closure7env anyref)))
    (type $closure8 (struct
      (field $closure8fn (ref $sig8))
      (field $closure8env anyref)))
    (type $closure9 (struct
      (field $closure9fn (ref $sig9))
      (field $closure9env anyref)))
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
      (field $d0f0 (mut i32))))
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

(func $f0 (param $l0 i32) (result (ref null $hd.variant))
    (if (result (ref null $hd.variant)) (local.get $l0)
      (then
        (struct.new $hd.variant (i32.const 1) (struct.new $hd.box-i32 (i32.const 40)))
      )
      (else
        (struct.new $hd.variant (i32.const 0) (ref.null any))
      )
    )
  )

(func $fv0 (type $sig0) (param $env anyref) (param $l0 i32) (result (ref null $hd.variant))
    (call $f0 (local.get $l0))
  )

(func $f1 (param $l0 i32) (result (ref null $hd.variant))
    (if (result (ref null $hd.variant)) (local.get $l0)
      (then
        (struct.new $hd.variant (i32.const 1) (struct.new $hd.box-f64 (f64.const 1.5)))
      )
      (else
        (struct.new $hd.variant (i32.const 0) (ref.null any))
      )
    )
  )

(func $fv1 (type $sig1) (param $env anyref) (param $l0 i32) (result (ref null $hd.variant))
    (call $f1 (local.get $l0))
  )

(func $f2 (param $l0 i32) (result (ref null $hd.variant))
    (local $tmp0 i32)
    (if (result (ref null $hd.variant)) (local.get $l0)
      (then
        (struct.new $hd.variant (i32.const 1) (block (result (ref null $d0))
          (local.set $tmp0 (i32.const 1))
          (struct.new $d0 (local.get $tmp0))
        ))
      )
      (else
        (struct.new $hd.variant (i32.const 0) (ref.null any))
      )
    )
  )

(func $fv2 (type $sig2) (param $env anyref) (param $l0 i32) (result (ref null $hd.variant))
    (call $f2 (local.get $l0))
  )

(func $f3 (param $l0 i32) (result (ref null $hd.variant))
    (if (result (ref null $hd.variant)) (local.get $l0)
      (then
        (struct.new $hd.variant (i32.const 1) (array.new_fixed $hd.bytes 1 (i32.const 116)))
      )
      (else
        (struct.new $hd.variant (i32.const 0) (ref.null any))
      )
    )
  )

(func $fv3 (type $sig3) (param $env anyref) (param $l0 i32) (result (ref null $hd.variant))
    (call $f3 (local.get $l0))
  )

(func $f4 (param $l0 i32) (result (ref null $hd.variant))
    (if (result (ref null $hd.variant)) (local.get $l0)
      (then
        (struct.new $hd.variant (i32.const 0) (struct.new $hd.box-i32 (i32.const 1)))
      )
      (else
        (struct.new $hd.variant (i32.const 1) (array.new_fixed $hd.bytes 3 (i32.const 98) (i32.const 97) (i32.const 100)))
      )
    )
  )

(func $fv4 (type $sig4) (param $env anyref) (param $l0 i32) (result (ref null $hd.variant))
    (call $f4 (local.get $l0))
  )

(func $f5 (param $l0 (ref null $hd.variant)) (result i32)
    (local $l1 i32)
    (local $tmp0 (ref null $hd.variant))
    (block (result i32)
      (local.set $tmp0 (local.get $l0))
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

(func $fv5 (type $sig5) (param $env anyref) (param $l0 (ref null $hd.variant)) (result i32)
    (call $f5 (local.get $l0))
  )

(func $f6 (param $l0 (ref null $hd.variant)) (result i32)
    (local $l1 (ref null $d0))
    (local $tmp0 (ref null $hd.variant))
    (block (result i32)
      (local.set $tmp0 (local.get $l0))
      (if (result i32)
        (i32.eq (struct.get $hd.variant $hd.variant-tag (local.get $tmp0)) (i32.const 0))
        (then
          (i32.const 0)
        )
        (else
          (if (result i32)
            (i32.eq (struct.get $hd.variant $hd.variant-tag (local.get $tmp0)) (i32.const 1))
            (then
              (local.set $l1 (ref.cast (ref null $d0) (struct.get $hd.variant $hd.variant-payload (local.get $tmp0))))
              (struct.get $d0 $d0f0 (local.get $l1))
            )
            (else
              (unreachable)
            )
          )
        )
      )
    )
  )

(func $fv6 (type $sig6) (param $env anyref) (param $l0 (ref null $hd.variant)) (result i32)
    (call $f6 (local.get $l0))
  )

(func $f7 (param $l0 (ref null $hd.variant)) (result i32)
    (local $l1 i32)
    (local $l2 (ref null $hd.bytes))
    (local $tmp0 (ref null $hd.variant))
    (block (result i32)
      (local.set $tmp0 (local.get $l0))
      (if (result i32)
        (i32.eq (struct.get $hd.variant $hd.variant-tag (local.get $tmp0)) (i32.const 0))
        (then
          (local.set $l1 (struct.get $hd.box-i32 $hd.box-i32-value (ref.cast (ref $hd.box-i32) (struct.get $hd.variant $hd.variant-payload (local.get $tmp0)))))
          (local.get $l1)
        )
        (else
          (if (result i32)
            (i32.eq (struct.get $hd.variant $hd.variant-tag (local.get $tmp0)) (i32.const 1))
            (then
              (local.set $l2 (ref.cast (ref null $hd.bytes) (struct.get $hd.variant $hd.variant-payload (local.get $tmp0))))
              (call $hd.string_len (local.get $l2))
            )
            (else
              (unreachable)
            )
          )
        )
      )
    )
  )

(func $fv7 (type $sig7) (param $env anyref) (param $l0 (ref null $hd.variant)) (result i32)
    (call $f7 (local.get $l0))
  )

(func $f8 (export "main") (result i32)
    (local $l0 f64)
    (local $l1 i32)
    (local $l2 (ref null $hd.bytes))
    (local $l3 i32)
    (local $tmp0 (ref null $hd.variant))
    (local $tmp1 (ref null $hd.variant))
    (local.set $l1 (block (result i32)
      (local.set $tmp0 (call $f1 (i32.const 1)))
      (if (result i32)
        (i32.eq (struct.get $hd.variant $hd.variant-tag (local.get $tmp0)) (i32.const 0))
        (then
          (i32.const 0)
        )
        (else
          (if (result i32)
            (i32.eq (struct.get $hd.variant $hd.variant-tag (local.get $tmp0)) (i32.const 1))
            (then
              (local.set $l0 (struct.get $hd.box-f64 $hd.box-f64-value (ref.cast (ref $hd.box-f64) (struct.get $hd.variant $hd.variant-payload (local.get $tmp0)))))
              (call $f9 (local.get $l0) (f64.const 1))
            )
            (else
              (unreachable)
            )
          )
        )
      )
    ))
    (local.set $l3 (block (result i32)
      (local.set $tmp1 (call $f3 (i32.const 0)))
      (if (result i32)
        (i32.eq (struct.get $hd.variant $hd.variant-tag (local.get $tmp1)) (i32.const 0))
        (then
          (i32.const 0)
        )
        (else
          (if (result i32)
            (i32.eq (struct.get $hd.variant $hd.variant-tag (local.get $tmp1)) (i32.const 1))
            (then
              (local.set $l2 (ref.cast (ref null $hd.bytes) (struct.get $hd.variant $hd.variant-payload (local.get $tmp1))))
              (call $hd.string_len (local.get $l2))
            )
            (else
              (unreachable)
            )
          )
        )
      )
    ))
    (call $hd.add_i32 (call $hd.add_i32 (call $hd.add_i32 (call $hd.add_i32 (call $f5 (call $f0 (i32.const 1))) (call $f6 (call $f2 (i32.const 1)))) (call $f7 (call $f4 (i32.const 1)))) (local.get $l1)) (local.get $l3))
  )

(func $fv8 (type $sig8) (param $env anyref)  (result i32)
    (call $f8)
  )

(func $f9 (export "above") (param $l0 f64) (param $l1 f64) (result i32)
    (if (result i32) (f64.eq (local.get $l0) (local.get $l1))
      (then
        (i32.const 1)
      )
      (else
        (i32.const 0)
      )
    )
  )

(func $fv9 (type $sig9) (param $env anyref) (param $l0 f64) (param $l1 f64) (result i32)
    (call $f9 (local.get $l0) (local.get $l1))
  )
