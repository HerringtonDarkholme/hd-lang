//! A small, bounded document printer for the one hd style.

use std::ops::Range;

pub const MAX_COLUMNS: usize = 100;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PrintStats {
    pub preprocessing_visits: usize,
    pub printing_visits: usize,
    pub fit_calls: usize,
    pub fits_events: usize,
    pub fit_jumps: usize,
    pub groups: usize,
}

#[derive(Clone, Debug)]
pub(crate) enum Command {
    Text(Range<usize>),
    Atom(&'static str),
    StartGroup,
    EndGroup,
    StartIndent,
    EndIndent,
    StartConditionalIndent,
    EndConditionalIndent,
    StartFlat,
    EndFlat,
    SoftLine(&'static str),
    HardLine,
    BlankLine,
    LineSuffix {
        span: Range<usize>,
        leading_space: bool,
    },
    ModeText {
        flat: &'static str,
        broken: &'static str,
    },
}

pub(crate) struct Document<'s> {
    source: &'s str,
    pub commands: Vec<Command>,
}

impl<'s> Document<'s> {
    pub(crate) fn new(source: &'s str) -> Self {
        Self {
            source,
            commands: Vec::new(),
        }
    }

    pub(crate) fn print(self) -> (String, PrintStats) {
        Printer::new(self.source, self.commands).print()
    }
}

struct Printer<'s> {
    source: &'s str,
    commands: Vec<Command>,
    group_end: Vec<usize>,
    next_flat: Vec<usize>,
    next_broken: Vec<usize>,
    flat_measure: Vec<u8>,
    output: String,
    column: usize,
    indent: usize,
    line_start: bool,
    modes: Vec<bool>,
    preserve_line_end: bool,
    stats: PrintStats,
}

impl<'s> Printer<'s> {
    fn new(source: &'s str, commands: Vec<Command>) -> Self {
        let mut group_end = vec![usize::MAX; commands.len()];
        let mut open = Vec::new();
        for (index, command) in commands.iter().enumerate() {
            match command {
                Command::StartGroup => open.push(index),
                Command::EndGroup => {
                    if let Some(start) = open.pop() {
                        group_end[start] = index;
                    }
                }
                _ => {}
            }
        }
        debug_assert!(open.is_empty(), "formatter document groups are balanced");
        #[cfg(debug_assertions)]
        {
            let mut content = Vec::with_capacity(commands.len() + 1);
            content.push(0_usize);
            for command in &commands {
                let present = match command {
                    Command::Text(span) => !span.is_empty(),
                    Command::Atom(text) | Command::SoftLine(text) => !text.is_empty(),
                    Command::HardLine | Command::BlankLine | Command::LineSuffix { .. } => true,
                    Command::ModeText { flat, broken } => !flat.is_empty() || !broken.is_empty(),
                    _ => false,
                };
                content.push(content.last().copied().unwrap_or(0) + usize::from(present));
            }
            for (start, &end) in group_end.iter().enumerate() {
                if end != usize::MAX {
                    debug_assert!(
                        content[end] > content[start + 1],
                        "formatter document contains an empty group at command {start}"
                    );
                }
            }
        }
        let len = commands.len();
        let mut next_flat = vec![len; len + 1];
        let mut next_broken = vec![len; len + 1];
        let mut flat_measure = vec![0; len];
        let mut following_flat = len;
        let mut following_broken = len;
        for index in (0..len).rev() {
            let ((width, newline), (expanded_width, expanded_newline)) = match &commands[index] {
                Command::Text(span) => {
                    let measured = capped_width(&source[span.clone()]);
                    (measured, measured)
                }
                Command::Atom(text) => {
                    let measured = capped_width(text);
                    (measured, measured)
                }
                Command::SoftLine(text) => (capped_width(text), (0, true)),
                Command::ModeText { flat, broken } => (capped_width(flat), capped_width(broken)),
                Command::HardLine | Command::BlankLine => ((0, true), (0, true)),
                _ => ((0, false), (0, false)),
            };
            let width = u8::try_from(width).expect("capped formatter width fits in one byte");
            flat_measure[index] = width | u8::from(newline) << 7;
            let hard_break = matches!(commands[index], Command::HardLine | Command::BlankLine);
            if width > 0 || newline || hard_break {
                following_flat = index;
            }
            if expanded_width > 0 || expanded_newline || hard_break {
                following_broken = index;
            }
            next_flat[index] = following_flat;
            next_broken[index] = following_broken;
        }
        Self {
            source,
            commands,
            group_end,
            next_flat,
            next_broken,
            flat_measure,
            output: String::with_capacity(source.len()),
            column: 0,
            indent: 0,
            line_start: true,
            modes: Vec::new(),
            preserve_line_end: false,
            stats: PrintStats {
                preprocessing_visits: len,
                ..PrintStats::default()
            },
        }
    }

    fn print(mut self) -> (String, PrintStats) {
        let mut index = 0;
        while index < self.commands.len() {
            self.stats.printing_visits += 1;
            match self.commands[index].clone() {
                Command::Text(span) => self.write(&self.source[span]),
                Command::Atom(text) => self.write(text),
                Command::StartGroup => {
                    self.stats.groups += 1;
                    let inherited_flat = self.modes.last().is_some_and(|mode| *mode);
                    let flat = inherited_flat || self.fits(index + 1, self.group_end[index]);
                    self.modes.push(flat);
                }
                Command::EndGroup => {
                    self.modes.pop();
                }
                Command::StartIndent => {
                    self.indent += 4;
                }
                Command::EndIndent => {
                    self.indent = self.indent.saturating_sub(4);
                }
                Command::StartConditionalIndent => {
                    if !self.modes.last().copied().unwrap_or(false) {
                        self.indent += 4;
                    }
                }
                Command::EndConditionalIndent => {
                    if !self.modes.last().copied().unwrap_or(false) {
                        self.indent = self.indent.saturating_sub(4);
                    }
                }
                Command::StartFlat => self.modes.push(true),
                Command::EndFlat => {
                    self.modes.pop();
                }
                Command::SoftLine(flat) => {
                    if self.modes.last().copied().unwrap_or(false) {
                        self.write(flat);
                    } else {
                        self.newline(false);
                    }
                }
                Command::HardLine => self.newline(false),
                Command::BlankLine => self.newline(true),
                Command::LineSuffix {
                    span,
                    leading_space,
                } => {
                    if leading_space {
                        self.write(" ");
                    }
                    self.write(&self.source[span]);
                    self.preserve_line_end = true;
                }
                Command::ModeText { flat, broken } => {
                    let text = if self.modes.last().copied().unwrap_or(false) {
                        flat
                    } else {
                        broken
                    };
                    self.write(text);
                }
            }
            index += 1;
        }
        while self.output.ends_with('\n') {
            self.output.pop();
        }
        if !self.output.is_empty() {
            self.output.push('\n');
        }
        debug_assert!(
            self.modes.is_empty(),
            "formatter document modes are balanced"
        );
        (self.output, self.stats)
    }

    /// Scan at most the width plus one positive-width events. Zero-width
    /// structure is skipped without recursion, so nesting cannot make a fit
    /// probe unbounded.
    fn fits(&mut self, index: usize, end: usize) -> bool {
        self.stats.fit_calls += 1;
        #[cfg(test)]
        let reference = self.fits_reference(index, end);
        let result = self.fits_optimized(index, end);
        #[cfg(test)]
        assert_eq!(
            result, reference,
            "optimized fit diverged at {index}..{end}"
        );
        result
    }

    fn fits_optimized(&mut self, mut index: usize, end: usize) -> bool {
        let column = if self.line_start {
            self.indent
        } else {
            self.column
        };
        let mut remaining = MAX_COLUMNS.saturating_sub(column);
        let mut positive = 0;
        while index < self.commands.len() {
            if index < end {
                let next = self.next_flat[index];
                index = if next < end { next } else { end };
            } else {
                index = self.next_broken[index];
            }
            self.stats.fit_jumps += 1;
            if index >= self.commands.len() {
                return true;
            }
            let continuation = index >= end;
            match &self.commands[index] {
                Command::Text(_) => {
                    if self.flat_measure[index] & 0x80 != 0 {
                        return false;
                    }
                    let width = usize::from(self.flat_measure[index] & 0x7f);
                    if width > 0 {
                        positive += 1;
                        self.stats.fits_events += 1;
                    }
                    if width > remaining {
                        return false;
                    }
                    remaining -= width;
                }
                Command::SoftLine(_) if continuation => return true,
                Command::Atom(_) | Command::SoftLine(_) => {
                    let width = usize::from(self.flat_measure[index] & 0x7f);
                    if width > 0 {
                        positive += 1;
                        self.stats.fits_events += 1;
                    }
                    if width > remaining {
                        return false;
                    }
                    remaining -= width;
                }
                Command::ModeText { .. } => {
                    let width = if continuation {
                        match &self.commands[index] {
                            Command::ModeText { broken, .. } => capped_width(broken).0,
                            _ => unreachable!(),
                        }
                    } else {
                        usize::from(self.flat_measure[index] & 0x7f)
                    };
                    if width > 0 {
                        positive += 1;
                        self.stats.fits_events += 1;
                    }
                    if width > remaining {
                        return false;
                    }
                    remaining -= width;
                }
                Command::HardLine | Command::BlankLine => return continuation,
                Command::LineSuffix { .. } => {}
                Command::StartGroup
                | Command::EndGroup
                | Command::StartIndent
                | Command::EndIndent
                | Command::StartConditionalIndent
                | Command::EndConditionalIndent
                | Command::StartFlat
                | Command::EndFlat => {}
            }
            if positive > MAX_COLUMNS + 1 {
                return false;
            }
            index += 1;
        }
        true
    }

    #[cfg(test)]
    fn fits_reference(&self, mut index: usize, end: usize) -> bool {
        let column = if self.line_start {
            self.indent
        } else {
            self.column
        };
        let mut remaining = MAX_COLUMNS.saturating_sub(column);
        let mut positive = 0;
        while index < self.commands.len() {
            let continuation = index >= end;
            let width = match &self.commands[index] {
                Command::Text(_) => {
                    if self.flat_measure[index] & 0x80 != 0 {
                        return false;
                    }
                    usize::from(self.flat_measure[index] & 0x7f)
                }
                Command::Atom(_) => usize::from(self.flat_measure[index] & 0x7f),
                Command::SoftLine(_) if continuation => return true,
                Command::SoftLine(_) => usize::from(self.flat_measure[index] & 0x7f),
                Command::ModeText { broken, .. } if continuation => capped_width(broken).0,
                Command::ModeText { .. } => usize::from(self.flat_measure[index] & 0x7f),
                Command::HardLine | Command::BlankLine => return continuation,
                Command::LineSuffix { .. }
                | Command::StartGroup
                | Command::EndGroup
                | Command::StartIndent
                | Command::EndIndent
                | Command::StartConditionalIndent
                | Command::EndConditionalIndent
                | Command::StartFlat
                | Command::EndFlat => 0,
            };
            if width > 0 {
                positive += 1;
                if width > remaining || positive > MAX_COLUMNS + 1 {
                    return false;
                }
                remaining -= width;
            }
            index += 1;
        }
        true
    }

    fn write(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.line_start {
            for _ in 0..self.indent {
                self.output.push(' ');
            }
            self.column = self.indent;
            self.line_start = false;
        }
        self.output.push_str(text);
        if let Some(last) = text.rsplit_once('\n') {
            self.column = scalar_width(last.1);
            self.line_start = text.ends_with('\n');
        } else {
            self.column += scalar_width(text);
        }
    }

    fn newline(&mut self, blank: bool) {
        if !self.preserve_line_end {
            while self.output.ends_with([' ', '\t', '\r']) {
                self.output.pop();
            }
        }
        if self.output.is_empty() {
            self.column = 0;
            self.line_start = true;
            return;
        }
        if !self.output.ends_with('\n') {
            self.output.push('\n');
        }
        if blank && !self.output.ends_with("\n\n") {
            self.output.push('\n');
        }
        self.column = 0;
        self.line_start = true;
        self.preserve_line_end = false;
    }
}

fn capped_width(text: &str) -> (usize, bool) {
    let mut width = 0;
    let mut newline = false;
    for &byte in text.as_bytes() {
        newline |= byte == b'\n';
        if byte & 0xc0 != 0x80 {
            width = (width + 1).min(MAX_COLUMNS + 1);
        }
    }
    (width, newline)
}

fn scalar_width(text: &str) -> usize {
    text.as_bytes()
        .iter()
        .filter(|&&byte| byte & 0xc0 != 0x80)
        .count()
}

#[cfg(test)]
mod tests {
    use super::{Command as C, Document, MAX_COLUMNS};

    #[test]
    fn groups_choose_one_flat_or_broken_layout() {
        let source = "abcdefghij";
        let mut short = Document::new(source);
        short.commands.extend([
            C::StartGroup,
            C::Text(0..5),
            C::SoftLine(" "),
            C::Text(5..10),
            C::EndGroup,
        ]);
        assert_eq!(short.print().0, "abcde fghij\n");

        let long = "x".repeat(MAX_COLUMNS);
        let mut broken = Document::new(&long);
        broken.commands.extend([
            C::StartGroup,
            C::Text(0..MAX_COLUMNS),
            C::StartIndent,
            C::SoftLine(" "),
            C::Atom("tail"),
            C::EndIndent,
            C::EndGroup,
        ]);
        assert_eq!(broken.print().0, format!("{long}\n    tail\n"));
    }

    #[test]
    fn fit_work_is_bounded_by_width_per_group() {
        let source = "a".repeat(1_000);
        let mut document = Document::new(&source);
        for index in 0..1_000 {
            document.commands.push(C::StartGroup);
            document.commands.push(C::Text(index..index + 1));
        }
        for _ in 0..1_000 {
            document.commands.push(C::EndGroup);
        }
        let (_, stats) = document.print();
        assert_eq!(stats.preprocessing_visits, 3_000);
        assert_eq!(stats.printing_visits, 3_000);
        assert!(stats.fit_calls <= stats.groups, "{stats:?}");
        assert!(stats.fits_events <= 101 * stats.fit_calls, "{stats:?}");
        assert!(stats.fit_jumps <= 102 * stats.fit_calls, "{stats:?}");
    }

    #[test]
    fn jump_fits_match_a_command_by_command_reference() {
        let source = format!("{}tail# note  ", "x".repeat(MAX_COLUMNS));
        for commands in [
            vec![
                C::StartGroup,
                C::Text(0..MAX_COLUMNS),
                C::StartIndent,
                C::SoftLine(" "),
                C::Atom("tail"),
                C::EndIndent,
                C::EndGroup,
                C::Atom(","),
                C::LineSuffix {
                    span: MAX_COLUMNS + 4..source.len(),
                    leading_space: false,
                },
                C::SoftLine(" "),
                C::Atom("next"),
            ],
            vec![
                C::StartGroup,
                C::StartIndent,
                C::EndIndent,
                C::ModeText {
                    flat: "",
                    broken: "(",
                },
                C::Text(0..4),
                C::HardLine,
                C::ModeText {
                    flat: "",
                    broken: ")",
                },
                C::EndGroup,
            ],
        ] {
            let mut document = Document::new(&source);
            document.commands = commands;
            // Every group probe compares the jump-table result with the
            // command-by-command reference inside `Printer::fits`.
            document.print();
        }
    }
}
