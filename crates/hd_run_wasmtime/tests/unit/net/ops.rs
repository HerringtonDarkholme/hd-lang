mod tests {
    use crate::net::test_support::{reserved_refused_port, runtime};
    use hd_run_wasmtime::internals::net::ops::{NetError, connect_candidates};
    use std::net::TcpListener;

    fn queued_udp_after_timeout() -> (
        tokio::runtime::Runtime,
        std::sync::Arc<hd_run_wasmtime::internals::net::ops::Udp>,
        std::net::UdpSocket,
    ) {
        use std::sync::Arc;
        use std::time::{Duration, Instant};

        let runtime = runtime();
        let socket = runtime.block_on(async {
            let socket = hd_run_wasmtime::internals::net::ops::Udp {
                socket: tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap(),
                ipv6: false,
                unread: std::sync::Mutex::default(),
            };
            let socket = Arc::new(socket);
            let expired = hd_run_wasmtime::internals::net::ops::Job::Receive {
                socket: Arc::clone(&socket),
                handle: 1,
                max: 8,
                bound: Some(Duration::from_millis(50)),
            }
            .run()
            .await;
            assert!(matches!(expired.result, Err(NetError::Timeout)));
            socket
        });
        let peer = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let address = socket.socket.local_addr().unwrap();
        peer.send_to(b"queued", address).unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        let mut probe = [std::mem::MaybeUninit::<u8>::uninit(); 6];
        loop {
            match socket2::SockRef::from(&socket.socket).peek_from(&mut probe) {
                Ok((n, from)) => {
                    assert_eq!(n, 6);
                    assert_eq!(from.as_socket(), Some(peer.local_addr().unwrap()));
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < until, "the datagram reaches the kernel");
                    std::thread::yield_now();
                }
                Err(error) => panic!("peeking at the queued datagram: {error}"),
            }
        }
        (runtime, socket, peer)
    }

    #[test]
    fn expired_udp_receive_probes_kernel_queue_without_cached_readiness() {
        let (runtime, socket, peer) = queued_udp_after_timeout();
        // Handle::block_on does not drive this current-thread runtime's
        // I/O. The non-waiting deadline probe must inspect the kernel even
        // when Tokio has not observed the newly queued datagram yet.
        let received = runtime
            .handle()
            .block_on(hd_run_wasmtime::internals::net::ops::receive_now(&socket));
        let (bytes, from) = received.expect("a queued datagram is not a timeout");
        assert_eq!(bytes, b"queued");
        assert_eq!(from, peer.local_addr().unwrap());
    }

    #[test]
    fn udp_receive_entry_reads_kernel_queue_without_cached_readiness() {
        use std::task::{Context, Poll, Waker};

        let (runtime, socket, peer) = queued_udp_after_timeout();
        let _entered = runtime.enter();
        let mut receive = std::pin::pin!(hd_run_wasmtime::internals::net::ops::receive(&socket));
        let mut context = Context::from_waker(Waker::noop());
        let Poll::Ready(received) = receive.as_mut().poll(&mut context) else {
            panic!("queued input must be probed before waiting for readiness");
        };
        let (bytes, from) = received.expect("the queued receive completes");
        assert_eq!(bytes, b"queued");
        assert_eq!(from, peer.local_addr().unwrap());
    }

    #[test]
    fn connection_candidates_advance_on_local_refusal_and_keep_the_first_error() {
        // Keep both established pairs, so their source ports stay reserved
        // with no listener through every attempt.
        let (refused_address, _refused) = reserved_refused_port();
        let (also_refused_address, _also_refused) = reserved_refused_port();
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = server.local_addr().unwrap();
        let runtime = runtime();
        let socket = runtime
            .block_on(connect_candidates(
                vec![refused_address, address],
                "localhost",
                address.port(),
            ))
            .unwrap_or_else(|e| panic!("connection failed: {e:?}"));
        assert_eq!(socket.peer_addr().unwrap(), address);
        let (_, _) = server.accept().unwrap();
        drop(socket);
        let error = runtime
            .block_on(connect_candidates(
                vec![refused_address, also_refused_address],
                "localhost",
                address.port(),
            ))
            .unwrap_err();
        // The error kept is the first candidate's. Where a closed port
        // refuses at once, that is `Refused`. Windows refuses a closed
        // loopback port only after about two seconds, past the 250 ms
        // attempt timer, so there the first candidate's error is its
        // timeout: an `Other` that names the first address.
        match error {
            NetError::Refused { address: shown } => {
                assert_eq!(shown, format!("localhost:{}", address.port()));
            }
            NetError::Other { message } => assert!(
                message.starts_with(&format!("connect {refused_address}: ")),
                "{message}"
            ),
            error => panic!("unexpected error {error:?}"),
        }
    }
}

mod queued_tests {
    use crate::net::test_support::runtime;
    use hd_run_wasmtime::internals::net::ops::{Got, accept_now, read_now, receive_now};
    use hd_run_wasmtime::internals::net::socket::{Bound, Stream, bind};
    use std::io::Write;
    use std::net::{TcpListener, TcpStream, UdpSocket};
    use std::time::Duration;

    #[test]
    fn queued_tcp_bytes_are_visible_before_the_driver_observes_readiness() {
        let runtime = runtime();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (socket, _) = listener.accept().unwrap();
        socket.set_nonblocking(true).unwrap();
        let stream = {
            let _enter = runtime.enter();
            Stream::new(tokio::net::TcpStream::from_std(socket).unwrap())
        };
        peer.write_all(b"queued").unwrap();
        std::thread::sleep(Duration::from_millis(50));
        // The single-thread runtime has not run since registration.
        let got = runtime.block_on(read_now(&stream, 8)).unwrap();
        assert_eq!(got, b"queued");
    }

    #[test]
    fn queued_udp_datagram_is_visible_before_the_driver_observes_readiness() {
        let runtime = runtime();
        let socket = {
            let _enter = runtime.enter();
            let Bound::Udp(socket) = bind(true, "127.0.0.1:0".parse().unwrap()).unwrap() else {
                panic!("UDP socket")
            };
            socket
        };
        let peer = UdpSocket::bind("127.0.0.1:0").unwrap();
        peer.send_to(b"queued", socket.socket.local_addr().unwrap())
            .unwrap();
        std::thread::sleep(Duration::from_millis(50));
        let (got, address) = runtime.block_on(receive_now(&socket)).unwrap();
        assert_eq!(got, b"queued");
        assert_eq!(address, peer.local_addr().unwrap());
    }

    #[test]
    fn queued_connection_is_visible_before_the_driver_observes_readiness() {
        let runtime = runtime();
        let listener = {
            let _enter = runtime.enter();
            let Bound::Listener(socket) = bind(false, "127.0.0.1:0".parse().unwrap()).unwrap()
            else {
                panic!("listener")
            };
            socket
        };
        let _peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        let got = runtime.block_on(accept_now(&listener)).unwrap();
        assert!(matches!(got, Got::Opened(_)));
    }
}
