// Capability grants (spec/cli/command-line.md#capability-grants): what a
// program's host providers may touch at run time. This prototype reads the
// `--cap NAME=VALUE` flags of `hd FILE` and `hd run`, and enforces the
// `Http` grant only (src/commands/http-host.ts). The `[capabilities]` table,
// the other traits' checks, and the startup refusal of a total deny are
// listed in src/KNOWN_ISSUES.md (CAPS).

import { UsageError } from "../cli-args.ts";

/** One trait's grant: no limit, a total deny, or the scope entries it covers. */
export type Grant =
  | { readonly kind: "all" }
  | { readonly kind: "deny"; readonly setting: string }
  | { readonly kind: "list"; readonly entries: readonly string[] };

/** The grant of each trait that a setting names, by trait name. */
export type CapabilityGrants = ReadonlyMap<string, Grant>;

/**
 * The capability keys and whether each one takes scope entries
 * (spec/cli/command-line.md#r-cli.cap.table.keys).
 */
const SCOPED: Readonly<Record<string, boolean>> = {
  FsRead: true,
  FsWrite: true,
  Http: true,
  Net: true,
  Env: true,
  Process: true,
  Sys: true,
  Console: false,
  ConsoleInput: false,
  Clock: false,
  Random: false,
  Args: false,
};

/** The traits whose grant this prototype enforces. */
const ENFORCED = new Set(["Http"]);

/** No limit: the grant of a trait that no setting names (cli.cap.order.default). */
export const UNLIMITED: Grant = { kind: "all" };

/**
 * The grants that `--cap NAME=VALUE` flags give, by the precedence of
 * spec/cli/command-line.md#grant-precedence: a `false` denies the trait,
 * else a `true` grants it with no limit, else the trait covers every entry
 * that the flags list. Throws a {@link UsageError} for a flag that names an
 * unknown trait (cli.cap.flag.unknown), or that gives a list to a trait
 * without scope entries (cli.cap.flag.unscoped).
 */
export function capabilityFlags(values: readonly string[], command: string): CapabilityGrants {
  const settings = new Map<string, { deny?: string; all?: boolean; entries: string[] }>();
  for (const value of values) {
    const equals = value.indexOf("=");
    const name = equals === -1 ? value : value.slice(0, equals);
    if (!(name in SCOPED))
      throw new UsageError(
        `${command}: --cap ${name}: unknown capability; use one of ${Object.keys(SCOPED).join(", ")}`,
      );
    if (equals === -1)
      throw new UsageError(`${command}: --cap ${name} needs a value: true, false, or a list`);
    const text = value.slice(equals + 1);
    const list = text !== "true" && text !== "false";
    if (list && !SCOPED[name])
      throw new UsageError(
        `${command}: --cap ${name} takes only true or false, since ${name} has no scope entries`,
      );
    if (!ENFORCED.has(name))
      throw new UsageError(
        `${command}: --cap ${name}: this prototype checks only the Http grant so far`,
      );
    const setting = settings.get(name) ?? { entries: [] };
    settings.set(name, setting);
    if (text === "false") setting.deny ??= `--cap ${value}`;
    else if (text === "true") setting.all = true;
    else {
      const entries = text.split(",");
      for (const entry of entries) checkEntry(name, entry, command);
      setting.entries.push(...entries);
    }
  }
  const grants = new Map<string, Grant>();
  for (const [name, setting] of settings)
    grants.set(
      name,
      setting.deny !== undefined
        ? { kind: "deny", setting: setting.deny }
        : setting.all
          ? UNLIMITED
          : { kind: "list", entries: setting.entries },
    );
  return grants;
}

function checkEntry(name: string, entry: string, command: string): void {
  if (name === "Http" && !hostEntry(entry))
    throw new UsageError(
      `${command}: --cap Http=${entry}: an entry is a host name, an IPv4 address, or a bracketed IPv6 address, with an optional :port, as in api.example.com or *.example.com`,
    );
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
