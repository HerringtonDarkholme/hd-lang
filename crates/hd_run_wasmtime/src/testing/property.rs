//! Deterministic property plans and bounded choice-stream shrinking.

use std::collections::VecDeque;

use crate::abi::PropertyCase;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanKind {
    Example,
    Saved,
    Fresh,
    Shrink,
}

#[derive(Clone, Debug)]
pub struct Plan {
    pub example: Option<u32>,
    pub seed: i64,
    pub size: i64,
    pub replay: Vec<i64>,
    pub kind: PlanKind,
}

impl Plan {
    /// The `PropertyCase` `PropertyRunner.start` returns. A size is at most
    /// the case count, an `i32`.
    pub(crate) fn case(&self) -> PropertyCase {
        PropertyCase {
            example: self
                .example
                .map(|index| usize::try_from(index).unwrap_or(usize::MAX)),
            seed: self.seed,
            size: i32::try_from(self.size).unwrap_or(i32::MAX),
            replay: self.replay.clone(),
        }
    }
}

pub struct Property {
    pub(crate) cases: u32,
    pub(crate) shrink: u32,
    examples: u32,
    example: u32,
    seed: Option<i64>,
    size: i64,
    passed: u32,
    pub(crate) discarded: u64,
    replays: VecDeque<Vec<i64>>,
    pending: Option<(Vec<i64>, i64)>,
    retry: Option<Plan>,
}

impl Property {
    #[must_use]
    pub fn new(cases: u32, shrink: u32, examples: u32, seed: i64, replays: Vec<Vec<i64>>) -> Self {
        Self {
            cases,
            shrink,
            examples,
            example: 0,
            seed: Some(seed),
            size: 0,
            passed: 0,
            discarded: 0,
            replays: replays.into(),
            pending: None,
            retry: None,
        }
    }

    #[cfg_attr(
        feature = "testing",
        expect(
            clippy::should_implement_trait,
            reason = "fallible, so not an `Iterator`"
        )
    )]
    pub fn next(&mut self) -> Result<Option<Plan>, String> {
        if let Some(plan) = self.retry.take() {
            return Ok(Some(plan));
        }
        if self.pending.is_none() && !self.more() {
            return Ok(None);
        }
        let seed = self
            .seed
            .ok_or("the property seed exceeds the i64 exchange-buffer range")?;
        self.seed = seed.checked_add(1);
        let (example, size, replay, kind) = if let Some((replay, size)) = self.pending.take() {
            (None, size, replay, PlanKind::Shrink)
        } else if self.example < self.examples {
            let example = self.example;
            self.example += 1;
            (Some(example), 0, Vec::new(), PlanKind::Example)
        } else if let Some(replay) = self.replays.pop_front() {
            (None, self.size, replay, PlanKind::Saved)
        } else if self.passed < self.cases {
            (None, self.size, Vec::new(), PlanKind::Fresh)
        } else {
            return Ok(None);
        };
        Ok(Some(Plan {
            example,
            seed,
            size,
            replay,
            kind,
        }))
    }

    #[must_use]
    pub fn more(&self) -> bool {
        self.example < self.examples || !self.replays.is_empty() || self.passed < self.cases
    }

    pub fn pass(&mut self, kind: PlanKind) {
        if kind == PlanKind::Fresh {
            self.passed += 1;
            self.size += 1;
        }
    }

    pub(crate) fn retry(&mut self, plan: Plan) {
        self.retry = Some(plan);
    }

    pub(crate) fn replay(&mut self, replay: Vec<i64>, size: i64) {
        self.pending = Some((replay, size));
    }
}

fn simpler(a: &[i64], b: &[i64]) -> bool {
    a.len() < b.len() || (a.len() == b.len() && a < b)
}

pub enum Trial<T> {
    Candidate(T),
    NotFailed,
    /// Initialization/deadline failure before a replay began is not a new
    /// counterexample. Stop shrinking without replacing the recorded best.
    Unavailable,
}

/// Matches Node's deletion/zeroing/halving order. Only a failing replay
/// with an actually simpler recorded stream becomes the new best.
pub fn shrink<T>(
    mut best: T,
    budget: u32,
    draws: impl Fn(&T) -> &[i64],
    mut attempt: impl FnMut(&[i64]) -> Result<Trial<T>, String>,
) -> Result<T, String> {
    let mut left = budget;
    let mut try_stream = |stream: &[i64], best: &mut T, left: &mut u32| -> Result<bool, String> {
        if *left == 0 {
            return Ok(false);
        }
        *left -= 1;
        match attempt(stream)? {
            Trial::Candidate(got) if simpler(draws(&got), draws(best)) => {
                *best = got;
                return Ok(true);
            }
            Trial::Unavailable => *left = 0,
            Trial::Candidate(_) | Trial::NotFailed => {}
        }
        Ok(false)
    };
    let mut improved = true;
    while improved && left > 0 {
        improved = false;
        for width in [8, 4, 2, 1] {
            let mut index = 0;
            while index + width <= draws(&best).len() && left > 0 {
                let mut stream = draws(&best).to_vec();
                stream.drain(index..index + width);
                if try_stream(&stream, &mut best, &mut left)? {
                    improved = true;
                } else {
                    index += 1;
                }
            }
        }
        for width in [8, 4, 2, 1] {
            let mut index = 0;
            while index + width <= draws(&best).len() && left > 0 {
                let mut stream = draws(&best).to_vec();
                if stream[index..index + width].iter().any(|draw| *draw != 0) {
                    stream[index..index + width].fill(0);
                    if try_stream(&stream, &mut best, &mut left)? {
                        improved = true;
                    }
                }
                index += 1;
            }
        }
        let mut index = 0;
        while index < draws(&best).len() && left > 0 {
            let mut low = 0;
            while left > 0 {
                let stream = draws(&best);
                let Some(&value) = stream.get(index).filter(|value| **value > low) else {
                    break;
                };
                let middle = low + (value - low) / 2;
                let mut stream = stream.to_vec();
                stream[index] = middle;
                if try_stream(&stream, &mut best, &mut left)? {
                    improved = true;
                } else {
                    low = middle + 1;
                }
            }
            index += 1;
        }
    }
    Ok(best)
}
