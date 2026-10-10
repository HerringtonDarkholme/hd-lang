//! The versions a selection reads, from the cache or fetched into it
//! (`cli.dep.implicit-fetch`, `cli.dep.fetch`). A version the cache holds
//! needs no git and no network (`cli.dep.offline`).

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use hd_diag::Code;
use hd_project::{HostPath, Version, compatibility_line};

use crate::cache::{Cache, Entry};
use crate::git::{Git, GitError, TagRelation};
use crate::select::Source;
use crate::sum::tree_key;
use crate::version::{PseudoCommit, pseudo_base, pseudo_commit, tag_name, tag_version};

/// The cache, and git to fill it.
pub struct Remote<'a> {
    cache: Cache,
    git: Git,
    /// The tags of each repository URL listed so far, or why listing
    /// failed.
    tags: HashMap<String, Result<BTreeMap<String, String>, GitError>>,
    /// Called with `HOST_PATH@VERSION` before each fetch.
    progress: &'a mut dyn FnMut(&str),
}

/// The code and message of an error.
pub type Failure = (Option<Code>, String);

impl<'a> Remote<'a> {
    pub fn new(cache: Cache, progress: &'a mut dyn FnMut(&str)) -> Remote<'a> {
        Remote {
            cache,
            git: Git::new(),
            tags: HashMap::new(),
            progress,
        }
    }

    /// The tags of the repository at `url`, listed once, with `repo` as
    /// the repository git runs in.
    fn tags_of(&mut self, url: &str, repo: &Path) -> Result<BTreeMap<String, String>, GitError> {
        self.tags
            .entry(url.to_owned())
            .or_insert_with(|| self.git.list_tags(repo, url))
            .clone()
    }

    /// Every version that a tag of the package's repository names for it,
    /// in no order, from the repository's tag list (`cli.dep.update`,
    /// `cli.dep.update.network`). The list is read from the network once
    /// per repository.
    pub fn versions(&mut self, host: &HostPath) -> Result<Vec<Version>, Failure> {
        let url = format!("https://{}", host.repository);
        let failed = |e: String| {
            (
                Some(Code::FetchFailed),
                format!(
                    "cannot list the tags of {} at {url}: {e}; `hd` reads them with git and git's own credentials, so check that `git ls-remote {url}` works",
                    host.path
                ),
            )
        };
        let tags = if let Some(listed) = self.tags.get(&url) {
            listed.clone()
        } else {
            let scratch = self.cache.scratch().map_err(|e| (None, e))?;
            let repo = scratch.path.join("repo.git");
            self.git.init(&repo).map_err(|e| failed(e.0))?;
            self.tags_of(&url, &repo)
        }
        .map_err(|e| failed(e.0))?;
        Ok(tags.keys().filter_map(|t| tag_version(host, t)).collect())
    }

    /// Fetches a version into the cache.
    fn fetch(&mut self, host: &HostPath, version: &Version, key: &str) -> Result<Entry, Failure> {
        // `cli.dep.repo-url`.
        let url = format!("https://{}", host.repository);
        let failed = |e: GitError| {
            (
                Some(Code::FetchFailed),
                format!(
                    "cannot fetch {key} from {url}: {}; `hd` fetches with git and git's own credentials, so check that `git ls-remote {url}` works",
                    e.0
                ),
            )
        };
        let scratch = self.cache.scratch().map_err(|e| (None, e))?;
        let repo = scratch.path.join("repo.git");
        self.git.init(&repo).map_err(failed)?;
        let rev = if let Some(commit) = pseudo_commit(version) {
            (self.progress)(key);
            self.git.fetch_all(&repo, &url).map_err(failed)?;
            self.pseudo_rev(&repo, host, version, &commit)
                .map_err(|e| e.unwrap_or_else(&failed))?
        } else {
            let tag = tag_name(host, version);
            let tags = self.tags_of(&url, &repo).map_err(failed)?;
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
            (self.progress)(key);
            self.git.fetch_tag(&repo, &url, &tag).map_err(failed)?;
            "FETCH_HEAD".to_owned()
        };
        let tree = scratch.path.join("tree");
        let found = self
            .git
            .export_tree(&repo, &rev, &host.subdirectory, &tree)
            .map_err(failed)?;
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
            return Err((
                Some(Code::InvalidRequirement),
                format!("{key} is no package: {at} has no {manifest}"),
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
