//! Exact layout and count views of the marked compiler fixture subject.
#[test]
fn stage_fixtures_match() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/stages");
    hd_testkit::stages::run_dir(&dir, &hd_testkit::stages::view);
}
