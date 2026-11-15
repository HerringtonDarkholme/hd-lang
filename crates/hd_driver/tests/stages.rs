//! Plain hd stage cases and separate generated baselines.
//! `HD_BLESS=1` updates only `tests/baselines/`.

use hd_testkit::stages::{Build, Program, node_host, program_wasm, shown_run, stdout};
use std::time::Duration;

#[test]
fn stage_fixtures_match() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cases");
    hd_testkit::stages::run_dir(&dir, &hd_testkit::stages::view);
    // Every `stdout` case of the target shares one Node host; another
    // starts only after one crashed or overran.
    let (starts, crashes) = node_host::starts();
    assert!(
        (1..=1 + crashes).contains(&starts),
        "{starts} Node hosts started after {crashes} crashes"
    );
}

/// The `stdout` view's text of a lone `node host/run.ts` run of `wasm`.
fn lone_run(wasm: &[u8], name: &str) -> String {
    let path =
        std::env::temp_dir().join(format!("hd-stage-lone-{}-{name}.wasm", std::process::id()));
    std::fs::write(&path, wasm).expect("write the program");
    let ran = std::process::Command::new("node")
        .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.ts"))
        .arg(&path)
        .output()
        .expect("node");
    let _ = std::fs::remove_file(&path);
    let exit = ran
        .status
        .code()
        .map_or_else(|| "signal".to_owned(), |c| c.to_string());
    shown_run(&exit, &ran.stdout, &ran.stderr)
}

const PANICS: &str =
    "pub fn main() -> void $ Console:\n    println(\"before\")\n    panic(\"stopped here\")\n";
const PRINTS: &str = "use std.console.eprintln\n\npub fn main() -> void $ Console:\n    println(\"one\")\n    eprintln(\"two\")\n";
const GLOBAL: &str = "let mut xs = [1, 2]\n\npub fn main() -> void $ Console:\n    xs.push(3)\n    println(xs.len())\n";

#[test]
fn the_shared_host_answers_as_a_lone_run_and_keeps_no_state_between_runs() {
    let expected = [
        "exit: 3\n-- stdout\nbefore\n-- stderr\npanic: explicit-panic: stopped here\n",
        "exit: 0\n-- stdout\none\n-- stderr\ntwo\n",
        // The second run of the same program sees its own fresh global.
        "exit: 0\n-- stdout\n3\n",
        "exit: 0\n-- stdout\n3\n",
    ];
    for (k, (text, expected)) in [PANICS, PRINTS, GLOBAL, GLOBAL]
        .iter()
        .zip(expected)
        .enumerate()
    {
        let p = Program::new(text).expect("program");
        let wasm = program_wasm(&p, Build::default()).expect("builds");
        let shared = stdout(&p, Build::default());
        assert_eq!(shared, expected, "run {k}");
        assert_eq!(shared, lone_run(&wasm, &k.to_string()), "run {k}");
    }
}

#[test]
fn a_run_that_overruns_fails_alone_and_the_next_gets_a_new_host() {
    let spin =
        Program::new("pub fn main() -> void $ Console:\n    while true:\n        println(\"\")\n")
            .expect("program");
    let failed = node_host::run(
        &program_wasm(&spin, Build::default()).expect("builds"),
        &[],
        Duration::from_millis(500),
    );
    assert!(
        failed.as_ref().is_err_and(|e| e.contains("did not end")),
        "{failed:?}"
    );
    let prints = Program::new(PRINTS).expect("program");
    assert_eq!(
        stdout(&prints, Build::default()),
        "exit: 0\n-- stdout\none\n-- stderr\ntwo\n"
    );
}
