import assert from "node:assert/strict";
import test from "node:test";

import type { HostSuspensionCall } from "../src/compiler.ts";
import { HOST_PROVIDERS } from "../src/host-functions.ts";

function consoleCall(methodName: string, text: string, provider: unknown): HostSuspensionCall {
  return {
    arguments: [text],
    functionCodeId: "console-test",
    functionIndex: 0,
    functionName: `Console.${methodName}`,
    methodName,
    provider,
    providerKey: "Console",
    resultType: "Result[void, ConsoleError]",
    siteId: "console-test:0",
  };
}

test("the built-in Console routes normal and error lines to distinct sinks", () => {
  const output: string[] = [];
  const errors: string[] = [];
  const providers: unknown[] = [];
  const provider = { name: "console" };
  const host = {
    console: (text: string, usedProvider: unknown) => {
      output.push(text);
      providers.push(usedProvider);
    },
    consoleError: (text: string, usedProvider: unknown) => {
      errors.push(text);
      providers.push(usedProvider);
    },
  };

  const written = HOST_PROVIDERS["Console.write_line"]!(
    consoleCall("write_line", "out", provider),
    host,
  );
  const errorWritten = HOST_PROVIDERS["Console.write_error_line"]!(
    consoleCall("write_error_line", "err", provider),
    host,
  );

  assert.deepEqual(output, ["out"]);
  assert.deepEqual(errors, ["err"]);
  assert.deepEqual(providers, [provider, provider]);
  assert.deepEqual(written, { pending: false, value: { tag: "ok" } });
  assert.deepEqual(errorWritten, { pending: false, value: { tag: "ok" } });
});
