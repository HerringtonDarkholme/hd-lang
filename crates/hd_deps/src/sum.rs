//! `hd.sum` (`cli.sum.*`): a tree line `HOST_PATH@VERSION h1:HASH` per
//! selected version, and a manifest line `HOST_PATH@VERSION/hd.toml
//! h1:HASH` per version whose manifest selection reads; and the tree hash
//! and manifest hash those lines hold.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use hd_project::{Version, host_path};
use sha2::{Digest, Sha256};

/// The suffix of a manifest line's key (`cli.sum.manifest-line`).
const MANIFEST_SUFFIX: &str = "/hd.toml";

/// The key of a version's tree line, as `github.com/acme/json@2.1.0`.
#[must_use]
pub fn tree_key(host: &str, version: &Version) -> String {
    format!("{host}@{version}")
}

/// The key of a version's manifest line, as
/// `github.com/acme/json@2.1.0/hd.toml`.
#[must_use]
pub fn manifest_key(host: &str, version: &Version) -> String {
    format!("{host}@{version}{MANIFEST_SUFFIX}")
}

/// A key's host path, version, and whether it is a manifest line's; `None`
/// when it is no key.
fn split_key(key: &str) -> Option<(&str, Version, bool)> {
    let (bare, manifest) = match key.strip_suffix(MANIFEST_SUFFIX) {
        Some(bare) => (bare, true),
        None => (key, false),
    };
    let (host, version) = bare.rsplit_once('@')?;
    host_path(host).ok()?;
    Some((host, Version::parse(version)?, manifest))
}

/// The order of `hd.sum`'s keys (`cli.sum.order`): by host path, then by
/// version order, a version's tree line before its manifest line.
fn key_order(a: &str, b: &str) -> Ordering {
    match (split_key(a), split_key(b)) {
        (Some((ha, va, ma)), Some((hb, vb, mb))) => ha
            .cmp(hb)
            .then_with(|| va.cmp(&vb))
            .then_with(|| ma.cmp(&mb)),
        _ => a.cmp(b),
    }
}

/// Why `hd.sum` cannot be read: the 1-based line and the message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SumError {
    pub line: u32,
    pub message: String,
}

/// The entries of `hd.sum`, by key (`cli.sum.entry`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sum {
    pub entries: BTreeMap<String, String>,
}

impl Sum {
    /// Reads `hd.sum`. A line that is no tree line or manifest line, a line
    /// out of `cli.sum.order`, a repeated key, or a last line without a
    /// newline is an error, since no command writes one. A line may end
    /// with CRLF as well as LF, as source and `hd.toml` lines may, so a
    /// Windows checkout that converts line endings still reads.
    pub fn parse(text: &str) -> Result<Sum, SumError> {
        const REPAIR: &str =
            "restore hd.sum from version control, or delete the line and run `hd fetch`";
        let mut entries = BTreeMap::new();
        let mut previous: Option<&str> = None;
        let mut rest = text;
        let mut number = 0u32;
        while !rest.is_empty() {
            number += 1;
            let err = |message: String| {
                Err(SumError {
                    line: number,
                    message,
                })
            };
            let Some((line, after)) = rest.split_once('\n') else {
                return err(format!(
                    "hd.sum line {number} does not end with a newline; {REPAIR}"
                ));
            };
            rest = after;
            let line = line.strip_suffix('\r').unwrap_or(line);
            let parsed = line
                .split_once(' ')
                .filter(|(key, hash)| split_key(key).is_some() && is_hash(hash));
            let Some((key, hash)) = parsed else {
                return err(format!(
                    "hd.sum line {number} is not `HOST_PATH@VERSION h1:HASH` or `HOST_PATH@VERSION/hd.toml h1:HASH`; {REPAIR}"
                ));
            };
            if entries.contains_key(key) {
                return err(format!(
                    "hd.sum line {number} repeats the entry {key}; {REPAIR}"
                ));
            }
            if let Some(p) = previous
                && key_order(p, key) != Ordering::Less
            {
                return err(format!(
                    "hd.sum line {number} is out of order: {key} sorts before {p}, by host path, then version; {REPAIR}"
                ));
            }
            entries.insert(key.to_owned(), hash.to_owned());
            previous = Some(key);
        }
        Ok(Sum { entries })
    }

    /// The text of `hd.sum`, in `cli.sum.order`, each line ending with a
    /// newline.
    #[must_use]
    pub fn text(&self) -> String {
        let mut keys: Vec<&String> = self.entries.keys().collect();
        keys.sort_by(|a, b| key_order(a, b));
        let mut out = String::new();
        for k in keys {
            let _ = writeln!(out, "{k} {}", self.entries[k]);
        }
        out
    }
}

/// Whether `text` has the form of a tree hash: `h1:` and the padded
/// Base64 of 32 bytes.
fn is_hash(text: &str) -> bool {
    text.strip_prefix("h1:").is_some_and(|b| {
        b.len() == 44
            && b.ends_with('=')
            && b[..43]
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'+' || c == b'/')
    })
}

/// Padded standard Base64.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The tree hash of files given by path and content (`cli.sum.summary`,
/// `cli.sum.hash`): `h1:` and the Base64 SHA-256 of the summary, one line
/// per file in path order with its SHA-256 in lowercase hexadecimal, two
/// spaces, and its path.
#[must_use]
pub fn summary_hash(files: &[(String, Vec<u8>)]) -> String {
    let mut sorted: Vec<&(String, Vec<u8>)> = files.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut summary = Sha256::new();
    for (path, content) in sorted {
        let mut line = String::with_capacity(path.len() + 67);
        for b in Sha256::digest(content) {
            let _ = write!(line, "{b:02x}");
        }
        let _ = writeln!(line, "  {path}");
        summary.update(line.as_bytes());
    }
    format!("h1:{}", base64(&summary.finalize()))
}

/// The files of a version's tree in `dir` (`cli.sum.tree`): every regular
/// file under it, by its path with `/`, sorted, except the files of a
/// subdirectory that holds its own `hd.toml`, which is another package.
/// Symbolic links are no part of it.
pub fn tree_files(dir: &Path) -> std::io::Result<Vec<String>> {
    fn walk(dir: &Path, rel: &str, out: &mut Vec<String>) -> std::io::Result<()> {
        if !rel.is_empty() && dir.join("hd.toml").symlink_metadata().is_ok() {
            return Ok(());
        }
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = if rel.is_empty() {
                name
            } else {
                format!("{rel}/{name}")
            };
            let kind = entry.file_type()?;
            if kind.is_dir() {
                walk(&entry.path(), &path, out)?;
            } else if kind.is_file() {
                out.push(path);
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(dir, "", &mut out)?;
    out.sort();
    Ok(out)
}

/// The tree hash of the package in `dir` (`cli.sum.hash`).
pub fn tree_hash(dir: &Path) -> std::io::Result<String> {
    let mut files = Vec::new();
    for path in tree_files(dir)? {
        let content = std::fs::read(dir.join(&path))?;
        files.push((path, content));
    }
    Ok(summary_hash(&files))
}

/// The manifest hash of the package in `dir`: the tree hash of a tree that
/// holds only its `hd.toml` (`cli.sum.manifest-hash`).
pub fn manifest_hash(dir: &Path) -> std::io::Result<String> {
    Ok(summary_hash(&[(
        "hd.toml".to_owned(),
        std::fs::read(dir.join("hd.toml"))?,
    )]))
}

#[cfg(test)]
mod tests {
    use super::{Sum, base64, summary_hash};

    #[test]
    fn base64_pads() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    /// The hashes the TypeScript prototype computes for the same tree
    /// (`src/dependencies/sum.ts`).
    #[test]
    fn tree_hash_matches_the_prototype() {
        let files = vec![
            (
                "src/lib.hd".to_owned(),
                b"pub fn shout(word: string) -> string:\n    word + \"!\"\n".to_vec(),
            ),
            (
                "hd.toml".to_owned(),
                b"[package]\nname = \"text\"\n".to_vec(),
            ),
        ];
        assert_eq!(summary_hash(&files), TREE);
        assert_eq!(summary_hash(&files[1..]), MANIFEST);
    }

    /// `node f.ts DIR` printing `treeHash(DIR)` and `manifestHash(DIR)` of
    /// `src/dependencies/sum.ts` for a directory holding these two files.
    const TREE: &str = "h1:TyJx+TVqURAOufHLOr28+vlhwtSCOk3GaYbc/BTNvP4=";
    const MANIFEST: &str = "h1:AYWneQ5gp+9vtuntpg3Riha67aXxgxxLKUd2ak+pNNk=";

    #[test]
    fn sum_reads_in_order_and_writes_sorted() {
        let h = |c: char| format!("h1:{}=", c.to_string().repeat(43));
        let text = format!(
            "example.com/a.git@1.0.0/hd.toml {}\ngithub.com/acme/json@2.0.0/hd.toml {}\ngithub.com/acme/json@2.1.0 {}\ngithub.com/acme/json@2.1.0/hd.toml {}\ngithub.com/acme/json@10.0.0 {}\n",
            h('a'),
            h('b'),
            h('c'),
            h('d'),
            h('e'),
        );
        let sum = Sum::parse(&text).expect("sum");
        assert_eq!(sum.entries.len(), 5);
        assert_eq!(sum.text(), text);
        let swapped = text.replacen("2.0.0", "3.0.0", 1);
        assert!(
            Sum::parse(&swapped)
                .unwrap_err()
                .message
                .contains("out of order")
        );
        assert_eq!(Sum::parse(text.trim_end()).unwrap_err().line, 5);
        assert_eq!(
            Sum::parse("github.com/acme/json@2.1.0 h1:x\n")
                .unwrap_err()
                .line,
            1
        );
        assert_eq!(Sum::parse("").expect("empty").entries.len(), 0);
    }

    /// A checkout that converts line endings to CRLF, as Git on Windows
    /// does, gives the same entries; `hd` writes LF.
    #[test]
    fn sum_reads_crlf_lines() {
        let h = |c: char| format!("h1:{}=", c.to_string().repeat(43));
        let text = format!(
            "github.com/acme/json@2.1.0 {}\ngithub.com/acme/json@2.1.0/hd.toml {}\n",
            h('a'),
            h('b'),
        );
        let crlf = text.replace('\n', "\r\n");
        let sum = Sum::parse(&crlf).expect("crlf sum");
        assert_eq!(sum, Sum::parse(&text).expect("lf sum"));
        assert_eq!(sum.text(), text);
        assert_eq!(Sum::parse(crlf.trim_end_matches('\n')).unwrap_err().line, 2);
    }
}
