#![forbid(unsafe_code)]
//! `hd_fmt`: the formatter on the green tree (`docs/compiler/design-overview.md` §2.1,
//! `crates/hd_cli/DESIGN.md` §7.6). The Wadler-style layout rules are not written yet;
//! today's layout is the part every layout shares: no trailing whitespace
//! at a line's end and exactly one newline at the file's end, outside
//! every token and comment, so meaning and comments stay as they are
//! (`cli.fmt.meaning`) and a formatted file is a fixed point
//! (`cli.fmt.idempotent`). `round_trip` is the lossless base every rule
//! builds on.

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
    let t = &p.tokens;
    // Byte ranges whose text stays as written: tokens (a string may hold
    // a line break) and comments (a doc comment's trailing spaces may be
    // Markdown).
    let mut kept: Vec<(usize, usize)> = t
        .start
        .iter()
        .zip(&t.end)
        .chain(t.com_start.iter().zip(&t.com_end))
        .map(|(&s, &e)| (s as usize, e as usize))
        .filter(|(s, e)| e > s)
        .collect();
    kept.sort_unstable();
    let inside = |at: usize| {
        let i = kept.partition_point(|&(s, _)| s <= at);
        i > 0 && at < kept[i - 1].1
    };
    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut line_start = 0;
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'\n' {
            continue;
        }
        let mut end = i;
        while end > line_start && matches!(bytes[end - 1], b' ' | b'\t' | b'\r') && !inside(end - 1)
        {
            end -= 1;
        }
        out.push_str(&source[line_start..end]);
        out.push('\n');
        line_start = i + 1;
    }
    out.push_str(&source[line_start..]);
    // Exactly one newline at the end: no token or comment ends in blank
    // lines, so trailing ones are layout.
    let body_end = out.trim_end_matches(['\n', ' ', '\r']).len();
    if body_end > 0 {
        out.truncate(body_end);
        out.push('\n');
    }
    Ok(out)
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
}
