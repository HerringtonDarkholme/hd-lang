#![forbid(unsafe_code)]
//! `hd_project`: manifests, module discovery and identity, folders, the
//! folder graph and the `SourceSet` trait (design-overview.md §2.1,
//! resolution-and-interfaces.md §4.7 and §4.8).
//!
//! No file system access here (§2.2 rule 3): sources come through
//! `SourceSet`, which the CLI implements over the disk and `hd_web` over JS.

use std::collections::BTreeMap;
use std::sync::Arc;

use hd_base::{FileId, FolderId, Hash128, ModuleId, NotImplemented, Stage, StageResult};

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
        self.files.insert(path.to_owned(), Arc::from(text.as_bytes()));
    }
}

impl SourceSet for MemorySources {
    fn list(&self) -> Vec<SourceEntry> {
        self.files.iter().map(|(p, b)| SourceEntry { path: p.clone(), size: b.len() as u64 }).collect()
    }
    fn read(&self, path: &str) -> Option<Arc<[u8]>> {
        self.files.get(path).cloned()
    }
}

/// The manifest's package section (`hd.toml`). The design parses it with
/// `toml`; this skeleton reads only `name = "..."` and `version = "..."`
/// lines (architecture skeleton, SK-8).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Manifest {
    pub name: String,
    pub version: Option<String>,
    pub dependencies: Vec<(String, String)>,
}

pub fn parse_manifest(text: &str) -> StageResult<Manifest> {
    let mut m = Manifest::default();
    let mut section = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            line.trim_matches(['[', ']']).clone_into(&mut section);
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let (k, v) = (k.trim(), v.trim().trim_matches('"'));
        match (section.as_str(), k) {
            ("package" | "", "name") => v.clone_into(&mut m.name),
            ("package" | "", "version") => m.version = Some(v.to_owned()),
            ("dependencies", _) => m.dependencies.push((k.to_owned(), v.to_owned())),
            ("package" | "", _) => {}
            (other, _) => return Err(NotImplemented::new(Stage::Discover, format!("manifest section [{other}]"))),
        }
    }
    Ok(m)
}

/// The role a file plays (cache.md §5.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    Lib,
    Exe,
    Test,
    Task,
}

/// One module: a file. Dense IDs; stable form is the module path.
#[derive(Clone, Debug)]
pub struct Module {
    pub id: ModuleId,
    pub file: FileId,
    pub path: String,
    pub folder: FolderId,
    pub role: Role,
}

/// One folder: the interface unit.
#[derive(Clone, Debug)]
pub struct Folder {
    pub id: FolderId,
    pub path: String,
    pub modules: Vec<ModuleId>,
}

/// Modules and folders of one package, in path order (§4.7).
#[derive(Clone, Debug, Default)]
pub struct ModuleTable {
    pub package: String,
    pub files: Vec<String>,
    pub modules: Vec<Module>,
    pub folders: Vec<Folder>,
    by_path: BTreeMap<String, ModuleId>,
}

/// The module path of a file: `package.` plus its path, `/` as `.`.
#[must_use]
pub fn module_path(package: &str, file: &str) -> String {
    format!("{package}.{}", file.trim_end_matches(".hd").replace('/', "."))
}

/// The folder path of a module path: everything before its last segment.
#[must_use]
pub fn folder_of(module: &str) -> &str {
    module.rsplit_once('.').map_or(module, |(f, _)| f)
}

impl ModuleTable {
    /// Discovery (§4.7): every `.hd` file is a module; its directory is its folder.
    #[must_use]
    pub fn discover(package: &str, sources: &dyn SourceSet) -> Self {
        let mut t = ModuleTable { package: package.to_owned(), ..Self::default() };
        let mut folders: BTreeMap<String, Vec<ModuleId>> = BTreeMap::new();
        let mut entries = sources.list();
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        for (i, e) in entries.iter().enumerate() {
            let path = module_path(package, &e.path);
            let id = ModuleId::from_raw(u32::try_from(i).expect("modules"));
            folders.entry(folder_of(&path).to_owned()).or_default().push(id);
            t.by_path.insert(path.clone(), id);
            t.files.push(e.path.clone());
            let role = if e.path.ends_with("_test.hd") { Role::Test } else { Role::Lib };
            t.modules.push(Module { id, file: FileId::from_raw(id.raw()), path, folder: FolderId::NONE, role });
        }
        for (i, (path, mods)) in folders.into_iter().enumerate() {
            let id = FolderId::from_raw(u32::try_from(i).expect("folders"));
            for &m in &mods {
                t.modules[m.idx()].folder = id;
            }
            t.folders.push(Folder { id, path, modules: mods });
        }
        t
    }

    #[must_use]
    pub fn module(&self, path: &str) -> Option<ModuleId> {
        self.by_path.get(path).copied()
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
        self.0.get(f.idx() / 64).is_some_and(|w| w & (1 << (f.idx() % 64)) != 0)
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
            (0..64).filter(move |b| bits & (1u64 << b) != 0).map(move |b| FolderId::from_raw(u32::try_from(w * 64 + b).expect("folder")))
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
        let mut g = FolderGraph { uses: vec![Vec::new(); n], ..Self::default() };
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
            g.visit(FolderId::from_raw(u32::try_from(f).expect("f")), &mut state, &mut stack_path);
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
                let min = cycle.iter().enumerate().min_by_key(|(_, c)| c.raw()).map_or(0, |(i, _)| i);
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

/// The package graph: the root package and its resolved dependencies. The
/// first release resolves path and host dependencies (§4.7); this skeleton
/// holds only the root.
#[derive(Clone, Debug, Default)]
pub struct PackageGraph {
    pub root: String,
    pub dependencies: Vec<(String, Hash128)>,
}

pub fn resolve_packages(manifest: &Manifest) -> StageResult<PackageGraph> {
    if manifest.dependencies.is_empty() {
        Ok(PackageGraph { root: manifest.name.clone(), dependencies: Vec::new() })
    } else {
        Err(NotImplemented::new(Stage::Discover, "package dependencies"))
    }
}

#[cfg(test)]
mod tests {
    use super::{FolderGraph, MemorySources, ModuleTable, parse_manifest};

    #[test]
    fn folders_order_closure_and_cycles() {
        let mut s = MemorySources::default();
        s.insert("main.hd", "");
        s.insert("a/x.hd", "");
        s.insert("b/y.hd", "");
        let t = ModuleTable::discover("pkg", &s);
        assert_eq!(t.folders.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), ["pkg", "pkg.a", "pkg.b"]);
        let main = t.module("pkg.main").expect("main").idx();
        let x = t.module("pkg.a.x").expect("x").idx();
        let mut uses = vec![Vec::new(); 3];
        uses[main] = vec!["pkg.a.x.thing".to_owned()];
        uses[x] = vec!["pkg.b.y".to_owned()];
        let g = FolderGraph::build(&t, &uses);
        assert!(g.cycles.is_empty());
        let names: Vec<&str> = g.order.iter().map(|f| t.folders[f.idx()].path.as_str()).collect();
        assert_eq!(names, ["pkg.b", "pkg.a", "pkg"]);
        assert_eq!(g.closure[0].len(), 3);
        assert_eq!(g.height[0], 2);
        let y = t.module("pkg.b.y").expect("y").idx();
        uses[y] = vec!["pkg.main".to_owned()];
        let g = FolderGraph::build(&t, &uses);
        assert_eq!(g.cycles.len(), 1);
        assert_eq!(g.cycles[0][0].raw(), 0, "named from the first member in path order");
    }

    #[test]
    fn manifest_package_section() {
        let m = parse_manifest("[package]\nname = \"shop\"\nversion = \"1.0\"\n").expect("manifest");
        assert_eq!(m.name, "shop");
        assert!(parse_manifest("[workspace]\nx = 1\n").is_err());
    }
}
