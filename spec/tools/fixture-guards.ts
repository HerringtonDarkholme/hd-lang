// Fixture guards from the 2026-11-14 conformance fixture review
// (docs/audit/conformance-fixture-review-2026-11-14.md, Action Items item 18),
// as conformance/README.md "Writing a Case" states them:
//
// - A10: an accept case keeps the `# test:` title of a reject case in its
//   folder, its twin, so the title describes a rejection it never makes.
// - A10c: a parse or type accept case's title says the case is rejected. A
//   runtime case's title may name an error value its program computes.
// - A11: a runtime case declares a private `main` and registers no test, so
//   nothing in it runs (module.entry.private-main).
// - A16: a comment spelled like an expectation directive (`# prints:`,
//   `# output:`, ...) that the runner never reads.
// - A17, literal part: a runtime case asserts `true`, or that a literal
//   equals itself, a check no implementation can fail.
//
// The review's A6 (directory against expectation) no longer applies: the
// directory names a topic and carries no expectation (README, Layout). Its
// A18 needs `hd`'s output and message text, so it is no check of the files.
//
// Cost: one pass over the cases reads each fixture's lines once; A10 keys
// reject titles by folder into one Map in that pass and looks each accept
// title up in a second pass over the accept cases. Linear in the suite's
// bytes; no fixture is compared with another.

import type { FixtureCase } from "./fixture-duplicates.ts";

/** One `cases.tsv` row with its fixture text and cited section. */
export interface GuardCase extends FixtureCase {
  readonly specification: string;
}

const title = /^# test: (.*?)\s*$/m;
const rejectedTitle =
  /\b(?:is (?:an|one) error|is rejected|is invalid|is misplaced|is nonlocal|is unknown|is not allowed)\b/i;
const privateMain = /^fn main!?\(/m;
const publicMain = /^pub fn main!?\(/m;
// A fixture registers a test, or runs a scenario, as conformance/README.md
// "Runtime Execution" says: a `tests:` block, a doc test, a test layout, a
// package tree that places it, or a runtime scenario.
const runsWithoutMain =
  /^(?:tests:|## |# fixture-test-layout:|# fixture-package-tree:|# fixture-runtime-scenario:)/m;
const directives = new Set([
  "test",
  "expect",
  "expect-stdout",
  "expect-empty-stdout",
  "fixture-runtime-profile",
  "fixture-runtime-scenario",
  "fixture-runtime-pending-function",
  "fixture-package-role",
  "fixture-test-layout",
  "fixture-package-tree",
  "fixture-target",
]);
const fakeDirective =
  /(?:^|\s)#\s*(prints?|outputs?|expected|expects?|stdout|expect-[a-z-]+|result|returns)\s*:/i;
const literal = String.raw`(?:true|false|[+-]?[0-9][0-9_.]*|"[^"\\$]*"|'[^'\\]')`;
const assertTrue = /(?:^|=>\s*)assert\(\s*true\s*[,)]/;
const assertSame = new RegExp(String.raw`^assert_equal\(\s*(${literal})\s*,\s*(${literal})\s*[,)]`);
// A case that tests the assertion functions, or their names in test code,
// asserts literals on purpose.
const testsAssertions =
  /^lang\/10-modules\.md#(?:standard-testing|r-module\.testing\.|r-module\.prelude\.test-assertions$)/;

const folder = (path: string): string => path.slice(0, path.lastIndexOf("/") + 1);

/** A16: comments spelled like a directive the runner never reads. */
function fakeDirectives(fixture: GuardCase, lines: readonly string[], problems: string[]): void {
  for (const [index, line] of lines.entries()) {
    const fake = fakeDirective.exec(line);
    if (fake && !directives.has(fake[1]!.toLowerCase())) {
      problems.push(
        `fixture ${fixture.path} line ${index + 1}: # ${fake[1]}: is no directive, so the runner never checks it; use # expect-stdout: or an assert`,
      );
    }
  }
}

/** A11 and the literal part of A17, for a runtime case. */
function runtimeGuards(fixture: GuardCase, lines: readonly string[], problems: string[]): void {
  if (
    privateMain.test(fixture.text) &&
    !publicMain.test(fixture.text) &&
    !runsWithoutMain.test(fixture.text)
  ) {
    problems.push(
      `runtime fixture ${fixture.path} declares a private main and registers no test, so nothing runs; make main pub or move its body into tests:`,
    );
  }
  if (testsAssertions.test(fixture.specification)) {
    return;
  }
  for (const [index, line] of lines.entries()) {
    const statement = line.trim();
    const same = assertSame.exec(statement);
    if (assertTrue.test(statement) || (same && same[1] === same[2])) {
      problems.push(
        `runtime fixture ${fixture.path} line ${index + 1} asserts a literal that cannot fail; check a value the rule under test computes`,
      );
    }
  }
}

/** One message per guard a case breaks. */
export function fixtureGuards(cases: Iterable<GuardCase>): string[] {
  const problems: string[] = [];
  const rejects = new Map<string, string>();
  const accepts: [GuardCase, string][] = [];
  for (const fixture of cases) {
    const named = title.exec(fixture.text)?.[1];
    if (named !== undefined && fixture.expectation.startsWith("reject:")) {
      const key = `${folder(fixture.path)}\0${named}`;
      if (!rejects.has(key)) {
        rejects.set(key, fixture.path);
      }
    } else if (named !== undefined && fixture.expectation === "accept") {
      accepts.push([fixture, named]);
      if (fixture.phase !== "runtime" && rejectedTitle.test(named)) {
        problems.push(
          `accept fixture ${fixture.path} has a title that says it is rejected: ${named}`,
        );
      }
    }
    const lines = fixture.text.split(/\r?\n/);
    fakeDirectives(fixture, lines, problems);
    if (fixture.phase === "runtime") {
      runtimeGuards(fixture, lines, problems);
    }
  }
  for (const [fixture, named] of accepts) {
    const twin = rejects.get(`${folder(fixture.path)}\0${named}`);
    if (twin !== undefined) {
      problems.push(
        `accept fixture ${fixture.path} keeps the title of reject fixture ${twin}; title what it accepts`,
      );
    }
  }
  return problems;
}
