//! The ABI model both hosts' codecs come from (#251, phase 1): every host
//! method of `hd_host_abi`'s tables with the std types its values cross
//! as, and each std data type, enum and newtype those types reach, read
//! from `lib/std` itself. The model is checked in as `host/gen/abi.json`;
//! this test fails when the tables or std change and the file does not.
//! `HD_UPDATE_ABI=1` rewrites it.
//!
//! A type is its source text. `Result[T, E]` (variant 0 `Ok`, 1 `Err`),
//! `T?`, `List[T]`, tuples and the primitives are the language's, and a
//! host handle (`HANDLES`) crosses as its handle number.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use hd_host_abi::{Codec, HANDLES, HostTrait, RUNNERS, ResourceArg, Scalar, TABLE, Wait};
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

/// The model's JSON text.
fn model() -> String {
    let decls = std_decls();
    let handles: Vec<&str> = HANDLES
        .iter()
        .filter_map(|h| h.std_path.rsplit('.').next())
        .collect();
    let groups: [(&str, &[HostTrait]); 3] = [
        ("traits", TABLE),
        ("runners", RUNNERS),
        ("handles", HANDLES),
    ];
    let mut out = String::from("{\n");
    let mut roots = Vec::new();
    for (group, table) in groups {
        let _ = writeln!(out, "  {}: [", js(group));
        for (ti, t) in table.iter().enumerate() {
            let _ = writeln!(
                out,
                "    {{ \"key\": {}, \"std\": {}, \"methods\": [",
                js(t.key),
                js(t.std_path)
            );
            for (mi, m) in t.methods.iter().enumerate() {
                let params: Vec<String> = m.params.iter().map(codec).collect();
                let result = codec(&m.result);
                roots.extend(params.iter().cloned());
                roots.push(result.clone());
                let resource = m.resource.map_or("null".to_owned(), |r| {
                    js(match r {
                        ResourceArg::Path => "path",
                        ResourceArg::Host => "host",
                        ResourceArg::Addr => "addr",
                        ResourceArg::EnvName => "env-name",
                        ResourceArg::Program => "program",
                        ResourceArg::SysName => "sys-name",
                    })
                });
                let params: Vec<String> = params.iter().map(|p| js(p)).collect();
                let _ = write!(
                    out,
                    "      {{ \"name\": {}, \"wait\": {}, \"resource\": {resource}, \"params\": [{}], \"result\": {} }}",
                    js(m.name),
                    js(match m.wait {
                        Wait::Never => "never",
                        Wait::May => "may",
                    }),
                    params.join(", "),
                    js(&result)
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
    // Every std type the methods' types reach, by name.
    let mut types: BTreeMap<String, String> = BTreeMap::new();
    let mut todo: Vec<(String, Vec<String>)> = roots.into_iter().map(|r| (r, Vec::new())).collect();
    while let Some((t, scope)) = todo.pop() {
        for n in names_in(&t) {
            if LANGUAGE.contains(&n) || scope.iter().any(|g| g == n) || types.contains_key(n) {
                continue;
            }
            if handles.contains(&n) {
                types.insert(n.to_owned(), "{ \"kind\": \"handle\" }".into());
                continue;
            }
            let found = decls.get(n).map_or(&[][..], Vec::as_slice);
            let [(module, decl)] = found else {
                panic!("`{n}` (in `{t}`) names {} std declarations", found.len());
            };
            let json = match decl {
                Decl::Data { generics, fields } => {
                    todo.extend(fields.iter().map(|f| (f.1.clone(), generics.clone())));
                    format!(
                        "{{ \"kind\": \"data\", \"module\": {}, \"generics\": [{}], \"fields\": {} }}",
                        js(module),
                        generics
                            .iter()
                            .map(|g| js(g))
                            .collect::<Vec<_>>()
                            .join(", "),
                        fields_json(fields)
                    )
                }
                Decl::Enum { generics, variants } => {
                    let vs: Vec<String> = variants
                        .iter()
                        .map(|(v, fs)| {
                            todo.extend(fs.iter().map(|f| (f.1.clone(), generics.clone())));
                            format!("{{ \"name\": {}, \"fields\": {} }}", js(v), fields_json(fs))
                        })
                        .collect();
                    format!(
                        "{{ \"kind\": \"enum\", \"module\": {}, \"generics\": [{}], \"variants\": [\n      {}\n    ] }}",
                        js(module),
                        generics
                            .iter()
                            .map(|g| js(g))
                            .collect::<Vec<_>>()
                            .join(", "),
                        vs.join(",\n      ")
                    )
                }
                Decl::Newtype(of) => {
                    todo.push((of.clone(), Vec::new()));
                    format!(
                        "{{ \"kind\": \"newtype\", \"module\": {}, \"of\": {} }}",
                        js(module),
                        js(of)
                    )
                }
            };
            types.insert(n.to_owned(), json);
        }
    }
    out.push_str("  \"types\": {\n");
    let rows: Vec<String> = types
        .iter()
        .map(|(n, j)| format!("    {}: {j}", js(n)))
        .collect();
    out.push_str(&rows.join(",\n"));
    out.push_str("\n  }\n}\n");
    out
}

#[test]
fn the_checked_in_abi_model_matches_the_tables_and_std() {
    let path = root().join("host/gen/abi.json");
    let want = model();
    if std::env::var_os("HD_UPDATE_ABI").is_some() {
        std::fs::create_dir_all(path.parent().expect("dir")).expect("host/gen");
        std::fs::write(&path, &want).expect("write the model");
        return;
    }
    let have = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        have == want,
        "host/gen/abi.json is stale: rerun with HD_UPDATE_ABI=1 and check in the result"
    );
}
