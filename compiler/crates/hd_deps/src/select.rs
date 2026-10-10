//! Minimal version selection (`module.select.*`): it starts at the package,
//! or at every member of its workspace (`module.workspace.selection`),
//! reads the manifest of every local package that a path requirement
//! reaches (`module.select.path`) and of every version that a dependency
//! requirement reaches (`module.select.reach`, `cli.dep.reached`), and
//! selects the largest minimum of each host path and compatibility line
//! (`module.select.largest`, `module.select.one-per-line`). Each manifest
//! it reads is checked against its `hd.sum` manifest line first
//! (`cli.dep.verify.manifest`), and each selected tree against its tree
//! line (`cli.dep.verify`).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use hd_diag::{Code, Severity};
use hd_project::{
    HostPath, Manifest, Requirement, Version, compatibility_line, host_path, parse_manifest,
    problems,
};

use crate::cache::Entry;
use crate::sum::{Sum, manifest_hash, manifest_key, tree_key};

/// A line of a local package's `hd.toml`, where a problem is reported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    /// The directory of the package whose `hd.toml` holds the line.
    pub dir: PathBuf,
    pub line: u32,
}

/// A dependency problem: its code (`None` for one no rule names, as an
/// unreadable cache), its place, and its message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub code: Option<Code>,
    pub place: Place,
    pub message: String,
}

/// What a missing `hd.sum` line is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// An error (`cli.dep.missing-sum`, `cli.dep.missing-sum.manifest`),
    /// as in `hd check`, `hd build`, `hd run` and `hd test`.
    Verify,
    /// A line to add, as in `hd fetch` (`cli.dep.fetch`).
    Record,
}

/// Where versions come from: the cache, or a fetch into it.
pub trait Source {
    /// The entry of `version` of the package at `host`, or the code and
    /// message of why there is none.
    fn entry(
        &mut self,
        host: &HostPath,
        version: &Version,
    ) -> Result<Entry, (Option<Code>, String)>;
}

/// A package of the selected graph.
#[derive(Clone, Debug)]
pub struct Package {
    pub dir: PathBuf,
    pub manifest: Manifest,
    /// A fetched version's `HOST_PATH@VERSION`; `None` for a local package.
    pub fetched: Option<String>,
    /// Each requirement's source name and the index of its package.
    pub requires: Vec<(String, usize)>,
    /// The dev requirements of a member, in the same form; empty for any
    /// other package (`module.select.dev-dependencies`).
    pub dev_requires: Vec<(String, usize)>,
}

/// The selected graph and its `hd.sum` lines.
#[derive(Clone, Debug, Default)]
pub struct Selection {
    /// The members first, in the order given, then the other local
    /// packages, then the selected versions.
    pub packages: Vec<Package>,
    /// The `hd.sum` lines of the whole selection: the existing entries, or
    /// the hashes of versions they lack.
    pub sums: BTreeMap<String, String>,
    pub problems: Vec<Problem>,
}

#[derive(Clone, Debug)]
enum Target {
    /// A local package, by its canonical directory.
    Local(PathBuf),
    Host(HostPath, Version),
}

#[derive(Clone, Debug)]
struct Edge {
    name: String,
    dev: bool,
    target: Target,
    at: Place,
}

struct Node {
    dir: PathBuf,
    manifest: Manifest,
    /// A fetched version's host path, version and recorded tree hash.
    fetched: Option<(HostPath, Version, String)>,
    edges: Vec<Edge>,
}

/// The manifest in `dir` when it declares a package.
fn package_manifest(dir: &Path) -> Option<Manifest> {
    let text = std::fs::read_to_string(dir.join("hd.toml")).ok()?;
    parse_manifest(&text)
        .ok()
        .filter(|m| m.declares_package && !m.name.is_empty())
}

/// The lines of a manifest's requirements that break a rule of the
/// manifest alone, with the message, and a too-old toolchain
/// (`cli.dep.invalid`, `module.toolchain.graph-minimum`).
fn requirement_errors(m: &Manifest, toolchain: &str) -> Vec<(Option<u32>, Code, String)> {
    problems(m, toolchain)
        .into_iter()
        .filter(|p| p.severity == Severity::Error)
        .filter_map(|p| match p.code {
            Some(c @ (Code::InvalidRequirement | Code::ToolchainTooOld)) => {
                Some((p.line, c, p.message))
            }
            _ => None,
        })
        .collect()
}

/// The host path and version of a dependency requirement.
fn host_target(r: &Requirement) -> Option<Target> {
    let (path, version) = r.host()?;
    Some(Target::Host(host_path(path).ok()?, version))
}

/// The edges of a local package: each valid requirement, its dev
/// requirements too when it is a member. A path requirement whose
/// directory holds no package is left out; the command reports it at the
/// package it builds (`module.path-dep.no-package`).
fn local_edges(dir: &Path, m: &Manifest, member: bool, toolchain: &str) -> Vec<Edge> {
    let errors = requirement_errors(m, toolchain);
    let dev: &[Requirement] = if member { &m.dev_dependencies } else { &[] };
    let tagged = m
        .dependencies
        .iter()
        .map(|r| (false, r))
        .chain(dev.iter().map(|r| (true, r)));
    let mut out = Vec::new();
    for (is_dev, r) in tagged {
        if errors.iter().any(|(line, _, _)| *line == Some(r.line)) {
            continue;
        }
        let target = match &r.path {
            Some(p) => std::fs::canonicalize(dir.join(p))
                .ok()
                .filter(|d| package_manifest(d).is_some())
                .map(Target::Local),
            None => host_target(r),
        };
        if let Some(target) = target {
            out.push(Edge {
                name: r.name(),
                dev: is_dev,
                target,
                at: Place {
                    dir: dir.to_path_buf(),
                    line: r.line,
                },
            });
        }
    }
    out
}

/// The dependency requirement that a fetched version's path requirement
/// stands for: its version, at the host path of the other directory of
/// the same repository (`module.workspace.path-version.fetched-version`,
/// `module.workspace.path-version.fetched-host`); or why it has none.
fn fetched_path_target(host: &HostPath, shown: &str, r: &Requirement) -> Result<Target, String> {
    let (Some(dir), Some(version)) = (&r.path, &r.version) else {
        return Err(format!(
            "{shown} requires `{}` by a path with no version, so this version cannot be fetched as a dependency",
            r.key
        ));
    };
    let mut segments: Vec<&str> = host
        .subdirectory
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    for s in dir.split('/') {
        match s {
            "" | "." => {}
            ".." => {
                if segments.pop().is_none() {
                    return Err(format!(
                        "{shown} requires `{}` at `{dir}`, which lies outside its repository",
                        r.key
                    ));
                }
            }
            s => segments.push(s),
        }
    }
    let path = if segments.is_empty() {
        host.repository.clone()
    } else {
        format!("{}/{}", host.repository, segments.join("/"))
    };
    let other = host_path(&path).map_err(|e| format!("{shown} requires `{}`: {e}", r.key))?;
    if other.repository != host.repository {
        return Err(format!(
            "{shown} requires `{}` at `{dir}`, which names no package of its repository",
            r.key
        ));
    }
    let version = Version::parse(version)
        .ok_or_else(|| format!("{shown} requires `{}` at the version `{version}`", r.key))?;
    Ok(Target::Host(other, version))
}

fn missing_manifest(key: &str, line: &str) -> String {
    format!(
        "{key} has no `hd.sum` line for its manifest, `{line}`, so selection cannot read it; run `hd fetch` to fetch it and record its hash, or `hd add` to change the requirement"
    )
}

fn missing_tree(key: &str) -> String {
    format!(
        "{key} has no `hd.sum` entry, so its tree cannot be checked; run `hd fetch` to fetch it and record its hash, or `hd add` to change the requirement"
    )
}

fn mismatch(key: &str, what: &str, hash: &str, entry: &str) -> String {
    format!(
        "{key}: the fetched {what}'s hash {hash} differs from its `hd.sum` entry {entry}; `hd` never replaces an entry, so find out why the {what} changed, as a moved tag, before you delete the entry and run `hd fetch`"
    )
}

/// The key of a host path's compatibility line.
fn line_key(host: &HostPath, version: &Version) -> String {
    format!("{}\0{}", host.path, compatibility_line(version))
}

/// Selects the versions of the packages in `members`, the package a
/// command works on first, against `sum`, taking each version it reads
/// from `source`.
pub fn select(
    members: &[PathBuf],
    sum: &Sum,
    mode: Mode,
    source: &mut dyn Source,
    toolchain: &str,
) -> Selection {
    let mut sel = Selection::default();
    let mut nodes: Vec<Node> = Vec::new();
    let mut local_index: HashMap<PathBuf, usize> = HashMap::new();
    for dir in members {
        let Ok(dir) = std::fs::canonicalize(dir) else {
            continue;
        };
        if local_index.contains_key(&dir) {
            continue;
        }
        let Some(manifest) = package_manifest(&dir) else {
            continue;
        };
        let edges = local_edges(&dir, &manifest, true, toolchain);
        local_index.insert(dir.clone(), nodes.len());
        nodes.push(Node {
            dir,
            manifest,
            fetched: None,
            edges,
        });
    }
    // The local packages that path requirements reach, in or outside the
    // workspace (`module.path-dep.any-package`). Their own workspace and
    // `hd.sum` are not read (`module.path-dep.root-selection`).
    let mut next = 0;
    while next < nodes.len() {
        let reached: Vec<PathBuf> = nodes[next]
            .edges
            .iter()
            .filter_map(|e| match &e.target {
                Target::Local(d) => Some(d.clone()),
                Target::Host(..) => None,
            })
            .collect();
        for dir in reached {
            if local_index.contains_key(&dir) {
                continue;
            }
            let Some(manifest) = package_manifest(&dir) else {
                continue;
            };
            let edges = local_edges(&dir, &manifest, false, toolchain);
            local_index.insert(dir.clone(), nodes.len());
            nodes.push(Node {
                dir,
                manifest,
                fetched: None,
                edges,
            });
        }
        next += 1;
    }
    let locals = nodes.len();
    let problem = |sel: &mut Selection, code: Option<Code>, at: &Place, message: String| {
        sel.problems.push(Problem {
            code,
            place: at.clone(),
            message,
        });
    };
    let host_edges = |node: &Node| -> Vec<(HostPath, Version, Place)> {
        node.edges
            .iter()
            .filter_map(|e| match &e.target {
                Target::Host(h, v) => Some((h.clone(), v.clone(), e.at.clone())),
                Target::Local(_) => None,
            })
            .collect()
    };
    let start: Vec<(HostPath, Version, Place)> = nodes.iter().flat_map(host_edges).collect();
    // Without a manifest line, selection cannot read a version, so a
    // command that only checks fails before it fetches anything
    // (`cli.dep.missing-sum.manifest`).
    if mode == Mode::Verify {
        for (h, v, at) in &start {
            let line = manifest_key(&h.path, v);
            if !sum.entries.contains_key(&line) {
                problem(
                    &mut sel,
                    Some(Code::MissingSumEntry),
                    at,
                    missing_manifest(&tree_key(&h.path, v), &line),
                );
            }
        }
    }
    let mut selected: Vec<(String, HostPath, Version, Place)> = Vec::new();
    let mut reached: HashMap<String, usize> = HashMap::new();
    if sel.problems.is_empty() {
        let mut failed: HashSet<String> = HashSet::new();
        let mut queue = start;
        let mut at_queue = 0;
        while at_queue < queue.len() {
            let (host, version, at) = queue[at_queue].clone();
            at_queue += 1;
            let line = line_key(&host, &version);
            match selected.iter_mut().find(|s| s.0 == line) {
                Some(best) if version > best.2 => {
                    *best = (line, host.clone(), version.clone(), at.clone());
                }
                Some(_) => {}
                None => selected.push((line, host.clone(), version.clone(), at.clone())),
            }
            let key = tree_key(&host.path, &version);
            if reached.contains_key(&key) || failed.contains(&key) {
                continue;
            }
            let manifest_line = manifest_key(&host.path, &version);
            let expected = sum.entries.get(&manifest_line);
            if expected.is_none() && mode == Mode::Verify {
                failed.insert(key.clone());
                problem(
                    &mut sel,
                    Some(Code::MissingSumEntry),
                    &at,
                    missing_manifest(&key, &manifest_line),
                );
                continue;
            }
            let entry = match source.entry(&host, &version) {
                Ok(e) => e,
                Err((code, message)) => {
                    failed.insert(key.clone());
                    problem(&mut sel, code, &at, message);
                    continue;
                }
            };
            let hash = match manifest_hash(&entry.dir) {
                Ok(h) => h,
                Err(e) => {
                    failed.insert(key.clone());
                    problem(
                        &mut sel,
                        Some(Code::InvalidRequirement),
                        &at,
                        format!("{key} is no package: its `hd.toml` cannot be read: {e}"),
                    );
                    continue;
                }
            };
            if let Some(expected) = expected
                && *expected != hash
            {
                failed.insert(key.clone());
                problem(
                    &mut sel,
                    Some(Code::SumMismatch),
                    &at,
                    mismatch(&manifest_line, "manifest", &hash, expected),
                );
                continue;
            }
            sel.sums.insert(manifest_line, hash);
            let Some(manifest) = package_manifest(&entry.dir) else {
                failed.insert(key.clone());
                problem(
                    &mut sel,
                    Some(Code::InvalidRequirement),
                    &at,
                    format!("{key} is no package: its `hd.toml` declares none"),
                );
                continue;
            };
            let mut edges = Vec::new();
            let errors = requirement_errors(&manifest, toolchain);
            for (line, code, message) in &errors {
                if *code == Code::ToolchainTooOld || line.is_none() {
                    problem(&mut sel, Some(*code), &at, format!("{key}: {message}"));
                }
            }
            // A fetched version's dev dependencies are never read
            // (`module.select.dev-dependencies`).
            for r in &manifest.dependencies {
                if let Some((_, code, message)) =
                    errors.iter().find(|(line, _, _)| *line == Some(r.line))
                {
                    problem(
                        &mut sel,
                        Some(*code),
                        &at,
                        format!("{key} has an invalid requirement in its `hd.toml`: {message}"),
                    );
                    continue;
                }
                let target = if r.path.is_some() {
                    fetched_path_target(&host, &key, r)
                } else {
                    host_target(r).ok_or_else(|| format!("{key}: `{}` is no requirement", r.text))
                };
                match target {
                    Ok(target) => edges.push(Edge {
                        name: r.name(),
                        dev: false,
                        target,
                        at: at.clone(),
                    }),
                    Err(message) => {
                        problem(&mut sel, Some(Code::InvalidRequirement), &at, message);
                    }
                }
            }
            let node = Node {
                dir: entry.dir,
                manifest,
                fetched: Some((host, version, entry.hash)),
                edges,
            };
            queue.extend(host_edges(&node));
            reached.insert(key, nodes.len());
            nodes.push(node);
        }
    }
    // Each selected version's tree line: kept, recorded, or an error
    // (`cli.dep.verify`, `cli.dep.missing-sum`, `cli.sum.keep`).
    let mut chosen: HashMap<String, usize> = HashMap::new();
    if sel.problems.is_empty() {
        for (line, host, version, at) in &selected {
            let key = tree_key(&host.path, version);
            let Some(&index) = reached.get(&key) else {
                continue;
            };
            chosen.insert(line.clone(), index);
            let Some((_, _, hash)) = &nodes[index].fetched else {
                continue;
            };
            match sum.entries.get(&key) {
                None if mode == Mode::Verify => {
                    problem(
                        &mut sel,
                        Some(Code::MissingSumEntry),
                        at,
                        missing_tree(&key),
                    );
                }
                Some(entry) if entry != hash => {
                    problem(
                        &mut sel,
                        Some(Code::SumMismatch),
                        at,
                        mismatch(&key, "tree", hash, entry),
                    );
                }
                _ => {
                    sel.sums.insert(key, hash.clone());
                }
            }
        }
    }
    // The order of the packages: the locals, then the selected versions.
    let mut order: Vec<usize> = (0..locals).collect();
    if sel.problems.is_empty() {
        for (line, ..) in &selected {
            if let Some(&i) = chosen.get(line) {
                order.push(i);
            }
        }
    }
    let position: HashMap<usize, usize> = order.iter().enumerate().map(|(p, &i)| (i, p)).collect();
    let target_of = |t: &Target| -> Option<usize> {
        let node = match t {
            Target::Local(d) => *local_index.get(d)?,
            Target::Host(h, v) => *chosen.get(&line_key(h, v))?,
        };
        position.get(&node).copied()
    };
    // A package that another depends on has a library
    // (`cli.dep.no-library`); a local one is checked where it is built.
    let mut checked: HashSet<usize> = HashSet::new();
    for &i in &order {
        for e in &nodes[i].edges {
            let Some(t) = target_of(&e.target) else {
                continue;
            };
            let node = &nodes[order[t]];
            if node.fetched.is_none() || !checked.insert(t) {
                continue;
            }
            let src = node
                .manifest
                .source_root
                .as_ref()
                .map_or("src", |(dir, _)| dir.trim_end_matches('/'));
            if !node.dir.join(src).join("lib.hd").is_file() {
                let key = node
                    .fetched
                    .as_ref()
                    .map(|(h, v, _)| tree_key(&h.path, v))
                    .unwrap_or_default();
                problem(
                    &mut sel,
                    Some(Code::InvalidRequirement),
                    &e.at,
                    format!(
                        "{key} has no library (`{src}/lib.hd`), so no package can depend on it"
                    ),
                );
            }
        }
    }
    for &i in &order {
        let node = &nodes[i];
        let edges = |dev: bool| -> Vec<(String, usize)> {
            node.edges
                .iter()
                .filter(|e| e.dev == dev)
                .filter_map(|e| Some((e.name.clone(), target_of(&e.target)?)))
                .collect()
        };
        sel.packages.push(Package {
            dir: node.dir.clone(),
            manifest: node.manifest.clone(),
            fetched: node.fetched.as_ref().map(|(h, v, _)| tree_key(&h.path, v)),
            requires: edges(false),
            dev_requires: edges(true),
        });
    }
    sel
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    use hd_diag::Code;
    use hd_project::{HostPath, Version};

    use super::{Mode, Selection, Source, select};
    use crate::cache::Entry;
    use crate::sum::{Sum, manifest_hash, tree_hash, tree_key};

    /// A directory under the workspace's `target/tmp`, empty.
    fn temp(name: &str) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tmp")
            .join(format!("hd-deps-select-{name}-{}", std::process::id()));
        let _ = crate::cache::remove_tree(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    fn write(dir: &Path, path: &str, text: &str) {
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
        std::fs::write(p, text).expect("write");
    }

    /// Versions served from directories, as if fetched; it records each
    /// version asked for.
    struct Fake {
        root: PathBuf,
        versions: BTreeMap<String, Vec<(String, String)>>,
        asked: Vec<String>,
    }

    impl Fake {
        fn new(root: &Path) -> Fake {
            Fake {
                root: root.join("fetched"),
                versions: BTreeMap::new(),
                asked: Vec::new(),
            }
        }

        /// A version whose manifest has `deps` as its `[dependencies]`.
        fn version(&mut self, key: &str, name: &str, deps: &str) -> &mut Fake {
            let files = vec![
                (
                    "hd.toml".to_owned(),
                    format!("[package]\nname = \"{name}\"\n\n[dependencies]\n{deps}"),
                ),
                (
                    "src/lib.hd".to_owned(),
                    format!("pub fn {name}() -> i32: 1\n"),
                ),
            ];
            self.versions.insert(key.to_owned(), files);
            self
        }
    }

    impl Source for Fake {
        fn entry(
            &mut self,
            host: &HostPath,
            version: &Version,
        ) -> Result<Entry, (Option<Code>, String)> {
            let key = tree_key(&host.path, version);
            self.asked.push(key.clone());
            let files = self
                .versions
                .get(&key)
                .ok_or((Some(Code::UnknownVersion), format!("no {key}")))?;
            let dir = self.root.join(&key);
            let _ = crate::cache::remove_tree(&dir);
            for (path, text) in files {
                write(&dir, path, text);
            }
            Ok(Entry {
                hash: tree_hash(&dir).expect("hash"),
                dir,
            })
        }
    }

    fn root_package(dir: &Path, deps: &str) -> PathBuf {
        let root = dir.join("app");
        write(
            &root,
            "hd.toml",
            &format!("[package]\nname = \"app\"\n\n{deps}"),
        );
        root
    }

    fn selected(sel: &Selection) -> Vec<String> {
        sel.packages
            .iter()
            .filter_map(|p| p.fetched.clone())
            .collect()
    }

    fn codes(sel: &Selection) -> Vec<(Option<Code>, u32)> {
        sel.problems
            .iter()
            .map(|p| (p.code, p.place.line))
            .collect()
    }

    #[test]
    fn the_largest_minimum_of_each_line_is_selected() {
        let dir = temp("largest");
        let root = root_package(
            &dir,
            "[dependencies]\njson = \"github.com/acme/json@2.1.0\"\nbilling = \"github.com/acme/billing@1.4.2\"\nold = \"github.com/acme/json@1.9.0\"\n",
        );
        let mut fake = Fake::new(&dir);
        fake.version("github.com/acme/json@2.1.0", "json", "")
            .version("github.com/acme/json@2.3.0", "json", "")
            .version("github.com/acme/json@1.9.0", "json", "")
            .version(
                "github.com/acme/billing@1.4.2",
                "billing",
                "json = \"github.com/acme/json@2.3.0\"\n",
            );
        let members = [root];
        let sel = select(&members, &Sum::default(), Mode::Record, &mut fake, "9.9.9");
        assert_eq!(codes(&sel), []);
        // A manifest's requirements are read in key order.
        assert_eq!(
            selected(&sel),
            [
                "github.com/acme/billing@1.4.2",
                "github.com/acme/json@2.3.0",
                "github.com/acme/json@1.9.0",
            ]
        );
        assert_eq!(
            sel.packages[0].requires,
            [
                ("billing".to_owned(), 1),
                ("json".to_owned(), 2),
                ("old".to_owned(), 3)
            ]
        );
        assert_eq!(sel.packages[1].requires, [("json".to_owned(), 2)]);
        // A tree line per selected version, a manifest line per version read.
        let keys: Vec<&str> = sel.sums.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "github.com/acme/billing@1.4.2",
                "github.com/acme/billing@1.4.2/hd.toml",
                "github.com/acme/json@1.9.0",
                "github.com/acme/json@1.9.0/hd.toml",
                "github.com/acme/json@2.1.0/hd.toml",
                "github.com/acme/json@2.3.0",
                "github.com/acme/json@2.3.0/hd.toml",
            ]
        );
        let json = dir.join("fetched/github.com/acme/json@2.3.0");
        assert_eq!(
            sel.sums["github.com/acme/json@2.3.0/hd.toml"],
            manifest_hash(&json).expect("hash")
        );
        // Verifying against the recorded lines selects the same versions.
        let sum = Sum {
            entries: sel.sums.clone(),
        };
        let again = select(&members, &sum, Mode::Verify, &mut fake, "9.9.9");
        assert_eq!(codes(&again), []);
        assert_eq!(selected(&again), selected(&sel));
    }

    #[test]
    fn a_check_without_manifest_lines_fails_before_it_fetches() {
        let dir = temp("missing");
        let root = root_package(
            &dir,
            "[dependencies]\njson = \"github.com/acme/json@2.1.0\"\n\n[dev-dependencies]\nfx = \"github.com/acme/fx@1.0.0\"\n",
        );
        let mut fake = Fake::new(&dir);
        let sel = select(&[root], &Sum::default(), Mode::Verify, &mut fake, "9.9.9");
        assert_eq!(
            codes(&sel),
            [
                (Some(Code::MissingSumEntry), 5),
                (Some(Code::MissingSumEntry), 8)
            ]
        );
        assert!(sel.problems[0].message.contains("hd fetch"));
        assert!(sel.problems[0].message.contains("hd add"));
        assert!(fake.asked.is_empty());
    }

    #[test]
    fn a_changed_tree_or_manifest_is_a_mismatch() {
        let dir = temp("mismatch");
        let members = [root_package(
            &dir,
            "[dependencies]\njson = \"github.com/acme/json@2.1.0\"\n",
        )];
        let mut fake = Fake::new(&dir);
        fake.version("github.com/acme/json@2.1.0", "json", "");
        let sel = select(&members, &Sum::default(), Mode::Record, &mut fake, "9.9.9");
        let forged = format!("h1:{}=", "A".repeat(43));
        for key in [
            "github.com/acme/json@2.1.0",
            "github.com/acme/json@2.1.0/hd.toml",
        ] {
            let mut sum = Sum {
                entries: sel.sums.clone(),
            };
            sum.entries.insert(key.to_owned(), forged.clone());
            for mode in [Mode::Verify, Mode::Record] {
                let bad = select(&members, &sum, mode, &mut fake, "9.9.9");
                assert_eq!(codes(&bad), [(Some(Code::SumMismatch), 5)], "{key}");
            }
        }
    }

    #[test]
    fn only_members_read_dev_dependencies() {
        let dir = temp("dev");
        let root = root_package(
            &dir,
            "[dependencies]\nlib = \"github.com/acme/lib@1.0.0\"\n\n[dev-dependencies]\nfx = \"github.com/acme/fx@1.0.0\"\n",
        );
        let mut fake = Fake::new(&dir);
        fake.version("github.com/acme/lib@1.0.0", "lib", "")
            .version("github.com/acme/fx@1.0.0", "fx", "");
        // lib's own dev dependency is never read.
        let lib = fake
            .versions
            .get_mut("github.com/acme/lib@1.0.0")
            .expect("lib");
        lib[0]
            .1
            .push_str("\n[dev-dependencies]\nnever = \"github.com/acme/never@1.0.0\"\n");
        let sel = select(&[root], &Sum::default(), Mode::Record, &mut fake, "9.9.9");
        assert_eq!(codes(&sel), []);
        assert_eq!(
            fake.asked,
            ["github.com/acme/lib@1.0.0", "github.com/acme/fx@1.0.0"]
        );
        assert_eq!(sel.packages[0].dev_requires, [("fx".to_owned(), 2)]);
    }

    #[test]
    fn a_fetched_path_requirement_stands_for_its_version() {
        let dir = temp("path");
        let members = [root_package(
            &dir,
            "[dependencies]\nlint = \"github.com/acme/tools/lint@0.3.0\"\n",
        )];
        let mut fake = Fake::new(&dir);
        fake.version(
            "github.com/acme/tools/lint@0.3.0",
            "lint",
            "core = { path = \"../core\", version = \"0.3.1\" }\n",
        )
        .version("github.com/acme/tools/core@0.3.1", "core", "");
        let sel = select(&members, &Sum::default(), Mode::Record, &mut fake, "9.9.9");
        assert_eq!(codes(&sel), []);
        assert_eq!(
            selected(&sel),
            [
                "github.com/acme/tools/lint@0.3.0",
                "github.com/acme/tools/core@0.3.1"
            ]
        );
        for (deps, why) in [
            ("core = { path = \"../core\" }\n", "no version"),
            (
                "core = { path = \"../../x\", version = \"1.0.0\" }\n",
                "outside its repository",
            ),
        ] {
            let mut fake = Fake::new(&dir.join("bad"));
            fake.version("github.com/acme/tools/lint@0.3.0", "lint", deps);
            let sel = select(&members, &Sum::default(), Mode::Record, &mut fake, "9.9.9");
            assert_eq!(codes(&sel), [(Some(Code::InvalidRequirement), 5)], "{deps}");
            assert!(
                sel.problems[0].message.contains(why),
                "{}",
                sel.problems[0].message
            );
        }
    }

    #[test]
    fn a_fetched_version_needs_a_library() {
        let dir = temp("nolib");
        let root = root_package(
            &dir,
            "[dependencies]\ntool = \"github.com/acme/tool@1.0.0\"\n",
        );
        let mut fake = Fake::new(&dir);
        fake.version("github.com/acme/tool@1.0.0", "tool", "");
        fake.versions
            .get_mut("github.com/acme/tool@1.0.0")
            .expect("tool")
            .retain(|(p, _)| p == "hd.toml");
        let sel = select(&[root], &Sum::default(), Mode::Record, &mut fake, "9.9.9");
        assert_eq!(codes(&sel), [(Some(Code::InvalidRequirement), 5)]);
        assert!(sel.problems[0].message.contains("no library"));
    }
}
