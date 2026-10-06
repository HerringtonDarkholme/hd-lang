// Reading `hd.sum` (spec/cli/command-line.md#hdsum): every line is a tree
// line or a manifest line (cli.sum.line, cli.sum.manifest-line), the lines
// are in order with no repeat, and the last one ends with a newline
// (cli.sum.order). Anything else is a malformed `hd.sum`.

import assert from "node:assert/strict";
import { test } from "node:test";

import { formatSum, readSum } from "../src/dependencies/sum.ts";

const HASH = "h1:QImzkbSXaOPX+xsL3JUAePYyxa0BrY9eO+G+DG7TT8o=";
const OTHER = "h1:0zlgWO/WTIfcgfu8JEX+16fK+dVcdXem8obIRXBB/C4=";

/** The line `readSum` reports for `text`, or undefined when it reads. */
function badLine(text: string): number | undefined {
  const read = readSum(text);
  return "entries" in read ? undefined : read.line;
}

test("a well-formed hd.sum reads, and formatSum writes it back unchanged", () => {
  const text = [
    `github.com/acme/json@1.2.0 ${HASH}`,
    `github.com/acme/json@1.2.0/hd.toml ${OTHER}`,
    `github.com/acme/json@1.10.0-rc.1 ${HASH}`,
    `github.com/acme/json@1.10.0 ${HASH}`,
    `github.com/acme/text@0.0.0-20260101000000-0123456789ab/hd.toml ${OTHER}`,
    "",
  ].join("\n");
  const read = readSum(text);
  assert.ok("entries" in read);
  assert.equal(read.entries.size, 5);
  assert.equal(read.entries.get("github.com/acme/json@1.2.0/hd.toml"), OTHER);
  assert.equal(formatSum(read.entries), text);
  assert.deepEqual(readSum(""), { entries: new Map() });
});

test("a line outside cli.sum.line or cli.sum.manifest-line is malformed", () => {
  for (const line of [
    `acme@1.0.0 ${HASH}`, // not a host path
    `github.com/acme@1.0.0 ${HASH}`, // names no repository
    `github.com/acme/json@v1.0.0 ${HASH}`, // not a version
    `github.com/acme/json@1.0 ${HASH}`,
    `github.com/acme/json@1.0.0/go.mod ${HASH}`, // a suffix other than /hd.toml
    `github.com/acme/json@1.0.0/hd.toml/x ${HASH}`,
    `github.com/acme/json@1.0.0  ${HASH}`, // two spaces
    `github.com/acme/json@1.0.0 h1:short=`,
    `github.com/acme/json@1.0.0 ${HASH} trailing`,
    "", // a blank line
  ])
    assert.equal(badLine(`${line}\n`), 1, line);
});

test("a repeated key, a line out of order, or a missing final newline is malformed", () => {
  const tree = (version: string): string => `github.com/acme/json@${version} ${HASH}\n`;
  // The same key twice, even with the same hash.
  assert.equal(badLine(tree("1.0.0") + tree("1.0.0")), 2);
  // Version order, not text order: 1.10.0 follows 1.2.0.
  assert.equal(badLine(tree("1.10.0") + tree("1.2.0")), 2);
  // A pre-release precedes its release.
  assert.equal(badLine(tree("1.0.0") + tree("1.0.0-rc.1")), 2);
  // Host paths sort first.
  assert.equal(badLine(`github.com/acme/text@1.0.0 ${HASH}\n${tree("2.0.0")}`), 2);
  // A version's tree line comes before its manifest line.
  assert.equal(badLine(`github.com/acme/json@1.0.0/hd.toml ${OTHER}\n${tree("1.0.0")}`), 2);
  // The last line ends with a newline too.
  assert.equal(badLine(tree("1.0.0") + tree("1.1.0").trimEnd()), 2);
});
