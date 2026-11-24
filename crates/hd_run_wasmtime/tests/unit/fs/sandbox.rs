mod timestamp_tests {
    use hd_run_wasmtime::internals::fs::sandbox::modified_millis;
    use std::time::UNIX_EPOCH;
    use std::{io, time::Duration};

    #[test]
    fn modification_times_are_signed_whole_milliseconds_or_absent() {
        assert_eq!(modified_millis(Ok(UNIX_EPOCH)), Some(0));
        assert_eq!(
            modified_millis(Ok(UNIX_EPOCH + Duration::from_millis(123))),
            Some(123)
        );
        assert_eq!(
            modified_millis(Ok(UNIX_EPOCH - Duration::from_millis(123))),
            Some(-123)
        );
        assert_eq!(
            modified_millis(Ok(UNIX_EPOCH - Duration::from_nanos(1))),
            Some(0)
        );
        assert_eq!(
            modified_millis(Err(io::ErrorKind::Unsupported.into())),
            None
        );
        if let Some(time) =
            UNIX_EPOCH.checked_add(Duration::from_millis(i64::MAX.cast_unsigned() + 1))
        {
            assert_eq!(modified_millis(Ok(time)), None);
        }
        let edge = Duration::from_millis(i64::MAX.cast_unsigned() + 1);
        if let Some(time) = UNIX_EPOCH.checked_sub(edge) {
            assert_eq!(modified_millis(Ok(time)), Some(i64::MIN));
        }
        if let Some(time) = UNIX_EPOCH.checked_sub(edge + Duration::from_millis(1)) {
            assert_eq!(modified_millis(Ok(time)), None);
        }
    }
}
