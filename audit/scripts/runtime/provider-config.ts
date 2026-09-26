// Phase 4.1: is a changed provider configuration rejected during replay?
// Usage: node --experimental-strip-types audit/scripts/runtime/provider-config.ts
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";

import { instantiate, type ReplayEvent } from "../../../src/compiler.ts";

const root = resolve(dirname(new URL(import.meta.url).pathname), "../../..");
const source = readFileSync(join(root, "audit/probes/runtime/edits/base.hd"), "utf8");
const counter = {
  hostCapabilities: ["Counter"],
  hostSuspensionInvoke: (call: { arguments: readonly (number | string)[] }) => ({
    pending: false,
    value: Number(call.arguments[0]) + Number(call.arguments[1]),
  }),
};

async function attempt(label: string, options: Parameters<typeof instantiate>[1]): Promise<void> {
  try {
    const { instance, replay } = await instantiate(source, options);
    const result = (instance.exports.main as CallableFunction)({ requirement: "Counter" });
    replay.assertComplete();
    console.log(`${label}: accepted, main = ${String(result)}`);
  } catch (error) {
    console.log(`${label}: rejected, ${(error as Error).message}`);
  }
}

const events: ReplayEvent[] = [];
await attempt("record  config=prod-v1", {
  ...counter,
  providerConfigurationId: "prod-v1",
  record: (event) => events.push(event),
});
await attempt("replay  config=prod-v1", { ...counter, providerConfigurationId: "prod-v1", replay: events });
await attempt("replay  config=prod-v2", { ...counter, providerConfigurationId: "prod-v2", replay: events });
await attempt("replay  config omitted (defaults to 'default')", { ...counter, replay: events });
await attempt("replay  config=prod-v1, live provider now multiplies (behavior change)", {
  hostCapabilities: ["Counter"],
  hostSuspensionInvoke: (call) => ({
    pending: false,
    value: Number(call.arguments[0]) * Number(call.arguments[1]),
  }),
  providerConfigurationId: "prod-v1",
  replay: events,
});
