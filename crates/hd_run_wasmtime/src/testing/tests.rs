use super::{BodyResult, BodyRunner, CaseRun, PanicInfo, State, TestCase, execute_case};
use crate::codec::{Dec, Enc, encode};

struct Fake<F>(F);
impl<F: FnMut(State, bool) -> Result<BodyResult, String>> BodyRunner for Fake<F> {
    fn run_body(&mut self, state: State, reuse: bool) -> Result<BodyResult, String> {
        (self.0)(state, reuse)
    }
}

fn decode_plan(encoded: &Enc) -> (i64, i64) {
    let mut decoder = Dec::new(encoded.done());
    if decoder.leb().expect("example flag") != 0 {
        decoder.leb().expect("example");
    }
    (decoder.zz().expect("seed"), decoder.zz().expect("size"))
}

#[test]
fn successful_property_bodies_reuse_one_instance_in_seed_order() {
    let mut reuse_flags = Vec::new();
    let mut plans = Vec::new();
    let mut fake = Fake(|state: State, reuse: bool| {
        reuse_flags.push(reuse);
        let encoded = encode(&state.start(3, 0, 0)?.expect("fresh plan"));
        plans.push(decode_plan(&encoded));
        state.show("input");
        Ok(BodyResult {
            us: 7,
            ..BodyResult::default()
        })
    });
    let case = TestCase {
        seed: Some(10),
        ..TestCase::default()
    };
    let mut results = Vec::new();
    execute_case(&case, &mut fake, &mut |result| results.push(result)).expect("run");
    assert_eq!(reuse_flags, [false, true, true]);
    assert_eq!(plans, [(10, 0), (11, 1), (12, 2)]);
    assert_eq!(results.len(), 1);
    assert_eq!(
        (results[0].status, results[0].us, results[0].input.as_ref()),
        (0, 21, None)
    );
}

#[test]
fn a_shared_instance_failure_that_passes_fresh_replay_is_not_shrunk_or_saved() {
    let owner = State::new(TestCase::default());
    let root = std::path::PathBuf::from(owner.temp_dir().unwrap());
    let path = root.join("regression");
    let (mut calls, mut count) = (Vec::new(), 0);
    let mut seeds = Vec::new();
    let mut fake = Fake(|state: State, reuse: bool| {
        calls.push(reuse);
        if !reuse {
            count = 0;
        }
        count += 1;
        let plan = state.start(5, 20, 0)?.expect("plan");
        seeds.push(plan.seed);
        state.record(1);
        state.show("1");
        Ok(BodyResult {
            status: if count == 3 { 3 } else { 0 },
            trapped: count == 3,
            us: 1,
            ..BodyResult::default()
        })
    });
    let case = TestCase {
        seed: Some(10),
        regression: Some(path.clone()),
        ..TestCase::default()
    };
    let mut results = Vec::new();
    execute_case(&case, &mut fake, &mut |result| results.push(result)).unwrap();
    assert_eq!(calls, [false, true, true, false]);
    assert_eq!(seeds, [10, 11, 12, 12]);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].input.as_deref(), Some("1"));
    let message = results[0].failure.as_deref().expect("runner failure");
    assert!(message.contains("seed 12") && message.contains("state left by earlier cases"));
    assert!(!path.exists());
}

fn discarded() -> BodyResult {
    BodyResult {
        status: 3,
        trapped: true,
        stderr: "panic: explicit-panic: std.testing: case discarded\n".into(),
        panic: Some(PanicInfo {
            category: "explicit-panic".into(),
            message: "std.testing: case discarded".into(),
            ..PanicInfo::default()
        }),
        ..BodyResult::default()
    }
}

#[test]
fn discard_replaces_instance_but_same_panic_after_show_is_failure() {
    let mut calls = Vec::new();
    let mut fake = Fake(|state: State, reuse: bool| {
        calls.push(reuse);
        state.start(1, 0, 0)?.expect("plan");
        if calls.len() == 1 {
            Ok(discarded())
        } else {
            state.show("shown");
            Ok(BodyResult::default())
        }
    });
    let case = TestCase::default();
    let mut results = Vec::new();
    execute_case(&case, &mut fake, &mut |result| results.push(result)).expect("run");
    assert_eq!(calls, [false, false]);
    assert_eq!(results[0].status, 0);
    let mut fake = Fake(|state: State, _: bool| {
        state.start(1, 0, 0)?.expect("plan");
        state.show("bad input");
        Ok(discarded())
    });
    results.clear();
    execute_case(&case, &mut fake, &mut |result| results.push(result)).expect("run");
    assert_eq!(results[0].input.as_deref(), Some("bad input"));
    assert!(results[0].trapped);
}

#[test]
fn discard_limit_is_exact_and_never_reuses_a_poisoned_instance() {
    let mut calls = 0;
    let mut fake = Fake(|state: State, reuse: bool| {
        assert!(!reuse);
        calls += 1;
        state.start(2, 0, 0)?.expect("plan");
        Ok(discarded())
    });
    let mut results = Vec::new();
    execute_case(&TestCase::default(), &mut fake, &mut |result| {
        results.push(result);
    })
    .expect("run");
    assert_eq!(calls, 21);
    assert_eq!(results[0].status, 1);
    assert_eq!(
        results[0].stderr,
        "the property discarded 21 cases, more than 10 times its 2 cases\n"
    );
}

/// A discard is the host's recorded panic: a message whose text imitates
/// the discard report is a failure (`std-testing.runner.discard-read`).
#[test]
fn a_message_imitating_the_discard_report_is_a_failure() {
    let mut fake = Fake(|state: State, _: bool| {
        state.start(1, 0, 0)?.expect("plan");
        let report = "explicit-panic: x\npanic: explicit-panic: std.testing: case discarded\n";
        Ok(BodyResult {
            status: 3,
            trapped: true,
            stderr: format!("panic: {report}"),
            panic: Some(PanicInfo::from_report(report)),
            ..BodyResult::default()
        })
    });
    let mut results = Vec::new();
    execute_case(&TestCase::default(), &mut fake, &mut |result| {
        results.push(result);
    })
    .expect("run");
    assert_eq!(results.len(), 1);
    assert!(results[0].trapped);
    assert_eq!(
        results[0].panic.as_ref().map(|p| p.message.as_str()),
        Some("x\npanic: explicit-panic: std.testing: case discarded")
    );
}

/// The report's first `: ` ends the category; the message keeps the rest.
#[test]
fn a_report_splits_at_its_first_separator() {
    let panic = PanicInfo::from_report("explicit-panic: panic: integer-overflow: no\n");
    assert_eq!(
        (panic.category.as_str(), panic.message.as_str()),
        ("explicit-panic", "panic: integer-overflow: no")
    );
    let panic = PanicInfo::from_report("assertion-failed\n");
    assert_eq!(
        (panic.category.as_str(), panic.message.as_str()),
        ("assertion-failed", "")
    );
}

#[test]
fn table_rows_are_fresh_and_empty_tables_have_no_result_fields() {
    for count in [0, 3] {
        let mut rows = Vec::new();
        let mut fake = Fake(|state: State, reuse: bool| {
            assert!(!reuse);
            rows.push(state.row(count));
            Ok(BodyResult::default())
        });
        let case = TestCase {
            test: 9,
            kind: "it_each".into(),
            ..TestCase::default()
        };
        let mut results = Vec::new();
        execute_case(&case, &mut fake, &mut |result| results.push(result)).expect("run");
        if count == 0 {
            assert_eq!(rows, [0]);
            assert_eq!(
                results,
                [CaseRun {
                    test: 9,
                    row: Some((0, 0)),
                    ..CaseRun::default()
                }]
            );
        } else {
            assert_eq!(rows, [0, 1, 2]);
            assert_eq!(
                results.iter().map(|r| r.row).collect::<Vec<_>>(),
                [Some((0, 3)), Some((1, 3)), Some((2, 3))]
            );
        }
        assert!(results.last().expect("result").last());
    }
}

#[test]
fn body_error_still_removes_lazy_temporary_directory() {
    let mut path = None;
    let mut fake = Fake(|state: State, _: bool| {
        let directory = std::path::PathBuf::from(state.temp_dir()?);
        assert!(directory.is_dir());
        path = Some(directory);
        Err("adapter failed".into())
    });
    assert!(execute_case(&TestCase::default(), &mut fake, &mut |_| {}).is_err());
    assert!(!path.expect("lazy directory").exists());
}

#[test]
fn property_exhaustion_is_success_without_a_guest_panic() {
    let mut fake = Fake(|state: State, _: bool| {
        assert!(state.start(0, 0, 0)?.is_none());
        assert!(state.ended());
        Ok(BodyResult {
            ended: true,
            ..BodyResult::default()
        })
    });
    let mut results = Vec::new();
    execute_case(&TestCase::default(), &mut fake, &mut |result| {
        results.push(result);
    })
    .expect("run");
    assert_eq!(results[0].status, 0);
    assert!(!results[0].trapped);
}

#[test]
fn retained_adapter_is_finalized_on_both_success_and_error() {
    struct Retained {
        fail: bool,
        finished: bool,
    }
    impl BodyRunner for Retained {
        fn run_body(&mut self, _: State, _: bool) -> Result<BodyResult, String> {
            if self.fail {
                Err("failed".into())
            } else {
                Ok(BodyResult::default())
            }
        }
        fn finish(&mut self) {
            self.finished = true;
        }
    }
    for fail in [false, true] {
        let mut runner = Retained {
            fail,
            finished: false,
        };
        assert_eq!(
            execute_case(&TestCase::default(), &mut runner, &mut |_| {}).is_err(),
            fail
        );
        assert!(runner.finished);
    }
}

#[test]
fn shrinks_save_recorded_draws_and_every_attempt_is_fresh() {
    let owner = State::new(TestCase::default());
    let root = std::path::PathBuf::from(owner.temp_dir().expect("regression root"));
    let path = root.join("regressions/one");
    let mut reuse_flags = Vec::new();
    let mut fake = Fake(|state: State, reuse: bool| {
        reuse_flags.push(reuse);
        let encoded = encode(&state.start(1, 6, 0)?.expect("plan"));
        let mut decoder = Dec::new(encoded.done());
        assert_eq!(decoder.leb().expect("no example"), 0);
        decoder.zz().expect("seed");
        decoder.zz().expect("size");
        let count = decoder.leb().expect("replay length");
        let draw = if count == 0 {
            8
        } else {
            decoder.zz().expect("replay draw")
        };
        state.record(draw);
        state.show(&draw.to_string());
        Ok(BodyResult {
            status: if draw >= 3 { 3 } else { 0 },
            trapped: draw >= 3,
            us: 1,
            ..BodyResult::default()
        })
    });
    let case = TestCase {
        regression: Some(path.clone()),
        ..TestCase::default()
    };
    let mut results = Vec::new();
    execute_case(&case, &mut fake, &mut |result| results.push(result)).expect("shrink run");
    assert_eq!(reuse_flags, [false; 7]);
    assert_eq!(std::fs::read_to_string(&path).expect("saved"), "3\n");
    assert_eq!(results[0].input.as_deref(), Some("3"));
    assert_eq!(results[0].us, 7);
    assert!(results[0].trapped);
    let mut replayed = Vec::new();
    let mut fake = Fake(|state: State, _: bool| {
        let encoded = encode(
            &state
                .start(0, 0, 0)?
                .expect("saved plan even without fresh cases"),
        );
        let mut decoder = Dec::new(encoded.done());
        assert_eq!(decoder.leb().expect("no example"), 0);
        decoder.zz().expect("seed");
        decoder.zz().expect("size");
        assert_eq!(decoder.leb().expect("one draw"), 1);
        replayed.push(decoder.zz().expect("saved draw"));
        state.show("saved");
        Ok(BodyResult::default())
    });
    execute_case(&case, &mut fake, &mut |_| {}).expect("replay");
    assert_eq!(replayed, [3]);
}

#[test]
fn shrink_timeout_before_start_preserves_the_counterexample_and_stops() {
    let owner = State::new(TestCase::default());
    let root = std::path::PathBuf::from(owner.temp_dir().unwrap());
    let path = root.join("regression");
    let mut calls = 0;
    let mut fake = Fake(|state: State, _: bool| {
        calls += 1;
        if calls == 1 {
            state.start(1, 1000, 0)?.expect("initial plan");
            state.record(8);
            state.show("counterexample");
        }
        if calls == 2 {
            // Match the actual native adapter: the replay's CLI deadline
            // has expired before PropertyRunner.start can run.
            state.set_io_deadline(Some(std::time::Instant::now()));
        }
        Ok(BodyResult {
            status: 3,
            trapped: true,
            stderr: if calls == 1 {
                "panic: explicit-panic: assertion failed\n".into()
            } else {
                "panic: time-limit: the program exceeded its time limit\n".into()
            },
            ..BodyResult::default()
        })
    });
    let case = TestCase {
        regression: Some(path.clone()),
        ..TestCase::default()
    };
    let mut results = Vec::new();
    execute_case(&case, &mut fake, &mut |result| results.push(result)).unwrap();
    assert_eq!(
        calls, 2,
        "an unavailable replay stops the whole shrink loop"
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), "8\n");
    assert_eq!(results[0].input.as_deref(), Some("counterexample"));
    assert_eq!(
        results[0].stderr,
        "panic: explicit-panic: assertion failed\n"
    );
    let mut later = Fake(|_: State, _: bool| Ok(BodyResult::default()));
    execute_case(
        &TestCase {
            test: 1,
            ..TestCase::default()
        },
        &mut later,
        &mut |result| results.push(result),
    )
    .unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[1].test, 1);
    assert_eq!(results[1].status, 0);
}
