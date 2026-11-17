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
use hd_run::{
    Consent, Engine, GrantValue, Grants, HostSetup, Instance, Limits, Outcome, StartError,
};
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

#[allow(dead_code, clippy::all, clippy::pedantic)]
mod values {
    include!("native/values.host.rs");
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
                    method("halt", vec![], Value::Never, true),
                ],
            ),
            interface(
                "window.Surface",
                "hd:pkg:window",
                false,
                vec![
                    method("close", vec![], Value::Void, false),
                    method("flush", vec![], Value::Void, true),
                ],
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

/// `example.org/ui-tui@0.3.0/values.Values`, the codec contract: `make`
/// returns a record with a field of every value form and `take` takes the
/// same forms as its arguments, so `make`'s result bytes are `take`'s
/// argument bytes. Two records of one Rust shape (`data: Vec<u8>`) differ
/// only in encoding: bytes, and a list of `u8`.
fn values_contract() -> Contract {
    let point = || Value::Record(vec![field("x", Value::F64), field("y", Value::F64)]);
    let tree = Value::Rec(
        0,
        Box::new(Value::Enum(vec![
            variant("Leaf", vec![field("value", int(32, true))]),
            variant(
                "Node",
                vec![
                    field("kids", Value::List(Box::new(Value::Ref(0)))),
                    field("parent", Value::Option(Box::new(Value::Ref(0)))),
                ],
            ),
        ])),
    );
    let forms = vec![
        ("unit", Value::Void),
        ("flag", Value::Bool),
        ("letter", Value::Char),
        ("tiny", int(8, true)),
        ("small", int(16, true)),
        ("medium", int(32, true)),
        ("large", int(64, true)),
        ("byte", int(8, false)),
        ("word", int(16, false)),
        ("dword", int(32, false)),
        ("qword", int(64, false)),
        ("single", Value::F32),
        ("double", Value::F64),
        ("text", Value::String),
        ("blob", Value::Bytes),
        ("octets", Value::List(Box::new(int(8, false)))),
        ("words", Value::List(Box::new(Value::String))),
        ("maybe", Value::Option(Box::new(int(64, true)))),
        (
            "outcome",
            Value::Result(Box::new(point()), Box::new(Value::String)),
        ),
        ("pair", Value::Tuple(vec![Value::Bool, Value::Void])),
        ("one", Value::Tuple(vec![Value::String])),
        (
            "table",
            Value::Map(Box::new(Value::String), Box::new(int(32, true))),
        ),
        (
            "person",
            Value::Record(vec![
                field("name", Value::String),
                field("at", point()),
                field("trail", Value::List(Box::new(point()))),
            ]),
        ),
        (
            "event",
            Value::Enum(vec![
                variant("Quit", vec![]),
                variant(
                    "Resize",
                    vec![
                        field("width", int(32, false)),
                        field("height", int(32, false)),
                    ],
                ),
            ]),
        ),
        (
            "only",
            Value::Enum(vec![variant("Ping", vec![field("id", int(8, false))])]),
        ),
        ("tree", tree),
        ("private", Value::Json),
        ("bin", Value::Record(vec![field("data", Value::Bytes)])),
        (
            "nums",
            Value::Record(vec![field("data", Value::List(Box::new(int(8, false))))]),
        ),
        // An enum without variants has no value, so only `None` crosses.
        ("nothing", Value::Option(Box::new(Value::Enum(vec![])))),
        ("empty", Value::Record(vec![])),
    ];
    let record = Value::Record(forms.iter().map(|(n, v)| field(n, v.clone())).collect());
    let params = forms.into_iter().map(|(_, v)| v).collect();
    Contract {
        interfaces: vec![interface(
            "values.Values",
            "hd:pkg:values",
            false,
            vec![
                method("make", vec![], record, false),
                method("take", params, Value::Void, false),
                method("text", vec![Value::String], Value::String, false),
                method("bad", vec![], Value::Json, false),
                method("stop", vec![], Value::Never, false),
            ],
        )],
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
fn the_companion_of_every_codec_is_its_snapshot() {
    snapshot("values.host.rs", &values_contract());
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
        fn halt(&mut self, _cx: CallContext<'_>) -> NativeCall<std::convert::Infallible> {
            NativeCall::Pending(native::pending(|| {}).0)
        }
    }
    struct Window;
    impl forms::Surface3 for Window {
        fn close(&mut self) -> HostResult<()> {
            Ok(())
        }
        fn flush(&mut self, _cx: CallContext<'_>) -> NativeCall<()> {
            NativeCall::Ready(Ok(()))
        }
    }
    // A suspending `()` method's ready answer is empty; a suspending
    // `never` method can only stay pending.
    let mut window: Box<dyn std::any::Any> = Box::new(Box::new(Window) as Box<dyn forms::Surface3>);
    let Ok(native::Answer::Ready(empty)) = forms::dispatch_Surface3(window.as_mut(), 1, &[]) else {
        panic!("`flush` answers at once")
    };
    assert!(empty.is_empty());
    let mut screen: Box<dyn std::any::Any> = Box::new(Box::new(Screen) as Box<dyn forms::Surface>);
    let Ok(native::Answer::Pending(_)) = forms::dispatch_Surface(screen.as_mut(), 2, &[]) else {
        panic!("`halt` stays pending")
    };
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

/// A run whose app manifest consents to both companions' root traits
/// (`cli.host.package.native-two-keys`).
fn setup() -> HostSetup {
    let table: Vec<(String, GrantValue)> =
        ["dep.ui_tui.surface.Surface", "dep.ui_tui.values.Values"]
            .into_iter()
            .map(|path| (path.to_owned(), GrantValue::All))
            .collect();
    HostSetup {
        providers: vec![],
        grants: Grants {
            consent: Consent::new(&table, &[]),
            ..Grants::default()
        },
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

/// A `Values` provider: `make` returns `made`, `take` records each
/// argument's `Debug` text.
struct Codecs {
    made: values::ValuesMakeResult,
    taken: Rc<std::cell::RefCell<Vec<String>>>,
}

impl values::Values for Codecs {
    fn make(&mut self) -> HostResult<values::ValuesMakeResult> {
        Ok(self.made.clone())
    }

    #[allow(clippy::too_many_arguments, clippy::many_single_char_names)]
    fn take(
        &mut self,
        a0: (),
        a1: bool,
        a2: char,
        a3: i8,
        a4: i16,
        a5: i32,
        a6: i64,
        a7: u8,
        a8: u16,
        a9: u32,
        a10: u64,
        a11: f32,
        a12: f64,
        a13: String,
        a14: Vec<u8>,
        a15: Vec<u8>,
        a16: Vec<String>,
        a17: Option<i64>,
        a18: Result<values::ValuesMakeResultOutcomeOk, String>,
        a19: (bool, ()),
        a20: (String,),
        a21: Vec<(String, i32)>,
        a22: values::ValuesMakeResultPerson,
        a23: values::ValuesMakeResultEvent,
        a24: values::ValuesMakeResultOnly,
        a25: values::ValuesTakeArg25,
        a26: Serde,
        a27: values::ValuesMakeResultBin,
        a28: values::ValuesMakeResultNums,
        a29: Option<values::ValuesMakeResultNothing>,
        a30: values::ValuesMakeResultEmpty,
    ) -> HostResult<()> {
        *self.taken.borrow_mut() = vec![
            format!("{a0:?}"),
            format!("{a1:?}"),
            format!("{a2:?}"),
            format!("{a3:?}"),
            format!("{a4:?}"),
            format!("{a5:?}"),
            format!("{a6:?}"),
            format!("{a7:?}"),
            format!("{a8:?}"),
            format!("{a9:?}"),
            format!("{a10:?}"),
            format!("{a11:?}"),
            format!("{a12:?}"),
            format!("{a13:?}"),
            format!("{a14:?}"),
            format!("{a15:?}"),
            format!("{a16:?}"),
            format!("{a17:?}"),
            format!("{a18:?}"),
            format!("{a19:?}"),
            format!("{a20:?}"),
            format!("{a21:?}"),
            format!("{a22:?}"),
            format!("{a23:?}"),
            format!("{a24:?}"),
            format!("{a25:?}"),
            format!("{a26:?}"),
            format!("{a27:?}"),
            format!("{a28:?}"),
            format!("{a29:?}"),
            format!("{a30:?}"),
        ];
        Ok(())
    }

    fn text(&mut self, text: String) -> HostResult<String> {
        Ok(text.to_uppercase())
    }

    fn bad(&mut self) -> HostResult<Serde> {
        Ok(Serde::Map(vec![(Serde::I64(1), Serde::Unit)]))
    }

    fn stop(&mut self) -> HostResult<std::convert::Infallible> {
        Err(HostError("stopped".into()))
    }
}

/// Each field of `made`, as `take` records its arguments.
fn fields_of(made: &values::ValuesMakeResult) -> Vec<String> {
    let m = made;
    vec![
        format!("{:?}", m.unit),
        format!("{:?}", m.flag),
        format!("{:?}", m.letter),
        format!("{:?}", m.tiny),
        format!("{:?}", m.small),
        format!("{:?}", m.medium),
        format!("{:?}", m.large),
        format!("{:?}", m.byte),
        format!("{:?}", m.word),
        format!("{:?}", m.dword),
        format!("{:?}", m.qword),
        format!("{:?}", m.single),
        format!("{:?}", m.double),
        format!("{:?}", m.text),
        format!("{:?}", m.blob),
        format!("{:?}", m.octets),
        format!("{:?}", m.words),
        format!("{:?}", m.maybe),
        format!("{:?}", m.outcome),
        format!("{:?}", m.pair),
        format!("{:?}", m.one),
        format!("{:?}", m.table),
        format!("{:?}", m.person),
        format!("{:?}", m.event),
        format!("{:?}", m.only),
        format!("{:?}", m.tree),
        format!("{:?}", m.private),
        format!("{:?}", m.bin),
        format!("{:?}", m.nums),
        format!("{:?}", m.nothing),
        format!("{:?}", m.empty),
    ]
}

fn made(first: bool) -> values::ValuesMakeResult {
    use values::{
        ValuesMakeResultBin as Bin, ValuesMakeResultEvent as Event, ValuesMakeResultNums as Nums,
        ValuesMakeResultOnly as Only, ValuesMakeResultOutcomeOk as Point,
        ValuesMakeResultPerson as Person, ValuesMakeResultTree as Tree,
    };
    let point = |x, y| Point { x, y };
    values::ValuesMakeResult {
        unit: (),
        flag: first,
        letter: if first { '字' } else { '\0' },
        tiny: if first { i8::MIN } else { i8::MAX },
        small: -2,
        medium: if first { i32::MAX } else { i32::MIN },
        large: if first { i64::MIN } else { i64::MAX },
        byte: u8::MAX,
        word: u16::MAX,
        dword: u32::MAX,
        qword: u64::MAX,
        single: if first { -0.0 } else { f32::MAX },
        double: if first { -0.0 } else { f64::MIN_POSITIVE },
        text: if first {
            "héllo\n\"".into()
        } else {
            String::new()
        },
        blob: vec![0, 255],
        octets: vec![200, 1],
        words: if first {
            vec!["a".into(), String::new()]
        } else {
            vec![]
        },
        maybe: first.then_some(i64::MAX),
        outcome: if first {
            Ok(point(1.5, -2.0))
        } else {
            Err("no".into())
        },
        pair: (!first, ()),
        one: ("solo".into(),),
        // Map order is kept, not sorted.
        table: vec![("b".into(), 2), ("a".into(), -1)],
        person: Person {
            name: "Ada".into(),
            at: point(0.0, 1.0),
            trail: vec![point(2.0, 3.0), point(-0.0, f64::MAX)],
        },
        event: if first {
            Event::Resize {
                width: 80,
                height: 24,
            }
        } else {
            Event::Quit
        },
        only: Only::Ping { id: 7 },
        tree: if first {
            Tree::Node {
                kids: vec![Tree::Leaf { value: 1 }, Tree::Leaf { value: -1 }],
                parent: Some(Box::new(Tree::Leaf { value: 3 })),
            }
        } else {
            Tree::Node {
                kids: vec![],
                parent: None,
            }
        },
        private: if first {
            Serde::Map(vec![
                (
                    Serde::String("name".into()),
                    Serde::String("x\u{1}\"\\é😀".into()),
                ),
                (Serde::String("n".into()), Serde::I64(i64::MIN)),
                (Serde::String("u".into()), Serde::U64(u64::MAX)),
                (Serde::String("f".into()), Serde::F64(-0.0)),
                (
                    Serde::String("xs".into()),
                    Serde::Seq(vec![Serde::Bool(true), Serde::None, Serde::Bool(false)]),
                ),
                (Serde::String("empty".into()), Serde::Map(vec![])),
                (Serde::String("seq".into()), Serde::Seq(vec![])),
            ])
        } else {
            // A private record whose custom serializer writes a string.
            Serde::String("custom".into())
        },
        bin: Bin { data: vec![200] },
        nums: Nums { data: vec![200] },
        nothing: None,
        empty: values::ValuesMakeResultEmpty {},
    }
}

type Taken = Rc<std::cell::RefCell<Vec<String>>>;

/// The `Values` dispatch, whose methods all answer at once.
fn dispatch_values(
    provider: &mut dyn std::any::Any,
    method: usize,
    args: &[u8],
) -> HostResult<Vec<u8>> {
    match values::dispatch_Values(provider, method, args)? {
        native::Answer::Ready(bytes) => Ok(bytes),
        native::Answer::Pending(_) => panic!("`Values` has no suspending method"),
    }
}

fn codecs(made: values::ValuesMakeResult) -> (Box<dyn std::any::Any>, Taken) {
    let taken = Rc::default();
    let provider: Box<dyn values::Values> = Box::new(Codecs {
        made,
        taken: Rc::clone(&taken),
    });
    (Box::new(provider), taken)
}

#[test]
fn every_value_form_round_trips_through_a_native_provider() {
    for first in [true, false] {
        let made = made(first);
        let (mut provider, taken) = codecs(made.clone());
        let bytes = dispatch_values(provider.as_mut(), 0, &[]).unwrap();
        // `bin` is bytes, its length then its bytes; `nums` a list of
        // `u8`, its count then each LEB128 value; `nothing` is `None`.
        assert!(bytes.ends_with(&[1, 200, 1, 0xc8, 1, 0]), "{bytes:?}");
        assert!(
            dispatch_values(provider.as_mut(), 1, &bytes)
                .unwrap()
                .is_empty()
        );
        assert_eq!(*taken.borrow(), fields_of(&made), "first: {first}");
    }
    let (mut provider, _) = codecs(made(true));
    // `-0.0` keeps its sign bit through both directions.
    let bytes = dispatch_values(provider.as_mut(), 0, &[]).unwrap();
    assert!(bytes.windows(8).any(|w| w == (-0.0_f64).to_le_bytes()));
}

#[test]
fn a_single_string_argument_is_its_bare_utf8() {
    let (mut provider, _) = codecs(made(true));
    let mut expected = native::wire::Encoder::default();
    expected.string("HÉ");
    assert_eq!(
        dispatch_values(provider.as_mut(), 2, "hé".as_bytes()).unwrap(),
        expected.into_bytes()
    );
}

#[test]
fn a_malformed_value_fails_the_call_with_the_js_hosts_message() {
    let (mut provider, _) = codecs(made(true));
    let fail = |provider: &mut Box<dyn std::any::Any>, method, args: &[u8]| {
        dispatch_values(provider.as_mut(), method, args)
            .unwrap_err()
            .0
    };
    assert_eq!(
        fail(&mut provider, 3, &[]),
        "package host: invalid private object key"
    );
    assert_eq!(fail(&mut provider, 4, &[]), "stopped");
    let bytes = dispatch_values(provider.as_mut(), 0, &[]).unwrap();
    assert_eq!(
        fail(&mut provider, 1, &bytes[..bytes.len() - 1]),
        "an argument does not decode"
    );
    assert_eq!(
        fail(&mut provider, 1, &[bytes.as_slice(), &[0]].concat()),
        "package host: trailing argument bytes"
    );
    // `nothing` as `Some`: an enum without variants has no tag.
    let some = [&bytes[..bytes.len() - 1], &[1, 0]].concat();
    assert_eq!(fail(&mut provider, 1, &some), "an argument does not decode");
    assert_eq!(
        fail(&mut provider, 5, &[]),
        "package host: no provider method at index 5"
    );
    let mut other: Box<dyn std::any::Any> = Box::new(7_u32);
    assert_eq!(
        fail(&mut other, 0, &[]),
        "package host: the registered provider is not this dispatch's trait object"
    );
}

/// A module embedding the `Values` contract whose `hd.init` runs `body`
/// over the imports `make`, `take` and `bad`, at indices 2, 3 and 4.
fn values_module(body: &[wasm_encoder::Instruction<'static>]) -> Vec<u8> {
    use wasm_encoder::ValType;
    let mut p = support::Program::new();
    assert_eq!(p.import("hd:pkg:values", "make", &[], &[ValType::I32]), 2);
    assert_eq!(p.import("hd:pkg:values", "take", &[ValType::I32], &[]), 3);
    assert_eq!(p.import("hd:pkg:values", "bad", &[], &[ValType::I32]), 4);
    p.instructions(body);
    let (mut wasm, _) = p.finish(0);
    // The contract's custom section, which may follow every other one.
    let mut payload = support::Enc(Vec::new())
        .bytes(hd_host_abi::schema::SECTION.as_bytes())
        .0;
    payload.extend_from_slice(values::CONTRACT);
    wasm.push(0);
    wasm.extend_from_slice(&support::Enc(Vec::new()).bytes(&payload).0);
    wasm
}

fn run_values(
    body: &[wasm_encoder::Instruction<'static>],
    made: values::ValuesMakeResult,
) -> (Outcome, String, Taken) {
    let engine = WasmtimeEngine::new(EngineConfig::default()).unwrap();
    let module = engine
        .load(&values_module(body), &MemoryStore::default())
        .unwrap();
    let taken = Taken::default();
    let bindings = values::Bindings::builder()
        .values(Codecs {
            made,
            taken: Rc::clone(&taken),
        })
        .build()
        .unwrap();
    let err = Captured::default();
    let io = HostIo {
        out: Box::new(std::io::sink()),
        err: Box::new(err.clone()),
        input: None,
    };
    let mut instance = engine
        .instantiate_with_bindings(&module, &setup(), io, bindings)
        .unwrap();
    let outcome = instance.init();
    let text = String::from_utf8(err.0.lock().unwrap().clone()).unwrap();
    (outcome, text, taken)
}

/// Standard error, kept for the test to read.
#[derive(Clone, Default)]
struct Captured(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_generated_companion_answers_the_guest_through_the_engine() {
    use wasm_encoder::Instruction as I;
    // `make`'s result length is `take`'s argument length: the guest hands
    // the provider's own encoded value back to it.
    let made = made(true);
    let (outcome, error, taken) = run_values(&[I::Call(2), I::Call(3)], made.clone());
    assert_eq!(outcome, Outcome::Exit(0), "{error}");
    assert_eq!(*taken.borrow(), fields_of(&made));
}

#[test]
fn a_provider_value_failing_its_codec_is_a_host_contract_panic_naming_the_method() {
    use wasm_encoder::Instruction as I;
    let (outcome, error, _) = run_values(&[I::Call(4), I::Drop], made(false));
    assert_eq!(outcome, Outcome::Exit(3));
    assert!(
        error.starts_with(
            "panic: host-contract: example.org/ui-tui@0.3.0/values.Values.bad: package host: invalid private object key"
        ),
        "{error}"
    );
}
