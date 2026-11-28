import assert from "node:assert/strict";
import { test } from "node:test";
import { createGrant } from "./core.ts";

test("explicit true grants are unrestricted across every resource matcher", () => {
  const grant = createGrant({ Http: true, Net: true, Env: true, Process: true, FsRead: true });
  for (const key of ["Http", "Net", "Env", "Process", "FsRead"]) {
    assert.equal(grant.allows(key), true);
    assert.equal(grant.limited(key), false);
  }
  assert.equal(grant.coversHost("Http", "example.test", 443), true);
  assert.equal(grant.coversHostName("Net", "example.test"), true);
  assert.equal(grant.coversName("Env", "NAME"), true);
  assert.equal(grant.coversName("Process", "tool"), true);
  assert.equal(grant.coversPath("FsRead", "/any/file", "/"), true);
});

test("total denials and scoped grants remain limited", () => {
  const grant = createGrant({ Http: false, Env: ["PUBLIC_*"], Net: ["example.test:443"] });
  assert.equal(grant.allows("Http"), false);
  assert.equal(grant.limited("Http"), true);
  assert.equal(grant.coversHost("Http", "example.test", 443), false);
  assert.equal(grant.coversName("Env", "PUBLIC_NAME"), true);
  assert.equal(grant.coversName("Env", "PRIVATE_NAME"), false);
  assert.equal(grant.coversHost("Net", "example.test", 443), true);
  assert.equal(grant.coversHost("Net", "example.test", 80), false);
  assert.equal(grant.limited("Clock"), false);
});

// #1901: an unrestricted grant or a total deny answers without resolving
// the path; a scoped grant resolves it once, however many names it has.
test("coversPath resolves a path only under a scoped grant, once per call", () => {
  let resolved = 0;
  const counting = (path: string) => {
    resolved += 1;
    return path;
  };
  for (const names of [8, 80]) {
    const path = `/${Array.from({ length: names }, (_, i) => `d${i}`).join("/")}`;
    for (const [limits, covered, calls] of [
      [{}, true, 0],
      [{ FsRead: true }, true, 0],
      [{ FsRead: false }, false, 0],
      [{ FsRead: ["/d0"] }, true, 1],
      [{ FsRead: ["/elsewhere"] }, false, 1],
    ] as [Record<string, boolean | string[]>, boolean, number][]) {
      resolved = 0;
      const grant = createGrant(limits, counting);
      assert.equal(grant.coversPath("FsRead", path, "/"), covered, JSON.stringify(limits));
      assert.equal(resolved, calls, `${names} names under ${JSON.stringify(limits)}`);
    }
  }
});

test("temp path scopes expand at setup and do not affect other resource names", () => {
  let calls = 0;
  const grant = createGrant(
    { FsRead: ["temp"], FsWrite: ["temp"], Process: ["temp"] },
    (path) => path,
    "/",
    () => {
      calls += 1;
      return "/os-temporary";
    },
  );
  const initial = calls;
  assert.equal(grant.coversPath("FsRead", "/os-temporary/file"), true);
  assert.equal(grant.coversPath("FsWrite", "/os-temporary/file"), true);
  assert.equal(grant.coversPath("FsWrite", "/package/temp/file"), false);
  assert.equal(grant.coversName("Process", "temp"), true);
  assert.equal(calls, initial);
  assert.ok(initial > 0);
});
