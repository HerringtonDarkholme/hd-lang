// One isolated guest run. The parent enforces a per-program wall deadline.
import { readFileSync } from "node:fs";
import { createHost } from "../../host/node.ts";
import { countEntry } from "./baseline.ts";

const [program, path] = process.argv.slice(2);
const module = await WebAssembly.compile(readFileSync(path));
const errors: string[] = [];
const host = createHost(
  {
    out() {},
    err(text) {
      errors.push(text);
    },
  },
  { module, program, seed: 0 },
);
try {
  const instance = await WebAssembly.instantiate(module, host.imports);
  const result = await host.run(instance, "hd.init", "hd.poll");
  if (result.status !== 0) {
    throw new Error(`${program}: ${errors.join("")}`);
  }
  const values = ["hd.retained", "hd.released", "hd.uniques"].map((name) => {
    const counter = instance.exports[name];
    if (typeof counter !== "function") {
      throw new Error(`${program}: missing ${name}`);
    }
    const value = counter();
    if (typeof value !== "bigint") {
      throw new Error(`${program}: invalid ${name}`);
    }
    return value;
  });
  const noOps = instance.exports["hd.no_ops"];
  const skipped = instance.exports["hd.skipped"];
  if (typeof noOps !== "function" || typeof skipped !== "function") {
    throw new Error(`${program}: missing hd.no_ops or hd.skipped`);
  }
  console.log(JSON.stringify(countEntry(program, values, noOps(), skipped())));
} finally {
  host.close();
}
