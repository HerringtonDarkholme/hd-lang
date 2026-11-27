mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use hd_run_wasmtime::internals::host::{Control, Host};
    use hd_run_wasmtime::internals::runtime::reactor::{Done, Reactor, Work};

    /// Waits until `due` completes something, and returns it.
    fn next_due(r: &mut Reactor) -> Vec<i32> {
        loop {
            assert!(r.wait(), "an operation is pending");
            let done = r.due();
            if !done.is_empty() {
                return done;
            }
        }
    }

    /// A native pending call of an `i64`, whose hook counts into `aborts`.
    fn native_call(
        aborts: &std::rc::Rc<std::cell::Cell<usize>>,
    ) -> (
        hd_run_wasmtime::internals::native::Suspension,
        hd_run_wasmtime::internals::native::Completion<i64>,
    ) {
        let hook = std::rc::Rc::clone(aborts);
        let (call, completion) =
            hd_run_wasmtime::internals::native::pending(move || hook.set(hook.get() + 1));
        let encode: hd_run_wasmtime::internals::native::Encode<i64> =
            |value, _| Ok(value.to_le_bytes().to_vec());
        (
            hd_run_wasmtime::internals::native::Suspension::new(call, encode),
            completion,
        )
    }

    #[test]
    fn a_native_completion_wakes_only_the_reactor_that_registered_it() {
        // Two instances' reactors number their operations alike, so both
        // calls get one handle number; the closed one's completion lands
        // nowhere (`cli.host.package.native-late-completion`).
        let aborts = std::rc::Rc::default();
        let (mut old, mut new) = (Reactor::new(None), Reactor::new(None));
        let (first, late) = native_call(&aborts);
        let (second, completion) = native_call(&aborts);
        let h = old.native(first, (0, 2)).unwrap();
        assert_eq!(new.native(second, (0, 2)), Some(h));
        old.drop_all();
        assert_eq!(aborts.get(), 1, "closing aborts the pending call");
        late.complete(Ok(7));
        assert!(old.due().is_empty() && !old.waiting());
        assert!(new.inbox.queue.lock().unwrap().is_empty(), "no wake");
        assert!(new.due().is_empty() && new.waiting());
        assert_eq!(
            new.finish_native(
                h,
                (0, 2),
                &mut hd_run_wasmtime::internals::native::Handles::default()
            ),
            None,
            "not completed"
        );
        // cost: one thread, the completion's sender.
        std::thread::spawn(move || completion.complete(Ok(9)))
            .join()
            .unwrap();
        assert_eq!(next_due(&mut new), [h]);
        assert_eq!(
            new.finish_native(
                h,
                (0, 1),
                &mut hd_run_wasmtime::internals::native::Handles::default()
            ),
            None,
            "another method's"
        );
        assert_eq!(
            new.finish_native(
                h,
                (0, 2),
                &mut hd_run_wasmtime::internals::native::Handles::default()
            ),
            Some(Ok(9_i64.to_le_bytes().to_vec()))
        );
        assert_eq!(
            new.finish_native(
                h,
                (0, 2),
                &mut hd_run_wasmtime::internals::native::Handles::default()
            ),
            None,
            "delivered once"
        );
        drop(new);
        assert_eq!(aborts.get(), 1, "a delivered call has no abort");
    }

    #[test]
    fn a_native_call_completed_before_registration_is_due_at_once() {
        let aborts = std::rc::Rc::default();
        let mut r = Reactor::new(None);
        let (call, completion) = native_call(&aborts);
        completion.complete(Ok(3));
        let h = r.native(call, (0, 0)).unwrap();
        assert_eq!(r.due(), [h]);
        assert_eq!(
            r.finish_native(
                h,
                (0, 0),
                &mut hd_run_wasmtime::internals::native::Handles::default()
            ),
            Some(Ok(3_i64.to_le_bytes().to_vec()))
        );
        // A call its provider aborted before returning it never completes,
        // so it is not registered.
        let (mut pending, _completion) = hd_run_wasmtime::internals::native::pending::<i64>(|| {});
        pending.abort();
        let encode: hd_run_wasmtime::internals::native::Encode<i64> = |_, _| Ok(Vec::new());
        let aborted = hd_run_wasmtime::internals::native::Suspension::new(pending, encode);
        assert_eq!(r.native(aborted, (0, 0)), None);
        assert!(!r.waiting());
        assert_eq!(aborts.get(), 0);
    }

    /// The `.finish` of offloaded operation `h`: the result it delivered.
    fn finish(r: &mut Reactor, h: i32) -> u32 {
        match r.remove(h).map(|op| op.work) {
            Some(Work::Offloaded(Some(Done::Test(n)))) => n,
            _ => panic!("operation {h} has no result"),
        }
    }

    /// An offloaded operation that never completes, and the flag its task
    /// sets when it is dropped, as an abort drops it.
    fn pending_task() -> (impl Future<Output = Done> + Send + 'static, Arc<AtomicBool>) {
        struct Dropped(Arc<AtomicBool>);
        impl Drop for Dropped {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let dropped = Arc::new(AtomicBool::new(false));
        let guard = Dropped(Arc::clone(&dropped));
        let work = async move {
            let _guard = guard;
            std::future::pending::<Done>().await
        };
        (work, dropped)
    }

    /// Whether `flag` is set within a generous bound: an abort reaches the
    /// task on the `hd-io` thread.
    fn eventually(flag: &AtomicBool) -> bool {
        let end = Instant::now() + Duration::from_secs(10);
        while Instant::now() < end {
            if flag.load(Ordering::SeqCst) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        false
    }

    #[test]
    fn offloaded_ops_complete_out_of_order_with_their_own_results() {
        let mut r = Reactor::new(None);
        let (release, gate) = tokio::sync::oneshot::channel::<()>();
        let slow = r
            .offload(async move {
                let _ = gate.await;
                Done::Test(1)
            })
            .expect("offloads");
        let fast = r.offload(async { Done::Test(2) }).expect("offloads");
        assert_eq!(next_due(&mut r), [fast], "the later op completes first");
        assert!(r.waiting(), "the earlier op is still pending");
        release.send(()).expect("the slow op waits");
        assert_eq!(next_due(&mut r), [slow]);
        assert_eq!(finish(&mut r, fast), 2);
        assert_eq!(finish(&mut r, slow), 1);
        assert!(!r.waiting(), "nothing pending");
    }

    #[test]
    fn an_aborted_ops_late_result_is_dropped() {
        let mut r = Reactor::new(None);
        let h = r.offload(async { Done::Test(1) }).expect("offloads");
        // The task delivers, and the op is aborted before `due` takes it.
        while r.inbox.queue.lock().expect("inbox").done.is_empty() {
            assert!(r.wait());
        }
        assert!(
            matches!(
                r.remove(h).map(|op| op.work),
                Some(Work::Offloaded(Some(Done::Test(1))))
            ),
            "the abort ends the op and hands back its result"
        );
        assert!(!r.waiting());

        assert!(r.due().is_empty(), "the late result completes nothing");
        assert!(
            r.inbox.queue.lock().expect("inbox").done.is_empty(),
            "the late result is dropped"
        );
        let next = r.offload(async { Done::Test(2) }).expect("offloads");
        assert_eq!(next_due(&mut r), [next]);
        assert_eq!(finish(&mut r, next), 2, "a later op gets its own result");
    }

    #[test]
    fn a_stop_wakes_the_wait_while_an_op_is_pending() {
        let mut r = Reactor::new(None);
        let (work, _) = pending_task();
        let h = r.offload(work).expect("offloads");
        let flag = Arc::new(AtomicBool::new(false));
        let stopper = {
            let (flag, waker) = (Arc::clone(&flag), r.waker());
            // As a session's `Stopper` does, once the wait sleeps.
            std::thread::spawn(move || {
                while !waker.parked() {
                    std::thread::sleep(Duration::from_millis(1));
                }
                flag.store(true, Ordering::Relaxed);
                waker.wake();
            })
        };
        let started = Instant::now();
        while !flag.load(Ordering::Relaxed) {
            assert!(r.wait_until(None, Some(&flag)));
        }
        stopper.join().expect("the stopper ends");
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(r.waiting(), "the op is still pending");

        assert!(r.due().is_empty(), "the wake completes nothing");
        r.remove(h);
    }

    #[test]
    fn control_abort_aborts_a_pending_task() {
        let mut host = Host::placeholder();
        let (work, dropped) = pending_task();
        let h = host.reactor.offload(work).expect("offloads");
        assert!(host.control(Control::Abort(h)).is_none());
        assert!(eventually(&dropped), "the task is aborted");
        assert!(!host.waiting(), "nothing pending");
    }

    #[test]
    fn drop_all_aborts_every_pending_task() {
        let mut r = Reactor::new(None);
        let (first, first_dropped) = pending_task();
        let (second, second_dropped) = pending_task();
        r.offload(first).expect("offloads");
        r.offload(second).expect("offloads");
        r.sleep(60_000);
        assert!(r.drop_all().is_empty(), "no task delivered");
        assert!(!r.waiting(), "nothing pending");
        assert!(eventually(&first_dropped) && eventually(&second_dropped));
    }

    #[test]
    fn an_instances_end_aborts_its_tasks() {
        let mut r = Reactor::new(None);
        let (work, dropped) = pending_task();
        r.offload(work).expect("offloads");
        drop(r);
        assert!(eventually(&dropped), "the task is aborted");
    }

    #[test]
    fn reads_complete_in_order_and_an_aborted_read_loses_no_line() {
        let input: &'static [u8] = b"a\nb\r\nc";
        let mut r = Reactor::new(Some(Box::new(input)));
        let first = r.read_line();
        let second = r.read_line();
        r.remove(first);

        let mut done = Vec::new();
        while done.is_empty() {
            assert!(r.wait());
            done = r.due();
        }
        assert_eq!(done, [second]);
        let Some(op) = r.remove(second) else {
            panic!("the read is gone");
        };
        // The aborted read took no line: the next read gets the first, and
        // the line read for it is kept for the read after.
        assert!(matches!(op.work, Work::Read(Some(Ok(Some(ref l)))) if l == b"a"));
        let third = r.read_line();
        let mut done = Vec::new();
        while done.is_empty() {
            assert!(r.wait());
            done = r.due();
        }
        assert_eq!(done, [third]);
        let op = r.remove(third).expect("read");
        assert!(matches!(op.work, Work::Read(Some(Ok(Some(ref l)))) if l == b"b"));
        assert!(!r.wait(), "nothing pending");
    }

    #[test]
    fn read_all_joins_the_rest_after_a_line() {
        let input: &'static [u8] = b"a\r\nb\r\n\nc\n";
        let mut r = Reactor::new(Some(Box::new(input)));
        let first = r.read_line();
        let rest = r.read_all();
        let again = r.read_all();

        let mut done = Vec::new();
        while done.len() < 3 {
            assert!(r.wait());
            done.extend(r.due());
        }
        let text = |r: &mut Reactor, h| match r.remove(h).map(|op| op.work) {
            Some(Work::Read(Some(Ok(text)))) => text,
            _ => panic!("read {h} has no result"),
        };
        assert_eq!(text(&mut r, first).as_deref(), Some(&b"a"[..]));
        assert_eq!(text(&mut r, rest).as_deref(), Some(&b"b\n\nc\n"[..]));
        assert_eq!(text(&mut r, again).as_deref(), Some(&b""[..]));
    }

    #[test]
    fn read_all_keeps_the_line_structure() {
        for (input, want) in [
            (&b"a\nb\n"[..], "a\nb\n"),
            (b"a\nb", "a\nb"),
            (b"\n", "\n"),
            (b"a\r\nb\r\n", "a\nb\n"),
            (b"a\r", "a\r"),
            (b"", ""),
        ] {
            let mut r = Reactor::new(Some(Box::new(input)));
            let h = r.read_all();

            while r.due().is_empty() {
                assert!(r.wait());
            }
            let Some(Work::Read(Some(Ok(Some(got))))) = r.remove(h).map(|op| op.work) else {
                panic!("no result");
            };
            assert_eq!(got, want.as_bytes(), "{input:?}");
        }
    }

    /// `StdinHandle.read`: each read takes the buffered bytes exactly as
    /// read, at most `max`, sharing the input with line reads; an empty
    /// result is the end, and `max` 0 takes nothing at once
    /// (`std-console.stdin.exact`, `std-console.stdin.shared`).
    #[test]
    fn byte_reads_take_the_bytes_as_read() {
        let input: &'static [u8] = b"a\r\nb\r\n\xffc";
        let mut r = Reactor::new(Some(Box::new(input)));
        let line = r.read_line();
        let none = r.read_bytes(0);
        let two = r.read_bytes(2);
        let rest = r.read_bytes(64);
        let end = r.read_bytes(64);
        let mut done = Vec::new();
        while done.len() < 5 {
            assert!(r.wait());
            done.extend(r.due());
        }
        let bytes = |r: &mut Reactor, h| match r.remove(h).map(|op| op.work) {
            Some(Work::Read(Some(Ok(bytes)))) => bytes,
            _ => panic!("read {h} has no result"),
        };
        assert_eq!(bytes(&mut r, line).as_deref(), Some(&b"a"[..]));
        assert_eq!(bytes(&mut r, none).as_deref(), Some(&b""[..]));
        assert_eq!(bytes(&mut r, two).as_deref(), Some(&b"b\r"[..]));
        assert_eq!(bytes(&mut r, rest).as_deref(), Some(&b"\n\xffc"[..]));
        assert_eq!(bytes(&mut r, end).as_deref(), Some(&b""[..]));
    }

    #[test]
    fn a_failed_input_preserves_its_numeric_host_code() {
        struct Failed;
        impl std::io::Read for Failed {
            fn read(&mut self, _bytes: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::from_raw_os_error(5))
            }
        }
        impl std::io::BufRead for Failed {
            fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
                Err(std::io::Error::from_raw_os_error(5))
            }
            fn consume(&mut self, _amount: usize) {}
        }

        let mut r = Reactor::new(Some(Box::new(Failed)));
        let h = r.read_line();
        while r.due().is_empty() {
            assert!(r.wait());
        }
        let Some(Work::Read(Some(Err(code)))) = r.remove(h).map(|op| op.work) else {
            panic!("no failed read result");
        };
        assert_eq!(code, 5);
    }

    #[test]
    fn a_closed_input_ends_at_once() {
        let mut r = Reactor::new(None);
        let h = r.read_line();

        assert!(r.wait());
        assert_eq!(r.due(), [h]);
    }

    #[test]
    fn gate_stays_pending_until_its_cancel_drops_it() {
        let mut r = Reactor::new(None);
        let h = r.gate();
        assert!(r.waiting(), "the wait is pending");

        assert!(r.due().is_empty(), "the wait never completes");
        assert!(r.waiting(), "still pending");
        let op = r.remove(h).expect("the cancel drops the wait");
        assert!(matches!(op.work, Work::Gate));
        assert!(!r.waiting(), "nothing pending");
    }
}
