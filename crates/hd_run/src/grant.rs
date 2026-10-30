//! Capability grants as the host applies them (Capability Grants, Grant
//! Precedence, Grant Scopes, Environment Grant, and the test grant of Test
//! Environments in `spec/cli/command-line.md`): one resolved grant per
//! program, built from a table and flags, and the scope checks a provider
//! asks before it touches a resource.

use hd_base::SliceSort as _;
use std::path::{Component, Path, PathBuf};

use hd_base::path::canonicalize;

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

/// A path with `..` and `.` resolved by its text alone: `a/b/../c` is
/// `a/c`, whatever `a/b` is.
#[must_use]
pub fn lexical_path(path: &Path) -> PathBuf {
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
    lexical
}

/// A path resolved as the OS resolves it, so no path escapes a granted
/// directory (`cli.cap.scope.path.resolved`): the longest existing prefix
/// by the OS, then each further name, following a symbolic link there
/// (even one whose target does not exist) before the `..` after it, as a
/// write or `create_dir_all` would. A name that does not exist is kept as
/// it is, and a `..` after it removes it. Windows resolves every `..` by
/// its text before it follows any link, so there the path is made lexical
/// first.
#[must_use]
pub fn resolve_path(path: &Path) -> PathBuf {
    resolve_following(path, 0)
}

/// The path a filesystem refusal names and suggests granting. Preserve
/// the written spelling unless resolution changes the target; show a
/// changed target relative to the real working directory when inside it.
/// `follow` is false when the refused operation acts on the final link
/// itself rather than on its destination.
#[must_use]
pub fn refused_path(path: &Path, cwd: &Path, follow: bool) -> String {
    let base = resolve_path(cwd);
    let target = if !follow && let Some(name) = path.file_name() {
        let parent = path.parent().unwrap_or(Path::new(""));
        resolve_path(&cwd.join(parent)).join(name)
    } else {
        resolve_path(&cwd.join(path))
    };
    if target == lexical_path(&base.join(path)) {
        return path.to_string_lossy().into_owned();
    }
    let shown = target.strip_prefix(&base).unwrap_or(&target);
    if shown.as_os_str().is_empty() {
        ".".to_owned()
    } else {
        shown
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/")
    }
}

/// How many symbolic links one resolution follows before it stops, as
/// the OS does with `ELOOP`; the access then fails on its own.
const MAX_LINKS: u32 = 40;

fn resolve_following(path: &Path, links: u32) -> PathBuf {
    let lexical;
    let path = if cfg!(windows) {
        lexical = lexical_path(path);
        &lexical
    } else {
        path
    };
    let parts: Vec<Component> = path.components().collect();
    let (mut out, start) = (0..=parts.len())
        .rev()
        .find_map(|k| {
            let prefix: PathBuf = parts[..k].iter().collect();
            canonicalize(&prefix).ok().map(|real| (real, k))
        })
        .unwrap_or_default();
    // How many names at the end of `out` do not exist.
    let mut missing = 0usize;
    for c in &parts[start..] {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
                missing = missing.saturating_sub(1);
            }
            Component::Normal(name) => {
                let next = out.join(name);
                if let Some(resolved) = (missing == 0).then(|| step(&next, links)).flatten() {
                    out = resolved;
                } else {
                    missing += 1;
                    out = next;
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// `path`, whose parent is resolved, resolved by the OS, or through the
/// symbolic link it names when the link's target does not exist; `None`
/// when no such file or link exists.
fn step(path: &Path, links: u32) -> Option<PathBuf> {
    if let Ok(real) = canonicalize(path) {
        return Some(real);
    }
    let target = std::fs::read_link(path)
        .ok()
        .filter(|_| links < MAX_LINKS)?;
    let from = path.parent().unwrap_or(Path::new(""));
    Some(resolve_following(&from.join(target), links + 1))
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
        keys.sort_stable();
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

    /// The test grant (`cli.test.env.grant.*`): the combined production/development tables and
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

    /// Contract shared with `resolvePath` in `host/node-files.mjs`: the nearest
    /// existing ancestor is resolved by the OS and the rest appended, so
    /// a grant entry and an accessed path that do not exist yet agree.
    #[test]
    fn resolve_path_resolves_the_existing_ancestor_and_appends_the_rest() {
        let dir = std::env::temp_dir();
        let real = hd_base::path::canonicalize(&dir).expect("temp dir");
        assert_eq!(super::resolve_path(&dir), real);
        assert_eq!(
            super::resolve_path(&dir.join("hd-no-such").join("x").join("..").join("y.txt")),
            real.join("hd-no-such").join("y.txt")
        );
        // `..` cannot leave a granted directory through the new tail.
        let g = Grants::resolve(&[kv("FsWrite", scopes(&["out/"]))], &dir, &[], &dir);
        assert!(g.covers_path("FsWrite", Path::new("out/new/a.txt"), &dir));
        assert!(!g.covers_path("FsWrite", Path::new("out/../top.txt"), &dir));
        assert!(!g.covers_path("FsWrite", Path::new("out-other/a.txt"), &dir));
    }

    #[test]
    fn plain_refusals_keep_the_written_path() {
        let cwd = std::env::temp_dir();
        for path in ["plain.txt", "new/../plain.txt", "new/./plain.txt"] {
            for follow in [true, false] {
                assert_eq!(super::refused_path(Path::new(path), &cwd, follow), path);
            }
        }
        let absolute = super::resolve_path(&cwd).join("plain.txt");
        assert_eq!(
            super::refused_path(&absolute, &cwd, true),
            absolute.to_string_lossy()
        );
    }

    /// A `..` after a symbolic link goes up from where the link leads, as
    /// the OS resolves it, and a link whose target does not exist yet is
    /// followed too, as a write through it would be (#399).
    #[cfg(unix)]
    #[test]
    fn resolve_path_follows_each_link_before_the_dot_dot_after_it() {
        use std::os::unix::fs::symlink;
        let dir = std::env::temp_dir().join(format!("hd-grant-links-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("granted/sub/deep")).unwrap();
        std::fs::create_dir_all(dir.join("outside/deep")).unwrap();
        let real = hd_base::path::canonicalize(&dir).unwrap();
        symlink("../outside/deep", dir.join("granted/link")).unwrap();
        symlink("sub/deep", dir.join("granted/inlink")).unwrap();
        symlink("../outside/made", dir.join("granted/broken")).unwrap();
        for (path, resolved) in [
            ("granted/link/../x", "outside/x"),
            ("granted/link/new/../../y", "outside/y"),
            ("granted/inlink/../y", "granted/sub/y"),
            ("granted/sub/../y", "granted/y"),
            ("granted/broken", "outside/made"),
            ("granted/broken/z", "outside/made/z"),
            ("granted/missing/../link/made", "outside/deep/made"),
            ("granted/missing/a/../../y", "granted/y"),
        ] {
            assert_eq!(
                super::resolve_path(&dir.join(path)),
                real.join(resolved),
                "{path}"
            );
        }
        let g = Grants::resolve(&[kv("FsRead", scopes(&["granted"]))], &dir, &[], &dir);
        assert!(!g.covers_path("FsRead", Path::new("granted/link/../x"), &dir));
        assert!(g.covers_path("FsRead", Path::new("granted/inlink/../y"), &dir));
        for (written, shown) in [
            ("granted/link/../x", "outside/x"),
            ("granted/link/x", "outside/deep/x"),
            ("granted/broken/z", "outside/made/z"),
            ("granted/sub/../y", "granted/sub/../y"),
        ] {
            assert_eq!(super::refused_path(Path::new(written), &dir, true), shown);
        }
        assert_eq!(
            super::refused_path(Path::new("link/../x"), &dir.join("granted"), true),
            real.join("outside/x").to_string_lossy()
        );
        assert_eq!(
            super::refused_path(Path::new("granted/link"), &dir, false),
            "granted/link"
        );
        std::fs::remove_dir_all(&dir).unwrap();
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
