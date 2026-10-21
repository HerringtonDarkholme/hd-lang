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
//!   static data by what it holds (`&"text"`, `&empty-buffer`, `&vtable[..]`),
//!   and a type by its signature.
//! - Memory 1, the heap, goes unnamed; the exchange buffer is `buf` and
//!   the shadow stack `stack`. Labels go; branch depths stay.
//! - Two different things that would print alike take `'1`, `'2`, ... in
//!   the order the view first names them.

use std::collections::BTreeMap;
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
    let mut out = super::Dump::new(p);
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
            let kept: Instructions = f
                .instructions
                .iter()
                .filter_map(|ins| {
                    let (file, lo, hi) = ins.origin.and_then(|o| positions.span(item, o))?;
                    if file != "main.hd" || !p.inside(lo as usize, hi as usize) {
                        return None;
                    }
                    let at = start + u64::from(ins.offset);
                    Some((
                        lo as usize,
                        tokens(lines.get(&at).map_or("?", String::as_str), f, &module),
                    ))
                })
                .collect();
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
            out.at(kept
                .iter()
                .map(|(lo, _)| *lo)
                .min()
                .expect("kept instructions"));
            let _ = writeln!(out, "{}", funcs.name(f.index));
            let mut depth = 0usize;
            for (lo, toks) in kept {
                out.at(*lo);
                let first = toks.first();
                let closes = matches!(first, Some(Tok::Text(w)) if w == "end" || w == "else");
                let opens = matches!(first, Some(Tok::Text(w))
                    if w == "block" || w == "loop" || w == "if" || w == "else");
                if closes {
                    depth = depth.saturating_sub(1);
                }
                let words: Vec<String> = toks
                    .iter()
                    .map(|t| match t {
                        Tok::Text(w) => w.clone(),
                        Tok::Func(k) => funcs.name(*k),
                        Tok::Local(l) => locals.names.name(*l),
                    })
                    .collect();
                let _ = writeln!(out, "  {}{}", "  ".repeat(depth), words.join(" "));
                if opens {
                    depth += 1;
                }
            }
        }
        if excerpts.is_empty() {
            out.push_str("(no code from the subject)\n");
            // Why a generic function's code is missing: no instance.
            for note in &built.fixture_notes {
                let _ = writeln!(out, "{}", note.replace("app.main.", ""));
            }
        }
    }
    let rendered = built.render();
    if !rendered.trim().is_empty() {
        out.at(p.text.len());
        let _ = writeln!(out, "{}", rendered.trim_end());
    }
    out.finish()
}

type Instructions = Vec<(usize, Vec<Tok>)>;

/// One word of a printed instruction: text, or a function or local that
/// is named once the whole view is known.
enum Tok {
    Text(String),
    Func(u32),
    Local(u32),
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
            let name = f
                .globals
                .iter()
                .find(|(k, _)| *k == g)
                .map_or_else(|| format!("global{g}"), |(_, n)| short_name(n));
            out.push(Tok::Text(name));
            return out;
        }
        ("call" | "return_call" | "ref.func", [k]) if num(k).is_some() => {
            out.push(Tok::Func(num(k).unwrap_or_default()));
            return out;
        }
        ("i32.const", [c]) => {
            let at = c.parse::<i32>().ok().map(i32::cast_unsigned);
            if let Some((_, data)) = f.statics.iter().find(|(a, _)| Some(*a) == at) {
                out.push(Tok::Text(format!("&{}", static_label(data, module))));
                return out;
            }
        }
        _ => {}
    }
    let mut rest = rest.iter().peekable();
    if memory {
        match rest.peek().and_then(|w| num(w)) {
            Some(1) => {
                rest.next();
            }
            Some(m) => {
                rest.next();
                out.push(Tok::Text(memory_name(m).to_owned()));
            }
            None => out.push(Tok::Text(memory_name(0).to_owned())),
        }
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
            out.push(Tok::Text(module.signature(t)));
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
    types: Vec<String>,
}

impl Module {
    fn read(wasm: &[u8]) -> Module {
        let mut module = Module {
            bodies: BTreeMap::new(),
            names: BTreeMap::new(),
            types: Vec::new(),
        };
        let mut imported = 0u32;
        let mut defined = 0u32;
        for payload in wasmparser::Parser::new(0).parse_all(wasm) {
            match payload.expect("the module parses") {
                wasmparser::Payload::TypeSection(types) => {
                    for ty in types.into_iter_err_on_gc_types() {
                        let ty = ty.expect("a function type");
                        let list = |ts: &[wasmparser::ValType]| {
                            let ts: Vec<String> = ts.iter().map(ToString::to_string).collect();
                            format!("({})", ts.join(" "))
                        };
                        module.types.push(format!(
                            "{} -> {}",
                            list(ty.params()),
                            list(ty.results())
                        ));
                    }
                }
                wasmparser::Payload::ImportSection(imports) => {
                    for import in imports.into_imports() {
                        let import = import.expect("an import");
                        if matches!(import.ty, wasmparser::TypeRef::Func(_)) {
                            module
                                .names
                                .insert(imported, format!("{}/{}", import.module, import.name));
                            imported += 1;
                        }
                    }
                }
                wasmparser::Payload::CodeSectionEntry(body) => {
                    module.bodies.insert(imported + defined, body.range().start);
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
        self.types
            .get(t as usize)
            .cloned()
            .unwrap_or_else(|| format!("type{t}"))
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
