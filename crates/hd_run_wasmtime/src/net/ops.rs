use std::io::{self, Write};
use std::net::Shutdown;
use std::sync::atomic::AtomicBool;
use std::thread;

use super::TICK;
use super::client::{NetError, Op, Outcome, Value};
use super::socket::{cancelled, resolve, socket_error, wrong_kind};
use super::state::{End, SocketKind, State, stream};
use super::validate::ip_literal;

impl State {
    pub(crate) fn handle_op(&mut self, op: &Op, stop: &AtomicBool) -> Outcome {
        let number = match op {
            Op::Read { handle, .. }
            | Op::Write { handle, .. }
            | Op::Send { handle, .. }
            | Op::Receive { handle, .. } => *handle,
            Op::Accept(handle) | Op::Close(handle) => *handle,
            _ => unreachable!("a handle operation"),
        };
        let Some(handle) = self.handles.get(&number) else {
            return Outcome::Unknown;
        };
        if handle.closed {
            return Outcome::Disposed;
        }
        if let Op::Close(_) = op {
            let handle = self.handles.get_mut(&number).expect("existing handle");
            match &mut handle.socket {
                SocketKind::Stream { socket, .. } => {
                    let _ = socket.shutdown(Shutdown::Write);
                }
                SocketKind::Listener { socket, .. } => {
                    socket.take();
                }
                SocketKind::Udp { socket, .. } => {
                    socket.take();
                }
            }
            handle.closed = true;
            return Outcome::Ok(Value::None);
        }
        if let Op::Send { host, port, .. } = op {
            let addresses = match resolve(host, *port, stop, self.deadline) {
                Ok(a) => a,
                Err(e) => return Outcome::Error(e),
            };
            let SocketKind::Udp {
                socket: Some(_),
                ipv6,
                ..
            } = &handle.socket
            else {
                return wrong_kind();
            };
            let address = if ip_literal(host) {
                addresses.into_iter().next()
            } else {
                addresses.into_iter().find(|a| a.is_ipv6() == *ipv6)
            };
            let Some(address) = address else {
                return Outcome::Error(NetError::new(2, host));
            };

            let Op::Send { bytes, .. } = op else {
                unreachable!()
            };
            loop {
                if cancelled(stop, self.deadline) {
                    return Outcome::Error(NetError::new(4, "the socket stopped"));
                }
                let SocketKind::Udp {
                    socket: Some(socket),
                    ..
                } = &self.handles.get(&number).expect("existing handle").socket
                else {
                    unreachable!()
                };
                match socket.send_to(bytes, address) {
                    Ok(_) => return Outcome::Ok(Value::None),
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) => {
                        return Outcome::Error(socket_error(
                            &error,
                            "send",
                            Some(address),
                            host,
                            *port,
                        ));
                    }
                }
                self.arrivals(stop);
                thread::sleep(TICK);
            }
        }
        let mut written = 0;
        loop {
            if cancelled(stop, self.deadline) {
                return Outcome::Error(NetError::new(4, "the socket stopped"));
            }
            self.arrivals(stop);
            let handle = self.handles.get_mut(&number).expect("existing handle");
            match (op, &mut handle.socket) {
                (
                    Op::Read { max, .. },
                    SocketKind::Stream {
                        inbox, rest, end, ..
                    },
                ) => {
                    if *max == 0 {
                        return Outcome::Ok(Value::Bytes(Vec::new()));
                    }
                    if rest.is_empty()
                        && let Some(chunk) = inbox.pop_front()
                    {
                        rest.extend(chunk);
                    }
                    if !rest.is_empty() {
                        let n = rest.len().min(*max as usize);
                        return Outcome::Ok(Value::Bytes(rest.drain(..n).collect()));
                    }
                    if let Some(why) = end {
                        return match why {
                            End::Eof => Outcome::Ok(Value::Bytes(Vec::new())),
                            End::Error(error) => Outcome::Error(error.clone()),
                        };
                    }
                }
                (Op::Write { bytes, .. }, SocketKind::Stream { socket, end, .. }) => {
                    if matches!(end, Some(End::Error(_))) {
                        return Outcome::Error(NetError::new(
                            4,
                            "cannot write to a failed TCP stream",
                        ));
                    }

                    if written == bytes.len() {
                        return Outcome::Ok(Value::None);
                    }
                    match socket.write(&bytes[written..]) {
                        Ok(0) => {
                            let error = socket_error(
                                &io::Error::from(io::ErrorKind::WriteZero),
                                "write",
                                None,
                                "",
                                0,
                            );
                            *end = Some(End::Error(error.clone()));
                            let _ = socket.shutdown(Shutdown::Both);
                            return Outcome::Error(error);
                        }
                        Ok(n) => {
                            written += n;
                            continue;
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                        Err(error) => {
                            let error = socket_error(&error, "write", None, "", 0);
                            *end = Some(End::Error(error.clone()));
                            let _ = socket.shutdown(Shutdown::Both);
                            return Outcome::Error(error);
                        }
                    }
                }
                (Op::Accept(_), SocketKind::Listener { inbox, end, .. }) => {
                    if let Some(socket) = inbox.pop_front() {
                        return self.open(stream(socket));
                    }
                    if let Some(error) = end {
                        return Outcome::Error(error.clone());
                    }
                }
                (Op::Receive { max, .. }, SocketKind::Udp { inbox, end, .. }) => {
                    if let Some((mut bytes, peer)) = inbox.pop_front() {
                        bytes.truncate(*max as usize);
                        return Outcome::Ok(Value::Datagram(bytes, peer));
                    }
                    if let Some(error) = end {
                        return Outcome::Error(error.clone());
                    }
                }
                _ => return wrong_kind(),
            }
            thread::sleep(TICK);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::codec::{Dec, Enc};
    use crate::net::Client;
    use crate::net::client::{Op, Outcome, Request};
    use crate::net::test_support::{
        assert_owner_stopped, host_args, opened, owner_requests, read_n, result_bytes,
    };
    use hd_run::{Grants, Limit};
    use socket2::Socket;
    use std::io::{Read, Write};
    use std::net::{Shutdown, TcpListener, UdpSocket};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn tcp_spillover_zero_read_eof_and_disposal_are_sticky() {
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        let writer = thread::spawn(move || {
            let (mut socket, _) = server.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            socket.write_all(b"abcdef").unwrap();
            socket.shutdown(Shutdown::Write).unwrap();
            let mut ack = [0; 1];
            socket.read_exact(&mut ack).unwrap();
            assert_eq!(ack, *b"x");
        });
        let mut client = Client::default();
        let handle = opened(&mut client, "connect", port);
        assert_eq!(read_n(&mut client, handle, 2, 2), b"ab");
        let empty = client
            .call("TcpStream.read", &[handle, 0], &[], &Grants::default())
            .unwrap();
        assert!(result_bytes(&empty).is_empty());
        assert_eq!(read_n(&mut client, handle, 4, i64::from(i32::MIN)), b"cdef");
        for _ in 0..2 {
            let eof = client
                .call("TcpStream.read", &[handle, 5], &[], &Grants::default())
                .unwrap();
            assert!(result_bytes(&eof).is_empty());
        }
        let actual = client
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
            client
                .call("TcpStream.close", &[handle], &[], &Grants::default())
                .unwrap()
                .done(),
            &[0]
        );
        for (method, params) in [
            ("TcpStream.close", vec![handle]),
            ("TcpStream.read", vec![handle, 1]),
        ] {
            assert_eq!(
                client
                    .call(method, &params, &[], &Grants::default())
                    .unwrap()
                    .done(),
                &[1, 1]
            );
        }
        client.stop();
        writer.join().unwrap();
    }
    #[test]
    fn udp_receives_one_datagram_and_zero_max_still_consumes_one() {
        let peer = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = peer.local_addr().unwrap().port();
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let server = thread::spawn(move || {
            let mut bytes = [0; 16];
            let (n, from) = peer.recv_from(&mut bytes).unwrap();
            assert_eq!(&bytes[..n], b"ping");
            peer.send_to(b"hello", from).unwrap();
            peer.send_to(b"", from).unwrap();
            peer.send_to(b"wide", from).unwrap();
        });
        let mut client = Client::default();
        let handle = opened(&mut client, "bind_udp", 0);
        let requests = owner_requests(&client);
        let mut args = host_args("127.0.0.1");
        args.bytes(b"ping");
        assert_eq!(
            client
                .call(
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
            let actual = client
                .call("UdpSocket.receive", &[handle, max], &[], &Grants::default())
                .unwrap();
            let mut d = Dec::new(actual.done());
            assert_eq!(d.leb().unwrap(), 0);
            assert_eq!(d.bytes().unwrap(), bytes);
            assert_eq!(d.str().unwrap(), "127.0.0.1");
            assert_eq!(d.leb().unwrap(), u64::from(port));
        }
        server.join().unwrap();
        client.stop();
        assert_owner_stopped(&client, &requests);
    }
    #[test]
    fn stop_wakes_a_pending_nonblocking_read_and_joins_the_owner() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut client = Client::default();
        let handle = opened(&mut client, "connect", port);
        let (mut peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let (reply, result) = mpsc::channel();
        client
            .worker
            .as_ref()
            .unwrap()
            .requests
            .send(Request {
                op: Op::Read { handle, max: 1 },
                reply,
                deadline: None,
            })
            .unwrap();
        client.stop();
        assert!(matches!(
            result.recv_timeout(Duration::from_secs(1)),
            Ok(Outcome::Error(_)) | Err(mpsc::RecvTimeoutError::Disconnected)
        ));
        assert_eq!(peer.read(&mut [0; 1]).unwrap(), 0);
    }
    #[test]
    fn closed_udp_send_is_disposed_before_denied_or_invalid_destination() {
        let mut client = Client::default();
        let handle = opened(&mut client, "bind_udp", 0);
        assert_eq!(
            client
                .call("UdpSocket.close", &[handle], &[], &Grants::default())
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
                client
                    .call("UdpSocket.send_to", &[handle, 9, 0], args.done(), &denied)
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
        let (ack, ready) = mpsc::channel();
        let peer = thread::spawn(move || {
            let (mut stream, _) = server.accept().unwrap();
            stream.write_all(b"abc").unwrap();
            ready.recv_timeout(Duration::from_secs(2)).unwrap();
            let socket = Socket::from(stream);
            socket.set_linger(Some(Duration::ZERO)).unwrap();
        });
        let mut client = Client::default();
        let handle = opened(&mut client, "connect", port);
        assert_eq!(read_n(&mut client, handle, 3, 3), b"abc");
        ack.send(()).unwrap();
        peer.join().unwrap();
        let first = client
            .call("TcpStream.read", &[handle, 3], &[], &Grants::default())
            .unwrap();
        let mut decoded = Dec::new(first.done());
        assert_eq!(decoded.leb().unwrap(), 1);
        assert_eq!(decoded.leb().unwrap(), 0);
        assert_eq!(decoded.leb().unwrap(), 4);
        assert!(decoded.str().unwrap().starts_with("read: "));
        let again = client
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
            .leb(4)
            .str("cannot write to a failed TCP stream");
        assert_eq!(
            client
                .call(
                    "TcpStream.write",
                    &[handle, 0],
                    args.done(),
                    &Grants::default()
                )
                .unwrap()
                .done(),
            expected.done()
        );
    }
}
