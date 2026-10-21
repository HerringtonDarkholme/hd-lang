//! The lexer's and the parser's stage fixtures, `tests/stages/*.stage`
//! (`hd_testkit::stages`): the `tokens` and `tree` views of each case's
//! subject. `HD_BLESS=1 cargo test -p hd_syntax --test all stages::`
//! rewrites their expected blocks.

#[test]
fn stage_fixtures_match() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/stages");
    hd_testkit::stages::run_dir(&dir, &hd_testkit::stages::view);
}
