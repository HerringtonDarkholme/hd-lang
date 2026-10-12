//! Capability grants as `hd` applies them before a program starts: the
//! package's `[capabilities]` table and the command's `--cap` flags
//! (Capability Grants, Grant Precedence), and the startup refusal of a
//! totally denied need (Total Deny, `cli.cap.total.*`).

use std::ffi::OsString;

use std::path::Path;

use hd_diag::Code;
use hd_project::{Grant, grant_problem};
use hd_run::{GrantValue, Grants};

use crate::report::Diag;

/// One `--cap NAME=VALUE` flag (`cli.cap.flag`), as written and parsed.
pub(crate) struct CapFlag {
    pub(crate) key: String,
    pub(crate) grant: Grant,
    pub(crate) text: String,
}

/// Reads `--cap NAME=VALUE` or `--cap=NAME=VALUE` at `args[i]`: the flag and
/// how many words it took, or `None` when `args[i]` is another word.
pub(crate) fn cap_flag(args: &[OsString], i: usize) -> Option<Result<(CapFlag, usize), String>> {
    let text = args.get(i)?.to_string_lossy();
    let (value, used) = if text == "--cap" {
        match args.get(i + 1) {
            Some(v) => (v.to_string_lossy().into_owned(), 2),
            None => return Some(Err("`--cap` needs NAME=VALUE".to_owned())),
        }
    } else {
        (text.strip_prefix("--cap=")?.to_owned(), 1)
    };
    Some(parse_flag(&value).map(|f| (f, used)))
}

/// `NAME=VALUE`, where VALUE is `true`, `false`, or scope entries separated
/// by commas (`cli.cap.flag.value`).
fn parse_flag(value: &str) -> Result<CapFlag, String> {
    let Some((key, v)) = value.split_once('=') else {
        return Err(format!("`--cap {value}` is not NAME=VALUE"));
    };
    let grant = match v {
        "true" => Grant::All,
        "false" => Grant::Deny,
        _ => Grant::Scopes(
            v.split(',')
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect(),
        ),
    };
    // `cli.cap.flag.unknown`, `cli.cap.flag.unscoped`.
    if let Some(why) = grant_problem(key, &grant) {
        return Err(format!("`--cap {value}`: {why}"));
    }
    Ok(CapFlag {
        key: key.to_owned(),
        grant,
        text: format!("--cap {value}"),
    })
}

/// The refusal of a module whose needs include a totally denied trait
/// (`cli.cap.total.refuse`): a `denied-capability` diagnostic with no
/// position that names the trait and the setting that denied it, the
/// `hd.toml` key or the `--cap` flag (`cli.cap.total.message`). A deny
/// always wins (`cli.cap.order.deny`).
pub(crate) fn refusal(wasm: &[u8], table: &[(String, Grant)], flags: &[CapFlag]) -> Option<Diag> {
    denied(wasm, table, "hd.toml", flags, "the program")
}

/// The same refusal of an integration test program or a doc test under the
/// test grant (`cli.cap.total.test`), whose table is `[test.capabilities]`
/// (`cli.test.env.grant.table`); `file` names the test program.
pub(crate) fn test_refusal(
    wasm: &[u8],
    table: &[(String, Grant)],
    flags: &[CapFlag],
    file: &str,
) -> Option<Diag> {
    denied(
        wasm,
        table,
        "`[test.capabilities]` of hd.toml",
        flags,
        &format!("the test program {file}"),
    )
}

fn denied(
    wasm: &[u8],
    table: &[(String, Grant)],
    table_name: &str,
    flags: &[CapFlag],
    who: &str,
) -> Option<Diag> {
    let imports = match hd_run::module_imports(wasm) {
        Ok(i) => i,
        Err(e) => return Some(Diag::error(None, &format!("the built module: {e}"))),
    };
    hd_run::needs(&imports).into_iter().find_map(|need| {
        let setting = table
            .iter()
            .find(|(k, g)| k == need && *g == Grant::Deny)
            .map(|(k, _)| format!("`{k} = false` in {table_name}"))
            .or_else(|| {
                flags
                    .iter()
                    .find(|f| f.key == need && f.grant == Grant::Deny)
                    .map(|f| format!("`{}`", f.text))
            })?;
        Some(Diag::error(
            Some(Code::DeniedCapability),
            &format!("{who} needs {need}, which {setting} denies, so it does not start"),
        ))
    })
}

/// The table's and the flags' values, as `Grants` reads them.
type Values = Vec<(String, GrantValue)>;

fn values(table: &[(String, Grant)], flags: &[CapFlag]) -> (Values, Values) {
    (
        table.iter().map(|(k, g)| (k.clone(), value(g))).collect(),
        flags
            .iter()
            .map(|f| (f.key.clone(), value(&f.grant)))
            .collect(),
    )
}

fn value(g: &Grant) -> GrantValue {
    match g {
        Grant::All => GrantValue::All,
        Grant::Deny => GrantValue::Deny,
        Grant::Scopes(list) => GrantValue::Scopes(list.clone()),
    }
}

/// The program's grant (Grant Precedence): the package's table, whose
/// path entries are relative to `table_dir`, and the flags, whose path
/// entries are relative to the working directory
/// (`cli.cap.scope.path.relative`).
pub(crate) fn grants(table: &[(String, Grant)], table_dir: &Path, flags: &[CapFlag]) -> Grants {
    let cwd = std::env::current_dir().unwrap_or_else(|_| table_dir.to_path_buf());
    let (table, flags) = values(table, flags);
    Grants::resolve(&table, table_dir, &flags, &cwd)
}

/// A test case's grant (`cli.test.env.grant`): the package's
/// `[test.capabilities]` table and the flags of `hd test` by Grant
/// Precedence, with `FsRead` covering the package directory and the case's
/// temporary directory and `FsWrite` that directory, as
/// `cli.test.env.grant.fs-read`, `.fs-write` and `.fs-added` say.
pub(crate) fn test_grants(
    table: &[(String, Grant)],
    package_dir: &Path,
    flags: &[CapFlag],
    temp_dir: &Path,
) -> Grants {
    let cwd = std::env::current_dir().unwrap_or_else(|_| package_dir.to_path_buf());
    let (table, flags) = values(table, flags);
    Grants::for_test(&table, package_dir, &flags, &cwd, temp_dir)
}
