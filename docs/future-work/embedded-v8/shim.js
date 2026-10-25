// Only the web primitives used by hd's portable core in these CPU controls.
globalThis.TextEncoder = class {
  encode(text = "") {
    return __encode(String(text));
  }
};
globalThis.TextDecoder = class {
  decode(bytes = new Uint8Array()) {
    return __decode(bytes);
  }
};
globalThis.performance = { timeOrigin: Date.now(), now: __now };
globalThis.queueMicrotask = (callback) => Promise.resolve().then(callback);
if (__decode(__encode("λ😀")) !== "λ😀") throw Error("UTF-8 control");
globalThis.crypto = { getRandomValues: __random };
