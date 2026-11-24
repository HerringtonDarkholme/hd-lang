mod tests {
    use hd_run_wasmtime::internals::workers::*;
    use std::sync::mpsc;
    use tokio::sync::oneshot;

    /// A cancelled send's frame is skipped, and the next open one is taken.
    #[test]
    fn a_frame_whose_send_was_cancelled_is_not_taken() {
        let (tx, rx) = mpsc::channel();
        let mut port: Port = (rx, None);
        let (cancelled, gone) = oneshot::channel();
        let (open, mut waits) = oneshot::channel();
        tx.send((vec![1], cancelled)).unwrap();
        tx.send((vec![2], open)).unwrap();
        drop((tx, gone));
        assert_eq!(take(&mut port), Some(vec![2]));
        port.1.take().unwrap().send(vec![4]).unwrap();
        assert_eq!(waits.try_recv(), Ok(vec![4]));
        assert_eq!(take(&mut port), None, "a closed channel has no frame");
    }
}
