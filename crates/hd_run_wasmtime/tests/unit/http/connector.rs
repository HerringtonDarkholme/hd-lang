mod tests {
    use crate::http::test_support::{
        bounded_accept, non_listening_socket, read_request, write_response_with_budget,
    };
    use hd_run_wasmtime::internals::http::connector::{
        HttpResolver, HttpTcpConnector, connect_candidates, resolve_lookup,
    };
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    use ureq::unversioned::resolver::Resolver;
    use ureq::unversioned::transport::{Connector, NextTimeout, RustlsConnector};

    #[cfg(not(windows))]
    #[test]
    fn ipv4_only_http_lookup_tries_ipv4_before_a_resolved_ipv6_candidate() {
        let v6 = "[2001:db8::8]:80".parse().unwrap();
        let v4 = "192.0.2.8:80".parse().unwrap();
        let candidates = Arc::new(Mutex::new(None));
        let resolver = HttpResolver {
            candidates: candidates.clone(),
            deadline: None,
            cancel: hd_run_wasmtime::internals::http::connector::Cancel::default(),
            lookup: Some(Arc::new(move |_| {
                hd_run_wasmtime::internals::net::socket::configured_lookup(
                    || Ok(vec![v6, v4]),
                    || (true, false),
                )
            })),
        };
        resolver
            .resolve(
                &"http://ipv4-only.test/".parse().unwrap(),
                &ureq::config::Config::default(),
                NextTimeout {
                    after: Duration::from_secs(1).into(),
                    reason: ureq::Timeout::Global,
                },
            )
            .unwrap();
        let addresses = candidates.lock().unwrap().take().unwrap();
        let mut attempted = Vec::new();
        connect_candidates(
            addresses,
            None,
            || Duration::ZERO,
            |address, _| {
                attempted.push(address);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(attempted, [v4]);
    }

    #[test]
    fn tcp_candidates_use_node_family_order_and_250ms_whole_deadline_budgets() {
        let v4a = "127.0.0.1:1".parse().unwrap();
        let v4b = "127.0.0.1:2".parse().unwrap();
        let v6 = "[::1]:1".parse().unwrap();
        let elapsed = std::cell::Cell::new(Duration::ZERO);
        let mut calls = Vec::new();
        let result = connect_candidates(
            vec![v4a, v4b, v4a, v6],
            Some(Duration::from_secs(30)),
            || elapsed.get(),
            |address, budget| {
                calls.push((address, budget));
                elapsed.set(elapsed.get() + budget.unwrap());
                if address == v4b {
                    Ok(())
                } else {
                    Err(ureq::Error::Io(std::io::ErrorKind::TimedOut.into()))
                }
            },
        );
        assert!(result.is_ok());
        assert_eq!(
            calls,
            [
                (v4a, Some(Duration::from_millis(250))),
                (v6, Some(Duration::from_millis(250))),
                (v4b, Some(Duration::from_millis(29_500)))
            ]
        );
        let elapsed = std::cell::Cell::new(Duration::ZERO);
        let mut calls = 0;
        let result: Result<(), _> = connect_candidates(
            vec![v4a, v6],
            Some(Duration::from_millis(5)),
            || elapsed.get(),
            |_, budget| {
                calls += 1;
                assert_eq!(budget, Some(Duration::from_millis(5)));
                elapsed.set(Duration::from_millis(5));
                Err(ureq::Error::Timeout(ureq::Timeout::Connect))
            },
        );
        assert!(matches!(
            result,
            Err(ureq::Error::Timeout(ureq::Timeout::Global))
        ));
        assert_eq!(calls, 1);
    }

    #[test]
    fn tcp_candidates_advance_from_a_reserved_non_listening_port_to_a_working_listener() {
        let refused = non_listening_socket();
        let bad = refused.local_addr().unwrap().as_socket().unwrap();
        let working = TcpListener::bind("127.0.0.1:0").unwrap();
        let good = working.local_addr().unwrap();
        let started = Instant::now();
        let stream = connect_candidates(
            vec![bad, good],
            Some(Duration::from_secs(1)),
            || started.elapsed(),
            |address, budget| {
                TcpStream::connect_timeout(&address, budget.unwrap()).map_err(ureq::Error::Io)
            },
        )
        .unwrap();
        assert_eq!(stream.peer_addr().unwrap(), good);
        let accepted = bounded_accept(&working);
        drop(accepted);
        drop(refused);
    }

    #[test]
    fn resolver_connector_handoff_reaches_the_working_seventeenth_candidate() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let good = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let started = Instant::now();
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(started.elapsed() < Duration::from_secs(8));
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("candidate17 accept failed: {error}"),
                }
            };
            // Accepted sockets may inherit O_NONBLOCK on Darwin; reads
            // below intentionally use a blocking socket with a timeout.
            stream.set_nonblocking(false).unwrap();
            let request = read_request(&mut stream); // Also bounds reads to two seconds.
            write_response_with_budget(
                &mut stream,
                b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nabc",
                Duration::from_secs(1),
            )
            .unwrap();
            request
        });
        // Keep all sixteen distinct ports bound but non-listening until
        // the request finishes, so parallel tests cannot steal any port.
        // Darwin blackholes these SYNs: sixteen nonfinal 250ms attempts
        // legitimately take four seconds. Budget eight seconds for this
        // path rather than retrying or weakening the 17-address assertion.
        // No external DNS/I/O occurs.
        let refused: Vec<_> = (0..16).map(|_| non_listening_socket()).collect();
        let mut addresses: Vec<_> = refused
            .iter()
            .map(|socket| socket.local_addr().unwrap().as_socket().unwrap())
            .collect();
        addresses.push(good);
        assert_eq!(addresses.len(), 17);
        assert!(addresses[..16].iter().all(|address| *address != good));
        let candidates = Arc::new(Mutex::new(None));
        let deadline = Some(Instant::now() + Duration::from_secs(8));
        let resolver = HttpResolver {
            candidates: candidates.clone(),
            deadline,
            cancel: hd_run_wasmtime::internals::http::connector::Cancel::default(),
            lookup: Some(Arc::new(move |_| Ok(addresses.clone()))),
        };
        let connector = HttpTcpConnector {
            deadline,
            connect_deadline: None,
            candidates: candidates.clone(),
            cancel: hd_run_wasmtime::internals::http::connector::Cancel::default(),
            phase: hd_run_wasmtime::internals::http::connector::Phase::default(),
        }
        .chain(RustlsConnector::default());
        let config = ureq::Agent::config_builder()
            .proxy(None)
            .max_idle_connections(0)
            .timeout_global(Some(Duration::from_secs(8)))
            .build();
        let agent = ureq::Agent::with_parts(config, connector, resolver);
        let mut response = agent.get("http://candidate.test/path").call().unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(
            response.body_mut().with_config().read_to_vec().unwrap(),
            b"abc"
        );
        assert!(candidates.lock().unwrap().is_none());
        server.join().unwrap();
        drop(refused);
    }

    #[test]
    fn full_candidate_resolver_preserves_numeric_literals() {
        for (uri, expected) in [
            ("http://127.0.0.1:8123/", "127.0.0.1:8123"),
            ("http://[::1]:8123/", "[::1]:8123"),
        ] {
            let candidates = Arc::new(Mutex::new(None));
            let resolver = HttpResolver {
                candidates: candidates.clone(),
                deadline: Some(Instant::now() + Duration::from_secs(1)),
                cancel: hd_run_wasmtime::internals::http::connector::Cancel::default(),
                lookup: None,
            };
            let representative = resolver
                .resolve(
                    &uri.parse().unwrap(),
                    &ureq::config::Config::default(),
                    NextTimeout {
                        after: Duration::from_secs(1).into(),
                        reason: ureq::Timeout::Global,
                    },
                )
                .unwrap();
            let expected = expected.parse().unwrap();
            assert_eq!(&*representative, [expected]);
            assert_eq!(
                candidates.lock().unwrap().as_deref(),
                Some([expected].as_slice())
            );
        }
    }

    #[test]
    fn cancelling_dns_without_a_deadline_ends_the_request_before_lookup_finishes() {
        let (ready, arrived) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        let released = Mutex::new(released);
        let (finished, completion) = std::sync::mpsc::channel();
        let cancel = hd_run_wasmtime::internals::http::connector::Cancel::default();
        let candidates = Arc::new(Mutex::new(None));
        let resolver = HttpResolver {
            candidates: candidates.clone(),
            deadline: None,
            cancel: cancel.clone(),
            lookup: Some(Arc::new(move |_| {
                ready.send(()).unwrap();
                let _ = released
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5));
                finished.send(()).unwrap();
                Ok(vec!["127.0.0.1:9".parse().unwrap()])
            })),
        };
        let connector = HttpTcpConnector {
            deadline: None,
            connect_deadline: None,
            candidates,
            cancel: cancel.clone(),
            phase: hd_run_wasmtime::internals::http::connector::Phase::default(),
        }
        .chain(RustlsConnector::default());
        let config = ureq::Agent::config_builder()
            .proxy(None)
            .max_idle_connections(0)
            .build();
        let agent = ureq::Agent::with_parts(config, connector, resolver);
        let (reply, receive) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            reply
                .send(agent.get("http://blocked.test/").call().map(|_| ()))
                .unwrap();
        });
        arrived.recv_timeout(Duration::from_secs(5)).unwrap();
        let started = Instant::now();
        cancel.cancel();
        let result = receive.recv_timeout(Duration::from_millis(500));
        let elapsed = started.elapsed();
        let lookup_pending = completion.try_recv().is_err();
        // Release and join even when the old resolver misses the cancel,
        // so a failing regression leaves no five-second worker behind.
        release.send(()).unwrap();
        worker.join().unwrap();
        completion.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(
            matches!(result, Ok(Err(ureq::Error::Io(ref error)))
                if error.kind() == std::io::ErrorKind::ConnectionAborted),
            "{result:?}"
        );
        assert!(elapsed < Duration::from_millis(500), "{elapsed:?}");
        assert!(
            lookup_pending,
            "the request waited for the lookup to finish"
        );
    }

    #[test]
    fn timed_dns_handoff_returns_promptly_without_waiting_for_the_blocked_lookup() {
        let (release, released) = std::sync::mpsc::channel();
        let (finished, completion) = std::sync::mpsc::channel();
        let started = Instant::now();
        let result = resolve_lookup(
            NextTimeout {
                after: Duration::from_secs(1).into(),
                reason: ureq::Timeout::Global,
            },
            Some(started + Duration::from_millis(20)),
            &hd_run_wasmtime::internals::http::connector::Cancel::default(),
            move || {
                released.recv_timeout(Duration::from_secs(2)).unwrap();
                finished.send(()).unwrap();
                Ok(vec!["127.0.0.1:8123".parse().unwrap()])
            },
        );
        assert!(matches!(
            result,
            Err(ureq::Error::Timeout(ureq::Timeout::Global))
        ));
        assert!(started.elapsed() < Duration::from_millis(500));
        release.send(()).unwrap();
        completion.recv_timeout(Duration::from_secs(1)).unwrap();
        let result = resolve_lookup(
            NextTimeout {
                after: Duration::from_secs(1).into(),
                reason: ureq::Timeout::Global,
            },
            None,
            &hd_run_wasmtime::internals::http::connector::Cancel::default(),
            || Err(std::io::ErrorKind::NotFound.into()),
        );
        assert!(matches!(result, Err(ureq::Error::HostNotFound)));
    }
}
