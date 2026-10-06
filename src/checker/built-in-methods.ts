/** Checker-intrinsic methods by receiver head (10-modules.md#built-in-methods). */
export const BUILT_IN_METHODS = {
  List: { length: "len", iterator: "iter", push: "push" },
  Map: { length: "len", iterator: "iter", get: "get", remove: "remove" },
} as const;

/** The checker-intrinsic method names supported by `receiverHead`. */
export function builtInMethodNames(receiverHead: string | undefined): readonly string[] {
  if (receiverHead === undefined || !(receiverHead in BUILT_IN_METHODS)) return [];
  return Object.values(BUILT_IN_METHODS[receiverHead as keyof typeof BUILT_IN_METHODS]);
}
