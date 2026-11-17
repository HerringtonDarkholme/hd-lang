//! Native package provider preflight and linking (`host/package.rs`):
//! `cli.host.package.native-unregistered`, `native-cli`, `native-reachable`,
//! `native-mismatch`, `native-preflight`, `duplicate`. Every refusal case's
//! module has a trapping start function, so a guest start is visible as a
//! trap instead of the refusal.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use hd_cache::MemoryStore;
use hd_host_abi::schema::{Contract, Identity, Interface, Method, Value};
use hd_run::{Engine, Grants, HostSetup, Instance, Limits, Outcome, StartError};
use wasm_encoder::{
    CodeSection, EntityType, ExportKind, ExportSection, Function, FunctionSection, ImportSection,
    Instruction as I, MemorySection, MemoryType, Module as Wasm, StartSection, TypeSection,
    ValType,
};

use super::{EngineConfig, WasmtimeEngine};
use crate::native::{Bindings, HostError, HostResult};
use crate::{HostIo, Shared};

const SERVER: &str = "example.org/sdk@1.2.3/server.Server";

fn identity(package: &str, version: &str, trait_path: &str) -> Identity {
    Identity {
        package: package.into(),
        version: version.into(),
        trait_path: trait_path.into(),
    }
}

fn method(name: &str, params: Vec<Value>, result: Value, suspends: bool) -> Method {
    Method {
        name: name.into(),
        member: name.into(),
        params,
        result,
        suspends,
    }
}

fn root(identity: Identity, module: &str, methods: Vec<Method>) -> Interface {
    Interface {
        selectors: vec![format!("dep.sdk.{}", identity.trait_path)],
        identity,
        module: module.into(),
        handle: false,
        methods,
    }
}

fn int() -> Value {
    Value::Int {
        bits: 64,
        signed: true,
    }
}

/// `example.org/sdk@1.2.3/server.Server` with `read() -> Int` and
/// `echo(Int)`.
fn server(identity: Identity) -> Contract {
    Contract {
        interfaces: vec![root(
            identity,
            "hd:pkg:server",
            vec![
                method("read", vec![], int(), false),
                method("echo", vec![int()], Value::Void, false),
            ],
        )],
    }
}

fn sdk() -> Identity {
    identity("example.org/sdk", "1.2.3", "server.Server")
}

/// A module embedding `contract`, importing each `(module, name, params,
/// results)` as a function of that many `i32`s, whose `hd.init` runs
/// `init`; with `trap`, a start function that traps.
fn module(
    contract: &Contract,
    imports: &[(&str, &str, usize, usize)],
    init: &[I<'_>],
    trap: bool,
) -> Vec<u8> {
    let mut types = TypeSection::new();
    let mut section = ImportSection::new();
    for (n, (module, name, params, results)) in imports.iter().enumerate() {
        types
            .ty()
            .function(vec![ValType::I32; *params], vec![ValType::I32; *results]);
        section.import(
            module,
            name,
            EntityType::Function(u32::try_from(n).unwrap()),
        );
    }
    let base = u32::try_from(imports.len()).unwrap();
    types.ty().function([], []);
    types.ty().function([ValType::I32], [ValType::I32]);
    let mut functions = FunctionSection::new();
    functions.function(base).function(base + 1).function(base);
    let mut memory = MemorySection::new();
    memory.memory(MemoryType {
        minimum: 1,
        maximum: None,
        memory64: false,
        shared: false,
        page_size_log2: None,
    });
    let mut exports = ExportSection::new();
    exports.export("hd.x", ExportKind::Memory, 0);
    exports.export("hd.init", ExportKind::Func, base);
    exports.export("hd.poll", ExportKind::Func, base + 1);
    let mut body = Function::new([]);
    for instruction in init {
        body.instruction(instruction);
    }
    body.instruction(&I::End);
    let mut poll = Function::new([]);
    poll.instruction(&I::I32Const(0)).instruction(&I::End);
    let mut start = Function::new([]);
    start.instruction(&I::Unreachable).instruction(&I::End);
    let mut code = CodeSection::new();
    code.function(&body).function(&poll).function(&start);
    let mut wasm = Wasm::new();
    wasm.section(&types)
        .section(&section)
        .section(&functions)
        .section(&memory)
        .section(&exports);
    if trap {
        wasm.section(&StartSection {
            function_index: base + 2,
        });
    }
    wasm.section(&code);
    wasm.section(&wasm_encoder::CustomSection {
        name: hd_host_abi::schema::SECTION.into(),
        data: contract.encode().into(),
    });
    wasm.finish()
}

/// The server module whose start traps: a refusal must come first.
fn guarded(contract: &Contract) -> Vec<u8> {
    module(
        contract,
        &[
            ("hd:pkg:server", "read", 0, 1),
            ("hd:pkg:server", "echo", 1, 0),
        ],
        &[],
        true,
    )
}

fn setup() -> HostSetup {
    HostSetup {
        providers: vec![],
        grants: Grants::default(),
        limits: Limits::default(),
        args: vec![],
        program: "native".into(),
    }
}

fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

fn contract_bytes(contract: &Contract) -> &'static [u8] {
    Box::leak(contract.encode().into_boxed_slice())
}

/// Each call's method index and argument bytes.
type Calls = Rc<RefCell<Vec<(usize, Vec<u8>)>>>;

/// A provider recording its calls.
#[derive(Default)]
struct Recorder {
    calls: Calls,
    reply: Vec<u8>,
    failure: Option<&'static str>,
}

fn dispatch(provider: &mut dyn Any, method: usize, args: &[u8]) -> HostResult<Vec<u8>> {
    let recorder = provider
        .downcast_mut::<Recorder>()
        .expect("the registered recorder");
    recorder.calls.borrow_mut().push((method, args.to_vec()));
    if let Some(failure) = recorder.failure {
        return Err(HostError(failure.into()));
    }
    Ok(if method == 0 {
        recorder.reply.clone()
    } else {
        Vec::new()
    })
}

/// Bindings generated from `companion`, registering `identities` in order.
fn bindings(companion: &Contract, identities: &[&str], recorder: Recorder) -> Bindings {
    let mut bindings = Bindings::new(contract_bytes(companion));
    let mut recorder = Some(recorder);
    for identity in identities {
        bindings.register_dispatch(
            leak((*identity).to_owned()),
            "hd:pkg:server",
            Box::new(recorder.take().unwrap_or_default()),
            dispatch,
        );
    }
    bindings
}

fn refusal(wasm: &[u8], bindings: Bindings) -> String {
    let engine = WasmtimeEngine::new(EngineConfig::default()).unwrap();
    let module = engine.load(wasm, &MemoryStore::default()).unwrap();
    match engine.instantiate_with_bindings(&module, &setup(), HostIo::process(), bindings) {
        Err(StartError::Refused(message)) => message,
        Err(StartError::Engine(message)) => panic!("not a refusal: {message}"),
        Ok(_) => panic!("instantiated"),
    }
}

#[test]
fn without_a_registry_a_required_trait_is_refused_before_the_guest_starts() {
    let engine = WasmtimeEngine::new(EngineConfig::default()).unwrap();
    let module = engine
        .load(&guarded(&server(sdk())), &MemoryStore::default())
        .unwrap();
    let Err(StartError::Refused(message)) = engine.instantiate(&module, &setup()) else {
        panic!("the hd command registers no provider")
    };
    assert_eq!(
        message,
        format!("no native provider registered for package host trait {SERVER:?}")
    );
}

#[test]
fn a_registered_provider_is_called_and_its_result_reaches_the_guest() {
    // `read`'s result length goes straight to `echo`, whose argument is
    // the exchange buffer's bytes: the provider sees its own reply back.
    // The registry holds exactly the contract's root traits: a trait the
    // manifest consents to but the module never calls is not in the
    // contract and needs no provider (`native-reachable`).
    let contract = server(sdk());
    let wasm = module(
        &contract,
        &[
            ("hd:pkg:server", "read", 0, 1),
            ("hd:pkg:server", "echo", 1, 0),
        ],
        &[I::Call(0), I::Call(1)],
        false,
    );
    let calls = Rc::default();
    let recorder = Recorder {
        calls: Rc::clone(&calls),
        reply: vec![0x2a, 7, 9],
        failure: None,
    };
    let engine = WasmtimeEngine::new(EngineConfig::default()).unwrap();
    let module = engine.load(&wasm, &MemoryStore::default()).unwrap();
    let mut instance = engine
        .instantiate_with_bindings(
            &module,
            &setup(),
            HostIo::process(),
            bindings(&contract, &[SERVER], recorder),
        )
        .unwrap();
    assert_eq!(instance.init(), Outcome::Exit(0));
    assert_eq!(*calls.borrow(), [(0, vec![]), (1, vec![0x2a, 7, 9])]);
}

#[test]
fn a_registry_without_the_required_trait_is_refused() {
    let contract = server(sdk());
    assert_eq!(
        refusal(
            &guarded(&contract),
            Bindings::new(contract_bytes(&contract))
        ),
        format!("no native provider registered for package host trait {SERVER:?}")
    );
}

#[test]
fn a_provider_for_another_version_or_local_hash_is_refused_naming_the_trait() {
    let contract = server(sdk());
    let other = "example.org/sdk@1.2.4/server.Server";
    assert_eq!(
        refusal(
            &guarded(&contract),
            bindings(&server(sdk()), &[other], Recorder::default())
        ),
        format!("native provider for package host trait {SERVER:?} is registered as {other:?}")
    );
    // A local package's identity is its directory hash, with no version.
    let local = server(identity("local:1f2e", "", "server.Server"));
    assert_eq!(
        refusal(
            &guarded(&local),
            bindings(&local, &["local:9a8b/server.Server"], Recorder::default())
        ),
        "native provider for package host trait \"local:1f2e/server.Server\" is registered as \"local:9a8b/server.Server\""
    );
    // A hand-written registration may name the trait path alone.
    assert_eq!(
        refusal(
            &guarded(&contract),
            bindings(&contract, &["server.Server"], Recorder::default())
        ),
        format!(
            "native provider for package host trait {SERVER:?} is registered as \"server.Server\""
        )
    );
}

#[test]
fn two_registrations_of_one_identity_are_refused() {
    let contract = server(sdk());
    assert_eq!(
        refusal(
            &guarded(&contract),
            bindings(&contract, &[SERVER, SERVER], Recorder::default())
        ),
        format!("duplicate native provider registrations for package host trait {SERVER:?}")
    );
}

#[test]
fn a_registration_the_contract_does_not_require_is_refused() {
    let contract = server(sdk());
    let extra = "example.org/sdk@1.2.3/client.Client";
    assert_eq!(
        refusal(
            &guarded(&contract),
            bindings(&contract, &[SERVER, extra], Recorder::default())
        ),
        format!(
            "native provider for package host trait {extra:?} names no trait this module requires"
        )
    );
}

#[test]
fn a_companion_of_other_method_schemas_is_refused() {
    let contract = server(sdk());
    let mut stale = contract.clone();
    stale.interfaces[0].methods[0].result = Value::String;
    assert_eq!(
        refusal(
            &guarded(&contract),
            bindings(&stale, &[SERVER], Recorder::default())
        ),
        format!(
            "native provider for package host trait {SERVER:?} was generated from a different contract"
        )
    );
    let mut invalid = Bindings::new(&[0, 0, 0, 0]);
    invalid.register_dispatch(
        SERVER,
        "hd:pkg:server",
        Box::new(Recorder::default()),
        dispatch,
    );
    assert_eq!(
        refusal(&guarded(&contract), invalid),
        "invalid native provider contract: unsupported package host ABI version"
    );
}

#[test]
fn suspending_methods_and_returned_handles_are_refused_before_the_guest_starts() {
    let mut waiting = server(sdk());
    waiting.interfaces[0]
        .methods
        .push(method("wait", vec![], Value::Void, true));
    let wasm = module(
        &waiting,
        &[
            ("hd:pkg:server", "wait.start", 0, 2),
            ("hd:pkg:server", "wait.finish", 1, 1),
        ],
        &[],
        true,
    );
    assert_eq!(
        refusal(&wasm, bindings(&waiting, &[SERVER], Recorder::default())),
        format!(
            "native provider method {SERVER:?}.wait suspends, and wasmtime does not drive pending native calls yet"
        )
    );

    let mut handles = server(sdk());
    handles.interfaces[0].methods[0].result = Value::Handle("hd:pkg:conn".into());
    handles.interfaces.push(Interface {
        identity: identity("example.org/sdk", "1.2.3", "server.Conn"),
        module: "hd:pkg:conn".into(),
        selectors: vec![],
        handle: true,
        methods: vec![],
    });
    let wasm = module(
        &handles,
        &[
            ("hd:pkg:server", "read", 0, 1),
            ("hd:pkg:conn", "$release", 1, 0),
        ],
        &[],
        true,
    );
    assert_eq!(
        refusal(&wasm, bindings(&handles, &[SERVER], Recorder::default())),
        "package host trait \"example.org/sdk@1.2.3/server.Conn\" crosses as a native handle, which wasmtime does not hold yet"
    );
}

#[test]
fn an_import_of_another_signature_is_refused_before_the_guest_starts() {
    let contract = server(sdk());
    let wasm = module(&contract, &[("hd:pkg:server", "read", 0, 0)], &[], true);
    assert_eq!(
        refusal(&wasm, bindings(&contract, &[SERVER], Recorder::default())),
        format!(
            "the import `hd:pkg:server` `read` of package host method {SERVER}.read has an unexpected signature"
        )
    );
}

/// Runs `init` against a recorder that fails with `failure`, and returns
/// the outcome and standard error.
fn run(
    contract: &Contract,
    imports: &[(&str, &str, usize, usize)],
    init: &[I<'_>],
    failure: Option<&'static str>,
) -> (Outcome, String) {
    let engine = WasmtimeEngine::new(EngineConfig::default()).unwrap();
    let module = engine
        .load(
            &module(contract, imports, init, false),
            &MemoryStore::default(),
        )
        .unwrap();
    let err = Shared::default();
    let io = HostIo {
        out: Box::new(Shared::default()),
        err: Box::new(err.clone()),
        input: None,
    };
    let recorder = Recorder {
        failure,
        ..Recorder::default()
    };
    let mut instance = engine
        .instantiate_with_bindings(
            &module,
            &setup(),
            io,
            bindings(contract, &[SERVER], recorder),
        )
        .unwrap();
    let outcome = instance.init();
    (outcome, String::from_utf8(err.take()).unwrap())
}

#[test]
fn a_provider_failure_is_a_host_contract_panic_naming_the_method() {
    let (outcome, error) = run(
        &server(sdk()),
        &[("hd:pkg:server", "read", 0, 1)],
        &[I::Call(0), I::Drop],
        Some("sensor offline"),
    );
    assert_eq!(outcome, Outcome::Exit(3));
    assert!(
        error.starts_with(&format!(
            "panic: host-contract: {SERVER}.read: sensor offline"
        )),
        "{error}"
    );
}

#[test]
fn a_provider_returning_from_a_never_method_is_a_host_contract_panic() {
    let mut contract = server(sdk());
    contract.interfaces[0].methods[0].result = Value::Never;
    let (outcome, error) = run(
        &contract,
        &[("hd:pkg:server", "read", 0, 0)],
        &[I::Call(0)],
        None,
    );
    assert_eq!(outcome, Outcome::Exit(3));
    assert!(
        error.starts_with(&format!(
            "panic: host-contract: {SERVER}.read: package host: the method never returns, but its provider returned"
        )),
        "{error}"
    );
}

#[test]
fn a_handle_method_is_never_linked_to_a_root_provider() {
    // A contract of a handle interface alone has no root to register, so
    // its method stays an import no host provides.
    let contract = Contract {
        interfaces: vec![Interface {
            identity: identity("example.org/sdk", "1.2.3", "server.Conn"),
            module: "hd:pkg:conn".into(),
            selectors: vec![],
            handle: true,
            methods: vec![method("peek", vec![], Value::Void, false)],
        }],
    };
    let wasm = module(&contract, &[("hd:pkg:conn", "peek", 0, 0)], &[], true);
    let engine = WasmtimeEngine::new(EngineConfig::default()).unwrap();
    let module = engine.load(&wasm, &MemoryStore::default()).unwrap();
    let Err(StartError::Engine(message)) = engine.instantiate(&module, &setup()) else {
        panic!("an unprovided handle method must not start")
    };
    assert!(message.contains("hd:pkg:conn"), "{message}");
}
