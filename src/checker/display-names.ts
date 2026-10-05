import { displayType } from "../types.ts";
import { isStandardModulePath } from "./standard-uses.ts";

/**
 * Renders a value or type name the way the user can write it, for
 * diagnostics. A hidden std rename (`__std_testing_arbitrary_With`) reads
 * as the path through the modules: `arbitrary.With` when `imports` names
 * `std.testing.arbitrary`, else the qualified `std.testing.arbitrary.With`.
 * A hidden package rename reads through the `pkg.` imports the same way.
 * Anything else passes through; `displayType` keeps handling full types.
 */
export function displayName(
  name: string,
  imports: ReadonlyMap<string, string> = new Map(),
): string {
  if (name.startsWith("__std_")) return displayStdName(name.slice("__std_".length), imports);
  if (name.startsWith("__pkg_")) return displayPackageName(name, imports);
  return name;
}

/** The user-facing path of a hidden std rename's module and member, if it names one. */
function displayStdName(rest: string, imports: ReadonlyMap<string, string>): string {
  // Longest module first: `__std_time_parse_rfc3339` is `time` plus `parse_rfc3339`.
  const parts = rest.split("_");
  for (let end = parts.length - 1; end >= 1; end -= 1) {
    const module = parts.slice(0, end).join(".");
    if (!isStandardModulePath(module)) continue;
    const member = parts.slice(end).join("_");
    if (member === "") continue;
    for (const [local, full] of imports) {
      if (full === `std.${module}`) return `${local}.${member}`;
    }
    return `std.${module}.${member}`;
  }
  return displayType(`__std_${rest}`);
}

/** The user-facing path of a hidden package rename, through the `pkg.` imports that name it. */
function displayPackageName(name: string, imports: ReadonlyMap<string, string>): string {
  let best: string | undefined;
  let bestLength = -1;
  for (const [local, full] of imports) {
    if (!full.startsWith("pkg.")) continue;
    const identity = full.slice("pkg.".length);
    const prefix = identity === "" ? "__pkg_" : `__pkg_${identity.replaceAll(".", "_")}_`;
    if (prefix.length > bestLength && name.startsWith(prefix) && name.length > prefix.length) {
      best = `${local}.${name.slice(prefix.length)}`;
      bestLength = prefix.length;
    }
  }
  return best ?? displayType(name);
}
