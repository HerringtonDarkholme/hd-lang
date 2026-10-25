import assert from "node:assert/strict";

let sourceIdentity;
function inspect(context, trait) {
  const identity = context.interfaceIdentity;
  assert.deepEqual(Object.keys(identity).sort(), ["package", "trait_path", "version"]);
  assert.equal(typeof identity.package, "string");
  assert.equal(typeof identity.version, "string");
  assert.equal(identity.trait_path, trait);
  assert.equal(Object.isFrozen(identity), true);
  assert.equal(Reflect.set(identity, "package", "forged"), false);
  assert.equal(Reflect.set(context, "interfaceIdentity", {}), false);
  assert.equal(Reflect.defineProperty(context, "interfaceIdentity", { value: {} }), false);
  assert.equal(Reflect.deleteProperty(context, "interfaceIdentity"), false);
  if (trait === "main.Source") sourceIdentity = { ...identity };
  else {
    assert.equal(identity.package, sourceIdentity.package);
    assert.equal(identity.version, sourceIdentity.version);
  }
}
export default {
  inspect(context) {
    inspect(context, "main.Source");
    return "source";
  },
  child(context) {
    inspect(context, "main.Source");
    return {
      inspect(context) {
        inspect(context, "main.Child");
        return context.pending(
          () => true,
          () => "child",
        );
      },
    };
  },
};
