const module = new WebAssembly.Module(WASM);
const host = HDCore.createCoreHost(
  {
    out: (text) => __write(text),
    err: (text) => {
      throw Error("guest stderr: " + text);
    },
  },
  { module },
);
const instance = new WebAssembly.Instance(module, host.imports);
globalThis.__probeDone = false;
globalThis.__probeError = null;
host.run(instance, "hd.init", "hd.poll").then(
  (result) => {
    try {
      if (result.status !== 0 || result.trapped)
        throw Error("guest failed: " + JSON.stringify(result));
    } catch (error) {
      globalThis.__probeError = String(error);
    } finally {
      host.close();
      globalThis.__probeDone = true;
    }
  },
  (error) => {
    globalThis.__probeError = String(error);
    host.close();
    globalThis.__probeDone = true;
  },
);
globalThis.__probeResult = "";
