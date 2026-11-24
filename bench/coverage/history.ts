// Differential coverage follows LCOV genhtml's DiffMap: map unchanged source
// chunks before comparing execution. Only uncovered intervals are retained.
export type Range = { file: string; first: number; last: number };
type Chunk = { first: number; last: number; offset: number };
type Change = { file: string | undefined; chunks: Chunk[] };

function hunk(line: string): { first: number; after: number; newAfter: number } {
  const match = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/.exec(line);
  if (!match) {
    throw new Error(`invalid coverage diff hunk: ${line}`);
  }
  const [, oldStart, oldCount = "1", newStart, newCount = "1"] = match;
  const first = Number(oldStart) + (Number(oldCount) === 0 ? 1 : 0);
  return {
    first,
    after: first + Number(oldCount),
    newAfter: Number(newStart) + (Number(newCount) === 0 ? 1 : 0) + Number(newCount),
  };
}

/** Parse zero-context git hunks, including renames and empty-side edits. */
function changes(diff: string): Map<string, Change> {
  const files = new Map<string, Change>();
  let old: string | undefined;
  let change: Change | undefined;
  let cursor = 1;
  let offset = 0;
  const finish = () => {
    if (change) {
      change.chunks.push({ first: cursor, last: Infinity, offset });
    }
  };
  for (const line of diff.split("\n")) {
    if (line.startsWith("diff --git ")) {
      finish();
      old = undefined;
      change = undefined;
      cursor = 1;
      offset = 0;
    } else if (line.startsWith("rename from ")) {
      old = line.slice(12);
    } else if (line.startsWith("rename to ") && old) {
      change = { file: line.slice(10), chunks: [] };
      files.set(old, change);
    } else if (line.startsWith("--- ")) {
      old = line.startsWith("--- a/") ? line.slice(6) : undefined;
    } else if (line.startsWith("+++ ") && old) {
      change = { file: line.startsWith("+++ b/") ? line.slice(6) : undefined, chunks: [] };
      files.set(old, change);
    } else if (line.startsWith("@@ ") && change) {
      const next = hunk(line);
      if (cursor < next.first) {
        change.chunks.push({ first: cursor, last: next.first - 1, offset });
      }
      cursor = next.after;
      offset = next.newAfter - cursor;
    }
  }
  finish();
  return files;
}

function grouped(ranges: readonly Range[]): Map<string, Range[]> {
  const files = new Map<string, Range[]>();
  for (const range of ranges) {
    const rows = files.get(range.file) ?? [];
    rows.push(range);
    files.set(range.file, rows);
  }
  for (const rows of files.values()) {
    rows.sort((left, right) => left.first - right.first);
  }
  return files;
}

/**
 * Cost: sort R ranges, then merge sorted ranges/chunks in O(R + H + output).
 * A billion-line interval stays one interval; no per-line or per-hunk rewalk.
 */
export function mapUncovered(previous: readonly Range[], diff: string): Range[] {
  const edits = changes(diff);
  const mapped: Range[] = [];
  for (const [file, ranges] of grouped(previous)) {
    const change = edits.get(file);
    if (!change) {
      for (const range of ranges) {
        mapped.push(range);
      }
      continue;
    }
    if (!change.file) {
      continue;
    }
    let at = 0;
    for (const range of ranges) {
      while (at < change.chunks.length && change.chunks[at].last < range.first) {
        at++;
      }
      for (let i = at; i < change.chunks.length; i++) {
        const chunk = change.chunks[i];
        if (chunk.first > range.last) {
          break;
        }
        const first = Math.max(range.first, chunk.first);
        const last = Math.min(range.last, chunk.last);
        if (first <= last) {
          mapped.push({
            file: change.file,
            first: first + chunk.offset,
            last: last + chunk.offset,
          });
        }
        if (chunk.last >= range.last) {
          break;
        }
        at = i + 1;
      }
    }
  }
  return mapped;
}

/** Current gaps minus mapped old gaps; unchanged coverage losses also fail. */
export function newUncovered(current: readonly Range[], previous: readonly Range[]): Range[] {
  const before = grouped(previous);
  const fresh: Range[] = [];
  for (const [file, ranges] of grouped(current)) {
    const old = before.get(file) ?? [];
    let at = 0;
    for (const range of ranges) {
      let first = range.first;
      while (at < old.length && old[at].last < first) {
        at++;
      }
      for (let i = at; i < old.length && old[i].first <= range.last; i++) {
        if (first < old[i].first) {
          fresh.push({ file, first, last: old[i].first - 1 });
        }
        first = Math.max(first, old[i].last + 1);
        if (first > range.last) {
          break;
        }
        at = i + 1;
      }
      if (first <= range.last) {
        fresh.push({ file, first, last: range.last });
      }
    }
  }
  return fresh;
}
