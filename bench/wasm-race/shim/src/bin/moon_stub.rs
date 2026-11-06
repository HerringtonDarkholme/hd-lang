//! Turns MoonBit's linear wasm output into a plain WASI command for the `wasmtime` CLI.
//! MoonBit imports a clock and argument reader through externref handles. These stubs return
//! null handles and keep the state in wasm globals; only those ten imports change, and
//! `fd_write`, `_start` and the program's code stay as compiled.
use walrus::ir::{BinaryOp as BinOp, ExtendedLoad, LoadKind, MemArg, StoreKind, UnaryOp, Value};
use walrus::{ConstExpr, DataKind, ExportItem, FunctionId, ImportKind, Module, RefType, ValType};
use BinOp as B;

const W: &str = "wasi_snapshot_preview1";
const T: &str = "__moonbit_time_unstable";
const F: &str = "__moonbit_fs_unstable";
const WINDOW: i32 = 4096; // bytes borrowed from the top of MoonBit's memory for WASI calls
const SENTINEL: i32 = 9000;

fn a(o: u32) -> MemArg {
    MemArg { align: 1, offset: o }
}

fn import_fn(m: &Module, module: &str, name: &str) -> FunctionId {
    let id = m.imports.find(module, name).unwrap_or_else(|| panic!("no import {module} {name}"));
    match m.imports.get(id).kind {
        ImportKind::Function(f) => f,
        _ => panic!("{module} {name} is not a function"),
    }
}

fn main() {
    let v: Vec<String> = std::env::args().collect();
    let mut m = Module::from_file(&v[1]).expect("read module");
    let mem = m
        .exports
        .iter()
        .find_map(|e| match (e.name.as_str() == "memory", e.item) {
            (true, ExportItem::Memory(x)) => Some(x),
            _ => None,
        })
        .expect("memory");
    let private = m.memories.add_local(false, false, 1, None, None);
    let sentinel = b"ffi_end_of_/string_array\0".to_vec();
    m.data.add(
        DataKind::Active { memory: private, offset: ConstExpr::Value(Value::I32(SENTINEL)) },
        sentinel,
    );

    use ValType::{I32, I64};
    let zero32 = || ConstExpr::Value(Value::I32(0));
    let start_ns = m.globals.add_local(I64, true, false, ConstExpr::Value(Value::I64(0)));
    let argc = m.globals.add_local(I32, true, false, zero32());
    let delta = m.globals.add_local(I32, true, false, zero32());
    let next = m.globals.add_local(I32, true, false, zero32());
    let cur = m.globals.add_local(I32, true, false, zero32());
    let chr = m.globals.add_local(I32, true, false, zero32());

    let t = m.types.add(&[I32, I64, I32], &[I32]);
    let (clock_time_get, _) = m.add_import_func(W, "clock_time_get", t);
    let t = m.types.add(&[I32, I32], &[I32]);
    let (args_sizes_get, _) = m.add_import_func(W, "args_sizes_get", t);
    let (args_get, _) = m.add_import_func(W, "args_get", t);

    // A helper: nanoseconds now, borrowing the top 8 bytes of MoonBit's memory for the call.
    let saved = m.locals.add(I64);
    let ptr = m.locals.add(I32);
    let ns = m.locals.add(I64);
    let i64l = LoadKind::I64 { atomic: false };
    let i64s = StoreKind::I64 { atomic: false };
    let mut now = walrus::FunctionBuilder::new(&mut m.types, &[], &[I64]);
    {
        let mut s = now.func_body();
        s.memory_size(mem).i32_const(16).binop(B::I32Shl).i32_const(8).binop(B::I32Sub).local_set(ptr);
        s.local_get(ptr).load(mem, i64l, a(0)).local_set(saved);
        s.i32_const(1).i64_const(1000).local_get(ptr).call(clock_time_get).drop();
        s.local_get(ptr).load(mem, i64l, a(0)).local_set(ns);
        s.local_get(ptr).local_get(saved).store(mem, i64s, a(0));
        s.local_get(ns);
    }
    let now_fn = now.finish(vec![], &mut m.funcs);

    let f = import_fn(&m, T, "instant_now");
    m.replace_imported_func(f, |(b, _)| {
        let mut s = b.func_body();
        s.call(now_fn).global_set(start_ns);
        s.ref_null(RefType::Externref);
    })
    .unwrap();
    let f = import_fn(&m, T, "instant_elapsed_as_secs_f64");
    m.replace_imported_func(f, |(b, _)| {
        let mut s = b.func_body();
        s.call(now_fn).global_get(start_ns).binop(B::I64Sub);
        s.unop(UnaryOp::F64ConvertUI64).f64_const(1e9).binop(B::F64Div);
    })
    .unwrap();

    // args_get: read argv through a window at the top of MoonBit's memory, keep a copy privately.
    let top = m.locals.add(I32);
    let f = import_fn(&m, F, "args_get");
    m.replace_imported_func(f, |(b, _)| {
        let mut s = b.func_body();
        s.memory_size(mem).i32_const(16).binop(B::I32Shl).i32_const(WINDOW).binop(B::I32Sub).local_set(top);
        // Save the window to private[0..4096].
        s.i32_const(0).local_get(top).i32_const(WINDOW).memory_copy(mem, private);
        s.local_get(top).local_get(top).i32_const(4).binop(B::I32Add).call(args_sizes_get).drop();
        s.local_get(top).load(mem, LoadKind::I32 { atomic: false }, a(0)).global_set(argc);
        s.local_get(top).local_get(top).i32_const(2048).binop(B::I32Add).call(args_get).drop();
        // Copy the argv window to private[4096..8192]; a pointer p reads at p + delta.
        s.i32_const(WINDOW).local_get(top).i32_const(WINDOW).memory_copy(mem, private);
        s.i32_const(WINDOW).local_get(top).binop(B::I32Sub).global_set(delta);
        // Restore the window.
        s.local_get(top).i32_const(0).i32_const(WINDOW).memory_copy(private, mem);
        s.ref_null(RefType::Externref);
    })
    .unwrap();
    let f = import_fn(&m, F, "begin_read_string_array");
    m.replace_imported_func(f, |(b, _)| {
        let mut s = b.func_body();
        s.i32_const(0).global_set(next);
        s.ref_null(RefType::Externref);
    })
    .unwrap();
    let f = import_fn(&m, F, "string_array_read_string");
    m.replace_imported_func(f, |(b, _)| {
        let mut s = b.func_body();
        s.global_get(next).global_get(argc).binop(B::I32LtU);
        s.if_else(
            None,
            |t| {
                t.global_get(next)
                    .i32_const(4)
                    .binop(B::I32Mul)
                    .i32_const(WINDOW)
                    .binop(B::I32Add)
                    .load(private, LoadKind::I32 { atomic: false }, a(0))
                    .global_get(delta)
                    .binop(B::I32Add)
                    .global_set(cur);
                t.global_get(next).i32_const(1).binop(B::I32Add).global_set(next);
            },
            |e| {
                e.i32_const(SENTINEL).global_set(cur);
            },
        );
        s.ref_null(RefType::Externref);
    })
    .unwrap();
    let f = import_fn(&m, F, "begin_read_string");
    m.replace_imported_func(f, |(b, _)| {
        let mut s = b.func_body();
        s.global_get(cur).global_set(chr);
        s.ref_null(RefType::Externref);
    })
    .unwrap();
    let c = m.locals.add(I32);
    let f = import_fn(&m, F, "string_read_char");
    m.replace_imported_func(f, |(b, _)| {
        let mut s = b.func_body();
        s.global_get(chr)
            .load(private, LoadKind::I32_8 { kind: ExtendedLoad::ZeroExtend }, a(0))
            .local_tee(c)
            .unop(UnaryOp::I32Eqz);
        s.if_else(
            I32,
            |t| {
                t.i32_const(-1);
            },
            |e| {
                e.global_get(chr).i32_const(1).binop(B::I32Add).global_set(chr);
                e.local_get(c);
            },
        );
    })
    .unwrap();
    for name in ["finish_read_string_array", "finish_read_string"] {
        let f = import_fn(&m, F, name);
        m.replace_imported_func(f, |(b, _)| {
            b.func_body();
        })
        .unwrap();
    }
    m.emit_wasm_file(&v[2]).expect("write module");
}
