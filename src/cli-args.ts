// The `hd` command line: which commands exist, which flags each one owns, and
// the help text. Each command owns its flags; `--format` is the only global
// flag. src/README.md (CLI) documents the commands.

/** A flag a command accepts. */
interface FlagSpec {
  /** The spelling, such as `--seed`. */
  readonly name: string;
  /** The value's placeholder, such as `N`; absent for a switch. */
  readonly value?: string;
  readonly help: string;
  /** Allowed values, checked when the flag is parsed. */
  readonly choices?: readonly string[];
  /** The value is a non-negative integer. */
  readonly count?: boolean;
}

interface CommandSpec {
  /** `build`, or `debug hir` for a subcommand. */
  readonly name: string;
  /** What follows the flags in the usage line, such as `FILE`. */
  readonly operands: string;
  readonly minOperands: number;
  readonly maxOperands: number;
  readonly summary: string;
  readonly flags: readonly FlagSpec[];
  /** Extra help lines after the flags. */
  readonly notes?: readonly string[];
  /** Not in the command list; `hd parse` stays for the conformance runner. */
  readonly hidden?: boolean;
}

const FORMAT: FlagSpec = {
  name: "--format",
  value: "FORMAT",
  choices: ["text", "json"],
  help: "print diagnostics as text (the default) or as JSON lines",
};

const RELEASE: FlagSpec = {
  name: "--release",
  help: "build for release: integer overflow wraps instead of panicking",
};

/** How a command finds its package (spec/cli/command-line.md#package-mode). */
const PACKAGE_NOTE = "The package is the one whose hd.toml is nearest above the current directory.";

/** What a FILE in a package means (spec/cli/command-line.md#r-cli.package.file). */
const FILE_NOTE =
  "A FILE under a package's src/ or tests/ is that module, linked with the rest of the package.";

const PARSE_SUMMARY = "parse FILE and its tests: block, and print 'FILE: ok'";

const COMMANDS: readonly CommandSpec[] = [
  {
    name: "build",
    operands: "[FILE]",
    minOperands: 0,
    maxOperands: 1,
    summary: "build the package's executables, or compile FILE to NAME.wasm",
    flags: [
      { name: "--wat", help: "with FILE, print the WebAssembly text instead of writing a file" },
      RELEASE,
    ],
    notes: [
      PACKAGE_NOTE,
      "Each executable NAME is written to build/debug/NAME.wasm, or build/release/ with --release.",
      FILE_NOTE,
    ],
  },
  {
    name: "run",
    operands: "[NAME]",
    minOperands: 0,
    maxOperands: 1,
    summary: "run the package's executable, or the executable or task named NAME",
    flags: [RELEASE],
    notes: [
      PACKAGE_NOTE,
      "Without NAME, the package must have exactly one executable: src/main.hd, or",
      "one [[executable]] table of hd.toml. Words after -- are the program's arguments.",
    ],
  },
  {
    name: "test",
    operands: "[FILE]",
    minOperands: 0,
    maxOperands: 1,
    summary: "run the test cases of the package, or of FILE",
    flags: [
      { name: "--update", help: "record snapshot files instead of failing on a difference" },
      { name: "--seed", value: "N", count: true, help: "property-test seed; a failure prints it" },
      { name: "--cases", value: "N", count: true, help: "cases per property test" },
      { name: "--shrink", value: "N", count: true, help: "most shrink steps for a failing case" },
    ],
    notes: [
      PACKAGE_NOTE,
      "Without FILE, it runs every tests: block, test module, and integration test.",
      FILE_NOTE,
    ],
  },
  {
    name: "check",
    operands: "[FILE]",
    minOperands: 0,
    maxOperands: 1,
    summary: "type-check the package, or FILE, without running it",
    flags: [
      { name: "--tests", help: "also check the test code" },
      { name: "--all", help: "also check the test code and the tasks" },
    ],
    notes: [PACKAGE_NOTE, "Without FILE, it checks the library and the executables.", FILE_NOTE],
  },
  {
    name: "new",
    operands: "[PATH]",
    minOperands: 0,
    maxOperands: 1,
    summary: "create a package in PATH, or in the current directory",
    flags: [
      { name: "--app", help: "create an application: src/main.hd and a test that runs it" },
      { name: "--lib", help: "create a library: src/lib.hd and a test that uses it" },
      {
        name: "--pages",
        help: "also write .github/workflows/docs.yml, which publishes hd doc to GitHub Pages",
      },
      {
        name: "--vcs",
        value: "VCS",
        choices: ["none"],
        help: "with none, run no git init and write no .gitignore",
      },
    ],
    notes: [
      "With neither --app nor --lib, hd new asks which kind on a terminal, and is",
      "an error otherwise. The package is named after its directory. Unless the",
      "directory is in a git repository already, hd new runs git init there.",
    ],
  },
  {
    name: "explain",
    operands: "CODE",
    minOperands: 1,
    maxOperands: 1,
    summary: "print what the specification says about a diagnostic code",
    flags: [],
  },
  {
    name: "doc",
    operands: "NAME [FILE|PKG]",
    minOperands: 1,
    maxOperands: 2,
    summary: "print a symbol's definition, documentation, and members",
    flags: [],
    notes: ["PKG is a package directory; the default is the current directory."],
  },
  {
    name: "def",
    operands: "NAME [FILE|PKG]",
    minOperands: 1,
    maxOperands: 2,
    summary: "print where a symbol is defined, with its signature",
    flags: [],
    notes: ["PKG is a package directory; the default is the current directory."],
  },
  {
    name: "repl",
    operands: "",
    minOperands: 0,
    maxOperands: 0,
    summary: "start an interactive session",
    flags: [],
  },
  {
    name: "debug parse",
    operands: "FILE",
    minOperands: 1,
    maxOperands: 1,
    summary: PARSE_SUMMARY,
    flags: [],
  },
  {
    name: "debug hir",
    operands: "FILE",
    minOperands: 1,
    maxOperands: 1,
    summary: "print FILE's checked HIR as JSON",
    flags: [],
  },
  {
    // `hd FILE` (cli.file.run): the first word is a path, not a command name.
    name: "file",
    operands: "FILE",
    minOperands: 1,
    maxOperands: 1,
    summary: "run FILE as a single-file program",
    flags: [],
    hidden: true,
  },
  {
    name: "parse",
    operands: "FILE",
    minOperands: 1,
    maxOperands: 1,
    summary: `${PARSE_SUMMARY}; the conformance runner's spelling of hd debug parse`,
    flags: [],
    hidden: true,
  },
];

const DEBUG_SUMMARY = "print internal compiler output: parse, hir";
const HELP_SUMMARY = "print the commands, or one command's flags";

/** A command-line mistake: hd prints the message and exits 2. */
export class UsageError extends Error {}

export type ParsedCommand =
  | { readonly kind: "help"; readonly topic?: string }
  | { readonly kind: "default"; readonly format: "text" | "json" }
  | {
      readonly kind: "command";
      readonly command: CommandSpec;
      readonly format: "text" | "json";
      readonly flags: ReadonlyMap<string, string | true>;
      readonly operands: readonly string[];
      /**
       * The words after the first `--`, which are the program's arguments
       * (spec/cli/command-line.md#r-cli.args.separator).
       */
      readonly programArguments: readonly string[];
    };

function commandNamed(name: string): CommandSpec | undefined {
  return COMMANDS.find((command) => command.name === name);
}

function usageLine(command: CommandSpec): string {
  const flags = command.flags.map((flag) => `[${flag.name}${flag.value ? ` ${flag.value}` : ""}]`);
  if (command.name === "file") return ["usage: hd FILE", ...flags, "[-- ARGS]"].join(" ");
  const args = command.name === "run" ? "[-- ARGS]" : "";
  return ["usage: hd", command.name, ...flags, command.operands, args].filter(Boolean).join(" ");
}

function sentence(text: string): string {
  return `${text[0]!.toUpperCase()}${text.slice(1)}.`;
}

/** Two aligned columns; a row's extra lines go under its second column. */
function columns(rows: readonly (readonly [string, string, ...string[]])[]): string[] {
  const width = Math.max(...rows.map(([label]) => label.length));
  return rows.flatMap(([label, text, ...more]) => [
    `  ${label.padEnd(width)}  ${text}`,
    ...more.map((line) => `  ${" ".repeat(width)}  ${line}`),
  ]);
}

function flagRows(flags: readonly FlagSpec[]): string[] {
  return columns(
    flags.map((flag): [string, string, ...string[]] => {
      const label = `${flag.name}${flag.value ? ` ${flag.value}` : ""}`;
      return flag.choices && flag !== FORMAT
        ? [label, flag.help, `${flag.value} is one of: ${flag.choices.join(", ")}`]
        : [label, flag.help];
    }),
  );
}

/** The short command list that `hd`, `hd --help`, and `hd help` print. */
export function overviewHelp(): string {
  const listed = COMMANDS.filter((command) => !command.hidden && !command.name.includes(" "));
  return [
    "usage: hd COMMAND [FLAGS] [ARGS]",
    "",
    "commands:",
    ...columns([
      ["FILE", "run FILE as a single-file program, which may use only std"],
      ...listed.map((command): [string, string] => [command.name, command.summary]),
      ["debug", DEBUG_SUMMARY],
      ["help", HELP_SUMMARY],
    ]),
    "",
    "global flag:",
    ...flagRows([FORMAT]),
    "",
    "Run 'hd help COMMAND' for a command's flags.",
  ].join("\n");
}

/** `hd help COMMAND`: the command's usage line and its own flags. */
export function commandHelp(topic: string): string | undefined {
  if (topic === "help") return ["usage: hd help [COMMAND]", "", sentence(HELP_SUMMARY)].join("\n");
  if (topic === "debug") {
    const subcommands = COMMANDS.filter((command) => command.name.startsWith("debug "));
    return [
      "usage: hd debug parse|hir FILE",
      "",
      "Print internal compiler output. It is for compiler work, not a stable interface.",
      "",
      "subcommands:",
      ...columns(
        subcommands.map((command): [string, string] => [
          `${command.name.slice("debug ".length)} ${command.operands}`,
          command.summary,
        ]),
      ),
      "",
      "'hd parse FILE' is the same as 'hd debug parse FILE'. The conformance",
      "runner uses that spelling (spec/conformance/README.md#command-contract).",
      "Run 'hd help debug hir' for its flags.",
    ].join("\n");
  }
  const command = commandNamed(topic);
  if (!command) return undefined;
  const own = command.flags;
  const lines = [usageLine(command), "", sentence(command.summary)];
  if (command.notes) lines.push("", ...command.notes);
  // `hd repl` and `hd new` print no diagnostics, so `--format` does nothing there.
  const flags = ["repl", "new"].includes(command.name) ? own : [...own, FORMAT];
  if (flags.length > 0) lines.push("", "flags:", ...flagRows(flags));
  return lines.join("\n");
}

function hint(command: string): string {
  return `Run 'hd help ${command}' for its flags.`;
}

/** Which commands accept a flag, for the wrong-command error. */
function ownersOf(name: string): string[] {
  return COMMANDS.filter(
    (command) => !command.hidden && command.flags.some((flag) => flag.name === name),
  ).map((command) => `hd ${command.name}`);
}

/** `a`, `a and b`, or `a, b, and c`. */
function listed(items: readonly string[]): string {
  if (items.length < 3) return items.join(" and ");
  return `${items.slice(0, -1).join(", ")}, and ${items.at(-1)}`;
}

/** Checks a flag's value; `command` is absent for a flag before the command. */
function flagValue(
  command: CommandSpec | undefined,
  flag: FlagSpec,
  value: string | undefined,
): string {
  const where = command ? `hd ${command.name}` : "hd";
  if (value === undefined || value.startsWith("--"))
    throw new UsageError(`${where}: ${flag.name} needs a value, ${flag.value}`);
  if (flag.choices && !flag.choices.includes(value))
    throw new UsageError(
      `${where}: ${flag.name} must be one of ${flag.choices.join(", ")}, not '${value}'`,
    );
  if (flag.count && !/^\d+$/.test(value))
    throw new UsageError(`${where}: ${flag.name} needs a non-negative integer, not '${value}'`);
  return value;
}

/**
 * Parses `hd` arguments. Flags may come before or after the operands; `--`
 * ends the flags. Throws a {@link UsageError} for a mistake.
 */
export function parseCommandLine(args: readonly string[]): ParsedCommand {
  const rest = [...args];
  let format: "text" | "json" = "text";
  // `--format` may also come before the command.
  while (rest[0] === "--format") {
    rest.shift();
    format = flagValue(undefined, FORMAT, rest.shift()) as "text" | "json";
  }
  const first = rest.shift();
  // `hd` alone opens the REPL, or runs standard input (cli.repl.open.terminal, cli.stdin.program).
  if (first === undefined) return { kind: "default", format };
  if (first === "--help" || first === "-h") return { kind: "help" };
  if (first === "help") {
    const topic = rest.join(" ");
    if (topic === "") return { kind: "help" };
    if (commandHelp(topic) === undefined)
      throw new UsageError(`hd help: no command '${topic}'\nRun 'hd help' for the command list.`);
    return { kind: "help", topic };
  }
  let name = first;
  if (first === "debug") {
    const sub = rest.shift();
    if (sub === undefined || sub === "--help" || sub === "-h")
      return { kind: "help", topic: "debug" };
    name = `debug ${sub}`;
    if (!commandNamed(name))
      throw new UsageError(
        `hd debug: no subcommand '${sub}'; use parse or hir\nRun 'hd help debug' for its subcommands.`,
      );
  }
  // `hd FILE` runs FILE as a single-file program (cli.file.run).
  const isFile =
    commandNamed(name) === undefined && !first.startsWith("-") && first.endsWith(".hd");
  if (isFile) {
    rest.unshift(first);
    name = "file";
  }
  // The word `file` is no command; it names the `hd FILE` form only.
  const command = name === "file" && !isFile ? undefined : commandNamed(name);
  if (!command) {
    const known = first.startsWith("-")
      ? `unknown flag ${first} before the command`
      : `unknown command '${first}'`;
    throw new UsageError(`hd: ${known}\nRun 'hd help' for the command list.`);
  }
  const flags = new Map<string, string | true>();
  const operands: string[] = [];
  // `hd` reads none of the words after the first `--` as its own
  // (spec/cli/command-line.md#r-cli.args.separator).
  const separator = rest.indexOf("--");
  const programArguments = separator === -1 ? [] : rest.splice(separator).slice(1);
  const takesArguments = command.name === "run" || command.name === "file";
  if (separator !== -1 && !takesArguments)
    throw new UsageError(
      `hd ${command.name}: takes no program arguments after --; only hd run and hd FILE pass them to a program\n${hint(command.name)}`,
    );
  const shown = command.name === "file" ? "FILE" : command.name;
  while (rest.length > 0) {
    const arg = rest.shift()!;
    if (!arg.startsWith("-") || arg === "-") {
      operands.push(arg);
      continue;
    }
    if (arg === "--help" || arg === "-h") return { kind: "help", topic: command.name };
    if (arg === "--format") {
      format = flagValue(command, FORMAT, rest.shift()) as "text" | "json";
      continue;
    }
    const flag = command.flags.find((candidate) => candidate.name === arg);
    if (!flag) {
      const owners = ownersOf(arg);
      const why =
        owners.length > 0
          ? `${arg} is not a flag of hd ${shown}; ${listed(owners)} accept${owners.length === 1 ? "s" : ""} it`
          : `unknown flag ${arg}`;
      throw new UsageError(`hd ${shown}: ${why}\n${hint(command.name)}`);
    }
    if (flags.has(flag.name)) throw new UsageError(`hd ${shown}: ${flag.name} is given twice`);
    flags.set(flag.name, flag.value ? flagValue(command, flag, rest.shift()) : true);
  }
  if (operands.length < command.minOperands || operands.length > command.maxOperands) {
    const extra = operands[command.maxOperands];
    // A further word before `--` is an error that suggests `--`
    // (spec/cli/command-line.md#r-cli.args.extra-word).
    const problem =
      operands.length < command.minOperands
        ? `missing ${command.operands.split(" ")[operands.length]}`
        : takesArguments
          ? `unexpected argument '${extra}'; pass the program's arguments after --, as in hd ${command.name === "file" ? operands[0] : `run ${operands[0]}`} -- ${extra}`
          : `unexpected argument '${extra}'`;
    throw new UsageError(
      `hd ${command.name === "file" ? "FILE" : command.name}: ${problem}\n${usageLine(command)}\n${hint(command.name)}`,
    );
  }
  return { kind: "command", command, format, flags, operands, programArguments };
}
