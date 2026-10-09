//! Test bodies (spec/lang/10-modules.md "Test Cases", "Test Outcomes";
//! spec/lang/05-expressions.md "Propagation In Test Blocks"): `hd test`
//! runs each form of an `it` body and reports its outcome, and checking
//! holds each form to its rules.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A fresh package `notes` whose `src/lib.hd` is `lib`.
fn package(name: &str, lib: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(root.with_extension("cache"));
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::write(root.join("hd.toml"), "[package]\nname = \"notes\"\n").expect("hd.toml");
    std::fs::write(root.join("src/lib.hd"), lib).expect("lib");
    root
}

fn hd(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hd"))
        .env("HD_CACHE", root.with_extension("cache"))
        .env_remove("HD_JOBS")
        .current_dir(root)
        .args(args)
        .output()
        .expect("run hd")
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// Errors with a cause, which a `?` in a test body propagates.
const ERRORS: &str = "\
@error(\"disk full\")
pub data DiskError:
    free: i64

@error
pub enum SaveError:
    @error(\"cannot save $path\")
    Write(path: string, @source error: DiskError)

pub fn save(free: i64) -> Result[void, SaveError]:
    if free > 0: .Ok(()) else: .Err(SaveError.Write(\"notes.txt\", DiskError { free: free }))
";

/// `expr.try.test.with-try`, `module.testing.err-print`: a body that uses
/// `?` returns `Result[void, dyn Error]`. `.Ok(())` passes; an `.Err` fails,
/// and the runner prints the error and its cause chain in its own report on
/// standard output, for an `it` body and an `it_each` row alike.
#[test]
fn a_try_body_passes_on_ok_and_reports_an_err_with_its_causes() {
    let lib = format!(
        "{ERRORS}
tests:
    use std.testing.it_each

    it(\"saves with room\"):
        save(10)?
        .Ok(())

    it(\"saves on a full disk\"):
        save(0)?
        .Ok(())

    it_each(\"saves each\", [5, 0], body=fn!(free: i64):
        save(free)?
        .Ok(())
    )
"
    );
    let root = package("test-bodies-try", &lib);
    let out = hd(&root, &["test"]);
    assert_eq!(
        text(&out.stdout),
        "\
FAIL src/lib.hd:20: saves on a full disk
    cannot save notes.txt
    caused by: disk full
    repro: hd test src/lib.hd --filter \"saves on a full disk\"
FAIL src/lib.hd:24: saves each[1]
    cannot save notes.txt
    caused by: disk full
    repro: hd test src/lib.hd --filter \"saves each[1]\"
test result: FAILED. 2 passed; 2 failed; 0 ignored
"
    );
    assert_eq!(text(&out.stderr), "");
    assert_eq!(out.status.code(), Some(1));
    let out = hd(
        &root,
        &["test", "--format", "json", "--filter", "full disk"],
    );
    assert_eq!(
        text(&out.stdout),
        "\
{\"kind\":\"test\",\"name\":\"saves on a full disk\",\"outcome\":\"failed\",\"message\":\"cannot save notes.txt\\ncaused by: disk full\"}
{\"kind\":\"summary\",\"errors\":0,\"warnings\":0,\"passed\":0,\"failed\":1,\"ignored\":0,\"status\":1}
"
    );
    assert_eq!(text(&out.stderr), "");
}
