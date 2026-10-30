// Private SDK schema validation. readPackageContract must run before this helper.
// Module spellings are compiler details; nominal identities and value shapes are not.
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
const bad = (message) => new Error(`SDK server schema: ${message}`);
const identityKey = (i) => JSON.stringify([i.package, i.version, i.trait_path]);
const normalizedIdentity = (identity, sdk) =>
  identity.package === sdk.package && identity.version === sdk.version
    ? { package: "$sdk", version: "$sdk", trait_path: identity.trait_path }
    : { package: identity.package, version: identity.version, trait_path: identity.trait_path };
export function normalizeServerContract(interfaces, sdk) {
  const modules = new Map(interfaces.map((i) => [i.module, i]));
  const key = (i) => identityKey(normalizedIdentity(i.identity, sdk));
  function value(v) {
    const [tag, a, b] = v;
    const fields = (fs) => fs.map(([name, type]) => [name, value(type)]);
    if (tag === 15) {
      const target = modules.get(a);
      if (!target?.handle) {
        throw bad("invalid returned-handle reference");
      }
      return [tag, key(target)];
    }
    if (tag === 8 || tag === 9) {
      return [tag, value(a)];
    }
    if (tag === 10) {
      return [tag, a.map(value)];
    }
    if (tag === 11) {
      return [tag, fields(a)];
    }
    if (tag === 12) {
      return [tag, a.map(([name, fs]) => [name, fields(fs)])];
    }
    if (tag === 13) {
      return [tag, value(a), value(b)];
    }
    return [...v];
  }
  return interfaces
    .map((i) => ({
      key: key(i),
      identity: normalizedIdentity(i.identity, sdk),
      handle: i.handle,
      methods: i.methods
        .map((m) => ({
          name: m.name,
          member: m.member,
          wait: m.wait,
          params: m.params.map(value),
          result: value(m.result),
        }))
        .sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0)),
    }))
    .sort((a, b) => (a.key < b.key ? -1 : a.key > b.key ? 1 : 0));
}
export function validateServerContract(interfaces, expectedSDKIdentity, schema) {
  if (
    !expectedSDKIdentity ||
    typeof expectedSDKIdentity.package !== "string" ||
    !expectedSDKIdentity.package ||
    typeof expectedSDKIdentity.version !== "string" ||
    expectedSDKIdentity.trait_path !== "server.HttpServer"
  ) {
    throw bad("trusted installed SDK identity required");
  }
  const roots = interfaces.filter((i) => !i.handle);
  if (
    roots.length !== 1 ||
    !same(roots[0].identity, {
      package: expectedSDKIdentity.package,
      version: expectedSDKIdentity.version,
      trait_path: "server.HttpServer",
    })
  ) {
    throw bad("HttpServer nominal identity is not the installed SDK");
  }
  if (schema?.root !== "server.HttpServer" || !Array.isArray(schema.interfaces)) {
    throw bad("unsupported expected schema");
  }
  const expected = new Map(schema.interfaces.map((i) => [i.key, i]));
  const actual = normalizeServerContract(interfaces, expectedSDKIdentity);
  for (const i of actual) {
    const target = expected.get(i.key);
    if (!target || target.handle !== i.handle || !same(target.identity, i.identity)) {
      throw bad("unknown interface or returned-handle identity");
    }
    const methods = new Map(target.methods.map((m) => [m.name, m]));
    for (const m of i.methods) {
      if (!same(methods.get(m.name), m)) {
        throw bad(`method shape differs: ${i.identity.trait_path}.${m.name}`);
      }
    }
  }
  return roots[0];
}
export function checkCoverage(interfaces, coverage) {
  for (const [name, names] of Object.entries(coverage)) {
    const actual = interfaces.find((i) => i.identity.trait_path === name);
    if (!actual || !same(actual.methods.map((m) => m.name).sort(), [...names].sort())) {
      throw bad(`reference probe does not cover every declared method of ${name}`);
    }
  }
  if (interfaces.length !== Object.keys(coverage).length) {
    throw bad("reference probe has unexpected interface inventory");
  }
}
