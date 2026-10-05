// Linking dependency packages (src/package.ts): `dep.NAME` reaches another
// package's library, whose modules keep their own `pkg`, `self`, and
// `super`, and whose private declarations stay private
// (spec/lang/10-modules.md#use-roots,
// spec/lang/10-modules.md#name-resolution-across-packages).

import assert from "node:assert/strict";
import test from "node:test";

import { instantiate } from "../src/compiler.ts";
import {
  linkedParseOptions,
  linkPackage,
  type DependencyPackage,
  type PackageDependencies,
} from "../src/package.ts";
import { ReplSession } from "../src/repl.ts";

const json: DependencyPackage = {
  id: "github.com/acme/json@1.2.0",
  shown: "dep.json",
  sourceRoot: "/cache/pkg/github.com/acme/json@1.2.0/src",
  files: {
    "lib.hd":
      "use pkg.text.{quote}\n\npub fn render(name: string) -> string:\n    quote(name)\n\nfn secret() -> i32: 7\n",
    "text.hd": 'pub fn quote(text: string) -> string:\n    "\\"" + text + "\\""\n',
  },
  dependencies: {},
};

const graph: PackageDependencies = {
  dependencies: { json: json.id },
  devDependencies: { fixtures: "fixtures" },
  packages: {
    [json.id]: json,
    fixtures: {
      id: "fixtures",
      shown: "dep.fixtures",
      sourceRoot: "/cache/fixtures/src",
      files: { "lib.hd": 'pub fn sample() -> string: "sample"\n' },
      dependencies: {},
    },
  },
};

function codes(files: Record<string, string>, entry = "src/main.hd"): string[] {
  return linkPackage(files, entry, { dependencies: graph }).diagnostics.map(
    ({ path, code, span }) => `${path}:${span.start.line}:${code}`,
  );
}

async function run(files: Record<string, string>): Promise<string[]> {
  const linked = linkPackage(files, "src/main.hd", { dependencies: graph });
  assert.deepEqual(linked.diagnostics, []);
  const lines: string[] = [];
  const { instance, compilation } = await instantiate(linked.source!, {
    parse: linkedParseOptions(linked),
    console: (text) => lines.push(text),
  });
  const main = compilation.hir.functions.find(({ entry }) => entry)!;
  const call = instance.exports[main.name] as (...providers: unknown[]) => unknown;
  call(...main.requirements.map((requirement) => ({ requirement })));
  return lines;
}

test("dep.NAME reaches a dependency's library, which keeps its own pkg", async () => {
  // The root package declares `quote` too: each package keeps its own.
  const lines = await run({
    "src/main.hd":
      'use dep.json.{render}\n\nfn quote(text: string) -> string: "<" + text + ">"\n\npub fn main() -> void $ Console:\n    println(render("ada"))\n    println(quote("ada"))\n',
  });
  assert.deepEqual(lines, ['"ada"', "<ada>"]);
});

test("a dependency's module and its namespace are reachable through dep.NAME", async () => {
  assert.deepEqual(
    await run({
      "src/main.hd":
        'use dep.json.text.{quote}\nuse dep.json\n\npub fn main() -> void $ Console:\n    println(quote("a"))\n    println(json.render("b"))\n',
    }),
    ['"a"', '"b"'],
  );
});

test("only pub declarations of a dependency are visible", () => {
  assert.deepEqual(codes({ "src/main.hd": "use dep.json.{secret}\n" }), [
    "src/main.hd:1:private-import",
  ]);
  assert.deepEqual(codes({ "src/main.hd": "use dep.json.{missing}\n" }), [
    "src/main.hd:1:unknown-import",
  ]);
  assert.deepEqual(codes({ "src/main.hd": "use dep.json.nothing.{x}\n" }), [
    "src/main.hd:1:unknown-module",
  ]);
});

test("an unknown key and a dev dependency in library code are unknown-module", () => {
  assert.deepEqual(codes({ "src/main.hd": "use dep.yaml.{parse}\n" }), [
    "src/main.hd:1:unknown-module",
  ]);
  // Only test code and tasks see dev dependencies
  // (spec/lang/10-modules.md#r-module.test.dev-dependency).
  assert.deepEqual(codes({ "src/main.hd": "use dep.fixtures.{sample}\n" }), [
    "src/main.hd:1:unknown-module",
  ]);
  assert.deepEqual(
    codes(
      { "src/main.hd": "pass\n", "tests/main.hd": "use dep.fixtures.{sample}\n" },
      "tests/main.hd",
    ),
    [],
  );
});

test("a diagnostic in a dependency names the dependency's file", () => {
  const broken: PackageDependencies = {
    ...graph,
    packages: {
      ...graph.packages,
      [json.id]: { ...json, files: { "lib.hd": "use pkg.gone.{x}\n" } },
    },
  };
  const linked = linkPackage({ "src/main.hd": "use dep.json.{x}\n" }, "src/main.hd", {
    dependencies: broken,
  });
  assert.ok(
    linked.diagnostics.some(
      ({ path, code }) =>
        path === "/cache/pkg/github.com/acme/json@1.2.0/src/lib.hd" && code === "unknown-module",
    ),
  );
});

test("each linked module's scope names its package and its imports", () => {
  // The checker reads them to tell packages apart (src/checker/package-ownership.ts).
  const linked = linkPackage(
    { "src/main.hd": "use dep.json.{render}\n\npub fn main() -> void:\n    _ := render\n" },
    "src/main.hd",
    { dependencies: graph },
  );
  const scopes = linked.packageScopes!.scopes;
  assert.deepEqual(
    scopes.map(({ package: owner }) => owner ?? "root"),
    [json.id, json.id, "root"],
  );
  assert.deepEqual(scopes.at(-1)!.imports, ["render"]);
});

test("a REPL session in a package uses its dependencies and dev dependencies", async () => {
  // spec/cli/command-line.md#r-cli.repl.package.dependencies
  const session = new ReplSession(
    {},
    { files: { "src/lib.hd": "" }, programs: [], dependencies: graph },
  );
  assert.deepEqual((await session.evaluate("use dep.json.{render}")).errors, []);
  assert.equal((await session.evaluate('render("ada")')).value, '""ada""');
  assert.deepEqual((await session.evaluate("use dep.fixtures.{sample}")).errors, []);
  assert.match((await session.evaluate("use dep.json.{secret}")).errors[0]!, /private-import/);
});
