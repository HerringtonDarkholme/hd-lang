//! The ABI model both hosts' codecs come from (#251): every host method of
//! `hd_host_abi`'s tables with the std types its values cross as, and each
//! std data type, enum and newtype those types reach, read from `lib/std`
//! itself. The model is checked in as `host/gen/abi.json`, and the JS
//! host's codecs and method descriptors generated from it as
//! `host/gen/abi.mjs`; these tests fail when the tables or std change and
//! the files do not. `HD_UPDATE_ABI=1` rewrites them.
//!
//! A type is its source text. `Result[T, E]` (variant 0 `Ok`, 1 `Err`),
//! `T?`, `List[T]`, tuples and the primitives are the language's, and a
//! host handle (`HANDLES`) crosses as its handle number.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use hd_host_abi::{
    Codec, HANDLES, HostMethod, HostTrait, RUNNERS, ResourceArg, Scalar, TABLE, Wait,
};
use hd_syntax::{NodeRef, Parse, SyntaxKind};

/// The names a type text may use that std does not declare.
const LANGUAGE: &[&str] = &[
    "void", "bool", "char", "string", "u8", "u16", "u32", "u64", "usize", "i8", "i16", "i32",
    "i64", "isize", "f32", "f64", "List", "Map", "Result",
];

/// A std declaration a boundary type reaches.
enum Decl {
    Data {
        generics: Vec<String>,
        fields: Vec<(String, String)>,
    },
    Enum {
        generics: Vec<String>,
        variants: Vec<(String, Vec<(String, String)>)>,
    },
    Newtype(String),
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The source text of `n`, its whitespace collapsed.
fn text(p: &Parse, src: &str, n: NodeRef<'_>) -> String {
    let (a, _) = p.tokens.span(n.first_token());
    let (_, b) = p.tokens.span(n.last_token());
    src[a as usize..b as usize]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn name(p: &Parse, src: &str, n: NodeRef<'_>) -> Option<String> {
    n.name(&p.tokens).map(|t| p.tokens.text(t, src).to_owned())
}

fn ty(p: &Parse, src: &str, n: NodeRef<'_>) -> Option<String> {
    n.children()
        .find(|c| c.kind().is_type())
        .map(|c| text(p, src, c))
}

fn generics(p: &Parse, src: &str, n: NodeRef<'_>) -> Vec<String> {
    n.child(SyntaxKind::GenericParameterList)
        .into_iter()
        .flat_map(NodeRef::children)
        .filter_map(|g| name(p, src, g))
        .collect()
}

/// Every top-level data type, enum and newtype of `lib/std`, by name, with
/// the module files that declare one of that name.
fn std_decls() -> BTreeMap<String, Vec<(String, Decl)>> {
    let mut out: BTreeMap<String, Vec<(String, Decl)>> = BTreeMap::new();
    let mut files = Vec::new();
    let mut dirs = vec![root().join("lib/std")];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).expect("lib/std") {
            let path = e.expect("entry").path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|x| x == "hd") {
                files.push(path);
            }
        }
    }
    files.sort();
    for f in files {
        let src = std::fs::read_to_string(&f).expect("std file");
        let p = hd_syntax::parse(src.as_bytes());
        let module = f
            .strip_prefix(root())
            .unwrap_or(&f)
            .to_string_lossy()
            .into_owned();
        for n in p.tree.root().children() {
            let Some(id) = name(&p, &src, n) else {
                continue;
            };
            let block = || {
                n.child(SyntaxKind::Block)
                    .into_iter()
                    .flat_map(NodeRef::children)
            };
            let decl = match n.kind() {
                SyntaxKind::DataDecl => Decl::Data {
                    generics: generics(&p, &src, n),
                    fields: block()
                        .filter(|c| c.kind() == SyntaxKind::DataField)
                        .filter_map(|c| Some((name(&p, &src, c)?, ty(&p, &src, c)?)))
                        .collect(),
                },
                SyntaxKind::EnumDecl => Decl::Enum {
                    generics: generics(&p, &src, n),
                    variants: block()
                        .filter(|c| c.kind() == SyntaxKind::EnumVariant)
                        .filter_map(|v| {
                            let fields = v
                                .child(SyntaxKind::ParameterList)
                                .into_iter()
                                .flat_map(NodeRef::children)
                                .filter_map(|q| Some((name(&p, &src, q)?, ty(&p, &src, q)?)))
                                .collect();
                            Some((name(&p, &src, v)?, fields))
                        })
                        .collect(),
                },
                SyntaxKind::TypeDecl => match ty(&p, &src, n) {
                    Some(of) => Decl::Newtype(of),
                    None => continue,
                },
                _ => continue,
            };
            out.entry(id).or_default().push((module.clone(), decl));
        }
    }
    out
}

/// The capitalized identifiers of a type text, in order.
fn names_in(t: &str) -> Vec<&str> {
    t.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty() && !w.starts_with(|c: char| c.is_ascii_digit()))
        .collect()
}

fn codec(c: &Codec) -> String {
    match c {
        Codec::Void => "void".into(),
        Codec::Scalar(Scalar::I32) => "i32".into(),
        Codec::Scalar(Scalar::I64) => "i64".into(),
        Codec::Scalar(Scalar::F64) => "f64".into(),
        Codec::Buffer(t) => (*t).to_owned(),
    }
}

/// A JSON string.
fn js(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn fields_json(fields: &[(String, String)]) -> String {
    let items: Vec<String> = fields
        .iter()
        .map(|(n, t)| format!("{{ \"name\": {}, \"type\": {} }}", js(n), js(t)))
        .collect();
    format!("[{}]", items.join(", "))
}

/// The three tables, by their names in the model.
fn groups() -> [(&'static str, &'static [HostTrait]); 3] {
    [
        ("traits", TABLE),
        ("runners", RUNNERS),
        ("handles", HANDLES),
    ]
}

/// What a name that a boundary type reaches is: a host handle, or a std
/// declaration and the module file that declares it.
enum Reached<'a> {
    Handle,
    Std(&'a str, &'a Decl),
}

/// Every std type the methods' types reach, by name.
fn reached(decls: &BTreeMap<String, Vec<(String, Decl)>>) -> BTreeMap<String, Reached<'_>> {
    let handles: Vec<&str> = HANDLES
        .iter()
        .filter_map(|h| h.std_path.rsplit('.').next())
        .collect();
    let mut todo: Vec<(String, Vec<String>)> = Vec::new();
    for (_, table) in groups() {
        for m in table.iter().flat_map(|t| t.methods) {
            todo.extend(m.params.iter().map(|p| (codec(p), Vec::new())));
            todo.push((codec(&m.result), Vec::new()));
        }
    }
    let mut types = BTreeMap::new();
    while let Some((t, scope)) = todo.pop() {
        for n in names_in(&t) {
            if LANGUAGE.contains(&n) || scope.iter().any(|g| g == n) || types.contains_key(n) {
                continue;
            }
            if handles.contains(&n) {
                types.insert(n.to_owned(), Reached::Handle);
                continue;
            }
            let found = decls.get(n).map_or(&[][..], Vec::as_slice);
            let [(module, decl)] = found else {
                panic!("`{n}` (in `{t}`) names {} std declarations", found.len());
            };
            match decl {
                Decl::Data { generics, fields } => {
                    todo.extend(fields.iter().map(|f| (f.1.clone(), generics.clone())));
                }
                Decl::Enum { generics, variants } => {
                    for (_, fs) in variants {
                        todo.extend(fs.iter().map(|f| (f.1.clone(), generics.clone())));
                    }
                }
                Decl::Newtype(of) => todo.push((of.clone(), Vec::new())),
            }
            types.insert(n.to_owned(), Reached::Std(module, decl));
        }
    }
    types
}

fn generics_json(generics: &[String]) -> String {
    generics
        .iter()
        .map(|g| js(g))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The model's JSON text.
fn model() -> String {
    let decls = std_decls();
    let mut out = String::from("{\n");
    for (group, table) in groups() {
        let _ = writeln!(out, "  {}: [", js(group));
        for (ti, t) in table.iter().enumerate() {
            let _ = writeln!(
                out,
                "    {{ \"key\": {}, \"std\": {}, \"methods\": [",
                js(t.key),
                js(t.std_path)
            );
            for (mi, m) in t.methods.iter().enumerate() {
                let resource = m.resource.map_or("null".to_owned(), |r| js(resource(r)));
                let params: Vec<String> = m.params.iter().map(|p| js(&codec(p))).collect();
                let _ = write!(
                    out,
                    "      {{ \"name\": {}, \"wait\": {}, \"resource\": {resource}, \"params\": [{}], \"result\": {} }}",
                    js(m.name),
                    js(wait(m.wait)),
                    params.join(", "),
                    js(&codec(&m.result))
                );
                out.push_str(if mi + 1 < t.methods.len() {
                    ",\n"
                } else {
                    "\n"
                });
            }
            out.push_str(if ti + 1 < table.len() {
                "    ]},\n"
            } else {
                "    ]}\n"
            });
        }
        out.push_str("  ],\n");
    }
    out.push_str("  \"types\": {\n");
    let rows: Vec<String> = reached(&decls)
        .iter()
        .map(|(n, r)| {
            let json = match r {
                Reached::Handle => "{ \"kind\": \"handle\" }".to_owned(),
                Reached::Std(module, Decl::Data { generics, fields }) => format!(
                    "{{ \"kind\": \"data\", \"module\": {}, \"generics\": [{}], \"fields\": {} }}",
                    js(module),
                    generics_json(generics),
                    fields_json(fields)
                ),
                Reached::Std(module, Decl::Enum { generics, variants }) => {
                    let vs: Vec<String> = variants
                        .iter()
                        .map(|(v, fs)| {
                            format!("{{ \"name\": {}, \"fields\": {} }}", js(v), fields_json(fs))
                        })
                        .collect();
                    format!(
                        "{{ \"kind\": \"enum\", \"module\": {}, \"generics\": [{}], \"variants\": [\n      {}\n    ] }}",
                        js(module),
                        generics_json(generics),
                        vs.join(",\n      ")
                    )
                }
                Reached::Std(module, Decl::Newtype(of)) => format!(
                    "{{ \"kind\": \"newtype\", \"module\": {}, \"of\": {} }}",
                    js(module),
                    js(of)
                ),
            };
            format!("    {}: {json}", js(n))
        })
        .collect();
    out.push_str(&rows.join(",\n"));
    out.push_str("\n  }\n}\n");
    out
}

fn resource(r: ResourceArg) -> &'static str {
    match r {
        ResourceArg::Path => "path",
        ResourceArg::Host => "host",
        ResourceArg::Addr => "addr",
        ResourceArg::EnvName => "env-name",
        ResourceArg::Program => "program",
        ResourceArg::SysName => "sys-name",
    }
}

fn wait(w: Wait) -> &'static str {
    match w {
        Wait::Never => "never",
        Wait::May => "may",
    }
}

/// A parsed type text: a name and its type arguments, a tuple, or `T?`.
enum Ty {
    Named(String, Vec<Ty>),
    Tuple(Vec<Ty>),
    Option(Box<Ty>),
}

/// Parses the type text `t`.
fn parse_ty(t: &str) -> Ty {
    fn list(s: &mut &str, close: char) -> Vec<Ty> {
        let mut out = vec![one(s)];
        loop {
            *s = s.trim_start();
            if let Some(rest) = s.strip_prefix(',') {
                *s = rest;
                out.push(one(s));
            } else if let Some(rest) = s.strip_prefix(close) {
                *s = rest;
                return out;
            } else {
                panic!("a type text has `{s}` where `,` or `{close}` belongs");
            }
        }
    }
    fn one(s: &mut &str) -> Ty {
        *s = s.trim_start();
        let mut ty = if let Some(rest) = s.strip_prefix('(') {
            *s = rest;
            Ty::Tuple(list(s, ')'))
        } else {
            let end = s
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(s.len());
            assert!(end > 0, "a type text has `{s}` where a name belongs");
            let name = s[..end].to_owned();
            *s = &s[end..];
            let args = match s.strip_prefix('[') {
                Some(rest) => {
                    *s = rest;
                    list(s, ']')
                }
                None => Vec::new(),
            };
            Ty::Named(name, args)
        };
        while let Some(rest) = s.strip_prefix('?') {
            *s = rest;
            ty = Ty::Option(Box::new(ty));
        }
        ty
    }
    let mut s = t;
    let ty = one(&mut s);
    assert!(s.trim().is_empty(), "`{t}` has `{s}` after its type");
    ty
}

/// The JS codec (host/wire.mjs, or a generated std type's) of `ty`;
/// `scope` holds the generic parameters in scope. `used` collects the
/// wire codecs it names.
fn js_codec(
    ty: &Ty,
    scope: &[String],
    types: &BTreeMap<String, Reached<'_>>,
    used: &mut BTreeSet<&'static str>,
) -> String {
    match ty {
        Ty::Option(inner) => {
            assert!(
                !matches!(**inner, Ty::Option(_)),
                "`T??` has no JS codec: `null` is both `.None`s"
            );
            let inner = js_codec(inner, scope, types, used);
            format!("{}({inner})", wire(used, "option"))
        }
        Ty::Tuple(ts) => {
            let ts: Vec<String> = ts.iter().map(|t| js_codec(t, scope, types, used)).collect();
            format!("{}({})", wire(used, "tuple"), ts.join(", "))
        }
        Ty::Named(name, args) => match (name.as_str(), args.as_slice()) {
            ("void", []) => wire(used, "unit"),
            ("string", []) => wire(used, "str"),
            ("u8" | "u16" | "u32" | "usize", []) => wire(used, "unsigned"),
            ("u64", []) => wire(used, "unsigned64"),
            ("i8" | "i16" | "i32" | "isize", []) => wire(used, "signed"),
            ("i64", []) => wire(used, "signed64"),
            ("List", [Ty::Named(u8, a)]) if u8 == "u8" && a.is_empty() => wire(used, "bytes"),
            ("List", [t]) => {
                let t = js_codec(t, scope, types, used);
                format!("{}({t})", wire(used, "list"))
            }
            ("Result", [ok, err]) => {
                let ok = js_codec(ok, scope, types, used);
                let err = js_codec(err, scope, types, used);
                format!("{}({ok}, {err})", wire(used, "result"))
            }
            (g, []) if scope.iter().any(|s| s == g) => g.to_owned(),
            (n, []) if types.contains_key(n) => n.to_owned(),
            (n, args) if types.contains_key(n) => {
                let args: Vec<String> = args
                    .iter()
                    .map(|t| js_codec(t, scope, types, used))
                    .collect();
                format!("{n}({})", args.join(", "))
            }
            (n, _) => panic!("no JS codec for `{n}`: add one to host/wire.mjs"),
        },
    }
}

/// The wire codec `name`, noted in `used` for the import.
fn wire(used: &mut BTreeSet<&'static str>, name: &'static str) -> String {
    used.insert(name);
    name.to_owned()
}

/// The JS value of one host method's parameter or result: a scalar is its
/// Wasm type's name, a buffer value its codec.
fn js_value(
    c: &Codec,
    types: &BTreeMap<String, Reached<'_>>,
    used: &mut BTreeSet<&'static str>,
) -> String {
    match c {
        Codec::Void => js("void"),
        Codec::Scalar(Scalar::I32) => js("i32"),
        Codec::Scalar(Scalar::I64) => js("i64"),
        Codec::Scalar(Scalar::F64) => panic!("no JS glue for an `f64` scalar yet"),
        Codec::Buffer(t) => js_codec(&parse_ty(t), &[], types, used),
    }
}

/// The std types of `types` in an order where each comes after the types
/// its fields name, so each `const` is defined before its first use.
fn ordered<'a>(types: &'a BTreeMap<String, Reached<'a>>) -> Vec<(&'a str, &'a Reached<'a>)> {
    fn visit<'a>(
        n: &'a str,
        types: &'a BTreeMap<String, Reached<'a>>,
        seen: &mut BTreeSet<&'a str>,
        out: &mut Vec<(&'a str, &'a Reached<'a>)>,
    ) {
        if !seen.insert(n) {
            return;
        }
        let r = &types[n];
        let fields: Vec<&str> = match r {
            Reached::Handle => Vec::new(),
            Reached::Std(_, Decl::Data { fields, .. }) => {
                fields.iter().map(|f| f.1.as_str()).collect()
            }
            Reached::Std(_, Decl::Enum { variants, .. }) => variants
                .iter()
                .flat_map(|v| v.1.iter().map(|f| f.1.as_str()))
                .collect(),
            Reached::Std(_, Decl::Newtype(of)) => vec![of.as_str()],
        };
        for t in fields {
            for d in names_in(t) {
                if let Some((k, _)) = types.get_key_value(d) {
                    visit(k, types, seen, out);
                }
            }
        }
        out.push((n, r));
    }
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for n in types.keys() {
        visit(n, types, &mut seen, &mut out);
    }
    out
}

/// The fields of a data type or variant as an object of codecs, inline.
fn js_fields(
    fields: &[(String, String)],
    scope: &[String],
    types: &BTreeMap<String, Reached<'_>>,
    used: &mut BTreeSet<&'static str>,
) -> String {
    if fields.is_empty() {
        return "{}".to_owned();
    }
    let fs: Vec<String> = fields
        .iter()
        .map(|(n, t)| format!("{n}: {}", js_codec(&parse_ty(t), scope, types, used)))
        .collect();
    format!("{{ {} }}", fs.join(", "))
}

/// The text of host/gen/abi.mjs: every std type the model reaches as a
/// codec, and every host method's descriptor by import module and name.
fn glue() -> String {
    let decls = std_decls();
    let types = reached(&decls);
    let mut used = BTreeSet::new();
    let mut body =
        String::from("// Each std type a host method crosses, from its declaration in lib/std.\n");
    for (n, r) in ordered(&types) {
        let def = match r {
            Reached::Handle => {
                used.insert("handle");
                "handle;".to_owned()
            }
            Reached::Std(_, Decl::Newtype(of)) => {
                let of = js_codec(&parse_ty(of), &[], &types, &mut used);
                used.insert("newtype");
                format!("newtype({of});")
            }
            Reached::Std(_, Decl::Data { generics, fields }) => {
                used.insert("data");
                let mut s = String::from("data({\n");
                for (f, t) in fields {
                    let c = js_codec(&parse_ty(t), generics, &types, &mut used);
                    let _ = writeln!(s, "{}  {f}: {c},", indent(generics));
                }
                let _ = write!(s, "{}}});", indent(generics));
                generic(generics, &s)
            }
            Reached::Std(_, Decl::Enum { generics, variants }) => {
                used.insert("variants");
                let mut s = String::from("variants({\n");
                for (v, fs) in variants {
                    let c = js_fields(fs, generics, &types, &mut used);
                    let _ = writeln!(s, "{}  {v}: {c},", indent(generics));
                }
                let _ = write!(s, "{}}});", indent(generics));
                generic(generics, &s)
            }
        };
        let _ = writeln!(body, "export const {n} = {def}");
    }
    body.push_str(
        "\n// Each host method by import module and name (crates/hd_run_wasmtime/DESIGN.md\n\
         // §17.2): `wait` is \"never\" for one import or \"may\" for `.start` and\n\
         // `.finish`; `resource` names the argument a grant checks; a scalar\n\
         // parameter or result is its Wasm type, any other a codec over the\n\
         // exchange buffer.\n\
         export const METHODS = {\n",
    );
    let mut modules: Vec<(&str, Vec<&HostMethod>)> = Vec::new();
    for (_, table) in groups() {
        for t in table {
            match modules.iter_mut().find(|(k, _)| *k == t.key) {
                Some((_, ms)) => ms.extend(t.methods),
                None => modules.push((t.key, t.methods.iter().collect())),
            }
        }
    }
    for (key, methods) in modules {
        let _ = writeln!(body, "  {}: {{", js(&format!("hd:{key}")));
        for m in methods {
            assert!(
                m.wait == Wait::Never || !matches!(m.result, Codec::Scalar(_)),
                "`{key}` `{}` waits, so its result crosses in the buffer",
                m.name
            );
            let params: Vec<String> = m
                .params
                .iter()
                .map(|p| js_value(p, &types, &mut used))
                .collect();
            let name = if m.name.contains('.') {
                js(m.name)
            } else {
                m.name.to_owned()
            };
            let _ = writeln!(body, "    {name}: {{");
            let _ = writeln!(body, "      wait: {},", js(wait(m.wait)));
            let _ = writeln!(
                body,
                "      resource: {},",
                m.resource.map_or("null".to_owned(), |r| js(resource(r)))
            );
            let _ = writeln!(body, "      params: [{}],", params.join(", "));
            let _ = writeln!(
                body,
                "      result: {},",
                js_value(&m.result, &types, &mut used)
            );
            body.push_str("    },\n");
        }
        body.push_str("  },\n");
    }
    body.push_str("};\n");
    let names: Vec<&str> = used.into_iter().collect();
    let one_line = format!("import {{ {} }} from \"../wire.mjs\";", names.join(", "));
    let import = if one_line.len() <= 100 {
        one_line
    } else {
        format!(
            "import {{\n{}}} from \"../wire.mjs\";",
            names.iter().fold(String::new(), |mut s, n| {
                let _ = writeln!(s, "  {n},");
                s
            })
        )
    };
    format!(
        "// The host ABI's JS codecs and method descriptors, generated from the\n\
         // ABI model (host/gen/abi.json) by crates/hd_host_abi/tests/abi_model.rs.\n\
         // Do not edit: `HD_UPDATE_ABI=1 cargo test -p hd_host_abi --test\n\
         // abi_model` rewrites it. host/wire.mjs says how each value crosses.\n\
         {import}\n\n{body}"
    )
}

/// The indent of a declaration's body: one more step in a generic one's
/// arrow function.
fn indent(generics: &[String]) -> &'static str {
    if generics.is_empty() { "" } else { "  " }
}

/// A generic declaration as a function of its type parameters' codecs.
fn generic(generics: &[String], def: &str) -> String {
    if generics.is_empty() {
        def.to_owned()
    } else {
        format!("({}) =>\n  {def}", generics.join(", "))
    }
}

/// Checks `path` against `want`, or rewrites it under `HD_UPDATE_ABI`.
fn check(rel: &str, want: &str) {
    let path = root().join(rel);
    if std::env::var_os("HD_UPDATE_ABI").is_some() {
        std::fs::create_dir_all(path.parent().expect("dir")).expect("host/gen");
        std::fs::write(&path, want).expect("write a generated file");
        return;
    }
    let have = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        have == want,
        "{rel} is stale: rerun with HD_UPDATE_ABI=1 and check in the result"
    );
}

#[test]
fn the_checked_in_abi_model_matches_the_tables_and_std() {
    check("host/gen/abi.json", &model());
}

#[test]
fn the_checked_in_js_glue_matches_the_model() {
    check("host/gen/abi.mjs", &glue());
}
