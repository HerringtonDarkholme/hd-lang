export function rowParameterName(requirement: string): string | undefined {
  return requirement.startsWith("row:") ? requirement.slice("row:".length) : undefined;
}

export function normalizedRequirements(requirements: readonly string[]): readonly string[] {
  return [...new Set(requirements)].sort();
}

export function sameRequirements(left: readonly string[], right: readonly string[]): boolean {
  const normalizedLeft = normalizedRequirements(left);
  const normalizedRight = normalizedRequirements(right);
  return (
    normalizedLeft.length === normalizedRight.length &&
    normalizedLeft.every((requirement, index) => requirement === normalizedRight[index])
  );
}
