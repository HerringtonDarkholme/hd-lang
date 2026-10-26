//! The `wat` view (#662): the Wasm instructions whose origin lies inside
//! the subject (`Packages::instruction_origins`), per function, printed
//! so the excerpt reads by eye and stays put when unrelated code changes.
//!
//! - Functions print by name: the case's own without `app/main/`, an
//!   instance's body without its `#0`, a runtime helper without the
//!   module-order `#k` that tells helpers of one name apart. A generic
//!   instance adds its type arguments (`identity[i32]`), and a std
//!   function prints by its own name (`println`), or by its path when
//!   another function in the view has that name.
//! - Locals print by name: a parameter or a local by its source name
//!   (`b.x` for a struct field of a local of several values, `f.K` for
//!   the `K`th value of another), a parameter with a role by the role
//!   (`ctx`, `frame`, `env`, ...), and any other local as `t0`, `t1`, ...
//!   in the order the excerpt first names it.
//! - The releases at a function's exit come from where each place began:
//!   a local's binding statement, a temporary's instruction. The rest of
//!   the exit (the closing `end`, the result) comes from the whole body.
//! - A global prints by its binding's path or the runtime's name for it,
//!   static data by its address with a comment saying what it holds,
//!   and a type by its WAT signature.
//! - Memories are `buf`, `heap` and `stack`. Labels go; branch depths stay.
//! - Two different things that would print alike take `'1`, `'2`, ... in
//!   the order the view first names them.
//! - The excerpt is a WAT module with only referenced imports. Source
//!   lines and omitted code are comments; control delimiters remain so
//!   a filtered body parses without pretending it validates.

use hd_base::SliceSort as _;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use hd_driver::{
    BuildProfile, Executor, FunctionOrigins, Goal, Host, NoClock, OptLevel, Packages, PositionPool,
    StaticData, build_packages,
};
use hd_sched::SerialOrder;

use super::{Build, Program};

/// The `wat` view of `p`: every function the case defines, emitted
/// without a `main` (`Goal::Fixture`), with each one's instructions from
/// inside the subject after its name; then the build's diagnostics.
#[must_use]
pub fn wat(p: &Program) -> String {
    wat_for(p, Build::default())
}

/// [`wat`], for the target and level `at` names.
#[must_use]
pub fn wat_for(p: &Program, at: Build) -> String {
    let src = p.sources();
    let store = hd_cache::MemoryStore::default();
    let host = Host {
        render_tir: &["app/*"],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let packages = Packages {
        instruction_origins: true,
        target: at.target,
        level: if at.release {
            OptLevel::Optimized
        } else {
            OptLevel::Dev
        },
        build_profile: if at.release {
            BuildProfile::Release
        } else {
            BuildProfile::Checked
        },
        ..Packages::default()
    };
    let built = build_packages(
        &host,
        "app",
        &packages,
        &Goal::Fixture {
            module: "main".into(),
        },
    );
    let mut out = String::from("(module\n");
    if let Some(wasm) = &built.wasm {
        let module = Module::read(wasm);
        let mut storage = String::new();
        let lines: BTreeMap<u64, String> = wasmprinter::Config::new()
            .offsets_and_lines(&strip_custom(wasm), &mut storage)
            .expect("the module prints")
            .filter_map(|(at, line)| Some((at?, line.trim().to_owned())))
            .collect();
        let mut pool = PositionPool::default();
        let positions = built.positions(&mut pool);
        // Each function's excerpt, as tokens, before anything is named.
        let mut excerpts: Vec<(&FunctionOrigins, Instructions)> = Vec::new();
        for f in &built.instruction_origins {
            let Some(item) = f.item.as_deref().filter(|i| i.starts_with("app/")) else {
                continue;
            };
            let start = module.bodies[&f.index];
            let instructions: Vec<_> = f
                .instructions
                .iter()
                .map(|ins| {
                    let source = ins
                        .origin
                        .and_then(|o| positions.span(item, o))
                        .filter(|(file, _, _)| *file == "main.hd")
                        .map(|(_, lo, hi)| (lo as usize, hi as usize));
                    let at = start + u64::from(ins.offset);
                    (
                        source,
                        tokens(lines.get(&at).map_or("?", String::as_str), f, &module),
                    )
                })
                .collect();
            let kept = focused(p, instructions);
            if !kept.is_empty() {
                excerpts.push((f, kept));
            }
        }
        let by_index: BTreeMap<u32, &FunctionOrigins> = built
            .instruction_origins
            .iter()
            .map(|f| (f.index, f))
            .collect();
        let func_name = |k: u32| {
            let name = by_index
                .get(&k)
                .map_or_else(|| module.name(k), |f| f.name.clone());
            let args = by_index.get(&k).map_or(&[][..], |f| &f.type_args[..]);
            function_names(&name, args)
        };
        let mut funcs = Names::default();
        for (f, kept) in &excerpts {
            funcs.see_as(f.index, func_name(f.index));
            for t in kept.iter().flat_map(|(_, ts)| ts) {
                if let Tok::Func(k) = t {
                    funcs.see_as(*k, func_name(*k));
                }
            }
        }
        let defined: BTreeSet<_> = excerpts.iter().map(|(f, _)| f.index).collect();
        let mut globals = Names::default();
        let mut memories = BTreeSet::new();
        let mut tables = BTreeSet::new();
        for (f, kept) in &excerpts {
            for t in kept.iter().flat_map(|(_, ts)| ts) {
                match t {
                    Tok::Global(g) => {
                        let name = f
                            .globals
                            .iter()
                            .find(|(k, _)| k == g)
                            .map_or_else(|| format!("global{g}"), |(_, n)| short_name(n));
                        globals.see(*g, name);
                    }
                    Tok::Memory(m) => {
                        memories.insert(*m);
                    }
                    Tok::Table(t) => {
                        tables.insert(*t);
                    }
                    _ => {}
                }
            }
        }
        for (k, _) in &funcs.seen {
            if !defined.contains(k) {
                let name = funcs.name(*k);
                let _ = writeln!(
                    out,
                    "  (import \"fixture\" {name:?} (func {}{}))",
                    id(&name),
                    module.signature(module.function_types[*k as usize])
                );
            }
        }
        for (k, _) in &globals.seen {
            let name = globals.name(*k);
            let ty = &module.globals[*k as usize];
            let ty = if ty.mutable {
                format!("(mut {})", ty.content_type)
            } else {
                ty.content_type.to_string()
            };
            let _ = writeln!(
                out,
                "  (import \"fixture\" {name:?} (global {} {ty}))",
                id(&name)
            );
        }
        for k in memories {
            let ty = &module.memories[k as usize];
            let max = ty.maximum.map_or(String::new(), |n| format!(" {n}"));
            let width = if ty.memory64 { " i64" } else { "" };
            let shared = if ty.shared { " shared" } else { "" };
            let name = memory_name(k);
            let _ = writeln!(
                out,
                "  (import \"fixture\" {name:?} (memory ${name}{width} {}{max}{shared}))",
                ty.initial
            );
        }
        for k in tables {
            let ty = &module.tables[k as usize];
            let _ = writeln!(
                out,
                "  (import \"fixture\" \"table{k}\" (table $table{k} {} {}))",
                ty.initial, ty.element_type
            );
        }
        for (f, kept) in &excerpts {
            let tir = f
                .item
                .as_deref()
                .and_then(|i| built.tir_text.get(i))
                .map_or("", String::as_str);
            let mut locals = Locals::new(f, tir);
            for t in kept.iter().flat_map(|(_, ts)| ts) {
                if let Tok::Local(l) = t {
                    locals.see(*l);
                }
            }
            let ty = &module.types[module.function_types[f.index as usize] as usize];
            let _ = write!(out, "  (func {}", id(&funcs.name(f.index)));
            for (i, t) in ty.params().iter().enumerate() {
                let i = u32::try_from(i).expect("local index");
                locals.see(i);
                let _ = write!(out, " (param {} {t})", id(&locals.names.name(i)));
            }
            results(&mut out, ty.results());
            let mut declared: Vec<_> = locals
                .names
                .seen
                .iter()
                .map(|(k, _)| *k)
                .filter(|k| (*k as usize) >= ty.params().len())
                .collect();
            declared.sort_stable();
            for l in declared {
                let _ = write!(
                    out,
                    " (local {} {})",
                    id(&locals.names.name(l)),
                    module.locals[&f.index][l as usize]
                );
            }
            out.push('\n');
            let mut depth = 0usize;
            let mut source = None;
            for (lo, toks) in kept {
                if let Some(lo) = lo {
                    let line = p.text[..*lo].bytes().filter(|b| *b == b'\n').count();
                    if source != Some(line) {
                        let text = p.text.lines().nth(line).unwrap_or_default();
                        let _ = writeln!(out, "    ;; {} | {text}", line + 1);
                        source = Some(line);
                    }
                }
                let first = toks.first();
                let closes = matches!(first, Some(Tok::Text(w)) if w == "end" || w == "else");
                let opens = matches!(first, Some(Tok::Text(w)) if w == "block" || w == "loop" || w == "if" || w == "else");
                if closes {
                    depth = depth.saturating_sub(1);
                }
                let words: Vec<String> = toks
                    .iter()
                    .map(|t| match t {
                        Tok::Text(w) => w.clone(),
                        Tok::Func(k) => id(&funcs.name(*k)),
                        Tok::Local(l) => id(&locals.names.name(*l)),
                        Tok::Global(g) => id(&globals.name(*g)),
                        Tok::Memory(m) => format!("${}", memory_name(*m)),
                        Tok::Table(t) => format!("$table{t}"),
                    })
                    .collect();
                let _ = writeln!(out, "    {}{}", "  ".repeat(depth), words.join(" "));
                if opens {
                    depth += 1;
                }
            }
            out.push_str("  )\n");
        }
        if excerpts.is_empty() {
            out.push_str("  ;; no code from the subject\n");
            for note in &built.fixture_notes {
                let _ = writeln!(out, "  ;; {}", note.replace("app.main.", ""));
            }
        }
    }
    for line in built.render().lines() {
        let _ = writeln!(out, "  ;; {line}");
    }
    out.push_str(")\n");
    out
}

type Instructions = Vec<(Option<usize>, Vec<Tok>)>;
type SourceInstructions = Vec<(Option<(usize, usize)>, Vec<Tok>)>;

/// Keep the subject's instructions, plus only the control delimiters
/// needed to parse the excerpt. Omitted runs are comments, not code.
fn focused(p: &Program, code: SourceInstructions) -> Instructions {
    let mut keep: Vec<bool> = code
        .iter()
        .map(|(span, _)| span.is_some_and(|(lo, hi)| p.inside(lo, hi)))
        .collect();
    let subject = keep.clone();
    let mut stack = Vec::new();
    let mut function_end = None;
    for (i, (_, ts)) in code.iter().enumerate() {
        let op = ts.first().and_then(|t| {
            if let Tok::Text(w) = t {
                Some(w.as_str())
            } else {
                None
            }
        });
        match op {
            Some("block" | "loop" | "if") => stack.push((i, None)),
            Some("else") => {
                stack.last_mut().expect("an if").1 = Some(i);
            }
            Some("end") => {
                if let Some((start, otherwise)) = stack.pop() {
                    if keep[start..=i].iter().any(|k| *k) {
                        keep[start] = true;
                        keep[i] = true;
                        if let Some(otherwise) = otherwise {
                            keep[otherwise] = true;
                        }
                    }
                } else {
                    // The implicit function end becomes the func's `)`.
                    keep[i] = false;
                    function_end = Some(i);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    let mut omitted = false;
    for (i, ((span, ts), keep)) in code.into_iter().zip(keep).enumerate() {
        if function_end == Some(i) {
            continue;
        }
        if keep {
            out.push((span.filter(|_| subject[i]).map(|(lo, _)| lo), ts));
            omitted = false;
        } else if !omitted {
            out.push((None, vec![Tok::Text(";; ...".into())]));
            omitted = true;
        }
    }
    if out
        .iter()
        .all(|(_, ts)| matches!(ts.first(), Some(Tok::Text(w)) if w == ";; ..."))
    {
        out.clear();
    }
    out
}

/// WAT supports quoted identifiers for source names containing spaces,
/// brackets or Unicode; ordinary identifier characters stay unquoted.
fn id(name: &str) -> String {
    if !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-./:<=>?@\\^_`|~".contains(&b))
    {
        format!("${name}")
    } else {
        format!("${name:?}")
    }
}

fn results(out: &mut String, types: &[wasmparser::ValType]) {
    if !types.is_empty() {
        out.push_str(" (result");
        for ty in types {
            let _ = write!(out, " {ty}");
        }
        out.push(')');
    }
}

/// One word of a printed instruction: text, or a function or local that
/// is named once the whole view is known.
enum Tok {
    Text(String),
    Func(u32),
    Local(u32),
    Global(u32),
    Memory(u32),
    Table(u32),
}

/// The words of `line`, one instruction as the printer wrote it with
/// indices, in function `f`: labels dropped, and globals, static data,
/// memories and types named.
fn tokens(line: &str, f: &FunctionOrigins, module: &Module) -> Vec<Tok> {
    // The printer's own comments: `;; label = @1` and `(;@1;)`.
    let mut text = line.split(";;").next().unwrap_or_default().to_owned();
    while let (Some(a), Some(b)) = (text.find("(;"), text.find(";)")) {
        if b < a {
            break;
        }
        text.replace_range(a..b + 2, "");
    }
    if text.trim() == ")" {
        // The body's final `end`.
        "end".clone_into(&mut text);
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    let Some((&op, rest)) = words.split_first() else {
        return Vec::new();
    };
    let num = |w: &str| w.parse::<u32>().ok();
    let mut out = vec![Tok::Text(op.to_owned())];
    let memory = op.contains(".load") || op.contains(".store");
    match (op, rest) {
        ("local.get" | "local.set" | "local.tee", [l]) if num(l).is_some() => {
            out.push(Tok::Local(num(l).unwrap_or_default()));
            return out;
        }
        ("global.get" | "global.set", [g]) if num(g).is_some() => {
            let g = num(g).unwrap_or_default();
            out.push(Tok::Global(g));
            return out;
        }
        ("call" | "return_call" | "ref.func", [k]) if num(k).is_some() => {
            out.push(Tok::Func(num(k).unwrap_or_default()));
            return out;
        }
        ("i32.const", [c]) => {
            let at = c.parse::<i32>().ok().map(i32::cast_unsigned);
            if let Some((_, data)) = f.statics.iter().find(|(a, _)| Some(*a) == at) {
                out.push(Tok::Text((*c).to_owned()));
                out.push(Tok::Text(format!(";; &{}", static_label(data, module))));
                return out;
            }
        }
        _ => {}
    }
    let mut rest = rest.iter().peekable();
    if memory {
        let m = rest.peek().and_then(|w| num(w)).map_or(0, |m| {
            rest.next();
            m
        });
        out.push(Tok::Memory(m));
    } else if op.starts_with("memory.") {
        let count = if op == "memory.copy" { 2 } else { 1 };
        for _ in 0..count {
            let m = rest.peek().and_then(|w| num(w)).map_or(0, |m| {
                rest.next();
                m
            });
            out.push(Tok::Memory(m));
        }
    } else if op == "call_indirect" || op == "return_call_indirect" {
        let t = rest.peek().and_then(|w| num(w)).map_or(0, |t| {
            rest.next();
            t
        });
        out.push(Tok::Table(t));
    }
    let rest: Vec<&str> = rest.copied().collect();
    let mut k = 0;
    while k < rest.len() {
        // `(type N)`: the signature it names.
        if rest[k] == "(type"
            && let Some(t) = rest
                .get(k + 1)
                .and_then(|w| w.strip_suffix(')'))
                .and_then(num)
        {
            out.push(Tok::Text(module.signature(t).trim().to_owned()));
            k += 2;
            continue;
        }
        out.push(Tok::Text(rest[k].to_owned()));
        k += 1;
    }
    out
}

fn memory_name(m: u32) -> &'static str {
    match m {
        0 => "buf",
        1 => "heap",
        2 => "stack",
        _ => "memory?",
    }
}

/// What static data holds, as the view names it.
fn static_label(data: &StaticData, module: &Module) -> String {
    match data {
        StaticData::Bytes(b) => match std::str::from_utf8(b) {
            Ok(s) if !s.contains('\0') => format!("{s:?}"),
            _ => format!("bytes[{}]", b.len()),
        },
        StaticData::Named(n) => short_name(n),
        StaticData::VTable(slots) => {
            let names: Vec<String> = slots.iter().map(|k| short_name(&module.name(*k))).collect();
            format!("vtable[{}]", names.join(", "))
        }
    }
}

/// A function's name without what changes with unrelated code or only
/// repeats the obvious: the case's own module path, an instance body's
/// `#0`, a helper's module-order `#k`.
fn short_name(name: &str) -> String {
    let name = name.strip_prefix("app/main/").unwrap_or(name);
    // A suspending body's part: `f#0.poll`.
    if let Some((base, part)) = name.split_once("#0.") {
        return format!("{base}.{part}");
    }
    if let Some((base, k)) = name.rsplit_once('#')
        && (k == "0" || (name.starts_with("rt:") && k.chars().all(|c| c.is_ascii_digit())))
    {
        return base.to_owned();
    }
    name.to_owned()
}

/// A function's names for the view, shortest first: a std function by
/// its own name (`println`, `List.push`), then by its path
/// (`std/console/println`); either with an instance's type arguments
/// (`identity[i32]`).
fn function_names(name: &str, args: &[String]) -> Vec<String> {
    let args = if args.is_empty() {
        String::new()
    } else {
        let args: Vec<String> = args.iter().map(|a| type_name(a)).collect();
        format!("[{}]", args.join(", "))
    };
    let full = format!("{}{args}", short_name(name));
    match full.strip_prefix("std/") {
        Some(rest) => {
            let own = rest.rsplit_once('/').map_or(rest, |(_, own)| own);
            vec![own.to_owned(), full.clone()]
        }
        None => vec![full],
    }
}

/// A type argument's printed name without the case's own module path:
/// `List[Body]`, not `List[main.Body]`.
fn type_name(printed: &str) -> String {
    let mut out = String::new();
    let mut path = String::new();
    let flush = |path: &mut String, out: &mut String| {
        let own = ["app.main.", "main.", "app."]
            .iter()
            .find_map(|m| path.strip_prefix(m))
            .unwrap_or(path);
        out.push_str(own);
        path.clear();
    };
    for c in printed.chars() {
        if c.is_alphanumeric() || c == '_' || c == '.' {
            path.push(c);
        } else {
            flush(&mut path, &mut out);
            out.push(c);
        }
    }
    flush(&mut path, &mut out);
    out
}

/// Names for things the view shows. Each thing has its texts, shortest
/// first, and prints by the first that no other thing in the view shares
/// at the same rank; things alike in every text take `'1`, `'2`, ... in
/// the order first seen.
#[derive(Default)]
struct Names {
    /// Each thing's texts, in the order first seen.
    seen: Vec<(u32, Vec<String>)>,
}

impl Names {
    fn see(&mut self, k: u32, text: String) {
        self.see_as(k, vec![text]);
    }

    fn see_as(&mut self, k: u32, texts: Vec<String>) {
        if !self.seen.iter().any(|(j, _)| *j == k) {
            self.seen.push((k, texts));
        }
    }

    fn name(&self, k: u32) -> String {
        let Some((_, texts)) = self.seen.iter().find(|(j, _)| *j == k) else {
            return format!("#{k}");
        };
        let at_rank = |ts: &[String], r: usize| ts.get(r).or(ts.last()).cloned();
        let ranks = self.seen.iter().map(|(_, ts)| ts.len()).max().unwrap_or(1);
        for r in 0..ranks {
            let text = at_rank(texts, r);
            let alike = self
                .seen
                .iter()
                .filter(|(_, ts)| at_rank(ts, r) == text)
                .count();
            if alike == 1 {
                return text.unwrap_or_default();
            }
        }
        let text = texts.last().cloned().unwrap_or_default();
        let alike: Vec<u32> = self
            .seen
            .iter()
            .filter(|(_, ts)| ts.last() == Some(&text))
            .map(|(j, _)| *j)
            .collect();
        let at = alike.iter().position(|j| *j == k).unwrap_or_default();
        format!("{text}'{}", at + 1)
    }
}

/// One function's local names.
struct Locals<'a> {
    f: &'a FunctionOrigins,
    /// Each TIR local's source name, by number, from the body's TIR text.
    tir: BTreeMap<String, String>,
    names: Names,
    temps: usize,
}

impl<'a> Locals<'a> {
    fn new(f: &'a FunctionOrigins, tir: &str) -> Self {
        let tir = tir
            .lines()
            .filter_map(|l| l.trim().strip_prefix("local $"))
            .filter_map(|l| {
                let (k, rest) = l.split_once(' ')?;
                let name = rest.split(':').next()?.trim();
                (!name.is_empty() && !name.starts_with('$') && !name.starts_with('_'))
                    .then(|| (k.to_owned(), name.to_owned()))
            })
            .collect();
        Locals {
            f,
            tir,
            names: Names::default(),
            temps: 0,
        }
    }

    fn see(&mut self, l: u32) {
        if self.names.seen.iter().any(|(j, _)| *j == l) {
            return;
        }
        let known = self.f.locals.get(l as usize).cloned().flatten();
        let text = known.and_then(|n| match n.strip_prefix('$') {
            Some(tir) => {
                let (k, part) = tir
                    .split_once('.')
                    .map_or((tir, None), |(k, p)| (k, Some(p)));
                let name = self.tir.get(k)?;
                Some(part.map_or_else(|| name.clone(), |p| format!("{name}.{p}")))
            }
            None => Some(n),
        });
        let text = text.unwrap_or_else(|| {
            self.temps += 1;
            format!("t{}", self.temps - 1)
        });
        self.names.see(l, text);
    }
}

/// What the view reads from the module itself.
struct Module {
    /// Each defined function's body start, by function index.
    bodies: BTreeMap<u32, u64>,
    /// Each function's name, from the `name` section, by index.
    names: BTreeMap<u32, String>,
    /// Each function type's signature, by type index.
    types: Vec<wasmparser::FuncType>,
    function_types: Vec<u32>,
    locals: BTreeMap<u32, Vec<wasmparser::ValType>>,
    globals: Vec<wasmparser::GlobalType>,
    memories: Vec<wasmparser::MemoryType>,
    tables: Vec<wasmparser::TableType>,
}

impl Module {
    fn read(wasm: &[u8]) -> Module {
        let mut module = Module {
            bodies: BTreeMap::new(),
            names: BTreeMap::new(),
            types: Vec::new(),
            function_types: Vec::new(),
            locals: BTreeMap::new(),
            globals: Vec::new(),
            memories: Vec::new(),
            tables: Vec::new(),
        };
        let mut imported = 0u32;
        let mut defined = 0u32;
        for payload in wasmparser::Parser::new(0).parse_all(wasm) {
            match payload.expect("the module parses") {
                wasmparser::Payload::TypeSection(types) => {
                    for ty in types.into_iter_err_on_gc_types() {
                        let ty = ty.expect("a function type");
                        module.types.push(ty);
                    }
                }
                wasmparser::Payload::ImportSection(imports) => {
                    for import in imports.into_imports() {
                        let import = import.expect("an import");
                        match import.ty {
                            wasmparser::TypeRef::Func(t) => {
                                module
                                    .names
                                    .insert(imported, format!("{}/{}", import.module, import.name));
                                module.function_types.push(t);
                                imported += 1;
                            }
                            wasmparser::TypeRef::Global(t) => module.globals.push(t),
                            wasmparser::TypeRef::Memory(t) => module.memories.push(t),
                            wasmparser::TypeRef::Table(t) => module.tables.push(t),
                            _ => {}
                        }
                    }
                }
                wasmparser::Payload::FunctionSection(functions) => {
                    module
                        .function_types
                        .extend(functions.into_iter().map(|t| t.expect("function type")));
                }
                wasmparser::Payload::GlobalSection(globals) => {
                    module
                        .globals
                        .extend(globals.into_iter().map(|g| g.expect("global").ty));
                }
                wasmparser::Payload::MemorySection(memories) => {
                    module
                        .memories
                        .extend(memories.into_iter().map(|m| m.expect("memory")));
                }
                wasmparser::Payload::TableSection(tables) => {
                    module
                        .tables
                        .extend(tables.into_iter().map(|t| t.expect("table").ty));
                }
                wasmparser::Payload::CodeSectionEntry(body) => {
                    let index = imported + defined;
                    module.bodies.insert(index, body.range().start);
                    let mut locals = module.types[module.function_types[index as usize] as usize]
                        .params()
                        .to_vec();
                    for local in body.get_locals_reader().expect("locals") {
                        let (n, ty) = local.expect("local type");
                        locals.extend(std::iter::repeat_n(ty, n as usize));
                    }
                    module.locals.insert(index, locals);
                    defined += 1;
                }
                wasmparser::Payload::CustomSection(c) => {
                    if let wasmparser::KnownCustom::Name(names) = c.as_known() {
                        for name in names {
                            if let Ok(wasmparser::Name::Function(map)) = name {
                                for n in map.into_iter().flatten() {
                                    module.names.insert(n.index, n.name.to_owned());
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        module
    }

    fn name(&self, k: u32) -> String {
        self.names
            .get(&k)
            .cloned()
            .unwrap_or_else(|| format!("func{k}"))
    }

    fn signature(&self, t: u32) -> String {
        let ty = &self.types[t as usize];
        let mut out = String::new();
        if !ty.params().is_empty() {
            out.push_str(" (param");
            for p in ty.params() {
                let _ = write!(out, " {p}");
            }
            out.push(')');
        }
        results(&mut out, ty.results());
        out
    }
}

/// `wasm` without its custom sections, which follow the code, so the
/// printer writes every function, local and type by its index.
fn strip_custom(wasm: &[u8]) -> Vec<u8> {
    let mut out = wasm[..8].to_vec();
    let mut at = 8;
    while at < wasm.len() {
        let (size, n) = leb(&wasm[at + 1..]);
        let end = at + 1 + n + size;
        if wasm[at] != 0 {
            out.extend_from_slice(&wasm[at..end]);
        }
        at = end;
    }
    out
}

/// An unsigned LEB128 value and its length in bytes.
fn leb(bytes: &[u8]) -> (usize, usize) {
    let (mut v, mut shift) = (0usize, 0);
    for (k, b) in bytes.iter().enumerate() {
        v |= usize::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            return (v, k + 1);
        }
        shift += 7;
    }
    (v, bytes.len())
}

#[cfg(test)]
mod tests {
    use super::{Names, function_names, short_name};

    #[test]
    fn names_drop_what_moves_with_unrelated_code() {
        assert_eq!(short_name("app/main/step#0"), "step");
        assert_eq!(short_name("app/main/adder#1"), "adder#1");
        assert_eq!(short_name("app/main/wait#0.poll"), "wait.poll");
        assert_eq!(short_name("rt:release_ref#3"), "rt:release_ref");
        assert_eq!(short_name("std/rt.alloc/retain#0"), "std/rt.alloc/retain");
    }

    #[test]
    fn std_functions_print_by_their_own_name_unless_two_share_it() {
        let mut names = Names::default();
        names.see_as(1, function_names("std/console/println#0", &[]));
        names.see_as(
            2,
            function_names("app/main/identity#0", &["main.Body".to_owned()]),
        );
        names.see_as(
            3,
            function_names("std/list/len#0", &["List[app.main.Body]".to_owned()]),
        );
        assert_eq!(names.name(1), "println");
        assert_eq!(names.name(2), "identity[Body]");
        assert_eq!(names.name(3), "len[List[Body]]");
        names.see_as(4, function_names("app/main/println#0", &[]));
        assert_eq!(names.name(1), "std/console/println");
        assert_eq!(names.name(4), "println");
    }

    #[test]
    fn alike_names_number_in_the_order_first_seen() {
        let mut names = Names::default();
        names.see(9, "f".to_owned());
        names.see(4, "g".to_owned());
        names.see(2, "f".to_owned());
        names.see(9, "f".to_owned());
        assert_eq!(names.name(9), "f'1");
        assert_eq!(names.name(2), "f'2");
        assert_eq!(names.name(4), "g");
    }
}
