#![forbid(unsafe_code)]
//! `hd_project`: manifests, module discovery and identity, folders, the
//! folder graph and the `SourceSet` trait (design-overview.md §2.1,
//! resolution-and-interfaces.md §4.7 and §4.8).
//!
//! No file system access here (§2.2 rule 3): sources come through
//! `SourceSet`, which the CLI implements over the disk and `hd_web` over JS.

pub mod edit;
mod manifest;

use std::collections::BTreeMap;
use std::sync::Arc;

use hd_base::{FileId, FolderId, ModuleId};
use hd_diag::Code;

pub use manifest::{
    CAPABILITY_KEYS, Executable, Grant, Manifest, Problem, Requirement, Version, Workspace,
    compatibility_line, grant_problem, parse_manifest, problems,
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

/// The role a file plays (cache.md §5.3).
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
    /// A package without that layout, as std or a single-file program:
    /// its files sit directly under the package directory.
    Flat,
}

impl Root {
    /// The root of a package-relative file, and the file's path below it.
    #[must_use]
    pub fn of(file: &str) -> (Root, &str) {
        if let Some(rest) = file.strip_prefix("src/") {
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
            Root::Source | Root::Flat => None,
        }
    }
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

/// A package-relative file's module path below its package root module
/// (`module.path.*`): `src/user/types.hd` is `user.types`,
/// `src/user/mod.hd` is `user`, `src/lib.hd` is the root itself (empty),
/// `tests/checkout.hd` is `$tests.checkout`, and a flat `main.hd` is
/// `main`.
#[must_use]
pub fn module_below(file: &str) -> String {
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
    /// The manifest name; module paths start with its `package_ident`.
    pub name: &'a str,
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
}

impl UseRoots {
    /// A use path with its root replaced by the module path it names.
    pub fn absolute<S: AsRef<str>>(&self, path: &[S]) -> Result<Vec<String>, UseRootError> {
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
                let (_, root) = self
                    .deps
                    .iter()
                    .find(|(n, _)| n == name.as_ref())
                    .ok_or(UseRootError::UnknownDependency)?;
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

/// A package-relative file's module path: its package's identifier, then
/// `module_below`.
#[must_use]
pub fn module_path(package: &str, file: &str) -> String {
    let below = module_below(file);
    let ident = package_ident(package);
    if below.is_empty() {
        ident
    } else {
        format!("{ident}.{below}")
    }
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
        let mut t = ModuleTable {
            package: packages.first().map_or("", |p| p.name).to_owned(),
            packages: packages.iter().map(|p| p.name.to_owned()).collect(),
            requires: packages.iter().map(|p| p.requires.clone()).collect(),
            dev_requires: packages.iter().map(|p| p.dev_requires.clone()).collect(),
            ..Self::default()
        };
        let mut folders: BTreeMap<String, (u16, Vec<ModuleId>)> = BTreeMap::new();
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
            let root = package_ident(p.name);
            for e in &entries {
                let path = module_path(p.name, &e.path);
                let executable = Root::of(&e.path).0 == Root::Source
                    && p.entries.contains(&module_below(&e.path));
                let id = ModuleId::from_raw(u32::try_from(t.modules.len()).expect("modules"));
                let stem = e.path.trim_end_matches(".hd");
                let mod_file = e.path == "mod.hd" || e.path.ends_with("/mod.hd");
                let folder = if mod_file || dirs.contains(stem) {
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
                }
                match t.by_path.entry(path.clone()) {
                    std::collections::btree_map::Entry::Vacant(v) => {
                        v.insert(id);
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
                    format!("<{}>/{}", p.name, e.path)
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
        // Test modules, integration tests and tasks see the dev
        // dependencies too (`module.test.dev-dependency`,
        // `cli.task.dev-dependencies`), except that a unit test module never
        // sees one that depends back on the package
        // (`module.test.cyclic-dev-unit`). Other code does not see them.
        let unit_test = Root::of(&self.files[m.idx()]).0 == Root::Source;
        let dev =
            self.dev_requires
                .get(package)
                .into_iter()
                .flatten()
                .filter(|(_, p)| match module.role {
                    Role::Task => true,
                    Role::Test => !(unit_test && self.reaches(*p, module.package)),
                    Role::Lib | Role::Exe => false,
                });
        UseRoots {
            pkg: package_ident(&self.packages[package]),
            base: module.base.clone(),
            floor: module.floor.clone(),
            deps: self.requires[package]
                .iter()
                .chain(dev)
                .map(|(name, p)| (name.clone(), package_ident(&self.packages[usize::from(*p)])))
                .collect(),
        }
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

/// A bit set over the program graph's folders (scheduler.md §6.1).
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
    /// Longest path to a sink: the `FolderIface` priority (scheduler.md §6.3).
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
            for u in uses {
                if let Some(target) = table.module_of_use(u) {
                    let to = table.modules[target.idx()].folder;
                    if to != from {
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
        assert!(parse_manifest("[source]\nroot = \"lib\"\n").is_err());
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
                sources: &shop,
                scope: Scope::All,
                requires: vec![("cash".to_owned(), 1)],
                entries: Vec::new(),
                dev_requires: Vec::new(),
            },
            PackageIn {
                name: "money",
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

    #[test]
    fn package_cycles_are_found_once() {
        let s = MemorySources::default();
        let p = |name, requires| PackageIn {
            name,
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
}
