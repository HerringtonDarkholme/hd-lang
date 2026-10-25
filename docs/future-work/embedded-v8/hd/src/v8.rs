#![forbid(unsafe_code)]
use std::io::Write as _;
use std::sync::OnceLock;
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();

fn write(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _rv: v8::ReturnValue) {
    let text = args.get(0).to_rust_string_lossy(scope);
    std::io::stdout()
        .write_all(text.as_bytes())
        .expect("stdout");
}

fn encode(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let text = args.get(0).to_rust_string_lossy(scope);
    let store = v8::ArrayBuffer::new_backing_store_from_vec(text.into_bytes()).make_shared();
    let buffer = v8::ArrayBuffer::with_backing_store(scope, &store);
    let value = v8::Uint8Array::new(scope, buffer, 0, store.byte_length()).expect("encoded bytes");
    rv.set(value.into());
}
fn decode(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let bytes = v8::Local::<v8::Uint8Array>::try_from(args.get(0)).expect("decode Uint8Array");
    let mut output = vec![0; bytes.byte_length()];
    bytes.copy_contents(&mut output);
    let text = String::from_utf8_lossy(&output);
    rv.set(v8::String::new(scope, &text).expect("decoded text").into());
}
fn now(_scope: &mut v8::PinScope, _args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    rv.set_double(START.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0);
}

fn random(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let array = v8::Local::<v8::ArrayBufferView>::try_from(args.get(0)).expect("random array view");
    assert!(array.byte_length() <= 65536, "random request quota");
    let mut bytes = vec![0; array.byte_length()];
    getrandom::fill(&mut bytes).expect("system random");
    let buffer = array.buffer(scope).expect("random buffer");
    let store = buffer.get_backing_store();
    for (cell, byte) in store[array.byte_offset()..array.byte_offset() + array.byte_length()]
        .iter()
        .zip(bytes)
    {
        cell.set(byte);
    }
    rv.set(args.get(0));
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let bytes = std::fs::read(args.get(1).expect("Wasm path")).expect("read Wasm");
    let source = args.get(2).map_or_else(
        || String::from("const m = new WebAssembly.Module(WASM); const i = new WebAssembly.Instance(m, {}); i.exports.run ? String(i.exports.run()) : 'ok'"),
        |path| std::fs::read_to_string(path).expect("script"),
    );
    let platform = v8::new_default_platform(0, false).make_shared();
    v8::V8::initialize_platform(platform);
    v8::V8::initialize();
    let isolate = &mut v8::Isolate::new(v8::CreateParams::default());
    v8::scope!(let handle_scope, isolate);
    let context = v8::Context::new(handle_scope, Default::default());
    let scope = &mut v8::ContextScope::new(handle_scope, context);
    let store = v8::ArrayBuffer::new_backing_store_from_vec(bytes).make_shared();
    let buffer = v8::ArrayBuffer::with_backing_store(scope, &store);
    let bytes = v8::Uint8Array::new(scope, buffer, 0, store.byte_length()).expect("bytes");
    let key = v8::String::new(scope, "WASM").expect("key");
    context
        .global(scope)
        .set(scope, key.into(), bytes.into())
        .expect("set Wasm");
    let output = v8::Function::new(scope, write).expect("write function");
    let key = v8::String::new(scope, "__write").expect("key");
    context
        .global(scope)
        .set(scope, key.into(), output.into())
        .expect("set writer");
    let function = v8::Function::new(scope, encode).expect("native callback");
    let key = v8::String::new(scope, "__encode").expect("key");
    context
        .global(scope)
        .set(scope, key.into(), function.into())
        .expect("set callback");
    let function = v8::Function::new(scope, decode).expect("native callback");
    let key = v8::String::new(scope, "__decode").expect("key");
    context
        .global(scope)
        .set(scope, key.into(), function.into())
        .expect("set callback");
    let function = v8::Function::new(scope, now).expect("native callback");
    let key = v8::String::new(scope, "__now").expect("key");
    context
        .global(scope)
        .set(scope, key.into(), function.into())
        .expect("set callback");
    let function = v8::Function::new(scope, random).expect("random callback");
    let key = v8::String::new(scope, "__random").expect("key");
    context
        .global(scope)
        .set(scope, key.into(), function.into())
        .expect("set random");
    v8::tc_scope!(let scope, scope);
    let source = v8::String::new(scope, &source).expect("source");
    let result = v8::Script::compile(scope, source, None).and_then(|script| script.run(scope));
    match result {
        Some(value) => {
            scope.perform_microtask_checkpoint();
            let control = v8::String::new(scope, "if (typeof __probeDone === 'boolean' && (!__probeDone || __probeError)) throw Error(__probeError || 'genuinely pending I/O is outside this CPU probe');").expect("completion check");
            if v8::Script::compile(scope, control, None)
                .and_then(|script| script.run(scope))
                .is_none()
            {
                let error = scope
                    .exception()
                    .expect("exception")
                    .to_rust_string_lossy(scope);
                eprintln!("{error}");
                std::process::exit(1);
            }
            let text = value.to_rust_string_lossy(scope);
            if !text.is_empty() {
                println!("{text}");
            }
        }
        None => {
            let error = scope
                .exception()
                .expect("exception")
                .to_rust_string_lossy(scope);
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
