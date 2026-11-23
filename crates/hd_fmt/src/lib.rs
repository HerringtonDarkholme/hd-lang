#![forbid(unsafe_code)]
//! `hd_fmt`: one opinionated source layout over the green tree. The source is
//! parsed once, converted to a compact document, and printed once.

mod document;
mod format;
pub use document::MAX_COLUMNS;

use hd_diag::Diagnostic;

/// The green tree reproduces its source byte for byte (syntax slice 1).
#[must_use]
pub fn round_trip(source: &str) -> String {
    let p = hd_syntax::parse(source.as_bytes());
    p.tree.reconstruct(&p.tokens, source)
}

/// Formats one file, or returns its syntax diagnostics: a file that does
/// not parse is left untouched (`cli.fmt.syntax-error`).
pub fn format(source: &str) -> Result<String, Vec<Diagnostic>> {
    let p = hd_syntax::parse(source.as_bytes());
    if !p.is_ok() {
        return Err(p.diagnostics);
    }
    Ok(format::format(source, &p).0)
}
