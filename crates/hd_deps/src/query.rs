//! Version queries (Version Queries, `cli.dep.query.*`): what a query
//! means, which version a repository's refs give it, and the pseudo-version
//! of a commit (`module.version.pseudo`). Everything here is pure; the
//! listing and fetching around it are `Remote`'s.

use hd_base::SliceSort as _;
use std::collections::BTreeMap;

use hd_project::{HostPath, Version};

use crate::version::tag_version;

/// A query, by the first row of the table it matches
/// (`cli.dep.query.order`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Query {
    /// No query, or `latest` (`cli.dep.query.latest`).
    Latest,
    /// A version or pseudo-version, without its `v` (`cli.dep.query.exact`,
    /// `cli.dep.query.v-prefix`).
    Exact(Version),
    /// `X` or `X.Y` (`cli.dep.query.partial`).
    Partial(u64, Option<u64>),
    /// A branch name, or a commit hash or a prefix of one
    /// (`cli.dep.query.revision`).
    Revision(String),
}

/// A number of a partial version: digits without a leading zero.
fn number(s: &str) -> Option<u64> {
    let ok =
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0'));
    if ok { s.parse().ok() } else { None }
}

/// Whether `name` could name a branch: git's ref name rules, roughly, and
/// never a word git would read as an option.
fn branch_like(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.starts_with('/')
        && !name.starts_with('.')
        && !name.ends_with('/')
        && !name.ends_with('.')
        && name.rsplit_once('.').is_none_or(|(_, ext)| ext != "lock")
        && !name.contains("..")
        && !name.contains("@{")
        && !name.contains("//")
        && name
            .chars()
            .all(|c| !c.is_control() && !c.is_whitespace() && !"~^:?*[\\".contains(c))
}

/// The query of a target, or why it is none (`cli.dep.add.check`).
pub fn parse_query(text: Option<&str>) -> Result<Query, String> {
    let Some(text) = text else {
        return Ok(Query::Latest);
    };
    if text == "latest" {
        return Ok(Query::Latest);
    }
    let bare = text.strip_prefix('v').unwrap_or(text);
    if let Some(v) = Version::parse(bare) {
        return Ok(Query::Exact(v));
    }
    let mut parts = bare.split('.');
    if let Some(major) = parts.next().and_then(number) {
        match (parts.next(), parts.next()) {
            (None, _) => return Ok(Query::Partial(major, None)),
            (Some(minor), None) => {
                if let Some(minor) = number(minor) {
                    return Ok(Query::Partial(major, Some(minor)));
                }
            }
            _ => {}
        }
    }
    if branch_like(text) {
        Ok(Query::Revision(text.to_owned()))
    } else if text.is_empty() {
        Err("nothing follows the `@`: write a version, as `@2.1.0`, or leave the `@` out for the newest release".to_owned())
    } else {
        Err(format!(
            "`{text}` is no version query: write a version, as `2.1.0`, a partial one, as `2` or `2.1`, `latest`, a branch name, or a commit hash"
        ))
    }
}

/// The refs a repository lists: the commit of its default branch's head,
/// and of each branch and tag (an annotated tag peeled to its commit).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Refs {
    pub head: Option<String>,
    pub heads: BTreeMap<String, String>,
    pub tags: BTreeMap<String, String>,
}

/// What a query picks from a repository's refs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pick {
    /// A tagged version, or the exact version the query named.
    Version(Version),
    /// A commit, whose version is its tag's or its pseudo-version;
    /// `untagged` when the package has no version tag at all
    /// (`cli.dep.query.latest.untagged`).
    Commit { commit: String, untagged: bool },
    /// A prefix of a commit hash, which only the fetched history resolves.
    Prefix(String),
}

/// Whether a query names a commit by a prefix of its hash.
fn hash_prefix(text: &str) -> Option<String> {
    (4..=40)
        .contains(&text.len())
        .then(|| text.to_ascii_lowercase())
        .filter(|t| t.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Every version a tag of `refs` names for the package at `host`
/// (`cli.dep.query.tags`).
#[must_use]
pub fn tag_versions(host: &HostPath, refs: &Refs) -> Vec<Version> {
    refs.tags
        .keys()
        .filter_map(|t| tag_version(host, t))
        .collect()
}

/// The greatest release among `versions`, else the greatest pre-release.
fn newest(versions: impl Iterator<Item = Version> + Clone) -> Option<Version> {
    versions
        .clone()
        .filter(|v| v.pre.is_empty())
        .max()
        .or_else(|| versions.max())
}

/// What `query` picks for the package at `host`, or the message of
/// `cli.dep.query.unknown`. `shown` is how the message names the package.
pub fn pick(host: &HostPath, refs: &Refs, query: &Query, shown: &str) -> Result<Pick, String> {
    let versions = tag_versions(host, refs);
    match query {
        Query::Exact(v) => Ok(Pick::Version(v.clone())),
        Query::Latest => {
            if let Some(v) = newest(versions.into_iter()) {
                return Ok(Pick::Version(v));
            }
            match &refs.head {
                Some(commit) => Ok(Pick::Commit {
                    commit: commit.clone(),
                    untagged: true,
                }),
                None => Err(format!(
                    "{shown} has no version tag and no default branch, so it has no version to require"
                )),
            }
        }
        Query::Partial(major, minor) => {
            let fits = versions
                .into_iter()
                .filter(|v| v.major == *major && minor.is_none_or(|m| v.minor == m));
            newest(fits).map(Pick::Version).ok_or_else(|| {
                let wanted = match minor {
                    Some(m) => format!("{major}.{m}"),
                    None => major.to_string(),
                };
                let all: Vec<Version> = tag_versions(host, refs);
                let listed = if all.is_empty() {
                    "it has no version tag".to_owned()
                } else {
                    let mut all = all;
                    all.sort_stable();
                    let shown: Vec<String> = all.iter().map(ToString::to_string).collect();
                    format!("its versions are {}", shown.join(", "))
                };
                format!("{shown} has no version {wanted}.x; {listed}")
            })
        }
        Query::Revision(name) => {
            if let Some(commit) = refs.heads.get(name) {
                return Ok(Pick::Commit {
                    commit: commit.clone(),
                    untagged: false,
                });
            }
            hash_prefix(name).map(Pick::Prefix).ok_or_else(|| {
                format!("{shown} has no branch `{name}`, and `{name}` is no commit hash")
            })
        }
    }
}

/// The greatest version of the package at `host` whose tag names `commit`
/// (`cli.dep.query.revision.tagged`).
#[must_use]
pub fn tagged_version(host: &HostPath, refs: &Refs, commit: &str) -> Option<Version> {
    refs.tags
        .iter()
        .filter(|(_, c)| c.as_str() == commit)
        .filter_map(|(t, _)| tag_version(host, t))
        .max()
}

/// The pseudo-version of `commit`, made at the UTC time `time`
/// (`yyyymmddhhmmss`), after the base tag version `base`, by Go's three
/// forms (`module.version.pseudo`, `module.version.pseudo.commit`).
#[must_use]
pub fn pseudo_version(base: Option<&Version>, time: &str, commit: &str) -> Option<Version> {
    let hash = commit.get(..12)?.to_ascii_lowercase();
    let text = match base {
        None => format!("0.0.0-{time}-{hash}"),
        Some(b) if b.pre.is_empty() => format!(
            "{}.{}.{}-0.{time}-{hash}",
            b.major,
            b.minor,
            b.patch.checked_add(1)?
        ),
        Some(b) => format!("{b}.0.{time}-{hash}"),
    };
    Version::parse(&text)
}

#[cfg(test)]
mod tests {
    use super::{Pick, Query, Refs, parse_query, pick, pseudo_version, tagged_version};
    use hd_project::{Version, host_path};

    fn v(s: &str) -> Version {
        Version::parse(s).expect(s)
    }

    fn refs(tags: &[(&str, &str)], heads: &[(&str, &str)], head: Option<&str>) -> Refs {
        Refs {
            head: head.map(str::to_owned),
            heads: heads
                .iter()
                .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
                .collect(),
            tags: tags
                .iter()
                .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
                .collect(),
        }
    }

    #[test]
    fn queries_take_the_first_matching_row() {
        assert_eq!(parse_query(None), Ok(Query::Latest));
        assert_eq!(parse_query(Some("latest")), Ok(Query::Latest));
        assert_eq!(parse_query(Some("2.1.0")), Ok(Query::Exact(v("2.1.0"))));
        assert_eq!(parse_query(Some("v2.1.0")), Ok(Query::Exact(v("2.1.0"))));
        assert_eq!(
            parse_query(Some("v1.0.0-rc.1")),
            Ok(Query::Exact(v("1.0.0-rc.1")))
        );
        assert_eq!(
            parse_query(Some("0.0.0-20260912081500-3f2c9e1a7b6d")),
            Ok(Query::Exact(v("0.0.0-20260912081500-3f2c9e1a7b6d")))
        );
        assert_eq!(parse_query(Some("2")), Ok(Query::Partial(2, None)));
        assert_eq!(parse_query(Some("v2.1")), Ok(Query::Partial(2, Some(1))));
        assert_eq!(
            parse_query(Some("main")),
            Ok(Query::Revision("main".to_owned()))
        );
        assert_eq!(
            parse_query(Some("9a1b2c3")),
            Ok(Query::Revision("9a1b2c3".to_owned()))
        );
        assert_eq!(
            parse_query(Some("2.1.0.4")),
            Ok(Query::Revision("2.1.0.4".to_owned()))
        );
        for bad in [
            "",
            "-x",
            "--upload-pack=x",
            "a b",
            "a..b",
            "x~1",
            "a:b",
            "x.lock",
        ] {
            assert!(parse_query(Some(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn latest_prefers_a_release_then_a_pre_release_then_the_head() {
        let json = host_path("github.com/acme/json").expect("host");
        let tagged = refs(
            &[
                ("v2.3.0", "c3"),
                ("v2.2.0", "c2"),
                ("v3.0.0-rc.1", "c4"),
                ("other", "c5"),
            ],
            &[],
            Some("c5"),
        );
        assert_eq!(
            pick(&json, &tagged, &Query::Latest, "json"),
            Ok(Pick::Version(v("2.3.0")))
        );
        let beta = refs(
            &[("v0.2.0-rc.1", "a"), ("v0.2.0-rc.2", "b")],
            &[],
            Some("b"),
        );
        assert_eq!(
            pick(&json, &beta, &Query::Latest, "json"),
            Ok(Pick::Version(v("0.2.0-rc.2")))
        );
        let none = refs(&[("release-1", "a")], &[("main", "h")], Some("h"));
        assert_eq!(
            pick(&json, &none, &Query::Latest, "json"),
            Ok(Pick::Commit {
                commit: "h".to_owned(),
                untagged: true
            })
        );
        assert!(pick(&json, &Refs::default(), &Query::Latest, "json").is_err());
    }

    #[test]
    fn partial_queries_read_the_package_tags_only() {
        let lint = host_path("github.com/acme/tools/lint").expect("host");
        let r = refs(
            &[
                ("lint/v2.1.0", "a"),
                ("lint/v2.1.3", "b"),
                ("lint/v2.2.0-rc.1", "c"),
                ("v2.9.0", "d"),
                ("lint/v3.0.0-beta", "e"),
            ],
            &[],
            None,
        );
        let got = |q: Query| pick(&lint, &r, &q, "lint");
        assert_eq!(got(Query::Partial(2, None)), Ok(Pick::Version(v("2.1.3"))));
        assert_eq!(
            got(Query::Partial(2, Some(1))),
            Ok(Pick::Version(v("2.1.3")))
        );
        assert_eq!(
            got(Query::Partial(2, Some(2))),
            Ok(Pick::Version(v("2.2.0-rc.1")))
        );
        assert_eq!(
            got(Query::Partial(3, None)),
            Ok(Pick::Version(v("3.0.0-beta")))
        );
        assert!(got(Query::Partial(2, Some(9))).is_err());
        assert!(got(Query::Latest).is_ok_and(|p| p == Pick::Version(v("2.1.3"))));
    }

    #[test]
    fn revisions_are_branches_then_hash_prefixes() {
        let json = host_path("github.com/acme/json").expect("host");
        let r = refs(
            &[("v1.0.0", "aaa")],
            &[("main", "bbb"), ("feature/x", "ccc")],
            Some("bbb"),
        );
        let got = |q: &str| pick(&json, &r, &Query::Revision(q.to_owned()), "json");
        assert_eq!(
            got("feature/x"),
            Ok(Pick::Commit {
                commit: "ccc".to_owned(),
                untagged: false
            })
        );
        assert_eq!(got("9A1B2C3"), Ok(Pick::Prefix("9a1b2c3".to_owned())));
        assert!(got("nosuch").is_err());
        assert!(got("abc").is_err());
    }

    #[test]
    fn a_tagged_commit_takes_its_greatest_version() {
        let json = host_path("github.com/acme/json").expect("host");
        let r = refs(
            &[
                ("v1.0.0", "c1"),
                ("v1.0.1", "c1"),
                ("lint/v9.0.0", "c1"),
                ("v2.0.0", "c2"),
            ],
            &[],
            None,
        );
        assert_eq!(tagged_version(&json, &r, "c1"), Some(v("1.0.1")));
        assert_eq!(tagged_version(&json, &r, "c3"), None);
    }

    #[test]
    fn pseudo_versions_take_their_base_form() {
        let commit = "9a1b2c3d4e5f60718293a4b5c6d7e8f901234567";
        let t = "20261002093000";
        assert_eq!(
            pseudo_version(None, t, commit),
            Some(v("0.0.0-20261002093000-9a1b2c3d4e5f"))
        );
        assert_eq!(
            pseudo_version(Some(&v("2.3.0")), t, commit),
            Some(v("2.3.1-0.20261002093000-9a1b2c3d4e5f"))
        );
        assert_eq!(
            pseudo_version(Some(&v("1.0.0-rc.1")), t, commit),
            Some(v("1.0.0-rc.1.0.20261002093000-9a1b2c3d4e5f"))
        );
        assert_eq!(pseudo_version(None, t, "abc"), None);
        // The forms round-trip through the pseudo-version reader.
        let made = pseudo_version(Some(&v("0.4.0")), t, commit).expect("made");
        let back = crate::version::pseudo_commit(&made).expect("pseudo");
        assert_eq!(
            (back.time.as_str(), back.hash.as_str()),
            (t, "9a1b2c3d4e5f")
        );
        assert_eq!(crate::version::pseudo_base(&made), Some(v("0.4.0")));
    }
}
