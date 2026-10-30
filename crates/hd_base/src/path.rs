//! Host paths as the user, Node and Wasmtime spell them: one
//! canonicalization for every path that is compared, keyed or shown.

use std::path::{Path, PathBuf};

/// A path without Windows' verbatim prefix: `\\?\D:\x` is `D:\x` and
/// `\\?\UNC\host\share` is `\\host\share`. Other text is unchanged.
fn strip_verbatim(text: &str) -> Option<String> {
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return Some(format!(r"\\{rest}"));
    }
    let rest = text.strip_prefix(r"\\?\")?;
    let b = rest.as_bytes();
    (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':').then(|| rest.to_owned())
}

/// `path` without the verbatim prefix (see [`strip_verbatim`]). Node,
/// Wasmtime and the user all write a path without it, so a path that
/// `std::fs::canonicalize` made must lose it before it is compared, shown
/// or put in a URL.
#[must_use]
pub fn plain_path(path: PathBuf) -> PathBuf {
    match path.to_str().and_then(strip_verbatim) {
        Some(plain) => PathBuf::from(plain),
        None => path,
    }
}

/// `std::fs::canonicalize` with no verbatim prefix on Windows: the path
/// the user, Node and a grant entry all spell. Every canonicalization in
/// the workspace goes through this function, so two paths for one place
/// compare equal.
///
/// # Errors
/// The error of `std::fs::canonicalize`.
pub fn canonicalize(path: impl AsRef<Path>) -> std::io::Result<PathBuf> {
    std::fs::canonicalize(path).map(plain_path)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{canonicalize, plain_path};

    #[test]
    fn a_verbatim_prefix_is_stripped() {
        for (from, to) in [
            (r"\\?\D:\a\hd\host\node.ts", r"D:\a\hd\host\node.ts"),
            (r"\\?\c:\", r"c:\"),
            (r"\\?\UNC\srv\share\x", r"\\srv\share\x"),
            (r"\\?\Volume{1}\x", r"\\?\Volume{1}\x"),
            (r"D:\a\b", r"D:\a\b"),
            ("/tmp/a", "/tmp/a"),
            ("", ""),
        ] {
            assert_eq!(plain_path(from.into()), Path::new(to), "{from}");
        }
    }

    #[test]
    fn canonicalize_matches_the_std_result_without_a_prefix() {
        let dir = std::env::temp_dir();
        let plain = canonicalize(&dir).expect("temp dir");
        assert_eq!(
            plain,
            plain_path(std::fs::canonicalize(&dir).expect("temp dir"))
        );
    }
}
