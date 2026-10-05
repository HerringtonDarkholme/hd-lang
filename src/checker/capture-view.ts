import type { HirLocal } from "../hir.ts";

/**
 * The names a closure body may capture, as a live view over the enclosing
 * checker's scopes. Every closure used to copy the whole merged map, so N
 * closures over a growing scope cost O(N^2) (closures-shared-var); the
 * view reads the same scopes with the same precedence in O(depth) per
 * lookup instead, and needs no invalidation as scopes grow.
 *
 * Structural ReadonlyMap: iteration yields plain iterables, which satisfy
 * every consumer (has/get/values spreads). The cast at the single
 * construction site bridges the library MapIterator types.
 */
class CaptureView {
  private readonly parent: ReadonlyMap<string, HirLocal>;
  private readonly scopes: readonly Map<string, HirLocal>[];
  private readonly providers: readonly Map<string, HirLocal>[];

  constructor(
    parent: ReadonlyMap<string, HirLocal>,
    scopes: readonly Map<string, HirLocal>[],
    providers: readonly Map<string, HirLocal>[],
  ) {
    this.parent = parent;
    this.scopes = scopes;
    this.providers = providers;
  }

  get(name: string): HirLocal | undefined {
    let found: HirLocal | undefined;
    for (const scope of this.providers)
      for (const local of scope.values()) if (local.name === name) found = local;
    if (found) return found;
    for (let index = this.scopes.length - 1; index >= 0; index -= 1) {
      const local = this.scopes[index]!.get(name);
      if (local) return local;
    }
    return this.parent.get(name);
  }

  has(name: string): boolean {
    return this.get(name) !== undefined;
  }

  private *names(): IterableIterator<string> {
    const seen = new Set<string>();
    const consider = function* (names: Iterable<string>): IterableIterator<string> {
      for (const name of names)
        if (!seen.has(name)) {
          seen.add(name);
          yield name;
        }
    };
    yield* consider(this.parent.keys());
    for (const scope of this.scopes) yield* consider(scope.keys());
    for (const scope of this.providers) {
      const names: string[] = [];
      for (const local of scope.values()) names.push(local.name);
      yield* consider(names);
    }
  }

  keys(): IterableIterator<string> {
    return this.names();
  }

  *values(): IterableIterator<HirLocal> {
    for (const name of this.names()) yield this.get(name)!;
  }

  *entries(): IterableIterator<[string, HirLocal]> {
    for (const name of this.names()) yield [name, this.get(name)!];
  }

  get size(): number {
    return [...this.names()].length;
  }

  forEach(
    callback: (value: HirLocal, key: string, map: ReadonlyMap<string, HirLocal>) => void,
    thisArg?: unknown,
  ): void {
    const self = this as unknown as ReadonlyMap<string, HirLocal>;
    for (const [key, value] of this.entries()) callback.call(thisArg, value, key, self);
  }

  [Symbol.iterator](): IterableIterator<[string, HirLocal]> {
    return this.entries();
  }
}

export function captureSources(
  parent: ReadonlyMap<string, HirLocal>,
  scopes: readonly Map<string, HirLocal>[],
  providers: readonly Map<string, HirLocal>[],
): ReadonlyMap<string, HirLocal> {
  return new CaptureView(parent, scopes, providers) as unknown as ReadonlyMap<string, HirLocal>;
}

/** Whether local is among captures: a name lookup, with a scan fallback. */
export function isCaptureSource(captures: ReadonlyMap<string, HirLocal>, local: HirLocal): boolean {
  return captures.get(local.name) === local || [...captures.values()].includes(local);
}
