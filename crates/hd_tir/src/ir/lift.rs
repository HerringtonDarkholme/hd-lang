//! Generic local functions lifted as Swift lifts them (step 3b of
//! `docs/future-work/BACKEND_NEUTRAL_TIR.md`, Appendix B, "Verified").
//! `swiftc -emit-silgen` shows one SIL function per generic local
//! function, typed by the enclosing parameters plus its own, its captures
//! as parameters (a mutated one boxed), and calls as direct `apply`s at
//! the type arguments. hd keys that function by (enclosing item, sub-body)
//! at the arguments of the item and of the functions around it, followed
//! by its own: collection's closure instance (`hd_mono`). Its body is its
//! own: [`function_template`] copies the function and the closures it
//! makes out of the item's template once (`inline::closures::extract_subs`),
//! and each instance instantiates that copy like any generic function, so
//! it costs the function, not the enclosing body. The `Closure` that
//! instantiates it builds the environment, its captured parameters. The
//! item's own instance takes [`item_template`]: the item without its
//! generic local functions.

use hd_types::{InternPool, TyList};

use super::{Body, NONE, Tag, Topology, sub_flags, u32_of};

/// The sub-body whose `Closure` makes each sub-body. A recursive
/// instantiation names a function from elsewhere, so it does not count.
fn making(b: &Body, topo: &Topology) -> Vec<u32> {
    let subs = b.sub_root.len();
    let mut enclosing = vec![NONE; subs];
    for i in (0..b.len()).filter(|&i| b.tags[i] == Tag::Closure && !b.is_instantiation(i)) {
        let s = b.data[i][0] as usize;
        if s < subs && enclosing[s] == NONE && topo.sub[i] as usize != s {
            enclosing[s] = topo.sub[i];
        }
    }
    enclosing
}

/// Each generic local function's own type-parameter range (first index,
/// count), from its instantiations' records.
fn ranges(b: &Body, pool: &InternPool) -> Vec<Option<(u32, u32)>> {
    let mut range = vec![None; b.sub_root.len()];
    for i in (0..b.len()).filter(|&i| b.is_instantiation(i)) {
        let r = b.record(b.data[i][1]);
        let n = pool.list_items(TyList(r[3])).len();
        if let Some(x) = range.get_mut(b.data[i][0] as usize) {
            x.get_or_insert((r[2], u32_of(n)));
        }
    }
    range
}

/// Whether `s` stands in `t` (or is `t`), up the making chain.
fn inside(enclosing: &[u32], mut s: usize, t: usize) -> bool {
    for _ in 0..enclosing.len() {
        if s == t {
            return true;
        }
        match enclosing[s] {
            NONE => return false,
            up => s = up as usize,
        }
    }
    false
}

/// Per sub-body of `b`, a checked body, the type-parameter ranges of the
/// generic local functions it stands in, its own first when it is one.
/// Collection keys a lifted function's instance by these alone.
#[must_use]
pub fn scopes(b: &Body, pool: &InternPool) -> Vec<Vec<(u32, u32)>> {
    let enclosing = making(b, &Topology::new(b));
    let range = ranges(b, pool);
    (0..b.sub_root.len())
        .map(|s| {
            let mut out = Vec::new();
            let mut at = s;
            for _ in 0..enclosing.len() {
                out.extend(range[at]);
                match enclosing[at] {
                    NONE => break,
                    up => at = up as usize,
                }
            }
            out
        })
        .collect()
}

/// A template's generic local functions, once per template: where each
/// sub-body stands and each function's own parameters.
#[derive(Debug, Default)]
pub struct Lifting {
    enclosing: Vec<u32>,
    range: Vec<Option<(u32, u32)>>,
}

impl Lifting {
    /// `b`'s, one walk of the template; empty with no generic local
    /// function.
    #[must_use]
    pub fn new(b: &Body, pool: &InternPool) -> Self {
        if !(0..b.sub_root.len()).any(|s| b.sub_is_generic(s)) {
            return Self::default();
        }
        Self {
            enclosing: making(b, &Topology::new(b)),
            range: ranges(b, pool),
        }
    }

    /// The functions an instance at arguments binding the parameter
    /// indices `bound` says is the code of: the innermost ones whose own
    /// parameters are all bound, several when they share them (a function
    /// and its parameters' defaults). Empty for the item's own instance.
    /// O(sub-bodies x depth).
    #[must_use]
    pub fn targets(&self, bound: &dyn Fn(u32) -> bool) -> Vec<usize> {
        let candidates: Vec<usize> = (1..self.range.len())
            .filter(|&s| self.range[s].is_some_and(|(f, n)| n > 0 && (f..f + n).all(bound)))
            .collect();
        (candidates.iter().copied())
            .filter(|&t| {
                !candidates
                    .iter()
                    .any(|&s| s != t && inside(&self.enclosing, s, t))
            })
            .collect()
    }
}

/// The template of the instances of generic local functions `ts`: `b`,
/// the item's template, with those and the plain closures they make
/// alone, and their defining `Closure`s (`inline::closures::extract_subs`).
/// Made once per function set.
///
/// # Errors
/// A kept row that names an instruction outside the kept part.
pub fn function_template(b: &Body, ts: &[usize]) -> Result<Body, String> {
    let enclosing = making(b, &Topology::new(b));
    let keep: Vec<bool> = (0..b.sub_root.len())
        .map(|s| {
            (ts.iter()).any(|&t| {
                inside(&enclosing, s, t) && (s == t || !generic_around(b, &enclosing, s, t))
            })
        })
        .collect();
    let defining: Vec<u32> = (0..b.len())
        .filter(|&i| {
            b.tags[i] == Tag::Closure
                && !b.is_instantiation(i)
                && ts.contains(&(b.data[i][0] as usize))
        })
        .map(u32_of)
        .collect();
    let mut out = crate::inline::extract_subs(b, &keep, &defining, false)?;
    for f in &mut out.sub_flags {
        *f &= !sub_flags::GENERIC;
    }
    Ok(out)
}

/// The template of the item's own instance: `b` without its generic local
/// functions and the sub-bodies they make, each another instance's code.
/// `None` when it has none.
///
/// # Errors
/// A kept row that names an instruction outside the kept part.
pub fn item_template(b: &Body) -> Result<Option<Body>, String> {
    if !(0..b.sub_root.len()).any(|s| b.sub_is_generic(s)) {
        return Ok(None);
    }
    let enclosing = making(b, &Topology::new(b));
    let keep: Vec<bool> = (0..b.sub_root.len())
        .map(|s| !generic_around(b, &enclosing, s, usize::MAX))
        .collect();
    let mut out = crate::inline::extract_subs(b, &keep, &[], true)?;
    for f in &mut out.sub_flags {
        *f &= !sub_flags::GENERIC;
    }
    Ok(Some(out))
}

/// Whether a generic local function other than `t` is `s` or stands
/// between `s` and `t` up the making chain.
fn generic_around(b: &Body, enclosing: &[u32], mut s: usize, t: usize) -> bool {
    for _ in 0..enclosing.len() {
        if s == t {
            return false;
        }
        if b.sub_is_generic(s) {
            return true;
        }
        match enclosing[s] {
            NONE => return false,
            up => s = up as usize,
        }
    }
    false
}
