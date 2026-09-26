// 5.6: string crossing cost at the host-provider boundary (and console output).
// Run: node --experimental-strip-types audit/scripts/arch/rtlib-strings.ts
import { writeFileSync } from "node:fs";
import { resolve } from "node:path";

import { instantiate } from "../../../src/compiler.ts";
import { exported, header, median } from "./bench-lib.ts";

const root = resolve(import.meta.dirname, "../../..");

function program(doublings: number, crossing: "host" | "none" | "console"): string {
  const requirement = crossing === "host" ? " $ TextBridge" : crossing === "console" ? " $ Console" : "";
  const use =
    crossing === "host"
      ? `r := $.use(TextBridge).join!(s, "")`
      : crossing === "console"
        ? `println(s)\n    r := s`
        : `r := s`;
  return `# probe 5.6: string crossing, 2^${doublings} bytes, crossing=${crossing}
pub trait TextBridge:
    fn join!(self, left: string, right: string) -> string

let result_len: i32 = 0

fn build(count: i32) -> string:
    let s: string = "a"
    let index: i32 = 0
    while index < count:
        s = s + s
        index = index + 1
    s

pub fn main!() -> void${requirement}:
    s := build(${doublings})
    ${use}
    result_len = r.len()

pub fn length() -> i32:
    result_len
`;
}

const lines = [
  header("node --experimental-strip-types audit/scripts/arch/rtlib-strings.ts"),
  "Host provider `TextBridge.join!(s, \"\")` returns its argument; console case prints via `println`. Crossing time = median(main with crossing) - median(main without). 5 samples, fresh instance each.",
  "",
  "| bytes | crossing | median main ms | baseline ms | crossing ms | ns per byte | host import calls |",
  "| --- | --- | --- | --- | --- | --- | --- |",
];
for (const doublings of [10, 17, 20]) {
  const bytes = 2 ** doublings;
  const measure = async (crossing: "host" | "none" | "console") => {
    const source = program(doublings, crossing);
    if (doublings === 10) writeFileSync(resolve(root, `audit/probes/arch/runtime-lib/string-${crossing}.hd`), source);
    const times: number[] = [];
    let calls = 0;
    for (let sample = 0; sample < 5; sample += 1) {
      const { instance } = await instantiate(source, {
        hostCapabilities: ["TextBridge"],
        console: () => undefined,
        hostSuspensionInvoke: (call) => ({ pending: false, value: `${call.arguments[0]}${call.arguments[1]}` }),
      });
      // Count host calls by wrapping nothing: derive from byte-level protocol.
      const main = exported(instance, "main");
      const start = performance.now();
      if (crossing === "host") main({ requirement: "TextBridge" });
      else if (crossing === "console") main({ requirement: "Console" });
      else main();
      times.push(performance.now() - start);
      if (Number(exported(instance, "length")()) !== bytes) throw new Error("length mismatch");
      calls = crossing === "host" ? 2 * bytes + 4 : crossing === "console" ? bytes + 1 : 0;
    }
    return { time: median(times), calls };
  };
  const base = await measure("none");
  for (const crossing of ["host", "console"] as const) {
    const result = await measure(crossing);
    const delta = result.time - base.time;
    lines.push(
      `| ${bytes} | ${crossing} | ${result.time.toFixed(2)} | ${base.time.toFixed(2)} | ${delta.toFixed(2)} | ${((delta * 1e6) / bytes).toFixed(1)} | ~${result.calls} |`,
    );
  }
}
lines.push(
  "",
  "Host-call counts are from the emitted protocol (one `argument_byte` import call per argument byte, one `result_byte` per result byte, plus begin/poll/result_length; one `console_byte` per output byte plus a terminator). See rtlib-imports.md and host-wrapper.wat.",
);
writeFileSync(resolve(root, "audit/evidence/05-requirements/rtlib-strings.md"), lines.join("\n") + "\n");
console.log(lines.join("\n"));
