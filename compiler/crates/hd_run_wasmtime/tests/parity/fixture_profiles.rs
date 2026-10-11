//! Fixture providers are opt-in, isolated per instance, and use the shared ABI.

use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use crate::support::{MEM, Program};
use hd_cache::MemoryStore;
use hd_run::{Engine, Grants, HostSetup, Instance, Limits, Outcome};
use hd_run_wasmtime::{
    EngineConfig, HostIo, RuntimeProfile, WasmtimeEngine, run_captured, run_captured_with_profile,
};
use wasm_encoder::{BlockType, Instruction as I, ValType};

fn setup() -> HostSetup {
    HostSetup {
        providers: vec![],
        grants: Grants::default(),
        limits: Limits::default(),
        args: vec![],
        program: String::new(),
    }
}

fn assert_profile(program: Program, profile: RuntimeProfile) {
    let (wasm, expected) = program.finish(7);
    let captured = run_captured_with_profile(EngineConfig::default(), profile, &wasm, &setup())
        .expect("fixture profile runs");
    assert_eq!(captured.status, 7);
    assert_eq!(captured.stdout, b"core-parity\n");
    assert_eq!(captured.stderr, expected.as_bytes());
}

#[test]
fn profile_names_are_explicit_and_unknown_profiles_are_errors() {
    for (name, expected) in [
        ("default", RuntimeProfile::Default),
        ("misbehaving-host", RuntimeProfile::MisbehavingHost),
        ("special-float-host", RuntimeProfile::SpecialFloatHost),
        ("serde-vault", RuntimeProfile::SerdeVault),
        ("pending-write", RuntimeProfile::PendingWrite),
    ] {
        assert_eq!(RuntimeProfile::from_name(name), Ok(expected));
    }
    for name in ["", "disposed-file", "pending-gate", "other"] {
        assert_eq!(
            RuntimeProfile::from_name(name),
            Err(format!("no runtime profile named {name}"))
        );
    }
}

#[test]
fn default_and_unrelated_profiles_reject_fixture_imports_at_load() {
    let engines = [
        RuntimeProfile::Default,
        RuntimeProfile::MisbehavingHost,
        RuntimeProfile::SpecialFloatHost,
        RuntimeProfile::SerdeVault,
        RuntimeProfile::PendingWrite,
    ]
    .map(|profile| {
        (
            profile,
            WasmtimeEngine::new_with_profile(EngineConfig::default(), profile).unwrap(),
        )
    });
    for (module, name, required) in [
        ("hd:Gauge", "level", RuntimeProfile::MisbehavingHost),
        ("hd:Sensor", "reading", RuntimeProfile::SpecialFloatHost),
        ("hd:Vault", "keep", RuntimeProfile::SerdeVault),
    ] {
        let mut program = Program::new();
        program.import(module, name, &[ValType::I32], &[ValType::I32]);
        let (wasm, _) = program.finish(7);
        for (profile, engine) in &engines {
            let result = engine.load(&wasm, &MemoryStore::default());
            if *profile == required {
                assert!(result.is_ok(), "{profile:?} accepts {module} {name}");
            } else {
                let Err(hd_run::LoadError::Engine(message)) = result else {
                    panic!("{profile:?} unexpectedly accepts {module} {name}");
                };
                assert!(message.contains(module) && message.contains(name));
            }
        }
        assert!(run_captured(EngineConfig::default(), &wasm, &setup()).is_err());
        let mut wrong_method = Program::new();
        wrong_method.import(module, "not-a-profile-method", &[], &[ValType::I32]);
        let (wasm, _) = wrong_method.finish(7);
        let engine = engines
            .iter()
            .find(|(profile, _)| *profile == required)
            .unwrap();
        assert!(engine.1.load(&wasm, &MemoryStore::default()).is_err());
    }
}

#[test]
fn misbehaving_gauge_returns_300_instead_of_silently_narrowing_to_u8() {
    let mut program = Program::new();
    for _ in 0..3 {
        program.abi_call("hd:Gauge", "level", &[], None, false, vec![0xac, 0x02]);
    }
    assert_profile(program, RuntimeProfile::MisbehavingHost);
}

#[test]
fn profiled_guest_contract_panic_is_captured_once_with_status_three() {
    let mut program = Program::new();
    let gauge = program.import("hd:Gauge", "level", &[], &[ValType::I32]);
    // A raw guest boundary check, followed by the same stderr/unreachable
    // reporting sequence emitted by the compiler's panic stubs.
    program.instructions(&[
        I::Call(gauge),
        I::Drop,
        I::I32Const(0),
        I::I32Load8U(MEM),
        I::I32Const(127),
        I::I32And,
        I::I32Const(1),
        I::I32Load8U(MEM),
        I::I32Const(7),
        I::I32Shl,
        I::I32Or,
        I::I32Const(255),
        I::I32GtU,
        I::If(BlockType::Empty),
    ]);
    let panic = b"panic: host-contract: the host result is outside u8\n";
    program.load_args(panic);
    program.instructions(&[
        I::I32Const(i32::try_from(panic.len()).unwrap()),
        I::Call(0), // hd:rt.stderr
        I::Unreachable,
        I::End,
    ]);
    let (wasm, _) = program.finish(7);
    let captured = run_captured_with_profile(
        EngineConfig::default(),
        RuntimeProfile::MisbehavingHost,
        &wasm,
        &setup(),
    )
    .expect("a reported guest panic is a captured run, not a startup error");
    assert_eq!(captured.status, 3);
    assert_eq!(captured.stdout, b"core-parity\n");
    assert_eq!(captured.stderr, panic);
}

fn sensor_program() -> (Vec<u8>, String) {
    let mut program = Program::new();
    for bits in [
        0x7ff8_0000_0000_0000_u64,
        0x7ff0_0000_0000_0000,
        1 << 63,
        1 << 63,
        1 << 63,
    ] {
        program.abi_call(
            "hd:Sensor",
            "reading",
            &[],
            None,
            false,
            bits.to_le_bytes().to_vec(),
        );
    }
    program.finish(7)
}

#[test]
fn sensor_returns_ieee_nan_infinity_and_repeated_negative_zero() {
    let (wasm, expected) = sensor_program();
    let captured = run_captured_with_profile(
        EngineConfig::default(),
        RuntimeProfile::SpecialFloatHost,
        &wasm,
        &setup(),
    )
    .unwrap();
    assert_eq!(captured.status, 7);
    assert_eq!(captured.stderr, expected.as_bytes());
}

#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<u8>>>);

impl Write for Sink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn sensor_sequence_resets_per_instance_on_one_shared_engine() {
    let engine =
        WasmtimeEngine::new_with_profile(EngineConfig::default(), RuntimeProfile::SpecialFloatHost)
            .unwrap();
    let (wasm, expected) = sensor_program();
    let module = engine.load(&wasm, &MemoryStore::default()).unwrap();
    // Keep the first ended instance alive while the next instance calls its
    // Sensor, proving state is not shared by either engine or provider module.
    let mut instances = Vec::new();
    for _ in 0..2 {
        let errors = Sink::default();
        let io = HostIo {
            out: Box::new(io::sink()),
            err: Box::new(errors.clone()),
            input: None,
        };
        let mut instance = engine.instantiate_with(&module, &setup(), io).unwrap();
        assert_eq!(instance.init(), Outcome::Exit(0));
        assert_eq!(hd_run::drive(&mut instance, 5), Outcome::Exit(7));
        assert_eq!(*errors.0.lock().unwrap(), expected.as_bytes());
        instances.push(instance);
    }
}

#[test]
fn vault_echoes_empty_and_binary_boundary_values_without_decoding_them() {
    let mut program = Program::new();
    for bytes in [&b""[..], &[0, 255, 128, 0, 0xc3, 0xa9][..]] {
        program.abi_call("hd:Vault", "keep", &[], Some(bytes), false, bytes.to_vec());
    }
    assert_profile(program, RuntimeProfile::SerdeVault);
}

#[test]
fn vault_returns_malformed_lengths_unchanged_and_does_not_touch_memory() {
    let mut program = Program::new();
    let keep = program.import("hd:Vault", "keep", &[ValType::I32], &[ValType::I32]);
    program.load_args(&[0, 255, 128]);
    for length in [-1, 0, 3, (4 << 20) + 1, i32::MAX] {
        program.instructions(&[
            I::I32Const(length),
            I::Call(keep),
            I::I32Const(length),
            I::I32Ne,
            I::If(BlockType::Empty),
            I::Unreachable,
            I::End,
        ]);
        for (offset, byte) in [(0, 0), (1, 255), (2, 128)] {
            program.instructions(&[
                I::I32Const(offset),
                I::I32Load8U(MEM),
                I::I32Const(byte),
                I::I32Ne,
                I::If(BlockType::Empty),
                I::Unreachable,
                I::End,
            ]);
        }
    }
    assert_profile(program, RuntimeProfile::SerdeVault);
}

#[test]
fn pending_write_uses_the_standard_pending_console_provider() {
    let mut program = Program::new();
    let start = program.import(
        "hd:Console",
        "write_line.start",
        &[ValType::I32],
        &[ValType::I32, ValType::I32],
    );
    let abort = program.import("hd:rt", "abort", &[ValType::I32], &[]);
    program.load_args(b"must-not-be-written");
    program.instructions(&[
        I::I32Const(19),
        I::Call(start),
        I::LocalSet(1),
        I::I32Const(1),
        I::I32Ne,
        I::If(BlockType::Empty),
        I::Unreachable,
        I::End,
        I::LocalGet(1),
        I::Call(abort),
    ]);
    assert_profile(program, RuntimeProfile::PendingWrite);
}
