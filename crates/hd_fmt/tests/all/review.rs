//! A whole-file case. Every other formatting case is an input and its
//! expected text in `spec/conformance/cli/fmt-width`.
use hd_fmt::format;

#[test]
fn comment_only_files_keep_every_comment() {
    let source = "# first  \n# second\n";
    let once = format(source).unwrap_or_else(|diagnostics| panic!("input failed: {diagnostics:?}"));
    assert_eq!(once, source);
    assert_eq!(format(&once).expect("output parses"), once);
}
