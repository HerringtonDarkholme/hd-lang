// Fake implementation for the W3 contract-rule plants. It answers every
// command for typing/w00-control-reject.hd (unknown-name on line 3) in the
// way FAKE_MODE selects.
import { relative } from "node:path";

const file = process.argv.at(-1)!;
const rel = relative(process.cwd(), file);
const mode = process.env.FAKE_MODE;
if (mode === "exit2") {
  console.error(`${file}:3:1: unknown-name: planted`);
  process.exit(2);
} else if (mode === "hang") {
  setTimeout(() => process.exit(1), 12_000);
} else if (mode === "signal") {
  process.kill(process.pid, "SIGKILL");
} else if (mode === "other-file") {
  console.error(`${process.execPath}:3:1: unknown-name: planted`);
  process.exit(1);
} else if (mode === "no-location") {
  console.error("unknown-name: planted");
  process.exit(1);
} else if (mode === "dotted-relative") {
  console.error(`./${rel.replace("/", "/./")}:3:9: unknown-name: planted`);
  process.exit(1);
} else {
  console.error(`${rel}:3:1: unknown-name: planted`);
  process.exit(1);
}
