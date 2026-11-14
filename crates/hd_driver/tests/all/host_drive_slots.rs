//! A closed program's immediate host drives do not root unused cold slots.

use hd_driver::{BuildProfile, EngineTarget, Executor, Goal, Host, NoClock, OptLevel, Packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn linked_slots(
    source: &str,
    target: EngineTarget,
    profile: BuildProfile,
    level: OptLevel,
) -> Vec<(String, bool)> {
    let mut sources = MemorySources::default();
    sources.insert("main.hd", source);
    let store = crate::warm_store();
    let out = hd_driver::build_packages(
        &Host {
            render_tir: &[],
            sources: &sources,
            store: &store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Priority),
        },
        "app",
        &Packages {
            target,
            build_profile: profile,
            level,
            ..Packages::default()
        },
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(!out.diags.has_errors(), "{}", out.render());
    let wasm = out.wasm.expect("the program links");
    wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
        .validate_all(&wasm)
        .expect("valid functions and element slots");
    let mut names = Vec::new();
    let mut table = std::collections::BTreeSet::new();
    for payload in wasmparser::Parser::new(0).parse_all(&wasm) {
        match payload.expect("payload") {
            wasmparser::Payload::CustomSection(section) if section.name() == "name" => {
                let map = wasmparser::NameSectionReader::new(wasmparser::BinaryReader::new(
                    section.data(),
                    section.data_offset(),
                ));
                for subsection in map {
                    if let wasmparser::Name::Function(map) = subsection.expect("name subsection") {
                        names.extend(map.into_iter().map(|entry| {
                            let entry = entry.expect("name");
                            (entry.index, entry.name.to_owned())
                        }));
                    }
                }
            }
            wasmparser::Payload::CustomSection(section) if section.name() == "hd.names" => {
                let mut reader =
                    wasmparser::BinaryReader::new(section.data(), section.data_offset());
                for _ in 0..reader.read_var_u32().expect("compact name count") {
                    let index = reader.read_var_u32().expect("function index");
                    names.push((
                        index,
                        reader.read_string().expect("function name").to_owned(),
                    ));
                }
                assert!(reader.eof());
            }
            wasmparser::Payload::ElementSection(elements) => {
                for element in elements {
                    match element.expect("element").items {
                        wasmparser::ElementItems::Functions(indices) => {
                            table.extend(
                                indices
                                    .into_iter()
                                    .map(|index| index.expect("table function")),
                            );
                        }
                        wasmparser::ElementItems::Expressions(_, expressions) => {
                            for expression in expressions {
                                let mut ops = expression
                                    .expect("element expression")
                                    .get_operators_reader();
                                if let wasmparser::Operator::RefFunc { function_index } =
                                    ops.read().expect("ref.func")
                                {
                                    table.insert(function_index);
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    names
        .into_iter()
        .map(|(index, name)| (name, table.contains(&index)))
        .collect()
}

#[test]
fn only_immediate_blocking_host_calls_omit_the_normal_cold_slot() {
    let immediate = r"
use std.task.block_on
use std.time.{Clock, ms}
pub fn main() -> void $ Clock:
    let clock: &mut dyn Clock = $.use(Clock)
    _ := block_on(clock.sleep(ms(1)))
";
    let stored = r"
use std.task.block_on
use std.time.{Clock, ms}
fn cold() -> Suspend[void] $ Clock:
    let clock: *mut dyn Clock = $.use(Clock)
    clock.sleep(ms(1))
pub fn main() -> void $ Clock:
    _ := block_on(cold())
";
    // An await may need the normal slot in a shielded dispose suite. Retain
    // asynchronous methods conservatively, even beside a blocking drive.
    let asynchronous = r"
use std.task.block_on
use std.time.{Clock, ms}
pub fn main!() -> void $ Clock:
    let clock: &mut dyn Clock = $.use(Clock)
    _ := block_on(clock.sleep(ms(1)))
    clock.sleep!(ms(1))
";
    let callable = r#"
use std.host.Sys
use std.task.block_on
use std.time.{Clock, ms}
fn invoke(change: fn(i32) -> i32, count: i32) -> i32:
    if count == 0: change(+0)
    else: invoke(change, count - 1)
pub fn main() -> void $ Console + Sys + Clock:
    let system: &dyn Sys = $.use(Sys)
    let change: fn(i32) -> i32 = if system.os().expect("os").len() == 0:
        fn(value: i32) -> i32: value + 1
    else:
        fn(value: i32) -> i32: value + 2
    let clock: &mut dyn Clock = $.use(Clock)
    _ := block_on(clock.sleep(ms(1)))
    println("${invoke(change, +1)}")
"#;
    // No trait-method adapter occurs here: the unresolved recursive callback
    // itself must retain the conservative cold-constructor ABI guard.
    let raw_constructor = r#"
use std.host.Sys
use std.task.block_on
use std.time.{Clock, ms}
fn first!() -> i32: 1
fn second!() -> i32: 2
fn invoke(make: fn() -> Suspend[i32], count: i32) -> Suspend[i32]:
    if count == 0: make()
    else: invoke(make, count - 1)
pub fn main() -> void $ Sys + Clock:
    let system: &dyn Sys = $.use(Sys)
    let make: fn() -> Suspend[i32] = if system.os().expect("os").len() == 0:
        first
    else:
        second
    let clock: &mut dyn Clock = $.use(Clock)
    _ := block_on(clock.sleep(ms(1)))
    _ := block_on(invoke(make, +1))
"#;
    let cold_callable = r"
use std.task.block_on
use std.time.{Clock, Duration, ms}
fn invoke(pause: fn(Duration) -> Suspend[void], d: Duration, count: i32) -> Suspend[void]:
    if count == 0: pause(d) else: invoke(pause, d, count - 1)
pub fn main!() -> void $ Clock:
    let clock: *mut dyn Clock = $.use(Clock)
    _ := block_on(clock.sleep(ms(1)))
    let pause: fn(Duration) -> Suspend[void] = clock::sleep
    let saved: Suspend[void] = invoke(pause, ms(1), +1)
    _ := block_on(saved)
    let bang: fn!(Duration) -> void = clock::sleep
    bang!(ms(1))
";
    let shielded = r"
use std.ops.AsyncDisposable
use std.task.block_on
use std.time.{Clock, Duration, ms}
struct Pause:
    clock: *mut dyn Clock
impl AsyncDisposable for Pause:
    fn dispose!(&mut self) -> void:
        let pause: fn!(Duration) -> void = self.clock::sleep
        pause!(ms(1))
pub fn main!() -> void $ Clock:
    let clock: &mut dyn Clock = $.use(Clock)
    _ := block_on(clock.sleep(ms(1)))
    using! pause = &mut Pause { clock: $.use(Clock) }
";
    for target in [EngineTarget::V8, EngineTarget::Wasmtime] {
        for (profile, level) in [
            (BuildProfile::Checked, OptLevel::Dev),
            (BuildProfile::Checked, OptLevel::Optimized),
            (BuildProfile::Release, OptLevel::Optimized),
        ] {
            for (source, expected) in [
                (immediate, false),
                (stored, true),
                (asynchronous, true),
                (callable, false),
                (shielded, true),
                (cold_callable, true),
                (raw_constructor, true),
            ] {
                let names = linked_slots(source, target, profile, level);
                assert_eq!(
                    names
                        .iter()
                        .any(|(name, in_table)| name == "rt:hd:Clock/sleep" && *in_table),
                    expected,
                    "{target:?}/{profile:?}/{level:?}: {names:?}"
                );
                if !expected {
                    assert!(
                        names
                            .iter()
                            .any(|(name, _)| name == "rt:hd:Clock/sleep.drive"
                                || name == "rt:hd:Clock/sleep.block_drive")
                    );
                }
            }
        }
    }
}

/// trait.dyn.supertrait-methods and module.host.package.marker: the same
/// package-host fixture exercises parent drives, its own pending drive,
/// and an immediate blocking parent call in each compiler pipeline.
#[test]
fn package_host_supertraits_keep_async_and_blocking_drive_slots()
-> Result<(), Box<dyn std::error::Error>> {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../spec/conformance/cli/host-supertrait-drive/src/main.hd"),
    )?;
    package_host_routes("host-supertraits-async-and-blocking", &source, false)
}

#[test]
fn transferred_host_arguments_survive_a_blocking_pending_call()
-> Result<(), Box<dyn std::error::Error>> {
    package_host_routes(
        "host-transferred-arguments-blocking-pending",
        include_str!("cold_blocking_host_arguments.hd"),
        true,
    )
}

fn package_host_routes(
    name: &str,
    source: &str,
    jspi: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    package_host_routes_with_provider(name, source, jspi, None)
}

fn package_host_routes_with_provider(
    name: &str,
    source: &str,
    jspi: bool,
    provider: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let fixture = root.join("spec/conformance/cli/host-supertrait-drive");
    let mut sources = MemorySources::default();
    sources.insert("main.hd", source);
    let store = crate::warm_store();
    let scratch = hd_testkit::Scratch::new(env!("CARGO_TARGET_TMPDIR"), name);
    let provider_path = if let Some(provider) = provider {
        let path = scratch.join("provider.mjs");
        std::fs::write(&path, provider)?;
        path
    } else {
        fixture.join("provider.ts")
    };
    for (profile, level, checked) in [
        (BuildProfile::Checked, OptLevel::Dev, false),
        (BuildProfile::Checked, OptLevel::Dev, true),
        (BuildProfile::Checked, OptLevel::Optimized, false),
        (BuildProfile::Checked, OptLevel::Optimized, true),
        (BuildProfile::Release, OptLevel::Optimized, false),
    ] {
        let out = hd_driver::build_packages(
            &Host {
                render_tir: &[],
                sources: &sources,
                store: &store,
                clock: &NoClock,
                executor: Executor::Serial(SerialOrder::Priority),
            },
            "super-host",
            &Packages {
                build_profile: profile,
                level,
                checked,
                verify_own: true,
                instruction_origins: true,
                host_packages: vec![hd_driver::HostPackage {
                    package: 0,
                    identity: fixture.to_string_lossy().into_owned(),
                    versioned_source: None,
                    manifest: "hd.toml".into(),
                    declarations: Some(hd_project::HostDeclarations {
                        providers: Some(vec![hd_project::HostProvider {
                            trait_path: "main.Child".into(),
                            path: Some("provider.ts".into()),
                            line: 5,
                        }]),
                        line: 4,
                    }),
                }],
                ..Packages::default()
            },
            &Goal::Program {
                entry: "main".into(),
            },
        );
        assert!(
            !out.diags.has_errors(),
            "{profile:?}/{level:?}/audit={checked}: {}",
            out.render()
        );
        for function in &out.instruction_origins {
            if function
                .name
                .rsplit('.')
                .next()
                .is_some_and(|mode| matches!(mode, "drive" | "block_drive"))
            {
                for count in &function.counts {
                    if count.subject == "host arguments" {
                        assert!(
                            count.origin.is_none(),
                            "a runtime site has no TIR instruction"
                        );
                    }
                    assert!(
                        !count.reason.to_string().starts_with("conservative:"),
                        "{}: {} {} ({})",
                        function.name,
                        count.action.name(),
                        count.subject,
                        count.reason,
                    );
                }
            }
        }
        let wasm = out.wasm.ok_or("missing Wasm")?;
        wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
            .validate_all(&wasm)?;
        let artifacts = out.host_artifacts.ok_or("missing host artifacts")?;
        assert_eq!(artifacts.contract.interfaces.len(), 1);
        let interface = &artifacts.contract.interfaces[0];
        assert_eq!(interface.identity.trait_path, "main.Child");
        assert_eq!(interface.methods.len(), 2, "own and inherited methods");
        let wasm_path = scratch.join("main.wasm");
        let glue_path = scratch.join("main.host.mjs");
        std::fs::write(&wasm_path, wasm)?;
        std::fs::write(&glue_path, artifacts.javascript)?;
        let script = format!(
            r"
import {{ readFileSync }} from 'node:fs';
import {{ pathToFileURL }} from 'node:url';
const root = pathToFileURL({root:?} + '/');
const {{ createCoreHost }} = await import(new URL('host/core.ts', root));
const {{ cycleReport }} = await import(new URL('host/checked-report.ts', root));
const {{ createPackageRuntime }} = await import(new URL('host/package-providers.ts', root));
const generated = await import(pathToFileURL({glue_path:?}));
// Exercise hosts without JSPI explicitly: Node 26 enables it by default.
if (!{jspi}) {{
  Object.defineProperties(WebAssembly, {{ Suspending: {{ value: undefined }}, promising: {{ value: undefined }} }});
}}
const {{ default: provider }} = await import(pathToFileURL({provider_path:?}));
const module = new WebAssembly.Module(readFileSync({wasm_path:?}));
let diagnostics = '';
const host = createCoreHost({{out: text => process.stdout.write(text), err: text => {{ diagnostics += text; }} }}, {{
  module, createPackageRuntime, grants: {{Console: true, 'pkg.main.Child': true}},
  packageBindings: runtime => generated.bind(runtime, new Map([[{module_name:?}, provider]])),
}});
try {{
  const instance = new WebAssembly.Instance(module, host.imports);
  const result = await host.run(instance, 'hd.init', 'hd.poll');
  if (result.trapped || result.status !== 0) throw Error(JSON.stringify({{result, diagnostics}}));
  // Checked reporting is an audit message; reject every other diagnostic.
  if (diagnostics !== (cycleReport(module, instance) ?? '')) throw Error(diagnostics);
}} finally {{ host.close(); }}
",
            root = root.to_string_lossy(),
            provider_path = provider_path.to_string_lossy(),
            glue_path = glue_path.to_string_lossy(),
            wasm_path = wasm_path.to_string_lossy(),
            module_name = interface.module
        );
        // cost: one Node process per fixed build mode, never per accumulated entry.
        let mut command = std::process::Command::new("node");
        let ran = command
            .args(["--input-type=module", "-e", &script])
            .output()?;
        assert!(
            ran.status.success(),
            "{profile:?}/{level:?}/audit={checked}: {}",
            String::from_utf8_lossy(&ran.stderr)
        );
        assert_eq!(
            String::from_utf8(ran.stdout)?,
            "35 7 35\n",
            "{profile:?}/{level:?}"
        );
        assert!(
            ran.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&ran.stderr)
        );
    }
    Ok(())
}

#[test]
fn opaque_noncopy_host_argument_survives_pending() -> Result<(), Box<dyn std::error::Error>> {
    package_host_routes_with_provider(
        "host-borrowed-collection-noncopy-pending",
        include_str!("cold_opaque_host_collection.hd"),
        false,
        Some(
            r"
export default {
  parent(label, values) {
    if (!label || values.join(',') !== 'one') throw Error('parent arguments lost');
    return 35;
  },
  async child(label, values) {
    await new Promise(resolve => setTimeout(resolve, 0));
    if (label !== 'pending35' || values.join(',') !== 'one') {
      throw Error('pending borrowed collection changed');
    }
    return 7;
  },
};
",
        ),
    )
}

/// module.rt.drop.glue and fn.suspension.hold: a std-only Drop record
/// transfers its inline payload to the host leaf, which runs its hooks.
#[test]
fn std_drop_host_arguments_cleanup_pending_ready_and_cancelled()
-> Result<(), Box<dyn std::error::Error>> {
    let mut sources = hd_driver::std_sources();
    sources.insert(
        "coverage/mod.hd",
        include_str!("cold_drop_host_argument.hd"),
    );
    let store = crate::warm_store();
    let core = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../host/core.ts")
        .canonicalize()?;
    let wire = core.with_file_name("wire.ts");
    let scratch = hd_testkit::Scratch::new(env!("CARGO_TARGET_TMPDIR"), "host-drop-arguments");
    let wasm_path = scratch.join("main.wasm");
    for (profile, level, checked) in [
        (BuildProfile::Checked, OptLevel::Dev, false),
        (BuildProfile::Checked, OptLevel::Dev, true),
        (BuildProfile::Checked, OptLevel::Optimized, false),
        (BuildProfile::Checked, OptLevel::Optimized, true),
        (BuildProfile::Release, OptLevel::Optimized, false),
    ] {
        let out = hd_driver::build_packages(
            &Host {
                render_tir: &[],
                sources: &sources,
                store: &store,
                clock: &NoClock,
                executor: Executor::Serial(SerialOrder::Priority),
            },
            "std",
            &Packages {
                profile: Some(vec!["std.coverage.Child".into()]),
                build_profile: profile,
                level,
                checked,
                verify_own: true,
                instruction_origins: true,
                ..Packages::default()
            },
            &Goal::Program {
                entry: "coverage".into(),
            },
        );
        assert!(
            !out.diags.has_errors(),
            "{profile:?}/{level:?}/audit={checked}: {}",
            out.render()
        );
        let wasm = out.wasm.ok_or("missing Wasm")?;
        if level == OptLevel::Dev {
            let leaf = out
                .instruction_origins
                .iter()
                .find(|function| {
                    function.name.starts_with("rt:") && function.name.ends_with("/child")
                })
                .ok_or("missing cold child leaf")?;
            let mut function_index = 0;
            let mut copies_borrowed_collection = false;
            for payload in wasmparser::Parser::new(0).parse_all(&wasm) {
                match payload? {
                    wasmparser::Payload::ImportSection(imports) => {
                        for import in imports.into_imports() {
                            function_index +=
                                u32::from(matches!(import?.ty, wasmparser::TypeRef::Func(_)));
                        }
                    }
                    wasmparser::Payload::CodeSectionEntry(body) => {
                        if function_index == leaf.index {
                            for op in body.get_operators_reader()? {
                                if let wasmparser::Operator::Call { function_index } = op? {
                                    copies_borrowed_collection |=
                                        out.instruction_origins.iter().any(|called| {
                                            called.index == function_index
                                                && called.name.split('#').next()
                                                    == Some("rt:copy_elems")
                                        });
                                }
                            }
                        }
                        function_index += 1;
                    }
                    _ => {}
                }
            }
            assert!(
                !copies_borrowed_collection,
                "the cold host leaf takes its moved collection without copying"
            );
        }
        std::fs::write(&wasm_path, &wasm)?;
        let script = format!(
            r"
import {{ readFileSync }} from 'node:fs';
import {{ pathToFileURL }} from 'node:url';
const {{ createCoreHost }} = await import(pathToFileURL({core:?}));
const {{ Dec }} = await import(pathToFileURL({wire:?}));
let instance;
let calls = 0;
let finishes = 0;
let inspections = 0;
const values = new Map();
const host = createCoreHost({{out(){{}},err(text){{process.stderr.write(text)}}}}, {{
  providers: ({{put,Enc,pending,finish}}) => ({{
    'hd:Child': {{
      inspect: len => {{
        const value = new Dec(new Uint8Array(instance.exports['hd.x'].buffer, 0, len)).str();
        if (JSON.parse(value).join(',') !== 'one') throw Error('synchronous argument changed');
        inspections++;
        return put(new Enc().leb(7));
      }},
      'child.start': len => {{
        const value = new Dec(new Uint8Array(instance.exports['hd.x'].buffer, 0, len)).str();
        const valuesSeen = JSON.parse(value).join(',');
        if (valuesSeen !== 'one' && valuesSeen !== 'ready') throw Error('cold argument changed');
        calls++;
        if (valuesSeen === 'ready') return [0, put(new Enc().leb(7))];
        const state = pending({{at: performance.now() + 1, done: null}});
        values.set(state[1], value);
        return state;
      }},
      'child.finish': handle => {{
        finish(handle);
        const value = values.get(handle);
        values.delete(handle);
        if (JSON.parse(value).join(',') !== 'one') throw Error('pending argument changed');
        finishes++;
        return put(new Enc().leb(7));
      }},
    }},
  }}),
}});
try {{
  instance = new WebAssembly.Instance(new WebAssembly.Module(readFileSync({wasm_path:?})), host.imports);
  const result = await host.run(instance,'hd.init','hd.poll');
  if (result.trapped || result.status !== 0) throw Error(JSON.stringify(result));
  if (calls !== 4 || finishes !== 2 || inspections !== 1) throw Error('wrong host call totals');
}} finally {{ host.close(); }}
",
            core = core.to_string_lossy(),
            wire = wire.to_string_lossy(),
            wasm_path = wasm_path.to_string_lossy()
        );
        // cost: one Node process per five fixed build modes, never accumulated state.
        let ran = std::process::Command::new("node")
            .args(["--input-type=module", "-e", &script])
            .output()?;
        assert!(
            ran.status.success(),
            "{profile:?}/{level:?}/audit={checked}: {}",
            String::from_utf8_lossy(&ran.stderr)
        );
    }
    Ok(())
}
