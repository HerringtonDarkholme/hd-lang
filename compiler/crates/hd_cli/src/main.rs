#![forbid(unsafe_code)]

use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use hd_driver::{MemStore, load_program, node, run};
use hd_syntax::parse;

const USAGE: &str = "usage:
  hd parse FILE
  hd run FILE.hd|DIR
  hd build FILE.hd|DIR -o OUT.wasm";

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let arguments: Vec<OsString> = env::args_os().skip(1).collect();
    let Some(command) = arguments.first().and_then(|c| c.to_str()) else {
        return usage();
    };
    match (command, &arguments[1..]) {
        ("parse", [file]) => parse_command(Path::new(file)),
        ("run", [target]) => run_command(Path::new(target)),
        ("build", [target, flag, out]) if flag == "-o" => build_command(Path::new(target), Path::new(out)),
        _ => usage(),
    }
}

fn parse_command(file: &Path) -> ExitCode {
    let source = match fs::read(file) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("{}: {error}", file.display());
            return ExitCode::from(2);
        }
    };
    let parsed = parse(&source);
    let text = String::from_utf8_lossy(&source);
    if parsed.diagnostics.is_empty() {
        print!("{}", parsed.tree.debug_tree(&parsed.tokens, &text));
        ExitCode::SUCCESS
    } else {
        for diagnostic in &parsed.diagnostics {
            eprintln!(
                "{}:{}..{}: {}",
                file.display(),
                diagnostic.primary.lo,
                diagnostic.primary.hi,
                diagnostic.code.as_str()
            );
        }
        ExitCode::FAILURE
    }
}

/// Compiles a file or a directory to Wasm bytes, printing diagnostics.
fn compile(target: &Path) -> Result<Vec<u8>, ExitCode> {
    let program = load_program(target).map_err(|error| {
        eprintln!("error: {error}");
        ExitCode::from(2)
    })?;
    let mut store = MemStore::default();
    let result = run(&mut store, &program.sources, &program.entry);
    if !result.diagnostics.is_empty() {
        for diagnostic in &result.diagnostics {
            eprintln!("error: {diagnostic}");
        }
        return Err(ExitCode::FAILURE);
    }
    result.wasm.ok_or_else(|| {
        eprintln!("error: no Wasm produced");
        ExitCode::FAILURE
    })
}

fn run_command(target: &Path) -> ExitCode {
    let wasm = match compile(target) {
        Ok(wasm) => wasm,
        Err(code) => return code,
    };
    let path: PathBuf = env::temp_dir().join(format!("hd-run-{}.wasm", std::process::id()));
    let output = node::run_wasm(&wasm, &path);
    let _ = fs::remove_file(&path);
    match output {
        Ok(output) => {
            let _ = std::io::stdout().write_all(&output.stdout);
            if output.status.success() {
                ExitCode::SUCCESS
            } else {
                let _ = std::io::stderr().write_all(&output.stderr);
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn build_command(target: &Path, out: &Path) -> ExitCode {
    let wasm = match compile(target) {
        Ok(wasm) => wasm,
        Err(code) => return code,
    };
    match fs::write(out, &wasm) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}: {error}", out.display());
            ExitCode::FAILURE
        }
    }
}
