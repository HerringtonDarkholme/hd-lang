//! The versions a selection reads, from the cache or fetched into it
//! (`cli.dep.implicit-fetch`, `cli.dep.fetch`). A version the cache holds
//! needs no git and no network (`cli.dep.offline`).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use hd_diag::Code;
use hd_project::{HostPath, Version, compatibility_line};

use crate::cache::{Cache, Entry, Scratch};
use crate::git::{Git, GitError, TagRelation};
use crate::query::{Pick, Query, Refs, pick, pseudo_version, tagged_version};
use crate::select::Source;
use crate::sum::tree_key;
use crate::version::{PseudoCommit, pseudo_base, pseudo_commit, tag_name, tag_version};

/// The version a query selects (`cli.dep.query.*`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub version: Version,
    /// Whether the package has no version tag, so the version is the
    /// pseudo-version of the default branch's head
    /// (`cli.dep.query.latest.untagged`).
    pub untagged: bool,
}

/// The cache, and git to fill it.
pub struct Remote<'a> {
    cache: Cache,
    git: Git,
    /// The refs of each repository listed so far, or why listing failed.
    refs: HashMap<String, Result<Refs, Unreachable>>,
    /// A scratch repository per repository whose branches and tags were
    /// fetched in this run, which later pseudo-versions read too.
    full: HashMap<String, Scratch>,
    /// The URL that answered for each repository, which its later git
    /// commands use too.
    answered: HashMap<String, String>,
    /// The hosts that answered over SSH in this run, which are tried over
    /// SSH first from then on.
    ssh_hosts: HashSet<String>,
    /// Called with `HOST_PATH@VERSION` before each fetch.
    progress: &'a mut dyn FnMut(&str),
}

/// The code and message of an error.
pub type Failure = (Option<Code>, String);

/// The host of a repository part, as `github.com` of `github.com/acme/tools`.
fn host_of(repository: &str) -> &str {
    repository.split('/').next().unwrap_or(repository)
}

/// The URLs tried for a repository, each with why git could not use it.
#[derive(Clone, Debug)]
struct Unreachable {
    host: String,
    tried: Vec<(String, GitError)>,
}

impl Unreachable {
    fn new(repository: &str, tried: Vec<(String, GitError)>) -> Unreachable {
        Unreachable {
            host: host_of(repository).to_owned(),
            tried,
        }
    }

    /// The message of what `hd` could not do, as `fetch KEY`, naming each
    /// URL with git's last line, and how to settle on one scheme
    /// (`cli.dep.no-secret`).
    fn message(&self, what: &str) -> String {
        match self.tried.as_slice() {
            [(url, e)] => format!(
                "cannot {what} from {url}: {}; `hd` uses git and git's own credentials, so check that `git ls-remote {url}` works",
                e.0
            ),
            tried => {
                let host = &self.host;
                let failures: Vec<String> = tried
                    .iter()
                    .map(|(url, e)| format!("{url} failed: {}", e.0))
                    .collect();
                format!(
                    "cannot {what}: {}; to use SSH only, run `git config --global url.\"git@{host}:\".insteadOf \"https://{host}/\"`; to use HTTPS, configure a git credential helper",
                    failures.join("; ")
                )
            }
        }
    }
}

impl<'a> Remote<'a> {
    pub fn new(cache: Cache, progress: &'a mut dyn FnMut(&str)) -> Remote<'a> {
        Remote {
            cache,
            git: Git::new(),
            refs: HashMap::new(),
            full: HashMap::new(),
            answered: HashMap::new(),
            ssh_hosts: HashSet::new(),
            progress,
        }
    }

    /// Runs `run` with the URL of `repository`: `https://` first, then
    /// `ssh://git@` when that fails for any reason, in the other order for
    /// a host that answered over SSH earlier in this run. A repository
    /// that answered keeps its URL (`cli.dep.repo-url`).
    fn reach<T>(
        &mut self,
        repository: &str,
        mut run: impl FnMut(&Git, &str) -> Result<T, GitError>,
    ) -> Result<T, Unreachable> {
        let host = host_of(repository);
        let urls = if let Some(url) = self.answered.get(repository) {
            vec![url.clone()]
        } else {
            let https = format!("https://{repository}");
            let ssh = format!("ssh://git@{repository}");
            if self.ssh_hosts.contains(host) {
                vec![ssh, https]
            } else {
                vec![https, ssh]
            }
        };
        let mut tried = Vec::new();
        for url in urls {
            match run(&self.git, &url) {
                Ok(done) => {
                    if url.starts_with("ssh://") {
                        self.ssh_hosts.insert(host.to_owned());
                    }
                    self.answered.insert(repository.to_owned(), url);
                    return Ok(done);
                }
                Err(e) => tried.push((url, e)),
            }
        }
        Err(Unreachable::new(repository, tried))
    }

    /// The refs of `host`'s repository, listed once.
    fn refs_of(&mut self, host: &HostPath, what: &str) -> Result<Refs, Failure> {
        let failed = |message: String| (Some(Code::FetchFailed), message);
        if let Some(listed) = self.refs.get(&host.repository) {
            return listed.clone().map_err(|e| failed(e.message(what)));
        }
        let scratch = self.cache.scratch().map_err(|e| (None, e))?;
        let repo = scratch.path.join("repo.git");
        self.git
            .init(&repo)
            .map_err(|e| failed(format!("cannot {what}: {}", e.0)))?;
        let listed = self.reach(&host.repository, |git, url| git.list_refs(&repo, url));
        self.refs.insert(host.repository.clone(), listed.clone());
        listed.map_err(|e| failed(e.message(what)))
    }

    /// Every version that a tag of the package's repository names for it,
    /// in no order, from the repository's tag list (`cli.dep.update`,
    /// `cli.dep.update.network`). The list is read from the network once
    /// per repository.
    pub fn versions(&mut self, host: &HostPath) -> Result<Vec<Version>, Failure> {
        let refs = self.refs_of(host, &format!("list the tags of {}", host.path))?;
        Ok(refs
            .tags
            .keys()
            .filter_map(|t| tag_version(host, t))
            .collect())
    }

    /// A repository with every branch and tag of `host`'s repository,
    /// fetched once per run (`cli.dep.pseudo`).
    fn full_repo(&mut self, host: &HostPath, what: &str) -> Result<PathBuf, Failure> {
        if let Some(scratch) = self.full.get(&host.repository) {
            return Ok(scratch.path.join("repo.git"));
        }
        let failed = |message: String| (Some(Code::FetchFailed), message);
        let scratch = self.cache.scratch().map_err(|e| (None, e))?;
        let repo = scratch.path.join("repo.git");
        self.git
            .init(&repo)
            .map_err(|e| failed(format!("cannot {what}: {}", e.0)))?;
        self.reach(&host.repository, |git, url| git.fetch_all(&repo, url))
            .map_err(|e| failed(e.message(what)))?;
        self.full.insert(host.repository.clone(), scratch);
        Ok(repo)
    }

    /// The version that `query` selects for the package at `host`
    /// (Version Queries). An exact version selects itself and needs no
    /// network (`cli.dep.query.exact`, `cli.dep.query.network`); its tag
    /// is checked when it is fetched. Any other query lists the
    /// repository's refs, and a branch or a commit fetches its history to
    /// make the commit's pseudo-version (`cli.dep.query.revision`,
    /// `cli.dep.query.revision.tagged`).
    pub fn resolve(&mut self, host: &HostPath, query: &Query) -> Result<Resolved, Failure> {
        if let Query::Exact(version) = query {
            return Ok(Resolved {
                version: version.clone(),
                untagged: false,
            });
        }
        let refs = self.refs_of(host, &format!("list the tags of {}", host.path))?;
        let unknown = |message: String| (Some(Code::UnknownVersion), message);
        let picked = pick(host, &refs, query, &host.path).map_err(unknown)?;
        let what = format!("fetch the history of {}", host.path);
        let failed = |e: GitError| (Some(Code::FetchFailed), format!("cannot {what}: {}", e.0));
        let (commit, untagged) = match picked {
            Pick::Version(version) => {
                return Ok(Resolved {
                    version,
                    untagged: false,
                });
            }
            Pick::Commit { commit, untagged } => {
                // A branch's head may carry a version tag.
                if let Some(version) = tagged_version(host, &refs, &commit).filter(|_| !untagged) {
                    return Ok(Resolved {
                        version,
                        untagged: false,
                    });
                }
                let repo = self.full_repo(host, &what)?;
                (self.git.commit(&repo, &commit).map_err(failed)?, untagged)
            }
            Pick::Prefix(prefix) => {
                let repo = self.full_repo(host, &what)?;
                let Some(full) = self.git.commit(&repo, &prefix).map_err(failed)? else {
                    return Err(unknown(format!(
                        "{} has no branch `{prefix}` and no commit whose hash starts with {prefix} on a branch or tag",
                        host.path
                    )));
                };
                if let Some(version) = tagged_version(host, &refs, &full) {
                    return Ok(Resolved {
                        version,
                        untagged: false,
                    });
                }
                (Some(full), false)
            }
        };
        let Some(commit) = commit else {
            return Err(unknown(format!(
                "{} lists a commit that its history does not hold",
                host.path
            )));
        };
        let repo = self.full_repo(host, &what)?;
        let time = self.git.commit_time(&repo, &commit).map_err(failed)?;
        let base = if untagged {
            None
        } else {
            self.git
                .merged_tags(&repo, &commit)
                .map_err(failed)?
                .iter()
                .filter_map(|t| tag_version(host, t))
                .max()
        };
        let version = pseudo_version(base.as_ref(), &time, &commit).ok_or_else(|| {
            unknown(format!(
                "the commit {commit} of {} has no pseudo-version",
                host.path
            ))
        })?;
        Ok(Resolved { version, untagged })
    }

    /// Fetches a version into the cache.
    fn fetch(&mut self, host: &HostPath, version: &Version, key: &str) -> Result<Entry, Failure> {
        let what = format!("fetch {key}");
        let failed = |message: String| (Some(Code::FetchFailed), message);
        let local = |e: GitError| failed(format!("cannot {what}: {}", e.0));
        let scratch = self.cache.scratch().map_err(|e| (None, e))?;
        // `cli.dep.repo-url`: the URL that answered the first git command
        // on the repository serves the later ones.
        let (repo, rev) = if let Some(commit) = pseudo_commit(version) {
            (self.progress)(key);
            let repo = self.full_repo(host, &what)?;
            let rev = self
                .pseudo_rev(&repo, host, version, &commit)
                .map_err(|e| e.unwrap_or_else(&local))?;
            (repo, rev)
        } else {
            let tag = tag_name(host, version);
            let tags = self.refs_of(host, &what)?.tags;
            // A missing tag is an error, with no fallback
            // (`module.version.tag-missing`, `module.version.no-fallback`).
            if !tags.contains_key(&tag) {
                let mut near: Vec<Version> = tags
                    .keys()
                    .filter_map(|t| tag_version(host, t))
                    .filter(|v| compatibility_line(v) == compatibility_line(version))
                    .collect();
                near.sort();
                let line = compatibility_line(version);
                let listed = if near.is_empty() {
                    format!("it has no tag on line {line}")
                } else {
                    let shown: Vec<String> = near.iter().map(ToString::to_string).collect();
                    format!("its versions on line {line} are {}", shown.join(", "))
                };
                return Err((
                    Some(Code::UnknownVersion),
                    format!(
                        "{} has no tag {tag}, so version {version} does not exist; {listed}",
                        host.path
                    ),
                ));
            }
            let repo = scratch.path.join("repo.git");
            self.git.init(&repo).map_err(local)?;
            (self.progress)(key);
            self.reach(&host.repository, |git, url| git.fetch_tag(&repo, url, &tag))
                .map_err(|e| failed(e.message(&what)))?;
            (repo, "FETCH_HEAD".to_owned())
        };
        let tree = scratch.path.join("tree");
        let found = self
            .git
            .export_tree(&repo, &rev, &host.subdirectory, &tree)
            .map_err(local)?;
        if !found || !tree.join("hd.toml").is_file() {
            let at = match pseudo_commit(version) {
                Some(c) => format!("the commit {}", c.hash),
                None => format!("the tag {}", tag_name(host, version)),
            };
            let manifest = if host.subdirectory.is_empty() {
                "hd.toml".to_owned()
            } else {
                format!("{}/hd.toml", host.subdirectory)
            };
            // `cli.dep.target.no-manifest`: the packages below it.
            let below = self
                .git
                .manifest_dirs(&repo, &rev, &host.subdirectory)
                .map_err(local)?;
            let found = if below.is_empty() {
                String::new()
            } else {
                let first = below[0].trim_end_matches('/');
                format!(
                    "; found packages at {}; add one by its directory, as {}/{first}",
                    below.join(", "),
                    host.path
                )
            };
            return Err((
                Some(Code::InvalidRequirement),
                format!("{key} is no package: {at} has no {manifest}{found}"),
            ));
        }
        self.cache.store(key, &tree).map_err(|e| (None, e))
    }

    /// The commit of a pseudo-version, checked against its time and base
    /// tag (`cli.dep.pseudo`, `cli.dep.pseudo.unknown`,
    /// `cli.dep.pseudo.base.*`); `Err(Ok(..))` for one that names no
    /// fitting commit, `Err(Err(..))` when git fails.
    fn pseudo_rev(
        &self,
        repo: &Path,
        host: &HostPath,
        version: &Version,
        commit: &PseudoCommit,
    ) -> Result<String, Result<Failure, GitError>> {
        let unknown = |message: String| Err(Ok((Some(Code::UnknownVersion), message)));
        let Some(full) = self.git.commit(repo, &commit.hash).map_err(Err)? else {
            return unknown(format!(
                "{} has no commit {} on a branch or tag, so the pseudo-version {version} does not exist",
                host.path, commit.hash
            ));
        };
        let time = self.git.commit_time(repo, &full).map_err(Err)?;
        if time != commit.time {
            let fixed = version.to_string().replace(&commit.time, &time);
            return unknown(format!(
                "the commit {} of {} has the time {time}, not {}, so the pseudo-version {version} names no commit; did you mean {fixed}?",
                commit.hash, host.path, commit.time
            ));
        }
        let Some(base) = pseudo_base(version) else {
            return Ok(full);
        };
        let tag = tag_name(host, &base);
        match self.git.tag_relation(repo, &tag, &full).map_err(Err)? {
            TagRelation::Descends => Ok(full),
            TagRelation::Missing => unknown(format!(
                "{} has no tag {tag}, which the pseudo-version {version} names as its base; a pseudo-version's base is a tag on its commit's history",
                host.path
            )),
            TagRelation::Same => unknown(format!(
                "the commit {} of {} is the tag {tag} itself, so require {base} instead of the pseudo-version {version}",
                commit.hash, host.path
            )),
            TagRelation::Unrelated => unknown(format!(
                "the commit {} of {} does not descend from the tag {tag}, which the pseudo-version {version} names as its base, so the pseudo-version would order above releases it does not follow",
                commit.hash, host.path
            )),
        }
    }
}

impl Source for Remote<'_> {
    fn entry(&mut self, host: &HostPath, version: &Version) -> Result<Entry, Failure> {
        let key = tree_key(&host.path, version);
        if let Some(entry) = self.cache.cached(&key).map_err(|e| (None, e))? {
            return Ok(entry);
        }
        self.fetch(host, version, &key)
    }
}
