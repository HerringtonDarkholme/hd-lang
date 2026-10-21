//! Instruction-source attributes survive the real emission/optimization pipeline.

use std::collections::BTreeSet;

use hd_cache::MemoryStore;
use hd_driver::{
    EngineTarget, Executor, Goal, Host, NoClock, OptLevel, Output, Packages, PositionPool,
    build_packages,
};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn compile(
    store: &MemoryStore,
    source: &str,
    target: EngineTarget,
    level: OptLevel,
    mapped: bool,
) -> Output {
    compile_goal(
        store,
        source,
        target,
        level,
        mapped,
        &Goal::Program {
            entry: "main".into(),
        },
    )
}

fn compile_goal(
    store: &MemoryStore,
    source: &str,
    target: EngineTarget,
    level: OptLevel,
    mapped: bool,
    goal: &Goal,
) -> Output {
    let mut sources = MemorySources::default();
    sources.insert("main.hd", source);
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let packages = Packages {
        target,
        level,
        instruction_origins: mapped,
        ..Packages::default()
    };
    let output = build_packages(&host, "app", &packages, goal);
    assert!(!output.diags.has_errors(), "{}", output.render());
    assert!(output.wasm.is_some(), "{}", output.report.render());
    output
}

#[test]
fn instruction_origins_survive_nbody_advance_and_leave_bytes_unchanged() {
    let source = include_str!("../../../../bench/study/nbody.hd");
    let store = MemoryStore::default();
    for target in [EngineTarget::V8, EngineTarget::Wasmtime] {
        for level in [OptLevel::Dev, OptLevel::Optimized] {
            let plain = compile(&store, source, target, level, false);
            let mapped = compile(&store, source, target, level, true);
            assert_eq!(plain.wasm, mapped.wasm, "{target:?} {level:?}");
            assert!(plain.instruction_origins.is_empty());
            let advance = mapped
                .instruction_origins
                .iter()
                .find(|f| f.name == "app/main/advance#0")
                .expect("advance mapping");
            assert_eq!(advance.item.as_deref(), Some("app/main/advance"));
            assert!(!advance.before_opt.is_empty() && !advance.instructions.is_empty());
            let before: BTreeSet<_> = advance.before_opt.iter().copied().flatten().collect();
            let after: BTreeSet<_> = advance
                .instructions
                .iter()
                .filter_map(|i| i.origin)
                .collect();
            assert!(!after.is_empty());
            assert!(
                after.is_subset(&before),
                "rewrites must not invent TIR instruction ids"
            );
            assert!(
                advance
                    .instructions
                    .windows(2)
                    .all(|w| w[0].offset < w[1].offset)
            );
            assert_eq!(
                operators(mapped.wasm.as_ref().unwrap(), advance.index)
                    .iter()
                    .map(|(offset, _)| *offset)
                    .collect::<Vec<_>>(),
                advance
                    .instructions
                    .iter()
                    .map(|i| i.offset)
                    .collect::<Vec<_>>()
            );
            let mut pool = PositionPool::default();
            let positions = mapped.positions(&mut pool);
            let before_lines: BTreeSet<_> = before
                .iter()
                .filter_map(|&i| positions.line("app/main/advance", i).map(|(_, l)| l))
                .collect();
            let after_lines: BTreeSet<_> = after
                .iter()
                .filter_map(|&i| positions.line("app/main/advance", i).map(|(_, l)| l))
                .collect();
            // All nine field-write subjects must remain attributable, not just calls.
            let writes: BTreeSet<_> = source
                .lines()
                .enumerate()
                .filter(|(_, line)| {
                    let line = line.trim_start();
                    (line.starts_with("bodies[i].") || line.starts_with("bodies[j]."))
                        && line.contains(" = ")
                })
                .filter(|(i, _)| {
                    *i < source
                        .lines()
                        .position(|l| l.starts_with("pub fn main"))
                        .unwrap()
                })
                .map(|(i, _)| u32::try_from(i + 1).unwrap())
                .collect();
            assert_eq!(writes.len(), 9);
            assert!(writes.is_subset(&before_lines), "before: {before_lines:?}");
            assert!(writes.is_subset(&after_lines), "after: {after_lines:?}");
        }
    }
}

#[test]
fn instruction_origins_are_rebuilt_on_a_warm_link_cache() {
    let source = "pub fn main() -> void $ Console:\n    println(7)\n";
    let store = MemoryStore::default();
    let first = compile(&store, source, EngineTarget::V8, OptLevel::Dev, false);
    let cached = compile(&store, source, EngineTarget::V8, OptLevel::Dev, false);
    assert_eq!(cached.counters.hit("link"), 1);
    let mapped = compile(&store, source, EngineTarget::V8, OptLevel::Dev, true);
    assert_eq!(mapped.counters.hit("link"), 0);
    assert!(!mapped.instruction_origins.is_empty());
    assert_eq!(first.wasm, mapped.wasm);
    assert_eq!(first.wasm, cached.wasm);
    let cached_again = compile(&store, source, EngineTarget::V8, OptLevel::Dev, false);
    assert_eq!(cached_again.counters.hit("link"), 1);
    assert!(cached_again.instruction_origins.is_empty());
}

fn operators(wasm: &[u8], function: u32) -> Vec<(u32, wasmparser::Operator<'_>)> {
    let mut index = 0;
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        match payload.unwrap() {
            wasmparser::Payload::ImportSection(imports) => {
                for import in imports.into_imports() {
                    if matches!(import.unwrap().ty, wasmparser::TypeRef::Func(_)) {
                        index += 1;
                    }
                }
            }
            wasmparser::Payload::CodeSectionEntry(body) => {
                if index == function {
                    let start = body.range().start;
                    let mut reader = body.get_operators_reader().unwrap();
                    let mut output = Vec::new();
                    while !reader.eof() {
                        let offset = u32::try_from(reader.original_position() - start).unwrap();
                        output.push((offset, reader.read().unwrap()));
                    }
                    return output;
                }
                index += 1;
            }
            _ => {}
        }
    }
    panic!("missing function {function}")
}

#[test]
fn instruction_origins_cover_wide_erased_and_suspending_bodies() {
    let store = MemoryStore::default();
    let cases = [
        (
            include_str!("../../../../spec/conformance/runtime/valid/wide-value-field-reads.hd"),
            false,
        ),
        (
            include_str!("../../../../spec/conformance/runtime/valid/wide-value-suspension.hd"),
            true,
        ),
        (
            include_str!(
                "../../../../spec/conformance/runtime/valid/parameter-reference-loan-erased-views.hd"
            ),
            false,
        ),
        (
            include_str!("../../../../spec/conformance/runtime/valid/nested-closure-captures.hd"),
            false,
        ),
        (
            include_str!(
                "../../../../spec/conformance/runtime/valid/suspending-closure-captures.hd"
            ),
            false,
        ),
        (
            include_str!(
                "../../../../spec/conformance/runtime/valid/string-builder-interpolation-order.hd"
            ),
            false,
        ),
    ];
    for (source, main) in cases {
        let goal = if main {
            Goal::Program {
                entry: "main".into(),
            }
        } else {
            Goal::Tests {
                module: None,
                filter: None,
            }
        };
        let plain = compile_goal(
            &store,
            source,
            EngineTarget::V8,
            OptLevel::Dev,
            false,
            &goal,
        );
        let mapped = compile_goal(&store, source, EngineTarget::V8, OptLevel::Dev, true, &goal);
        assert_eq!(plain.wasm, mapped.wasm);
        let own: Vec<_> = mapped
            .instruction_origins
            .iter()
            .filter(|f| f.item.as_ref().is_some_and(|p| p.starts_with("app/")))
            .collect();
        assert!(!own.is_empty());
        for function in own {
            let ops = operators(mapped.wasm.as_ref().unwrap(), function.index);
            assert_eq!(
                ops.iter().map(|(offset, _)| *offset).collect::<Vec<_>>(),
                function
                    .instructions
                    .iter()
                    .map(|i| i.offset)
                    .collect::<Vec<_>>(),
                "{}",
                function.name
            );
            assert!(
                function.instructions.iter().all(|i| i.origin.is_some()),
                "{}",
                function.name
            );
        }
    }
}

#[test]
fn instruction_origins_put_scope_exit_releases_on_the_scope() {
    let source = "fn length(value: string) -> usize:\n    text := \"$value!\"\n    text.len()\n\npub fn main() -> void $ Console:\n    println(length(\"work\"))\n";
    let output = compile(
        &MemoryStore::default(),
        source,
        EngineTarget::V8,
        OptLevel::Dev,
        true,
    );
    let function = output
        .instruction_origins
        .iter()
        .find(|f| f.name == "app/main/length#0")
        .unwrap();
    let scope = function
        .instructions
        .last()
        .unwrap()
        .origin
        .expect("function scope");
    let ops = operators(output.wasm.as_ref().unwrap(), function.index);
    let mut scope_release = false;
    for ((_, op), source) in ops.iter().zip(&function.instructions) {
        if let wasmparser::Operator::Call { function_index } = op {
            let release = output
                .instruction_origins
                .iter()
                .any(|f| f.index == *function_index && f.name.contains("release"));
            if release && source.origin == Some(scope) {
                scope_release = true;
            }
        }
    }
    assert!(
        scope_release,
        "scope-exit release must carry the scope's TIR origin"
    );
}
