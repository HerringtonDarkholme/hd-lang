//! Versions as git tags (`module.version.*`): a version's tag, the version
//! a tag names, and the commit and base tag of a pseudo-version. Parsing
//! and `SemVer` precedence are `hd_project::Version`'s.

use hd_project::{HostPath, Version};

/// A version's tag: `vVERSION` at the repository root, or prefixed with the
/// package's directory (`module.version.tag`, `module.version.tag-prefix`).
#[must_use]
pub fn tag_name(host: &HostPath, version: &Version) -> String {
    if host.subdirectory.is_empty() {
        format!("v{version}")
    } else {
        format!("{}/v{version}", host.subdirectory)
    }
}

/// The version a tag names for the package at `host`, or `None` when it
/// names none of its versions.
#[must_use]
pub fn tag_version(host: &HostPath, tag: &str) -> Option<Version> {
    let rest = if host.subdirectory.is_empty() {
        tag
    } else {
        tag.strip_prefix(host.subdirectory.as_str())?
            .strip_prefix('/')?
    };
    Version::parse(rest.strip_prefix('v')?)
}

/// The commit a pseudo-version names (`module.version.pseudo.commit`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PseudoCommit {
    /// The commit's UTC time, `yyyymmddhhmmss`.
    pub time: String,
    /// The first 12 lowercase hexadecimal digits of the commit hash.
    pub hash: String,
}

/// The commit of a pseudo-version, or `None` when `version` has none of
/// Go's three forms (`module.version.pseudo`): `0.0.0-TIME-HASH`,
/// `X.Y.(Z+1)-0.TIME-HASH` and `X.Y.Z-PRE.0.TIME-HASH`.
#[must_use]
pub fn pseudo_commit(version: &Version) -> Option<PseudoCommit> {
    let (last, base) = version.pre.split_last()?;
    let (time, hash) = last.split_once('-')?;
    let digits = |s: &str, n: usize, ok: fn(u8) -> bool| s.len() == n && s.bytes().all(ok);
    if !digits(time, 14, |b| b.is_ascii_digit())
        || !digits(hash, 12, |b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return None;
    }
    let untagged = base.is_empty() && (version.major, version.minor, version.patch) == (0, 0, 0);
    if !untagged && base.last().is_none_or(|b| b != "0") {
        return None;
    }
    // `X.Y.(Z+1)-0.TIME-HASH` follows a release, so its patch is at least 1.
    if base.len() == 1 && version.patch == 0 {
        return None;
    }
    Some(PseudoCommit {
        time: time.to_owned(),
        hash: hash.to_owned(),
    })
}

/// The version of a pseudo-version's base tag (`cli.dep.pseudo.base`):
/// `X.Y.Z` for `X.Y.(Z+1)-0.TIME-HASH`, `X.Y.Z-PRE` for
/// `X.Y.Z-PRE.0.TIME-HASH`, and `None` for `0.0.0-TIME-HASH`, which has
/// none. `version` must be a pseudo-version.
#[must_use]
pub fn pseudo_base(version: &Version) -> Option<Version> {
    if version.pre.len() < 2 {
        return None;
    }
    let base = &version.pre[..version.pre.len() - 2];
    Some(if base.is_empty() {
        Version {
            patch: version.patch.checked_sub(1)?,
            pre: Vec::new(),
            ..version.clone()
        }
    } else {
        Version {
            pre: base.to_vec(),
            ..version.clone()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{pseudo_base, pseudo_commit, tag_name, tag_version};
    use hd_base::SliceSort as _;
    use hd_project::{Version, compatibility_line, host_path};

    fn v(s: &str) -> Version {
        Version::parse(s).expect(s)
    }

    #[test]
    fn tags_carry_the_package_directory() {
        let root = host_path("github.com/acme/json").expect("host");
        let lint = host_path("github.com/acme/tools/lint").expect("host");
        assert_eq!(tag_name(&root, &v("2.3.0")), "v2.3.0");
        assert_eq!(tag_name(&lint, &v("2.3.0")), "lint/v2.3.0");
        assert_eq!(tag_version(&lint, "lint/v2.3.0"), Some(v("2.3.0")));
        assert_eq!(tag_version(&lint, "v2.3.0"), None);
        assert_eq!(tag_version(&lint, "lint2/v2.3.0"), None);
        assert_eq!(tag_version(&root, "lint/v2.3.0"), None);
        assert_eq!(tag_version(&root, "v1.0.0-rc.1"), Some(v("1.0.0-rc.1")));
        assert_eq!(tag_version(&root, "release-1"), None);
    }

    #[test]
    fn host_paths_split_into_repository_and_directory() {
        let h = host_path("git.example.com/shop/billing.git/core").expect("host");
        assert_eq!(h.repository, "git.example.com/shop/billing.git");
        assert_eq!(h.subdirectory, "core");
        let g = host_path("github.com/acme/tools/lint").expect("host");
        assert_eq!(g.repository, "github.com/acme/tools");
        assert_eq!(g.subdirectory, "lint");
        for bad in [
            "github.com/acme",
            "example.com/shop/billing",
            "github.com/acme/json/v2",
            "github.com/acme/../json",
            "localhost/a/b.git",
        ] {
            assert!(host_path(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn versions_order_and_fall_into_lines() {
        let mut list = [
            v("1.10.0"),
            v("1.2.0"),
            v("1.2.0-rc.1"),
            v("1.2.0-0.20260101000000-aaaaaaaaaaaa"),
            v("0.0.0-20260101000000-aaaaaaaaaaaa"),
            v("1.2.0-rc.1.0.20260101000000-aaaaaaaaaaaa"),
        ];
        list.sort_stable();
        let shown: Vec<String> = list.iter().map(ToString::to_string).collect();
        assert_eq!(
            shown,
            [
                "0.0.0-20260101000000-aaaaaaaaaaaa",
                "1.2.0-0.20260101000000-aaaaaaaaaaaa",
                "1.2.0-rc.1",
                "1.2.0-rc.1.0.20260101000000-aaaaaaaaaaaa",
                "1.2.0",
                "1.10.0",
            ]
        );
        assert_eq!(compatibility_line(&v("2.3.1")), "2");
        assert_eq!(compatibility_line(&v("0.4.2")), "0.4");
        assert_eq!(
            compatibility_line(&v("0.0.0-20260101000000-aaaaaaaaaaaa")),
            "0.0"
        );
    }

    #[test]
    fn pseudo_versions_take_go_forms() {
        let commit = |s: &str| pseudo_commit(&v(s)).map(|c| (c.time, c.hash));
        let named = Some(("20260912081500".to_owned(), "3f2c9e1a7b6d".to_owned()));
        assert_eq!(commit("0.0.0-20260912081500-3f2c9e1a7b6d"), named);
        assert_eq!(commit("0.4.1-0.20260912081500-3f2c9e1a7b6d"), named);
        assert_eq!(commit("1.0.0-rc.1.0.20260912081500-3f2c9e1a7b6d"), named);
        for not in [
            "1.0.0",
            "1.0.0-rc.1",
            "1.2.3-20260912081500-3f2c9e1a7b6d",
            "0.4.0-0.20260912081500-3f2c9e1a7b6d",
            "0.4.1-1.20260912081500-3f2c9e1a7b6d",
            "0.4.1-0.2026091208150-3f2c9e1a7b6d",
            "0.4.1-0.20260912081500-3F2C9E1A7B6D",
        ] {
            assert_eq!(commit(not), None, "{not}");
        }
        let base = |s: &str| pseudo_base(&v(s)).map(|b| b.to_string());
        assert_eq!(base("0.0.0-20260912081500-3f2c9e1a7b6d"), None);
        assert_eq!(
            base("0.4.1-0.20260912081500-3f2c9e1a7b6d").as_deref(),
            Some("0.4.0")
        );
        assert_eq!(
            base("1.0.0-rc.1.0.20260912081500-3f2c9e1a7b6d").as_deref(),
            Some("1.0.0-rc.1")
        );
    }
}
