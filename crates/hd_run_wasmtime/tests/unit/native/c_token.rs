//! A C provider's suspending calls (#2388 step 5): the token's completions
//! from any thread, the first completion winning, and the abort slot run at
//! most once with its `completed` flag (`cli.host.c.thread`,
//! `cli.host.c.complete`, `cli.host.c.abort`, `cli.host.c.token`,
//! `cli.host.c.bytes`). The library is this file's own C-ABI functions,
//! called through the engine's C call trampolines as a loaded library's are.
mod tests {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier, Condvar, Mutex, MutexGuard, PoisonError, mpsc};

    use hd_base::HashMap;
    use hd_host_abi::c::{Num, Ret};
    use hd_run_wasmtime::internals::native::c_token::{live, start};
    use hd_run_wasmtime::internals::native::{Failure, Handles, HostError, Suspension};
    use hd_run_wasmtime::internals::runtime::reactor::Reactor;

    /// The tests that count live tokens run one at a time.
    static TOKENS: Mutex<()> = Mutex::new(());

    fn serial() -> MutexGuard<'static, ()> {
        TOKENS.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The C header's `hd_completions` and `struct hd_call`.
    #[repr(C)]
    #[allow(clippy::struct_field_names, reason = "the C header's field names")]
    struct Completions {
        complete_i32: unsafe extern "C" fn(*const Token, i32) -> bool,
        complete_i64: unsafe extern "C" fn(*const Token, i64) -> bool,
        complete_f64: unsafe extern "C" fn(*const Token, f64) -> bool,
        complete_bytes: unsafe extern "C" fn(*const Token, *const u8, usize) -> bool,
        complete_error: unsafe extern "C" fn(*const Token, i32) -> bool,
    }

    #[repr(C)]
    struct Token {
        completions: *const Completions,
    }

    /// The table of `token`, as the header's inline functions read it.
    #[allow(unsafe_code)]
    fn table(token: usize) -> &'static Completions {
        // SAFETY: a live token this test's library holds.
        unsafe { &*(*(token as *const Token)).completions }
    }

    /// How the slot treats a call, its second argument.
    #[derive(Clone, Copy)]
    #[repr(i64)]
    enum Mode {
        /// The worker thread completes it with `hd_complete_i32`.
        Worker = 0,
        /// Nothing completes it.
        Keep,
        /// The slot completes it before it returns, with `hd_complete_i32`.
        Now,
        /// As `Now`, with `value` bytes of a buffer the slot then
        /// overwrites.
        Bytes,
        /// As `Now`, with `hd_complete_error`.
        Error,
        /// As `Now`, with `hd_complete_i64`.
        Wide,
        /// As `Now`, with `hd_complete_f64` of `value / 4`.
        Float,
        /// As `Now`, twice: `value`, then `value + 1`.
        Twice,
        /// The worker completes it after meeting the test at a barrier,
        /// whether or not the call is aborting; the abort slot waits for
        /// that completion to return.
        Race,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Call {
        Pending,
        Completed,
    }

    /// What the abort slot saw of one call: its `completed` flag, and
    /// whether the library had completed it.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Aborted {
        flag: bool,
        completed: bool,
    }

    /// The library's state, its provider `self`: each pending call by its
    /// id, under one lock that the worker's completions and the abort slot
    /// share, so no completion follows the abort slot's return
    /// (`cli.host.c.token`).
    #[derive(Default)]
    struct Library {
        calls: Mutex<HashMap<i64, (usize, Call)>>,
        ids: Mutex<HashMap<usize, i64>>,
        aborts: Mutex<Vec<(i64, Aborted)>>,
        /// What each completion returned, by call id.
        results: Mutex<Vec<(i64, bool)>>,
        /// Signalled after each completion records its result.
        recorded: Condvar,
        slot_calls: AtomicUsize,
        worker_completions: AtomicUsize,
        work: Mutex<Option<mpsc::Sender<(i64, usize, bool)>>>,
        /// The ids of the calls in `Race` mode.
        racing: Mutex<hd_base::HashSet<i64>>,
    }

    impl Library {
        fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
            m.lock().unwrap_or_else(PoisonError::into_inner)
        }

        /// Records what a completion of call `id` returned.
        fn record(&self, id: i64, accepted: bool) {
            Self::lock(&self.results).push((id, accepted));
            self.recorded.notify_all();
        }

        fn results_of(&self, id: i64) -> Vec<bool> {
            Self::lock(&self.results)
                .iter()
                .filter(|(i, _)| *i == id)
                .map(|(_, a)| *a)
                .collect()
        }

        /// Completes call `id` with `hd_complete_i32`, if it is pending.
        #[allow(unsafe_code)]
        fn complete(&self, id: i64, value: i32) {
            let mut calls = Self::lock(&self.calls);
            if let Some((token, state)) = calls.get_mut(&id)
                && *state == Call::Pending
            {
                // SAFETY: a pending call's token is alive.
                let accepted =
                    unsafe { (table(*token).complete_i32)(*token as *const Token, value) };
                *state = Call::Completed;
                self.worker_completions.fetch_add(1, Ordering::SeqCst);
                self.record(id, accepted);
            }
        }

        /// Completes the call `id` of `token` with `hd_complete_i32`
        /// unconditionally: the abort slot waits for it.
        #[allow(unsafe_code)]
        fn race(&self, id: i64, token: usize) {
            // SAFETY: the token is alive until this completion returns,
            // since the abort slot waits for it.
            let accepted = unsafe { (table(token).complete_i32)(token as *const Token, 1) };
            if accepted && let Some(call) = Self::lock(&self.calls).get_mut(&id) {
                call.1 = Call::Completed;
            }
            self.worker_completions.fetch_add(1, Ordering::SeqCst);
            self.record(id, accepted);
        }

        fn aborts_of(&self, id: i64) -> Vec<Aborted> {
            Self::lock(&self.aborts)
                .iter()
                .filter(|(i, _)| *i == id)
                .map(|(_, a)| *a)
                .collect()
        }
    }

    /// The `!` slot: `(self, token, id, mode, value)`, which returns at once.
    #[allow(unsafe_code)]
    unsafe extern "C" fn slot(this: *mut c_void, token: usize, id: i64, mode: i64, value: i64) {
        // SAFETY: the test's library outlives every call.
        let lib = unsafe { &*this.cast::<Library>() };
        lib.slot_calls.fetch_add(1, Ordering::SeqCst);
        // A live token's address is never another live call's: only a
        // call the guest took, which gets no abort, frees its address.
        let mut calls = Library::lock(&lib.calls);
        if let Some(old) = Library::lock(&lib.ids).insert(token, id) {
            assert_eq!(calls.get(&old).map(|c| c.1), Some(Call::Completed));
        }
        calls.insert(id, (token, Call::Pending));
        drop(calls);
        if mode == Mode::Race as i64 {
            Library::lock(&lib.racing).insert(id);
        }
        let t = token as *const Token;
        let c = table(token);
        let now = |lib: &Library| {
            Library::lock(&lib.calls).insert(id, (token, Call::Completed));
        };
        // SAFETY: the token is alive until the slot returns, at least.
        unsafe {
            match mode {
                m if m == Mode::Worker as i64 || m == Mode::Race as i64 => {
                    if let Some(work) = Library::lock(&lib.work).as_ref() {
                        work.send((id, token, m == Mode::Race as i64))
                            .expect("the worker");
                    }
                }
                m if m == Mode::Now as i64 => {
                    lib.record(id, (c.complete_i32)(t, i32::try_from(value).unwrap_or(0)));
                    now(lib);
                }
                m if m == Mode::Bytes as i64 => {
                    let mut bytes: Vec<u8> = (0..u8::try_from(value).unwrap_or(0)).collect();
                    lib.record(id, (c.complete_bytes)(t, bytes.as_ptr(), bytes.len()));
                    // The host copied them before the call returned.
                    bytes.fill(0xEE);
                    drop(bytes);
                    now(lib);
                }
                m if m == Mode::Error as i64 => {
                    lib.record(id, (c.complete_error)(t, i32::try_from(value).unwrap_or(0)));
                    now(lib);
                }
                m if m == Mode::Wide as i64 => {
                    lib.record(id, (c.complete_i64)(t, value));
                    now(lib);
                }
                m if m == Mode::Float as i64 => {
                    #[allow(clippy::cast_precision_loss)]
                    lib.record(id, (c.complete_f64)(t, value as f64 / 4.0));
                    now(lib);
                }
                m if m == Mode::Twice as i64 => {
                    let v = i32::try_from(value).unwrap_or(0);
                    lib.record(id, (c.complete_i32)(t, v));
                    lib.record(id, (c.complete_i32)(t, v + 1));
                    now(lib);
                }
                _ => {}
            }
        }
    }

    /// The abort slot: `(self, token, completed)`. Under the call lock, so
    /// a gated worker's completion lands before it or not at all; a racing
    /// one it waits for, so none follows its return.
    #[allow(unsafe_code)]
    unsafe extern "C" fn abort(this: *mut c_void, token: usize, completed: bool) {
        // SAFETY: as `slot`.
        let lib = unsafe { &*this.cast::<Library>() };
        let id = Library::lock(&lib.ids)
            .remove(&token)
            .expect("an issued token");
        let state = Library::lock(&lib.calls).remove(&id).map(|(_, s)| s);
        if Library::lock(&lib.racing).contains(&id) {
            let results = Library::lock(&lib.results);
            drop(
                lib.recorded
                    .wait_while(results, |r| !r.iter().any(|(i, _)| *i == id))
                    .unwrap_or_else(PoisonError::into_inner),
            );
        }
        Library::lock(&lib.aborts).push((
            id,
            Aborted {
                flag: completed,
                completed: state == Some(Call::Completed),
            },
        ));
    }

    /// A worker thread that completes each call id it gets, after meeting
    /// the test at `barrier` when there is one.
    fn worker(lib: &Arc<Library>, barrier: Option<Arc<Barrier>>) -> std::thread::JoinHandle<()> {
        let (send, receive) = mpsc::channel::<(i64, usize, bool)>();
        *Library::lock(&lib.work) = Some(send);
        let lib = Arc::clone(lib);
        // cost: one thread, the library's worker.
        std::thread::spawn(move || {
            for (id, token, race) in receive {
                if let Some(barrier) = &barrier {
                    barrier.wait();
                }
                if race {
                    lib.race(id, token);
                } else {
                    lib.complete(id, i32::try_from(id).unwrap_or(0));
                }
            }
        })
    }

    /// Ends the worker of `lib` and waits for it.
    fn stop(lib: &Library, worker: std::thread::JoinHandle<()>) {
        drop(Library::lock(&lib.work).take());
        worker.join().expect("the worker");
    }

    /// Starts call `id` of `lib` in `mode`, of the C result row `result`.
    #[allow(unsafe_code)]
    fn begin(lib: &Arc<Library>, id: i64, mode: Mode, value: i64, result: &Ret) -> Suspension {
        let f = slot as unsafe extern "C" fn(*mut c_void, usize, i64, i64, i64);
        let a = abort as unsafe extern "C" fn(*mut c_void, usize, bool);
        // SAFETY: both have the signatures `start` names, and the library
        // outlives the call.
        unsafe {
            start(
                Arc::as_ptr(lib).cast_mut().cast(),
                f as *const c_void,
                a as *const c_void,
                &[id, mode as i64, value],
                result,
            )
        }
    }

    fn i32_row() -> Ret {
        Ret::Num(Num::I32)
    }

    /// The `.finish` of `h`, a call of method (0, 0).
    fn finish(r: &mut Reactor, h: i32) -> Option<Result<Vec<u8>, Failure>> {
        r.finish_native(h, (0, 0), &mut Handles::default())
    }

    fn zz(v: i64) -> Vec<u8> {
        let mut out = Vec::new();
        hd_base::leb::put(
            &mut out,
            (v.unsigned_abs() << 1).wrapping_sub(u64::from(v < 0)),
        );
        out
    }

    /// The doc's test: 1000 calls, each cancelled, each abort slot run once,
    /// and nothing delivered after its cancel. A third complete before the
    /// slot returns (`completed` true), a third never complete (`false`), and
    /// a third race a worker thread's completion. The work is counted: one
    /// slot call and one abort per call, and no token outlives its call.
    #[test]
    fn a_thousand_cancels_each_abort_once_and_deliver_nothing_late() {
        let _serial = serial();
        let lib = Arc::new(Library::default());
        let worker = worker(&lib, None);
        let mut r = Reactor::new(None);
        let calls = 1000;
        for id in 0..calls {
            let mode = [Mode::Now, Mode::Keep, Mode::Worker][usize::try_from(id % 3).unwrap()];
            let h = r
                .native(begin(&lib, id, mode, id, &i32_row()), (0, 0))
                .unwrap();
            for _ in 0..(id % 7) * 20 {
                std::hint::spin_loop();
            }
            drop(r.remove(h).expect("pending until cancelled"));
            let aborted = lib.aborts_of(id);
            assert_eq!(aborted.len(), 1, "call {id}: one abort, at the cancel");
            let Aborted { flag, completed } = aborted[0];
            assert!(!flag || completed, "call {id}: a flag with no completion");
            let results = lib.results_of(id);
            match mode {
                Mode::Now => assert!(flag && results == [true], "call {id}: accepted"),
                Mode::Keep => assert!(!flag && !completed, "call {id}: never completed"),
                // Exactly one: accepted before the cancel, refused after it,
                // or never made, since the abort slot ran first.
                _ => assert_eq!(
                    usize::from(flag)
                        + usize::from(results == [false])
                        + usize::from(results.is_empty()),
                    1,
                    "call {id}: {flag} {results:?}"
                ),
            }
            assert_eq!(finish(&mut r, h), None, "call {id}: nothing to take");
        }
        stop(&lib, worker);
        // Every wake a completion pushed names a call that ended.
        assert!(r.due().is_empty() && !r.waiting(), "no late delivery");
        assert_eq!(lib.slot_calls.load(Ordering::SeqCst), 1000);
        assert_eq!(Library::lock(&lib.aborts).len(), 1000, "one abort per call");
        assert!(lib.worker_completions.load(Ordering::SeqCst) <= 333);
        assert!(Library::lock(&lib.calls).is_empty());
        assert_eq!(live(), 0, "every token freed");
    }

    /// A completion from another thread wakes the reactor, and the
    /// `.finish` takes the value; no abort follows a taken value.
    #[test]
    fn a_completion_from_another_thread_wakes_the_reactor() {
        let _serial = serial();
        let lib = Arc::new(Library::default());
        let worker = worker(&lib, None);
        let mut r = Reactor::new(None);
        let h = r
            .native(begin(&lib, -7, Mode::Worker, 0, &i32_row()), (0, 0))
            .unwrap();
        let woken = loop {
            assert!(r.wait(), "the call is pending");
            let due = r.due();
            if !due.is_empty() {
                break due;
            }
        };
        assert_eq!(woken, [h]);
        assert_eq!(finish(&mut r, h), Some(Ok(zz(-7))));
        assert_eq!(finish(&mut r, h), None, "taken once");
        stop(&lib, worker);
        drop(r);
        assert!(Library::lock(&lib.aborts).is_empty(), "no abort");
        assert_eq!(live(), 0);
    }

    /// A completion racing a cancel (decision 18): at a barrier, the worker
    /// completes as the test cancels the call, or on every other call takes
    /// the value if it is due, 200 times. For each call exactly one holds: an accepted completion
    /// was taken, the abort slot saw `completed` true, or the completion
    /// returned false. Each cancelled call is aborted once.
    #[test]
    fn a_completion_racing_an_abort_is_taken_discarded_or_refused() {
        let _serial = serial();
        let lib = Arc::new(Library::default());
        let barrier = Arc::new(Barrier::new(2));
        let worker = worker(&lib, Some(Arc::clone(&barrier)));
        let mut r = Reactor::new(None);
        let mut outcomes = [0usize; 3];
        for id in 0..200 {
            let h = r
                .native(begin(&lib, id, Mode::Race, 0, &i32_row()), (0, 0))
                .unwrap();
            barrier.wait();
            // Odd calls are cancelled at once; even ones taken if due.
            let taken = if id % 2 == 0 && r.due().contains(&h) {
                assert_eq!(finish(&mut r, h), Some(Ok(zz(1))), "call {id}");
                true
            } else {
                drop(r.remove(h).expect("pending until cancelled"));
                assert_eq!(finish(&mut r, h), None, "call {id}");
                false
            };
            // The worker's completion returned before the abort slot did,
            // or before the reactor saw it due.
            let results = loop {
                let results = lib.results_of(id);
                if !results.is_empty() {
                    break results;
                }
                std::thread::yield_now();
            };
            let aborted = lib.aborts_of(id);
            assert_eq!(aborted.len(), usize::from(!taken), "call {id}");
            let discarded = aborted.first().is_some_and(|a| a.flag);
            let refused = results == [false];
            assert_eq!(
                usize::from(taken) + usize::from(discarded) + usize::from(refused),
                1,
                "call {id}: taken {taken}, discarded {discarded}, refused {refused}"
            );
            outcomes[usize::from(discarded) + 2 * usize::from(refused)] += 1;
        }
        stop(&lib, worker);
        assert_eq!(outcomes.iter().sum::<usize>(), 200, "{outcomes:?}");
        assert!(r.due().is_empty() && !r.waiting());
        assert_eq!(live(), 0);
    }

    /// The abort slot's `completed`: true when a completion arrived before
    /// the cancel, false when none did; a call whose value the guest took
    /// gets no abort.
    #[test]
    fn the_abort_slot_says_whether_a_completed_result_was_discarded() {
        let _serial = serial();
        let lib = Arc::new(Library::default());
        let mut r = Reactor::new(None);
        let done = r
            .native(begin(&lib, 1, Mode::Now, 5, &i32_row()), (0, 0))
            .unwrap();
        assert_eq!(r.due(), [done], "completed in the slot: due at once");
        drop(r.remove(done));
        let open = r
            .native(begin(&lib, 2, Mode::Keep, 0, &i32_row()), (0, 0))
            .unwrap();
        drop(r.remove(open));
        let taken = r
            .native(begin(&lib, 3, Mode::Now, 9, &i32_row()), (0, 0))
            .unwrap();
        assert_eq!(r.due(), [taken]);
        assert_eq!(finish(&mut r, taken), Some(Ok(zz(9))));
        drop(r);
        let flag = Aborted {
            flag: true,
            completed: true,
        };
        assert_eq!(lib.aborts_of(1), [flag]);
        let none = Aborted {
            flag: false,
            completed: false,
        };
        assert_eq!(lib.aborts_of(2), [none]);
        assert_eq!(lib.aborts_of(3), [], "a taken value has no abort");
        assert_eq!(live(), 0);
    }

    /// Each completion crosses in the encoding the guest decodes: copied
    /// bytes as a `List[u8]` (`cli.host.c.bytes`), a `Result`'s value or
    /// its `i32` error, a narrow integer cut to its width, an `f32`
    /// rounded; the first of two completions wins. A completion of the
    /// wrong kind fails the call.
    #[test]
    fn bytes_results_and_numbers_cross_as_the_guest_decodes_them() {
        let _serial = serial();
        let lib = Arc::new(Library::default());
        let mut r = Reactor::new(None);
        let mut run = |id: i64, mode: Mode, value: i64, row: Ret| {
            let h = r
                .native(begin(&lib, id, mode, value, &row), (0, 0))
                .unwrap();
            assert_eq!(r.due(), [h], "completed in the slot: due at once");
            finish(&mut r, h).expect("completed")
        };
        let bytes = Ret::Bytes;
        let result = |ret: Ret| Ret::Result(Box::new(ret));
        let mut list = vec![5];
        list.extend(0..5u8);
        assert_eq!(run(1, Mode::Bytes, 5, bytes.clone()), Ok(list.clone()));
        let mut ok = vec![0];
        ok.extend(&list);
        assert_eq!(run(2, Mode::Bytes, 5, result(bytes.clone())), Ok(ok));
        let mut err = vec![1];
        err.extend(zz(-5));
        assert_eq!(run(3, Mode::Error, -5, result(bytes.clone())), Ok(err));
        let mut wide = vec![0];
        wide.extend(zz(-(1 << 40)));
        assert_eq!(
            run(4, Mode::Wide, -(1 << 40), result(Ret::Num(Num::I64))),
            Ok(wide)
        );
        assert_eq!(run(5, Mode::Now, 300, Ret::Num(Num::U8)), Ok(vec![44]));
        assert_eq!(
            run(6, Mode::Float, 3, Ret::Num(Num::F32)),
            Ok(0.75f32.to_le_bytes().to_vec())
        );
        assert_eq!(
            run(7, Mode::Twice, 1, i32_row()),
            Ok(zz(1)),
            "the first wins"
        );
        assert_eq!(lib.results_of(7), [true, false], "the second is refused");
        assert_eq!(
            run(8, Mode::Bytes, 2, i32_row()),
            Err(Failure::Host(HostError(
                "the C provider completed a call of `i32` with hd_complete_bytes".into()
            )))
        );
        assert_eq!(
            run(9, Mode::Error, 1, i32_row()),
            Err(Failure::Host(HostError(
                "the C provider completed with hd_complete_error, but the method's result is not a `Result`".into()
            )))
        );
        drop(r);
        assert!(Library::lock(&lib.aborts).is_empty());
        assert_eq!(live(), 0);
    }
}
