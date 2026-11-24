//! `hd_wasm`'s unit tests, over the `testing` facade (`src/testing.rs`):
//! one module per source module they test, at the same path, so each
//! test keeps its name.

mod boundary;
mod bulk;
mod count_tests;
mod cycle_graph;
mod emit;
mod map;
mod module;
mod opt;
mod rt;
mod static_data;
mod tables;
mod task;
mod tests;
mod views;
mod wide;

mod layout_error_location_tests {
    use hd_base::{FileId, NotImplemented, Span, Stage};
    use hd_wasm::testing::locate_layout_error;

    #[test]
    fn layout_stops_use_available_context_and_preserve_inner_locations() {
        let span = Span {
            file: FileId::from_raw(3),
            lo: 17,
            hi: 24,
        };
        let error = NotImplemented::too_large(Stage::Emit, "large value");
        assert_eq!(
            locate_layout_error(error.clone(), || Some(span)),
            error.clone().at(span)
        );
        assert_eq!(locate_layout_error(error.clone(), || None), error);
        let located = error.at(span);
        assert_eq!(
            locate_layout_error(located.clone(), || panic!("already located")),
            located
        );
        let bug = NotImplemented::bug(Stage::Emit, "internal check");
        assert_eq!(
            locate_layout_error(bug.clone(), || panic!("not a layout stop")),
            bug
        );
    }
}
