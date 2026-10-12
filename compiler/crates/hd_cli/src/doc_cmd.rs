//! `hd doc [--private] [--out DIR] [--open] [--format json] [-p NAME]`
//! (`cli.doc.*`): checks the package as `hd check` does, and when that
//! reports no error writes its documentation pages, HTML and Markdown
//! together, with `hd_doc`. `hd doc NAME` is not implemented yet.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use hd_diag::Severity;
use hd_driver::{BuildProfile, Goal};
use hd_project::{Role, SourceSet as _, module_below, role_of};

use crate::report::{self, Diag, Reporter};
use crate::{build_goal, disk, fail, package_of_cwd};

struct Options {
    json: bool,
    private: bool,
    open: bool,
    out: Option<PathBuf>,
    packages: Vec<String>,
    name: Option<String>,
}

fn options(args: &[OsString]) -> Result<Options, String> {
    let mut o = Options {
        json: false,
        private: false,
        open: false,
        out: None,
        packages: Vec::new(),
        name: None,
    };
    let mut i = 0;
    while i < args.len() {
        if let Some(format) = report::format_flag(args, i) {
            let (json, used) = format?;
            o.json = json;
            i += used;
            continue;
        }
        if let Some(p) = disk::package_flag(args, i) {
            let (name, used) = p?;
            o.packages.push(name);
            i += used;
            continue;
        }
        let text = args[i].to_string_lossy();
        i += 1;
        match text.as_ref() {
            "--private" => o.private = true,
            "--open" => o.open = true,
            "--out" => {
                let dir = args.get(i).ok_or("`--out` needs a DIR")?;
                o.out = Some(PathBuf::from(dir));
                i += 1;
            }
            _ if text.starts_with("--out=") => {
                o.out = Some(PathBuf::from(&text["--out=".len()..]));
            }
            _ if text.starts_with('-') => return Err(format!("`hd doc` has no option `{text}`")),
            _ if o.name.is_some() => return Err("`hd doc` takes at most one NAME".to_owned()),
            _ => o.name = Some(text.into_owned()),
        }
    }
    Ok(o)
}

/// The package `hd doc` documents (`cli.doc.package`): the one member
/// `-p NAME` selects in a workspace (`cli.doc.workspace`), else the
/// package of the working directory.
fn package_root(packages: &[String]) -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    if !packages.is_empty() {
        return match disk::select_members(&cwd, packages)?.as_slice() {
            [(root, _)] => Ok(root.clone()),
            _ => Err(
                "`hd doc` documents one workspace member; select it with one `-p NAME`".to_owned(),
            ),
        };
    }
    if disk::workspace_root(&cwd).is_some() {
        return Err("this is a workspace; select the member to document with `-p NAME`".to_owned());
    }
    package_of_cwd("doc")
}

pub(crate) fn command(args: &[OsString]) -> ExitCode {
    let o = match options(args) {
        Ok(o) => o,
        Err(e) => return fail(&e),
    };
    let mut rep = Reporter::stdout(o.json);
    if let Some(name) = &o.name {
        return rep.fail(&format!(
            "`hd doc {name}` prints one item, which is not implemented yet; `hd doc` writes every page"
        ));
    }
    let root = match package_root(&o.packages) {
        Ok(r) => r,
        Err(e) => return rep.fail(&e),
    };
    match document(&root, &o, &mut rep) {
        Ok(()) => rep.finish(0),
        Err(code) => code,
    }
}

/// Checks the package (`cli.doc.check`), then writes its pages.
fn document(root: &Path, o: &Options, rep: &mut Reporter) -> Result<(), ExitCode> {
    let mut program = disk::load_package(root, "main").map_err(|e| rep.fail(&e))?;
    program.only_program("");
    let out = build_goal(&program, &Goal::Analyze, BuildProfile::Checked, rep)?;
    let mut texts: Vec<(String, String)> = Vec::new();
    for entry in program.sources.list() {
        let path = &entry.path;
        if !path.starts_with("src/") {
            continue;
        }
        let module = module_below(path);
        // `cli.doc.main`: an executable's module is no library module, so
        // it has a page only with `--private`.
        let library = role_of(path) == Role::Lib && !program.entries.contains(&module);
        if !(library || (o.private && role_of(path) == Role::Exe)) {
            continue;
        }
        let Some(bytes) = program.sources.read(path) else {
            return Err(rep.fail(&format!(
                "{}: cannot be read",
                program.sources.display(path)
            )));
        };
        texts.push((module, String::from_utf8_lossy(&bytes).into_owned()));
    }
    let modules: Vec<hd_doc::Module<'_>> = texts
        .iter()
        .map(|(path, text)| hd_doc::Module { path, text })
        .collect();
    let root_ident = hd_project::package_ident(&program.package);
    let files = hd_doc::render(&hd_doc::Package {
        name: &program.package,
        root: &root_ident,
        modules: &modules,
        ifaces: &out.ifaces,
        private: o.private,
    });
    // `cli.doc.dir`: `build/doc`, or `--out DIR` below the working directory.
    let dir = o
        .out
        .clone()
        .unwrap_or_else(|| root.join("build").join("doc"));
    for f in &files {
        let path = dir.join(&f.path);
        let written = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&path, &f.text));
        if let Err(e) = written {
            return Err(rep.fail(&format!("{}: {e}", path.display())));
        }
    }
    if o.open {
        open(&dir.join("index.html"), rep);
    }
    Ok(())
}

/// `cli.doc.open`: opens the entry page with the platform's default
/// program. The pages are written already, so a failure only warns.
fn open(page: &Path, rep: &mut Reporter) {
    let mut command = if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else if cfg!(windows) {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]);
        c
    } else {
        std::process::Command::new("xdg-open")
    };
    let status = command.arg(page).status();
    if !status.is_ok_and(|s| s.success()) {
        rep.diag(&Diag::new(
            None,
            Severity::Warning,
            &format!("could not open {}", page.display()),
        ));
    }
}
