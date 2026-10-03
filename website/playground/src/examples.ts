// Example projects. The sources are the repository's own files, bundled as
// text: programs the conformance suite runs, and the playground's own
// examples/, which test/runner.test.ts runs.

import closures from "../examples/closures.hd";
import derive from "../examples/derive.hd";
import exitCode from "../examples/exit-code.hd";
import requirements from "../examples/requirements.hd";
import concurrency from "../examples/concurrency.hd";
import errors from "../examples/errors.hd";
import exhaustive from "../examples/exhaustive.hd";
import inventory from "../examples/inventory.hd";
import numbers from "../examples/numbers.hd";
import panic from "../examples/panic.hd";
import standard from "../examples/std.hd";
import suffixes from "../examples/suffixes.hd";
import tests from "../examples/tests.hd";
import topLevel from "../examples/top-level.hd";
import packageMain from "../examples/package/src/main.hd";
import packageModels from "../examples/package/src/models/mod.hd";
import packageUser from "../examples/package/src/models/user.hd";
import hello from "../../../spec/conformance/runtime/valid/println-console-stdout.hd";
import { DEFAULT_MAIN, type Project } from "./project.ts";

export interface Example {
  readonly id: string;
  readonly title: string;
  readonly project: Project;
}

const single = (id: string, title: string, source: string): Example => ({
  id,
  title,
  project: { files: { [DEFAULT_MAIN]: source }, main: DEFAULT_MAIN },
});

export const EXAMPLES: readonly Example[] = [
  single("hello", "Hello, world", hello),
  single("requirements", "Requirements and providers: fake mail and clock in a test", requirements),
  single("derive", "@derive(Arbitrary) and a property test that finds a bug", derive),
  single("errors", "Result, ?, and .context: typed errors with a report", errors),
  single("exhaustive", "Exhaustive match: a new variant shows every place to update", exhaustive),
  single("concurrency", "all! and defer: two services at once", concurrency),
  single("tests", "Tests: it, it_each, and snapshot", tests),
  single("closures", "Closures that capture local state", closures),
  single("numbers", "Checked integers: i64 and u8", numbers),
  single("suffixes", 'Literal suffixes and string prefixes: 250ms and r"..."', suffixes),
  single("std", "Parsing messy input with the toy standard library", standard),
  single("top-level", "Top-level code without main", topLevel),
  single("inventory", "Data, loops, and Option: a stock report", inventory),
  single("exit-code", "Exit codes from main", exitCode),
  single("panic", "Runtime panic", panic),
  {
    id: "package",
    title: "Multi-file package",
    project: {
      files: {
        [DEFAULT_MAIN]: packageMain,
        "src/models/mod.hd": packageModels,
        "src/models/user.hd": packageUser,
      },
      main: DEFAULT_MAIN,
    },
  },
];
