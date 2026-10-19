//! `hd fix --object-model` (`cli.fix.object-model.*`): the conservative
//! rewrite of tier 1's migration (docs/future-work/OBJECT_MODEL_TIER1.md
//! §15). Every `data` type becomes a `struct` that the program reaches
//! through references, and every list or map that is shared today becomes a
//! reference to one, so the program keeps its meaning.
//!
//! The rewrite has two parts. The first is planned from the syntax, with
//! names resolved in the resolver's module scopes: the declaration keyword,
//! each written type that names a former data type (`D` to `&D`, `mut D`
//! to `&mut D`), the shared list and map positions, the receivers, and the
//! literals. It comes in units, one per data type and one per declaration
//! whose shared lists it rewrites, and a unit's edits stand or fall
//! together. The second part is typed: a check of the rewritten package
//! reports each place where a value meets a reference to it, and each `==`
//! of two references, with a migration edit
//! ([`hd_diag::FixSafety::Migration`]), and rounds apply them until a check
//! offers none.
//!
//! A set of units is kept when the package then checks with no diagnostic
//! that the original lacked. All units are tried together first. When that
//! fails, the units that edit a line where a new diagnostic is are set
//! aside, since the edits add no line; when none does, each unit is tried
//! in turn. The units set aside are then tried again on top of those kept,
//! until a try keeps none. A unit left is reported at its declaration. A
//! second run tries the units left on the text the first one wrote, which
//! the first run's last try already did, so it changes nothing.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use hd_base::HashMap;
use hd_cache::DeferredStore;
use hd_driver::{Goal, Host};
use hd_project::SourceSet as _;
use hd_resolve::Named;
use hd_syntax::{NodeRef, Parse, SyntaxKind as K, TokenKind as T};

use crate::fmt_cmd::line_column;
use crate::report::{Diag, Edit};
use crate::{HD_FAILURE, Wall, check_cmd, disk, executor, fail};

/// The texts of the package's source files, by the path their
/// diagnostics name.
type Texts = BTreeMap<String, String>;

/// A declared item: its module's path and its name.
type Key = (String, String);

/// The most rounds of typed edits that one trial applies.
const ROUNDS: usize = 8;

pub(crate) fn command(root: &Path) -> ExitCode {
    let sources = match disk::sources_of(root) {
        Ok((s, _)) => s,
        Err(e) => return fail(&e),
    };
    // Each source file: its listed path and where it is on disk.
    let mut files: BTreeMap<String, (String, PathBuf)> = BTreeMap::new();
    let mut original = Texts::new();
    for e in sources.list() {
        let shown = sources.display(&e.path);
        let path = sources.disk_path(&e.path);
        if let Ok(text) = std::fs::read_to_string(&path) {
            original.insert(shown.clone(), text);
            files.insert(shown, (e.path, path));
        }
    }
    let check = |now: &Texts| match disk::load_package(root, "main") {
        Ok(mut program) => {
            for (shown, text) in now {
                if original.get(shown) != Some(text) {
                    program.sources.set_text(&files[shown].0, text);
                }
            }
            check_cmd::check_package_all(program)
        }
        Err(e) => vec![Diag::error(None, &e)],
    };
    let before = check(&original);
    let mut report = String::new();
    // `cli.fix.object-model.check`: only a package that checks is
    // rewritten.
    if before.iter().any(Diag::is_error) {
        for d in &before {
            let _ = writeln!(report, "{}", d.text());
        }
        let errors = before.iter().filter(|d| d.is_error()).count();
        let _ = writeln!(
            report,
            "object-model result: FAILED. errors: {errors}; warnings: {}",
            before.len() - errors
        );
        eprint!("{report}");
        return ExitCode::from(HD_FAILURE);
    }
    let names = match resolution(root) {
        Ok(r) => r,
        Err(e) => return fail(&e),
    };
    let plan = plan(&original, &names);
    let out = settle(&original, &plan, &before, check);
    let mut failed = false;
    for (shown, text) in &out.texts {
        if original.get(shown) == Some(text) {
            continue;
        }
        if let Err(e) = crate::fix_cmd::write_file(&files[shown].1, &original[shown], text) {
            let _ = writeln!(report, "error: {shown}: {e}");
            failed = true;
        }
    }
    let at = |u: &Unit| {
        let (line, column) = line_column(&original[&u.file], u.at);
        format!("{}:{line}:{column}", u.file)
    };
    // `cli.fix.object-model.report`: in the order of the files and their
    // declarations.
    let order = |u: usize| (&plan.units[u].file, plan.units[u].at);
    let mut kept = out.kept.clone();
    kept.sort_by_key(|&u| order(u));
    let mut left = out.left.clone();
    left.sort_by_key(|(u, _)| order(*u));
    for &u in &kept {
        let unit = &plan.units[u];
        let _ = writeln!(report, "rewrote: {}: {}", at(unit), unit.label);
    }
    for (u, why) in &left {
        let unit = &plan.units[*u];
        let _ = writeln!(report, "left: {}: {}: {why}", at(unit), unit.label);
    }
    let result = if failed { "FAILED" } else { "ok" };
    let _ = writeln!(
        report,
        "object-model result: {result}. rewritten: {}; left: {}",
        out.kept.len(),
        out.left.len()
    );
    eprint!("{report}");
    if failed {
        ExitCode::from(HD_FAILURE)
    } else {
        ExitCode::SUCCESS
    }
}

// ------------------------------------------------------------- resolution

/// What the planned part reads of the resolver: each source file's module
/// and each module's scope, the names its declarations and `use`s bind.
struct Resolution {
    module_of: BTreeMap<String, String>,
    scopes: HashMap<String, HashMap<String, Named>>,
}

fn resolution(root: &Path) -> Result<Resolution, String> {
    let program = disk::load_package(root, "main")?;
    let mut packages = program.packages();
    packages.doc_scopes = true;
    let store = DeferredStore::new(crate::disk_store());
    let clock = Wall(Instant::now());
    let host = Host {
        render_tir: &[],
        sources: &program.sources,
        store: &store,
        clock: &clock,
        executor: executor(),
    };
    let out = hd_driver::build_packages(&host, &program.package, &packages, &Goal::CheckTests);
    let module_of = out
        .files
        .iter()
        .zip(&out.modules)
        .map(|(f, m)| (program.sources.display(f), m.clone()))
        .collect();
    let scopes = out
        .scopes
        .into_iter()
        .map(|(m, names)| (m, names.into_iter().collect()))
        .collect();
    Ok(Resolution { module_of, scopes })
}

// ------------------------------------------------------------------ plan

/// The edits that stand or fall together.
struct Unit {
    /// What the report calls it, as `data Point` or `fn fill`.
    label: String,
    file: String,
    /// The byte its declaration starts at.
    at: usize,
    edits: Vec<Edit>,
    /// Why the rewrite leaves it before any trial, a construct it does not
    /// cover: `embedding` or `local` (`cli.fix.object-model.left`).
    blocked: Option<&'static str>,
}

#[derive(Default)]
struct Plan {
    units: Vec<Unit>,
    /// The unit of each data type the package declares at the top level.
    data: HashMap<Key, usize>,
    /// The traits the package declares.
    traits: BTreeSet<Key>,
    /// The unit of the shared lists of each declaration, by file and node.
    items: HashMap<(String, u32), usize>,
    /// The unit of the shared lists of each trait the package declares,
    /// which its implementations' method headers share.
    trait_units: HashMap<Key, usize>,
}

impl Plan {
    fn unit(&mut self, label: String, file: &str, at: usize) -> usize {
        self.units.push(Unit {
            label,
            file: file.to_owned(),
            at,
            edits: Vec::new(),
            blocked: None,
        });
        self.units.len() - 1
    }
}

/// The planned part of the rewrite, in units (`cli.fix.object-model.data`,
/// `.types`, `.lists`, `.literals`, `.receivers`).
fn plan(texts: &Texts, names: &Resolution) -> Plan {
    let parses: Vec<(&String, &String, Parse)> = texts
        .iter()
        .map(|(file, text)| (file, text, hd_syntax::parse(text.as_bytes())))
        .collect();
    let mut plan = Plan::default();
    // The declarations first, so a use in any file finds them.
    for (file, text, parse) in &parses {
        let Some(module) = names.module_of.get(*file) else {
            continue;
        };
        for item in parse.tree.root().children() {
            let Some(name) = item.name(&parse.tokens) else {
                continue;
            };
            let name = parse.tokens.text(name, text).to_owned();
            let key = (module.clone(), name.clone());
            match item.kind() {
                K::DataDecl if item.direct_token(&parse.tokens, T::KwData).is_some() => {
                    let at = span(parse, item).0;
                    let u = plan.unit(format!("data {name}"), file, at);
                    if item.descendants().any(|n| n.kind() == K::EmbeddedField) {
                        plan.units[u].blocked = Some("embedding");
                    }
                    plan.data.insert(key, u);
                }
                K::TraitDecl => {
                    plan.traits.insert(key);
                }
                _ => {}
            }
        }
    }
    // A data type declared in a body is left.
    for (file, text, parse) in &parses {
        let top: BTreeSet<u32> = parse
            .tree
            .root()
            .children()
            .map(|n| n.index().raw())
            .collect();
        for n in parse.tree.root().descendants() {
            if n.kind() == K::DataDecl
                && n.direct_token(&parse.tokens, T::KwData).is_some()
                && !top.contains(&n.index().raw())
            {
                let name = n
                    .name(&parse.tokens)
                    .map(|t| parse.tokens.text(t, text))
                    .unwrap_or_default();
                let u = plan.unit(format!("data {name}"), file, span(parse, n).0);
                plan.units[u].blocked = Some("local");
            }
        }
    }
    for (file, text, parse) in &parses {
        let Some(module) = names.module_of.get(*file) else {
            continue;
        };
        let mut w = Walker {
            file,
            text,
            parse,
            module,
            scope: names.scopes.get(module.as_str()),
            plan: &mut plan,
        };
        let cx = Cx::default();
        for item in parse.tree.root().children() {
            w.item(item, &cx);
        }
    }
    plan
}

/// The byte range of a node.
fn span(parse: &Parse, n: NodeRef<'_>) -> (usize, usize) {
    let (lo, hi) = (n.first_token(), n.last_token());
    if lo.get().is_none() || hi.get().is_none() {
        return (0, 0);
    }
    (
        parse.tokens.span(lo).0 as usize,
        parse.tokens.span(hi).1 as usize,
    )
}

/// What a walk knows about where it is.
#[derive(Clone, Default)]
struct Cx {
    /// The generic parameters in scope, which hide a type of that name.
    generics: Vec<String>,
    /// The data unit `Self` names as a type: in an inherent
    /// implementation of a data type.
    self_ty: Option<usize>,
    /// The data unit `Self { ... }` builds: in any implementation of one.
    self_lit: Option<usize>,
    /// Types keep their spelling: a method header of an implementation of
    /// a trait the package does not declare, which must match that trait.
    frozen: bool,
    /// The unit a shared list position here belongs to.
    item: Option<usize>,
    /// The body of the function the walk is in.
    body: Option<u32>,
}

struct Walker<'a, 'p> {
    file: &'a str,
    text: &'a str,
    parse: &'a Parse,
    module: &'a str,
    scope: Option<&'a HashMap<String, Named>>,
    plan: &'p mut Plan,
}

/// What a written type names.
enum Names {
    /// A data type the package declares, by its unit.
    Data(usize),
    /// `List` or `Map` of std.
    Collection,
    Other,
}

impl Walker<'_, '_> {
    fn edit(&mut self, unit: usize, start: usize, end: usize, text: &str) {
        self.plan.units[unit].edits.push(Edit {
            file: self.file.to_owned(),
            start: u32::try_from(start).unwrap_or(u32::MAX),
            end: u32::try_from(end).unwrap_or(u32::MAX),
            text: text.to_owned(),
        });
    }

    fn token_text(&self, t: hd_base::TokenIdx) -> &str {
        self.parse.tokens.text(t, self.text)
    }

    /// The identifiers of a type's path, as `shapes.Point`.
    fn type_path(&self, n: NodeRef<'_>) -> Vec<String> {
        n.direct_tokens()
            .filter(|&t| {
                matches!(
                    self.parse.tokens.kind(t),
                    T::Ident | T::RawIdent | T::KwSelfType
                )
            })
            .map(|t| self.token_text(t).to_owned())
            .collect()
    }

    /// The identifiers of an expression path, as `shapes.Point`, or none
    /// for any other expression.
    fn expr_path(&self, n: NodeRef<'_>) -> Option<Vec<String>> {
        match n.kind() {
            K::NameExpr => Some(self.type_path(n)),
            K::FieldExpr => {
                let mut path = self.expr_path(n.children().next()?)?;
                path.push(self.token_text(n.name(&self.parse.tokens)?).to_owned());
                Some(path)
            }
            K::TypeArgsExpr => self.expr_path(n.children().next()?),
            _ => None,
        }
    }

    /// The item a path names in this module's scope.
    fn resolve(&self, path: &[String], cx: &Cx) -> Option<Key> {
        let scope = self.scope?;
        match path {
            [one] => {
                if cx.generics.iter().any(|g| g == one) {
                    return None;
                }
                match scope.get(one)? {
                    Named::Item { module, name } => Some((module.clone(), name.clone())),
                    Named::Module(_) => None,
                }
            }
            [first, middle @ .., last] => match scope.get(first)? {
                Named::Module(m) => {
                    let mut module = m.clone();
                    for part in middle {
                        module.push('.');
                        module.push_str(part);
                    }
                    Some((module, last.clone()))
                }
                Named::Item { .. } => None,
            },
            [] => None,
        }
    }

    fn names(&self, path: &[String], cx: &Cx, self_unit: Option<usize>) -> Names {
        if let [one] = path
            && one == "Self"
        {
            return self_unit.map_or(Names::Other, Names::Data);
        }
        let Some(key) = self.resolve(path, cx) else {
            return Names::Other;
        };
        if let Some(&u) = self.plan.data.get(&key) {
            return Names::Data(u);
        }
        if (key.1 == "List" || key.1 == "Map") && key.0.split('.').next() == Some("std") {
            return Names::Collection;
        }
        Names::Other
    }

    /// The unit of a declaration's shared lists, made on first use.
    fn item_unit(&mut self, n: NodeRef<'_>, label: String) -> usize {
        let key = (self.file.to_owned(), n.index().raw());
        if let Some(&u) = self.plan.items.get(&key) {
            return u;
        }
        let at = span(self.parse, n).0;
        let u = self.plan.unit(label, self.file, at);
        self.plan.items.insert(key, u);
        u
    }

    fn trait_unit(&mut self, key: &Key, at: usize) -> usize {
        if let Some(&u) = self.plan.trait_units.get(key) {
            return u;
        }
        let u = self.plan.unit(format!("trait {}", key.1), self.file, at);
        self.plan.trait_units.insert(key.clone(), u);
        u
    }

    fn with_generics(&self, n: NodeRef<'_>, cx: &Cx) -> Cx {
        let mut cx = cx.clone();
        if let Some(list) = n.child(K::GenericParameterList) {
            for p in list.children() {
                if let Some(t) = p.name(&self.parse.tokens) {
                    cx.generics.push(self.token_text(t).to_owned());
                }
            }
        }
        cx
    }

    fn name_of(&self, n: NodeRef<'_>) -> String {
        n.name(&self.parse.tokens)
            .map(|t| self.token_text(t).to_owned())
            .unwrap_or_default()
    }

    /// A top-level item.
    fn item(&mut self, n: NodeRef<'_>, cx: &Cx) {
        let cx = self.with_generics(n, cx);
        match n.kind() {
            K::DataDecl => {
                let name = self.name_of(n);
                let key = (self.module.to_owned(), name.clone());
                let data = n.direct_token(&self.parse.tokens, T::KwData);
                if let (Some(kw), Some(&u)) = (data, self.plan.data.get(&key)) {
                    let (lo, hi) = self.parse.tokens.span(kw);
                    self.edit(u, lo as usize, hi as usize, "struct");
                    self.fields(
                        n,
                        &Cx {
                            item: Some(u),
                            ..cx
                        },
                        true,
                    );
                } else {
                    let u = self.item_unit(n, format!("struct {name}"));
                    self.fields(
                        n,
                        &Cx {
                            item: Some(u),
                            ..cx
                        },
                        false,
                    );
                }
            }
            K::EnumDecl | K::TypeDecl | K::FnDecl => {
                let word = match n.kind() {
                    K::EnumDecl => "enum",
                    K::TypeDecl => "type",
                    _ => "fn",
                };
                let u = self.item_unit(n, format!("{word} {}", self.name_of(n)));
                self.walk(
                    n,
                    &Cx {
                        item: Some(u),
                        ..cx
                    },
                );
            }
            K::TraitDecl => {
                let name = self.name_of(n);
                let key = (self.module.to_owned(), name);
                let u = self.trait_unit(&key, span(self.parse, n).0);
                self.walk(
                    n,
                    &Cx {
                        item: Some(u),
                        ..cx
                    },
                );
            }
            K::ImplDecl => self.implementation(n, &cx),
            _ => self.walk(n, &cx),
        }
    }

    /// The fields of a data or struct declaration. A data type's readonly
    /// list field shares today (`types.coll.legacy.data-field`).
    fn fields(&mut self, n: NodeRef<'_>, cx: &Cx, data: bool) {
        for c in n.children() {
            if c.kind() != K::Block {
                continue;
            }
            for f in c.children() {
                match f.kind() {
                    K::DataField => {
                        for g in f.children() {
                            if g.kind().is_type() {
                                self.ty(g, cx, data);
                            } else if g.kind() != K::Decorator {
                                self.walk(g, cx);
                            }
                        }
                    }
                    // A data type that another embeds is left with it.
                    K::EmbeddedField => {
                        if let Some(t) = f.children().find(|g| g.kind().is_type())
                            && let Names::Data(u) = self.names(&self.type_path(t), cx, None)
                        {
                            self.plan.units[u].blocked = Some("embedding");
                        }
                    }
                    _ => self.walk(f, cx),
                }
            }
        }
    }

    fn implementation(&mut self, n: NodeRef<'_>, cx: &Cx) {
        // The trait, for `impl Trait for T`, and the target.
        let header: Vec<NodeRef<'_>> = n.children().filter(|c| c.kind().is_type()).collect();
        let (trait_ty, target) = match header.as_slice() {
            [t, target] => (Some(*t), Some(*target)),
            [target] => (None, Some(*target)),
            _ => (None, None),
        };
        let target_unit = target.and_then(|t| match self.names(&self.type_path(t), cx, None) {
            Names::Data(u) => Some(u),
            _ => None,
        });
        // The trait the package declares, whose implementations change with
        // its declaration.
        let own_trait = trait_ty
            .and_then(|t| self.resolve(&self.type_path(t), cx))
            .filter(|k| self.plan.traits.contains(k));
        let mut cx = Cx {
            self_ty: if trait_ty.is_none() {
                target_unit
            } else {
                None
            },
            self_lit: target_unit,
            ..cx.clone()
        };
        if let Some(k) = &own_trait {
            cx.item = Some(self.trait_unit(k, span(self.parse, n).0));
        }
        let frozen = trait_ty.is_some() && own_trait.is_none();
        for c in n.children() {
            if c.kind() != K::Block {
                continue;
            }
            for m in c.children() {
                match m.kind() {
                    K::FnDecl => {
                        let mut mcx = self.with_generics(m, &cx);
                        if own_trait.is_none() {
                            let label = format!("fn {}", self.name_of(m));
                            mcx.item = Some(self.item_unit(m, label));
                        }
                        if let Some(u) = target_unit {
                            self.receiver(m, u, trait_ty.is_none());
                        }
                        self.function(m, &mcx, frozen);
                    }
                    K::AssociatedTypeDecl if frozen => {}
                    _ => self.walk(m, &cx),
                }
            }
        }
    }

    /// A method's receiver on a former data type
    /// (`cli.fix.object-model.receivers`): `mut self` becomes `&mut self`,
    /// and in an inherent implementation `self` becomes `&self`.
    fn receiver(&mut self, m: NodeRef<'_>, unit: usize, inherent: bool) {
        let Some(first) = m.child(K::ParameterList).and_then(|p| p.children().next()) else {
            return;
        };
        let tokens: Vec<_> = first.direct_tokens().collect();
        let kinds: Vec<T> = tokens.iter().map(|&t| self.parse.tokens.kind(t)).collect();
        match kinds.as_slice() {
            [T::KwMut, T::KwSelfValue] => {
                let (lo, hi) = self.parse.tokens.span(tokens[0]);
                self.edit(unit, lo as usize, hi as usize, "&mut");
            }
            [T::KwSelfValue] if inherent => {
                let lo = self.parse.tokens.span(tokens[0]).0 as usize;
                self.edit(unit, lo, lo, "&");
            }
            _ => {}
        }
    }

    /// A function or method: its header's types, unless `frozen`, and its
    /// body.
    fn function(&mut self, n: NodeRef<'_>, cx: &Cx, frozen: bool) {
        let body = n.child(K::Block).map(|b| b.index().raw());
        let cx = Cx {
            frozen,
            body: body.or(cx.body),
            ..cx.clone()
        };
        for c in n.children() {
            match c.kind() {
                K::Block => self.walk(
                    c,
                    &Cx {
                        frozen: false,
                        ..cx.clone()
                    },
                ),
                K::ParameterList => {
                    for p in c.children() {
                        for g in p.children() {
                            if g.kind().is_type() {
                                self.ty(g, &cx, false);
                            } else {
                                self.walk(g, &cx);
                            }
                        }
                    }
                }
                // A frozen header's types stay (`ty` checks `cx.frozen`).
                k if k.is_type() => self.ty(c, &cx, false),
                _ => {}
            }
        }
    }

    /// A written type (`cli.fix.object-model.types`, `.lists`). `field`:
    /// the whole type of a data type's field.
    fn ty(&mut self, n: NodeRef<'_>, cx: &Cx, field: bool) {
        if cx.frozen {
            return;
        }
        match n.kind() {
            K::NamedType => {
                let lo = span(self.parse, n).0;
                match self.names(&self.type_path(n), cx, cx.self_ty) {
                    Names::Data(u) => self.edit(u, lo, lo, "&"),
                    Names::Collection if field => {
                        if let Some(u) = cx.item {
                            self.edit(u, lo, lo, "&");
                        }
                    }
                    _ => {}
                }
                self.type_args(n, cx);
            }
            K::MutType => {
                let Some(inner) = n.children().next() else {
                    return;
                };
                let Some(kw) = n.direct_token(&self.parse.tokens, T::KwMut) else {
                    return;
                };
                let (lo, hi) = self.parse.tokens.span(kw);
                if inner.kind() == K::NamedType {
                    let unit = match self.names(&self.type_path(inner), cx, cx.self_ty) {
                        Names::Data(u) => Some(u),
                        Names::Collection => cx.item,
                        Names::Other => None,
                    };
                    if let Some(u) = unit {
                        self.edit(u, lo as usize, hi as usize, "&mut");
                    }
                    self.type_args(inner, cx);
                } else {
                    self.ty(inner, cx, false);
                }
            }
            K::RefType => {
                for c in n.children() {
                    if c.kind() == K::NamedType {
                        self.type_args(c, cx);
                    } else {
                        self.ty(c, cx, false);
                    }
                }
            }
            K::OptionalType => {
                for c in n.children() {
                    self.ty(c, cx, field);
                }
            }
            K::TupleType | K::ParenType | K::FunctionType => {
                for c in n.children() {
                    if c.kind().is_type() {
                        self.ty(c, cx, false);
                    }
                }
            }
            _ => {}
        }
    }

    fn type_args(&mut self, n: NodeRef<'_>, cx: &Cx) {
        for list in n.children() {
            if list.kind() != K::TypeArgumentList {
                continue;
            }
            for c in list.children() {
                if c.kind().is_type() {
                    self.ty(c, cx, false);
                } else {
                    self.walk(c, cx);
                }
            }
        }
    }

    /// Statements and expressions.
    fn walk(&mut self, n: NodeRef<'_>, cx: &Cx) {
        match n.kind() {
            K::FnDecl => {
                let cx = self.with_generics(n, cx);
                return self.function(n, &cx, false);
            }
            K::ImplDecl => {
                let cx = self.with_generics(n, cx);
                return self.implementation(n, &cx);
            }
            K::ClosureExpr => {
                let body = n.child(K::Block).map(|b| b.index().raw());
                let cx = Cx {
                    body: body.or(cx.body),
                    ..cx.clone()
                };
                for c in n.children() {
                    if c.kind() == K::ParameterList {
                        for p in c.children() {
                            for g in p.children() {
                                if g.kind().is_type() {
                                    self.ty(g, &cx, false);
                                } else {
                                    self.walk(g, &cx);
                                }
                            }
                        }
                    } else if c.kind().is_type() {
                        self.ty(c, &cx, false);
                    } else {
                        self.walk(c, &cx);
                    }
                }
                return;
            }
            // A type declared in a body is left, but the former data types
            // its fields name are rewritten with them.
            K::DataDecl => {
                let cx = Cx {
                    item: None,
                    ..self.with_generics(n, cx)
                };
                return self.fields(n, &cx, false);
            }
            K::LetStmt if self.local_list(n, cx) => return,
            K::DataExpr => self.literal(n, cx),
            // A literal already behind `&` or `&mut` is a reference.
            K::RefExpr => {
                for c in n.children() {
                    if c.kind() == K::DataExpr {
                        self.children(c, cx);
                    } else {
                        self.walk(c, cx);
                    }
                }
                return;
            }
            K::Decorator => return,
            _ => {}
        }
        self.children(n, cx);
    }

    fn children(&mut self, n: NodeRef<'_>, cx: &Cx) {
        for c in n.children() {
            if c.kind().is_type() {
                self.ty(c, cx, false);
            } else if c.kind() == K::TypeArgumentList {
                for t in c.children() {
                    self.ty(t, cx, false);
                }
            } else {
                self.walk(c, cx);
            }
        }
    }

    /// A data literal of a former data type becomes a new object
    /// (`cli.fix.object-model.literals`): `&mut D { ... }`.
    fn literal(&mut self, n: NodeRef<'_>, cx: &Cx) {
        let Some(head) = n.children().next() else {
            return;
        };
        let Some(path) = self.expr_path(head) else {
            return;
        };
        let Names::Data(u) = self.names(&path, cx, cx.self_lit) else {
            return;
        };
        let lo = span(self.parse, n).0;
        self.edit(u, lo, lo, "&mut ");
    }

    /// `let xs: mut List[T] = [...]` becomes `let mut xs: List[T] = [...]`
    /// (`cli.fix.object-model.local-list`) when the list is new and every
    /// later assignment to it is a new list too, so no binding shared it.
    /// Says whether it rewrote the statement; any other `mut List[T]`
    /// local becomes `&mut List[T]` as any position does.
    fn local_list(&mut self, n: NodeRef<'_>, cx: &Cx) -> bool {
        let mut children = n.children();
        let (Some(pat), Some(ty), Some(init)) = (children.next(), children.next(), children.next())
        else {
            return false;
        };
        if pat.kind() != K::BindingPattern || ty.kind() != K::MutType || cx.frozen {
            return false;
        }
        let Some(inner) = ty.children().next() else {
            return false;
        };
        if inner.kind() != K::NamedType
            || !matches!(
                self.names(&self.type_path(inner), cx, None),
                Names::Collection
            )
            || !fresh(init)
        {
            return false;
        }
        let Some(name) = pat.name(&self.parse.tokens) else {
            return false;
        };
        let name_text = self.token_text(name).to_owned();
        if let Some(body) = cx.body {
            let body = self.parse.tree.node(hd_base::NodeIdx::from_raw(body));
            let shared = body.descendants().any(|a| {
                a.kind() == K::AssignmentStmt && {
                    let mut parts = a.children();
                    let target = parts.next();
                    let value = parts.next();
                    target.is_some_and(|t| {
                        t.kind() == K::NameExpr
                            && t.name(&self.parse.tokens)
                                .is_some_and(|x| self.token_text(x) == name_text)
                    }) && !value.is_some_and(fresh)
                }
            });
            if shared {
                return false;
            }
        }
        let Some(u) = cx.item else {
            return false;
        };
        let Some(kw) = ty.direct_token(&self.parse.tokens, T::KwMut) else {
            return false;
        };
        if pat.direct_token(&self.parse.tokens, T::KwMut).is_none() {
            let at = self.parse.tokens.span(name).0 as usize;
            self.edit(u, at, at, "mut ");
        }
        let lo = self.parse.tokens.span(kw).0 as usize;
        let hi = span(self.parse, inner).0;
        self.edit(u, lo, hi, "");
        self.type_args(inner, cx);
        self.walk(init, cx);
        true
    }
}

/// Whether an expression makes a new list or map.
fn fresh(n: NodeRef<'_>) -> bool {
    matches!(n.kind(), K::ListExpr | K::MapExpr | K::ComprehensionExpr)
}

// ---------------------------------------------------------------- trials

/// What the trials kept: the final texts, and the units kept and left,
/// each left one with why.
struct Outcome {
    texts: Texts,
    kept: Vec<usize>,
    left: Vec<(usize, String)>,
}

/// A diagnostic a trial brought: its code and where it is. The planned
/// edits and the typed ones add no line, so its line is the original's.
struct Brought {
    code: Option<&'static str>,
    file: Option<String>,
    line: Option<usize>,
}

impl Brought {
    /// The reason a report gives (`cli.fix.object-model.report`): the
    /// diagnostic's code, or `error` for an error with none.
    fn why(&self) -> String {
        self.code.unwrap_or("error").to_owned()
    }
}

/// The trials of `cli.fix.object-model.safe`.
struct Trials<'a, C> {
    original: &'a Texts,
    plan: &'a Plan,
    before: &'a [Diag],
    check: C,
    /// The lines each unit edits, by file.
    lines: Vec<BTreeSet<(String, usize)>>,
    /// Why each unit left so far was left.
    why: BTreeMap<usize, String>,
}

fn settle(
    original: &Texts,
    plan: &Plan,
    before: &[Diag],
    check: impl FnMut(&Texts) -> Vec<Diag>,
) -> Outcome {
    let mut left: Vec<(usize, String)> = Vec::new();
    let mut candidates = Vec::new();
    for (u, unit) in plan.units.iter().enumerate() {
        if let Some(why) = unit.blocked {
            left.push((u, why.to_owned()));
        } else if !unit.edits.is_empty() {
            candidates.push(u);
        }
    }
    let lines = plan
        .units
        .iter()
        .map(|unit| {
            unit.edits
                .iter()
                .map(|e| {
                    let line = line_column(&original[&e.file], e.start as usize).0;
                    (e.file.clone(), line)
                })
                .collect()
        })
        .collect();
    let mut t = Trials {
        original,
        plan,
        before,
        check,
        lines,
        why: BTreeMap::new(),
    };
    let mut out = Outcome {
        texts: original.clone(),
        kept: Vec::new(),
        left: Vec::new(),
    };
    // Each solve keeps units on top of those kept before, until one keeps
    // none: then every unit left fails on top of the kept ones, as a
    // second run would find (`cli.fix.object-model.idempotent`).
    let mut rest = candidates;
    loop {
        let (more, texts) = t.solve(&out.kept, rest.clone());
        let Some(texts) = texts else {
            break;
        };
        if more.is_empty() {
            break;
        }
        out.texts = texts;
        out.kept.extend(&more);
        rest.retain(|u| !more.contains(u));
        t.why.retain(|u, _| !more.contains(u));
    }
    out.kept.sort_unstable();
    left.extend(t.why);
    left.sort_unstable();
    out.left = left;
    out
}

impl<C: FnMut(&Texts) -> Vec<Diag>> Trials<'_, C> {
    /// The units of `cand` that keep the package checking on top of
    /// `base`, with the texts of that trial. First all of them; when that
    /// fails, the units that edit a line where a new diagnostic is are set
    /// aside and the rest tried again. When no unit edits such a line,
    /// each unit is tried in turn on top of the ones kept.
    fn solve(&mut self, base: &[usize], cand: Vec<usize>) -> (Vec<usize>, Option<Texts>) {
        let mut s = cand;
        while !s.is_empty() {
            let set: Vec<usize> = base.iter().chain(&s).copied().collect();
            let brought = match self.trial(&set) {
                Ok(texts) => return (s, Some(texts)),
                Err(brought) => brought,
            };
            let mut blamed = Vec::new();
            for &u in &s {
                let on_line = brought.iter().find(|b| {
                    let (Some(file), Some(line)) = (&b.file, b.line) else {
                        return false;
                    };
                    self.lines[u].contains(&(file.clone(), line))
                });
                if let Some(b) = on_line {
                    self.why.insert(u, b.why());
                    blamed.push(u);
                }
            }
            if blamed.is_empty() {
                return self.one_by_one(base, &s, &brought);
            }
            s.retain(|u| !blamed.contains(u));
        }
        (Vec::new(), None)
    }

    fn one_by_one(
        &mut self,
        base: &[usize],
        s: &[usize],
        brought: &[Brought],
    ) -> (Vec<usize>, Option<Texts>) {
        let mut kept = Vec::new();
        let mut texts = None;
        for &u in s {
            let set: Vec<usize> = base.iter().chain(&kept).chain([&u]).copied().collect();
            match self.trial(&set) {
                Ok(t) => {
                    kept.push(u);
                    texts = Some(t);
                }
                Err(b) => {
                    let why = b
                        .first()
                        .or(brought.first())
                        .map_or_else(String::new, Brought::why);
                    self.why.insert(u, why);
                }
            }
        }
        (kept, texts)
    }

    /// The units `set` applied, then the typed edits in rounds: the texts,
    /// when they check with no diagnostic that `before` lacks, or else the
    /// diagnostics they bring.
    fn trial(&mut self, set: &[usize]) -> Result<Texts, Vec<Brought>> {
        let planned: Vec<&Edit> = set
            .iter()
            .flat_map(|&u| &self.plan.units[u].edits)
            .collect();
        let mut texts = apply(self.original, &planned);
        let mut diags = (self.check)(&texts);
        for _ in 0..ROUNDS {
            let typed = typed_edits(&diags, &texts);
            if typed.is_empty() {
                break;
            }
            let refs: Vec<&Edit> = typed.iter().flatten().collect();
            texts = apply(&texts, &refs);
            diags = (self.check)(&texts);
        }
        let brought = added(self.before, &diags);
        if brought.is_empty() {
            Ok(texts)
        } else {
            Err(brought)
        }
    }
}

/// The migration edits a check offers, in output order: of each
/// diagnostic its first, when its edits fit the texts and overlap no edit
/// taken before (`cli.fix.object-model.coerce`, `.equality`).
fn typed_edits(diags: &[Diag], texts: &Texts) -> Vec<Vec<Edit>> {
    let mut taken: Vec<Vec<Edit>> = Vec::new();
    for d in diags {
        let Some(f) = d.fixes.iter().find(|f| f.migration) else {
            continue;
        };
        let fits = f.edits.iter().all(|e| {
            texts.get(&e.file).is_some_and(|t| {
                let (s, x) = (e.start as usize, e.end as usize);
                s <= x && x <= t.len() && t.is_char_boundary(s) && t.is_char_boundary(x)
            })
        });
        let overlaps = taken.iter().flatten().any(|a| {
            f.edits.iter().any(|b| {
                a.file == b.file && ((a.start < b.end && b.start < a.end) || a.start == b.start)
            })
        });
        if fits && !overlaps && !f.edits.is_empty() {
            taken.push(f.edits.clone());
        }
    }
    taken
}

/// `texts` with `edits` applied, from the end of each file backwards.
/// Edits at one byte apply in the order given.
fn apply(texts: &Texts, edits: &[&Edit]) -> Texts {
    let mut out = texts.clone();
    let mut order: Vec<(usize, &Edit)> = edits.iter().copied().enumerate().collect();
    order.sort_by(|(i, a), (j, b)| (&b.file, b.start, b.end, j).cmp(&(&a.file, a.start, a.end, i)));
    for (_, e) in order {
        if let Some(text) = out.get_mut(&e.file) {
            let (s, x) = (e.start as usize, e.end as usize);
            if s <= x && x <= text.len() && text.is_char_boundary(s) && text.is_char_boundary(x) {
                text.replace_range(s..x, &e.text);
            }
        }
    }
    out
}

/// The diagnostics of `after` that `before` lacks, by code, severity and
/// file: every error, since the package checked before, and each warning
/// past the count `before` has.
fn added(before: &[Diag], after: &[Diag]) -> Vec<Brought> {
    type Count<'d> = (Option<&'static str>, bool, Option<&'d str>);
    let key = |d: &Diag| (d.code.map(hd_diag::Code::as_str), d.is_error());
    let mut counts: BTreeMap<Count<'_>, isize> = BTreeMap::new();
    for d in before {
        let (code, error) = key(d);
        *counts.entry((code, error, d.file.as_deref())).or_insert(0) += 1;
    }
    let mut out = Vec::new();
    for d in after {
        let (code, error) = key(d);
        let n = counts.entry((code, error, d.file.as_deref())).or_insert(0);
        *n -= 1;
        if *n < 0 || error {
            out.push(Brought {
                code,
                file: d.file.clone(),
                line: d.line,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Plan, Texts, settle};
    use crate::report::{Diag, Edit, Fix};
    use hd_diag::Code;

    fn texts(text: &str) -> Texts {
        Texts::from([("f.hd".to_owned(), text.to_owned())])
    }

    fn insert(at: u32, text: &str) -> Edit {
        Edit {
            file: "f.hd".to_owned(),
            start: at,
            end: at,
            text: text.to_owned(),
        }
    }

    /// A plan of one unit per edit, named after its text.
    fn plan(edits: &[Edit]) -> Plan {
        let mut plan = Plan::default();
        for e in edits {
            let u = plan.unit(e.text.clone(), "f.hd", e.start as usize);
            plan.units[u].edits.push(e.clone());
        }
        plan
    }

    fn error(line: usize) -> Diag {
        Diag::error(Some(Code::TypeMismatch), "m").at("f.hd", Some(line))
    }

    /// `cli.fix.object-model.safe`: units whose rewrite checks are all kept
    /// in one trial.
    #[test]
    fn units_that_check_together_are_kept() {
        let mut checks = 0;
        let out = settle(
            &texts("a\nb\n"),
            &plan(&[insert(0, "X"), insert(2, "Y")]),
            &[],
            |_| {
                checks += 1;
                Vec::new()
            },
        );
        assert_eq!(out.kept, [0, 1]);
        assert!(out.left.is_empty());
        assert_eq!(out.texts["f.hd"], "Xa\nYb\n");
        assert_eq!(checks, 1);
    }

    /// `cli.fix.object-model.safe`, `.left`: a unit that edits the line of
    /// a new error is set aside, and the rest is kept without it.
    #[test]
    fn a_unit_on_the_line_of_a_new_error_is_left() {
        let out = settle(
            &texts("a\nb\n"),
            &plan(&[insert(0, "X"), insert(2, "Y")]),
            &[],
            |t| {
                if t["f.hd"].contains('Y') {
                    vec![error(2)]
                } else {
                    Vec::new()
                }
            },
        );
        assert_eq!(out.kept, [0]);
        assert_eq!(out.left, [(1, "type-mismatch".to_owned())]);
        assert_eq!(out.texts["f.hd"], "Xa\nb\n");
    }

    /// An error on a line no unit edits leaves the units to be tried one at
    /// a time on top of those kept.
    #[test]
    fn an_error_elsewhere_tries_units_one_at_a_time() {
        let out = settle(
            &texts("a\nb\nc\n"),
            &plan(&[insert(0, "X"), insert(2, "Y")]),
            &[],
            |t| {
                if t["f.hd"].contains('Y') {
                    vec![error(3)]
                } else {
                    Vec::new()
                }
            },
        );
        assert_eq!(out.kept, [0]);
        assert_eq!(out.left, [(1, "type-mismatch".to_owned())]);
    }

    /// `cli.fix.object-model.coerce`: a check's migration edits apply in
    /// rounds until it offers none; its other fix-its never apply.
    #[test]
    fn migration_edits_apply_in_rounds() {
        let out = settle(&texts("a\n"), &plan(&[insert(0, "X")]), &[], |t| {
            let text = &t["f.hd"];
            if text.starts_with('&') {
                return Vec::new();
            }
            let mut d = error(1);
            d.fixes.push(Fix {
                message: "suggested".to_owned(),
                edits: vec![insert(0, "?")],
                exact: false,
                migration: false,
            });
            d.fixes.push(Fix {
                message: "write `&`".to_owned(),
                edits: vec![insert(0, "&")],
                exact: false,
                migration: true,
            });
            vec![d]
        });
        assert_eq!(out.kept, [0]);
        assert_eq!(out.texts["f.hd"], "&Xa\n");
    }

    /// A warning the original had is no new diagnostic, but one more of
    /// the same code is.
    #[test]
    fn only_new_warnings_count() {
        let warning = || {
            Diag::new(
                Some(Code::UnusedLocalBinding),
                hd_diag::Severity::Warning,
                "w",
            )
            .at("f.hd", Some(1))
        };
        let before = [warning()];
        let kept = settle(&texts("a\n"), &plan(&[insert(0, "X")]), &before, |_| {
            vec![warning()]
        });
        assert_eq!(kept.kept, [0]);
        let left = settle(&texts("a\n"), &plan(&[insert(0, "X")]), &before, |_| {
            vec![warning(), warning()]
        });
        assert_eq!(left.left, [(0, "unused-local-binding".to_owned())]);
    }
}
