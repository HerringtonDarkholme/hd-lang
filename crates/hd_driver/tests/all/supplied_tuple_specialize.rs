//! Supplied tuple traversal families survive specialization and keep
//! borrowed operand lifetimes, custom walker effects and enum checks.

use hd_cache::MemoryStore;
use hd_driver::{BuildProfile, Executor, Goal, Host, NoClock, OptLevel, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

const SOURCE: &str = include_str!("programs/supplied_tuple_specialize/source.hd");

#[test]
fn tuple_comparisons_keep_owners_walker_effects_and_unordered_results()
-> Result<(), Box<dyn std::error::Error>> {
    let mut sources = MemorySources::default();
    let fields = std::iter::repeat_n("string", 32)
        .collect::<Vec<_>>()
        .join(", ");
    let wide =
        format!("@cold\nfn wide(left: ({fields}), right: ({fields})) -> bool:\n    left < right");
    let mut left = vec!["left._0"; 31];
    left.push("\"a\"");
    let mut right = vec!["right._0"; 31];
    right.push("\"b\"");
    let check = format!("check(wide(({}), ({})))", left.join(", "), right.join(", "));
    let observed_fields = std::iter::repeat_n("Observed", 32)
        .collect::<Vec<_>>()
        .join(", ");
    let wide_effect = format!(
        "@cold\nfn wide_effect(left: ({observed_fields}), right: ({observed_fields})) -> bool:\n    left < right"
    );
    let mut effect_left = vec!["observed"; 31];
    effect_left.push("Observed { value: +2 }");
    let mut effect_right = vec!["observed"; 31];
    effect_right.push("Observed { value: +3 }");
    let effect_check = format!(
        "check(wide_effect(({}), ({})))",
        effect_left.join(", "),
        effect_right.join(", ")
    );
    let source = SOURCE
        .replace("# WIDE_FUNCTION", &format!("{wide}\n\n{wide_effect}"))
        .replace("# WIDE_CHECK", &check)
        .replace("# EFFECT_CHECK", &effect_check);
    sources.insert("main.hd", &source);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &["app/main/wide_effect", "app/main/ordered"],
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    for level in [OptLevel::Dev, OptLevel::Optimized] {
        for checked in [false, true] {
            let packages = Packages {
                build_profile: BuildProfile::Release,
                level,
                checked,
                verify_own: true,
                ..Packages::default()
            };
            let out = build_packages(
                &host,
                "app",
                &packages,
                &Goal::Program {
                    entry: "main".into(),
                },
            );
            assert!(!out.diags.has_errors(), "{}", out.render());
            if level == OptLevel::Optimized {
                for (path, writes) in [("app/main/wide_effect", true), ("app/main/ordered", false)]
                {
                    let owned = &out.own_text[path].text;
                    let summary = owned
                        .lines()
                        .find(|line| line.starts_with("summary "))
                        .ok_or("missing summary")?;
                    assert_eq!(summary.contains("writes"), writes, "{path}: {owned}");
                }
            }
            let wasm = out.wasm.ok_or("no emitted module")?;
            let path =
                hd_testkit::Scratch::unmade(env!("CARGO_TARGET_TMPDIR"), "supplied-tuple.wasm");
            std::fs::write(&path, wasm)?;
            // cost: one Node process for each of four fixed pipeline/check combinations.
            let ran = hd_testkit::node::command()
                .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.ts"))
                .arg(&path)
                .output()?;
            assert!(
                ran.status.success(),
                "{level:?} checked={checked}: {}",
                String::from_utf8_lossy(&ran.stderr)
            );
            assert_eq!(String::from_utf8(ran.stdout)?.trim(), "ok");
        }
    }
    Ok(())
}

#[test]
fn tuple_effect_summaries_borrow_small_and_shared_wide_operands() {
    let store = MemoryStore::default();
    // Exercise small and shared-wide instances. The separate scale test
    // also builds 4,096 fields and checks verifier work, not elapsed time.
    for width in [2, 32, 128, 2048] {
        let fields = std::iter::repeat_n("string", width)
            .collect::<Vec<_>>()
            .join(", ");
        let source = format!(
            "use std.annotation.cold\ntype Words = ({fields})\n@cold\nfn ordered(left: Words, right: Words) -> bool:\n    left < right\npub fn main() -> void:\n    pass\n"
        );
        let mut sources = MemorySources::default();
        sources.insert("main.hd", &source);
        let host = Host {
            sources: &sources,
            store: &store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Priority),
            render_tir: &["app/main/ordered"],
        };
        let out = build_packages(
            &host,
            "app",
            &Packages {
                build_profile: BuildProfile::Release,
                level: OptLevel::Optimized,
                verify_own: true,
                ..Packages::default()
            },
            &Goal::Fixture {
                module: "main".into(),
            },
        );
        assert!(!out.diags.has_errors(), "{}", out.render());
        let owned = &out.own_text["app/main/ordered"].text;
        let summary = owned.lines().find(|line| line.starts_with("summary "));
        eprintln!("tuple width {width}: {summary:?}");
        assert!(
            summary.is_some_and(|line| line.contains("borrows [$0 $1]")
                && !line.contains("writes")
                && !line.contains("kept [$")),
            "width {width}: {owned}"
        );
    }
}

#[test]
fn existing_read_only_string_helpers_keep_their_effects() {
    let source = include_str!(
        "../cases/counts/a-field-through-a-local-reference-borrows-across-a-pure-call.hd"
    );
    let mut sources = MemorySources::default();
    sources.insert("main.hd", source);
    let store = MemoryStore::default();
    let host = Host {
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
        render_tir: &[
            "std/serde/member_name",
            "std/serde/case_name",
            "std/text/concat_all",
            "app/main/impl Reader.name",
        ],
    };
    let out = build_packages(
        &host,
        "app",
        &Packages {
            build_profile: BuildProfile::Release,
            level: OptLevel::Optimized,
            verify_own: true,
            ..Packages::default()
        },
        &Goal::Fixture {
            module: "main".into(),
        },
    );
    assert!(!out.diags.has_errors(), "{}", out.render());
    let owned = &out.own_text["std/serde/member_name"].text;
    assert!(
        owned
            .lines()
            .any(|line| line.starts_with("summary ") && !line.contains("writes")),
        "{owned}"
    );
}

#[test]
fn wide_tuple_comparisons_build_with_linear_place_verifier_work() {
    let mut measured = Vec::new();
    for width in [2048, 4096] {
        let fields = vec!["string"; width].join(", ");
        let text = include_str!("inputs/wide-tuple-comparison.hd").replace("FIELDS", &fields);
        let mut sources = MemorySources::default();
        sources.insert("main.hd", &text);
        let store = MemoryStore::default();
        let host = Host {
            sources: &sources,
            store: &store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Priority),
            render_tir: &[],
        };
        let (out, work) = hd_tir::work::count_empty_place_work(|| {
            build_packages(
                &host,
                "app",
                &Packages {
                    build_profile: BuildProfile::Release,
                    level: OptLevel::Optimized,
                    verify_own: true,
                    ..Packages::default()
                },
                &Goal::Fixture {
                    module: "main".into(),
                },
            )
        });
        assert!(!out.diags.has_errors(), "{}", out.render());
        assert!(out.wasm.is_some());
        eprintln!("tuple width {width}: place-verifier work {work}");
        measured.push(work);
    }
    assert!(
        measured[0] > 0 && measured[1] * 4 <= measured[0] * 9,
        "{measured:?}"
    );
}
