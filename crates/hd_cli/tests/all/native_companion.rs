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
