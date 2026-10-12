//! The targets of `hd add` (Targets, `cli.dep.target.*`) and the key a
//! target's package gives (`cli.dep.key.inferred`). Parsing is pure: it
//! reads no file and runs no git, so `hd add` checks every target before it
//! fetches anything (`cli.dep.add.check`).

use hd_project::{HostPath, host_path, key_problem};

/// What a target names: a local directory or a fetched package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TargetForm {
    /// A path requirement's directory, as written, relative to the working
    /// directory (`cli.dep.target.path`).
    Path(String),
    /// A host path, from a repository URL, a host path, or `OWNER/REPO`.
    Host(HostPath),
}

/// One target word of `hd add`: its form and the text after its `@`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub form: TargetForm,
    pub query: Option<String>,
}

/// The target word split at its first `@` that is not the `git@` of a
/// repository URL (`cli.dep.target.split`).
#[must_use]
pub fn split_target(word: &str) -> (&str, Option<&str>) {
    let skip = if word.starts_with("ssh://git@") {
        "ssh://git@".len()
    } else if word.starts_with("git@") {
        "git@".len()
    } else {
        0
    };
    match word[skip..].find('@') {
        Some(i) => (&word[..skip + i], Some(&word[skip + i + 1..])),
        None => (word, None),
    }
}

/// Whether `text` is a path target: `.`, `..`, a path that starts with
/// `./` or `../`, or an absolute path.
fn is_path(text: &str) -> bool {
    text == "."
        || text == ".."
        || text.starts_with("./")
        || text.starts_with("../")
        || text.starts_with('/')
        || (cfg!(windows) && (text.starts_with(".\\") || text.starts_with("..\\")))
        || std::path::Path::new(text).is_absolute()
}

/// The host path a repository URL names (`cli.dep.target.url`,
/// `cli.dep.target.url.git-suffix`), `None` when `text` is no URL of the
/// table, or why it names no repository.
fn url_host_path(text: &str) -> Option<Result<String, String>> {
    let rest = if let Some(r) = text.strip_prefix("https://") {
        r.to_owned()
    } else if let Some(r) = text.strip_prefix("ssh://git@") {
        r.to_owned()
    } else {
        let r = text.strip_prefix("git@")?;
        let Some((host, path)) = r.split_once(':') else {
            return Some(Err(format!(
                "`{text}` is no repository URL: write `git@HOST:PATH`, as `git@github.com:acme/json.git`"
            )));
        };
        format!("{host}/{path}")
    };
    let rest = rest.trim_end_matches('/');
    let Some((host, _)) = rest.split_once('/') else {
        return Some(Err(format!("`{text}` names no repository")));
    };
    if host == "github.com" {
        let path = rest.strip_suffix(".git").unwrap_or(rest);
        if path.split('/').count() != 3 {
            return Some(Err(format!(
                "`{text}` is no repository URL: on github.com it is `https://github.com/OWNER/REPO`; for a package in a directory of a repository, write `OWNER/REPO/DIR`"
            )));
        }
        Some(Ok(path.to_owned()))
    } else if rest.rsplit_once('.').is_some_and(|(_, ext)| ext == "git") {
        Some(Ok(rest.to_owned()))
    } else {
        Some(Ok(format!("{rest}.git")))
    }
}

/// A target word, or why it names no package (`cli.dep.target.order`):
/// the first of a path, a repository URL, a host path whose first segment
/// holds a dot, and `OWNER/REPO[/DIR...]` on `github.com`
/// (`cli.dep.target.github`, `cli.dep.target.github.only`) that it
/// matches. A path target takes no query
/// (`cli.dep.target.path.no-query`).
pub fn parse_target(word: &str) -> Result<Target, String> {
    let (text, query) = split_target(word);
    if text.is_empty() {
        return Err(format!(
            "`{word}` names no package: write OWNER/REPO, a host path, a repository URL, or a path that starts with `./`"
        ));
    }
    if is_path(text) {
        if let Some(q) = query {
            return Err(format!(
                "the path target `{text}` takes no version, but `{word}` names `{q}`; a path requirement uses the package in that directory as it is"
            ));
        }
        return Ok(Target {
            form: TargetForm::Path(text.to_owned()),
            query: None,
        });
    }
    let path = if let Some(found) = url_host_path(text) {
        found?
    } else if text.split('/').next().is_some_and(|s| s.contains('.')) {
        text.to_owned()
    } else {
        if !text.contains('/') {
            return Err(format!(
                "`{text}` names no repository: write OWNER/REPO for a package on github.com, as `acme/{text}`, or a path that starts with `./` for a local one"
            ));
        }
        format!("github.com/{text}")
    };
    let host = host_path(&path)?;
    if let Some(q) = query {
        crate::query::parse_query(Some(q))?;
    }
    Ok(Target {
        form: TargetForm::Host(host),
        query: query.map(str::to_owned),
    })
}

/// The key of a package named `name` in its `hd.toml`
/// (`cli.dep.key.inferred`): the name with each `-` as `_`, or why source
/// could not write it as `dep.NAME` (`cli.dep.key.inferred.invalid`).
pub fn inferred_key(name: &str, shown: &str) -> Result<String, String> {
    let key = name.replace('-', "_");
    if key.is_empty() {
        return Err(format!(
            "{shown} declares no package name in its hd.toml; choose a key with `--name`"
        ));
    }
    match key_problem(&key) {
        None => Ok(key),
        Some(_) => Err(format!(
            "the package name `{name}` of {shown} gives the key `{key}`, which source cannot write as `dep.{key}`; choose a key with `--name`, as `hd add --name NAME ...`"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{TargetForm, inferred_key, parse_target, split_target};

    fn host(word: &str) -> (String, Option<String>) {
        let t = parse_target(word).unwrap_or_else(|e| panic!("{word}: {e}"));
        match t.form {
            TargetForm::Host(h) => (h.path, t.query),
            TargetForm::Path(p) => panic!("{word} is the path {p}"),
        }
    }

    fn at(path: &str, query: Option<&str>) -> (String, Option<String>) {
        (path.to_owned(), query.map(str::to_owned))
    }

    #[test]
    fn a_word_splits_at_its_first_at_past_git_at() {
        assert_eq!(
            split_target("acme/json@2.1.0"),
            ("acme/json", Some("2.1.0"))
        );
        assert_eq!(split_target("acme/json"), ("acme/json", None));
        assert_eq!(
            split_target("git@github.com:acme/json.git@2.1.0"),
            ("git@github.com:acme/json.git", Some("2.1.0"))
        );
        assert_eq!(
            split_target("ssh://git@github.com/acme/json@main"),
            ("ssh://git@github.com/acme/json", Some("main"))
        );
        assert_eq!(split_target("./a@b@c"), ("./a", Some("b@c")));
    }

    #[test]
    fn owner_repo_is_on_github() {
        assert_eq!(host("acme/json"), at("github.com/acme/json", None));
        assert_eq!(
            host("acme/tools/lint@2.3"),
            at("github.com/acme/tools/lint", Some("2.3"))
        );
        assert!(parse_target("json").is_err());
        assert!(parse_target("acme").is_err());
    }

    #[test]
    fn a_host_path_is_taken_as_written() {
        assert_eq!(
            host("git.example.com/shop/billing.git@0.4.2"),
            at("git.example.com/shop/billing.git", Some("0.4.2"))
        );
        assert_eq!(
            host("github.com/acme/json"),
            at("github.com/acme/json", None)
        );
        // Another host's repository part needs its `.git`.
        assert!(parse_target("git.example.com/shop/billing@0.4.2").is_err());
        assert!(parse_target("github.com/acme@2.1.0").is_err());
    }

    #[test]
    fn urls_normalize_to_host_paths() {
        for url in [
            "https://github.com/acme/json",
            "https://github.com/acme/json.git",
            "https://github.com/acme/json/",
            "ssh://git@github.com/acme/json",
            "ssh://git@github.com/acme/json.git",
            "git@github.com:acme/json.git",
            "git@github.com:acme/json",
        ] {
            assert_eq!(host(url), at("github.com/acme/json", None), "{url}");
        }
        assert_eq!(
            host("git@github.com:acme/json.git@2.1.0"),
            at("github.com/acme/json", Some("2.1.0"))
        );
        for url in [
            "https://git.example.com/shop/billing",
            "https://git.example.com/shop/billing.git",
            "ssh://git@git.example.com/shop/billing",
            "git@git.example.com:shop/billing",
        ] {
            assert_eq!(
                host(url),
                at("git.example.com/shop/billing.git", None),
                "{url}"
            );
        }
        // A URL names a repository, never a directory in one.
        assert!(parse_target("https://github.com/acme/tools/lint").is_err());
        assert!(parse_target("git@github.com").is_err());
        assert!(parse_target("https://github.com").is_err());
    }

    #[test]
    fn paths_take_no_query() {
        for word in [".", "..", "./money", "../billing", "/srv/pkg"] {
            assert_eq!(
                parse_target(word).map(|t| t.form),
                Ok(TargetForm::Path(word.to_owned())),
                "{word}"
            );
        }
        assert!(parse_target("./money@1.0.0").is_err());
        assert!(parse_target("@1.0.0").is_err());
    }

    #[test]
    fn queries_are_checked_with_the_target() {
        assert!(parse_target("acme/json@").is_err());
        assert!(parse_target("acme/json@-x").is_err());
        assert!(parse_target("acme/json@a..b").is_err());
        assert!(parse_target("acme/json@feature/x").is_ok());
    }

    #[test]
    fn keys_come_from_package_names() {
        assert_eq!(inferred_key("left-pad", "x").as_deref(), Ok("left_pad"));
        assert_eq!(inferred_key("json", "x").as_deref(), Ok("json"));
        assert!(inferred_key("9tools", "x").is_err_and(|e| e.contains("--name")));
        assert!(inferred_key("", "x").is_err());
    }
}
