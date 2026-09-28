import assert from "node:assert/strict";
import test from "node:test";

import { decodeBase64Url, encodeBase64Url, projectFromHash, projectHash } from "../src/share.ts";

const SOURCE = 'pub fn main() -> void $ Console:\n    println("λ ${1 + 1} 🎉 ~?>")\n';

test("base64url uses the URL alphabet without padding and round-trips UTF-8", () => {
  for (const text of ["", "a", "ab", "abc", "~~~???>>>", SOURCE, "x".repeat(100_000)]) {
    const encoded = encodeBase64Url(text);
    assert.match(encoded, /^[A-Za-z0-9_-]*$/);
    assert.equal(decodeBase64Url(encoded), text);
  }
  assert.equal(encodeBase64Url("~~~???>>>"), "fn5-Pz8_Pj4-");
  assert.equal(encodeBase64Url("ab"), "YWI");
});

test("#code= round-trips a single src/main.hd project", () => {
  const project = { files: { "src/main.hd": SOURCE }, main: "src/main.hd" };
  const hash = projectHash(project);
  assert.equal(hash, `#code=${encodeBase64Url(SOURCE)}`);
  assert.deepEqual(projectFromHash(hash), project);
  assert.deepEqual(projectFromHash(hash.slice(1)), project);
  const website = Buffer.from(SOURCE, "utf8").toString("base64url");
  assert.deepEqual(projectFromHash(`#code=${website}`), project);
});

test("#project= round-trips every file and the entry module", () => {
  const project = {
    files: {
      "src/app.hd": "use pkg.models.user.{User}\npub fn main() -> void: pass\n",
      "src/models/user.hd": "pub data User: pass\n",
    },
    main: "src/app.hd",
  };
  const hash = projectHash(project);
  assert.match(hash, /^#project=[A-Za-z0-9_-]+$/);
  assert.deepEqual(projectFromHash(hash), project);
  const renamedMain = { files: { "src/app.hd": "pass\n" }, main: "src/app.hd" };
  assert.deepEqual(projectFromHash(projectHash(renamedMain)), renamedMain);
});

test("malformed hashes load nothing", () => {
  const bad = (json: string) => `#project=${encodeBase64Url(json)}`;
  for (const hash of [
    "",
    "#",
    "#other=abc",
    "#code=***",
    "#code=_w",
    bad("not json"),
    bad("[]"),
    bad('{"files":{}}'),
    bad('{"files":{"src/main.hd":1}}'),
    bad('{"files":{"../etc/passwd.hd":"x"}}'),
  ])
    assert.equal(projectFromHash(hash), undefined, hash);
  assert.deepEqual(projectFromHash(bad('{"files":{"src/a.hd":"x"},"main":"src/b.hd"}')), {
    files: { "src/a.hd": "x" },
    main: "src/a.hd",
  });
});
