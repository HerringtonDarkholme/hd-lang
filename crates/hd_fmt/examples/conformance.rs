//! Repository maintenance only: format indexed conformance fixtures while
//! preserving lexical/layout inputs. No `hd fmt FILE` behavior changes.
//! One manifest pass, one read per eligible file, one formatter invocation
//! per parseable file, and writes only for changed eligible files.
use hd_base::HashSet;
use std::path::{Path, PathBuf};

#[path = "conformance/policy.rs"]
mod policy;
use policy::{exemptions, protected};

fn run(suite: &Path, write: bool) -> Result<bool, String> {
    let exemptions = exemptions(suite).map_err(|e| e.to_string())?;
    let index = std::fs::read_to_string(suite.join("cases.tsv")).map_err(|e| e.to_string())?;
    let rows: Vec<_> = index
        .lines()
        .skip(1)
        .map(|line| {
            let fields: Vec<_> = line.split('\t').collect();
            let [path, _, _, specification] = fields.as_slice() else {
                return Err(format!("invalid cases.tsv row: {line}"));
            };
            Ok((*path, *specification))
        })
        .collect::<Result<_, _>>()?;
    let indexed: HashSet<_> = rows.iter().map(|(path, _)| *path).collect();
    if let Some(path) = exemptions
        .iter()
        .find(|path| !indexed.contains(path.as_str()))
    {
        return Err(format!("format exemption is not an indexed case: {path}"));
    }
    let mut protected_count = 0;
    let mut changed = 0;
    let mut formatted = 0;
    for (path, specification) in rows {
        if protected(specification, path, &exemptions) {
            protected_count += 1;
            continue;
        }
        let file = suite.join(path);
        let text = std::fs::read_to_string(&file).map_err(|e| format!("{path}: {e}"))?;
        // Fixtures that do not parse remain byte-for-byte inputs, as with hd fmt.
        let Ok(output) = hd_fmt::format(&text) else {
            continue;
        };
        formatted += 1;
        if output != text {
            changed += 1;
            if write {
                std::fs::write(&file, output).map_err(|e| format!("{path}: {e}"))?;
            }
        }
    }
    println!(
        "conformance format: {protected_count} protected, {formatted} formatted, {changed} changed"
    );
    Ok(changed != 0)
}

fn main() -> std::process::ExitCode {
    let mut suite = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/conformance");
    let mut write = false;
    for arg in std::env::args().skip(1) {
        if arg == "--write" {
            write = true;
        } else if arg.starts_with('-') {
            eprintln!("usage: cargo run -p hd_fmt --example conformance -- [--write] [SUITE]");
            return std::process::ExitCode::from(2);
        } else {
            suite = PathBuf::from(arg);
        }
    }
    match run(&suite, write) {
        Ok(changed) => std::process::ExitCode::from(u8::from(changed && !write)),
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn protected_bytes_survive_an_actual_suite_format() {
        let dir = std::env::temp_dir().join(format!("hd-fmt-conformance-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch suite");
        let fixtures = [
            ("bom.hd", "lang/01-lexical.md", "\u{feff}x := 1\n"),
            ("crlf.hd", "lang/02-grammar.md", "fn f() -> void: pass\r\n"),
            ("parens.hd", "lang/05-expressions.md", "x := (1)\n"),
            (
                "regular.hd",
                "lang/07-functions.md",
                "fn f() -> void: pass\n",
            ),
        ];
        let mut index = "path\tphase\texpectation\tspecification\n".to_owned();
        for (path, spec, source) in fixtures {
            std::fs::write(dir.join(path), source).expect("fixture");
            index.push_str(&format!("{path}\tparse\taccept\t{spec}\n"));
        }
        std::fs::write(dir.join("cases.tsv"), index).expect("index");
        std::fs::write(
            dir.join("format-exempt.tsv"),
            "path\treason\nparens.hd\tparentheses\n",
        )
        .expect("exemptions");
        assert!(super::run(&dir, true).expect("suite formats"));
        for (path, _, source) in fixtures.into_iter().take(3) {
            assert_eq!(
                std::fs::read(dir.join(path)).expect("protected bytes"),
                source.as_bytes()
            );
        }
        assert_eq!(
            std::fs::read_to_string(dir.join("regular.hd")).expect("formatted control"),
            "fn f() -> void:\n    pass\n"
        );
        assert!(!super::run(&dir, false).expect("second run is stable"));
        std::fs::remove_dir_all(dir).expect("remove scratch suite");
    }
}
