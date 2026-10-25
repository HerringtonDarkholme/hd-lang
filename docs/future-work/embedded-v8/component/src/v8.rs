#![forbid(unsafe_code)]
use std::io::Write as _;

fn write(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _rv: v8::ReturnValue) {
    let text = args.get(0).to_rust_string_lossy(scope);
    std::io::stdout()
        .write_all(text.as_bytes())
        .expect("stdout");
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
    v8::tc_scope!(let scope, scope);
    let source = v8::String::new(scope, &source).expect("source");
    let result = v8::Script::compile(scope, source, None).and_then(|script| script.run(scope));
    match result {
        Some(value) => {
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
