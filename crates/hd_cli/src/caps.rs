//! Capability grants as `hd` applies them before a program starts: the
//! package's `[capabilities]` table and the command's `--cap` flags
//! (Capability Grants, Grant Precedence), and the startup refusal of a
//! totally denied need (Total Deny, `cli.cap.total.*`).

use std::ffi::OsString;

use std::path::Path;

use hd_diag::Code;
use hd_project::{Grant, SourceSet as _, grant_problem};
use hd_run::{GrantValue, Grants};

use crate::report::{Diag, Edit, Fix};

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
    // `cli.cap.flag.unknown.with-package-host`, `cli.cap.flag.unscoped`.
    if let Some(why) = grant_problem(key, &grant) {
        return Err(format!("`--cap {value}`: {why}"));
    }
    Ok(CapFlag {
        key: key.to_owned(),
        grant,
        text: format!("--cap {value}"),
    })
}

/// Combine the root's production and development tables before applying flags.
/// Denial is sticky; unrestricted grants subsume scopes; scoped grants unite.
pub(crate) fn development_grants(
    base: &[(String, Grant)],
    dev: &[(String, Grant)],
) -> Vec<(String, Grant)> {
    let mut merged = std::collections::BTreeMap::<String, Grant>::new();
    for (key, grant) in base.iter().chain(dev) {
        merged
            .entry(key.clone())
            .and_modify(|old| {
                *old = match (&*old, grant) {
                    (Grant::Deny, _) | (_, Grant::Deny) => Grant::Deny,
                    (Grant::All, _) | (_, Grant::All) => Grant::All,
                    (Grant::Scopes(a), Grant::Scopes(b)) => {
                        let mut scopes = a.clone();
                        for scope in b {
                            if !scopes.contains(scope) {
                                scopes.push(scope.clone());
                            }
                        }
                        Grant::Scopes(scopes)
                    }
                };
            })
            .or_insert_with(|| grant.clone());
    }
    merged.into_iter().collect()
}

/// Check the actual root entry rows, including unused written requirements.
pub(crate) fn required(program: &crate::disk::Program, out: &hd_driver::Output) -> Vec<Diag> {
    if program.single_file || program.package_dir.is_none() || out.entry_capabilities.is_empty() {
        return Vec::new();
    }
    let Some(bytes) = program.sources.read("hd.toml") else {
        return Vec::new();
    };
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return Vec::new();
    };
    let manifest = match hd_project::parse_manifest(text) {
        Ok(m) => m,
        Err(error) => {
            return vec![
                Diag::error(Some(Code::InvalidManifest), &error.message)
                    .at("hd.toml", error.at.map(|p| p.0 as usize)),
            ];
        }
    };
    let listed = |table: &[(String, Option<Grant>, u32)], selectors: &[String]| {
        table
            .iter()
            .any(|(key, grant, _)| grant.is_some() && selectors.contains(key))
    };
    let missing: Vec<_> = out
        .entry_capabilities
        .iter()
        .filter(|need| {
            !listed(&manifest.capabilities, &need.selectors)
                && (need.entry == hd_driver::CapabilityEntry::Main
                    || !listed(&manifest.dev_capabilities, &need.selectors))
        })
        .collect();
    let production: Vec<_> = missing
        .iter()
        .filter(|n| n.entry == hd_driver::CapabilityEntry::Main)
        .map(|n| n.selectors[0].clone())
        .collect();
    let development: Vec<_> = missing
        .iter()
        .filter(|need| need.entry != hd_driver::CapabilityEntry::Main)
        .map(|need| need.selectors[0].clone())
        .collect();
    let fix = |keys: &[String], development| {
        hd_project::capability_insertion_for(text, keys, development).map(|(at, text)| Fix {
            message: "list the missing capabilities in hd.toml".to_owned(),
            exact: true,
            edits: vec![Edit {
                file: "hd.toml".to_owned(),
                start: at,
                end: at,
                text,
            }],
        })
    };
    // Keep main fixes stable across the library/test checks. When both tables
    // insert at EOF, hd check --fix applies the overlapping development fix next round.
    let production_fix = fix(&production, false);
    let development_fix = fix(&development, true);
    missing
        .into_iter()
        .map(|need| {
            let who = match &need.entry {
                hd_driver::CapabilityEntry::Main => "`main`".to_owned(),
                hd_driver::CapabilityEntry::Task => "the task".to_owned(),
                hd_driver::CapabilityEntry::Test(name) => format!("test `{name}`"),
            };
            let row = if need.written_row {
                let row = out.texts[need.span.file.idx()]
                    .get(need.span.lo as usize..need.span.hi as usize)
                    .unwrap_or("")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                format!(" in `{row}`")
            } else {
                String::new()
            };
            let tables = if need.entry == hd_driver::CapabilityEntry::Main {
                "`[capabilities]` in hd.toml does not list it"
            } else {
                "neither `[capabilities]` nor `[dev-capabilities]` in hd.toml lists it"
            };
            let mut diag = Diag::error(
                Some(Code::MissingCapability),
                &format!("{who} requires `{}`{row}, but {tables}", need.selectors[0]),
            );
            let (file, line, column) = out.locate(need.span);
            let (_, end_line, end_column) = out.locate(hd_base::Span {
                lo: need.span.hi,
                ..need.span
            });
            diag.file = Some(file);
            diag.line = Some(line);
            diag.column = Some(column);
            diag.end_line = Some(end_line);
            diag.end_column = Some(end_column);
            diag.fixes
                .extend(if need.entry == hd_driver::CapabilityEntry::Main {
                    production_fix.clone()
                } else {
                    development_fix.clone()
                });
            diag
        })
        .collect()
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
/// test grant (`cli.cap.total.test`); `file` names the test program.
pub(crate) fn test_refusal(
    wasm: &[u8],
    table: &[(String, Grant)],
    flags: &[CapFlag],
    file: &str,
) -> Option<Diag> {
    denied(
        wasm,
        table,
        "`[capabilities]` or `[dev-capabilities]` of hd.toml",
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
    let mut needs: Vec<String> = hd_run::needs(&imports)
        .into_iter()
        .map(str::to_owned)
        .collect();
    match hd_driver::host_artifacts(wasm) {
        Ok(Some(a)) => needs.extend(
            a.contract
                .interfaces
                .into_iter()
                .filter(|i| !i.handle)
                .flat_map(|i| i.selectors),
        ),
        Ok(None) => {}
        Err(e) => return Some(Diag::error(None, &format!("the built host contract: {e}"))),
    }
    needs.into_iter().find_map(|need| {
        let setting = table
            .iter()
            .find(|(k, g)| k == &need && *g == Grant::Deny)
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

/// A test case's grant (`cli.test.env.grant.combined`): the package's
/// production and development tables and the flags of `hd test` by Grant
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

#[cfg(test)]
mod development_tests {
    use super::*;

    #[test]
    fn development_grants_preserve_denials_and_unite_scopes() {
        let scopes = |xs: &[&str]| Grant::Scopes(xs.iter().map(|s| (*s).to_owned()).collect());
        let base = vec![
            ("Http".into(), scopes(&["base", "both"])),
            ("Process".into(), Grant::Deny),
            ("FsRead".into(), Grant::All),
        ];
        let dev = vec![
            ("Http".into(), scopes(&["both", "dev"])),
            ("Process".into(), Grant::All),
            ("FsRead".into(), scopes(&["dev"])),
            ("Console".into(), Grant::All),
        ];
        let combined = development_grants(&base, &dev);
        assert!(combined.contains(&("Http".into(), scopes(&["base", "both", "dev"]))));
        assert!(combined.contains(&("Process".into(), Grant::Deny)));
        assert!(combined.contains(&("FsRead".into(), Grant::All)));
        assert!(combined.contains(&("Console".into(), Grant::All)));
        assert_eq!(
            development_grants(
                &[("Console".into(), Grant::All)],
                &[("Console".into(), Grant::Deny)]
            ),
            vec![("Console".into(), Grant::Deny)]
        );
    }
}
