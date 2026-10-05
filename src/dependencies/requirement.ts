// Dependency requirements (spec/lang/10-modules.md#dependency-requirements):
// a key's source name, a host path and the repository it names
// (spec/lang/10-modules.md#host-paths), and versions with their tags and
// compatibility lines (spec/lang/10-modules.md#versions). A requirement that
// breaks one of these rules is `invalid-requirement`
// (spec/cli/command-line.md#r-cli.dep.invalid).

import { KEYWORDS } from "../lexer.ts";
import type { TomlValue } from "../manifest.ts";

/** A SemVer 2.0.0 version without a leading `v`, such as `2.1.0` or `1.0.0-rc.1`. */
export interface Version {
  readonly major: number;
  readonly minor: number;
  readonly patch: number;
  /** The pre-release identifiers; empty for a release. */
  readonly pre: readonly string[];
  /** The version as written. */
  readonly text: string;
}

const NUMBER = "(0|[1-9]\\d*)";
const IDENTIFIER = "(?:0|[1-9]\\d*|\\d*[A-Za-z-][0-9A-Za-z-]*)";
const VERSION = new RegExp(
  `^${NUMBER}\\.${NUMBER}\\.${NUMBER}(?:-(${IDENTIFIER}(?:\\.${IDENTIFIER})*))?$`,
);

/** A version, or undefined when `text` is not one. */
export function parseVersion(text: string): Version | undefined {
  const match = VERSION.exec(text);
  if (!match) return undefined;
  const [major, minor, patch] = [match[1], match[2], match[3]].map(Number) as [
    number,
    number,
    number,
  ];
  if (![major, minor, patch].every(Number.isSafeInteger)) return undefined;
  return { major, minor, patch, pre: match[4]?.split(".") ?? [], text };
}

/** SemVer 2.0.0 precedence (spec/lang/10-modules.md#r-module.version.order). */
export function compareVersions(left: Version, right: Version): number {
  for (const part of ["major", "minor", "patch"] as const)
    if (left[part] !== right[part]) return left[part] - right[part];
  if (left.pre.length === 0 || right.pre.length === 0)
    return right.pre.length - left.pre.length || 0;
  for (let index = 0; index < Math.max(left.pre.length, right.pre.length); index += 1) {
    const [a, b] = [left.pre[index], right.pre[index]];
    if (a === undefined) return -1;
    if (b === undefined) return 1;
    if (a === b) continue;
    const [numericA, numericB] = [/^\d+$/.test(a), /^\d+$/.test(b)];
    if (numericA && numericB) return Number(a) - Number(b);
    if (numericA !== numericB) return numericA ? -1 : 1;
    return a < b ? -1 : 1;
  }
  return 0;
}

/** The compatibility line of a version (spec/lang/10-modules.md#r-module.version.line). */
export function compatibilityLine(version: Version): string {
  return version.major >= 1 ? `${version.major}` : `0.${version.minor}`;
}

/** The commit a pseudo-version names: its UTC time and its hash's first 12 digits. */
export interface PseudoCommit {
  /** `yyyymmddhhmmss` (spec/lang/10-modules.md#r-module.version.pseudo.commit). */
  readonly time: string;
  /** The first 12 lowercase hexadecimal digits of the commit hash. */
  readonly hash: string;
}

/**
 * The commit a pseudo-version names, or undefined when `version` has none
 * of the three forms of spec/lang/10-modules.md#r-module.version.pseudo:
 * `0.0.0-TIME-HASH`, `X.Y.Z-0.TIME-HASH`, and `X.Y.Z-PRE.0.TIME-HASH`.
 */
export function pseudoCommit(version: Version): PseudoCommit | undefined {
  const match = /^(\d{14})-([0-9a-f]{12})$/.exec(version.pre.at(-1) ?? "");
  if (!match) return undefined;
  const base = version.pre.slice(0, -1);
  const noTag =
    base.length === 0 && version.major === 0 && version.minor === 0 && version.patch === 0;
  if (!noTag && base.at(-1) !== "0") return undefined;
  // `X.Y.(Z+1)-0.TIME-HASH` follows a release, so its patch is at least 1.
  if (base.length === 1 && version.patch === 0) return undefined;
  return { time: match[1]!, hash: match[2]! };
}

/**
 * The version of a pseudo-version's base tag
 * (spec/cli/command-line.md#r-cli.dep.pseudo.base): `X.Y.Z` for
 * `X.Y.(Z+1)-0.TIME-HASH`, `X.Y.Z-PRE` for `X.Y.Z-PRE.0.TIME-HASH`, and
 * undefined for `0.0.0-TIME-HASH`, which has none.
 */
export function pseudoBase(version: Version): Version | undefined {
  const base = version.pre.slice(0, -2);
  const { major, minor, patch } = version;
  if (version.pre.length === 1) return undefined;
  if (base.length === 0) {
    const text = `${major}.${minor}.${patch - 1}`;
    return { major, minor, patch: patch - 1, pre: [], text };
  }
  return { major, minor, patch, pre: base, text: `${major}.${minor}.${patch}-${base.join(".")}` };
}

/** Whether a version is a pseudo-version (spec/lang/10-modules.md#r-module.version.pseudo). */
export function isPseudoVersion(version: Version): boolean {
  return pseudoCommit(version) !== undefined;
}

/** A host path and the repository it names (spec/lang/10-modules.md#host-paths). */
export interface HostPath {
  /** The host path as written, such as `github.com/acme/tools/lint`. */
  readonly path: string;
  /** The repository part, such as `github.com/acme/tools`. */
  readonly repository: string;
  /** The package's directory in the repository, such as `lint`; `""` at its root. */
  readonly subdirectory: string;
}

const SEGMENT = /^[A-Za-z0-9._~-]+$/;

/** A host path, or the message of why `path` is not one. */
export function parseHostPath(path: string): HostPath | string {
  const segments = path.split("/");
  if (segments.some((segment) => !SEGMENT.test(segment) || segment === "." || segment === ".."))
    return `'${path}' is not a host path: each segment is letters, digits, '.', '_', '~', or '-', separated by single '/'`;
  const host = segments[0]!;
  if (!host.includes(".") || host.startsWith(".") || host.endsWith("."))
    return `'${path}' does not start with a host name, such as github.com`;
  let parts: number;
  if (host === "github.com") {
    // On github.com, the host, an owner, and a repository name the
    // repository (spec/lang/10-modules.md#r-module.repo.github).
    if (segments.length < 3)
      return `'${path}' names no repository: on github.com, a host path is github.com/OWNER/REPOSITORY`;
    parts = 3;
  } else {
    // Elsewhere the repository part ends with a segment that ends in `.git`
    // (spec/lang/10-modules.md#r-module.repo.git-suffix).
    const end = segments.findIndex((segment, index) => index > 0 && segment.endsWith(".git"));
    if (end < 0)
      return `'${path}' names no repository: on a host other than github.com, the repository part ends with a segment that ends in .git, as git.example.com/shop/billing.git`;
    parts = end + 1;
  }
  const subdirectory = segments.slice(parts);
  // No major-version suffix such as /v2 (spec/lang/10-modules.md#r-module.dep.no-major-suffix).
  if (subdirectory.some((segment) => /^v\d+$/.test(segment)))
    return `'${path}' has a major-version suffix; a host path carries none, so write a second key for a second line`;
  return {
    path,
    repository: segments.slice(0, parts).join("/"),
    subdirectory: subdirectory.join("/"),
  };
}

/** The URL `hd` fetches a repository from (spec/cli/command-line.md#r-cli.dep.repo-url). */
export function repositoryUrl(host: HostPath): string {
  return `https://${host.repository}`;
}

/** A version's tag (spec/lang/10-modules.md#r-module.version.tag-prefix). */
export function tagName(host: HostPath, version: Version): string {
  return host.subdirectory === "" ? `v${version.text}` : `${host.subdirectory}/v${version.text}`;
}

/** The version a tag names for a package, or undefined when it names none. */
export function tagVersion(host: HostPath, tag: string): Version | undefined {
  const prefix = host.subdirectory === "" ? "v" : `${host.subdirectory}/v`;
  return tag.startsWith(prefix) ? parseVersion(tag.slice(prefix.length)) : undefined;
}

/** A checked requirement value. */
export type Requirement =
  | { readonly kind: "host"; readonly host: HostPath; readonly version: Version }
  | { readonly kind: "path"; readonly path: string; readonly version?: Version };

/** A version in a requirement, or the message of why `text` is none. */
function requiredVersion(text: string): Version | string {
  const version = parseVersion(text);
  if (version) return version;
  if (text.startsWith("v") && parseVersion(text.slice(1)))
    return `the version '${text}' has a leading 'v'; write ${text.slice(1)}, the tag v${text.slice(1)} without it`;
  return `'${text}' is not a version: write MAJOR.MINOR.PATCH, as 2.1.0`;
}

/** A requirement `PATH@VERSION`, or the message of why `text` is none. */
export function parseHostRequirement(
  text: string,
): Extract<Requirement, { readonly kind: "host" }> | string {
  const at = text.lastIndexOf("@");
  if (at < 0) return `'${text}' has no version: write PATH@VERSION, as github.com/acme/json@2.1.0`;
  const host = parseHostPath(text.slice(0, at));
  if (typeof host === "string") return host;
  const version = requiredVersion(text.slice(at + 1));
  if (typeof version === "string") return version;
  return { kind: "host", host, version };
}

/**
 * A dependency key's value: `"PATH@VERSION"`, or a path requirement
 * `{ path = "DIR" }` with an optional version
 * (spec/lang/10-modules.md#r-module.dep.requirement-form); or the message
 * of why it is neither.
 */
export function parseRequirement(value: TomlValue): Requirement | string {
  if (typeof value === "string") return parseHostRequirement(value);
  if (typeof value !== "object" || value === null || Array.isArray(value))
    return 'a requirement is a string "PATH@VERSION" or a table { path = "DIR" }';
  const table = value as Readonly<Record<string, TomlValue>>;
  const extra = Object.keys(table).find((key) => key !== "path" && key !== "version");
  if (extra !== undefined)
    return `a path requirement has only 'path' and 'version', not '${extra}'`;
  if (typeof table.path !== "string" || table.path === "")
    return 'a path requirement needs path = "DIR", the other package\'s directory';
  if (table.version === undefined) return { kind: "path", path: table.path };
  if (typeof table.version !== "string") return "a path requirement's version is a string";
  const version = requiredVersion(table.version);
  if (typeof version === "string") return version;
  return { kind: "path", path: table.path, version };
}

/** A dependency key's name in source (spec/lang/10-modules.md#r-module.dep.key-name). */
export function sourceName(key: string): string {
  return key.replaceAll("-", "_");
}

/** The message of why `key` cannot be a dependency key, or undefined when it can. */
export function invalidKey(key: string): string | undefined {
  const name = sourceName(key);
  if (!/^[\p{ID_Start}_][\p{ID_Continue}]*$/u.test(name) || KEYWORDS.has(name))
    return `the dependency key '${key}' is not a name source can write as dep.${name}: use letters, digits, '_', and '-', starting with a letter`;
  return undefined;
}
