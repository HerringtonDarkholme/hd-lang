// Editing `hd.toml` line by line (spec/cli/command-line.md#r-cli.dep.edit):
// a dependency command changes, adds, or deletes one key's line, so the
// manifest's comments and its other lines stay as they are.

/** A table header line, such as `[dependencies]`, with its name. */
const HEADER = /^\s*\[\s*([^\]]+?)\s*\](?:\s*#.*)?\s*$/;
const ARRAY_HEADER = /^\s*\[\[/;

function keyPattern(key: string): RegExp {
  const escaped = key.replace(/[.*+?^${}()|[\]\\-]/g, "\\$&");
  return new RegExp(`^(\\s*)(?:${escaped}|"${escaped}"|'${escaped}')(\\s*=\\s*)(.*)$`);
}

/** The lines of a table: the header's index, and the index after its last line. */
function tableRange(lines: readonly string[], table: string): [number, number] | undefined {
  const start = lines.findIndex(
    (line) => HEADER.exec(line)?.[1] === table && !ARRAY_HEADER.test(line),
  );
  if (start < 0) return undefined;
  let end = start + 1;
  while (end < lines.length && !/^\s*\[/.test(lines[end]!)) end += 1;
  return [start, end];
}

/** A value's trailing comment, kept when the value changes. */
function trailingComment(value: string): string {
  const match = /^("(?:[^"\\]|\\.)*"|'[^']*'|\{[^}]*\})(\s*#.*)$/.exec(value.trim());
  return match ? match[2]! : "";
}

/**
 * Sets `key` in `table`, `[dependencies]` by default, to the string
 * `value`: its line changes, or a new line follows the table's last key, or
 * a new table ends the file.
 */
export function setDependency(
  text: string,
  key: string,
  value: string,
  table = "dependencies",
): string {
  const lines = text.split("\n");
  const written = `${key} = ${JSON.stringify(value)}`;
  const range = tableRange(lines, table);
  if (!range) {
    const body = text === "" || text.endsWith("\n") ? text : `${text}\n`;
    const gap = body === "" || body.endsWith("\n\n") ? "" : "\n";
    return `${body}${gap}[${table}]\n${written}\n`;
  }
  const [start, end] = range;
  const pattern = keyPattern(key);
  for (let index = start + 1; index < end; index += 1) {
    const match = pattern.exec(lines[index]!);
    if (!match) continue;
    lines[index] = `${match[1]}${written}${trailingComment(match[3]!)}`;
    return lines.join("\n");
  }
  // After the table's last key line, so a blank line before the next table stays.
  let last = start;
  for (let index = start + 1; index < end; index += 1)
    if (/^\s*[^\s#]/.test(lines[index]!)) last = index;
  lines.splice(last + 1, 0, written);
  return lines.join("\n");
}

/**
 * Deletes `key`'s line from `[dependencies]` or `[dev-dependencies]`, or
 * from the `tables` named, and returns undefined when none has a line for it.
 */
export function removeDependency(
  text: string,
  key: string,
  tables: readonly string[] = ["dependencies", "dev-dependencies"],
): string | undefined {
  const lines = text.split("\n");
  const pattern = keyPattern(key);
  for (const table of tables) {
    const range = tableRange(lines, table);
    if (!range) continue;
    for (let index = range[0] + 1; index < range[1]; index += 1) {
      if (!pattern.exec(lines[index]!)) continue;
      lines.splice(index, 1);
      // Do not leave an empty dependency table behind
      // (cli.dep.edit.empty-table). Comments and blank lines are not table
      // entries, and stay where the user wrote them when the header goes.
      const end = range[1] - 1;
      if (!lines.slice(range[0] + 1, end).some((line) => /^\s*[^\s#]/.test(line)))
        lines.splice(range[0], 1);
      return lines.join("\n");
    }
  }
  return undefined;
}

/**
 * Adds the directory `member` to the `members` array of a workspace
 * manifest's `[workspace]` table (spec/cli/command-line.md#r-cli.new.workspace-member),
 * on one line or on many as the array is written, so the other lines stay.
 * Returns undefined when the manifest has no `[workspace]` table.
 */
export function addWorkspaceMember(text: string, member: string): string | undefined {
  return addWorkspaceEntry(text, "members", member);
}

/**
 * Adds `directory` to the `members` or `exclude` array of a workspace
 * manifest, as `addWorkspaceMember` does, for the fix-its of an unlisted
 * member (spec/cli/command-line.md#r-cli.mode.member.unlisted.fix). A missing
 * `members` key opens the table; a missing `exclude` key follows its last key.
 */
export function addWorkspaceEntry(
  text: string,
  key: "members" | "exclude",
  directory: string,
): string | undefined {
  const lines = text.split("\n");
  const range = tableRange(lines, "workspace");
  if (!range) return undefined;
  const [start, end] = range;
  const quoted = JSON.stringify(directory);
  const pattern = keyPattern(key);
  for (let index = start + 1; index < end; index += 1) {
    const match = pattern.exec(lines[index]!);
    if (!match) continue;
    const value = match[3]!;
    // On one line: `members = ["a"]`, maybe with a comment after it.
    const inline = /^\[(.*)\](\s*#.*)?$/.exec(value.trim());
    if (inline) {
      const items = inline[1]!.trim().replace(/,$/, "");
      lines[index] =
        `${match[1]}${key}${match[2]}[${items === "" ? quoted : `${items}, ${quoted}`}]${inline[2] ?? ""}`;
      return lines.join("\n");
    }
    // On many lines: a new item line before the line that closes the array.
    let close = index + 1;
    while (close < end && !/^\s*\]/.test(lines[close]!)) close += 1;
    if (close === end) return undefined;
    let last = close - 1;
    while (last > index && !/^\s*["']/.test(lines[last]!)) last -= 1;
    const indent = last > index ? /^\s*/.exec(lines[last]!)![0] : `${match[1]}    `;
    if (last > index && !/,\s*(#.*)?$/.test(lines[last]!))
      lines[last] = lines[last]!.replace(/^(\s*(?:"(?:[^"\\]|\\.)*"|'[^']*'))/, "$1,");
    lines.splice(close, 0, `${indent}${quoted},`);
    return lines.join("\n");
  }
  let at = start + 1;
  if (key === "exclude")
    for (let index = start + 1; index < end; index += 1)
      if (/^\s*[^\s#]/.test(lines[index]!)) at = index + 1;
  lines.splice(at, 0, `${key} = [${quoted}]`);
  return lines.join("\n");
}
