use std::path::Path;
use std::process::Command;

fn repository_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repository root")
}

#[test]
fn parse_command_prints_tree_or_diagnostics() {
    let valid = repository_root().join("spec/conformance/parse/valid/declarations.hd");
    let output = Command::new(env!("CARGO_BIN_EXE_hd"))
        .args(["parse", valid.to_str().expect("UTF-8 path")])
        .output()
        .expect("run hd parse");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("FnDecl"));

    let invalid = repository_root().join("spec/conformance/parse/invalid/semicolon.hd");
    let output = Command::new(env!("CARGO_BIN_EXE_hd"))
        .args(["parse", invalid.to_str().expect("UTF-8 path")])
        .output()
        .expect("run hd parse");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("reserved-semicolon"));
}
