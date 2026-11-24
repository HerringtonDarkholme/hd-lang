mod tests {
    use hd_wasm::testing::bulk::*;

    use hd_wasm::testing::module::{Ins, Module, Val};
    use hd_wasm::testing::op::Op as I;

    use hd_base::Hash128;
    use hd_wasm::testing::module::{BUF, Sym};
    use std::io::Write;

    #[test]
    fn immutable_copies_fold_all_short_widths_and_preserve_traps() {
        let mut m = Module::default();
        let data: Vec<u8> = (0u8..40)
            .map(|i| i.wrapping_mul(37).wrapping_add(129))
            .collect();
        let at = m.static_bytes(&data, 1);
        for n in 1..=31 {
            let mut f = m.func("constant", vec![Val::I32], vec![]);
            f.get(0);
            f.u32(at + 3);
            f.u32(n);
            f.push(copy(hd_wasm::testing::module::HEAP, BUF));
            assert!(lower(&mut f, |address, len| {
                let start = address.checked_sub(at)? as usize;
                data.get(start..start + len as usize)
            }));
            assert!(!f.body.iter().any(|i| matches!(
                i,
                Ins::Op(I::I32Load(_) | I::I64Load(_) | I::I32Load8U(_) | I::I32Load16U(_))
            )));
            let symbol = Sym::Inst(Hash128(u128::from(n)));
            m.define(symbol.clone(), f);
            m.export(format!("copy{n}"), symbol);
        }
        let bytes = m.finish().expect("link constant copies");
        wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
            .validate_all(&bytes)
            .expect("validate constant copies");
        // cost: one Node process covers all 31 fixed widths, independent of repository size.
        let mut child = hd_testkit::node::command()
            .args(["--input-type=module", "-e", r"
                import assert from 'node:assert/strict';
                const chunks=[]; for await(const c of process.stdin) chunks.push(c);
                const {instance:{exports:e}}=await WebAssembly.instantiate(Buffer.concat(chunks));
                const bytes=new Uint8Array(e['hd.x'].buffer), end=bytes.length;
                for(let n=1;n<=31;n++) {
                    bytes.fill(37);
                    e['copy'+n](64);
                    assert.deepEqual(bytes.slice(64,64+n),Uint8Array.from({length:n},(_,i)=>((i+3)*37+129)&255));
                    for(const to of [end-n+1,0xffffffff]) {
                        bytes.fill(37); const before=bytes.slice();
                        assert.throws(()=>e['copy'+n](to),WebAssembly.RuntimeError);
                        assert.deepEqual(bytes,before,'failed constant copy writes no bytes');
                    }
                }
            "])
            // cost: the same single process runs the bounded byte and trap checks above.
            .stdin(std::process::Stdio::piped()).spawn().expect("start node");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(&bytes)
            .expect("write wasm");
        assert!(child.wait().expect("run constant copies").success());
    }

    #[test]
    fn lowered_copies_and_fills_preserve_bytes_overlap_and_trap_atomicity() {
        let mut m = Module::default();
        let mut next = 1;
        for copying in [true, false] {
            for count in std::iter::once(None).chain((0..=128).map(Some)) {
                let name = format!(
                    "{}{}",
                    if copying { "copy" } else { "fill" },
                    count.map_or_else(|| "dynamic".to_owned(), |n| n.to_string())
                );
                let mut f = m.func(
                    &name,
                    vec![Val::I32; if count.is_some() { 2 } else { 3 }],
                    vec![],
                );
                f.get(0);
                f.get(1);
                if let Some(n) = count {
                    f.i32(n);
                } else {
                    f.get(2);
                }
                f.push(if copying { copy(BUF, BUF) } else { fill(BUF) });
                let symbol = Sym::Inst(Hash128(next));
                next += 1;
                m.define(symbol.clone(), f);
                m.export(name, symbol);
            }
        }
        let bytes = m.finish().expect("link bulk fixture");
        wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
            .validate_all(&bytes)
            .expect("validate lowered bulk operations");
        let mut child = hd_testkit::node::command()
            .args(["--input-type=module", "-e", r"
                import assert from 'node:assert/strict';
                const chunks=[]; for await (const c of process.stdin) chunks.push(c);
                const {instance:{exports:e}}=await WebAssembly.instantiate(Buffer.concat(chunks));
                const bytes=new Uint8Array(e['hd.x'].buffer), end=bytes.length;
                const seed=Uint8Array.from({length:1024},(_,i)=>i%251);
                for(let n=0;n<=128;n++) {
                    const copies=[e.copydynamic];
                    copies.push(e['copy'+n]);
                    for(const copy of copies) for(let from=32;from<40;from++) for(let to=24;to<48;to++) {
                        bytes.set(seed);
                        const want=seed.slice();want.copyWithin(to,from,from+n);
                        copy(to,from,n);
                        assert.deepEqual(bytes.subarray(0,1024),want);
                    }
                    const fills=[e.filldynamic];
                    fills.push(e['fill'+n]);
                    for(const fill of fills) for(let to=24;to<32;to++) for(const value of [0,1,255,0x12345678,-1]) {
                        bytes.set(seed);
                        const want=seed.slice();want.fill(value,to,to+n);
                        fill(to,value,n);
                        assert.deepEqual(bytes.subarray(0,1024),want);
                    }
                    for(const copy of copies) {
                        if(n===0) {
                            copy(end,end,0);
                            assert.throws(()=>copy(end+1,0,0),WebAssembly.RuntimeError);
                            assert.throws(()=>copy(0,end+1,0),WebAssembly.RuntimeError);
                        } else {
                            for(const [to,from] of [[end-n+1,128],[128,end-n+1],[0xffffffff,128],[128,0xffffffff]]) {
                                bytes.fill(37); bytes.fill(93,128,192); bytes.fill(7,end-64);
                                const before=bytes.slice();
                                assert.throws(()=>copy(to,from,n),WebAssembly.RuntimeError,`${copy.name} ${n} ${to} ${from}`);
                                assert.deepEqual(bytes,before,'a failed copy writes no bytes');
                            }
                        }
                    }
                    for(const fill of fills) {
                        if(n===0) {
                            fill(end,255,0);
                            assert.throws(()=>fill(end+1,255,0),WebAssembly.RuntimeError);
                        } else {
                            for(const to of [end-n+1,0xffffffff]) {
                                bytes.fill(37); const before=bytes.slice();
                                assert.throws(()=>fill(to,255,n),WebAssembly.RuntimeError,`${fill.name} ${n} ${to}`);
                                assert.deepEqual(bytes,before,'a failed fill writes no bytes');
                            }
                        }
                    }
                }
                // A full 4-GiB memory: a range ending exactly at 2^32 is valid,
                // and one past it traps with no write, though an address
                // computed with a wrapping add would land at the bottom.
                const memory=e['hd.x'];
                memory.grow(65536-memory.buffer.byteLength/65536);
                const top=2**32, all=new Uint8Array(memory.buffer);
                const windows=()=>[all.slice(0,512),all.slice(top-512)];
                for(let n=1;n<=128;n++) {
                    for(const copy of [e.copydynamic,e['copy'+n]]) {
                        for(let past=1;past<Math.min(n,18);past++) {
                            for(const [to,from] of [[128,top-n+past],[top-n+past,128]]) {
                                all.fill(37,0,512); all.fill(93,top-512);
                                const before=windows();
                                assert.throws(()=>copy(to,from,n),WebAssembly.RuntimeError,`${copy.name} ${n} ${to} ${from}`);
                                assert.deepEqual(windows(),before,'a failed copy writes no bytes');
                            }
                        }
                        all.fill(37,0,512); all.fill(93,top-512); all.fill(5,128,128+n);
                        copy(top-n,128,n);
                        assert.deepEqual(all.slice(top-n),new Uint8Array(n).fill(5));
                        all.fill(9,top-n);
                        copy(128,top-n,n);
                        assert.deepEqual(all.slice(128,128+n),new Uint8Array(n).fill(9));
                    }
                    for(const fill of [e.filldynamic,e['fill'+n]]) {
                        for(let past=1;past<Math.min(n,18);past++) {
                            all.fill(37,0,512); all.fill(93,top-512);
                            const before=windows();
                            assert.throws(()=>fill(top-n+past,255,n),WebAssembly.RuntimeError,`${fill.name} ${n} ${past}`);
                            assert.deepEqual(windows(),before,'a failed fill writes no bytes');
                        }
                        fill(top-n,3,n);
                        assert.deepEqual(all.slice(top-n),new Uint8Array(n).fill(3));
                    }
                }
            "])
            .stdin(std::process::Stdio::piped()).spawn().expect("start node");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(&bytes)
            .expect("write wasm");
        assert!(child.wait().expect("run bulk fixture").success());
    }
}
