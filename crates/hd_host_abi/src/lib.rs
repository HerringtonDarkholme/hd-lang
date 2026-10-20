#![forbid(unsafe_code)]
//! `hd_host_abi`: the one host ABI description (`crates/hd_run_wasmtime/DESIGN.md` §16.4,
//! §17.1, §17.2): capability traits, methods, codecs and wait flags; the
//! import names they produce; and operation handles.
//!
//! The table mirrors std's capability traits in `lib/std`. `hd_wasm`
//! generates the hd-side provider stubs of the default profile from it.
//! `tests/abi_model.rs` builds the ABI model from it and `lib/std`: every
//! method with the std types it crosses and the std declarations they
//! reach, checked in as `host/gen/abi.json`, and from the model both
//! hosts' codecs and method descriptors: the JS host's `host/gen/abi.mjs`
//! and the wasmtime host's `hd_run_wasmtime/src/abi.rs`.

/// Scalar Wasm types at the boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scalar {
    I32,
    I64,
    F64,
}

/// How a value crosses the boundary (§17.2, §17.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    Void,
    Scalar(Scalar),
    /// A structured value in the exchange buffer, named by its std type.
    Buffer(&'static str),
}

/// Whether a method may wait: start-and-poll, or one call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wait {
    Never,
    May,
}

/// Which argument a grant checks (§17.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceArg {
    Path,
    Host,
    Addr,
    EnvName,
    Program,
    SysName,
}

#[derive(Debug)]
pub struct HostMethod {
    pub name: &'static str,
    pub params: &'static [Codec],
    pub result: Codec,
    pub wait: Wait,
    pub resource: Option<ResourceArg>,
}

#[derive(Debug)]
pub struct HostTrait {
    pub key: &'static str,
    pub std_path: &'static str,
    pub methods: &'static [HostMethod],
}

const fn m(
    name: &'static str,
    params: &'static [Codec],
    result: Codec,
    wait: Wait,
    resource: Option<ResourceArg>,
) -> HostMethod {
    HostMethod {
        name,
        params,
        result,
        wait,
        resource,
    }
}

use Codec::{Buffer as B, Scalar as S, Void as V};

/// The ABI table: std's capability traits (a test compares it with `lib/std`).
pub static TABLE: &[HostTrait] = &[
    HostTrait {
        key: "Console",
        std_path: "std.console.Console",
        methods: &[
            m(
                "write_line",
                &[B("string")],
                B("Result[void, ConsoleError]"),
                Wait::May,
                None,
            ),
            m(
                "write_error_line",
                &[B("string")],
                B("Result[void, ConsoleError]"),
                Wait::May,
                None,
            ),
        ],
    },
    HostTrait {
        key: "ConsoleInput",
        std_path: "std.console.ConsoleInput",
        methods: &[
            m(
                "read_line",
                &[],
                B("Result[string?, ConsoleError]"),
                Wait::May,
                None,
            ),
            m(
                "read_all",
                &[],
                B("Result[string, ConsoleError]"),
                Wait::May,
                None,
            ),
        ],
    },
    HostTrait {
        key: "Clock",
        std_path: "std.time.Clock",
        methods: &[
            m("now", &[], S(Scalar::I64), Wait::Never, None),
            m("monotonic", &[], S(Scalar::I64), Wait::Never, None),
            m("sleep", &[S(Scalar::I64)], V, Wait::May, None),
        ],
    },
    HostTrait {
        key: "FsRead",
        std_path: "std.fs.FsRead",
        methods: &[
            m(
                "read_bytes",
                &[B("Path")],
                B("Result[List[u8], FsError]"),
                Wait::May,
                Some(ResourceArg::Path),
            ),
            m(
                "read_text",
                &[B("Path")],
                B("Result[string, FsError]"),
                Wait::May,
                Some(ResourceArg::Path),
            ),
            m(
                "list_dir",
                &[B("Path")],
                B("Result[List[Entry], FsError]"),
                Wait::May,
                Some(ResourceArg::Path),
            ),
            m(
                "stat",
                &[B("Path")],
                B("Result[Entry?, FsError]"),
                Wait::May,
                Some(ResourceArg::Path),
            ),
        ],
    },
    HostTrait {
        key: "FsWrite",
        std_path: "std.fs.FsWrite",
        methods: &[
            m(
                "write_bytes",
                &[B("Path"), B("List[u8]")],
                B("Result[void, FsError]"),
                Wait::May,
                Some(ResourceArg::Path),
            ),
            m(
                "write_text",
                &[B("Path"), B("string")],
                B("Result[void, FsError]"),
                Wait::May,
                Some(ResourceArg::Path),
            ),
            m(
                "append_text",
                &[B("Path"), B("string")],
                B("Result[void, FsError]"),
                Wait::May,
                Some(ResourceArg::Path),
            ),
            m(
                "create_dir_all",
                &[B("Path")],
                B("Result[void, FsError]"),
                Wait::May,
                Some(ResourceArg::Path),
            ),
            m(
                "remove",
                &[B("Path")],
                B("Result[void, FsError]"),
                Wait::May,
                Some(ResourceArg::Path),
            ),
            m(
                "rename",
                &[B("Path"), B("Path")],
                B("Result[void, FsError]"),
                Wait::May,
                Some(ResourceArg::Path),
            ),
        ],
    },
    HostTrait {
        key: "Random",
        std_path: "std.random.Random",
        methods: &[
            m("next_u64", &[], S(Scalar::I64), Wait::Never, None),
            m("fill", &[S(Scalar::I32)], B("List[u8]"), Wait::Never, None),
        ],
    },
    HostTrait {
        key: "Http",
        std_path: "std.http.Http",
        methods: &[m(
            "send",
            &[B("Request")],
            B("Result[Response, HttpError]"),
            Wait::May,
            Some(ResourceArg::Host),
        )],
    },
    HostTrait {
        key: "Net",
        std_path: "std.net.Net",
        methods: &[
            m(
                "lookup",
                &[B("string")],
                B("Result[List[string], NetError]"),
                Wait::May,
                Some(ResourceArg::Host),
            ),
            m(
                "connect",
                &[B("string"), S(Scalar::I32)],
                B("Result[TcpStream, NetError]"),
                Wait::May,
                Some(ResourceArg::Addr),
            ),
            m(
                "connect_timeout",
                &[B("string"), S(Scalar::I32), S(Scalar::I64)],
                B("Result[TcpStream, NetError]"),
                Wait::May,
                Some(ResourceArg::Addr),
            ),
            m(
                "listen",
                &[B("string"), S(Scalar::I32)],
                B("Result[TcpListener, NetError]"),
                Wait::May,
                Some(ResourceArg::Addr),
            ),
            m(
                "bind_udp",
                &[B("string"), S(Scalar::I32)],
                B("Result[UdpSocket, NetError]"),
                Wait::May,
                Some(ResourceArg::Addr),
            ),
        ],
    },
    HostTrait {
        key: "Process",
        std_path: "std.process.Process",
        methods: &[m(
            "run",
            &[B("Command")],
            B("Result[ProcessOutput, ProcessError]"),
            Wait::May,
            Some(ResourceArg::Program),
        )],
    },
    HostTrait {
        key: "Sys",
        std_path: "std.sys.Sys",
        methods: &[
            m(
                "os",
                &[],
                B("Result[string, SysError]"),
                Wait::Never,
                Some(ResourceArg::SysName),
            ),
            m(
                "arch",
                &[],
                B("Result[string, SysError]"),
                Wait::Never,
                Some(ResourceArg::SysName),
            ),
            m(
                "hostname",
                &[],
                B("Result[string, SysError]"),
                Wait::Never,
                Some(ResourceArg::SysName),
            ),
            m(
                "cpu_count",
                &[],
                B("Result[u32, SysError]"),
                Wait::Never,
                Some(ResourceArg::SysName),
            ),
        ],
    },
    HostTrait {
        key: "Env",
        std_path: "std.host.Env",
        methods: &[
            m(
                "get",
                &[B("string")],
                B("string?"),
                Wait::Never,
                Some(ResourceArg::EnvName),
            ),
            m("names", &[], B("List[string]"), Wait::Never, None),
        ],
    },
    HostTrait {
        key: "Args",
        std_path: "std.host.Args",
        methods: &[
            m("program", &[], B("string"), Wait::Never, None),
            m("list", &[], B("List[string]"), Wait::Never, None),
        ],
    },
];

/// The test runner's host traits (§16.4, `docs/compiler/engines-and-test-runner.md` §19;
/// `std-testing.runner.declares-ref.reference-values`): imported from `hd:<Key>` like a
/// capability, but no grant names them.
pub static RUNNERS: &[HostTrait] = &[
    HostTrait {
        key: "TestRunner",
        std_path: "std.testing.TestRunner",
        methods: &[
            m("row", &[S(Scalar::I32)], S(Scalar::I32), Wait::Never, None),
            m("report_timeout", &[S(Scalar::I64)], V, Wait::Never, None),
            m(
                "snapshot_check",
                &[B("string")],
                B("string"),
                Wait::Never,
                None,
            ),
            m("temp_dir", &[], B("string"), Wait::Never, None),
        ],
    },
    HostTrait {
        key: "PropertyRunner",
        std_path: "std.testing.PropertyRunner",
        methods: &[
            m(
                "start",
                &[S(Scalar::I32), S(Scalar::I32), S(Scalar::I32)],
                B("PropertyCase"),
                Wait::Never,
                None,
            ),
            m("record", &[S(Scalar::I64)], V, Wait::Never, None),
            m("show", &[B("string")], V, Wait::Never, None),
        ],
    },
];

/// Host handles (`crates/hd_run_wasmtime/DESIGN.md` §17.4): the traits of values that a
/// host method returns as a handle number, such as `Net`'s sockets. In hd
/// such a value is a trait value whose vtable's slots are host calls that
/// take the handle first, as an `i32`; they are imported from the module
/// of the capability that made the handle, named `<Trait>.<method>`. A
/// supertrait's methods are the handle trait's too, as `TcpStream.dispose`
/// for `std.ops.Disposable`.
pub static HANDLES: &[HostTrait] = &[
    HostTrait {
        key: "Net",
        std_path: "std.net.TcpStream",
        methods: &[
            m(
                "TcpStream.read",
                &[S(Scalar::I32), S(Scalar::I32)],
                B("Result[List[u8], NetError]"),
                Wait::May,
                None,
            ),
            m(
                "TcpStream.write",
                &[S(Scalar::I32), B("List[u8]")],
                B("Result[void, NetError]"),
                Wait::May,
                None,
            ),
            m(
                "TcpStream.set_timeout",
                &[S(Scalar::I32), B("Duration?")],
                B("Result[void, NetError]"),
                Wait::Never,
                None,
            ),
            m(
                "TcpStream.set_nodelay",
                &[S(Scalar::I32), S(Scalar::I32)],
                B("Result[void, NetError]"),
                Wait::Never,
                None,
            ),
            m(
                "TcpStream.set_keepalive",
                &[S(Scalar::I32), B("Duration?")],
                B("Result[void, NetError]"),
                Wait::Never,
                None,
            ),
            m(
                "TcpStream.close",
                &[S(Scalar::I32)],
                B("Result[void, NetError]"),
                Wait::Never,
                None,
            ),
            m("TcpStream.dispose", &[S(Scalar::I32)], V, Wait::May, None),
        ],
    },
    HostTrait {
        key: "Net",
        std_path: "std.net.TcpListener",
        methods: &[
            m(
                "TcpListener.accept",
                &[S(Scalar::I32)],
                B("Result[TcpStream, NetError]"),
                Wait::May,
                None,
            ),
            m(
                "TcpListener.set_timeout",
                &[S(Scalar::I32), B("Duration?")],
                B("Result[void, NetError]"),
                Wait::Never,
                None,
            ),
            m(
                "TcpListener.local_address",
                &[S(Scalar::I32)],
                B("Result[(string, u16), NetError]"),
                Wait::Never,
                None,
            ),
            m(
                "TcpListener.close",
                &[S(Scalar::I32)],
                B("Result[void, NetError]"),
                Wait::Never,
                None,
            ),
            m("TcpListener.dispose", &[S(Scalar::I32)], V, Wait::May, None),
        ],
    },
    HostTrait {
        key: "Net",
        std_path: "std.net.UdpSocket",
        methods: &[
            m(
                "UdpSocket.send_to",
                &[S(Scalar::I32), B("string"), S(Scalar::I32), B("List[u8]")],
                B("Result[void, NetError]"),
                Wait::May,
                Some(ResourceArg::Addr),
            ),
            m(
                "UdpSocket.receive",
                &[S(Scalar::I32), S(Scalar::I32)],
                B("Result[Datagram, NetError]"),
                Wait::May,
                None,
            ),
            m(
                "UdpSocket.set_timeout",
                &[S(Scalar::I32), B("Duration?")],
                B("Result[void, NetError]"),
                Wait::Never,
                None,
            ),
            m(
                "UdpSocket.local_address",
                &[S(Scalar::I32)],
                B("Result[(string, u16), NetError]"),
                Wait::Never,
                None,
            ),
            m(
                "UdpSocket.close",
                &[S(Scalar::I32)],
                B("Result[void, NetError]"),
                Wait::Never,
                None,
            ),
            m("UdpSocket.dispose", &[S(Scalar::I32)], V, Wait::May, None),
        ],
    },
];

/// The host trait, capability or runner, whose std path is `std_path`.
#[must_use]
pub fn host_trait(std_path: &str) -> Option<&'static HostTrait> {
    TABLE.iter().chain(RUNNERS).find(|t| t.std_path == std_path)
}

/// The host handle trait whose std path is `std_path` (`HANDLES`).
#[must_use]
pub fn handle_trait(std_path: &str) -> Option<&'static HostTrait> {
    HANDLES.iter().find(|t| t.std_path == std_path)
}

/// Non-capability import modules (§16.4).
pub const RUNTIME_MODULES: &[&str] = &["hd:rt", "hd:TestRunner", "hd:PropertyRunner", "hd:hook"];

/// The runtime's own imports (§17.2).
pub const RUNTIME_IMPORTS: &[(&str, &str)] = &[
    ("hd:rt", "block"),
    ("hd:rt", "abort"),
    ("hd:rt", "stderr"),
    ("hd:rt", "stdout"),
    ("hd:rt", "panic"),
    ("hd:rt", "fact"),
    ("hd:rt", "hash_seed"),
];

/// How the compiler supplies an `@intrinsic("key")` body of std
/// (`crates/hd_stdpack/DESIGN.md`, "Primitive Functions"; `crates/hd_mono/DESIGN.md` §12.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lowering {
    /// Inline code the emitter writes (string and list kernels, `dbg`,
    /// panics, task frames, facts).
    Compiler,
    /// A host primitive import of module `hd:prim`, by name.
    HostPrimitive(&'static str),
}

/// Every intrinsic key std may name, and its lowering. The keys are the
/// declarations' `@intrinsic` arguments; `panic_message`, `task_all` and
/// `test_case` belong to the compiler-supplied `std.core.panic`,
/// `std.task.all` and `std.testing.it`, `assert_equal` to
/// `std.testing.assert_equal`, `entry_write` to `std.rt`,
/// `type_id_of` to `std.inspect`, the two reference recovery keys to
/// inherent methods in `std.inspect` and `std.error`,
/// `ref_serial` to the compiler-supplied `std.core.ref_serial`,
/// `ref_identity` to `std.cmp.identity_of`, which the checker lowers, and
/// the `mem_` keys to the runtime module `std.rt.memory`, which only the
/// linear backend lowers (`crates/hd_wasm_linear/DESIGN.md` §23.12).
pub static INTRINSICS: &[(&str, Lowering)] = &[
    ("bytes_len", Lowering::Compiler),
    ("bytes_at", Lowering::Compiler),
    ("bytes_slice", Lowering::Compiler),
    ("bytes_concat", Lowering::Compiler),
    ("concat_all", Lowering::Compiler),
    ("string_from_bytes", Lowering::Compiler),
    ("char_scalar", Lowering::Compiler),
    ("char_from_scalar", Lowering::Compiler),
    ("f64_to_bits", Lowering::Compiler),
    ("f64_from_bits", Lowering::Compiler),
    ("f32_to_bits", Lowering::Compiler),
    ("f32_from_bits", Lowering::Compiler),
    ("list_truncate", Lowering::Compiler),
    ("list_steps", Lowering::Compiler),
    ("map_steps", Lowering::Compiler),
    ("panic", Lowering::Compiler),
    ("panic_message", Lowering::Compiler),
    ("facts_of", Lowering::Compiler),
    ("type_id_of", Lowering::Compiler),
    ("downcast_ref", Lowering::Compiler),
    ("downcast_mut", Lowering::Compiler),
    ("block_on", Lowering::Compiler),
    ("task_race_frame", Lowering::Compiler),
    ("task_cleanup_frame", Lowering::Compiler),
    ("task_abandon", Lowering::Compiler),
    ("task_pending_frame", Lowering::Compiler),
    ("task_all_frame", Lowering::Compiler),
    ("task_all", Lowering::Compiler),
    ("test_case", Lowering::Compiler),
    ("entry_write", Lowering::Compiler),
    ("assert_equal", Lowering::Compiler),
    ("dbg", Lowering::Compiler),
    ("dbg_text", Lowering::Compiler),
    ("dbg_write", Lowering::Compiler),
    ("repl_write", Lowering::Compiler),
    ("dbg_show", Lowering::Compiler),
    ("map_hash_seed", Lowering::Compiler),
    ("ref_serial", Lowering::Compiler),
    ("ref_identity", Lowering::Compiler),
    ("format_int", Lowering::Compiler),
    ("mem_pages", Lowering::Compiler),
    ("mem_grow", Lowering::Compiler),
    ("mem_load_u8", Lowering::Compiler),
    ("mem_load_u16", Lowering::Compiler),
    ("mem_load_u32", Lowering::Compiler),
    ("mem_load_u64", Lowering::Compiler),
    ("mem_store_u8", Lowering::Compiler),
    ("mem_store_u16", Lowering::Compiler),
    ("mem_store_u32", Lowering::Compiler),
    ("mem_store_u64", Lowering::Compiler),
    ("mem_copy", Lowering::Compiler),
    ("mem_fill", Lowering::Compiler),
    ("format_f64", Lowering::HostPrimitive("format_f64")),
    ("format_f32", Lowering::HostPrimitive("format_f32")),
    ("string_lower", Lowering::HostPrimitive("string_lower")),
    ("string_upper", Lowering::HostPrimitive("string_upper")),
    ("parse_f64", Lowering::HostPrimitive("parse_f64")),
    (
        "snapshot_update",
        Lowering::HostPrimitive("snapshot_update"),
    ),
    (
        "format_f64_fixed",
        Lowering::HostPrimitive("format_f64_fixed"),
    ),
];

/// The lowering of an intrinsic key, if std may name it.
#[must_use]
pub fn intrinsic(key: &str) -> Option<Lowering> {
    INTRINSICS.iter().find(|(k, _)| *k == key).map(|(_, l)| *l)
}

/// A Wasm import: module and name fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Import {
    pub module: String,
    pub name: String,
}

/// The imports one method produces (§17.2): one function, or a
/// `.start`/`.finish` pair when it may wait.
#[must_use]
pub fn imports_of(t: &HostTrait, method: &HostMethod) -> Vec<Import> {
    let module = format!("hd:{}", t.key);
    match method.wait {
        Wait::Never => vec![Import {
            module,
            name: method.name.to_owned(),
        }],
        Wait::May => vec![
            Import {
                module: module.clone(),
                name: format!("{}.start", method.name),
            },
            Import {
                module,
                name: format!("{}.finish", method.name),
            },
        ],
    }
}

/// `hd FILE.wasm`'s check (§16.4): is this import in the ABI table?
#[must_use]
pub fn is_known_import(module: &str, name: &str) -> bool {
    RUNTIME_IMPORTS
        .iter()
        .any(|(m, n)| *m == module && *n == name)
        || (module == "hd:prim"
            && INTRINSICS
                .iter()
                .any(|(_, l)| matches!(l, Lowering::HostPrimitive(n) if *n == name)))
        || (RUNTIME_MODULES.contains(&module) && module != "hd:rt")
        || TABLE.iter().chain(HANDLES).any(|t| {
            t.methods.iter().any(|m| {
                imports_of(t, m)
                    .iter()
                    .any(|i| i.module == module && i.name == name)
            })
        })
}

/// An operation handle (§17.2): 20 bits of slot, 11 of generation; never negative.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Handle(pub u32);

impl Handle {
    pub const SLOT_BITS: u32 = 20;
    pub const GEN_BITS: u32 = 11;
    #[must_use]
    pub fn new(slot: u32, generation: u32) -> Handle {
        assert!(slot < (1 << Self::SLOT_BITS), "slot over 20 bits");
        Handle(((generation & ((1 << Self::GEN_BITS) - 1)) << Self::SLOT_BITS) | slot)
    }
    #[must_use]
    pub fn slot(self) -> u32 {
        self.0 & ((1 << Self::SLOT_BITS) - 1)
    }
    #[must_use]
    pub fn generation(self) -> u32 {
        self.0 >> Self::SLOT_BITS
    }
}

/// The host side of one operation slot (§17.2 table).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotState {
    Free,
    Pending,
    Completed,
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::{
        HANDLES, Handle, INTRINSICS, RUNNERS, TABLE, imports_of, intrinsic, is_known_import,
    };

    #[test]
    fn waiting_methods_import_a_start_finish_pair() {
        let fs = TABLE.iter().find(|t| t.key == "FsRead").expect("FsRead");
        let i = imports_of(fs, &fs.methods[1]);
        assert_eq!(i.len(), 2);
        assert_eq!(
            (i[0].module.as_str(), i[0].name.as_str()),
            ("hd:FsRead", "read_text.start")
        );
        assert!(is_known_import("hd:Clock", "now"));
        assert!(is_known_import("hd:rt", "block"));
        assert!(!is_known_import("hd:Clock", "now.start"));
        assert!(!is_known_import("env", "x"));
    }

    #[test]
    fn handles_pack_slot_and_generation() {
        let h = Handle::new(0xF_FFFF, 0x7FF);
        assert_eq!(h.slot(), 0xF_FFFF);
        assert_eq!(h.generation(), 0x7FF);
        assert!(i32::try_from(h.0).is_ok(), "never negative");
    }

    #[test]
    fn every_std_intrinsic_key_has_a_lowering() {
        let std = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../lib/std");
        let mut stack = vec![std];
        let mut keys = Vec::new();
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir).expect("std dir").flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "hd") {
                    let text = std::fs::read_to_string(&p).expect("std file");
                    for line in text.lines() {
                        if let Some(rest) = line.trim().strip_prefix("@intrinsic(\"") {
                            keys.push(rest.trim_end_matches("\")").to_owned());
                        }
                    }
                }
            }
        }
        assert!(
            keys.len() > 20,
            "std's intrinsic declarations were not found"
        );
        for k in &keys {
            assert!(
                intrinsic(k).is_some(),
                "@intrinsic(\"{k}\") has no lowering"
            );
        }
        assert!(INTRINSICS.iter().all(|(k, _)| !k.is_empty()));
    }

    #[test]
    fn table_mirrors_std_capability_traits() {
        let std = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../lib/std");
        for t in TABLE.iter().chain(RUNNERS).chain(HANDLES) {
            let (module, name) = t.std_path.rsplit_once('.').expect("path");
            let file = std.join(format!(
                "{}.hd",
                module.trim_start_matches("std.").replace('.', "/")
            ));
            let text = std::fs::read_to_string(&file).expect("std file");
            // A handle trait may list supertraits, as `< Disposable`.
            assert!(
                text.contains(&format!("pub trait {name}:"))
                    || text.contains(&format!("pub trait {name} <")),
                "{} not in {}",
                t.std_path,
                file.display()
            );
            for m in t.methods {
                // A handle's method is named `<Trait>.<method>`.
                let method = m.name.rsplit('.').next().unwrap_or(m.name);
                assert!(
                    text.contains(&format!("fn {method}")),
                    "{}.{} missing",
                    t.key,
                    m.name
                );
            }
        }
    }
}
