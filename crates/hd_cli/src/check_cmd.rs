//! `hd check [FILE] [--tests | --all] [--format json]` (`cli.package.whole`, `cli.check.*`,
//! `cli.file.check-test`, `cli.json.*`): the pipeline through checking and
//! coherence, with no collection or emission.

use hd_base::SliceSort as _;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use hd_cache::{CacheStore, DeferredStore};
use hd_diag::Code;
use hd_driver::{Goal, Host, Output};

use hd_project::{ModuleTable, Role, SourceSet, role_of};

use crate::report::{self, Diag, Summary};
use crate::{HD_FAILURE, Wall, disk, executor, fail, package_of_cwd};

/// What `hd check` was asked for.
#[derive(Default)]
struct Options {
    file: Option<OsString>,
    json: bool,
    summary: bool,
    max_errors: Option<usize>,
    /// `--tests`: also the test code (`cli.check.tests`).
    tests: bool,
    /// `--all`: also the test code and the tasks (`cli.check.all`).
    all: bool,
    /// The members `-p NAME` selects (`cli.workspace.select.anywhere`).
    packages: Vec<String>,
    lint: Option<hd_lint::Policy>,
    /// Apply the safe fix-its of this command before reporting what remains.
    fix: bool,
}

fn options(args: &[OsString], command: &str) -> Result<Options, String> {
    let mut o = Options::default();
    let mut i = 0;
    while i < args.len() {
        if let Some(format) = report::format_flag(args, i) {
            let (json, used) = format?;
            o.json = json;
            i += used;
            continue;
        }
        if let Some(jobs) = crate::jobs_flag(args, i) {
            let (n, used) = jobs?;
            crate::set_jobs(n);
            i += used;
            continue;
        }
        if let Some(p) = disk::package_flag(args, i) {
            let (name, used) = p?;
            o.packages.push(name);
            i += used;
            continue;
        }
        let a = &args[i];
        i += 1;
        let text = a.to_string_lossy();
        let switch = match text.as_ref() {
            "--fix" => Some(&mut o.fix),
            "--summary" => Some(&mut o.summary),
            "--tests" => Some(&mut o.tests),
            "--all" => Some(&mut o.all),
            _ => None,
        };
        if let Some(on) = switch {
            *on = true;
            continue;
        }
        let max = if text == "--max-errors" {
            let value = args.get(i).ok_or("`--max-errors` needs a value")?;
            i += 1;
            Some(value.to_string_lossy().into_owned())
        } else {
            text.strip_prefix("--max-errors=").map(str::to_owned)
        };
        if let Some(value) = max {
            match value.parse::<usize>() {
                Ok(n) if n >= 1 => o.max_errors = Some(n),
                _ => {
                    return Err(format!(
                        "`--max-errors` takes a whole number of at least 1, not `{value}`"
                    ));
                }
            }
        } else if text.starts_with('-') {
            return Err(format!("`hd {command}` has no option `{text}`"));
        } else if o.file.replace(a.clone()).is_some() {
            return Err(format!("`hd {command}` takes at most one FILE"));
        }
    }
    Ok(o)
}

/// A program to check.
struct Target {
    program: disk::Program,
    /// The package-relative FILE whose module is checked (`cli.package.file`).
    only: Option<String>,
    /// In workspace mode, the member's directory below the workspace root,
    /// which its diagnostics' files start with.
    member: Option<String>,
}

/// What `hd check` checks: FILE's package or FILE alone, the package of the
/// working directory, or in workspace mode every member
/// (`cli.workspace.members`).
fn targets(
    file: Option<&OsString>,
    packages: &[String],
    command: &str,
) -> Result<Vec<Target>, String> {
    let Some(file) = file else {
        let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
        if !packages.is_empty() {
            return disk::select_members(&cwd, packages)?
                .into_iter()
                .map(|(dir, rel)| {
                    Ok(Target {
                        program: disk::load_package(&dir, "main")?,
                        only: None,
                        member: Some(rel),
                    })
                })
                .collect();
        }
        if let Some((ws, members)) = disk::workspace_root(&cwd) {
            return members
                .iter()
                .map(|dir| {
                    Ok(Target {
                        program: disk::load_package(dir, "main")?,
                        only: None,
                        member: Some(
                            dir.strip_prefix(&ws)
                                .unwrap_or(dir)
                                .to_string_lossy()
                                .replace('\\', "/"),
                        ),
                    })
                })
                .collect();
        }
        let root = package_of_cwd(command)
            .map_err(|e| format!("{e}; or pass a FILE to check it as a single-file program"))?;
        return Ok(vec![Target {
            program: disk::load_package(&root, "main")?,
            only: None,
            member: None,
        }]);
    };
    let path = Path::new(file);
    if path.extension().is_none_or(|x| x != "hd") {
        return Err(format!(
            "`{}` is not an .hd file; `hd {command}` takes a FILE.hd",
            path.display()
        ));
    }
    let full = hd_run::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let dir = full.parent().unwrap_or(Path::new("."));
    match disk::package_root(dir).filter(|root| disk::under_root(root, &full)) {
        Some(root) => {
            let only = full.strip_prefix(&root).ok().map(|rel| {
                let rel = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/");
                disk::listed_path(&root, &rel)
            });
            Ok(vec![Target {
                program: disk::load_package(&root, "main")?,
                only,
                member: None,
            }])
        }
        None => Ok(vec![Target {
            program: disk::load_file(path)?,
            only: None,
            member: None,
        }]),
    }
}

/// Checks one target: its manifest's diagnostics and, unless one is an
/// error, the compiler's; and the count of modules whose bodies it checked.
fn check_one(mut t: Target, o: &Options, store: &dyn CacheStore) -> (Vec<Diag>, usize) {
    if t.program.problems.iter().any(Diag::is_error) {
        return (t.program.problems, 0);
    }
    let lint = match o.lint {
        None => None,
        Some(explicit) => match hd_lint::Policy::from_manifest(t.program.lint.as_ref()) {
            Ok(manifest) => Some(manifest.overlay(explicit)),
            Err(error) => {
                t.program.problems.push(
                    Diag::error(None, &error.message).at("hd.toml", Some(error.line as usize)),
                );
                return (t.program.problems, 0);
            }
        },
    };
    let (tests, left_out) = scope(&mut t, o);
    // A warm run whose inputs are unchanged replays the report a run over
    // them stored (#1510), before the doc tests' skims or any build.
    let stored = stored_report(&t, tests, lint, store);
    if let Some(mut shown) =
        stored.and_then(|(key, client)| hd_driver::diagnostics::decode(&key.read(store, client)?))
    {
        report::relocate(&mut shown, |f| t.program.sources.display(f));
        let mut diags = t.program.problems;
        diags.extend(left_out);
        diags.extend(shown);
        return (diags, 0);
    }
    // `cli.check.tests.doc`: the doc tests too, but for compile-fail ones.
    let docs: Vec<crate::test_cmd::Doc> = if tests && t.program.as_written.is_none() {
        crate::test_cmd::doc_tests(
            &t.program.sources,
            &hd_driver::source_flags(&t.program.package, &t.program.sources, store),
            t.only.as_deref(),
        )
        .into_iter()
        .filter(|d| d.test.compile_fail.is_none())
        .collect()
    } else {
        Vec::new()
    };
    // `cli.check.tests`: the library role as plain `hd check` checks it,
    // then the test role over the same sources and the doc tests. The
    // library role's entries are the ones plain `hd check` caches. A
    // diagnostic both roles report is shown once.
    let library;
    let lib_sources = if tests && t.program.as_written.is_none() {
        let mut only_library = t.program.sources.clone();
        only_library.retain(|path| matches!(role_of(path), Role::Lib | Role::Exe));
        library = only_library;
        &library
    } else {
        &t.program.sources
    };
    let (mut shown, mut checked) = check_role(&t, store, lib_sources, &Goal::Analyze, &[], lint);
    if tests {
        for d in &docs {
            t.program.sources.add_synthetic(&d.path, &d.test.program);
        }
        let (more, names) = check_role(
            &t,
            store,
            &t.program.sources,
            &Goal::CheckTests,
            &docs,
            lint,
        );
        for x in more {
            if !shown.contains(&x) {
                shown.push(x);
            }
        }
        checked.extend(names);
        checked.sort_stable();
        checked.dedup();
    }
    if let Some((key, client)) = stored
        && shown.iter().all(|d| d.code.is_some() || !d.is_error())
    {
        key.write(store, client, &hd_driver::diagnostics::encode(&shown));
    }
    report::relocate(&mut shown, |f| t.program.sources.display(f));
    let mut diags = t.program.problems;
    diags.extend(left_out);
    diags.extend(shown);
    (diags, checked.len())
}

/// The key of `t`'s stored report (#1510): the package-result key of a
/// check over its sources, and the hash of what the report reads besides
/// them: the scope, the FILE, a single file as the command line wrote it
/// (`cli.diagnostic.file` names its diagnostics so), the lint policy
/// and a package's manifest text (`caps::required` reads its capability
/// tables). A report is a function of these alone: the doc tests come from
/// the sources, and every other input is the run's. `None` when the run
/// could not take a stored result.
fn stored_report(
    t: &Target,
    tests: bool,
    lint: Option<hd_lint::Policy>,
    store: &dyn CacheStore,
) -> Option<(hd_driver::ResultKey, hd_base::Hash128)> {
    let clock = Wall(Instant::now());
    let host = Host {
        render_tir: &[],
        sources: &t.program.sources,
        store,
        clock: &clock,
        executor: executor(),
    };
    let key = hd_driver::result_key(
        &host,
        &t.program.package,
        &t.program.packages(),
        &Goal::Analyze,
    )?;
    let mut client = hd_base::StableHasher::new("cli-check-report-v3");
    client.u8(u8::from(tests));
    client.str(t.only.as_deref().unwrap_or(""));
    match &t.program.as_written {
        Some(file) => {
            client.u8(1);
            client.str(file);
        }
        None => client.u8(0),
    }
    client.str(&format!("{lint:?}"));
    client.str(&t.program.entry);
    match crate::manifest_text(&t.program) {
        Some(text) => client.hash(hd_base::hash128(&text)),
        None => client.u8(0),
    }
    Some((key, client.finish()))
}

/// One role's run over `t`'s sources: its diagnostics, with the doc tests'
/// relocated to their source, and the modules whose bodies it checked.
fn check_role(
    t: &Target,
    store: &dyn CacheStore,
    sources: &disk::DiskSources,
    goal: &Goal,
    docs: &[crate::test_cmd::Doc],
    lint: Option<hd_lint::Policy>,
) -> (Vec<Diag>, Vec<String>) {
    let clock = Wall(Instant::now());
    let host = Host {
        render_tir: &[],
        sources,
        store,
        clock: &clock,
        executor: executor(),
    };
    // `hd check FILE` checks only FILE's module, its doc tests and what
    // they use, deeply (crates/hd_cli/DESIGN.md §7.2), the modules `shown`
    // keeps.
    let mut packages = t.program.packages();
    packages.doc_scopes = matches!(goal, Goal::Analyze)
        && lint.is_some_and(|policy| !policy.allows(Code::BrokenDocLink));
    packages.check_roots = t.only.as_ref().map(|only| {
        std::iter::once(only.clone())
            .chain(docs.iter().map(|d| d.path.clone()))
            .collect()
    });
    let mut out = hd_driver::build_packages(&host, &t.program.package, &packages, goal);
    let Some(policy) = lint else {
        let mut shown = shown(&out, t, docs);
        relocate_docs(&mut shown, docs);
        return (shown, out.counters.modules_checked);
    };
    if shown(&out, t, docs).iter().any(Diag::is_error) {
        let mut shown = shown(&out, t, docs);
        relocate_docs(&mut shown, docs);
        return (shown, out.counters.modules_checked);
    }

    let root_files: BTreeSet<_> = sources.list().into_iter().map(|entry| entry.path).collect();
    let reached = reached_files(&out, t, docs);
    let selected: Vec<_> = out
        .files
        .iter()
        .enumerate()
        .map(|(file, path)| {
            root_files.contains(path)
                && reached
                    .as_ref()
                    .is_none_or(|reached| reached.get(file).copied().unwrap_or(false))
        })
        .collect();
    let source_policy =
        match hd_lint::SourcePolicy::from_output(&out, &selected) {
            Ok(policy) => policy,
            Err(errors) => {
                let mut shown = shown(&out, t, docs);
                shown.extend(errors.into_iter().map(|error| {
                    Diag::error(None, &error.message).at(&error.file, Some(error.line))
                }));
                relocate_docs(&mut shown, docs);
                return (shown, out.counters.modules_checked);
            }
        };
    let mut command_findings = Vec::new();
    if !policy.all_allow() {
        out = hd_driver::build_packages_with_analysis(
            &host,
            &t.program.package,
            &packages,
            goal,
            Some(&hd_lint::ANALYZER),
        );
        if !shown(&out, t, docs).iter().any(Diag::is_error) {
            let findings = policy.apply_with_source(&out.analysis_diags, &source_policy);
            out.diags.append(&findings);
            if matches!(goal, Goal::Analyze) && !policy.allows(Code::BrokenDocLink) {
                match crate::doc_cmd::lint_broken_links(
                    &t.program,
                    &out,
                    policy,
                    reached.as_deref(),
                ) {
                    Ok(findings) => command_findings = findings,
                    Err(message) => command_findings.push(Diag::error(None, &message)),
                }
            }
        }
    }
    let mut shown = shown(&out, t, docs);
    shown.extend(command_findings);
    relocate_docs(&mut shown, docs);
    (shown, out.counters.modules_checked)
}

fn relocate_docs(shown: &mut [Diag], docs: &[crate::test_cmd::Doc]) {
    for diagnostic in shown {
        if let Some(doc) = docs
            .iter()
            .find(|doc| diagnostic.file.as_ref() == Some(&doc.path))
        {
            doc.relocate(diagnostic);
        }
    }
}

/// Whole-package diagnostics including tests and tasks, as `hd check --all`
/// reports them. The fixing pass uses this so every package entry is
/// considered, as the former standalone command did.
pub(crate) fn fix_package(program: disk::Program, lint: Option<hd_lint::Policy>) -> Vec<Diag> {
    let t = Target {
        program,
        only: None,
        member: None,
    };
    check_one(
        t,
        &Options {
            all: true,
            lint,
            ..Options::default()
        },
        &DeferredStore::new(crate::disk_store()),
    )
    .0
}

pub(crate) fn command(args: &[OsString]) -> ExitCode {
    command_with(args, None)
}

pub(crate) fn lint_command(args: &[OsString], policy: hd_lint::Policy) -> ExitCode {
    command_with(args, Some(policy))
}

fn command_with(args: &[OsString], lint: Option<hd_lint::Policy>) -> ExitCode {
    let command = if lint.is_some() { "lint" } else { "check" };
    let mut o = match options(args, command) {
        Ok(o) => o,
        Err(e) => return fail(&e),
    };
    o.lint = lint;
    if o.fix {
        if o.file.is_some()
            || !o.packages.is_empty()
            || o.tests
            || o.summary
            || o.max_errors.is_some()
            || o.json
        {
            return fail(&format!(
                "`hd {command} --fix` works on one whole package and takes no FILE, -p, --tests, --summary, --max-errors, or --format json"
            ));
        }
        return crate::fix::command(command, lint);
    }
    let (mut diags, mut modules_checked) = (Vec::new(), 0);
    // Print, then publish (`crates/hd_cache/DESIGN.md` §5.4): the entries
    // reach the disk when the store drops, after the report.
    let store = DeferredStore::new(crate::disk_store());
    if o.file.is_some() && !o.packages.is_empty() {
        return fail(&format!(
            "`hd {command}` takes a FILE or `-p NAME`, not both"
        ));
    }
    match targets(o.file.as_ref(), &o.packages, command) {
        Ok(ts) => {
            for t in ts {
                let member = t.member.clone();
                let (mut ds, n) = check_one(t, &o, &store);
                if let Some(m) = member {
                    for d in &mut ds {
                        if let Some(f) = &mut d.file {
                            *f = format!("{m}/{f}");
                        }
                        for e in d.fixes.iter_mut().flat_map(|f| &mut f.edits) {
                            e.file = format!("{m}/{}", e.file);
                        }
                    }
                }
                diags.extend(ds);
                modules_checked += n;
            }
        }
        Err(e) => diags.push(Diag::error(None, &e)),
    }
    if lint.is_some() {
        diags.sort_stable_by(|a, b| {
            (&a.file, a.line, a.column, a.code.map(Code::as_str)).cmp(&(
                &b.file,
                b.line,
                b.column,
                b.code.map(Code::as_str),
            ))
        });
    }
    let failed = diags.iter().any(Diag::is_error);
    let status = if failed { HD_FAILURE } else { 0 };
    if o.json {
        let mut text = String::new();
        for d in &diags {
            let _ = writeln!(text, "{}", d.json());
        }
        let summary = Summary {
            status,
            modules_checked: Some(modules_checked),
            ..Summary::of(&diags)
        };
        let _ = writeln!(text, "{}", summary.json());
        print!("{text}");
    } else {
        eprint!("{}", text_report(&diags, &o));
    }
    ExitCode::from(status)
}

/// Narrows a package's sources to what this check covers, and says whether
/// that includes test code. A whole-package check covers the library and
/// the executables (`cli.check.default`); `--tests` adds the test code, its
/// `tests:` blocks, test modules and integration test files
/// (`cli.check.tests`); `--all` adds the tasks too (`cli.check.all`). A
/// FILE of test code or a task brings in its kind. A single-file program is
/// its one file. The layout errors of the files left out, such as
/// `cli.task.beside-directory`, hold whatever the scope, so they are returned.
fn scope(t: &mut Target, o: &Options) -> (bool, Vec<Diag>) {
    let only = t.only.as_deref().map(role_of);
    let tests = o.tests || o.all || only == Some(Role::Test);
    let tasks = o.all || only == Some(Role::Task);
    let keep = |path: &str| match role_of(path) {
        Role::Test => tests,
        Role::Task => tasks,
        Role::Lib | Role::Exe => true,
    };
    if t.program.as_written.is_some() {
        return (tests, Vec::new());
    }
    let layout = ModuleTable::discover(&t.program.package, &t.program.sources);
    let left_out = layout
        .problems
        .iter()
        .map(|(file, code, message)| (&layout.files[file.idx()], code, message))
        .filter(|(path, _, _)| !keep(path))
        .map(|(path, code, message)| Diag::error(Some(*code), message).at(path, Some(1)))
        .collect();
    t.program.sources.retain(keep);
    (tests, left_out)
}

/// The diagnostics this run reports, in content order. A FILE in a package
/// narrows them to its module and the modules it uses, deeply
/// (`cli.package.file`), and the doc tests of its module; a diagnostic of
/// no file is kept.
fn shown(out: &Output, t: &Target, docs: &[crate::test_cmd::Doc]) -> Vec<Diag> {
    let as_written = t.program.as_written.as_deref();
    let mut all = report::from_output(out, as_written);
    all.extend(crate::caps::required(&t.program, out));
    let Some(reached) = reached_files(out, t, docs) else {
        return all;
    };
    all.into_iter()
        .filter(|d| {
            d.file.as_ref().is_none_or(|file| {
                out.files
                    .iter()
                    .position(|f| f == file)
                    .is_some_and(|i| reached[i])
            })
        })
        .collect()
}

/// FILE checks include its used modules and checked doc-test roots.
fn reached_files(out: &Output, t: &Target, docs: &[crate::test_cmd::Doc]) -> Option<Vec<bool>> {
    let only = t.only.as_ref()?;
    let mut reached = vec![false; out.files.len()];
    let mut stack: Vec<usize> = out
        .files
        .iter()
        .enumerate()
        .filter(|(_, f)| *f == only || docs.iter().any(|d| d.path == **f))
        .map(|(i, _)| i)
        .collect();
    while let Some(f) = stack.pop() {
        if !std::mem::replace(&mut reached[f], true) {
            stack.extend(out.uses.get(f).into_iter().flatten().copied());
        }
    }
    Some(reached)
}

/// The text report of `cli.check.*`: each diagnostic, or with `--summary` one
/// line per severity, file, and code, then the `check result` line.
fn text_report(diags: &[Diag], o: &Options) -> String {
    let mut text = String::new();
    let (mut errors, mut warnings) = (0, 0);
    let mut stopped = false;
    let mut counts: BTreeMap<(&str, &str, &str), usize> = BTreeMap::new();
    for d in diags {
        if d.is_error() {
            errors += 1;
        } else {
            warnings += 1;
        }
        if o.summary {
            let file = d.file.as_deref().unwrap_or("");
            let code = d.code.map_or("", Code::as_str);
            *counts.entry((file, code, d.severity_word())).or_insert(0) += 1;
            continue;
        }
        if stopped {
            continue;
        }
        let _ = writeln!(text, "{}", d.text());
        // Printing stops after the Nth error; the counts keep going.
        stopped = d.is_error() && o.max_errors.is_some_and(|n| errors >= n);
    }
    for ((file, code, severity), n) in &counts {
        let _ = writeln!(text, "{severity}: {file}: {code}: {n}");
    }
    let result = if errors > 0 { "FAILED" } else { "ok" };
    let _ = writeln!(
        text,
        "check result: {result}. errors: {errors}; warnings: {warnings}"
    );
    text
}

#[cfg(all(test, unix))]
mod stored_report_tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use hd_base::Hash128;
    use hd_cache::{EntryKind, MemoryStore};
    use hd_testkit::Scratch;

    /// A store that counts its lookups.
    #[derive(Default)]
    struct Counted {
        store: MemoryStore,
        gets: AtomicUsize,
    }

    impl CacheStore for Counted {
        fn get(&self, kind: EntryKind, key: &Hash128) -> Option<Arc<[u8]>> {
            self.gets.fetch_add(1, Ordering::Relaxed);
            self.store.get(kind, key)
        }
        fn put(&self, kind: EntryKind, key: &Hash128, bytes: &[u8]) {
            self.store.put(kind, key, bytes);
        }
        fn touch(&self, kind: EntryKind, key: &Hash128) {
            self.store.touch(kind, key);
        }
    }

    /// `check_one` of the package at `root`, listed now: its report and
    /// the cache lookups it made.
    fn run(root: &Path, store: &Counted, o: &Options, only: Option<&str>) -> (Vec<Diag>, usize) {
        let t = Target {
            program: disk::load_package(root, "main").expect("package"),
            only: only.map(str::to_owned),
            member: None,
        };
        store.gets.store(0, Ordering::Relaxed);
        let (diags, _) = check_one(t, o, store);
        (diags, store.gets.load(Ordering::Relaxed))
    }

    /// Waits until every file written so far has a stamp.
    fn settle() {
        std::thread::sleep(disk::SETTLE + std::time::Duration::from_millis(200));
    }

    fn write(root: &Path, path: &str, text: &str) {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        std::fs::write(path, text).expect("write");
    }

    /// `cli.test.doc.location` (#1710): `hd check --tests` reports a doc
    /// test's diagnostic in its source file, end included: the end line maps
    /// through the `##` lines as the start does, and no column names the
    /// doc program's text. A warm run replays the same position.
    #[test]
    fn a_doc_test_diagnostic_ends_in_its_source_file() {
        let dir = Scratch::new(
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/tmp"),
            "check-doc-end",
        );
        let root = dir.join("p");
        write(&root, "hd.toml", "[package]\nname = \"shop\"\n");
        write(
            &root,
            "src/m.hd",
            "pub fn one() -> i32: 1\n\n## ```hd\n## use pkg.m.{nothing}\n## pass\n## ```\npub fn zero() -> i32: 0\n",
        );
        settle();
        let o = Options {
            tests: true,
            ..Options::default()
        };
        let store = Counted::default();
        for run_name in ["cold", "warm"] {
            let (diags, _) = run(&root, &store, &o, None);
            let found: Vec<_> = diags
                .iter()
                .filter(|d| d.message.contains("nothing"))
                .map(|d| {
                    (
                        d.file.as_deref(),
                        d.line,
                        d.column,
                        d.end_line,
                        d.end_column,
                    )
                })
                .collect();
            assert_eq!(
                found,
                [(Some("src/m.hd"), Some(4), None, Some(4), None)],
                "{run_name}: {diags:?}"
            );
        }
    }

    /// Cost Model (#1510): a warm `hd check` or `hd lint`, whole or of a
    /// FILE, with or without `--tests`, replays the stored report with one
    /// cache lookup: no doc-test skim, no source-allow parse, no rendered
    /// docs, no build. Its report equals the run that stored it, warnings,
    /// lint findings, doc-test errors and capability rows included. An
    /// edit, an added file, a removed file and a manifest edit each miss.
    #[test]
    fn a_warm_report_replays_with_one_lookup_and_each_change_misses() {
        let dir = Scratch::new(
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/tmp"),
            "check-stored-report",
        );
        let root = dir.join("p");
        write(
            &root,
            "hd.toml",
            "[package]\nname = \"shop\"\n\n[capabilities]\nConsole = true\n",
        );
        write(
            &root,
            "src/main.hd",
            "use pkg.f1.m.{value}\n\npub fn main() -> void $ Console:\n    println(\"{value()}\")\n",
        );
        // f0's doc test fails to check; f1 holds a lint finding.
        write(
            &root,
            "src/f0/m.hd",
            "## ```hd\n## use pkg.f0.m.{nothing}\n## pass\n## ```\npub fn zero() -> i32: 0\n",
        );
        write(
            &root,
            "src/f1/m.hd",
            "pub fn value() -> i32:\n    unread := +1\n    1\n",
        );
        for i in 2..12 {
            write(
                &root,
                &format!("src/f{i}/m.hd"),
                &format!("pub fn v() -> i32: {i}\n"),
            );
        }
        settle();
        let lint = Some(hd_lint::Policy::default());
        let runs = [
            Options::default(),
            Options {
                tests: true,
                ..Options::default()
            },
            Options {
                lint,
                ..Options::default()
            },
            Options {
                lint,
                tests: true,
                ..Options::default()
            },
        ];
        let store = Counted::default();
        let mut reports = Vec::new();
        for o in &runs {
            for only in [None, Some("src/f1/m.hd")] {
                let (cold, _) = run(&root, &store, o, only);
                let (warm, gets) = run(&root, &store, o, only);
                assert_eq!(
                    warm,
                    cold,
                    "tests {}, lint {}, {only:?}",
                    o.tests,
                    o.lint.is_some()
                );
                assert_eq!(
                    gets,
                    1,
                    "tests {}, lint {}, {only:?}",
                    o.tests,
                    o.lint.is_some()
                );
                reports.push(cold);
            }
        }
        let codes = |d: &[Diag]| {
            d.iter()
                .filter_map(|d| d.code)
                .map(Code::as_str)
                .collect::<Vec<_>>()
        };
        assert!(
            codes(&reports[5]).contains(&"unused-local-binding"),
            "{:?}",
            reports[5]
        );
        assert!(
            reports[2].iter().any(|d| d.message.contains("nothing")),
            "{:?}",
            reports[2]
        );
        let lint = &runs[2];
        let changes: [(&str, &dyn Fn()); 4] = [
            ("an edit", &|| {
                write(&root, "src/f1/m.hd", "pub fn value() -> i32:\n    1\n");
            }),
            ("an added file", &|| {
                write(&root, "src/g/x.hd", "pub fn x() -> i32: y\n");
            }),
            // `main` uses the removed module.
            ("a removed file", &|| {
                std::fs::remove_file(root.join("src/f1/m.hd")).expect("remove");
            }),
            ("a manifest edit", &|| {
                write(&root, "hd.toml", "[package]\nname = \"shop\"\n");
            }),
        ];
        let mut last = run(&root, &store, lint, None).0;
        for (change, apply) in changes {
            apply();
            settle();
            let (report, gets) = run(&root, &store, lint, None);
            assert!(gets > 1, "{change} misses");
            assert_ne!(report, last, "{change}");
            let (again, gets) = run(&root, &store, lint, None);
            assert_eq!((&again, gets), (&report, 1), "after {change}");
            last = report;
        }
        assert!(
            last.iter().any(|d| d.message.contains("Console")),
            "the manifest no longer lists `Console`: {last:?}"
        );
    }

    /// Cost Model (#1510): a warm `hd check FILE` or `hd lint FILE` of a
    /// single-file program replays its report with one lookup, though the
    /// file reaches std modules. The report names the file as the command
    /// line wrote it, so another spelling of it stores its own; a settled
    /// edit misses.
    #[test]
    fn a_warm_single_file_report_replays_with_one_lookup() {
        let dir = Scratch::new(
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/tmp"),
            "check-stored-single-file",
        );
        let file = dir.join("alone.hd");
        let text = |n: i32| format!("pub fn main() -> void:\n    unread := +{n}\n");
        std::fs::write(&file, text(1)).expect("file");
        settle();
        let store = Counted::default();
        let run = |path: &Path, o: &Options| {
            let t = Target {
                program: disk::load_file(path).expect("file"),
                only: None,
                member: None,
            };
            store.gets.store(0, Ordering::Relaxed);
            let (diags, _) = check_one(t, o, &store);
            (diags, store.gets.load(Ordering::Relaxed))
        };
        let lint = Options {
            lint: Some(hd_lint::Policy::default()),
            ..Options::default()
        };
        for o in [&Options::default(), &lint] {
            let (cold, gets) = run(&file, o);
            assert!(gets > 3, "cold, {gets} lookups");
            let (warm, gets) = run(&file, o);
            assert_eq!((&warm, gets), (&cold, 1), "lint {}", o.lint.is_some());
        }
        let (named, _) = run(&file, &lint);
        assert!(
            named
                .iter()
                .any(|d| d.code == Some(Code::UnusedLocalBinding)),
            "{named:?}"
        );
        let spelled = dir.join(".").join("alone.hd");
        let (other, gets) = run(&spelled, &lint);
        assert!(gets > 1, "another spelling stores its own report");
        let (again, gets) = run(&spelled, &lint);
        assert_eq!((&again, gets), (&other, 1));
        std::fs::write(&file, text(2)).expect("edit");
        settle();
        let (_, gets) = run(&file, &lint);
        assert!(gets > 1, "an edit misses");
        assert_eq!(run(&file, &lint).1, 1, "warm after the edit");
    }
}
