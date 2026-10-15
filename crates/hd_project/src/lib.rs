#![forbid(unsafe_code)]
//! `hd_project`: manifests, module discovery and identity, folders, the
//! folder graph and the `SourceSet` trait (`docs/compiler/design-overview.md` §2.1,
//! `crates/hd_resolve/DESIGN.md` §4.7 and §4.8).
//!
//! No file system access here (§2.2 rule 3): sources come through
//! `SourceSet`, which the CLI implements over the disk and `hd_web` over JS.

mod doctest;
pub mod edit;
mod manifest;

use std::collections::BTreeMap;
use std::sync::Arc;

use hd_base::{FileId, FolderId, ModuleId};
use hd_diag::Code;

pub use doctest::{DocTest, doc_tests};
pub use manifest::{
    CAPABILITY_KEYS, Executable, Grant, HostPath, Manifest, Problem, Requirement, Version,
    Workspace, compatibility_line, grant_problem, host_path, key_problem, parse_manifest, problems,
    requirement_problem,
};

/// One file of a source set: its package-relative path with `/`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceEntry {
    pub path: String,
    pub size: u64,
}

/// Where sources come from (§2.2 rule 3).
pub trait SourceSet: Sync {
    /// Every `.hd` file of the package, sorted by path bytes.
    fn list(&self) -> Vec<SourceEntry>;
    fn read(&self, path: &str) -> Option<Arc<[u8]>>;
}

/// An in-memory source set: tests, the playground, the REPL.
#[derive(Default, Clone)]
pub struct MemorySources {
    files: BTreeMap<String, Arc<[u8]>>,
}

impl MemorySources {
    pub fn insert(&mut self, path: &str, text: &str) {
        self.files
            .insert(path.to_owned(), Arc::from(text.as_bytes()));
    }

    /// Keeps the files whose path `keep` accepts.
    pub fn retain(&mut self, keep: impl Fn(&str) -> bool) {
        self.files.retain(|p, _| keep(p));
    }
}

impl SourceSet for MemorySources {
    fn list(&self) -> Vec<SourceEntry> {
        self.files
            .iter()
            .map(|(p, b)| SourceEntry {
                path: p.clone(),
                size: b.len() as u64,
            })
            .collect()
    }
    fn read(&self, path: &str) -> Option<Arc<[u8]>> {
        self.files.get(path).cloned()
    }
}

/// The role a file plays (`crates/hd_cache/DESIGN.md` §5.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    Lib,
    Exe,
    Test,
    Task,
}

/// The root a file's module path and its relative uses start from (§4.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Root {
    /// `src`, the source root: the package root module that `pkg` names
    /// (`module.manifest.source-root`).
    Source,
    /// `tests`, the test root (`module.test.integration`).
    Test,
    /// `tasks` (`cli.task.file`).
    Task,
    /// A REPL input (`docs/future-work/live-execution.md` "REPL Decisions"): the synthetic file `repl#N` of
    /// the session's `N`th input, outside every package folder. It is its
    /// own module and its own folder, and its relative lookup starts at
    /// the package root module, as `src/lib.hd`'s does (`cli.repl.package.lib`).
    Repl,
    /// A package without that layout, as std or a single-file program:
    /// its files sit directly under the package directory.
    Flat,
}

impl Root {
    /// The root of a package-relative file, and the file's path below it.
    #[must_use]
    pub fn of(file: &str) -> (Root, &str) {
        if repl_input(file).is_some() {
            (Root::Repl, file)
        } else if let Some(rest) = file.strip_prefix("src/") {
            (Root::Source, rest)
        } else if let Some(rest) = file.strip_prefix("tests/") {
            (Root::Test, rest)
        } else if let Some(rest) = file.strip_prefix("tasks/") {
            (Root::Task, rest)
        } else {
            (Root::Flat, file)
        }
    }

    /// This root's namespace below the package root module. The test and
    /// task roots get a segment no source path spells, so `pkg` never
    /// reaches them (`module.test.no-tests-root`) and a module under
    /// `src/tests/` stays a different module.
    fn segment(self) -> Option<&'static str> {
        match self {
            Root::Test => Some("$tests"),
            Root::Task => Some("$tasks"),
            Root::Source | Root::Flat | Root::Repl => None,
        }
    }
}

/// The number `N` of a REPL input's synthetic file `repl#N`
/// ([`Root::Repl`]), counted from 1. No file on disk has this name: a
/// source file's name ends in `.hd`.
#[must_use]
pub fn repl_input(file: &str) -> Option<u32> {
    let n = file.strip_prefix("repl#")?;
    if n.starts_with('0') || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    n.parse().ok().filter(|n| *n > 0)
}

/// The synthetic file of a session's `n`th input, as diagnostics name it
/// (`repl#3:1:5`).
#[must_use]
pub fn repl_file(n: u32) -> String {
    format!("repl#{n}")
}

/// The identifier form of a package name, which module paths start with:
/// each `-` written `_` (`trait.typeid.name.package`).
#[must_use]
pub fn package_ident(name: &str) -> String {
    name.replace('-', "_")
}

/// User text with each internal module path of test or task code named
/// by its file instead (`module.test.integration.no-path`,
/// `cli.task.no-path`). Such a path has a `$tests` or `$tasks` segment,
/// which no source spells, after a `.` (or a `/` in an item path such as
/// `shop/$tests/flow/main`); `file_of` gives the file of a module that
/// exists, and any other path is named by its directory under the root, as
/// `tests/common`.
#[must_use]
pub fn user_text(text: &str, file_of: &dyn Fn(&str) -> Option<String>) -> String {
    let word = |c: char| matches!(c, '.' | '/' | '$' | '_') || c.is_alphanumeric();
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = [".$tests", ".$tasks", "/$tests", "/$tasks"]
        .iter()
        .filter_map(|seg| rest.find(seg))
        .min()
    {
        let start = rest[..at]
            .char_indices()
            .rev()
            .find(|&(_, c)| !word(c))
            .map_or(0, |(i, c)| i + c.len_utf8());
        let end = rest[at + 1..]
            .char_indices()
            .find(|&(_, c)| !word(c))
            .map_or(rest.len(), |(i, _)| at + 1 + i);
        let path = rest[start..end].trim_end_matches(['.', '/']);
        let end = start + path.len();
        out.push_str(&rest[..start]);
        out.push_str(&file_of(path).unwrap_or_else(|| {
            let below = path.split_once('$').map_or(path, |(_, b)| b);
            below.replace('.', "/")
        }));
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// `text` with each module root of `renamed` written as its package's
/// identifier (`ModuleTable::renamed_roots`). A root is replaced where no
/// identifier character comes before it and no digit after it.
#[must_use]
pub fn rename_roots(text: &str, renamed: &[(String, String)]) -> String {
    let mut out = text.to_owned();
    for (root, ident) in renamed {
        let mut next = String::with_capacity(out.len());
        let mut rest = out.as_str();
        while let Some(at) = rest.find(root.as_str()) {
            let end = at + root.len();
            let before = rest[..at]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '$');
            let after = rest[end..].starts_with(|c: char| c.is_ascii_digit());
            next.push_str(&rest[..at]);
            next.push_str(if before || after { root } else { ident });
            rest = &rest[end..];
        }
        next.push_str(rest);
        out = next;
    }
    out
}

/// A package-relative file's module path below its package root module
/// (`module.path.*`): `src/user/types.hd` is `user.types`,
/// `src/user/mod.hd` is `user`, `src/lib.hd` is the root itself (empty),
/// `tests/checkout.hd` is `$tests.checkout`, and a flat `main.hd` is
/// `main`.
#[must_use]
pub fn module_below(file: &str) -> String {
    // A REPL input's segment is the compiler's own, which no source spells.
    if let Some(n) = repl_input(file) {
        return format!("$repl{n}");
    }
    let (root, rest) = Root::of(file);
    let stem = rest.strip_suffix(".hd").unwrap_or(rest);
    let mut segs: Vec<&str> = root.segment().into_iter().collect();
    if !(root == Root::Source && stem == "lib") {
        segs.extend(stem.split('/'));
        if segs.last() == Some(&"mod") {
            segs.pop();
        }
    }
    segs.join(".")
}

/// Whether a file is a root file (`module.relative.root-file`,
/// `cli.task.root-file`): `src/lib.hd`, `src/main.hd`, or a file directly
/// under the test root or `tasks`. Its relative lookup starts at its root.
fn is_root_file(file: &str) -> bool {
    match Root::of(file) {
        (Root::Source, rest) => rest == "lib.hd" || rest == "main.hd",
        (Root::Test | Root::Task, rest) => !rest.contains('/'),
        (Root::Repl, _) => true,
        (Root::Flat, _) => false,
    }
}

/// Whether a file is its own program, which no other module may use
/// (`module.path.main-no-use`, `module.test.integration.program-use`).
fn is_entry(file: &str) -> bool {
    is_root_file(file) && file != "src/lib.hd"
}

/// The role of a package-relative file: an entry, test code (a test
/// module or an integration test file), a task, or library code.
#[must_use]
pub fn role_of(file: &str) -> Role {
    match Root::of(file) {
        (Root::Source, "main.hd") => Role::Exe,
        (Root::Test, _) => Role::Test,
        (Root::Task, _) => Role::Task,
        _ if file.ends_with("_test.hd") => Role::Test,
        _ => Role::Lib,
    }
}

/// Whether a package-relative file belongs to the program whose entry is
/// `entry_file`: the library, the executables and `entry_file`
/// (`cli.check.default`, `module.test.code`). Test code and the other
/// tasks are no part of a program that `hd run` or `hd build` builds. A
/// task's program also has the shared task modules, its own copy of them
/// (`cli.task.shared-copy`); no other program does (`cli.task.not-shipped`).
#[must_use]
pub fn in_program(entry_file: &str, file: &str) -> bool {
    file == entry_file
        || matches!(role_of(file), Role::Lib | Role::Exe)
        || (role_of(entry_file) == Role::Task && shared_task_module(file))
}

/// Whether a package-relative file is a shared task module: below a
/// subdirectory of `tasks` (`cli.task.shared`).
fn shared_task_module(file: &str) -> bool {
    matches!(Root::of(file), (Root::Task, rest) if rest.contains('/'))
}

/// Which files of a package are its modules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// Every file: the package being built, std, a single file.
    All,
    /// Only its library under the source root: a dependency, whose test
    /// code and executables are never built (`module.test.code`,
    /// `module.path.main-file`).
    Library,
}

/// One package as discovery reads it.
pub struct PackageIn<'a> {
    /// The manifest name; module paths start with its `package_ident`,
    /// unless an earlier package holds it (`ModuleTable::roots`).
    pub name: &'a str,
    /// What tells it from another package of the same name
    /// (`module.dep.identity`): a fetched version's host path and
    /// compatibility line, a local dependency's canonical directory, or
    /// the name of the root package and of std. It keys the package's
    /// cached outputs, and names its files when its name is shared.
    pub identity: &'a str,
    pub sources: &'a dyn SourceSet,
    pub scope: Scope,
    /// Each dependency, by the name source writes after `dep.`, and the
    /// index of its package in the discovery list.
    pub requires: Vec<(String, u16)>,
    /// The entry modules its `[[executable]]` tables name, by their path
    /// below the source root (`cli.exe.entry-program`); `src/main.hd` is
    /// one whatever they say (`module.path.main-file`).
    pub entries: Vec<String>,
    /// Its dev dependencies, as `requires` (`module.test.dev-dependency`):
    /// only the root's are read (`module.select.dev-dependencies`).
    pub dev_requires: Vec<(String, u16)>,
}

/// One module: a file. Dense IDs; stable form is the module path.
#[derive(Clone, Debug)]
pub struct Module {
    pub id: ModuleId,
    pub file: FileId,
    pub path: String,
    pub folder: FolderId,
    pub role: Role,
    /// Index into `ModuleTable::packages`: 0 is the root package.
    pub package: u16,
    /// The module path relative lookup starts at, which `self` names: the
    /// module itself, or for a root file its root.
    pub base: String,
    /// The root `super` must not move above: the package root module, or
    /// the test or task root.
    pub floor: String,
    /// Its own program, which no `use` reaches.
    pub entry: bool,
}

/// What the roots of one module's use paths name (`module.root.*`,
/// `module.relative.*`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UseRoots {
    /// The package root module, which `pkg` names.
    pub pkg: String,
    /// Where relative lookup starts, which `self` names.
    pub base: String,
    /// The root `super` must not move above.
    pub floor: String,
    /// Each dependency name and its package root module (`dep.NAME`).
    pub deps: Vec<(String, String)>,
    /// Each dev dependency name, its package root module, and whether it
    /// depends back on the package (`module.test.dev-dependency`).
    pub dev: Vec<(String, String, bool)>,
    /// What the module's own uses may name of the dev dependencies.
    pub access: DevAccess,
}

/// Which dev dependencies a use may name (`module.test.non-test-use.*`,
/// `module.test.cyclic-dev-unit`, `cli.task.dev-dependencies`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum DevAccess {
    /// None: code that is not test code.
    #[default]
    None,
    /// Those that do not depend back on the package: a unit test module,
    /// or a `tests:` block (`module.test.dev-dependency.in-tests`).
    Unit,
    /// All: an integration test, a shared test module or a task.
    All,
}

/// Why a use path names no module path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UseRootError {
    /// Its first segment is no use root (`module.root.absolute`).
    UnknownRoot,
    /// `dep.NAME` names no dependency of the package.
    UnknownDependency,
    /// A `super` moves above the root (`module.relative.above-root`,
    /// `module.relative.root-file.super`).
    AboveRoot,
    /// `dep.NAME` is a dev dependency, and the use is not in test code
    /// (`module.test.non-test-use.dev-dependency`).
    TestOnly,
    /// `dep.NAME` is a dev dependency that depends back on the package,
    /// and the use is in a unit test (`module.test.cyclic-dev-unit`).
    CyclicTest,
}

impl UseRoots {
    /// A use path with its root replaced by the module path it names, for
    /// a use at the module's top level.
    pub fn absolute<S: AsRef<str>>(&self, path: &[S]) -> Result<Vec<String>, UseRootError> {
        self.absolute_with(path, self.access)
    }

    /// `absolute`, for a use inside a `tests:` block: test code, so it may
    /// name a dev dependency that does not depend back on the package
    /// (`module.test.dev-dependency.in-tests`).
    pub fn absolute_in_tests<S: AsRef<str>>(
        &self,
        path: &[S],
    ) -> Result<Vec<String>, UseRootError> {
        self.absolute_with(path, self.access.max(DevAccess::Unit))
    }

    fn absolute_with<S: AsRef<str>>(
        &self,
        path: &[S],
        access: DevAccess,
    ) -> Result<Vec<String>, UseRootError> {
        let segs = |p: &str| -> Vec<String> { p.split('.').map(str::to_owned).collect() };
        let Some((first, mut rest)) = path.split_first() else {
            return Err(UseRootError::UnknownRoot);
        };
        let mut out = match first.as_ref() {
            "std" => vec!["std".to_owned()],
            "pkg" => segs(&self.pkg),
            "self" => segs(&self.base),
            "dep" => {
                let (name, after) = rest.split_first().ok_or(UseRootError::UnknownDependency)?;
                rest = after;
                let root =
                    if let Some((_, root)) = self.deps.iter().find(|(n, _)| n == name.as_ref()) {
                        root
                    } else {
                        let (_, root, cyclic) = self
                            .dev
                            .iter()
                            .find(|(n, _, _)| n == name.as_ref())
                            .ok_or(UseRootError::UnknownDependency)?;
                        match access {
                            DevAccess::None => return Err(UseRootError::TestOnly),
                            DevAccess::Unit if *cyclic => return Err(UseRootError::CyclicTest),
                            _ => root,
                        }
                    };
                segs(root)
            }
            "super" => {
                let mut out = segs(&self.base);
                out.pop();
                while rest.first().is_some_and(|s| s.as_ref() == "super") {
                    rest = &rest[1..];
                    out.pop();
                }
                if out.len() < self.floor.split('.').count() {
                    return Err(UseRootError::AboveRoot);
                }
                out
            }
            _ => return Err(UseRootError::UnknownRoot),
        };
        out.extend(rest.iter().map(|s| s.as_ref().to_owned()));
        Ok(out)
    }
}

/// One folder: the interface unit.
#[derive(Clone, Debug)]
pub struct Folder {
    pub id: FolderId,
    pub path: String,
    pub modules: Vec<ModuleId>,
    pub package: u16,
}

/// Modules and folders of the root package and the packages it reaches
/// (std among them), in path order (§4.7).
#[derive(Clone, Debug, Default)]
pub struct ModuleTable {
    pub package: String,
    /// Package names: the root, then each dependency.
    pub packages: Vec<String>,
    /// Per package: its identity, as `PackageIn::identity`.
    pub identities: Vec<String>,
    /// Per package: its module root, which its module paths start with,
    /// unique in the table (`module_roots`).
    pub roots: Vec<String>,
    /// Per package: each dependency name and its package index.
    pub requires: Vec<Vec<(String, u16)>>,
    /// Per package: its dev dependencies, in the same form; only the root
    /// has any.
    pub dev_requires: Vec<Vec<(String, u16)>>,
    /// Display paths by `FileId`: package-relative for the root package,
    /// `<pkg>/path` for a dependency.
    pub files: Vec<String>,
    /// Where each file's text comes from: (package index, relative path).
    pub sources: Vec<(u16, String)>,
    pub modules: Vec<Module>,
    pub folders: Vec<Folder>,
    /// Identity errors of discovery (§4.7 step 3): each module's file, the
    /// code and the message, in path order.
    pub problems: Vec<(FileId, Code, String)>,
    by_path: BTreeMap<String, ModuleId>,
}

/// The first path component of `file` that its module identity uses but
/// that is no identifier (`module.path.identifier`): `mod.hd` is special,
/// `src/lib.hd` is the root itself, and the `$tests`/`$tasks` segments are
/// the compiler's own. A flat file is no module, so it has none.
fn invalid_module_component(file: &str) -> Option<String> {
    let (root, rest) = Root::of(file);
    if matches!(root, Root::Flat | Root::Repl) {
        return None;
    }
    let stem = rest.strip_suffix(".hd").unwrap_or(rest);
    if root == Root::Source && stem == "lib" {
        return None;
    }
    let mut segs: Vec<&str> = stem.split('/').collect();
    if segs.last() == Some(&"mod") {
        segs.pop();
    }
    segs.into_iter()
        .find(|s| !is_module_ident(s))
        .map(str::to_owned)
}

/// Whether `comp` is an NFC identifier under the source identifier rules.
/// A `$` names the compiler's own segments (`$tests`, `$tasks`, doc-test
/// files), which no source spells, so they are exempt.
fn is_module_ident(comp: &str) -> bool {
    use unicode_ident::{is_xid_continue, is_xid_start};
    use unicode_normalization::UnicodeNormalization;
    if comp.contains('$') {
        return true;
    }
    if comp.is_empty() || !comp.nfc().eq(comp.chars()) {
        return false;
    }
    let mut chars = comp.chars();
    let first = chars.next().expect("nonempty");
    if first != '_' && !is_xid_start(first) {
        return false;
    }
    chars.all(|c| c == '_' || is_xid_continue(c))
}

/// A package-relative file's module path: its package's identifier, then
/// `module_below`.
#[must_use]
pub fn module_path(package: &str, file: &str) -> String {
    rooted_path(&package_ident(package), file)
}

/// A package-relative file's module path below the module root `root`.
/// A module path with each dot-separated component case-folded and then
/// NFC-normalized (`module.path.case-collision`). Folding is the full
/// lowercase mapping plus the folds that differ from it (`ß` to `ss`,
/// final sigma and long s to their plain forms); the `unicode-normalization`
/// crate supplies NFC.
fn folded_path(path: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    path.split('.')
        .map(|part| {
            let mut folded = String::new();
            for c in part.chars() {
                match c {
                    '\u{df}' | '\u{1e9e}' => folded.push_str("ss"),
                    '\u{17f}' => folded.push('s'),
                    '\u{3c2}' => folded.push('\u{3c3}'),
                    _ => folded.extend(c.to_lowercase()),
                }
            }
            folded.nfc().collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(".")
}

fn rooted_path(root: &str, file: &str) -> String {
    let below = module_below(file);
    if below.is_empty() {
        root.to_owned()
    } else {
        format!("{root}.{below}")
    }
}

/// Each package's module root: the identifier form of its name, unique
/// among the packages. Packages are told apart by identity, not by name
/// (`module.dep.identity`, `module.package.no-sharing`), so the second and
/// later packages of one identifier get `IDENT$N`, the `N`th of them, a
/// segment no source spells. The root package claims its identifier
/// first, then the last package (std, in a build), then the others in
/// order.
fn module_roots(packages: &[PackageIn<'_>]) -> Vec<String> {
    let n = packages.len();
    let mut roots = vec![String::new(); n];
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let claims = (0..n.min(1))
        .chain((1..n).rev().take(1))
        .chain(1..n.saturating_sub(1));
    for i in claims {
        let ident = package_ident(packages[i].name);
        let count = seen.entry(ident.clone()).or_insert(0);
        *count += 1;
        roots[i] = if *count == 1 {
            ident
        } else {
            format!("{ident}${count}")
        };
    }
    roots
}

/// The folder path of a module path: everything before its last segment.
/// A module with child modules is in its own folder instead
/// (`module.folder.parent-file`); `ModuleTable` applies that rule.
#[must_use]
pub fn folder_of(module: &str) -> &str {
    module.rsplit_once('.').map_or(module, |(f, _)| f)
}

impl ModuleTable {
    /// Discovery (§4.7) of one package.
    #[must_use]
    pub fn discover(package: &str, sources: &dyn SourceSet) -> Self {
        Self::discover_all(&[PackageIn {
            name: package,
            identity: package,
            sources,
            scope: Scope::All,
            requires: Vec::new(),
            entries: Vec::new(),
            dev_requires: Vec::new(),
        }])
    }

    /// Discovery (§4.7) of the root package (first) and the packages it
    /// reaches: every file in a package's scope is a module, its path from
    /// `module_path`. Its directory is its folder, except that `x.hd`
    /// beside a directory `x/` of source files is in folder `x/`
    /// (`module.folder.parent-file`), and `x/mod.hd` is module `x`.
    #[must_use]
    pub fn discover_all(packages: &[PackageIn<'_>]) -> Self {
        let roots = module_roots(packages);
        let mut t = ModuleTable {
            package: packages.first().map_or("", |p| p.name).to_owned(),
            packages: packages.iter().map(|p| p.name.to_owned()).collect(),
            identities: packages.iter().map(|p| p.identity.to_owned()).collect(),
            requires: packages.iter().map(|p| p.requires.clone()).collect(),
            dev_requires: packages.iter().map(|p| p.dev_requires.clone()).collect(),
            ..Self::default()
        };
        let mut folders: BTreeMap<String, (u16, Vec<ModuleId>)> = BTreeMap::new();
        let mut folded_seen: BTreeMap<String, usize> = BTreeMap::new();
        for (pi, p) in packages.iter().enumerate() {
            let pi = u16::try_from(pi).expect("packages");
            let mut entries = p.sources.list();
            entries.sort_by(|a, b| a.path.cmp(&b.path));
            if p.scope == Scope::Library {
                entries.retain(|e| {
                    matches!(Root::of(&e.path), (Root::Source, rest) if rest != "main.hd")
                        && !e.path.ends_with("_test.hd")
                });
            }
            let mut dirs = std::collections::BTreeSet::new();
            for e in &entries {
                let mut d = e.path.as_str();
                while let Some((parent, _)) = d.rsplit_once('/') {
                    dirs.insert(parent.to_owned());
                    d = parent;
                }
            }
            let root = roots[usize::from(pi)].clone();
            // A file of a package whose name another package shares is
            // named by the package's identity.
            let ident = package_ident(p.name);
            let shown = if packages
                .iter()
                .enumerate()
                .any(|(j, o)| j != usize::from(pi) && package_ident(o.name) == ident)
            {
                p.identity
            } else {
                p.name
            };
            for e in &entries {
                let path = rooted_path(&root, &e.path);
                let executable = Root::of(&e.path).0 == Root::Source
                    && p.entries.contains(&module_below(&e.path));
                let id = ModuleId::from_raw(u32::try_from(t.modules.len()).expect("modules"));
                let stem = e.path.trim_end_matches(".hd");
                let mod_file = e.path == "mod.hd" || e.path.ends_with("/mod.hd");
                // The test unit (`crates/hd_resolve/DESIGN.md` §4.8): every
                // test module of a package, wherever its file lies, is one
                // folder, after the folders its uses reach; so no library
                // folder holds test code (`module.cycle.test-code`).
                let folder = if !executable && role_of(&e.path) == Role::Test {
                    format!("{root}.$tests")
                } else if mod_file || dirs.contains(stem) || repl_input(&e.path).is_some() {
                    path.clone()
                } else {
                    folder_of(&path).to_owned()
                };
                folders.entry(folder).or_insert((pi, Vec::new())).1.push(id);
                let file = FileId::from_raw(id.raw());
                let beside = match Root::of(&e.path) {
                    (Root::Test | Root::Task, rest) => !rest.contains('/') && dirs.contains(stem),
                    _ => false,
                };
                if pi == 0 && e.path == "src/mod.hd" {
                    // `module.path.no-root-mod`.
                    t.problems.push((
                        file,
                        Code::ReservedModuleName,
                        "`src/mod.hd` is reserved: the package root module is `src/lib.hd`; rename it `src/lib.hd`"
                            .to_owned(),
                    ));
                } else if pi == 0 && (e.path == "src/pkg.hd" || e.path == "src/pkg/mod.hd") {
                    // `module.path.reserved-pkg`.
                    t.problems.push((
                        file,
                        Code::ReservedModuleName,
                        format!(
                            "`{}` is reserved: `pkg` names the package root module `src/lib.hd`",
                            e.path
                        ),
                    ));
                } else if beside {
                    // `module.test.integration.beside-dir`, `cli.task.beside-dir`.
                    t.problems.push((
                        file,
                        Code::DuplicateModuleName,
                        format!(
                            "`{}` lies beside the directory `{stem}/`, and both are one module; move it to `{stem}/mod.hd`",
                            e.path
                        ),
                    ));
                } else if role_of(&e.path) == Role::Lib
                    && let Some(bad) = invalid_module_component(&e.path)
                {
                    // `module.path.identifier`: test code keeps its file
                    // names (integration programs have no module path), so
                    // only library modules are checked.
                    t.problems.push((
                        file,
                        Code::InvalidModulePath,
                        format!(
                            "`{}` is not a module path: `{bad}` is not an identifier",
                            e.path
                        ),
                    ));
                }
                let key = folded_path(&path);
                let earlier = folded_seen.get(&key).copied();
                folded_seen.entry(key).or_insert(t.modules.len());
                match t.by_path.entry(path.clone()) {
                    std::collections::btree_map::Entry::Vacant(v) => {
                        v.insert(id);
                        if let Some(first) = earlier {
                            // `module.path.case-collision`.
                            t.problems.push((
                                file,
                                Code::DuplicateModuleName,
                                format!(
                                    "`{}` and `{}` collide after case folding and NFC normalization, so both name one module",
                                    t.sources[first].1, e.path
                                ),
                            ));
                        }
                    }
                    std::collections::btree_map::Entry::Occupied(o) => {
                        // `module.path.unique`; a file reported above, as
                        // beside its directory, is not reported twice.
                        let first = o.get().idx();
                        let flagged = t
                            .problems
                            .iter()
                            .any(|(f, _, _)| f.idx() == first || *f == file);
                        if !flagged {
                            t.problems.push((
                                file,
                                Code::DuplicateModuleName,
                                format!(
                                    "`{}` and `{}` are both module `{path}`",
                                    t.sources[first].1, e.path
                                ),
                            ));
                        }
                    }
                }
                t.files.push(if pi == 0 {
                    e.path.clone()
                } else {
                    format!("<{shown}>/{}", e.path)
                });
                t.sources.push((pi, e.path.clone()));
                let floor = match Root::of(&e.path).0.segment() {
                    Some(seg) => format!("{root}.{seg}"),
                    None => root.clone(),
                };
                t.modules.push(Module {
                    id,
                    file,
                    base: if is_root_file(&e.path) {
                        floor.clone()
                    } else {
                        path.clone()
                    },
                    path,
                    folder: FolderId::NONE,
                    role: if executable {
                        Role::Exe
                    } else {
                        role_of(&e.path)
                    },
                    package: pi,
                    floor,
                    entry: is_entry(&e.path) || executable,
                });
            }
        }
        t.roots = roots;
        for (i, (path, (package, mods))) in folders.into_iter().enumerate() {
            let id = FolderId::from_raw(u32::try_from(i).expect("folders"));
            for &m in &mods {
                t.modules[m.idx()].folder = id;
            }
            t.folders.push(Folder {
                id,
                path,
                modules: mods,
                package,
            });
        }
        t
    }

    #[must_use]
    pub fn module(&self, path: &str) -> Option<ModuleId> {
        self.by_path.get(path).copied()
    }

    /// Whether a `use` may name the module at `path`: it exists and is not
    /// its own program (`module.path.main-no-use`,
    /// `module.test.integration.program-use`).
    #[must_use]
    pub fn usable(&self, path: &str) -> bool {
        self.module(path)
            .is_some_and(|m| !self.modules[m.idx()].entry)
    }

    /// What the use roots of module `m` name.
    #[must_use]
    pub fn use_roots(&self, m: ModuleId) -> UseRoots {
        let module = &self.modules[m.idx()];
        let package = usize::from(module.package);
        // An integration test, a shared test module or a task may use every
        // dev dependency, a unit test module those that do not depend back
        // on the package, other code none (`module.test.dev-dependency`,
        // `module.test.cyclic-dev-unit`, `cli.task.dev-dependencies`).
        let root = Root::of(&self.files[m.idx()]).0;
        let unit_test = root == Root::Source;
        let access = match module.role {
            // A REPL session may use the dev dependencies
            // (`cli.repl.package.dependencies`).
            _ if root == Root::Repl => DevAccess::All,
            Role::Task => DevAccess::All,
            Role::Test if unit_test => DevAccess::Unit,
            Role::Test => DevAccess::All,
            Role::Lib | Role::Exe => DevAccess::None,
        };
        let ident = |p: u16| self.roots[usize::from(p)].clone();
        UseRoots {
            pkg: self.roots[package].clone(),
            base: module.base.clone(),
            floor: module.floor.clone(),
            deps: self.requires[package]
                .iter()
                .map(|(name, p)| (name.clone(), ident(*p)))
                .collect(),
            dev: self
                .dev_requires
                .get(package)
                .into_iter()
                .flatten()
                .map(|(name, p)| (name.clone(), ident(*p), self.reaches(*p, module.package)))
                .collect(),
            access,
        }
    }

    /// The module roots that differ from their package's identifier, each
    /// with that identifier: what user text shows for them.
    #[must_use]
    pub fn renamed_roots(&self) -> Vec<(String, String)> {
        self.roots
            .iter()
            .zip(&self.packages)
            .map(|(root, name)| (root.clone(), package_ident(name)))
            .filter(|(root, ident)| root != ident)
            .collect()
    }

    /// Whether package `from` reaches package `to` through requirements.
    fn reaches(&self, from: u16, to: u16) -> bool {
        let mut seen = vec![false; self.packages.len()];
        let mut stack = vec![from];
        while let Some(p) = stack.pop() {
            if p == to {
                return true;
            }
            if !std::mem::replace(&mut seen[usize::from(p)], true) {
                stack.extend(self.requires[usize::from(p)].iter().map(|(_, q)| *q));
            }
        }
        false
    }

    /// Each cycle of the package graph (`module.cycle.package`): package
    /// indices from its least member, found depth-first in package order.
    #[must_use]
    pub fn package_cycles(&self) -> Vec<Vec<u16>> {
        fn visit(
            p: u16,
            requires: &[Vec<(String, u16)>],
            state: &mut [u8],
            path: &mut Vec<u16>,
            out: &mut Vec<Vec<u16>>,
        ) {
            match state[usize::from(p)] {
                2 => return,
                1 => {
                    let start = path.iter().position(|&q| q == p).unwrap_or(0);
                    let mut cycle = path[start..].to_vec();
                    let least = cycle
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, q)| **q)
                        .map_or(0, |(i, _)| i);
                    cycle.rotate_left(least);
                    if !out.contains(&cycle) {
                        out.push(cycle);
                    }
                    return;
                }
                _ => {}
            }
            state[usize::from(p)] = 1;
            path.push(p);
            for (_, q) in &requires[usize::from(p)] {
                visit(*q, requires, state, path, out);
            }
            path.pop();
            state[usize::from(p)] = 2;
        }
        let mut state = vec![0u8; self.packages.len()];
        let mut out = Vec::new();
        for p in 0..self.packages.len() {
            let p = u16::try_from(p).expect("packages");
            visit(p, &self.requires, &mut state, &mut Vec::new(), &mut out);
        }
        out
    }

    /// The folder of a module path, if the module exists.
    #[must_use]
    pub fn folder_of_module(&self, path: &str) -> Option<FolderId> {
        self.module(path).map(|m| self.modules[m.idx()].folder)
    }

    /// Merges each group of folders into one folder, named after the
    /// group's first folder in path order: folder-cycle recovery, which
    /// resolves the folders of a cycle together as one folder
    /// (`crates/hd_resolve/DESIGN.md` §4.8 rule 5). Groups that share a
    /// folder become one. Folder IDs are renumbered in path order.
    pub fn merge_folders(&mut self, groups: &[Vec<FolderId>]) {
        let n = self.folders.len();
        let mut parent: Vec<usize> = (0..n).collect();
        for g in groups {
            let Some(first) = g.first() else { continue };
            for f in &g[1..] {
                let (a, b) = (find(&mut parent, first.idx()), find(&mut parent, f.idx()));
                // The folder first in path order stays.
                let (keep, gone) = if a < b { (a, b) } else { (b, a) };
                parent[gone] = keep;
            }
        }
        let old = std::mem::take(&mut self.folders);
        let mut new_id = vec![usize::MAX; n];
        for i in 0..n {
            let r = find(&mut parent, i);
            if r == i {
                new_id[i] = self.folders.len();
                let mut f = old[i].clone();
                f.id = FolderId::from_raw(u32::try_from(self.folders.len()).expect("folders"));
                f.modules.clear();
                self.folders.push(f);
            }
        }
        for i in 0..n {
            let target = new_id[find(&mut parent, i)];
            self.folders[target]
                .modules
                .extend(old[i].modules.iter().copied());
        }
        for f in &mut self.folders {
            f.modules.sort_by_key(|m| m.raw());
            for &m in &f.modules {
                self.modules[m.idx()].folder = f.id;
            }
        }
    }

    /// The module a `use` path names: its longest prefix that is a module.
    #[must_use]
    pub fn module_of_use(&self, use_path: &str) -> Option<ModuleId> {
        let mut p = use_path.split(".{").next().unwrap_or(use_path).trim();
        loop {
            if let Some(m) = self.module(p) {
                return Some(m);
            }
            p = p.rsplit_once('.')?.0;
        }
    }
}

/// The representative of `x` in a union-find forest, with path compression.
fn find(parent: &mut [usize], x: usize) -> usize {
    let mut r = x;
    while parent[r] != r {
        r = parent[r];
    }
    let mut y = x;
    while parent[y] != r {
        let next = parent[y];
        parent[y] = r;
        y = next;
    }
    r
}

/// A bit set over the program graph's folders (`crates/hd_sched/DESIGN.md` §6.1).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct FolderSet(Vec<u64>);

impl FolderSet {
    #[must_use]
    pub fn with_capacity(n: usize) -> Self {
        Self(vec![0; n.div_ceil(64)])
    }
    pub fn insert(&mut self, f: FolderId) -> bool {
        let (w, b) = (f.idx() / 64, f.idx() % 64);
        if w >= self.0.len() {
            self.0.resize(w + 1, 0);
        }
        let was = self.0[w] & (1 << b) != 0;
        self.0[w] |= 1 << b;
        !was
    }
    #[must_use]
    pub fn contains(&self, f: FolderId) -> bool {
        self.0
            .get(f.idx() / 64)
            .is_some_and(|w| w & (1 << (f.idx() % 64)) != 0)
    }
    pub fn union(&mut self, other: &FolderSet) {
        if other.0.len() > self.0.len() {
            self.0.resize(other.0.len(), 0);
        }
        for (a, b) in self.0.iter_mut().zip(&other.0) {
            *a |= b;
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = FolderId> + '_ {
        self.0.iter().enumerate().flat_map(|(w, &bits)| {
            (0..64)
                .filter(move |b| bits & (1u64 << b) != 0)
                .map(move |b| FolderId::from_raw(u32::try_from(w * 64 + b).expect("folder")))
        })
    }
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.iter().map(|w| w.count_ones() as usize).sum()
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|w| *w == 0)
    }
}

/// The folder graph (§4.8): use edges between folders, a DAG order and
/// `folder-cycle` diagnostics. Self edges are dropped.
#[derive(Clone, Debug, Default)]
pub struct FolderGraph {
    pub uses: Vec<Vec<FolderId>>,
    /// Folders in dependency order: every folder after the folders it uses.
    pub order: Vec<FolderId>,
    /// Each cycle, named from its first member in path order.
    pub cycles: Vec<Vec<FolderId>>,
    /// `closure(F)`: F plus every folder its uses reach.
    pub closure: Vec<FolderSet>,
    /// Longest path to a sink: the `FolderIface` priority (`crates/hd_sched/DESIGN.md` §6.3).
    pub height: Vec<u32>,
}

impl FolderGraph {
    /// Builds the graph from each module's non-test use paths.
    #[must_use]
    pub fn build(table: &ModuleTable, module_uses: &[Vec<String>]) -> Self {
        let n = table.folders.len();
        let mut g = FolderGraph {
            uses: vec![Vec::new(); n],
            ..Self::default()
        };
        for (m, uses) in module_uses.iter().enumerate() {
            let from = table.modules[m].folder;
            let test_code = table.modules[m].role == Role::Test;
            for u in uses {
                if let Some(target) = table.module_of_use(u) {
                    let to = table.modules[target.idx()].folder;
                    // Only test code may use a test module; another use is
                    // an error of its own and makes no edge.
                    let into_tests = table.modules[target.idx()].role == Role::Test;
                    if to != from && (test_code || !into_tests) {
                        let u = &mut g.uses[from.idx()];
                        if let Err(at) = u.binary_search_by_key(&to.raw(), |f| f.raw()) {
                            u.insert(at, to);
                        }
                    }
                }
            }
        }
        // Depth-first in path order (folders are already path-sorted).
        let mut state = vec![0u8; n]; // 0 new, 1 visiting, 2 done
        let mut stack_path: Vec<FolderId> = Vec::new();
        for f in 0..n {
            g.visit(
                FolderId::from_raw(u32::try_from(f).expect("f")),
                &mut state,
                &mut stack_path,
            );
        }
        g.closure = vec![FolderSet::with_capacity(n); n];
        g.height = vec![0; n];
        for &f in &g.order {
            let mut c = FolderSet::with_capacity(n);
            c.insert(f);
            let mut h = 0;
            for &u in &g.uses[f.idx()] {
                c.union(&g.closure[u.idx()]);
                h = h.max(g.height[u.idx()] + 1);
            }
            g.closure[f.idx()] = c;
            g.height[f.idx()] = h;
        }
        g
    }

    fn visit(&mut self, f: FolderId, state: &mut [u8], path: &mut Vec<FolderId>) {
        match state[f.idx()] {
            2 => return,
            1 => {
                let start = path.iter().position(|&p| p == f).unwrap_or(0);
                let mut cycle = path[start..].to_vec();
                let min = cycle
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, c)| c.raw())
                    .map_or(0, |(i, _)| i);
                cycle.rotate_left(min);
                if !self.cycles.contains(&cycle) {
                    self.cycles.push(cycle);
                }
                return;
            }
            _ => {}
        }
        state[f.idx()] = 1;
        path.push(f);
        let next: Vec<FolderId> = self.uses[f.idx()].clone();
        for u in next {
            self.visit(u, state, path);
        }
        path.pop();
        state[f.idx()] = 2;
        self.order.push(f);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FolderGraph, MemorySources, ModuleTable, PackageIn, Scope, UseRootError, parse_manifest,
    };

    fn collisions(files: &[&str]) -> Vec<(String, hd_diag::Code)> {
        let mut s = MemorySources::default();
        for f in files {
            s.insert(f, "");
        }
        let t = ModuleTable::discover("shop", &s);
        t.problems
            .iter()
            .map(|(f, c, _)| (t.files[f.idx()].clone(), *c))
            .collect()
    }

    /// `module.path.case-collision`: paths that differ only by case.
    #[test]
    fn paths_that_differ_by_case_collide() {
        let dup = hd_diag::Code::DuplicateModuleName;
        assert_eq!(
            collisions(&["src/Foo.hd", "src/foo.hd"]),
            [("src/foo.hd".to_owned(), dup)]
        );
        assert_eq!(
            collisions(&["src/a/B/c.hd", "src/a/b/c.hd"]),
            [("src/a/b/c.hd".to_owned(), dup)]
        );
        assert_eq!(
            collisions(&["tests/Foo.hd", "tests/foo.hd"]),
            [("tests/foo.hd".to_owned(), dup)]
        );
        assert_eq!(
            collisions(&["src/Dir/mod.hd", "src/dir/x.hd", "src/dir.hd"]),
            [("src/dir.hd".to_owned(), dup)]
        );
        assert_eq!(
            collisions(&["src/Stra\u{df}e.hd", "src/strasse.hd"]),
            [("src/strasse.hd".to_owned(), dup)]
        );
        assert!(collisions(&["src/foo.hd", "src/bar.hd", "src/a/foo.hd"]).is_empty());
    }

    /// Composed and decomposed spellings of one name are one module. The
    /// decomposed spelling is also no NFC identifier
    /// (`module.path.identifier`).
    #[test]
    fn composed_and_decomposed_names_collide() {
        assert_eq!(
            collisions(&["src/caf\u{e9}.hd", "src/cafe\u{301}.hd"]),
            [
                (
                    "src/cafe\u{301}.hd".to_owned(),
                    hd_diag::Code::InvalidModulePath
                ),
                (
                    "src/caf\u{e9}.hd".to_owned(),
                    hd_diag::Code::DuplicateModuleName
                )
            ]
        );
    }

    /// An exact duplicate keeps one error, not two.
    #[test]
    fn a_file_beside_its_mod_file_is_one_error() {
        assert_eq!(collisions(&["src/x.hd", "src/x/mod.hd"]).len(), 1);
    }

    #[test]
    fn folders_order_closure_and_cycles() {
        let mut s = MemorySources::default();
        s.insert("main.hd", "");
        s.insert("a/x.hd", "");
        s.insert("b/y.hd", "");
        let t = ModuleTable::discover("pkg", &s);
        assert_eq!(
            t.folders
                .iter()
                .map(|f| f.path.as_str())
                .collect::<Vec<_>>(),
            ["pkg", "pkg.a", "pkg.b"]
        );
        let main = t.module("pkg.main").expect("main").idx();
        let x = t.module("pkg.a.x").expect("x").idx();
        let mut uses = vec![Vec::new(); 3];
        uses[main] = vec!["pkg.a.x.thing".to_owned()];
        uses[x] = vec!["pkg.b.y".to_owned()];
        let g = FolderGraph::build(&t, &uses);
        assert!(g.cycles.is_empty());
        let names: Vec<&str> = g
            .order
            .iter()
            .map(|f| t.folders[f.idx()].path.as_str())
            .collect();
        assert_eq!(names, ["pkg.b", "pkg.a", "pkg"]);
        assert_eq!(g.closure[0].len(), 3);
        assert_eq!(g.height[0], 2);
        let y = t.module("pkg.b.y").expect("y").idx();
        uses[y] = vec!["pkg.main".to_owned()];
        let g = FolderGraph::build(&t, &uses);
        assert_eq!(g.cycles.len(), 1);
        assert_eq!(
            g.cycles[0][0].raw(),
            0,
            "named from the first member in path order"
        );
    }

    #[test]
    fn a_parent_file_lives_in_its_children_folder() {
        let mut s = MemorySources::default();
        s.insert("testing.hd", "");
        s.insert("testing/arbitrary.hd", "");
        s.insert("text.hd", "");
        s.insert("shop/mod.hd", "");
        s.insert("shop/cart.hd", "");
        let t = ModuleTable::discover("std", &s);
        let folder = |m: &str| {
            let id = t.module(m).expect(m);
            t.folders[t.modules[id.idx()].folder.idx()].path.clone()
        };
        assert_eq!(folder("std.testing"), "std.testing");
        assert_eq!(folder("std.testing.arbitrary"), "std.testing");
        assert_eq!(folder("std.text"), "std");
        assert_eq!(folder("std.shop"), "std.shop");
        assert_eq!(folder("std.shop.cart"), "std.shop");
    }

    #[test]
    fn manifest_package_section() {
        let m =
            parse_manifest("[package]\nname = \"shop\"\nversion = \"1.0\"\n").expect("manifest");
        assert_eq!(m.name, "shop");
        assert_eq!(m.unknown, [("package.version".to_owned(), 3)]);
        let w = parse_manifest("[workspace]\nmembers = [\"app\"]\nx = 1\n").expect("manifest");
        assert!(!w.declares_package);
        assert_eq!(w.workspace.map(|w| w.members), Some(vec!["app".to_owned()]));
        assert_eq!(w.unknown, [("workspace.x".to_owned(), 3)]);
        let src = parse_manifest("[source]\nroot = \"lib\"\n").expect("manifest");
        assert_eq!(src.source_root, Some(("lib".to_owned(), 2)));
    }

    #[test]
    fn manifest_requirements() {
        let m = parse_manifest(
            "[package]\nname = \"shop\"\n[dependencies]\nmy-money = { path = \"money\" }\njson = \"github.com/acme/json@2.1.0\"\n[dev-dependencies]\nfx = \"github.com/acme/fx@1.0.0\"\n",
        )
        .expect("manifest");
        let deps: Vec<(String, Option<&str>)> = m
            .dependencies
            .iter()
            .map(|r| (r.name(), r.path.as_deref()))
            .collect();
        assert_eq!(
            deps,
            [
                ("json".to_owned(), None),
                ("my_money".to_owned(), Some("money"))
            ]
        );
        assert_eq!(m.dev_dependencies[0].key, "fx");
    }

    /// Module paths under the source and test roots, and what each root of
    /// a use names from them (`module.path.*`, `module.relative.*`).
    #[test]
    fn module_paths_and_use_roots() {
        let mut shop = MemorySources::default();
        for f in [
            "src/lib.hd",
            "src/main.hd",
            "src/user/mod.hd",
            "src/user/service.hd",
            "src/a.hd",
            "tests/checkout.hd",
            "tests/common/mod.hd",
            "tasks/seed.hd",
        ] {
            shop.insert(f, "");
        }
        let mut money = MemorySources::default();
        money.insert("src/lib.hd", "");
        money.insert("src/main.hd", "");
        money.insert("tests/t.hd", "");
        let t = ModuleTable::discover_all(&[
            PackageIn {
                name: "acme-shop",
                identity: "acme-shop",
                sources: &shop,
                scope: Scope::All,
                requires: vec![("cash".to_owned(), 1)],
                entries: Vec::new(),
                dev_requires: Vec::new(),
            },
            PackageIn {
                name: "money",
                identity: "money",
                sources: &money,
                scope: Scope::Library,
                requires: Vec::new(),
                entries: Vec::new(),
                dev_requires: Vec::new(),
            },
        ]);
        let paths: Vec<&str> = t.modules.iter().map(|m| m.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "acme_shop.a",
                "acme_shop",
                "acme_shop.main",
                "acme_shop.user",
                "acme_shop.user.service",
                "acme_shop.$tasks.seed",
                "acme_shop.$tests.checkout",
                "acme_shop.$tests.common",
                "money",
            ]
        );
        assert!(t.usable("acme_shop.user") && t.usable("acme_shop.$tests.common"));
        assert!(!t.usable("acme_shop.main") && !t.usable("acme_shop.$tests.checkout"));
        let roots = |m: &str| t.use_roots(t.module(m).expect(m));
        let abs = |m: &str, p: &str| {
            roots(m)
                .absolute(&p.split('.').collect::<Vec<_>>())
                .map(|v| v.join("."))
        };
        assert_eq!(
            abs("acme_shop.main", "self.user"),
            Ok("acme_shop.user".into())
        );
        assert_eq!(abs("acme_shop", "pkg.a"), Ok("acme_shop.a".into()));
        assert_eq!(
            abs("acme_shop.main", "super.a"),
            Err(UseRootError::AboveRoot)
        );
        assert_eq!(abs("acme_shop.a", "self.x"), Ok("acme_shop.a.x".into()));
        assert_eq!(
            abs("acme_shop.a", "super.user"),
            Ok("acme_shop.user".into())
        );
        assert_eq!(
            abs("acme_shop.a", "super.super.b"),
            Err(UseRootError::AboveRoot)
        );
        assert_eq!(
            abs("acme_shop.user.service", "super.super.a"),
            Ok("acme_shop.a".into())
        );
        assert_eq!(abs("acme_shop.user", "super.a"), Ok("acme_shop.a".into()));
        assert_eq!(
            abs("acme_shop.$tests.checkout", "self.common"),
            Ok("acme_shop.$tests.common".into())
        );
        assert_eq!(
            abs("acme_shop.$tests.checkout", "super.common"),
            Err(UseRootError::AboveRoot)
        );
        assert_eq!(
            abs("acme_shop.$tests.common", "super.checkout"),
            Ok("acme_shop.$tests.checkout".into())
        );
        assert_eq!(abs("acme_shop.a", "dep.cash"), Ok("money".into()));
        assert_eq!(
            abs("acme_shop.a", "dep.nope.x"),
            Err(UseRootError::UnknownDependency)
        );
        assert_eq!(abs("acme_shop.a", "std.text"), Ok("std.text".into()));
        assert_eq!(
            abs("acme_shop.a", "acme_shop.a"),
            Err(UseRootError::UnknownRoot)
        );
        assert_eq!(
            abs("acme_shop.a", "tests.common"),
            Err(UseRootError::UnknownRoot)
        );
    }

    /// Two packages of one name are two packages (`module.dep.identity`,
    /// `module.package.no-sharing`): each has its own module root, its
    /// dependers reach it by their own keys (`module.dep.key-name`), and
    /// people still read its name.
    #[test]
    fn same_named_packages_get_their_own_roots() {
        let mut lib = MemorySources::default();
        lib.insert("src/lib.hd", "");
        lib.insert("src/rates.hd", "");
        let p = |name, identity, requires| PackageIn {
            name,
            identity,
            sources: &lib,
            scope: Scope::Library,
            requires,
            entries: Vec::new(),
            dev_requires: Vec::new(),
        };
        let t = ModuleTable::discover_all(&[
            p(
                "money",
                "money",
                vec![("money".to_owned(), 1), ("cash".to_owned(), 2)],
            ),
            p("money", "/w/a", Vec::new()),
            p("money", "/w/b", vec![("fx".to_owned(), 3)]),
            p("std", "std", Vec::new()),
        ]);
        assert!(t.problems.is_empty(), "{:?}", t.problems);
        assert_eq!(t.roots, ["money", "money$2", "money$3", "std"]);
        assert!(t.module("money$3.rates").is_some());
        assert_eq!(t.files[2], "</w/a>/src/lib.hd");
        let roots = t.use_roots(t.module("money").expect("root"));
        assert_eq!(roots.pkg, "money");
        assert_eq!(
            roots.absolute(&["dep", "cash", "rates"]),
            Ok(vec!["money$3".to_owned(), "rates".to_owned()])
        );
        assert_eq!(
            t.use_roots(t.module("money$3").expect("b")).deps,
            [("fx".to_owned(), "std".to_owned())]
        );
        assert_eq!(
            super::rename_roots("`money$2.f` and `money$23` x$2", &t.renamed_roots()),
            "`money.f` and `money$23` x$2"
        );
    }

    #[test]
    fn package_cycles_are_found_once() {
        let s = MemorySources::default();
        let p = |name, requires| PackageIn {
            name,
            identity: name,
            sources: &s,
            scope: Scope::Library,
            requires,
            entries: Vec::new(),
            dev_requires: Vec::new(),
        };
        let t = ModuleTable::discover_all(&[
            p("shop", vec![("money".to_owned(), 1)]),
            p("money", vec![("shop".to_owned(), 0), ("fx".to_owned(), 2)]),
            p("fx", Vec::new()),
        ]);
        assert_eq!(t.package_cycles(), [vec![0, 1]]);
    }

    /// `tasks/shared.hd` beside `tasks/shared/` is one error on the file
    /// (`cli.task.beside-dir`); `src/x.hd` beside `src/x/` is the parent
    /// file of folder `x` and no error (`module.folder.parent-file`).
    #[test]
    fn a_task_file_beside_its_directory_is_a_duplicate_module() {
        let mut s = MemorySources::default();
        for f in [
            "src/x.hd",
            "src/x/y.hd",
            "tasks/shared.hd",
            "tasks/shared/mod.hd",
        ] {
            s.insert(f, "");
        }
        let t = ModuleTable::discover("shop", &s);
        let found: Vec<(&str, hd_diag::Code)> = t
            .problems
            .iter()
            .map(|(f, c, _)| (t.files[f.idx()].as_str(), *c))
            .collect();
        assert_eq!(
            found,
            [("tasks/shared.hd", hd_diag::Code::DuplicateModuleName)]
        );
    }

    #[test]
    fn user_text_names_test_and_task_code_by_file() {
        let file_of = |p: &str| (p == "shop.$tests.flow").then(|| "tests/flow.hd".to_owned());
        assert_eq!(
            super::user_text(
                "no module named `shop.$tests` or shop.$tasks.seed.x; see shop.$tests.flow.",
                &file_of
            ),
            "no module named `tests` or tasks/seed/x; see tests/flow.hd."
        );
        assert_eq!(
            super::user_text("`shop.util` is fine", &file_of),
            "`shop.util` is fine"
        );
        assert_eq!(
            super::user_text("(in `shop/$tests/flow/main`)", &file_of),
            "(in `tests/flow/main`)"
        );
    }

    /// Folder-cycle recovery: two folders that use each other become one,
    /// named after the first in path order.
    #[test]
    fn merged_folders_hold_both_modules() {
        let mut s = MemorySources::default();
        for f in ["src/a/x.hd", "src/b/y.hd", "src/c/z.hd"] {
            s.insert(f, "");
        }
        let mut t = ModuleTable::discover("shop", &s);
        let a = t.folder_of_module("shop.a.x").expect("a");
        let b = t.folder_of_module("shop.b.y").expect("b");
        t.merge_folders(&[vec![b, a]]);
        assert_eq!(t.folders.len(), 2);
        assert_eq!(
            t.folder_of_module("shop.a.x"),
            t.folder_of_module("shop.b.y")
        );
        let merged = t.folder_of_module("shop.a.x").expect("merged");
        assert_eq!(t.folders[merged.idx()].path, "shop.a");
        assert_eq!(t.folders[merged.idx()].modules.len(), 2);
        assert_ne!(t.folder_of_module("shop.c.z"), Some(merged));
    }

    /// `module.test.non-test-use.dev-dependency`, `module.test.cyclic-dev-unit`:
    /// what a use may name of the dev dependencies depends on its module.
    #[test]
    fn dev_dependencies_by_kind_of_code() {
        let mut shop = MemorySources::default();
        for f in [
            "src/lib.hd",
            "src/lib_test.hd",
            "tests/flow.hd",
            "tasks/seed.hd",
        ] {
            shop.insert(f, "");
        }
        let mut back = MemorySources::default();
        back.insert("src/lib.hd", "");
        let mut plain = MemorySources::default();
        plain.insert("src/lib.hd", "");
        let t = ModuleTable::discover_all(&[
            PackageIn {
                name: "shop",
                identity: "shop",
                sources: &shop,
                scope: Scope::All,
                requires: Vec::new(),
                entries: Vec::new(),
                dev_requires: vec![("back".to_owned(), 1), ("plain".to_owned(), 2)],
            },
            PackageIn {
                name: "back",
                identity: "back",
                sources: &back,
                scope: Scope::Library,
                requires: vec![("shop".to_owned(), 0)],
                entries: Vec::new(),
                dev_requires: Vec::new(),
            },
            PackageIn {
                name: "plain",
                identity: "plain",
                sources: &plain,
                scope: Scope::Library,
                requires: Vec::new(),
                entries: Vec::new(),
                dev_requires: Vec::new(),
            },
        ]);
        let roots = |p: &str| t.use_roots(t.module(p).expect(p));
        let lib = roots("shop");
        assert_eq!(lib.absolute(&["dep", "plain"]), Err(UseRootError::TestOnly));
        assert_eq!(
            lib.absolute_in_tests(&["dep", "plain"]).as_deref(),
            Ok(&["plain".to_owned()][..])
        );
        assert_eq!(
            lib.absolute_in_tests(&["dep", "back"]),
            Err(UseRootError::CyclicTest)
        );
        assert_eq!(
            roots("shop.lib_test").absolute(&["dep", "back"]),
            Err(UseRootError::CyclicTest)
        );
        assert!(roots("shop.$tests.flow").absolute(&["dep", "back"]).is_ok());
        assert!(roots("shop.$tasks.seed").absolute(&["dep", "back"]).is_ok());
    }
}
