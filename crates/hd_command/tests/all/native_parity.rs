//! Native and Node providers of the same trait observe the same hd values
//! (`module.host.package.native-parity`). Each `host-binding-*` CLI case
//! that reaches a provider method is built once with `--engine wasmtime`;
//! Node runs that `.wasm` through `hd FILE.wasm --engine node` with the
//! case's JS provider, and Wasmtime runs it in process with a Rust
//! provider that does what the JS one does. Their exit status, standard
//! output and first line of standard error must be equal.
//!
//! Cases a Rust provider cannot express are left out:
//! - `host-binding-recursive-cycle`: its provider returns a tree that holds
//!   itself, and the companion's owned values cannot form a cycle.
//! - `host-binding-never-returns`: the companion types a `never` result as
//!   `Infallible`, so a provider cannot return from it.
//! - `host-binding-never-exits`: its provider ends the process, which an
//!   in-process provider would do to the test itself.
//!
//! Each case's companion is generated from the built module's contract
//! with a fixed package identity, since a local trait's identity hashes
//! its scratch directory; `HD_BLESS=1` rewrites the snapshots under
//! `tests/all/native/parity/`. Its dispatches are registered under the
//! built module's real identities, as the written companion registers them.

use std::path::Path;

use hd_base::SliceSort as _;
use hd_host_abi::schema::Contract;
use hd_run::{Consent, Engine as _, GrantValue, Grants, HostSetup, Instance as _, Limits, Outcome};
use hd_run_wasmtime::native::{self, CallContext, HostError, HostResult, NativeCall};
use hd_run_wasmtime::{EngineConfig, HostIo, WasmtimeEngine};
use hd_testkit::Scratch;

/// One run's exit status, standard output and first standard error line.
#[derive(Debug, PartialEq, Eq)]
struct Run {
    status: i32,
    stdout: String,
    stderr: String,
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().to_owned()
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("directory");
    for entry in std::fs::read_dir(from).expect("case directory") {
        let entry = entry.expect("entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("type").is_dir() {
            copy(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).expect("copy");
        }
    }
}

/// `hd ARGS` in `root`, with its own cache.
fn hd(root: &Path, args: &[&str]) -> std::process::Output {
    hd_testkit::node::child(env!("CARGO_BIN_EXE_hd"))
        .env(
            "HD_CACHE",
            crate::shared_objects(root.with_extension("cache")),
        )
        .env_remove("HD_JOBS")
        .env_remove("HD_ENGINE")
        .env_remove("HD_WAT")
        .current_dir(root)
        .args(args)
        .output()
        .expect("run hd")
}

/// A case copied to a scratch directory and built for Wasmtime.
struct Built {
    root: Scratch,
    stem: &'static str,
    wasm: Vec<u8>,
    contract: Contract,
}

fn build(case: &str, stem: &'static str) -> Built {
    let root = Scratch::new(env!("CARGO_TARGET_TMPDIR"), &format!("parity-{case}"));
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../spec/conformance/cli")
        .join(format!("host-binding-{case}"));
    copy(&source, &root);
    built(root, stem)
}

/// The package at `root`, built for Wasmtime.
fn built(root: Scratch, stem: &'static str) -> Built {
    let out = hd(&root, &["build", "--engine", "wasmtime"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{stem}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let wasm = std::fs::read(root.join(format!("build/debug/{stem}.wasm"))).expect("wasm");
    let contract = hd_host_abi::schema::from_wasm(&wasm)
        .expect("a valid contract")
        .expect("a package host contract");
    Built {
        root,
        stem,
        wasm,
        contract,
    }
}

/// The companion of `contract`, with each hashed package and module name
/// replaced by a fixed one of the same length, is the checked-in one
/// compiled here. The replacement runs on the encoded contract, so a
/// handle's reference to its trait's identity changes with it. Names are
/// fixed in the order of each trait's manifest selectors, and the fixed
/// traits sorted by identity as emission sorts them, since two packages'
/// traits otherwise come in the order of their directories' hashes.
fn snapshot(case: &str, contract: &Contract) {
    let mut order: Vec<_> = contract.interfaces.iter().collect();
    order.sort_stable_by_key(|i| (&i.selectors, &i.identity.trait_path));
    let mut names: Vec<String> = Vec::new();
    for i in order {
        for name in [&i.identity.package, &i.module] {
            if !names.contains(name) {
                names.push(name.clone());
            }
        }
    }
    let mut bytes = contract.encode();
    for (n, name) in names.iter().enumerate() {
        let Some((prefix, hash)) = name.rsplit_once(':') else {
            continue;
        };
        let fixed = format!("{prefix}:{n:0>width$}", width = hash.len());
        let (from, to) = (name.as_bytes(), fixed.as_bytes());
        let mut at = 0;
        while let Some(found) = bytes[at..].windows(from.len()).position(|w| w == from) {
            bytes[at + found..at + found + from.len()].copy_from_slice(to);
            at += found + from.len();
        }
    }
    let mut fixed = Contract::decode(&bytes).expect("the fixed contract decodes");
    fixed
        .interfaces
        .sort_stable_by_key(|i| (i.identity.to_string(), i.handle));
    let text = hd_host_abi::native_rust::generate(&fixed).expect("a companion");
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/all/native/parity")
        .join(format!("{case}.host.rs"));
    if std::env::var_os("HD_BLESS").is_some() {
        std::fs::write(&path, &text).expect("bless");
    }
    assert_eq!(
        std::fs::read_to_string(&path).expect("snapshot"),
        text,
        "{} is stale; rerun with HD_BLESS=1",
        path.display()
    );
}

fn node(built: &Built) -> Run {
    let wasm = format!("build/debug/{}.wasm", built.stem);
    let out = hd(&built.root, &[&wasm, "--engine", "node"]);
    Run {
        status: out.status.code().expect("Node exits"),
        stdout: String::from_utf8(out.stdout).expect("utf-8"),
        stderr: first_line(&String::from_utf8_lossy(&out.stderr)),
    }
}

/// Output kept for the test to read.
#[derive(Clone, Default)]
struct Captured(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl Captured {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().expect("output").clone()).expect("utf-8")
    }
}

impl std::io::Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("output").extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

/// A root trait's provider: the selector of its trait in the app manifest,
/// its trait object (`Box<dyn Trait>`, boxed again as `Any`) and its
/// dispatch.
type Provider = (&'static str, Box<dyn std::any::Any>, native::Dispatch);

/// The registry of a built module, each provider under the identity and
/// module of the root trait its selector names.
fn bindings(contract: &Contract, providers: Vec<Provider>) -> native::Bindings {
    let mut bindings = native::Bindings::new(Box::leak(contract.encode().into_boxed_slice()));
    for (selector, provider, dispatch) in providers {
        let root = contract
            .interfaces
            .iter()
            .find(|i| i.selectors.iter().any(|s| s == selector))
            .expect("a root trait with that selector");
        bindings.register_dispatch(
            leak(root.identity.to_string()),
            leak(root.module.clone()),
            provider,
            dispatch,
        );
    }
    bindings
}

/// The root manifest's consent: each quoted `[capabilities]` key set `true`.
fn consent(root: &Path) -> Consent {
    let manifest = std::fs::read_to_string(root.join("hd.toml")).expect("hd.toml");
    let table: Vec<(String, GrantValue)> = manifest
        .lines()
        .filter_map(|l| {
            l.strip_suffix(" = true")?
                .strip_prefix('"')?
                .strip_suffix('"')
        })
        .map(|key| (key.to_owned(), GrantValue::All))
        .collect();
    Consent::new(&table, &[])
}

fn wasmtime(built: &Built, providers: Vec<Provider>) -> Run {
    let engine = WasmtimeEngine::new(EngineConfig::default()).expect("engine");
    let module = engine
        .load(&built.wasm, &hd_cache::MemoryStore::default())
        .expect("the module loads");
    let (stdout, stderr) = (Captured::default(), Captured::default());
    let io = HostIo {
        out: Box::new(stdout.clone()),
        err: Box::new(stderr.clone()),
        input: None,
    };
    let setup = HostSetup {
        grants: Grants::with_consent(consent(&built.root)),
        limits: Limits::default(),
        args: vec![],
        program: built.stem.into(),
    };
    let mut instance = engine
        .instantiate_with_bindings(&module, &setup, io, bindings(&built.contract, providers))
        .expect("the registered providers load");
    let outcome = match instance.init() {
        Outcome::Exit(0) => hd_run::drive(&mut instance, 1 << 16),
        other => other,
    };
    drop(instance);
    let Outcome::Exit(status) = outcome else {
        panic!("{}: {outcome:?}", built.stem)
    };
    Run {
        status: i32::from(status),
        stdout: stdout.text(),
        stderr: first_line(&stderr.text()),
    }
}

/// Builds `case`, checks its companion snapshot, and asserts that its
/// Wasmtime run with `providers` equals its Node run.
fn parity(case: &str, stem: &'static str, providers: impl FnOnce() -> Vec<Provider>) -> Run {
    let built = build(case, stem);
    snapshot(case, &built.contract);
    same(&built, providers)
}

/// Asserts that the Wasmtime run of `built` with `providers` equals its
/// Node run.
fn same(built: &Built, providers: impl FnOnce() -> Vec<Provider>) -> Run {
    let case = built.stem;
    let node = node(built);
    let native = wasmtime(built, providers());
    assert_eq!(
        native, node,
        "{case}: Wasmtime (left) and Node (right) differ"
    );
    native
}

/// A provider boxed as its registration holds it.
fn provider<T: ?Sized + 'static>(value: Box<T>) -> Box<dyn std::any::Any> {
    Box::new(value)
}

/// A pending call whose completion lands after `delay` on another thread.
fn later<T: Send + 'static>(delay: u64, value: T) -> NativeCall<T> {
    let (call, completion) = native::pending(|| {});
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(delay));
        completion.complete(Ok(value));
    });
    NativeCall::Pending(call)
}

/// A pending call completed before it is returned, as an awaited
/// `Promise.resolve` is.
fn settled<T: 'static>(value: T) -> NativeCall<T> {
    let (call, completion) = native::pending(|| {});
    completion.complete(Ok(value));
    NativeCall::Pending(call)
}

macro_rules! companion {
    ($name:ident, $file:literal) => {
        #[allow(dead_code, clippy::all, clippy::pedantic)]
        mod $name {
            include!(concat!("native/parity/", $file, ".host.rs"));
        }
    };
}

companion!(block_on, "block-on-promise");
companion!(counted, "counted-ref");
companion!(counted_cycle, "counted-ref-cycle");
companion!(event_handle, "event-handle");
companion!(fieldless, "fieldless-struct");
companion!(map, "map");
companion!(probe, "native-companion");
companion!(two_packages, "node");
companion!(private, "private-values");
companion!(readiness, "readiness");
companion!(recursive, "recursive-type");
companion!(self_handle, "self-returning-handle");
companion!(union, "union");
companion!(failing, "failures");

struct Service;

impl block_on::Service for Service {
    fn wait(&mut self, kind: String, _: CallContext<'_>) -> NativeCall<u32> {
        match kind.as_str() {
            "ready" => NativeCall::Ready(Ok(7)),
            "promise" => settled(7),
            _ => later(1, 7),
        }
    }
}

#[test]
fn a_block_on_call_completes_ready_settled_or_later() {
    let run = parity("block-on-promise", "blockingpromises", || {
        let p: Box<dyn block_on::Service> = Box::new(Service);
        vec![("pkg.main.Service", provider(p), block_on::dispatch_Service)]
    });
    assert_eq!(run.stdout, "ready 7\npromise 7\ntimer 7\n");
}

struct Counted;

fn total(e: &counted::Expr) -> i32 {
    match e {
        counted::Expr::Num { n } => *n,
        counted::Expr::Add { a, b } => total(a) + total(b),
    }
}

fn result_of(e: &counted::Expr2) -> counted::Expr3 {
    match e {
        counted::Expr2::Num { n } => counted::Expr3::Num { n: *n },
        counted::Expr2::Add { a, b } => counted::Expr3::Add {
            a: Box::new(result_of(a)),
            b: Box::new(result_of(b)),
        },
    }
}

impl counted::Surface for Counted {
    fn eval(&mut self, e: &counted::Expr) -> HostResult<i32> {
        Ok(total(e))
    }

    fn twice(&mut self, e: &counted::Expr2) -> HostResult<counted::Expr3> {
        let one = result_of(e);
        Ok(counted::Expr3::Add {
            a: Box::new(one.clone()),
            b: Box::new(one),
        })
    }
}

#[test]
fn a_counted_reference_crosses_as_its_tree_both_ways() {
    let run = parity("counted-ref", "counted_boundary", || {
        let p: Box<dyn counted::Surface> = Box::new(Counted);
        vec![("pkg.main.Surface", provider(p), counted::dispatch_Surface)]
    });
    assert_eq!(run.stdout, "4 false\n");
}

struct NeverCalled;

impl counted_cycle::Surface for NeverCalled {
    fn size(&mut self, _: &counted_cycle::Node) -> HostResult<i32> {
        panic!("the guest's encoder meets the cycle before the call")
    }
}

#[test]
fn a_cycle_in_an_argument_fails_before_either_provider_runs() {
    let run = parity("counted-ref-cycle", "counted_cycle", || {
        let p: Box<dyn counted_cycle::Surface> = Box::new(NeverCalled);
        vec![(
            "pkg.main.Surface",
            provider(p),
            counted_cycle::dispatch_Surface,
        )]
    });
    assert_eq!(run.status, 3);
    assert_eq!(run.stderr, "panic: boundary-cycle: a value reaches itself");
}

struct Surface;

struct Events {
    closed: bool,
}

impl event_handle::Events for Events {
    fn next(
        &mut self,
        _: CallContext<'_>,
    ) -> NativeCall<Result<Option<event_handle::Event>, event_handle::EventError>> {
        NativeCall::Ready(Ok(if self.closed {
            Err(event_handle::EventError::Closed)
        } else {
            Ok(Some(event_handle::Event::Click { tag: 7 }))
        }))
    }

    fn dispose(&mut self, _: CallContext<'_>) -> NativeCall<()> {
        self.closed = true;
        NativeCall::Ready(Ok(()))
    }
}

impl event_handle::Surface for Surface {
    fn render(&mut self, commands: Vec<String>, _: CallContext<'_>) -> NativeCall<u32> {
        NativeCall::Ready(Ok(u32::try_from(commands.len()).expect("few commands")))
    }

    fn events(&mut self, _: CallContext<'_>) -> NativeCall<Box<dyn event_handle::Events>> {
        NativeCall::Ready(Ok(Box::new(Events { closed: false })))
    }
}

#[test]
fn an_event_handle_answers_and_is_disposed() {
    let run = parity("event-handle", "surface", || {
        let p: Box<dyn event_handle::Surface> = Box::new(Surface);
        vec![(
            "pkg.main.Surface",
            provider(p),
            event_handle::dispatch_Surface,
        )]
    });
    assert_eq!(run.stdout, "rendered 1\nclick 7\n");
}

struct Fieldless;

impl fieldless::Surface for Fieldless {
    fn render(
        &mut self,
        alone: fieldless::Empty,
        wrap: fieldless::Wrap,
        pick: fieldless::Pick,
        many: Vec<fieldless::Empty>,
    ) -> HostResult<(
        fieldless::Empty,
        fieldless::Wrap,
        fieldless::Pick,
        Vec<fieldless::Empty>,
    )> {
        use fieldless::{Empty, Pick, Wrap};
        assert_eq!(alone, Empty {});
        let wrapped = |before, after: &str| Wrap {
            before,
            e: Empty {},
            after: after.into(),
        };
        assert_eq!(wrap, wrapped(1, "z"));
        assert_eq!(pick, Pick::One { e: Empty {}, n: 7 });
        assert_eq!(many, [Empty {}, Empty {}]);
        Ok((
            Empty {},
            wrapped(2, "y"),
            pick,
            vec![Empty {}, Empty {}, Empty {}],
        ))
    }
}

#[test]
fn a_fieldless_struct_crosses_in_every_position() {
    let run = parity("fieldless-struct", "fieldless_struct", || {
        let p: Box<dyn fieldless::Surface> = Box::new(Fieldless);
        vec![("pkg.main.Surface", provider(p), fieldless::dispatch_Surface)]
    });
    assert_eq!(run.stdout, "2 y 7 3\n");
}

struct Map;

impl map::Surface for Map {
    fn render(&mut self, given: Vec<(String, i32)>) -> HostResult<Vec<(String, i32)>> {
        // A JS `Map`: a key set again keeps its first place.
        let mut out = vec![("z".to_owned(), 26)];
        for (key, value) in given {
            match out.iter_mut().find(|(k, _)| *k == key) {
                Some(entry) => entry.1 = value * 10,
                None => out.push((key, value * 10)),
            }
        }
        Ok(out)
    }
}

#[test]
fn a_map_crosses_in_its_entry_order() {
    let run = parity("map", "map_boundary", || {
        let p: Box<dyn map::Surface> = Box::new(Map);
        vec![("pkg.main.Surface", provider(p), map::dispatch_Surface)]
    });
    assert_eq!(run.stdout, "3 z=26;b=20;a=10; 10\n");
}

struct Probe;

impl probe::Server for Probe {
    fn read(&mut self) -> HostResult<i32> {
        Ok(7)
    }
}

#[test]
fn a_root_provider_answers_its_guest() {
    let run = parity("native-companion", "host_probe", || {
        let p: Box<dyn probe::Server> = Box::new(Probe);
        vec![("pkg.main.Server", provider(p), probe::dispatch_Server)]
    });
    assert_eq!(run.stdout, "guest started\n7\n");
}

/// One of two packages' `Server`s: it prefixes its letter.
struct Lettered(&'static str);

impl two_packages::Server for Lettered {
    fn read(&mut self, text: String) -> HostResult<String> {
        Ok(format!("{} {text}", self.0))
    }

    fn wait(&mut self, _: CallContext<'_>) -> NativeCall<String> {
        settled(format!("{} pending", self.0))
    }
}

impl two_packages::Server2 for Lettered {
    fn read(&mut self, text: String) -> HostResult<String> {
        two_packages::Server::read(self, text)
    }

    fn wait(&mut self, cx: CallContext<'_>) -> NativeCall<String> {
        two_packages::Server::wait(self, cx)
    }
}

#[test]
fn two_packages_traits_of_one_path_keep_their_own_providers() {
    let run = parity("node", "host_app", || {
        // The companion names the second `Server`, package `b`'s, `Server2`.
        let a: Box<dyn two_packages::Server> = Box::new(Lettered("A"));
        let b: Box<dyn two_packages::Server2> = Box::new(Lettered("B"));
        vec![
            ("dep.a.Server", provider(a), two_packages::dispatch_Server),
            ("dep.b.Server", provider(b), two_packages::dispatch_Server2),
        ]
    });
    assert_eq!(run.stdout, "A first\nB second\nA pending\nB pending\n");
}

struct Private;

impl private::Server for Private {
    fn secret(&mut self, value: native::Serde) -> HostResult<native::Serde> {
        use native::Serde as S;
        let S::Map(fields) = value else {
            return Err(HostError(format!("not a record: {value:?}")));
        };
        let field = |name: &str| {
            fields
                .iter()
                .find(|(k, _)| *k == S::String(name.into()))
                .map(|(_, v)| v.clone())
        };
        assert_eq!(field("large"), Some(S::U64(u64::MAX)));
        assert_eq!(field("signed"), Some(S::I64(i64::MIN)));
        assert_eq!(field("ratio"), Some(S::F64(1.0)));
        assert!(matches!(field("zero"), Some(S::F64(z)) if z == 0.0 && z.is_sign_negative()));
        assert_eq!(
            field("children"),
            Some(S::Seq(vec![S::String("nested".into())]))
        );
        Ok(S::Map(
            fields
                .into_iter()
                .map(|(k, v)| {
                    let v = match v {
                        S::U64(n) => S::U64(n - 1),
                        S::I64(n) => S::I64(n + 1),
                        other => other,
                    };
                    (k, v)
                })
                .collect(),
        ))
    }

    fn custom(&mut self, value: native::Serde) -> HostResult<native::Serde> {
        assert_eq!(value, native::Serde::String("custom:private".into()));
        Ok(native::Serde::String("custom reply".into()))
    }
}

#[test]
fn private_values_cross_as_their_serde_tree() {
    let run = parity("private-values", "private_values", || {
        let p: Box<dyn private::Server> = Box::new(Private);
        vec![("pkg.main.Server", provider(p), private::dispatch_Server)]
    });
    assert_eq!(
        run.stdout,
        "18446744073709551614 -9223372036854775807 4607182418800017408 9223372036854775808 nested\ncustom reply\n"
    );
}

/// `wait("cancel")` drops its completion and waits until aborted; any other kind
/// delivers after a millisecond. `aborts` counts the abort hooks run.
struct Source(std::rc::Rc<std::cell::Cell<u32>>);

impl readiness::Source for Source {
    fn wait(&mut self, kind: String, _: CallContext<'_>) -> NativeCall<String> {
        if kind != "cancel" {
            return later(1, "delivered".to_owned());
        }
        let aborted = std::rc::Rc::clone(&self.0);
        let (call, completion) = native::pending(move || aborted.set(aborted.get() + 1));
        // cli.host.package.native-completion-dropped: dropping the sender
        // leaves the call waiting, just like the Node provider's unresolved Promise.
        drop(completion);
        NativeCall::Pending(call)
    }

    fn aborts(&mut self) -> HostResult<u32> {
        Ok(self.0.get())
    }
}

#[test]
fn a_timed_out_call_is_aborted_once_and_a_later_one_delivers() {
    let run = parity("readiness", "readiness", || {
        let p: Box<dyn readiness::Source> = Box::new(Source(std::rc::Rc::default()));
        vec![("pkg.main.Source", provider(p), readiness::dispatch_Source)]
    });
    assert_eq!(run.stdout, "cancelled\naborts 1\ndelivered\n");
}

struct Recursive;

fn tree_of(t: &recursive::Tree) -> recursive::Tree2 {
    match t {
        recursive::Tree::Leaf => recursive::Tree2::Leaf,
        recursive::Tree::Node { kids } => recursive::Tree2::Node {
            kids: kids.iter().map(tree_of).collect(),
        },
    }
}

impl recursive::Surface for Recursive {
    fn render(&mut self, t: &recursive::Tree) -> HostResult<recursive::Tree2> {
        use recursive::Tree2 as T;
        let mut tree = tree_of(t);
        for _ in 0..39 {
            tree = T::Node { kids: vec![tree] };
        }
        Ok(T::Node {
            kids: vec![tree.clone(), tree],
        })
    }
}

#[test]
fn a_recursive_type_crosses_deep_and_shared_as_copies() {
    let run = parity("recursive-type", "recursive_type", || {
        let p: Box<dyn recursive::Surface> = Box::new(Recursive);
        vec![("pkg.main.Surface", provider(p), recursive::dispatch_Surface)]
    });
    assert_eq!(run.stdout, "80 2\n");
}

struct Root;

struct Node(i32);

impl self_handle::Node for Node {
    fn depth_of(&mut self) -> HostResult<i32> {
        Ok(self.0)
    }

    fn next(&mut self) -> HostResult<Box<dyn self_handle::Node>> {
        Ok(Box::new(Node(self.0 + 1)))
    }
}

impl self_handle::Surface for Root {
    fn root(&mut self) -> HostResult<Box<dyn self_handle::Node>> {
        Ok(Box::new(Node(0)))
    }
}

#[test]
fn a_handle_method_returns_a_handle_of_its_own_trait() {
    let run = parity("self-returning-handle", "self_returning_handle", || {
        let p: Box<dyn self_handle::Surface> = Box::new(Root);
        vec![(
            "pkg.main.Surface",
            provider(p),
            self_handle::dispatch_Surface,
        )]
    });
    assert_eq!(run.stdout, "3\n");
}

struct Union;

// The companion names a union's cases `_0`, `_1` as hd does.
#[allow(clippy::used_underscore_items)]
impl union::Surface for Union {
    fn get(&mut self, flip: bool) -> HostResult<union::SurfaceGetResult> {
        Ok(if flip {
            union::SurfaceGetResult::_1 { _0: "a".into() }
        } else {
            union::SurfaceGetResult::_0 { _0: 42 }
        })
    }

    fn render(&mut self, value: union::SurfaceRenderArg0) -> HostResult<String> {
        Ok(match value {
            union::SurfaceRenderArg0::_0 { _0: n } => format!("i64 {n}"),
            union::SurfaceRenderArg0::_1 { _0: p } => format!("point {},{}", p.x, p.y),
            union::SurfaceRenderArg0::_2 { _0: items } => format!("list {}", items.join("+")),
        })
    }
}

#[test]
fn a_union_crosses_as_its_case_and_payload() {
    let run = parity("union", "host_union", || {
        let p: Box<dyn union::Surface> = Box::new(Union);
        vec![("pkg.main.Surface", provider(p), union::dispatch_Surface)]
    });
    assert_eq!(run.stdout, "int 42\ntext a\ni64 7\npoint 1,2\nlist p+q\n");
}

const FAILING: &str = "@host\npub trait Server:\n    fn read(&self) -> i32\n    fn wait!(&mut self) -> i32\n\npub fn main!() -> void $ Console + Server:\n    println(\"before\")\n    println(\"${$.use(Server).wait!()}\")\n    println(\"${$.use(Server).read()}\")\n";

/// How the `failures` provider answers: `read` throws, or `wait!` throws
/// at once, or its pending call fails.
#[derive(Clone, Copy)]
enum Fails {
    Read,
    WaitAtOnce,
    WaitLater,
}

impl failing::Server for Fails {
    fn read(&mut self) -> HostResult<i32> {
        match self {
            Self::Read => Err(HostError("sensor offline".into())),
            _ => Ok(7),
        }
    }

    fn wait(&mut self, _: CallContext<'_>) -> NativeCall<i32> {
        let error = || HostError("sensor offline".into());
        match self {
            Self::Read => settled(8),
            Self::WaitAtOnce => NativeCall::Ready(Err(error())),
            Self::WaitLater => {
                let (call, completion) = native::pending(|| {});
                completion.complete(Err(error()));
                NativeCall::Pending(call)
            }
        }
    }
}

/// A provider's own failure prints the JS host's text for the same
/// failure: a throw names the provider as throwing, a failed pending call
/// as rejecting (`module.host.package.native-parity`).
#[test]
fn a_provider_failure_prints_the_js_hosts_text() {
    let cases = [
        (
            Fails::Read,
            "read() { throw \"sensor offline\"; }, async wait() { return 8; }",
            "read: provider threw sensor offline",
        ),
        (
            Fails::WaitAtOnce,
            "read() { return 7; }, wait() { throw \"sensor offline\"; }",
            "wait: provider threw sensor offline",
        ),
    ];
    let rejected = (
        Fails::WaitLater,
        "read() { return 7; }, async wait() { throw \"sensor offline\"; }",
    );
    let mut seen = Vec::new();
    for (n, (fails, js)) in cases
        .iter()
        .map(|(f, js, _)| (*f, *js))
        .chain([rejected])
        .enumerate()
    {
        let root = Scratch::new(env!("CARGO_TARGET_TMPDIR"), &format!("parity-failures-{n}"));
        std::fs::create_dir_all(root.join("src")).expect("src");
        std::fs::write(
            root.join("hd.toml"),
            "[package]\nname = \"failures\"\n\n[host]\n\"main.Server\" = \"provider.ts\"\n\n[capabilities]\nConsole = true\n\"pkg.main.Server\" = true\n",
        )
        .expect("hd.toml");
        std::fs::write(
            root.join("provider.ts"),
            format!("export default {{ {js} }};\n"),
        )
        .expect("provider");
        std::fs::write(root.join("src/main.hd"), FAILING).expect("main");
        let built = built(root, "failures");
        snapshot("failures", &built.contract);
        let run = same(&built, || {
            let p: Box<dyn failing::Server> = Box::new(fails);
            vec![("pkg.main.Server", provider(p), failing::dispatch_Server)]
        });
        assert_eq!(run.status, 3);
        let label = built.contract.interfaces[0].identity.to_string();
        seen.push(run.stderr.replace(&label, "LABEL"));
    }
    assert_eq!(
        seen,
        [
            "panic: host-contract: LABEL.read: provider threw sensor offline",
            "panic: host-contract: LABEL.wait: provider threw sensor offline",
            "panic: host-contract: package host: provider LABEL.wait rejected: sensor offline",
        ]
    );
}

companion!(bench, "bench-surface");

/// hdru's renderer boundary: `apply!` takes a list of commands with named
/// payloads, strings, numbers and optionals and answers at once;
/// `next_event!` answers through a pending call.
const SURFACE: &str = "pub enum Command:
    Text(id: u32, text: string)
    Move(id: u32, x: f64, y: f64)
    Style(id: u32, color: string?, bold: bool)

pub enum UiEvent:
    Click(id: u32)
    Key(code: u32)

@host
pub trait Surface:
    fn apply!(&mut self, commands: List[Command]) -> void
    fn next_event!(&mut self) -> UiEvent

pub fn main!() -> void $ Console + Surface:
    let i = +0
    while i < READY:
        $.use(Surface).apply!([Command.Text(1, \"hello\"), Command.Move(1, 2.0, 3.0), Command.Style(1, .Some(\"red\"), true)])
        i += 1
    let clicks = +0
    let j = +0
    while j < PENDING:
        match $.use(Surface).next_event!():
            .Click(_) => clicks += 1
            .Key(_) => pass
        j += 1
    println(\"$i $clicks\")
";

const SURFACE_JS: &str = "export default {
  apply(commands: unknown[]) {
    if (commands.length !== 3) throw new Error(\"three commands\");
  },
  async next_event() {
    return { tag: \"Click\", fields: { id: 1 } };
  },
};
";

struct Renderer;

impl bench::Surface for Renderer {
    fn apply(&mut self, commands: Vec<bench::Command>, _: CallContext<'_>) -> NativeCall<()> {
        assert_eq!(commands.len(), 3, "three commands");
        NativeCall::Ready(Ok(()))
    }

    fn next_event(&mut self, _: CallContext<'_>) -> NativeCall<bench::UiEvent> {
        settled(bench::UiEvent::Click { id: 1 })
    }
}

/// The hdru-shaped package with `ready` ready calls and `pending` pending
/// ones, built for Wasmtime with `args`.
fn surface_package(name: &str, ready: u32, pending: u32) -> Scratch {
    let root = Scratch::new(env!("CARGO_TARGET_TMPDIR"), name);
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::write(
        root.join("hd.toml"),
        "[package]\nname = \"bench_surface\"\n\n[host]\n\"main.Surface\" = \"provider.ts\"\n\n[capabilities]\nConsole = true\n\"pkg.main.Surface\" = true\n",
    )
    .expect("hd.toml");
    std::fs::write(root.join("provider.ts"), SURFACE_JS).expect("provider");
    let source = SURFACE
        .replace("READY", &format!("+{ready}"))
        .replace("PENDING", &format!("+{pending}"));
    std::fs::write(root.join("src/main.hd"), source).expect("main");
    root
}

fn surface_providers() -> Vec<Provider> {
    let p: Box<dyn bench::Surface> = Box::new(Renderer);
    vec![("pkg.main.Surface", provider(p), bench::dispatch_Surface)]
}

/// The hdru-shaped surface matches Node at a few calls of each form.
#[test]
fn an_hdru_shaped_surface_answers_ready_and_pending_calls_alike() {
    let built = built(surface_package("parity-surface", 3, 2), "bench_surface");
    snapshot("bench-surface", &built.contract);
    let run = same(&built, surface_providers);
    assert_eq!(run.stdout, "3 2\n");
    // module.host.package.contract-source-names and cli.host.package.native-source-names:
    // changing only declaration names must invalidate the contract metadata too.
    let source = built.root.join("src/main.hd");
    let renamed = std::fs::read_to_string(&source)
        .unwrap()
        .replace("Command", "Action")
        .replace("UiEvent", "Input")
        .replace("commands", "batch");
    std::fs::write(source, renamed).unwrap();
    let renamed = self::built(built.root, "bench_surface");
    let surface = renamed
        .contract
        .interfaces
        .iter()
        .find(|i| i.identity.trait_path == "main.Surface")
        .unwrap();
    let apply = surface
        .methods
        .iter()
        .find(|m| m.member == "apply")
        .unwrap();
    assert_eq!(apply.params[0].name, "batch");
    assert!(
        matches!(&apply.params[0].value, hd_host_abi::schema::Value::List(item)
        if matches!(item.as_ref(), hd_host_abi::schema::Value::Enum(name, _) if name == "Action"))
    );
    let rust = hd_host_abi::native_rust::generate(&renamed.contract).unwrap();
    assert!(rust.contains("pub enum Action {"));
    assert!(rust.contains("pub enum Input {"));
    assert!(rust.contains("fn apply(&mut self, batch: Vec<Action>"));
}

fn median(mut samples: Vec<f64>) -> f64 {
    samples.sort_stable_by(f64::total_cmp);
    samples[samples.len() / 2]
}

/// The A/B of #2083, run by hand on a release build:
/// `cargo test --release -p hd_command --features wasmtime --test all
/// native_parity::measure -- --ignored --nocapture`. Each engine runs the
/// same release `.wasm` with no calls, `READY` ready calls and `PENDING` pending
/// calls, alternating engines, and reports each form's median cost per
/// call over the no-call run. Then `hd build --release` of the package
/// is timed with (`--engine wasmtime`) and without (`--engine node`) the
/// Rust companion, alternating.
#[test]
#[ignore = "a measurement, run by hand on a release build"]
fn measure_call_cost_and_companion_build_time() {
    // Node's pending calls cost about a millisecond each, its ready ones
    // microseconds: each form gets enough calls to stand over the noise of
    // Node's start.
    const READY: u32 = 200_000;
    const PENDING: u32 = 2_000;
    const RUNS: usize = 7;
    let forms = [("none", 0, 0), ("ready", READY, 0), ("pending", 0, PENDING)];
    let mut times: Vec<(Vec<f64>, Vec<f64>)> = vec![(Vec::new(), Vec::new()); forms.len()];
    let mut built_forms = Vec::new();
    for (name, ready, pending) in forms {
        let root = surface_package(&format!("measure-{name}"), ready, pending);
        let out = hd(&root, &["build", "--release", "--engine", "wasmtime"]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let wasm = std::fs::read(root.join("build/release/bench_surface.wasm")).expect("wasm");
        let contract = hd_host_abi::schema::from_wasm(&wasm)
            .expect("contract")
            .expect("contract");
        built_forms.push(Built {
            root,
            stem: "bench_surface",
            wasm,
            contract,
        });
    }
    for _ in 0..RUNS {
        for (n, b) in built_forms.iter().enumerate() {
            let start = std::time::Instant::now();
            let out = hd(
                &b.root,
                &["build/release/bench_surface.wasm", "--engine", "node"],
            );
            times[n].0.push(start.elapsed().as_secs_f64());
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            let start = std::time::Instant::now();
            let run = wasmtime(b, surface_providers());
            times[n].1.push(start.elapsed().as_secs_f64());
            assert_eq!(run.status, 0, "{}", run.stderr);
        }
    }
    let (node0, native0) = (median(times[0].0.clone()), median(times[0].1.clone()));
    eprintln!(
        "no calls: node {:.1} ms, wasmtime {:.1} ms",
        node0 * 1e3,
        native0 * 1e3
    );
    for (n, (name, ready, pending)) in forms.iter().enumerate().skip(1) {
        let calls = f64::from(ready + pending);
        let node = (median(times[n].0.clone()) - node0) / calls * 1e9;
        let native = (median(times[n].1.clone()) - native0) / calls * 1e9;
        eprintln!("{name}: node {node:.0} ns/call, wasmtime {native:.0} ns/call");
    }
    let root = &built_forms[0].root;
    let (mut with, mut without) = (Vec::new(), Vec::new());
    let main = std::fs::read_to_string(root.join("src/main.hd")).expect("main");
    for run in 0..RUNS {
        for (engine, into) in [("wasmtime", &mut with), ("node", &mut without)] {
            // A new string literal each build, so the module recompiles.
            let text = main.replace("\"hello\"", &format!("\"hello {run} {engine}\""));
            std::fs::write(root.join("src/main.hd"), text).expect("main");
            let start = std::time::Instant::now();
            let out = hd(root, &["build", "--release", "--engine", engine]);
            into.push(start.elapsed().as_secs_f64());
            assert!(out.status.success());
        }
    }
    eprintln!(
        "build --release: with companion {:.1} ms, without {:.1} ms",
        median(with) * 1e3,
        median(without) * 1e3
    );
}
