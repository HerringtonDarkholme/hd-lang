//! Rule 19 must know runtime call shapes before helper bodies are reached.

#[test]
fn tuple_reference_fixture_has_no_fallthrough_branches() -> Result<(), String> {
    let case = hd_testkit::stages::parse(include_str!(
        "cases/counts/function-variance-borrows-a-tuple-input.hd"
    ))?;
    let wat = hd_testkit::stages::view("wat", case.build, &case.program)
        .ok_or_else(|| "WAT view is available".to_owned())?;
    let (_, definition) = wat
        .split_once("  (func $main_like ")
        .ok_or_else(|| "main_like is emitted".to_owned())?;
    let body = definition
        .split_once("\n  (func ")
        .map_or(definition, |(body, _)| body);
    assert!(
        !body.lines().any(|line| line.trim() == "br 0"),
        "fallthrough branches remain: {body}"
    );
    Ok(())
}
