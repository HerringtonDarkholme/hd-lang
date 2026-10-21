//! A prototype of the memory IR of the IR study (issue #667,
//! `docs/compiler/memory-ir.md`). Not on the production path: only the
//! `mir` development view ([`crate::views::mir`]) builds it.
//!
//! One function per instance, after layout, with concrete types. Values
//! live in variables (a variable is its type's Wasm values; a variable
//! whose address is taken lives in a counted cell). Memory is reached by
//! an address plus a constant offset. Control flow is structured, as Wasm
//! is: `block`, `loop`, `if`, `br`. Counts are statements: `retain`,
//! `release`, `kill` (zero a variable's counted values, the drop flag), and
//! copy-on-write is `unique`.
//!
//! [`lower`] builds a function from closure-stripped TIR with conservative
//! counts; [`own`] then places releases and removes counts by local,
//! per-function passes; [`verify`] checks the structure and that every
//! count is balanced on every path.

pub mod lower;
pub mod own;
pub mod verify;

use std::fmt::Write as _;

use crate::module::Val;

pub type VarId = u32;
pub type Label = u32;

/// How a parameter takes its argument (`layout::Pass`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The callee owns a count of each counted value.
    Owned,
    /// `&T`: the caller keeps the referent alive; nobody counts.
    Borrowed,
    /// Nothing in the value is counted.
    Plain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarKind {
    Param(Mode),
    /// A TIR local.
    Local,
    /// A TIR instruction's value, or a lowering temporary.
    Temp,
    /// The function's result, which `return` hands to the caller.
    Result,
}

#[derive(Clone, Debug)]
pub struct Var {
    /// `$N` for TIR local `N` (the view prints its source name), else `tN`.
    pub name: String,
    pub tir_local: Option<u32>,
    /// The hd type, printed.
    pub ty: String,
    pub vals: Vec<Val>,
    /// Whether some value of the type is counted.
    pub counted: bool,
    /// How many of its values are counted references.
    pub refs: u32,
    pub kind: VarKind,
    /// The variable holds the address of a counted cell that holds the
    /// value (a local some reference points at).
    pub cell: bool,
}

/// Some values of a variable: a field path or an enum payload field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proj {
    pub leaves: Vec<u32>,
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Operand {
    Var(VarId, Option<Proj>),
    /// A constant, as printed; immortal when counted.
    Const(String),
}

impl Operand {
    #[must_use]
    pub fn var(&self) -> Option<VarId> {
        match self {
            Operand::Var(v, _) => Some(*v),
            Operand::Const(_) => None,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Rvalue {
    /// The operand's bits; no count changes.
    Use(Operand),
    /// A struct or tuple value from its fields' values, in order.
    Make { ty: String, parts: Vec<Operand> },
    /// An enum value: the variant's tag and its payload values.
    Variant {
        ty: String,
        variant: String,
        parts: Vec<Operand>,
    },
    /// An enum value's tag.
    Tag(Operand),
    /// The value of type `ty` at address `addr + off`.
    Load { addr: Operand, off: u32, ty: String },
    /// `Load`, with the memory's count of it: a store is about to replace
    /// the value there.
    Take { addr: Operand, off: u32, ty: String },
    /// An arithmetic, comparison or conversion operator.
    Prim { op: String, args: Vec<Operand> },
    /// `base + off`, an address (Go's `OffPtr`).
    Offset { base: Operand, off: u32 },
    /// `base + off + index * size` (Go's `PtrIndex`).
    IndexAddr {
        base: Operand,
        index: Operand,
        size: u32,
        off: u32,
    },
    /// A new counted object of type `ty`, with one count, uninitialized.
    Alloc { ty: String },
    /// The list buffer at the place `addr`, made the place's own: when
    /// another owner shares it (its count is over 1 and it is not
    /// pinned), a copy is stored back to the place first. The result
    /// borrows from the place (Swift's `begin_cow_mutation`).
    Unique { addr: Operand, ty: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Count {
    Retain,
    Release,
}

#[derive(Clone, Debug)]
pub enum Callee {
    /// An instance, by its item's path and type arguments.
    Direct(String),
    /// A function value: its code and environment.
    Value(Operand),
}

#[derive(Clone, Debug)]
pub enum Kind {
    Assign {
        dst: VarId,
        proj: Option<Proj>,
        rv: Rvalue,
    },
    /// Stores `val` of type `ty` at `addr + off`; `val` holds no count
    /// afterwards when the store consumes it (a counted type).
    Store {
        addr: Operand,
        off: u32,
        ty: String,
        val: Operand,
    },
    Count {
        op: Count,
        var: VarId,
        ty: String,
    },
    /// The variable's counted values become 0 with no count change: the
    /// value moved out, or was released.
    Kill {
        var: VarId,
        proj: Option<Proj>,
    },
    /// Sets the pinned flag of a list buffer (`ref.elem.pin`); the pin's
    /// count is an ordinary retain.
    Pin {
        var: VarId,
    },
    /// Panics `index-out-of-bounds` unless `index < len`.
    Check {
        index: Operand,
        len: Operand,
    },
    Call {
        dst: Option<VarId>,
        callee: Callee,
        args: Vec<(Operand, Mode)>,
    },
    If {
        cond: Operand,
        then: Vec<Stmt>,
        els: Vec<Stmt>,
    },
    /// `br label` inside goes to the block's end.
    Block {
        label: Label,
        body: Vec<Stmt>,
    },
    /// `br label` inside goes to the loop's start; so does falling off
    /// the end of the body.
    Loop {
        label: Label,
        body: Vec<Stmt>,
    },
    Br {
        label: Label,
    },
    /// Goes to the function's exit, which returns the result variable.
    Return,
    Unreachable,
    Unsupported(String),
}

#[derive(Clone, Debug)]
pub struct Stmt {
    /// Unique within its function: what the passes key their facts by.
    pub id: u32,
    /// The TIR instruction it comes from, or `NONE`.
    pub origin: u32,
    pub kind: Kind,
}

/// What the ownership passes did, for the view's summary line.
#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub retains_before: usize,
    pub releases_before: usize,
    pub moved: usize,
    pub borrowed: usize,
    pub placed: usize,
    pub null_releases: usize,
}

#[derive(Clone, Debug)]
pub struct Func {
    /// The item's stable path, as the views name it (`app/main/update`).
    pub item: String,
    pub vars: Vec<Var>,
    pub params: Vec<VarId>,
    pub result: Option<VarId>,
    pub body: Vec<Stmt>,
    /// Runs after the body and after every `return`: the conservative
    /// releases of every counted variable.
    pub exit: Vec<Stmt>,
    pub labels: u32,
    pub next_id: u32,
    pub stats: Stats,
    /// Set when the lowering met a TIR form the prototype does not cover.
    pub unsupported: Option<String>,
    /// Verifier findings after the passes (empty when it holds).
    pub errors: Vec<String>,
    /// How many instructions its TIR body has: the origins a view may ask
    /// about.
    pub tir_len: u32,
}

impl Func {
    #[must_use]
    pub fn new(item: String) -> Self {
        Func {
            item,
            vars: Vec::new(),
            params: Vec::new(),
            result: None,
            body: Vec::new(),
            exit: Vec::new(),
            labels: 0,
            next_id: 0,
            stats: Stats::default(),
            unsupported: None,
            errors: Vec::new(),
            tir_len: 0,
        }
    }

    pub fn stmt(&mut self, origin: u32, kind: Kind) -> Stmt {
        let id = self.next_id;
        self.next_id += 1;
        Stmt { id, origin, kind }
    }

    pub fn label(&mut self) -> Label {
        self.labels += 1;
        self.labels - 1
    }

    #[must_use]
    pub fn counted(&self, v: VarId) -> bool {
        self.vars[v as usize].counted
    }
}

/// Calls `f` on every statement list of `body`, outermost first.
pub fn each_list<'a>(body: &'a [Stmt], f: &mut dyn FnMut(&'a [Stmt])) {
    f(body);
    for s in body {
        match &s.kind {
            Kind::If { then, els, .. } => {
                each_list(then, f);
                each_list(els, f);
            }
            Kind::Block { body, .. } | Kind::Loop { body, .. } => each_list(body, f),
            _ => {}
        }
    }
}

/// Calls `f` on every statement list of `body`, mutably, innermost first.
pub fn each_list_mut(body: &mut Vec<Stmt>, f: &mut dyn FnMut(&mut Vec<Stmt>)) {
    for s in body.iter_mut() {
        match &mut s.kind {
            Kind::If { then, els, .. } => {
                each_list_mut(then, f);
                each_list_mut(els, f);
            }
            Kind::Block { body, .. } | Kind::Loop { body, .. } => each_list_mut(body, f),
            _ => {}
        }
    }
    f(body);
}

/// Every statement of `body`, nested ones included, in order.
pub fn each_stmt<'a>(body: &'a [Stmt], f: &mut dyn FnMut(&'a Stmt)) {
    for s in body {
        f(s);
        match &s.kind {
            Kind::If { then, els, .. } => {
                each_stmt(then, f);
                each_stmt(els, f);
            }
            Kind::Block { body, .. } | Kind::Loop { body, .. } => each_stmt(body, f),
            _ => {}
        }
    }
}

/// The variables statement `s` reads, not counting its nested lists.
#[must_use]
pub fn reads(s: &Stmt) -> Vec<VarId> {
    let mut out = Vec::new();
    let mut op = |o: &Operand| {
        if let Some(v) = o.var() {
            out.push(v);
        }
    };
    match &s.kind {
        Kind::Assign { dst, proj, rv } => {
            if proj.is_some() {
                op(&Operand::Var(*dst, None));
            }
            rvalue_operands(rv, &mut op);
        }
        Kind::Store { addr, val, .. } => {
            op(addr);
            op(val);
        }
        Kind::Count { var, .. } | Kind::Pin { var } => op(&Operand::Var(*var, None)),
        Kind::Check { index, len } => {
            op(index);
            op(len);
        }
        Kind::Call { callee, args, .. } => {
            if let Callee::Value(f) = callee {
                op(f);
            }
            for (a, _) in args {
                op(a);
            }
        }
        Kind::If { cond, .. } => op(cond),
        Kind::Kill { proj: Some(_), var } => op(&Operand::Var(*var, None)),
        _ => {}
    }
    out
}

/// Calls `f` on every operand of statement `s` (not of its nested lists).
pub fn operands_mut(s: &mut Stmt, f: &dyn Fn(&mut Operand)) {
    let rv = |rv: &mut Rvalue| match rv {
        Rvalue::Use(o) | Rvalue::Tag(o) => f(o),
        Rvalue::Make { parts, .. } | Rvalue::Variant { parts, .. } => parts.iter_mut().for_each(f),
        Rvalue::Load { addr, .. } | Rvalue::Take { addr, .. } | Rvalue::Unique { addr, .. } => {
            f(addr);
        }
        Rvalue::Prim { args, .. } => args.iter_mut().for_each(f),
        Rvalue::Offset { base, .. } => f(base),
        Rvalue::IndexAddr { base, index, .. } => {
            f(base);
            f(index);
        }
        Rvalue::Alloc { .. } => {}
    };
    match &mut s.kind {
        Kind::Assign { rv: r, .. } => rv(r),
        Kind::Store { addr, val, .. } => {
            f(addr);
            f(val);
        }
        Kind::Check { index, len } => {
            f(index);
            f(len);
        }
        Kind::Call { callee, args, .. } => {
            if let Callee::Value(o) = callee {
                f(o);
            }
            for (a, _) in args.iter_mut() {
                f(a);
            }
        }
        Kind::If { cond, .. } => f(cond),
        _ => {}
    }
}

pub fn rvalue_operands(rv: &Rvalue, f: &mut dyn FnMut(&Operand)) {
    match rv {
        Rvalue::Use(o) | Rvalue::Tag(o) => f(o),
        Rvalue::Make { parts, .. } | Rvalue::Variant { parts, .. } => parts.iter().for_each(f),
        Rvalue::Load { addr, .. } | Rvalue::Take { addr, .. } | Rvalue::Unique { addr, .. } => {
            f(addr);
        }
        Rvalue::Prim { args, .. } => args.iter().for_each(f),
        Rvalue::Offset { base, .. } => f(base),
        Rvalue::IndexAddr { base, index, .. } => {
            f(base);
            f(index);
        }
        Rvalue::Alloc { .. } => {}
    }
}

/// How the printer names things the function does not know.
pub struct Names<'a> {
    /// A TIR local's source name, by number.
    pub local: &'a dyn Fn(u32) -> Option<String>,
    /// Whether to print a statement from TIR instruction `origin`;
    /// a nested list is printed when any of its statements is.
    pub keep: &'a dyn Fn(u32) -> bool,
}

impl Func {
    /// A variable's printed name: `_N` and, when it has one, its source
    /// name (`_2 body`).
    fn var_name(&self, v: VarId, names: &Names<'_>) -> String {
        let var = &self.vars[v as usize];
        match var.tir_local.and_then(|l| (names.local)(l)) {
            Some(n) => n,
            None => format!("_{v}"),
        }
    }

    fn operand(&self, o: &Operand, names: &Names<'_>) -> String {
        match o {
            Operand::Var(v, p) => {
                let base = self.var_name(*v, names);
                match p {
                    Some(p) => format!("{base}{}", p.path),
                    None => base,
                }
            }
            Operand::Const(c) => c.clone(),
        }
    }

    fn addr(&self, a: &Operand, off: u32, names: &Names<'_>) -> String {
        if off == 0 {
            format!("[{}]", self.operand(a, names))
        } else {
            format!("[{} + {off}]", self.operand(a, names))
        }
    }

    fn rvalue(&self, rv: &Rvalue, names: &Names<'_>) -> String {
        let list = |os: &[Operand]| {
            os.iter()
                .map(|o| self.operand(o, names))
                .collect::<Vec<_>>()
                .join(", ")
        };
        match rv {
            Rvalue::Use(o) => self.operand(o, names),
            Rvalue::Make { ty, parts } => format!("{ty} {{ {} }}", list(parts)),
            Rvalue::Variant { ty, variant, parts } if parts.is_empty() => {
                format!("{ty}.{variant}")
            }
            Rvalue::Variant { ty, variant, parts } => format!("{ty}.{variant}({})", list(parts)),
            Rvalue::Tag(o) => format!("tag {}", self.operand(o, names)),
            Rvalue::Load { addr, off, ty } => format!("load {ty} {}", self.addr(addr, *off, names)),
            Rvalue::Take { addr, off, ty } => format!("take {ty} {}", self.addr(addr, *off, names)),
            Rvalue::Prim { op, args } => format!("{op} {}", list(args)),
            Rvalue::Offset { base, off } => format!("{} + {off}", self.operand(base, names)),
            Rvalue::IndexAddr {
                base,
                index,
                size,
                off,
            } => format!(
                "{} + {off} + {} * {size}",
                self.operand(base, names),
                self.operand(index, names)
            ),
            Rvalue::Alloc { ty } => format!("alloc {ty}"),
            Rvalue::Unique { addr, ty } => format!("unique {ty} {}", self.addr(addr, 0, names)),
        }
    }

    fn line(&self, s: &Stmt, names: &Names<'_>) -> String {
        match &s.kind {
            // A list variable's own buffer made unique in place.
            Kind::Assign {
                dst,
                proj,
                rv: Rvalue::Unique { addr, ty },
            } if addr == &Operand::Var(*dst, proj.clone()) => {
                let d = self.operand(addr, names);
                format!("{d} = unique {ty} {d}")
            }
            Kind::Assign { dst, proj, rv } => {
                let d = self.operand(&Operand::Var(*dst, proj.clone()), names);
                format!("{d} = {}", self.rvalue(rv, names))
            }
            Kind::Store { addr, off, ty, val } => format!(
                "store {ty} {} = {}",
                self.addr(addr, *off, names),
                self.operand(val, names)
            ),
            Kind::Count { op, var, ty } => {
                let name = self.var_name(*var, names);
                match op {
                    Count::Retain => format!("retain {ty} {name}"),
                    Count::Release => format!("release {ty} {name}"),
                }
            }
            Kind::Kill { var, proj } => {
                format!(
                    "kill {}",
                    self.operand(&Operand::Var(*var, proj.clone()), names)
                )
            }
            Kind::Pin { var } => format!("set_pinned {}", self.var_name(*var, names)),
            Kind::Check { index, len } => format!(
                "check {} < {}",
                self.operand(index, names),
                self.operand(len, names)
            ),
            Kind::Call { dst, callee, args } => {
                let c = match callee {
                    Callee::Direct(p) => p.clone(),
                    Callee::Value(o) => format!("call_value {}", self.operand(o, names)),
                };
                let a = args
                    .iter()
                    .map(|(o, m)| {
                        let o = self.operand(o, names);
                        match m {
                            Mode::Owned => format!("own {o}"),
                            Mode::Borrowed => format!("ref {o}"),
                            Mode::Plain => o,
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                match dst {
                    Some(d) => format!("{} = call {c}({a})", self.var_name(*d, names)),
                    None => format!("call {c}({a})"),
                }
            }
            Kind::If { cond, .. } => format!("if {}", self.operand(cond, names)),
            Kind::Block { label, .. } => format!("block L{label}"),
            Kind::Loop { label, .. } => format!("loop L{label}"),
            Kind::Br { label } => format!("br L{label}"),
            Kind::Return => "return".to_owned(),
            Kind::Unreachable => "unreachable".to_owned(),
            Kind::Unsupported(w) => format!("unsupported {w}"),
        }
    }

    /// Whether `s` or a statement nested in it is printed.
    fn kept(s: &Stmt, names: &Names<'_>) -> bool {
        if s.origin != hd_tir::ir::NONE && (names.keep)(s.origin) {
            return true;
        }
        let any = |l: &[Stmt]| l.iter().any(|s| Self::kept(s, names));
        match &s.kind {
            Kind::If { then, els, .. } => any(then) || any(els),
            Kind::Block { body, .. } | Kind::Loop { body, .. } => any(body),
            _ => false,
        }
    }

    /// Whether `s` copies variable `v`'s values at `p`, which the next
    /// statement zeroes: a move.
    fn moves(s: &Stmt, next: Option<&Stmt>) -> bool {
        let (
            Kind::Assign {
                rv: Rvalue::Use(Operand::Var(v, p)),
                ..
            },
            Some(Kind::Kill { var, proj }),
        ) = (&s.kind, next.map(|n| &n.kind))
        else {
            return false;
        };
        v == var && p == proj
    }

    fn list(&self, l: &[Stmt], depth: usize, names: &Names<'_>, out: &mut String) {
        let mut k = 0;
        while k < l.len() {
            let s = &l[k];
            k += 1;
            if !Self::kept(s, names) {
                continue;
            }
            let pad = "  ".repeat(depth);
            let mut line = self.line(s, names);
            if Self::moves(s, l.get(k)) {
                // `d = move s`, as Rust's MIR writes a move.
                if let Some((d, src)) = line.split_once(" = ") {
                    line = format!("{d} = move {src}");
                }
                k += 1;
            }
            let _ = writeln!(out, "{pad}{line}");
            match &s.kind {
                Kind::If { then, els, .. } => {
                    self.list(then, depth + 1, names, out);
                    if els.iter().any(|s| Self::kept(s, names)) {
                        let _ = writeln!(out, "{pad}else");
                        self.list(els, depth + 1, names, out);
                    }
                    let _ = writeln!(out, "{pad}end");
                }
                Kind::Block { body, .. } | Kind::Loop { body, .. } => {
                    self.list(body, depth + 1, names, out);
                    let _ = writeln!(out, "{pad}end");
                }
                _ => {}
            }
        }
    }

    /// The function's text: its variables that a printed statement names,
    /// then the printed statements.
    #[must_use]
    pub fn print(&self, names: &Names<'_>) -> String {
        let mut body = String::new();
        self.list(&self.body, 1, names, &mut body);
        // The exit's releases that the passes could not remove: every
        // one is a variable that may still own a count there.
        let mut exit = String::new();
        let all = Names {
            local: names.local,
            keep: &|_| true,
        };
        self.list(&self.exit, 1, &all, &mut exit);
        let mut out = String::new();
        let mut used = vec![false; self.vars.len()];
        let mut mark = |l: &[Stmt]| {
            for s in l {
                if !Self::kept(s, names) {
                    continue;
                }
                for v in reads(s) {
                    used[v as usize] = true;
                }
                match &s.kind {
                    Kind::Assign { dst, .. }
                    | Kind::Call { dst: Some(dst), .. }
                    | Kind::Kill { var: dst, .. } => used[*dst as usize] = true,
                    _ => {}
                }
            }
        };
        each_list(&self.body, &mut mark);
        for s in &self.exit {
            for v in reads(s) {
                used[v as usize] = true;
            }
        }
        for (k, v) in self.vars.iter().enumerate() {
            if !used[k] {
                continue;
            }
            let role = match v.kind {
                VarKind::Param(Mode::Owned) => "param own ",
                VarKind::Param(Mode::Borrowed) => "param ref ",
                VarKind::Param(Mode::Plain) => "param ",
                VarKind::Local => "local ",
                VarKind::Temp => "temp ",
                VarKind::Result => "result ",
            };
            let name = self.var_name(u32::try_from(k).expect("vars"), names);
            let cell = if v.cell { " (cell)" } else { "" };
            let _ = writeln!(out, "  {role}{name}: {}{cell}", v.ty);
        }
        out.push_str(&body);
        if !exit.is_empty() {
            out.push_str("  exit:\n");
            out.push_str(&exit);
        }
        if let Some(w) = &self.unsupported {
            let _ = writeln!(out, "  not lowered: {w}");
        }
        for e in &self.errors {
            let _ = writeln!(out, "  verifier: {e}");
        }
        out
    }
}
