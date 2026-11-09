// Returns a new map: "z" first, then each entry it was given with its
// value times ten, in the order the given map holds them.
export default {
  render(given: Map<string, number>): Map<string, number> {
    const out = new Map<string, number>([["z", 26]]);
    for (const [key, value] of given) {
      out.set(key, value * 10);
    }
    return out;
  },
};
