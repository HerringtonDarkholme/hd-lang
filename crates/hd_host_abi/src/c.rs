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

/// The custom section that holds a module's C tables.
pub const SECTION: &str = "hd.c";
/// The section encoding's version.
const SECTION_VERSION: u32 = 1;
/// `HD_HOST_ABI_VERSION`: the layout of a C table's fixed head (the
/// version, the hash, `create` and `destroy`) and of a token.
pub const ABI_VERSION: u32 = 1;

/// A number type of the C value table, by its hd name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Num {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    Usize,
    F32,
    F64,
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
        // ruled out: `ALL` lists every variant once.
        u8::try_from(Self::ALL.iter().position(|n| *n == self).unwrap_or(0)).unwrap_or(0)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Table {
        Table {
            module: "hd:pkg:00".into(),
            trait_name: "Disk".into(),
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
