//! Owned signatures for package host adapters. The custom section contains
//! data, never executable JavaScript. Its encoding is toolchain ABI v1.

use std::collections::BTreeSet;

pub const SECTION: &str = "hd.capabilities";
pub const VERSION: u32 = 1;
const MAX_DEPTH: usize = 32;

/// The exact selected package, version, and package-relative trait name.
/// Local packages use a directory hash, never an absolute path here.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Identity {
    pub package: String,
    pub version: String,
    pub trait_path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Void,
    Bool,
    Char,
    Int {
        bits: u8,
        signed: bool,
    },
    F32,
    F64,
    String,
    Bytes,
    List(Box<Self>),
    Option(Box<Self>),
    Tuple(Vec<Self>),
    Record(Vec<Field>),
    Enum(Vec<Variant>),
    Result(Box<Self>, Box<Self>),
    /// A private-field boundary's Serialize/Deserialize value (typed transport).
    Json,
    /// The import module of a host-returned handle interface.
    Handle(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub value: Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Variant {
    pub name: String,
    pub fields: Vec<Field>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Method {
    /// Wasm import name, qualified for an inherited method when necessary.
    pub name: String,
    /// The plain JavaScript method name, independent of import qualification.
    pub member: String,
    pub params: Vec<Value>,
    pub result: Value,
    pub suspends: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Interface {
    pub identity: Identity,
    pub module: String,
    /// Root-program source paths for explicit registration, not identity.
    pub selectors: Vec<String>,
    /// Handles come from provider results, rather than startup registration.
    pub handle: bool,
    /// Reachable slots only, in source order. Uncalled slots have no import.
    pub methods: Vec<Method>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Contract {
    /// Emission sorts these by nominal identity and role.
    pub interfaces: Vec<Interface>,
}

impl Contract {
    /// Only the imports described by this contract, including handle release.
    #[must_use]
    pub fn contains_import(&self, module: &str, name: &str) -> bool {
        self.interfaces.iter().any(|i| {
            i.module == module
                && ((i.handle && name == "$release")
                    || i.methods.iter().any(|m| {
                        if m.suspends {
                            name.strip_suffix(".start") == Some(m.name.as_str())
                                || name.strip_suffix(".finish") == Some(m.name.as_str())
                        } else {
                            name == m.name
                        }
                    }))
        })
    }
    /// A deterministic, length-delimited representation. The compiler
    /// validates its own constructed contract before embedding these bytes.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer(Vec::new());
        w.0.extend_from_slice(&VERSION.to_le_bytes());
        w.len(self.interfaces.len());
        for i in &self.interfaces {
            w.string(&i.identity.package);
            w.string(&i.identity.version);
            w.string(&i.identity.trait_path);
            w.string(&i.module);
            w.len(i.selectors.len());
            for selector in &i.selectors {
                w.string(selector);
            }
            w.0.push(u8::from(i.handle));
            w.len(i.methods.len());
            for m in &i.methods {
                w.string(&m.name);
                w.string(&m.member);
                w.0.push(u8::from(m.suspends));
                w.values(&m.params);
                w.value(&m.result);
            }
        }
        w.0
    }

    /// Reject unknown versions/tags, truncation, trailing bytes, identity
    /// collisions, missing handle interfaces, and recursive/deep schemas.
    pub fn decode(bytes: &[u8]) -> Result<Self, &'static str> {
        let mut r = Reader { bytes, at: 0 };
        let version = r.take(4)?;
        if version != VERSION.to_le_bytes() {
            return Err("unsupported package host ABI version");
        }
        let mut interfaces = Vec::new();
        for _ in 0..r.len()? {
            let identity = Identity {
                package: r.string()?,
                version: r.string()?,
                trait_path: r.string()?,
            };
            let module = r.string()?;
            let selectors = (0..r.len()?)
                .map(|_| r.string())
                .collect::<Result<Vec<_>, _>>()?;
            let handle = r.boolean()?;
            let mut methods = Vec::new();
            for _ in 0..r.len()? {
                methods.push(Method {
                    name: r.string()?,
                    member: r.string()?,
                    suspends: r.boolean()?,
                    params: r.values(0)?,
                    result: r.value(0)?,
                });
            }
            interfaces.push(Interface {
                identity,
                module,
                selectors,
                handle,
                methods,
            });
        }
        if r.at != bytes.len() {
            return Err("trailing package host contract bytes");
        }
        let contract = Self { interfaces };
        contract.validate()?;
        if contract.encode() != bytes {
            return Err("noncanonical package host contract");
        }
        Ok(contract)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        let mut identities = BTreeSet::new();
        let mut modules = BTreeSet::new();
        let mut selectors = BTreeSet::new();
        for i in &self.interfaces {
            if i.identity.package.is_empty()
                || i.identity.trait_path.is_empty()
                || !i.module.starts_with("hd:pkg:")
                || i.module.len() == "hd:pkg:".len()
            {
                return Err("invalid package host identity or namespace");
            }
            if !identities.insert((&i.identity, i.handle)) || !modules.insert(&i.module) {
                return Err("duplicate package host identity or namespace");
            }
            for selector in &i.selectors {
                let parts: Vec<_> = selector.split('.').collect();
                if i.handle
                    || parts.len() < 2
                    || !matches!(parts[0], "pkg" | "dep")
                    || parts.iter().any(|p| p.is_empty())
                    || !selectors.insert(selector)
                {
                    return Err("invalid or duplicate package host selector");
                }
            }
            let mut names = BTreeSet::new();
            let mut imports = BTreeSet::new();
            if i.handle {
                imports.insert("$release".to_owned());
            }
            for m in &i.methods {
                if m.name.is_empty() || m.member.is_empty() || !names.insert(&m.name) {
                    return Err("empty or duplicate package host method");
                }
                let expanded = if m.suspends {
                    vec![format!("{}.start", m.name), format!("{}.finish", m.name)]
                } else {
                    vec![m.name.clone()]
                };
                for name in expanded {
                    if !imports.insert(name) {
                        return Err("colliding package host import names");
                    }
                }
                for v in m.params.iter().chain(std::iter::once(&m.result)) {
                    self.validate_value(v, 0)?;
                }
            }
        }
        Ok(())
    }

    fn validate_value(&self, v: &Value, depth: usize) -> Result<(), &'static str> {
        if depth > MAX_DEPTH {
            return Err("package host value nesting exceeds the boundary limit");
        }
        match v {
            Value::Int { bits, .. } if ![8, 16, 32, 64].contains(bits) => {
                return Err("invalid package host integer width");
            }
            Value::Handle(module)
                if !self
                    .interfaces
                    .iter()
                    .any(|i| i.handle && &i.module == module) =>
            {
                return Err("missing package host handle interface");
            }
            Value::List(t) | Value::Option(t) => self.validate_value(t, depth + 1)?,
            Value::Result(a, b) => {
                self.validate_value(a, depth + 1)?;
                self.validate_value(b, depth + 1)?;
            }
            Value::Tuple(ts) => {
                for t in ts {
                    self.validate_value(t, depth + 1)?;
                }
            }
            Value::Record(fs) => self.validate_fields(fs, depth + 1)?,
            Value::Enum(vs) => {
                let mut names = BTreeSet::new();
                for variant in vs {
                    if variant.name.is_empty() || !names.insert(&variant.name) {
                        return Err("empty or duplicate package host variant");
                    }
                    self.validate_fields(&variant.fields, depth + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn validate_fields(&self, fs: &[Field], depth: usize) -> Result<(), &'static str> {
        let mut names = BTreeSet::new();
        for f in fs {
            if f.name.is_empty() || !names.insert(&f.name) {
                return Err("empty or duplicate package host field");
            }
            self.validate_value(&f.value, depth)?;
        }
        Ok(())
    }
}

struct Writer(Vec<u8>);
impl Writer {
    fn len(&mut self, n: usize) {
        let mut n = u32::try_from(n).expect("Wasm contract length");
        loop {
            let mut b = (n & 127) as u8;
            n >>= 7;
            if n != 0 {
                b |= 128;
            }
            self.0.push(b);
            if n == 0 {
                break;
            }
        }
    }
    fn string(&mut self, s: &str) {
        self.len(s.len());
        self.0.extend_from_slice(s.as_bytes());
    }
    fn values(&mut self, vs: &[Value]) {
        self.len(vs.len());
        for v in vs {
            self.value(v);
        }
    }
    fn fields(&mut self, fs: &[Field]) {
        self.len(fs.len());
        for f in fs {
            self.string(&f.name);
            self.value(&f.value);
        }
    }
    fn value(&mut self, v: &Value) {
        self.0.push(match v {
            Value::Void => 0,
            Value::Bool => 1,
            Value::Char => 2,
            Value::Int { .. } => 3,
            Value::F32 => 4,
            Value::F64 => 5,
            Value::String => 6,
            Value::Bytes => 7,
            Value::List(_) => 8,
            Value::Option(_) => 9,
            Value::Tuple(_) => 10,
            Value::Record(_) => 11,
            Value::Enum(_) => 12,
            Value::Result(..) => 13,
            Value::Json => 14,
            Value::Handle(_) => 15,
        });
        match v {
            Value::Int { bits, signed } => self.0.extend_from_slice(&[*bits, u8::from(*signed)]),
            Value::List(t) | Value::Option(t) => self.value(t),
            Value::Tuple(ts) => self.values(ts),
            Value::Record(fs) => self.fields(fs),
            Value::Enum(vs) => {
                self.len(vs.len());
                for variant in vs {
                    self.string(&variant.name);
                    self.fields(&variant.fields);
                }
            }
            Value::Result(a, b) => {
                self.value(a);
                self.value(b);
            }
            Value::Handle(module) => self.string(module),
            _ => {}
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], &'static str> {
        let end = self
            .at
            .checked_add(n)
            .ok_or("oversized package host contract")?;
        let b = self
            .bytes
            .get(self.at..end)
            .ok_or("truncated package host contract")?;
        self.at = end;
        Ok(b)
    }
    fn byte(&mut self) -> Result<u8, &'static str> {
        Ok(self.take(1)?[0])
    }
    fn boolean(&mut self) -> Result<bool, &'static str> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err("invalid package host boolean"),
        }
    }
    fn len(&mut self) -> Result<usize, &'static str> {
        let mut n = 0_u32;
        for shift in (0..35).step_by(7) {
            let b = self.byte()?;
            if shift == 28 && b > 15 {
                return Err("oversized package host contract length");
            }
            n |= u32::from(b & 127) << shift;
            if b < 128 {
                let n = n as usize;
                return if n <= self.bytes.len() - self.at {
                    Ok(n)
                } else {
                    Err("truncated package host contract vector")
                };
            }
        }
        Err("invalid package host contract length")
    }
    fn string(&mut self) -> Result<String, &'static str> {
        let n = self.len()?;
        String::from_utf8(self.take(n)?.to_vec())
            .map_err(|_| "invalid UTF-8 in package host contract")
    }
    fn values(&mut self, depth: usize) -> Result<Vec<Value>, &'static str> {
        (0..self.len()?).map(|_| self.value(depth)).collect()
    }
    fn fields(&mut self, depth: usize) -> Result<Vec<Field>, &'static str> {
        (0..self.len()?)
            .map(|_| {
                Ok(Field {
                    name: self.string()?,
                    value: self.value(depth + 1)?,
                })
            })
            .collect()
    }
    fn value(&mut self, depth: usize) -> Result<Value, &'static str> {
        if depth > MAX_DEPTH {
            return Err("package host value nesting exceeds the boundary limit");
        }
        Ok(match self.byte()? {
            0 => Value::Void,
            1 => Value::Bool,
            2 => Value::Char,
            3 => Value::Int {
                bits: self.byte()?,
                signed: self.boolean()?,
            },
            4 => Value::F32,
            5 => Value::F64,
            6 => Value::String,
            7 => Value::Bytes,
            8 => Value::List(Box::new(self.value(depth + 1)?)),
            9 => Value::Option(Box::new(self.value(depth + 1)?)),
            10 => Value::Tuple(self.values(depth + 1)?),
            11 => Value::Record(self.fields(depth)?),
            12 => Value::Enum(
                (0..self.len()?)
                    .map(|_| {
                        Ok(Variant {
                            name: self.string()?,
                            fields: self.fields(depth)?,
                        })
                    })
                    .collect::<Result<_, &'static str>>()?,
            ),
            13 => Value::Result(
                Box::new(self.value(depth + 1)?),
                Box::new(self.value(depth + 1)?),
            ),
            14 => Value::Json,
            15 => Value::Handle(self.string()?),
            _ => return Err("unknown package host value codec"),
        })
    }
}

/// Read only the package contract; the engine still validates the Wasm.
/// Multiple contract sections are rejected rather than choosing one.
pub fn from_wasm(wasm: &[u8]) -> Result<Option<Contract>, &'static str> {
    if wasm.get(..8) != Some(b"\0asm\x01\0\0\0") {
        return Err("not a core Wasm module");
    }
    let mut r = Reader { bytes: wasm, at: 8 };
    let mut contract = None;
    while r.at < wasm.len() {
        let kind = r.byte()?;
        let size = r.len()?;
        let bytes = r.take(size)?;
        if kind != 0 {
            continue;
        }
        let mut section = Reader { bytes, at: 0 };
        if section.string()? != SECTION {
            continue;
        }
        if contract.is_some() {
            return Err("duplicate package host contract section");
        }
        contract = Some(Contract::decode(&bytes[section.at..])?);
    }
    Ok(contract)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn sample() -> Contract {
        Contract {
            interfaces: vec![
                Interface {
                    identity: Identity {
                        package: "example.org/ui".into(),
                        version: "1.2.3".into(),
                        trait_path: "screen.Surface".into(),
                    },
                    module: "hd:pkg:surface".into(),
                    selectors: vec!["pkg.screen.Surface".into()],
                    handle: false,
                    methods: vec![Method {
                        name: "draw".into(),
                        member: "draw".into(),
                        params: vec![
                            Value::Bool,
                            Value::Char,
                            Value::Int {
                                bits: 64,
                                signed: true,
                            },
                            Value::Int {
                                bits: 8,
                                signed: false,
                            },
                            Value::F32,
                            Value::F64,
                            Value::String,
                            Value::Bytes,
                            Value::List(Box::new(Value::String)),
                            Value::Option(Box::new(Value::Json)),
                            Value::Tuple(vec![
                                Value::Void,
                                Value::Int {
                                    bits: 32,
                                    signed: false,
                                },
                            ]),
                            Value::Record(vec![Field {
                                name: "text".into(),
                                value: Value::String,
                            }]),
                            Value::Enum(vec![Variant {
                                name: "Click".into(),
                                fields: vec![Field {
                                    name: "node".into(),
                                    value: Value::Int {
                                        bits: 64,
                                        signed: false,
                                    },
                                }],
                            }]),
                        ],
                        result: Value::Result(
                            Box::new(Value::Handle("hd:pkg:events".into())),
                            Box::new(Value::String),
                        ),
                        suspends: true,
                    }],
                },
                Interface {
                    identity: Identity {
                        package: "example.org/ui".into(),
                        version: "1.2.3".into(),
                        trait_path: "screen.Events".into(),
                    },
                    module: "hd:pkg:events".into(),
                    selectors: Vec::new(),
                    handle: true,
                    methods: vec![Method {
                        name: "close".into(),
                        member: "close".into(),
                        params: vec![],
                        result: Value::Void,
                        suspends: false,
                    }],
                },
            ],
        }
    }

    #[test]
    fn package_contract_round_trips_all_value_forms() {
        let c = sample();
        assert_eq!(Contract::decode(&c.encode()).unwrap(), c);
        assert_eq!(
            Contract::decode(&Contract::default().encode()).unwrap(),
            Contract::default()
        );
    }

    #[test]
    fn hostile_contract_bytes_never_partially_decode() {
        let bytes = sample().encode();
        for len in 0..bytes.len() {
            assert!(Contract::decode(&bytes[..len]).is_err(), "prefix {len}");
        }
        let mut bad = bytes.clone();
        bad[0] = 255;
        assert!(Contract::decode(&bad).is_err());
        let mut bad = bytes.clone();
        *bad.last_mut().unwrap() = 255;
        assert!(Contract::decode(&bad).is_err());
        let mut bad = bytes;
        bad.push(0);
        assert!(Contract::decode(&bad).is_err());
        let noncanonical = [1, 0, 0, 0, 128, 0];
        assert!(Contract::decode(&noncanonical).is_err());
        let huge = [1, 0, 0, 0, 255, 255, 255, 255, 15];
        assert!(Contract::decode(&huge).is_err());
    }

    #[test]
    fn identities_roles_namespaces_and_handle_references_are_checked() {
        let mut c = sample();
        c.interfaces.push(c.interfaces[0].clone());
        assert!(c.validate().is_err());
        let mut c = sample();
        c.interfaces[1].module = c.interfaces[0].module.clone();
        assert!(c.validate().is_err());
        let mut c = sample();
        c.interfaces.pop();
        assert!(c.validate().is_err());
        let mut c = sample();
        c.interfaces[0].methods[0].params = vec![Value::Int {
            bits: 7,
            signed: false,
        }];
        assert!(Contract::decode(&c.encode()).is_err());
        let mut c = sample();
        let m = c.interfaces[0].methods[0].clone();
        c.interfaces[0].methods.push(m);
        assert!(c.validate().is_err());
    }

    #[test]
    fn boundary_depth_limit_is_the_same_for_parameters_and_results() {
        let mut v = Value::String;
        for _ in 0..MAX_DEPTH {
            v = Value::List(Box::new(v));
        }
        let mut c = sample();
        c.interfaces[0].methods[0].params = vec![v.clone()];
        c.interfaces[0].methods[0].result = v.clone();
        assert_eq!(Contract::decode(&c.encode()).unwrap(), c);
        c.interfaces[0].methods[0].params = vec![Value::List(Box::new(v))];
        assert!(c.validate().is_err());
        assert!(Contract::decode(&c.encode()).is_err());
    }

    #[test]
    fn nominal_versions_and_packages_do_not_collapse_to_the_final_name() {
        let mut c = sample();
        let mut other = c.interfaces[0].clone();
        other.selectors.clear();
        other.identity.version = "1.2.4".into();
        other.module = "hd:pkg:other-version".into();
        c.interfaces.push(other.clone());
        other.identity.package = "example.net/ui".into();
        other.module = "hd:pkg:other-package".into();
        c.interfaces.push(other);
        assert_eq!(Contract::decode(&c.encode()).unwrap(), c);
    }

    #[test]
    fn source_selectors_and_expanded_imports_cannot_alias() {
        let mut c = sample();
        c.interfaces[1].selectors = c.interfaces[0].selectors.clone();
        assert!(c.validate().is_err());
        let mut c = sample();
        c.interfaces[0].selectors = vec!["dep..Server".into()];
        assert!(c.validate().is_err());
        let mut c = sample();
        c.interfaces[1].methods[0].name = "$release".into();
        assert!(c.validate().is_err());
        let mut c = sample();
        let mut method = c.interfaces[0].methods[0].clone();
        method.name.push_str(".start");
        method.suspends = false;
        c.interfaces[0].methods.push(method);
        assert!(c.validate().is_err());
    }

    #[test]
    fn prebuilt_import_authorization_is_exact_and_includes_handle_release() {
        let c = sample();
        let root = &c.interfaces[0];
        let method = &root.methods[0];
        assert!(c.contains_import(&root.module, &format!("{}.start", method.name)));
        assert!(c.contains_import(&root.module, &format!("{}.finish", method.name)));
        assert!(!c.contains_import(&root.module, &method.name));
        assert!(!c.contains_import(&root.module, "$release"));
        assert!(!c.contains_import("hd:pkg:unknown", &format!("{}.start", method.name)));
        assert!(c.contains_import(&c.interfaces[1].module, "$release"));
        assert!(!c.contains_import(&c.interfaces[1].module, "$release.finish"));
    }

    #[test]
    fn wasm_contract_lookup_rejects_duplicate_sections_and_preserves_ordinary_modules() {
        let mut wasm = b"\0asm\x01\0\0\0".to_vec();
        assert_eq!(from_wasm(&wasm).unwrap(), None);
        let c = sample();
        let mut payload = Writer(Vec::new());
        payload.string(SECTION);
        payload.0.extend(c.encode());
        let mut section = Writer(vec![0]);
        section.len(payload.0.len());
        section.0.extend(payload.0);
        wasm.extend_from_slice(&section.0);
        assert_eq!(from_wasm(&wasm).unwrap(), Some(c));
        wasm.extend_from_slice(&section.0);
        assert!(from_wasm(&wasm).is_err());
    }
}
