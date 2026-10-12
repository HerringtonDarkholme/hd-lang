use std::io::{self, Cursor, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

use super::engine::TestEnvironment;
use crate::Shared;
use crate::codec::{Dec, Enc};
use crate::host::{Host, HostIo, Stop};
use hd_run::{Grants, Instance, Outcome};

/// Child output is bounded across both streams. The flag is checked after
/// each host call and at epoch checks, so an overflowing child is interrupted.
struct ProcessSink {
    bytes: Shared,
    count: Arc<AtomicUsize>,
    overflow: Arc<AtomicBool>,
    limit: usize,
}

impl Write for ProcessSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let start = self.count.fetch_add(bytes.len(), Ordering::Relaxed);
        let kept = bytes.len().min(self.limit.saturating_sub(start));
        self.bytes.write_all(&bytes[..kept])?;
        if kept != bytes.len() {
            self.overflow.store(true, Ordering::Relaxed);
            return Err(io::ErrorKind::FileTooLarge.into());
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) fn process(
    bytes: &[u8],
    grants: &Grants,
    environment: &TestEnvironment,
    deadline: Option<Instant>,
    memory_bytes: u64,
) -> wasmtime::Result<Enc> {
    process_with_limit(bytes, grants, environment, deadline, memory_bytes, 1 << 30)
}

fn process_with_limit(
    bytes: &[u8],
    grants: &Grants,
    environment: &TestEnvironment,
    deadline: Option<Instant>,
    memory_bytes: u64,
    output_limit: usize,
) -> wasmtime::Result<Enc> {
    let mut arguments = Dec::new(bytes);
    let name = arguments.str()?;
    let args = arguments.strings()?;
    let input = arguments.bytes()?.to_vec();
    let mut encoded = Enc::default();
    if !grants.covers_name("Process", &name) {
        encoded.leb(1).leb(2);
        return Ok(encoded);
    }
    let Some(child) = environment.children.iter().find(|child| child.name == name) else {
        encoded.leb(1).leb(0);
        return Ok(encoded);
    };
    let out = Shared::default();
    let err = Shared::default();
    let count = Arc::new(AtomicUsize::new(0));
    let overflow = Arc::new(AtomicBool::new(false));
    let result = std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("hd-test-process".into())
            .stack_size(crate::thread_stack(&child.engine.config))
            .spawn_scoped(scope, || {
                let mut host = Host::new(
                    HostIo {
                        out: Box::new(ProcessSink {
                            bytes: out.clone(),
                            count: Arc::clone(&count),
                            overflow: Arc::clone(&overflow),
                            limit: output_limit,
                        }),
                        err: Box::new(ProcessSink {
                            bytes: err.clone(),
                            count: Arc::clone(&count),
                            overflow: Arc::clone(&overflow),
                            limit: output_limit,
                        }),
                        input: Some(Box::new(Cursor::new(input))),
                    },
                    args,
                    name.clone(),
                    child.grants.clone(),
                    None,
                    memory_bytes,
                );
                host.environment = Some(TestEnvironment {
                    cwd: environment.cwd.clone(),
                    program: name.clone(),
                    children: Vec::new(),
                });
                // A named child uses the ordinary Process provider, not test names.
                host.set_deadline(deadline);
                host.output_overflow = Some(Arc::clone(&overflow));
                host.output_budget = Some((Arc::clone(&count), output_limit));
                let mut instance = child
                    .engine
                    .instantiate_host(&child.module, host)
                    .map_err(|error| format!("{error:?}"))?;
                Ok::<_, String>(match instance.init() {
                    Outcome::Exit(0) => hd_run::drive(&mut instance, usize::MAX),
                    other => other,
                })
            })
            .map_err(|error| error.to_string())?
            .join()
            .map_err(|_| "the test child thread panicked".to_owned())?
    });
    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        return Err(Stop::TimeLimit.into());
    }
    if overflow.load(Ordering::Relaxed) {
        encoded
            .leb(1)
            .leb(3)
            .str("process output exceeds the host limit");
        return Ok(encoded);
    }
    match result {
        Ok(Outcome::Exit(status)) => {
            encoded
                .leb(0)
                .str(&String::from_utf8_lossy(&out.take()))
                .str(&String::from_utf8_lossy(&err.take()))
                .zz(i64::from(status));
        }
        Ok(Outcome::Panic(_)) => {
            encoded
                .leb(0)
                .str(&String::from_utf8_lossy(&out.take()))
                .str(&String::from_utf8_lossy(&err.take()))
                .zz(3);
        }
        Ok(Outcome::Internal(message)) | Err(message) => {
            encoded.leb(1).leb(3).str(&message);
        }
    }
    Ok(encoded)
}
