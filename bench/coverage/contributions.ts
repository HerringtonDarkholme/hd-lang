/** Unique covered core lines per set, in O(total set memberships) time/space. */
export function contributions(sets: ReadonlyMap<string, ReadonlySet<string>>): Map<string, number> {
  const owner = new Map<string, string | null>();
  const unique = new Map<string, number>();
  for (const [name, lines] of sets) {
    unique.set(name, 0);
    for (const line of lines) {
      owner.set(line, owner.has(line) ? null : name);
    }
  }
  for (const name of owner.values()) {
    if (name !== null) {
      unique.set(name, (unique.get(name) ?? 0) + 1);
    }
  }
  return unique;
}
