//! `hd build --engine wasmtime` writes the Rust companion `NAME.host.rs`
//! beside `NAME.wasm` (`cli.host.package.native-companion`): the same text
//! on every build (`native-deterministic`), and a `.wasm` byte-identical to
//! the Node build's (`native-wasm-identity`). A build that needs no
//! companion leaves none behind.

use std::path::Path;

use hd_testkit::Scratch;

const MANIFEST: &str = "[package]\nname = \"probe\"\n\n[host]\n\"main.Server\" = \"provider.ts\"\n\n[capabilities]\nConsole = true\n\"pkg.main.Server\" = true\n";
const MAIN: &str = "@host\npub trait Server:\n    fn read(&self) -> i32\n\npub fn main() -> void $ Console + Server:\n    println(\"${$.use(Server).read()}\")\n";
const PLAIN: &str = "pub fn main() -> void $ Console:\n    println(\"plain\")\n";

/// `hd build ARGS` in `root`, with its own cache.
fn build(root: &Path, args: &[&str]) {
    let out = hd_testkit::node::child(env!("CARGO_BIN_EXE_hd"))
        .env(
            "HD_CACHE",
            crate::shared_objects(root.with_extension("cache")),
        )
        .env_remove("HD_JOBS")
        .env_remove("HD_ENGINE")
        .env_remove("HD_WAT")
        .current_dir(root)
        .arg("build")
        .args(args)
        .output()
        .expect("run hd");
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn package(name: &str, manifest: &str, main: &str) -> Scratch {
    let root = Scratch::new(env!("CARGO_TARGET_TMPDIR"), name);
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::write(root.join("hd.toml"), manifest).expect("hd.toml");
    std::fs::write(
        root.join("provider.ts"),
        "export default { read() { return 7; } };\n",
    )
    .expect("provider");
    std::fs::write(root.join("src/main.hd"), main).expect("main");
    root
}

/// Every output form: the package's executable and `hd build FILE`, in the
/// dev and release profiles.
const FORMS: [(&[&str], &str, &str); 4] = [
    (&[], "debug", "probe"),
    (&["--release"], "release", "probe"),
    (&["src/main.hd"], "debug/files", "main"),
    (&["--release", "src/main.hd"], "release/files", "main"),
];

#[test]
fn a_wasmtime_build_writes_the_same_companion_and_the_node_module() {
    let root = package("native-companion", MANIFEST, MAIN);
    for (args, dir, stem) in FORMS {
        let out = root.join("build").join(dir);
        let rs = out.join(format!("{stem}.host.rs"));
        let wasm = out.join(format!("{stem}.wasm"));
        let wasmtime = [&["--engine", "wasmtime"][..], args].concat();
        build(&root, &wasmtime);
        let first = std::fs::read(&rs).expect("the Rust companion");
        let module = std::fs::read(&wasm).expect("wasm");
        assert!(out.join(format!("{stem}.host.mjs")).exists(), "{dir}");
        let text = String::from_utf8(first.clone()).expect("utf-8");
        assert!(text.contains("pub trait Server {"), "{dir}:\n{text}");
        assert!(text.contains("pub fn server(mut self"), "{dir}:\n{text}");

        build(&root, &wasmtime);
        assert_eq!(
            std::fs::read(&rs).expect("rs"),
            first,
            "{dir}: deterministic"
        );

        build(&root, &[&["--engine", "node"][..], args].concat());
        assert_eq!(
            std::fs::read(&wasm).expect("wasm"),
            module,
            "{dir}: same module"
        );
        assert!(!rs.exists(), "{dir}: a Node build leaves no Rust companion");
    }
}

#[test]
fn a_module_without_package_host_traits_gets_no_companion() {
    let root = package(
        "native-companion-plain",
        "[package]\nname = \"probe\"\n\n[capabilities]\nConsole = true\n",
        PLAIN,
    );
    for (args, dir, stem) in FORMS {
        build(&root, &[&["--engine", "wasmtime"][..], args].concat());
        let out = root.join("build").join(dir);
        assert!(out.join(format!("{stem}.wasm")).exists(), "{dir}");
        assert!(!out.join(format!("{stem}.host.rs")).exists(), "{dir}");
        assert!(!out.join(format!("{stem}.host.mjs")).exists(), "{dir}");
    }
}

/// The companion of `MAIN` for a fixed identity: a local trait's identity
/// hashes its package directory, so the one a scratch build writes is this
/// text with that identity. `HD_BLESS=1` rewrites it.
#[allow(dead_code, clippy::all, clippy::pedantic)]
mod probe {
    include!("native/probe.host.rs");
}

/// A `Server` provider answering 7.
struct Seven;

impl probe::Server for Seven {
    fn read(&mut self) -> hd_run_wasmtime::native::HostResult<i32> {
        Ok(7)
    }
}

/// Standard output, kept for the test to read.
#[derive(Clone, Default)]
struct Captured(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("output").extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

#[test]
fn the_companion_of_a_wasmtime_build_answers_its_guest() {
    use hd_run::{Engine as _, Grants, HostSetup, Instance as _, Limits, Outcome};
    use hd_run_wasmtime::{EngineConfig, HostIo, WasmtimeEngine, native};

    let root = package("native-companion-run", MANIFEST, MAIN);
    build(&root, &["--engine", "wasmtime"]);
    let out = root.join("build/debug");
    let wasm = std::fs::read(out.join("probe.wasm")).expect("wasm");
    let written = std::fs::read_to_string(out.join("probe.host.rs")).expect("companion");
    let contract = hd_host_abi::schema::from_wasm(&wasm)
        .expect("a valid contract")
        .expect("a package host contract");
    assert_eq!(
        written,
        hd_host_abi::native_rust::generate(&contract).unwrap()
    );
    let [server] = &contract.interfaces[..] else {
        panic!("one host trait: {contract:?}")
    };
    // The same companion, generated for a fixed identity, is the one
    // compiled here.
    let mut fixed = contract.clone();
    fixed.interfaces[0].identity.package = "example.org/probe".into();
    fixed.interfaces[0].identity.version = "1.0.0".into();
    fixed.interfaces[0].module = "hd:pkg:server".into();
    let text = hd_host_abi::native_rust::generate(&fixed).unwrap();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/all/native/probe.host.rs");
    if std::env::var_os("HD_BLESS").is_some() {
        std::fs::write(&path, &text).expect("bless");
    }
    assert_eq!(
        std::fs::read_to_string(&path).expect("snapshot"),
        text,
        "{} is stale; rerun with HD_BLESS=1",
        path.display()
    );
    assert_eq!(
        Some(server.identity.to_string().as_str()),
        written
            .lines()
            .find_map(|l| l.trim().strip_prefix("bindings.register_dispatch(\""))
            .and_then(|l| l.split('"').next())
    );

    // The compiled companion's dispatch, registered as the written one
    // registers it: under the built module's identity and module.
    let mut bindings = native::Bindings::new(Box::leak(contract.encode().into_boxed_slice()));
    let provider: Box<dyn probe::Server> = Box::new(Seven);
    bindings.register_dispatch(
        leak(server.identity.to_string()),
        leak(server.module.clone()),
        Box::new(provider),
        probe::dispatch_Server,
    );
    let engine = WasmtimeEngine::new(EngineConfig::default()).expect("engine");
    let module = engine
        .load(&wasm, &hd_cache::MemoryStore::default())
        .expect("the module loads");
    let stdout = Captured::default();
    let io = HostIo {
        out: Box::new(stdout.clone()),
        err: Box::new(std::io::sink()),
        input: None,
    };
    let setup = HostSetup {
        providers: vec![],
        grants: Grants::default(),
        limits: Limits::default(),
        args: vec![],
        program: "probe".into(),
    };
    let mut instance = engine
        .instantiate_with_bindings(&module, &setup, io, bindings)
        .expect("the registered provider loads");
    assert_eq!(instance.init(), Outcome::Exit(0));
    // The program waits on nothing: its first poll finishes it.
    assert_eq!(instance.poll(), std::task::Poll::Ready(Outcome::Exit(0)));
    drop(instance);
    assert_eq!(
        String::from_utf8(stdout.0.lock().expect("output").clone()).expect("utf-8"),
        "7\n"
    );
}
