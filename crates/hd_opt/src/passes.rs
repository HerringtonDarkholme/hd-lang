//! The pass manager (tiering.md §6): pass declarations, pipelines as data,
//! and the startup validator. Body-unit passes (opt-pipeline.md §2.1) run
//! per generic body and feed collection through `crate::planner`.
//! The other named passes are declared; their bodies report not
//! implemented, except `liveness`, which suspension lowering owns.

use hd_base::{DefId, Fuel, NotImplemented, StableHasher, Stage, StageResult};
use hd_mono::ProgramEnv;
use hd_mono::plan::Source;
use hd_tir::ir::{Body, Tag};

use crate::value_flow::ValueFlow;

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
    ValueFlow,
    KnownCallees,
}

/// What a pass sees: one instance.
pub struct InstanceCx {
    pub gate: u8,
}

/// What a body-unit pass sees: one generic body, before and during
/// collection (opt-pipeline.md §2.1).
pub struct BodyCx<'a> {
    /// The item whose body it is (or for which the compiler generated it).
    pub def: DefId,
    pub body: &'a Body,
    pub env: &'a dyn ProgramEnv,
}

/// The facts body-unit passes write for one body.
#[derive(Default)]
pub struct BodyFacts {
    /// `FactId::ValueFlow`.
    pub value_flow: Option<ValueFlow>,
    /// `FactId::KnownCallees`: per `CallValue`, its callee's source.
    pub known_callees: Vec<(u32, Source)>,
}

/// The summary bits of `body` that body-unit passes gate on: a `Call`
/// sets `HAS_CALL` and a `CallValue` sets `HAS_DYN_CALL`. Every other bit
/// is set, so a gate on it never skips a pass.
#[must_use]
pub fn body_gate(body: &Body) -> u8 {
    let mut g = !(gate::HAS_CALL | gate::HAS_DYN_CALL);
    for t in &body.tags {
        match t {
            Tag::Call => g |= gate::HAS_CALL,
            Tag::CallValue => g |= gate::HAS_DYN_CALL,
            _ => {}
        }
    }
    g
}

pub enum PassOutcome {
    Done,
    Skipped,
}

/// A pass body, by the unit it runs on.
#[derive(Clone, Copy)]
pub enum Run {
    /// Per generic body, before and during collection.
    Body(fn(&BodyCx<'_>, &mut BodyFacts, &mut Fuel) -> StageResult<PassOutcome>),
    /// Per instance, at emission.
    Instance(fn(&mut InstanceCx, &mut Fuel) -> StageResult<PassOutcome>),
}

pub struct PassSpec {
    pub name: &'static str,
    pub version: u32,
    pub level: Level,
    pub reads: &'static [FactId],
    pub writes: &'static [FactId],
    pub gate: u8,
    pub required_by: Requirement,
    pub run: Run,
}

/// A declared pass with no body yet changes nothing (tiering.md §6): it
/// reports `Skipped`, after the one fuel step every pass pays to run.
fn not_yet(_: &mut InstanceCx, fuel: &mut Fuel) -> StageResult<PassOutcome> {
    if fuel.charge(1) {
        Ok(PassOutcome::Skipped)
    } else {
        Err(NotImplemented::new(Stage::Emit, "pass budget"))
    }
}

/// One fuel step per instruction a body-unit pass walks.
fn charge_body(body: &Body, fuel: &mut Fuel) -> StageResult<()> {
    if fuel.charge(body.len() as u64) {
        Ok(())
    } else {
        Err(NotImplemented::new(Stage::Collect, "pass budget"))
    }
}

/// `value_flow`: the body's data flow (`ValueFlow`).
fn value_flow(cx: &BodyCx<'_>, facts: &mut BodyFacts, fuel: &mut Fuel) -> StageResult<PassOutcome> {
    charge_body(cx.body, fuel)?;
    facts.value_flow = Some(ValueFlow::new(cx.body));
    Ok(PassOutcome::Done)
}

/// `devirt_known`, option A1 of closure-calls.md: each `CallValue` whose
/// callee is a closure value made in the call's own sub-body
/// (`ValueFlow::made`) has that instruction as its callee's source.
/// Collection makes the call direct where that instruction is a closure.
fn devirt_known(
    cx: &BodyCx<'_>,
    facts: &mut BodyFacts,
    fuel: &mut Fuel,
) -> StageResult<PassOutcome> {
    let Some(flow) = &facts.value_flow else {
        return Ok(PassOutcome::Skipped);
    };
    charge_body(cx.body, fuel)?;
    let body = cx.body;
    for i in 0..body.len() {
        if body.tags[i] != Tag::CallValue {
            continue;
        }
        if let Some(made) = flow.made(body, body.data[i][0], i) {
            let at = u32::try_from(i).expect("insts");
            let made = u32::try_from(made).expect("insts");
            facts.known_callees.push((at, Source::Made(made)));
        }
    }
    Ok(PassOutcome::Done)
}

fn liveness(cx: &mut InstanceCx, _: &mut Fuel) -> StageResult<PassOutcome> {
    if cx.gate & gate::HAS_SUSPEND == 0 {
        Ok(PassOutcome::Skipped)
    } else {
        Err(NotImplemented::new(Stage::Emit, "suspension liveness"))
    }
}

macro_rules! pass {
    ($name:literal, $level:ident, [$($r:ident),*], [$($w:ident),*], $gate:expr, $req:ident, $unit:ident($run:expr)) => {
        PassSpec {
            name: $name,
            version: 1,
            level: Level::$level,
            reads: &[$(FactId::$r),*],
            writes: &[$(FactId::$w),*],
            gate: $gate,
            required_by: Requirement::$req,
            run: Run::$unit($run),
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
        Instance(not_yet)
    ),
    pass!(
        "inline_plan",
        TirDecision,
        [UseCounts],
        [InlinePlan],
        gate::HAS_CALL,
        Optional,
        Instance(not_yet)
    ),
    pass!(
        "closure_spec",
        TirDecision,
        [UseCounts],
        [ClosureSpec],
        gate::HAS_CLOSURE,
        Optional,
        Instance(not_yet)
    ),
    pass!(
        "value_flow",
        TirAnalysis,
        [],
        [ValueFlow],
        gate::HAS_CALL | gate::HAS_DYN_CALL,
        Optional,
        Body(value_flow)
    ),
    pass!(
        "devirt_known",
        TirDecision,
        [ValueFlow],
        [KnownCallees],
        gate::HAS_DYN_CALL,
        Optional,
        Body(devirt_known)
    ),
    pass!(
        "escape",
        TirAnalysis,
        [InlinePlan],
        [Escape],
        gate::HAS_ALLOC,
        Optional,
        Instance(not_yet)
    ),
    pass!(
        "scalar_replace",
        TirDecision,
        [Escape],
        [],
        gate::HAS_ALLOC,
        Optional,
        Instance(not_yet)
    ),
    pass!(
        "liveness",
        TirAnalysis,
        [],
        [Liveness],
        gate::HAS_SUSPEND,
        WhenGate,
        Instance(liveness)
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
        "value_flow",
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

pub(crate) fn spec(name: &str) -> Option<&'static PassSpec> {
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
    use super::{DEV, InstanceCx, OPTIMIZED, PASSES, PassOutcome, Pipeline, Run, validate};
    use hd_base::Fuel;

    #[test]
    fn declared_passes_without_a_body_are_skipped_not_errors() {
        for p in PASSES.iter().filter(|p| p.name != "liveness") {
            let Run::Instance(run) = p.run else {
                continue;
            };
            let mut cx = InstanceCx { gate: u8::MAX };
            let out = run(&mut cx, &mut Fuel::new(8));
            assert!(matches!(out, Ok(PassOutcome::Skipped)), "{}", p.name);
        }
    }

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

    #[test]
    fn pipeline_hashes_are_pinned() {
        let dev = validate(&DEV).expect("dev validates").0;
        let optimized = validate(&OPTIMIZED).expect("optimized validates").0;
        assert_eq!(
            dev, 237_331_321_808_658_408_736_501_902_528_458_987_524,
            "dev"
        );
        assert_eq!(
            optimized, 22_771_714_890_862_924_033_644_553_952_768_316_268,
            "optimized"
        );
    }
}
