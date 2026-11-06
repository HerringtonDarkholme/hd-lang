//! Turns an `hd build --release` module into a WASI command for a plain `wasmtime` CLI run.
//! Only the eight `hd:*` host imports and the entry change; the program's code is untouched.
//! `hd.x` is hd's separate exchange memory, so it is exported as WASI's `memory` and the shim's
//! scratch lives there. The float primitives trap: only JSON's float text needs them.
use walrus::ir::{BinaryOp, LoadKind, MemArg, StoreKind};
use walrus::{ExportItem, FunctionBuilder, FunctionId, ImportKind, InstrSeqBuilder, MemoryId, Module, ValType};

const IOV: i32 = 60008; // iovec for fd_write
const NWRITTEN: i32 = 60016;
const CLOCK: i32 = 60032;
const ARGC: i32 = 60040;
const ARGV_SIZE: i32 = 60044;
const ARGV: i32 = 61000;
const ARGV_BUF: i32 = 62000;
const W: &str = "wasi_snapshot_preview1";

fn arg(o: u32) -> MemArg {
    MemArg { align: 1, offset: o }
}

fn import_fn(m: &mut Module, module: &str, name: &str) -> FunctionId {
    let id = m.imports.find(module, name).unwrap_or_else(|| panic!("no import {module} {name}"));
    match m.imports.get(id).kind {
        ImportKind::Function(f) => f,
        _ => panic!("{module} {name} is not a function"),
    }
}

fn export_fn(m: &Module, name: &str) -> FunctionId {
    m.exports
        .iter()
        .find_map(|e| match (e.name.as_str() == name, e.item) {
            (true, ExportItem::Function(f)) => Some(f),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no export {name}"))
}

/// Writes exchange bytes [0, len) to `fd`.
fn write_fd(b: &mut InstrSeqBuilder, mem: MemoryId, fd_write: FunctionId, fd: i32, len: walrus::LocalId) {
    b.i32_const(IOV).i32_const(0).store(mem, StoreKind::I32 { atomic: false }, arg(0));
    b.i32_const(IOV + 4).local_get(len).store(mem, StoreKind::I32 { atomic: false }, arg(0));
    b.i32_const(fd).i32_const(IOV).i32_const(1).i32_const(NWRITTEN).call(fd_write).drop();
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut m = Module::from_file(&a[1]).expect("read module");
    let mem = m
        .exports
        .iter()
        .find_map(|e| match (e.name.as_str() == "hd.x", e.item) {
            (true, ExportItem::Memory(x)) => Some(x),
            _ => None,
        })
        .expect("hd.x");
    m.exports.add("memory", mem);

    use ValType::{I32, I64};
    let t = m.types.add(&[I32, I32, I32, I32], &[I32]);
    let (fd_write, _) = m.add_import_func(W, "fd_write", t);
    let t = m.types.add(&[I32, I32], &[I32]);
    let (args_sizes_get, _) = m.add_import_func(W, "args_sizes_get", t);
    let (args_get, _) = m.add_import_func(W, "args_get", t);
    let t = m.types.add(&[I32, I64, I32], &[I32]);
    let (clock_time_get, _) = m.add_import_func(W, "clock_time_get", t);
    let t = m.types.add(&[I32], &[]);
    let (proc_exit, _) = m.add_import_func(W, "proc_exit", t);

    let f = import_fn(&mut m, "hd:Clock", "monotonic");
    m.replace_imported_func(f, |(b, _)| {
        let mut s = b.func_body();
        s.i32_const(1).i64_const(1000).i32_const(CLOCK).call(clock_time_get).drop();
        s.i32_const(CLOCK).load(mem, LoadKind::I64 { atomic: false }, arg(0));
        s.i64_const(1_000_000).binop(BinaryOp::I64DivU);
    })
    .unwrap();

    let f = import_fn(&mut m, "hd:rt", "hash_seed");
    m.replace_imported_func(f, |(b, _)| {
        b.func_body().i64_const(0x1234_5678_9abc_def1);
    })
    .unwrap();

    let f = import_fn(&mut m, "hd:Console", "write_sync");
    m.replace_imported_func(f, |(b, args)| {
        let len = args[0];
        let mut s = b.func_body();
        write_fd(&mut s, mem, fd_write, 1, len);
        // The encoded `Ok(())` at offset 0, and its length.
        s.i32_const(0).i32_const(0).store(mem, StoreKind::I32_8 { atomic: false }, arg(0));
        s.i32_const(1);
    })
    .unwrap();

    let f = import_fn(&mut m, "hd:rt", "stderr");
    m.replace_imported_func(f, |(b, args)| {
        let len = args[0];
        let mut s = b.func_body();
        write_fd(&mut s, mem, fd_write, 2, len);
    })
    .unwrap();

    let f = import_fn(&mut m, "hd:rt", "panic");
    m.replace_imported_func(f, |(b, _)| {
        b.func_body();
    })
    .unwrap();

    for (name, ()) in [("format_f64", ()), ("parse_f64", ())] {
        let f = import_fn(&mut m, "hd:prim", name);
        m.replace_imported_func(f, |(b, _)| {
            b.func_body().unreachable();
        })
        .unwrap();
    }

    // Args: the wire list of strings at offset 0, without argv[0] (the module's name).
    let k = m.locals.add(I32);
    let out = m.locals.add(I32);
    let p = m.locals.add(I32);
    let n = m.locals.add(I32);
    let f = import_fn(&mut m, "hd:Args", "list");
    m.replace_imported_func(f, |(b, _)| {
        let mut s = b.func_body();
        s.i32_const(ARGC).i32_const(ARGV_SIZE).call(args_sizes_get).drop();
        s.i32_const(ARGV).i32_const(ARGV_BUF).call(args_get).drop();
        let load32 = LoadKind::I32 { atomic: false };
        let load8 = LoadKind::I32_8 { kind: walrus::ir::ExtendedLoad::ZeroExtend };
        let store8 = StoreKind::I32_8 { atomic: false };
        // count = argc - 1, one byte (arguments here are few).
        s.i32_const(0)
            .i32_const(ARGC)
            .load(mem, load32, arg(0))
            .i32_const(1)
            .binop(BinaryOp::I32Sub)
            .store(mem, store8, arg(0));
        s.i32_const(1).local_set(out);
        s.i32_const(1).local_set(k);
        s.block(None, |done| {
            let done_id = done.id();
            done.loop_(None, |lp| {
                let lp_id = lp.id();
                lp.local_get(k)
                    .i32_const(ARGC)
                    .load(mem, load32, arg(0))
                    .binop(BinaryOp::I32GeU)
                    .br_if(done_id);
                lp.local_get(k)
                    .i32_const(4)
                    .binop(BinaryOp::I32Mul)
                    .i32_const(ARGV)
                    .binop(BinaryOp::I32Add)
                    .load(mem, load32, arg(0))
                    .local_set(p);
                // strlen
                lp.i32_const(0).local_set(n);
                lp.block(None, |sd| {
                    let sd_id = sd.id();
                    sd.loop_(None, |sl| {
                        let sl_id = sl.id();
                        sl.local_get(p)
                            .local_get(n)
                            .binop(BinaryOp::I32Add)
                            .load(mem, load8, arg(0))
                            .unop(walrus::ir::UnaryOp::I32Eqz)
                            .br_if(sd_id);
                        sl.local_get(n).i32_const(1).binop(BinaryOp::I32Add).local_set(n);
                        sl.br(sl_id);
                    });
                });
                // one-byte length, then the bytes
                lp.local_get(out).local_get(n).store(mem, store8, arg(0));
                lp.local_get(out).i32_const(1).binop(BinaryOp::I32Add).local_set(out);
                lp.local_get(out).local_get(p).local_get(n).memory_copy(mem, mem);
                lp.local_get(out).local_get(n).binop(BinaryOp::I32Add).local_set(out);
                lp.local_get(k).i32_const(1).binop(BinaryOp::I32Add).local_set(k);
                lp.br(lp_id);
            });
        });
        s.local_get(out);
    })
    .unwrap();

    // _start: hd.init, then one poll; these programs never suspend, so -1 is a failure.
    let init = export_fn(&m, "hd.init");
    let poll = export_fn(&m, "hd.poll");
    let mut b = FunctionBuilder::new(&mut m.types, &[], &[]);
    b.func_body().call(init).i32_const(0).call(poll).call(proc_exit);
    let start = b.finish(vec![], &mut m.funcs);
    m.exports.add("_start", start);

    m.emit_wasm_file(&a[2]).expect("write module");
}
