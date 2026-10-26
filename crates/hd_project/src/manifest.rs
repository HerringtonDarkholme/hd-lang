//! The manifest `hd.toml`: its tables, and the rules a manifest alone can
//! break (`crates/hd_cli/DESIGN.md` §7.1 step 1: [Dependency Requirements], [Host Paths],
//! [Versions], [Toolchain Version] of `spec/lang/10-modules.md`,
//! `cli.dep.invalid`, Package Tooling's `cli.manifest.*`).

use hd_base::SliceSort as _;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt::Write as _;

use hd_diag::{Code, Severity};

/// A parsed `hd.toml`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Manifest {
    /// Whether it declares a package: it has a `[package]` table
    /// (`cli.mode.package.nearest`).
    pub declares_package: bool,
    pub name: String,
    /// `[package] hd`, the minimum toolchain version
    /// (`cli.manifest.toolchain-keys`), and its 1-based line.
    pub hd: Option<(String, u32)>,
    /// `[toolchain] pin` (`module.toolchain.pin`) and its line.
    pub pin: Option<(String, u32)>,
    /// `[source] root`, the source root when it is not `src`
    /// (`module.manifest.source-root`), and its line.
    pub source_root: Option<(String, u32)>,
    pub dependencies: Vec<Requirement>,
    pub dev_dependencies: Vec<Requirement>,
    /// Package-owned host traits and their provider files. The root app's
    /// explicit capability grant authorizes loading; a declaration alone does not.
    pub host: Option<HostDeclarations>,
    /// `[capabilities]` (`cli.cap.table`): each key as written, its grant
    /// (`None` when the value is neither a boolean nor a list of strings),
    /// and its line.
    pub capabilities: Vec<(String, Option<Grant>, u32)>,
    /// `[dev-capabilities]`, added to the root grant for tests and tasks.
    pub dev_capabilities: Vec<(String, Option<Grant>, u32)>,
    /// The `[[executable]]` tables, in order (`cli.exe.table`).
    pub executables: Vec<Executable>,
    /// `[workspace]` (`cli.mode.workspace`).
    pub workspace: Option<Workspace>,
    /// Each table or key that no rule gives a meaning, by its dotted path,
    /// and its line (`cli.manifest.unknown-key.with-host`).
    pub unknown: Vec<(String, u32)>,
}

/// The package's trait-to-provider map and its manifest location.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostDeclarations {
    /// `None` preserves a non-table value for a located diagnostic.
    pub providers: Option<Vec<HostProvider>>,
    pub line: u32,
}

/// One `[host]` entry. The path is relative to the declaring package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostProvider {
    pub trait_path: String,
    /// `None` preserves a non-string value for a located diagnostic.
    pub path: Option<String>,
    pub line: u32,
}

impl HostDeclarations {
    pub fn trait_paths(&self) -> impl Iterator<Item = &str> {
        self.providers
            .iter()
            .flatten()
            .map(|entry| entry.trait_path.as_str())
    }
}

/// One `[[executable]]` table: `name`, `module`, and the line of its
/// header. A missing or non-string field is `None`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Executable {
    pub name: Option<String>,
    pub module: Option<String>,
    pub line: u32,
}

/// A `[workspace]` table: its members and excluded directories, relative
/// to the workspace root.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Workspace {
    pub members: Vec<String>,
    pub exclude: Vec<String>,
    /// The 1-based line of its `members` key, or of the `[workspace]`
    /// table when there is none.
    pub line: u32,
}

/// What a capability grant gives one host capability trait
/// (`cli.cap.value.*`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Grant {
    /// `true`: no limit.
    All,
    /// `false`: a total deny.
    Deny,
    /// A list of scope entries.
    Scopes(Vec<String>),
}

/// The host capability traits a grant may name, and whether each takes
/// scope entries (Capability Grants, `cli.cap.table.keys.with-package-host`).
pub const CAPABILITY_KEYS: &[(&str, bool)] = &[
    ("Console", false),
    ("ConsoleInput", false),
    ("Args", false),
    ("Env", true),
    ("Clock", false),
    ("Random", false),
    ("FsRead", true),
    ("FsWrite", true),
    ("Process", true),
    ("Http", true),
    ("Net", true),
    ("Sys", true),
];

/// Why a grant for `key` breaks `cli.cap.table.keys.with-package-host` or
/// `cli.cap.value.unscoped` (or their flag forms), or `None`.
#[must_use]
pub fn grant_problem(key: &str, grant: &Grant) -> Option<String> {
    if is_capability_selector(key) {
        return matches!(grant, Grant::Scopes(_)).then(|| {
            format!("`{key}` is a package capability; its grant is `true` or `false`, not a list")
        });
    }
    let names = || {
        CAPABILITY_KEYS
            .iter()
            .map(|(k, _)| *k)
            .collect::<Vec<_>>()
            .join(", ")
    };
    match CAPABILITY_KEYS.iter().find(|(k, _)| *k == key) {
        None => Some(format!(
            "`{key}` is no host capability trait; a grant names one of {}",
            names()
        )),
        Some((_, false)) if matches!(grant, Grant::Scopes(_)) => Some(format!(
            "`{key}` has no scope entries, so its grant is `true` or `false`, not a list"
        )),
        Some(_) => None,
    }
}

/// A root source-use path, resolved to nominal identity by the built program.
#[must_use]
pub fn is_capability_selector(key: &str) -> bool {
    let parts: Vec<_> = key.split('.').collect();
    let prefix = match parts.first() {
        Some(&"pkg") => parts.len() >= 2,
        Some(&"dep") => parts.len() >= 3,
        _ => false,
    };
    prefix
        && parts
            .iter()
            .all(|p| *p != "_" && !p.contains('$') && crate::is_module_ident(p))
}

/// One dependency requirement of a manifest (`module.dep.*`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Requirement {
    /// The manifest key, as written.
    pub key: String,
    /// The directory of a path requirement, relative to the requiring
    /// manifest's directory (`module.path-dep.form`).
    pub path: Option<String>,
    /// The requirement as written: `PATH@VERSION`, or the inline table.
    pub text: String,
    /// Whether the value is a table, as a path requirement is.
    pub table: bool,
    /// A path requirement's `version` (`module.workspace.path-version`).
    pub version: Option<String>,
    /// The 1-based line of its key in `hd.toml`.
    pub line: u32,
}

impl Requirement {
    /// The name source writes after `dep.`: the key with each `-` as `_`
    /// (`module.dep.key-name`).
    #[must_use]
    pub fn name(&self) -> String {
        self.key.replace('-', "_")
    }

    /// The host path and version of a dependency requirement `PATH@VERSION`;
    /// `None` for a path requirement or a malformed one.
    #[must_use]
    pub fn host(&self) -> Option<(&str, Version)> {
        if self.table {
            return None;
        }
        let (path, version) = self.text.rsplit_once('@')?;
        Some((path, Version::parse(version)?))
    }
}

/// A rule a manifest breaks: its code, its severity, the 1-based line it is
/// at (`None` for the manifest as a whole), and the message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    /// `None` for a rule the specification gives no code, such as an
    /// unknown `[capabilities]` key.
    pub code: Option<Code>,
    pub severity: Severity,
    pub line: Option<u32>,
    pub message: String,
}

/// The 1-based line of each table and key of a manifest, by its dotted
/// path; an entry of an array of tables has its index as a segment, as
/// `executable.0.module`.
fn key_lines(text: &str) -> BTreeMap<String, u32> {
    fn walk(
        prefix: &str,
        t: &toml::de::DeTable<'_>,
        line: &dyn Fn(usize) -> u32,
        out: &mut BTreeMap<String, u32>,
    ) {
        for (key, value) in t {
            let k: &str = key.get_ref();
            let path = if prefix.is_empty() {
                k.to_owned()
            } else {
                format!("{prefix}.{k}")
            };
            out.insert(path.clone(), line(key.span().start));
            match value.get_ref() {
                toml::de::DeValue::Table(inner) => walk(&path, inner, line, out),
                toml::de::DeValue::Array(items) => {
                    for (i, item) in items.iter().enumerate() {
                        if let toml::de::DeValue::Table(inner) = item.get_ref() {
                            let at = format!("{path}.{i}");
                            out.insert(at.clone(), line(item.span().start));
                            walk(&at, inner, line, out);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let line = |at: usize| {
        let before = text.get(..at).unwrap_or(text);
        u32::try_from(before.bytes().filter(|&b| b == b'\n').count() + 1).unwrap_or(u32::MAX)
    };
    let mut out = BTreeMap::new();
    if let Ok(doc) = toml::de::DeTable::parse(text) {
        walk("", doc.get_ref(), &line, &mut out);
    }
    out
}

fn requirements(
    section: &str,
    table: &toml::Table,
    lines: &BTreeMap<String, u32>,
) -> Vec<Requirement> {
    table
        .iter()
        .map(|(k, v)| {
            let field = |name: &str| {
                v.as_table()
                    .and_then(|t| t.get(name))
                    .and_then(toml::Value::as_str)
                    .map(str::to_owned)
            };
            Requirement {
                key: k.clone(),
                path: field("path"),
                text: v.as_str().map_or_else(|| v.to_string(), str::to_owned),
                table: v.is_table(),
                version: field("version"),
                line: lines.get(&format!("{section}.{k}")).copied().unwrap_or(1),
            }
        })
        .collect()
}

/// A grant table: `[capabilities]` or `[dev-capabilities]`.
fn grants(
    path: &str,
    table: &toml::Table,
    lines: &BTreeMap<String, u32>,
) -> Vec<(String, Option<Grant>, u32)> {
    table
        .iter()
        .map(|(k, v)| {
            let grant = match v {
                toml::Value::Boolean(true) => Some(Grant::All),
                toml::Value::Boolean(false) => Some(Grant::Deny),
                toml::Value::Array(items) => items
                    .iter()
                    .map(|i| i.as_str().map(str::to_owned))
                    .collect::<Option<Vec<_>>>()
                    .map(Grant::Scopes),
                _ => None,
            };
            let line = lines.get(&format!("{path}.{k}")).copied().unwrap_or(1);
            (k.clone(), grant, line)
        })
        .collect()
}

/// The strings of a list, or `None` when it is not a list of strings.
fn strings(v: Option<&toml::Value>) -> Vec<String> {
    v.and_then(toml::Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// A TOML syntax error in `hd.toml`, with the 1-based line and column it
/// starts at when the parser gives a span.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestError {
    pub message: String,
    pub at: Option<(u32, u32)>,
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ManifestError {}

/// Parses `hd.toml` by the Package Tooling table: each table and key it
/// lists is read, and every other one is recorded as unknown
/// (`cli.manifest.unknown-key.with-host`).
pub fn parse_manifest(text: &str) -> Result<Manifest, ManifestError> {
    let table: toml::Table = text.parse().map_err(|e: toml::de::Error| {
        let before = e.span().and_then(|r| text.get(..r.start));
        ManifestError {
            message: e.message().to_owned(),
            at: before.map(|b| {
                let line = b.matches('\n').count() + 1;
                let col = b.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
                (
                    u32::try_from(line).unwrap_or(u32::MAX),
                    u32::try_from(col).unwrap_or(u32::MAX),
                )
            }),
        }
    })?;
    let lines = key_lines(text);
    let at = |path: &str| lines.get(path).copied().unwrap_or(1);
    let mut m = Manifest::default();
    let mut unknowns: Vec<(String, u32)> = Vec::new();
    // The keys a table may hold; any other key is unknown.
    let only = |unknowns: &mut Vec<(String, u32)>, path: &str, t: &toml::Table, keys: &[&str]| {
        for k in t.keys() {
            if !keys.contains(&k.as_str()) {
                let p = format!("{path}.{k}");
                let line = at(&p);
                unknowns.push((p, line));
            }
        }
    };
    for (section, value) in &table {
        match (section.as_str(), value) {
            ("package", toml::Value::Table(p)) => {
                m.declares_package = true;
                only(&mut unknowns, "package", p, &["name", "hd"]);
                if let Some(toml::Value::String(n)) = p.get("name") {
                    n.clone_into(&mut m.name);
                }
                if let Some(toml::Value::String(v)) = p.get("hd") {
                    m.hd = Some((v.clone(), at("package.hd")));
                }
            }
            ("toolchain", toml::Value::Table(t)) => {
                only(&mut unknowns, "toolchain", t, &["pin"]);
                if let Some(toml::Value::String(v)) = t.get("pin") {
                    m.pin = Some((v.clone(), at("toolchain.pin")));
                }
            }
            ("dependencies" | "dev-dependencies", toml::Value::Table(d)) => {
                let reqs = requirements(section, d, &lines);
                if section == "dependencies" {
                    m.dependencies = reqs;
                } else {
                    m.dev_dependencies = reqs;
                }
            }
            ("host", toml::Value::Table(h)) => {
                m.host = Some(HostDeclarations {
                    providers: Some(
                        h.iter()
                            .map(|(trait_path, value)| HostProvider {
                                trait_path: trait_path.clone(),
                                path: value.as_str().map(str::to_owned),
                                line: at(&format!("host.{trait_path}")),
                            })
                            .collect(),
                    ),
                    line: at("host"),
                });
            }
            ("host", _) => {
                m.host = Some(HostDeclarations {
                    providers: None,
                    line: at("host"),
                });
            }
            ("capabilities", toml::Value::Table(c)) => {
                m.capabilities = grants("capabilities", c, &lines);
            }
            ("dev-capabilities", toml::Value::Table(c)) => {
                m.dev_capabilities = grants("dev-capabilities", c, &lines);
            }
            ("workspace", toml::Value::Table(w)) => {
                only(&mut unknowns, "workspace", w, &["members", "exclude"]);
                m.workspace = Some(Workspace {
                    members: strings(w.get("members")),
                    exclude: strings(w.get("exclude")),
                    line: lines
                        .get("workspace.members")
                        .copied()
                        .unwrap_or_else(|| at("workspace")),
                });
            }
            ("executable", toml::Value::Array(items)) => {
                for (i, item) in items.iter().enumerate() {
                    let path = format!("executable.{i}");
                    let field = |name: &str| {
                        item.get(name)
                            .and_then(toml::Value::as_str)
                            .map(str::to_owned)
                    };
                    if let Some(t) = item.as_table() {
                        only(&mut unknowns, &path, t, &["name", "module"]);
                    }
                    m.executables.push(Executable {
                        name: field("name"),
                        module: field("module"),
                        line: at(&path),
                    });
                }
            }
            ("source", toml::Value::Table(t)) => {
                only(&mut unknowns, "source", t, &["root"]);
                if let Some(toml::Value::String(v)) = t.get("root") {
                    m.source_root = Some((v.clone(), at("source.root")));
                }
            }
            (other, _) => unknowns.push((other.to_owned(), at(other))),
        }
    }
    unknowns.sort_stable_by_key(|(_, line)| *line);
    m.unknown = unknowns;
    Ok(m)
}

/// A Semantic Versioning 2.0.0 version without a leading `v` (`module.version.tag`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// The pre-release identifiers; empty for a release.
    pub pre: Vec<String>,
}

impl Version {
    /// `MAJOR.MINOR.PATCH[-PRE]`, with no leading zeros, or `None`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Version> {
        let number = |s: &str| {
            let ok = !s.is_empty()
                && s.bytes().all(|b| b.is_ascii_digit())
                && (s == "0" || !s.starts_with('0'));
            if ok { s.parse::<u64>().ok() } else { None }
        };
        let (core, pre) = match text.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (text, None),
        };
        let mut parts = core.split('.');
        let major = number(parts.next()?)?;
        let minor = number(parts.next()?)?;
        let patch = number(parts.next()?)?;
        if parts.next().is_some() {
            return None;
        }
        let pre: Vec<String> = match pre {
            None => Vec::new(),
            Some(p) => {
                let ids: Vec<&str> = p.split('.').collect();
                let valid = |id: &&str| {
                    !id.is_empty()
                        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                        && (!id.bytes().all(|b| b.is_ascii_digit()) || number(id).is_some())
                };
                if !ids.iter().all(valid) {
                    return None;
                }
                ids.into_iter().map(str::to_owned).collect()
            }
        };
        Some(Version {
            major,
            minor,
            patch,
            pre,
        })
    }
}

/// The version as a requirement writes it, as `2.1.0` or `1.0.0-rc.1`.
impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if !self.pre.is_empty() {
            write!(f, "-{}", self.pre.join("."))?;
        }
        Ok(())
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Semantic Versioning 2.0.0 precedence (`module.version.order`).
impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        let core =
            (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch));
        if core != Ordering::Equal {
            return core;
        }
        match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, true) => return Ordering::Equal,
            (true, false) => return Ordering::Greater,
            (false, true) => return Ordering::Less,
            (false, false) => {}
        }
        for (a, b) in self.pre.iter().zip(&other.pre) {
            let order = match (a.parse::<u64>(), b.parse::<u64>()) {
                (Ok(x), Ok(y)) => x.cmp(&y),
                (Ok(_), Err(_)) => Ordering::Less,
                (Err(_), Ok(_)) => Ordering::Greater,
                (Err(_), Err(_)) => a.cmp(b),
            };
            if order != Ordering::Equal {
                return order;
            }
        }
        self.pre.len().cmp(&other.pre.len())
    }
}

/// The compatibility line of a version (`module.version.line`): its major
/// when at least 1, else `0.MINOR`.
#[must_use]
pub fn compatibility_line(v: &Version) -> String {
    if v.major >= 1 {
        v.major.to_string()
    } else {
        format!("0.{}", v.minor)
    }
}

/// A host path split into the repository it names and the package's
/// directory inside it (`module.repo.github`, `module.repo.git-suffix`,
/// `module.repo.subdirectory`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HostPath {
    /// The host path as written, as `github.com/acme/tools/lint`.
    pub path: String,
    /// The repository part, as `github.com/acme/tools`.
    pub repository: String,
    /// The package's directory in the repository, as `lint`; empty at its
    /// root.
    pub subdirectory: String,
}

/// A host path, or why it breaks [Host Paths] (`module.repo.*`,
/// `module.dep.no-major-suffix`).
pub fn host_path(path: &str) -> Result<HostPath, String> {
    let segments: Vec<&str> = path.split('/').collect();
    let segment_ok = |s: &&str| {
        !s.is_empty()
            && *s != "."
            && *s != ".."
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'~' | b'-'))
    };
    if !segments.iter().all(segment_ok) {
        return Err(format!(
            "`{path}` is not a host path: each segment is letters, digits, `.`, `_`, `~` or `-`, separated by single `/`"
        ));
    }
    let host = segments[0];
    if !host.contains('.') || host.starts_with('.') || host.ends_with('.') {
        return Err(format!(
            "`{path}` does not start with a host name, such as `github.com`"
        ));
    }
    let parts = if host == "github.com" {
        if segments.len() < 3 {
            return Err(format!(
                "`{path}` names no repository: on github.com, a host path is `github.com/OWNER/REPOSITORY`"
            ));
        }
        3
    } else {
        match segments
            .iter()
            .skip(1)
            .position(|s| s.rsplit_once('.').is_some_and(|(_, ext)| ext == "git"))
        {
            Some(i) => i + 2,
            None => {
                return Err(format!(
                    "`{path}` names no repository: on a host other than github.com, the repository part ends with a segment that ends in `.git`"
                ));
            }
        }
    };
    let suffix = |s: &&str| {
        s.strip_prefix('v')
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    };
    if segments[parts..].iter().any(suffix) {
        return Err(format!(
            "`{path}` has a major-version suffix; a host path carries none, so a second compatibility line takes a second key"
        ));
    }
    Ok(HostPath {
        path: path.to_owned(),
        repository: segments[..parts].join("/"),
        subdirectory: segments[parts..].join("/"),
    })
}

/// Why a version in a requirement is malformed, or `None`.
fn version_problem(text: &str) -> Option<String> {
    if Version::parse(text).is_some() {
        return None;
    }
    if let Some(bare) = text.strip_prefix('v')
        && Version::parse(bare).is_some()
    {
        return Some(format!(
            "the version `{text}` has a leading `v`; write `{bare}`"
        ));
    }
    Some(format!(
        "`{text}` is not a version: write MAJOR.MINOR.PATCH, as `2.1.0`"
    ))
}

/// Why `key` cannot be a dependency key, or `None` (`module.dep.key-name`):
/// source writes it as `dep.NAME`, so `NAME` is one identifier, and a bare
/// TOML key has only letters, digits, `_` and `-`.
#[must_use]
pub fn key_problem(key: &str) -> Option<String> {
    let name = key.replace('-', "_");
    let bare = !key.is_empty()
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    let lexed = hd_syntax::lex(name.as_bytes());
    let one_identifier = lexed.diagnostics.is_empty()
        && lexed.tokens.kind.first() == Some(&hd_syntax::TokenKind::Ident)
        && lexed.tokens.end.first().map(|&e| e as usize) == Some(name.len());
    (!(bare && one_identifier)).then(|| {
        format!(
            "the dependency key `{key}` is not a name source can write as `dep.{name}`; use letters, digits, `_` and `-`, starting with a letter"
        )
    })
}

/// Why one requirement breaks `module.dep.requirement-form` or
/// `module.path-dep.form`, or `None`.
fn form_problem(r: &Requirement) -> Option<String> {
    if r.table {
        if r.path.is_none() {
            return Some(format!(
                "the requirement `{}` is a table without `path`; write `PATH@VERSION` or `{{ path = \"DIR\" }}`",
                r.key
            ));
        }
        return r.version.as_deref().and_then(version_problem);
    }
    let Some((path, version)) = r.text.rsplit_once('@') else {
        return Some(format!(
            "`{}` has no version: write PATH@VERSION, as `github.com/acme/json@2.1.0`",
            r.text
        ));
    };
    host_path(path).err().or_else(|| version_problem(version))
}

/// Why the key and the string `text` cannot be a dependency requirement
/// `PATH@VERSION`, or `None` (`cli.dep.invalid`).
#[must_use]
pub fn requirement_problem(key: &str, text: &str) -> Option<String> {
    key_problem(key).or_else(|| {
        form_problem(&Requirement {
            key: key.to_owned(),
            path: None,
            text: text.to_owned(),
            table: false,
            version: None,
            line: 1,
        })
    })
}

/// What one package is to `module.dep.one-key-per-line`: a host path and
/// compatibility line, or a local directory.
fn identity(r: &Requirement) -> Option<String> {
    if let Some(dir) = &r.path {
        let dir = dir.trim_end_matches('/');
        return Some(format!("path {}", dir.strip_prefix("./").unwrap_or(dir)));
    }
    let (path, version) = r.host()?;
    Some(format!("{path}@{}", compatibility_line(&version)))
}

/// The rules a manifest alone breaks, in line order: each requirement's
/// form, key-name collisions and keys that share a package
/// (`cli.dep.invalid`), a minimum toolchain above `toolchain`
/// (`module.toolchain.graph-minimum`), each `[capabilities]` and
/// `[dev-capabilities]` key and value (`cli.cap.table.keys.with-package-host`,
/// `cli.cap.value.unscoped`), an `[[executable]]` table without a string
/// `name` and `module` (`cli.manifest.known-key`), and a warning for each
/// unknown table or key (`cli.manifest.unknown-key.with-host`).
#[must_use]
pub fn problems(m: &Manifest, toolchain: &str) -> Vec<Problem> {
    let mut out = Vec::new();
    let mut names: BTreeMap<String, &str> = BTreeMap::new();
    let mut packages: BTreeMap<String, &str> = BTreeMap::new();
    let invalid = |line: u32, message: String| Problem {
        code: Some(Code::InvalidRequirement),
        severity: Severity::Error,
        line: Some(line),
        message,
    };
    for r in m.dependencies.iter().chain(&m.dev_dependencies) {
        if let Some(why) = key_problem(&r.key).or_else(|| form_problem(r)) {
            out.push(invalid(r.line, why));
            continue;
        }
        if let Some(other) = names.insert(r.name(), &r.key) {
            out.push(invalid(
                r.line,
                format!(
                    "the keys `{other}` and `{}` both name `dep.{}`; rename one",
                    r.key,
                    r.name()
                ),
            ));
            continue;
        }
        if let Some(id) = identity(r)
            && let Some(other) = packages.insert(id, &r.key)
        {
            out.push(invalid(
                r.line,
                format!(
                    "the keys `{other}` and `{}` name one package and compatibility line; keep one key",
                    r.key
                ),
            ));
        }
    }
    if let Some((text, line)) = &m.hd
        && let (Some(min), Some(have)) = (Version::parse(text), Version::parse(toolchain))
        && min > have
    {
        out.push(Problem {
            code: Some(Code::ToolchainTooOld),
            severity: Severity::Error,
            line: Some(*line),
            message: format!(
                "this package needs toolchain {text} or newer, and this `hd` is {toolchain}"
            ),
        });
    }
    if let Some(host) = &m.host {
        let mut report = |line, message| {
            out.push(Problem {
                code: Some(Code::InvalidManifest),
                severity: Severity::Error,
                line: Some(line),
                message,
            });
        };
        if let Some(providers) = &host.providers {
            for entry in providers {
                let path = &entry.trait_path;
                let valid = path.split('.').all(|part| {
                    // A raw identifier has its canonical spelling here.
                    part != "_" && !part.contains('$') && crate::is_module_ident(part)
                });
                if !valid {
                    report(
                        entry.line,
                        format!(
                            "host trait `{path}` is not a package-relative trait path; write a path such as `server.HttpServer`"
                        ),
                    );
                }
                if !entry.path.as_deref().is_some_and(|file| {
                    !file.is_empty()
                        && !file.contains('\0')
                        && !file.starts_with(['/', '\\'])
                        && file.as_bytes().get(1) != Some(&b':')
                }) {
                    report(
                        entry.line,
                        format!(
                            "provider for host trait `{path}` must be a nonempty package-relative file path, such as \"providers/http.mjs\""
                        ),
                    );
                }
            }
        } else {
            report(
                host.line,
                "`[host]` must map package-relative trait paths to provider file paths".to_owned(),
            );
        }
    }
    for (key, grant, line) in m.capabilities.iter().chain(&m.dev_capabilities) {
        let why = match grant {
            None => Some(format!(
                "the grant of `{key}` is `true`, `false`, or a list of scope entries"
            )),
            Some(g) => grant_problem(key, g),
        };
        if let Some(message) = why {
            out.push(Problem {
                code: Some(Code::InvalidManifest),
                severity: Severity::Error,
                line: Some(*line),
                message,
            });
        }
    }
    if let Some((root, line)) = &m.source_root {
        let plain = !root.is_empty()
            && !root.starts_with('/')
            && root
                .split('/')
                .all(|seg| !seg.is_empty() && seg != "." && seg != "..");
        if !plain || matches!(root.as_str(), "tests" | "tasks" | "build") {
            out.push(Problem {
                code: Some(Code::InvalidManifest),
                severity: Severity::Error,
                line: Some(*line),
                message: format!(
                    "`[source] root = \"{root}\"` is no source root: it is a directory below the package directory, other than `tests`, `tasks` and `build`"
                ),
            });
        }
    }
    for e in &m.executables {
        if e.name.is_none() || e.module.is_none() {
            out.push(Problem {
                code: Some(Code::InvalidManifest),
                severity: Severity::Error,
                line: Some(e.line),
                message: "an `[[executable]]` table has a string `name` and a string `module`"
                    .to_owned(),
            });
        }
    }
    for (path, line) in &m.unknown {
        out.push(Problem {
            code: Some(Code::UnknownManifestKey),
            severity: Severity::Warning,
            line: Some(*line),
            message: format!("`{path}` has no meaning in `hd.toml`, so it is ignored"),
        });
    }
    out.sort_stable_by_key(|p| p.line);
    out
}

#[cfg(test)]
mod tests {
    use super::{
        Grant, Version, grant_problem, is_capability_selector, key_problem, parse_manifest,
        problems,
    };
    use hd_diag::Code;

    #[test]
    fn versions_order_by_semver_precedence() {
        let v = |s: &str| Version::parse(s).expect(s);
        assert!(v("1.0.0-alpha") < v("1.0.0-alpha.1"));
        assert!(v("1.0.0-alpha.1") < v("1.0.0-beta"));
        assert!(v("1.0.0-rc.1") < v("1.0.0"));
        assert!(v("0.9.0") < v("0.10.0"));
        for bad in ["1.0", "01.0.0", "v1.0.0", "1.0.0-", "1.0.0-01"] {
            assert!(Version::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn manifest_problems_have_codes_and_lines() {
        let m = parse_manifest(
            "[package]\nname = \"app\"\nhd = \"9.0.0\"\n\n[dependencies]\njson = \"github.com/acme@2.1.0\"\nmy-money = { path = \"a\" }\nmy_money = { path = \"b\" }\nm1 = { path = \"m\", version = \"0.4.0\" }\nm2 = { path = \"m/\", version = \"0.4.2\" }\n",
        )
        .expect("manifest");
        let got: Vec<(Option<Code>, Option<u32>)> = problems(&m, "0.1.0")
            .into_iter()
            .map(|p| (p.code, p.line))
            .collect();
        assert_eq!(
            got,
            [
                (Some(Code::ToolchainTooOld), Some(3)),
                (Some(Code::InvalidRequirement), Some(6)),
                (Some(Code::InvalidRequirement), Some(8)),
                (Some(Code::InvalidRequirement), Some(10)),
            ]
        );
        let ok = parse_manifest(
            "[package]\nname = \"app\"\n[dependencies]\njson = \"github.com/acme/json@2.1.0\"\njson_v1 = \"github.com/acme/json@1.9.0\"\nbilling = \"git.example.com/shop/billing.git@0.4.2\"\n",
        )
        .expect("manifest");
        assert!(problems(&ok, "0.1.0").is_empty());
    }

    #[test]
    fn capabilities_parse_and_check_their_keys() {
        let m = parse_manifest(
            "[package]\nname = \"app\"\n[capabilities]\nConsole = false\nFsRead = [\"data/\"]\nClock = [\"x\"]\nTime = true\nHttp = 3\n",
        )
        .expect("manifest");
        let grant = |k: &str| {
            m.capabilities
                .iter()
                .find(|c| c.0 == k)
                .map(|c| c.1.clone())
        };
        assert_eq!(grant("Console"), Some(Some(super::Grant::Deny)));
        assert_eq!(
            grant("FsRead"),
            Some(Some(super::Grant::Scopes(vec!["data/".to_owned()])))
        );
        let lines: Vec<Option<u32>> = problems(&m, "0.1.0").into_iter().map(|p| p.line).collect();
        assert_eq!(lines, [Some(6), Some(7), Some(8)]);
    }

    #[test]
    fn host_declarations_keep_their_location_without_granting_capabilities() {
        let m = parse_manifest(
            "[package]\nname = \"sdk\"\n[host]\n\"server.HttpServer\" = \"providers/http.mjs\"\n\"ui.Σurface\" = \"ui.mjs\"\n\"server.type\" = \"type.mjs\"\n",
        ).unwrap();
        let host = m.host.as_ref().unwrap();
        assert_eq!(host.line, 3);
        assert_eq!(
            host.trait_paths().collect::<Vec<_>>(),
            ["server.HttpServer", "server.type", "ui.Σurface"]
        );
        let entries = host.providers.as_ref().unwrap();
        assert_eq!(entries[0].path.as_deref(), Some("providers/http.mjs"));
        assert_eq!(
            entries.iter().map(|e| e.line).collect::<Vec<_>>(),
            [4, 6, 5]
        );
        assert!(m.capabilities.is_empty());
        assert!(m.unknown.is_empty());
        assert!(
            problems(&m, "0.1.0").is_empty(),
            "{:?}",
            problems(&m, "0.1.0")
        );
        assert!(problems(&parse_manifest("[host]").unwrap(), "0.1.0").is_empty());
    }

    #[test]
    fn package_capability_grants_use_source_paths_and_only_booleans() {
        for key in [
            "pkg.Ui",
            "pkg.ui.Ui",
            "dep.server_sdk.server.HttpServer",
            "dep.sdk.type",
        ] {
            assert!(is_capability_selector(key));
            assert_eq!(grant_problem(key, &Grant::All), None);
            assert_eq!(grant_problem(key, &Grant::Deny), None);
            assert!(grant_problem(key, &Grant::Scopes(vec!["anything".into()])).is_some());
        }
        for key in [
            "Ui",
            "pkg",
            "dep.sdk",
            "dep..Ui",
            "pkg._",
            "pkg.ui$",
            "example.org/sdk@1.2.3/Ui",
        ] {
            assert!(!is_capability_selector(key));
            assert!(grant_problem(key, &Grant::All).is_some());
        }
    }

    #[test]
    fn invalid_host_declarations_are_located_errors() {
        for text in [
            "host = true",
            "[host]\ntraits = [\"Server\"]",
            "[host]\n\"server.Server\" = true",
            "[host]\n\"server..Server\" = \"provider.mjs\"",
            "[host]\n\"server.$internal\" = \"provider.mjs\"",
            "[host]\n\"server._\" = \"provider.mjs\"",
            "[host]\nServer = \"\"",
            "[host]\nServer = \"/provider.mjs\"",
            "[host]\nServer = \"C:/provider.mjs\"",
        ] {
            let m = parse_manifest(text).unwrap();
            let errors = problems(&m, "0.1.0");
            assert_eq!(errors.len(), 1, "{text}: {errors:?}");
            assert_eq!(errors[0].code, Some(hd_diag::Code::InvalidManifest));
            assert_eq!(errors[0].severity, hd_diag::Severity::Error);
            assert_eq!(
                errors[0].line,
                Some(if text == "host = true" { 1 } else { 2 })
            );
        }
        assert!(parse_manifest("[host]\nServer = \"a.mjs\"\nServer = \"b.mjs\"").is_err());
    }

    #[test]
    fn a_key_is_a_name_source_can_write() {
        for ok in ["json", "my-app", "my_app", "_x", "json2"] {
            assert_eq!(key_problem(ok), None, "{ok}");
        }
        for bad in [
            "9json",
            "",
            "a b",
            "fn",
            "if",
            "self",
            "_",
            "a.b",
            "json\u{e9}",
        ] {
            assert!(key_problem(bad).is_some(), "{bad}");
        }
    }
}

/// One insertion that adds missing unrestricted grants without rewriting
/// existing TOML or changing any grant already present. The caller supplies
/// valid capability selectors; existing entries, including false, are kept.
#[must_use]
pub fn capability_insertion(text: &str, names: &[String]) -> Option<(u32, String)> {
    capability_insertion_for(text, names, false)
}

/// Inserts a production or development grant table entry, preserving TOML.
#[must_use]
pub fn capability_insertion_for(
    text: &str,
    names: &[String],
    development: bool,
) -> Option<(u32, String)> {
    let table_name = if development {
        "dev-capabilities"
    } else {
        "capabilities"
    };
    let table: toml::Table = text.parse().ok()?;
    let present = table.get(table_name).and_then(toml::Value::as_table);
    let names: std::collections::BTreeSet<_> = names
        .iter()
        .filter(|key| grant_problem(key, &Grant::All).is_none())
        .filter(|key| !present.is_some_and(|t| t.contains_key(key.as_str())))
        .collect();
    if names.is_empty() {
        return None;
    }
    let key = |name: &str| {
        if name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            name.to_owned()
        } else {
            toml::Value::String(name.to_owned()).to_string()
        }
    };
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let lines = |prefix: &str| {
        let mut out = String::new();
        for name in &names {
            let _ = write!(out, "{prefix}{} = true{newline}", key(name));
        }
        out
    };
    let doc = toml::de::DeTable::parse(text).ok()?;
    let entry = doc
        .get_ref()
        .iter()
        .find(|(k, _)| k.get_ref() == table_name);
    let (at, addition) = if let Some((name, value)) = entry {
        let start = name.span().start;
        let line = text[..start].rfind('\n').map_or(0, |i| i + 1);
        if text[line..start].trim_start().starts_with('[') {
            // A normal or quoted table heading. Insert immediately after it,
            // before any nested or subsequent table can change the context.
            match text[start..].find('\n') {
                Some(end) => (start + end + 1, lines("")),
                None => (text.len(), format!("{newline}{}", lines(""))),
            }
        } else {
            let range = value.span();
            let value_text = text.get(range.clone())?;
            if value_text.trim_start().starts_with('{') {
                let at = range.start + value_text.find('{')? + 1;
                let values = names
                    .iter()
                    .map(|name| format!("{} = true", key(name)))
                    .collect::<Vec<_>>()
                    .join(", ");
                let tail = if present.is_some_and(|t| !t.is_empty()) {
                    ", "
                } else {
                    ""
                };
                (at, format!("{values}{tail}"))
            } else {
                // Root dotted keys define an implicit table; extending that
                // table with more dotted keys avoids declaring it twice.
                (0, lines(&format!("{table_name}.")))
            }
        }
    } else {
        let gap = if text.is_empty() || text.ends_with('\n') {
            ""
        } else {
            newline
        };
        (
            text.len(),
            format!("{gap}{newline}[{table_name}]{newline}{}", lines("")),
        )
    };
    Some((u32::try_from(at).ok()?, addition))
}

#[cfg(test)]
mod capability_insertion_tests {
    use super::*;

    #[test]
    fn capability_insertions_preserve_toml_and_existing_denials() {
        for text in [
            "# keep\n[package]\nname = \"app\"\n",
            "# keep\n[package]\nname = \"app\"\n[capabilities]\nConsole = false\n[dependencies]\n",
            "# keep\r\n[package]\r\nname = \"app\"\r\n[ \"capabilities\" ] # header\r\nConsole = false\r\n",
            "# keep\ncapabilities = { Console = false, }\n[package]\nname = \"app\"\n",
            "# keep\ncapabilities = {}\n[package]\nname = \"app\"\n",
            "# keep\ncapabilities.Console = false\n[package]\nname = \"app\"\n",
        ] {
            let keys = vec!["Process".to_owned(), "dep.sdk.server.HttpServer".to_owned()];
            let (at, add) = capability_insertion(text, &keys).expect(text);
            let after = format!("{}{}{}", &text[..at as usize], add, &text[at as usize..]);
            let mut expected: toml::Table = text.parse().unwrap();
            let caps = expected
                .entry("capabilities")
                .or_insert_with(|| toml::Value::Table(toml::Table::new()))
                .as_table_mut()
                .unwrap();
            for key in &keys {
                caps.insert(key.clone(), toml::Value::Boolean(true));
            }
            let actual: toml::Table = after.parse().unwrap_or_else(|e| panic!("{e}: {after}"));
            assert_eq!(actual, expected, "{after}");
            assert!(after.contains("# keep"));
            assert!(capability_insertion(&after, &keys).is_none());
        }
    }

    #[test]
    fn development_insertions_preserve_all_toml_forms() {
        for text in [
            "[package]\nname=\"app\"\n",
            "[dev-capabilities]\nConsole=false\n",
            "[ \"dev-capabilities\" ] # keep\r\nConsole=false\r\n",
            "dev-capabilities={Console=false}\n",
            "dev-capabilities={}\n",
            "dev-capabilities.Console=false\n",
        ] {
            let keys = vec!["Process".to_owned(), "dep.sdk.Server".to_owned()];
            let (at, add) = capability_insertion_for(text, &keys, true).unwrap();
            let after = format!("{}{}{}", &text[..at as usize], add, &text[at as usize..]);
            let mut expected: toml::Table = text.parse().unwrap();
            let table = expected
                .entry("dev-capabilities")
                .or_insert_with(|| toml::Value::Table(toml::Table::new()))
                .as_table_mut()
                .unwrap();
            for key in &keys {
                table.insert(key.clone(), toml::Value::Boolean(true));
            }
            assert_eq!(after.parse::<toml::Table>().unwrap(), expected, "{after}");
            assert!(capability_insertion_for(&after, &keys, true).is_none());
        }
    }

    #[test]
    fn a_present_false_grant_is_never_overwritten() {
        assert!(
            capability_insertion("[capabilities]\nProcess = false\n", &["Process".to_owned()])
                .is_none()
        );
    }
}
