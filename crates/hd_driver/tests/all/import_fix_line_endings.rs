//! A missing import's fix-it holds byte offsets and the file's line ending
//! (#1862), so a cached result is reused only for the same line-ending
//! form: the same package checked with LF and then CRLF bytes, on one
//! warm store, gets each file's own edit.

use hd_diag::{Code, DiagBuf};
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn diags(main: &str) -> DiagBuf {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", &Goal::Analyze).diags
}

/// Each `unknown-trait` fix-it's one edit: its byte and its text.
fn import_edits(d: &DiagBuf) -> Vec<(u32, String)> {
    (0..d.code.len())
        .filter(|&i| d.code[i] == Code::UnknownTrait)
        .map(|i| {
            let fixes = d.fixes[i];
            assert_eq!(fixes.len, 1);
            let edits = d.fix_edits[fixes.start as usize];
            let edit = &d.edits[edits.start as usize];
            (edit.span.lo, d.get_text(edit.text).to_owned())
        })
        .collect()
}

#[test]
fn a_crlf_file_after_its_lf_twin_gets_its_own_import_fix() {
    let lf = include_str!("programs/import_fix_line_endings/lf.hd");
    let crlf = lf.replace('\n', "\r\n");
    let use_end =
        |text: &str| u32::try_from(text.find("later").expect("the use line") + 5).unwrap();
    let first = import_edits(&diags(lf));
    assert_eq!(first, [(use_end(lf), "\nuse std.num.Integer".to_owned())]);
    let second = import_edits(&diags(&crlf));
    assert_eq!(
        second,
        [(use_end(&crlf), "\r\nuse std.num.Integer".to_owned())]
    );
    // And back: the LF bytes get the LF edit again.
    assert_eq!(import_edits(&diags(lf)), first);
}
