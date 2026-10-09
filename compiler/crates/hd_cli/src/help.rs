//! `hd help`, `hd --help`, `hd help COMMAND` and `hd COMMAND --help`
//! (`cli.command.help`, `cli.command.help.command`): the command list of
//! the Commands table, and each command's usage line and flags.

use std::fmt::Write as _;

/// One command's help.
struct Command {
    name: &'static str,
    /// Its line in the command list.
    summary: &'static str,
    usage: &'static str,
    about: &'static [&'static str],
    flags: &'static [(&'static str, &'static str)],
}

const RELEASE: (&str, &str) = (
    "--release",
    "build for release: integer overflow wraps instead of panicking",
);
const PACKAGE: (&str, &str) = (
    "-p, --package NAME",
    "act on the workspace member NAME only; may be repeated",
);
const CAP: (&str, &str) = (
    "--cap NAME=VALUE",
    "grant the capability NAME: true, false, or a comma-separated list, as in Http=api.example.com",
);
const MAX_HEAP: (&str, &str) = (
    "--max-heap SIZE",
    "limit the program's heap, as 256M; past it the program panics",
);
const TIME_LIMIT: (&str, &str) = (
    "--time-limit DURATION",
    "limit how long the program runs, as 30s; past it the program panics",
);
const FORMAT: (&str, &str) = (
    "--format FORMAT",
    "print diagnostics as text (the default) or as JSON lines",
);
const JOBS: (&str, &str) = (
    "--jobs N",
    "use N threads; the default is HD_JOBS, else the cores, at most 8",
);
const MAX_MEMORY: (&str, &str) = (
    "--max-memory SIZE",
    "cap the memory of hd itself, as 4G; the default is HD_MAX_MEMORY, else none",
);

/// The commands of the Commands table, in its order.
const COMMANDS: &[Command] = &[
    Command {
        name: "FILE",
        summary: "run FILE as a single-file program, which may use only std",
        usage: "hd [--release] [--cap NAME=VALUE] [--max-heap SIZE] [--time-limit DURATION] FILE [-- ARGS]",
        about: &[
            "Run FILE as a single-file program. Outside a package it may use only std;",
            "words after -- are the program's arguments.",
        ],
        flags: &[RELEASE, CAP, MAX_HEAP, TIME_LIMIT, FORMAT, JOBS, MAX_MEMORY],
    },
    Command {
        name: "FILE.wasm",
        summary: "run a module that hd build wrote",
        usage: "hd [--cap NAME=VALUE] [--max-heap SIZE] [--time-limit DURATION] FILE.wasm [-- ARGS]",
        about: &["Run a module that hd build wrote. Its grant comes from --cap flags alone."],
        flags: &[CAP, MAX_HEAP, TIME_LIMIT],
    },
    Command {
        name: "build",
        summary: "build the package's executables, or the module of one FILE in the package",
        usage: "hd build [--release] [-p NAME] [FILE]",
        about: &[
            "Build each executable NAME of the package to build/debug/NAME.wasm, or to",
            "build/release/ with --release; with FILE, its module to build/debug/files/STEM.wasm.",
            "A package with no executable is checked, and no .wasm file is written.",
        ],
        flags: &[RELEASE, PACKAGE, FORMAT, JOBS, MAX_MEMORY],
    },
    Command {
        name: "run",
        summary: "run the package's executable, or the executable or task named NAME",
        usage: "hd run [--release] [-p NAME] [--cap NAME=VALUE] [NAME] [-- ARGS]",
        about: &[
            "Run the package's executable, or the executable or task named NAME.",
            "",
            "The package is the one whose hd.toml is nearest above the current directory; at a workspace root, every member.",
            "Without NAME, the package must have exactly one executable: src/main.hd, or",
            "one [[executable]] table of hd.toml. Words after -- are the program's arguments.",
        ],
        flags: &[
            RELEASE, PACKAGE, CAP, MAX_HEAP, TIME_LIMIT, FORMAT, JOBS, MAX_MEMORY,
        ],
    },
    Command {
        name: "test",
        summary: "run the test cases of the package, or of FILE",
        usage: "hd test [--release] [-p NAME] [--filter PATTERN] [--seed N] [--affected] [--update] [FILE]",
        about: &[
            "Run the test cases of the package, or of FILE's module. Test builds are",
            "always checked; --release only selects the optimizing pipeline.",
        ],
        flags: &[
            RELEASE,
            PACKAGE,
            (
                "--filter PATTERN",
                "run only the cases whose name contains PATTERN",
            ),
            ("--seed N", "give every property test the base seed N"),
            (
                "--affected",
                "run only the test programs whose fingerprint changed since they last passed",
            ),
            ("--update", "rewrite each failing snapshot's expected text"),
            CAP,
            MAX_HEAP,
            TIME_LIMIT,
            FORMAT,
            JOBS,
            MAX_MEMORY,
        ],
    },
    Command {
        name: "check",
        summary: "type-check the package, or FILE, without running it",
        usage: "hd check [-p NAME] [--tests | --all] [--max-errors N] [--summary] [FILE]",
        about: &[
            "Check the package's library and executables, or FILE's module and the modules",
            "it uses, without building or running anything.",
        ],
        flags: &[
            PACKAGE,
            (
                "--tests",
                "also check the test code: tests: blocks, test modules, integration tests",
            ),
            ("--all", "also check the test code and the tasks"),
            (
                "--max-errors N",
                "stop printing diagnostics after the Nth error",
            ),
            (
                "--summary",
                "print one line per severity, file and code instead of each diagnostic",
            ),
            FORMAT,
            JOBS,
            MAX_MEMORY,
        ],
    },
    Command {
        name: "doc",
        summary: "write the package's documentation, or print one item's",
        usage: "hd doc [--private] [--out DIR] [--open] [NAME]",
        about: &[
            "Write the package's documentation as HTML and Markdown under build/doc, or",
            "with NAME print one item's Markdown and write no file.",
        ],
        flags: &[
            ("--private", "also document the private items and members"),
            ("--out DIR", "write the documentation to DIR"),
            ("--open", "open the entry page after writing it"),
            FORMAT,
        ],
    },
    Command {
        name: "new",
        summary: "create a package in PATH, or in the current directory",
        usage: "hd new [--app | --lib] [--pages] [--vcs none] [PATH]",
        about: &[
            "Create an application or a library package in PATH, or in the current directory.",
        ],
        flags: &[
            (
                "--app",
                "create an application, whose src/main.hd prints hello, world",
            ),
            (
                "--lib",
                "create a library, whose src/lib.hd declares one sample function",
            ),
            (
                "--pages",
                "also write a workflow that publishes the documentation to GitHub Pages",
            ),
            ("--vcs none", "run no git init and write no .gitignore"),
        ],
    },
    Command {
        name: "add",
        summary: "require the dependency NAME at PATH@VERSION, fetch it, and record its hash",
        usage: "hd add [--dev] NAME PATH@VERSION",
        about: &[
            "Set the requirement NAME in [dependencies], fetch it, and record its hash in hd.sum.",
        ],
        flags: &[("--dev", "set it in [dev-dependencies] instead"), FORMAT],
    },
    Command {
        name: "update",
        summary: "move dependencies to the newest release on their compatibility line",
        usage: "hd update [NAME]",
        about: &[
            "Move every dependency, or NAME, to the newest release on its compatibility line.",
        ],
        flags: &[FORMAT],
    },
    Command {
        name: "remove",
        summary: "delete the dependency NAME and its hd.sum entries",
        usage: "hd remove NAME",
        about: &["Delete the requirement NAME from hd.toml, and tidy hd.sum."],
        flags: &[FORMAT],
    },
    Command {
        name: "fetch",
        summary: "fetch every selected dependency version the cache lacks, as CI does",
        usage: "hd fetch",
        about: &["Fetch every selected version the cache lacks, and record missing hashes."],
        flags: &[FORMAT],
    },
    Command {
        name: "clean",
        summary: "remove the package's build directory, or with --cache the dependency cache",
        usage: "hd clean [--cache]",
        about: &["Remove the package's build directory, or every fetched dependency version."],
        flags: &[(
            "--cache",
            "remove the fetched versions from the cache instead",
        )],
    },
    Command {
        name: "fmt",
        summary: "format the package's source files, or FILE",
        usage: "hd fmt [--check] [FILE]",
        about: &["Format every .hd file of the package in place, or FILE alone."],
        flags: &[
            (
                "--check",
                "write nothing; print each file that formatting would change",
            ),
            JOBS,
        ],
    },
    Command {
        name: "fix",
        summary: "apply the package's safe fix-its",
        usage: "hd fix",
        about: &["Check the package and apply each safe fix-it, then check again."],
        flags: &[FORMAT, JOBS, MAX_MEMORY],
    },
    Command {
        name: "cache gc",
        summary: "evict compiled entries down to the size cap",
        usage: "hd cache gc",
        about: &["Evict entries of the compiled cache down to its size cap."],
        flags: &[],
    },
    Command {
        name: "help",
        summary: "print the commands, or one command's flags",
        usage: "hd help [COMMAND]",
        about: &["Print the commands, or one command's usage and flags."],
        flags: &[],
    },
];

/// The command list (`cli.command.help`).
pub(crate) fn list() -> String {
    let width = COMMANDS.iter().map(|c| c.name.len()).max().unwrap_or(0) + 2;
    let mut out = String::from("usage: hd COMMAND [FLAGS] [ARGS]\n\ncommands:\n");
    let _ = writeln!(
        out,
        "  {:width$}open the REPL, or run standard input when it is not a terminal",
        "(none)"
    );
    for c in COMMANDS {
        let _ = writeln!(out, "  {:width$}{}", c.name, c.summary);
    }
    let _ = write!(
        out,
        "\nglobal flag:\n  {}  {}\n\nRun 'hd help COMMAND' for a command's flags.\n",
        FORMAT.0, FORMAT.1
    );
    out
}

/// One command's usage line and flags (`cli.command.help.command`), or
/// `None` for a word that names no command.
pub(crate) fn command(name: &str) -> Option<String> {
    let c = COMMANDS
        .iter()
        .find(|c| c.name == name || (name == "cache" && c.name == "cache gc"))?;
    let mut out = format!("usage: {}\n\n", c.usage);
    for line in c.about {
        let _ = writeln!(out, "{line}");
    }
    if !c.flags.is_empty() {
        let width = c.flags.iter().map(|(f, _)| f.len()).max().unwrap_or(0) + 2;
        out.push_str("\nflags:\n");
        for (flag, text) in c.flags {
            let _ = writeln!(out, "  {flag:width$}{text}");
        }
    }
    Some(out)
}
