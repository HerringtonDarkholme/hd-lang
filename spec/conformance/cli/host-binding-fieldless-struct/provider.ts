import assert from "node:assert/strict";

export default {
  render(alone: object, wrap: object, pick: object, many: object[]) {
    assert.deepEqual(alone, {});
    assert.deepEqual(wrap, { before: 1, e: {}, after: "z" });
    assert.deepEqual(pick, { tag: "One", fields: { e: {}, n: 7 } });
    assert.deepEqual(many, [{}, {}]);
    return [{}, { before: 2, e: {}, after: "y" }, pick, [{}, {}, {}]];
  },
};
