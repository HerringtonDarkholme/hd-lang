//! Exercise stable removal and count actual allocations and compaction work.
use hd_driver::{Executor, Goal, Host, NoClock, Packages, build_packages};
use hd_sched::SerialOrder;

#[test]
fn ordered_object_removal_has_linear_work_for_unique_and_duplicate_keys() -> Result<(), String> {
    let json = include_str!("../../../../lib/std/json.hd");
    let scan = "        for entry in self.entries:";
    let start = json
        .find("    fn compact(&mut self)")
        .ok_or("compaction method")?;
    let (prefix, suffix) = json.split_at(start);
    let compact_end = suffix
        .find("    pub fn keys(")
        .ok_or("keys after compaction")?;
    assert_eq!(prefix.matches("impl JsonObject:").count(), 1);
    assert_eq!(
        suffix[..compact_end].matches(scan).count(),
        1,
        "one compaction scan"
    );
    let instrumented_json = "use std.rt.memory.size_of\n".to_owned()
        + &prefix.replace(
            "impl JsonObject:",
            "let object_compaction_steps: usize = 0\n\nimpl JsonObject:",
        )
        + &suffix.replacen(
            scan,
            &format!("{scan}\n            object_compaction_steps += 1"),
            1,
        )
        + r#"

pub fn duplicate_object_for_test(count: usize) -> JsonObject:
    let mut entries: List[ObjectEntry] = []
    let index: usize = 0
    while index < count:
        entries.push(.Member("same", Json.Number(Number::from_u64(u64(index)))))
        index += 1
    JsonObject::from_entries(entries)

pub fn slot_queries_for_test() -> bool:
    let mut empty = JsonObject::new()
    metadata := empty.take_metadata()
    if metadata._0 != 0 || !metadata._1.is_empty() || !empty.is_empty():
        return false
    empty.set("", Json.Null)
    if empty.remove("") != .Some(Json.Null):
        return false
    let mut object = JsonObject::new()
    let index: usize = 0
    while index < 9:
        object.set("s$index", Json.Bool(true))
        index += 1
    _ := object.remove("s0")
    (
        object.value_at(0).is_none()
        && object.value_at(object.entries.len() - 1).is_none()
        && object.get("s1") == .Some(Json.Bool(true))
    )

pub fn object_storage_sizes_for_test() -> (usize, usize, usize):
    (size_of::[JsonObject](), size_of::[ObjectEntry](), size_of::[Json]())

pub fn compaction_steps_for_test() -> usize: object_compaction_steps
"#;
    let alloc = include_str!("../../../../lib/std/rt/alloc.hd");
    for marker in [
        "let table: usize = 0",
        "pub fn alloc(size: usize) -> usize:\n",
        "pub fn alloc_class(size: usize, class: usize) -> usize:\n",
    ] {
        assert_eq!(
            alloc.matches(marker).count(),
            1,
            "allocation counter hook: {marker}"
        );
    }
    let instrumented_alloc = alloc
        .replacen("let table: usize = 0", "let requested_bytes: u64 = 0\nlet table: usize = 0", 1)
        .replace("pub fn alloc(size: usize) -> usize:\n", "pub fn alloc(size: usize) -> usize:\n    requested_bytes += u64(size)\n")
        .replace("pub fn alloc_class(size: usize, class: usize) -> usize:\n", "pub fn alloc_class(size: usize, class: usize) -> usize:\n    if class != 0:\n        requested_bytes += u64(size)\n")
        + "\npub fn requested_bytes_for_test() -> u64: requested_bytes\n";
    let mut sources = hd_driver::std_sources();
    sources.insert("json.hd", &instrumented_json);
    sources.insert("rt/alloc.hd", &instrumented_alloc);
    sources.insert(
        "rt_tests/json_object_remove.hd",
        include_str!("json_object_remove.hd"),
    );
    let store = crate::warm_store();
    let host = Host {
        sources: &sources,
        store: &store,
        render_tir: &[],
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    for level in [hd_driver::OptLevel::Dev, hd_driver::OptLevel::Optimized] {
        let out = build_packages(
            &host,
            "std",
            &Packages {
                level,
                build_profile: hd_driver::BuildProfile::Checked,
                std_leaf: Some("rt_tests/json_object_remove.hd"),
                ..Packages::default()
            },
            &Goal::Program {
                entry: "rt_tests.json_object_remove".into(),
            },
        );
        if out.diags.has_errors() {
            return Err(out.render());
        }
        let path =
            hd_testkit::Scratch::unmade(env!("CARGO_TARGET_TMPDIR"), "json-object-remove.wasm");
        std::fs::write(&path, out.wasm.ok_or("no object test module")?)
            .map_err(|e| e.to_string())?;
        // cost: one Node process for each of two fixed pipelines; guest counters measure the work.
        let ran = std::process::Command::new("node")
            .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.ts"))
            .arg(&path)
            .output()
            .map_err(|e| e.to_string())?;
        assert!(
            ran.status.success(),
            "{}{}",
            String::from_utf8_lossy(&ran.stdout),
            String::from_utf8_lossy(&ran.stderr)
        );
        let output = String::from_utf8(ran.stdout).map_err(|e| e.to_string())?;
        println!("{level:?}: {output}");
        assert_eq!(output.lines().count(), 4);
    }
    Ok(())
}
