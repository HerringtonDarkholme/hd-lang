use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak, mpsc};
use std::time::Instant;

use hd_run::Grants;
use url::Url;

use crate::codec::Enc;
use crate::host::Stop;

use super::request::{Failure, Request, encode, send};

#[derive(Default)]
pub struct Client {
    pub(crate) worker: Option<Arc<Worker>>,
}

impl Client {
    #[cfg(test)]
    pub fn send(&mut self, bytes: &[u8], grants: &Grants) -> wasmtime::Result<Enc> {
        self.send_until(bytes, grants, None)
    }

    /// The earlier request/program deadline includes queueing and every hop.
    /// A program deadline traps, rather than becoming an Http error value.
    pub fn send_until(
        &mut self,
        bytes: &[u8],
        grants: &Grants,
        program_deadline: Option<Instant>,
    ) -> wasmtime::Result<Enc> {
        let started = Instant::now();
        if program_deadline.is_some_and(|deadline| started >= deadline) {
            return Err(Stop::TimeLimit.into());
        }
        let request = Request::decode(bytes)?;
        // Preserve invalid-URL/grant precedence even if the shared worker
        // is busy. Queue time belongs to the whole-request deadline.
        if let Err(failure) = granted_url(&request.url, grants) {
            return Ok(encode(Err(failure)));
        }
        let timeout = request.timeout();
        let request_deadline = started.checked_add(timeout);
        let deadline = match (request_deadline, program_deadline) {
            (Some(request), Some(program)) => Some(request.min(program)),
            (request, None) => request,
            (None, program) => program,
        };
        let remaining = timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Ok(encode(Err(Failure::Timeout)));
        }
        if self.worker.is_none() {
            self.worker = Some(shared_worker()?);
        }
        let (reply, receive) = mpsc::channel();
        self.worker
            .as_ref()
            .and_then(|w| w.sender.as_ref())
            .ok_or_else(|| Stop::Internal("the Http client is closed".into()))?
            .send(Job {
                request,
                grants: grants.clone(),
                reply,
                deadline,
            })
            .map_err(|_| Stop::Internal("the Http client stopped".into()))?;
        let Some(deadline) = deadline else {
            return receive.recv().map_err(|_| {
                Stop::Internal("the Http client stopped without a reply".into()).into()
            });
        };
        let program_first = program_deadline
            .is_some_and(|program| request_deadline.is_none_or(|request| program <= request));
        match receive.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(reply) if !(program_first && Instant::now() >= deadline) => Ok(reply),
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) if program_first => {
                if let Some(worker) = &self.worker {
                    worker.detach_on_drop.store(true, Ordering::Relaxed);
                }
                Err(Stop::TimeLimit.into())
            }
            Ok(reply) => Ok(reply),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(encode(Err(Failure::Timeout))),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(Stop::Internal("the Http client stopped without a reply".into()).into())
            }
        }
    }
}

pub(crate) struct Worker {
    sender: Option<mpsc::Sender<Job>>,
    thread: Option<std::thread::JoinHandle<()>>,
    detach_on_drop: AtomicBool,
}

impl Drop for Worker {
    fn drop(&mut self) {
        // The worker owns no Arc<Worker>. Closing the final guard drops
        // the queue sender. Usually it is idle; a caller-side timeout may
        // race the active job's same network deadline. A program must not
        // exceed its budget waiting for worker teardown. Only that final
        // join is detached; each expired job still skips all future I/O.
        self.sender.take();
        if let Some(thread) = self.thread.take()
            && (!self.detach_on_drop.load(Ordering::Relaxed) || thread.is_finished())
        {
            let _ = thread.join();
        }
    }
}

struct Job {
    request: Request,
    grants: Grants,
    reply: mpsc::Sender<Enc>,
    deadline: Option<Instant>,
}

fn shared_worker() -> wasmtime::Result<Arc<Worker>> {
    static WORKER: OnceLock<Mutex<Weak<Worker>>> = OnceLock::new();
    let mut shared = WORKER
        .get_or_init(Mutex::default)
        .lock()
        .map_err(|_| Stop::Internal("the Http client lock is poisoned".into()))?;
    if let Some(worker) = shared.upgrade() {
        return Ok(worker);
    }
    let worker = spawn_worker()?;
    *shared = Arc::downgrade(&worker);
    Ok(worker)
}

pub(crate) fn spawn_worker() -> wasmtime::Result<Arc<Worker>> {
    let (sender, receive) = mpsc::channel::<Job>();
    let thread = std::thread::Builder::new()
        .name("hd-http".into())
        .spawn(move || {
            for job in receive {
                let response = if job
                    .deadline
                    .is_some_and(|deadline| Instant::now() >= deadline)
                {
                    Err(Failure::Timeout)
                } else {
                    send(job.request, &job.grants, job.deadline)
                };
                let _ = job.reply.send(encode(response));
            }
        })
        .map_err(|e| Stop::Internal(format!("the Http client cannot start: {e}")))?;
    let worker = Arc::new(Worker {
        sender: Some(sender),
        thread: Some(thread),
        detach_on_drop: AtomicBool::new(false),
    });
    Ok(worker)
}

pub(crate) fn granted_url(value: &str, grants: &Grants) -> Result<Url, Failure> {
    let url = Url::parse(value).map_err(|_| Failure::InvalidUrl(value.into()))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(Failure::InvalidUrl(value.into()));
    }
    let host = url.host_str().unwrap_or_default().trim_matches(['[', ']']);
    if !grants.covers_host("Http", host, url.port_or_known_default()) {
        return Err(Failure::NotGranted(host.into()));
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::{Client, Job, Worker, spawn_worker};
    use crate::codec::Enc;
    use crate::host::Stop;
    use crate::http::request::{Failure, Request, encode, outgoing};
    use crate::http::test_support::{
        bounded_accept, read_request, request, response, server, write_response,
    };
    use hd_run::Grants;
    use std::io::Read;
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use url::Url;

    #[test]
    fn deadline_covers_waiting_for_the_response_and_worker_closes_promptly() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (release, receive) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let mut stream = bounded_accept(&listener);
            read_request(&mut stream);
            let _ = receive.recv_timeout(Duration::from_secs(2));
        });
        let r = request("GET", &format!("http://{address}/"), &vec![], &[], 200);
        let mut client = Client {
            worker: Some(spawn_worker().unwrap()),
        };
        let started = Instant::now();
        let encoded = client.send(r.done(), &Grants::default());
        drop(client);
        let elapsed = started.elapsed();
        let _ = release.send(());
        let joined = server.join();
        // Release and join even when the deadline wins before accept/read.
        joined.unwrap();
        assert_eq!(encoded.unwrap().done(), [1, 5]);
        assert!(elapsed < Duration::from_secs(1));
    }

    #[test]
    fn deadline_includes_shared_worker_queue_and_expired_job_never_connects() {
        // An isolated worker avoids interfering with concurrent loopback
        // tests while exercising the same two-client sharing mechanism.
        let worker = spawn_worker().unwrap();
        let mut second = Client {
            worker: Some(worker.clone()),
        };
        let mut first = Client {
            worker: Some(worker),
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (arrived, arrival) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let mut stream = bounded_accept(&listener);
            read_request(&mut stream);
            arrived.send(()).unwrap();
            released.recv_timeout(Duration::from_secs(2)).unwrap();
            write_response(&mut stream, b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n").unwrap();
        });
        let first_request = request("GET", &format!("http://{address}/"), &vec![], &[], 2000);
        let first_reply = std::thread::spawn(move || {
            first
                .send(first_request.done(), &Grants::default())
                .unwrap()
        });
        arrival.recv_timeout(Duration::from_secs(1)).unwrap();
        let target = TcpListener::bind("127.0.0.1:0").unwrap();
        target.set_nonblocking(true).unwrap();
        let queued = request(
            "GET",
            &format!("http://{}/", target.local_addr().unwrap()),
            &vec![],
            &[],
            30,
        );
        let started = Instant::now();
        assert_eq!(
            second
                .send(queued.done(), &Grants::default())
                .unwrap()
                .done(),
            [1, 5]
        );
        assert!(started.elapsed() < Duration::from_millis(500));
        release.send(()).unwrap();
        assert_eq!(response(first_reply.join().unwrap().done()).0, 200);
        // A FIFO barrier proves that the timed-out job has been skipped,
        // not merely that the worker has not reached it yet.
        let (reply, barrier) = std::sync::mpsc::channel();
        second
            .worker
            .as_ref()
            .unwrap()
            .sender
            .as_ref()
            .unwrap()
            .send(Job {
                request: Request::decode(request("GET", "relative", &vec![], &[], 1000).done())
                    .unwrap(),
                grants: Grants::default(),
                reply,
                deadline: Some(Instant::now() + Duration::from_secs(1)),
            })
            .unwrap();
        let encoded = barrier.recv_timeout(Duration::from_secs(1)).unwrap();
        let mut expected = Enc::default();
        expected.leb(1).leb(1).str("relative");
        assert_eq!(encoded.done(), expected.done());
        assert_eq!(
            target.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        server.join().unwrap();
    }

    #[test]
    fn tls_handshake_timeout_and_final_worker_drop_use_the_same_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (release, released) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let mut stream = bounded_accept(&listener);
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut first = [0; 1];
            assert_eq!(stream.read(&mut first).unwrap(), 1);
            let _ = released.recv_timeout(Duration::from_secs(2));
        });
        let r = request("GET", &format!("https://{address}/"), &vec![], &[], 300);
        let mut client = Client {
            worker: Some(spawn_worker().unwrap()),
        };
        let started = Instant::now();
        let encoded = client.send(r.done(), &Grants::default());
        drop(client);
        let elapsed = started.elapsed();
        let _ = release.send(());
        let joined = server.join();
        joined.unwrap();
        assert_eq!(encoded.unwrap().done(), [1, 5]);
        assert!(elapsed < Duration::from_secs(1));
    }

    #[test]
    fn expired_program_deadline_is_lazy_and_traps_instead_of_encoding_http_timeout() {
        let mut client = Client::default();
        let error = client
            .send_until(&[], &Grants::default(), Some(Instant::now()))
            .err()
            .expect("program deadline traps");
        assert!(matches!(
            error.downcast_ref::<Stop>(),
            Some(Stop::TimeLimit)
        ));
        assert!(client.worker.is_none());
    }

    #[test]
    fn earlier_request_deadline_remains_an_http_error() {
        let worker = spawn_worker().unwrap();
        // Keep this isolated worker occupied: the queued request times out
        // without any external DNS or loopback connection being attempted.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (release, released) = std::sync::mpsc::channel();
        let (ready, arrived) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let mut stream = bounded_accept(&listener);
            read_request(&mut stream);
            ready.send(()).unwrap();
            let _ = released.recv_timeout(Duration::from_secs(2));
            let _ = write_response(&mut stream, b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
        });
        let first_request = request("GET", &format!("http://{address}/"), &vec![], &[], 1000);
        let mut first = Client {
            worker: Some(worker.clone()),
        };
        let first_reply = std::thread::spawn(move || {
            first
                .send(first_request.done(), &Grants::default())
                .unwrap()
        });
        arrived.recv_timeout(Duration::from_secs(1)).unwrap();
        let mut second = Client {
            worker: Some(worker),
        };
        let queued = request("GET", "http://127.0.0.1:9/", &vec![], &[], 20);
        assert_eq!(
            second
                .send_until(
                    queued.done(),
                    &Grants::default(),
                    Some(Instant::now() + Duration::from_secs(1))
                )
                .unwrap()
                .done(),
            [1, 5]
        );
        release.send(()).unwrap();
        assert_eq!(response(first_reply.join().unwrap().done()).0, 200);
        drop(second);
        server.join().unwrap();
    }

    #[test]
    fn program_expiration_skips_queued_io_without_stopping_another_guest() {
        let worker = spawn_worker().unwrap();
        let mut first = Client {
            worker: Some(worker.clone()),
        };
        let mut second = Client {
            worker: Some(worker),
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (ready, arrived) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let mut stream = bounded_accept(&listener);
            read_request(&mut stream);
            ready.send(()).unwrap();
            let _ = released.recv_timeout(Duration::from_secs(2));
            write_response(&mut stream, b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n").unwrap();
        });
        let first_request = request("GET", &format!("http://{address}/"), &vec![], &[], 1000);
        let first_reply = std::thread::spawn(move || {
            first
                .send(first_request.done(), &Grants::default())
                .unwrap()
        });
        arrived.recv_timeout(Duration::from_secs(1)).unwrap();
        let target = TcpListener::bind("127.0.0.1:0").unwrap();
        target.set_nonblocking(true).unwrap();
        let queued = request(
            "GET",
            &format!("http://{}/", target.local_addr().unwrap()),
            &vec![],
            &[],
            30_000,
        );
        let error = second
            .send_until(
                queued.done(),
                &Grants::default(),
                Some(Instant::now() + Duration::from_millis(20)),
            )
            .err()
            .expect("program deadline traps");
        assert!(matches!(
            error.downcast_ref::<Stop>(),
            Some(Stop::TimeLimit)
        ));
        release.send(()).unwrap();
        assert_eq!(response(first_reply.join().unwrap().done()).0, 200);
        // The subsequent FIFO reply proves the expired job was processed
        // and skipped, rather than merely still waiting in the queue.
        let invalid = request("GET", "relative", &vec![], &[], 1000);
        let (reply, barrier) = std::sync::mpsc::channel();
        second
            .worker
            .as_ref()
            .unwrap()
            .sender
            .as_ref()
            .unwrap()
            .send(Job {
                request: Request::decode(invalid.done()).unwrap(),
                grants: Grants::default(),
                reply,
                deadline: Some(Instant::now() + Duration::from_secs(1)),
            })
            .unwrap();
        assert_eq!(
            barrier.recv_timeout(Duration::from_secs(1)).unwrap().done(),
            encode(Err(Failure::InvalidUrl("relative".into()))).done()
        );
        assert_eq!(
            target.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        drop(second);
        server.join().unwrap();
    }

    #[test]
    fn expired_caller_does_not_join_a_worker_that_has_not_finished() {
        let (sender, receive) = std::sync::mpsc::channel::<Job>();
        let (release, released) = std::sync::mpsc::channel();
        let (finished, completion) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let _job = receive.recv_timeout(Duration::from_secs(1)).unwrap();
            let _ = released.recv_timeout(Duration::from_secs(2));
            let _ = finished.send(());
        });
        let worker = Arc::new(Worker {
            sender: Some(sender),
            thread: Some(thread),
            detach_on_drop: std::sync::atomic::AtomicBool::new(false),
        });
        let mut client = Client {
            worker: Some(worker),
        };
        let request = request("GET", "http://127.0.0.1:9/", &vec![], &[], 30_000);
        let start = Instant::now();
        let error = client
            .send_until(
                request.done(),
                &Grants::default(),
                Some(start + Duration::from_millis(20)),
            )
            .err()
            .expect("program deadline traps");
        assert!(matches!(
            error.downcast_ref::<Stop>(),
            Some(Stop::TimeLimit)
        ));
        drop(client);
        assert!(start.elapsed() < Duration::from_millis(500));
        assert_eq!(
            completion.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        );
        release.send(()).unwrap();
        completion.recv_timeout(Duration::from_secs(1)).unwrap();
    }

    #[test]
    fn client_keeps_status_raw_headers_bytes_and_does_not_decode_gzip() {
        let (url, server) = server(b"HTTP/1.1 103 Early Hints\r\nLink: ignored\r\n\r\nHTTP/1.1 500 Error\r\nX-A: first\r\nx-B: \xe9\r\nX-A: second\r\nContent-Encoding: gzip\r\nContent-Length: 3\r\n\r\n\xff\0z".to_vec());
        let r = request("GET", &url, &vec![], &[], 1000);
        let encoded = Client::default()
            .send(r.done(), &Grants::default())
            .unwrap();
        let (status, headers, body) = response(encoded.done());
        assert_eq!(status, 500);
        assert_eq!(body, b"\xff\0z");
        assert_eq!(
            headers,
            [
                ("X-A".into(), "first".into()),
                ("x-B".into(), "é".into()),
                ("X-A".into(), "second".into()),
                ("Content-Encoding".into(), "gzip".into()),
                ("Content-Length".into(), "3".into())
            ]
        );
        let wire = server.join().unwrap();
        assert_eq!(
            wire,
            outgoing(&Url::parse(&url).unwrap(), "GET", &vec![], &[]).unwrap()
        );
    }

    #[test]
    fn get_head_and_custom_methods_send_the_original_body() {
        for method in ["GET", "HEAD", "PROPFIND"] {
            let (url, server) = server(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_vec());
            let headers = vec![
                ("X-One".into(), "one".into()),
                ("X-Two".into(), "two".into()),
                ("X-One".into(), "three".into()),
            ];
            let r = request(method, &url, &headers, b"request-body", 1000);
            let encoded = Client::default()
                .send(r.done(), &Grants::default())
                .unwrap();
            assert_eq!(response(encoded.done()).0, 200);
            assert_eq!(
                server.join().unwrap(),
                outgoing(
                    &Url::parse(&url).unwrap(),
                    method,
                    &headers,
                    b"request-body"
                )
                .unwrap()
            );
        }
    }
}
