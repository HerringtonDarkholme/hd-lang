// Small statistics helpers for the metric scripts.

/**
 * The nearest-rank percentile of `values`: the smallest value such that at
 * least `p` percent of the values are at or below it. `p` is in 0..100.
 * Throws on an empty list, since a metric with no samples has no value.
 */
export function percentile(values: readonly number[], p: number): number {
  if (values.length === 0) throw new Error("percentile of an empty list");
  if (!(p >= 0 && p <= 100)) throw new Error(`percentile ${p} is outside 0..100`);
  const sorted = [...values].sort((a, b) => a - b);
  const rank = Math.max(1, Math.ceil((p / 100) * sorted.length));
  return sorted[rank - 1]!;
}

export const p50 = (values: readonly number[]): number => percentile(values, 50);
export const p95 = (values: readonly number[]): number => percentile(values, 95);

/** The share of `items` for which `test` holds, in 0..1; 0 for an empty list. */
export function share<T>(items: readonly T[], test: (item: T) => boolean): number {
  if (items.length === 0) return 0;
  return items.filter(test).length / items.length;
}

/**
 * The growth exponent between two (size, cost) points: k in cost ~ size^k.
 * 1 is linear, 2 quadratic. A cost at or below zero gives NaN, since the
 * ratio means nothing then.
 */
export function growthExponent(
  small: { readonly size: number; readonly cost: number },
  large: { readonly size: number; readonly cost: number },
): number {
  if (small.cost <= 0 || large.cost <= 0 || large.size <= small.size) return Number.NaN;
  return Math.log(large.cost / small.cost) / Math.log(large.size / small.size);
}

/** Rough token count of a text: four bytes per token, the common rule of thumb. */
export function estimateTokens(text: string): number {
  return Math.ceil(Buffer.byteLength(text, "utf8") / 4);
}
