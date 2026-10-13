//! The REPL (spec `cli.repl.*`, repl-plan.md job C9): `hd` with no
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

use std::io::Write;
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Instant;

use hd_cache::DiskStore;
use hd_driver::{BuildProfile, Goal, Host};
use hd_run::{HostSetup, Outcome};
use hd_run_wasmtime::{EngineConfig, HostIo, Stopper, WasmtimeEngine, WasmtimeSession};

use crate::{HD_FAILURE, Wall, disk, report};

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
  :quit, :q     end the session (also Ctrl-D at an empty prompt)
Ctrl-C clears the line, or stops the input that runs.
";

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
}

/// The terminal's line editor, with the session's history in memory.
struct Editor(rustyline::DefaultEditor);

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
    let editor = match rustyline::DefaultEditor::with_config(config) {
        Ok(editor) => editor,
        Err(e) => return crate::fail(&format!("the terminal: {e}")),
    };
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
    match run(&mut Editor(editor), program, streams, on_start) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => crate::fail(&e),
    }
}

/// The session loop: reads inputs from `lines` until its end or `:quit`,
/// and runs each in a session over `program`. `on_start` gets the
/// session's stopper before the first input. `Err` is a session that could
/// not start or whose thread ended.
pub(crate) fn run(
    lines: &mut dyn Lines,
    program: disk::Program,
    streams: Streams,
    on_start: impl FnOnce(Stopper),
) -> Result<(), String> {
    let config = EngineConfig::pipeline(false);
    let (send_input, inputs) = mpsc::channel::<String>();
    let (send_done, done) = mpsc::channel::<()>();
    let (send_start, started) = mpsc::channel::<Result<Stopper, String>>();
    let thread = std::thread::Builder::new()
        .name("hd-repl".into())
        .stack_size(hd_run_wasmtime::thread_stack(&config))
        .spawn(move || serve(config, program, streams, &send_start, &inputs, &send_done))
        .map_err(|e| format!("the session thread: {e}"))?;
    let stopper = started
        .recv()
        .map_err(|_| "the session thread ended".to_owned())??;
    on_start(stopper);
    let mut result = Ok(());
    while let Some(text) = read_input(lines) {
        if send_input.send(text).is_err() || done.recv().is_err() {
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

/// Reads the next input to run: its lines, joined. An empty input and a
/// command are handled here. `None` ends the session.
fn read_input(lines: &mut dyn Lines) -> Option<String> {
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
            return Some(text);
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
    program: disk::Program,
    streams: Streams,
    started: &mpsc::Sender<Result<Stopper, String>>,
    inputs: &mpsc::Receiver<String>,
    done: &mpsc::Sender<()>,
) {
    let Streams { io, mut diags } = streams;
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
        Ok(session) => {
            let _ = started.send(Ok(session.stopper()));
            session
        }
        Err(e) => {
            let _ = started.send(Err(e));
            return;
        }
    };
    let mut repl = Repl::new(session, program);
    if let Err(e) = repl.start() {
        let _ = writeln!(diags, "error: the session's prelude: {e}");
        return;
    }
    for text in inputs {
        repl.input(&text, &mut *diags);
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
    program: disk::Program,
    store: DiskStore,
    /// The number of the last input read (`repl#N`).
    next: u32,
}

impl Repl {
    fn new(session: WasmtimeSession, program: disk::Program) -> Repl {
        Repl {
            session,
            built: hd_driver::Session::start(),
            program,
            store: DiskStore {
                root: crate::cache_dir(),
            },
            next: 0,
        }
    }

    /// Builds the session's prelude, or reads it from the cache, and runs
    /// it as input 0.
    fn start(&mut self) -> Result<(), String> {
        let clock = Wall(Instant::now());
        let host = Host {
            render_tir: &[],
            sources: &self.program.sources,
            store: &self.store,
            clock: &clock,
            executor: crate::executor(),
        };
        let out = hd_driver::build_repl_prelude(&host, BuildProfile::Checked);
        let Some(wasm) = &out.wasm else {
            return Err(out.render());
        };
        match self.session.run_input(wasm)? {
            Outcome::Exit(0) => {}
            other => return Err(format!("it ended with {other:?}")),
        }
        self.built
            .add(out.session_added.ok_or("it adds nothing to the session")?);
        Ok(())
    }

    /// Builds `text` as the next input over the earlier ones and runs it.
    /// Its diagnostics, named `repl#N:LINE:COL`, go to `diags`; the guest's
    /// output and a panic report go to the session's streams.
    fn input(&mut self, text: &str, diags: &mut dyn Write) {
        self.next += 1;
        let file = hd_project::repl_file(self.next);
        self.program.sources.add_synthetic(&file, text);
        if !self.build_and_run(&file, diags) {
            self.program.sources.remove_synthetic(&file);
        }
    }

    /// Whether the input `file` built and ended with status 0, so it joins
    /// the session.
    fn build_and_run(&mut self, file: &str, diags: &mut dyn Write) -> bool {
        let clock = Wall(Instant::now());
        let host = Host {
            render_tir: &[],
            sources: &self.program.sources,
            store: &self.store,
            clock: &clock,
            executor: crate::executor(),
        };
        let packages = hd_driver::Packages {
            build_profile: BuildProfile::Checked,
            session: Some(&self.built),
            ..self.program.packages()
        };
        let goal = Goal::ReplInput { input: self.next };
        let out = hd_driver::build_packages(&host, &self.program.package, &packages, &goal);
        let mut found = report::from_output(&out, None);
        report::relocate(&mut found, |f| self.program.sources.display(f));
        for d in &found {
            let _ = writeln!(diags, "{}", d.text());
        }
        if found.iter().any(report::Diag::is_error) {
            return false;
        }
        let (Some(wasm), Some(added)) = (out.wasm, out.session_added) else {
            let _ = writeln!(diags, "error: {file}: no Wasm produced");
            return false;
        };
        match self.session.run_input(&wasm) {
            Ok(Outcome::Exit(0)) => {
                self.built.add(added);
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

#[cfg(test)]
#[path = "repl_tests.rs"]
mod tests;
