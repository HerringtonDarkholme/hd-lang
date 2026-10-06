// Example projects. The sources are the repository's own files, bundled as
// text: programs the conformance suite runs, and the playground's own
// examples/, which test/runner.test.ts runs.

import closures from "../examples/closures.hd";
import derive from "../examples/derive.hd";
import exitCode from "../examples/exit-code.hd";
import effectsInSignature from "../examples/effects-in-signature.hd";
import leastAuthority from "../examples/least-authority.hd";
import missingProvider from "../examples/missing-provider.hd";
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
  single(
    "effects-in-signature",
    "Effects you can review: fake mail and clock in a test",
    effectsInSignature,
  ),
  single("hello", "Hello, world", hello),
  single("missing-provider", "Forget a provider and it won't compile", missingProvider),
  single("least-authority", "Least authority: main limits what is reachable", leastAuthority),
  single("derive", "@derive(Arbitrary): a property test that finds a bug", derive),
  single("errors", "Errors are values: ?, .context, and a report", errors),
  single("exhaustive", "Exhaustive match: a new variant lists every place to update", exhaustive),
  single("concurrency", "all!: two calls at once", concurrency),
  single("tests", "Tests next to the code: it, it_each, snapshot", tests),
  single("closures", "Closures see your locals", closures),
  single("numbers", "No silent overflow: i64 and u8", numbers),
  single("suffixes", "Units in the type: 250ms and 12px", suffixes),
  single("std", "Messy input, no crashes", standard),
  single("top-level", "Try an idea with no main", topLevel),
  single("inventory", "No null: a missing item is .None", inventory),
  single("exit-code", "? in main sets the exit code", exitCode),
  single("panic", "A broken assumption panics loudly", panic),
  {
    id: "package",
    title: "Multi-file package with a mod.hd",
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

/**
 * The project a first visit opens: the home page's welcome example, the same
 * program its code window runs (website/src/home.ts).
 */
export const DEFAULT_EXAMPLE: Example = EXAMPLES[0]!;
