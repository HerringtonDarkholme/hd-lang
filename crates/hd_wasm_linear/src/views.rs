//! Opt-in development views use the same layout and cycle analysis as emission.

use hd_base::SliceSort as _;
use hd_base::StageResult;
use hd_mono::{ProgramEnv, printed_type_name, subst};
use hd_types::{InternPool, Ty, TyData};

use crate::layout::{self, Walk};

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

fn counted(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    ty: Ty,
    base: u32,
) -> StageResult<Vec<CountedLeaf>> {
    fn walk(w: &Walk) -> std::collections::BTreeMap<u32, bool> {
        use std::collections::BTreeMap;
        match w {
            Walk::Ref(k) | Walk::Known(k) => BTreeMap::from([(*k, false)]),
            Walk::Glue => BTreeMap::new(),
            Walk::Seq(ws) => {
                let mut out = BTreeMap::new();
                for w in ws {
                    for (k, conditional) in walk(w) {
                        out.entry(k)
                            .and_modify(|c| *c &= conditional)
                            .or_insert(conditional);
                    }
                }
                out
            }
            Walk::Switch(_, arms) => {
                let arms: Vec<_> = arms
                    .iter()
                    .map(|w| w.as_ref().map(walk).unwrap_or_default())
                    .collect();
                let mut out = BTreeMap::new();
                for k in arms.iter().flat_map(|a| a.keys()) {
                    out.insert(*k, arms.iter().any(|a| a.get(k).copied().unwrap_or(true)));
                }
                out
            }
        }
    }
    let mem = layout::mem(pool, env, ty)?;
    let refs = layout::walk(pool, env, ty)
        .as_ref()
        .map(walk)
        .unwrap_or_default();
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
        let TyData::Adt { def, args } = pool.get(layout::strip(pool, env, ty)) else {
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
    /// A count of a collection through its copy glue (`rt:coll_copy`).
    Copy,
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
            Self::Copy => "copy",
            Self::Release => "release",
            Self::Free => "free",
            Self::Unique => "unique",
        }
    }
}

pub(crate) fn count_action(
    s: &crate::module::Sym,
    runtime: Option<&crate::Runtime>,
) -> Option<CountAction> {
    use crate::{Helper, module::Sym};
    use CountAction::{Copy, Free, Release, Retain, Unique};
    match s {
        Sym::Helper(Helper::CollCopy) => Some(Copy),
        Sym::Helper(Helper::ListUnique { .. } | Helper::MapUnique { .. }) => Some(Unique),
        Sym::Helper(
            Helper::Retain { .. }
            | Helper::RetainKnown
            | Helper::RetainFields { .. }
            | Helper::RetainAny,
        ) => Some(Retain),
        Sym::Helper(
            Helper::Release { .. }
            | Helper::ReleaseRef
            | Helper::ReleaseKnown
            | Helper::TakeBox
            | Helper::DropFields { .. }
            | Helper::DropValue { .. }
            | Helper::CycleRelease,
        ) => Some(Release),
        Sym::Helper(Helper::CycleFree) => Some(Free),
        Sym::Inst(k) => {
            let r = runtime?;
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
        _ => None,
    }
}

/// Extra source-map tokens live after the body's real TIR instruction range.
/// Rewriters already copy these opaque origins; final maps resolve the token
/// back to the original TIR instruction. Count labels never enter Wasm.
#[derive(Clone, Debug)]
pub(crate) struct CountOrigin {
    pub origin: u32,
    pub site: u32,
    pub action: CountAction,
    pub subject: String,
    pub reason: hd_tir::own::Reason,
}

/// Name the counted parts through the same value paths used by WAT locals.
pub(crate) fn count_subject(pool: &InternPool, env: &dyn ProgramEnv, ty: Ty) -> String {
    let name = printed_type_name(pool, env, ty);
    let aggregate = match pool.get(layout::strip(pool, env, ty)) {
        TyData::Tuple { .. } => true,
        TyData::Adt { def, .. } => {
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

#[cfg(test)]
mod tests {
    use super::CountAction;
    use crate::module::{Module, Sym, Val};
    use hd_base::Hash128;

    /// The runtime module is intentionally std-only in hd source. Exercise a
    /// direct free at the real backend encoder, including acyclic relinking.
    #[test]
    fn a_final_free_call_keeps_its_source_and_block_label() {
        let keys: Vec<_> = (1..=crate::runtime_items(false).len())
            .map(|n| Some(Hash128(n as u128)))
            .collect();
        let runtime = crate::Runtime::from_keys(&keys).expect("runtime keys");
        let mut module = Module::default();
        module.runtime = Some(runtime);
        module.track_origins = true;
        let free = module.func("free", vec![Val::I32], vec![]);
        module.define(Sym::Inst(runtime.free), free);
        let mut caller = module.func("app/free_block", vec![Val::I32], vec![]);
        caller.origins = Some(Vec::new());
        caller.origin_at = Some(0);
        caller.origin_root = Some(0);
        caller.origin_item = Some("app/free_block".into());
        caller.get(0);
        caller.call(Sym::Inst(runtime.free));
        let key = Sym::Inst(Hash128(100));
        module.define(key.clone(), caller);
        module.export("free_block", key);
        let mapped = module.finish_with_origins().expect("encode");
        wasmparser::Validator::new()
            .validate_all(&mapped.wasm)
            .expect("valid module");
        let function = mapped
            .functions
            .iter()
            .find(|f| f.name == "app/free_block")
            .expect("caller");
        assert_eq!(function.counts.len(), 1);
        let count = &function.counts[0];
        assert_eq!(count.action, CountAction::Free);
        assert_eq!(count.subject, "block");
        assert_eq!(count.origin, Some(0));
        assert!(
            function
                .instructions
                .iter()
                .any(|i| i.offset == count.offset)
        );
    }
}
