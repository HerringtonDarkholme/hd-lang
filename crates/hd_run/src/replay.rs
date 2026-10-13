//! Replay (live-execution.md §5): the inputs a session's journal recorded,
//! and [`Replay`], which serves one input's host calls and wakes from its
//! rows instead of making them again (`cli.repl.host.once`).
//!
//! A replayed input must ask for what the record holds, in its order: the
//! same method with the same arguments (compared as the journal stores
//! them, the bytes or their digest), a finish or an abort of the same
//! handle, a wake at the same poll. The first request that does not match
//! is a divergence (§5.2): the replay ends there, and its message names the
//! input, what it asked for and what the record has. A replay never goes
//! on live after a divergence.

use std::collections::VecDeque;

use crate::journal::{self, End, Event, Request, Value};

/// One input as a journal recorded it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedInput {
    pub number: u32,
    /// The hash of its source text.
    pub source: u128,
    /// The hash of its module.
    pub module: u128,
    /// Its host calls and driver events, in order: the rows between its
    /// `Input` and `End` rows.
    pub events: Vec<Event>,
    /// How it ended; `None` when the journal ends inside it.
    pub end: Option<End>,
}

impl RecordedInput {
    /// Whether `source` is the text this input ran, by its hash.
    #[must_use]
    pub fn ran(&self, source: &str) -> bool {
        hd_base::hash128(source.as_bytes()).0 == self.source
    }
}

/// What a session's journal recorded: its execution id and its inputs, in
/// the order they ran.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recorded {
    pub execution: u128,
    pub inputs: Vec<RecordedInput>,
}

/// Reads a session's journal into its inputs. A torn tail is cut (§4.3);
/// an input it cuts has no end.
pub fn recorded(bytes: &[u8]) -> Result<Recorded, String> {
    let (rows, _) = journal::read(bytes);
    let mut events = journal::decode(&rows)?.into_iter();
    let execution = match events.next() {
        Some(Event::Header { format, execution }) if format == journal::FORMAT => execution,
        Some(Event::Header { format, .. }) => {
            return Err(format!(
                "the journal has format {format}, not {}",
                journal::FORMAT
            ));
        }
        _ => return Err("the journal has no header".into()),
    };
    let mut inputs: Vec<RecordedInput> = Vec::new();
    for event in events {
        if let Event::Input {
            number,
            source,
            module,
        } = event
        {
            inputs.push(RecordedInput {
                number,
                source,
                module,
                events: Vec::new(),
                end: None,
            });
            continue;
        }
        let Some(input) = inputs.last_mut().filter(|i| i.end.is_none()) else {
            return Err(format!(
                "the journal has {} outside an input",
                describe(Some(&event))
            ));
        };
        match event {
            Event::End(end) => input.end = Some(end),
            other => input.events.push(other),
        }
    }
    Ok(Recorded { execution, inputs })
}

/// Serves one recorded input's host results while it runs again.
#[derive(Debug)]
pub struct Replay {
    number: u32,
    events: VecDeque<Event>,
    /// The input's polls so far, as a `Wake` row counts them.
    polls: u64,
    diverged: Option<String>,
}

/// A host call's served reply: its value and the result bytes it writes.
pub type Reply = (Value, Option<Vec<u8>>);

impl Replay {
    #[must_use]
    pub fn new(input: &RecordedInput) -> Replay {
        Replay {
            number: input.number,
            events: input.events.iter().cloned().collect(),
            polls: 0,
            diverged: None,
        }
    }

    /// The first divergence, once there is one.
    #[must_use]
    pub fn diverged(&self) -> Option<&str> {
        self.diverged.as_deref()
    }

    /// A call of the import `module` `name` with the arguments `args` (see
    /// `journal::put_int_arg`): the reply the record holds.
    pub fn call(&mut self, module: &str, name: &str, args: &[u8]) -> Result<Reply, String> {
        let asked = format!("called {module} {name}");
        match self.events.front() {
            Some(Event::Call {
                module: m,
                name: n,
                request,
                ..
            }) if m == module && n == name => {
                if *request != Request::of(args) {
                    return Err(self.diverge(format!(
                        "input {} {asked} with other arguments than before",
                        self.number
                    )));
                }
                match self.events.pop_front() {
                    Some(Event::Call { value, result, .. }) => Ok((value, result)),
                    _ => unreachable!("the front row is this call"),
                }
            }
            _ => Err(self.unexpected(&asked)),
        }
    }

    /// A `.finish(handle)`: the reply the record holds.
    pub fn finish(&mut self, handle: u32) -> Result<Reply, String> {
        match self.events.front() {
            Some(Event::Finish { handle: h, .. }) if *h == handle => {
                match self.events.pop_front() {
                    Some(Event::Finish { value, result, .. }) => Ok((value, result)),
                    _ => unreachable!("the front row is this finish"),
                }
            }
            _ => Err(self.unexpected(&format!("finished operation {handle}"))),
        }
    }

    /// An `hd:rt` `block`: the handles the record holds, or `None` when the
    /// record has nothing more, so the block finds nothing pending (a
    /// deadlock, as the first run had one).
    pub fn block(&mut self) -> Result<Option<Vec<u32>>, String> {
        match self.events.front() {
            None => Ok(None),
            Some(Event::Block { .. }) => match self.events.pop_front() {
                Some(Event::Block { handles }) => Ok(Some(handles)),
                _ => unreachable!("the front row is this block"),
            },
            Some(_) => Err(self.unexpected("blocked")),
        }
    }

    /// An `hd:rt` `abort(handle)`, checked against the record.
    pub fn abort(&mut self, handle: u32) -> Result<(), String> {
        match self.events.front() {
            Some(Event::Abort { handle: h }) if *h == handle => {
                self.events.pop_front();
                Ok(())
            }
            _ => Err(self.unexpected(&format!("aborted operation {handle}"))),
        }
    }

    /// One poll of the input.
    pub fn poll(&mut self) {
        self.polls += 1;
    }

    /// Whether the record's next row is a wake: the input waits for one.
    /// When it is not, a wait finds nothing pending.
    #[must_use]
    pub fn wake_next(&self) -> bool {
        matches!(self.events.front(), Some(Event::Wake { .. }))
    }

    /// The input waits on the host after its polls so far: the handles the
    /// record wakes it with then.
    pub fn wake(&mut self) -> Result<Vec<u32>, String> {
        match self.events.front() {
            Some(Event::Wake { poll, .. }) if *poll == self.polls => {
                match self.events.pop_front() {
                    Some(Event::Wake { handles, .. }) => Ok(handles),
                    _ => unreachable!("the front row is this wake"),
                }
            }
            _ => {
                let asked = format!("waited on the host after {} polls", self.polls);
                Err(self.unexpected(&asked))
            }
        }
    }

    /// The input ended with `end`: a divergence unless the record ended it
    /// so, `recorded`, after every row was served.
    pub fn end(&mut self, end: End, recorded: Option<End>) -> Result<(), String> {
        if let Some(diverged) = &self.diverged {
            return Err(diverged.clone());
        }
        if !self.events.is_empty() {
            return Err(self.unexpected(&format!("{} early", ended(end))));
        }
        match recorded {
            Some(r) if r == end => Ok(()),
            Some(r) => Err(self.diverge(format!(
                "input {} {}, but before it {}",
                self.number,
                ended(end),
                ended(r)
            ))),
            None => Err(self.diverge(format!(
                "input {} {}, but the record of it is cut short",
                self.number,
                ended(end)
            ))),
        }
    }

    /// The divergence of a request `asked` that is not the record's next
    /// row.
    fn unexpected(&mut self, asked: &str) -> String {
        let has = describe(self.events.front());
        self.diverge(format!(
            "input {} {asked}, but the record has {has}",
            self.number
        ))
    }

    fn diverge(&mut self, message: String) -> String {
        self.diverged.get_or_insert(message).clone()
    }
}

/// A row, as a divergence message names what the record has.
fn describe(event: Option<&Event>) -> String {
    match event {
        None => "nothing more".into(),
        Some(Event::Header { .. }) => "a header".into(),
        Some(Event::Input { number, .. }) => format!("input {number}"),
        Some(Event::Call { module, name, .. }) => format!("a call of {module} {name}"),
        Some(Event::Finish { handle, .. }) => format!("the finish of operation {handle}"),
        Some(Event::Wake { poll, .. }) => format!("a wake after {poll} polls"),
        Some(Event::Block { .. }) => "a block".into(),
        Some(Event::Abort { handle }) => format!("the abort of operation {handle}"),
        Some(Event::Cancel { .. }) => "a cancel".into(),
        Some(Event::End(end)) => format!("its end: it {}", ended(*end)),
    }
}

/// How an input ended, as a sentence's verb phrase.
fn ended(end: End) -> String {
    match end {
        End::Exit(0) => "ended".into(),
        End::Exit(status) => format!("ended with status {status}"),
        End::Trapped => "panicked".into(),
        End::Stopped => "was stopped".into(),
        End::Cancelled => "was cancelled".into(),
        End::Failed => "failed to run".into(),
    }
}

#[cfg(test)]
#[path = "replay_tests.rs"]
mod tests;
