#![forbid(unsafe_code)]
//! `hd_fmt`: the formatter on the green tree (design-overview.md §2.1,
//! commands.md §7.6). The layout rules are not written yet: `format`
//! refuses a file that does not parse and reports the rest as not
//! implemented. `round_trip` is the lossless base every rule builds on.

use hd_base::{NotImplemented, Stage, StageResult};

/// The green tree reproduces its source byte for byte (syntax slice 1).
#[must_use]
pub fn round_trip(source: &str) -> String {
    let p = hd_syntax::parse(source.as_bytes());
    p.tree.reconstruct(&p.tokens, source)
}

/// Formats one file. Not implemented beyond the parse gate.
pub fn format(source: &str) -> StageResult<String> {
    let p = hd_syntax::parse(source.as_bytes());
    if !p.is_ok() {
        return Err(NotImplemented::new(Stage::Parse, "formatting a file with syntax errors is refused"));
    }
    Err(NotImplemented::new(Stage::Parse, "formatter layout rules"))
}

#[cfg(test)]
mod tests {
    use super::{format, round_trip};

    #[test]
    fn round_trip_is_lossless_and_format_reports() {
        let src = "fn main():\n    println(1)\n";
        assert_eq!(round_trip(src), src);
        assert!(format(src).is_err());
    }
}
