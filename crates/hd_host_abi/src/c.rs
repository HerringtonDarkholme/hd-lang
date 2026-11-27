//! C provider tables (`module.host.package.c-types`,
//! `docs/future-work/FFI.md`): the function table a C library returns for
//! one package host trait, method by method in source order, with the C
//! type of each parameter and result. A module records one table per
//! reachable C-bound root trait in its [`SECTION`], beside the package
//! host contract; the table's [`Table::hash`] is the `contract_hash` a
//! library's table must carry.
//!
//! Cost: encoding and hashing are one pass over the table's methods and
//! parameters; decoding is one pass over the section's bytes.

use crate::schema::{Reader, Writer};
use std::fmt::Write as _;

/// The custom section that holds a module's C tables.
pub const SECTION: &str = "hd.c";
/// The section encoding's version.
const SECTION_VERSION: u32 = 1;
/// `HD_HOST_ABI_VERSION`: the layout of a C table's fixed head (the
/// version, the hash, `create` and `destroy`) and of a token.
pub const ABI_VERSION: u32 = 1;

/// A number type of the C value table, by its hd name.
/// Its discriminant is its code in the `hd.c` section and the hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Num {
    I8 = 0,
    I16 = 1,
    I32 = 2,
    I64 = 3,
    U8 = 4,
    U16 = 5,
    U32 = 6,
    U64 = 7,
    Usize = 8,
    F32 = 9,
    F64 = 10,
}

impl Num {
    const ALL: [Num; 11] = [
        Num::I8,
        Num::I16,
        Num::I32,
        Num::I64,
        Num::U8,
        Num::U16,
        Num::U32,
        Num::U64,
        Num::Usize,
        Num::F32,
        Num::F64,
    ];

    /// The C type of the same width.
    #[must_use]
    pub fn c_type(self) -> &'static str {
        match self {
            Num::I8 => "int8_t",
            Num::I16 => "int16_t",
            Num::I32 => "int32_t",
            Num::I64 => "int64_t",
            Num::U8 => "uint8_t",
            Num::U16 => "uint16_t",
            Num::U32 => "uint32_t",
            Num::U64 => "uint64_t",
            Num::Usize => "size_t",
            Num::F32 => "float",
            Num::F64 => "double",
        }
    }

    /// Its size in bytes in the guest's linear memory.
    #[must_use]
    pub fn bytes(self) -> u8 {
        match self {
            Num::I8 | Num::U8 => 1,
            Num::I16 | Num::U16 => 2,
            Num::I32 | Num::U32 | Num::Usize | Num::F32 => 4,
            Num::I64 | Num::U64 | Num::F64 => 8,
        }
    }

    fn code(self) -> u8 {
        self as u8
    }

    fn of(code: u8) -> Result<Self, &'static str> {
        Self::ALL
            .get(usize::from(code))
            .copied()
            .ok_or("unknown C table number type")
    }
}

/// A parameter's row of the C value table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Param {
    Bool,
    Num(Num),
    /// `&T` or `&mut T`: `const T *` or `T *`.
    Ref {
        num: Num,
        mutable: bool,
    },
    /// `&List[T]` or `&mut List[T]`: `const T *` or `T *`, then a
    /// `size_t` length.
    List {
        num: Num,
        mutable: bool,
    },
    /// `string`: `const uint8_t *` to its UTF-8 bytes, then a `size_t`
    /// length.
    Str,
}

/// A result's row: what a method returns, or what a suspending method's
/// token completes with.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Ret {
    Void,
    Bool,
    Num(Num),
    /// `List[u8]`, completed with copied bytes.
    Bytes,
    /// `Result[T, i32]`: `T`, or an `i32` error code.
    Result(Box<Ret>),
    /// A returned host handle of the C-bound handle trait of this name,
    /// in the same library: `void *`, the library's handle object
    /// (`module.host.package.c-handles`).
    Handle(String),
}

/// One method of a C table, at its slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Method {
    /// The method's source name, which is its slot's field name.
    pub name: String,
    pub params: Vec<(String, Param)>,
    pub result: Ret,
    /// A suspending method's slot takes a token, and an abort slot follows
    /// it.
    pub suspends: bool,
}

/// The C table of one package host trait.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    /// The trait's import module in the package host contract.
    pub module: String,
    /// The trait's own name, which names its entry symbol.
    pub trait_name: String,
    /// Whether it is a returned handle's trait, whose objects come from a
    /// method's result rather than its table's `create`.
    pub handle: bool,
    /// Every method of the trait, in source order.
    pub methods: Vec<Method>,
}

impl Table {
    /// The trait's `contract_hash`: xxh3-128 of the ABI version and each
    /// method's name, suspension and C types in order. Parameter names
    /// do not enter it, so renaming one keeps a built library valid.
    #[must_use]
    pub fn hash(&self) -> [u8; 16] {
        let mut h = hd_base::StableHasher::new("hd-c-table");
        h.u32(ABI_VERSION);
        h.u8(u8::from(self.handle));
        h.len(self.methods.len());
        for m in &self.methods {
            h.str(&m.name);
            h.u8(u8::from(m.suspends));
            h.len(m.params.len());
            for (_, p) in &m.params {
                let mut w = Writer(Vec::new());
                param(&mut w, *p);
                h.bytes(&w.0);
            }
            let mut w = Writer(Vec::new());
            ret(&mut w, &m.result);
            h.bytes(&w.0);
        }
        h.finish().0.to_le_bytes()
    }

    /// The slot index of method `name` among the table's method slots:
    /// a suspending method's abort slot takes the index after it.
    #[must_use]
    pub fn slot(&self, name: &str) -> Option<usize> {
        let mut at = 0;
        for m in &self.methods {
            if m.name == name {
                return Some(at);
            }
            at += 1 + usize::from(m.suspends);
        }
        None
    }
}

fn param(w: &mut Writer, p: Param) {
    match p {
        Param::Bool => w.0.push(0),
        Param::Num(n) => w.0.extend_from_slice(&[1, n.code()]),
        Param::Ref { num, mutable } => w.0.extend_from_slice(&[2, num.code(), u8::from(mutable)]),
        Param::List { num, mutable } => w.0.extend_from_slice(&[3, num.code(), u8::from(mutable)]),
        Param::Str => w.0.push(4),
    }
}

fn ret(w: &mut Writer, r: &Ret) {
    match r {
        Ret::Void => w.0.push(0),
        Ret::Bool => w.0.push(1),
        Ret::Num(n) => w.0.extend_from_slice(&[2, n.code()]),
        Ret::Bytes => w.0.push(3),
        Ret::Result(inner) => {
            w.0.push(4);
            ret(w, inner);
        }
        Ret::Handle(name) => {
            w.0.push(5);
            w.string(name);
        }
    }
}

fn read_param(r: &mut Reader<'_>) -> Result<Param, &'static str> {
    Ok(match r.byte()? {
        0 => Param::Bool,
        1 => Param::Num(Num::of(r.byte()?)?),
        2 => Param::Ref {
            num: Num::of(r.byte()?)?,
            mutable: r.boolean()?,
        },
        3 => Param::List {
            num: Num::of(r.byte()?)?,
            mutable: r.boolean()?,
        },
        4 => Param::Str,
        _ => return Err("unknown C table parameter"),
    })
}

fn read_ret(r: &mut Reader<'_>, nested: bool) -> Result<Ret, &'static str> {
    Ok(match r.byte()? {
        0 => Ret::Void,
        1 => Ret::Bool,
        2 => Ret::Num(Num::of(r.byte()?)?),
        3 => Ret::Bytes,
        // A completion's `Result` holds a number or bytes, never another.
        4 if !nested => Ret::Result(Box::new(read_ret(r, true)?)),
        5 if !nested => Ret::Handle(r.string()?),
        _ => return Err("unknown C table result"),
    })
}

/// The section's bytes for `tables`, which the caller sorts by module.
#[must_use]
pub fn encode(tables: &[Table]) -> Vec<u8> {
    let mut w = Writer(Vec::new());
    w.0.extend_from_slice(&SECTION_VERSION.to_le_bytes());
    w.len(tables.len());
    for t in tables {
        w.string(&t.module);
        w.string(&t.trait_name);
        w.0.push(u8::from(t.handle));
        w.len(t.methods.len());
        for m in &t.methods {
            w.string(&m.name);
            w.0.push(u8::from(m.suspends));
            w.len(m.params.len());
            for (name, p) in &m.params {
                w.string(name);
                param(&mut w, *p);
            }
            ret(&mut w, &m.result);
        }
    }
    w.0
}

/// The tables of a section, refusing an unknown version, truncation,
/// trailing bytes and noncanonical bytes.
pub fn decode(bytes: &[u8]) -> Result<Vec<Table>, &'static str> {
    let mut r = Reader { bytes, at: 0 };
    if r.take(4)? != SECTION_VERSION.to_le_bytes() {
        return Err("unsupported C table section version");
    }
    let mut tables = Vec::new();
    for _ in 0..r.len()? {
        let module = r.string()?;
        let trait_name = r.string()?;
        let handle = r.boolean()?;
        let mut methods = Vec::new();
        for _ in 0..r.len()? {
            let name = r.string()?;
            let suspends = r.boolean()?;
            let params = (0..r.len()?)
                .map(|_| Ok((r.string()?, read_param(&mut r)?)))
                .collect::<Result<_, &'static str>>()?;
            methods.push(Method {
                name,
                params,
                result: read_ret(&mut r, false)?,
                suspends,
            });
        }
        tables.push(Table {
            module,
            trait_name,
            handle,
            methods,
        });
    }
    if r.at != bytes.len() {
        return Err("trailing C table section bytes");
    }
    if encode(&tables) != bytes {
        return Err("noncanonical C table section");
    }
    Ok(tables)
}

/// C11's keywords and the names a table's head or a slot's parameters
/// take, which a source name may not reuse as written.
const RESERVED: &[&str] = &[
    "_Alignas",
    "_Alignof",
    "_Atomic",
    "_Bool",
    "_Complex",
    "_Generic",
    "_Imaginary",
    "_Noreturn",
    "_Static_assert",
    "_Thread_local",
    "abi_version",
    "auto",
    "bool",
    "break",
    "case",
    "char",
    "completed",
    "const",
    "contract_hash",
    "continue",
    "create",
    "default",
    "destroy",
    "do",
    "double",
    "else",
    "enum",
    "extern",
    "false",
    "float",
    "for",
    "goto",
    "if",
    "inline",
    "int",
    "long",
    "register",
    "restrict",
    "return",
    "self",
    "short",
    "signed",
    "sizeof",
    "static",
    "struct",
    "switch",
    "token",
    "true",
    "typedef",
    "union",
    "unsigned",
    "void",
    "volatile",
    "while",
];

/// `name` as a C identifier: ASCII as written, any other character as a
/// universal character name (C11 6.4.3), and a reserved name with `_`
/// after it. An hd name is a valid identifier, so the result is one.
fn c_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii() {
            out.push(ch);
        } else if u32::from(ch) <= 0xffff {
            let _ = write!(out, "\\u{:04x}", u32::from(ch));
        } else {
            let _ = write!(out, "\\U{:08x}", u32::from(ch));
        }
    }
    if RESERVED.contains(&out.as_str()) {
        out.push('_');
    }
    out
}

/// A name not yet in `taken`, recorded there: `name`, else `name` with
/// one more `_` until it is free.
fn fresh(taken: &mut std::collections::BTreeSet<String>, mut name: String) -> String {
    while !taken.insert(name.clone()) {
        name.push('_');
    }
    name
}

/// The C type a method returns at once.
fn c_return(r: &Ret) -> &'static str {
    match r {
        Ret::Bool => "bool",
        Ret::Num(n) => n.c_type(),
        Ret::Handle(_) => "void *",
        // A suspending method's slot returns at once; its result comes
        // through the token.
        Ret::Void | Ret::Bytes | Ret::Result(_) => "void",
    }
}

/// The C header of a package's C-bound traits, `native/include/<package>.h`
/// (`cli.host.c.header`): for each trait, with the library that provides
/// it, its function table in source order and its one entry symbol
/// `LIBRARY_TRAIT`, after the token and completion functions every table
/// shares. The same tables give the same bytes. `tables` holds each
/// trait's path, library and table, in the order the header lists them.
///
/// Cost: one pass over the tables' methods and parameters.
#[must_use]
pub fn header(package: &str, tables: &[(&str, &str, &Table)]) -> String {
    let guard: String = std::iter::once("HD_".to_owned())
        .chain(package.chars().map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase().to_string()
            } else {
                "_".to_owned()
            }
        }))
        .chain(std::iter::once("_H".to_owned()))
        .collect();
    let mut out = String::new();
    let _ = write!(
        out,
        "/* Generated by `hd build` from package `{}`; do not edit.\n * A program checks each table's abi_version and contract_hash when it\n * starts, and refuses a table built for another version of its trait. */\n#ifndef {guard}\n#define {guard}\n\n#include <stdbool.h>\n#include <stddef.h>\n#include <stdint.h>\n\n#ifdef __cplusplus\nextern \"C\" {{\n#endif\n\n",
        package.replace("*/", "* /")
    );
    let _ = write!(
        out,
        "#ifndef HD_HOST_ABI_VERSION\n#define HD_HOST_ABI_VERSION {ABI_VERSION}u\n\n/* A suspending call's token. Complete it at most once, from any thread,\n * and use it only until that completion returns or until its abort slot\n * returns. A completion returns true when the host accepted the value,\n * and false when the call was already aborting or completed: the library\n * then keeps the value. */\ntypedef struct hd_call *hd_token;\n\ntypedef struct hd_completions {{\n    bool (*complete_i32)(hd_token token, int32_t value);\n    bool (*complete_i64)(hd_token token, int64_t value);\n    bool (*complete_f64)(hd_token token, double value);\n    bool (*complete_bytes)(hd_token token, const uint8_t *bytes, size_t len);\n    bool (*complete_error)(hd_token token, int32_t error);\n}} hd_completions;\n\n/* The host's call: its completions first, then the host's own state. */\nstruct hd_call {{\n    const hd_completions *completions;\n}};\n\n/* An integer of 32 bits or fewer. */\nstatic inline bool hd_complete_i32(hd_token token, int32_t value) {{\n    return token->completions->complete_i32(token, value);\n}}\n\n/* A 64-bit integer. */\nstatic inline bool hd_complete_i64(hd_token token, int64_t value) {{\n    return token->completions->complete_i64(token, value);\n}}\n\n/* A float. */\nstatic inline bool hd_complete_f64(hd_token token, double value) {{\n    return token->completions->complete_f64(token, value);\n}}\n\n/* A `List[u8]`: the host copies the bytes before it returns. */\nstatic inline bool hd_complete_bytes(hd_token token, const uint8_t *bytes, size_t len) {{\n    return token->completions->complete_bytes(token, bytes, len);\n}}\n\n/* A `Result`'s `i32` error. */\nstatic inline bool hd_complete_error(hd_token token, int32_t error) {{\n    return token->completions->complete_error(token, error);\n}}\n#endif /* HD_HOST_ABI_VERSION */\n"
    );
    for (path, library, table) in tables {
        let name = c_name(&table.trait_name);
        let entry = format!("{library}_{name}");
        let (made, ends) = if table.handle {
            (
                "NULL: a method's result makes each handle",
                "at the handle's last release",
            )
        } else {
            ("at instance start", "at instance close, trap or not")
        };
        let _ = write!(
            out,
            "\n/* {path}: its function table, in source order. */\ntypedef struct hd_{name} {{\n    uint32_t abi_version; /* HD_HOST_ABI_VERSION */\n    uint8_t contract_hash[16]; /* HD_CONTRACT_HASH_{entry} */\n    void *(*create)(void); /* {made} */\n    void (*destroy)(void *self); /* {ends} */\n"
        );
        let mut fields: std::collections::BTreeSet<String> =
            ["abi_version", "contract_hash", "create", "destroy"]
                .map(str::to_owned)
                .into();
        for m in &table.methods {
            let slot = fresh(&mut fields, c_name(&m.name));
            let mut params = vec!["void *self".to_owned()];
            let mut taken: std::collections::BTreeSet<String> =
                ["self", "token"].map(str::to_owned).into();
            if m.suspends {
                params.push("hd_token token".to_owned());
            }
            for (pname, p) in &m.params {
                let pname = fresh(&mut taken, c_name(pname));
                match *p {
                    Param::Bool => params.push(format!("bool {pname}")),
                    Param::Num(n) => params.push(format!("{} {pname}", n.c_type())),
                    Param::Ref { num, mutable } => params.push(format!(
                        "{}{} *{pname}",
                        if mutable { "" } else { "const " },
                        num.c_type()
                    )),
                    Param::List { num, mutable } => {
                        let len = fresh(&mut taken, format!("{pname}_len"));
                        params.push(format!(
                            "{}{} *{pname}",
                            if mutable { "" } else { "const " },
                            num.c_type()
                        ));
                        params.push(format!("size_t {len}"));
                    }
                    Param::Str => {
                        let len = fresh(&mut taken, format!("{pname}_len"));
                        params.push(format!("const uint8_t *{pname}"));
                        params.push(format!("size_t {len}"));
                    }
                }
            }
            let ret = if m.suspends {
                "void"
            } else {
                c_return(&m.result)
            };
            let note = if m.suspends {
                " /* returns at once; complete the token */"
            } else {
                ""
            };
            // A pointer result binds to the slot's name, as `void *(*open)(...)`.
            let gap = if ret.ends_with('*') { "" } else { " " };
            let _ = writeln!(out, "    {ret}{gap}(*{slot})({});{note}", params.join(", "));
            if m.suspends {
                let abort = fresh(&mut fields, format!("{slot}_abort"));
                let _ = writeln!(
                    out,
                    "    void (*{abort})(void *self, hd_token token, bool completed); /* completed: a completed result was discarded */"
                );
            }
        }
        let _ = write!(
            out,
            "}} hd_{name};\n\n/* The library's one entry symbol for {path}. */\nconst hd_{name} *{entry}(void);\n\n/* {path}'s contract_hash, for its table's initializer. */\n#define HD_CONTRACT_HASH_{entry} {{"
        );
        for (k, b) in table.hash().iter().enumerate() {
            let _ = write!(out, "{}0x{b:02x}", if k == 0 { " " } else { ", " });
        }
        out.push_str(" }\n");
    }
    let _ = write!(
        out,
        "\n#ifdef __cplusplus\n}}\n#endif\n\n#endif /* {guard} */\n"
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Table {
        Table {
            module: "hd:pkg:00".into(),
            trait_name: "Disk".into(),
            handle: false,
            methods: vec![
                Method {
                    name: "write".into(),
                    params: vec![
                        ("path".into(), Param::Str),
                        (
                            "data".into(),
                            Param::List {
                                num: Num::U8,
                                mutable: false,
                            },
                        ),
                        (
                            "at".into(),
                            Param::Ref {
                                num: Num::U64,
                                mutable: true,
                            },
                        ),
                    ],
                    result: Ret::Num(Num::I32),
                    suspends: false,
                },
                Method {
                    name: "read".into(),
                    params: vec![
                        ("ms".into(), Param::Num(Num::F64)),
                        ("on".into(), Param::Bool),
                    ],
                    result: Ret::Result(Box::new(Ret::Bytes)),
                    suspends: true,
                },
                Method {
                    name: "close".into(),
                    params: Vec::new(),
                    result: Ret::Void,
                    suspends: false,
                },
            ],
        }
    }

    #[test]
    fn tables_round_trip_and_refuse_hostile_bytes() {
        let tables = vec![sample()];
        let bytes = encode(&tables);
        assert_eq!(decode(&bytes), Ok(tables));
        for n in 0..bytes.len() {
            assert!(decode(&bytes[..n]).is_err(), "prefix {n}");
        }
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(decode(&trailing).is_err());
    }

    /// A suspending method's abort slot follows it.
    #[test]
    fn a_suspending_method_takes_two_slots() {
        let t = sample();
        assert_eq!(
            ["write", "read", "close", "open"].map(|m| t.slot(m)),
            [Some(0), Some(1), Some(3), None]
        );
    }

    /// A table whose names are C keywords, clash with the table's own
    /// fields or slot parameters, or are not ASCII.
    fn awkward() -> Table {
        Table {
            module: "hd:pkg:01".into(),
            trait_name: "Dätä".into(),
            handle: false,
            methods: vec![
                Method {
                    name: "default".into(),
                    params: vec![
                        (
                            "int".into(),
                            Param::List {
                                num: Num::I16,
                                mutable: true,
                            },
                        ),
                        ("int_len".into(), Param::Num(Num::Usize)),
                        ("self".into(), Param::Bool),
                        ("ünï".into(), Param::Num(Num::I8)),
                    ],
                    result: Ret::Bool,
                    suspends: false,
                },
                Method {
                    name: "create".into(),
                    params: vec![("token".into(), Param::Num(Num::U16))],
                    result: Ret::Num(Num::F32),
                    suspends: true,
                },
                Method {
                    name: "create_abort".into(),
                    params: Vec::new(),
                    result: Ret::Void,
                    suspends: false,
                },
            ],
        }
    }

    /// `cli.host.c.header`: the same tables give the same bytes, and the
    /// header compiles with the system C compiler under `-Werror`, beside a
    /// library that fills each table and exports its entry symbol.
    #[test]
    fn a_generated_header_compiles_and_a_library_fills_its_tables() {
        let (disk, odd) = (sample(), awkward());
        let tables = [
            ("main.Disk", "hd_disk", &disk),
            ("main.Dätä", "hd_disk", &odd),
        ];
        let text = header("disk-sdk", &tables);
        assert_eq!(text, header("disk-sdk", &tables));
        assert!(text.contains("const hd_Disk *hd_disk_Disk(void);"));
        assert!(text.contains("    int32_t (*write)(void *self, const uint8_t *path, size_t path_len, const uint8_t *data, size_t data_len, uint64_t *at);"));
        assert!(text.contains("    void (*read)(void *self, hd_token token, double ms, bool on);"));
        assert!(
            text.contains("    void (*read_abort)(void *self, hd_token token, bool completed);")
        );
        let library = r#"
#include "disk-sdk.h"

static int state;

static void *create(void) { return &state; }
static void destroy(void *self) { (void)self; }

static int32_t disk_write(void *self, const uint8_t *path, size_t path_len,
                          const uint8_t *data, size_t data_len, uint64_t *at) {
    (void)self; (void)path; (void)data;
    *at += 1;
    return (int32_t)(path_len + data_len);
}
static void disk_read(void *self, hd_token token, double ms, bool on) {
    (void)self; (void)ms;
    static const uint8_t bytes[1] = {7};
    if (on) { hd_complete_bytes(token, bytes, 1); } else { hd_complete_error(token, -1); }
}
static void disk_read_abort(void *self, hd_token token, bool completed) {
    (void)self; (void)token; (void)completed;
}
static void disk_close(void *self) { (void)self; }

static const hd_Disk disk = {
    HD_HOST_ABI_VERSION, HD_CONTRACT_HASH_hd_disk_Disk, create, destroy,
    disk_write, disk_read, disk_read_abort, disk_close,
};
const hd_Disk *hd_disk_Disk(void) { return &disk; }

static bool odd_default(void *self, int16_t *int_, size_t int_len, size_t int_len_,
                        bool self_, int8_t ünï) {
    (void)self; (void)self_; (void)ünï;
    if (int_len > 0) { int_[0] = (int16_t)int_len_; }
    return true;
}
static void odd_create(void *self, hd_token token, uint16_t token_) {
    (void)self;
    hd_complete_f64(token, token_);
}
static void odd_create_abort(void *self, hd_token token, bool completed) {
    (void)self; (void)token; (void)completed;
}
static void odd_create_abort_(void *self) { (void)self; }

static const hd_Dätä odd = {
    HD_HOST_ABI_VERSION, HD_CONTRACT_HASH_hd_disk_Dätä, create, destroy,
    odd_default, odd_create, odd_create_abort, odd_create_abort_,
};
const hd_Dätä *hd_disk_Dätä(void) { return &odd; }
"#;
        let dir = std::env::temp_dir().join(format!("hd-c-header-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("disk-sdk.h"), &text).unwrap();
        std::fs::write(dir.join("disk.c"), library).unwrap();
        // cost: one C compiler process for this test's one source file.
        let out = std::process::Command::new(std::env::var("CC").unwrap_or_else(|_| "cc".into()))
            .current_dir(&dir)
            .args([
                "-std=c11",
                "-Wall",
                "-Wextra",
                "-Werror",
                "-pedantic",
                "-c",
                "disk.c",
                "-o",
                "disk.o",
            ])
            .output()
            .expect("the system C compiler, `cc` or $CC");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            out.status.success(),
            "{}\n{text}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// A number type's code is its discriminant, and `of` reads it back:
    /// the codes `hd.c` and the hash use, unchanged.
    #[test]
    fn a_number_types_code_is_its_discriminant() {
        for (k, num) in Num::ALL.into_iter().enumerate() {
            assert_eq!(usize::from(num.code()), k);
            assert_eq!(Num::of(num.code()), Ok(num));
        }
    }

    /// The hash covers signatures, not parameter names.
    #[test]
    fn the_hash_changes_with_a_signature_not_a_parameter_name() {
        let base = sample();
        let mut renamed = sample();
        renamed.methods[0].params[0].0 = "file".into();
        assert_eq!(base.hash(), renamed.hash());
        let mut retyped = sample();
        retyped.methods[1].params[0].1 = Param::Num(Num::F32);
        assert_ne!(base.hash(), retyped.hash());
        let mut reordered = sample();
        reordered.methods.swap(0, 2);
        assert_ne!(base.hash(), reordered.hash());
        let mut suspended = sample();
        suspended.methods[2].suspends = true;
        assert_ne!(base.hash(), suspended.hash());
    }
}
