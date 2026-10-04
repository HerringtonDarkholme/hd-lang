import assert from "node:assert/strict";
import test from "node:test";

import { instantiate } from "../src/compiler.ts";
import { linkPackage } from "../src/package.ts";

test("ordinary source files resolve self children and super siblings from their own module", async () => {
  const linked = linkPackage(
    {
      "src/main.hd": "use pkg.a.{read}\nfn main() -> i32: read()\n",
      "src/a.hd":
        "use self.x.{child}\nuse super.b.{sibling}\npub fn read() -> i32: child() + sibling()\n",
      "src/a/x.hd": "pub fn child() -> i32: 20\n",
      "src/b/mod.hd": "pub fn sibling() -> i32: 22\n",
    },
    "src/main.hd",
  );
  assert.deepEqual(linked.diagnostics, []);
  const { instance } = await instantiate(linked.source!);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("nested files resolve child, sibling and repeated-parent uses", () => {
  const linked = linkPackage(
    {
      "src/main.hd": "use pkg.user.service.{read}\nfn main() -> i32: read()\n",
      "src/user/service.hd":
        "use self.types.{child}\nuse super.types.{sibling}\nuse super.super.root.{ancestor}\npub fn read() -> i32: child() + sibling() + ancestor()\n",
      "src/user/service/types.hd": "pub fn child() -> i32: 10\n",
      "src/user/types.hd": "pub fn sibling() -> i32: 12\n",
      "src/root/mod.hd": "pub fn ancestor() -> i32: 20\n",
    },
    "src/main.hd",
  );
  assert.deepEqual(linked.diagnostics, []);
  assert.equal(linked.modules.length, 5);
});

test("ordinary source files cannot move above the package root", () => {
  const linked = linkPackage({ "src/a.hd": "use super.super.x.{helper}\n" }, "src/a.hd");
  assert.deepEqual(
    linked.diagnostics.map(({ code, message }) => [code, message]),
    [["unknown-module", "'super' moves above the package root"]],
  );
});
