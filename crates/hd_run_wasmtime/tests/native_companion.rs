//! The Rust companion of a module requiring package host traits
//! (`cli.host.package.native-generated`, `native-deterministic`) and the
//! embedder surface it compiles against (`native-registration`,
//! `native-completion-forms`, `native-abort`, `native-late-completion`).
//!
//! The checked-in companions under `tests/native/` are the generator's
//! snapshots and are also compiled here, so a generator change that breaks
//! the embedder API fails this test. `HD_BLESS=1` rewrites them.
mod support;

use std::cell::Cell;
use std::rc::Rc;

use hd_cache::MemoryStore;
use hd_host_abi::schema::{Contract, Field, Identity, Interface, Method, Value, Variant};
use hd_run::{Engine, Grants, HostSetup, Instance, Limits, Outcome, StartError};
use hd_run_wasmtime::native::{
    self, CallContext, Completion, HostError, HostResult, NativeCall, Serde,
};
use hd_run_wasmtime::{EngineConfig, HostIo, WasmtimeEngine};

#[allow(dead_code, clippy::all, clippy::pedantic)]
mod surface {
    include!("native/surface.host.rs");
}

#[allow(dead_code, clippy::all, clippy::pedantic)]
mod forms {
    include!("native/forms.host.rs");
}

fn int(bits: u8, signed: bool) -> Value {
    Value::Int { bits, signed }
}

fn field(name: &str, value: Value) -> Field {
    Field {
        name: name.into(),
        value,
    }
}

fn variant(name: &str, fields: Vec<Field>) -> Variant {
    Variant {
        name: name.into(),
        fields,
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

fn interface(trait_path: &str, module: &str, handle: bool, methods: Vec<Method>) -> Interface {
    Interface {
        identity: Identity {
            package: "example.org/ui-tui".into(),
            version: "0.3.0".into(),
            trait_path: trait_path.into(),
        },
        module: module.into(),
        selectors: if handle {
            vec![]
        } else {
            vec![format!("dep.ui_tui.{trait_path}")]
        },
        handle,
        methods,
    }
}

/// Hdru's renderer boundary: `apply!(&mut self, commands: List[Command])`
/// and `next_event!(&mut self) -> UiEvent`, an enum with named payload
/// fields.
fn surface_contract() -> Contract {
    let style = Value::Record(vec![
        field("fg", Value::Option(Box::new(int(32, false)))),
        field("bold", Value::Bool),
    ]);
    let command = Value::Enum(vec![
        variant(
            "Text",
            vec![field("node", int(64, false)), field("text", Value::String)],
        ),
        variant(
            "Style",
            vec![field("node", int(64, false)), field("style", style)],
        ),
        variant("Remove", vec![field("node", int(64, false))]),
    ]);
    let event = Value::Enum(vec![
        variant(
            "Key",
            vec![field("code", Value::String), field("ctrl", Value::Bool)],
        ),
        variant(
            "Resize",
            vec![
                field("width", int(16, false)),
                field("height", int(16, false)),
            ],
        ),
        variant("Click", vec![field("node", int(64, false))]),
        variant("Quit", vec![]),
    ]);
    Contract {
        interfaces: vec![interface(
            "surface.Surface",
            "hd:pkg:surface",
            false,
            vec![
                method(
                    "apply",
                    vec![Value::List(Box::new(command))],
                    Value::Void,
                    false,
                ),
                method("next_event", vec![], event, true),
            ],
        )],
    }
}

/// Every value form, Rust keyword names, two traits with one final name,
/// a shared record shape, recursion through a list and an option, a
/// handle-holding record, and a `never` result.
fn forms_contract() -> Contract {
    let point = || Value::Record(vec![field("x", Value::F64), field("y", Value::F64)]);
    let tree = Value::Rec(
        0,
        Box::new(Value::Enum(vec![
            variant("Leaf", vec![]),
            variant(
                "Node",
                vec![
                    field("kids", Value::List(Box::new(Value::Ref(0)))),
                    field("parent", Value::Option(Box::new(Value::Ref(0)))),
                ],
            ),
        ])),
    );
    let all = vec![
        Value::Void,
        Value::Bool,
        Value::Char,
        int(8, true),
        int(16, true),
        int(32, true),
        int(64, true),
        int(8, false),
        int(64, false),
        Value::F32,
        Value::F64,
        Value::String,
        Value::Bytes,
        Value::List(Box::new(Value::String)),
        Value::Option(Box::new(Value::Json)),
        Value::Tuple(vec![Value::Bool]),
        Value::Tuple(vec![Value::Void, int(32, false)]),
        Value::Map(Box::new(Value::String), Box::new(point())),
        Value::Result(Box::new(point()), Box::new(Value::String)),
        Value::Record(vec![
            field("type", Value::String),
            field("self", Value::Bool),
            field("at", point()),
        ]),
        Value::Enum(vec![
            variant("Self", vec![]),
            variant("Moved", vec![field("match", point())]),
        ]),
        tree,
        Value::Record(vec![field("events", Value::Handle("hd:pkg:events".into()))]),
        // The same handle record again, inside another: both derive nothing.
        Value::Record(vec![field(
            "inner",
            Value::Record(vec![field("events", Value::Handle("hd:pkg:events".into()))]),
        )]),
    ];
    Contract {
        interfaces: vec![
            // Takes `Surface2`, so the second `Surface` becomes `Surface3`.
            interface("dialog.Surface2", "hd:pkg:dialog", true, vec![]),
            // A prelude type's name: `String2`, never shadowing `String`.
            interface("text.String", "hd:pkg:text", true, vec![]),
            interface(
                "screen.Surface",
                "hd:pkg:screen",
                false,
                vec![
                    method("draw", all, Value::Handle("hd:pkg:events".into()), true),
                    method("type", vec![], Value::Never, false),
                ],
            ),
            interface(
                "window.Surface",
                "hd:pkg:window",
                false,
                vec![method("close", vec![], Value::Void, false)],
            ),
            interface(
                "screen.Events",
                "hd:pkg:events",
                true,
                vec![method(
                    "next",
                    vec![],
                    Value::Option(Box::new(Value::String)),
                    true,
                )],
            ),
        ],
    }
}

fn snapshot(name: &str, contract: &Contract) {
    let text = hd_host_abi::native_rust::generate(contract).unwrap();
    assert_eq!(
        text,
        hd_host_abi::native_rust::generate(contract).unwrap(),
        "two generations of one contract differ"
    );
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/native")
        .join(name);
    if std::env::var_os("HD_BLESS").is_some() {
        std::fs::write(&path, &text).unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        text,
        "{} is stale; rerun with HD_BLESS=1",
        path.display()
    );
}

#[test]
fn the_companion_of_hdrus_surface_is_its_snapshot() {
    snapshot("surface.host.rs", &surface_contract());
}

#[test]
fn the_companion_of_every_value_form_is_its_snapshot() {
    snapshot("forms.host.rs", &forms_contract());
}

#[test]
fn a_module_without_package_host_traits_has_no_companion() {
    assert_eq!(
        hd_host_abi::native_rust::generate(&Contract::default()).unwrap(),
        ""
    );
    let mut bad = surface_contract();
    bad.interfaces[0].methods[0].params = vec![Value::Ref(9)];
    assert!(hd_host_abi::native_rust::generate(&bad).is_err());
    // A trait path's last segment names the Rust trait.
    let mut nameless = surface_contract();
    nameless.interfaces[0].identity.trait_path = "surface.".into();
    assert_eq!(
        hd_host_abi::native_rust::generate(&nameless),
        Err("package host trait path ends in an empty segment")
    );
}

#[test]
fn the_companion_embeds_its_contract() {
    assert_eq!(
        Contract::decode(surface::CONTRACT).unwrap(),
        surface_contract()
    );
}

/// A renderer whose window state is not `Send`, as a GUI's is.
struct Renderer {
    applied: Rc<Cell<usize>>,
    events: Vec<surface::SurfaceNextEventResult>,
}

impl surface::Surface for Renderer {
    fn apply(&mut self, commands: Vec<surface::SurfaceApplyArg0Item>) -> HostResult<()> {
        self.applied.set(self.applied.get() + commands.len());
        Ok(())
    }

    fn next_event(&mut self, _cx: CallContext<'_>) -> NativeCall<surface::SurfaceNextEventResult> {
        match self.events.pop() {
            Some(event) => NativeCall::Ready(Ok(event)),
            None => NativeCall::Ready(Err(HostError("no event".into()))),
        }
    }
}

#[test]
fn the_registry_holds_each_root_provider_by_value() {
    use surface::{Surface, SurfaceApplyArg0Item as Command, SurfaceNextEventResult as Event};
    let Err(missing) = surface::Bindings::builder().build() else {
        panic!("a root trait without a provider must be reported")
    };
    assert_eq!(
        missing.to_string(),
        "no native provider registered for package host trait \"example.org/ui-tui@0.3.0/surface.Surface\""
    );
    let applied = Rc::new(Cell::new(0));
    let bindings = surface::Bindings::builder()
        .surface(Renderer {
            applied: Rc::clone(&applied),
            events: vec![Event::Resize {
                width: 80,
                height: 24,
            }],
        })
        .build()
        .unwrap();
    assert_eq!(bindings.contract(), surface::CONTRACT);
    let [registration] = bindings.registrations() else {
        panic!("one root trait, one registration")
    };
    assert_eq!(
        registration.identity,
        "example.org/ui-tui@0.3.0/surface.Surface"
    );
    assert_eq!(registration.module, "hd:pkg:surface");
    let mut provider = bindings
        .into_registrations()
        .pop()
        .unwrap()
        .provider
        .downcast::<Box<dyn Surface>>()
        .expect("the registered trait object");
    provider
        .apply(vec![
            Command::Text {
                node: 1,
                text: "hi".into(),
            },
            Command::Style {
                node: 1,
                style: surface::SurfaceApplyArg0ItemStyleStyle {
                    fg: Some(0xff_00_00),
                    bold: true,
                },
            },
            Command::Remove { node: u64::MAX },
        ])
        .unwrap();
    assert_eq!(applied.get(), 3);
    let NativeCall::Ready(Ok(Event::Resize {
        width: 80,
        height: 24,
    })) = provider.next_event(CallContext::new())
    else {
        panic!("the ready event")
    };
    let NativeCall::Ready(Err(e)) = provider.next_event(CallContext::default()) else {
        panic!("a provider failure")
    };
    assert_eq!(e.to_string(), "no event");
}

#[test]
fn every_root_trait_of_a_companion_gets_a_setter() {
    struct Screen;
    impl forms::Surface for Screen {
        #[allow(clippy::too_many_arguments)]
        fn draw(
            &mut self,
            (): (),
            _: bool,
            _: char,
            _: i8,
            _: i16,
            _: i32,
            _: i64,
            _: u8,
            _: u64,
            _: f32,
            _: f64,
            _: String,
            _: Vec<u8>,
            _: Vec<String>,
            _: Option<Serde>,
            _: (bool,),
            _: ((), u32),
            _: Vec<(String, forms::SurfaceDrawArg17Value)>,
            _: Result<forms::SurfaceDrawArg17Value, String>,
            _: forms::SurfaceDrawArg19,
            _: forms::SurfaceDrawArg20,
            tree: forms::SurfaceDrawArg21,
            _: forms::SurfaceDrawArg22,
            _: forms::SurfaceDrawArg23,
            _cx: CallContext<'_>,
        ) -> NativeCall<Box<dyn forms::Events>> {
            let forms::SurfaceDrawArg21::Node { kids, parent } = tree else {
                return NativeCall::Ready(Err(HostError("leaf".into())));
            };
            assert_eq!(kids, vec![forms::SurfaceDrawArg21::Leaf]);
            assert_eq!(parent, Some(Box::new(forms::SurfaceDrawArg21::Leaf)));
            NativeCall::Ready(Err(HostError("node".into())))
        }
        fn r#type(&mut self) -> HostResult<std::convert::Infallible> {
            Err(HostError("never returns".into()))
        }
    }
    struct Window;
    impl forms::Surface3 for Window {
        fn close(&mut self) -> HostResult<()> {
            Ok(())
        }
    }
    let Err(missing) = forms::Bindings::builder().surface(Screen).build() else {
        panic!("the second root trait has no provider")
    };
    assert_eq!(missing.0, "example.org/ui-tui@0.3.0/window.Surface");
    let bindings = forms::Bindings::builder()
        .surface(Screen)
        .surface3(Window)
        .build()
        .unwrap();
    let modules: Vec<_> = bindings.registrations().iter().map(|r| r.module).collect();
    assert_eq!(modules, ["hd:pkg:screen", "hd:pkg:window"]);
    let record = forms::SurfaceDrawArg19 {
        r#type: "t".into(),
        self_: true,
        at: forms::SurfaceDrawArg17Value { x: -0.0, y: 1.5 },
    };
    assert_eq!(record.clone(), record);
    assert_eq!(
        forms::SurfaceDrawArg20::Moved {
            r#match: record.at.clone()
        },
        forms::SurfaceDrawArg20::Moved { r#match: record.at }
    );
    let _ = forms::SurfaceDrawArg20::Self_;
}

#[test]
fn a_pending_call_completes_once_from_another_thread() {
    let aborted = Rc::new(Cell::new(0));
    let hook = Rc::clone(&aborted);
    let (mut call, completion) = native::pending::<String>(move || hook.set(hook.get() + 1));
    assert!(call.take().is_none(), "still pending");
    std::thread::spawn(move || completion.complete(Ok("event".into())))
        .join()
        .unwrap();
    assert_eq!(call.take(), Some(Ok("event".into())));
    assert!(call.take().is_none(), "a value is delivered once");
    call.abort();
    assert_eq!(aborted.get(), 0, "a completed call has no abort to run");
}

#[test]
fn an_aborted_call_runs_its_hook_once_and_ignores_a_late_completion() {
    let aborted = Rc::new(Cell::new(0));
    let captured = Rc::new(());
    let (hook, held) = (Rc::clone(&aborted), Rc::clone(&captured));
    let (mut call, completion): (_, Completion<u32>) = native::pending(move || {
        drop(held);
        hook.set(hook.get() + 1);
    });
    call.abort();
    call.abort();
    assert_eq!(aborted.get(), 1);
    assert_eq!(
        Rc::strong_count(&captured),
        1,
        "the hook's captures are released"
    );
    completion.complete(Ok(7));
    assert!(call.take().is_none(), "a late completion is ignored");
}

fn setup() -> HostSetup {
    HostSetup {
        providers: vec![],
        grants: Grants::default(),
        limits: Limits::default(),
        args: vec![],
        program: String::new(),
    }
}

#[test]
fn instantiation_takes_the_registry_by_value() {
    let engine = WasmtimeEngine::new(EngineConfig::default()).unwrap();
    let (wasm, _) = support::Program::new().finish(5);
    let module = engine.load(&wasm, &MemoryStore::default()).unwrap();
    let mut instance = engine
        .instantiate_with_bindings(
            &module,
            &setup(),
            HostIo::process(),
            native::Bindings::new(&[]),
        )
        .unwrap();
    assert_eq!(instance.init(), Outcome::Exit(0));
    assert_eq!(instance.poll(), std::task::Poll::Ready(Outcome::Exit(5)));

    // This module has no package host contract, so a provider names no
    // trait it requires: refused before the guest starts.
    let bindings = surface::Bindings::builder()
        .surface(Renderer {
            applied: Rc::new(Cell::new(0)),
            events: vec![],
        })
        .build()
        .unwrap();
    let Err(StartError::Refused(message)) =
        engine.instantiate_with_bindings(&module, &setup(), HostIo::process(), bindings)
    else {
        panic!("an unrequired provider must be refused")
    };
    assert_eq!(
        message,
        "native provider for package host trait \"example.org/ui-tui@0.3.0/surface.Surface\" names no trait this module requires"
    );
}
