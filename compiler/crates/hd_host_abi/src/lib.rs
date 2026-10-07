#![forbid(unsafe_code)]
//! `hd_host_abi`: the one host ABI description (runtime-and-host.md §16.4,
//! §17.1, §17.2): capability traits, methods, codecs and wait flags; the
//! import names they produce; and operation handles.
//!
//! The table mirrors std's capability traits in `lib/std`. The generators
//! (hd-side stubs, wasmtime stubs, JS glue) are not written yet.

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

const fn m(name: &'static str, params: &'static [Codec], result: Codec, wait: Wait, resource: Option<ResourceArg>) -> HostMethod {
    HostMethod { name, params, result, wait, resource }
}

use Codec::{Buffer as B, Scalar as S, Void as V};

/// The ABI table: std's capability traits (a test compares it with `lib/std`).
pub static TABLE: &[HostTrait] = &[
    HostTrait {
        key: "Console",
        std_path: "std.console.Console",
        methods: &[m("write_line", &[B("string")], B("Result[void, ConsoleError]"), Wait::May, None)],
    },
    HostTrait {
        key: "ConsoleInput",
        std_path: "std.console.ConsoleInput",
        methods: &[m("read_line", &[], B("Result[string?, ConsoleError]"), Wait::May, None)],
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
            m("read_bytes", &[B("Path")], B("Result[List[u8], FsError]"), Wait::May, Some(ResourceArg::Path)),
            m("read_text", &[B("Path")], B("Result[string, FsError]"), Wait::May, Some(ResourceArg::Path)),
            m("list_dir", &[B("Path")], B("Result[List[Entry], FsError]"), Wait::May, Some(ResourceArg::Path)),
            m("stat", &[B("Path")], B("Result[Entry?, FsError]"), Wait::May, Some(ResourceArg::Path)),
        ],
    },
    HostTrait { key: "FsWrite", std_path: "std.fs.FsWrite", methods: &[] },
    HostTrait { key: "Random", std_path: "std.random.Random", methods: &[] },
    HostTrait { key: "Http", std_path: "std.http.Http", methods: &[] },
    HostTrait { key: "Process", std_path: "std.process.Process", methods: &[] },
    HostTrait {
        key: "Env",
        std_path: "std.host.Env",
        methods: &[
            m("get", &[B("string")], B("string?"), Wait::Never, Some(ResourceArg::EnvName)),
            m("names", &[], B("List[string]"), Wait::Never, None),
        ],
    },
    HostTrait {
        key: "Args",
        std_path: "std.host.Args",
        methods: &[m("program", &[], B("string"), Wait::Never, None), m("list", &[], B("List[string]"), Wait::Never, None)],
    },
];

/// Non-capability import modules (§16.4).
pub const RUNTIME_MODULES: &[&str] = &["hd:rt", "hd:TestRunner", "hd:PropertyRunner", "hd:hook"];

/// The runtime's own imports (§17.2).
pub const RUNTIME_IMPORTS: &[(&str, &str)] = &[("hd:rt", "block"), ("hd:rt", "abort"), ("hd:rt", "stderr")];

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
        Wait::Never => vec![Import { module, name: method.name.to_owned() }],
        Wait::May => vec![
            Import { module: module.clone(), name: format!("{}.start", method.name) },
            Import { module, name: format!("{}.finish", method.name) },
        ],
    }
}

/// `hd FILE.wasm`'s check (§16.4): is this import in the ABI table?
#[must_use]
pub fn is_known_import(module: &str, name: &str) -> bool {
    RUNTIME_IMPORTS.iter().any(|(m, n)| *m == module && *n == name)
        || (RUNTIME_MODULES.contains(&module) && module != "hd:rt")
        || TABLE.iter().any(|t| t.methods.iter().any(|m| imports_of(t, m).iter().any(|i| i.module == module && i.name == name)))
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
    use super::{Handle, TABLE, imports_of, is_known_import};

    #[test]
    fn waiting_methods_import_a_start_finish_pair() {
        let fs = TABLE.iter().find(|t| t.key == "FsRead").expect("FsRead");
        let i = imports_of(fs, &fs.methods[1]);
        assert_eq!(i.len(), 2);
        assert_eq!((i[0].module.as_str(), i[0].name.as_str()), ("hd:FsRead", "read_text.start"));
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
    fn table_mirrors_std_capability_traits() {
        let std = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../lib/std");
        for t in TABLE {
            let (module, name) = t.std_path.rsplit_once('.').expect("path");
            let file = std.join(format!("{}.hd", module.trim_start_matches("std.").replace('.', "/")));
            let text = std::fs::read_to_string(&file).expect("std file");
            assert!(text.contains(&format!("pub trait {name}:")), "{} not in {}", t.std_path, file.display());
            for m in t.methods {
                assert!(text.contains(&format!("fn {}", m.name)), "{}.{} missing", t.key, m.name);
            }
        }
    }
}
