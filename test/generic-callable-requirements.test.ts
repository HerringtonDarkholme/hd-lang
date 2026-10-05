import assert from "node:assert/strict";
import test from "node:test";

import { analyze } from "../src/compiler.ts";

const repositories = `trait Repo[T]:
    fn number(self) -> i32

data User: pass
data Post: pass
data UserRepo: pass
data PostRepo: pass

impl Repo[User] for UserRepo:
    fn number(self) -> i32: 20

impl Repo[Post] for PostRepo:
    fn number(self) -> i32: 1
`;

test("unordered requirement keys do not guess an ambiguous binder mapping", () => {
  const source = `${repositories}
data Job[A, B]:
    callback: fn() -> i32 $ Repo[A] + Repo[B]

fn read_both() -> i32 $ Repo[User] + Repo[Post]:
    $.use(Repo[User]).number() + $.use(Repo[Post]).number()

fn ambiguous() -> void:
    _ := Job { callback: read_both }
`;
  assert.deepEqual(
    analyze(source).diagnostics.map((diagnostic) => diagnostic.code),
    ["ambiguous-type"],
  );
});
