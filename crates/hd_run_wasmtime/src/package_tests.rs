//! Native package provider preflight and linking (`host/package.rs`):
//! `cli.host.package.native-unregistered`, `native-cli`, `native-reachable`,
//! `native-mismatch`, `native-preflight`, `native-two-keys`,
//! `manifest-consent`, `no-flag-consent`, `duplicate`. Every refusal case's
//! module has a trapping start function, so a guest start is visible as a
//! trap instead of the refusal.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use hd_cache::MemoryStore;
use hd_host_abi::schema::{Contract, Identity, Interface, Method, Value};
use hd_run::{
    Consent, Engine, GrantValue, Grants, HostSetup, Instance, Limits, Outcome, StartError,
};
use wasm_encoder::{
    CodeSection, EntityType, ExportKind, ExportSection, Function, FunctionSection, ImportSection,
    Instruction as I, MemorySection, MemoryType, Module as Wasm, StartSection, TypeSection,
    ValType,
};

use super::{EngineConfig, WasmtimeEngine};
use crate::native::{
    Answer, Bindings, Completion, Dispatch, Dispatched, Failure, Handles, HostError, HostResult,
    NativeCall, pending, wire,
};
use crate::{HostIo, Shared};

const SERVER: &str = "example.org/sdk@1.2.3/server.Server";
/// The root source path through which the app manifest consents to it.
const SELECTOR: &str = "dep.sdk.server.Server";

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
    let mut body = Function::new([(2, ValType::I32)]);
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

/// A run whose app manifest's table and flags are `table` and `flags`.
fn setup_with(table: &[(&str, GrantValue)], flags: &[(&str, GrantValue)]) -> HostSetup {
    let values = |list: &[(&str, GrantValue)]| -> Vec<(String, GrantValue)> {
        list.iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect()
    };
    HostSetup {
        providers: vec![],
        grants: Grants::with_consent(Consent::new(&values(table), &values(flags))),
        limits: Limits::default(),
        args: vec![],
        program: "native".into(),
    }
}

/// A run whose app manifest consents to the server trait.
fn setup() -> HostSetup {
    setup_with(&[(SELECTOR, GrantValue::All)], &[])
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

fn dispatch(provider: &mut dyn Any, method: usize, args: &[u8], _: &mut Handles) -> Dispatched {
    let recorder = provider
        .downcast_mut::<Recorder>()
        .expect("the registered recorder");
    recorder.calls.borrow_mut().push((method, args.to_vec()));
    if let Some(failure) = recorder.failure {
        return Err(Failure::Threw(HostError(failure.into())));
    }
    Ok(Answer::Ready(if method == 0 {
        recorder.reply.clone()
    } else {
        Vec::new()
    }))
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
    refusal_in(wasm, &setup(), bindings)
}

fn refusal_in(wasm: &[u8], setup: &HostSetup, bindings: Bindings) -> String {
    let engine = WasmtimeEngine::new(EngineConfig::default()).unwrap();
    let module = engine.load(wasm, &MemoryStore::default()).unwrap();
    match engine.instantiate_with_bindings(&module, setup, HostIo::process(), bindings) {
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
    // Both keys (`native-two-keys`): the manifest consents and the
    // embedder registers. `read`'s result length goes straight to `echo`,
    // whose argument is the exchange buffer's bytes: the provider sees its
    // own reply back. The registry holds exactly the contract's root
    // traits: `client.Client`, which the manifest consents to but the
    // module never calls, is not in the contract and needs no provider
    // (`native-reachable`).
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
            &setup_with(
                &[
                    (SELECTOR, GrantValue::All),
                    ("dep.sdk.client.Client", GrantValue::All),
                ],
                &[],
            ),
            HostIo::process(),
            bindings(&contract, &[SERVER], recorder),
        )
        .unwrap();
    assert_eq!(instance.init(), Outcome::Exit(0));
    assert_eq!(*calls.borrow(), [(0, vec![]), (1, vec![0x2a, 7, 9])]);
}

#[test]
fn a_registered_provider_without_manifest_consent_is_refused_and_never_called() {
    // The module's start traps and its `hd.init` calls `read`: a refusal
    // must come before both, so the recorder sees no call.
    let contract = server(sdk());
    let not_granted = format!(
        "package capability {SERVER} is not granted; add `\"{SELECTOR}\" = true` to the app's `[capabilities]` table"
    );
    let denied = format!("package capability denied: {SERVER}, by a `false` grant of `{SELECTOR}`");
    for (table, flags, want) in [
        // No entry at all.
        (vec![], vec![], &not_granted),
        // Another trait's consent grants nothing to this one.
        (
            vec![("dep.sdk.server.Other", GrantValue::All)],
            vec![],
            &not_granted,
        ),
        // A `--cap` flag cannot replace the manifest (`no-flag-consent`).
        (vec![], vec![(SELECTOR, GrantValue::All)], &not_granted),
        // An explicit `false`.
        (vec![(SELECTOR, GrantValue::Deny)], vec![], &denied),
        // A flag's `false` wins over the table's `true` (`cli.cap.order.deny`).
        (
            vec![(SELECTOR, GrantValue::All)],
            vec![(SELECTOR, GrantValue::Deny)],
            &denied,
        ),
    ] {
        let calls: Calls = Rc::default();
        let recorder = Recorder {
            calls: Rc::clone(&calls),
            ..Recorder::default()
        };
        let wasm = module(
            &contract,
            &[("hd:pkg:server", "read", 0, 1)],
            &[I::Call(0), I::Drop],
            true,
        );
        assert_eq!(
            refusal_in(
                &wasm,
                &setup_with(&table, &flags),
                bindings(&contract, &[SERVER], recorder)
            ),
            *want,
            "table {table:?}, flags {flags:?}"
        );
        assert!(calls.borrow().is_empty());
    }
}

#[test]
fn a_root_trait_with_no_source_path_has_no_consent_to_find() {
    let mut contract = server(sdk());
    contract.interfaces[0].selectors.clear();
    assert_eq!(
        refusal(
            &guarded(&contract),
            bindings(&contract, &[SERVER], Recorder::default())
        ),
        format!(
            "package capability {SERVER} has no source path the app's `[capabilities]` table can grant"
        )
    );
}

#[test]
fn manifest_consent_without_a_registration_is_refused_as_unregistered() {
    // Consent selects nothing native: with it and no provider, the run is
    // refused by `native-unregistered`, with or without a registry.
    let contract = server(sdk());
    let unregistered = format!("no native provider registered for package host trait {SERVER:?}");
    assert_eq!(
        refusal(
            &guarded(&contract),
            Bindings::new(contract_bytes(&contract))
        ),
        unregistered
    );
    let engine = WasmtimeEngine::new(EngineConfig::default()).unwrap();
    let module = engine
        .load(&guarded(&contract), &MemoryStore::default())
        .unwrap();
    let Err(StartError::Refused(message)) = engine.instantiate(&module, &setup()) else {
        panic!("consent registers no provider")
    };
    assert_eq!(message, unregistered);
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
            "panic: host-contract: {SERVER}.read: provider threw sensor offline"
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

/// A value a completion carries: its `Arc` shows when it is dropped.
struct Held {
    _value: Arc<()>,
    n: i64,
}

fn held(value: Arc<()>, n: i64) -> Held {
    Held { _value: value, n }
}

/// What a `Waiter`'s `wait` does.
#[derive(Clone, Copy)]
enum Mode {
    /// Returns a pending call and sends its completion out.
    Pending,
    /// Answers at once.
    Ready,
    /// Completes its call before returning it.
    CompletedFirst,
    /// Aborts its call before returning it.
    AbortedFirst,
}

/// A provider of `read`, `echo` and the suspending `wait`. It holds an
/// `Rc`, so it is not `Send` and stays on its instance's thread
/// (`cli.host.package.native-thread`); only each `wait`'s completion
/// crosses to the test's thread.
struct Waiter {
    mode: Mode,
    /// Each pending `wait`'s hook counts here and holds a clone.
    aborts: Rc<std::cell::Cell<usize>>,
    completions: mpsc::Sender<Completion<Held>>,
    calls: Calls,
    /// `read` returns a pending call though it does not suspend.
    pending_read: bool,
}

impl Waiter {
    fn call(&self) -> (NativeCall<Held>, Completion<Held>) {
        let aborts = Rc::clone(&self.aborts);
        let (mut call, completion) = pending(move || aborts.set(aborts.get() + 1));
        match self.mode {
            Mode::CompletedFirst => {
                completion.complete(Ok(held(Arc::default(), 11)));
                let (_, unused) = pending(|| {});
                return (NativeCall::Pending(call), unused);
            }
            Mode::AbortedFirst => call.abort(),
            Mode::Pending | Mode::Ready => {}
        }
        (NativeCall::Pending(call), completion)
    }
}

/// The codec `wire::answer` takes, which may fail.
#[allow(clippy::unnecessary_wraps)]
#[allow(clippy::needless_pass_by_value)]
fn encode_held(value: Held, handles: &mut Handles) -> HostResult<Vec<u8>> {
    let mut e = wire::Encoder::new(handles);
    e.signed(value.n);
    Ok(e.into_bytes())
}

fn encoded(value: i64) -> Vec<u8> {
    encode_held(held(Arc::default(), value), &mut Handles::default()).unwrap()
}

fn dispatch_waiter(
    provider: &mut dyn Any,
    method: usize,
    args: &[u8],
    handles: &mut Handles,
) -> Dispatched {
    let waiter = provider.downcast_mut::<Waiter>().expect("the waiter");
    waiter.calls.borrow_mut().push((method, args.to_vec()));
    match method {
        0 if waiter.pending_read => {
            let (call, _) = waiter.call();
            wire::answer(call, encode_held, handles)
        }
        2 if matches!(waiter.mode, Mode::Ready) => wire::answer(
            NativeCall::Ready(Ok(held(Arc::default(), 5))),
            encode_held,
            handles,
        ),
        2 => {
            let (call, completion) = waiter.call();
            let _ = waiter.completions.send(completion);
            wire::answer(call, encode_held, handles)
        }
        _ => Ok(Answer::Ready(Vec::new())),
    }
}

/// `server` with the suspending `wait() -> Int`, method 2.
fn waiting() -> Contract {
    let mut contract = server(sdk());
    contract.interfaces[0]
        .methods
        .push(method("wait", vec![], int(), true));
    contract
}

/// The imports a `wait` body calls, at indices 0 to 5.
const WAIT_IMPORTS: &[(&str, &str, usize, usize)] = &[
    ("hd:pkg:server", "wait.start", 0, 2),
    ("hd:pkg:server", "wait.finish", 1, 1),
    ("hd:pkg:server", "echo", 1, 0),
    ("hd:rt", "block", 0, 1),
    ("hd:rt", "abort", 1, 0),
    ("hd:pkg:server", "read", 0, 1),
];

/// `wait.start`, then the wait for its completion and `echo` of its
/// `.finish`ed value.
const WAKE: &[I<'static>] = &[
    I::Call(0),
    I::LocalSet(1),
    I::LocalSet(0),
    I::Call(3),
    I::Drop,
    I::LocalGet(1),
    I::Call(1),
    I::Call(2),
];

/// `wait.start`, with the call left pending when `hd.init` returns.
const LEAVE: &[I<'static>] = &[I::Call(0), I::Drop, I::Drop];

/// A run of `body` against a `Waiter`: the instance, after `hd.init`.
struct Waited {
    instance: crate::WasmtimeInstance,
    outcome: Outcome,
    err: Shared,
    calls: Calls,
    aborts: Rc<std::cell::Cell<usize>>,
}

impl Waited {
    fn error(&self) -> String {
        String::from_utf8(self.err.take()).unwrap()
    }

    /// The bytes each `echo` took.
    fn echoed(&self) -> Vec<Vec<u8>> {
        self.calls
            .borrow()
            .iter()
            .filter(|(m, _)| *m == 1)
            .map(|(_, args)| args.clone())
            .collect()
    }
}

/// Runs `body` with `wait` in `mode`; `complete` gets each completion on
/// another thread, started before the guest.
fn waited(
    body: &[I<'_>],
    mode: Mode,
    pending_read: bool,
    complete: impl FnOnce(mpsc::Receiver<Completion<Held>>) + Send + 'static,
) -> Waited {
    let contract = waiting();
    let engine = WasmtimeEngine::new(EngineConfig::default()).unwrap();
    let module = engine
        .load(
            &module(&contract, WAIT_IMPORTS, body, false),
            &MemoryStore::default(),
        )
        .unwrap();
    let (completions, received) = mpsc::channel();
    let calls = Calls::default();
    let aborts = Rc::default();
    let waiter = Waiter {
        mode,
        aborts: Rc::clone(&aborts),
        completions,
        calls: Rc::clone(&calls),
        pending_read,
    };
    let mut bindings = Bindings::new(contract_bytes(&contract));
    bindings.register_dispatch(SERVER, "hd:pkg:server", Box::new(waiter), dispatch_waiter);
    let err = Shared::default();
    let io = HostIo {
        out: Box::new(Shared::default()),
        err: Box::new(err.clone()),
        input: None,
    };
    let mut instance = engine
        .instantiate_with_bindings(&module, &setup(), io, bindings)
        .unwrap();
    // cost: one thread per test run, for the completions it sends.
    let thread = std::thread::spawn(move || complete(received));
    let outcome = instance.init();
    thread.join().unwrap();
    Waited {
        instance,
        outcome,
        err,
        calls,
        aborts,
    }
}

#[test]
fn a_pending_call_wakes_its_instance_from_another_thread() {
    let value = Arc::new(());
    let sent = Arc::clone(&value);
    let run = waited(WAKE, Mode::Pending, false, move |received| {
        let completion = received.recv().unwrap();
        std::thread::sleep(Duration::from_millis(20));
        completion.complete(Ok(held(sent, 42)));
    });
    assert_eq!(run.outcome, Outcome::Exit(0), "{}", run.error());
    assert_eq!(run.echoed(), [encoded(42)]);
    assert_eq!(
        Arc::strong_count(&value),
        1,
        "the delivered value is dropped"
    );
    drop(run.instance);
    assert_eq!(run.aborts.get(), 0, "a delivered call has no abort");
}

#[test]
fn a_ready_or_completed_answer_reaches_the_guest_without_a_wait() {
    let ready = &[
        I::Call(0),
        I::LocalSet(1),
        I::Drop,
        I::LocalGet(1),
        I::Call(2),
    ];
    let run = waited(ready, Mode::Ready, false, |_| {});
    assert_eq!(run.outcome, Outcome::Exit(0), "{}", run.error());
    assert_eq!(run.echoed(), [encoded(5)]);
    // Completed before `.start` registered it: the first wait is due.
    let run = waited(WAKE, Mode::CompletedFirst, false, |_| {});
    assert_eq!(run.outcome, Outcome::Exit(0), "{}", run.error());
    assert_eq!(run.echoed(), [encoded(11)]);
}

#[test]
fn closing_an_instance_aborts_its_pending_call_once_and_drops_a_late_completion() {
    let (sender, late) = mpsc::channel();
    let run = waited(LEAVE, Mode::Pending, false, move |received| {
        sender.send(received.recv().unwrap()).unwrap();
    });
    assert_eq!(run.outcome, Outcome::Exit(0), "{}", run.error());
    assert_eq!(run.aborts.get(), 0, "still pending");
    let aborts = Rc::clone(&run.aborts);
    drop(run);
    assert_eq!(aborts.get(), 1, "the close aborts it");
    assert_eq!(
        Rc::strong_count(&aborts),
        1,
        "the hook's captures are released"
    );
    let value = Arc::new(());
    late.recv()
        .unwrap()
        .complete(Ok(held(Arc::clone(&value), 1)));
    assert_eq!(Arc::strong_count(&value), 1, "the late value is dropped");
    assert_eq!(aborts.get(), 1);
}

#[test]
fn aborting_a_pending_call_twice_runs_its_hook_once() {
    let twice = &[
        I::Call(0),
        I::LocalSet(1),
        I::Drop,
        I::LocalGet(1),
        I::Call(4),
        I::LocalGet(1),
        I::Call(4),
    ];
    let (sender, late) = mpsc::channel();
    let run = waited(twice, Mode::Pending, false, move |received| {
        sender.send(received.recv().unwrap()).unwrap();
    });
    assert_eq!(run.outcome, Outcome::Exit(0), "{}", run.error());
    assert_eq!(run.aborts.get(), 1);
    let value = Arc::new(());
    late.recv()
        .unwrap()
        .complete(Ok(held(Arc::clone(&value), 1)));
    assert_eq!(Arc::strong_count(&value), 1, "the late value is dropped");
    let aborts = Rc::clone(&run.aborts);
    drop(run);
    assert_eq!(aborts.get(), 1, "the close does not abort it again");
}

#[test]
fn a_new_instance_is_not_woken_by_a_closed_instances_completion() {
    // Both instances' first operation has one handle number. The closed
    // instance's completion, sent first, neither wakes the new instance
    // nor answers its wait.
    let (sender, old) = mpsc::channel();
    drop(waited(LEAVE, Mode::Pending, false, move |received| {
        sender.send(received.recv().unwrap()).unwrap();
    }));
    let old = old.recv().unwrap();
    let late = Arc::new(());
    let kept = Arc::clone(&late);
    let run = waited(WAKE, Mode::Pending, false, move |received| {
        let completion = received.recv().unwrap();
        old.complete(Ok(held(kept, 7)));
        std::thread::sleep(Duration::from_millis(20));
        completion.complete(Ok(held(Arc::default(), 9)));
    });
    assert_eq!(run.outcome, Outcome::Exit(0), "{}", run.error());
    assert_eq!(run.echoed(), [encoded(9)]);
    assert_eq!(Arc::strong_count(&late), 1, "the late value is dropped");
}

#[test]
fn a_failed_completion_is_a_host_contract_panic_naming_the_method() {
    let run = waited(WAKE, Mode::Pending, false, |received| {
        received
            .recv()
            .unwrap()
            .complete(Err(HostError("sensor offline".into())));
    });
    assert_eq!(run.outcome, Outcome::Exit(3));
    let error = run.error();
    assert!(
        error.starts_with(&format!(
            "panic: host-contract: package host: provider {SERVER}.wait rejected: sensor offline"
        )),
        "{error}"
    );
}

#[test]
fn a_pending_call_out_of_place_is_a_host_contract_panic() {
    let cases: [(&[I<'_>], Mode, bool, &str); 3] = [
        // A `.finish` before the call completed.
        (
            &[
                I::Call(0),
                I::LocalSet(1),
                I::Drop,
                I::LocalGet(1),
                I::Call(1),
                I::Drop,
            ],
            Mode::Pending,
            false,
            "wait: a finish of an operation that is not pending",
        ),
        (
            &[I::Call(0), I::Drop, I::Drop],
            Mode::AbortedFirst,
            false,
            "wait: the provider aborted the pending call it returned",
        ),
        (
            &[I::Call(5), I::Drop],
            Mode::Pending,
            true,
            "read: a method that does not suspend returned a pending call",
        ),
    ];
    for (body, mode, pending_read, message) in cases {
        let run = waited(body, mode, pending_read, drop);
        assert_eq!(run.outcome, Outcome::Exit(3));
        let error = run.error();
        assert!(
            error.starts_with(&format!("panic: host-contract: {SERVER}.{message}")),
            "{error}"
        );
        let aborts = Rc::clone(&run.aborts);
        drop(run);
        assert_eq!(aborts.get(), 1, "{message}: aborted once");
    }
}

#[test]
fn a_suspending_import_of_another_signature_is_refused_before_the_guest_starts() {
    let contract = waiting();
    for (name, params, results) in [("wait.start", 0, 1), ("wait.finish", 1, 2)] {
        let wasm = module(
            &contract,
            &[("hd:pkg:server", name, params, results)],
            &[],
            true,
        );
        assert_eq!(
            refusal(&wasm, bindings(&contract, &[SERVER], Recorder::default())),
            format!(
                "the import `hd:pkg:server` `{name}` of package host method {SERVER}.wait has an unexpected signature"
            )
        );
    }
}

/// What the native objects behind returned handles saw: their drops and
/// method calls.
#[derive(Default)]
struct Seen {
    drops: std::cell::Cell<usize>,
    calls: std::cell::Cell<usize>,
    /// The argument of the last `seek`.
    sought: std::cell::Cell<i64>,
}

impl Seen {
    fn call(&self) {
        self.calls.set(self.calls.get() + 1);
    }
}

/// One native object; two handles may share it through an `Rc`.
struct Object(Rc<Seen>);

impl Drop for Object {
    fn drop(&mut self) {
        self.0.drops.set(self.0.drops.get() + 1);
    }
}

/// The trait object of a `Conn` handle, as a companion boxes it.
type Conn = Rc<Object>;
/// The trait object of a `File` handle.
struct File(Rc<Seen>);

const CONN_TRAIT: u32 = 1;
const FILE_TRAIT: u32 = 2;

fn handle_trait(trait_path: &str, module: &str, methods: Vec<Method>) -> Interface {
    Interface {
        identity: identity("example.org/sdk", "1.2.3", trait_path),
        module: module.into(),
        selectors: vec![],
        handle: true,
        methods,
    }
}

/// `server` returning handles: `open() -> (Conn, Conn)` of one object,
/// `probe()`, `file() -> File` and the suspending `later() -> Conn`; `Conn`
/// has `peek() -> Int`, `dup() -> Conn`, the suspending `wait() -> Int`
/// and `seek(Int)`, `File` has `size() -> Int`.
fn handled() -> Contract {
    let conn = || Value::Handle("hd:pkg:conn".into());
    Contract {
        interfaces: vec![
            root(
                sdk(),
                "hd:pkg:server",
                vec![
                    method("open", vec![], Value::Tuple(vec![conn(), conn()]), false),
                    method("probe", vec![], Value::Void, false),
                    method("file", vec![], Value::Handle("hd:pkg:file".into()), false),
                    method("later", vec![], conn(), true),
                ],
            ),
            handle_trait(
                "server.Conn",
                "hd:pkg:conn",
                vec![
                    method("peek", vec![], int(), false),
                    method("dup", vec![], conn(), false),
                    method("wait", vec![], int(), true),
                    method("seek", vec![int()], Value::Void, false),
                ],
            ),
            handle_trait(
                "server.File",
                "hd:pkg:file",
                vec![method("size", vec![], int(), false)],
            ),
        ],
    }
}

/// The imports a handle body calls, at indices 0 to 13.
const HANDLE_IMPORTS: &[(&str, &str, usize, usize)] = &[
    ("hd:pkg:server", "open", 0, 1),
    ("hd:pkg:server", "probe", 0, 0),
    ("hd:pkg:server", "file", 0, 1),
    ("hd:pkg:conn", "$release", 1, 0),
    ("hd:pkg:conn", "peek", 1, 1),
    ("hd:pkg:conn", "dup", 1, 1),
    ("hd:pkg:file", "$release", 1, 0),
    ("hd:pkg:file", "size", 1, 1),
    ("hd:pkg:server", "later.start", 0, 2),
    ("hd:pkg:server", "later.finish", 1, 1),
    ("hd:rt", "block", 0, 1),
    ("hd:pkg:conn", "wait.start", 1, 2),
    ("hd:pkg:conn", "wait.finish", 1, 1),
    ("hd:pkg:conn", "seek", 2, 0),
];

/// The root provider: each `probe` logs the object's drops so far.
struct Opener {
    seen: Rc<Seen>,
    probes: Rc<RefCell<Vec<usize>>>,
}

#[allow(clippy::unnecessary_wraps)]
fn encode_int(value: i64, handles: &mut Handles) -> HostResult<Vec<u8>> {
    let mut e = wire::Encoder::new(handles);
    e.signed(value);
    Ok(e.into_bytes())
}

#[allow(clippy::needless_pass_by_value)]
fn encode_conn(value: Conn, handles: &mut Handles) -> HostResult<Vec<u8>> {
    let mut e = wire::Encoder::new(handles);
    e.handle(Box::new(value), CONN_TRAIT, dispatch_conn)?;
    Ok(e.into_bytes())
}

fn dispatch_opener(
    provider: &mut dyn Any,
    method: usize,
    _: &[u8],
    handles: &mut Handles,
) -> Dispatched {
    let opener = wire::provider::<Opener>(provider)?;
    let mut e = wire::Encoder::new(handles);
    match method {
        0 => {
            let object: Conn = Rc::new(Object(Rc::clone(&opener.seen)));
            e.handle(Box::new(Rc::clone(&object)), CONN_TRAIT, dispatch_conn)?;
            e.handle(Box::new(object), CONN_TRAIT, dispatch_conn)?;
        }
        1 => opener.probes.borrow_mut().push(opener.seen.drops.get()),
        2 => e.handle(
            Box::new(File(Rc::clone(&opener.seen))),
            FILE_TRAIT,
            dispatch_file,
        )?,
        _ => {
            // Completed before it is returned: its `.finish` issues the
            // handle.
            drop(e);
            let (call, completion) = pending(|| {});
            completion.complete(Ok(Rc::new(Object(Rc::clone(&opener.seen)))));
            return wire::answer(NativeCall::Pending(call), encode_conn, handles);
        }
    }
    Ok(Answer::Ready(e.into_bytes()))
}

fn dispatch_conn(
    provider: &mut dyn Any,
    method: usize,
    args: &[u8],
    handles: &mut Handles,
) -> Dispatched {
    let conn = wire::provider::<Conn>(provider)?;
    conn.0.call();
    match method {
        0 => Ok(Answer::Ready(encode_int(7, handles)?)),
        1 => Ok(Answer::Ready(encode_conn(Rc::clone(conn), handles)?)),
        2 => wire::answer(NativeCall::Ready(Ok(9)), encode_int, handles),
        _ => {
            conn.0.sought.set(wire::Decoder::new(args).signed()?);
            Ok(Answer::Ready(Vec::new()))
        }
    }
}

fn dispatch_file(provider: &mut dyn Any, _: usize, _: &[u8], handles: &mut Handles) -> Dispatched {
    wire::provider::<File>(provider)?.0.call();
    Ok(Answer::Ready(encode_int(3, handles)?))
}

/// A run of `body` against an `Opener`.
struct Opened {
    outcome: Outcome,
    error: String,
    seen: Rc<Seen>,
    probes: Rc<RefCell<Vec<usize>>>,
}

fn opened(engine: &WasmtimeEngine, body: &[I<'_>]) -> Opened {
    let contract = handled();
    let module = engine
        .load(
            &module(&contract, HANDLE_IMPORTS, body, false),
            &MemoryStore::default(),
        )
        .unwrap();
    let seen = Rc::<Seen>::default();
    let probes = Rc::<RefCell<Vec<usize>>>::default();
    let opener = Opener {
        seen: Rc::clone(&seen),
        probes: Rc::clone(&probes),
    };
    let mut bindings = Bindings::new(contract_bytes(&contract));
    let entry: Dispatch = dispatch_opener;
    bindings.register_dispatch(SERVER, "hd:pkg:server", Box::new(opener), entry);
    let err = Shared::default();
    let io = HostIo {
        out: Box::new(Shared::default()),
        err: Box::new(err.clone()),
        input: None,
    };
    let mut instance = engine
        .instantiate_with_bindings(&module, &setup(), io, bindings)
        .unwrap();
    let outcome = instance.init();
    Opened {
        outcome,
        error: String::from_utf8(err.take()).unwrap(),
        seen,
        probes,
    }
}

#[test]
fn two_handles_to_one_object_drop_it_at_the_second_release_in_either_order() {
    let engine = WasmtimeEngine::new(EngineConfig::default()).unwrap();
    for (first, second) in [(1, 2), (2, 1)] {
        let run = opened(
            &engine,
            &[
                I::Call(0),
                I::Drop,
                I::I32Const(first),
                I::Call(3),
                I::Call(1),
                I::I32Const(second),
                I::Call(3),
                I::Call(1),
            ],
        );
        assert_eq!(run.outcome, Outcome::Exit(0), "{}", run.error);
        assert_eq!(
            *run.probes.borrow(),
            [0, 1],
            "the object drops at the second release, {first} then {second}"
        );
        assert_eq!(run.seen.calls.get(), 0, "a release calls no method");
    }
}

#[test]
fn a_forged_released_foreign_or_other_trait_handle_calls_no_method() {
    let engine = WasmtimeEngine::new(EngineConfig::default()).unwrap();
    let call = "package host: call of an invalid handle";
    let release = "package host: release of an invalid handle";
    let cases: [(&str, &[I<'_>], &str); 8] = [
        ("never issued", &[I::I32Const(7), I::Call(4), I::Drop], call),
        ("negative", &[I::I32Const(-1), I::Call(4), I::Drop], call),
        (
            "never issued, suspending",
            &[I::I32Const(1), I::Call(11), I::Drop, I::Drop],
            call,
        ),
        (
            "released",
            &[
                I::Call(0),
                I::Drop,
                I::I32Const(1),
                I::Call(3),
                I::I32Const(1),
                I::Call(4),
                I::Drop,
            ],
            call,
        ),
        (
            "released, reused slot",
            &[
                I::Call(0),
                I::Drop,
                I::I32Const(2),
                I::Call(3),
                I::Call(2),
                I::Drop,
                I::I32Const(2),
                I::Call(7),
                I::Drop,
            ],
            call,
        ),
        (
            "another trait",
            &[I::Call(2), I::Drop, I::I32Const(1), I::Call(4), I::Drop],
            call,
        ),
        (
            "release of another trait",
            &[I::Call(2), I::Drop, I::I32Const(1), I::Call(3)],
            release,
        ),
        (
            "released twice",
            &[
                I::Call(0),
                I::Drop,
                I::I32Const(1),
                I::Call(3),
                I::I32Const(1),
                I::Call(3),
            ],
            release,
        ),
    ];
    for (case, body, message) in cases {
        let run = opened(&engine, body);
        assert_eq!(run.outcome, Outcome::Exit(3), "{case}: {}", run.error);
        assert!(
            run.error
                .starts_with(&format!("panic: host-contract: {message}")),
            "{case}: {}",
            run.error
        );
        assert_eq!(run.seen.calls.get(), 0, "{case}: no method runs");
    }
    // Another instance's handle: this one issued none, though the other,
    // still open, holds that number.
    let owner = opened(&engine, &[I::Call(0), I::Drop]);
    assert_eq!(owner.outcome, Outcome::Exit(0), "{}", owner.error);
    let foreign = opened(&engine, &[I::I32Const(1), I::Call(4), I::Drop]);
    assert_eq!(foreign.outcome, Outcome::Exit(3));
    assert!(
        foreign
            .error
            .starts_with(&format!("panic: host-contract: {call}"))
    );
    assert_eq!(owner.seen.calls.get() + foreign.seen.calls.get(), 0);
}

#[test]
fn methods_on_a_returned_handle_dispatch_through_its_own_trait() {
    let engine = WasmtimeEngine::new(EngineConfig::default()).unwrap();
    let run = opened(
        &engine,
        &[
            // `open` issues 1 and 2; `dup` on 1 issues 3 while 1 is out
            // of its slot for the call.
            I::Call(0),
            I::Drop,
            I::I32Const(1),
            I::Call(4),
            I::Drop,
            I::I32Const(1),
            I::Call(5),
            I::Drop,
            I::I32Const(3),
            I::Call(4),
            I::Drop,
            // `file` issues 4; `size` calls the `File` trait's dispatch.
            I::Call(2),
            I::Drop,
            I::I32Const(4),
            I::Call(7),
            I::Drop,
            // A handle's suspending method answers at once.
            I::I32Const(3),
            I::Call(11),
            I::Drop,
            I::Drop,
            // `seek(-3)`: its argument, zigzag 5, follows the handle.
            I::I32Const(0),
            I::I32Const(5),
            I::I32Store8(wasm_encoder::MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            I::I32Const(2),
            I::I32Const(1),
            I::Call(13),
            // `later.finish` issues 5 from the delivered value.
            I::Call(8),
            I::LocalSet(1),
            I::Drop,
            I::Call(10),
            I::Drop,
            I::LocalGet(1),
            I::Call(9),
            I::Drop,
            I::I32Const(5),
            I::Call(4),
            I::Drop,
            // Every handle released: both objects drop.
            I::I32Const(5),
            I::Call(3),
            I::I32Const(4),
            I::Call(6),
            I::I32Const(1),
            I::Call(3),
            I::I32Const(2),
            I::Call(3),
            I::I32Const(3),
            I::Call(3),
            I::Call(1),
        ],
    );
    assert_eq!(run.outcome, Outcome::Exit(0), "{}", run.error);
    assert_eq!(run.seen.calls.get(), 7);
    assert_eq!(run.seen.sought.get(), -3);
    assert_eq!(*run.probes.borrow(), [2]);
}

#[test]
fn a_companion_of_other_handle_traits_is_refused_before_the_guest_starts() {
    let wasm = module(&handled(), HANDLE_IMPORTS, &[], true);
    let refused = |companion: &Contract| {
        let mut bindings = Bindings::new(contract_bytes(companion));
        let entry: Dispatch = dispatch_opener;
        let opener = Opener {
            seen: Rc::default(),
            probes: Rc::default(),
        };
        bindings.register_dispatch(SERVER, "hd:pkg:server", Box::new(opener), entry);
        refusal(&wasm, bindings)
    };
    let expected = "native provider for package host trait \"example.org/sdk@1.2.3/server.Conn\" was generated from a different contract";
    // Other methods: the companion's `Conn` dispatch numbers them otherwise.
    let mut methods = handled();
    methods.interfaces[1].methods.pop();
    assert_eq!(refused(&methods), expected);
    // The same traits in another order: a companion names a handle trait
    // by its index, which would mean another trait here.
    let mut order = handled();
    order.interfaces.swap(1, 2);
    assert_eq!(refused(&order), expected);
}
