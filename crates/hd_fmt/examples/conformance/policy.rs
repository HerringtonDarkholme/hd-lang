//! Shared by the repository batch formatter and its checker-verdict gate.
use hd_base::HashSet;
use std::path::Path;

pub fn protected(specification: &str, path: &str, exemptions: &HashSet<String>) -> bool {
    specification.starts_with("lang/01-")
        || specification.starts_with("lang/02-")
        || exemptions.contains(path)
}

pub fn exemptions(suite: &Path) -> std::io::Result<HashSet<String>> {
    let text = std::fs::read_to_string(suite.join("format-exempt.tsv"))?;
    text.lines()
        .skip(1)
        .map(|line| {
            let Some((path, reason)) = line.split_once('\t') else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "invalid format exemption",
                ));
            };
            if path.is_empty() || reason.is_empty() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "format exemption needs a path and reason",
                ));
            }
            Ok(path.to_owned())
        })
        .collect()
}
