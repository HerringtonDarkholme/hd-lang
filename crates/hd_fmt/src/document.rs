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
    SoftLine(&'static str),
    HardLine,
    BlankLine,
    LineSuffix(Range<usize>),
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
    next_break: Vec<usize>,
    flat_width: Vec<usize>,
    cannot_flatten: Vec<bool>,
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
        let len = commands.len();
        let mut next_flat = vec![len; len + 1];
        let mut next_break = vec![len; len + 1];
        let mut flat_width = vec![0; len];
        let mut cannot_flatten = vec![false; len];
        let mut following_flat = len;
        let mut following_break = len;
        for index in (0..len).rev() {
            let (width, newline) = match &commands[index] {
                Command::Text(span) => capped_width(&source[span.clone()]),
                Command::Atom(text) | Command::SoftLine(text) => capped_width(text),
                Command::ModeText { flat, .. } => capped_width(flat),
                _ => (0, false),
            };
            flat_width[index] = width;
            cannot_flatten[index] = newline;
            let hard_break = matches!(commands[index], Command::HardLine | Command::BlankLine);
            let soft_break = matches!(commands[index], Command::SoftLine(_));
            if width > 0 || newline || hard_break {
                following_flat = index;
            }
            if soft_break || hard_break {
                following_break = index;
            }
            next_flat[index] = following_flat;
            next_break[index] = following_break;
        }
        Self {
            source,
            commands,
            group_end,
            next_flat,
            next_break,
            flat_width,
            cannot_flatten,
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
                Command::SoftLine(flat) => {
                    if self.modes.last().copied().unwrap_or(false) {
                        self.write(flat);
                    } else {
                        self.newline(false);
                    }
                }
                Command::HardLine => self.newline(false),
                Command::BlankLine => self.newline(true),
                Command::LineSuffix(span) => {
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
        (self.output, self.stats)
    }

    /// Scan at most the width plus one positive-width events. Zero-width
    /// structure is skipped without recursion, so nesting cannot make a fit
    /// probe unbounded.
    fn fits(&mut self, mut index: usize, end: usize) -> bool {
        self.stats.fit_calls += 1;
        let column = if self.line_start {
            self.indent
        } else {
            self.column
        };
        let mut remaining = MAX_COLUMNS.saturating_sub(column);
        let mut positive = 0;
        while index < self.commands.len() {
            let continuation_break = self.next_break[index.max(end)];
            index = self.next_flat[index].min(continuation_break);
            self.stats.fit_jumps += 1;
            if index >= self.commands.len() {
                return true;
            }
            let continuation = index >= end;
            match &self.commands[index] {
                Command::Text(_) => {
                    if self.cannot_flatten[index] {
                        return false;
                    }
                    let width = self.flat_width[index];
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
                    let width = self.flat_width[index];
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
                    let width = self.flat_width[index];
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
                Command::LineSuffix(_) => {}
                Command::StartGroup
                | Command::EndGroup
                | Command::StartIndent
                | Command::EndIndent
                | Command::StartConditionalIndent
                | Command::EndConditionalIndent => {}
            }
            if positive > MAX_COLUMNS + 1 {
                return false;
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
            self.output.push_str(&" ".repeat(self.indent));
            self.column = self.indent;
            self.line_start = false;
        }
        self.output.push_str(text);
        if let Some(last) = text.rsplit_once('\n') {
            self.column = last.1.chars().count();
            self.line_start = text.ends_with('\n');
        } else {
            self.column += text.chars().count();
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
    for character in text.chars() {
        newline |= character == '\n';
        width = (width + 1).min(MAX_COLUMNS + 1);
    }
    (width, newline)
}

#[cfg(test)]
mod tests {
    use super::{Command as C, Document, MAX_COLUMNS, Printer};

    fn reference_print(source: &str, commands: Vec<C>) -> String {
        let mut printer = Printer::new(source, commands);
        for (index, next) in printer.next_flat.iter_mut().enumerate() {
            *next = index;
        }
        for (index, next) in printer.next_break.iter_mut().enumerate() {
            *next = index;
        }
        printer.print().0
    }

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
                C::LineSuffix(MAX_COLUMNS + 4..source.len()),
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
            document.commands = commands.clone();
            assert_eq!(document.print().0, reference_print(&source, commands));
        }
    }
}
