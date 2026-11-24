//! Structured conditional emission with explicit continuations. Literal-match
//! decision chains may contain one If per arm; they consume heap work entries,
//! rather than one native `inst/block_into` call frame per arm.

use super::{Ctl, Em, Instruction, NONE, Reason, Tag};
use hd_base::StageResult;
use wasm_encoder::BlockType;

/// Each open target's current absolute position. Entries remember shadowed
/// positions, so popping restores a prior target without searching the stack.
#[derive(Default)]
pub struct Stack {
    entries: Vec<(Ctl, Option<usize>)>,
    targets: hd_base::HashMap<Ctl, usize>,
    plain: Option<usize>,
}

impl Stack {
    #[must_use]
    #[cfg_attr(
        feature = "testing",
        expect(
            clippy::len_without_is_empty,
            reason = "no caller asks whether it is empty"
        )
    )]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn push(&mut self, target: Ctl) {
        crate::work::control(1);
        let at = self.entries.len();
        let prior = if target == Ctl::Plain {
            self.plain.replace(at)
        } else {
            self.targets.insert(target, at)
        };
        self.entries.push((target, prior));
    }

    pub fn pop(&mut self) -> Option<Ctl> {
        crate::work::control(1);
        let (target, prior) = self.entries.pop()?;
        if target == Ctl::Plain {
            self.plain = prior;
        } else if let Some(prior) = prior {
            self.targets.insert(target, prior);
        } else {
            self.targets.remove(&target);
        }
        Some(target)
    }

    #[must_use]
    pub fn position(&self, target: Ctl) -> Option<usize> {
        crate::work::control(1);
        if target == Ctl::Plain {
            self.plain
        } else {
            self.targets.get(&target).copied()
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct InstState {
    pub(super) at: Option<u32>,
    pub(super) origin_at: Option<u32>,
    pub(super) reason: Option<Reason>,
}

pub(super) enum Task {
    Block(u32, Option<u32>),
    EndBlock {
        blk: u32,
        dest: Option<u32>,
        tail: u32,
        labelled: bool,
    },
    Inst(u32),
    If(u32),
    Else(u32, u32),
    Close,
    EndIfInst(u32, InstState),
}

impl Em<'_, '_, '_> {
    pub(super) fn control(&mut self, first: Task) -> StageResult<()> {
        let mut work = vec![first];
        while let Some(task) = work.pop() {
            crate::work::control(1);
            match task {
                Task::Block(blk, dest) => {
                    // ruled out: block_into and conditional scheduling filter
                    // absent blocks before constructing a Block task.
                    assert_eq!(self.b.tags[blk as usize], Tag::Block);
                    let [list, mut tail] = self.b.data[blk as usize];
                    let labelled = self.topo.label_of[blk as usize] != NONE;
                    if labelled {
                        self.open(Ctl::Brk(blk));
                    }
                    let mut items = self.items(list);
                    if let Some(k) = self.dead_from(&items, tail) {
                        tail = items[k];
                        items.truncate(k + 1);
                    }
                    work.push(Task::EndBlock {
                        blk,
                        dest,
                        tail,
                        labelled,
                    });
                    if self.range(blk).is_some() {
                        self.resume_list(&items)?;
                    } else {
                        work.extend(items.into_iter().rev().map(Task::Inst));
                    }
                }
                Task::EndBlock {
                    blk,
                    dest,
                    tail,
                    labelled,
                } => {
                    self.end_block(blk, dest, tail, labelled)?;
                }
                Task::Inst(i) => {
                    if self.b.tags[i as usize] != Tag::If {
                        self.inst(i)?;
                        continue;
                    }
                    let state = self.begin_inst(i)?;
                    // count::read_after only names a Read. An If never consumes
                    // that cached place walk; inst's ordinary fallback discards it.
                    self.walked = None;
                    self.borrow_access(i)?;
                    work.push(Task::EndIfInst(i, state));
                    work.push(Task::If(i));
                }
                Task::If(i) => {
                    let [a, bw] = self.b.data[i as usize];
                    let r = self.rec(bw);
                    self.slot(i)?;
                    // Known at link time: only the arm that runs.
                    if let Some(owned) = self.param_owned(a) {
                        let arm = r.get(usize::from(!owned)).copied();
                        work.extend(arm.filter(|&x| x != NONE).map(|x| Task::Block(x, Some(i))));
                        continue;
                    }
                    self.push(a)?;
                    self.resume_branch(i, r[0]);
                    self.f.op(Instruction::If(BlockType::Empty));
                    self.ctl.push(Ctl::Plain);
                    work.push(Task::Close);
                    if let Some(&els) = r.get(1).filter(|e| **e != NONE) {
                        work.push(Task::Else(i, els));
                    }
                    if r[0] != NONE {
                        work.push(Task::Block(r[0], Some(i)));
                    }
                }
                Task::Else(i, blk) => {
                    self.f.op(Instruction::Else);
                    work.push(Task::Block(blk, Some(i)));
                }
                Task::Close => self.close(),
                Task::EndIfInst(i, state) => {
                    self.end_inst(i, state, Ok(()))?;
                }
            }
        }
        Ok(())
    }
}
