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
 * returns undefined when neither table has a line for it.
 */
export function removeDependency(text: string, key: string): string | undefined {
  const lines = text.split("\n");
  const pattern = keyPattern(key);
  for (const table of ["dependencies", "dev-dependencies"]) {
    const range = tableRange(lines, table);
    if (!range) continue;
    for (let index = range[0] + 1; index < range[1]; index += 1) {
      if (!pattern.exec(lines[index]!)) continue;
      lines.splice(index, 1);
      return lines.join("\n");
    }
  }
  return undefined;
}
