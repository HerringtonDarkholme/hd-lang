//! Plain hd stage cases and separate generated baselines.
//! `HD_BLESS=1` updates only `tests/baselines/`.

#[test]
fn stage_fixtures_match() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cases");
    hd_testkit::stages::run_dir(&dir, &hd_testkit::stages::view);
}
