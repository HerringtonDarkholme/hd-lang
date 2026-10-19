//! Runtime pieces the compiler generates (`crates/hd_wasm/DESIGN.md`
//! §23.9): the entry exports, panics, the exchange-buffer copy, the bump
//! allocation that serves until the std allocator (job 5), integer text
//! and the host provider stubs. Each is a [`Helper`], built once on its
//! first request.

use hd_base::{Hash128, StageResult};
use hd_types::{Ty, TyData, TyList};
use wasm_encoder::{BlockType, Instruction};

use crate::module::{BUF, Func, Global, HEAP, Import, Ins, Module, Sym, Val, mem};
use crate::{Entry, Input, unsupported};

/// A generated function, by what it does.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Helper {
    /// `hd.init`: the inits in order; a flagged one belongs to a module
    /// other than the entry's and runs as a forbidden context
    /// (`req.drive.block-on.forbidden-contexts`).
    EntryInit { inits: Vec<(Hash128, bool)> },
    /// `hd.poll(ctx)`: runs `main` with one host provider per vtable in
    /// `providers` (data 0), passes its results to `report` when there is
    /// one, then runs `then` as the script's later inits; returns the exit
    /// status (0 without a report).
    EntryPoll {
        main: Hash128,
        providers: Vec<usize>,
        results: u32,
        report: Option<Hash128>,
        then: Vec<(Hash128, bool)>,
    },
    /// `()`: the panic report `MESSAGE` (`CATEGORY: TEXT`), then a trap.
    Panic(String),
    /// `(text)`: the `explicit-panic` report of `panic(text)`.
    PanicStr,
    /// `(category, message)`: std's `panic_with`, whose report shows the
    /// message after the category when it is not empty
    /// (`flow.panic.report`).
    PanicWith,
    /// `(text) -> len`: copies a string into the exchange buffer at 0,
    /// growing the buffer to fit, and returns its length.
    ToBuf,
    /// `(size) -> address`: `size` bytes of the heap, 8-aligned, from the
    /// bump pointer; the heap grows to fit, and a refused growth is
    /// `heap-exhausted`.
    Alloc,
    /// `(value: i64) -> string`: an integer's decimal text, read as
    /// signed when `signed`.
    FmtInt { signed: bool },
    /// `(a, b) -> bool`: two strings' bytes are equal.
    StrEq,
    /// The forbidden-context check of `block_on`
    /// (`req.drive.block-on.forbidden-contexts`).
    ForbidCheck,
    /// A host provider's vtable slot for a waiting method that takes one
    /// string and answers an enum of tags (`crates/hd_run_wasmtime/DESIGN.md`
    /// §17.4): `(data, text) -> frame`, a cold suspension whose poll is
    /// [`Helper::HostPoll`].
    HostCold { module: String, method: String },
    /// `(frame) -> (ready, tag, payload tag)`: starts the host call on its
    /// first poll and finishes it on the poll after the host reported it
    /// done. While a program runs one host call at a time, the completion
    /// `hd:rt.block` reports is this call's.
    HostPoll { module: String, method: String },
}

/// The frame of a host call: its poll slot, its state (0 not started, 1
/// waiting, 2 done), the host handle and the text argument.
const HOST_FRAME: i32 = 20;

/// Defines the entry exports of `input.entry` in `m`.
pub fn entry(input: &Input<'_>, m: &mut Module) -> StageResult<()> {
    let (main, key, before, after, report) = match &input.entry {
        Entry::Main {
            main,
            key,
            inits,
            report,
        } => (*main, *key, inits.clone(), Vec::new(), *report),
        Entry::Script {
            init,
            key,
            before,
            after,
            report,
        } => (*init, *key, before.clone(), after.clone(), *report),
        Entry::Unsupported(what) => return unsupported(*what),
    };
    let mut providers = Vec::new();
    for k in input.env.row_keys(main, TyList::EMPTY) {
        providers.push(host_vtable(input, m, k)?);
    }
    let ret = input.env.ret(main).unwrap_or(Ty::VOID);
    let results = crate::layout::vals(input.pool, input.env, ret)?.len();
    m.export("hd.init", Sym::Helper(Helper::EntryInit { inits: before }));
    m.export(
        "hd.poll",
        Sym::Helper(Helper::EntryPoll {
            main: key,
            providers,
            results: u32::try_from(results).expect("results"),
            report,
            then: after,
        }),
    );
    Ok(())
}

/// The vtable of the host's provider of capability `key`, in the trait's
/// method order (`ProgramEnv::trait_methods`). A method the ABI table
/// does not list is answered by its first method, as the Wasm GC
/// backend's default profile does for `write_error_line!`.
fn host_vtable(input: &Input<'_>, m: &mut Module, key: Ty) -> StageResult<usize> {
    let TyData::TraitValue { def, args, .. } = input.pool.get(key) else {
        return unsupported("a provider key that is not a trait");
    };
    let p = (input.path)(def).replace('/', ".");
    let Some(host) = hd_host_abi::host_trait(&p).filter(|_| args == TyList::EMPTY) else {
        return unsupported(format!("the default provider of `{p}`"));
    };
    let mut slots = Vec::new();
    for method in input.env.trait_methods(def) {
        let mp = (input.path)(method);
        let name = mp.rsplit(['.', '/']).next().unwrap_or("");
        let Some(hm) = host
            .methods
            .iter()
            .find(|x| x.name == name)
            .or_else(|| host.methods.first())
        else {
            return unsupported(format!("the host method `{name}`"));
        };
        let one_string = matches!(hm.params, [hd_host_abi::Codec::Buffer("string")]);
        if hm.wait != hd_host_abi::Wait::May || !one_string || !tags_result(input, method) {
            return unsupported(format!("the host method `{}.{}`", host.key, hm.name));
        }
        slots.push(Sym::Helper(Helper::HostCold {
            module: format!("hd:{}", host.key),
            method: hm.name.to_owned(),
        }));
    }
    Ok(m.vtable(slots))
}

/// Whether a host method's result is a `Result` of `void` and an enum of
/// fieldless variants, which crosses as tags.
fn tags_result(input: &Input<'_>, method: hd_base::DefId) -> bool {
    let Some(ret) = input.env.ret(method) else {
        return false;
    };
    let fieldless = |t: Ty| match input.pool.get(t) {
        TyData::Adt { def, args } => input
            .env
            .enum_variants(def, args)
            .is_some_and(|vs| vs.iter().all(Vec::is_empty)),
        _ => false,
    };
    match input.pool.get(ret) {
        TyData::Adt { def, args } => input.env.enum_variants(def, args).is_some_and(|vs| {
            matches!(vs.as_slice(), [ok, err] if ok.iter().all(|t| *t == Ty::VOID)
                && matches!(err.as_slice(), [e] if fieldless(*e)))
        }),
        _ => false,
    }
}

/// Adds `by` to the forbidden-context counter.
fn forbid_by(f: &mut Func, by: i32) {
    f.global_get(Global::Forbid);
    f.i32(by);
    f.op(Instruction::I32Add);
    f.global_set(Global::Forbid);
}

/// Calls init `k`, as a forbidden context when flagged.
fn call_init(f: &mut Func, k: Hash128, forbid: bool) {
    if forbid {
        forbid_by(f, 1);
    }
    f.call(Sym::Inst(k));
    if forbid {
        forbid_by(f, -1);
    }
}

fn rt_import(name: &str, params: &[Val], results: &[Val]) -> Sym {
    Sym::Import(Import::new("hd:rt", name, params, results))
}

/// Writes the literal `text` to standard error.
fn write_lit(f: &mut Func, m: &mut Module, text: &str) {
    let at = m.static_bytes(text.as_bytes(), 1);
    f.u32(at);
    f.u32(u32::try_from(text.len()).expect("literal"));
    f.helper(Helper::ToBuf);
    f.call(rt_import("stderr", &[Val::I32], &[]));
}

/// Writes the string in locals `s` and `s + 1` to standard error.
fn write_str(f: &mut Func, s: u32) {
    f.get(s);
    f.get(s + 1);
    f.helper(Helper::ToBuf);
    f.call(rt_import("stderr", &[Val::I32], &[]));
}

/// Builds helper `h`; it may ask `m` for more helpers and static data.
pub fn helper(h: &Helper, m: &mut Module) -> Func {
    match h {
        Helper::EntryInit { inits } => {
            let mut f = Func::new("rt:entry_init", vec![], vec![]);
            for (k, forbid) in inits {
                call_init(&mut f, *k, *forbid);
            }
            f
        }
        Helper::EntryPoll {
            main,
            providers,
            results,
            report,
            then,
        } => {
            // Parameter 0 is the driver's `PollContext`; a plain entry
            // ignores it.
            let mut f = Func::new("rt:entry_poll", vec![Val::I32], vec![Val::I32]);
            for v in providers {
                f.i32(0);
                f.body.push(Ins::VTable(*v));
            }
            f.call(Sym::Inst(*main));
            if let Some(k) = report {
                f.call(Sym::Inst(*k));
            } else {
                for _ in 0..*results {
                    f.op(Instruction::Drop);
                }
                // A script's later inits: its own bodies, which share the
                // first's result, and the others, as forbidden contexts.
                for (k, forbid) in then {
                    call_init(&mut f, *k, *forbid);
                    if !forbid {
                        for _ in 0..*results {
                            f.op(Instruction::Drop);
                        }
                    }
                }
                f.i32(0);
            }
            f
        }
        Helper::Panic(msg) => {
            let mut f = Func::new("rt:panic", vec![], vec![]);
            f.call(rt_import("panic", &[], &[]));
            write_lit(&mut f, m, &format!("{msg}\n"));
            f.op(Instruction::Unreachable);
            f
        }
        Helper::PanicStr => {
            let mut f = Func::new("rt:panic_str", vec![Val::I32, Val::I32], vec![]);
            f.call(rt_import("panic", &[], &[]));
            write_lit(&mut f, m, "explicit-panic: ");
            write_str(&mut f, 0);
            write_lit(&mut f, m, "\n");
            f.op(Instruction::Unreachable);
            f
        }
        Helper::PanicWith => {
            let mut f = Func::new("rt:panic_with", vec![Val::I32; 4], vec![]);
            f.call(rt_import("panic", &[], &[]));
            write_str(&mut f, 0);
            f.get(3);
            f.op(Instruction::If(BlockType::Empty));
            write_lit(&mut f, m, ": ");
            write_str(&mut f, 2);
            f.op(Instruction::End);
            write_lit(&mut f, m, "\n");
            f.op(Instruction::Unreachable);
            f
        }
        Helper::ToBuf => {
            let mut f = Func::new("rt:to_buf", vec![Val::I32, Val::I32], vec![Val::I32]);
            // Grow the buffer by the pages the text needs beyond its size.
            f.get(1);
            f.op(Instruction::MemorySize(BUF));
            f.i32(16);
            f.op(Instruction::I32Shl);
            f.op(Instruction::I32GtU);
            f.op(Instruction::If(BlockType::Empty));
            f.get(1);
            f.op(Instruction::MemorySize(BUF));
            f.i32(16);
            f.op(Instruction::I32Shl);
            f.op(Instruction::I32Sub);
            f.i32(0xffff);
            f.op(Instruction::I32Add);
            f.i32(16);
            f.op(Instruction::I32ShrU);
            f.op(Instruction::MemoryGrow(BUF));
            f.i32(-1);
            f.op(Instruction::I32Eq);
            f.op(Instruction::If(BlockType::Empty));
            f.helper(Helper::Panic(
                "heap-exhausted: the exchange buffer cannot grow".into(),
            ));
            f.op(Instruction::End);
            f.op(Instruction::End);
            f.i32(0);
            f.get(0);
            f.get(1);
            f.op(Instruction::MemoryCopy {
                src_mem: HEAP,
                dst_mem: BUF,
            });
            f.get(1);
            f
        }
        Helper::Alloc => alloc(),
        Helper::StrEq => str_eq(),
        Helper::FmtInt { signed } => fmt_int(*signed),
        Helper::ForbidCheck => {
            let mut f = Func::new("rt:forbid_check", vec![], vec![]);
            f.global_get(Global::Forbid);
            f.op(Instruction::If(BlockType::Empty));
            f.helper(Helper::Panic(
                "suspension-forbidden-context: block_on in a forbidden context".into(),
            ));
            f.op(Instruction::End);
            f
        }
        Helper::HostCold { module, method } => {
            let mut f = Func::new(
                format!("rt:{module}/{method}"),
                vec![Val::I32; 3],
                vec![Val::I32],
            );
            let fr = f.local(Val::I32);
            f.i32(HOST_FRAME);
            f.helper(Helper::Alloc);
            f.set(fr);
            let store = |f: &mut Func, offset: u64, value: &dyn Fn(&mut Func)| {
                f.get(fr);
                value(f);
                f.op(Instruction::I32Store(mem(HEAP, offset, 2)));
            };
            let poll = Sym::Helper(Helper::HostPoll {
                module: module.clone(),
                method: method.clone(),
            });
            store(&mut f, 0, &|f| f.body.push(Ins::TableSlot(poll.clone())));
            store(&mut f, 4, &|f| f.i32(0));
            store(&mut f, 8, &|f| f.i32(-1));
            store(&mut f, 12, &|f| f.get(1));
            store(&mut f, 16, &|f| f.get(2));
            f.get(fr);
            f
        }
        Helper::HostPoll { module, method } => host_poll(module, method),
    }
}

/// [`Helper::Alloc`].
fn alloc() -> Func {
    let mut f = Func::new("rt:alloc", vec![Val::I32], vec![Val::I32]);
    let (at, end) = (f.local(Val::I32), f.local(Val::I32));
    f.global_get(Global::HeapTop);
    f.set(at);
    // end = (at + size + 7) & !7, refusing a wrap past 4 GiB.
    f.get(at);
    f.get(0);
    f.op(Instruction::I32Add);
    f.i32(7);
    f.op(Instruction::I32Add);
    f.i32(-8);
    f.op(Instruction::I32And);
    f.set(end);
    f.get(end);
    f.get(at);
    f.op(Instruction::I32LtU);
    f.op(Instruction::If(BlockType::Empty));
    f.helper(Helper::Panic(
        "heap-exhausted: an allocation does not fit the heap".into(),
    ));
    f.op(Instruction::End);
    f.get(end);
    f.op(Instruction::MemorySize(HEAP));
    f.i32(16);
    f.op(Instruction::I32Shl);
    f.op(Instruction::I32GtU);
    f.op(Instruction::If(BlockType::Empty));
    f.get(end);
    f.op(Instruction::MemorySize(HEAP));
    f.i32(16);
    f.op(Instruction::I32Shl);
    f.op(Instruction::I32Sub);
    f.i32(0xffff);
    f.op(Instruction::I32Add);
    f.i32(16);
    f.op(Instruction::I32ShrU);
    f.op(Instruction::MemoryGrow(HEAP));
    f.i32(-1);
    f.op(Instruction::I32Eq);
    f.op(Instruction::If(BlockType::Empty));
    f.helper(Helper::Panic(
        "heap-exhausted: an allocation does not fit the heap".into(),
    ));
    f.op(Instruction::End);
    f.op(Instruction::End);
    f.get(end);
    f.global_set(Global::HeapTop);
    f.get(at);
    f
}

/// [`Helper::FmtInt`]: the digits go backward into a 20-byte allocation,
/// the most a `u64` or a signed `i64` needs; the string is its tail.
fn fmt_int(signed: bool) -> Func {
    let mut f = Func::new(
        if signed { "rt:fmt_i64" } else { "rt:fmt_u64" },
        vec![Val::I64],
        vec![Val::I32, Val::I32],
    );
    let (buf, at, neg, v) = (
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I64),
    );
    f.i32(20);
    f.helper(Helper::Alloc);
    f.set(buf);
    f.get(buf);
    f.i32(20);
    f.op(Instruction::I32Add);
    f.set(at);
    f.get(0);
    f.set(v);
    if signed {
        // The magnitude of a negative value, read unsigned, is `0 - v`:
        // that holds for `i64::MIN` too.
        f.get(0);
        f.op(Instruction::I64Const(0));
        f.op(Instruction::I64LtS);
        f.set(neg);
        f.get(neg);
        f.op(Instruction::If(BlockType::Empty));
        f.op(Instruction::I64Const(0));
        f.get(0);
        f.op(Instruction::I64Sub);
        f.set(v);
        f.op(Instruction::End);
    }
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(at);
    f.i32(1);
    f.op(Instruction::I32Sub);
    f.set(at);
    f.get(at);
    f.get(v);
    f.op(Instruction::I64Const(10));
    f.op(Instruction::I64RemU);
    f.op(Instruction::I32WrapI64);
    f.i32(i32::from(b'0'));
    f.op(Instruction::I32Add);
    f.op(Instruction::I32Store8(mem(HEAP, 0, 0)));
    f.get(v);
    f.op(Instruction::I64Const(10));
    f.op(Instruction::I64DivU);
    f.set(v);
    f.get(v);
    f.op(Instruction::I64Const(0));
    f.op(Instruction::I64Ne);
    f.op(Instruction::BrIf(0));
    f.op(Instruction::End);
    if signed {
        f.get(neg);
        f.op(Instruction::If(BlockType::Empty));
        f.get(at);
        f.i32(1);
        f.op(Instruction::I32Sub);
        f.set(at);
        f.get(at);
        f.i32(i32::from(b'-'));
        f.op(Instruction::I32Store8(mem(HEAP, 0, 0)));
        f.op(Instruction::End);
    }
    f.get(at);
    f.get(buf);
    f.i32(20);
    f.op(Instruction::I32Add);
    f.get(at);
    f.op(Instruction::I32Sub);
    f
}

/// [`Helper::HostPoll`]: the host contract of a waiting method
/// (`crates/hd_run_wasmtime/DESIGN.md` §17.2). `M.start(len)` answers
/// `(status, value)`: a non-zero status means the call waits, under the
/// handle `value`; zero means the result is in the exchange buffer, `value`
/// bytes long. `M.finish(handle)` answers the result's length once the
/// host reported the call done.
fn host_poll(module: &str, method: &str) -> Func {
    let mut f = Func::new(
        format!("rt:{module}/{method}.poll"),
        vec![Val::I32],
        vec![Val::I32; 3],
    );
    let (st, n) = (f.local(Val::I32), f.local(Val::I32));
    let state = |f: &mut Func| {
        f.get(0);
        f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
    };
    let set_state = |f: &mut Func, v: i32| {
        f.get(0);
        f.i32(v);
        f.op(Instruction::I32Store(mem(HEAP, 4, 2)));
    };
    state(&mut f);
    f.i32(2);
    f.op(Instruction::I32Eq);
    f.op(Instruction::If(BlockType::Empty));
    f.helper(Helper::Panic(
        "suspension-invalid-state: a completed host call was polled".into(),
    ));
    f.op(Instruction::End);
    state(&mut f);
    f.op(Instruction::If(BlockType::Empty));
    // Waiting: the host reported the call done.
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, 8, 2)));
    f.call(Sym::Import(Import::new(
        module,
        &format!("{method}.finish"),
        &[Val::I32],
        &[Val::I32],
    )));
    f.set(n);
    f.op(Instruction::Else);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, 12, 2)));
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, 16, 2)));
    f.helper(Helper::ToBuf);
    f.call(Sym::Import(Import::new(
        module,
        &format!("{method}.start"),
        &[Val::I32],
        &[Val::I32, Val::I32],
    )));
    f.set(n);
    f.set(st);
    f.get(st);
    f.op(Instruction::If(BlockType::Empty));
    f.get(0);
    f.get(n);
    f.op(Instruction::I32Store(mem(HEAP, 8, 2)));
    set_state(&mut f, 1);
    f.i32(0);
    f.i32(0);
    f.i32(0);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.op(Instruction::End);
    set_state(&mut f, 2);
    // The tags codec: the variant index, then the payload's variant index
    // when the result has one.
    f.i32(1);
    f.i32(0);
    f.op(Instruction::I32Load8U(mem(BUF, 0, 0)));
    f.get(n);
    f.i32(1);
    f.op(Instruction::I32GtS);
    f.op(Instruction::If(BlockType::Result(
        wasm_encoder::ValType::I32,
    )));
    f.i32(0);
    f.op(Instruction::I32Load8U(mem(BUF, 1, 0)));
    f.op(Instruction::Else);
    f.i32(0);
    f.op(Instruction::End);
    f
}

/// [`Helper::StrEq`]: equal lengths, then each byte.
fn str_eq() -> Func {
    let mut f = Func::new("rt:str_eq", vec![Val::I32; 4], vec![Val::I32]);
    let k = f.local(Val::I32);
    f.get(1);
    f.get(3);
    f.op(Instruction::I32Ne);
    f.op(Instruction::If(BlockType::Empty));
    f.i32(0);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.op(Instruction::Block(BlockType::Empty));
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(k);
    f.get(1);
    f.op(Instruction::I32GeU);
    f.op(Instruction::BrIf(1));
    f.get(0);
    f.get(k);
    f.op(Instruction::I32Add);
    f.op(Instruction::I32Load8U(mem(HEAP, 0, 0)));
    f.get(2);
    f.get(k);
    f.op(Instruction::I32Add);
    f.op(Instruction::I32Load8U(mem(HEAP, 0, 0)));
    f.op(Instruction::I32Ne);
    f.op(Instruction::If(BlockType::Empty));
    f.i32(0);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.get(k);
    f.i32(1);
    f.op(Instruction::I32Add);
    f.set(k);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
    f.i32(1);
    f
}
