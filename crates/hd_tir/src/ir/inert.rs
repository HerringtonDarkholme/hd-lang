//! Whether a body does nothing once some of its values are known: a
//! monomorphized `Drop` hook whose work sits under `if needs_drop::[T]()`
//! does nothing for a `T` that needs no drop, so its type has no hook
//! (`hd_layout::hook_runs`).

use hd_types::Ty;

use super::{Body, NONE, PrimOp, Ref, Root, Tag, Topology};

/// Whether running block `root` of `b` changes nothing a caller can see
/// and always ends. Each statement it runs reads, compares, constructs,
/// coerces, takes a reference, stores into a local's own storage,
/// returns, or is a call whose value `fold` decides (a type fact such as
/// `needs_drop::[T]()`). An `if`, `and` or `or` whose condition [`decided`]
/// settles runs only the arm it takes; a cleanup scope runs its body and
/// suites. A loop, any other call, a store through memory, an element
/// read (which checks its index) or arithmetic (which checks overflow) is
/// work. O(statements), for the topology.
pub fn inert_block(b: &Body, root: u32, fold: &dyn Fn(usize) -> Option<bool>) -> bool {
    let owner = Topology::new(b).owner;
    let mut stack = vec![root];
    while let Some(x) = stack.pop() {
        let i = x as usize;
        let Some(&[p, q]) = b.data.get(i) else {
            return false;
        };
        // A missing `else` is `NONE`.
        let arms = |k: u32| b.record(k).iter().copied().filter(|&m| m != NONE);
        match b.tags[i] {
            // A listed block that an instruction runs runs where it says.
            Tag::Block => stack.extend(
                arms(p).filter(|&m| b.tags[m as usize] != Tag::Block || owner[m as usize] == NONE),
            ),
            Tag::If => match decided(b, p, fold) {
                Some(c) => stack.extend(b.record(q).get(usize::from(!c)).filter(|&&m| m != NONE)),
                None => stack.extend(arms(q)),
            },
            Tag::And | Tag::Or if decided(b, p, fold) == Some(b.tags[i] == Tag::Or) => {}
            Tag::And | Tag::Or => stack.push(q),
            Tag::Scope => {
                stack.push(p);
                stack.extend(arms(q));
            }
            Tag::Prim if !matches!(PrimOp::from_u32(p), Some(op) if checks_nothing(op)) => {
                return false;
            }
            Tag::Read if b.view(p).kinds(b).1 => return false,
            Tag::Store
                if !matches!(b.view(p).root, Root::Local(_))
                    || b.view(p).kinds(b) != (0, false) =>
            {
                return false;
            }
            Tag::Call if fold(i).is_none() => return false,
            Tag::Call
            | Tag::Prim
            | Tag::Read
            | Tag::Store
            | Tag::Ref
            | Tag::Coerce
            | Tag::NewStruct
            | Tag::NewTuple
            | Tag::NewVariant
            | Tag::Is
            | Tag::ItemRef
            | Tag::Defer
            | Tag::Return => {}
            _ => return false,
        }
    }
    true
}

/// The value of boolean `r` when the body settles it: a constant, `not`
/// of a settled value, `and`/`or` short-circuited or with both sides
/// settled, or a value `fold` decides.
#[must_use]
pub fn decided(b: &Body, r: u32, fold: &dyn Fn(usize) -> Option<bool>) -> Option<bool> {
    let Some(i) = Ref(r).as_inst() else {
        let &(ty, bits) = b.consts.get((r & !Ref::CONST_BIT) as usize)?;
        return (ty == Ty::BOOL).then_some(bits != 0);
    };
    let i = i.0 as usize;
    let [p, q] = *b.data.get(i)?;
    match b.tags[i] {
        Tag::Prim if PrimOp::from_u32(p) == Some(PrimOp::Not) => {
            Some(!decided(b, *b.record(q).first()?, fold)?)
        }
        Tag::And | Tag::Or => {
            let short = b.tags[i] == Tag::Or;
            match decided(b, p, fold)? {
                v if v == short => Some(short),
                _ => decided(b, b.data.get(q as usize)?[1], fold),
            }
        }
        _ => fold(i),
    }
}

/// Whether a primitive can never stop the program: a comparison or a
/// bitwise or logical operator, which checks no overflow and divides by
/// nothing.
fn checks_nothing(op: PrimOp) -> bool {
    use PrimOp as P;
    matches!(
        op,
        P::Eq
            | P::Ne
            | P::Lt
            | P::Le
            | P::Gt
            | P::Ge
            | P::And
            | P::Or
            | P::BitAnd
            | P::BitOr
            | P::BitXor
            | P::Not
    )
}
