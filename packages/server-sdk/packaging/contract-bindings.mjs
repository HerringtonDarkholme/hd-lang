// Private SDK binding construction over the validated package-host data ABI.
// No application-supplied JavaScript is evaluated or imported here.
// The caller separately pins the installed SDK identity and checks method
// shapes against its trusted provider schema. Decoding data is not authentication.
export function contractBindings(bytes, readPackageContract) {
  if (!(bytes instanceof Uint8Array)) throw new Error("contract must be bytes");
  const checked = Uint8Array.from(bytes);
  const declarations = readPackageContract(checked);
  const interfaces = Object.freeze(
    declarations.map(({ identity, module, handle, selectors }) =>
      Object.freeze({
        identity: Object.freeze({ ...identity }),
        module,
        handle,
        selectors: Object.freeze([...selectors]),
      }),
    ),
  );
  return Object.freeze({
    abiVersion: new DataView(checked.buffer, checked.byteOffset, checked.byteLength).getUint32(
      0,
      true,
    ),
    contract: checked.slice(),
    interfaces,
    bind(runtime, implementations) {
      if (!(implementations instanceof Map))
        throw new Error("provider implementations must be a map keyed by interface module");
      const roots = interfaces.filter((i) => !i.handle);
      for (const key of implementations.keys())
        if (!roots.some((i) => i.module === key))
          throw new Error("implementation names no required package capability");
      const providers = roots.map((i) => {
        if (!implementations.has(i.module))
          throw new Error("missing package provider implementation");
        return Object.freeze({
          identity: i.identity,
          module: i.module,
          contract: checked.slice(),
          implementation: implementations.get(i.module),
        });
      });
      const fields = (fs) => fs.map(([name, type]) => [name, codec(type)]);
      const codec = ([tag, a, b]) => {
        switch (tag) {
          case 0:
            return runtime.unit;
          case 1:
            return runtime.bool;
          case 2:
            return runtime.char;
          case 3:
            return runtime.int(a, b);
          case 4:
            return runtime.f32;
          case 5:
            return runtime.f64;
          case 6:
            return runtime.str;
          case 7:
            return runtime.bytes;
          case 8:
            return runtime.list(codec(a));
          case 9:
            return runtime.option(codec(a));
          case 10:
            return runtime.tuple(a.map(codec));
          case 11:
            return runtime.record(fields(a));
          case 12:
            return runtime.enumeration(a.map(([name, fs]) => [name, fields(fs)]));
          case 13:
            return runtime.result(codec(a), codec(b));
          case 14:
            return runtime.json;
          case 15:
            return runtime.handle(a);
          default:
            throw new Error("unknown validated contract codec tag");
        }
      };
      const shapes = declarations.map(({ module, handle, methods }) => ({
        module,
        handle,
        methods: methods.map(({ name, member, wait, params, result }) => ({
          name,
          member,
          wait,
          params: params.map(codec),
          result: codec(result),
        })),
      }));
      return runtime.bind(checked.slice(), shapes, providers);
    },
  });
}
