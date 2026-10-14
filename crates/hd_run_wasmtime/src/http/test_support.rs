use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use socket2::{Domain, Protocol, Socket, Type};

use hd_run::Grants;

use crate::codec::{Dec, Enc};
use crate::reactor::{Done, Reactor, Work};

use super::Headers;
use super::client::{Job, Sent, Start, finish, start};
use super::headers::header_end;

/// Offloads `job` onto the `hd-io` runtime, as `send.start` does, and
/// returns the operation's handle.
pub(crate) fn offload(reactor: &mut Reactor, job: Job) -> i32 {
    reactor
        .offload(async move { Done::Http(job.run().await) })
        .expect("offloads")
}

/// Waits for the operation `h` to complete and takes its result, as
/// `block` and `send.finish` do.
pub(crate) fn finished(reactor: &mut Reactor, h: i32) -> Sent {
    let mut out = Vec::new();
    // An earlier `due` may already have completed `h` with another handle.
    while !reactor.is_completed(h) && !reactor.due(&mut out).contains(&h) {
        assert!(reactor.wait(), "the request is pending");
    }
    match reactor.remove(h).map(|op| op.work) {
        Some(Work::Offloaded(Some(Done::Http(sent)))) => sent,
        _ => panic!("the request has no result"),
    }
}

/// `send.start` then, when it sends the request, `send.finish`: the
/// encoded result.
pub(crate) fn send(bytes: &[u8], grants: &Grants) -> Enc {
    match start(bytes, grants).expect("the request decodes") {
        Start::Ready(e) => e,
        Start::Run(job) => {
            let mut reactor = Reactor::new(None);
            let h = offload(&mut reactor, job);
            finish(finished(&mut reactor, h)).expect("the request ends")
        }
    }
}

pub(crate) fn request(
    method: &str,
    url: &str,
    headers: &Headers,
    body: &[u8],
    timeout: i64,
) -> Enc {
    let mut e = Enc::default();
    e.leb(7)
        .str(method)
        .str(url)
        .list(headers, |e, (name, value)| {
            e.str(name).str(value);
        })
        .bytes(body)
        .leb(1)
        .zz(timeout);
    e
}

pub(crate) fn read_request(stream: &mut TcpStream) -> Vec<u8> {
    // A socket accepted from a nonblocking listener can inherit that
    // mode. Explicitly normalize fixture sockets so read timeouts bound
    // blocking reads instead of racing the arrival of request bytes.
    stream.set_nonblocking(false).unwrap();
    let until = Instant::now() + Duration::from_secs(2);
    let mut bytes = Vec::new();
    let mut buf = [0; 8192];
    loop {
        let remaining = until.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "request completes within fixture budget"
        );
        stream.set_read_timeout(Some(remaining)).unwrap();
        let n = loop {
            match stream.read(&mut buf) {
                Ok(n) => break n,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => panic!("request completes within fixture budget: {error}"),
            }
        };
        assert_ne!(n, 0);
        bytes.extend_from_slice(&buf[..n]);
        if let Some(end) = header_end(&bytes) {
            let header = String::from_utf8_lossy(&bytes[..end]);
            let length = header
                .lines()
                .filter_map(|s| s.split_once(':'))
                .find(|(n, _)| n.eq_ignore_ascii_case("content-length"))
                .map_or(0, |(_, v)| v.trim().parse::<usize>().unwrap());
            let chunked = header
                .to_ascii_lowercase()
                .contains("transfer-encoding: chunked");
            if bytes.len() >= end + length && (!chunked || bytes.ends_with(b"0\r\n\r\n")) {
                return bytes;
            }
        }
    }
}

pub(crate) fn write_response(stream: &mut TcpStream, bytes: &[u8]) -> std::io::Result<()> {
    write_response_with_budget(stream, bytes, Duration::from_secs(2))
}

pub(crate) fn write_response_with_budget(
    stream: &mut TcpStream,
    mut bytes: &[u8],
    budget: Duration,
) -> std::io::Result<()> {
    // Bound the whole response, not each partial write independently.
    let until = Instant::now() + budget;
    while !bytes.is_empty() {
        let remaining = until.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "response exceeds fixture budget",
            ));
        }
        stream.set_write_timeout(Some(remaining))?;
        match stream.write(bytes) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "fixture response socket closed",
                ));
            }
            Ok(written) => bytes = &bytes[written..],
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

pub(crate) fn server(response: Vec<u8>) -> (String, std::thread::JoinHandle<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/path?q=1", listener.local_addr().unwrap());
    let thread = std::thread::spawn(move || {
        let mut stream = bounded_accept(&listener);
        let request = read_request(&mut stream);
        write_response(&mut stream, &response).unwrap();
        request
    });
    (url, thread)
}

pub(crate) fn non_listening_socket() -> Socket {
    let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP)).unwrap();
    socket
        .bind(
            &"127.0.0.1:0"
                .parse::<std::net::SocketAddr>()
                .unwrap()
                .into(),
        )
        .unwrap();
    // Keep the port reserved without accepting connections. Darwin
    // times these out; other platforms may refuse immediately.
    socket
}

pub(crate) fn response(encoded: &[u8]) -> (u64, Headers, Vec<u8>) {
    let mut d = Dec::new(encoded);
    assert_eq!(d.leb().unwrap(), 0);
    let status = d.leb().unwrap();
    let headers = (0..d.leb().unwrap())
        .map(|_| (d.str().unwrap(), d.str().unwrap()))
        .collect();
    let body = d.bytes().unwrap().to_vec();
    (status, headers, body)
}

pub(crate) fn bounded_accept(listener: &TcpListener) -> TcpStream {
    listener.set_nonblocking(true).unwrap();
    let until = Instant::now() + Duration::from_secs(2);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                return stream;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < until, "request reaches loopback fixture");
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(error) => panic!("accept: {error}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{bounded_accept, write_response_with_budget};
    use std::net::{TcpListener, TcpStream};
    use std::time::{Duration, Instant};

    #[test]
    fn response_fixture_bounds_backpressured_writes() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let _peer = bounded_accept(&listener);
        let socket = socket2::Socket::from(client);
        socket.set_send_buffer_size(4096).unwrap();
        let mut stream: TcpStream = socket.into();
        let payload = vec![0; 16 * 1024 * 1024];
        let start = Instant::now();
        let result = write_response_with_budget(&mut stream, &payload, Duration::from_millis(30));
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(matches!(
            result.unwrap_err().kind(),
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
        ));
    }

    // The flag is read back with fcntl, which has no Windows equivalent.
    #[cfg(unix)]
    #[test]
    fn bounded_accept_returns_a_blocking_fixture_socket() {
        use nix::fcntl::{FcntlArg, OFlag, fcntl};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let _peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let stream = bounded_accept(&listener);
        let flags = OFlag::from_bits_retain(fcntl(&stream, FcntlArg::F_GETFL).unwrap());
        assert!(!flags.contains(OFlag::O_NONBLOCK));
    }
}
