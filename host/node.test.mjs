import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";

import { cgroupCpuCount } from "./node.mjs";

function fixture(t, files) {
  const root = mkdtempSync(join(tmpdir(), "hd-cgroup-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  for (const [name, content] of Object.entries(files)) {
    const path = join(root, name);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, content);
  }
  return root;
}

// #408: libuv's last finite ancestor must not override a stricter child.
test("v2: one-CPU child inside a four-CPU parent offers one CPU", (t) => {
  const root = fixture(t, {
    "cpu.max": "max 100000\n",
    "parent/cpu.max": "400000 100000\n",
    "parent/child/cpu.max": "100000 100000\n",
  });
  assert.equal(cgroupCpuCount(root, "/parent/child", 4), 1);
});

test("v2: the tightest ancestor can be the parent", (t) => {
  const root = fixture(t, {
    "parent/cpu.max": "100000 100000\n",
    "parent/child/cpu.max": "400000 100000\n",
  });
  assert.equal(cgroupCpuCount(root, "/parent/child", 8), 1);
});

test("v2: an unlimited parent preserves its child's limit", (t) => {
  const root = fixture(t, {
    "parent/cpu.max": "max 100000\n",
    "parent/child/cpu.max": "100000 100000\n",
  });
  assert.equal(cgroupCpuCount(root, "/parent/child", 4), 1);
});

test("v2: an unlimited child inherits its parent's limit", (t) => {
  const root = fixture(t, {
    "parent/cpu.max": "400000 100000\n",
    "parent/child/cpu.max": "max 100000\n",
  });
  assert.equal(cgroupCpuCount(root, "/parent/child", 8), 4);
});

test("v1: each quota uses its own period and ancestors take the minimum", (t) => {
  const root = fixture(t, {
    "parent/cpu.cfs_quota_us": "800000\n",
    "parent/cpu.cfs_period_us": "200000\n",
    "parent/child/cpu.cfs_quota_us": "100000\n",
    "parent/child/cpu.cfs_period_us": "100000\n",
  });
  assert.equal(cgroupCpuCount(root, "/parent/child", 4), 1);
});

test("v1: negative unlimited quota does not erase a finite child", (t) => {
  const root = fixture(t, {
    "parent/cpu.cfs_quota_us": "-1\n",
    "parent/cpu.cfs_period_us": "100000\n",
    "parent/child/cpu.cfs_quota_us": "100000\n",
    "parent/child/cpu.cfs_period_us": "100000\n",
  });
  assert.equal(cgroupCpuCount(root, "/parent/child", 4), 1);
});

test("the OS affinity count can be stricter than any quota", (t) => {
  const root = fixture(t, { "cpu.max": "400000 100000\n" });
  assert.equal(cgroupCpuCount(root, "/", 2), 2);
});

test("fractional quotas round down like Rust, with at least one CPU", (t) => {
  for (const quota of [150000, 50000]) {
    const root = fixture(t, { "cpu.max": `${quota} 100000\n` });
    assert.equal(cgroupCpuCount(root, "/", 8), 1);
  }
});

test("missing cgroup filesystem leaves the OS count intact", (t) => {
  const root = fixture(t, {});
  assert.equal(cgroupCpuCount(join(root, "absent"), "/parent/child", 4), 4);
});

test("malformed, zero-period and missing limits do not hide an ancestor", (t) => {
  const root = fixture(t, {
    "cpu.max": "200000 100000\n",
    "parent/cpu.max": "400000 0\n",
    "parent/child/cpu.max": "not-a-quota\n",
  });
  assert.equal(cgroupCpuCount(root, "/parent/child/missing", 8), 2);
});

test("the walk stops at its cgroup mount", (t) => {
  const root = fixture(t, {
    "cpu.max": "100000 100000\n",
    "mount/cpu.max": "400000 100000\n",
  });
  assert.equal(cgroupCpuCount(join(root, "mount"), "/", 8), 4);
});
