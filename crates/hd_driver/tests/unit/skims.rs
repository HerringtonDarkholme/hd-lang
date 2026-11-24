use hd_driver::testing::FileSkim;

mod tests {
    use super::FileSkim;
    use hd_base::wire::{Reader, Writer};

    #[test]
    fn a_file_skim_round_trips() {
        let text = include_str!("programs/skims/file_skim.hd");
        let s = FileSkim::of(text);
        assert!(s.has_tests_block && s.public_main.is_some() && s.private_main.is_none());
        assert_eq!(s.test_uses, ["pkg.other"]);
        // The block's `use` line is no unit-edge line, and a line led by
        // another name is no use line.
        let lines: Vec<&str> = s
            .use_lines
            .iter()
            .map(|&(lo, hi)| &text[lo as usize..hi as usize])
            .collect();
        assert_eq!(lines, ["use std.text"]);
        assert_eq!(s.use_paths, ["std.text"]);
        let mut w = Writer::default();
        s.write(&mut w);
        let mut r = Reader::new(&w.bytes);
        assert_eq!(FileSkim::read(&mut r), s);
        assert!(r.ok() && r.at_end());
        let mut short = Reader::new(&w.bytes[..w.bytes.len() - 3]);
        let _ = FileSkim::read(&mut short);
        assert!(!short.ok());
    }

    /// Each written form of a `use` line gives its path, in a `tests:`
    /// block and out of it, with LF and CRLF endings: `pub`, a group, a
    /// rename, a trailing comment.
    #[test]
    fn use_paths_are_the_written_paths() {
        let lf = include_str!("programs/skims/use_forms.hd");
        for text in [lf.to_owned(), lf.replace('\n', "\r\n")] {
            let s = FileSkim::of(&text);
            assert_eq!(s.use_paths, ["std.a", "self.d"]);
            assert_eq!(s.test_uses, ["pkg.f", "std.h"]);
        }
    }
}
