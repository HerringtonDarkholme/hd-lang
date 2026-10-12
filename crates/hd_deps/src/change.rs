//! The version changes `hd add` and `hd update` make to a requirement
//! (`cli.dep.add.lower`, `cli.dep.update`, `cli.dep.update.release`).

use hd_project::{Requirement, Version, compatibility_line};

/// The newest release among `versions` that moves a requirement at
/// `current`: the greatest version without a pre-release suffix on
/// `current`'s compatibility line (`cli.dep.update.release`). `None` when
/// that is not above `current`, so a requirement already at or above it
/// keeps its version, a pre-release of a later release moves up to that
/// release, and no other line is ever entered.
#[must_use]
pub fn newest_release(versions: &[Version], current: &Version) -> Option<Version> {
    let line = compatibility_line(current);
    versions
        .iter()
        .filter(|v| v.pre.is_empty() && compatibility_line(v) == line)
        .max()
        .filter(|newest| *newest > current)
        .cloned()
}

/// The version that setting a requirement to `version` of the host path
/// `path` lowers (`cli.dep.add.lower`): the one `old` requires, when it is
/// a later version of the same host path.
#[must_use]
pub fn lowered(old: &Requirement, path: &str, version: &Version) -> Option<Version> {
    let (old_path, old_version) = old.host()?;
    (old_path == path && *version < old_version).then_some(old_version)
}

#[cfg(test)]
mod tests {
    use hd_project::{Requirement, Version};

    use super::{lowered, newest_release};

    fn v(text: &str) -> Version {
        Version::parse(text).expect("version")
    }

    fn all(texts: &[&str]) -> Vec<Version> {
        texts.iter().map(|t| v(t)).collect()
    }

    #[test]
    fn update_takes_the_newest_release_on_the_line() {
        let tags = all(&["1.0.0", "1.2.0", "1.10.0", "2.0.0", "2.1.0", "0.4.0"]);
        assert_eq!(newest_release(&tags, &v("1.2.0")), Some(v("1.10.0")));
        // Version order, not text order: 1.10.0 follows 1.2.0.
        assert_eq!(newest_release(&tags, &v("1.0.0")), Some(v("1.10.0")));
        // Another major version is another line, and stays unused.
        assert_eq!(newest_release(&tags, &v("2.0.0")), Some(v("2.1.0")));
    }

    #[test]
    fn update_skips_pre_releases() {
        let tags = all(&["2.0.0", "2.1.0", "2.2.0-rc.1", "2.2.0-beta"]);
        assert_eq!(newest_release(&tags, &v("2.0.0")), Some(v("2.1.0")));
        // With no newer release, the pre-release is no target.
        let only = all(&["2.0.0", "2.1.0-rc.1"]);
        assert_eq!(newest_release(&only, &v("2.0.0")), None);
    }

    #[test]
    fn a_requirement_at_or_above_the_newest_keeps_its_version() {
        let tags = all(&["1.0.0", "1.2.0"]);
        assert_eq!(newest_release(&tags, &v("1.2.0")), None);
        assert_eq!(newest_release(&tags, &v("1.5.0")), None);
        assert_eq!(newest_release(&[], &v("1.0.0")), None);
    }

    #[test]
    fn a_zero_major_line_is_its_minor_version() {
        let tags = all(&["0.3.0", "0.3.4", "0.4.0", "0.4.2"]);
        assert_eq!(newest_release(&tags, &v("0.3.0")), Some(v("0.3.4")));
        assert_eq!(newest_release(&tags, &v("0.4.0")), Some(v("0.4.2")));
    }

    #[test]
    fn a_pre_release_requirement_moves_to_its_release() {
        let tags = all(&["1.0.0", "1.1.0-rc.1", "1.1.0", "1.1.1-rc.1"]);
        assert_eq!(newest_release(&tags, &v("1.1.0-rc.1")), Some(v("1.1.0")));
    }

    fn requirement(text: &str) -> Requirement {
        Requirement {
            key: "json".to_owned(),
            path: None,
            text: text.to_owned(),
            table: false,
            version: None,
            line: 1,
        }
    }

    #[test]
    fn add_lowers_only_the_same_host_path() {
        let old = requirement("github.com/acme/json@2.1.0");
        assert_eq!(
            lowered(&old, "github.com/acme/json", &v("2.0.0")),
            Some(v("2.1.0"))
        );
        assert_eq!(lowered(&old, "github.com/acme/json", &v("2.1.0")), None);
        assert_eq!(lowered(&old, "github.com/acme/json", &v("2.2.0")), None);
        assert_eq!(lowered(&old, "github.com/acme/other", &v("1.0.0")), None);
        let path = Requirement {
            table: true,
            path: Some("../json".to_owned()),
            ..requirement("{path = \"../json\"}")
        };
        assert_eq!(lowered(&path, "github.com/acme/json", &v("1.0.0")), None);
    }
}
