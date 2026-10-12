//! `hd new [--app | --lib] [--pages] [--vcs none] [PATH]` (Creating A
//! Package, `cli.new.*`): the files of a new application or library, with
//! a passing integration test, a git repository unless asked not to, and
//! membership in an enclosing workspace.

use std::ffi::OsString;
use std::io::{BufRead as _, IsTerminal as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use crate::{disk, fail};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    App,
    Lib,
}

struct Options {
    kind: Option<Kind>,
    pages: bool,
    vcs: bool,
    path: Option<PathBuf>,
}

fn options(args: &[OsString]) -> Result<Options, String> {
    let mut o = Options {
        kind: None,
        pages: false,
        vcs: true,
        path: None,
    };
    let mut words = args.iter();
    while let Some(a) = words.next() {
        let text = a.to_string_lossy();
        let kind = match text.as_ref() {
            "--app" => Some(Kind::App),
            "--lib" => Some(Kind::Lib),
            _ => None,
        };
        if let Some(k) = kind {
            if o.kind.is_some_and(|had| had != k) {
                return Err("`hd new` takes `--app` or `--lib`, not both".to_owned());
            }
            o.kind = Some(k);
        } else if text == "--pages" {
            o.pages = true;
        } else if text == "--vcs" || text.starts_with("--vcs=") {
            let value = match text.strip_prefix("--vcs=") {
                Some(v) => v.to_owned(),
                None => words
                    .next()
                    .map(|v| v.to_string_lossy().into_owned())
                    .ok_or("`--vcs` needs a value: `none`")?,
            };
            if value != "none" {
                return Err(format!("`--vcs` takes `none`, not `{value}`"));
            }
            o.vcs = false;
        } else if text.starts_with('-') {
            return Err(format!("`hd new` has no option `{text}`"));
        } else if o.path.replace(PathBuf::from(a)).is_some() {
            return Err("`hd new` takes at most one PATH".to_owned());
        }
    }
    Ok(o)
}

/// Asks a terminal which kind to create, and whether to publish the
/// documentation (`cli.new.kind.ask`, `cli.new.pages.ask`).
fn ask(o: &mut Options) -> Result<(), String> {
    let mut line = String::new();
    let mut prompt = |question: &str| -> Result<String, String> {
        eprint!("{question}");
        let _ = std::io::stderr().flush();
        line.clear();
        std::io::stdin()
            .lock()
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        Ok(line.trim().to_ascii_lowercase())
    };
    o.kind = match prompt("Create an application or a library? [app/lib] ")?.as_str() {
        "app" | "a" | "application" => Some(Kind::App),
        "lib" | "l" | "library" => Some(Kind::Lib),
        other => return Err(format!("`{other}` is neither `app` nor `lib`")),
    };
    o.pages = matches!(
        prompt("Publish the documentation to GitHub Pages? [y/N] ")?.as_str(),
        "y" | "yes"
    );
    Ok(())
}

/// Whether a package name is a name `hd` accepts: letters, digits, `_` and
/// `-` (`cli.name.hyphen`), starting with a letter or `_`.
fn valid_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// The files of the new package, by path below its directory.
fn files(kind: Kind, name: &str, pages: bool) -> Vec<(String, String)> {
    let ident = hd_project::package_ident(name);
    let mut out = vec![(
        "hd.toml".to_owned(),
        format!("[package]\nname = \"{name}\"\n"),
    )];
    match kind {
        Kind::App => {
            out.push((
                "src/main.hd".to_owned(),
                "pub fn main() -> void $ Console:\n    println(\"hello, world\")\n".to_owned(),
            ));
            out.push((
                format!("tests/{ident}.hd"),
                format!(
                    "use std.testing.{{assert_equal, hd_run}}\n\nit(\"prints a greeting\"):\n    let out = hd_run!(\"{name}\")\n    assert_equal(out.stdout, \"hello, world\\n\", reason=\"the greeting\")\n    assert_equal(out.status, 0, reason=\"a clean exit\")\n"
                ),
            ));
        }
        Kind::Lib => {
            out.push((
                "src/lib.hd".to_owned(),
                "## Greets `name`.\npub fn greet(name: string) -> string:\n    \"hello, ${name}\"\n"
                    .to_owned(),
            ));
            out.push((
                format!("tests/{ident}.hd"),
                "use pkg.{greet}\nuse std.testing.assert_equal\n\nit(\"greets by name\"):\n    assert_equal(greet(\"world\"), \"hello, world\", reason=\"the greeting\")\n"
                    .to_owned(),
            ));
        }
    }
    if pages {
        out.push((".github/workflows/docs.yml".to_owned(), workflow()));
    }
    out
}

/// The GitHub Pages workflow of `cli.new.pages.workflow`.
fn workflow() -> String {
    let v = env!("CARGO_PKG_VERSION");
    format!(
        "# Written by `hd new --pages` (hd {v}).
name: Docs
on:
  push:
    branches: [main]
  workflow_dispatch:
permissions:
  contents: read
  pages: write
  id-token: write
concurrency:
  group: pages
  cancel-in-progress: false
jobs:
  docs:
    runs-on: ubuntu-latest
    environment:
      name: github-pages
      url: ${{{{ steps.deploy.outputs.page_url }}}}
    steps:
      - uses: actions/checkout@v5
      - uses: hd-lang/setup-hd@{v}
      - run: hd doc --out _site
      - uses: actions/upload-pages-artifact@v4
        with:
          path: _site
      - id: deploy
        uses: actions/deploy-pages@v4
"
    )
}

/// Whether `dir` lies inside a git repository (`cli.new.vcs`).
fn in_git_repository(dir: &Path) -> bool {
    dir.ancestors().any(|d| d.join(".git").exists())
}

pub(crate) fn command(args: &[OsString]) -> ExitCode {
    let mut o = match options(args) {
        Ok(o) => o,
        Err(e) => return fail(&e),
    };
    if o.kind.is_none() {
        // `cli.new.kind.no-terminal`: never a kind of its own choosing.
        if !std::io::stdin().is_terminal() {
            return fail(
                "`hd new` needs `--app` for an application or `--lib` for a library, since standard input is not a terminal to ask",
            );
        }
        if let Err(e) = ask(&mut o) {
            return fail(&e);
        }
    }
    let kind = o.kind.unwrap_or(Kind::App);
    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => return fail(&e.to_string()),
    };
    let dir = match &o.path {
        Some(p) if p != Path::new(".") => cwd.join(p),
        _ => cwd.clone(),
    };
    let Some(name) = dir.file_name().map(|n| n.to_string_lossy().into_owned()) else {
        return fail("`hd new` needs a directory with a name");
    };
    if !valid_name(&name) {
        return fail(&format!(
            "`{name}` is no package name: it holds letters, digits, `_` and `-`, and starts with a letter or `_`"
        ));
    }
    let files = files(kind, &name, o.pages);
    // `cli.new.existing`: one existing file stops it before any write.
    if let Some((path, _)) = files.iter().find(|(p, _)| dir.join(p).exists()) {
        return fail(&format!(
            "`{}` exists already, so `hd new` writes nothing",
            dir.join(path).display()
        ));
    }
    let vcs = o.vcs && !in_git_repository(&dir);
    if vcs && dir.join(".gitignore").exists() {
        return fail(&format!(
            "`{}` exists already, so `hd new` writes nothing",
            dir.join(".gitignore").display()
        ));
    }
    let workspace = std::fs::create_dir_all(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))
        .and_then(|()| workspace_of(&dir));
    let workspace = match workspace {
        Ok(w) => w,
        Err(e) => return fail(&e),
    };
    for (path, text) in &files {
        let full = dir.join(path);
        let written = full
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&full, text));
        if let Err(e) = written {
            return fail(&format!("{}: {e}", full.display()));
        }
    }
    if vcs {
        let init = Command::new("git")
            .arg("init")
            .arg("--quiet")
            .current_dir(&dir)
            .stdin(Stdio::null())
            .status();
        if !init.is_ok_and(|s| s.success()) {
            return fail(&format!(
                "`git init` failed in {}; pass `--vcs none` to skip it",
                dir.display()
            ));
        }
        if let Err(e) = std::fs::write(dir.join(".gitignore"), "/build/\n") {
            return fail(&format!("{}: {e}", dir.join(".gitignore").display()));
        }
    }
    // `cli.new.workspace-member`.
    if let Some((manifest, text)) = workspace
        && let Err(e) = std::fs::write(&manifest, text)
    {
        return fail(&format!("{}: {e}", manifest.display()));
    }
    let what = match kind {
        Kind::App => "application",
        Kind::Lib => "library",
    };
    println!("created {what} package `{name}` in {}", dir.display());
    ExitCode::SUCCESS
}

/// The workspace manifest above `dir`, with `dir` added to its members, or
/// `None` when no workspace manifest lies above it or `dir` is listed in
/// its `exclude`.
fn workspace_of(dir: &Path) -> Result<Option<(PathBuf, String)>, String> {
    let Some(parent) = dir.parent() else {
        return Ok(None);
    };
    let Some((root, _)) = disk::workspace_root(parent) else {
        return Ok(None);
    };
    let full = std::fs::canonicalize(dir).map_err(|e| e.to_string())?;
    let rel = full
        .strip_prefix(&root)
        .map_err(|e| e.to_string())?
        .to_string_lossy()
        .replace('\\', "/");
    let manifest = root.join("hd.toml");
    let text =
        std::fs::read_to_string(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let excluded = hd_project::parse_manifest(&text)
        .ok()
        .and_then(|m| m.workspace)
        .is_some_and(|w| w.exclude.iter().any(|x| x.trim_end_matches('/') == rel));
    if excluded {
        return Ok(None);
    }
    Ok(hd_project::edit::add_member(&text, &rel).map(|t| (manifest, t)))
}
