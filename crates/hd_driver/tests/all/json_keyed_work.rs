//! `decode`'s key loop names each key's member in O(key bytes): a 64-field
//! struct whose keys come shuffled costs the same key comparisons per
//! document at n and 10n documents (#1809).
use hd_driver::{Executor, Goal, Host, NoClock, Packages, build_packages};
use hd_sched::SerialOrder;

const FIELDS: usize = 64;

#[test]
fn shuffled_keys_of_a_wide_record_cost_linear_key_work() -> Result<(), String> {
    let json = include_str!("../../../../lib/std/json.hd");
    // Every key comparison or key copy the reader makes goes through one of
    // these two Parser methods, and the name table is filled in `table_for`.
    let hooks = [
        "    fn key_at(&mut self, quote: usize) -> Result[string, JsonError]:\n",
        "    fn key_is(&mut self, quote: usize, name: string) -> Result[bool, JsonError]:\n",
        "            if named(&members[k]):\n                index[member_name(&members[k], facts)] = k\n",
    ];
    let mut instrumented = "let key_steps: usize = 0\n\n".to_owned() + json;
    for hook in hooks {
        assert_eq!(json.matches(hook).count(), 1, "counter hook: {hook}");
        let indent = if hook.starts_with("            ") {
            "                "
        } else {
            "        "
        };
        instrumented = instrumented.replacen(hook, &format!("{hook}{indent}key_steps += 1\n"), 1);
    }
    instrumented += "\npub fn key_steps_for_test() -> usize: key_steps\n";

    // Keys in the order 37·i mod 64 (37 is odd, so every field once), and
    // one unknown key per object.
    let fields = (0..FIELDS)
        .map(|i| format!("    f{i}: i64"))
        .collect::<Vec<_>>()
        .join("\n");
    let object: String = {
        let mut keys: Vec<String> = (0..FIELDS)
            .map(|i| (i * 37) % FIELDS)
            .map(|f| format!("\\\"f{f}\\\":{f}"))
            .collect();
        keys.insert(FIELDS / 2, "\\\"other\\\":[1,{\\\"f1\\\":2}]".into());
        format!("{{{}}}", keys.join(","))
    };
    let program = format!(
        r#"use std.json.{{decode, key_steps_for_test}}
use std.serde.Deserialize
use std.testing.assert_equal

@derive(Deserialize)
struct Wide:
{fields}

fn documents(n: usize) -> string:
    let mut parts: List[string] = []
    for _ in 0..n:
        parts.push("{object}")
    "[" + parts.join(",") + "]"

pub fn main() -> void $ Console:
    for n in [10, 100]:
        text := documents(n)
        before := key_steps_for_test()
        match decode::[List[Wide]](text):
            .Ok(rows) =>
                assert_equal(rows.len(), n)
                assert_equal(rows[n - 1].f63, 63)
            .Err(error) => panic("wide decode failed: $error")
        println("${{key_steps_for_test() - before}}")
"#
    );
    let mut sources = hd_driver::std_sources();
    sources.insert("json.hd", &instrumented);
    sources.insert("rt_tests/json_keyed_work.hd", &program);
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
            std_leaf: Some("rt_tests/json_keyed_work.hd"),
            ..Packages::default()
        },
        &Goal::Program {
            entry: "rt_tests.json_keyed_work".into(),
        },
    );
    if out.diags.has_errors() {
        return Err(out.render());
    }
    let path = hd_testkit::Scratch::unmade(env!("CARGO_TARGET_TMPDIR"), "json-keyed-work.wasm");
    std::fs::write(&path, out.wasm.ok_or("no keyed work module")?).map_err(|e| e.to_string())?;
    // cost: one Node process; the guest counter measures the work.
    let ran = hd_testkit::node::command()
        .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.ts"))
        .arg(&path)
        .output()
        .map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&ran.stdout).into_owned();
    assert!(
        ran.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    let steps: Vec<usize> = stdout
        .lines()
        .map(|line| line.trim().parse().map_err(|_| stdout.clone()))
        .collect::<Result<_, _>>()?;
    let [small, large] = steps[..] else {
        return Err(format!("two counts expected: {stdout}"));
    };
    let keys = 10 * (FIELDS + 1);
    println!("key steps: {small} at 10 documents, {large} at 100");
    // The guess, then one table lookup per key: at most two steps per key,
    // plus the table built once for the type and the first 64 misses of a
    // decode, each scanning at most every field.
    assert!(
        small <= 2 * keys + FIELDS + 64 * FIELDS,
        "{small} steps for {keys} keys"
    );
    assert!(
        large <= 10 * small,
        "{large} steps at 10n against {small} at n"
    );
    assert!(
        large >= 9 * small.saturating_sub(65 * FIELDS) && large > small,
        "{large} steps at 10n against {small} at n"
    );
    Ok(())
}
