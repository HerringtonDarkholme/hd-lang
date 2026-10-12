//! Capability grants as the host applies them (Capability Grants, Grant
//! Precedence, Grant Scopes, Environment Grant, and the test grant of Test
//! Environments in `spec/cli/command-line.md`): one resolved grant per
//! program, built from a table and flags, and the scope checks a provider
//! asks before it touches a resource.

use std::path::{Component, Path, PathBuf};

/// One trait's value in a table or a flag (`cli.cap.value.*`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GrantValue {
    /// `true`: no limit.
    All,
    /// `false`: a total deny.
    Deny,
    /// A list of scope entries.
    Scopes(Vec<String>),
}

/// A trait's resolved limit; a trait with none has no limit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Limit {
    /// Totally denied (`cli.cap.total.*`).
    Deny,
    /// Only what these entries cover (`cli.cap.partial.*`); path entries
    /// are absolute and resolved (`cli.cap.scope.path.resolved`).
    Entries(Vec<String>),
}

/// A program's grant: the limit of each trait that has one
/// (`cli.cap.order.default`: every other trait has no limit).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Grants {
    pub limits: Vec<(String, Limit)>,
}

/// The traits whose scope entries are paths.
const PATH_KEYS: &[&str] = &["FsRead", "FsWrite"];

/// A path with `..` and `.` resolved and symbolic links followed as far as
/// the path exists, so no path escapes a granted directory
/// (`cli.cap.scope.path.resolved`).
#[must_use]
pub fn resolve_path(path: &Path) -> PathBuf {
    let mut lexical = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                lexical.pop();
            }
            Component::CurDir => {}
            other => lexical.push(other),
        }
    }
    // Canonicalize the longest existing ancestor, then add the rest.
    let mut rest = Vec::new();
    let mut base = lexical.as_path();
    loop {
        if let Ok(real) = std::fs::canonicalize(base) {
            let mut out = real;
            for part in rest.iter().rev() {
                out.push(part);
            }
            return out;
        }
        match (base.parent(), base.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_owned());
                base = parent;
            }
            _ => return lexical,
        }
    }
}

impl Grants {
    /// Grant Precedence (`cli.cap.order.*`): a `false` in the table or any
    /// flag denies totally; else flags that name the trait give no limit
    /// when one says `true`, and else every entry they list; else the
    /// table's value; else no limit. Path entries of the table are
    /// relative to `table_base`, the package directory, and those of a
    /// flag to `flag_base`, the working directory
    /// (`cli.cap.scope.path.relative`).
    #[must_use]
    pub fn resolve(
        table: &[(String, GrantValue)],
        table_base: &Path,
        flags: &[(String, GrantValue)],
        flag_base: &Path,
    ) -> Grants {
        let mut keys: Vec<&str> = table.iter().chain(flags).map(|(k, _)| k.as_str()).collect();
        keys.sort_unstable();
        keys.dedup();
        let entries = |key: &str, list: &[String], base: &Path| -> Vec<String> {
            list.iter()
                .map(|e| {
                    if PATH_KEYS.contains(&key) {
                        resolve_path(&base.join(e)).to_string_lossy().into_owned()
                    } else {
                        e.clone()
                    }
                })
                .collect()
        };
        let mut limits = Vec::new();
        for key in keys {
            let mut in_table = table.iter().filter(|(k, _)| k == key).map(|(_, v)| v);
            let in_flags: Vec<&GrantValue> = flags
                .iter()
                .filter(|(k, _)| k == key)
                .map(|(_, v)| v)
                .collect();
            let limit = if in_table
                .clone()
                .chain(in_flags.iter().copied())
                .any(|v| *v == GrantValue::Deny)
            {
                Some(Limit::Deny)
            } else if !in_flags.is_empty() {
                if in_flags.iter().any(|v| **v == GrantValue::All) {
                    None
                } else {
                    let mut all = Vec::new();
                    for v in &in_flags {
                        if let GrantValue::Scopes(list) = v {
                            all.extend(entries(key, list, flag_base));
                        }
                    }
                    Some(Limit::Entries(all))
                }
            } else {
                match in_table.next_back() {
                    Some(GrantValue::Scopes(list)) => {
                        Some(Limit::Entries(entries(key, list, table_base)))
                    }
                    _ => None,
                }
            };
            if let Some(l) = limit {
                limits.push((key.to_owned(), l));
            }
        }
        Grants { limits }
    }

    /// The test grant (`cli.test.env.grant.*`): `[test.capabilities]` and
    /// the flags of `hd test`, by Grant Precedence, where `FsRead` also
    /// covers the package directory and the test case's temporary
    /// directory, and `FsWrite` that temporary directory, unless the
    /// trait is denied or has no limit.
    #[must_use]
    pub fn for_test(
        table: &[(String, GrantValue)],
        package_dir: &Path,
        flags: &[(String, GrantValue)],
        cwd: &Path,
        temp_dir: &Path,
    ) -> Grants {
        let mut g = Grants::resolve(table, package_dir, flags, cwd);
        let named = |key: &str| table.iter().chain(flags).any(|(k, _)| k == key);
        for (key, dirs) in [
            ("FsRead", vec![package_dir, temp_dir]),
            ("FsWrite", vec![temp_dir]),
        ] {
            let resolved: Vec<String> = dirs
                .iter()
                .map(|d| resolve_path(d).to_string_lossy().into_owned())
                .collect();
            match g.limits.iter_mut().find(|(k, _)| k == key) {
                Some((_, Limit::Entries(list))) => list.extend(resolved),
                Some((_, Limit::Deny)) => {}
                None if !named(key) => g.limits.push((key.to_owned(), Limit::Entries(resolved))),
                None => {}
            }
        }
        g
    }

    /// The limit of a trait, or `None` for no limit.
    #[must_use]
    pub fn limit(&self, key: &str) -> Option<&Limit> {
        self.limits.iter().find(|(k, _)| k == key).map(|(_, l)| l)
    }

    /// Whether a trait is not totally denied.
    #[must_use]
    pub fn allows(&self, key: &str) -> bool {
        self.limit(key) != Some(&Limit::Deny)
    }

    /// Whether an `FsRead` or `FsWrite` grant covers `path`, relative to
    /// `cwd` when relative: an entry covers the file it names or every
    /// path under the directory it names (`cli.cap.scope.path`). An
    /// `FsWrite` entry grants no read (`cli.cap.scope.write-not-read`),
    /// since each trait has its own limit.
    #[must_use]
    pub fn covers_path(&self, key: &str, path: &Path, cwd: &Path) -> bool {
        match self.limit(key) {
            None => true,
            Some(Limit::Deny) => false,
            Some(Limit::Entries(list)) => {
                let target = resolve_path(&cwd.join(path));
                list.iter().any(|e| target.starts_with(Path::new(e)))
            }
        }
    }

    /// Whether an `Http` or `Net` grant covers `host` and `port`
    /// (`cli.cap.scope.host`, `.host.forms`, `cli.cap.scope.net`): an
    /// entry's host matches the name, `*.example.com` every name ending in
    /// `.example.com`, and an entry's port, when it gives one, must equal
    /// `port`.
    #[must_use]
    pub fn covers_host(&self, key: &str, host: &str, port: Option<u16>) -> bool {
        match self.limit(key) {
            None => true,
            Some(Limit::Deny) => false,
            Some(Limit::Entries(list)) => list.iter().any(|e| {
                let (entry_host, entry_port) = split_host(e);
                let host_ok = match entry_host.strip_prefix("*.") {
                    Some(suffix) => host
                        .strip_suffix(suffix)
                        .is_some_and(|head| head.ends_with('.') && head.len() > 1),
                    None => entry_host.eq_ignore_ascii_case(host),
                };
                host_ok && entry_port.is_none_or(|p| Some(p) == port)
            }),
        }
    }

    /// Whether an `Env`, `Process` or `Sys` grant covers `name`
    /// (`cli.cap.scope.env`, `.process`, `.sys`): an entry names it, or
    /// for `Env` an entry ending in `*` covers each name starting with the
    /// text before it.
    #[must_use]
    pub fn covers_name(&self, key: &str, name: &str) -> bool {
        match self.limit(key) {
            None => true,
            Some(Limit::Deny) => false,
            Some(Limit::Entries(list)) => list.iter().any(|e| match e.strip_suffix('*') {
                Some(prefix) if key == "Env" => name.starts_with(prefix),
                _ => e == name,
            }),
        }
    }
}

/// An entry's host and port: `host`, `host:port`, `[v6]` or `[v6]:port`.
fn split_host(entry: &str) -> (&str, Option<u16>) {
    if let Some(rest) = entry.strip_prefix('[') {
        let (host, after) = rest.split_once(']').unwrap_or((rest, ""));
        return (host, after.strip_prefix(':').and_then(|p| p.parse().ok()));
    }
    match entry.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => (host, port.parse().ok()),
        _ => (entry, None),
    }
}

/// The notice of a set variable that an `Env` grant does not cover
/// (`cli.cap.env.notice.text`); the host writes it once per name in a run
/// (`cli.cap.env.notice`).
#[must_use]
pub fn env_notice(name: &str) -> String {
    format!("hd: env {name} is set but not granted; run with --cap Env={name}")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{GrantValue, Grants, Limit, env_notice};

    fn kv(key: &str, v: GrantValue) -> (String, GrantValue) {
        (key.to_owned(), v)
    }

    fn scopes(list: &[&str]) -> GrantValue {
        GrantValue::Scopes(list.iter().map(|s| (*s).to_owned()).collect())
    }

    #[test]
    fn precedence_deny_then_flags_then_table_then_no_limit() {
        let base = std::env::temp_dir();
        let table = [
            kv("FsRead", scopes(&["data/"])),
            kv("Process", GrantValue::Deny),
            kv("Http", scopes(&["api.example.com"])),
        ];
        let flags = [
            kv("FsRead", scopes(&["out/"])),
            kv("Process", GrantValue::All),
            kv("Env", GrantValue::All),
        ];
        let g = Grants::resolve(&table, &base, &flags, &base);
        assert_eq!(g.limit("Process"), Some(&Limit::Deny));
        assert!(!g.allows("Process"));
        assert!(g.covers_path("FsRead", Path::new("out/a.txt"), &base));
        assert!(!g.covers_path("FsRead", Path::new("data/a.txt"), &base));
        assert!(!g.covers_path("FsRead", Path::new("out/../data/a.txt"), &base));
        assert!(g.covers_host("Http", "api.example.com", Some(443)));
        assert!(g.covers_name("Env", "HOME"));
        assert!(g.covers_name("Console", "anything"));
        assert_eq!(g.limit("Console"), None);
    }

    #[test]
    fn hosts_names_and_the_env_notice() {
        let here = Path::new("/");
        let g = Grants::resolve(
            &[
                kv(
                    "Http",
                    scopes(&["*.example.com", "localhost:8080", "[::1]:9"]),
                ),
                kv("Env", scopes(&["HOME", "APP_*"])),
                kv("Sys", scopes(&["os"])),
            ],
            here,
            &[],
            here,
        );
        assert!(g.covers_host("Http", "api.example.com", None));
        assert!(!g.covers_host("Http", "example.com", None));
        assert!(g.covers_host("Http", "localhost", Some(8080)));
        assert!(!g.covers_host("Http", "localhost", Some(80)));
        assert!(g.covers_host("Http", "::1", Some(9)));
        assert!(g.covers_name("Env", "APP_MODE") && !g.covers_name("Env", "PATH"));
        assert!(g.covers_name("Sys", "os") && !g.covers_name("Sys", "arch"));
        assert_eq!(
            env_notice("PATH"),
            "hd: env PATH is set but not granted; run with --cap Env=PATH"
        );
    }

    #[test]
    fn the_test_grant_reads_the_package_and_writes_its_temp_dir() {
        let pkg = std::env::temp_dir().join("hd-grant-pkg");
        let tmp = std::env::temp_dir().join("hd-grant-tmp");
        let g = Grants::for_test(&[], &pkg, &[], &pkg, &tmp);
        assert!(g.covers_path("FsRead", Path::new("fixtures/a.csv"), &pkg));
        assert!(g.covers_path("FsWrite", &tmp.join("out.csv"), &pkg));
        assert!(!g.covers_path("FsWrite", Path::new("out.csv"), &pkg));
        let open = Grants::for_test(&[kv("FsWrite", GrantValue::All)], &pkg, &[], &pkg, &tmp);
        assert!(open.covers_path("FsWrite", Path::new("out.csv"), &pkg));
    }
}
