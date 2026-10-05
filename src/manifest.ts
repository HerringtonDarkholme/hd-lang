// `hd.toml`, a package's manifest (spec/lang/10-modules.md#package-manifest).
// The prototype reads the keys the CLI tier gives a meaning: the `[package]`
// name, the `[[executable]]` tables (spec/cli/command-line.md#executables),
// the `[source]` root, and whether the manifest is a workspace manifest
// (spec/cli/command-line.md#package-mode). The complete schema belongs to
// package tooling (spec/cli/command-line.md#r-cli.tooling.package-schema), so
// other tables and keys are read as TOML and otherwise left alone.

/** A TOML value of the subset `hd.toml` uses. */
export type TomlValue = string | number | boolean | readonly TomlValue[] | TomlTable;

export interface TomlTable {
  readonly [key: string]: TomlValue;
}

/** A mistake in a manifest, at a 1-based line of it. */
export interface ManifestError {
  readonly line: number;
  readonly message: string;
}

/** One `[[executable]]` table (spec/cli/command-line.md#r-cli.exe.table). */
export interface ExecutableDeclaration {
  readonly name: string;
  /** The entry module's path under the source root, such as `tools.migrate`. */
  readonly module: string;
  /** The line of the table's `[[executable]]` header. */
  readonly line: number;
}

export interface Manifest {
  /** The `[package]` table's `name`; absent in a workspace manifest. */
  readonly name?: string;
  /** The line of the `[package]` header, for errors about the package. */
  readonly packageLine: number;
  /** The `[[executable]]` tables, in order. */
  readonly executables: readonly ExecutableDeclaration[];
  /** A workspace manifest lists members and declares no package (cli.mode.workspace). */
  readonly workspace: boolean;
}

class TomlError extends Error {
  readonly line: number;
  constructor(line: number, message: string) {
    super(message);
    this.line = line;
  }
}

type MutableTable = Record<string, TomlValue>;

const BARE_KEY = /[A-Za-z0-9_-]/;

/** A recursive-descent reader of the TOML subset: tables, arrays of tables, and inline values. */
class TomlReader {
  private index = 0;
  private line = 1;
  readonly root: MutableTable = {};
  /** The line of each table's header, for errors about its keys. */
  readonly headers = new Map<TomlTable, number>();
  /** Tables an inline value or a dotted key closed, which no header may reopen. */
  private readonly sealed = new Set<TomlTable>();
  /** Tables a header has defined, which no later header may define again. */
  private readonly defined = new Set<TomlTable>();
  private readonly text: string;

  constructor(text: string) {
    this.text = text;
  }

  read(): MutableTable {
    let current = this.root;
    this.headers.set(this.root, 1);
    for (;;) {
      this.skipBlank();
      if (this.index >= this.text.length) return this.root;
      if (this.peek() === "[") current = this.header();
      else this.keyValue(current);
      this.endOfLine();
    }
  }

  private fail(message: string): never {
    throw new TomlError(this.line, message);
  }

  private peek(offset = 0): string | undefined {
    return this.text[this.index + offset];
  }

  private next(): string {
    const char = this.text[this.index++];
    if (char === undefined) this.fail("unexpected end of the file");
    if (char === "\n") this.line += 1;
    return char;
  }

  /** Skips spaces and tabs on the current line. */
  private skipSpace(): void {
    while (this.peek() === " " || this.peek() === "\t") this.index += 1;
  }

  private skipComment(): void {
    if (this.peek() !== "#") return;
    while (this.index < this.text.length && this.peek() !== "\n") this.index += 1;
  }

  /** Skips spaces, comments, and line ends: what may come between two lines' items. */
  private skipBlank(): void {
    for (;;) {
      this.skipSpace();
      this.skipComment();
      if (this.peek() === "\r" && this.peek(1) === "\n") this.index += 1;
      if (this.peek() !== "\n") return;
      this.next();
    }
  }

  private endOfLine(): void {
    this.skipSpace();
    this.skipComment();
    if (this.peek() === "\r") this.index += 1;
    if (this.index < this.text.length && this.peek() !== "\n")
      this.fail(`expected the end of the line, found '${this.peek()}'`);
  }

  private header(): MutableTable {
    const line = this.line;
    this.next();
    const array = this.peek() === "[";
    if (array) this.next();
    this.skipSpace();
    const path = this.keyPath();
    this.skipSpace();
    if (this.next() !== "]" || (array && this.next() !== "]"))
      this.fail(`expected '${array ? "]]" : "]"}' after the table name`);
    let table = this.root;
    for (const [position, key] of path.entries()) {
      const last = position === path.length - 1;
      const existing = table[key];
      if (last && array) {
        if (existing !== undefined && !Array.isArray(existing))
          this.fail(`'${path.join(".")}' is already a value, not an array of tables`);
        const element: MutableTable = {};
        this.headers.set(element, line);
        table[key] = [...((existing as TomlValue[] | undefined) ?? []), element];
        return element;
      }
      if (existing === undefined) {
        const created: MutableTable = {};
        this.headers.set(created, line);
        if (last) this.defined.add(created);
        table[key] = created;
        table = created;
        continue;
      }
      if (Array.isArray(existing)) {
        const lastTable = existing.at(-1);
        if (typeof lastTable !== "object" || lastTable === null || Array.isArray(lastTable))
          this.fail(`'${key}' is an array of values, not of tables`);
        table = lastTable as MutableTable;
        continue;
      }
      if (typeof existing !== "object" || this.sealed.has(existing as TomlTable))
        this.fail(`'${path.slice(0, position + 1).join(".")}' is already defined`);
      if (last) {
        if (this.defined.has(existing as TomlTable))
          this.fail(`the table [${path.join(".")}] is defined twice`);
        this.defined.add(existing as TomlTable);
        this.headers.set(existing as TomlTable, line);
      }
      table = existing as MutableTable;
    }
    return table;
  }

  private keyPath(): string[] {
    const path = [this.key()];
    for (;;) {
      this.skipSpace();
      if (this.peek() !== ".") return path;
      this.next();
      this.skipSpace();
      path.push(this.key());
    }
  }

  private key(): string {
    const char = this.peek();
    if (char === '"') return this.basicString();
    if (char === "'") return this.literalString();
    let key = "";
    while (this.peek() !== undefined && BARE_KEY.test(this.peek()!)) key += this.next();
    if (key === "") this.fail(`expected a key, found '${char ?? "the end of the file"}'`);
    return key;
  }

  private keyValue(table: MutableTable): void {
    const path = this.keyPath();
    this.skipSpace();
    if (this.next() !== "=") this.fail(`expected '=' after the key '${path.join(".")}'`);
    this.skipSpace();
    const value = this.value();
    this.assign(table, path, value);
  }

  private assign(table: MutableTable, path: readonly string[], value: TomlValue): void {
    let target = table;
    for (const key of path.slice(0, -1)) {
      const existing = target[key];
      if (existing === undefined) {
        const created: MutableTable = {};
        this.sealed.add(created);
        target[key] = created;
        target = created;
      } else if (typeof existing === "object" && !Array.isArray(existing))
        target = existing as MutableTable;
      else this.fail(`'${key}' is already a value`);
    }
    const key = path.at(-1)!;
    if (key in target) this.fail(`the key '${path.join(".")}' is defined twice`);
    target[key] = value;
  }

  private value(): TomlValue {
    const char = this.peek();
    if (char === '"') return this.basicString();
    if (char === "'") return this.literalString();
    if (char === "[") return this.array();
    if (char === "{") return this.inlineTable();
    let word = "";
    while (this.peek() !== undefined && /[A-Za-z0-9_+\-.:]/.test(this.peek()!)) word += this.next();
    if (word === "true") return true;
    if (word === "false") return false;
    if (/^[+-]?\d(_?\d)*$/.test(word)) return Number(word.replaceAll("_", ""));
    if (/^[+-]?\d(_?\d)*(\.\d(_?\d)*)?([eE][+-]?\d(_?\d)*)?$/.test(word))
      return Number(word.replaceAll("_", ""));
    return this.fail(`expected a value, found '${word || (char ?? "the end of the file")}'`);
  }

  private basicString(): string {
    this.next();
    let text = "";
    for (;;) {
      if (this.peek() === "\n" || this.peek() === undefined)
        this.fail("a string must end on its line");
      const char = this.next();
      if (char === '"') return text;
      if (char !== "\\") {
        text += char;
        continue;
      }
      const escape = this.next();
      const simple: Record<string, string> = {
        b: "\b",
        t: "\t",
        n: "\n",
        f: "\f",
        r: "\r",
        '"': '"',
        "\\": "\\",
      };
      if (escape in simple) text += simple[escape];
      else if (escape === "u" || escape === "U") {
        const digits = this.text.slice(this.index, this.index + (escape === "u" ? 4 : 8));
        if (!/^[0-9A-Fa-f]+$/.test(digits) || digits.length !== (escape === "u" ? 4 : 8))
          this.fail(`invalid escape '\\${escape}${digits}'`);
        this.index += digits.length;
        text += String.fromCodePoint(Number.parseInt(digits, 16));
      } else this.fail(`invalid escape '\\${escape}'`);
    }
  }

  private literalString(): string {
    this.next();
    let text = "";
    for (;;) {
      if (this.peek() === "\n" || this.peek() === undefined)
        this.fail("a string must end on its line");
      const char = this.next();
      if (char === "'") return text;
      text += char;
    }
  }

  private array(): TomlValue[] {
    this.next();
    const values: TomlValue[] = [];
    for (;;) {
      this.skipBlank();
      if (this.peek() === "]") {
        this.next();
        return values;
      }
      values.push(this.value());
      this.skipBlank();
      if (this.peek() === ",") this.next();
      else if (this.peek() !== "]") this.fail("expected ',' or ']' in an array");
    }
  }

  private inlineTable(): TomlTable {
    this.next();
    const table: MutableTable = {};
    this.sealed.add(table);
    this.skipSpace();
    if (this.peek() === "}") {
      this.next();
      return table;
    }
    for (;;) {
      this.skipSpace();
      const path = this.keyPath();
      this.skipSpace();
      if (this.next() !== "=") this.fail(`expected '=' after the key '${path.join(".")}'`);
      this.skipSpace();
      this.assign(table, path, this.value());
      this.skipSpace();
      const char = this.next();
      if (char === "}") return table;
      if (char !== ",") this.fail("expected ',' or '}' in an inline table");
    }
  }
}

/** A dotted module path under the source root, such as `tools.migrate`. */
const MODULE_PATH = /^[\p{ID_Start}_][\p{ID_Continue}_]*(\.[\p{ID_Start}_][\p{ID_Continue}_]*)*$/u;

function isTable(value: TomlValue | undefined): value is TomlTable {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/**
 * Reads `hd.toml`. Returns the manifest, or the errors that make it invalid
 * (spec/cli/command-line.md#r-cli.exit.hd-failure: `hd` rejects a manifest).
 */
export function readManifest(
  text: string,
): { readonly manifest: Manifest } | { readonly errors: readonly ManifestError[] } {
  const reader = new TomlReader(text);
  let root: TomlTable;
  try {
    root = reader.read();
  } catch (error) {
    if (error instanceof TomlError)
      return { errors: [{ line: error.line, message: error.message }] };
    throw error;
  }
  const errors: ManifestError[] = [];
  const lineOf = (table: TomlTable | undefined): number =>
    (table && reader.headers.get(table)) ?? 1;
  const packageTable = root.package;
  const workspace = root.workspace !== undefined;
  if (packageTable !== undefined && !isTable(packageTable))
    errors.push({ line: 1, message: "'package' must be a table, [package]" });
  const packageLine = lineOf(isTable(packageTable) ? packageTable : undefined);
  let name: string | undefined;
  if (isTable(packageTable)) {
    if (typeof packageTable.name !== "string" || packageTable.name === "")
      errors.push({
        line: packageLine,
        message: 'the [package] table needs a name, such as name = "shop"',
      });
    else name = packageTable.name;
  } else if (!workspace)
    errors.push({
      line: 1,
      message: "hd.toml declares no package: add a [package] table with a name",
    });
  // The prototype knows only the default source root
  // (spec/lang/10-modules.md#r-module.manifest.source-root).
  const source = root.source;
  if (source !== undefined) {
    const sourceRoot = isTable(source) ? source.root : undefined;
    if (sourceRoot !== undefined && sourceRoot !== "src")
      errors.push({
        line: lineOf(isTable(source) ? source : undefined),
        message: `the prototype supports only the default source root, root = "src", not ${JSON.stringify(sourceRoot)}`,
      });
  }
  const executables: ExecutableDeclaration[] = [];
  const declared = root.executable;
  if (declared !== undefined && (!Array.isArray(declared) || !declared.every(isTable)))
    errors.push({ line: 1, message: "'executable' must be an array of tables, [[executable]]" });
  else
    for (const table of (declared ?? []) as readonly TomlTable[]) {
      const line = lineOf(table);
      const { name: executableName, module } = table;
      if (typeof executableName !== "string" || executableName === "") {
        errors.push({
          line,
          message: 'an [[executable]] table needs a name, such as name = "migrate"',
        });
        continue;
      }
      if (typeof module !== "string" || !MODULE_PATH.test(module)) {
        errors.push({
          line,
          message: `executable '${executableName}' needs a module, its entry module's path under the source root, such as module = "tools.migrate"`,
        });
        continue;
      }
      // Executables have unique names (spec/cli/command-line.md#r-cli.exe.several).
      if (executables.some((executable) => executable.name === executableName)) {
        errors.push({ line, message: `two executables are named '${executableName}'` });
        continue;
      }
      executables.push({ name: executableName, module, line });
    }
  if (errors.length > 0) return { errors };
  return {
    manifest: { ...(name === undefined ? {} : { name }), packageLine, executables, workspace },
  };
}
