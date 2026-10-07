//! The pass manager (tiering.md §6): pass declarations, pipelines as data,
//! and the startup validator. The named passes are declared; their bodies
//! report not implemented, except `liveness`, which suspension lowering owns.

use hd_base::{Fuel, NotImplemented, StableHasher, Stage, StageResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    TirAnalysis,
    TirDecision,
    WasmModule,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Requirement {
    Always,
    WhenGate,
    Optional,
}

/// Summary bits from the `tir` entry (§6.2).
pub mod gate {
    pub const HAS_LOOP: u8 = 1;
    pub const HAS_CLOSURE: u8 = 2;
    pub const HAS_ALLOC: u8 = 4;
    pub const HAS_SUSPEND: u8 = 8;
    pub const HAS_DYN_CALL: u8 = 16;
    pub const HAS_CALL: u8 = 32;
}

/// Facts passes read and write (side tables).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FactId {
    UseCounts,
    InlinePlan,
    Liveness,
    Escape,
    ClosureSpec,
    KnownVtables,
}

/// What a pass sees: one instance.
pub struct InstanceCx {
    pub gate: u8,
}

pub enum PassOutcome {
    Done,
    Skipped,
}

pub struct PassSpec {
    pub name: &'static str,
    pub version: u32,
    pub level: Level,
    pub reads: &'static [FactId],
    pub writes: &'static [FactId],
    pub gate: u8,
    pub required_by: Requirement,
    pub run: fn(&mut InstanceCx, &mut Fuel) -> StageResult<PassOutcome>,
}

fn not_yet(_: &mut InstanceCx, _: &mut Fuel) -> StageResult<PassOutcome> {
    Err(NotImplemented::new(Stage::Emit, "optimizing TIR pass"))
}

fn liveness(cx: &mut InstanceCx, _: &mut Fuel) -> StageResult<PassOutcome> {
    if cx.gate & gate::HAS_SUSPEND == 0 {
        Ok(PassOutcome::Skipped)
    } else {
        Err(NotImplemented::new(Stage::Emit, "suspension liveness"))
    }
}

macro_rules! pass {
    ($name:literal, $level:ident, [$($r:ident),*], [$($w:ident),*], $gate:expr, $req:ident, $run:expr) => {
        PassSpec {
            name: $name,
            version: 1,
            level: Level::$level,
            reads: &[$(FactId::$r),*],
            writes: &[$(FactId::$w),*],
            gate: $gate,
            required_by: Requirement::$req,
            run: $run,
        }
    };
}

pub static PASSES: &[PassSpec] = &[
    pass!(
        "use_counts",
        TirAnalysis,
        [],
        [UseCounts],
        0,
        Optional,
        not_yet
    ),
    pass!(
        "inline_plan",
        TirDecision,
        [UseCounts],
        [InlinePlan],
        gate::HAS_CALL,
        Optional,
        not_yet
    ),
    pass!(
        "closure_spec",
        TirDecision,
        [UseCounts],
        [ClosureSpec],
        gate::HAS_CLOSURE,
        Optional,
        not_yet
    ),
    pass!(
        "devirt_known",
        TirDecision,
        [],
        [KnownVtables],
        gate::HAS_DYN_CALL,
        Optional,
        not_yet
    ),
    pass!(
        "escape",
        TirAnalysis,
        [InlinePlan],
        [Escape],
        gate::HAS_ALLOC,
        Optional,
        not_yet
    ),
    pass!(
        "scalar_replace",
        TirDecision,
        [Escape],
        [],
        gate::HAS_ALLOC,
        Optional,
        not_yet
    ),
    pass!(
        "liveness",
        TirAnalysis,
        [],
        [Liveness],
        gate::HAS_SUSPEND,
        WhenGate,
        liveness
    ),
];

/// A pipeline (§6.3): TIR passes per instance, link passes per program.
pub struct Pipeline {
    pub name: &'static str,
    pub tir: &'static [&'static str],
    pub link: &'static [&'static str],
}

pub static DEV: Pipeline = Pipeline {
    name: "dev",
    tir: &["liveness"],
    link: &["fold"],
};
pub static OPTIMIZED: Pipeline = Pipeline {
    name: "optimized",
    tir: &[
        "use_counts",
        "inline_plan",
        "closure_spec",
        "devirt_known",
        "escape",
        "scalar_replace",
        "liveness",
    ],
    link: &["drop_dead", "fold", "leb_compact", "compact_names"],
};

fn spec(name: &str) -> Option<&'static PassSpec> {
    PASSES.iter().find(|p| p.name == name)
}

/// The startup validator (§6.2): each read is produced earlier, and every
/// required pass is present. Returns the pipeline hash on success.
pub fn validate(p: &Pipeline) -> Result<hd_base::Hash128, String> {
    let mut produced: Vec<FactId> = Vec::new();
    let mut h = StableHasher::new("pipeline");
    h.str(p.name);
    for name in p.tir {
        let s = spec(name).ok_or_else(|| format!("unknown pass `{name}`"))?;
        if let Some(missing) = s.reads.iter().find(|r| !produced.contains(r)) {
            return Err(format!(
                "pass `{name}` reads {missing:?} before any pass writes it"
            ));
        }
        produced.extend_from_slice(s.writes);
        h.str(s.name);
        h.u32(s.version);
    }
    for s in PASSES
        .iter()
        .filter(|s| s.required_by != Requirement::Optional)
    {
        if !p.tir.contains(&s.name) {
            return Err(format!("required pass `{}` missing", s.name));
        }
    }
    Ok(h.finish())
}

#[cfg(test)]
mod tests {
    use super::{DEV, OPTIMIZED, Pipeline, validate};

    #[test]
    fn both_pipelines_validate_and_a_bad_order_fails() {
        assert!(validate(&DEV).is_ok());
        assert!(validate(&OPTIMIZED).is_ok());
        assert_ne!(validate(&DEV), validate(&OPTIMIZED));
        let bad = Pipeline {
            name: "bad",
            tir: &["escape", "liveness"],
            link: &[],
        };
        assert!(validate(&bad).is_err());
        let missing = Pipeline {
            name: "missing",
            tir: &[],
            link: &[],
        };
        assert!(validate(&missing).is_err());
    }
}
