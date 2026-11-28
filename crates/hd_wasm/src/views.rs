//! Opt-in development views use the same layout and cycle analysis as emission.

use hd_base::SliceSort as _;
use hd_base::StageResult;
use hd_mono::{ProgramEnv, printed_type_name, subst};
use hd_types::AdtId;
use hd_types::{InternPool, Ty, TyData};

use crate::layout;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeLayout {
    pub name: String,
    pub size: u32,
    pub align: u32,
    pub counted: Vec<CountedLeaf>,
    pub cycle_capable: bool,
    pub fields: Vec<FieldLayout>,
    /// The largest niche (`layout::Niche`): bytes with values no value of
    /// the type has.
    pub niche: Option<layout::Niche>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CountedLeaf {
    pub offset: u32,
    /// The shared backend value path of this counted leaf.
    pub path: String,
    /// Only some enum/option variants hold this counted address.
    pub conditional: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldLayout {
    pub path: String,
    pub ty: String,
    pub offset: u32,
    pub size: u32,
    pub align: u32,
    pub counted: Vec<CountedLeaf>,
    pub niche: Option<layout::Niche>,
}

/// Whether a leaf held under the variant tests `held` (one list per place
/// it appears) may be absent: no place holds it untested, and no test
/// holds it in every variant unconditionally. O(the places).
fn conditional(held: &[&[(layout::Discr, usize, usize)]]) -> bool {
    if held.iter().any(|g| g.is_empty()) {
        return false;
    }
    let mut tests: Vec<layout::Discr> = Vec::new();
    for g in held {
        if !tests.contains(&g[0].0) {
            tests.push(g[0].0);
        }
    }
    !tests.into_iter().any(|d| {
        let under: Vec<_> = held.iter().filter(|g| g[0].0 == d).collect();
        let n = under[0][0].2;
        (0..n).all(|v| {
            let arm: Vec<&[(layout::Discr, usize, usize)]> = under
                .iter()
                .filter(|g| g[0].1 == v)
                .map(|g| &g[1..])
                .collect();
            !arm.is_empty() && !conditional(&arm)
        })
    })
}

fn counted(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    ty: Ty,
    base: u32,
) -> StageResult<Vec<CountedLeaf>> {
    let mem = layout::mem(pool, env, ty)?;
    // Each reference leaf once; conditional unless every variant of a
    // test holds it unconditionally.
    let mut by_value = std::collections::BTreeMap::<u32, Vec<layout::Guards>>::new();
    for leaf in layout::ref_leaves(pool, env, ty) {
        by_value.entry(leaf.value).or_default().push(leaf.guards);
    }
    let refs: std::collections::BTreeMap<u32, bool> = by_value
        .into_iter()
        .map(|(k, held)| {
            (
                k,
                conditional(&held.iter().map(Vec::as_slice).collect::<Vec<_>>()),
            )
        })
        .collect();
    let paths = layout::value_paths(pool, env, ty);
    let mut out: Vec<_> = refs
        .into_iter()
        .map(|(k, conditional)| CountedLeaf {
            offset: base + mem.leaves[k as usize].offset,
            path: paths.get(k as usize).cloned().unwrap_or_default(),
            conditional,
        })
        .collect();
    out.sort_stable();
    out.dedup();
    Ok(out)
}

/// Inspect one concrete type using the shared source-field and value-path
/// APIs, without generated Wasm identifiers.
pub fn inspect_layout(pool: &InternPool, env: &dyn ProgramEnv, ty: Ty) -> StageResult<TypeLayout> {
    // The fields of struct `ty` at `base`, named under `prefix`, last
    // first, so a stack pops them in declaration order. Anything else
    // has none.
    fn children(
        pool: &InternPool,
        env: &dyn ProgramEnv,
        ty: Ty,
        prefix: &str,
        base: u32,
    ) -> StageResult<Vec<(Ty, String, u32)>> {
        let Some((def, args)) = pool.adt(layout::strip_newtype(pool, env, ty)) else {
            return Ok(Vec::new());
        };
        if !env.is_struct(def) || env.std_kind(def) != hd_mono::layout::StdKind::Other {
            return Ok(Vec::new());
        }
        let types: Vec<_> = env
            .data_fields(def)
            .unwrap_or_default()
            .into_iter()
            .map(|field| subst(pool, env, def, args, field))
            .collect();
        let names_here = env.data_field_names(def).unwrap_or_default();
        let (_, parts) = layout::payload_mem(pool, env, &types)?;
        let mut out: Vec<_> = (types.into_iter().zip(parts).zip(names_here))
            .map(|((field, part), name)| {
                let path = if prefix.is_empty() {
                    name
                } else {
                    format!("{prefix}.{name}")
                };
                (field, path, base + part.offset)
            })
            .collect();
        out.reverse();
        Ok(out)
    }
    let mem = layout::mem(pool, env, ty)?;
    let mut out = TypeLayout {
        name: printed_type_name(pool, env, ty),
        size: mem.size,
        align: mem.align,
        counted: counted(pool, env, ty, 0)?,
        cycle_capable: crate::cycle::object(pool, env, ty),
        fields: Vec::new(),
        niche: mem.niche,
    };
    // Each field, then the fields inside it, in declaration order: a
    // preorder walk on an explicit stack, so a deep nesting of inline
    // structs costs heap, not native stack. Work: one entry per field.
    let mut stack = children(pool, env, ty, "", 0)?;
    while let Some((field, path, offset)) = stack.pop() {
        let m = layout::mem(pool, env, field)?;
        out.fields.push(FieldLayout {
            path: path.clone(),
            ty: printed_type_name(pool, env, field),
            offset,
            size: m.size,
            align: m.align,
            counted: counted(pool, env, field, offset)?,
            niche: m.niche.map(|n| layout::Niche {
                offset: offset + n.offset,
                ..n
            }),
        });
        stack.extend(children(pool, env, field, &path, offset)?);
    }
    Ok(out)
}

/// One actual count/free operation, attributed to source by the existing map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CountOperation {
    pub offset: u32,
    pub origin: Option<u32>,
    pub action: CountAction,
    /// A source type or a named runtime object, never a Wasm local index.
    pub subject: String,
    /// Why the operation stands.
    pub reason: hd_tir::own::Reason,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CountAction {
    Retain,
    Release,
    Free,
    /// A list's or a map's uniqueness check (`rt:list_unique`,
    /// `rt:map_unique`).
    Unique,
}

impl CountAction {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Retain => "retain",
            Self::Release => "release",
            Self::Free => "free",
            Self::Unique => "unique",
        }
    }
}

pub(crate) fn helper_count_action(s: &crate::module::Sym) -> Option<CountAction> {
    use crate::{Helper, module::Sym};
    use CountAction::{Free, Release, Retain, Unique};
    match s {
        Sym::Helper(Helper::ListUnique { .. } | Helper::MapUnique { .. }) => Some(Unique),
        Sym::Helper(
            Helper::RetainKnown
            | Helper::RetainColl
            | Helper::RetainFields { .. }
            | Helper::RetainAny,
        ) => Some(Retain),
        Sym::Helper(
            Helper::ReleaseRef
            | Helper::ReleaseKnown
            | Helper::ReleaseFresh
            | Helper::DropParts { .. }
            | Helper::ReleaseSpilled { .. }
            | Helper::CycleRelease,
        ) => Some(Release),
        Sym::Helper(Helper::CycleFree) => Some(Free),
        Sym::Shim(_, true, ..) => Some(Retain),
        Sym::Shim(_, false, ..) => Some(Release),
        _ => None,
    }
}

pub(crate) fn count_action(
    s: &crate::module::Sym,
    runtime: &crate::Runtime,
) -> Option<CountAction> {
    use crate::module::Sym;
    use CountAction::{Free, Release, Retain};
    match s {
        Sym::Inst(k) => {
            let r = runtime;
            if [r.retain, r.retain_known].contains(k) {
                Some(Retain)
            } else if [
                r.release,
                r.release_known,
                r.acyclic.release,
                r.acyclic.release_known,
            ]
            .contains(k)
            {
                Some(Release)
            } else if [r.free, r.acyclic.free].contains(k) {
                Some(Free)
            } else {
                None
            }
        }
        _ => helper_count_action(s),
    }
}

/// Extra source-map tokens live after the body's real TIR instruction range.
/// Rewriters already copy these opaque origins; final maps resolve the token
/// back to the original TIR instruction. Count labels never enter Wasm.
#[derive(Clone, Debug)]
pub struct CountOrigin {
    pub origin: Option<u32>,
    pub site: u32,
    pub action: CountAction,
    pub subject: String,
    pub reason: hd_tir::own::Reason,
}

/// Name the counted parts through the same value paths used by WAT locals.
pub(crate) fn count_subject(pool: &InternPool, env: &dyn ProgramEnv, ty: Ty) -> String {
    let name = printed_type_name(pool, env, ty);
    let aggregate = match pool.get(layout::strip_newtype(pool, env, ty)) {
        TyData::Adt(AdtId::Product { .. }, _) => true,
        TyData::Adt(AdtId::Declared(def), _) => {
            env.is_struct(def) && env.std_kind(def) == hd_mono::layout::StdKind::Other
        }
        _ => false,
    };
    if !aggregate {
        return name;
    }
    let paths: Vec<_> = counted(pool, env, ty, 0)
        .unwrap_or_default()
        .into_iter()
        .map(|l| l.path)
        .filter(|p| !p.is_empty())
        .collect();
    if paths.is_empty() {
        name
    } else {
        format!("{name} [{}]", paths.join(", "))
    }
}
