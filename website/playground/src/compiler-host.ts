// The compiler's only web host operation: NFC normalization. Rust lends
// the input for one call and owns the output buffer. A zero-capacity call
// queries the required UTF-8 length; a later call fills that buffer.
export function compilerImports(getMemory: () => WebAssembly.Memory): WebAssembly.Imports {
  const decoder = new TextDecoder("utf-8", { fatal: true });
  const encoder = new TextEncoder();
  return {
    hd_web: {
      nfc_utf8(input: number, length: number, output: number, capacity: number): number {
        const text = decoder.decode(new Uint8Array(getMemory().buffer, input >>> 0, length >>> 0));
        const bytes = encoder.encode(text.normalize("NFC"));
        if (capacity >>> 0 !== 0 && capacity >>> 0 >= bytes.length) {
          new Uint8Array(getMemory().buffer, output >>> 0, bytes.length).set(bytes);
        }
        return bytes.length;
      },
    },
  };
}

function linker() {
  let memory: WebAssembly.Memory | undefined;
  return {
    imports: compilerImports(() => {
      if (memory === undefined) {
        throw new Error("the compiler called NFC before its memory was ready");
      }
      return memory;
    }),
    attach(instance: WebAssembly.Instance): void {
      memory = instance.exports.memory as WebAssembly.Memory;
    },
  };
}

/** Loads a compiler over the same imports in a worker and in Node tests. */
export async function instantiateCompiler(bytes: BufferSource) {
  const host = linker();
  const loaded = await WebAssembly.instantiate(bytes, host.imports);
  host.attach(loaded.instance);
  return loaded;
}

/** The website's synchronous syntax checker already has a compiled module. */
export function instantiateCompilerModule(module: WebAssembly.Module): WebAssembly.Instance {
  const host = linker();
  const instance = new WebAssembly.Instance(module, host.imports);
  host.attach(instance);
  return instance;
}
