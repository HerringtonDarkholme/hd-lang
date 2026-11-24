mod tests {
    use hd_run_wasmtime::internals::net::socket::{
        CONNECT_ATTEMPT_TIMEOUT, NetError, connect_attempts, resolve_with,
    };
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    #[cfg(not(windows))]
    #[test]
    fn ipv4_only_configuration_tries_ipv4_before_a_resolved_ipv6_candidate() {
        let v6 = "[2001:db8::8]:80".parse().unwrap();
        let v4 = "192.0.2.8:80".parse().unwrap();
        let probed = std::cell::Cell::new(0);
        let addresses = hd_run_wasmtime::internals::net::socket::configured_lookup(
            || Ok(vec![v6, v4]),
            || {
                probed.set(probed.get() + 1);
                (true, false)
            },
        )
        .unwrap();
        let attempts = connect_attempts(addresses);
        assert_eq!(attempts[0].address, v4);
        assert_eq!(attempts.len(), 1);
        assert_eq!(probed.get(), 1);
    }

    #[test]
    fn configured_lookup_preserves_order_and_falls_back_when_filtering_is_empty() {
        let v6a = "[2001:db8::1]:80".parse().unwrap();
        let v4a = "192.0.2.1:80".parse().unwrap();
        let v6b = "[2001:db8::2]:80".parse().unwrap();
        let v4b = "192.0.2.2:80".parse().unwrap();
        let original = vec![v6a, v4a, v6b, v4b];
        for (families, expected) in [
            ((true, true), original.clone()),
            ((true, false), vec![v4a, v4b]),
            ((false, true), vec![v6a, v6b]),
            ((false, false), original.clone()),
        ] {
            let found = hd_run_wasmtime::internals::net::socket::configured_lookup(
                || Ok(original.clone()),
                || families,
            )
            .unwrap();
            assert_eq!(
                found,
                if cfg!(windows) {
                    original.clone()
                } else {
                    expected
                }
            );
        }
        assert_eq!(
            hd_run_wasmtime::internals::net::socket::configured_lookup(
                || Ok(vec![v6a, v6b]),
                || (true, false)
            )
            .unwrap(),
            vec![v6a, v6b],
        );
        let empty = hd_run_wasmtime::internals::net::socket::configured_lookup(
            || Ok(Vec::new()),
            || (true, false),
        )
        .unwrap();
        assert!(empty.is_empty());
        let failed = hd_run_wasmtime::internals::net::socket::configured_lookup(
            || Err(std::io::ErrorKind::NotFound.into()),
            || panic!("a failed lookup needs no family probe"),
        );
        assert_eq!(failed.unwrap_err().kind(), std::io::ErrorKind::NotFound);
    }

    #[cfg(windows)]
    #[test]
    fn windows_lookup_keeps_every_family_without_probing() {
        let addresses = vec!["[::1]:80".parse().unwrap(), "127.0.0.1:80".parse().unwrap()];
        let found = hd_run_wasmtime::internals::net::socket::configured_lookup(
            || Ok(addresses.clone()),
            || panic!("Windows does not apply AI_ADDRCONFIG"),
        )
        .unwrap();
        assert_eq!(found, addresses);
    }

    #[test]
    fn socket_errors_keep_spec_categories_and_native_context() {
        use std::io::{Error, ErrorKind};
        for kind in [ErrorKind::ConnectionRefused, ErrorKind::AddrInUse] {
            let error = hd_run_wasmtime::internals::net::socket::socket_error(
                &Error::from(kind),
                "connect",
                Some("127.0.0.1:9".parse().unwrap()),
                "example.test",
                9,
            );
            assert_eq!(
                error,
                NetError::Refused {
                    address: "example.test:9".into()
                }
            );
        }
        let native = Error::from(ErrorKind::ConnectionReset);
        let error =
            hd_run_wasmtime::internals::net::socket::socket_error(&native, "read", None, "", 0);
        assert_eq!(
            error,
            NetError::Other {
                message: format!("read: {native}")
            }
        );
    }

    #[test]
    fn family_selection_alternates_deduplicates_and_times_only_nonfinal_attempts() {
        let addresses = [
            "[::1]:80",
            "[::2]:80",
            "127.0.0.1:80",
            "[::1]:80",
            "127.0.0.2:80",
        ]
        .map(|s| s.parse().unwrap());
        let attempts = connect_attempts(addresses.to_vec());
        assert_eq!(
            attempts.iter().map(|a| a.address).collect::<Vec<_>>(),
            [addresses[0], addresses[2], addresses[1], addresses[4]]
        );
        // Every candidate but the last waits at most 250 ms; the last one
        // stays until it connects, fails, or its operation is aborted.
        assert_eq!(
            attempts.iter().map(|a| a.timeout).collect::<Vec<_>>(),
            [
                Some(CONNECT_ATTEMPT_TIMEOUT),
                Some(CONNECT_ATTEMPT_TIMEOUT),
                Some(CONNECT_ATTEMPT_TIMEOUT),
                None
            ]
        );
        assert_eq!(CONNECT_ATTEMPT_TIMEOUT, Duration::from_millis(250));
        let single = connect_attempts(vec![addresses[0], addresses[0]]);
        assert_eq!(single.len(), 1);
        assert_eq!(single[0].timeout, None);
        let v4first = connect_attempts(vec![addresses[2], addresses[0], addresses[4]]);
        assert_eq!(v4first[0].address, addresses[2]);
        assert_eq!(v4first[1].address, addresses[0]);
        assert_eq!(v4first[2].address, addresses[4]);
    }

    /// An OS lookup that does not return holds no task: aborting the
    /// lookup's task ends it at once, and the lookup finishes later on its
    /// own pool thread.
    #[test]
    fn an_aborted_lookup_does_not_wait_for_the_os_resolver() {
        // The `hd-io` runtime, which its own thread drives.
        let runtime = hd_run_wasmtime::internals::runtime::io::handle().unwrap();
        let (ready, arrived) = mpsc::channel();
        let (release, released) = mpsc::channel::<()>();
        let (finished, completion) = mpsc::channel();
        let task = runtime.spawn(async move {
            resolve_with("local.test", move || {
                ready.send(()).unwrap();
                let _ = released.recv_timeout(Duration::from_secs(10));
                let _ = finished.send(());
                Ok(vec!["127.0.0.1:9".parse().unwrap()])
            })
            .await
        });
        arrived.recv_timeout(Duration::from_secs(10)).unwrap();
        let start = Instant::now();
        task.abort();
        assert!(runtime.block_on(task).unwrap_err().is_cancelled());
        assert!(start.elapsed() < Duration::from_secs(1));
        assert_eq!(completion.try_recv(), Err(mpsc::TryRecvError::Empty));
        release.send(()).unwrap();
        completion.recv_timeout(Duration::from_secs(10)).unwrap();
    }
}
