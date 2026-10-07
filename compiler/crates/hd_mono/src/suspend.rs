//! Suspension lowering's state machine types (suspension.md §14.1 to §14.6)
//! and the liveness analysis that feeds them (§14.2 step 1).
//!
//! Emission builds the state machine from TIR's explicit suspension
//! points; TIR is not changed. These are the analysis results and the plan
//! the emitter follows.

use hd_base::{NotImplemented, Stage, StageResult};
use hd_tir::ir::{Body, Tag};

/// The functions one suspending body lowers to (§14.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SuspendFn {
    /// The cold constructor: allocates nothing until first poll needs a frame.
    Ctor,
    /// The poll function: resumes from `frame.state`.
    Poll,
    /// Cancellation: runs the `defer` suites of the scopes live at the state.
    Cancel,
}

/// One suspension point (§14.2): its instruction, resume case and saved slots.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SuspendPoint {
    pub inst: u32,
    pub state: u32,
    pub resume_case: u32,
    /// Locals and values live across the point, plus locals the enclosing
    /// scopes' `defer` suites read: the frame fields it saves.
    pub saved: Vec<FrameSlot>,
    /// The enclosing scopes, innermost first (TIR's side record).
    pub scopes: Vec<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FrameSlot {
    Local(u32),
    Value(u32),
}

/// The frame struct: `state` plus one field per slot saved anywhere (§14.3).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct FrameLayout {
    pub fields: Vec<FrameSlot>,
}

/// Frame states (§14.2, §14.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameState {
    Start,
    Suspended(u32),
    Done,
    Cancelled,
}

/// Results of a poll at the ABI (§14.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PollResult {
    Pending,
    Ready,
}

/// The plan for one suspending body: points, frame and the flattened
/// blocks (blocks containing a point become one dispatch loop, §14.2 step 2).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct StateMachine {
    pub points: Vec<SuspendPoint>,
    pub frame: FrameLayout,
    pub flattened_blocks: Vec<u32>,
}

/// Kinds of join (§14.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinKind {
    All,
    Race,
}

fn is_point(t: Tag) -> bool {
    matches!(
        t,
        Tag::Await | Tag::AwaitValue | Tag::AwaitAll | Tag::AwaitRace
    )
}

/// Finds the suspension points and the blocks to flatten. Liveness (the
/// saved slots) is the backward dataflow of §14.2, not implemented yet.
pub fn plan_state_machine(body: &Body) -> StageResult<StateMachine> {
    let mut sm = StateMachine::default();
    for (i, t) in body.tags.iter().enumerate() {
        if is_point(*t) {
            let i = u32::try_from(i).expect("inst");
            let state = u32::try_from(sm.points.len()).expect("points") + 1;
            sm.points.push(SuspendPoint {
                inst: i,
                state,
                resume_case: state,
                saved: vec![],
                scopes: vec![],
            });
        }
    }
    if sm.points.is_empty() {
        return Ok(sm);
    }
    for (b, t) in body.tags.iter().enumerate() {
        if *t == Tag::Block {
            let list = body.record(body.data[b][0]);
            if list.iter().any(|&c| is_point(body.tags[c as usize])) {
                sm.flattened_blocks.push(u32::try_from(b).expect("block"));
            }
        }
    }
    Err(NotImplemented::new(
        Stage::Emit,
        "suspension liveness and frame layout",
    ))
}

#[cfg(test)]
mod tests {
    use super::plan_state_machine;
    use hd_base::{DefId, NodeIdx};
    use hd_tir::ir::{BodyKind, Ref, Tag, TirBuilder, TirSink};
    use hd_types::Ty;

    #[test]
    fn a_body_without_points_needs_no_machine() {
        let mut b = TirBuilder::new(DefId::from_raw(0), BodyKind::Fn);
        let blk = b.open_block();
        let r = b.close_block(blk, Some(Ref::konst(0)), Ty::I32, NodeIdx::NONE);
        let body = b.finish(r, &[]).expect("body");
        assert!(plan_state_machine(&body).expect("plan").points.is_empty());
        let mut b = TirBuilder::new(DefId::from_raw(0), BodyKind::Fn);
        let blk = b.open_block();
        let v = b.emit(
            Tag::Unreachable,
            u32::MAX,
            u32::MAX,
            Ty::NEVER,
            NodeIdx::NONE,
        );
        b.emit(Tag::AwaitValue, v.0, u32::MAX, Ty::I32, NodeIdx::NONE);
        let r = b.close_block(blk, None, Ty::VOID, NodeIdx::NONE);
        let body = b.finish(r, &[]).expect("body");
        assert!(
            plan_state_machine(&body).is_err(),
            "liveness is reported as not implemented"
        );
    }
}
