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

use hd_base::HashSet;
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
    let mut target = vec![false; b.sub_root.len()];
    for &t in ts {
        if let Some(x) = target.get_mut(t) {
            *x = true;
        }
    }
    let stop = stops(b, &making(b, &Topology::new(b)), &target);
    let keep: Vec<bool> = stop
        .iter()
        .map(|&t| t != NONE && target[t as usize])
        .collect();
    let defining: HashSet<u32> = (0..b.len())
        .filter(|&i| {
            work!(1);
            b.tags[i] == Tag::Closure
                && !b.is_instantiation(i)
                && target.get(b.data[i][0] as usize) == Some(&true)
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
    let none = vec![false; b.sub_root.len()];
    let stop = stops(b, &making(b, &Topology::new(b)), &none);
    let keep: Vec<bool> = stop.iter().map(|&t| t == NONE).collect();
    let mut out = crate::inline::extract_subs(b, &keep, &HashSet::default(), true)?;
    for f in &mut out.sub_flags {
        *f &= !sub_flags::GENERIC;
    }
    Ok(Some(out))
}

/// Per sub-body, the first one up its making chain, itself first, that is
/// a generic local function or a `target`; `NONE` when none is. One pass:
/// each sub-body is resolved once, from the nearest resolved one above it,
/// so O(sub-bodies) for any nesting.
fn stops(b: &Body, enclosing: &[u32], target: &[bool]) -> Vec<u32> {
    const OPEN: u32 = NONE - 1;
    let mut stop = vec![OPEN; enclosing.len()];
    let mut path = Vec::new();
    for s in 0..enclosing.len() {
        let mut at = s;
        let found = loop {
            work!(1);
            if stop[at] != OPEN {
                break stop[at];
            }
            path.push(at);
            if b.sub_is_generic(at) || target[at] {
                break u32_of(at);
            }
            match enclosing[at] {
                // A chain that loops (never from the checker) stops nowhere.
                up if up == NONE || path.len() > enclosing.len() => break NONE,
                up => at = up as usize,
            }
        };
        for x in path.drain(..) {
            stop[x] = found;
        }
    }
    stop
}

#[cfg(test)]
mod tests {
    use super::{function_template, item_template};
    use crate::ir::{
        Body, BodyKind, TirBuilder, TirSink, Topology, sub_flags, u32_of, verify_structure,
    };
    use crate::measurements;
    use hd_base::{DefId, NodeIdx};
    use hd_types::Ty;

    /// `k` local functions, each with a few instructions: nested, each made
    /// inside the one before, or side by side in the root. All generic, or
    /// (`plain`) only the outermost, the others its closures.
    fn functions(k: usize, nested: bool, plain: bool) -> Body {
        let n = NodeIdx::from_raw(1);
        let fn_ty = Ty(1_000_000);
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let root = b.open_block();
        let mut open = Vec::new();
        for _ in 0..k {
            let sub = b.open_sub(&[]);
            let block = b.open_block();
            let one = b.const_value(Ty::I32, 1);
            b.prim(0, &[one, one], Ty::I32, n);
            if nested {
                open.push((sub, block));
            } else {
                let r = b.close_block(block, None, Ty::VOID, n);
                b.close_sub(sub, r, fn_ty, n);
            }
        }
        while let Some((sub, block)) = open.pop() {
            let r = b.close_block(block, None, Ty::VOID, n);
            b.close_sub(sub, r, fn_ty, n);
        }
        let r = b.close_block(root, None, Ty::VOID, n);
        let mut body = b.finish(r, &[]).expect("generated TIR");
        for f in body
            .sub_flags
            .iter_mut()
            .skip(1)
            .take(if plain { 1 } else { k })
        {
            *f |= sub_flags::GENERIC;
        }
        let errors = verify_structure(&body, &Topology::new(&body));
        assert!(errors.is_empty(), "{errors:?}");
        body
    }

    /// The templates of all the generic functions together and of the item.
    fn work(k: usize, nested: bool, plain: bool) -> usize {
        let body = functions(k, nested, plain);
        let ts: Vec<usize> = (1..body.sub_root.len())
            .filter(|&s| body.sub_is_generic(s))
            .collect();
        measurements::reset();
        let all = function_template(&body, &ts).expect("function template");
        let own = item_template(&body)
            .expect("item template")
            .expect("generic");
        let work = measurements::total();
        let closures = |b: &Body| {
            b.tags
                .iter()
                .filter(|&&t| t == crate::ir::Tag::Closure)
                .count()
        };
        assert_eq!(closures(&all), k);
        // The item's own instance makes none: each defining `Closure` goes.
        assert_eq!(closures(&own), 0);
        assert!(u32_of(all.len()) > 0);
        work
    }

    #[test]
    fn templates_are_linear_in_the_number_of_local_functions() {
        for (nested, plain) in [(false, false), (true, false), (true, true)] {
            let small = work(200, nested, plain);
            let large = work(2000, nested, plain);
            assert!(
                large <= 11 * small,
                "{nested} {plain}: {small} then {large}"
            );
        }
    }
}
