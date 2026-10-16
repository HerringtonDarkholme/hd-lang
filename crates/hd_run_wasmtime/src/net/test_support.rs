use hd_base::HashSet;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::codec::{Dec, Enc};
use crate::limits::Ledger;
use crate::reactor::{Done, Reactor, Work};
use hd_run::Grants;

use super::Client;
use super::client::Start;

/// A runtime of a test's own, for the async parts alone.
pub(crate) fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

pub(crate) fn host_args(host: &str) -> Enc {
    let mut e = Enc::default();
    e.str(host);
    e
}

pub(crate) fn reserved_refused_port() -> (SocketAddr, [TcpStream; 2]) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let client =
        TcpStream::connect_timeout(&listener.local_addr().unwrap(), Duration::from_secs(1))
            .unwrap();
    let (peer, _) = listener.accept().unwrap();
    // An established client's source port stays reserved but has no
    // listener, so an unrelated connection tuple is refused. Merely
    // binding an unlistening socket would instead time out on Darwin.
    (client.local_addr().unwrap(), [client, peer])
}

/// A `.start` as the host makes it: complete, or pending as `h`.
pub(crate) enum Started {
    Ready(Enc),
    Pending(i32),
}

/// An instance's `Net` table and reactor, driven as `host/call.rs` drives
/// them: each `.start`, its wait, and its `.finish` or abort.
pub(crate) struct Net {
    pub(crate) client: Client,
    pub(crate) reactor: Reactor,
    /// The heap budget, with no maximum.
    pub(crate) ledger: Arc<Ledger>,
    /// The operations completed and not finished or aborted yet.
    completed: HashSet<i32>,
}

impl Net {
    pub(crate) fn new() -> Self {
        Self {
            client: Client::default(),
            reactor: Reactor::new(None),
            ledger: Ledger::new(u64::MAX),
            completed: HashSet::default(),
        }
    }

    pub(crate) fn start(
        &mut self,
        method: &str,
        params: &[i64],
        bytes: &[u8],
        grants: &Grants,
    ) -> wasmtime::Result<Started> {
        Ok(
            match self
                .client
                .start(method, params, bytes, grants, &self.ledger)?
            {
                Start::Ready(e) => Started::Ready(e),
                Start::Closed { reply, handle } => {
                    super::ops::dispose(&mut self.reactor, handle);
                    Started::Ready(reply)
                }
                Start::Run(job) => Started::Pending(job.offload(&mut self.reactor).unwrap()),
            },
        )
    }

    /// Waits until pending operation `h` completes.
    pub(crate) fn wait_for(&mut self, h: i32) {
        let until = Instant::now() + Duration::from_secs(20);
        let mut out = Vec::new();
        loop {
            self.completed.extend(self.reactor.due(&mut out));
            if self.completed.contains(&h) {
                return;
            }
            assert!(Instant::now() < until, "operation {h} completes");
            assert!(
                self.reactor
                    .wait_until(Some(Instant::now() + Duration::from_millis(100)), None),
                "an operation is pending"
            );
        }
    }

    /// The `.finish` of completed operation `h`.
    pub(crate) fn finish(&mut self, h: i32) -> Enc {
        self.completed.remove(&h);
        let Some(Work::Offloaded(Some(Done::Net(done)))) = self.reactor.remove(h).map(|op| op.work)
        else {
            panic!("operation {h} has no result");
        };
        self.client.finish(done).unwrap()
    }

    /// `hd:rt.abort` of operation `h`, as `Host::control` does it.
    pub(crate) fn abort(&mut self, h: i32) {
        self.completed.remove(&h);
        if let Some(Work::Offloaded(Some(Done::Net(done)))) =
            self.reactor.remove(h).map(|op| op.work)
        {
            self.client.unread(done);
        }
    }

    /// A whole call: its `.start`, then, if it waits, its `.finish`.
    pub(crate) fn call(
        &mut self,
        method: &str,
        params: &[i64],
        bytes: &[u8],
        grants: &Grants,
    ) -> wasmtime::Result<Enc> {
        match self.start(method, params, bytes, grants)? {
            Started::Ready(e) => Ok(e),
            Started::Pending(h) => {
                self.wait_for(h);
                Ok(self.finish(h))
            }
        }
    }

    /// Opens a socket on 127.0.0.1 with `method` and returns its handle.
    pub(crate) fn opened(&mut self, method: &str, port: u16) -> i64 {
        let result = self
            .call(
                method,
                &[i64::from(port), 0],
                host_args("127.0.0.1").done(),
                &Grants::default(),
            )
            .unwrap();
        let mut d = Dec::new(result.done());
        assert_eq!(d.leb().unwrap(), 0);
        i64::try_from(d.leb().unwrap()).unwrap()
    }

    /// Reads exactly `count` bytes from `handle`, `max` at a time: TCP
    /// returns at most `max` bytes, not an application message.
    pub(crate) fn read_n(&mut self, handle: i64, count: usize, max: i64) -> Vec<u8> {
        let mut bytes = Vec::new();
        while bytes.len() < count {
            let encoded = self
                .call("TcpStream.read", &[handle, max], &[], &Grants::default())
                .unwrap();
            let chunk = result_bytes(&encoded);
            assert!(!chunk.is_empty(), "peer ended before the fixture message");
            bytes.extend(chunk);
        }
        assert_eq!(bytes.len(), count);
        bytes
    }
}

pub(crate) fn result_bytes(e: &Enc) -> Vec<u8> {
    let mut d = Dec::new(e.done());
    assert_eq!(d.leb().unwrap(), 0);
    d.bytes().unwrap().to_vec()
}
