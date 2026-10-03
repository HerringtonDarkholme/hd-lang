/**
 * A declaration registry with additional import spellings.
 *
 * Only canonical declarations are stored and iterated. An imported spelling
 * therefore finds the same declaration without duplicating it in later
 * declaration, implementation, or emission passes.
 */
export class ImportBindingMap<V> extends Map<string, V> {
  readonly aliases: ReadonlyMap<string, string>;

  constructor(
    aliases: ReadonlyMap<string, string> = new Map(),
    entries?: readonly (readonly [string, V])[],
  ) {
    super(entries);
    this.aliases = aliases;
  }

  /** The stored spelling reached by `name`; a directly stored name wins. */
  protected storedName(name: string): string {
    if (super.has(name)) return name;
    const seen = new Set<string>();
    let current = name;
    while (!seen.has(current)) {
      seen.add(current);
      const target = this.aliases.get(current);
      if (target === undefined) return current;
      current = target;
      if (super.has(current)) return current;
    }
    return current;
  }

  override has(name: string): boolean {
    return super.has(this.storedName(name));
  }

  override get(name: string): V | undefined {
    return super.get(this.storedName(name));
  }
}
