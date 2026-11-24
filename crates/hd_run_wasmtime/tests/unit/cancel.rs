mod tests {
    use hd_run_wasmtime::internals::cancel::{CancelStep, HostCancel};
    use std::time::{Duration, Instant};

    #[test]
    fn the_first_request_cancels_and_the_next_or_the_bound_abandons() {
        let t0 = Instant::now();
        let mut c = HostCancel::new(Duration::from_millis(100));
        assert_eq!(c.next(false, t0), None);
        assert_eq!(c.next(true, t0), Some(CancelStep::Cancel));
        let deadline = t0 + Duration::from_millis(100);
        assert_eq!(c.deadline(), Some(deadline));
        assert_eq!(c.next(false, t0 + Duration::from_millis(50)), None);
        assert_eq!(c.next(false, deadline), Some(CancelStep::Abandon));
        assert_eq!(c.deadline(), None);
        assert_eq!(c.next(true, deadline), None);

        let mut twice = HostCancel::new(Duration::from_secs(10));
        assert_eq!(twice.next(true, t0), Some(CancelStep::Cancel));
        assert_eq!(twice.next(true, t0), Some(CancelStep::Abandon));
    }
}
