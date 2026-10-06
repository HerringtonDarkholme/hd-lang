// Capability grants (spec/cli/command-line.md#capability-grants): what a
// program's host providers may touch at run time. A grant comes from the
// package's `[capabilities]` table, or `[test.capabilities]` for a test
// case, and the command's `--cap NAME=VALUE` flags, by Grant Precedence.
// The default profile checks each call against it (default-profile.ts,
// http-host.ts), and `hd` refuses to start a module whose import list
// needs a totally denied trait (execute.ts).

import { existsSync, realpathSync } from "node:fs";
import { dirname, isAbsolute, join, resolve, sep } from "node:path";

import { UsageError } from "../cli-args.ts";
import { CAPABILITY_KEYS, type CapabilityTable } from "../manifest.ts";

/** One trait's grant: no limit, a total deny, or the scope entries it covers. */
export type Grant =
  | { readonly kind: "all" }
  | { readonly kind: "deny"; readonly setting: string }
  | {
      readonly kind: "list";
      /** The entries; a path entry is absolute, with its links resolved. */
      readonly entries: readonly string[];
      /** Entries known only while the program runs, as a test case's temporary directory. */
      readonly more?: () => readonly string[];
    };

/** The grant of each trait that a setting names, by trait name. */
export type CapabilityGrants = ReadonlyMap<string, Grant>;

/** No limit: the grant of a trait that no setting names (cli.cap.order.default). */
export const UNLIMITED: Grant = { kind: "all" };

/** The traits whose scope entries are paths. */
const PATH_TRAITS = new Set(["FsRead", "FsWrite"]);

/** One `--cap NAME=VALUE` flag, checked. */
export interface CapabilityFlag {
  readonly name: string;
  readonly value: boolean | readonly string[];
  /** The flag as written, which a total deny names. */
  readonly text: string;
}

/**
 * Checks `--cap NAME=VALUE` flags. Throws a {@link UsageError} for a flag
 * that names an unknown trait (cli.cap.flag.unknown), that gives a list to a
 * trait without scope entries (cli.cap.flag.unscoped), or whose `Http`
 * entry is no host.
 */
export function capabilityFlags(values: readonly string[], command: string): CapabilityFlag[] {
  return values.map((value) => {
    const equals = value.indexOf("=");
    const name = equals === -1 ? value : value.slice(0, equals);
    const scoped = CAPABILITY_KEYS[name];
    if (scoped === undefined)
      throw new UsageError(
        `${command}: --cap ${name}: unknown capability; use one of ${Object.keys(CAPABILITY_KEYS).join(", ")}`,
      );
    if (equals === -1)
      throw new UsageError(`${command}: --cap ${name} needs a value: true, false, or a list`);
    const text = value.slice(equals + 1);
    if (text === "true" || text === "false")
      return { name, value: text === "true", text: `--cap ${value}` };
    if (!scoped)
      throw new UsageError(
        `${command}: --cap ${name} takes only true or false, since ${name} has no scope entries`,
      );
    const entries = text === "" ? [] : text.split(",");
    for (const entry of entries)
      if (name === "Http" && !hostEntry(entry))
        throw new UsageError(
          `${command}: --cap Http=${entry}: an entry is a host name, an IPv4 address, or a bracketed IPv6 address, with an optional :port, as in api.example.com or *.example.com`,
        );
    return { name, value: entries, text: `--cap ${value}` };
  });
}

/** Where a program's grant comes from (spec/cli/command-line.md#grant-precedence). */
export interface GrantSources {
  /** The table that applies, if any, and the setting a deny in it names. */
  readonly table?: CapabilityTable;
  readonly tableName?: string;
  /** The directory a relative path in the table is relative to: the package directory. */
  readonly tableBase?: string;
  readonly flags: readonly CapabilityFlag[];
  /** The directory a relative path in a flag is relative to: the command's working directory. */
  readonly flagBase: string;
}

/**
 * Each named trait's grant, by Grant Precedence: a `false` in the table or
 * any flag denies it (cli.cap.order.deny); else flags that name it give no
 * limit when one gives `true`, or else every entry they list
 * (cli.cap.order.flag); else the table's value (cli.cap.order.table). A
 * trait that nothing names is absent, so it has no limit
 * (cli.cap.order.default).
 */
export function grantsOf(sources: GrantSources): Map<string, Grant> {
  const grants = new Map<string, Grant>();
  const table = sources.table ?? {};
  const names = new Set([...Object.keys(table), ...sources.flags.map(({ name }) => name)]);
  for (const name of names) {
    const flags = sources.flags.filter((flag) => flag.name === name);
    const denyingFlag = flags.find((flag) => flag.value === false);
    if (table[name] === false)
      grants.set(name, { kind: "deny", setting: `${name} = false in ${sources.tableName}` });
    else if (denyingFlag) grants.set(name, { kind: "deny", setting: denyingFlag.text });
    else if (flags.length > 0)
      grants.set(
        name,
        flags.some((flag) => flag.value === true)
          ? UNLIMITED
          : listGrant(
              name,
              flags.flatMap((flag) => flag.value as readonly string[]),
              sources.flagBase,
            ),
      );
    else {
      const value = table[name]!;
      grants.set(
        name,
        value === true ? UNLIMITED : listGrant(name, value as readonly string[], sources.tableBase),
      );
    }
  }
  return grants;
}

function listGrant(name: string, entries: readonly string[], base = "."): Grant {
  return {
    kind: "list",
    entries: PATH_TRAITS.has(name) ? entries.map((entry) => resolvedPath(base, entry)) : entries,
  };
}

/**
 * The test grant (spec/cli/command-line.md#r-cli.test.env.grant): Grant
 * Precedence over `[test.capabilities]` and the flags of `hd test`, where
 * `FsRead` covers the package directory and the test case's temporary
 * directory, and `FsWrite` that directory, unless the table or a flag
 * names them; a list gets those entries added (cli.test.env.grant.fs-added).
 */
export function testGrantsOf(
  sources: GrantSources,
  packageRoot: string,
  tempDir: () => string | undefined,
): Map<string, Grant> {
  const grants = grantsOf(sources);
  const temporary = (): readonly string[] => {
    const directory = tempDir();
    return directory === undefined ? [] : [resolvedPath("/", directory)];
  };
  for (const [name, fixed] of [
    ["FsRead", [resolvedPath("/", packageRoot)]],
    ["FsWrite", []],
  ] as const) {
    const grant = grants.get(name);
    if (grant === undefined) grants.set(name, { kind: "list", entries: fixed, more: temporary });
    else if (grant.kind === "list")
      grants.set(name, { kind: "list", entries: [...grant.entries, ...fixed], more: temporary });
  }
  return grants;
}

/**
 * `path` from `base`, absolute, with `..` and symbolic links resolved
 * (spec/cli/command-line.md#r-cli.cap.scope.path.resolved). The part of the
 * path that does not exist yet is joined to its nearest existing parent.
 */
export function resolvedPath(base: string, path: string): string {
  const absolute = isAbsolute(path) ? resolve(path) : resolve(base, path);
  const missing: string[] = [];
  let existing = absolute;
  while (!existsSync(existing) && dirname(existing) !== existing) {
    missing.unshift(existing.slice(dirname(existing).length).replace(/^[/\\]/, ""));
    existing = dirname(existing);
  }
  let real = existing;
  try {
    real = realpathSync(existing);
  } catch {
    // An unreadable parent stays as written.
  }
  return missing.length === 0 ? real : join(real, ...missing);
}

/**
 * Whether `grant` covers `path`, already resolved: a path entry covers the
 * file it names, or every path under the directory it names
 * (spec/cli/command-line.md#r-cli.cap.scope.path).
 */
export function coversPath(grant: Grant, path: string): boolean {
  if (grant.kind === "all") return true;
  if (grant.kind === "deny") return false;
  return [...grant.entries, ...(grant.more?.() ?? [])].some(
    (entry) => path === entry || path.startsWith(entry.endsWith(sep) ? entry : entry + sep),
  );
}

/**
 * Whether `grant` covers the environment variable `name`: an entry names it,
 * or ends in `*` and its text before the `*` starts `name`
 * (spec/cli/command-line.md#r-cli.cap.scope.env).
 */
export function coversName(grant: Grant, name: string): boolean {
  if (grant.kind === "all") return true;
  if (grant.kind === "deny") return false;
  return grant.entries.some((entry) =>
    entry.endsWith("*") ? name.startsWith(entry.slice(0, -1)) : name === entry,
  );
}

/**
 * The message of a startup refusal (spec/cli/command-line.md#r-cli.cap.total.message):
 * the first need of the module that its grants deny totally, or undefined.
 */
export function totalDenial(
  needs: readonly string[],
  grants: CapabilityGrants | undefined,
): string | undefined {
  for (const need of needs) {
    const grant = grants?.get(need);
    if (grant?.kind === "deny")
      return `hd: the program needs ${need}, which ${grant.setting} denies`;
  }
  return undefined;
}

/** An `Http` entry: a host to match, whether it is a `*.` wildcard, and its port. */
interface HostEntry {
  readonly host: string;
  readonly wildcard: boolean;
  readonly port?: string;
}

/**
 * Parses an `Http` scope entry (spec/cli/command-line.md#r-cli.cap.scope.host.forms):
 * a name, an IPv4 address, or a bracketed IPv6 address, each with an
 * optional `:port`, or `*.` before a name. Hosts are normalized as URL hosts
 * are, so `[0:0::1]` matches `[::1]`.
 */
function hostEntry(entry: string): HostEntry | undefined {
  const wildcard = entry.startsWith("*.");
  const rest = wildcard ? entry.slice(2) : entry;
  if (rest === "" || /[/?#@\s*]/.test(rest)) return undefined;
  let url: URL;
  try {
    url = new URL(`http://${rest}`);
  } catch {
    return undefined;
  }
  // A port in the entry must be written out, even one equal to the default.
  const written = /:(\d+)$/.exec(rest)?.[1];
  if (url.port === "" && written === undefined && /:[^\]]*$/.test(rest)) return undefined;
  if (wildcard && url.hostname.startsWith("[")) return undefined;
  return written === undefined
    ? { host: url.hostname, wildcard }
    : { host: url.hostname, wildcard, port: String(Number(written)) };
}

/**
 * Whether `grant` covers a request to `url`
 * (spec/cli/command-line.md#r-cli.cap.scope.host): its host matches an
 * entry's host, and its port equals the entry's port when the entry gives
 * one. A URL with no port has its scheme's default port.
 */
export function coversHost(grant: Grant, url: URL): boolean {
  if (grant.kind === "all") return true;
  if (grant.kind === "deny") return false;
  const port = url.port || (url.protocol === "https:" ? "443" : "80");
  return grant.entries.some((text) => {
    const entry = hostEntry(text);
    if (!entry) return false;
    const host = entry.wildcard
      ? url.hostname.endsWith(`.${entry.host}`)
      : url.hostname === entry.host;
    return host && (entry.port === undefined || entry.port === port);
  });
}
