//! A cold frame's local-reference owners, including conditional payloads.
//!
//! The ownership passes track local references, thin or `&dyn`, which
//! counting counts nothing of (`layout::ref_leaves`): each one's leaf under
//! its discriminant path. Guard prefixes are interned, so a depth-D guard
//! protecting R references occupies D + R records, rather than D * R
//! copied predicates.

use hd_base::HashMap;

use crate::layout::Discr;

pub(crate) fn for_type(
    pool: &hd_types::InternPool,
    env: &dyn hd_mono::ProgramEnv,
    ty: hd_types::Ty,
) -> Plan {
    let mut out = Plan::default();
    let mut index: HashMap<Guard, usize> = HashMap::default();
    for leaf in crate::layout::ref_leaves(pool, env, ty) {
        if leaf.counted {
            continue;
        }
        // A guard is made only for a kept local reference, after its
        // parent: one forward pass evaluates them.
        let mut parent = None;
        for (discr, variant, _) in leaf.guards {
            let guard = Guard {
                parent,
                discr,
                variant,
            };
            let id = *index.entry(guard).or_insert_with(|| {
                out.guards.push(guard);
                out.guards.len() - 1
            });
            parent = Some(id);
        }
        out.refs.push(Referent {
            guard: parent,
            value: leaf.value,
        });
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Guard {
    pub parent: Option<usize>,
    pub discr: Discr,
    pub variant: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Referent {
    pub guard: Option<usize>,
    pub value: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Plan {
    /// Parents precede children, allowing one forward guard evaluation.
    pub guards: Vec<Guard>,
    pub refs: Vec<Referent>,
}
