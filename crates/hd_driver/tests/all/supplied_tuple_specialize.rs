//! Supplied tuple traversal families survive specialization and keep
//! borrowed operand lifetimes, custom walker effects and enum checks.

use hd_cache::MemoryStore;
use hd_driver::{BuildProfile, Executor, Goal, Host, NoClock, OptLevel, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

const SOURCE: &str = r#"
use std.annotation.cold
use std.function.Tuple
use std.structure.{Field, Structure, Variant, Walker}

fn check(value: bool) -> void:
    if !value:
        panic("tuple specialization changed the result")

@cold
fn label(value: usize) -> string:
    "tuple-$value"

@cold
fn ordered(left: (string, usize), right: (string, usize)) -> bool:
    left < right

# WIDE_FUNCTION

let comparisons = +0

struct Observed:
    value: i32

impl Eq for Observed:
    fn eq(&self, other: &Observed) -> bool:
        self.value == other.value

impl PartialOrd for Observed:
    fn partial_cmp(&self, other: &Observed) -> Ordering?:
        comparisons += 1
        self.value.partial_cmp(&other.value)

let evaluations = +0

struct Marker:
    number: i32

fn stamp() -> Marker:
    evaluations += 1
    Marker { number: evaluations }

@derive(Eq, PartialOrd)
struct Marked:
    @stamp()
    value: string

@derive(Eq, PartialOrd)
enum Choice:
    Word(string)
    Number(i64)

trait Fields:
    fn fields(self) -> usize

struct Counter:
    count: usize

impl[S] Walker[S] for Counter:
    type Error = never
    fn variant(&mut self, v: Variant[S]) -> Result[void, never]:
        .Ok(())
    fn member[F](&mut self, h: Field[S, F], value: &F) -> Result[void, never]:
        self.count += 1
        .Ok(())

impl[T < Tuple] Fields for T by Structure:
    fn fields(self) -> usize:
        let mut counter = Counter { count: 0 }
        _ := Structure::walk(&self, &mut counter)
        counter.count

pub fn main() -> void $ Console:
    let mut values = [label(10)]
    left := (values[0], 1)
    right := (values[0], 2)
    check(ordered(left, right))
    check(!ordered(right, left))
    check(!ordered(left, left))
    values[0] = label(20)
    check(left._0 == "tuple-10")
    check(right._0 == "tuple-10")
    check(((left, "z") < (right, "a")))
    check(Choice.Word(left._0) < Choice.Number(+0))
    check(evaluations == +0)
    marked := Marked { value: left._0 }
    check(marked < Marked { value: "tuple-20" })
    check(evaluations == +1)
    check(!(marked < marked))
    check(evaluations == +1)
    check((left, values[0], +3).fields() == 3)
    # WIDE_CHECK
    check(comparisons == +0)
    observed := Observed { value: +1 }
    # EFFECT_CHECK
    check(comparisons == +32)
    let nan: f64 = 0.0 / 0.0
    check(!((nan, 1) < (1.0, 2)))
    check((1.0, nan) < (2.0, 0.0))
    println("ok")
"#;

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
            let ran = std::process::Command::new("node")
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
    for width in [2, 32, 128, 2048, 4096] {
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
