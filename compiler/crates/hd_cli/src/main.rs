#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::process::ExitCode;

use hd_syntax::parse;

fn main() -> ExitCode {
    let mut arguments = env::args_os();
    let _program = arguments.next();
    let Some(command) = arguments.next() else {
        eprintln!("usage: hd parse FILE");
        return ExitCode::from(2);
    };
    let Some(file) = arguments.next() else {
        eprintln!("usage: hd parse FILE");
        return ExitCode::from(2);
    };
    if command != "parse" || arguments.next().is_some() {
        eprintln!("usage: hd parse FILE");
        return ExitCode::from(2);
    }
    let source = match fs::read(&file) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("{}: {error}", file.to_string_lossy());
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
                file.to_string_lossy(),
                diagnostic.primary.lo,
                diagnostic.primary.hi,
                diagnostic.code.as_str()
            );
        }
        ExitCode::FAILURE
    }
}
