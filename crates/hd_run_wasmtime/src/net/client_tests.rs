//! `Client`'s handle table and each operation's `.start` and `.finish`.

use super::Entry;
use crate::codec::{Dec, Enc};
use crate::host::Stop;
use crate::net::test_support::{Net, Started, host_args, result_bytes};
use hd_run::{Grants, Limit};
use socket2::Socket;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream, UdpSocket};
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

fn pending(started: Started) -> i32 {
    match started {
        Started::Pending(h) => h,
        Started::Ready(e) => panic!("completed at once: {:?}", e.done()),
    }
}

fn ready(started: Started) -> Enc {
    match started {
        Started::Ready(e) => e,
        Started::Pending(h) => panic!("operation {h} waits"),
    }
}

fn read(net: &mut Net, handle: i64, max: i64) -> Started {
    net.start("TcpStream.read", &[handle, max], &[], &Grants::default())
        .unwrap()
}

#[test]
fn refusals_complete_at_once_and_unknown_handles_precede_destination_checks() {
    let mut net = Net::new();
    let grants = Grants {
        limits: vec![("Net".into(), Limit::Deny)],
    };
    let mut expected = Enc::default();
    expected.leb(1).leb(0).str("127.0.0.1:9");
    let refused = net
        .start("connect", &[9, 0], host_args("127.0.0.1").done(), &grants)
        .unwrap();
    assert_eq!(ready(refused).done(), expected.done());
    for (mut bytes, message) in [
        (
            host_args("127.0.0.1"),
            "an operation on a handle the host never made",
        ),
        (
            host_args("bad host"),
            "an operation on a handle the host never made",
        ),
        // Every argument decodes before the handle is looked up.
        (Enc::default(), "an argument does not decode"),
    ] {
        if !bytes.done().is_empty() {
            bytes.bytes(b"payload");
        }
        let error = net
            .start("UdpSocket.send_to", &[123, 9, 0], bytes.done(), &grants)
            .err()
            .unwrap();
        assert_eq!(error.downcast_ref::<Stop>().unwrap().to_string(), message);
    }
    let error = net
        .start("TcpStream.close", &[123], &[], &Grants::default())
        .err()
        .unwrap();
    assert_eq!(
        error.downcast_ref::<Stop>().unwrap().to_string(),
        "an operation on a handle the host never made"
    );
    assert!(!net.reactor.waiting(), "nothing waits");
}

#[test]
fn tcp_partial_reads_zero_read_eof_and_disposal_are_sticky() {
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = server.local_addr().unwrap().port();
    let writer = thread::spawn(move || {
        let (mut socket, _) = server.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        socket.write_all(b"abcdef").unwrap();
        socket.shutdown(Shutdown::Write).unwrap();
        let mut ack = [0; 1];
        socket.read_exact(&mut ack).unwrap();
        assert_eq!(ack, *b"x");
    });
    let mut net = Net::new();
    let handle = net.opened("connect", port);
    assert_eq!(net.read_n(handle, 2, 2), b"ab");
    assert!(result_bytes(&ready(read(&mut net, handle, 0))).is_empty());
    assert_eq!(net.read_n(handle, 4, i64::from(i32::MIN)), b"cdef");
    for _ in 0..2 {
        let eof = net
            .call("TcpStream.read", &[handle, 5], &[], &Grants::default())
            .unwrap();
        assert!(result_bytes(&eof).is_empty());
    }
    let actual = net
        .call(
            "TcpStream.write",
            &[handle, 0],
            Enc::default().bytes(b"x").done(),
            &Grants::default(),
        )
        .unwrap();
    assert_eq!(
        actual.done(),
        &[0],
        "read EOF does not close the write half"
    );
    assert_eq!(
        net.call("TcpStream.close", &[handle], &[], &Grants::default())
            .unwrap()
            .done(),
        &[0]
    );
    for (method, params) in [
        ("TcpStream.close", vec![handle]),
        ("TcpStream.read", vec![handle, 1]),
    ] {
        assert_eq!(
            net.call(method, &params, &[], &Grants::default())
                .unwrap()
                .done(),
            &[1, 1]
        );
    }
    writer.join().unwrap();
    net.client.stop();
}

#[test]
fn udp_receives_one_datagram_and_zero_max_still_consumes_one() {
    let peer = UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = peer.local_addr().unwrap().port();
    peer.set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let server = thread::spawn(move || {
        let mut bytes = [0; 16];
        let (n, from) = peer.recv_from(&mut bytes).unwrap();
        assert_eq!(&bytes[..n], b"ping");
        peer.send_to(b"hello", from).unwrap();
        peer.send_to(b"", from).unwrap();
        peer.send_to(b"wide", from).unwrap();
    });
    let mut net = Net::new();
    let handle = net.opened("bind_udp", 0);
    let mut args = host_args("127.0.0.1");
    args.bytes(b"ping");
    assert_eq!(
        net.call(
            "UdpSocket.send_to",
            &[handle, i64::from(port), 0],
            args.done(),
            &Grants::default()
        )
        .unwrap()
        .done(),
        &[0]
    );
    for (max, bytes) in [
        (2, &b"he"[..]),
        (0, &b""[..]),
        (i64::from(i32::MIN), &b"wide"[..]),
    ] {
        let actual = net
            .call("UdpSocket.receive", &[handle, max], &[], &Grants::default())
            .unwrap();
        let mut d = Dec::new(actual.done());
        assert_eq!(d.leb().unwrap(), 0);
        assert_eq!(d.bytes().unwrap(), bytes);
        assert_eq!(d.str().unwrap(), "127.0.0.1");
        assert_eq!(d.leb().unwrap(), u64::from(port));
    }
    server.join().unwrap();
    net.client.stop();
}

/// `std-net.cancel.read`: a read cancelled while it waits takes nothing,
/// and one cancelled after it completed, before its `.finish`, gives
/// its bytes back. Either way the next reads return the first bytes.
#[test]
fn a_cancelled_read_leaves_its_bytes_for_the_next_read() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut net = Net::new();
    let handle = net.opened("connect", listener.local_addr().unwrap().port());
    let (mut peer, _) = listener.accept().unwrap();
    let waiting = pending(read(&mut net, handle, 8));
    net.abort(waiting);
    peer.write_all(b"abc").unwrap();
    assert_eq!(net.read_n(handle, 3, 8), b"abc");
    let completed = pending(read(&mut net, handle, 8));
    peer.write_all(b"xyz").unwrap();
    net.wait_for(completed);
    net.abort(completed);
    assert_eq!(result_bytes(&ready(read(&mut net, handle, 2))), b"xy");
    assert_eq!(result_bytes(&ready(read(&mut net, handle, 8))), b"z");
    assert!(!net.reactor.waiting());
    net.client.stop();
}

/// As a read, a receive cancelled after it completed gives its whole
/// datagram back for the next receive.
#[test]
fn a_cancelled_receive_leaves_its_datagram_for_the_next_receive() {
    let peer = UdpSocket::bind("127.0.0.1:0").unwrap();
    let mut net = Net::new();
    let handle = net.opened("bind_udp", 0);
    let mut args = host_args("127.0.0.1");
    args.bytes(b"hi");
    net.call(
        "UdpSocket.send_to",
        &[handle, i64::from(peer.local_addr().unwrap().port()), 0],
        args.done(),
        &Grants::default(),
    )
    .unwrap();
    let mut bytes = [0; 8];
    let (_, address) = peer.recv_from(&mut bytes).unwrap();
    let receive = |net: &mut Net, max: i64| {
        net.start("UdpSocket.receive", &[handle, max], &[], &Grants::default())
            .unwrap()
    };
    let waiting = pending(receive(&mut net, 8));
    net.abort(waiting);
    let completed = pending(receive(&mut net, 1));
    peer.send_to(b"datagram", address).unwrap();
    net.wait_for(completed);
    net.abort(completed);
    let kept = ready(receive(&mut net, 8));
    let mut d = Dec::new(kept.done());
    assert_eq!(d.leb().unwrap(), 0);
    assert_eq!(d.bytes().unwrap(), b"datagram");
    net.client.stop();
}

/// `std-host.op.wait`: an accept and a connect to it wait together, so
/// both complete.
#[test]
fn an_accept_and_a_connect_to_it_complete_together() {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut net = Net::new();
    let listener = net.opened("listen", port);
    let accept = pending(
        net.start("TcpListener.accept", &[listener], &[], &Grants::default())
            .unwrap(),
    );
    let connect = pending(
        net.start(
            "connect",
            &[i64::from(port), 0],
            host_args("127.0.0.1").done(),
            &Grants::default(),
        )
        .unwrap(),
    );
    net.wait_for(accept);
    net.wait_for(connect);
    assert_eq!(net.finish(accept).done(), &[0, 2]);
    assert_eq!(net.finish(connect).done(), &[0, 3]);
    let mut args = Enc::default();
    args.bytes(b"ok");
    net.call("TcpStream.write", &[3, 0], args.done(), &Grants::default())
        .unwrap();
    assert_eq!(net.read_n(2, 2, 8), b"ok");
    net.client.stop();
}

/// An abort ends a waiting accept, receive or backpressured write at
/// once, and the socket goes on working.
#[test]
fn an_abort_ends_a_waiting_accept_receive_or_write_at_once() {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut net = Net::new();
    let listener = net.opened("listen", port);
    let udp = net.opened("bind_udp", 0);
    let accept = pending(
        net.start("TcpListener.accept", &[listener], &[], &Grants::default())
            .unwrap(),
    );
    let receive = pending(
        net.start("UdpSocket.receive", &[udp, 8], &[], &Grants::default())
            .unwrap(),
    );
    let start = Instant::now();
    net.abort(accept);
    net.abort(receive);
    assert!(start.elapsed() < Duration::from_secs(1));
    // The listener still accepts: the connection waited in the kernel.
    let client = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    assert_eq!(
        net.call("TcpListener.accept", &[listener], &[], &Grants::default())
            .unwrap()
            .done(),
        &[0, 3]
    );
    let stream = net.opened("connect", server.local_addr().unwrap().port());
    let (peer, _) = server.accept().unwrap();
    socket2::SockRef::from(&peer)
        .set_recv_buffer_size(4096)
        .unwrap();
    let mut payload = Enc::default();
    payload.bytes(&vec![b'x'; 16 * 1024 * 1024]);
    let write = pending(
        net.start(
            "TcpStream.write",
            &[stream, 0],
            payload.done(),
            &Grants::default(),
        )
        .unwrap(),
    );
    let start = Instant::now();
    net.abort(write);
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(!net.reactor.waiting());
    drop(client);
    net.client.stop();
}

/// The instance's end closes a stream whose read still waits, so the
/// peer reads the end at once.
#[test]
fn stop_closes_a_stream_with_a_waiting_read() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut net = Net::new();
    let handle = net.opened("connect", listener.local_addr().unwrap().port());
    let (mut peer, _) = listener.accept().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let waiting = pending(read(&mut net, handle, 1));
    net.client.stop();
    assert_eq!(peer.read(&mut [0; 1]).unwrap(), 0);
    net.abort(waiting);
}

fn disposed() -> &'static [u8] {
    &[1, 1]
}

/// `std-net.close.release` and `std-net.close.pending`: `close` ends a
/// waiting read with `Disposed`, drops the socket at once, and the peer
/// reads the end at once.
#[test]
fn close_ends_a_waiting_read_and_releases_the_socket() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut net = Net::new();
    let handle = net.opened("connect", listener.local_addr().unwrap().port());
    let (mut peer, _) = listener.accept().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let socket = match net.client.handles.get(&handle) {
        Some(Entry::Stream(stream)) => Arc::downgrade(stream),
        _ => panic!("a stream handle"),
    };
    let waiting = pending(read(&mut net, handle, 1));
    let close = net
        .start("TcpStream.close", &[handle], &[], &Grants::default())
        .unwrap();
    assert_eq!(ready(close).done(), &[0]);
    assert!(socket.upgrade().is_none(), "the socket is dropped");
    assert_eq!(peer.read(&mut [0; 1]).unwrap(), 0);
    net.wait_for(waiting);
    assert_eq!(net.finish(waiting).done(), disposed());
    assert!(!net.reactor.waiting());
    for method in ["TcpStream.read", "TcpStream.close"] {
        assert_eq!(
            net.call(method, &[handle, 1], &[], &Grants::default())
                .unwrap()
                .done(),
            disposed()
        );
    }
}

/// A read and a write both pending, and a read that completed before
/// its `.finish`, all end with `Disposed`.
#[test]
fn close_disposes_every_operation_not_yet_finished() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut net = Net::new();
    let handle = net.opened("connect", listener.local_addr().unwrap().port());
    let (mut peer, _) = listener.accept().unwrap();
    let completed = pending(read(&mut net, handle, 8));
    peer.write_all(b"abc").unwrap();
    net.wait_for(completed);
    let waiting = pending(read(&mut net, handle, 8));
    net.call("TcpStream.close", &[handle], &[], &Grants::default())
        .unwrap();
    for op in [completed, waiting] {
        net.wait_for(op);
        assert_eq!(net.finish(op).done(), disposed());
    }
    assert!(!net.reactor.waiting());
}

/// A closed listener ends its waiting accept with `Disposed` and frees
/// its port, so a new `listen!` on it succeeds.
#[test]
fn close_ends_a_waiting_accept_and_frees_the_port() {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut net = Net::new();
    let listener = net.opened("listen", port);
    let accept = pending(
        net.start("TcpListener.accept", &[listener], &[], &Grants::default())
            .unwrap(),
    );
    net.call("TcpListener.close", &[listener], &[], &Grants::default())
        .unwrap();
    net.wait_for(accept);
    assert_eq!(net.finish(accept).done(), disposed());
    assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
    let again = net.opened("listen", port);
    assert_ne!(again, listener);
    net.client.stop();
}

#[test]
fn close_ends_a_waiting_receive_and_frees_the_port() {
    let mut net = Net::new();
    let socket = net.opened("bind_udp", 0);
    let port = match net.client.handles.get(&socket) {
        Some(Entry::Udp(udp)) => udp.socket.local_addr().unwrap().port(),
        _ => panic!("a udp handle"),
    };
    let receive = pending(
        net.start("UdpSocket.receive", &[socket, 8], &[], &Grants::default())
            .unwrap(),
    );
    net.call("UdpSocket.close", &[socket], &[], &Grants::default())
        .unwrap();
    net.wait_for(receive);
    assert_eq!(net.finish(receive).done(), disposed());
    UdpSocket::bind(("127.0.0.1", port)).unwrap();
}

#[test]
fn closed_udp_send_is_disposed_before_denied_or_invalid_destination() {
    let mut net = Net::new();
    let handle = net.opened("bind_udp", 0);
    assert_eq!(
        net.call("UdpSocket.close", &[handle], &[], &Grants::default())
            .unwrap()
            .done(),
        &[0]
    );
    let denied = Grants {
        limits: vec![("Net".into(), Limit::Deny)],
    };
    for host in ["127.0.0.1", "invalid/host"] {
        let mut args = host_args(host);
        args.bytes(b"x");
        assert_eq!(
            net.call("UdpSocket.send_to", &[handle, 9, 0], args.done(), &denied)
                .unwrap()
                .done(),
            &[1, 1]
        );
    }
}

#[test]
fn tcp_reset_is_sticky_after_queued_bytes_are_delivered() {
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = server.local_addr().unwrap().port();
    let (ack, ready_to_reset) = mpsc::channel();
    let peer = thread::spawn(move || {
        let (mut stream, _) = server.accept().unwrap();
        stream.write_all(b"abc").unwrap();
        ready_to_reset
            .recv_timeout(Duration::from_secs(10))
            .unwrap();
        let socket = Socket::from(stream);
        socket.set_linger(Some(Duration::ZERO)).unwrap();
    });
    let mut net = Net::new();
    let handle = net.opened("connect", port);
    assert_eq!(net.read_n(handle, 3, 3), b"abc");
    ack.send(()).unwrap();
    peer.join().unwrap();
    let first = net
        .call("TcpStream.read", &[handle, 3], &[], &Grants::default())
        .unwrap();
    let mut decoded = Dec::new(first.done());
    assert_eq!(decoded.leb().unwrap(), 1);
    assert_eq!(decoded.leb().unwrap(), 0);
    assert_eq!(decoded.leb().unwrap(), 5);
    assert!(decoded.str().unwrap().starts_with("read: "));
    let again = net
        .call("TcpStream.read", &[handle, 3], &[], &Grants::default())
        .unwrap();
    assert_eq!(
        again.done(),
        first.done(),
        "the native read error remains sticky"
    );
    let mut args = Enc::default();
    args.bytes(b"x");
    let mut expected = Enc::default();
    expected
        .leb(1)
        .leb(0)
        .leb(5)
        .str("cannot write to a failed TCP stream");
    assert_eq!(
        net.call(
            "TcpStream.write",
            &[handle, 0],
            args.done(),
            &Grants::default()
        )
        .unwrap()
        .done(),
        expected.done()
    );
    net.client.stop();
}
/// The stream of `handle`, to read an option back from the socket.
fn stream_of(net: &Net, handle: i64) -> Arc<super::Stream> {
    match net.client.handles.get(&handle) {
        Some(Entry::Stream(stream)) => Arc::clone(stream),
        _ => panic!("a stream handle"),
    }
}

fn some_millis(millis: i64) -> Enc {
    crate::codec::encode(&Some(crate::abi::Duration { millis }))
}

/// `std-net.option.nodelay`, `std-net.option.keepalive` and
/// `std-net.option.default`: a connected or accepted stream starts at
/// the system default, each option reads back from the socket, and a
/// closed handle is `Disposed`.
#[test]
fn nodelay_and_keepalive_set_the_socket_and_start_at_the_default() {
    use socket2::SockRef;
    let mut net = Net::new();
    let listener = net.opened("listen", 0);
    let port = match net.client.handles.get(&listener) {
        Some(Entry::Listener(l)) => l.local_addr().unwrap().port(),
        _ => panic!("a listener handle"),
    };
    let grants = Grants::default();
    let client = net.opened("connect", port);
    let accepted = {
        let encoded = net
            .call("TcpListener.accept", &[listener], &[], &grants)
            .unwrap();
        let mut d = Dec::new(encoded.done());
        assert_eq!(d.leb().unwrap(), 0);
        i64::try_from(d.leb().unwrap()).unwrap()
    };
    for handle in [client, accepted] {
        let socket = stream_of(&net, handle);
        assert!(!socket.socket.nodelay().unwrap(), "nodelay starts off");
        assert!(
            !SockRef::from(&socket.socket).keepalive().unwrap(),
            "keep-alive starts off"
        );
    }
    let flag = |net: &mut Net, enabled: i64| {
        net.call("TcpStream.set_nodelay", &[client, enabled], &[], &grants)
            .unwrap()
    };
    assert_eq!(flag(&mut net, 1).done(), &[0]);
    assert!(stream_of(&net, client).socket.nodelay().unwrap());
    assert!(!stream_of(&net, accepted).socket.nodelay().unwrap());
    assert_eq!(flag(&mut net, 0).done(), &[0]);
    assert!(!stream_of(&net, client).socket.nodelay().unwrap());
    let keepalive = |net: &mut Net, idle: &Enc| {
        net.call("TcpStream.set_keepalive", &[client], idle.done(), &grants)
            .unwrap()
    };
    assert_eq!(keepalive(&mut net, &some_millis(30_000)).done(), &[0]);
    let raw = stream_of(&net, client);
    assert!(SockRef::from(&raw.socket).keepalive().unwrap());
    #[cfg(unix)]
    assert_eq!(
        SockRef::from(&raw.socket).tcp_keepalive_time().unwrap(),
        Duration::from_secs(30)
    );
    assert!(
        !SockRef::from(&stream_of(&net, accepted).socket)
            .keepalive()
            .unwrap()
    );
    assert_eq!(
        keepalive(
            &mut net,
            &crate::codec::encode(&None::<crate::abi::Duration>)
        )
        .done(),
        &[0]
    );
    assert!(!SockRef::from(&raw.socket).keepalive().unwrap());
    // A listener is not a stream.
    let wrong = net
        .call("TcpStream.set_nodelay", &[listener, 1], &[], &grants)
        .unwrap();
    assert_eq!(
        wrong.done(),
        super::reply(super::Outcome::Error(super::wrong_kind()), true)
            .unwrap()
            .done()
    );
    net.call("TcpStream.close", &[client], &[], &grants)
        .unwrap();
    assert_eq!(flag(&mut net, 1).done(), disposed());
    assert_eq!(keepalive(&mut net, &some_millis(1000)).done(), disposed());
    net.client.stop();
}

/// `std-net.local-address`: a port-0 listener and UDP socket report the
/// port the system picked, a client connects to it, and a closed handle
/// is `Disposed`.
#[test]
fn local_address_reports_the_picked_port() {
    let mut net = Net::new();
    let grants = Grants::default();
    let listener = net.opened("listen", 0);
    let udp = net.opened("bind_udp", 0);
    let report = |net: &mut Net, method: &str, handle: i64| {
        let encoded = net.call(method, &[handle], &[], &grants).unwrap();
        let mut d = Dec::new(encoded.done());
        assert_eq!(d.leb().unwrap(), 0);
        let host = <String as crate::codec::Wire>::dec(&mut d).unwrap();
        (host, u16::try_from(d.leb().unwrap()).unwrap())
    };
    let (host, port) = report(&mut net, "TcpListener.local_address", listener);
    assert_eq!(host, "127.0.0.1");
    assert_ne!(port, 0);
    let actual = match net.client.handles.get(&listener) {
        Some(Entry::Listener(l)) => l.local_addr().unwrap().port(),
        _ => panic!("a listener handle"),
    };
    assert_eq!(port, actual);
    // The reported port accepts a connection.
    net.opened("connect", port);
    let (host, udp_port) = report(&mut net, "UdpSocket.local_address", udp);
    assert_eq!(host, "127.0.0.1");
    assert_ne!(udp_port, 0);
    // A UDP socket is not a listener.
    let wrong = net
        .call("TcpListener.local_address", &[udp], &[], &grants)
        .unwrap();
    assert_eq!(
        wrong.done(),
        super::reply(super::Outcome::Error(super::wrong_kind()), true)
            .unwrap()
            .done()
    );
    for (method, handle) in [("TcpListener", listener), ("UdpSocket", udp)] {
        net.call(&format!("{method}.close"), &[handle], &[], &grants)
            .unwrap();
        let closed = net
            .call(&format!("{method}.local_address"), &[handle], &[], &grants)
            .unwrap();
        assert_eq!(closed.done(), disposed());
    }
    net.client.stop();
}

/// `std-net.listen.reuse`: a port with old connections in `TIME_WAIT`
/// rebinds at once, and two live listeners never share an address.
#[cfg(unix)]
#[test]
fn listen_reuses_a_time_wait_port_and_refuses_a_live_one() {
    let mut net = Net::new();
    let grants = Grants::default();
    let listener = net.opened("listen", 0);
    let port = match net.client.handles.get(&listener) {
        Some(Entry::Listener(l)) => l.local_addr().unwrap().port(),
        _ => panic!("a listener handle"),
    };
    let mut second = Enc::default();
    second.str("127.0.0.1");
    let refused = net
        .call("listen", &[i64::from(port), 0], second.done(), &grants)
        .unwrap();
    let mut d = Dec::new(refused.done());
    assert_eq!(d.leb().unwrap(), 1, "an error");
    assert_eq!(d.leb().unwrap(), 3, "Refused");
    // The listener's side closes first, which leaves it in TIME_WAIT.
    let mut peer = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let accepted = {
        let encoded = net
            .call("TcpListener.accept", &[listener], &[], &grants)
            .unwrap();
        let mut d = Dec::new(encoded.done());
        assert_eq!(d.leb().unwrap(), 0);
        i64::try_from(d.leb().unwrap()).unwrap()
    };
    net.call("TcpStream.close", &[accepted], &[], &grants)
        .unwrap();
    assert_eq!(peer.read(&mut [0; 1]).unwrap(), 0);
    drop(peer);
    net.call("TcpListener.close", &[listener], &[], &grants)
        .unwrap();
    let again = net.opened("listen", port);
    assert_ne!(again, listener);
    net.client.stop();
}
