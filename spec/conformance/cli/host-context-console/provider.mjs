import assert from "node:assert/strict";

export default {
  inspect(context) {
    const policy = context.console;
    assert.equal(Object.isFrozen(policy), true);
    assert.equal(Reflect.set(context, "console", {}), false);
    assert.equal(Reflect.defineProperty(context, "console", { value: {} }), false);
    assert.equal(Reflect.deleteProperty(context, "console"), false);
    assert.equal(
      Reflect.set(policy, "allows", () => true),
      false,
    );
    assert.equal(context.console, policy);
    process.stdout.write(String(policy.allows()) + "\n");
  },
};
