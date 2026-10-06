// Did-you-mean suggestions for diagnostics. Every function here runs only
// when a diagnostic is reported, never on a successful check.

/**
 * The Damerau-Levenshtein distance between two names: a transposition
 * counts as one edit, so `psuh` is one edit from `push`.
 */
function nameDistance(left: string, right: string): number {
  const leftChars = [...left];
  const rightChars = [...right];
  const rows = leftChars.length + 1;
  const columns = rightChars.length + 1;
  const distance: number[][] = Array.from({ length: rows }, (_, row) =>
    Array.from({ length: columns }, (_, column) => (row === 0 ? column : column === 0 ? row : 0)),
  );
  for (let row = 1; row < rows; row += 1)
    for (let column = 1; column < columns; column += 1) {
      const cost = leftChars[row - 1] === rightChars[column - 1] ? 0 : 1;
      distance[row]![column] = Math.min(
        distance[row - 1]![column]! + 1,
        distance[row]![column - 1]! + 1,
        distance[row - 1]![column - 1]! + cost,
      );
      if (
        row > 1 &&
        column > 1 &&
        leftChars[row - 1] === rightChars[column - 2] &&
        leftChars[row - 2] === rightChars[column - 1]
      )
        distance[row]![column] = Math.min(
          distance[row]![column]!,
          distance[row - 2]![column - 2]! + 1,
        );
    }
  return distance[leftChars.length]![rightChars.length]!;
}

/**
 * Up to three of `candidates` closest to `name`: at most one edit per three
 * characters of `name`, nearest first, then in name order.
 */
export function closestNames(name: string, candidates: Iterable<string>): string[] {
  const allowed = Math.max(1, Math.floor(Math.max(name.length, 1) / 3));
  const scored: { readonly candidate: string; readonly distance: number }[] = [];
  for (const candidate of new Set(candidates)) {
    if (candidate === name) continue;
    // Length alone bounds the distance from below.
    if (Math.abs(candidate.length - name.length) > allowed) continue;
    const distance = nameDistance(name, candidate);
    if (distance <= allowed) scored.push({ candidate, distance });
  }
  scored.sort(
    (left, right) => left.distance - right.distance || (left.candidate < right.candidate ? -1 : 1),
  );
  return scored.slice(0, 3).map(({ candidate }) => candidate);
}

/** The `; did you mean 'a', 'b'?` clause for `names`, or nothing when there are none. */
export function didYouMean(names: readonly string[]): string {
  return names.length > 0 ? `; did you mean ${names.map((name) => `'${name}'`).join(", ")}?` : "";
}

/**
 * Method names that other languages use, each with the hd names a probe
 * reached for it with. A synonym is suggested only when the receiver has it.
 */
const METHOD_SYNONYMS: ReadonlyMap<string, readonly string[]> = new Map([
  ["append", ["push"]],
  ["add", ["push", "insert"]],
  ["put", ["insert"]],
  ["set", ["insert"]],
  ["length", ["len"]],
  ["size", ["len"]],
  ["has", ["contains", "contains_key"]],
  ["includes", ["contains"]],
  ["has_key", ["contains_key"]],
  ["delete", ["remove"]],
  ["erase", ["remove"]],
  ["sort", ["sorted"]],
  ["strip", ["trim"]],
  ["to_upper", ["upper"]],
  ["to_uppercase", ["upper"]],
  ["to_lower", ["lower"]],
  ["to_lowercase", ["lower"]],
  ["substring", ["slice"]],
  ["concat", ["extend"]],
]);

/**
 * The supported methods of a receiver that `name` most likely meant: its
 * synonyms first, then names within edit distance, at most three.
 */
export function similarMethods(name: string, supported: ReadonlySet<string>): string[] {
  const synonyms = (METHOD_SYNONYMS.get(name) ?? []).filter((method) => supported.has(method));
  return [...new Set([...synonyms, ...closestNames(name, supported)])].slice(0, 3);
}

/**
 * The fix for an unknown variant: the variants it may misspell, or else the
 * enum's variants when there are at most six.
 */
export function variantHint(name: string, variants: readonly string[]): string {
  const close = didYouMean(closestNames(name, variants));
  if (close || variants.length > 6) return close;
  return `; its variants are ${variants.map((variant) => `'${variant}'`).join(", ")}`;
}
