use hd_diag::{Code, Severity};
use hd_driver::diagnostics::{Diag, Edit, Fix, Label, decode, encode, from_output};
use hd_driver::testing::report_stop;

mod tests {
    use super::{Code, Edit, Fix, Severity};

    #[test]
    fn encoded_diagnostics_decode_equal() {
        let mut warned = Diag::new(Some(Code::UnknownName), Severity::Warning, "w");
        warned.file = Some("src/a.hd".to_owned());
        (
            warned.line,
            warned.column,
            warned.end_line,
            warned.end_column,
        ) = (Some(1), Some(2), Some(3), Some(4));
        warned.fixes.push(Fix {
            message: "fix".to_owned(),
            exact: true,
            edits: vec![Edit {
                file: "src/a.hd".to_owned(),
                start: 1,
                end: 2,
                text: "é".to_owned(),
            }],
        });
        warned.labels.push(super::Label {
            message: "previous implementation".into(),
            file: Some("src/b.hd".into()),
            line: Some(2),
            column: Some(3),
            end_line: Some(2),
            end_column: Some(8),
        });
        assert!(
            warned
                .text()
                .contains("note: src/b.hd:2:3: previous implementation")
        );
        assert!(
            warned
                .json()
                .contains("\"labels\":[{\"message\":\"previous implementation\"")
        );
        let diags = vec![warned, Diag::error(None, "no code")];
        assert_eq!(super::decode(&super::encode(&diags)), Some(diags));
        assert_eq!(super::decode(&[1, 0, 0]), None);
    }

    #[test]
    fn json_marks_uncertain_and_ranked_repairs_as_guesses_after_cache_decode() {
        let mut d = Diag::error(Some(Code::MixedSignedness), "choose an integer family");
        let fix = |message: &str, exact| Fix {
            message: message.into(),
            exact,
            edits: vec![Edit {
                file: "main.hd".into(),
                start: 0,
                end: 0,
                text: "i64(".into(),
            }],
        };
        d.fixes.push(fix("unambiguous", true));
        assert!(d.json().contains("\"guess\":false"));
        d.fixes[0].exact = false;
        assert!(d.json().contains("\"guess\":true"));
        d.fixes[0].exact = true;
        d.fixes.push(fix("alternative", true));
        let encoded = super::encode(&[d.clone()]);
        assert_eq!(super::decode(&encoded), Some(vec![d.clone()]));
        assert_eq!(d.json().matches("\"guess\":true").count(), 2);
        assert!(d.json().find("unambiguous") < d.json().find("alternative"));
    }

    use hd_base::{DefId, NodeIdx, NotImplemented, Stage};
    use hd_diag::DiagBuf;
    use hd_tir::ir::{BodyKind, TirBuilder, TirSink, VerifyError};
    use hd_types::Ty;

    use super::{Diag, report_stop};

    /// A body whose reserved slot stays empty fails the TIR verifier; the
    /// stop reads as a compiler bug with no code, never as `unsupported`.
    #[test]
    fn a_verifier_failure_reports_an_internal_error() {
        let mut b = TirBuilder::new(DefId::from_raw(0), BodyKind::Fn);
        let blk = b.open_block();
        let slot = b.reserve();
        let root = b.close_block(blk, Some(slot.as_ref()), Ty::VOID, NodeIdx::NONE);
        let errs = b.finish(root, &[]).expect_err("an unfilled slot");
        assert!(errs.iter().any(|e| e.invariant == 14), "{errs:?}");
        let mut diags = DiagBuf::default();
        let stop = VerifyError::bug(Stage::Body, "TIR verifier", &errs);
        report_stop(&mut diags, Stage::Body, &stop);
        assert!(diags.is_internal(0));
        let text = Diag::error(diags.shown_code(0), diags.get_text(diags.message[0])).text();
        assert!(
            text.starts_with(
                "error: internal-error: the compiler hit a bug in stage Body (TIR verifier: invariant 14 at %"
            ),
            "{text}"
        );
        assert!(text.ends_with("); please report it"), "{text}");
        assert!(!text.contains("unsupported"), "{text}");
        // A construct this build lacks still reads as `unsupported`.
        report_stop(
            &mut diags,
            Stage::Emit,
            &NotImplemented::new(Stage::Emit, "x"),
        );
        assert!(!diags.is_internal(1));
    }

    #[test]
    fn a_std_instance_invariant_keeps_its_file_in_single_file_diagnostics() {
        use hd_base::{FileId, Span};
        use hd_project::MemorySources;
        let mut sources = MemorySources::default();
        sources.insert("param.hd", include_str!("programs/diagnostics/param.hd"));
        let store = crate::support::warm_std_store();
        let host = hd_driver::Host {
            render_tir: &[],
            sources: &sources,
            store: &store,
            clock: &hd_driver::NoClock,
            executor: hd_driver::Executor::Serial(hd_sched::SerialOrder::Priority),
        };
        let mut out = hd_driver::build(&host, "app", &hd_driver::Goal::Analyze);
        assert!(!out.diags.has_errors(), "{}", out.render());
        let file = out.files.iter().position(|f| f == "<std>/ops.hd").unwrap();
        let text = &out.texts[file];
        let body = text.find("    fn default() -> bool:\n").unwrap();
        let lo = body + text[body..].find("false").unwrap();
        let span = Span {
            file: FileId::from_raw(u32::try_from(file).unwrap()),
            lo: u32::try_from(lo).unwrap(),
            hi: u32::try_from(lo + 5).unwrap(),
        };
        let finding = VerifyError {
            invariant: 24,
            inst: 2,
            what: "injected std instance finding".into(),
        };
        let stop = VerifyError::bug(Stage::Collect, "instance", &[finding]).at(span);
        report_stop(&mut out.diags, Stage::Collect, &stop);
        let user = Span {
            file: FileId::from_raw(
                u32::try_from(out.files.iter().position(|f| f == "param.hd").unwrap()).unwrap(),
            ),
            lo: 0,
            hi: 3,
        };
        out.diags
            .error(Code::TypeMismatch, user, "injected input finding");
        out.diags.add_fix(
            "cross-file edit",
            hd_diag::FixSafety::Exact,
            &[(span, "true")],
        );
        let reported = super::from_output(&out, Some("scratch/param.hd"));
        let std = reported
            .iter()
            .find(|d| d.message.contains("invariant 24"))
            .unwrap();
        assert_eq!(std.file.as_deref(), Some("<std>/ops.hd"));
        let (_, line, column) = out.locate(span);
        assert!(line > 2);
        assert_eq!((std.line, std.column), (Some(line), Some(column)));
        assert!(std.json().contains("\"file\":\"<std>/ops.hd\""));
        assert!(
            std.text()
                .contains(&format!("<std>/ops.hd:{line}:{column}"))
        );
        let input = reported
            .iter()
            .find(|d| d.message == "injected input finding")
            .unwrap();
        assert_eq!(input.file.as_deref(), Some("scratch/param.hd"));
        assert_eq!(input.fixes[0].edits[0].file, "<std>/ops.hd");
    }
}
