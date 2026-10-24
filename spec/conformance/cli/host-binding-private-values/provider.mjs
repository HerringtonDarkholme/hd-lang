import assert from "node:assert/strict";
export default {
  secret(value) {
    assert.equal(value.large, 18446744073709551615n);
    assert.equal(value.signed, -9223372036854775808n);
    assert.equal(value.ratio, 1);
    assert.equal(typeof value.ratio, "number");
    assert.ok(Object.is(value.zero, -0));
    assert.deepEqual(value.children, ["nested"]);
    return { ...value, large: value.large - 1n, signed: value.signed + 1n };
  },
  custom(value) {
    assert.equal(value, "custom:private");
    return "custom reply";
  },
};
