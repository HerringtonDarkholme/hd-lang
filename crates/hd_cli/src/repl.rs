//! The REPL (spec `cli.repl.*`, `f1c55020:future-work/compiler/repl-plan.md` job C9): `hd` with no
//! arguments on a terminal (`cli.repl.open.terminal`). It runs on wasmtime
//! for terminal input; scripted sessions also run on Node.
//!
//! Two threads share the work. The main thread reads inputs at the prompt
//! ([`Lines`]: rustyline on a terminal, a script in tests) and waits while
//! an input runs; Ctrl-C then interrupts the input through its session's
//! [`SessionStopper`]: an input that computes is stopped (`cli.repl.stop`), and
//! one that waits on the host is cancelled at once, running its `defer`
//! suites (`cli.repl.cancel`). The session picks which by the input's
//! state. The session thread, with the stack a guest needs, owns the
//! [`ReplSession`].
//!
//! Each input `N` builds as `repl#N` with `Goal::ReplInput`, in one module
//! with every input the session keeps (`hd_driver::Packages::inputs_from`),
//! on the linear backend. The session then runs that module: it replays
//! the kept inputs from its journal of host calls, so none is made again
//! (`cli.repl.host.once`) and nothing prints again, and runs the new input
//! live (`ReplSession::run`): every input is a rebuild by
//! `cli.repl.rebuild`. An input that ends with status 0 joins the session.
//! One that fails to build adds nothing, nor does one that was stopped or
//! cancelled, which the next rebuild skips and reports
//! (`cli.repl.rebuild.stopped`). One that panicked or ended with another
//! status stays in the session's program, so the next rebuild repeats its
//! changes to earlier values (`cli.repl.panic.mutations`), but no later
//! input sees what it declared (`cli.repl.panic.binding`,
//! `cli.repl.panic.declarations`; `hd_driver::Packages::failed_inputs`).
//!
//! `:reload` reads the session's scope from disk again and rebuilds the
//! session over it: the inputs that still build and replay are kept; the
//! REPL reports each stopped input the rebuild skipped and the first
//! input it dropped.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Instant;

use crate::engine::{EngineKind, HostIo, Ran, ReplSession, SessionStopper};
use hd_cache::DiskStore;
use hd_driver::{EngineTarget, Goal, Host, OptLevel};
use hd_project::SourceSet;
use hd_run::Outcome;
use hd_run::journal::End;

use crate::{HD_FAILURE, Wall, disk, report};

#[cfg(feature = "wasmtime")]
mod complete;
#[cfg(feature = "wasmtime")]
mod highlight;

// Scripted sessions do not offer completion in a build without an editor.
#[cfg(not(feature = "wasmtime"))]
mod complete {
    #[derive(Clone, Default)]
    pub(crate) struct Names {
        _private: (),
    }
    // Match the optional editor interface; scripts have no completion state.
    #[allow(clippy::unused_self)]
    impl Names {
        pub(crate) fn set_package(&self, _text: Option<&str>) {}
        pub(crate) fn add_input(&self, _number: u32, _text: &str) {}
        pub(crate) fn drop_input(&self, _number: u32) {}
    }
}

/// The prompt of an input's first line, and of each line that continues it.
const PROMPT: &str = "hd> ";
const CONTINUE: &str = "... ";

/// One indentation level, which a continuation line after a block header
/// starts with.
const INDENT: &str = "    ";

const HELP: &str = "\
Type a declaration, a statement or an expression; an expression shows its value and type.
An input continues while it is incomplete, after a `:` header or an open bracket;
an empty line ends it.
  :help         this text
  :reload       read the package's files again and rebuild the session over them
  :quit, :q     end the session (also Ctrl-D at an empty prompt)
Ctrl-C clears the line, or stops the input that runs.
";

/// Said once, after the input in which the session's journal of host
/// calls passed its cap (`docs/future-work/live-execution.md` §4.6):
/// recording stopped, and every input replays the journal, so the session
/// can run no further input.
const JOURNAL_FULL: &str =
    "note: the session's record of host calls is full, so this session can run no further input";

/// Why an input cannot run once the journal is full.
const NO_JOURNAL: &str =
    "error: the session's record of host calls is full, so it cannot run another input";

/// How large a session may grow: its journal's cap in bytes. Tests lower
/// it.
#[derive(Clone, Copy)]
pub(crate) struct Bounds {
    pub(crate) journal_cap: usize,
}

impl Default for Bounds {
    fn default() -> Bounds {
        Bounds {
            journal_cap: hd_run::journal::SESSION_CAP,
        }
    }
}

/// What the prompt hands the session thread.
enum Request {
    /// An input's text, its lines joined.
    Input(String),
    /// `:reload`: the package's files changed on disk.
    Reload,
}

/// What reading one line at a prompt gave.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Line {
    Text(String),
    /// Ctrl-C at the prompt: the input so far is dropped.
    #[cfg(feature = "wasmtime")]
    Interrupted,
    /// Ctrl-D at an empty prompt, or the end of a script.
    Eof,
}

/// Where the REPL reads its lines: the terminal's line editor, or in tests
/// a script.
pub(crate) trait Lines {
    /// Reads one line after `prompt`, with `initial` already typed: the
    /// indentation a continuation line starts with.
    fn read(&mut self, prompt: &str, initial: &str) -> Line;
    /// Remembers a line for the session's history (no history file).
    fn remember(&mut self, line: &str);
    /// The names that Tab completes, which the session keeps up to date.
    fn names(&self) -> complete::Names {
        complete::Names::default()
    }
}

/// The terminal's line editor, with the session's history in memory.
#[cfg(feature = "wasmtime")]
struct Editor(rustyline::Editor<highlight::Helper, rustyline::history::DefaultHistory>);

#[cfg(feature = "wasmtime")]
impl Lines for Editor {
    fn read(&mut self, prompt: &str, initial: &str) -> Line {
        match self.0.readline_with_initial(prompt, (initial, "")) {
            Ok(text) => Line::Text(text),
            Err(rustyline::error::ReadlineError::Interrupted) => Line::Interrupted,
            // Ctrl-D, or a terminal that went away.
            Err(_) => Line::Eof,
        }
    }

    fn remember(&mut self, line: &str) {
        let _ = self.0.add_history_entry(line);
    }

    fn names(&self) -> complete::Names {
        self.0
            .helper()
            .map(highlight::Helper::names)
            .unwrap_or_default()
    }
}

/// The streams of a session: the guest's standard streams, and where the
/// REPL writes diagnostics and its own messages.
pub(crate) struct Streams {
    pub(crate) io: HostIo,
    pub(crate) diags: Box<dyn Write + Send>,
}

/// `hd` on a terminal: a session in the package around the working
/// directory, or outside any package, until Ctrl-D or `:quit`.
#[cfg(feature = "wasmtime")]
pub(crate) fn command() -> ExitCode {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => return crate::fail(&format!("the working directory: {e}")),
    };
    let program = match disk::load_session(&cwd) {
        Ok(program) => program,
        Err(e) => return crate::fail(&e),
    };
    let mut rep = report::Reporter::stderr(false);
    if rep.diags(&program.problems) {
        return rep.finish(HD_FAILURE);
    }
    let config = rustyline::Config::builder().auto_add_history(false).build();
    let mut editor = match rustyline::Editor::with_config(config) {
        Ok(editor) => editor,
        Err(e) => return crate::fail(&format!("the terminal: {e}")),
    };
    editor.set_helper(Some(highlight::Helper::new(highlight::color_enabled())));
    let streams = Streams {
        io: HostIo::process(),
        diags: Box::new(std::io::stderr()),
    };
    // Ctrl-C reaches the process as SIGINT only while an input runs: at the
    // prompt the line editor reads it as a key. The handler stops or
    // cancels the input that runs and never ends the process; a stop while
    // none runs is dropped when the next input starts.
    let on_start = |stopper: SessionStopper| {
        if let Err(e) = ctrlc::set_handler(move || stopper.stop()) {
            eprintln!("warning: Ctrl-C cannot stop an input: {e}");
        }
    };
    let bounds = Bounds::default();
    match run(
        &mut Editor(editor),
        &cwd,
        program,
        streams,
        bounds,
        on_start,
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => crate::fail(&e),
    }
}

/// The lines of a session script (`hd --repl FILE`): each input line is
/// echoed after its prompt, as a terminal shows what a user typed, so the
/// standard output is the session's transcript (`cli.repl.script.echo`).
struct ScriptLines {
    lines: std::vec::IntoIter<String>,
}

impl Lines for ScriptLines {
    fn read(&mut self, prompt: &str, _initial: &str) -> Line {
        let Some(line) = self.lines.next() else {
            return Line::Eof;
        };
        let prompt = prompt.trim_end();
        let mut out = std::io::stdout().lock();
        let _ = if line.is_empty() {
            writeln!(out, "{prompt}")
        } else {
            writeln!(out, "{prompt} {line}")
        };
        let _ = out.flush();
        Line::Text(line)
    }

    fn remember(&mut self, _line: &str) {}
}

/// `hd --repl FILE` (`cli.repl.script`): a session over the lines of
/// `file`, in the package around the working directory or outside any
/// package, until the end of `file` or `:quit`.
pub(crate) fn script(file: &Path, kind: EngineKind) -> ExitCode {
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(e) => return crate::fail(&format!("{}: {e}", file.display())),
    };
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => return crate::fail(&format!("the working directory: {e}")),
    };
    let program = match disk::load_session(&cwd) {
        Ok(program) => program,
        Err(e) => return crate::fail(&e),
    };
    let mut rep = report::Reporter::stderr(false);
    if rep.diags(&program.problems) {
        return rep.finish(HD_FAILURE);
    }
    let streams = Streams {
        io: HostIo::process(),
        diags: Box::new(std::io::stderr()),
    };
    let mut lines = ScriptLines {
        lines: text
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>()
            .into_iter(),
    };
    match run_on(
        kind,
        &mut lines,
        &cwd,
        program,
        streams,
        Bounds::default(),
        |_| {},
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => crate::fail(&e),
    }
}

/// The session loop: reads inputs from `lines` until its end or `:quit`,
/// and runs each in a session over `program`, loaded from `cwd`, within
/// `bounds`. `:reload` loads the session's
/// scope from `cwd` again. `on_start` gets the session's stopper before the
/// first input. `Err` is a session that could not start or whose thread
/// ended.
#[cfg(feature = "wasmtime")]
pub(crate) fn run(
    lines: &mut dyn Lines,
    cwd: &Path,
    program: disk::Program,
    streams: Streams,
    bounds: Bounds,
    on_start: impl FnOnce(SessionStopper),
) -> Result<(), String> {
    run_on(
        EngineKind::Wasmtime,
        lines,
        cwd,
        program,
        streams,
        bounds,
        on_start,
    )
}

fn run_on(
    kind: EngineKind,
    lines: &mut dyn Lines,
    cwd: &Path,
    program: disk::Program,
    streams: Streams,
    bounds: Bounds,
    on_start: impl FnOnce(SessionStopper),
) -> Result<(), String> {
    let names = lines.names();
    let cwd = cwd.to_path_buf();
    let (send_input, inputs) = mpsc::channel::<Request>();
    let (send_done, done) = mpsc::channel::<()>();
    let (send_start, started) = mpsc::channel::<Result<SessionStopper, String>>();
    let thread = std::thread::Builder::new()
        .name("hd-repl".into())
        .stack_size(kind.worker_stack(OptLevel::Dev).unwrap_or(8 * 1024 * 1024))
        .spawn(move || {
            serve(
                kind,
                cwd,
                program,
                bounds,
                streams,
                names,
                &send_start,
                &inputs,
                &send_done,
            );
        })
        .map_err(|e| format!("the session thread: {e}"))?;
    let stopper = started
        .recv()
        .map_err(|_| "the session thread ended".to_owned())??;
    on_start(stopper);
    let mut result = Ok(());
    while let Some(request) = read_input(lines) {
        if send_input.send(request).is_err() || done.recv().is_err() {
            result = Err("the session ended".to_owned());
            break;
        }
        // The input's output that ends without a line ending shows before
        // the next prompt.
        let _ = std::io::stdout().flush();
    }
    drop(send_input);
    let _ = thread.join();
    result
}

/// Reads the next input to run, its lines joined, or `:reload`. An empty
/// input and the other commands are handled here. `None` ends the session.
fn read_input(lines: &mut dyn Lines) -> Option<Request> {
    loop {
        let first = match lines.read(PROMPT, "") {
            Line::Text(text) => text,
            #[cfg(feature = "wasmtime")]
            Line::Interrupted => continue,
            Line::Eof => return None,
        };
        if first.trim().is_empty() {
            continue;
        }
        lines.remember(&first);
        match first.trim() {
            ":quit" | ":q" | ":exit" => return None,
            ":help" => {
                print!("{HELP}");
                continue;
            }
            ":reload" => return Some(Request::Reload),
            _ => {}
        }
        let mut text = first;
        text.push('\n');
        // Whether the input ended, or Ctrl-C dropped it.
        let ended = loop {
            if is_complete(&text) {
                break true;
            }
            let initial = continuation_indent(&text);
            match lines.read(CONTINUE, &initial) {
                Line::Text(line) if line.trim().is_empty() => break true,
                Line::Text(line) => {
                    lines.remember(&line);
                    text.push_str(&line);
                    text.push('\n');
                }
                #[cfg(feature = "wasmtime")]
                Line::Interrupted => break false,
                Line::Eof => return None,
            }
        };
        if ended {
            return Some(Request::Input(text));
        }
    }
}

/// Whether `text`, an input's lines so far, is a whole input: the parser
/// does not stop at its end inside a bracket, a `"""` string or a
/// statement (a block header waiting for its block, `1 +`), and its last
/// line is not inside an indented block, which an empty line ends.
fn is_complete(text: &str) -> bool {
    let end = text.trim_end().len();
    let parse = hd_syntax::parse(text.as_bytes());
    let stops_inside = parse.diagnostics.iter().any(|d| {
        let lo = d.primary.lo as usize;
        match d.code {
            hd_diag::Code::UnclosedDelimiter => true,
            hd_diag::Code::UnterminatedString => {
                let literal = text.get(lo..).unwrap_or_default();
                literal
                    .trim_start_matches(|c: char| c.is_ascii_alphabetic())
                    .starts_with("\"\"\"")
            }
            _ => d.severity == hd_diag::Severity::Error && lo >= end,
        }
    });
    !stops_inside && !ends_in_block(text, &parse.tokens)
}

/// Whether the last line of `text` is a line of an indented block: the
/// logical line it belongs to starts indented, outside every bracket and
/// string. Such an input may go on with more lines of the block, so an
/// empty line ends it.
fn ends_in_block(text: &str, tokens: &hd_syntax::TokenBuf) -> bool {
    let body = text.trim_end_matches('\n');
    let last = body.rfind('\n').map_or(0, |i| i + 1);
    // The bracket depth at `offset`, from the tokens before it.
    let depth_at = |offset: usize| -> i64 {
        tokens
            .kind
            .iter()
            .zip(&tokens.start)
            .take_while(|(_, at)| (**at as usize) < offset)
            .map(|(kind, _)| i64::from(kind.opens()) - i64::from(kind.closes()))
            .sum()
    };
    let starts_logical = |line: usize| {
        let at = tokens.line_start[line] as usize;
        tokens.line_flags[line].0 & hd_syntax::LineFlags::CONTINUES_STRING == 0 && depth_at(at) <= 0
    };
    let Some(last_line) = tokens.line_start.iter().rposition(|&s| s as usize <= last) else {
        return false;
    };
    let first = (0..=last_line)
        .rev()
        .find(|&l| starts_logical(l))
        .unwrap_or(0);
    let at = tokens.line_start[first] as usize;
    body.get(at..)
        .is_some_and(|line| line.starts_with([' ', '\t']))
}

/// The indentation a continuation line starts with: the last line's, one
/// level deeper after a line that ends in `:`.
fn continuation_indent(text: &str) -> String {
    let last = text
        .trim_end_matches('\n')
        .rsplit('\n')
        .next()
        .unwrap_or("");
    let body = last.trim_start();
    let mut indent = last[..last.len() - body.len()].to_owned();
    if body.trim_end().ends_with(':') {
        indent.push_str(INDENT);
    }
    indent
}

/// The session thread: starts the session, then builds and runs each
/// input that arrives, answering on `done` after each.
fn serve(
    kind: EngineKind,
    cwd: PathBuf,
    program: disk::Program,
    bounds: Bounds,
    streams: Streams,
    names: complete::Names,
    started: &mpsc::Sender<Result<SessionStopper, String>>,
    inputs: &mpsc::Receiver<Request>,
    done: &mpsc::Sender<()>,
) {
    let Streams { io, mut diags } = streams;
    // Before the engine, so its inputs reuse earlier runs' functions.
    #[cfg(feature = "wasmtime")]
    let _code = crate::engine::code_cache();
    let session = ReplSession::new(kind, io, &cwd);
    let session = match session {
        Ok(mut session) => {
            session.set_journal_cap(bounds.journal_cap);
            let _ = started.send(Ok(session.stopper()));
            session
        }
        Err(e) => {
            let _ = started.send(Err(e));
            return;
        }
    };
    let mut repl = Repl::new(session, cwd, program, names);
    for request in inputs {
        match request {
            Request::Input(text) => repl.input(&text, &mut *diags),
            Request::Reload => repl.reload(&mut *diags),
        }
        let _ = diags.flush();
        if done.send(()).is_err() {
            return;
        }
    }
}

/// An input the session keeps: its text, and whether it ended with status
/// 0, so later inputs see what it declares.
struct Kept {
    text: String,
    joined: bool,
}

/// The text of each kept input, by number.
fn sources(inputs: &BTreeMap<u32, Kept>) -> Vec<(u32, &str)> {
    inputs
        .iter()
        .map(|(n, kept)| (*n, kept.text.as_str()))
        .collect()
}

/// A session: the engine's session, which holds the journal, and the
/// sources of the inputs it keeps.
struct Repl {
    session: ReplSession,
    /// Where the session started, which `:reload` loads its scope from.
    cwd: PathBuf,
    program: disk::Program,
    store: DiskStore,
    /// The number of the last input read (`repl#N`).
    next: u32,
    /// The inputs the session keeps, by number; each one's file is among
    /// `program`'s sources.
    inputs: BTreeMap<u32, Kept>,
    /// The names Tab completes: `src/lib.hd`'s and the joined inputs'.
    names: complete::Names,
}

/// The pipeline of every module a session builds (`crates/hd_opt/DESIGN.md` (optimization pipelines) §5): the
/// REPL runs unoptimized, on the default backend.
#[cfg(all(test, feature = "wasmtime"))]
pub(crate) const PIPELINE: crate::Pipeline =
    crate::Pipeline::of(crate::engine::EngineKind::Wasmtime, false);

/// Builds the session program: every input among `program`'s sources up
/// to `number` as one module, the `failed` ones hidden from later inputs
/// (`hd_driver::Packages::failed_inputs`). The diagnostics, named
/// `repl#N:LINE:COL`, are those of input `number` and every error.
fn build_session(
    program: &disk::Program,
    store: &DiskStore,
    failed: &[u32],
    number: u32,
    target: EngineTarget,
) -> (Option<Vec<u8>>, Vec<report::Diag>) {
    let clock = Wall(Instant::now());
    let host = Host {
        render_tir: &[],
        sources: &program.sources,
        store,
        clock: &clock,
        executor: crate::executor(),
    };
    let start = hd_driver::Session::start();
    let packages = hd_driver::Packages {
        target,
        level: OptLevel::Dev,
        session: Some(&start),
        inputs_from: Some(1),
        failed_inputs: failed,
        ..program.packages()
    };
    let goal = Goal::ReplInput { input: number };
    let out = hd_driver::build_packages(&host, &program.package, &packages, &goal);
    crate::note_built(&out);
    let file = hd_project::repl_file(number);
    let mut found = hd_driver::diagnostics::for_input(report::from_output(&out, None), &file);
    report::relocate(&mut found, |f| program.sources.display(f));
    if found.iter().any(report::Diag::is_error) {
        return (None, found);
    }
    if out.wasm.is_none() {
        found.push(report::Diag::error(None, "no Wasm produced").at(&file, None));
    }
    (out.wasm, found)
}

impl Repl {
    fn new(
        session: impl Into<ReplSession>,
        cwd: PathBuf,
        program: disk::Program,
        names: complete::Names,
    ) -> Repl {
        let repl = Repl {
            session: session.into(),
            cwd,
            program,
            store: crate::disk_store(),
            next: 0,
            inputs: BTreeMap::new(),
            names,
        };
        repl.publish_package_names();
        repl
    }

    /// Reads `src/lib.hd`'s top-level names for Tab, when a package is open.
    fn publish_package_names(&self) {
        let lib = self
            .program
            .package_dir
            .as_ref()
            .and_then(|_| self.program.sources.read("src/lib.hd"));
        let text = lib.as_deref().map(String::from_utf8_lossy);
        self.names.set_package(text.as_deref());
    }

    /// The kept inputs that did not end with status 0.
    fn failed(&self) -> Vec<u32> {
        self.inputs
            .iter()
            .filter(|(_, kept)| !kept.joined)
            .map(|(n, _)| *n)
            .collect()
    }

    /// Forgets input `n`: its text, its file and its names.
    fn forget(&mut self, n: u32) {
        self.inputs.remove(&n);
        self.names.drop_input(n);
        self.program
            .sources
            .remove_synthetic(&hd_project::repl_file(n));
    }

    /// Builds `text` as the next input with the kept ones and runs it.
    /// Its diagnostics, named `repl#N:LINE:COL`, go to `diags`; the guest's
    /// output and a panic report go to the session's streams.
    fn input(&mut self, text: &str, diags: &mut dyn Write) {
        self.next += 1;
        let number = self.next;
        let file = hd_project::repl_file(number);
        if !self.session.recording() {
            let _ = writeln!(diags, "{NO_JOURNAL}");
            return;
        }
        self.program.sources.add_synthetic(&file, text);
        let target = self.session.kind().target();
        let (wasm, found) =
            build_session(&self.program, &self.store, &self.failed(), number, target);
        for d in &found {
            let _ = writeln!(diags, "{}", d.text());
        }
        let Some(wasm) = wasm else {
            self.program.sources.remove_synthetic(&file);
            return;
        };
        let mut sources = sources(&self.inputs);
        sources.push((number, text));
        let ran = self.session.run(&wasm, &sources, Some(number));
        if self.session.take_journal_notice() {
            let _ = writeln!(diags, "{JOURNAL_FULL}");
        }
        let ran = match ran {
            Ok(ran) => ran,
            Err(e) => {
                let _ = writeln!(diags, "error: {file}: {e}");
                self.program.sources.remove_synthetic(&file);
                return;
            }
        };
        self.report(&ran, &[], diags);
        let Some((outcome, end)) = ran.live else {
            self.program.sources.remove_synthetic(&file);
            return;
        };
        match outcome {
            // A panic, a stop or a cancel, reported by the session, or a
            // program's own exit status.
            Outcome::Exit(_) => {}
            Outcome::Panic(p) => {
                let _ = writeln!(diags, "panic: {}: {}", p.category, p.message);
            }
            Outcome::Internal(e) => {
                let _ = writeln!(diags, "error: {file}: {e}");
            }
        }
        match end {
            // A stopped input ended partway, so no rebuild repeats it
            // (`cli.repl.rebuild.stopped`): the next input's skips it.
            End::Stopped | End::Cancelled => {
                let how = if end == End::Stopped {
                    "stopped"
                } else {
                    "cancelled"
                };
                let _ = writeln!(
                    diags,
                    "note: {file} was {how}, so the next rebuild skips it; its changes are not restored"
                );
                self.program.sources.remove_synthetic(&file);
            }
            end => {
                let joined = end == End::Exit(0);
                if joined {
                    self.names.add_input(number, text);
                }
                let text = text.to_owned();
                self.inputs.insert(number, Kept { text, joined });
            }
        }
    }

    /// Reports what a run of the session's program dropped
    /// (`cli.repl.rebuild.prefix`), with `diagnostics`, those of the first
    /// dropped input when it no longer builds, and forgets those inputs.
    /// The inputs it skipped were reported when they were stopped
    /// (`cli.repl.rebuild.stopped`).
    fn report(&mut self, ran: &Ran, diagnostics: &[report::Diag], diags: &mut dyn Write) {
        // Inputs this session forgot already, which a rebuild left out,
        // are dropped from the journal now, unreported.
        let fresh = ran
            .dropped
            .first()
            .is_some_and(|n| self.inputs.contains_key(n));
        if let (true, Some(first), Some(reason)) = (fresh, ran.dropped.first(), &ran.reason) {
            let file = hd_project::repl_file(*first);
            if diagnostics.is_empty() {
                let _ = writeln!(diags, "error: {file} does not replay: {reason}");
            } else {
                for d in diagnostics {
                    let _ = writeln!(diags, "{}", d.text());
                }
            }
            let later = ran.dropped.len() - 1;
            let _ = writeln!(
                diags,
                "note: the rebuild dropped {file} and the {later} input{} after it",
                if later == 1 { "" } else { "s" }
            );
        }
        for n in ran.skipped.iter().chain(&ran.dropped) {
            self.forget(*n);
        }
    }
}

impl Repl {
    /// `:reload`: loads the session's scope from disk again, so edited
    /// package files are read anew, and rebuilds the session over it from
    /// its journal (`cli.repl.rebuild`). It reports each stopped input the
    /// rebuild skipped (`.stopped`) and the first input it dropped
    /// (`.prefix`). A session whose journal is full, or a package whose
    /// manifest is broken, is left as it was.
    fn reload(&mut self, diags: &mut dyn Write) {
        if !self.session.recording() {
            let _ = writeln!(
                diags,
                "error: the session's record of host calls is full, so it cannot be rebuilt"
            );
            return;
        }
        let mut program = match disk::load_session(&self.cwd) {
            Ok(program) => program,
            Err(e) => {
                let _ = writeln!(diags, "error: {e}");
                return;
            }
        };
        for d in &program.problems {
            let _ = writeln!(diags, "{}", d.text());
        }
        if program.problems.iter().any(report::Diag::is_error) {
            return;
        }
        for (n, kept) in &self.inputs {
            program
                .sources
                .add_synthetic(&hd_project::repl_file(*n), &kept.text);
        }
        self.program = program;
        self.publish_package_names();
        self.rebuild(diags);
    }

    /// Rebuilds the session over `self.program` from its journal
    /// (`cli.repl.rebuild`): builds the kept inputs, leaving out the first
    /// one that no longer builds and every later one (`.prefix`), then
    /// runs the program with no new input. It reports what it skipped and
    /// dropped, and counts the inputs it replayed.
    fn rebuild(&mut self, diags: &mut dyn Write) {
        let target = self.session.kind().target();
        // The first input that no longer builds, and its diagnostics.
        let mut cut: Option<(u32, Vec<report::Diag>)> = None;
        let before = |cut: &Option<(u32, Vec<report::Diag>)>, n: u32| {
            cut.as_ref().is_none_or(|(first, _)| n < *first)
        };
        let wasm = loop {
            let Some(last) = self.inputs.keys().copied().rev().find(|n| before(&cut, *n)) else {
                break None;
            };
            let failed: Vec<u32> = self
                .failed()
                .into_iter()
                .filter(|n| before(&cut, *n))
                .collect();
            let (wasm, found) = build_session(&self.program, &self.store, &failed, last, target);
            if let Some(wasm) = wasm {
                break Some(wasm);
            }
            // The first input with an error no longer builds; the inputs
            // before it do not see the ones after it.
            let in_input = |d: &report::Diag, n: u32| {
                let file = hd_project::repl_file(n);
                d.file.as_deref().is_some_and(|f| f.starts_with(&file))
            };
            let first = self
                .inputs
                .keys()
                .copied()
                .filter(|n| before(&cut, *n))
                .find(|n| found.iter().any(|d| d.is_error() && in_input(d, *n)))
                .unwrap_or(last);
            for n in self.inputs.keys().filter(|n| **n >= first) {
                self.program
                    .sources
                    .remove_synthetic(&hd_project::repl_file(*n));
            }
            let shown = found.into_iter().filter(|d| in_input(d, first)).collect();
            cut = Some((first, shown));
        };
        let mut ran = Ran {
            kept: Vec::new(),
            skipped: Vec::new(),
            dropped: Vec::new(),
            reason: None,
            live: None,
        };
        if let Some(wasm) = wasm {
            let sources: Vec<(u32, &str)> = sources(&self.inputs)
                .into_iter()
                .filter(|(n, _)| before(&cut, *n))
                .collect();
            match self.session.run(&wasm, &sources, None) {
                Ok(done) => ran = done,
                Err(e) => {
                    let _ = writeln!(diags, "error: the rebuild: {e}");
                    return;
                }
            }
        }
        let mut shown = Vec::new();
        if let Some((first, diagnostics)) = cut
            && ran.dropped.first().is_none_or(|d| *d >= first)
        {
            ran.dropped = self
                .inputs
                .keys()
                .copied()
                .filter(|n| *n >= first)
                .collect();
            ran.reason = Some("it no longer builds".into());
            shown = diagnostics;
        }
        self.report(&ran, &shown, diags);
        let kept = ran.kept.len();
        let _ = writeln!(
            diags,
            "reloaded: {kept} input{} replayed",
            if kept == 1 { "" } else { "s" }
        );
    }
}

#[cfg(all(test, feature = "wasmtime"))]
#[path = "repl_tests.rs"]
mod tests;
