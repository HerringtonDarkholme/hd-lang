//! The REPL (spec `cli.repl.*`, `f1c55020:future-work/compiler/repl-plan.md` job C9): `hd` with no
//! arguments on a terminal (`cli.repl.open.terminal`). It runs on wasmtime
//! only, so it exists in a build with the `wasmtime` feature; `main.rs`
//! says so in a build without it.
//!
//! Two threads share the work. The main thread reads inputs at the prompt
//! ([`Lines`]: rustyline on a terminal, a script in tests) and waits while
//! an input runs; Ctrl-C then interrupts the input through its session's
//! [`Stopper`]: an input that computes is stopped (`cli.repl.stop`), and
//! one that waits on the host is cancelled at once, running its `defer`
//! suites (`cli.repl.cancel`). The session picks which by the input's
//! state. The session thread, with the stack a
//! guest needs, owns the [`WasmtimeSession`]: it builds and runs the
//! session's prelude first, while the prompt already waits for the first
//! line, then builds each input as `repl#N` with `Goal::ReplInput` and runs
//! it. An input that ends with status 0 joins the session; one that fails
//! to build, panics or is stopped adds nothing (`cli.repl.panic.binding`,
//! `cli.repl.panic.declarations`).
//!
//! `:reload` reads the session's scope from disk again and rebuilds the
//! session over it from its journal (`cli.repl.rebuild`,
//! `WasmtimeSession::rebuild`): each input is built again from its text
//! and run again with its host calls served from the journal. The inputs
//! that still build and replay are kept; the REPL reports each stopped
//! input the rebuild skipped and the first input it dropped.
//!
//! Each input holds its wasmtime instance until the session ends. Once
//! more than `hd_run_wasmtime::REBUILD_AFTER` inputs ran since the session
//! started or last rebuilt, the REPL rebuilds it over the same files,
//! silently but for those reports (#228). The rebuild holds the inputs it
//! replays in one instance, which frees the memory of the others.

use hd_base::HashMap;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Instant;

use hd_cache::DiskStore;
use hd_driver::{BuildProfile, Goal, Host};
use hd_project::SourceSet;
use hd_run::{HostSetup, Outcome};
use hd_run_wasmtime::{EngineConfig, HostIo, Rebuilder, Stopper, WasmtimeEngine, WasmtimeSession};

use crate::{HD_FAILURE, Wall, disk, report};

mod complete;
mod highlight;

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
/// calls passed its cap (`docs/future-work/live-execution.md` §4.6): recording stopped, so
/// the session cannot be rebuilt (`cli.repl.rebuild`). The session itself
/// goes on.
const JOURNAL_FULL: &str =
    "note: the session's record of host calls is full, so this session can no longer be rebuilt";

/// How large a session may grow: its journal's cap in bytes, and how many
/// inputs run before it rebuilds to free their instances. Tests lower
/// them.
#[derive(Clone, Copy)]
pub(crate) struct Bounds {
    pub(crate) journal_cap: usize,
    pub(crate) rebuild_after: usize,
}

impl Default for Bounds {
    fn default() -> Bounds {
        Bounds {
            journal_cap: hd_run::journal::SESSION_CAP,
            rebuild_after: hd_run_wasmtime::REBUILD_AFTER,
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
struct Editor(rustyline::Editor<highlight::Helper, rustyline::history::DefaultHistory>);

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
    let on_start = |stopper: Stopper| {
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

/// The lines of a session script (`hd repl --script`): each input line is
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

/// `hd repl --script FILE` (`cli.repl.script`): a session over the lines of
/// `file`, in the package around the working directory or outside any
/// package, until the end of `file` or `:quit`.
pub(crate) fn script(file: &Path) -> ExitCode {
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
    match run(
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
pub(crate) fn run(
    lines: &mut dyn Lines,
    cwd: &Path,
    program: disk::Program,
    streams: Streams,
    bounds: Bounds,
    on_start: impl FnOnce(Stopper),
) -> Result<(), String> {
    let config = EngineConfig::pipeline(PIPELINE.level);
    let names = lines.names();
    let cwd = cwd.to_path_buf();
    let (send_input, inputs) = mpsc::channel::<Request>();
    let (send_done, done) = mpsc::channel::<()>();
    let (send_start, started) = mpsc::channel::<Result<Stopper, String>>();
    let thread = std::thread::Builder::new()
        .name("hd-repl".into())
        .stack_size(hd_run_wasmtime::thread_stack(&config))
        .spawn(move || {
            serve(
                config,
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

/// The session thread: starts the session and its prelude, then builds and
/// runs each input that arrives, answering on `done` after each.
fn serve(
    config: EngineConfig,
    cwd: PathBuf,
    program: disk::Program,
    bounds: Bounds,
    streams: Streams,
    names: complete::Names,
    started: &mpsc::Sender<Result<Stopper, String>>,
    inputs: &mpsc::Receiver<Request>,
    done: &mpsc::Sender<()>,
) {
    let Streams { io, mut diags } = streams;
    // Before the engine, so its inputs reuse earlier runs' functions.
    let _code = crate::engine::code_cache();
    let session = WasmtimeEngine::new(config).and_then(|engine| {
        // `cli.repl.host.default-profile`, `.args` (no program name or
        // arguments) and `cli.cap.source.flags-only` (bare `hd` has no
        // flags, so no grant narrows a trait); relative paths resolve from
        // the working directory (`cli.repl.host.cwd`).
        let setup = HostSetup {
            providers: Vec::new(),
            grants: hd_run::Grants::default(),
            limits: hd_run::Limits::default(),
            args: Vec::new(),
            program: String::new(),
        };
        engine.session(&setup, io).map_err(|e| match e {
            hd_run::StartError::Refused(m) | hd_run::StartError::Engine(m) => m,
        })
    });
    let session = match session {
        Ok(mut session) => {
            session.set_journal_cap(bounds.journal_cap);
            session.set_rebuild_after(bounds.rebuild_after);
            let _ = started.send(Ok(session.stopper()));
            session
        }
        Err(e) => {
            let _ = started.send(Err(e));
            return;
        }
    };
    let mut repl = Repl::new(session, cwd, program, names);
    if let Err(e) = repl.start() {
        let _ = writeln!(diags, "error: the session's prelude: {e}");
        return;
    }
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

/// A session: the wasmtime store, what its modules hold for the next
/// build, and the sources of the inputs that joined it.
struct Repl {
    session: WasmtimeSession,
    built: hd_driver::Session,
    /// Where the session started, which `:reload` loads its scope from.
    cwd: PathBuf,
    program: disk::Program,
    store: DiskStore,
    /// The number of the last input read (`repl#N`).
    next: u32,
    /// The text of each input that ran, the prelude as input 0, which a
    /// rebuild builds again.
    texts: BTreeMap<u32, String>,
    /// The names Tab completes: `src/lib.hd`'s and the kept inputs'.
    names: complete::Names,
}

/// The pipeline of every module a session builds (`crates/hd_opt/DESIGN.md` (optimization pipelines) §5): the
/// REPL runs on wasmtime only, unoptimized.
pub(crate) const PIPELINE: crate::Pipeline =
    crate::Pipeline::of(crate::engine::EngineKind::Wasmtime, false);

/// Builds the session's prelude, or reads it from the cache.
fn build_prelude(program: &disk::Program, store: &DiskStore) -> hd_driver::Output {
    let clock = Wall(Instant::now());
    let host = Host {
        render_tir: &[],
        sources: &program.sources,
        store,
        clock: &clock,
        executor: crate::executor(),
    };
    let out = hd_driver::build_repl_prelude(&host, BuildProfile::Checked, PIPELINE.target);
    crate::note_built(&out);
    out
}

/// Builds the input `number`, whose file is in `program`'s sources, over
/// the session `built`, and writes its diagnostics, named
/// `repl#N:LINE:COL`, to `diags`. With `from`, it builds the inputs from
/// `from` to `number` as one module (`hd_driver::Packages::inputs_from`).
/// `None` when it does not build.
fn build_input(
    program: &disk::Program,
    store: &DiskStore,
    built: &hd_driver::Session,
    from: Option<u32>,
    number: u32,
    diags: &mut dyn Write,
) -> Option<(Vec<u8>, hd_driver::Session)> {
    let clock = Wall(Instant::now());
    let host = Host {
        render_tir: &[],
        sources: &program.sources,
        store,
        clock: &clock,
        executor: crate::executor(),
    };
    let packages = hd_driver::Packages {
        build_profile: BuildProfile::Checked,
        target: PIPELINE.target,
        level: PIPELINE.level,
        session: Some(built),
        inputs_from: from,
        ..program.packages()
    };
    let goal = Goal::ReplInput { input: number };
    let out = hd_driver::build_packages(&host, &program.package, &packages, &goal);
    crate::note_built(&out);
    let file = hd_project::repl_file(number);
    let mut found = hd_driver::diagnostics::for_input(report::from_output(&out, None), &file);
    report::relocate(&mut found, |f| program.sources.display(f));
    for d in &found {
        let _ = writeln!(diags, "{}", d.text());
    }
    if found.iter().any(report::Diag::is_error) {
        return None;
    }
    let (Some(wasm), Some(added)) = (out.wasm, out.session_added) else {
        let _ = writeln!(diags, "error: {file}: no Wasm produced");
        return None;
    };
    Some((wasm, added))
}

impl Repl {
    fn new(
        session: WasmtimeSession,
        cwd: PathBuf,
        program: disk::Program,
        names: complete::Names,
    ) -> Repl {
        Repl {
            session,
            built: hd_driver::Session::start(),
            cwd,
            program,
            store: crate::disk_store(),
            next: 0,
            texts: BTreeMap::new(),
            names,
        }
    }

    /// Builds the session's prelude, or reads it from the cache, and runs
    /// it as input 0.
    fn start(&mut self) -> Result<(), String> {
        let out = build_prelude(&self.program, &self.store);
        let Some(wasm) = &out.wasm else {
            return Err(out.render());
        };
        self.texts.insert(0, String::new());
        match self.session.run_input(0, "", wasm)? {
            Outcome::Exit(0) => {}
            other => return Err(format!("it ended with {other:?}")),
        }
        self.built
            .add(out.session_added.ok_or("it adds nothing to the session")?);
        self.publish_package_names();
        Ok(())
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

    /// Builds `text` as the next input over the earlier ones and runs it.
    /// Its diagnostics, named `repl#N:LINE:COL`, go to `diags`; the guest's
    /// output and a panic report go to the session's streams.
    fn input(&mut self, text: &str, diags: &mut dyn Write) {
        self.next += 1;
        let file = hd_project::repl_file(self.next);
        self.program.sources.add_synthetic(&file, text);
        if !self.build_and_run(&file, text, diags) {
            self.program.sources.remove_synthetic(&file);
        }
        if self.session.wants_rebuild() && self.session.journal().is_some() {
            self.rebuild(diags, false);
        }
    }

    /// Whether the input `file`, of the text `text`, built and ended with
    /// status 0, so it joins the session.
    fn build_and_run(&mut self, file: &str, text: &str, diags: &mut dyn Write) -> bool {
        let Some((wasm, added)) = build_input(
            &self.program,
            &self.store,
            &self.built,
            None,
            self.next,
            diags,
        ) else {
            return false;
        };
        self.texts.insert(self.next, text.to_owned());
        let outcome = self.session.run_input(self.next, text, &wasm);
        if self.session.take_journal_notice() {
            let _ = writeln!(diags, "{JOURNAL_FULL}");
        }
        match outcome {
            Ok(Outcome::Exit(0)) => {
                self.built.add(added);
                self.names.add_input(self.next, text);
                true
            }
            // A panic, a stop or a cancel, reported by the session, or a
            // program's own exit status.
            Ok(Outcome::Exit(_)) => false,
            Ok(Outcome::Panic(p)) => {
                let _ = writeln!(diags, "panic: {}: {}", p.category, p.message);
                false
            }
            Ok(Outcome::Internal(e)) | Err(e) => {
                let _ = writeln!(diags, "error: {file}: {e}");
                false
            }
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
        if self.session.journal().is_none() {
            let _ = writeln!(
                diags,
                "error: the session's record of host calls is full, so it cannot be rebuilt"
            );
            return;
        }
        let program = match disk::load_session(&self.cwd) {
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
        self.program = program;
        self.rebuild(diags, true);
    }

    /// Rebuilds the session over `self.program` from its journal
    /// (`cli.repl.rebuild`), and reports each stopped input it skipped
    /// (`.stopped`) and the first input it dropped (`.prefix`). `summary`
    /// adds the line that counts the inputs it replayed, which `:reload`
    /// writes and a rebuild that frees memory does not.
    fn rebuild(&mut self, diags: &mut dyn Write, summary: bool) {
        // The rebuild adds each input's file again as it builds it.
        for n in self.texts.keys().filter(|n| **n != 0) {
            self.program
                .sources
                .remove_synthetic(&hd_project::repl_file(*n));
        }
        self.built = hd_driver::Session::start();
        let mut again = Again {
            program: &mut self.program,
            built: &mut self.built,
            store: &self.store,
            texts: &self.texts,
            added: HashMap::default(),
            diagnostics: Vec::new(),
        };
        let rebuilt = self.session.rebuild(&mut again);
        let diagnostics = std::mem::take(&mut again.diagnostics);
        let rebuilt = match rebuilt {
            Ok(rebuilt) => rebuilt,
            Err(e) => {
                let _ = writeln!(diags, "error: the rebuild: {e}");
                return;
            }
        };
        for n in &rebuilt.skipped {
            let _ = writeln!(
                diags,
                "note: {} was stopped, so the rebuild skipped it; its changes are not restored",
                hd_project::repl_file(*n)
            );
        }
        if let (Some(first), Some(reason)) = (rebuilt.dropped.first(), &rebuilt.reason) {
            let file = hd_project::repl_file(*first);
            if diagnostics.is_empty() {
                let _ = writeln!(diags, "error: {file} does not replay: {reason}");
            } else {
                let _ = diags.write_all(&diagnostics);
            }
            let later = rebuilt.dropped.len() - 1;
            let _ = writeln!(
                diags,
                "note: the rebuild dropped {file} and the {later} input{} after it",
                if later == 1 { "" } else { "s" }
            );
        }
        for n in rebuilt.skipped.iter().chain(&rebuilt.dropped) {
            self.texts.remove(n);
            self.names.drop_input(*n);
            self.program
                .sources
                .remove_synthetic(&hd_project::repl_file(*n));
        }
        self.publish_package_names();
        if !summary {
            return;
        }
        let kept = rebuilt.kept.iter().filter(|n| **n != 0).count();
        let _ = writeln!(
            diags,
            "reloaded: {kept} input{} replayed",
            if kept == 1 { "" } else { "s" }
        );
    }
}

/// A rebuild's builder: it builds each input again from its text, over
/// the package as it is on disk now; input 0 is the prelude.
struct Again<'r> {
    program: &'r mut disk::Program,
    built: &'r mut hd_driver::Session,
    store: &'r DiskStore,
    texts: &'r BTreeMap<u32, String>,
    /// What each input built adds to the session once it is replayed.
    added: HashMap<u32, hd_driver::Session>,
    /// The diagnostics of the input that no longer builds.
    diagnostics: Vec<u8>,
}

impl Rebuilder for Again<'_> {
    fn build(&mut self, number: u32) -> Result<(String, Vec<u8>), String> {
        let text = self
            .texts
            .get(&number)
            .ok_or_else(|| format!("the text of input {number} is gone"))?
            .clone();
        let (wasm, added) = if number == 0 {
            let out = build_prelude(self.program, self.store);
            let Some(wasm) = out.wasm else {
                return Err(out.render());
            };
            (wasm, out.session_added)
        } else {
            let file = hd_project::repl_file(number);
            self.program.sources.add_synthetic(&file, &text);
            let mut diagnostics = Vec::new();
            let built = build_input(
                self.program,
                self.store,
                self.built,
                None,
                number,
                &mut diagnostics,
            );
            let Some((wasm, added)) = built else {
                self.program.sources.remove_synthetic(&file);
                let reason = String::from_utf8_lossy(&diagnostics).trim_end().to_owned();
                self.diagnostics = diagnostics;
                return Err(reason);
            };
            (wasm, Some(added))
        };
        if let Some(added) = added {
            self.added.insert(number, added);
        }
        Ok((text, wasm))
    }

    fn build_joined(&mut self, numbers: &[u32]) -> Result<(Vec<String>, Vec<u8>), String> {
        let (Some(&first), Some(&last)) = (numbers.first(), numbers.last()) else {
            return Err("no inputs to build".into());
        };
        let mut texts = Vec::new();
        for n in numbers {
            let text = self
                .texts
                .get(n)
                .ok_or_else(|| format!("the text of input {n} is gone"))?;
            self.program
                .sources
                .add_synthetic(&hd_project::repl_file(*n), text);
            texts.push(text.clone());
        }
        // The rebuild that follows a failure builds each input alone and
        // reports the first that fails, so these diagnostics are dropped.
        let mut diagnostics = Vec::new();
        let built = build_input(
            self.program,
            self.store,
            self.built,
            Some(first),
            last,
            &mut diagnostics,
        );
        let Some((wasm, added)) = built else {
            return Err(String::from_utf8_lossy(&diagnostics).into_owned());
        };
        self.added.insert(last, added);
        Ok((texts, wasm))
    }

    fn replayed(&mut self, number: u32, joined: bool) {
        let added = self.added.remove(&number);
        match added {
            Some(added) if joined => self.built.add(added),
            _ if joined || number == 0 => {}
            _ => self
                .program
                .sources
                .remove_synthetic(&hd_project::repl_file(number)),
        }
    }

    fn restart(&mut self) {
        *self.built = hd_driver::Session::start();
        self.added.clear();
        self.diagnostics.clear();
        for n in self.texts.keys().filter(|n| **n != 0) {
            self.program
                .sources
                .remove_synthetic(&hd_project::repl_file(*n));
        }
    }
}

#[cfg(test)]
#[path = "repl_tests.rs"]
mod tests;
