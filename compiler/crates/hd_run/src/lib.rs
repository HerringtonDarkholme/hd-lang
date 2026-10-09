#![forbid(unsafe_code)]
//! `hd_run`: the embedding API (runtime-and-host.md §17.9), the poll/wake
//! driver (suspension.md §14.4), the test runner core
//! (engines-and-test-runner.md §19) and the live-execution journal
//! (live-execution.md §4). No engine lives here: wasmtime is in
//! `hd_run_wasmtime`, the browser's in `hd_web`.

mod imports;
pub mod journal;
pub mod tests_model;

pub use imports::{module_imports, needs};

use std::task::Poll;

use hd_base::{NotImplemented, StageResult};
use hd_cache::CacheStore;

/// A panic as the runner reports it (§16.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanicReport {
    pub category: String,
    pub message: String,
    pub sites: Vec<(u32, u32)>,
}

/// How a run or a case ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Exit(u8),
    Panic(PanicReport),
    Internal(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    NotHd(String),
    Engine(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartError {
    /// A capability the program imports is not granted (§17.7): startup refusal.
    Refused(String),
    Engine(String),
}

/// Capability grants (§17.7): which keys, and which resources per key.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Grants {
    pub keys: Vec<(String, Vec<String>)>,
}

impl Grants {
    #[must_use]
    pub fn allows(&self, key: &str) -> bool {
        self.keys.iter().any(|(k, _)| k == key)
    }
}

/// Resource limits (§17.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub memory_bytes: u64,
    pub fuel: Option<u64>,
    pub wall_ms: Option<u64>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            memory_bytes: 1 << 30,
            fuel: None,
            wall_ms: None,
        }
    }
}

/// A host method call's result: ready now, or pending on an operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallResult {
    Ready,
    Pending(u32),
}

/// What a provider sees per call.
pub struct CallCx<'a> {
    pub grants: &'a Grants,
}

/// One capability trait's host implementation.
pub trait Provider: Send + Sync {
    fn key(&self) -> &str;
    fn call(&self, method: u32, args: &[u8], out: &mut Vec<u8>, cx: &mut CallCx<'_>) -> CallResult;
}

pub struct HostSetup {
    pub providers: Vec<Box<dyn Provider>>,
    pub grants: Grants,
    pub limits: Limits,
}

/// A running instance (§17.9).
pub trait Instance {
    fn init(&mut self) -> Outcome;
    fn poll(&mut self) -> Poll<Outcome>;
    fn wake(&mut self, handles: &[u32]);
    fn call_test(&mut self, export: u32) -> Poll<Outcome>;
    /// Handles completed since the last call: the reactor's batch.
    fn completed(&mut self) -> Vec<u32>;
}

/// An engine: wasmtime natively, the JS host in the browser.
pub trait Engine: Send + Sync {
    type Module: Send + Sync;
    fn load(&self, wasm: &[u8], cache: &dyn CacheStore) -> Result<Self::Module, LoadError>;
    fn instantiate(
        &self,
        m: &Self::Module,
        host: &HostSetup,
    ) -> Result<Box<dyn Instance>, StartError>;
}

/// The poll/wake driver (§14.4): poll until ready; between polls, wake
/// the instance with the handles its host operations completed. A poll
/// that returns pending with nothing to wake is a deadlock (§14.8).
pub fn drive(inst: &mut dyn Instance, max_polls: usize) -> Outcome {
    for _ in 0..max_polls {
        match inst.poll() {
            Poll::Ready(o) => return o,
            Poll::Pending => {
                let batch = inst.completed();
                if batch.is_empty() {
                    return Outcome::Panic(PanicReport {
                        category: "deadlock".into(),
                        message: "no pending operation can wake the program".into(),
                        sites: vec![],
                    });
                }
                inst.wake(&batch);
            }
        }
    }
    Outcome::Internal("poll limit reached".into())
}

/// Startup refusal (§17.7): every imported capability key must be granted.
pub fn check_grants(imported_keys: &[&str], grants: &Grants) -> Result<(), StartError> {
    match imported_keys.iter().find(|k| !grants.allows(k)) {
        Some(k) => Err(StartError::Refused(format!(
            "capability `{k}` is not granted"
        ))),
        None => Ok(()),
    }
}

/// Runs a whole program: the engine-independent part of `hd run`.
pub fn run_program<E: Engine>(
    engine: &E,
    wasm: &[u8],
    cache: &dyn CacheStore,
    host: &HostSetup,
) -> StageResult<Outcome> {
    let m = engine
        .load(wasm, cache)
        .map_err(|e| NotImplemented::new(hd_base::Stage::Run, format!("load: {e:?}")))?;
    let mut inst = engine
        .instantiate(&m, host)
        .map_err(|e| NotImplemented::new(hd_base::Stage::Run, format!("start: {e:?}")))?;
    match inst.init() {
        Outcome::Exit(0) => Ok(drive(inst.as_mut(), 1 << 20)),
        other => Ok(other),
    }
}

#[cfg(test)]
mod tests {
    use super::{Grants, Instance, Outcome, check_grants, drive};
    use std::task::Poll;

    struct Fake {
        polls: u32,
        woke: Vec<u32>,
    }
    impl Instance for Fake {
        fn init(&mut self) -> Outcome {
            Outcome::Exit(0)
        }
        fn poll(&mut self) -> Poll<Outcome> {
            self.polls += 1;
            if self.polls == 3 {
                Poll::Ready(Outcome::Exit(0))
            } else {
                Poll::Pending
            }
        }
        fn wake(&mut self, h: &[u32]) {
            self.woke.extend_from_slice(h);
        }
        fn call_test(&mut self, _: u32) -> Poll<Outcome> {
            Poll::Ready(Outcome::Exit(0))
        }
        fn completed(&mut self) -> Vec<u32> {
            vec![self.polls]
        }
    }

    #[test]
    fn driver_wakes_until_ready() {
        let mut f = Fake {
            polls: 0,
            woke: vec![],
        };
        assert_eq!(drive(&mut f, 10), Outcome::Exit(0));
        assert_eq!(f.woke, [1, 2]);
    }

    #[test]
    fn ungranted_capability_is_refused() {
        let g = Grants {
            keys: vec![("Console".into(), vec![])],
        };
        assert!(check_grants(&["Console"], &g).is_ok());
        assert!(check_grants(&["Console", "FsRead"], &g).is_err());
    }
}
