// Example projects. The sources are the repository's own files, bundled as
// text, so the menu always shows programs the conformance suite runs.

import panic from "../examples/panic.hd";
import topLevel from "../examples/top-level.hd";
import packageMain from "../examples/package/src/main.hd";
import packageModels from "../examples/package/src/models/mod.hd";
import packageUser from "../examples/package/src/models/user.hd";
import core from "../../../examples/core.hd";
import suspension from "../../../examples/suspension.hd";
import providers from "../../../spec/conformance/runtime/valid/context-values-install-providers.hd";
import embedding from "../../../spec/conformance/runtime/valid/embedded-field-satisfies-trait.hd";
import hello from "../../../spec/conformance/runtime/valid/println-console-stdout.hd";
import conversion from "../../../spec/conformance/runtime/valid/propagation-from-two-domains.hd";
import scopes from "../../../spec/conformance/runtime/valid/println-around-suspending-provider-scope.hd";
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
  single("top-level", "Top-level code without main", topLevel),
  single("core", "Data, loops, and functions", core),
  single("providers", "Requirements and providers (tests)", providers),
  single("provider-scope", "Provider scope around a suspending call", scopes),
  single("suspension", "Suspension", suspension),
  single("embedding", "Embedding and traits (tests)", embedding),
  single("errors", "Error conversion with ? (tests)", conversion),
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
