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

#[cfg(test)]
mod tests {
    use super::{format, round_trip};

    #[test]
    fn duration_units_keep_their_spelling() {
        let src = "fn durations():\n    [500ms, 30s, 1_500ms, 1h + 30min]  \n";
        assert_eq!(round_trip(src), src);
        let formatted = format(src).expect("duration tokens parse");
        assert_eq!(formatted, src.replace("]  ", "]"));
        assert_eq!(format(&formatted).expect("parses"), formatted);
    }

    #[test]
    fn round_trip_is_lossless() {
        let src = "fn main():\n    println(1)\n";
        assert_eq!(round_trip(src), src);
    }

    #[test]
    fn format_trims_line_ends_and_the_file_end_and_is_a_fixed_point() {
        let src = "fn main() -> void $ Console:  \n    println(\"a  \")  \n\n\n";
        let once = format(src).expect("parses");
        assert_eq!(once, "fn main() -> void $ Console:\n    println(\"a  \")\n");
        assert_eq!(format(&once).expect("parses"), once);
        assert!(format("fn main(:\n").is_err());
    }

    /// `ref.check.formatter`: every reference form formats, keeps its
    /// tokens, and is a fixed point.
    #[test]
    fn reference_forms_round_trip_and_format_as_written() {
        let src = "fn demo(r: &mut Point, xs: List[&Point], f: fn(&Point) -> &mut Point) -> &Point?:  \n    view := &xs[0].x\n    edit := &mut r.y\n    made := &Point { x: 1, y: 2 }\n    *r = *view\n    *r.x += 1\n    both := a && b & &c * *d ^ e\n    .None\n\n\n";
        let clean = src.replace("?:  \n", "?:\n").trim_end().to_owned() + "\n";
        assert_eq!(round_trip(src), src);
        let once = format(src).expect("parses");
        assert_eq!(once, clean);
        assert_eq!(format(&once).expect("parses"), once);
    }

    /// `grammar.fn.param-mut`: `mut` before a parameter name formats as written.
    #[test]
    fn mut_parameters_format_as_written() {
        let src = "impl S:\n    fn put(&mut self, mut n: i32, f: fn(i32) -> i32) -> S:\n        g := fn(mut a: i32): a\n        self\n";
        assert_eq!(round_trip(src), src);
        assert_eq!(format(src).expect("parses"), src);
    }

    #[test]
    fn union_types_space_pipes_and_keep_nesting_parentheses() {
        let src = "type Flat = A|B|C\ntype Nested = (A|B)|C\ntype Applied = Box[A]|B\n";
        let expected =
            "type Flat = A | B | C\ntype Nested = (A | B) | C\ntype Applied = Box[A] | B\n";
        assert_eq!(round_trip(src), src);
        let formatted = format(src).expect("union types parse");
        assert_eq!(formatted, expected);
        assert_eq!(
            format(&formatted).expect("formatted union types parse"),
            formatted
        );
    }
}
