//! The manifest `hd.toml`: its package section and requirements, and the
//! rules a manifest alone can break (commands.md §7.1 step 1:
//! [Dependency Requirements], [Host Paths], [Versions], [Toolchain Version]
//! of `spec/lang/10-modules.md`, `cli.dep.invalid`).

use std::cmp::Ordering;
use std::collections::BTreeMap;

use hd_base::{NotImplemented, Stage, StageResult};
use hd_diag::Code;

/// The manifest's package section (`hd.toml`), parsed with `toml`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Manifest {
    pub name: String,
    pub version: Option<String>,
    /// `[package] hd`, the minimum toolchain version
    /// (`cli.manifest.toolchain-keys`), and its 1-based line.
    pub hd: Option<(String, u32)>,
    pub dependencies: Vec<Requirement>,
    pub dev_dependencies: Vec<Requirement>,
    /// `[capabilities]` (`cli.cap.table`): each key as written, its grant
    /// (`None` when the value is neither a boolean nor a list of strings),
    /// and its line.
    pub capabilities: Vec<(String, Option<Grant>, u32)>,
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
/// scope entries (Capability Grants, `cli.cap.table.keys`).
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

/// Why a grant for `key` breaks `cli.cap.table.keys` or
/// `cli.cap.value.unscoped` (or their flag forms), or `None`.
#[must_use]
pub fn grant_problem(key: &str, grant: &Grant) -> Option<String> {
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

/// A rule a manifest breaks: its code, the 1-based line it is at (`None`
/// for the manifest as a whole), and the message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    /// `None` for a rule the specification gives no code, such as an
    /// unknown `[capabilities]` key.
    pub code: Option<Code>,
    pub line: Option<u32>,
    pub message: String,
}

/// The 1-based line of each `[section] key` of a manifest, and of each
/// section header under the key `""`.
fn key_lines(text: &str) -> BTreeMap<(String, String), u32> {
    let line = |at: usize| {
        let before = text.get(..at).unwrap_or(text);
        u32::try_from(before.bytes().filter(|&b| b == b'\n').count() + 1).unwrap_or(u32::MAX)
    };
    let mut out = BTreeMap::new();
    let Ok(doc) = toml::de::DeTable::parse(text) else {
        return out;
    };
    for (section, value) in doc.get_ref() {
        let name: &str = section.get_ref();
        out.insert((name.to_owned(), String::new()), line(section.span().start));
        if let toml::de::DeValue::Table(t) = value.get_ref() {
            for (key, _) in t {
                let k: &str = key.get_ref();
                out.insert((name.to_owned(), k.to_owned()), line(key.span().start));
            }
        }
    }
    out
}

fn requirements(
    section: &str,
    table: &toml::Table,
    lines: &BTreeMap<(String, String), u32>,
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
                line: lines
                    .get(&(section.to_owned(), k.clone()))
                    .copied()
                    .unwrap_or(1),
            }
        })
        .collect()
}

/// Parses `hd.toml`. Sections other than `[package]`, `[dependencies]` and
/// `[dev-dependencies]` are not implemented yet.
pub fn parse_manifest(text: &str) -> StageResult<Manifest> {
    let table: toml::Table = text.parse().map_err(|e: toml::de::Error| {
        NotImplemented::new(Stage::Discover, format!("manifest: {}", e.message()))
    })?;
    let lines = key_lines(text);
    let mut m = Manifest::default();
    for (section, value) in &table {
        match (section.as_str(), value) {
            ("package", toml::Value::Table(p)) => {
                if let Some(toml::Value::String(n)) = p.get("name") {
                    n.clone_into(&mut m.name);
                }
                if let Some(toml::Value::String(v)) = p.get("version") {
                    m.version = Some(v.clone());
                }
                if let Some(toml::Value::String(v)) = p.get("hd") {
                    let line = lines
                        .get(&("package".to_owned(), "hd".to_owned()))
                        .copied()
                        .unwrap_or(1);
                    m.hd = Some((v.clone(), line));
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
            ("capabilities", toml::Value::Table(c)) => {
                m.capabilities = c
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
                        let line = lines
                            .get(&("capabilities".to_owned(), k.clone()))
                            .copied()
                            .unwrap_or(1);
                        (k.clone(), grant, line)
                    })
                    .collect();
            }
            (other, _) => {
                return Err(NotImplemented::new(
                    Stage::Discover,
                    format!("manifest section [{other}]"),
                ));
            }
        }
    }
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

/// Why a host path breaks [Host Paths] (`module.repo.*`,
/// `module.dep.no-major-suffix`), or `None` when it names a repository.
fn host_path_problem(path: &str) -> Option<String> {
    let segments: Vec<&str> = path.split('/').collect();
    let segment_ok = |s: &&str| {
        !s.is_empty()
            && *s != "."
            && *s != ".."
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'~' | b'-'))
    };
    if !segments.iter().all(segment_ok) {
        return Some(format!(
            "`{path}` is not a host path: each segment is letters, digits, `.`, `_`, `~` or `-`, separated by single `/`"
        ));
    }
    let host = segments[0];
    if !host.contains('.') || host.starts_with('.') || host.ends_with('.') {
        return Some(format!(
            "`{path}` does not start with a host name, such as `github.com`"
        ));
    }
    let parts = if host == "github.com" {
        if segments.len() < 3 {
            return Some(format!(
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
                return Some(format!(
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
        return Some(format!(
            "`{path}` has a major-version suffix; a host path carries none, so a second compatibility line takes a second key"
        ));
    }
    None
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
    host_path_problem(path).or_else(|| version_problem(version))
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
/// (`module.toolchain.graph-minimum`), and each `[capabilities]` key and
/// value (`cli.cap.table.keys`, `cli.cap.value.unscoped`).
#[must_use]
pub fn problems(m: &Manifest, toolchain: &str) -> Vec<Problem> {
    let mut out = Vec::new();
    let mut names: BTreeMap<String, &str> = BTreeMap::new();
    let mut packages: BTreeMap<String, &str> = BTreeMap::new();
    let invalid = |line: u32, message: String| Problem {
        code: Some(Code::InvalidRequirement),
        line: Some(line),
        message,
    };
    for r in m.dependencies.iter().chain(&m.dev_dependencies) {
        if let Some(why) = form_problem(r) {
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
            line: Some(*line),
            message: format!(
                "this package needs toolchain {text} or newer, and this `hd` is {toolchain}"
            ),
        });
    }
    for (key, grant, line) in &m.capabilities {
        let why = match grant {
            None => Some(format!(
                "the grant of `{key}` is `true`, `false`, or a list of scope entries"
            )),
            Some(g) => grant_problem(key, g),
        };
        if let Some(message) = why {
            out.push(Problem {
                code: None,
                line: Some(*line),
                message,
            });
        }
    }
    out.sort_by_key(|p| p.line);
    out
}

#[cfg(test)]
mod tests {
    use super::{Version, parse_manifest, problems};
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
}
