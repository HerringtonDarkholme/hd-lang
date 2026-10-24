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
    CAPABILITY_KEYS, Executable, Grant, HostDeclarations, HostPath, HostProvider, Manifest,
    ManifestError, Problem, Requirement, Version, Workspace, compatibility_line, grant_problem,
    host_path, is_capability_selector, key_problem, parse_manifest, problems, requirement_problem,
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
    /// A synthetic doc test uses the integration-test library view
    /// (`module.test.doc.view`), even when its source is an executable.
    pub doc_test: bool,
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
    /// The path names a runtime module, `std.rt` or one under it, and the
    /// use is not in std (`module.rt.std-only`).
    StdOnly,
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
        if out.first().is_some_and(|s| s == "std")
            && out.get(1).is_some_and(|s| s == "rt")
            && self.pkg != "std"
        {
            return Err(UseRootError::StdOnly);
        }
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
// Generated from Unicode 17.0.0 CaseFolding.txt, statuses C and F only.
// https://www.unicode.org/Public/17.0.0/ucd/CaseFolding.txt
// SHA-256: ff8d8fefbf123574205085d6714c36149eb946d717a0c585c27f0f4ef58c4183
// Single-scalar mappings with equal deltas are coalesced at stride 1 or 2;
// other mappings are stored as expansions. Unlisted scalars map to themselves.
// Unicode 17 matches the locked unicode-normalization tables.
/*
UNICODE LICENSE V3

COPYRIGHT AND PERMISSION NOTICE

Copyright © 1991-2026 Unicode, Inc.

NOTICE TO USER: Carefully read the following legal agreement. BY
DOWNLOADING, INSTALLING, COPYING OR OTHERWISE USING DATA FILES, AND/OR
SOFTWARE, YOU UNEQUIVOCALLY ACCEPT, AND AGREE TO BE BOUND BY, ALL OF THE
TERMS AND CONDITIONS OF THIS AGREEMENT. IF YOU DO NOT AGREE, DO NOT
DOWNLOAD, INSTALL, COPY, DISTRIBUTE OR USE THE DATA FILES OR SOFTWARE.

Permission is hereby granted, free of charge, to any person obtaining a
copy of data files and any associated documentation (the "Data Files") or
software and any associated documentation (the "Software") to deal in the
Data Files or Software without restriction, including without limitation
the rights to use, copy, modify, merge, publish, distribute, and/or sell
copies of the Data Files or Software, and to permit persons to whom the
Data Files or Software are furnished to do so, provided that either (a)
this copyright and permission notice appear with all copies of the Data
Files or Software, or (b) this copyright and permission notice appear in
associated Documentation.

THE DATA FILES AND SOFTWARE ARE PROVIDED "AS IS", WITHOUT WARRANTY OF ANY
KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF
THIRD PARTY RIGHTS.

IN NO EVENT SHALL THE COPYRIGHT HOLDER OR HOLDERS INCLUDED IN THIS NOTICE
BE LIABLE FOR ANY CLAIM, OR ANY SPECIAL INDIRECT OR CONSEQUENTIAL DAMAGES,
OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS,
WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION,
ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THE DATA
FILES OR SOFTWARE.

Except as contained in this notice, the name of a copyright holder shall
not be used in advertising or otherwise to promote the sale, use or other
dealings in these Data Files or Software without prior written
authorization of the copyright holder.
*/
const CASE_FOLD_RANGES: &[(u32, u32, u32, i32)] = &[
    (0x41, 0x5a, 1, 32),
    (0xb5, 0xb5, 1, 775),
    (0xc0, 0xd6, 1, 32),
    (0xd8, 0xde, 1, 32),
    (0x100, 0x12e, 2, 1),
    (0x132, 0x136, 2, 1),
    (0x139, 0x147, 2, 1),
    (0x14a, 0x176, 2, 1),
    (0x178, 0x178, 1, -121),
    (0x179, 0x17d, 2, 1),
    (0x17f, 0x17f, 1, -268),
    (0x181, 0x181, 1, 210),
    (0x182, 0x184, 2, 1),
    (0x186, 0x186, 1, 206),
    (0x187, 0x187, 1, 1),
    (0x189, 0x18a, 1, 205),
    (0x18b, 0x18b, 1, 1),
    (0x18e, 0x18e, 1, 79),
    (0x18f, 0x18f, 1, 202),
    (0x190, 0x190, 1, 203),
    (0x191, 0x191, 1, 1),
    (0x193, 0x193, 1, 205),
    (0x194, 0x194, 1, 207),
    (0x196, 0x196, 1, 211),
    (0x197, 0x197, 1, 209),
    (0x198, 0x198, 1, 1),
    (0x19c, 0x19c, 1, 211),
    (0x19d, 0x19d, 1, 213),
    (0x19f, 0x19f, 1, 214),
    (0x1a0, 0x1a4, 2, 1),
    (0x1a6, 0x1a6, 1, 218),
    (0x1a7, 0x1a7, 1, 1),
    (0x1a9, 0x1a9, 1, 218),
    (0x1ac, 0x1ac, 1, 1),
    (0x1ae, 0x1ae, 1, 218),
    (0x1af, 0x1af, 1, 1),
    (0x1b1, 0x1b2, 1, 217),
    (0x1b3, 0x1b5, 2, 1),
    (0x1b7, 0x1b7, 1, 219),
    (0x1b8, 0x1b8, 1, 1),
    (0x1bc, 0x1bc, 1, 1),
    (0x1c4, 0x1c4, 1, 2),
    (0x1c5, 0x1c5, 1, 1),
    (0x1c7, 0x1c7, 1, 2),
    (0x1c8, 0x1c8, 1, 1),
    (0x1ca, 0x1ca, 1, 2),
    (0x1cb, 0x1db, 2, 1),
    (0x1de, 0x1ee, 2, 1),
    (0x1f1, 0x1f1, 1, 2),
    (0x1f2, 0x1f4, 2, 1),
    (0x1f6, 0x1f6, 1, -97),
    (0x1f7, 0x1f7, 1, -56),
    (0x1f8, 0x21e, 2, 1),
    (0x220, 0x220, 1, -130),
    (0x222, 0x232, 2, 1),
    (0x23a, 0x23a, 1, 10795),
    (0x23b, 0x23b, 1, 1),
    (0x23d, 0x23d, 1, -163),
    (0x23e, 0x23e, 1, 10792),
    (0x241, 0x241, 1, 1),
    (0x243, 0x243, 1, -195),
    (0x244, 0x244, 1, 69),
    (0x245, 0x245, 1, 71),
    (0x246, 0x24e, 2, 1),
    (0x345, 0x345, 1, 116),
    (0x370, 0x372, 2, 1),
    (0x376, 0x376, 1, 1),
    (0x37f, 0x37f, 1, 116),
    (0x386, 0x386, 1, 38),
    (0x388, 0x38a, 1, 37),
    (0x38c, 0x38c, 1, 64),
    (0x38e, 0x38f, 1, 63),
    (0x391, 0x3a1, 1, 32),
    (0x3a3, 0x3ab, 1, 32),
    (0x3c2, 0x3c2, 1, 1),
    (0x3cf, 0x3cf, 1, 8),
    (0x3d0, 0x3d0, 1, -30),
    (0x3d1, 0x3d1, 1, -25),
    (0x3d5, 0x3d5, 1, -15),
    (0x3d6, 0x3d6, 1, -22),
    (0x3d8, 0x3ee, 2, 1),
    (0x3f0, 0x3f0, 1, -54),
    (0x3f1, 0x3f1, 1, -48),
    (0x3f4, 0x3f4, 1, -60),
    (0x3f5, 0x3f5, 1, -64),
    (0x3f7, 0x3f7, 1, 1),
    (0x3f9, 0x3f9, 1, -7),
    (0x3fa, 0x3fa, 1, 1),
    (0x3fd, 0x3ff, 1, -130),
    (0x400, 0x40f, 1, 80),
    (0x410, 0x42f, 1, 32),
    (0x460, 0x480, 2, 1),
    (0x48a, 0x4be, 2, 1),
    (0x4c0, 0x4c0, 1, 15),
    (0x4c1, 0x4cd, 2, 1),
    (0x4d0, 0x52e, 2, 1),
    (0x531, 0x556, 1, 48),
    (0x10a0, 0x10c5, 1, 7264),
    (0x10c7, 0x10c7, 1, 7264),
    (0x10cd, 0x10cd, 1, 7264),
    (0x13f8, 0x13fd, 1, -8),
    (0x1c80, 0x1c80, 1, -6222),
    (0x1c81, 0x1c81, 1, -6221),
    (0x1c82, 0x1c82, 1, -6212),
    (0x1c83, 0x1c84, 1, -6210),
    (0x1c85, 0x1c85, 1, -6211),
    (0x1c86, 0x1c86, 1, -6204),
    (0x1c87, 0x1c87, 1, -6180),
    (0x1c88, 0x1c88, 1, 35267),
    (0x1c89, 0x1c89, 1, 1),
    (0x1c90, 0x1cba, 1, -3008),
    (0x1cbd, 0x1cbf, 1, -3008),
    (0x1e00, 0x1e94, 2, 1),
    (0x1e9b, 0x1e9b, 1, -58),
    (0x1ea0, 0x1efe, 2, 1),
    (0x1f08, 0x1f0f, 1, -8),
    (0x1f18, 0x1f1d, 1, -8),
    (0x1f28, 0x1f2f, 1, -8),
    (0x1f38, 0x1f3f, 1, -8),
    (0x1f48, 0x1f4d, 1, -8),
    (0x1f59, 0x1f5f, 2, -8),
    (0x1f68, 0x1f6f, 1, -8),
    (0x1fb8, 0x1fb9, 1, -8),
    (0x1fba, 0x1fbb, 1, -74),
    (0x1fbe, 0x1fbe, 1, -7173),
    (0x1fc8, 0x1fcb, 1, -86),
    (0x1fd8, 0x1fd9, 1, -8),
    (0x1fda, 0x1fdb, 1, -100),
    (0x1fe8, 0x1fe9, 1, -8),
    (0x1fea, 0x1feb, 1, -112),
    (0x1fec, 0x1fec, 1, -7),
    (0x1ff8, 0x1ff9, 1, -128),
    (0x1ffa, 0x1ffb, 1, -126),
    (0x2126, 0x2126, 1, -7517),
    (0x212a, 0x212a, 1, -8383),
    (0x212b, 0x212b, 1, -8262),
    (0x2132, 0x2132, 1, 28),
    (0x2160, 0x216f, 1, 16),
    (0x2183, 0x2183, 1, 1),
    (0x24b6, 0x24cf, 1, 26),
    (0x2c00, 0x2c2f, 1, 48),
    (0x2c60, 0x2c60, 1, 1),
    (0x2c62, 0x2c62, 1, -10743),
    (0x2c63, 0x2c63, 1, -3814),
    (0x2c64, 0x2c64, 1, -10727),
    (0x2c67, 0x2c6b, 2, 1),
    (0x2c6d, 0x2c6d, 1, -10780),
    (0x2c6e, 0x2c6e, 1, -10749),
    (0x2c6f, 0x2c6f, 1, -10783),
    (0x2c70, 0x2c70, 1, -10782),
    (0x2c72, 0x2c72, 1, 1),
    (0x2c75, 0x2c75, 1, 1),
    (0x2c7e, 0x2c7f, 1, -10815),
    (0x2c80, 0x2ce2, 2, 1),
    (0x2ceb, 0x2ced, 2, 1),
    (0x2cf2, 0x2cf2, 1, 1),
    (0xa640, 0xa66c, 2, 1),
    (0xa680, 0xa69a, 2, 1),
    (0xa722, 0xa72e, 2, 1),
    (0xa732, 0xa76e, 2, 1),
    (0xa779, 0xa77b, 2, 1),
    (0xa77d, 0xa77d, 1, -35332),
    (0xa77e, 0xa786, 2, 1),
    (0xa78b, 0xa78b, 1, 1),
    (0xa78d, 0xa78d, 1, -42280),
    (0xa790, 0xa792, 2, 1),
    (0xa796, 0xa7a8, 2, 1),
    (0xa7aa, 0xa7aa, 1, -42308),
    (0xa7ab, 0xa7ab, 1, -42319),
    (0xa7ac, 0xa7ac, 1, -42315),
    (0xa7ad, 0xa7ad, 1, -42305),
    (0xa7ae, 0xa7ae, 1, -42308),
    (0xa7b0, 0xa7b0, 1, -42258),
    (0xa7b1, 0xa7b1, 1, -42282),
    (0xa7b2, 0xa7b2, 1, -42261),
    (0xa7b3, 0xa7b3, 1, 928),
    (0xa7b4, 0xa7c2, 2, 1),
    (0xa7c4, 0xa7c4, 1, -48),
    (0xa7c5, 0xa7c5, 1, -42307),
    (0xa7c6, 0xa7c6, 1, -35384),
    (0xa7c7, 0xa7c9, 2, 1),
    (0xa7cb, 0xa7cb, 1, -42343),
    (0xa7cc, 0xa7da, 2, 1),
    (0xa7dc, 0xa7dc, 1, -42561),
    (0xa7f5, 0xa7f5, 1, 1),
    (0xab70, 0xabbf, 1, -38864),
    (0xff21, 0xff3a, 1, 32),
    (0x10400, 0x10427, 1, 40),
    (0x104b0, 0x104d3, 1, 40),
    (0x10570, 0x1057a, 1, 39),
    (0x1057c, 0x1058a, 1, 39),
    (0x1058c, 0x10592, 1, 39),
    (0x10594, 0x10595, 1, 39),
    (0x10c80, 0x10cb2, 1, 64),
    (0x10d50, 0x10d65, 1, 32),
    (0x118a0, 0x118bf, 1, 32),
    (0x16e40, 0x16e5f, 1, 32),
    (0x16ea0, 0x16eb8, 1, 27),
    (0x1e900, 0x1e921, 1, 34),
];

const CASE_FOLD_EXPANSIONS: &[(char, &str)] = &[
    ('\u{df}', "\u{73}\u{73}"),
    ('\u{130}', "\u{69}\u{307}"),
    ('\u{149}', "\u{2bc}\u{6e}"),
    ('\u{1f0}', "\u{6a}\u{30c}"),
    ('\u{390}', "\u{3b9}\u{308}\u{301}"),
    ('\u{3b0}', "\u{3c5}\u{308}\u{301}"),
    ('\u{587}', "\u{565}\u{582}"),
    ('\u{1e96}', "\u{68}\u{331}"),
    ('\u{1e97}', "\u{74}\u{308}"),
    ('\u{1e98}', "\u{77}\u{30a}"),
    ('\u{1e99}', "\u{79}\u{30a}"),
    ('\u{1e9a}', "\u{61}\u{2be}"),
    ('\u{1e9e}', "\u{73}\u{73}"),
    ('\u{1f50}', "\u{3c5}\u{313}"),
    ('\u{1f52}', "\u{3c5}\u{313}\u{300}"),
    ('\u{1f54}', "\u{3c5}\u{313}\u{301}"),
    ('\u{1f56}', "\u{3c5}\u{313}\u{342}"),
    ('\u{1f80}', "\u{1f00}\u{3b9}"),
    ('\u{1f81}', "\u{1f01}\u{3b9}"),
    ('\u{1f82}', "\u{1f02}\u{3b9}"),
    ('\u{1f83}', "\u{1f03}\u{3b9}"),
    ('\u{1f84}', "\u{1f04}\u{3b9}"),
    ('\u{1f85}', "\u{1f05}\u{3b9}"),
    ('\u{1f86}', "\u{1f06}\u{3b9}"),
    ('\u{1f87}', "\u{1f07}\u{3b9}"),
    ('\u{1f88}', "\u{1f00}\u{3b9}"),
    ('\u{1f89}', "\u{1f01}\u{3b9}"),
    ('\u{1f8a}', "\u{1f02}\u{3b9}"),
    ('\u{1f8b}', "\u{1f03}\u{3b9}"),
    ('\u{1f8c}', "\u{1f04}\u{3b9}"),
    ('\u{1f8d}', "\u{1f05}\u{3b9}"),
    ('\u{1f8e}', "\u{1f06}\u{3b9}"),
    ('\u{1f8f}', "\u{1f07}\u{3b9}"),
    ('\u{1f90}', "\u{1f20}\u{3b9}"),
    ('\u{1f91}', "\u{1f21}\u{3b9}"),
    ('\u{1f92}', "\u{1f22}\u{3b9}"),
    ('\u{1f93}', "\u{1f23}\u{3b9}"),
    ('\u{1f94}', "\u{1f24}\u{3b9}"),
    ('\u{1f95}', "\u{1f25}\u{3b9}"),
    ('\u{1f96}', "\u{1f26}\u{3b9}"),
    ('\u{1f97}', "\u{1f27}\u{3b9}"),
    ('\u{1f98}', "\u{1f20}\u{3b9}"),
    ('\u{1f99}', "\u{1f21}\u{3b9}"),
    ('\u{1f9a}', "\u{1f22}\u{3b9}"),
    ('\u{1f9b}', "\u{1f23}\u{3b9}"),
    ('\u{1f9c}', "\u{1f24}\u{3b9}"),
    ('\u{1f9d}', "\u{1f25}\u{3b9}"),
    ('\u{1f9e}', "\u{1f26}\u{3b9}"),
    ('\u{1f9f}', "\u{1f27}\u{3b9}"),
    ('\u{1fa0}', "\u{1f60}\u{3b9}"),
    ('\u{1fa1}', "\u{1f61}\u{3b9}"),
    ('\u{1fa2}', "\u{1f62}\u{3b9}"),
    ('\u{1fa3}', "\u{1f63}\u{3b9}"),
    ('\u{1fa4}', "\u{1f64}\u{3b9}"),
    ('\u{1fa5}', "\u{1f65}\u{3b9}"),
    ('\u{1fa6}', "\u{1f66}\u{3b9}"),
    ('\u{1fa7}', "\u{1f67}\u{3b9}"),
    ('\u{1fa8}', "\u{1f60}\u{3b9}"),
    ('\u{1fa9}', "\u{1f61}\u{3b9}"),
    ('\u{1faa}', "\u{1f62}\u{3b9}"),
    ('\u{1fab}', "\u{1f63}\u{3b9}"),
    ('\u{1fac}', "\u{1f64}\u{3b9}"),
    ('\u{1fad}', "\u{1f65}\u{3b9}"),
    ('\u{1fae}', "\u{1f66}\u{3b9}"),
    ('\u{1faf}', "\u{1f67}\u{3b9}"),
    ('\u{1fb2}', "\u{1f70}\u{3b9}"),
    ('\u{1fb3}', "\u{3b1}\u{3b9}"),
    ('\u{1fb4}', "\u{3ac}\u{3b9}"),
    ('\u{1fb6}', "\u{3b1}\u{342}"),
    ('\u{1fb7}', "\u{3b1}\u{342}\u{3b9}"),
    ('\u{1fbc}', "\u{3b1}\u{3b9}"),
    ('\u{1fc2}', "\u{1f74}\u{3b9}"),
    ('\u{1fc3}', "\u{3b7}\u{3b9}"),
    ('\u{1fc4}', "\u{3ae}\u{3b9}"),
    ('\u{1fc6}', "\u{3b7}\u{342}"),
    ('\u{1fc7}', "\u{3b7}\u{342}\u{3b9}"),
    ('\u{1fcc}', "\u{3b7}\u{3b9}"),
    ('\u{1fd2}', "\u{3b9}\u{308}\u{300}"),
    ('\u{1fd3}', "\u{3b9}\u{308}\u{301}"),
    ('\u{1fd6}', "\u{3b9}\u{342}"),
    ('\u{1fd7}', "\u{3b9}\u{308}\u{342}"),
    ('\u{1fe2}', "\u{3c5}\u{308}\u{300}"),
    ('\u{1fe3}', "\u{3c5}\u{308}\u{301}"),
    ('\u{1fe4}', "\u{3c1}\u{313}"),
    ('\u{1fe6}', "\u{3c5}\u{342}"),
    ('\u{1fe7}', "\u{3c5}\u{308}\u{342}"),
    ('\u{1ff2}', "\u{1f7c}\u{3b9}"),
    ('\u{1ff3}', "\u{3c9}\u{3b9}"),
    ('\u{1ff4}', "\u{3ce}\u{3b9}"),
    ('\u{1ff6}', "\u{3c9}\u{342}"),
    ('\u{1ff7}', "\u{3c9}\u{342}\u{3b9}"),
    ('\u{1ffc}', "\u{3c9}\u{3b9}"),
    ('\u{fb00}', "\u{66}\u{66}"),
    ('\u{fb01}', "\u{66}\u{69}"),
    ('\u{fb02}', "\u{66}\u{6c}"),
    ('\u{fb03}', "\u{66}\u{66}\u{69}"),
    ('\u{fb04}', "\u{66}\u{66}\u{6c}"),
    ('\u{fb05}', "\u{73}\u{74}"),
    ('\u{fb06}', "\u{73}\u{74}"),
    ('\u{fb13}', "\u{574}\u{576}"),
    ('\u{fb14}', "\u{574}\u{565}"),
    ('\u{fb15}', "\u{574}\u{56b}"),
    ('\u{fb16}', "\u{57e}\u{576}"),
    ('\u{fb17}', "\u{574}\u{56d}"),
];

fn fold_case(c: char, out: &mut String) {
    if let Ok(i) = CASE_FOLD_EXPANSIONS.binary_search_by_key(&c, |&(c, _)| c) {
        out.push_str(CASE_FOLD_EXPANSIONS[i].1);
        return;
    }
    let scalar = u32::from(c);
    let at = CASE_FOLD_RANGES.partition_point(|&(start, _, _, _)| start <= scalar);
    if let Some(i) = at.checked_sub(1) {
        let (start, end, stride, delta) = CASE_FOLD_RANGES[i];
        if scalar <= end && (scalar - start).is_multiple_of(stride) {
            out.push(
                char::from_u32(scalar.wrapping_add_signed(delta))
                    .expect("case folding maps to Unicode scalars"),
            );
            return;
        }
    }
    out.push(c);
}

/// Full default Unicode case folding followed by NFC per component
/// (`module.path.case-collision`). Turkic-only mappings are excluded.
fn folded_path(path: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    path.split('.')
        .map(|part| {
            let mut folded = String::new();
            for c in part.chars() {
                fold_case(c, &mut folded);
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
                    // `module.test.integration.beside-directory`, `cli.task.beside-directory`.
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
            doc_test: module.path.contains(".$tests.$doc"),
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
    /// Each strongly connected component of more than one folder, its
    /// members in path order (§4.8 rules 3 and 4).
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
        let mut t = Tarjan {
            index: vec![u32::MAX; n],
            low: vec![0; n],
            on_stack: vec![false; n],
            stack: Vec::new(),
            next: 0,
        };
        for f in 0..n {
            let f = FolderId::from_raw(u32::try_from(f).expect("f"));
            g.visit(f, &mut state);
            if t.index[f.idx()] == u32::MAX {
                g.strong(f, &mut t);
            }
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

    fn visit(&mut self, f: FolderId, state: &mut [u8]) {
        if state[f.idx()] != 0 {
            return;
        }
        state[f.idx()] = 1;
        let next: Vec<FolderId> = self.uses[f.idx()].clone();
        for u in next {
            self.visit(u, state);
        }
        state[f.idx()] = 2;
        self.order.push(f);
    }

    /// Tarjan's algorithm from `f` (§4.8 rule 3): each strongly connected
    /// component of more than one folder becomes one cycle, its members in
    /// path order.
    fn strong(&mut self, f: FolderId, t: &mut Tarjan) {
        let v = f.idx();
        t.index[v] = t.next;
        t.low[v] = t.next;
        t.next += 1;
        t.stack.push(v);
        t.on_stack[v] = true;
        for i in 0..self.uses[v].len() {
            let u = self.uses[v][i];
            let w = u.idx();
            if t.index[w] == u32::MAX {
                self.strong(u, t);
                t.low[v] = t.low[v].min(t.low[w]);
            } else if t.on_stack[w] {
                t.low[v] = t.low[v].min(t.index[w]);
            }
        }
        if t.low[v] == t.index[v] {
            let mut scc = Vec::new();
            while let Some(w) = t.stack.pop() {
                t.on_stack[w] = false;
                scc.push(FolderId::from_raw(u32::try_from(w).expect("w")));
                if w == v {
                    break;
                }
            }
            if scc.len() > 1 {
                scc.sort_by_key(|g| g.raw());
                self.cycles.push(scc);
            }
        }
    }
}

/// The state of Tarjan's algorithm over the folder graph.
struct Tarjan {
    index: Vec<u32>,
    low: Vec<u32>,
    on_stack: Vec<bool>,
    stack: Vec<usize>,
    next: u32,
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

    #[test]
    fn paths_use_full_default_unicode_case_folding() {
        for (left, right) in [
            ("shop.Foo", "shop.foo"),
            ("shop.Σςσ", "shop.σσσ"),
            ("shop.Straße", "shop.STRASSE"),
            ("shop.ŉ", "shop.ʼn"),
            ("shop.ﬃ", "shop.ffi"),
            ("shop.ΐ", "shop.ι\u{308}\u{301}"),
            ("shop.Ꭰ", "shop.ꭰ"),
        ] {
            assert_eq!(super::folded_path(left), super::folded_path(right));
        }
        // In-memory discovery avoids the host filesystem merging these
        // two distinct spellings before the compiler sees them.
        assert_eq!(
            collisions(&["src/ŉ.hd", "src/ʼn.hd"]),
            [("src/ʼn.hd".to_owned(), hd_diag::Code::DuplicateModuleName)]
        );
        assert_ne!(super::folded_path("shop.I"), super::folded_path("shop.ı"));
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
    /// (`cli.task.beside-directory`); `src/x.hd` beside `src/x/` is the parent
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

    /// Only std may use a runtime module (`module.rt.std-only`): `std.rt`
    /// and the modules under it, by any path.
    #[test]
    fn only_std_uses_a_runtime_module() {
        let roots = |pkg: &str| super::UseRoots {
            pkg: pkg.to_owned(),
            base: pkg.to_owned(),
            floor: pkg.to_owned(),
            ..super::UseRoots::default()
        };
        let app = roots("app");
        for path in [&["std", "rt"][..], &["std", "rt", "memory", "load_u8"]] {
            assert_eq!(app.absolute(path), Err(UseRootError::StdOnly), "{path:?}");
            assert_eq!(app.absolute_in_tests(path), Err(UseRootError::StdOnly));
        }
        assert!(app.absolute(&["std", "text", "rt"]).is_ok());
        assert!(app.absolute(&["std", "rtx"]).is_ok());
        assert_eq!(
            roots("std").absolute(&["std", "rt", "memory"]).as_deref(),
            Ok(&["std".to_owned(), "rt".to_owned(), "memory".to_owned()][..])
        );
        let rt = super::UseRoots {
            base: "std.rt".to_owned(),
            ..roots("std")
        };
        assert_eq!(
            rt.absolute(&["self", "memory"]).as_deref(),
            Ok(&["std".to_owned(), "rt".to_owned(), "memory".to_owned()][..])
        );
    }
}
