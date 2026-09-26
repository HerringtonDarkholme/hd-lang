// Contract-preserving delta minimizer. The predicate decides whether a
// candidate still shows the target signature; it is built from the same
// command contract and oracles as the fuzzer, so minimization is portable.
import { joinLines, splitLines } from "./mutate.ts";

export type Predicate = (source: string) => Promise<boolean>;

async function ddmin<T>(
  items: readonly T[],
  build: (kept: readonly T[]) => string,
  keeps: Predicate,
  budget: { remaining: number },
): Promise<T[]> {
  let current = [...items];
  let chunks = 2;
  while (current.length >= 2 && budget.remaining > 0) {
    const size = Math.ceil(current.length / chunks);
    let reduced = false;
    for (let start = 0; start < current.length && budget.remaining > 0; start += size) {
      const complement = [...current.slice(0, start), ...current.slice(start + size)];
      if (complement.length === 0) continue;
      budget.remaining -= 1;
      if (await keeps(build(complement))) {
        current = complement;
        chunks = Math.max(chunks - 1, 2);
        reduced = true;
        break;
      }
    }
    if (!reduced) {
      if (chunks >= current.length) break;
      chunks = Math.min(current.length, chunks * 2);
    }
  }
  return current;
}

const closers: Readonly<Record<string, string>> = { "(": ")", "[": "]", "{": "}" };

async function emptyGroups(
  source: string,
  keeps: Predicate,
  budget: { remaining: number },
): Promise<string> {
  let best = source;
  for (let changed = true; changed && budget.remaining > 0;) {
    changed = false;
    const lines = splitLines(best);
    const groups: Array<{ row: number; start: number; end: number }> = [];
    for (const [row, line] of lines.entries()) {
      const stack: number[] = [];
      for (const [index, token] of line.tokens.entries()) {
        if (closers[token]) stack.push(index);
        else if (Object.values(closers).includes(token) && stack.length) {
          const start = stack.pop()!;
          if (index - start > 1) groups.push({ end: index, row, start });
        }
      }
    }
    groups.sort((left, right) => right.end - right.start - (left.end - left.start));
    for (const group of groups) {
      if (budget.remaining <= 0) break;
      const line = lines[group.row]!;
      const tokens = [...line.tokens.slice(0, group.start + 1), ...line.tokens.slice(group.end)];
      const candidate = normalize(
        joinLines(
          lines.map((other, index) =>
            index === group.row ? { ...other, original: undefined, tokens } : other,
          ),
        ),
      );
      budget.remaining -= 1;
      if (await keeps(candidate)) {
        best = candidate;
        changed = true;
        break;
      }
    }
  }
  return best;
}

function normalize(source: string): string {
  return `${source.replace(/\n*$/, "")}\n`;
}

/**
 * Minimizes `source` while `keeps` holds. Passes: drop lines, drop tokens
 * within each remaining line, then dedent-free cleanup of comments.
 * `maxTests` bounds the number of predicate calls.
 */
export async function minimize(source: string, keeps: Predicate, maxTests = 400): Promise<string> {
  const budget = { remaining: maxTests };
  let best = normalize(source);
  // Pass 0: remove comments if that keeps the signature.
  const uncommented = normalize(
    joinLines(splitLines(best).map((line) => ({ ...line, comment: "" }))).replaceAll(
      /\n{2,}/g,
      "\n",
    ),
  );
  if (uncommented !== best && (await keeps(uncommented))) best = uncommented;
  // Pass 1: lines.
  const lines = best.split("\n").filter((line, index, all) => line || index < all.length - 1);
  const keptLines = await ddmin(lines, (kept) => normalize(kept.join("\n")), keeps, budget);
  best = normalize(keptLines.join("\n"));
  // Pass 2: empty balanced bracket groups, largest first ("f(a, (b))" -> "f()").
  best = await emptyGroups(best, keeps, budget);
  // Pass 3: tokens within each line, one line at a time.
  const parsed = splitLines(best);
  for (let row = 0; row < parsed.length && budget.remaining > 0; row += 1) {
    const line = parsed[row]!;
    if (line.tokens.length < 2) continue;
    const render = (tokens: readonly string[]): string =>
      normalize(
        joinLines(
          parsed.map((other, index) =>
            index === row ? { ...other, original: undefined, tokens: [...tokens] } : other,
          ),
        ),
      );
    const keptTokens = await ddmin(line.tokens, render, keeps, budget);
    if (keptTokens.length < line.tokens.length) {
      parsed[row] = { ...line, original: undefined, tokens: keptTokens };
      best = normalize(joinLines(parsed));
    }
  }
  return best;
}
