//! Rule 19 at every independent site in one rebuild (#1978): a
//! short-circuit value `T; if (result i32) X else i32.const K end` (`&&`
//! for K = 0, `||` for K = 1) whose result only feeds a branch becomes a
//! chain of branches. The value form makes a backend join the two arms and
//! test the joined value: a taken jump onto a branch on every evaluation,
//! which Cranelift does not thread (#1725).
//!
//! The test after the value, flipped by an `i32.eqz` before it, is one of:
//! - `br_if d`: when K takes the branch, `T; i32.eqz; br_if d; X` and the
//!   test; else `T; if; X` and the test to `d+1`, then `end`;
//! - `if A else B end`: the arm K picks (`Outer`) is reached from two
//!   places and the other (`Inner`) from one, so `Inner` nests inside the
//!   test of `X`: `T; if; X; if Inner end; end` when `Outer` is empty, else
//!   `block; T; if; X; if Inner; br 2; end; end; Outer; end`.
//!
//! Moving runs in and out of blocks changes their branch depths. Rather
//! than shifting each moved run, one pass names every branch's target by
//! the instruction that opens its label ([`FUNC`] for the function's own),
//! the rebuild copies those names, and one last pass turns each name back
//! into a depth from a stack of open labels. A rewrite that adds a label
//! gives it the name of the label it stands for: the `block` of the last
//! form ends where the test's `if` ended, so it takes that `if`'s name.
//!
//! The sites a round rewrites are those whose instructions overlap no
//! other's, outermost first, as the one-site-per-pass rule rewrote the
//! outermost value first: `a && b && c` feeding a branch is the outer `&&`
//! feeding it, and then the inner one feeds it too, in the next round. A
//! site inside another's waits a round.
//!
//! Cost: one pass to name targets, one test per site (the block form walks
//! back over `T`, at most `OPERAND_REACH` instructions), a sort of the
//! sites taken, one rebuild and one pass to name depths: linear in the
//! body per round. Rewriting one site per pass over the body was
//! quadratic in a body's branch-feeding values.

use super::{Arity, Func, I, Ins, Remap, operand_start, short_circuit};
use hd_base::SliceSort as _;
use wasm_encoder::{BlockType, ValType};

/// The function's own label, past every instruction's name.
const FUNC: u32 = u32::MAX;
/// No label: an instruction that opens none and branches nowhere.
const NONE: u32 = u32::MAX - 1;

/// What a rewrite does with a site `(s, e, n)` and its test at `c`.
#[derive(Clone, Copy)]
enum Form {
    /// `br_if` that K falls through: `T; if; X; [eqz]; br_if d+1; end`.
    Falls { c: usize },
    /// `br_if` that K takes: `T; i32.eqz; br_if d; X; [eqz]; br_if d`.
    Takes { c: usize },
    /// `if` with an empty `Outer`: `T; if; X; [eqz]; if Inner end; end`.
    Nested {
        c: usize,
        cn: usize,
        inner: (usize, usize),
        k: u32,
    },
    /// `block; T; if; X; [eqz]; (br_if 1 | if Inner; br 2; end); end;
    /// Outer; end`, opened at `j`.
    Block {
        j: usize,
        c: usize,
        cn: usize,
        inner: (usize, usize),
        outer: (usize, usize),
        k: u32,
        bt: BlockType,
    },
}

/// A site to rewrite: the value's `if`, `else` and `end`, and its form.
#[derive(Clone, Copy)]
struct Plan {
    s: usize,
    e: usize,
    n: usize,
    form: Form,
}

impl Plan {
    /// The instructions the rewrite reads, first and last.
    fn span(&self) -> (usize, usize) {
        match self.form {
            Form::Falls { c } | Form::Takes { c } => (self.s, c),
            Form::Nested { cn, .. } => (self.s, cn),
            Form::Block { j, cn, .. } => (j, cn),
        }
    }
}

/// Each instruction's label name: an opener's own index, a `br` or
/// `br_if`'s target, a `br_table`'s index into `tables` (its targets and
/// its default); `NONE` for any
/// other. And whether a branch names each opener.
struct Names {
    of: Vec<u32>,
    /// Whether a branch to each opener's label carries stack values: a
    /// loop's takes its parameters, a block's or an `if`'s its results.
    values: Vec<bool>,
    tables: Vec<(Vec<u32>, u32)>,
    targeted: Vec<bool>,
}

fn names(f: &Func) -> Names {
    let n = f.body.len();
    let mut of = vec![NONE; n];
    let mut tables = Vec::new();
    let mut targeted = vec![false; n];
    let mut values = vec![false; n];
    let mut open: Vec<u32> = Vec::new();
    let name = |d: u32, open: &[u32], targeted: &mut [bool]| -> u32 {
        match open.len().checked_sub(d as usize + 1) {
            Some(at) => {
                targeted[open[at] as usize] = true;
                open[at]
            }
            None => FUNC,
        }
    };
    for (at, ins) in f.body.iter().enumerate() {
        crate::work::control(1);
        let at32 = word(at);
        // Other instructions name no label.
        if let Ins::Op(I::Loop(bt)) = ins {
            values[at] = matches!(bt, BlockType::FunctionType(_));
            of[at] = at32;
            open.push(at32);
        } else if let Ins::Op(I::Block(bt) | I::If(bt)) = ins {
            values[at] = !matches!(bt, BlockType::Empty);
            of[at] = at32;
            open.push(at32);
        } else if let Ins::Op(I::End) = ins {
            open.pop();
        } else if let Ins::Op(I::Br(d) | I::BrIf(d)) = ins {
            of[at] = name(*d, &open, &mut targeted);
        } else if let Ins::Op(I::BrTable(table, default)) = ins {
            let list = f
                .label_list(*table)
                .iter()
                .map(|&d| name(d, &open, &mut targeted))
                .collect();
            let default = name(*default, &open, &mut targeted);
            of[at] = word(tables.len());
            tables.push((list, default));
        }
    }
    Names {
        of,
        values,
        tables,
        targeted,
    }
}

/// Index `i` as a word.
fn word(i: usize) -> u32 {
    // ruled out: a function's instructions, its sites and its branch
    // tables are each fewer than its body's length, which the module
    // encodes in 32 bits.
    u32::try_from(i).expect("a body index fits 32 bits")
}

/// Whether a branch to label `name` carries stack values.
fn takes_values(f: &Func, names: &Names, name: u32) -> bool {
    if name == FUNC {
        !f.results.is_empty()
    } else {
        names.values[name as usize]
    }
}

/// The rewrite of site `(s, e, n)`, if it is one.
fn plan(
    f: &Func,
    names: &Names,
    (s, e, n): (usize, Option<usize>, usize),
    at_start: &[u32],
    sites: &[(usize, Option<usize>, usize)],
    arity: &Arity,
) -> Option<Plan> {
    let b = &f.body;
    let (Ins::Op(I::If(BlockType::Result(ValType::I32))), Some(e)) = (b[s], e) else {
        return None;
    };
    let k = short_circuit(&b[e + 1..n])?;
    // A branch to the value's own `if` comes from `X`.
    if names.targeted[s] {
        return None;
    }
    let flip = matches!(b.get(n + 1), Some(Ins::Op(I::I32Eqz)));
    let c = n + 1 + usize::from(flip);
    let form = match *b.get(c)? {
        // K takes the branch exactly when K, flipped, is nonzero.
        Ins::Op(I::BrIf(_)) if k == u32::from(flip) => {
            // Inside the new `if`, the branch reaches no stack values.
            if takes_values(f, names, names.of[c]) {
                return None;
            }
            Form::Falls { c }
        }
        Ins::Op(I::BrIf(_)) => Form::Takes { c },
        Ins::Op(I::If(bt)) if !matches!(bt, BlockType::FunctionType(_)) => {
            let (_, ce, cn) = sites[at_start[c] as usize];
            let a = (c + 1, ce.unwrap_or(cn));
            let other = ce.map_or((cn, cn), |ce| (ce + 1, cn));
            // A nested short-circuit value is a value use: it stays.
            if matches!(bt, BlockType::Result(_)) && short_circuit(&b[other.0..other.1]).is_some() {
                return None;
            }
            // `then` is reached when the value is nonzero.
            let (then, otherwise) = if flip { (other, a) } else { (a, other) };
            let (outer, inner) = if k == 0 {
                (otherwise, then)
            } else {
                (then, otherwise)
            };
            let empty = |r: (usize, usize)| r.0 == r.1;
            if empty(inner) && empty(outer) {
                return None;
            }
            if empty(outer) {
                Form::Nested { c, cn, inner, k }
            } else {
                let j = operand_start(b, s, arity, &f.symbols)?;
                Form::Block {
                    j,
                    c,
                    cn,
                    inner,
                    outer,
                    k,
                    bt,
                }
            }
        }
        _ => return None,
    };
    Some(Plan { s, e, n, form })
}

/// The rebuilt body, each instruction with its label name.
struct Out<'a> {
    body: &'a [Ins],
    names: &'a Names,
    ins: Vec<Ins>,
    name: Vec<u32>,
    tables: Vec<(Vec<u32>, u32)>,
    origins: Remap,
}

impl Out<'_> {
    /// Copies instructions `from..to` with their names.
    fn copy(&mut self, from: usize, to: usize) {
        for at in from..to {
            crate::work::control(1);
            let ins = self.body[at];
            let name = match ins {
                Ins::Op(I::BrTable(..)) => {
                    let list = self.names.tables[self.names.of[at] as usize].clone();
                    self.tables.push(list);
                    word(self.tables.len() - 1)
                }
                _ => self.names.of[at],
            };
            self.ins.push(ins);
            self.name.push(name);
        }
        self.origins.copy(from..to);
    }

    /// A new instruction named `name`, from `origin`.
    fn add(&mut self, i: I, name: u32, origin: Option<u32>) {
        crate::work::control(1);
        self.ins.push(Ins::Op(i));
        self.name.push(name);
        self.origins.repeat(origin, 1);
    }

    /// Site `p`'s rewrite.
    fn rewrite(&mut self, p: Plan) {
        let Plan { s, e, n, form } = p;
        let s32 = word(s);
        let c = match form {
            Form::Falls { c }
            | Form::Takes { c }
            | Form::Nested { c, .. }
            | Form::Block { c, .. } => c,
        };
        let origin = self
            .origins
            .first(s..s + 1)
            .or_else(|| self.origins.first(n + 1..c + 1));
        let keep_if = |out: &mut Self| {
            let origin = out.origins.first(s..s + 1);
            out.add(I::If(BlockType::Empty), s32, origin);
        };
        match form {
            Form::Falls { c } => {
                keep_if(self);
                self.copy(s + 1, e);
                self.copy(n + 1, c + 1);
                self.add(I::End, NONE, origin);
            }
            Form::Takes { c } => {
                self.add(I::I32Eqz, NONE, origin);
                self.add(I::BrIf(0), self.names.of[c], origin);
                self.copy(s + 1, e);
                // The test stays after `X`.
                self.copy(n + 1, c + 1);
            }
            Form::Nested { c, cn, inner, k } => {
                keep_if(self);
                self.copy(s + 1, e);
                if k == 1 {
                    self.add(I::I32Eqz, NONE, origin);
                }
                // `Outer` is empty, so the test's `if` is void.
                self.copy(c, c + 1);
                self.copy(inner.0, inner.1);
                self.copy(cn, cn + 1);
                self.add(I::End, NONE, origin);
            }
            Form::Block {
                j,
                c,
                cn,
                inner,
                outer,
                k,
                bt,
            } => {
                // The block ends where the test's `if` ended: a branch to
                // that `if` from `Inner` or `Outer` reaches it.
                let c32 = word(c);
                self.add(I::Block(bt), c32, origin);
                self.copy(j, s);
                self.add(I::If(BlockType::Empty), s32, origin);
                self.copy(s + 1, e);
                if k == 1 {
                    self.add(I::I32Eqz, NONE, origin);
                }
                if inner.0 == inner.1 {
                    self.add(I::BrIf(0), c32, origin);
                } else {
                    // No branch names the new `if`.
                    let test = self.origins.first(c..c + 1);
                    self.add(I::If(BlockType::Empty), NONE, test);
                    self.copy(inner.0, inner.1);
                    let falls = !matches!(
                        self.body[inner.1 - 1],
                        Ins::Op(I::Br(_) | I::BrTable(..) | I::Return | I::Unreachable)
                    );
                    if falls {
                        self.add(I::Br(0), c32, origin);
                    }
                    self.add(I::End, NONE, origin);
                }
                self.add(I::End, NONE, origin);
                self.copy(outer.0, outer.1);
                self.copy(cn, cn + 1);
            }
        }
    }
}

/// Rule 19 at every site of `sites` whose instructions overlap no other
/// taken site's, outermost first, in one rebuild. Whether it rewrote one.
pub(super) fn run(f: &mut Func, sites: &[(usize, Option<usize>, usize)], arity: &Arity) -> bool {
    // Name the targets only for a body with a short-circuit value.
    let value = |&(s, e, n): &(usize, Option<usize>, usize)| {
        matches!(f.body[s], Ins::Op(I::If(BlockType::Result(ValType::I32))))
            && e.is_some_and(|e| short_circuit(&f.body[e + 1..n]).is_some())
    };
    if !sites.iter().any(value) {
        return false;
    }
    let names = names(f);
    // Each site by the index of its `if`.
    let mut at_start = vec![NONE; f.body.len()];
    for (k, &(s, _, _)) in sites.iter().enumerate() {
        at_start[s] = word(k);
    }
    let mut plans: Vec<Plan> = sites
        .iter()
        .filter_map(|&site| {
            crate::work::control(1);
            plan(f, &names, site, &at_start, sites, arity)
        })
        .collect();
    if plans.is_empty() {
        return false;
    }
    // Outermost first: by first instruction, the longer span first.
    plans.sort_stable_by_key(|p| {
        let (a, z) = p.span();
        (a, std::cmp::Reverse(z))
    });
    let mut taken = Vec::with_capacity(plans.len());
    let mut end = None;
    for p in plans {
        let (a, z) = p.span();
        if end.is_none_or(|end| a > end) {
            taken.push(p);
            end = Some(z);
        }
    }
    let body = std::mem::take(&mut f.body);
    let mut out = Out {
        body: &body,
        names: &names,
        ins: Vec::with_capacity(body.len() + 4 * taken.len()),
        name: Vec::with_capacity(body.len() + 4 * taken.len()),
        tables: Vec::new(),
        origins: Remap::new(f.origins.as_deref()),
    };
    let mut at = 0;
    for p in taken {
        let (a, z) = p.span();
        out.copy(at, a);
        out.rewrite(p);
        at = z + 1;
    }
    out.copy(at, body.len());
    let Out {
        mut ins,
        name,
        tables,
        origins,
        ..
    } = out;
    // Each name back to a depth, from the height its label opened at.
    let mut open = 0u32;
    let mut heights = vec![0u32; body.len()];
    let depth = |name: u32, open: u32, heights: &[u32]| {
        if name == FUNC {
            open
        } else {
            open - 1 - heights[name as usize]
        }
    };
    for (i, nm) in ins.iter_mut().zip(name) {
        crate::work::control(1);
        // Other instructions name no label.
        if let Ins::Op(I::Block(_) | I::Loop(_) | I::If(_)) = i {
            // A new `if` no branch names has no slot.
            if let Some(h) = heights.get_mut(nm as usize) {
                *h = open;
            }
            open += 1;
        } else if let Ins::Op(I::End) = i {
            open = open.saturating_sub(1);
        } else if let Ins::Op(I::Br(d) | I::BrIf(d)) = i {
            *d = depth(nm, open, &heights);
        } else if let Ins::Op(I::BrTable(table, default)) = i {
            let (targets, otherwise) = &tables[nm as usize];
            let list: Vec<u32> = targets.iter().map(|&t| depth(t, open, &heights)).collect();
            *default = depth(*otherwise, open, &heights);
            *table = if list == f.label_list(*table) {
                *table
            } else {
                f.intern_labels(list)
            };
        }
    }
    f.body = ins;
    origins.finish(f);
    true
}
