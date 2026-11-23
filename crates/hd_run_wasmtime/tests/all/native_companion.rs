//! The Rust companion of a module requiring package host traits
//! (`cli.host.package.native-generated`, `native-deterministic`) and the
//! embedder surface it compiles against (`native-registration`,
//! `native-completion-forms`, `native-abort`, `native-late-completion`).
//!
//! The checked-in companions under `tests/native/` are the generator's
//! snapshots and are also compiled here, so a generator change that breaks
//! the embedder API fails this test. `HD_BLESS=1` rewrites them.
use crate::support;

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
    include!("../native/surface.host.rs");
}

#[allow(dead_code, clippy::all, clippy::pedantic)]
mod forms {
    include!("../native/forms.host.rs");
}

#[allow(dead_code, clippy::all, clippy::pedantic)]
mod values {
    include!("../native/values.host.rs");
}

#[allow(dead_code, clippy::all, clippy::pedantic)]
mod handles {
    include!("../native/handles.host.rs");
}

#[allow(dead_code, clippy::all, clippy::pedantic)]
mod deep {
    include!("../native/deep.host.rs");
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
        params: params
            .into_iter()
            .enumerate()
            .map(|(n, value)| field(&format!("p{n}"), value))
            .collect(),
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
    let style = Value::Record(
        String::new(),
        vec![
            field("fg", Value::Option(Box::new(int(32, false)))),
            field("bold", Value::Bool),
        ],
    );
    let command = Value::Enum(
        String::new(),
        vec![
            variant(
                "Text",
                vec![field("node", int(64, false)), field("text", Value::String)],
            ),
            variant(
                "Style",
                vec![field("node", int(64, false)), field("style", style)],
            ),
            variant("Remove", vec![field("node", int(64, false))]),
        ],
    );
    let event = Value::Enum(
        String::new(),
        vec![
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
        ],
    );
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
    let point = || {
        Value::Record(
            String::new(),
            vec![field("x", Value::F64), field("y", Value::F64)],
        )
    };
    let tree = Value::Rec(
        0,
        Box::new(Value::Enum(
            String::new(),
            vec![
                variant("Leaf", vec![]),
                variant(
                    "Node",
                    vec![
                        field("kids", Value::List(Box::new(Value::Ref(0)))),
                        field("parent", Value::Option(Box::new(Value::Ref(0)))),
                    ],
                ),
            ],
        )),
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
        Value::Record(
            String::new(),
            vec![
                field("type", Value::String),
                field("self", Value::Bool),
                field("at", point()),
            ],
        ),
        Value::Enum(
            String::new(),
            vec![
                variant("Self", vec![]),
                variant("Moved", vec![field("match", point())]),
            ],
        ),
        tree,
        // An anonymous union `i32 | string`: each case's one payload is
        // its field `0`.
        Value::Enum(
            String::new(),
            vec![
                variant("_0", vec![field("0", int(32, true))]),
                variant("_1", vec![field("0", Value::String)]),
            ],
        ),
        Value::Record(
            String::new(),
            vec![field("events", Value::Handle("hd:pkg:events".into()))],
        ),
        // The same handle record again, inside another: both derive nothing.
        Value::Record(
            String::new(),
            vec![field(
                "inner",
                Value::Record(
                    String::new(),
                    vec![field("events", Value::Handle("hd:pkg:events".into()))],
                ),
            )],
        ),
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
    let point = || {
        Value::Record(
            String::new(),
            vec![field("x", Value::F64), field("y", Value::F64)],
        )
    };
    let tree = Value::Rec(
        0,
        Box::new(Value::Enum(
            String::new(),
            vec![
                variant("Leaf", vec![field("value", int(32, true))]),
                variant(
                    "Node",
                    vec![
                        field("kids", Value::List(Box::new(Value::Ref(0)))),
                        field("parent", Value::Option(Box::new(Value::Ref(0)))),
                    ],
                ),
            ],
        )),
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
            Value::Record(
                String::new(),
                vec![
                    field("name", Value::String),
                    field("at", point()),
                    field("trail", Value::List(Box::new(point()))),
                ],
            ),
        ),
        (
            "event",
            Value::Enum(
                String::new(),
                vec![
                    variant("Quit", vec![]),
                    variant(
                        "Resize",
                        vec![
                            field("width", int(32, false)),
                            field("height", int(32, false)),
                        ],
                    ),
                ],
            ),
        ),
        (
            "only",
            Value::Enum(
                String::new(),
                vec![variant("Ping", vec![field("id", int(8, false))])],
            ),
        ),
        ("tree", tree),
        ("private", Value::Json),
        (
            "bin",
            Value::Record(String::new(), vec![field("data", Value::Bytes)]),
        ),
        (
            "nums",
            Value::Record(
                String::new(),
                vec![field("data", Value::List(Box::new(int(8, false))))],
            ),
        ),
        // An enum without variants has no value, so only `None` crosses.
        (
            "nothing",
            Value::Option(Box::new(Value::Enum(String::new(), vec![]))),
        ),
        ("empty", Value::Record(String::new(), vec![])),
    ];
    let record = Value::Record(
        String::new(),
        forms.iter().map(|(n, v)| field(n, v.clone())).collect(),
    );
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

/// A recursive type through every container form and a nested recursive
/// type reaching back to it: `Node` is `End`, `Next(next: Node)`,
/// `Many(items: List[Node])`, `Maybe(at: Node?)`, `Pair(pair: (Node,
/// i32))`, `Keyed(map: Map[String, Node])`, `Either(either: Result[Node,
/// String])`, `Inner(inner: Twig)` or `Bad(bad: json, rest: Node)`, where
/// `Twig` is `{ up: Node?, kids: List[Twig], tag: i32 }`; `make(depth,
/// form) -> Node` and `take(node: Node) -> u32`.
fn deep_contract() -> Contract {
    let node = Value::Rec(
        0,
        Box::new(Value::Enum(
            String::new(),
            vec![
                variant("End", vec![]),
                variant("Next", vec![field("next", Value::Ref(0))]),
                variant(
                    "Many",
                    vec![field("items", Value::List(Box::new(Value::Ref(0))))],
                ),
                variant(
                    "Maybe",
                    vec![field("at", Value::Option(Box::new(Value::Ref(0))))],
                ),
                variant(
                    "Pair",
                    vec![field(
                        "pair",
                        Value::Tuple(vec![Value::Ref(0), int(32, true)]),
                    )],
                ),
                variant(
                    "Keyed",
                    vec![field(
                        "map",
                        Value::Map(Box::new(Value::String), Box::new(Value::Ref(0))),
                    )],
                ),
                variant(
                    "Either",
                    vec![field(
                        "either",
                        Value::Result(Box::new(Value::Ref(0)), Box::new(Value::String)),
                    )],
                ),
                variant(
                    "Inner",
                    vec![field(
                        "inner",
                        Value::Rec(
                            1,
                            Box::new(Value::Record(
                                String::new(),
                                vec![
                                    field("up", Value::Option(Box::new(Value::Ref(0)))),
                                    field("kids", Value::List(Box::new(Value::Ref(1)))),
                                    field("tag", int(32, true)),
                                ],
                            )),
                        ),
                    )],
                ),
                variant(
                    "Bad",
                    vec![field("bad", Value::Json), field("rest", Value::Ref(0))],
                ),
            ],
        )),
    );
    Contract {
        interfaces: vec![interface(
            "Deep",
            "hd:pkg:deep",
            false,
            vec![
                method(
                    "make",
                    vec![int(32, false), int(32, false)],
                    node.clone(),
                    false,
                ),
                method("take", vec![node], int(32, false), false),
                // A recursive value under each outer form: each argument
                // is lent and taken apart through its own template.
                method(
                    "hold",
                    vec![
                        Value::List(Box::new(chain_type())),
                        Value::Option(Box::new(chain_type())),
                        Value::Result(Box::new(int(32, true)), Box::new(chain_type())),
                        Value::Map(Box::new(chain_type()), Box::new(chain_type())),
                        Value::Tuple(vec![chain_type(), int(32, true)]),
                        Value::Record(
                            String::new(),
                            vec![field("node", chain_type()), field("n", int(32, true))],
                        ),
                        Value::Enum(
                            String::new(),
                            vec![
                                variant("Empty", vec![]),
                                variant(
                                    "Full",
                                    vec![field("node", chain_type()), field("n", int(32, true))],
                                ),
                            ],
                        ),
                        Value::Enum(
                            String::new(),
                            vec![variant("Only", vec![field("node", chain_type())])],
                        ),
                    ],
                    int(32, false),
                    false,
                ),
            ],
        )],
    }
}

/// `enum Chain: End, Next(next: Chain)`, its own recursive group at each
/// use.
fn chain_type() -> Value {
    Value::Rec(
        2,
        Box::new(Value::Enum(
            String::new(),
            vec![
                variant("End", vec![]),
                variant("Next", vec![field("next", Value::Ref(2))]),
            ],
        )),
    )
}

/// A root trait returning handles of a handle trait whose own method
/// returns another: `pair() -> (File, File)`, `bad() -> (File, Json)`,
/// and `File`'s `size() -> u64` and `dup() -> File`.
fn handles_contract() -> Contract {
    let file = || Value::Handle("hd:pkg:file".into());
    Contract {
        interfaces: vec![
            interface(
                "files.Files",
                "hd:pkg:files",
                false,
                vec![
                    method("pair", vec![], Value::Tuple(vec![file(), file()]), false),
                    method(
                        "bad",
                        vec![],
                        Value::Tuple(vec![file(), Value::Json]),
                        false,
                    ),
                    // A recursive group holding a handle: no clone,
                    // compare or `Debug`, as a handle has none.
                    method(
                        "tree",
                        vec![],
                        Value::Rec(
                            3,
                            Box::new(Value::Enum(
                                String::new(),
                                vec![
                                    variant("Leaf", vec![field("file", file())]),
                                    variant(
                                        "Node",
                                        vec![field("kids", Value::List(Box::new(Value::Ref(3))))],
                                    ),
                                ],
                            )),
                        ),
                        false,
                    ),
                ],
            ),
            interface(
                "files.File",
                "hd:pkg:file",
                true,
                vec![
                    method("size", vec![], int(64, false), false),
                    method("dup", vec![], file(), false),
                ],
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
fn the_companion_of_every_codec_is_its_snapshot() {
    snapshot("values.host.rs", &values_contract());
}

#[test]
fn the_companion_of_returned_handles_is_its_snapshot() {
    snapshot("handles.host.rs", &handles_contract());
}

#[test]
fn the_companion_of_a_recursive_group_is_its_snapshot() {
    snapshot("deep.host.rs", &deep_contract());
}

#[test]
fn a_module_without_package_host_traits_has_no_companion() {
    assert_eq!(
        hd_host_abi::native_rust::generate(&Contract::default()).unwrap(),
        ""
    );
    let mut bad = surface_contract();
    bad.interfaces[0].methods[0].params = vec![field("value", Value::Ref(9))];
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
            tree: &forms::SurfaceDrawArg21,
            _: forms::SurfaceDrawArg22,
            _: forms::SurfaceDrawArg23,
            _: forms::SurfaceDrawArg24,
            _cx: CallContext<'_>,
        ) -> NativeCall<Box<dyn forms::Events>> {
            let forms::SurfaceDrawArg21::Node { kids, parent } = tree else {
                return NativeCall::Ready(Err(HostError("leaf".into())));
            };
            assert_eq!(*kids, vec![forms::SurfaceDrawArg21::Leaf]);
            assert_eq!(*parent, Some(Box::new(forms::SurfaceDrawArg21::Leaf)));
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
    let Ok(native::Answer::Ready(empty)) =
        forms::dispatch_Surface3(window.as_mut(), 1, &[], &mut native::Handles::default())
    else {
        panic!("`flush` answers at once")
    };
    assert!(empty.is_empty());
    let mut screen: Box<dyn std::any::Any> = Box::new(Box::new(Screen) as Box<dyn forms::Surface>);
    let Ok(native::Answer::Pending(_)) =
        forms::dispatch_Surface(screen.as_mut(), 2, &[], &mut native::Handles::default())
    else {
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
    let table: Vec<(String, GrantValue)> = [
        "dep.ui_tui.surface.Surface",
        "dep.ui_tui.values.Values",
        "dep.ui_tui.files.Files",
    ]
    .into_iter()
    .map(|path| (path.to_owned(), GrantValue::All))
    .collect();
    HostSetup {
        grants: Grants::with_consent(Consent::new(&table, &[])),
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
        a25: &values::ValuesTakeArg25,
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
) -> Result<Vec<u8>, native::Failure> {
    match values::dispatch_Values(provider, method, args, &mut native::Handles::default())? {
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
    let mut handles = native::Handles::default();
    let mut expected = native::wire::Encoder::new(&mut handles);
    expected.string("HÉ");
    assert_eq!(
        dispatch_values(provider.as_mut(), 2, "hé".as_bytes()).unwrap(),
        expected.into_bytes()
    );
}

#[test]
fn a_malformed_value_fails_the_call_with_the_js_hosts_message() {
    let (mut provider, _) = codecs(made(true));
    // A codec or host check fails as the host's own failure; only the
    // provider's own error is its throw.
    let fail = |provider: &mut Box<dyn std::any::Any>, method, args: &[u8]| match dispatch_values(
        provider.as_mut(),
        method,
        args,
    )
    .unwrap_err()
    {
        native::Failure::Host(error) => error.0,
        other => panic!("not the host's failure: {other:?}"),
    };
    assert_eq!(
        fail(&mut provider, 3, &[]),
        "package host: invalid private object key"
    );
    assert_eq!(
        dispatch_values(provider.as_mut(), 4, &[]),
        Err(native::Failure::Threw(HostError("stopped".into())))
    );
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

/// The native object behind returned `File` handles: its drops and the
/// method calls on it.
#[derive(Default)]
struct Disk {
    drops: Cell<usize>,
    calls: Cell<usize>,
}

/// One native object two handles may share.
struct Object(Rc<Disk>);

impl Drop for Object {
    fn drop(&mut self) {
        self.0.drops.set(self.0.drops.get() + 1);
    }
}

struct Files(Rc<Disk>);

struct OpenFile(Rc<Object>);

impl handles::Files for Files {
    fn pair(&mut self) -> HostResult<(Box<dyn handles::File>, Box<dyn handles::File>)> {
        let object = Rc::new(Object(Rc::clone(&self.0)));
        Ok((
            Box::new(OpenFile(Rc::clone(&object))),
            Box::new(OpenFile(object)),
        ))
    }

    fn bad(&mut self) -> HostResult<(Box<dyn handles::File>, Serde)> {
        let object = Rc::new(Object(Rc::clone(&self.0)));
        Ok((
            Box::new(OpenFile(object)),
            Serde::Map(vec![(Serde::Bool(true), Serde::Unit)]),
        ))
    }

    /// Only its companion's text is checked: a group holding a handle
    /// compiles without clone, compare or `Debug`.
    fn tree(&mut self) -> HostResult<handles::FilesTreeResult> {
        let object = Rc::new(Object(Rc::clone(&self.0)));
        Ok(handles::FilesTreeResult::Node {
            kids: vec![handles::FilesTreeResult::Leaf {
                file: Box::new(OpenFile(object)),
            }],
        })
    }
}

impl handles::File for OpenFile {
    fn size(&mut self) -> HostResult<u64> {
        let disk = &self.0.0;
        disk.calls.set(disk.calls.get() + 1);
        Ok(42)
    }

    fn dup(&mut self) -> HostResult<Box<dyn handles::File>> {
        let disk = &self.0.0;
        disk.calls.set(disk.calls.get() + 1);
        Ok(Box::new(OpenFile(Rc::clone(&self.0))))
    }
}

/// Runs `body` over `pair`, `$release`, `size`, `dup` and `bad`, at
/// indices 2 to 6, and reports the outcome, standard error, and the
/// object's drops and calls while the instance is still open and after
/// it is dropped.
fn run_handles(body: &[wasm_encoder::Instruction<'static>]) -> (Outcome, String, [usize; 3]) {
    use wasm_encoder::ValType::I32;
    let mut p = support::Program::new();
    assert_eq!(p.import("hd:pkg:files", "pair", &[], &[I32]), 2);
    assert_eq!(p.import("hd:pkg:file", "$release", &[I32], &[]), 3);
    assert_eq!(p.import("hd:pkg:file", "size", &[I32], &[I32]), 4);
    assert_eq!(p.import("hd:pkg:file", "dup", &[I32], &[I32]), 5);
    assert_eq!(p.import("hd:pkg:files", "bad", &[], &[I32]), 6);
    p.instructions(body);
    let (mut wasm, _) = p.finish(0);
    let mut payload = support::Enc(Vec::new())
        .bytes(hd_host_abi::schema::SECTION.as_bytes())
        .0;
    payload.extend_from_slice(handles::CONTRACT);
    wasm.push(0);
    wasm.extend_from_slice(&support::Enc(Vec::new()).bytes(&payload).0);
    let engine = WasmtimeEngine::new(EngineConfig::default()).unwrap();
    let module = engine.load(&wasm, &MemoryStore::default()).unwrap();
    let disk = Rc::<Disk>::default();
    let bindings = handles::Bindings::builder()
        .files(Files(Rc::clone(&disk)))
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
    let open = disk.drops.get();
    drop(instance);
    let text = String::from_utf8(err.0.lock().unwrap().clone()).unwrap();
    (outcome, text, [open, disk.drops.get(), disk.calls.get()])
}

#[test]
fn a_generated_companion_keeps_returned_handles_until_their_last_release() {
    use wasm_encoder::Instruction as I;
    // `pair` issues 1 and 2 for one object, `dup` on 2 issues 3 through
    // the `File` dispatch; the object drops at the last release, with no
    // method call.
    let calls = [
        I::Call(2),
        I::Drop,
        I::I32Const(1),
        I::Call(4),
        I::Drop,
        I::I32Const(2),
        I::Call(5),
        I::Drop,
        I::I32Const(2),
        I::Call(3),
        I::I32Const(1),
        I::Call(3),
    ];
    let mut all = calls.to_vec();
    all.extend([I::I32Const(3), I::Call(3)]);
    let (outcome, error, seen) = run_handles(&all);
    assert_eq!(outcome, Outcome::Exit(0), "{error}");
    assert_eq!(seen, [1, 1, 2], "dropped at the last release");
    // A handle still held when the instance closes drops with it.
    let (outcome, error, seen) = run_handles(&calls);
    assert_eq!(outcome, Outcome::Exit(0), "{error}");
    assert_eq!(seen, [0, 1, 2], "held until the instance closes");
}

#[test]
fn a_handle_in_a_result_that_fails_its_codec_is_released_at_once() {
    use wasm_encoder::Instruction as I;
    let (outcome, error, seen) = run_handles(&[I::Call(6), I::Drop]);
    assert_eq!(outcome, Outcome::Exit(3));
    assert!(
        error.starts_with(
            "panic: host-contract: example.org/ui-tui@0.3.0/files.Files.bad: package host: invalid private object key"
        ),
        "{error}"
    );
    assert_eq!(seen, [1, 1, 0], "released before the guest saw it");
}

/// Each allocation's call and bytes on the current thread: a counted
/// measure of a codec's work, which a codec that copied or rescanned its
/// stacks would grow faster than its input.
struct Counting;

thread_local! {
    static ALLOCATED: Cell<(u64, u64)> = const { Cell::new((0, 0)) };
}

fn counted(bytes: usize) {
    let bytes = u64::try_from(bytes).unwrap_or(u64::MAX);
    let _ = ALLOCATED.try_with(|n| n.set((n.get().0 + 1, n.get().1 + bytes)));
}

// SAFETY: every method forwards to `System` with its own arguments, as
// `hd_command`'s work allocator does; the count touches only a thread-local
// cell and never allocates.
#[allow(unsafe_code)]
unsafe impl std::alloc::GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        counted(layout.size());
        // SAFETY: the caller's contract for `alloc` is `System`'s.
        unsafe { std::alloc::System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        // SAFETY: `ptr` came from `System` with `layout`.
        unsafe { std::alloc::System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, new_size: usize) -> *mut u8 {
        counted(new_size);
        // SAFETY: as `dealloc`, and the caller's `new_size` contract.
        unsafe { std::alloc::System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

use deep::{
    DeepMakeResult as Made, DeepMakeResultInnerInner as MadeTwig, DeepTakeArg0 as Got,
    DeepTakeArg0InnerInner as GotTwig,
};

/// A `Node` `depth` levels deep, each level wrapping the one below in one
/// container form: 0 `Next`, 1 `Many`, 2 `Maybe`, 3 `Pair`, 4 `Keyed`, 5
/// `Either`, 6 `Inner.up`, 7 `Bad.rest`, 8 `Twig.kids` under one `Inner`;
/// 9 is form 7 under a `Bad` whose json fails to encode.
fn chain(depth: u32, form: u32) -> Made {
    if form == 8 {
        let mut twig = MadeTwig {
            up: None,
            kids: vec![],
            tag: 0,
        };
        for _ in 0..depth {
            twig = MadeTwig {
                up: None,
                kids: vec![twig],
                tag: 1,
            };
        }
        return Made::Inner { inner: twig };
    }
    let mut node = Made::End;
    for _ in 0..depth {
        node = match form {
            0 => Made::Next {
                next: Box::new(node),
            },
            1 => Made::Many { items: vec![node] },
            2 => Made::Maybe {
                at: Some(Box::new(node)),
            },
            3 => Made::Pair {
                pair: (Box::new(node), 7),
            },
            4 => Made::Keyed {
                map: vec![("k".into(), Box::new(node))],
            },
            5 => Made::Either {
                either: Ok(Box::new(node)),
            },
            6 => Made::Inner {
                inner: MadeTwig {
                    up: Some(Box::new(node)),
                    kids: vec![],
                    tag: 1,
                },
            },
            _ => Made::Bad {
                bad: Serde::Unit,
                rest: Box::new(node),
            },
        };
    }
    if form == 9 {
        node = Made::Bad {
            bad: Serde::Map(vec![(Serde::Bool(true), Serde::Unit)]),
            rest: Box::new(node),
        };
    }
    node
}

/// The `Node`s and `Twig`s of the lent `node`, counted one at a time.
fn levels(node: &Got) -> u32 {
    enum Item<'a> {
        Node(&'a Got),
        Twig(&'a GotTwig),
    }
    let mut stack = vec![Item::Node(node)];
    let mut count = 0;
    while let Some(item) = stack.pop() {
        count += 1;
        match item {
            Item::Node(
                Got::Next { next }
                | Got::Maybe { at: Some(next) }
                | Got::Pair { pair: (next, _) }
                | Got::Either { either: Ok(next) }
                | Got::Bad { rest: next, .. },
            ) => stack.push(Item::Node(next)),
            Item::Node(Got::End | Got::Maybe { at: None } | Got::Either { either: Err(_) }) => {}
            Item::Node(Got::Many { items }) => stack.extend(items.iter().map(Item::Node)),
            Item::Node(Got::Keyed { map }) => {
                stack.extend(map.iter().map(|(_, n)| Item::Node(n)));
            }
            Item::Node(Got::Inner { inner }) => stack.push(Item::Twig(inner)),
            Item::Twig(twig) => {
                stack.extend(twig.up.as_deref().map(Item::Node));
                stack.extend(twig.kids.iter().map(Item::Twig));
            }
        }
    }
    count
}

struct Deeps;

impl deep::Deep for Deeps {
    fn make(&mut self, depth: u32, form: u32) -> HostResult<Made> {
        Ok(chain(depth, form))
    }

    /// Reads the lent value; the engine takes it apart after the call.
    fn take(&mut self, node: &Got) -> HostResult<u32> {
        Ok(levels(node))
    }

    /// Reads each lent chain under its outer form.
    fn hold(
        &mut self,
        list: &Vec<deep::DeepHoldArg0Item>,
        option: &Option<deep::DeepHoldArg1>,
        result: &Result<i32, deep::DeepHoldArg2Err>,
        map: &Vec<(deep::DeepHoldArg3Key, deep::DeepHoldArg3Value)>,
        tuple: &(deep::DeepHoldArg40, i32),
        record: &deep::DeepHoldArg5,
        variant: &deep::DeepHoldArg6,
        only: &deep::DeepHoldArg7,
    ) -> HostResult<u32> {
        /// The links of a lent `Chain`, its `End` included.
        macro_rules! links {
            ($t:path, $x:expr) => {{
                use $t as T;
                let mut n = 1;
                let mut at: &T = $x;
                while let T::Next { next } = at {
                    n += 1;
                    at = next;
                }
                n
            }};
        }
        let mut n = 0;
        for x in list {
            n += links!(deep::DeepHoldArg0Item, x);
        }
        if let Some(x) = option {
            n += links!(deep::DeepHoldArg1, x);
        }
        if let Err(x) = result {
            n += links!(deep::DeepHoldArg2Err, x);
        }
        for (k, x) in map {
            n += links!(deep::DeepHoldArg3Key, k) + links!(deep::DeepHoldArg3Value, x);
        }
        n += links!(deep::DeepHoldArg40, &tuple.0);
        n += links!(deep::DeepHoldArg5Node, &record.node);
        if let deep::DeepHoldArg6::Full { node, .. } = variant {
            n += links!(deep::DeepHoldArg6FullNode, node);
        }
        let deep::DeepHoldArg7::Only { node } = only;
        n += links!(deep::DeepHoldArg7OnlyNode, node);
        Ok(n)
    }
}

/// A `Chain` `depth` links deep, then its `End`.
fn chain_bytes(e: &mut native::wire::Encoder<'_>, depth: u32) {
    for _ in 0..depth {
        e.tag(1);
    }
    e.tag(0);
}

/// `hold`'s arguments: a `depth`-deep chain under each outer form, and a
/// second, one-link chain in the list.
fn hold_args(depth: u32) -> Vec<u8> {
    let mut handles = native::Handles::default();
    let mut e = native::wire::Encoder::new(&mut handles);
    e.count(2);
    chain_bytes(&mut e, depth);
    chain_bytes(&mut e, 1);
    e.tag(1);
    chain_bytes(&mut e, depth);
    e.tag(1);
    chain_bytes(&mut e, depth);
    e.count(1);
    chain_bytes(&mut e, depth);
    chain_bytes(&mut e, depth);
    chain_bytes(&mut e, depth);
    e.signed(5);
    chain_bytes(&mut e, depth);
    e.signed(5);
    e.tag(1);
    chain_bytes(&mut e, depth);
    e.signed(5);
    e.tag(0);
    chain_bytes(&mut e, depth);
    e.into_bytes()
}

/// `hold`'s arguments, shallow, up to the one at `cut` (0 list, 3 map
/// value, 7 map key, 4 tuple, 5 record, 6 variant), whose `depth`-deep
/// chain is its last byte: the part after it, in the same argument, is
/// missing.
fn hold_cut(depth: u32, cut: u32) -> Vec<u8> {
    let mut handles = native::Handles::default();
    let mut e = native::wire::Encoder::new(&mut handles);
    let at = |form: u32| if form == cut { depth } else { 0 };
    e.count(2);
    chain_bytes(&mut e, at(0));
    if cut == 0 {
        return e.into_bytes();
    }
    chain_bytes(&mut e, 0);
    e.tag(0);
    e.tag(0);
    e.signed(1);
    e.count(2);
    chain_bytes(&mut e, at(7));
    if cut == 7 {
        return e.into_bytes();
    }
    chain_bytes(&mut e, at(3));
    if cut == 3 {
        return e.into_bytes();
    }
    chain_bytes(&mut e, 0);
    chain_bytes(&mut e, 0);
    chain_bytes(&mut e, at(4));
    if cut == 4 {
        return e.into_bytes();
    }
    e.signed(5);
    chain_bytes(&mut e, at(5));
    if cut == 5 {
        return e.into_bytes();
    }
    e.signed(5);
    e.tag(1);
    chain_bytes(&mut e, depth);
    e.into_bytes()
}

#[test]
fn an_argument_failing_after_its_million_deep_part_takes_that_part_apart_in_constant_native_stack()
{
    on_small_stack(|| {
        // Shallow, each cut argument fails where its next part is missing.
        for cut in [0, 3, 7, 4, 5, 6] {
            assert!(call_deep(2, &hold_cut(1, cut)).is_err(), "cut {cut}");
        }
        for cut in [0, 3, 7, 4, 5, 6] {
            assert!(
                call_deep(2, &hold_cut(1_000_000, cut)).is_err(),
                "cut {cut}"
            );
        }
    });
}

#[test]
fn a_lent_million_deep_argument_under_each_outer_form_is_taken_apart_in_constant_native_stack() {
    on_small_stack(|| {
        const DEPTH: u32 = 1_000_000;
        let args = hold_args(DEPTH);
        let answer = call_deep(2, &args).unwrap();
        let links = native::wire::Decoder::new(&answer)
            .unsigned::<u32>()
            .unwrap();
        assert_eq!(links, 9 * (DEPTH + 1) + 2);
        // A trailing byte fails after every argument is read; a missing
        // last byte fails the last argument after the others are read.
        let mut trailing = args.clone();
        trailing.push(0);
        assert!(call_deep(2, &trailing).is_err());
        assert!(call_deep(2, &args[..args.len() - 1]).is_err());
    });
}

/// `method` of a `Deeps` provider with `args`, answered at once.
fn call_deep(method: usize, args: &[u8]) -> Result<Vec<u8>, native::Failure> {
    let provider: Box<dyn deep::Deep> = Box::new(Deeps);
    let mut provider: Box<dyn std::any::Any> = Box::new(provider);
    let mut handles = native::Handles::default();
    match deep::dispatch_Deep(provider.as_mut(), method, args, &mut handles)? {
        native::Answer::Ready(bytes) => Ok(bytes),
        native::Answer::Pending(_) => panic!("`Deep` has no suspending method"),
    }
}

/// The encoded `Node` of `make(depth, form)`.
fn made_bytes(depth: u32, form: u32) -> Result<Vec<u8>, native::Failure> {
    let mut handles = native::Handles::default();
    let mut args = native::wire::Encoder::new(&mut handles);
    args.unsigned(depth.into());
    args.unsigned(form.into());
    call_deep(0, &args.into_bytes())
}

/// The level count `take` reads back from `bytes`.
fn taken_levels(bytes: &[u8]) -> Result<u32, native::Failure> {
    let answer = call_deep(1, bytes)?;
    Ok(native::wire::Decoder::new(&answer)
        .unsigned::<u32>()
        .unwrap())
}

/// `f` on a thread whose native stack is far smaller than one frame per
/// level of a 1,000,000-deep value.
fn on_small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap()
}

#[test]
fn a_million_deep_value_crosses_a_generated_codec_both_ways_in_constant_native_stack() {
    on_small_stack(|| {
        const DEPTH: u32 = 1_000_000;
        for form in 0..=8 {
            let bytes = made_bytes(DEPTH, form).unwrap();
            let expected = match form {
                6 => 2 * DEPTH + 1,
                8 => DEPTH + 2,
                _ => DEPTH + 1,
            };
            assert_eq!(taken_levels(&bytes).unwrap(), expected, "form {form}");
            if form == 3 {
                // The outermost pair's `i32` is the last byte: without it
                // the whole finished value below is dismantled.
                assert!(taken_levels(&bytes[..bytes.len() - 1]).is_err());
            }
            // A trailing byte fails the call after the whole argument is
            // read, which the engine then takes apart.
            let mut trailing = bytes;
            trailing.push(0);
            assert!(taken_levels(&trailing).is_err(), "form {form}");
        }
        // A provider value failing part way leaves its deep rest to drop.
        assert!(made_bytes(DEPTH, 9).is_err());
    });
}

/// A clone of `node`, which equals it, and a copy differing at its deepest
/// level, which does not; each is taken apart by its group's loop.
fn clone_and_compare(node: &Made, differ: impl FnOnce(&mut Made)) {
    let copy = node.clone();
    assert_eq!(copy, *node);
    let mut other = node.clone();
    differ(&mut other);
    // Both sides' order of fields and variants.
    assert_ne!(other, *node);
    assert_ne!(*node, other);
    copy.dismantle();
    other.dismantle();
}

/// The deepest `Node` of a form-0 chain.
fn deepest(mut node: &mut Made) -> &mut Made {
    while let Made::Next { next } = node {
        node = &mut **next;
    }
    node
}

#[test]
fn a_million_deep_value_clones_and_compares_in_constant_native_stack() {
    on_small_stack(|| {
        const DEPTH: u32 = 1_000_000;
        for form in 0..=8 {
            let node = chain(DEPTH, form);
            let copy = node.clone();
            assert_eq!(copy, node, "form {form}");
            copy.dismantle();
            node.dismantle();
        }
        // Equal down to the last level, which differs.
        let node = chain(DEPTH, 0);
        clone_and_compare(&node, |node| {
            *deepest(node) = Made::Maybe { at: None };
        });
        node.dismantle();
        // Every container form's own difference.
        let differ = [
            (1, Made::Many { items: vec![] }),
            (2, Made::Maybe { at: None }),
            (
                3,
                Made::Pair {
                    pair: (Box::new(Made::End), 8),
                },
            ),
            (4, Made::Keyed { map: vec![] }),
            (
                5,
                Made::Either {
                    either: Err("e".into()),
                },
            ),
        ];
        for (form, top) in differ {
            let node = chain(3, form);
            clone_and_compare(&node, |node| *node = top);
            node.dismantle();
        }
        let twig = chain(3, 8);
        clone_and_compare(&twig, |node| {
            if let Made::Inner { inner } = node {
                inner.kids[0].tag = 2;
            }
        });
        twig.dismantle();
    });
}

#[test]
fn a_recursive_groups_debug_prints_as_a_derived_one() {
    let shown = [
        (0, "Next { next: End }"),
        (1, "Many { items: [End] }"),
        (2, "Maybe { at: Some(End) }"),
        (3, "Pair { pair: (End, 7) }"),
        (4, "Keyed { map: [(\"k\", End)] }"),
        (5, "Either { either: Ok(End) }"),
        (
            6,
            "Inner { inner: DeepMakeResultInnerInner { up: Some(End), kids: [], tag: 1 } }",
        ),
        (7, "Bad { bad: Unit, rest: End }"),
        (
            8,
            "Inner { inner: DeepMakeResultInnerInner { up: None, kids: [DeepMakeResultInnerInner { up: None, kids: [], tag: 0 }], tag: 1 } }",
        ),
    ];
    for (form, text) in shown {
        assert_eq!(format!("{:?}", chain(1, form)), text, "form {form}");
    }
    let err = Made::Either {
        either: Err("e".into()),
    };
    assert_eq!(format!("{err:?}"), "Either { either: Err(\"e\") }");
    assert_eq!(
        format!("{:?}", Made::Maybe { at: None }),
        "Maybe { at: None }"
    );
    // Pretty: each level indented, a leaf's own lines too.
    assert_eq!(
        format!("{:#?}", chain(1, 4)),
        "Keyed {\n    map: [\n        (\n            \"k\",\n            End,\n        ),\n    ],\n}"
    );
    let bad = Made::Bad {
        bad: Serde::Seq(vec![Serde::Unit]),
        rest: Box::new(Made::Many { items: vec![] }),
    };
    assert_eq!(
        format!("{bad:#?}"),
        "Bad {\n    bad: Seq(\n        [\n            Unit,\n        ],\n    ),\n    rest: Many {\n        items: [],\n    },\n}"
    );
    assert_eq!(
        format!("{:#?}", chain(1, 3)),
        "Pair {\n    pair: (\n        End,\n        7,\n    ),\n}"
    );
    // A million levels print in constant native stack.
    on_small_stack(|| {
        let node = chain(1_000_000, 2);
        let text = format!("{node:?}");
        assert_eq!(
            text.len(),
            1_000_000 * "Maybe { at: Some(".len() + "End".len() + 1_000_000 * ") }".len()
        );
        node.dismantle();
    });
}

#[test]
fn a_generated_clone_and_compares_work_is_linear_in_its_values_depth() {
    let counted = |depth: u32, form: u32| {
        on_small_stack(move || {
            let node = chain(depth, form);
            let before = ALLOCATED.with(Cell::get);
            let copy = node.clone();
            assert_eq!(copy, node);
            copy.dismantle();
            let after = ALLOCATED.with(Cell::get);
            node.dismantle();
            (after.0 - before.0, after.1 - before.1)
        })
    };
    for form in [0, 1, 4, 8] {
        let (small, large) = (counted(100_000, form), counted(1_000_000, form));
        for (what, s, l) in [("calls", small.0, large.0), ("bytes", small.1, large.1)] {
            assert!(
                l <= 11 * s,
                "form {form}: {what} {s} at 100,000 and {l} at 1,000,000"
            );
        }
    }
}

#[test]
fn a_failed_arguments_take_apart_is_linear_in_its_depth() {
    let counted = |depth: u32, cut: u32| {
        on_small_stack(move || {
            let args = hold_cut(depth, cut);
            let before = ALLOCATED.with(Cell::get);
            assert!(call_deep(2, &args).is_err());
            let after = ALLOCATED.with(Cell::get);
            (after.0 - before.0, after.1 - before.1)
        })
    };
    for cut in [0, 7, 4, 6] {
        let (small, large) = (counted(100_000, cut), counted(1_000_000, cut));
        for (what, s, l) in [("calls", small.0, large.0), ("bytes", small.1, large.1)] {
            assert!(
                l <= 11 * s,
                "cut {cut}: {what} {s} at 100,000 and {l} at 1,000,000"
            );
        }
    }
}

#[test]
fn a_generated_codecs_work_is_linear_in_its_values_depth() {
    let counted = |depth: u32, form: u32| {
        on_small_stack(move || {
            let before = ALLOCATED.with(Cell::get);
            let bytes = made_bytes(depth, form).unwrap();
            taken_levels(&bytes).unwrap();
            let after = ALLOCATED.with(Cell::get);
            (after.0 - before.0, after.1 - before.1)
        })
    };
    for form in [0, 1, 4, 8] {
        let (small, large) = (counted(100_000, form), counted(1_000_000, form));
        // Ten times the depth is at most ten times the work, with slack
        // for the stacks' power-of-two growth.
        for (what, s, l) in [("calls", small.0, large.0), ("bytes", small.1, large.1)] {
            assert!(
                l <= 11 * s,
                "form {form}: {what} {s} at 100,000 and {l} at 1,000,000"
            );
        }
    }
}
