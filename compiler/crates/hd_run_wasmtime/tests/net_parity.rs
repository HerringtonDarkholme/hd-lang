//! Socket ABI parity. Every peer binds loopback; DNS uses numeric addresses.
mod support;

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use hd_run::{Engine, Grants, HostSetup, Instance, Limit, Limits, Outcome};
use hd_run_wasmtime::{EngineConfig, HostIo, WasmtimeEngine};
use support::*;
use wasm_encoder::{Instruction as I, ValType};

fn net_error(variant: u64, text: &str) -> Vec<u8> {
    Enc::default().leb(1).leb(variant).str(text).0
}
fn operation_error(variant: u64, text: &str) -> Vec<u8> {
    Enc::default().leb(1).leb(0).leb(variant).str(text).0
}
fn open(p: &mut Program, method: &str, host: &str, port: u16, result: Vec<u8>) {
    p.abi_call(
        "hd:Net",
        method,
        &[i32::from(port)],
        Some(&Enc::default().str(host).0),
        true,
        result,
    );
}
fn handle(p: &mut Program, method: &str, args: &[i32], payload: Option<&[u8]>, expected: Vec<u8>) {
    p.abi_call(
        "hd:Net",
        method,
        args,
        payload,
        !matches!(
            method,
            "TcpStream.close" | "TcpListener.close" | "UdpSocket.close"
        ),
        expected,
    );
}

#[test]
fn net_numeric_lookup_and_scope_host_match_ignores_port() {
    let scratch = Scratch::new();
    let mut p = Program::new();
    p.call(
        "hd:Net",
        "lookup",
        b"127.0.0.1",
        Enc::default().leb(0).leb(1).str("127.0.0.1").0,
    );
    p.call(
        "hd:Net",
        "lookup",
        b"::1",
        Enc::default().leb(0).leb(1).str("::1").0,
    );
    p.call("hd:Net", "lookup", b"127.0.0.2", net_error(0, "127.0.0.2"));
    p.call("hd:Net", "lookup", b"bad host", net_error(1, "bad host"));
    assert_program(
        &scratch,
        p,
        vec![(
            "Net",
            Limit::Entries(vec!["127.0.0.1:99".into(), "[::1]:99".into()]),
        )],
        || {},
    );
}

#[test]
fn net_invalid_addresses_precede_grants_and_all_open_methods_obey_scopes() {
    let scratch = Scratch::new();
    let mut p = Program::new();
    for method in ["connect", "listen", "bind_udp"] {
        open(&mut p, method, "bad host", 9, net_error(1, "bad host:9"));
        open(&mut p, method, "127.0.0.1", 9, net_error(0, "127.0.0.1:9"));
    }
    assert_program(&scratch, p, vec![("Net", Limit::Entries(vec![]))], || {});
}

#[test]
fn tcp_binary_write_read_zero_partial_reads_eof_and_disposal() {
    let scratch = Scratch::new();
    let server = TcpFixture::new(|mut peer| {
        let mut received = [0; 4];
        peer.read_exact(&mut received).unwrap();
        assert_eq!(received, [0, 255, 195, 169]);
        peer.write_all(&[1, 255, 0, 42]).unwrap();
    });
    let mut p = Program::new();
    open(
        &mut p,
        "connect",
        "127.0.0.1",
        server.address.port(),
        vec![0, 1],
    );
    handle(&mut p, "TcpStream.read", &[1, 0], None, vec![0, 0]);
    handle(
        &mut p,
        "TcpStream.write",
        &[1],
        Some(&Enc::default().bytes(&[0, 255, 195, 169]).0),
        vec![0],
    );
    for byte in [1, 255, 0, 42] {
        handle(&mut p, "TcpStream.read", &[1, 1], None, vec![0, 1, byte]);
    }
    for _ in 0..2 {
        handle(&mut p, "TcpStream.read", &[1, 8], None, vec![0, 0]);
    }
    handle(&mut p, "TcpStream.close", &[1], None, vec![0]);
    handle(&mut p, "TcpStream.read", &[1, 0], None, vec![1, 1]);
    handle(
        &mut p,
        "TcpStream.write",
        &[1],
        Some(&Enc::default().bytes(b"x").0),
        vec![1, 1],
    );
    handle(&mut p, "TcpStream.close", &[1], None, vec![1, 1]);
    assert_program(
        &scratch,
        p,
        vec![(
            "Net",
            Limit::Entries(vec![format!("127.0.0.1:{}", server.address.port())]),
        )],
        || {},
    );
}

#[test]
fn tcp_listener_accept_write_close_and_disposed_accept() {
    let scratch = Scratch::new();
    // The ABI exposes no listener local_addr, so the peer needs a selected
    // port before the guest binds it. This setup still requires a lease
    // handoff; cleanup tests below never rebind a released guest port.
    let address = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let mut p = Program::new();
    open(&mut p, "listen", "127.0.0.1", address.port(), vec![0, 1]);
    handle(&mut p, "TcpListener.accept", &[1], None, vec![0, 2]);
    handle(&mut p, "TcpStream.read", &[2, 1], None, vec![0, 1, b'x']);
    handle(
        &mut p,
        "TcpStream.write",
        &[2],
        Some(&Enc::default().bytes(b"ack").0),
        vec![0],
    );
    handle(&mut p, "TcpStream.close", &[2], None, vec![0]);
    handle(&mut p, "TcpListener.close", &[1], None, vec![0]);
    handle(&mut p, "TcpListener.accept", &[1], None, vec![1, 1]);
    handle(&mut p, "TcpListener.close", &[1], None, vec![1, 1]);
    let clients = Mutex::new(Vec::new());
    assert_program(&scratch, p, vec![], || {
        clients.lock().unwrap().push(std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut peer = loop {
                if let Ok(peer) = TcpStream::connect_timeout(&address, Duration::from_millis(50)) {
                    break peer;
                }
                assert!(
                    Instant::now() < deadline,
                    "listener fixture starts before its deadline"
                );
                std::thread::sleep(Duration::from_millis(5));
            };
            peer.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            peer.write_all(b"x").unwrap();
            let mut bytes = Vec::new();
            peer.read_to_end(&mut bytes).unwrap();
            assert_eq!(bytes, b"ack");
        }));
    });
    for client in clients.into_inner().unwrap() {
        client.join().unwrap();
    }
}

struct UdpFixture {
    address: SocketAddr,
    stopped: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    seen: Arc<Mutex<Vec<Vec<u8>>>>,
}
impl UdpFixture {
    fn new() -> Self {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let address = socket.local_addr().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let records = seen.clone();
        let thread = std::thread::spawn(move || {
            let mut bytes = [0; 4096];
            loop {
                match socket.recv_from(&mut bytes) {
                    Ok((n, from)) => {
                        if stop.load(Ordering::Relaxed) {
                            break;
                        }
                        records.lock().unwrap().push(bytes[..n].to_vec());
                        socket.send_to(&bytes[..n], from).unwrap();
                    }
                    Err(err)
                        if matches!(
                            err.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) =>
                    {
                        if stop.load(Ordering::Relaxed) {
                            break;
                        }
                    }
                    Err(err) => panic!("local datagram fixture: {err}"),
                }
            }
        });
        Self {
            address,
            stopped,
            thread: Some(thread),
            seen,
        }
    }
}
impl Drop for UdpFixture {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        let wake = UdpSocket::bind("127.0.0.1:0").unwrap();
        let _ = wake.send_to(b"", self.address);
        if let Some(thread) = self.thread.take()
            && let Err(error) = thread.join()
            && !std::thread::panicking()
        {
            std::panic::resume_unwind(error);
        }
    }
}

fn send(p: &mut Program, host: &str, port: u16, bytes: &[u8], expected: Vec<u8>) {
    handle(
        p,
        "UdpSocket.send_to",
        &[1, i32::from(port)],
        Some(&Enc::default().str(host).bytes(bytes).0),
        expected,
    );
}
fn datagram(bytes: &[u8], port: u16) -> Vec<u8> {
    Enc::default()
        .leb(0)
        .bytes(bytes)
        .str("127.0.0.1")
        .leb(u64::from(port))
        .0
}

#[test]
fn udp_binary_truncation_zero_receive_consumption_and_each_send_grant() {
    let scratch = Scratch::new();
    let server = UdpFixture::new();
    let port = server.address.port();
    let mut p = Program::new();
    open(&mut p, "bind_udp", "127.0.0.1", 0, vec![0, 1]);
    for (bytes, max, received) in [
        (b"abcd".as_slice(), 2, b"ab".as_slice()),
        (b"discard", 0, b""),
        (b"next", 4, b"next"),
        (b"", 9, b""),
    ] {
        send(&mut p, "127.0.0.1", port, bytes, vec![0]);
        handle(
            &mut p,
            "UdpSocket.receive",
            &[1, max],
            None,
            datagram(received, port),
        );
    }
    send(
        &mut p,
        "127.0.0.2",
        port,
        b"denied",
        operation_error(0, &format!("127.0.0.2:{port}")),
    );
    send(
        &mut p,
        "bad host",
        port,
        b"invalid",
        operation_error(1, &format!("bad host:{port}")),
    );
    handle(&mut p, "UdpSocket.close", &[1], None, vec![0]);
    handle(&mut p, "UdpSocket.receive", &[1, 1], None, vec![1, 1]);
    send(&mut p, "127.0.0.1", port, b"closed", vec![1, 1]);
    handle(&mut p, "UdpSocket.close", &[1], None, vec![1, 1]);
    assert_program(
        &scratch,
        p,
        vec![(
            "Net",
            Limit::Entries(vec!["127.0.0.1:0".into(), format!("127.0.0.1:{port}")]),
        )],
        || {},
    );
    assert_eq!(server.seen.lock().unwrap().len(), 8);
}

#[test]
fn net_refused_connect_and_occupied_tcp_udp_bind_addresses() {
    let scratch = Scratch::new();
    let occupied = TcpListener::bind("127.0.0.1:0").unwrap();
    let tcp_port = occupied.local_addr().unwrap().port();
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    let udp_port = udp.local_addr().unwrap().port();
    let reservation = RefusedPort::new();
    let refused = reservation.address.port();
    let mut p = Program::new();
    open(
        &mut p,
        "connect",
        "127.0.0.1",
        refused,
        net_error(3, &format!("127.0.0.1:{refused}")),
    );
    open(
        &mut p,
        "listen",
        "127.0.0.1",
        tcp_port,
        net_error(3, &format!("127.0.0.1:{tcp_port}")),
    );
    open(
        &mut p,
        "bind_udp",
        "127.0.0.1",
        udp_port,
        net_error(3, &format!("127.0.0.1:{udp_port}")),
    );
    assert_program(&scratch, p, vec![], || {});
}

#[test]
fn net_unknown_handles_and_stale_finish_calls_are_contract_panics() {
    let scratch = Scratch::new();
    for (method, args, payload) in [
        ("TcpStream.read", vec![99, 0], None),
        (
            "TcpStream.write",
            vec![99],
            Some(Enc::default().bytes(b"x").0),
        ),
        ("TcpStream.close", vec![99], None),
        ("TcpListener.accept", vec![99], None),
        ("TcpListener.close", vec![99], None),
        ("UdpSocket.receive", vec![99, 0], None),
        (
            "UdpSocket.send_to",
            vec![99, 9],
            Some(Enc::default().str("127.0.0.1").bytes(b"x").0),
        ),
        ("UdpSocket.close", vec![99], None),
    ] {
        let mut p = Program::new();
        handle(&mut p, method, &args, payload.as_deref(), vec![]);
        let (wasm, _) = p.finish(7);
        let r = parity(&scratch, &wasm, vec![], || {});
        assert_eq!(r.status, 3);
        assert_eq!(
            r.stderr,
            b"panic: host-contract: an operation on a handle the host never made\n"
        );
    }
    for method in [
        "lookup",
        "connect",
        "listen",
        "bind_udp",
        "TcpStream.read",
        "TcpStream.write",
        "TcpListener.accept",
        "UdpSocket.send_to",
        "UdpSocket.receive",
    ] {
        let mut p = Program::new();
        let call = p.import(
            "hd:Net",
            &format!("{method}.finish"),
            &[ValType::I32],
            &[ValType::I32],
        );
        p.instructions(&[I::I32Const(1), I::Call(call), I::Drop]);
        let (wasm, _) = p.finish(7);
        let r = parity(&scratch, &wasm, vec![], || {});
        assert_eq!(r.status, 3);
        assert_eq!(
            r.stderr,
            b"panic: host-contract: a finish of an operation that is not pending\n"
        );
    }
}

#[test]
fn closed_udp_disposed_precedes_denied_or_invalid_destination() {
    let scratch = Scratch::new();
    let mut p = Program::new();
    open(&mut p, "bind_udp", "127.0.0.1", 0, vec![0, 1]);
    handle(&mut p, "UdpSocket.close", &[1], None, vec![0]);
    send(&mut p, "127.0.0.2", 9, b"denied", vec![1, 1]);
    send(&mut p, "bad host", 9, b"invalid", vec![1, 1]);
    assert_program(
        &scratch,
        p,
        vec![("Net", Limit::Entries(vec!["127.0.0.1:0".into()]))],
        || {},
    );
}

#[test]
fn unknown_udp_handle_precedes_denied_or_invalid_destination() {
    let scratch = Scratch::new();
    for host in ["127.0.0.2", "bad host"] {
        let mut p = Program::new();
        handle(
            &mut p,
            "UdpSocket.send_to",
            &[99, 9],
            Some(&Enc::default().str(host).bytes(b"x").0),
            vec![],
        );
        let (wasm, _) = p.finish(7);
        let r = parity(
            &scratch,
            &wasm,
            vec![("Net", Limit::Entries(vec![]))],
            || {},
        );
        assert_eq!(r.status, 3);
        assert_eq!(
            r.stderr,
            b"panic: host-contract: an operation on a handle the host never made\n"
        );
    }
}

#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<u8>>>);
impl Write for Sink {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn ended_instance_stops_socket_owner_while_embedding_retains_instance() {
    retained_instance_stops_socket_owner(false);
}

#[test]
fn trapped_instance_stops_socket_owner_while_embedding_retains_instance() {
    retained_instance_stops_socket_owner(true);
}

fn retained_instance_stops_socket_owner(trap: bool) {
    let _spawn = CHILD_SPAWN.lock().unwrap();
    let (closed, peer_closed) = std::sync::mpsc::channel();
    let peer = TcpFixture::new(move |mut peer| {
        assert_eq!(
            peer.read(&mut [0]).unwrap(),
            0,
            "ending the guest stops its socket owner and closes the peer stream"
        );
        closed.send(()).unwrap();
    });
    let mut p = Program::new();
    open(&mut p, "listen", "127.0.0.1", 0, vec![0, 1]);
    open(&mut p, "bind_udp", "127.0.0.1", 0, vec![0, 2]);
    open(
        &mut p,
        "connect",
        "127.0.0.1",
        peer.address.port(),
        vec![0, 3],
    );
    if trap {
        p.instructions(&[I::Unreachable]);
    }
    // No explicit handle close: ending the instance stops the worker that
    // exclusively owns all three socket kinds. Observe stream EOF while
    // retaining the instance, without racing another test to rebind its ports.
    // Unit tests separately prove this owner's request receiver is dropped
    // after State::serve clears every handle. hd:rt.stderr has already emitted
    // the proof of all successful opens,
    // so a subsequent raw trap does not append another panic diagnostic.
    let (wasm, expected) = p.finish(7);
    let setup = HostSetup {
        providers: vec![],
        grants: Grants::default(),
        limits: Limits::default(),
        args: vec![],
        program: String::new(),
    };
    let engine = WasmtimeEngine::new(EngineConfig::default()).unwrap();
    let module = engine
        .load(&wasm, &hd_cache::MemoryStore::default())
        .unwrap();
    let output = Sink::default();
    let errors = Sink::default();
    let io = HostIo {
        out: Box::new(output.clone()),
        err: Box::new(errors.clone()),
        input: None,
    };
    let mut instance: Box<dyn Instance> =
        Box::new(engine.instantiate_with(&module, &setup, io).unwrap());
    assert_eq!(instance.init(), Outcome::Exit(if trap { 3 } else { 0 }));
    assert_eq!(
        hd_run::drive(instance.as_mut(), 5),
        Outcome::Exit(if trap { 3 } else { 7 })
    );
    assert_eq!(*errors.0.lock().unwrap(), expected.as_bytes());
    assert_eq!(*output.0.lock().unwrap(), b"core-parity\n");
    peer_closed
        .recv_timeout(Duration::from_secs(3))
        .expect("guest socket owner stops while its ended instance remains alive");
    drop(instance);
}

#[cfg(unix)]
#[test]
fn udp_ipv4_socket_to_numeric_ipv6_reports_exact_other_error() {
    let scratch = Scratch::new();
    let mut p = Program::new();
    open(&mut p, "bind_udp", "127.0.0.1", 0, vec![0, 1]);
    // A UDP4 socket cannot reach an IPv6 destination; this fails locally
    // before any datagram can leave the machine.
    send(
        &mut p,
        "::1",
        9,
        b"x",
        operation_error(4, "send EINVAL ::1:9"),
    );
    handle(&mut p, "UdpSocket.close", &[1], None, vec![0]);
    assert_program(&scratch, p, vec![], || {});
}
