//! Fetching with the system's `git` (`cli.dep.git`): git's own
//! configuration applies, so its credential helpers, SSH keys and
//! `url.<base>.insteadOf` rewrites reach a private repository, and `hd`
//! stores no credentials (`module.repo.credentials`,
//! `module.repo.no-stored-credentials`). git never asks a question
//! (`cli.dep.no-prompt`), and no message shows a credential
//! (`cli.dep.no-secret`).
//!
//! Every repository `hd` fetches into is a bare scratch repository of its
//! own, named with `--git-dir`, so neither the working directory's
//! repository nor a `GIT_DIR` that a hook set is read or written. A
//! version's files are read from git's objects, not checked out, so no
//! attribute, filter or line-ending setting changes their bytes.

use std::collections::BTreeMap;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::OnceLock;

/// Why git could not do what `hd` asked: git's last words, masked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitError(pub String);

/// Replaces the user name and password of each URL in `text` with `***`
/// (`cli.dep.no-secret`), as `https://***@github.com/acme/private`.
#[must_use]
pub fn mask_credentials(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("://") {
        let scheme_start = rest[..at]
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-')))
            .map_or(0, |i| i + 1);
        let scheme_ok = rest[scheme_start..at]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic());
        let after = &rest[at + 3..];
        let end = after
            .find(|c: char| c == '/' || c == '@' || c.is_whitespace())
            .unwrap_or(after.len());
        out.push_str(&rest[..at + 3]);
        if scheme_ok && end > 0 && after[end..].starts_with('@') {
            out.push_str("***");
            rest = &after[end..];
        } else {
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// The variables that would point git at another repository than the one
/// `hd` names, as a hook's `GIT_DIR` does; git runs without them.
const REPOSITORY_VARIABLES: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
    "GIT_PREFIX",
];

/// How a pseudo-version's commit relates to its base tag
/// (`cli.dep.pseudo.base.*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TagRelation {
    /// The repository has no such tag.
    Missing,
    /// The tag names the commit itself.
    Same,
    /// The tag's commit is an ancestor of the commit.
    Descends,
    Unrelated,
}

/// The system's `git`, with prompts turned off.
#[derive(Default)]
pub struct Git {
    /// Whether to run ssh in batch mode: when the user set no ssh command.
    batch_ssh: OnceLock<bool>,
}

impl Git {
    #[must_use]
    pub fn new() -> Git {
        Git::default()
    }

    /// A git command: prompts off (`GIT_TERMINAL_PROMPT=0`, no credential
    /// manager dialog), and ssh in batch mode with a connect timeout unless
    /// the user set an ssh command, so a passphrase, an unknown host or an
    /// unreachable one fails instead of asking or hanging.
    fn command(&self) -> Command {
        let mut c = git_command();
        c.env("GIT_TERMINAL_PROMPT", "0")
            .env("GCM_INTERACTIVE", "never")
            .stdin(Stdio::null());
        if *self.batch_ssh.get_or_init(no_ssh_command) {
            c.env("GIT_SSH_COMMAND", SSH_COMMAND);
        }
        c
    }

    fn run(&self, args: &[&str]) -> Result<Output, GitError> {
        self.command()
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| spawn_error(&e))
    }

    /// Runs git and fails with its complaint unless it succeeds.
    fn must(&self, args: &[&str]) -> Result<String, GitError> {
        let out = self.run(args)?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(complaint(&out))
        }
    }

    /// Makes a new bare repository at `repo` to fetch into.
    pub fn init(&self, repo: &Path) -> Result<(), GitError> {
        self.must(&["init", "--bare", "--quiet", &path_arg(repo)])
            .map(drop)
    }

    /// The tags of the repository at `url`, each with the object it names.
    /// A repository git cannot reach or read is an error.
    pub fn list_tags(&self, repo: &Path, url: &str) -> Result<BTreeMap<String, String>, GitError> {
        let out = self.must(&[&git_dir(repo), "ls-remote", "--tags", "--refs", url])?;
        Ok(out
            .lines()
            .filter_map(|l| {
                let (object, name) = l.split_once('\t')?;
                Some((
                    name.strip_prefix("refs/tags/")?.to_owned(),
                    object.to_owned(),
                ))
            })
            .collect())
    }

    /// Fetches the commit of `tag` from `url` into `repo`, shallowly, as
    /// `FETCH_HEAD`.
    pub fn fetch_tag(&self, repo: &Path, url: &str, tag: &str) -> Result<(), GitError> {
        self.must(&[
            &git_dir(repo),
            "fetch",
            "--quiet",
            "--depth",
            "1",
            "--no-tags",
            url,
            &format!("refs/tags/{tag}"),
        ])
        .map(drop)
    }

    /// Fetches every branch and tag of `url` into `repo`, to find the
    /// commit of a pseudo-version (`cli.dep.pseudo`).
    pub fn fetch_all(&self, repo: &Path, url: &str) -> Result<(), GitError> {
        self.must(&[
            &git_dir(repo),
            "fetch",
            "--quiet",
            "--no-tags",
            url,
            "+refs/heads/*:refs/remotes/origin/*",
            "+refs/tags/*:refs/tags/*",
        ])
        .map(drop)
    }

    /// The full hash of the commit of `repo` whose hash starts with
    /// `prefix`, or `None` when it has none.
    pub fn commit(&self, repo: &Path, prefix: &str) -> Result<Option<String>, GitError> {
        let out = self.run(&[
            &git_dir(repo),
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{prefix}^{{commit}}"),
        ])?;
        let hash = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        Ok((out.status.success() && hash.starts_with(prefix)).then_some(hash))
    }

    /// A commit's committer time in UTC, as `yyyymmddhhmmss`
    /// (`module.version.pseudo.commit`).
    pub fn commit_time(&self, repo: &Path, commit: &str) -> Result<String, GitError> {
        let out = self.must(&[&git_dir(repo), "log", "-1", "--format=%ct", commit, "--"])?;
        let seconds: i64 = out
            .trim()
            .parse()
            .map_err(|_| GitError(format!("git printed `{}` as a commit time", out.trim())))?;
        Ok(utc_stamp(seconds))
    }

    /// How `commit` relates to the tag `tag` of `repo`.
    pub fn tag_relation(
        &self,
        repo: &Path,
        tag: &str,
        commit: &str,
    ) -> Result<TagRelation, GitError> {
        let found = self.run(&[
            &git_dir(repo),
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/tags/{tag}^{{commit}}"),
        ])?;
        if !found.status.success() {
            return Ok(TagRelation::Missing);
        }
        let tagged = String::from_utf8_lossy(&found.stdout).trim().to_owned();
        if tagged == commit {
            return Ok(TagRelation::Same);
        }
        let ancestor = self.run(&[
            &git_dir(repo),
            "merge-base",
            "--is-ancestor",
            &tagged,
            commit,
        ])?;
        match ancestor.status.code() {
            Some(0) => Ok(TagRelation::Descends),
            Some(1) => Ok(TagRelation::Unrelated),
            _ => Err(complaint(&ancestor)),
        }
    }

    /// Writes the package directory `subdirectory` of the commit `rev` of
    /// `repo` into the new directory `into` (`cli.dep.tag`), its regular
    /// files only (`cli.sum.tree`): no symbolic link, no submodule, and no
    /// file of a subdirectory that holds its own `hd.toml`. Returns false
    /// when the commit has no such directory.
    pub fn export_tree(
        &self,
        repo: &Path,
        rev: &str,
        subdirectory: &str,
        into: &Path,
    ) -> Result<bool, GitError> {
        let tree = if subdirectory.is_empty() {
            format!("{rev}^{{tree}}")
        } else {
            format!("{rev}:{subdirectory}")
        };
        let kind = self.run(&[&git_dir(repo), "cat-file", "-t", &tree])?;
        if !kind.status.success() || String::from_utf8_lossy(&kind.stdout).trim() != "tree" {
            return Ok(false);
        }
        let listing = self.run(&[&git_dir(repo), "ls-tree", "-r", "-z", &tree])?;
        if !listing.status.success() {
            return Err(complaint(&listing));
        }
        let mut files: Vec<(String, String)> = Vec::new();
        for record in listing.stdout.split(|&b| b == 0).filter(|r| !r.is_empty()) {
            let record = String::from_utf8_lossy(record);
            let Some((meta, path)) = record.split_once('\t') else {
                continue;
            };
            let mut meta = meta.split(' ');
            let (Some(mode), Some(object)) = (meta.next(), meta.nth(1)) else {
                continue;
            };
            if mode != "100644" && mode != "100755" {
                continue;
            }
            if !safe_path(path) {
                return Err(GitError(format!(
                    "the tree holds the path `{path}`, which no package file can have"
                )));
            }
            files.push((path.to_owned(), object.to_owned()));
        }
        let packages: Vec<&str> = files
            .iter()
            .filter_map(|(p, _)| p.strip_suffix("/hd.toml"))
            .collect();
        let in_other_package = |path: &str| {
            packages.iter().any(|dir| {
                path.strip_prefix(dir)
                    .is_some_and(|rest| rest.starts_with('/'))
            })
        };
        let files: Vec<(String, String)> = files
            .iter()
            .filter(|(p, _)| !in_other_package(p))
            .cloned()
            .collect();
        std::fs::create_dir_all(into).map_err(|e| io_error(into, &e))?;
        let contents = self.blobs(repo, &files)?;
        for ((path, _), content) in files.iter().zip(contents) {
            let target = into.join(path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| io_error(parent, &e))?;
            }
            std::fs::write(&target, content).map_err(|e| io_error(&target, &e))?;
        }
        Ok(true)
    }

    /// The contents of the blobs of `files`, in order, from one
    /// `git cat-file --batch`.
    fn blobs(&self, repo: &Path, files: &[(String, String)]) -> Result<Vec<Vec<u8>>, GitError> {
        let mut child = self
            .command()
            .args([&git_dir(repo), "cat-file", "--batch"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| spawn_error(&e))?;
        let mut input = String::new();
        for (_, object) in files {
            input.push_str(object);
            input.push('\n');
        }
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| GitError("no stdin".into()))?;
        let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| GitError("no stdout".into()))?;
        let mut reader = BufReader::new(stdout);
        let mut out = Vec::with_capacity(files.len());
        for (path, object) in files {
            let mut header = String::new();
            reader
                .read_line(&mut header)
                .map_err(|e| GitError(e.to_string()))?;
            let size = header
                .trim_end()
                .strip_prefix(object.as_str())
                .and_then(|r| r.strip_prefix(" blob "))
                .and_then(|n| n.parse::<usize>().ok())
                .ok_or_else(|| GitError(format!("git cannot read `{path}`: {}", header.trim())))?;
            let mut content = vec![0; size];
            reader
                .read_exact(&mut content)
                .map_err(|e| GitError(e.to_string()))?;
            let mut newline = [0u8; 1];
            reader
                .read_exact(&mut newline)
                .map_err(|e| GitError(e.to_string()))?;
            out.push(content);
        }
        let _ = writer.join();
        let status = child
            .wait_with_output()
            .map_err(|e| GitError(e.to_string()))?;
        if !status.status.success() {
            return Err(complaint(&status));
        }
        Ok(out)
    }
}

/// The ssh command `hd` sets when the user set none: it never asks and
/// gives up on a host that does not answer.
const SSH_COMMAND: &str = "ssh -o BatchMode=yes -o ConnectTimeout=10";

/// A `git` child, run without the repository variables and in the C
/// locale, so its messages read the same on every machine.
fn git_command() -> Command {
    let mut c = Command::new("git");
    for v in REPOSITORY_VARIABLES {
        c.env_remove(v);
    }
    c.env("LC_ALL", "C");
    c
}

/// Whether the user set an ssh command, which then stays as it is: the
/// variables `GIT_SSH_COMMAND` or `GIT_SSH`, or git's `core.sshCommand`.
fn no_ssh_command() -> bool {
    if ["GIT_SSH_COMMAND", "GIT_SSH"]
        .iter()
        .any(|v| std::env::var_os(v).is_some_and(|s| !s.is_empty()))
    {
        return false;
    }
    let configured = git_command()
        .args(["config", "--global", "--get", "core.sshCommand"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .is_ok_and(|o| o.status.success() && !o.stdout.trim_ascii().is_empty());
    !configured
}

/// `--git-dir=REPO`.
fn git_dir(repo: &Path) -> String {
    format!("--git-dir={}", repo.display())
}

fn path_arg(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn spawn_error(e: &std::io::Error) -> GitError {
    if e.kind() == std::io::ErrorKind::NotFound {
        GitError("the `git` command is not installed or not on PATH".to_owned())
    } else {
        GitError(format!("git did not start: {e}"))
    }
}

fn io_error(path: &Path, e: &std::io::Error) -> GitError {
    GitError(format!("{}: {e}", PathBuf::from(path).display()))
}

/// The lines git adds to every failed connection, which say nothing about
/// the cause.
const GENERIC_COMPLAINTS: &[&str] = &[
    "Could not read from remote repository",
    "Please make sure you have the correct access rights",
    "and the repository exists",
];

/// git's complaint, masked, for a message (never the decision to fall
/// back, which needs only that git failed).
fn complaint(out: &Output) -> GitError {
    let stderr = String::from_utf8_lossy(&out.stderr);
    GitError(mask_credentials(
        &cause(&stderr).unwrap_or_else(|| format!("git exited with {}", out.status)),
    ))
}

/// The text of a line git prints as `fatal: TEXT` or `error: TEXT`.
fn message(line: &str) -> Option<&str> {
    line.strip_prefix("fatal: ")
        .or_else(|| line.strip_prefix("error: "))
}

/// The line of git's `stderr` that names the cause, as ssh's `Permission
/// denied (publickey)` or `fatal: repository ... not found`, not the
/// generic lines or the `hint:` advice around it: the last of git's own
/// `fatal:` or `error:` lines if there are any, else the last line, and
/// the generic ones only when nothing else is left.
fn cause(stderr: &str) -> Option<String> {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("hint:"))
        .collect();
    let informative: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|l| {
            let text = message(l).unwrap_or(l);
            !GENERIC_COMPLAINTS.iter().any(|g| text.starts_with(g))
        })
        .collect();
    let pool = if informative.is_empty() {
        &lines
    } else {
        &informative
    };
    pool.iter()
        .rev()
        .find_map(|l| message(l))
        .or_else(|| pool.last().copied())
        .map(|l| l.trim_end_matches('.').to_owned())
}

/// Whether a path of a tree is one a package file can have: relative
/// segments, none empty, `.`, `..` or `.git`.
fn safe_path(path: &str) -> bool {
    path.split('/').all(|s| {
        !s.is_empty()
            && s != "."
            && s != ".."
            && !s.eq_ignore_ascii_case(".git")
            && !s.contains('\\')
    })
}

/// Seconds since the Unix epoch as `yyyymmddhhmmss` in UTC.
fn utc_stamp(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let rem = seconds.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}{:02}{:02}{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::{cause, mask_credentials, safe_path, utc_stamp};

    #[test]
    fn the_cause_is_not_the_generic_trailer() {
        let denied = "git@github.com: Permission denied (publickey).\n\
            fatal: Could not read from remote repository.\n\n\
            Please make sure you have the correct access rights\n\
            and the repository exists.\n";
        assert_eq!(
            cause(denied).as_deref(),
            Some("git@github.com: Permission denied (publickey)")
        );
        let prompt =
            "fatal: could not read Username for 'https://github.com': terminal prompts disabled\n";
        assert_eq!(
            cause(prompt).as_deref(),
            Some("could not read Username for 'https://github.com': terminal prompts disabled")
        );
        let missing = "remote: Repository not found.\n\
            fatal: repository 'https://github.com/acme/x/' not found\n\
            hint: check the URL\n";
        assert_eq!(
            cause(missing).as_deref(),
            Some("repository 'https://github.com/acme/x/' not found")
        );
        let timeout = "ssh: connect to host example.com port 22: Connection timed out\n\
            fatal: Could not read from remote repository.\n";
        assert_eq!(
            cause(timeout).as_deref(),
            Some("ssh: connect to host example.com port 22: Connection timed out")
        );
        // Nothing else is left: the generic line stays.
        assert_eq!(
            cause("fatal: Could not read from remote repository.\n").as_deref(),
            Some("Could not read from remote repository")
        );
        assert_eq!(cause(""), None);
    }

    #[test]
    fn credentials_are_masked() {
        assert_eq!(
            mask_credentials("fatal: could not read https://bot:t0ken@github.com/acme/private"),
            "fatal: could not read https://***@github.com/acme/private"
        );
        assert_eq!(
            mask_credentials("'https://user@host.example/x' and ssh://git@h/y"),
            "'https://***@host.example/x' and ssh://***@h/y"
        );
        assert_eq!(
            mask_credentials("https://github.com/acme/a@1.0.0"),
            "https://github.com/acme/a@1.0.0"
        );
        assert_eq!(mask_credentials("no url here"), "no url here");
        assert_eq!(mask_credentials("://x@y"), "://x@y");
    }

    #[test]
    fn tree_paths_stay_inside() {
        assert!(safe_path("src/lib.hd"));
        assert!(safe_path(".github/ci.yml"));
        for bad in [
            "../x",
            "a/../b",
            "/abs",
            "a//b",
            ".git/config",
            "a/.GIT/x",
            "a\\b",
        ] {
            assert!(!safe_path(bad), "{bad}");
        }
    }

    #[test]
    fn utc_stamps() {
        assert_eq!(utc_stamp(0), "19700101000000");
        assert_eq!(utc_stamp(1_757_665_500), "20250912082500");
        assert_eq!(utc_stamp(951_782_400), "20000229000000");
    }
}
