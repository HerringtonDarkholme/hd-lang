//! The pass manager (`docs/compiler/tiering.md` §6): pass declarations, pipelines as data,
//! and the startup validator. Body-unit passes (`crates/hd_opt/DESIGN.md` (optimization pipelines) §2.1) run
//! per generic body and feed collection through `crate::planner`.
//! The other named passes are declared; their bodies report not
//! implemented, except `liveness`, which suspension lowering owns.

use hd_base::{
    DefId, EngineTarget, Fuel, Hash128, NotImplemented, OptLevel, StableHasher, Stage, StageResult,
};
use hd_mono::ProgramEnv;
use hd_mono::plan::Source;
use hd_tir::ir::{Body, Callee, Tag};

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

impl FactId {
    /// Whether body-unit passes write the fact, before and during
    /// collection, which reads it through `hd_mono::plan::BodyPlan`.
    /// Every other fact is an instance fact, written at emission.
    #[must_use]
    pub const fn is_body(self) -> bool {
        match self {
            Self::ValueFlow | Self::KnownCallees | Self::ClosureSpec => true,
            Self::UseCounts | Self::InlinePlan | Self::Liveness | Self::Escape => false,
        }
    }
}

/// A tuning knob a pass declares (`crates/hd_opt/DESIGN.md` (optimization pipelines) §2.1): its name and the
/// inclusive range of values a pipeline may give it.
pub struct Param {
    pub name: &'static str,
    pub min: u32,
    pub max: u32,
}

/// A pipeline's value for one knob of one of its passes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Knob {
    pub pass: &'static str,
    pub name: &'static str,
    pub value: u32,
}

/// What a pass sees: one instance.
pub struct InstanceCx {
    pub gate: u8,
}

/// What a body-unit pass sees: one generic body, before and during
/// collection (`crates/hd_opt/DESIGN.md` (optimization pipelines) §2.1).
pub struct BodyCx<'a> {
    /// The item whose body it is (or for which the compiler generated it).
    pub def: DefId,
    pub body: &'a Body,
    pub env: &'a dyn ProgramEnv,
    /// The running pass's knobs, from the pipeline.
    pub params: &'a [(&'static str, u32)],
}

impl BodyCx<'_> {
    /// The value of the running pass's knob `name`. A validated pipeline
    /// gives every declared knob a value.
    #[must_use]
    pub fn param(&self, name: &str) -> u32 {
        self.params
            .iter()
            .find(|p| p.0 == name)
            .map_or_else(|| panic!("knob `{name}` has no value"), |p| p.1)
    }
}

/// The facts body-unit passes write for one body.
#[derive(Default)]
pub struct BodyFacts {
    /// `FactId::ValueFlow`.
    pub value_flow: Option<ValueFlow>,
    /// `FactId::KnownCallees`: per `CallValue`, its callee's source.
    pub known_callees: Vec<(u32, Source)>,
    /// `FactId::ClosureSpec`.
    pub closure_spec: ClosureSpec,
}

/// What `closure_spec` decides for one body (option A2 of
/// `crates/hd_opt/DESIGN.md` (known closure calls)), in the shape of `hd_mono::plan::BodyPlan`.
#[derive(Default)]
pub struct ClosureSpec {
    /// Per `CallValue` of an item parameter: `Source::Param` of it.
    pub param_calls: Vec<(u32, Source)>,
    /// Per `Call` of an item: each argument's source, where any has one.
    pub spec_args: Vec<(u32, Vec<Option<Source>>)>,
    /// Per `Closure`: the item parameters its sub-body calls, where any.
    pub carry: Vec<(u32, Vec<u32>)>,
    /// This body as a callee: the parameters it calls, itself or in a
    /// closure it creates; `None` when it may not be specialized.
    pub spec_params: Option<Vec<bool>>,
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
    /// The knobs the pass reads; a pipeline holding it gives each a value.
    pub params: &'static [Param],
}

/// A declared pass with no body yet changes nothing (`docs/compiler/tiering.md` §6): it
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

/// `devirt_known`, option A1 of `crates/hd_opt/DESIGN.md` (known closure calls): each `CallValue` whose
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

/// `closure_spec`, option A2 of `crates/hd_opt/DESIGN.md` (known closure calls): a small callee that calls
/// a parameter is worth one instance per closure literal passed for it,
/// in which the parameter's calls are direct.
///
/// - As a callee, the body's `spec_params` are the parameters it calls,
///   itself or in a closure it creates (`ValueFlow::called_under` sub 0).
///   None for a body over the `max_insts` knob (TIR instructions, their
///   sub-bodies included; `crates/hd_opt/DESIGN.md` (known closure calls) §3.1, owner Q2), an intrinsic,
///   a suspending function, or one that calls no parameter.
/// - Per `Call` of an item, each argument's source: a value made in the
///   call's sub-body (`ValueFlow::made`) or a parameter passed on
///   unchanged (`ValueFlow::param`).
/// - Per `Closure`, the parameters its sub-body calls, which a specialized
///   instance carries into it.
/// - Per `CallValue` of a parameter, `Source::Param`, which is direct in an
///   instance specialized on it.
fn closure_spec(
    cx: &BodyCx<'_>,
    facts: &mut BodyFacts,
    fuel: &mut Fuel,
) -> StageResult<PassOutcome> {
    let Some(flow) = &facts.value_flow else {
        return Ok(PassOutcome::Skipped);
    };
    charge_body(cx.body, fuel)?;
    let (body, env, def) = (cx.body, cx.env, cx.def);
    let n = flow.param_count();
    let at = |i: usize| u32::try_from(i).expect("insts");
    let mut spec = ClosureSpec::default();
    let max_insts = cx.param("max_insts") as usize;
    if body.len() <= max_insts && env.intrinsic(def).is_none() && !env.suspends(def) {
        let called: Vec<bool> = (0..n)
            .map(|p| flow.called_under(u32::try_from(p).expect("params"), 0))
            .collect();
        spec.spec_params = called.contains(&true).then_some(called);
    }
    for i in 0..body.len() {
        let [a, b] = body.data[i];
        match body.tags[i] {
            Tag::CallValue => {
                if let Some(p) = flow.param(body, a) {
                    spec.param_calls.push((at(i), Source::Param(p)));
                }
            }
            Tag::Call
                if matches!(
                    Callee::from_words(body.record(a)),
                    Some(Callee::Item { .. })
                ) =>
            {
                let rec = body.record(b);
                let sources: Vec<Option<Source>> = rec[..rec.len().saturating_sub(3)]
                    .iter()
                    .map(|&w| match flow.made(body, w, i) {
                        Some(m) => Some(Source::Made(at(m))),
                        None => flow.param(body, w).map(Source::Param),
                    })
                    .collect();
                if sources.iter().any(Option::is_some) {
                    spec.spec_args.push((at(i), sources));
                }
            }
            Tag::Closure => {
                let listed: Vec<u32> = (0..n)
                    .map(|p| u32::try_from(p).expect("params"))
                    .filter(|&p| flow.called_under(p, a))
                    .collect();
                if !listed.is_empty() {
                    spec.carry.push((at(i), listed));
                }
            }
            _ => {}
        }
    }
    facts.closure_spec = spec;
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
        pass!($name, $level, [$($r),*], [$($w),*], $gate, $req, $unit($run), [])
    };
    ($name:literal, $level:ident, [$($r:ident),*], [$($w:ident),*], $gate:expr, $req:ident, $unit:ident($run:expr), [$($p:ident: $min:literal..=$max:literal),*]) => {
        PassSpec {
            name: $name,
            version: 1,
            level: Level::$level,
            reads: &[$(FactId::$r),*],
            writes: &[$(FactId::$w),*],
            gate: $gate,
            required_by: Requirement::$req,
            run: Run::$unit($run),
            params: &[$(Param { name: stringify!($p), min: $min, max: $max }),*],
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
        "closure_spec",
        TirDecision,
        [ValueFlow],
        [ClosureSpec],
        gate::HAS_CALL | gate::HAS_DYN_CALL,
        Optional,
        Body(closure_spec),
        [max_insts: 1..=4096, max_depth: 1..=8]
    ),
    // The release summaries (`docs/future-work/RELEASE_INTERPROC.md`):
    // the driver owns bodies callees first and records what each body's
    // code says of its parameters when its pipeline holds this pass.
    pass!(
        "summaries",
        TirAnalysis,
        [],
        [],
        0,
        Optional,
        Instance(not_yet)
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

/// A pipeline (§6.3): body and instance passes in order, link passes per
/// program, and the values of their passes' knobs.
pub struct Pipeline {
    /// A label for messages; not content, so never hashed.
    pub name: &'static str,
    pub tir: &'static [&'static str],
    pub link: &'static [&'static str],
    pub knobs: &'static [Knob],
}

impl Pipeline {
    /// The value this pipeline gives knob `name` of pass `pass`.
    #[must_use]
    pub fn knob(&self, pass: &str, name: &str) -> Option<u32> {
        self.knobs
            .iter()
            .find(|k| k.pass == pass && k.name == name)
            .map(|k| k.value)
    }

    /// Whether `self` and `other` hold the same passes in the same order
    /// with the same knob values: what the pipeline hash covers.
    #[must_use]
    pub fn same_content(&self, other: &Self) -> bool {
        self.tir == other.tir
            && self.link == other.link
            && self.tir.iter().filter_map(|n| spec(n)).all(|s| {
                s.params
                    .iter()
                    .all(|p| self.knob(s.name, p.name) == other.knob(s.name, p.name))
            })
    }
}

pub static DEV: Pipeline = Pipeline {
    name: "dev",
    tir: &["liveness"],
    link: &["fold"],
    knobs: &[],
};
pub static OPTIMIZED: Pipeline = Pipeline {
    name: "optimized",
    tir: &[
        "value_flow",
        "use_counts",
        "inline_plan",
        "devirt_known",
        "closure_spec",
        "summaries",
        "escape",
        "scalar_replace",
        "liveness",
    ],
    link: &["drop_dead", "fold", "leb_compact", "compact_names"],
    // `closure_spec`'s bounds (`crates/hd_opt/DESIGN.md` (known closure calls) §3.1): bodies of at most
    // 64 TIR instructions, and a specialized instance requested from
    // inside a specialized instance is the last. Measured per target in
    // job (e) of #162 (`crates/hd_opt/DESIGN.md` (optimization pipelines) §8): 128/2 emits
    // the same runtime-bench Wasm as 64/2, while 32/1 and no
    // `closure_spec` lose most of A2's gain on V8 and wasmtime alike, so
    // every target shares these values.
    knobs: &[
        Knob {
            pass: "closure_spec",
            name: "max_insts",
            value: 64,
        },
        Knob {
            pass: "closure_spec",
            name: "max_depth",
            value: 2,
        },
    ],
};

/// [`DEV`] for wasmtime: the link also inlines the count fast paths
/// (`inline_counts`, `crates/hd_wasm_linear/DESIGN.md` §23.30), which
/// Cranelift does not inline on its own; V8 does, and runs the inline form
/// slower (#644).
pub static DEV_WASMTIME: Pipeline = Pipeline {
    name: "dev",
    tir: DEV.tir,
    link: &["fold", "inline_counts"],
    knobs: DEV.knobs,
};
/// [`OPTIMIZED`] for wasmtime, with `inline_counts` as [`DEV_WASMTIME`].
pub static OPTIMIZED_WASMTIME: Pipeline = Pipeline {
    name: "optimized",
    tir: OPTIMIZED.tir,
    link: &[
        "drop_dead",
        "fold",
        "leb_compact",
        "compact_names",
        "inline_counts",
    ],
    knobs: OPTIMIZED.knobs,
};

/// The pipeline of each (target, level) (`crates/hd_opt/DESIGN.md` (optimization pipelines) §3.1). The match
/// is total with no wildcard, so a new target or level does not compile
/// until it has a row (validator check 1). Every target has the same TIR
/// passes and knobs: job (e) of #162 measured `closure_spec`'s knobs on V8
/// and wasmtime and found one best value for both. Wasmtime's link also
/// inlines the count fast paths (#644).
#[must_use]
pub const fn pipeline(target: EngineTarget, level: OptLevel) -> &'static Pipeline {
    match (target, level) {
        (EngineTarget::V8 | EngineTarget::Browser, OptLevel::Dev) => &DEV,
        (EngineTarget::V8 | EngineTarget::Browser, OptLevel::Optimized) => &OPTIMIZED,
        (EngineTarget::Wasmtime, OptLevel::Dev) => &DEV_WASMTIME,
        (EngineTarget::Wasmtime, OptLevel::Optimized) => &OPTIMIZED_WASMTIME,
    }
}

pub(crate) fn spec(name: &str) -> Option<&'static PassSpec> {
    PASSES.iter().find(|p| p.name == name)
}

/// Validator check 3: every knob belongs to a pass of the pipeline that
/// declares it, lies within the declared range, and every declared knob of
/// every pass present has exactly one value.
fn check_knobs(p: &Pipeline) -> Result<(), String> {
    for k in p.knobs {
        if !p.tir.contains(&k.pass) {
            return Err(format!("knob `{}.{}` for an absent pass", k.pass, k.name));
        }
        let s = spec(k.pass).ok_or_else(|| format!("unknown pass `{}`", k.pass))?;
        let param = s
            .params
            .iter()
            .find(|d| d.name == k.name)
            .ok_or_else(|| format!("pass `{}` declares no knob `{}`", k.pass, k.name))?;
        if !(param.min..=param.max).contains(&k.value) {
            return Err(format!(
                "knob `{}.{}` = {} is outside {}..={}",
                k.pass, k.name, k.value, param.min, param.max
            ));
        }
    }
    for s in p.tir.iter().filter_map(|n| spec(n)) {
        for d in s.params {
            let n = p
                .knobs
                .iter()
                .filter(|k| k.pass == s.name && k.name == d.name)
                .count();
            if n != 1 {
                return Err(format!(
                    "knob `{}.{}` has {n} values; it needs one",
                    s.name, d.name
                ));
            }
        }
    }
    Ok(())
}

/// The startup validator (§6.2, `crates/hd_opt/DESIGN.md` (optimization pipelines) §3.2): each read is
/// produced earlier, every required pass is present, a body-unit pass
/// reads only body facts and only body-unit passes write them (check 2),
/// and the knobs are complete and in range (check 3). Returns the pass
/// hash on success (`crates/hd_opt/DESIGN.md` (optimization pipelines) §3.3): each pass's name, version and
/// knob values in order, then the link passes. The pipeline's name and
/// the target it serves are not hashed, only content.
pub fn validate(p: &Pipeline) -> Result<Hash128, String> {
    let mut produced: Vec<FactId> = Vec::new();
    let mut h = StableHasher::new("pipeline");
    h.u32(u32::try_from(p.tir.len()).map_err(|_| "too many passes")?);
    for name in p.tir {
        let s = spec(name).ok_or_else(|| format!("unknown pass `{name}`"))?;
        if let Some(missing) = s.reads.iter().find(|r| !produced.contains(r)) {
            return Err(format!(
                "pass `{name}` reads {missing:?} before any pass writes it"
            ));
        }
        let body = matches!(s.run, Run::Body(_));
        if body && let Some(f) = s.reads.iter().find(|f| !f.is_body()) {
            return Err(format!("body pass `{name}` reads the instance fact {f:?}"));
        }
        if !body && let Some(f) = s.writes.iter().find(|f| f.is_body()) {
            return Err(format!("instance pass `{name}` writes the body fact {f:?}"));
        }
        produced.extend_from_slice(s.writes);
        h.str(s.name);
        h.u32(s.version);
        for d in s.params {
            h.str(d.name);
            h.u32(p.knob(s.name, d.name).unwrap_or_default());
        }
    }
    for s in PASSES
        .iter()
        .filter(|s| s.required_by != Requirement::Optional)
    {
        if !p.tir.contains(&s.name) {
            return Err(format!("required pass `{}` missing", s.name));
        }
    }
    check_knobs(p)?;
    h.u32(u32::try_from(p.link.len()).map_err(|_| "too many link passes")?);
    for name in p.link {
        h.str(name);
    }
    Ok(h.finish())
}

#[cfg(test)]
mod tests {
    use super::{
        DEV, InstanceCx, Knob, OPTIMIZED, PASSES, PassOutcome, Pipeline, Run, pipeline, validate,
    };
    use hd_base::{EngineTarget, Fuel, OptLevel};

    /// `OPTIMIZED` with other knob values.
    fn optimized_with(knobs: &'static [Knob]) -> Pipeline {
        Pipeline {
            name: "optimized",
            tir: OPTIMIZED.tir,
            link: OPTIMIZED.link,
            knobs,
        }
    }

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
            knobs: &[],
        };
        assert!(validate(&bad).is_err());
        let missing = Pipeline {
            name: "missing",
            tir: &[],
            link: &[],
            knobs: &[],
        };
        assert!(validate(&missing).is_err());
    }

    /// Check 2: an instance fact is no input to a body pass.
    #[test]
    fn a_body_pass_reading_an_instance_fact_fails() {
        for p in PASSES.iter().filter(|p| matches!(p.run, Run::Body(_))) {
            assert!(p.reads.iter().all(|f| f.is_body()), "{}", p.name);
        }
        for p in PASSES.iter().filter(|p| matches!(p.run, Run::Instance(_))) {
            assert!(p.writes.iter().all(|f| !f.is_body()), "{}", p.name);
        }
    }

    /// Check 3: knobs are complete, known and in range.
    #[test]
    fn knobs_out_of_range_missing_or_unknown_fail() {
        let zero_depth = optimized_with(&[
            Knob {
                pass: "closure_spec",
                name: "max_insts",
                value: 64,
            },
            Knob {
                pass: "closure_spec",
                name: "max_depth",
                value: 0,
            },
        ]);
        let err = validate(&zero_depth).expect_err("max_depth 0");
        assert!(err.contains("outside"), "{err}");
        let missing = optimized_with(&[Knob {
            pass: "closure_spec",
            name: "max_insts",
            value: 64,
        }]);
        let err = validate(&missing).expect_err("no max_depth");
        assert!(err.contains("needs one"), "{err}");
        let unknown = Pipeline {
            name: "dev",
            tir: DEV.tir,
            link: DEV.link,
            knobs: &[Knob {
                pass: "liveness",
                name: "max_insts",
                value: 1,
            }],
        };
        let err = validate(&unknown).expect_err("liveness has no knob");
        assert!(err.contains("declares no knob"), "{err}");
        let absent = Pipeline {
            name: "dev",
            tir: DEV.tir,
            link: DEV.link,
            knobs: &[Knob {
                pass: "closure_spec",
                name: "max_insts",
                value: 64,
            }],
        };
        let err = validate(&absent).expect_err("closure_spec is absent");
        assert!(err.contains("absent pass"), "{err}");
    }

    /// The pass hash is content: a knob value changes it, the name does not.
    #[test]
    fn the_pass_hash_covers_knobs_not_the_name() {
        let renamed = Pipeline {
            name: "other",
            ..optimized_with(OPTIMIZED.knobs)
        };
        assert!(renamed.same_content(&OPTIMIZED));
        assert_eq!(validate(&renamed), validate(&OPTIMIZED));
        let wider = optimized_with(&[
            Knob {
                pass: "closure_spec",
                name: "max_insts",
                value: 128,
            },
            Knob {
                pass: "closure_spec",
                name: "max_depth",
                value: 2,
            },
        ]);
        assert!(!wider.same_content(&OPTIMIZED));
        assert_ne!(validate(&wider), validate(&OPTIMIZED));
    }

    #[test]
    fn every_target_has_a_valid_row_per_level() {
        for target in EngineTarget::ALL {
            assert_eq!(pipeline(target, OptLevel::Dev).name, DEV.name);
            assert_eq!(pipeline(target, OptLevel::Optimized).name, OPTIMIZED.name);
        }
    }

    /// A change to these values changes every code key. Job (c1) changed
    /// both: the hash dropped the pipeline name and gained the knob values
    /// and the link passes; the release summaries changed the optimized
    /// one, and only it.
    #[test]
    fn pipeline_hashes_are_pinned() {
        let dev = validate(&DEV).expect("dev validates").0;
        let optimized = validate(&OPTIMIZED).expect("optimized validates").0;
        assert_eq!(
            dev, 226_259_624_280_722_390_681_558_611_773_950_027_154,
            "dev"
        );
        assert_eq!(
            optimized, 197_633_577_451_581_921_990_823_630_658_444_050_384,
            "optimized"
        );
    }
}
