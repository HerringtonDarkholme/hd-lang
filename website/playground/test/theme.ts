import assert from "node:assert/strict";

import type { Locator } from "playwright-core";

/** Resolve a palette token in the expected scheme, including light-dark(). */
export async function assertBackgroundToken(
  locator: Locator,
  token: string,
  scheme: "light" | "dark",
): Promise<string> {
  const { actual, expected, value } = await locator.evaluate(
    (node, { token, scheme }) => {
      const probe = node.ownerDocument.createElement("span");
      probe.style.display = "none";
      probe.style.colorScheme = scheme;
      probe.style.color = `var(${token})`;
      node.append(probe);
      try {
        const style = getComputedStyle(node);
        return {
          actual: style.backgroundColor,
          expected: getComputedStyle(probe).color,
          value: style.getPropertyValue(token).trim(),
        };
      } finally {
        probe.remove();
      }
    },
    { token, scheme },
  );
  assert.notEqual(value, "", `${token} is declared`);
  assert.equal(actual, expected, `${scheme}: background follows ${token}`);
  return actual;
}
