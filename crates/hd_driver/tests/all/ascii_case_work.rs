//! ASCII case conversion scans/copies O(bytes), without a host fallback.
use hd_driver::{Executor, Goal, Host, NoClock, Packages, build_packages};
use hd_sched::SerialOrder;

#[test]
fn ascii_case_work_is_linear_and_unicode_keeps_its_host_fallback() -> Result<(), String> {
    let text = include_str!("../../../../lib/std/text.hd");
    let (before, helpers) = text
        .split_once("@inline(.Always)\nfn case_ascii_only")
        .ok_or("ASCII helper missing")?;
    // Count word loads, scalar inspections, stores, and bulk-copied bytes.
    // Only these private helpers are instrumented; their callers are unchanged.
    let hooks = [
        ("    if size >= 8:\n", "        case_units += 2\n", 1),
        (
            "        while size - at >= 32:\n",
            "            case_units += 4\n",
            1,
        ),
        (
            "        while size - at >= 8:\n",
            "            case_units += 1\n",
            1,
        ),
        ("    while at < size:\n", "        case_units += 1\n", 2),
        ("    while first < size:\n", "        case_units += 1\n", 1),
        (
            "    copy(address, string_bytes(text).address, size)\n",
            "    case_units += size\n",
            1,
        ),
        (
            "            store_u8(address + at, mapped)\n",
            "            case_units += 1\n",
            1,
        ),
    ];
    let mut helpers = helpers.to_owned();
    for (hook, counter, count) in hooks {
        assert_eq!(helpers.matches(hook).count(), count, "counter hook: {hook}");
        helpers = helpers.replace(hook, &format!("{hook}{counter}"));
    }
    let fallback = "        return if upper: host_upper(text) else: host_lower(text)\n";
    assert_eq!(helpers.matches(fallback).count(), 1);
    helpers = helpers.replace(
        fallback,
        &format!("        case_fallbacks += 1\n{fallback}"),
    );
    let instrumented = format!(
        "let case_units: usize = 0\nlet case_fallbacks: usize = 0\n\n\
         {before}@inline(.Always)\nfn case_ascii_only{helpers}\n\
         pub fn case_units_for_test() -> usize: case_units\n\
         pub fn case_fallbacks_for_test() -> usize: case_fallbacks\n"
    );
    let program = r#"use std.text.{case_units_for_test, case_fallbacks_for_test}

pub fn main() -> void $ Console:
    for n in [1000, 10000]:
        text := "A".repeat(n)
        before := case_units_for_test()
        fallbacks := case_fallbacks_for_test()
        bytes := text.lower().len() + text.upper().len()
        println("${case_units_for_test() - before} ${case_fallbacks_for_test() - fallbacks} $bytes")
        mixed := text + "Σ"
        unicode_before := case_units_for_test()
        unicode_fallbacks := case_fallbacks_for_test()
        unicode_bytes := mixed.lower().len() + mixed.upper().len()
        println("${case_units_for_test() - unicode_before} ${case_fallbacks_for_test() - unicode_fallbacks} $unicode_bytes")
"#;
    let mut sources = hd_driver::std_sources();
    sources.insert("text.hd", &instrumented);
    sources.insert("rt_tests/ascii_case_work.hd", program);
    let store = crate::warm_store();
    let host = Host {
        sources: &sources,
        store: &store,
        render_tir: &[],
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build_packages(
        &host,
        "std",
        &Packages {
            level: hd_driver::OptLevel::Optimized,
            std_leaf: Some("rt_tests/ascii_case_work.hd"),
            ..Packages::default()
        },
        &Goal::Program {
            entry: "rt_tests.ascii_case_work".into(),
        },
    );
    if out.diags.has_errors() {
        return Err(out.render());
    }
    let dir = env!("CARGO_TARGET_TMPDIR");
    let wasm = hd_testkit::Scratch::unmade(dir, "ascii-case-work.wasm");
    let config = hd_testkit::Scratch::unmade(dir, "ascii-case-work.json");
    std::fs::write(&wasm, out.wasm.ok_or("no ASCII cost module")?).map_err(|e| e.to_string())?;
    std::fs::write(&config, r#"{"timeLimitMs":10000}"#).map_err(|e| e.to_string())?;
    let ran = hd_testkit::node::command()
        .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.ts"))
        .arg(&wasm)
        .arg(&config)
        .output()
        .map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&ran.stdout);
    assert!(
        ran.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    let rows = stdout
        .lines()
        .map(|line| {
            line.split_whitespace()
                .map(|word| word.parse::<usize>().map_err(|e| e.to_string()))
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;
    let [small, unicode_small, large, unicode_large] = rows.as_slice() else {
        return Err(format!("four cost rows expected: {stdout}"));
    };
    let [small, 0, 2000] = small[..] else {
        return Err(format!("ASCII n must avoid the host: {stdout}"));
    };
    let [large, 0, 20000] = large[..] else {
        return Err(format!("ASCII 10n must avoid the host: {stdout}"));
    };
    assert_eq!(unicode_small, &[4, 2, 2004]);
    assert_eq!(unicode_large, &[4, 2, 20004]);
    assert!(small > 0 && small <= 6 * 1000 + 32, "{stdout}");
    assert!(large >= 9 * small && large <= 10 * small, "{stdout}");
    println!("ASCII work at n and 10n; Unicode fallback counts:\n{stdout}");
    Ok(())
}
