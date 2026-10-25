#![forbid(unsafe_code)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).expect("Wasm path");
    let bytes = std::fs::read(path)?;
    let engine = wasmtime::Engine::new(&wasmtime::Config::new())?;
    let module = wasmtime::Module::new(&engine, bytes)?;
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])?;
    if let Ok(run) = instance.get_typed_func::<(), i64>(&mut store, "run") {
        println!("{}", run.call(&mut store, ())?);
    } else {
        println!("ok");
    }
    Ok(())
}
