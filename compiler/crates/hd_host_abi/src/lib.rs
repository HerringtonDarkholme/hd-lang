#![forbid(unsafe_code)]
//! `hd_host_abi`: the one host ABI description (runtime-and-host.md §16.4,
//! §17.1, §17.2): capability traits, methods, codecs and wait flags; the
//! import names they produce; and operation handles.
//!
//! The table mirrors std's capability traits in `lib/std`. `hd_wasm`
//! generates the hd-side provider stubs of the default profile from it;
//! the wasmtime stubs and the JS glue generator are not written yet.

use hd_base::{NotImplemented, Stage, StageResult};

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
        methods: &[m(
            "write_line",
            &[B("string")],
            B("Result[void, ConsoleError]"),
            Wait::May,
            None,
        )],
    },
    HostTrait {
        key: "ConsoleInput",
        std_path: "std.console.ConsoleInput",
        methods: &[m(
            "read_line",
            &[],
            B("Result[string?, ConsoleError]"),
            Wait::May,
            None,
        )],
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
        methods: &[],
    },
    HostTrait {
        key: "Random",
        std_path: "std.random.Random",
        methods: &[],
    },
    HostTrait {
        key: "Http",
        std_path: "std.http.Http",
        methods: &[],
    },
    HostTrait {
        key: "Process",
        std_path: "std.process.Process",
        methods: &[],
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

/// Non-capability import modules (§16.4).
pub const RUNTIME_MODULES: &[&str] = &["hd:rt", "hd:TestRunner", "hd:PropertyRunner", "hd:hook"];

/// The runtime's own imports (§17.2).
pub const RUNTIME_IMPORTS: &[(&str, &str)] =
    &[("hd:rt", "block"), ("hd:rt", "abort"), ("hd:rt", "stderr")];

/// How the compiler supplies an `@intrinsic("key")` body of std
/// (std-bootstrap.md, "Primitive Functions"; codegen.md §12.2).
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
/// `std.testing.assert_equal`, and `entry_write` to `std.rt`.
pub static INTRINSICS: &[(&str, Lowering)] = &[
    ("bytes_len", Lowering::Compiler),
    ("bytes_at", Lowering::Compiler),
    ("bytes_slice", Lowering::Compiler),
    ("bytes_concat", Lowering::Compiler),
    ("string_from_bytes", Lowering::Compiler),
    ("char_scalar", Lowering::Compiler),
    ("char_from_scalar", Lowering::Compiler),
    ("list_truncate", Lowering::Compiler),
    ("panic", Lowering::Compiler),
    ("panic_message", Lowering::Compiler),
    ("facts_of", Lowering::Compiler),
    ("downcast_val", Lowering::Compiler),
    ("task_race_frame", Lowering::Compiler),
    ("task_all_frame", Lowering::Compiler),
    ("task_all", Lowering::Compiler),
    ("test_case", Lowering::Compiler),
    ("entry_write", Lowering::Compiler),
    ("assert_equal", Lowering::Compiler),
    ("dbg", Lowering::Compiler),
    ("dbg_text", Lowering::Compiler),
    ("dbg_write", Lowering::HostPrimitive("dbg_write")),
    ("format_f64", Lowering::HostPrimitive("format_f64")),
    ("format_f32", Lowering::HostPrimitive("format_f32")),
    ("string_lower", Lowering::HostPrimitive("string_lower")),
    ("string_upper", Lowering::HostPrimitive("string_upper")),
    ("parse_f64", Lowering::HostPrimitive("parse_f64")),
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
        || TABLE.iter().any(|t| {
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

/// The generators of §17.1: hd-side stubs, wasmtime stubs, the JS glue.
pub fn generate_js_glue() -> StageResult<String> {
    Err(NotImplemented::new(Stage::Link, "JS glue generator"))
}

#[cfg(test)]
mod tests {
    use super::{Handle, INTRINSICS, TABLE, imports_of, intrinsic, is_known_import};

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
        let std = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../lib/std");
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
        let std = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../lib/std");
        for t in TABLE {
            let (module, name) = t.std_path.rsplit_once('.').expect("path");
            let file = std.join(format!(
                "{}.hd",
                module.trim_start_matches("std.").replace('.', "/")
            ));
            let text = std::fs::read_to_string(&file).expect("std file");
            assert!(
                text.contains(&format!("pub trait {name}:")),
                "{} not in {}",
                t.std_path,
                file.display()
            );
            for m in t.methods {
                assert!(
                    text.contains(&format!("fn {}", m.name)),
                    "{}.{} missing",
                    t.key,
                    m.name
                );
            }
        }
    }
}
