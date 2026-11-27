use hd_run::{Grants, HostSetup, Limits, Outcome};
use hd_run_wasmtime::{EngineConfig, HostIo};

fn spinning_start() -> Vec<u8> {
    use wasm_encoder::{
        BlockType, CodeSection, ExportKind, ExportSection, Function, FunctionSection, Instruction,
        Module, NameMap, NameSection, StartSection, TypeSection, ValType,
    };
    let mut types = TypeSection::new();
    types.ty().function([], []);
    types.ty().function([], [ValType::I32]);
    let mut functions = FunctionSection::new();
    functions.function(0).function(0).function(1);
    let mut exports = ExportSection::new();
    exports
        .export("hd.init", ExportKind::Func, 1)
        .export("hd.poll", ExportKind::Func, 2);
    let mut code = CodeSection::new();
    let mut hot = Function::new([]);
    hot.instruction(&Instruction::Loop(BlockType::Empty))
        .instruction(&Instruction::Br(0))
        .instruction(&Instruction::End)
        .instruction(&Instruction::End);
    code.function(&hot);
    let mut init = Function::new([]);
    init.instruction(&Instruction::End);
    code.function(&init);
    let mut poll = Function::new([]);
    poll.instruction(&Instruction::I32Const(1))
        .instruction(&Instruction::End);
    code.function(&poll);
    let mut names = NameSection::new();
    let mut map = NameMap::new();
    map.append(0, "hot_named_start");
    map.append(1, "init");
    map.append(2, "poll");
    names.functions(&map);
    let mut module = Module::new();
    module
        .section(&types)
        .section(&functions)
        .section(&exports)
        .section(&StartSection { function_index: 0 })
        .section(&code)
        .section(&names);
    module.finish()
}

fn setup() -> HostSetup {
    HostSetup {
        grants: Grants::default(),
        limits: Limits {
            wall_ms: Some(40),
            ..Limits::default()
        },
        args: Vec::new(),
        program: "profile-test".into(),
    }
}

fn io() -> HostIo {
    HostIo {
        out: Box::new(std::io::sink()),
        err: Box::new(std::io::sink()),
        input: None,
    }
}

#[test]
#[allow(unsafe_code)]
fn sampling_covers_start_functions_and_keeps_deadlines_for_wasm_and_native()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR"))?;
    let wasm = spinning_start();
    for native in [false, true] {
        let output = dir.path().join(if native {
            "native.cwasm"
        } else {
            "profile.json"
        });
        let (outcome, _) = if native {
            let code = hd_run_wasmtime::precompile(&wasm, 1)?;
            let bytes = hd_run::artifact::encode(code, &wasm, hd_base::Hash128(0))?;
            std::fs::write(&output, bytes)?;
            let image = hd_run::artifact::Native::open(&output)?;
            // SAFETY: this test just produced the image. Output is published
            // by replacement, so profiling never changes the mapped bytes.
            unsafe { hd_run_wasmtime::run_native_sampled(&image, &setup(), io(), &output, None) }?
        } else {
            hd_run_wasmtime::run_sampled(
                EngineConfig::default(),
                &wasm,
                &setup(),
                io(),
                &output,
                None,
            )?
        };
        assert!(matches!(outcome, Outcome::Exit(3)), "deadline: {outcome:?}");
        let text = std::fs::read_to_string(&output)?;
        let profile: serde_json::Value = serde_json::from_str(&text)?;
        assert!(
            text.contains("hot_named_start"),
            "function symbols retained"
        );
        assert!(
            profile["threads"][0]["samples"]["length"]
                .as_u64()
                .ok_or("sample count")?
                > 0
        );
    }
    Ok(())
}

#[test]
fn a_failed_profile_start_preserves_the_previous_output() -> Result<(), Box<dyn std::error::Error>>
{
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR"))?;
    let output = dir.path().join("profile.json");
    std::fs::write(&output, "previous profile")?;
    let result = hd_run_wasmtime::run_sampled(
        EngineConfig::default(),
        b"\0asm\x01\0\0\0",
        &setup(),
        io(),
        &output,
        None,
    );
    assert!(result.is_err(), "missing hd entry points");
    assert_eq!(std::fs::read_to_string(&output)?, "previous profile");
    assert_eq!(
        std::fs::read_dir(dir.path())?.count(),
        1,
        "no abandoned temporary file"
    );
    Ok(())
}
