//! `hd doc [--private] [--out DIR] [--open] [--format json] [-p NAME]`
//! (`cli.doc.*`): checks the package as `hd check` does, and when that
//! reports no error writes its documentation pages, HTML and Markdown
//! together, with `hd_doc`. `hd doc NAME` (`cli.doc.name`) checks the
//! package the same way, then prints one item, module or standard library
//! entry to standard output and writes no file.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use hd_diag::{Code, Severity};
use hd_driver::{BuildProfile, Goal};
use hd_project::{Role, SourceSet as _, module_below, role_of};

use crate::report::{self, Diag, Reporter};
use crate::{build_goal_with, disk, fail, package_of_cwd};

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
    let out = build_goal_with(&program, &Goal::Analyze, BuildProfile::Checked, true, rep)?;
    let texts =
        module_texts(&program.sources, &program.entries, o.private).map_err(|e| rep.fail(&e))?;
    let modules = modules_of(&texts);
    let root_ident = hd_project::package_ident(&program.package);
    let deps = dependency_roots(&program);
    let std_names = std_names();
    let std: Vec<hd_doc::Module<'_>> = std_names
        .iter()
        .map(|(path, text)| hd_doc::Module { path, text })
        .collect();
    let package = hd_doc::Package {
        name: &program.package,
        root: &root_ident,
        modules: &modules,
        ifaces: &out.ifaces,
        private: o.private,
        scopes: &out.scopes,
        deps: &deps,
        std: &std,
    };
    if let Some(name) = &o.name {
        return print_name(&program, &package, &out, name, rep);
    }
    let rendered = hd_doc::render(&package);
    for b in &rendered.broken {
        rep.diag(&broken_link(&program, b));
    }
    let files = rendered.files;
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

/// The dependencies' keys with the root module of each package.
fn dependency_roots(program: &disk::Program) -> Vec<(String, String)> {
    program
        .requires
        .iter()
        .filter_map(|(key, i)| {
            let dep = program.deps.get(usize::from(*i).checked_sub(1)?)?;
            Some((key.clone(), hd_project::package_ident(&dep.name)))
        })
        .collect()
}

/// The warning for a link that names no documented item
/// (`cli.doc.link.broken`), at the source file of its module.
fn broken_link(program: &disk::Program, b: &hd_doc::BrokenLink) -> Diag {
    let why = if b.private {
        "is private and not documented"
    } else {
        "names no item"
    };
    let mut diag = Diag::new(
        Some(Code::BrokenDocLink),
        Severity::Warning,
        &format!("the link `[`{}`]` {why}", b.name),
    );
    let root = hd_project::package_ident(&program.package);
    let module = b
        .module
        .strip_prefix(&root)
        .map(|m| m.trim_start_matches('.'));
    let file = program.sources.list().into_iter().find_map(|e| {
        let path = &e.path;
        (path.starts_with("src/") && Some(module_below(path).as_str()) == module)
            .then(|| program.sources.display(path))
    });
    if let Some(file) = file {
        diag = diag.at(&file, None);
    }
    diag
}

/// The (module path, text) of each module `hd doc` documents in `sources`
/// (`cli.doc.items`): the library modules, and with `main` also the
/// executables' (`cli.doc.main`).
fn module_texts(
    sources: &disk::DiskSources,
    entries: &[String],
    main: bool,
) -> Result<Vec<(String, String)>, String> {
    let mut texts = Vec::new();
    for entry in sources.list() {
        let path = &entry.path;
        if !path.starts_with("src/") {
            continue;
        }
        let module = module_below(path);
        // `cli.doc.main`: an executable's module is no library module, so
        // it has a page only with `--private`.
        let library = role_of(path) == Role::Lib && !entries.contains(&module);
        if !(library || (main && role_of(path) == Role::Exe)) {
            continue;
        }
        let Some(bytes) = sources.read(path) else {
            return Err(format!("{}: cannot be read", sources.display(path)));
        };
        texts.push((module, String::from_utf8_lossy(&bytes).into_owned()));
    }
    Ok(texts)
}

/// The standard library's modules, `text` for `lib/std/text.hd`, with the
/// source embedded in the driver.
fn std_names() -> Vec<(String, &'static str)> {
    hd_driver::STD_FILES
        .iter()
        .filter_map(|(path, text)| Some((path.strip_suffix(".hd")?.replace('/', "."), *text)))
        .collect()
}

fn modules_of(texts: &[(String, String)]) -> Vec<hd_doc::Module<'_>> {
    texts
        .iter()
        .map(|(path, text)| hd_doc::Module { path, text })
        .collect()
}

/// `hd doc NAME` (`cli.doc.name`): prints the section, page or entry
/// NAME names to standard output. A NAME under `dep.KEY.` is looked up in
/// that dependency's `pub` items only (`cli.doc.name.dependency`).
fn print_name(
    program: &disk::Program,
    package: &hd_doc::Package<'_>,
    out: &hd_driver::Output,
    name: &str,
    rep: &mut Reporter,
) -> Result<(), ExitCode> {
    let found = match name.strip_prefix("dep.") {
        None => hd_doc::lookup(package, name),
        Some(rest) => {
            let (key, item) = rest.split_once('.').unwrap_or((rest, ""));
            let Some(dep) = program
                .requires
                .iter()
                .find(|(k, _)| k == key)
                .and_then(|(_, i)| program.deps.get(usize::from(*i).checked_sub(1)?))
            else {
                let keys: Vec<String> = program
                    .requires
                    .iter()
                    .map(|(k, _)| format!("dep.{k}"))
                    .collect();
                return Err(missing(name, &keys, rep));
            };
            let texts = module_texts(&dep.sources, &[], false).map_err(|e| rep.fail(&e))?;
            let modules = modules_of(&texts);
            let root = hd_project::package_ident(&dep.name);
            let dep_package = hd_doc::Package {
                name: &dep.name,
                root: &root,
                modules: &modules,
                ifaces: &out.ifaces,
                private: false,
                scopes: &[],
                deps: &[],
                std: package.std,
            };
            let prefix = format!("dep.{key}.");
            if item.is_empty() {
                Err(vec![format!("{prefix}pkg")])
            } else {
                dependency_item(&dep_package, item)
                    .map_err(|c| c.into_iter().map(|n| format!("{prefix}{n}")).collect())
            }
        }
    };
    match found {
        Ok(text) => {
            print!("{text}");
            Ok(())
        }
        Err(candidates) => Err(missing(name, &candidates, rep)),
    }
}

/// An item of a dependency, named from its root module: `dep.json.parse`
/// is `parse` of the dependency's `pkg`, and `dep.json.text.slugify` is
/// the module `text`.
fn dependency_item(dep: &hd_doc::Package<'_>, item: &str) -> Result<String, Vec<String>> {
    let modules = hd_doc::lookup(dep, item);
    let Err(mut candidates) = modules else {
        return modules;
    };
    match hd_doc::lookup(dep, &format!("pkg.{item}")) {
        Ok(text) => Ok(text),
        Err(root) => {
            candidates.extend(
                root.into_iter()
                    .map(|n| n.strip_prefix("pkg.").map_or(n.clone(), str::to_owned)),
            );
            candidates.dedup();
            Err(candidates)
        }
    }
}

/// `cli.doc.name.missing`: the error for a NAME that names nothing, with
/// the closest documented names.
fn missing(name: &str, candidates: &[String], rep: &mut Reporter) -> ExitCode {
    let tail = if candidates.is_empty() {
        String::new()
    } else {
        format!("; candidates: {}", candidates.join(", "))
    };
    rep.fail(&format!(
        "`hd doc {name}`: no documented item has this name{tail}"
    ))
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
