import assert from "node:assert/strict";

interface Identity {
  package: string;
  version: string;
  trait_path: string;
}

interface IdentityContext {
  interfaceIdentity: Identity;
  pending?<T>(ready: () => boolean, done: () => T): unknown;
}

let sourceIdentity: Identity | undefined;
function inspect(context: IdentityContext, trait: string): void {
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
  if (trait === "main.Source") {
    sourceIdentity = { ...identity };
  } else {
    assert.ok(sourceIdentity);
    assert.equal(identity.package, sourceIdentity.package);
    assert.equal(identity.version, sourceIdentity.version);
  }
}
export default {
  inspect(context: IdentityContext) {
    inspect(context, "main.Source");
    return "source";
  },
  child(context: IdentityContext) {
    inspect(context, "main.Source");
    return {
      inspect(context: IdentityContext) {
        inspect(context, "main.Child");
        assert.ok(context.pending);
        return context.pending(
          () => true,
          () => "child",
        );
      },
    };
  },
};
