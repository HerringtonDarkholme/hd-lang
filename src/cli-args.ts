// The `hd` command line: which commands exist, which flags each one owns, and
// the help text. Each command owns its flags; `--format` is the only global
// flag. src/README.md (CLI) documents the commands.

/** A flag a command accepts. */
export interface FlagSpec {
  /** The spelling, such as `--seed`. */
  readonly name: string;
  /** The value's placeholder, such as `N`; absent for a switch. */
  readonly value?: string;
  readonly help: string;
  /** Allowed values, checked when the flag is parsed. */
  readonly choices?: readonly string[];
  /** The value is a non-negative integer. */
  readonly count?: boolean;
  /**
   * A flag the conformance runner passes (spec/conformance/README.md#command-contract).
   * Help lists it apart from the flags people use.
   */
  readonly conformance?: boolean;
}

export interface CommandSpec {
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

export const RUNTIME_PROFILE_NAMES = [
  "misbehaving-host",
  "pending-gate",
  "pending-write",
  "ready-counter",
  "ready-float",
  "ready-gate",
  "ready-text",
  "special-float-host",
] as const;
export const RUNTIME_SCENARIO_NAMES = [
  "cancellation-cleanup",
  "competing-drivers",
  "reentrant-poll",
] as const;

const FORMAT: FlagSpec = {
  name: "--format",
  value: "FORMAT",
  choices: ["text", "json"],
  help: "print diagnostics as text (the default) or as JSON lines",
};

const PROFILE: FlagSpec = {
  name: "--profile",
  value: "NAME",
  choices: RUNTIME_PROFILE_NAMES,
  help: "runtime profile: the host capabilities a fixture's entry point may require",
  conformance: true,
};
const TEST_LAYOUT: FlagSpec = {
  name: "--test-layout",
  value: "LAYOUT",
  choices: ["test-module", "integration"],
  help: "compile FILE as a test module of this layout",
  conformance: true,
};
const PACKAGE_TREE: FlagSpec = {
  name: "--package-tree",
  value: "DIR",
  help: "the other files of FILE's package; needs --package-path",
  conformance: true,
};
const PACKAGE_PATH: FlagSpec = {
  name: "--package-path",
  value: "PATH",
  help: "the package path FILE takes in --package-tree, such as src/shop/mod.hd",
  conformance: true,
};

/** How a command finds FILE's package (src/README.md, Commands). */
const PACKAGE_NOTE =
  "A FILE under a package's src/ or tests/ is linked with the rest of the package.";

const PARSE_SUMMARY = "parse FILE and its tests: block, and print 'FILE: ok'";

export const COMMANDS: readonly CommandSpec[] = [
  {
    name: "build",
    operands: "FILE",
    minOperands: 1,
    maxOperands: 1,
    summary: "compile FILE to NAME.wasm in the current directory",
    flags: [
      { name: "--wat", help: "print the WebAssembly text instead of writing a file" },
      PROFILE,
    ],
    notes: [PACKAGE_NOTE],
  },
  {
    name: "run",
    operands: "FILE",
    minOperands: 1,
    maxOperands: 1,
    summary: "run FILE's entry point, the public main or main!",
    flags: [
      {
        name: "--entry",
        value: "NAME",
        help: "run the exported function NAME instead, and print its result",
      },
      PROFILE,
    ],
    notes: [PACKAGE_NOTE],
  },
  {
    name: "test",
    operands: "[FILE|DIR]",
    minOperands: 0,
    maxOperands: 1,
    summary: "run the test cases of FILE, of DIR, or of the current package",
    flags: [
      { name: "--update", help: "record snapshot files instead of failing on a difference" },
      { name: "--seed", value: "N", count: true, help: "property-test seed; a failure prints it" },
      { name: "--cases", value: "N", count: true, help: "cases per property test" },
      { name: "--shrink", value: "N", count: true, help: "most shrink steps for a failing case" },
      PROFILE,
      {
        name: "--scenario",
        value: "NAME",
        choices: RUNTIME_SCENARIO_NAMES,
        help: "drive the program through a runtime scenario instead of running tests",
        conformance: true,
      },
      {
        name: "--pending-function",
        value: "NAME",
        help: "the suspending function that stays pending; needs --scenario cancellation-cleanup",
        conformance: true,
      },
      TEST_LAYOUT,
      PACKAGE_TREE,
      PACKAGE_PATH,
    ],
    notes: [
      "With DIR, a package (a directory with hd.toml or src/) runs each module",
      "under src/ and tests/ with the other modules linked; any other directory",
      "runs each .hd file in it. With no path, it tests the package that holds",
      "the current directory, or else the current directory.",
      PACKAGE_NOTE,
    ],
  },
  {
    name: "check",
    operands: "FILE",
    minOperands: 1,
    maxOperands: 1,
    summary: "type-check FILE without running it",
    flags: [
      { name: "--tests", help: "also check the tests: block and test-only code" },
      PROFILE,
      TEST_LAYOUT,
      PACKAGE_TREE,
      PACKAGE_PATH,
    ],
    notes: [PACKAGE_NOTE],
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
    flags: [PROFILE],
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
  | {
      readonly kind: "command";
      readonly command: CommandSpec;
      readonly format: "text" | "json";
      readonly flags: ReadonlyMap<string, string | true>;
      readonly operands: readonly string[];
    };

function commandNamed(name: string): CommandSpec | undefined {
  return COMMANDS.find((command) => command.name === name);
}

function usageLine(command: CommandSpec): string {
  const own = command.flags.filter((flag) => !flag.conformance);
  const flags = own.map((flag) => `[${flag.name}${flag.value ? ` ${flag.value}` : ""}]`);
  return ["usage: hd", command.name, ...flags, command.operands].filter(Boolean).join(" ");
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
  const own = command.flags.filter((flag) => !flag.conformance);
  const conformance = command.flags.filter((flag) => flag.conformance);
  const lines = [usageLine(command), "", sentence(command.summary)];
  if (command.notes) lines.push("", ...command.notes);
  // `hd repl` prints no diagnostics, so `--format` does nothing there.
  const flags = command.name === "repl" ? own : [...own, FORMAT];
  if (flags.length > 0) lines.push("", "flags:", ...flagRows(flags));
  if (conformance.length > 0) {
    lines.push(
      "",
      "flags for conformance fixtures (spec/conformance/README.md#command-contract):",
      ...flagRows(conformance),
    );
    if (conformance.includes(PACKAGE_TREE))
      lines.push(
        "",
        "--test-layout, --package-tree, and --package-path are temporary. A package's",
        "layout should come from its hd.toml, which the prototype does not read yet.",
      );
  }
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
  if (first === undefined || first === "--help" || first === "-h") return { kind: "help" };
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
  const command = commandNamed(name);
  if (!command) {
    const known = first.startsWith("-")
      ? `unknown flag ${first} before the command`
      : `unknown command '${first}'`;
    throw new UsageError(`hd: ${known}\nRun 'hd help' for the command list.`);
  }
  const flags = new Map<string, string | true>();
  const operands: string[] = [];
  let flagsEnded = false;
  while (rest.length > 0) {
    const arg = rest.shift()!;
    if (flagsEnded || !arg.startsWith("-") || arg === "-") {
      operands.push(arg);
      continue;
    }
    if (arg === "--") {
      flagsEnded = true;
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
          ? `${arg} is not a flag of hd ${command.name}; ${listed(owners)} accept${owners.length === 1 ? "s" : ""} it`
          : `unknown flag ${arg}`;
      throw new UsageError(`hd ${command.name}: ${why}\n${hint(command.name)}`);
    }
    if (flags.has(flag.name))
      throw new UsageError(`hd ${command.name}: ${flag.name} is given twice`);
    flags.set(flag.name, flag.value ? flagValue(command, flag, rest.shift()) : true);
  }
  if (operands.length < command.minOperands || operands.length > command.maxOperands) {
    const problem =
      operands.length < command.minOperands
        ? `missing ${command.operands.split(" ")[operands.length]}`
        : `unexpected argument '${operands[command.maxOperands]}'`;
    throw new UsageError(
      `hd ${command.name}: ${problem}\n${usageLine(command)}\n${hint(command.name)}`,
    );
  }
  return { kind: "command", command, format, flags, operands };
}
