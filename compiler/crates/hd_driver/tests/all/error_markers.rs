//! `invalid-error-marker` (spec 14 `annot.error.form.argument`,
//! `annot.error.form.marker-argument`, `annot.error.cause.one`,
//! `annot.error.from.type-parameter`): each bad `@error`, `@from`, or
//! `@source` form is one diagnostic on its line, and valid forms are clean.

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn analyze(main: &str) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", &Goal::Analyze)
}

fn codes(main: &str) -> Vec<Code> {
    let out = analyze(main);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .filter(|c| *c != Code::Unsupported)
        .collect()
}

const DISK: &str = "@error(\"disk full\")\ndata DiskError:\n    free: i64\n\n";

#[test]
fn valid_forms_are_clean() {
    let src = format!(
        "{DISK}@error\nenum SaveError[E]:\n    @error(\"cannot save\")\n    Write(@from error: DiskError)\n    @error(transparent)\n    Wrap(@source inner: E)\n    @error(\"two\")\n    Pair(a: string, @source b: DiskError)\n    @error(\"opt\")\n    Opt(@source c: DiskError?)\n"
    );
    assert_eq!(codes(&src), vec![]);
}

#[test]
fn transparent_data_type_is_clean() {
    let src = format!("{DISK}@error(transparent)\ndata Wrap:\n    @from\n    inner: DiskError\n");
    assert_eq!(codes(&src), vec![]);
}

#[test]
fn argument_forms_are_rejected_once() {
    for line in [
        "@error(opaque)",
        "@error(42)",
        "@error(\"closed\", \"shut\")",
        "@error()",
    ] {
        let src = format!("@error\nenum E:\n    {line}\n    Full\n");
        assert_eq!(codes(&src), vec![Code::InvalidErrorMarker], "{line}");
    }
    let src = "@error(42)\ndata D:\n    free: i64\n";
    assert_eq!(codes(src), vec![Code::InvalidErrorMarker]);
}

#[test]
fn wrong_target_stays_target_kind() {
    let src = "@error(opaque)\nfn tool() -> string:\n    \"hammer\"\n";
    assert_eq!(codes(src), vec![Code::DecoratorTargetKind]);
}

#[test]
fn marker_argument_is_rejected() {
    for marker in ["@from(strict)", "@source(strict)"] {
        let src =
            format!("{DISK}@error\nenum E:\n    @error(\"x\")\n    Y({marker} error: DiskError)\n");
        assert_eq!(codes(&src), vec![Code::InvalidErrorMarker], "{marker}");
    }
}

#[test]
fn second_cause_is_rejected() {
    let src = format!(
        "{DISK}@error\nenum E:\n    @error(\"x\")\n    Both(@source a: DiskError, @source b: DiskError)\n"
    );
    assert_eq!(codes(&src), vec![Code::InvalidErrorMarker]);
}

#[test]
fn second_cause_in_data_type_is_rejected() {
    let src = format!(
        "{DISK}@error(\"x\")\ndata D:\n    @source\n    a: DiskError\n    @source\n    b: DiskError\n"
    );
    assert_eq!(codes(&src), vec![Code::InvalidErrorMarker]);
}

#[test]
fn from_type_parameter_is_rejected() {
    let src = "@error\nenum App[E]:\n    @error(\"inner\")\n    Inner(@from error: E)\n";
    assert_eq!(codes(src), vec![Code::InvalidErrorMarker]);
}

#[test]
fn from_type_parameter_is_by_resolution_not_spelling() {
    let src = format!(
        "{DISK}@error\nenum App[E]:\n    @error(\"inner\")\n    Inner(@from error: DiskError)\n    @error(\"list\")\n    Items(@source items: E?)\n"
    );
    assert_eq!(codes(&src), vec![]);
    let src =
        "@error\nenum App[DiskError]:\n    @error(\"inner\")\n    Inner(@from error: DiskError)\n";
    assert_eq!(codes(src), vec![Code::InvalidErrorMarker]);
}

#[test]
fn from_type_parameter_on_data_is_rejected() {
    let src = "@error(\"x\")\ndata D[E]:\n    @from\n    inner: E\n";
    assert_eq!(codes(src), vec![Code::InvalidErrorMarker]);
}

/// `annot.error.cause.type`: a cause member whose type lacks `Error` is an
/// unmet bound, reported once, plain or optional.
#[test]
fn cause_without_error_is_an_unmet_bound() {
    for ty in ["string", "string?"] {
        let src = format!("@error\nenum E:\n    @error(\"bad\")\n    Path(@source reason: {ty})\n");
        assert_eq!(codes(&src), vec![Code::UnsatisfiedTraitBound], "{ty}");
    }
}
