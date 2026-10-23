// A tiny module: import () -> i32, export f() -> i32 calling that import.
import wasm from "./feature.wasm";
export default {
  async fetch() {
    const available = {
      Suspending: typeof WebAssembly.Suspending,
      promising: typeof WebAssembly.promising,
      SharedArrayBuffer: typeof SharedArrayBuffer,
    };
    if (available.Suspending === "function" && available.promising === "function") {
      const instance = new WebAssembly.Instance(wasm, {
        m: {
          f: new WebAssembly.Suspending(async () => {
            await Promise.resolve();
            return 7;
          }),
        },
      });
      available.result = await WebAssembly.promising(instance.exports.f)();
    }
    return Response.json(available);
  },
};
