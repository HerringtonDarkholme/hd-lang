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
