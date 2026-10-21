//! Development views preserve actual code and attribution across optimizers.
use hd_driver::{EngineTarget, Executor, Goal, Host, NoClock, OptLevel, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

const SOURCE: &str = "\
struct Value:
    flag: bool
    text: string
    items: List[i32]
fn duplicate(value: Value) -> (Value, Value):
    (value, value)
fn duplicate_list(items: List[i32]) -> (List[i32], List[i32]):
    (items, items)
";

#[test]
fn count_labels_preserve_bytes_offsets_and_real_source_origins() {
    let mut sources = MemorySources::default();
    sources.insert("main.hd", SOURCE);
    let store = crate::warm_store();
    let host = Host {
        sources: &sources,
        store: &store,
        render_tir: &[],
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let goal = Goal::Fixture {
        module: "main".into(),
    };
    let mut by_level = [None, None];
    for target in [EngineTarget::V8, EngineTarget::Wasmtime] {
        for level in [OptLevel::Dev, OptLevel::Optimized] {
            let plain = build_packages(
                &host,
                "app",
                &Packages {
                    target,
                    level,
                    ..Packages::default()
                },
                &goal,
            );
            let mapped = build_packages(
                &host,
                "app",
                &Packages {
                    target,
                    level,
                    instruction_origins: true,
                    ..Packages::default()
                },
                &goal,
            );
            assert!(!mapped.diags.has_errors(), "{}", mapped.render());
            assert_eq!(plain.wasm, mapped.wasm, "{target:?} {level:?}");
            let list = mapped
                .instruction_origins
                .iter()
                .find(|f| f.name == "app/main/duplicate_list#0")
                .expect("list map");
            let operations: Vec<_> = list
                .counts
                .iter()
                .map(|c| (c.action, c.subject.clone(), c.origin))
                .collect();
            let baseline = &mut by_level[usize::from(level == OptLevel::Optimized)];
            if let Some(expected) = baseline {
                assert_eq!(
                    &operations, expected,
                    "inlining must not duplicate or lose count sites"
                );
            } else {
                *baseline = Some(operations);
            }
            let f = mapped
                .instruction_origins
                .iter()
                .find(|f| f.name == "app/main/duplicate#0")
                .expect("duplicate map");
            assert!(!f.counts.is_empty());
            assert!(
                f.counts
                    .iter()
                    .any(|c| c.subject == "app.main.Value [text.0, items]")
            );
            let mut pool = hd_driver::PositionPool::default();
            let positions = mapped.positions(&mut pool);
            for c in &f.counts {
                assert!(
                    f.instructions.iter().any(|i| i.offset == c.offset),
                    "count has an actual encoded opcode"
                );
                let origin = c.origin.expect("a real source instruction");
                let (file, lo, hi) = positions
                    .span(f.item.as_deref().expect("item"), origin)
                    .expect("source span");
                assert_eq!(file, "main.hd");
                assert!(lo < hi && hi as usize <= SOURCE.len());
            }
            for origin in f
                .before_opt
                .iter()
                .chain(f.instructions.iter().map(|i| &i.origin))
                .flatten()
            {
                assert!(
                    positions
                        .span(f.item.as_deref().expect("item"), *origin)
                        .is_some(),
                    "internal metadata tokens never escape"
                );
            }
            let layout = &mapped
                .fixture_layouts
                .iter()
                .find(|l| l.layout.name == "app.main.Value")
                .expect("layout")
                .layout;
            assert_eq!(layout.size, 16);
            assert_eq!(
                layout
                    .fields
                    .iter()
                    .map(|f| (f.path.as_str(), f.offset))
                    .collect::<Vec<_>>(),
                [("flag", 0), ("text", 4), ("items", 12)]
            );
            assert_eq!(
                layout.counted.iter().map(|l| l.offset).collect::<Vec<_>>(),
                [4, 12]
            );
        }
    }
}

#[test]
fn topic_views_ignore_unrelated_source_and_function_numbering() {
    use hd_testkit::stages::{Program, view};
    let code =
        "fn duplicate(value: string) -> (string, string):\n    # subject\n    (value, value)\n";
    let plain = Program::new(code).expect("program");
    let extra = Program::new(&format!("struct Other:\n    text: string\nfn unrelated(text: string) -> (string, string): (text, text)\n{code}")).expect("program");
    let expected = view("counts", &plain).expect("counts");
    assert!(expected.contains("retain string"), "{expected}");
    assert_eq!(expected, view("counts", &extra).expect("counts"));
    let code = "# subject\nstruct Pair:\n    small: u8\n    large: i64\n";
    let plain = Program::new(code).expect("program");
    let extra = Program::new(&format!("struct Other:\n    text: string\n{code}")).expect("program");
    assert_eq!(view("layout", &plain), view("layout", &extra));
}
