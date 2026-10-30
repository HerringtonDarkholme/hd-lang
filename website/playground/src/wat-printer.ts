// Loads only on the first WAT request; ordinary compiler requests never call it.
export interface WatExports {
  readonly memory: WebAssembly.Memory;
  hd_input(len: number): number;
  hd_print(): number;
  hd_output(): number;
  hd_output_len(): number;
}

export function printWat(exports: WatExports, wasm: Uint8Array): string {
  const ptr = exports.hd_input(wasm.length) >>> 0;
  new Uint8Array(exports.memory.buffer, ptr, wasm.length).set(wasm);
  const status = exports.hd_print();
  const text = new TextDecoder().decode(
    new Uint8Array(exports.memory.buffer, exports.hd_output() >>> 0, exports.hd_output_len() >>> 0),
  );
  if (status !== 0) {
    throw new Error(text);
  }
  return text;
}

export function createWatPrinter(load: () => Promise<BufferSource>) {
  let pending: Promise<WatExports> | undefined;
  return async (wasm: Uint8Array, onLoading?: () => void): Promise<string> => {
    if (!pending) {
      onLoading?.();
      pending = load()
        .then(
          async (bytes) =>
            (await WebAssembly.instantiate(bytes, {})).instance.exports as unknown as WatExports,
        )
        .catch((error: unknown) => {
          pending = undefined;
          throw error;
        });
    }
    return printWat(await pending, wasm);
  };
}
