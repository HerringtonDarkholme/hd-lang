//! The Rust companion `NAME.host.rs` of a module requiring package host
//! traits (`cli.host.package.native-companion`, `native-generated`): one
//! provider trait per host interface, the owned value types its methods
//! cross (the Native Value Table, `module.host.package.native-values`), and
//! a registry builder whose `build` yields the `hd_run_wasmtime::native::
//! Bindings` an embedder passes to `instantiate_with_bindings`.
//!
//! Generated from the checked contract only, never WIT. The text depends on
//! the contract alone, so two builds write the same bytes
//! (`native-deterministic`).
//!
//! Each contract type also gets its codec (`native-generated`): an encode
//! statement over a borrowed value and a decode expression, in the
//! exchange-buffer encoding of the JS host's codecs, through the scalar
//! steps of `hd_run_wasmtime::native::wire`. A record or an enum carries
//! its codec as two methods of its own type, except one inside a recursive
//! group, which its group's codec reads and writes; every other form is
//! written inline where it occurs. Each trait gets one `dispatch_TRAIT`: it decodes
//! a call's arguments, calls the method, and encodes its result, taking
//! the value, so a handle it holds moves into the instance's registry with
//! its trait's index and dispatch. The registry registers a root trait's
//! dispatch with its provider; a handle trait's travels with each handle.
//!
//! Cost: one pass over the contract's values. Records and enums are
//! hash-consed bottom-up (a child's Rust type and codec are rendered before
//! its parent's key is built), so each structural type is named once and
//! every lookup is one hash probe; the boundary depth limit of the contract
//! bounds each key's length. The key holds the decoder as well as the
//! type, so two shapes with one Rust type and two encodings (`Vec<u8>` for
//! bytes and for a list of `u8`) stay two types. A generated codec is
//! linear in the value it reads or writes.
//!
//! A recursive type's codec never recurses natively per nesting level: its
//! group (the `Value::Rec` and every type its body reaches recursively) gets
//! one explicit-stack codec, [`Machine`], so a value as deep as the guest or
//! the provider makes it is read and written, and dismantled on a failure,
//! in constant native stack. The same group's loops clone, compare and
//! print its types (`Clone`, `PartialEq`, `Debug`), and take apart an
//! argument holding one of its values, which a provider only borrows
//! (`&T`) and the dispatch dismantles when it ends.

use crate::schema::{Contract, Field, Interface, Value};
use hd_base::{HashMap, HashSet};
use std::fmt::Write as _;

/// Every runtime name is spelled in full, so the companion imports nothing
/// that an unused trait form would leave unused, and no generated type
/// name can shadow one.
const NATIVE: &str = "hd_run_wasmtime::native";

/// The codec steps a companion calls.
const WIRE: &str = "hd_run_wasmtime::native::wire";

/// Where an encode template puts the borrowed value it writes: always an
/// identifier, never a character a name can hold.
const V: &str = "\u{1}";

/// A contract type's Rust type and codec.
#[derive(Clone)]
struct Ty {
    rust: String,
    /// Statements writing the `T` at [`V`], which they take, to the
    /// encoder `e`.
    enc: String,
    /// An expression reading a `T` from the decoder `d`.
    dec: String,
    /// Statements taking apart the `T` at [`V`], which they take, through
    /// the explicit-stack loop of each recursive group's value it holds;
    /// `None` when it holds none, so dropping it in place is bounded by the
    /// contract's boundary depth limit.
    drop: Option<String>,
}

impl Ty {
    fn new(rust: impl Into<String>, enc: impl Into<String>, dec: impl Into<String>) -> Self {
        Self {
            rust: rust.into(),
            enc: enc.into(),
            dec: dec.into(),
            drop: None,
        }
    }

    fn dropping(mut self, drop: Option<String>) -> Self {
        self.drop = drop;
        self
    }

    /// The statements writing the value bound to `value`.
    fn enc(&self, value: &str) -> String {
        self.enc.replace(V, value)
    }

    /// The statements dismantling the value bound to `value`, or nothing
    /// when it drops in place.
    fn drop(&self, value: &str) -> String {
        self.drop
            .as_ref()
            .map_or_else(String::new, |d| d.replace(V, value))
    }
}

/// The companion text, or `""` for a module with no package host interface.
pub fn generate(contract: &Contract) -> Result<String, &'static str> {
    contract.validate()?;
    if contract.interfaces.is_empty() {
        return Ok(String::new());
    }
    let mut g = Gen::default();
    // The registry's own names and the prelude types the companion
    // spells unqualified: a trait or type of the same name takes a suffix
    // rather than shadow one.
    g.used.extend(
        [
            "Bindings",
            "BindingsBuilder",
            "Box",
            "Option",
            "Result",
            "Self",
            "String",
            "Vec",
        ]
        .map(str::to_owned),
    );
    // Trait names first: handle values name their trait by module. The
    // last path segment names the trait; the compiler never writes an
    // empty one, and a hand-made contract with one has no Rust name.
    let mut traits = Vec::with_capacity(contract.interfaces.len());
    for i in &contract.interfaces {
        let last = i.identity.trait_path.rsplit('.').next().unwrap_or_default();
        if last.is_empty() {
            return Err("package host trait path ends in an empty segment");
        }
        traits.push(g.fresh(&camel(last)));
    }
    for (n, (i, name)) in contract.interfaces.iter().zip(&traits).enumerate() {
        g.handles.insert(i.module.clone(), (name.clone(), n));
    }
    let mut methods = String::new();
    let mut dispatches = String::new();
    for (i, name) in contract.interfaces.iter().zip(&traits) {
        g.interface(&mut methods, &mut dispatches, i, name);
    }

    let mut out = String::from(
        "// Generated by hd; do not edit.\n// Native provider bindings for this module's package host traits.\n\n/// The module's checked package host contract (`hd.capabilities`).\npub const CONTRACT: &[u8] = &[",
    );
    for (n, b) in contract.encode().iter().enumerate() {
        if n != 0 {
            out.push_str(if n % 32 == 0 { ",\n    " } else { ", " });
        } else {
            out.push_str("\n    ");
        }
        let _ = write!(out, "{b}");
    }
    out.push_str(",\n];\n");
    out.push_str(&g.types);
    out.push_str(&methods);
    out.push_str(&dispatches);

    // The registry: one setter per root interface, in contract order.
    let roots: Vec<(&Interface, &String)> = contract
        .interfaces
        .iter()
        .zip(&traits)
        .filter(|(i, _)| !i.handle)
        .collect();
    out.push_str("\n/// The providers of this module's root host traits.\npub struct Bindings;\n\nimpl Bindings {\n    #[must_use]\n    pub fn builder() -> BindingsBuilder {\n        BindingsBuilder {\n");
    let setters: Vec<String> = roots.iter().map(|(_, t)| ident(&snake(t))).collect();
    for s in &setters {
        let _ = writeln!(out, "            {s}: None,");
    }
    out.push_str("        }\n    }\n}\n\npub struct BindingsBuilder {\n");
    for ((_, t), s) in roots.iter().zip(&setters) {
        let _ = writeln!(out, "    {s}: Option<Box<dyn {t}>>,");
    }
    out.push_str("}\n\nimpl BindingsBuilder {\n");
    for ((_, t), s) in roots.iter().zip(&setters) {
        let _ = write!(
            out,
            "    #[must_use]\n    pub fn {s}(mut self, provider: impl {t} + 'static) -> Self {{\n        self.{s} = Some(Box::new(provider));\n        self\n    }}\n\n"
        );
    }
    let _ = write!(
        out,
        "    /// The registry, or the first root trait left without a provider.\n    pub fn build(self) -> Result<{NATIVE}::Bindings, {NATIVE}::Unregistered> {{\n        let mut bindings = {NATIVE}::Bindings::new(CONTRACT);\n"
    );
    for ((i, t), s) in roots.iter().zip(&setters) {
        let _ = writeln!(
            out,
            "        let Some(provider) = self.{s} else {{\n            return Err({NATIVE}::Unregistered::new({}));\n        }};\n        bindings.register_dispatch({}, {}, Box::new(provider), dispatch_{t});",
            quoted(&i.identity.to_string()),
            quoted(&i.identity.to_string()),
            quoted(&i.module),
        );
    }
    out.push_str("        Ok(bindings)\n    }\n}\n");
    Ok(out)
}

/// A rendered record or enum.
struct Body {
    /// Its structural key, which holds the decoder, so one Rust shape of
    /// two encodings is two types.
    key: String,
    text: String,
    /// Its own `wire_encode` and `wire_decode`.
    codec: String,
    /// Whether it holds no handle.
    plain: bool,
    /// Whether it reaches a recursive reference.
    spine: bool,
    /// Its `wire_dismantle`, when it holds a recursive group's value.
    dismantle: Option<String>,
}

#[derive(Default)]
struct Gen {
    /// Generated struct and enum definitions, in first-use order.
    types: String,
    /// Structural key -> Rust type name: one definition per shape.
    interned: HashMap<String, String>,
    /// Every type and trait name handed out, for collision suffixes.
    used: HashSet<String>,
    /// Base name -> the last numeric suffix tried for it.
    suffix: HashMap<String, usize>,
    /// Import module -> trait name and contract index, for `Value::Handle`.
    handles: HashMap<String, (String, usize)>,
    /// Recursive binder id -> its type name, while its body is rendered.
    recs: HashMap<u32, String>,
    /// Named types holding a handle, which derive nothing.
    opaque: HashSet<String>,
    /// Whether the body being rendered holds no handle so far.
    plain: bool,
    /// Inside a recursive group: each rendered node's type and codec, by
    /// the node's address, for the group's [`Machine`].
    group: HashMap<usize, Ty>,
    /// Whether the body being rendered reaches a recursive reference.
    reached: bool,
}

impl Gen {
    /// `base`, or `base2`, `base3`... when taken. `base` is never empty.
    /// Each base resumes from its last suffix, so `k` uses of one base
    /// probe `O(k)` names in all, not `O(k^2)`.
    fn fresh(&mut self, base: &str) -> String {
        if self.used.insert(base.to_owned()) {
            return base.to_owned();
        }
        let next = self.suffix.entry(base.to_owned()).or_insert(1);
        loop {
            *next += 1;
            let name = format!("{base}{next}");
            if self.used.insert(name.clone()) {
                return name;
            }
        }
    }

    /// The provider trait of `i` into `out`, and its dispatch into
    /// `dispatches`.
    fn interface(&mut self, out: &mut String, dispatches: &mut String, i: &Interface, name: &str) {
        let _ = write!(out, "\n/// `{}`", i.identity);
        if i.handle {
            out.push_str(", a host handle trait");
        }
        let _ = write!(out, ".\npub trait {name} {{\n");
        let mut arms = String::new();
        // Whether any method reads its arguments or encodes a result: the
        // dispatch of a trait whose methods do neither leaves `args` or
        // `handles` unread.
        let mut reads = false;
        let mut encodes = false;
        for (index, m) in i.methods.iter().enumerate() {
            let base = format!("{name}{}", camel(&m.member));
            let params: Vec<Ty> = m
                .params
                .iter()
                .enumerate()
                .map(|(n, p)| self.ty(p, &format!("{base}Arg{n}"), false))
                .collect();
            let result = self.ty(&m.result, &format!("{base}Result"), false);
            let member = ident(&snake(&m.member));
            let _ = write!(out, "    fn {member}(&mut self");
            // A recursive argument is lent: the engine takes it apart
            // after the call, so a depth the guest chose never reaches a
            // provider's drop.
            for (n, p) in params.iter().enumerate() {
                let lent = if p.drop.is_some() { "&" } else { "" };
                let _ = write!(out, ", arg{n}: {lent}{}", p.rust);
            }
            if m.suspends {
                let _ = writeln!(
                    out,
                    ", cx: {NATIVE}::CallContext<'_>) -> {NATIVE}::NativeCall<{}>;",
                    result.rust
                );
            } else {
                let _ = writeln!(out, ") -> {NATIVE}::HostResult<{}>;", result.rust);
            }
            reads |= !params.is_empty();
            encodes |= m.suspends || !matches!(m.result, Value::Void | Value::Never);
            arms.push_str(&Self::arm(index, m, &member, &params, &result));
        }
        out.push_str("}\n");
        let args = if reads { "args" } else { "_args" };
        let handles = if encodes { "handles" } else { "_handles" };
        let whose = if i.handle {
            "a returned"
        } else {
            "the registered"
        };
        // A trait with no reachable method still checks its trait object,
        // and calls nothing on it.
        let bind = if i.methods.is_empty() {
            ""
        } else {
            "let provider = "
        };
        let _ = write!(
            dispatches,
            "\n/// Calls the method at index `method` of {whose} `{name}` with its\n/// encoded arguments and returns its encoded result, or a suspending\n/// method's pending call.\n#[allow(non_snake_case)]\npub fn dispatch_{name}(provider: &mut dyn std::any::Any, method: usize, {args}: &[u8], {handles}: &mut {NATIVE}::Handles) -> {NATIVE}::Dispatched {{\n    {bind}{WIRE}::provider::<Box<dyn {name}>>(provider)?;\n    match method {{\n{arms}        _ => Err({WIRE}::unknown_method(method).into()),\n    }}\n}}\n"
        );
    }

    /// The dispatch arm of method `index`: its arguments decoded in order
    /// (one `string` argument is its bare UTF-8 bytes, as the JS host
    /// passes it), the call, and its result encoded.
    fn arm(
        index: usize,
        m: &crate::schema::Method,
        member: &str,
        params: &[Ty],
        result: &Ty,
    ) -> String {
        let mut arm = format!("        {index} => {{\n");
        if matches!(m.params[..], [Value::String]) {
            let _ = writeln!(arm, "            let arg0 = {WIRE}::text(args);");
        } else if !params.is_empty() {
            let _ = writeln!(arm, "            let d = &mut {WIRE}::Decoder::new(args);");
            for (n, p) in params.iter().enumerate() {
                if p.drop.is_some() {
                    // Taken apart when the arm ends, on every path.
                    let _ = writeln!(
                        arm,
                        "            let arg{n} = {WIRE}::Lent::new({}, |x: {}| {{ {} }});",
                        p.dec,
                        p.rust,
                        p.drop("x")
                    );
                } else {
                    let _ = writeln!(arm, "            let arg{n} = {};", p.dec);
                }
            }
            arm.push_str("            d.finish()?;\n");
        }
        let args: Vec<String> = params
            .iter()
            .enumerate()
            .map(|(n, p)| {
                if p.drop.is_some() {
                    format!("&arg{n}")
                } else {
                    format!("arg{n}")
                }
            })
            .collect();
        let mut call = format!("provider.{member}({}", args.join(", "));
        if m.suspends {
            // The value's codec, which the `.finish` of a pending call runs.
            if !params.is_empty() {
                call.push_str(", ");
            }
            let _ = write!(call, "{NATIVE}::CallContext::new())");
            let encode = match m.result {
                Value::Void => format!("|(): (), _: &mut {NATIVE}::Handles| Ok(Vec::new())"),
                Value::Never => format!(
                    "|value: std::convert::Infallible, _: &mut {NATIVE}::Handles| match value {{}}"
                ),
                _ => format!(
                    "|value: {}, handles: &mut {NATIVE}::Handles| {{\n                let mut enc = {WIRE}::Encoder::new(handles);\n                let e = &mut enc;\n                {}\n                Ok(enc.into_bytes())\n            }}",
                    result.rust,
                    result.enc("value")
                ),
            };
            let _ = writeln!(arm, "            {WIRE}::answer({call}, {encode}, handles)");
            arm.push_str("        }\n");
            return arm;
        }
        // The provider's own error is its throw; every other `?` here is
        // the codec's or the host's failure.
        let _ = write!(call, ").map_err({NATIVE}::Failure::Threw)");
        match m.result {
            Value::Void => {
                let _ = writeln!(
                    arm,
                    "            let () = {call}?;\n            Ok({NATIVE}::Answer::Ready(Vec::new()))"
                );
            }
            Value::Never => {
                let _ = writeln!(
                    arm,
                    "            let value = {call}?;\n            match value {{}}"
                );
            }
            _ => {
                let _ = writeln!(
                    arm,
                    "            let value = {call}?;\n            let mut enc = {WIRE}::Encoder::new(handles);\n            let e = &mut enc;\n            {}\n            Ok({NATIVE}::Answer::Ready(enc.into_bytes()))",
                    result.enc("value")
                );
            }
        }
        arm.push_str("        }\n");
        arm
    }

    /// The Rust type and codec of `v`; `hint` names a record or enum
    /// defined here. A recursive reference is boxed unless a list already
    /// holds it (`in_list`). Inside a recursive group each node's is kept
    /// for the group's codec.
    fn ty(&mut self, v: &Value, hint: &str, in_list: bool) -> Ty {
        let t = self.ty_of(v, hint, in_list);
        if !self.recs.is_empty() {
            self.group.insert(addr(v), t.clone());
        }
        t
    }

    fn ty_of(&mut self, v: &Value, hint: &str, in_list: bool) -> Ty {
        let int = |rust: String, signed: bool| {
            let (enc, dec) = if signed {
                (
                    format!("e.signed(i64::from({V}));"),
                    format!("d.signed::<{rust}>()?"),
                )
            } else {
                (
                    format!("e.unsigned(u64::from({V}));"),
                    format!("d.unsigned::<{rust}>()?"),
                )
            };
            Ty::new(rust, enc, dec)
        };
        match v {
            Value::Void => Ty::new("()", format!("let _ = {V};"), "()"),
            Value::Bool => Ty::new("bool", format!("e.bool({V});"), "d.bool()?"),
            Value::Char => Ty::new("char", format!("e.char({V});"), "d.char()?"),
            Value::Int { bits, signed } => int(
                format!("{}{bits}", if *signed { 'i' } else { 'u' }),
                *signed,
            ),
            Value::F32 => Ty::new("f32", format!("e.f32({V});"), "d.f32()?"),
            Value::F64 => Ty::new("f64", format!("e.f64({V});"), "d.f64()?"),
            Value::String => Ty::new("String", format!("e.string(&{V});"), "d.string()?"),
            Value::Bytes => Ty::new("Vec<u8>", format!("e.bytes(&{V});"), "d.bytes()?"),
            Value::Json => Ty::new(
                format!("{NATIVE}::Serde"),
                format!("e.serde(&{V})?;"),
                "d.serde()?",
            ),
            // ruled out: `Contract::validate` keeps `never` to a method's
            // whole result, which `arm` writes without a codec.
            Value::Never => Ty::new(
                "std::convert::Infallible",
                format!("match {V} {{}}"),
                "d.absent()?",
            ),
            Value::Handle(module) => {
                self.plain = false;
                let (name, index) = &self.handles[module.as_str()];
                Ty::new(
                    format!("Box<dyn {name}>"),
                    format!("e.handle(Box::new({V}), {index}, dispatch_{name})?;"),
                    "d.handle()?",
                )
            }
            Value::List(t) => {
                let t = self.ty(t, &format!("{hint}Item"), true);
                Ty::new(
                    format!("Vec<{}>", t.rust),
                    format!(
                        "{{ e.count({V}.len()); for x in {V} {{ {} }} }}",
                        t.enc("x")
                    ),
                    format!(
                        "{{ let n = d.list_len()?; let mut x = Vec::with_capacity(d.capacity(n)); for _ in 0..n {{ x.push({}); }} x }}",
                        t.dec
                    ),
                )
                .dropping(
                    t.drop
                        .is_some()
                        .then(|| format!("for x in {V} {{ {} }}", t.drop("x"))),
                )
            }
            Value::Option(t) => {
                let t = self.ty(t, hint, false);
                Ty::new(
                    format!("Option<{}>", t.rust),
                    format!(
                        "match {V} {{ None => e.tag(0), Some(x) => {{ e.tag(1); {} }} }}",
                        t.enc("x")
                    ),
                    format!("if d.tag(2)? == 0 {{ None }} else {{ Some({}) }}", t.dec),
                )
                .dropping(
                    t.drop
                        .is_some()
                        .then(|| format!("if let Some(x) = {V} {{ {} }}", t.drop("x"))),
                )
            }
            Value::Result(a, b) => {
                let a = self.ty(a, &format!("{hint}Ok"), false);
                let b = self.ty(b, &format!("{hint}Err"), false);
                Ty::new(
                    format!("Result<{}, {}>", a.rust, b.rust),
                    format!(
                        "match {V} {{ Ok(x) => {{ e.tag(0); {} }} Err(x) => {{ e.tag(1); {} }} }}",
                        a.enc("x"),
                        b.enc("x")
                    ),
                    format!(
                        "if d.tag(2)? == 0 {{ Ok({}) }} else {{ Err({}) }}",
                        a.dec, b.dec
                    ),
                )
                .dropping((a.drop.is_some() || b.drop.is_some()).then(|| {
                    // A side holding no group value drops in place.
                    let side = |t: &Ty| {
                        if t.drop.is_some() {
                            ("x", t.drop("x"))
                        } else {
                            ("_", String::new())
                        }
                    };
                    let ((ok, a), (err, b)) = (side(&a), side(&b));
                    format!("match {V} {{ Ok({ok}) => {{ {a} }} Err({err}) => {{ {b} }} }}")
                }))
            }
            // `Map` has no Native Value Table row yet: its entries in map
            // order, as the JS host's `Map` iterates them.
            Value::Map(k, v) => {
                let k = self.ty(k, &format!("{hint}Key"), false);
                let v = self.ty(v, &format!("{hint}Value"), false);
                Ty::new(
                    format!("Vec<({}, {})>", k.rust, v.rust),
                    format!(
                        "{{ e.count({V}.len()); for (k, x) in {V} {{ {} {} }} }}",
                        k.enc("k"),
                        v.enc("x")
                    ),
                    format!(
                        "{{ let n = d.map_len()?; let mut x = Vec::with_capacity(d.capacity(n)); for _ in 0..n {{ let k = {}; x.push((k, {})); }} x }}",
                        k.dec, v.dec
                    ),
                )
                .dropping((k.drop.is_some() || v.drop.is_some()).then(|| {
                    let key = if k.drop.is_some() { "k" } else { "_" };
                    let value = if v.drop.is_some() { "x" } else { "_" };
                    format!(
                        "for ({key}, {value}) in {V} {{ {} {} }}",
                        k.drop("k"),
                        v.drop("x")
                    )
                }))
            }
            Value::Tuple(ts) => {
                let parts: Vec<Ty> = ts
                    .iter()
                    .enumerate()
                    .map(|(n, t)| self.ty(t, &format!("{hint}{n}"), false))
                    .collect();
                let names: Vec<String> = (0..parts.len()).map(|n| format!("t{n}")).collect();
                let rust: Vec<&str> = parts.iter().map(|p| p.rust.as_str()).collect();
                let decs: Vec<&str> = parts.iter().map(|p| p.dec.as_str()).collect();
                let encs: Vec<String> = parts.iter().zip(&names).map(|(p, n)| p.enc(n)).collect();
                let comma = if parts.len() == 1 { "," } else { "" };
                let drops: String = parts.iter().zip(&names).map(|(p, n)| p.drop(n)).collect();
                // A part holding no group value drops in place.
                let held: Vec<&str> = parts
                    .iter()
                    .zip(&names)
                    .map(|(p, n)| if p.drop.is_some() { n.as_str() } else { "_" })
                    .collect();
                Ty::new(
                    format!("({}{comma})", rust.join(", ")),
                    format!(
                        "{{ let ({}{comma}) = {V}; {} }}",
                        names.join(", "),
                        encs.join(" ")
                    ),
                    format!("({}{comma})", decs.join(", ")),
                )
                .dropping(
                    parts
                        .iter()
                        .any(|p| p.drop.is_some())
                        .then(|| format!("{{ let ({}{comma}) = {V}; {drops} }}", held.join(", "))),
                )
            }
            Value::Ref(id) => {
                self.reached = true;
                let name = self.recs[id].clone();
                if in_list {
                    let enc = format!("{name}::wire_encode({V}, e)?;");
                    let dec = format!("{name}::wire_decode(d)?");
                    Ty::new(name, enc, dec)
                } else {
                    let enc = format!("{name}::wire_encode(*{V}, e)?;");
                    let dec = format!("Box::new({name}::wire_decode(d)?)");
                    Ty::new(format!("Box<{name}>"), enc, dec)
                }
            }
            Value::Rec(id, body) => {
                // The name is fixed before the body so its references can
                // use it; a recursive type is never shared structurally.
                let name = self.fresh(hint);
                let outermost = self.recs.is_empty();
                let reached = self.reached;
                self.recs.insert(*id, name.clone());
                self.group.insert(addr(body), Ty::new(name.clone(), "", ""));
                let b = self.body(body, &name);
                // Every type of the group is read and written by the
                // outermost type's codec, which alone has codec methods,
                // and is cloned and compared by its loops.
                self.emit(
                    &name,
                    &b.text,
                    None,
                    matches!(**body, Value::Enum(_)),
                    b.plain,
                    "",
                );
                self.recs.remove(id);
                if !outermost {
                    return Self::named(name);
                }
                {
                    let wire = self.fresh(&format!("{name}Wire"));
                    let op = self.fresh(&format!("{name}WireOp"));
                    let mut spine = HashSet::default();
                    mark(v, &mut spine);
                    let pair = self.fresh(&format!("{name}WirePair"));
                    let show = self.fresh(&format!("{name}WireShow"));
                    let mut machine = Machine {
                        group: &self.group,
                        spine,
                        roots: HashMap::default(),
                        count: 0,
                        plain: b.plain,
                        wire,
                        op,
                        pair,
                        vals: String::new(),
                        ops: String::new(),
                        dec: String::new(),
                        enc: String::new(),
                        drop: String::new(),
                        drop_ops: String::new(),
                        pairs: String::new(),
                        eq: String::new(),
                        show,
                        shows: String::new(),
                        show_arms: String::new(),
                        named: Vec::new(),
                        seen: HashSet::default(),
                    };
                    machine.walk(v, false);
                    let codec = machine.finish(&name);
                    self.types.push_str(&codec);
                    self.group.clear();
                    self.reached = reached;
                }
                // The group's value outside it: taken apart by its loop.
                let drop = format!("{name}::dismantle({V});");
                Self::named(name).dropping(Some(drop))
            }
            Value::Record(_) | Value::Enum(_) => {
                let b = self.body(v, hint);
                let held = b.dismantle.is_some();
                let name = if let Some(name) = self.interned.get(&b.key) {
                    name.clone()
                } else {
                    let name = self.fresh(hint);
                    self.interned.insert(b.key, name.clone());
                    // A type reaching a reference is read and written by
                    // its group's codec, and cloned, compared and printed
                    // by its loops.
                    let (codec, derives) = if b.spine {
                        (None, "")
                    } else {
                        let codec = format!("{}{}", b.codec, b.dismantle.unwrap_or_default());
                        (Some(codec), "Clone, Debug, PartialEq")
                    };
                    self.emit(
                        &name,
                        &b.text,
                        codec.as_deref(),
                        matches!(v, Value::Enum(_)),
                        b.plain,
                        derives,
                    );
                    name
                };
                if self.opaque.contains(&name) {
                    self.plain = false;
                }
                let drop = held.then(|| format!("{name}::wire_dismantle({V});"));
                Self::named(name).dropping(drop)
            }
        }
    }

    /// A record or an enum defined here: its codec is its own two methods.
    fn named(name: String) -> Ty {
        let enc = format!("{name}::wire_encode({V}, e)?;");
        let dec = format!("{name}::wire_decode(d)?");
        Ty::new(name, enc, dec)
    }

    /// A record or an enum, rendered.
    fn body(&mut self, v: &Value, hint: &str) -> Body {
        let outer = std::mem::replace(&mut self.plain, true);
        let outer_reached = std::mem::replace(&mut self.reached, false);
        // The fields holding a group's value outside it, taken apart in
        // the type's own `wire_dismantle`, and how many variants hold one.
        let mut drops = String::new();
        let mut held = 0;
        let (key, text, enc, dec) = match v {
            Value::Record(fs) => {
                let fields = self.fields(fs, hint);
                if fields.iter().any(|(_, t)| t.drop.is_some()) {
                    let (pattern, held) = Self::held(&fields);
                    let _ = write!(drops, "        let Self {{ {pattern} }} = self;\n{held}");
                }
                let mut lines = String::new();
                for (name, t) in &fields {
                    let _ = writeln!(lines, "    pub {name}: {},", t.rust);
                }
                let (pattern, encs, decs) = Self::codec_fields(&fields);
                // A record without fields reads and writes no bytes.
                let (enc, dec) = if fields.is_empty() {
                    (
                        "        let _ = e;\n        Ok(())\n".to_owned(),
                        "        let _ = d;\n        Ok(Self {})\n".to_owned(),
                    )
                } else {
                    (
                        format!("        let Self {{ {pattern} }} = self;\n{encs}        Ok(())\n"),
                        format!("        Ok(Self {{ {decs} }})\n"),
                    )
                };
                (
                    format!("struct{{{lines}}}"),
                    format!(" {{\n{lines}}}\n"),
                    enc,
                    dec,
                )
            }
            Value::Enum(vs) => {
                let mut body = String::new();
                let mut encs = String::new();
                // Each variant's value, decoded after its tag.
                let mut ctors = Vec::with_capacity(vs.len());
                for (n, var) in vs.iter().enumerate() {
                    let name = ident(&var.name);
                    if var.fields.is_empty() {
                        let _ = writeln!(body, "    {name},");
                        let _ = writeln!(encs, "            Self::{name} => e.tag({n}),");
                        ctors.push(format!("Self::{name}"));
                    } else {
                        let fields =
                            self.fields(&var.fields, &format!("{hint}{}", camel(&var.name)));
                        if fields.iter().any(|(_, t)| t.drop.is_some()) {
                            held += 1;
                            let (pattern, lines) = Self::held(&fields);
                            let _ = write!(
                                drops,
                                "            Self::{name} {{ {pattern} }} => {{\n{}            }}\n",
                                lines.replace("        ", "                ")
                            );
                        }
                        let _ = writeln!(body, "    {name} {{");
                        for (field, t) in &fields {
                            let _ = writeln!(body, "        {field}: {},", t.rust);
                        }
                        body.push_str("    },\n");
                        let (pattern, fenc, fdec) = Self::codec_fields(&fields);
                        let _ = write!(
                            encs,
                            "            Self::{name} {{ {pattern} }} => {{\n                e.tag({n});\n{}            }}\n",
                            fenc.replace("        ", "                ")
                        );
                        ctors.push(format!("Self::{name} {{ {fdec} }}"));
                    }
                }
                let enc = if vs.is_empty() {
                    "        let _ = e;\n        match self {}\n".to_owned()
                } else {
                    format!("        match self {{\n{encs}        }}\n        Ok(())\n")
                };
                // The last variant takes every other tag, which `tag` has
                // already refused.
                let dec = match &ctors[..] {
                    [] => "        d.absent()\n".to_owned(),
                    [one] => format!("        d.tag(1)?;\n        Ok({one})\n"),
                    [init @ .., last] => {
                        let mut arms = String::new();
                        for (n, ctor) in init.iter().enumerate() {
                            let _ = writeln!(arms, "            {n} => {ctor},");
                        }
                        format!(
                            "        Ok(match d.tag({})? {{\n{arms}            _ => {last},\n        }})\n",
                            ctors.len()
                        )
                    }
                };
                (
                    format!("enum{{{body}}}"),
                    format!(" {{\n{body}}}\n"),
                    enc,
                    dec,
                )
            }
            // ruled out: `Contract::validate` refuses a recursive body other
            // than a record or an enum, and only those reach here.
            _ => unreachable!("only records and enums are named types"),
        };
        let plain = self.plain;
        self.plain = outer && plain;
        let spine = self.reached;
        self.reached = outer_reached || spine;
        let codec = format!(
            "    fn wire_encode(self, e: &mut {WIRE}::Encoder<'_>) -> {NATIVE}::HostResult<()> {{\n{enc}    }}\n\n    fn wire_decode(d: &mut {WIRE}::Decoder<'_>) -> {NATIVE}::HostResult<Self> {{\n{dec}    }}\n"
        );
        let dismantle = (!drops.is_empty()).then(|| {
            let body = match v {
                // The variants holding no group value drop in place.
                Value::Enum(vs) if held < vs.len() => {
                    format!("        match self {{\n{drops}            _ => {{}}\n        }}\n")
                }
                Value::Enum(_) => format!("        match self {{\n{drops}        }}\n"),
                _ => drops,
            };
            format!("\n    fn wire_dismantle(self) {{\n{body}    }}\n")
        });
        Body {
            key: format!("{key}{dec}"),
            text,
            codec,
            plain,
            spine,
            dismantle,
        }
    }

    /// A pattern binding each field holding a group's value to `f0`,
    /// `f1`..., and the statements dismantling them.
    fn held(fields: &[(String, Ty)]) -> (String, String) {
        let mut pattern = Vec::with_capacity(fields.len() + 1);
        let mut held = String::new();
        for (n, (name, t)) in fields.iter().enumerate() {
            if t.drop.is_some() {
                pattern.push(format!("{name}: f{n}"));
                let _ = writeln!(held, "        {}", t.drop(&format!("f{n}")));
            }
        }
        pattern.push("..".to_owned());
        (pattern.join(", "), held)
    }

    /// Each field's Rust name and type, in contract order.
    fn fields(&mut self, fs: &[Field], hint: &str) -> Vec<(String, Ty)> {
        fs.iter()
            .map(|f| {
                let t = self.ty(&f.value, &format!("{hint}{}", camel(&f.name)), false);
                (ident(&snake(&f.name)), t)
            })
            .collect()
    }

    /// A pattern binding each field to `f0`, `f1`..., the statements
    /// encoding them, and the field initializers decoding them in order.
    fn codec_fields(fields: &[(String, Ty)]) -> (String, String, String) {
        let mut pattern = Vec::with_capacity(fields.len());
        let mut encs = String::new();
        let mut decs = Vec::with_capacity(fields.len());
        for (n, (name, t)) in fields.iter().enumerate() {
            pattern.push(format!("{name}: f{n}"));
            let _ = writeln!(encs, "        {}", t.enc(&format!("f{n}")));
            decs.push(format!("{name}: {}", t.dec));
        }
        (pattern.join(", "), encs, decs.join(", "))
    }

    /// `derives` is empty for a type of a recursive group, whose loops
    /// clone and compare it.
    fn emit(
        &mut self,
        name: &str,
        body: &str,
        codec: Option<&str>,
        is_enum: bool,
        plain: bool,
        derives: &str,
    ) {
        // A handle is a trait object: no derives cross it.
        if plain {
            if !derives.is_empty() {
                let _ = write!(self.types, "\n#[derive({derives})]");
            }
        } else {
            self.opaque.insert(name.to_owned());
        }
        let _ = write!(
            self.types,
            "\npub {} {name}{body}",
            if is_enum { "enum" } else { "struct" }
        );
        // A type only results cross is never decoded, and one only
        // arguments cross is never encoded.
        if let Some(codec) = codec {
            let _ = write!(
                self.types,
                "\n#[allow(dead_code)]\nimpl {name} {{\n{codec}}}\n"
            );
        }
    }
}

/// The node at `v`'s address, the key of [`Gen::group`] and
/// [`Machine::spine`].
fn addr(v: &Value) -> usize {
    std::ptr::from_ref(v).addr()
}

/// Marks each node of a recursive group that reaches a reference of the
/// group, bottom-up in one pass; returns whether `v` does.
fn mark(v: &Value, spine: &mut HashSet<usize>) -> bool {
    let reaches = match v {
        Value::Ref(_) => true,
        Value::Rec(_, body) => mark(body, spine),
        Value::List(t) | Value::Option(t) => mark(t, spine),
        Value::Map(a, b) | Value::Result(a, b) => mark(a, spine) | mark(b, spine),
        Value::Tuple(ts) => ts.iter().fold(false, |r, t| mark(t, spine) | r),
        Value::Record(fs) => fs.iter().fold(false, |r, f| mark(&f.value, spine) | r),
        Value::Enum(vs) => vs
            .iter()
            .flat_map(|var| &var.fields)
            .fold(false, |r, f| mark(&f.value, spine) | r),
        _ => false,
    };
    if reaches {
        spine.insert(addr(v));
    }
    reaches
}

/// The explicit-stack codec of one recursive group (`Value::Rec` and every
/// type its body reaches recursively): a codec that recursed once per
/// nesting level would overflow the host's stack on a value as deep as the
/// guest or the provider makes it (AGENTS.md "Performant").
///
/// Each node of the group that reaches a reference, and each child of
/// one, is a position `k`: a variant `Pk` of the value enum, holding a
/// value of the node's Rust type, and the decode op `Dk`. Decoding runs a
/// stack of ops over a stack of finished values, the way the private-value
/// token reader in `wire.rs` keeps its open containers: `Dk` reads a
/// position's tag or count and pushes the ops of its children and the op
/// that builds it from their values; a list keeps its items so far in its
/// op. Encoding takes the value and pops it from a stack of values: a
/// position writes its own tag or count and pushes its children, last
/// first. A failure part way dismantles both stacks through the same
/// stack, so nothing drops recursively either. A child reaching no
/// reference is a leaf, read and written by its inline codec, whose depth
/// the contract's boundary depth limit bounds.
///
/// In a group without handles, the same positions clone, compare and print
/// its values. A clone reads a borrowed position (`Ck`) and pushes its
/// children's clone ops and the decoder's own build op, so it builds
/// bottom-up exactly as decoding does; a list's or map's clone op holds its
/// items so far and the iterator over the rest (`Kk`, `Jk`). A compare pops
/// a pair of positions, checks their own tag or length, and pushes their
/// children's pairs; a `Debug` pops a position and writes its text around
/// its children through `wire::Show`, as a derived `Debug` would.
///
/// Cost: generation is linear in the group's nodes. A generated codec,
/// clone, compare or `Debug` does O(1) work per position of the value plus
/// its leaves' own, and its stacks hold the open positions and their
/// pending siblings.
struct Machine<'a> {
    group: &'a HashMap<usize, Ty>,
    spine: HashSet<usize>,
    /// Binder id -> the position of its body.
    roots: HashMap<u32, usize>,
    count: usize,
    /// Whether the group holds no handle, so it is cloned and compared.
    plain: bool,
    wire: String,
    op: String,
    /// The enum of a compare's pairs of positions.
    pair: String,
    /// Value enum variants.
    vals: String,
    /// Op enum variants.
    ops: String,
    /// Decode arms, encode arms and dismantle arms.
    dec: String,
    enc: String,
    drop: String,
    /// Dismantle arms of ops holding a partial list or map.
    drop_ops: String,
    /// Pair enum variants and compare arms.
    pairs: String,
    eq: String,
    /// The enum of a `Debug`'s positions, its variants and its arms.
    show: String,
    shows: String,
    show_arms: String,
    /// Each named type of the group and its first position, which clone
    /// and compare start from.
    named: Vec<(String, usize)>,
    seen: HashSet<String>,
}

impl Machine<'_> {
    fn rust(&self, v: &Value) -> &str {
        &self.group[&addr(v)].rust
    }

    /// A clone arm (`Ck` reads a borrowed position and pushes its children,
    /// `Kk`/`Jk` hold a list's or map's items so far and the rest to
    /// read) and the ops it adds, in a group that is cloned.
    fn clone_arm(&mut self, ops: &str, arm: &str) {
        if self.plain {
            self.ops.push_str(ops);
            let _ = writeln!(self.dec, "                {arm}");
        }
    }

    /// A compare arm over a pair of positions, in a group that is compared.
    fn eq_arm(&mut self, arm: &str) {
        if self.plain {
            let _ = writeln!(self.eq, "                {arm}");
        }
    }

    /// A `Debug` arm printing one position at `depth`, in a group that is
    /// printed.
    fn show_arm(&mut self, arm: &str) {
        if self.plain {
            let _ = writeln!(self.show_arms, "            {arm}");
        }
    }

    /// `Show::open` of a composite at `depth` with `children`.
    fn open(open: &str, close: &str, children: &str) -> String {
        format!(
            "{WIRE}::Show::open(stack, f, depth, {}, {}, {children}),",
            quoted(open),
            quoted(close)
        )
    }

    /// `Some(Self::Pc(x)) = vals.pop()` as a `let ... else`.
    fn pop(c: usize, x: &str) -> String {
        format!(
            "let Some(Self::P{c}({x})) = vals.pop() else {{ unreachable!(\"ruled out: each op pops what its own children pushed\") }};"
        )
    }

    /// Children positions of fields, and the decode ops pushing them last
    /// first.
    fn children<'v>(&mut self, vs: impl Iterator<Item = &'v Value>) -> Vec<usize> {
        vs.map(|v| self.walk(v, false)).collect()
    }

    fn push_decs(cs: &[usize]) -> String {
        cs.iter().rev().fold(String::new(), |mut out, c| {
            let _ = write!(out, " ops.push({{OP}}::D{c});");
            out
        })
    }

    /// The position of `v` and its arms. A reference in a list is its
    /// binder's position itself: the list holds the type unboxed.
    fn walk(&mut self, v: &Value, in_list: bool) -> usize {
        match (v, in_list) {
            (Value::Ref(id), true) => return self.roots[id],
            // A binder's body takes the binder's position.
            (Value::Rec(id, body), _) => {
                self.roots.insert(*id, self.count);
                return self.walk(body, false);
            }
            _ => {}
        }
        let k = self.count;
        self.count += 1;
        let rust = self.rust(v).to_owned();
        let _ = writeln!(self.vals, "    P{k}({rust}),");
        let _ = writeln!(self.ops, "    D{k},");
        if self.plain {
            let _ = writeln!(self.ops, "    C{k}(&'a {rust}),");
            let _ = writeln!(self.pairs, "    P{k}(&'a {rust}, &'a {rust}),");
            let _ = writeln!(self.shows, "    P{k}(&'a {rust}),");
        }
        if !self.spine.contains(&addr(v)) {
            self.clone_arm(
                "",
                &format!("{{OP}}::C{k}(x) => vals.push(Self::P{k}(x.clone())),"),
            );
            self.eq_arm(&format!(
                "Self::P{k}(a, b) => if a != b {{ return false; }},"
            ));
            self.show_arm(&format!("Self::P{k}(x) => {WIRE}::show_leaf(f, x, depth),"));
            let t = &self.group[&addr(v)];
            let _ = writeln!(
                self.dec,
                "                {{OP}}::D{k} => vals.push(Self::P{k}({})),",
                t.dec
            );
            let _ = writeln!(
                self.enc,
                "                Self::P{k}(x) => {{ {} }}",
                t.enc("x")
            );
            return k;
        }
        match v {
            Value::Ref(id) => {
                let t = self.roots[id];
                let _ = writeln!(self.ops, "    W{k},");
                let _ = writeln!(
                    self.dec,
                    "                {{OP}}::D{k} => {{ ops.push({{OP}}::W{k}); ops.push({{OP}}::D{t}); }}"
                );
                let _ = writeln!(
                    self.dec,
                    "                {{OP}}::W{k} => {{ {} vals.push(Self::P{k}(Box::new(x))); }}",
                    Self::pop(t, "x")
                );
                let _ = writeln!(
                    self.enc,
                    "                Self::P{k}(x) => stack.push(Self::P{t}(*x)),"
                );
                let _ = writeln!(
                    self.drop,
                    "                Self::P{k}(x) => items.push(Self::P{t}(*x)),"
                );
                self.clone_arm(
                    "",
                    &format!("{{OP}}::C{k}(x) => {{ ops.push({{OP}}::W{k}); ops.push({{OP}}::C{t}(&**x)); }}"),
                );
                self.eq_arm(&format!(
                    "Self::P{k}(a, b) => stack.push(Self::P{t}(&**a, &**b)),"
                ));
                self.show_arm(&format!("Self::P{k}(x) => {{ stack.push({WIRE}::Show::At(Self::P{t}(&**x), depth)); Ok(()) }}"));
                k
            }
            Value::Record(fs) => {
                self.name(&rust, k);
                let cs = self.children(fs.iter().map(|f| &f.value));
                let names: Vec<String> = fs.iter().map(|f| ident(&snake(&f.name))).collect();
                let values: Vec<&Value> = fs.iter().map(|f| &f.value).collect();
                self.build(k, &format!("{rust} {{"), "}", &names, &cs, &values, None);
                k
            }
            Value::Tuple(ts) => {
                let cs = self.children(ts.iter());
                let comma = if ts.len() == 1 { "," } else { "" };
                let values: Vec<&Value> = ts.iter().collect();
                self.build(k, "(", &format!("{comma})"), &[], &cs, &values, None);
                k
            }
            Value::Enum(vs) => {
                self.name(&rust, k);
                let mut arms = Vec::with_capacity(vs.len());
                for (n, var) in vs.iter().enumerate() {
                    let name = ident(&var.name);
                    if var.fields.is_empty() {
                        let _ = writeln!(
                            self.enc,
                            "                Self::P{k}({rust}::{name}) => e.tag({n}),"
                        );
                        self.clone_arm(
                            "",
                            &format!("{{OP}}::C{k}({rust}::{name}) => vals.push(Self::P{k}({rust}::{name})),"),
                        );
                        self.eq_arm(&format!(
                            "Self::P{k}({rust}::{name}, {rust}::{name}) => {{}}"
                        ));
                        self.show_arm(&format!(
                            "Self::P{k}({rust}::{name}) => f.write_str({}),",
                            quoted(&var.name)
                        ));
                        arms.push(format!("vals.push(Self::P{k}({rust}::{name}));"));
                        continue;
                    }
                    let cs = self.children(var.fields.iter().map(|f| &f.value));
                    let names: Vec<String> =
                        var.fields.iter().map(|f| ident(&snake(&f.name))).collect();
                    let values: Vec<&Value> = var.fields.iter().map(|f| &f.value).collect();
                    self.build(
                        k,
                        &format!("{rust}::{name} {{"),
                        "}",
                        &names,
                        &cs,
                        &values,
                        Some(n),
                    );
                    arms.push(format!(
                        "ops.push({{OP}}::B{k}v{n});{}",
                        Self::push_decs(&cs)
                    ));
                }
                let read = match &arms[..] {
                    [one] => format!("{{ d.tag(1)?; {one} }}"),
                    [init @ .., last] => {
                        let mut m = format!("match d.tag({})? {{ ", arms.len());
                        for (n, arm) in init.iter().enumerate() {
                            let _ = write!(m, "{n} => {{ {arm} }} ");
                        }
                        let _ = write!(m, "_ => {{ {last} }} }}");
                        m
                    }
                    // ruled out: an enum without variants reaches no
                    // reference, so it is a leaf.
                    [] => unreachable!("a spine enum has a variant"),
                };
                let _ = writeln!(self.dec, "                {{OP}}::D{k} => {read},");
                // Two different variants.
                self.eq_arm(&format!("Self::P{k}(..) => return false,"));
                k
            }
            Value::List(t) => {
                let c = self.walk(t, true);
                let item = self.rust(t).to_owned();
                let _ = writeln!(self.ops, "    L{k}(Vec<{item}>, usize),");
                let _ = writeln!(
                    self.dec,
                    "                {{OP}}::D{k} => {{ let n = d.list_len()?; let x = Vec::with_capacity(d.capacity(n)); if n == 0 {{ vals.push(Self::P{k}(x)); }} else {{ ops.push({{OP}}::L{k}(x, n)); ops.push({{OP}}::D{c}); }} }}"
                );
                let _ = writeln!(
                    self.dec,
                    "                {{OP}}::L{k}(mut x, n) => {{ {} x.push(y); if n == 1 {{ vals.push(Self::P{k}(x)); }} else {{ ops.push({{OP}}::L{k}(x, n - 1)); ops.push({{OP}}::D{c}); }} }}",
                    Self::pop(c, "y")
                );
                let _ = writeln!(
                    self.enc,
                    "                Self::P{k}(x) => {{ e.count(x.len()); stack.extend(x.into_iter().rev().map(Self::P{c})); }}"
                );
                let _ = writeln!(
                    self.drop,
                    "                Self::P{k}(x) => items.extend(x.into_iter().map(Self::P{c})),"
                );
                let _ = writeln!(
                    self.drop_ops,
                    "                {{OP}}::L{k}(x, _) => items.push(Self::P{k}(x)),"
                );
                let next = format!(
                    "if let Some(y) = it.next() {{ ops.push({{OP}}::K{k}(x, it)); ops.push({{OP}}::C{c}(y)); }} else {{ vals.push(Self::P{k}(x)); }}"
                );
                self.clone_arm(
                    &format!("    K{k}(Vec<{item}>, std::slice::Iter<'a, {item}>),\n"),
                    &format!("{{OP}}::C{k}(x) => {{ let mut it = x.iter(); let x = Vec::with_capacity(x.len()); {next} }}"),
                );
                self.clone_arm(
                    "",
                    &format!(
                        "{{OP}}::K{k}(mut x, mut it) => {{ {} x.push(y); {next} }}",
                        Self::pop(c, "y")
                    ),
                );
                self.eq_arm(&format!("Self::P{k}(a, b) => {{ if a.len() != b.len() {{ return false; }} stack.extend(a.iter().zip(b).rev().map(|(a, b)| Self::P{c}(a, b))); }}"));
                self.show_arm(&format!(
                    "Self::P{k}(x) => {}",
                    Self::open(
                        "[",
                        "]",
                        &format!("x.iter().map(|y| (\"\", Self::P{c}(y)))")
                    )
                ));
                k
            }
            Value::Map(a, b) => {
                let (ca, cb) = (self.walk(a, false), self.walk(b, false));
                let pair = format!("({}, {})", self.rust(a), self.rust(b));
                let _ = writeln!(self.ops, "    M{k}(Vec<{pair}>, usize),");
                let next = format!("ops.push({{OP}}::D{cb}); ops.push({{OP}}::D{ca});");
                let _ = writeln!(
                    self.dec,
                    "                {{OP}}::D{k} => {{ let n = d.map_len()?; let x = Vec::with_capacity(d.capacity(n)); if n == 0 {{ vals.push(Self::P{k}(x)); }} else {{ ops.push({{OP}}::M{k}(x, n)); {next} }} }}"
                );
                let _ = writeln!(
                    self.dec,
                    "                {{OP}}::M{k}(mut x, n) => {{ {} {} x.push((a, b)); if n == 1 {{ vals.push(Self::P{k}(x)); }} else {{ ops.push({{OP}}::M{k}(x, n - 1)); {next} }} }}",
                    Self::pop(cb, "b"),
                    Self::pop(ca, "a")
                );
                let _ = writeln!(
                    self.enc,
                    "                Self::P{k}(x) => {{ e.count(x.len()); for (a, b) in x.into_iter().rev() {{ stack.push(Self::P{cb}(b)); stack.push(Self::P{ca}(a)); }} }}"
                );
                let _ = writeln!(
                    self.drop,
                    "                Self::P{k}(x) => for (a, b) in x {{ items.push(Self::P{ca}(a)); items.push(Self::P{cb}(b)); }}"
                );
                let _ = writeln!(
                    self.drop_ops,
                    "                {{OP}}::M{k}(x, _) => items.push(Self::P{k}(x)),"
                );
                let next = format!(
                    "if let Some((a, b)) = it.next() {{ ops.push({{OP}}::J{k}(x, it)); ops.push({{OP}}::C{cb}(b)); ops.push({{OP}}::C{ca}(a)); }} else {{ vals.push(Self::P{k}(x)); }}"
                );
                self.clone_arm(
                    &format!("    J{k}(Vec<{pair}>, std::slice::Iter<'a, {pair}>),\n"),
                    &format!("{{OP}}::C{k}(x) => {{ let mut it = x.iter(); let x = Vec::with_capacity(x.len()); {next} }}"),
                );
                self.clone_arm(
                    "",
                    &format!(
                        "{{OP}}::J{k}(mut x, mut it) => {{ {} {} x.push((a, b)); {next} }}",
                        Self::pop(cb, "b"),
                        Self::pop(ca, "a")
                    ),
                );
                self.eq_arm(&format!("Self::P{k}(a, b) => {{ if a.len() != b.len() {{ return false; }} for ((a0, a1), (b0, b1)) in a.iter().zip(b).rev() {{ stack.push(Self::P{cb}(a1, b1)); stack.push(Self::P{ca}(a0, b0)); }} }}"));
                if self.plain {
                    let (ra, rb) = (self.rust(a).to_owned(), self.rust(b).to_owned());
                    let _ = writeln!(self.shows, "    M{k}(&'a {ra}, &'a {rb}),");
                }
                self.show_arm(&format!(
                    "Self::P{k}(x) => {}",
                    Self::open(
                        "[",
                        "]",
                        &format!("x.iter().map(|(a, b)| (\"\", Self::M{k}(a, b)))")
                    )
                ));
                self.show_arm(&format!(
                    "Self::M{k}(a, b) => {}",
                    Self::open(
                        "(",
                        ")",
                        &format!("[(\"\", Self::P{ca}(a)), (\"\", Self::P{cb}(b))]")
                    )
                ));
                k
            }
            Value::Option(t) => {
                let c = self.walk(t, false);
                let _ = writeln!(self.ops, "    S{k},");
                let _ = writeln!(
                    self.dec,
                    "                {{OP}}::D{k} => if d.tag(2)? == 0 {{ vals.push(Self::P{k}(None)); }} else {{ ops.push({{OP}}::S{k}); ops.push({{OP}}::D{c}); }}"
                );
                let _ = writeln!(
                    self.dec,
                    "                {{OP}}::S{k} => {{ {} vals.push(Self::P{k}(Some(x))); }}",
                    Self::pop(c, "x")
                );
                let _ = writeln!(
                    self.enc,
                    "                Self::P{k}(x) => match x {{ None => e.tag(0), Some(x) => {{ e.tag(1); stack.push(Self::P{c}(x)); }} }}"
                );
                let _ = writeln!(
                    self.drop,
                    "                Self::P{k}(Some(x)) => items.push(Self::P{c}(x)),"
                );
                self.clone_arm(
                    "",
                    &format!("{{OP}}::C{k}(None) => vals.push(Self::P{k}(None)),"),
                );
                self.clone_arm(
                    "",
                    &format!("{{OP}}::C{k}(Some(x)) => {{ ops.push({{OP}}::S{k}); ops.push({{OP}}::C{c}(x)); }}"),
                );
                self.eq_arm(&format!("Self::P{k}(None, None) => {{}}"));
                self.show_arm(&format!("Self::P{k}(None) => f.write_str(\"None\"),"));
                self.show_arm(&format!(
                    "Self::P{k}(Some(x)) => {}",
                    Self::open("Some(", ")", &format!("[(\"\", Self::P{c}(x))]"))
                ));
                self.eq_arm(&format!(
                    "Self::P{k}(Some(a), Some(b)) => stack.push(Self::P{c}(a, b)),"
                ));
                self.eq_arm(&format!("Self::P{k}(..) => return false,"));
                k
            }
            Value::Result(a, b) => {
                let (ca, cb) = (self.walk(a, false), self.walk(b, false));
                let _ = writeln!(self.ops, "    O{k},\n    R{k},");
                let _ = writeln!(
                    self.dec,
                    "                {{OP}}::D{k} => if d.tag(2)? == 0 {{ ops.push({{OP}}::O{k}); ops.push({{OP}}::D{ca}); }} else {{ ops.push({{OP}}::R{k}); ops.push({{OP}}::D{cb}); }}"
                );
                let _ = writeln!(
                    self.dec,
                    "                {{OP}}::O{k} => {{ {} vals.push(Self::P{k}(Ok(x))); }}",
                    Self::pop(ca, "x")
                );
                let _ = writeln!(
                    self.dec,
                    "                {{OP}}::R{k} => {{ {} vals.push(Self::P{k}(Err(x))); }}",
                    Self::pop(cb, "x")
                );
                let _ = writeln!(
                    self.enc,
                    "                Self::P{k}(x) => match x {{ Ok(x) => {{ e.tag(0); stack.push(Self::P{ca}(x)); }} Err(x) => {{ e.tag(1); stack.push(Self::P{cb}(x)); }} }}"
                );
                let _ = writeln!(
                    self.drop,
                    "                Self::P{k}(Ok(x)) => items.push(Self::P{ca}(x)),\n                Self::P{k}(Err(x)) => items.push(Self::P{cb}(x)),"
                );
                self.clone_arm(
                    "",
                    &format!("{{OP}}::C{k}(Ok(x)) => {{ ops.push({{OP}}::O{k}); ops.push({{OP}}::C{ca}(x)); }}"),
                );
                self.clone_arm(
                    "",
                    &format!("{{OP}}::C{k}(Err(x)) => {{ ops.push({{OP}}::R{k}); ops.push({{OP}}::C{cb}(x)); }}"),
                );
                self.eq_arm(&format!(
                    "Self::P{k}(Ok(a), Ok(b)) => stack.push(Self::P{ca}(a, b)),"
                ));
                self.show_arm(&format!(
                    "Self::P{k}(Ok(x)) => {}",
                    Self::open("Ok(", ")", &format!("[(\"\", Self::P{ca}(x))]"))
                ));
                self.show_arm(&format!(
                    "Self::P{k}(Err(x)) => {}",
                    Self::open("Err(", ")", &format!("[(\"\", Self::P{cb}(x))]"))
                ));
                self.eq_arm(&format!(
                    "Self::P{k}(Err(a), Err(b)) => stack.push(Self::P{cb}(a, b)),"
                ));
                self.eq_arm(&format!("Self::P{k}(..) => return false,"));
                k
            }
            // ruled out: `mark` puts only these forms on the spine.
            _ => unreachable!("only containers reach a reference"),
        }
    }

    /// The build op, encode arm and dismantle arm of a record, a tuple or
    /// an enum variant at `k` whose fields are at `cs`; `names` is empty
    /// for a tuple. A variant's build op is `Bkvn`.
    #[allow(clippy::too_many_arguments)]
    fn build(
        &mut self,
        k: usize,
        open: &str,
        close: &str,
        names: &[String],
        cs: &[usize],
        values: &[&Value],
        variant: Option<usize>,
    ) {
        let op = match variant {
            Some(n) => format!("B{k}v{n}"),
            None => format!("B{k}"),
        };
        let _ = writeln!(self.ops, "    {op},");
        let field = |n: usize, x: &str| {
            if names.is_empty() {
                x.to_owned()
            } else {
                format!("{}: {x}", names[n])
            }
        };
        let all: Vec<String> = (0..cs.len()).map(|n| field(n, &format!("f{n}"))).collect();
        let shape = format!("{open} {} {close}", all.join(", "));
        let mut pops = String::new();
        for (n, c) in cs.iter().enumerate().rev() {
            pops.push_str(&Self::pop(*c, &format!("f{n}")));
            pops.push(' ');
        }
        let _ = writeln!(
            self.dec,
            "                {{OP}}::{op} => {{ {pops}vals.push(Self::P{k}({shape})); }}"
        );
        let pushes = cs
            .iter()
            .enumerate()
            .rev()
            .fold(String::new(), |mut out, (n, c)| {
                let _ = write!(out, " stack.push(Self::P{c}(f{n}));");
                out
            });
        let tag = variant.map_or(String::new(), |n| format!(" e.tag({n});"));
        let _ = writeln!(
            self.enc,
            "                Self::P{k}({shape}) => {{{tag}{pushes} }}"
        );
        if variant.is_none() {
            let _ = writeln!(
                self.dec,
                "                {{OP}}::D{k} => {{ ops.push({{OP}}::{op});{} }}",
                Self::push_decs(cs)
            );
        }
        // Only the children that reach a reference are dismantled through
        // the stack; the rest drop in place.
        let mut bound = Vec::new();
        let mut pushes = String::new();
        for (n, (c, v)) in cs.iter().zip(values).enumerate() {
            if self.spine.contains(&addr(v)) {
                bound.push(field(n, &format!("f{n}")));
                let _ = write!(pushes, " items.push(Self::P{c}(f{n}));");
            } else if names.is_empty() {
                bound.push("_".to_owned());
            }
        }
        if !names.is_empty() {
            bound.push("..".to_owned());
        }
        let _ = writeln!(
            self.drop,
            "                Self::P{k}({open} {} {close}) => {{{pushes} }}",
            bound.join(", ")
        );
        // A clone reads the fields in order, as the decoder does; a compare
        // pairs them up.
        let reads = cs
            .iter()
            .enumerate()
            .rev()
            .fold(String::new(), |mut out, (n, c)| {
                let _ = write!(out, " ops.push({{OP}}::C{c}(f{n}));");
                out
            });
        self.clone_arm(
            "",
            &format!("{{OP}}::C{k}({shape}) => {{ ops.push({{OP}}::{op});{reads} }}"),
        );
        let side = |x: &str| {
            let all: Vec<String> = (0..cs.len())
                .map(|n| field(n, &format!("{x}{n}")))
                .collect();
            format!("{open} {} {close}", all.join(", "))
        };
        let pairs = cs
            .iter()
            .enumerate()
            .rev()
            .fold(String::new(), |mut out, (n, c)| {
                let _ = write!(out, " stack.push(Self::P{c}(a{n}, b{n}));");
                out
            });
        let (a, b) = (side("a"), side("b"));
        self.eq_arm(&format!("Self::P{k}({a}, {b}) => {{{pairs} }}"));
        // `Debug` names a struct by its type and a variant by itself, as
        // a derived one does.
        let shown = open
            .rsplit("::")
            .next()
            .unwrap_or(open)
            .trim_start_matches("r#");
        let children: Vec<String> = cs
            .iter()
            .enumerate()
            .map(|(n, c)| {
                let label = names.get(n).map_or(String::new(), |name| {
                    format!("{}: ", name.trim_start_matches("r#"))
                });
                format!("({}, Self::P{c}(f{n}))", quoted(&label))
            })
            .collect();
        let close = if names.is_empty() { ")" } else { "}" };
        self.show_arm(&format!(
            "Self::P{k}({shape}) => {}",
            Self::open(shown, close, &format!("[{}]", children.join(", ")))
        ));
    }

    /// Records `rust`, a named type of the group at position `k`, the first
    /// time it occurs.
    fn name(&mut self, rust: &str, k: usize) {
        if self.seen.insert(rust.to_owned()) {
            self.named.push((rust.to_owned(), k));
        }
    }

    /// The group's enums, their loops, the root's codec methods, and, in
    /// a group without handles, each named type's `Clone` and `PartialEq`.
    fn finish(self, root: &str) -> String {
        let (wire, op) = (&self.wire, &self.op);
        // A clone op borrows the value it reads.
        let (lt, lu) = if self.plain {
            ("<'a>", "<'_>")
        } else {
            ("", "")
        };
        let fill = |s: &str| s.replace("{OP}", op);
        let drop_ops = if self.drop_ops.is_empty() {
            "        let _ = (ops, items);\n".to_owned()
        } else {
            format!(
                "        for op in ops {{\n            match op {{\n{}                _ => {{}}\n            }}\n        }}\n",
                fill(&self.drop_ops)
            )
        };
        format!(
            "
#[allow(dead_code)]
impl {root} {{
    fn wire_encode(self, e: &mut {WIRE}::Encoder<'_>) -> {NATIVE}::HostResult<()> {{
        {wire}::encode(self, e)
    }}

    fn wire_decode(d: &mut {WIRE}::Decoder<'_>) -> {NATIVE}::HostResult<Self> {{
        {wire}::decode(d)
    }}

    /// Drops this value one level at a time, in constant native stack,
    /// where dropping it in place would recurse once per nesting level.
    pub fn dismantle(self) {{
        {wire}::dismantle(vec![{wire}::P0(self)]);
    }}
}}

/// A value of `{root}`'s recursive group at one position of its codec.
#[allow(dead_code, clippy::large_enum_variant)]
enum {wire} {{
{vals}}}

/// A step of `{root}`'s decoder, or of a clone of one of its group's values.
#[allow(dead_code, clippy::large_enum_variant)]
enum {op}{lt} {{
{ops}}}

#[allow(dead_code, unreachable_patterns, clippy::match_single_binding)]
impl {wire} {{
    fn encode(root: {root}, e: &mut {WIRE}::Encoder<'_>) -> {NATIVE}::HostResult<()> {{
        let mut stack = vec![Self::P0(root)];
        let written = Self::encode_all(&mut stack, e);
        Self::dismantle(stack);
        written
    }}

    fn encode_all(stack: &mut Vec<Self>, e: &mut {WIRE}::Encoder<'_>) -> {NATIVE}::HostResult<()> {{
        while let Some(item) = stack.pop() {{
            match item {{
{enc}            }}
        }}
        Ok(())
    }}

    fn decode(d: &mut {WIRE}::Decoder<'_>) -> {NATIVE}::HostResult<{root}> {{
        let mut ops = vec![{op}::D0];
        let mut vals = Vec::new();
        if let Err(error) = Self::decode_all(d, &mut ops, &mut vals) {{
            Self::dismantle_ops(ops, &mut vals);
            Self::dismantle(vals);
            return Err(error);
        }}
        let Some(Self::P0(root)) = vals.pop() else {{ unreachable!(\"ruled out: a finished decode leaves its root\") }};
        Ok(root)
    }}

    fn decode_all(d: &mut {WIRE}::Decoder<'_>, ops: &mut Vec<{op}{lu}>, vals: &mut Vec<Self>) -> {NATIVE}::HostResult<()> {{
        while let Some(op) = ops.pop() {{
            match op {{
{dec}            }}
        }}
        Ok(())
    }}

    /// The partial lists and maps a failed decode leaves in its ops.
    fn dismantle_ops(ops: Vec<{op}{lu}>, items: &mut Vec<Self>) {{
{drop_ops}    }}

    /// Drops values of the group one position at a time, so a deep value
    /// never drops recursively.
    fn dismantle(mut items: Vec<Self>) {{
        while let Some(item) = items.pop() {{
            match item {{
{drop}                _ => {{}}
            }}
        }}
    }}
}}
",
            vals = self.vals,
            ops = self.ops,
            enc = self.enc,
            dec = fill(&self.dec),
            drop = self.drop,
        ) + &self.copies(root)
    }

    /// The clone and compare loops of a group without handles, and each
    /// named type's `Clone` and `PartialEq` starting them at its position:
    /// a derived one would recurse once per nesting level.
    fn copies(&self, root: &str) -> String {
        if !self.plain {
            return String::new();
        }
        let (wire, op, pair, show) = (&self.wire, &self.op, &self.pair, &self.show);
        let mut out = format!(
            "
#[allow(dead_code)]
impl {wire} {{
    /// The value `op` reads, built bottom-up through the decoder's own
    /// build ops: a clone reads no bytes.
    fn clone_of(op: {op}<'_>) -> Self {{
        let mut ops = vec![op];
        let mut vals = Vec::new();
        let Ok(()) = Self::decode_all(&mut {WIRE}::Decoder::new(&[]), &mut ops, &mut vals) else {{ unreachable!(\"ruled out: a clone reads no bytes\") }};
        let Some(value) = vals.pop() else {{ unreachable!(\"ruled out: a finished clone leaves its root\") }};
        value
    }}
}}

/// Two values of `{root}`'s recursive group at one position, compared one
/// position at a time.
#[allow(dead_code)]
enum {pair}<'a> {{
{pairs}}}

#[allow(dead_code, unreachable_patterns, clippy::match_single_binding)]
impl {pair}<'_> {{
    fn all(mut stack: Vec<Self>) -> bool {{
        while let Some(pair) = stack.pop() {{
            match pair {{
{eq}            }}
        }}
        true
    }}
}}
",
            pairs = self.pairs,
            eq = self.eq,
        );
        let _ = write!(
            out,
            "
/// A value of `{root}`'s recursive group at one position of its `Debug`,
/// or a map entry's pair of positions.
#[allow(dead_code)]
enum {show}<'a> {{
{shows}}}

#[allow(dead_code, unreachable_patterns, clippy::match_single_binding)]
impl {show}<'_> {{
    fn show(self, depth: usize, stack: &mut Vec<{WIRE}::Show<Self>>, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{
        match self {{
{arms}        }}
    }}
}}
",
            shows = self.shows,
            arms = self.show_arms,
        );
        for (name, k) in &self.named {
            let _ = write!(
                out,
                "
#[allow(irrefutable_let_patterns)]
impl Clone for {name} {{
    fn clone(&self) -> Self {{
        let {wire}::P{k}(value) = {wire}::clone_of({op}::C{k}(self)) else {{ unreachable!(\"ruled out: a clone builds the position it reads\") }};
        value
    }}
}}

impl PartialEq for {name} {{
    fn eq(&self, other: &Self) -> bool {{
        {pair}::all(vec![{pair}::P{k}(self, other)])
    }}
}}

impl std::fmt::Debug for {name} {{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{
        {WIRE}::Show::run(f, {show}::P{k}(self), {show}::show)
    }}
}}
"
            );
        }
        out
    }
}

/// `next_event` -> `NextEvent`, `screen` -> `Screen`.
fn camel(s: &str) -> String {
    let mut out = String::new();
    let mut up = true;
    for c in s.chars() {
        if c == '_' {
            up = true;
        } else if up {
            out.extend(c.to_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// `NextEvent` -> `next_event`; already snake text is unchanged.
fn snake(s: &str) -> String {
    let mut out = String::new();
    for (n, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if n != 0 {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// A Rust identifier for an hd name: a raw identifier for a Rust keyword,
/// a trailing `_` for the four keywords no raw identifier may spell, and a
/// leading `_` for a union case's positional payload `0`, which no Rust
/// field declaration may spell.
fn ident(s: &str) -> String {
    const STRICT: &[&str] = &[
        "as", "async", "await", "break", "const", "continue", "dyn", "else", "enum", "extern",
        "false", "fn", "for", "gen", "if", "impl", "in", "let", "loop", "match", "mod", "move",
        "mut", "pub", "ref", "return", "static", "struct", "trait", "true", "try", "type",
        "unsafe", "use", "where", "while", "abstract", "become", "box", "do", "final", "macro",
        "override", "priv", "typeof", "unsized", "virtual", "yield",
    ];
    if s.starts_with(|c: char| c.is_ascii_digit()) {
        format!("_{s}")
    } else if matches!(s, "self" | "Self" | "super" | "crate" | "_") {
        format!("{s}_")
    } else if STRICT.contains(&s) {
        format!("r#{s}")
    } else {
        s.to_owned()
    }
}

/// A Rust string literal of `s`.
fn quoted(s: &str) -> String {
    format!("{s:?}")
}
