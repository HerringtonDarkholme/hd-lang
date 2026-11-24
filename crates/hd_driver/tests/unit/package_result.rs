use hd_base::wire::{Reader, Writer};
use hd_base::{FileId, Span};
use hd_diag::{Code, DiagBuf};
use hd_driver::testing::{read_diags, write_diags};

mod label_tests {
    use super::*;

    #[test]
    fn package_diagnostics_keep_secondary_spans_and_labels() {
        let span = Span {
            file: FileId::from_raw(3),
            lo: 8,
            hi: 14,
        };
        let mut before = DiagBuf::default();
        before.error(Code::OverlappingImpl, span, "overlap");
        before.add_label(
            Span {
                lo: 1,
                hi: 7,
                ..span
            },
            "other implementation",
        );
        before.error(Code::TypeMismatch, span, "another error");
        let mut w = Writer::default();
        write_diags(&mut w, &before);
        let mut r = Reader::new(&w.bytes);
        let after = read_diags(&mut r, &Code::from_name).expect("valid diagnostics");
        assert!(r.at_end());
        assert_eq!(after.render_json(), before.render_json());
        assert_eq!(after.labels(0), before.labels(0));
        assert!(after.labels(1).is_empty());
    }
}
