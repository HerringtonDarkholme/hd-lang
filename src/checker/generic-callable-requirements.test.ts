import assert from "node:assert/strict";
import test from "node:test";

import { analyze, instantiate } from "../compiler.ts";

async function runMain(source: string): Promise<number> {
  const result = analyze(source);
  assert.deepEqual(result.diagnostics, []);
  const { instance } = await instantiate(source);
  return (instance.exports.main as CallableFunction)();
}

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

test("a generic field substitutes its callable requirement key through storage", async () => {
  const source = `${repositories}
data Job[T]:
    callback: fn() -> i32 $ Repo[T]

fn read_user() -> i32 $ Repo[User]:
    $.use(Repo[User]).number()

fn main() -> i32:
    $.with(Repo[User]=UserRepo {}):
        let job: Job[User] = Job { callback: read_user }
        (job.callback)() + 22
`;
  assert.equal(await runMain(source), 42);
});

test("callable rows infer generic data arguments from requirement keys", async () => {
  const source = `${repositories}
data Job[T]:
    callback: fn() -> i32 $ Repo[T]

fn read_user() -> i32 $ Repo[User]:
    $.use(Repo[User]).number()

fn main() -> i32:
    $.with(Repo[User]=UserRepo {}):
        job := Job { callback: read_user }
        (job.callback)() + 22
`;
  assert.equal(await runMain(source), 42);
});

test("generic calls infer type arguments from callable requirement keys", async () => {
  const source = `${repositories}
fn invoke[T](callback: fn() -> i32 $ Repo[T]) -> i32 $ Repo[T]:
    callback()

fn read_user() -> i32 $ Repo[User]:
    $.use(Repo[User]).number()

fn main() -> i32:
    $.with(Repo[User]=UserRepo {}):
        invoke(read_user) + 22
`;
  assert.equal(await runMain(source), 42);
});

test("generic callable results restore concrete requirement keys", async () => {
  const source = `${repositories}
fn identity[T](callback: fn() -> i32 $ Repo[T]) -> (fn() -> i32 $ Repo[T]):
    callback

fn read_user() -> i32 $ Repo[User]:
    $.use(Repo[User]).number()

fn main() -> i32:
    $.with(Repo[User]=UserRepo {}):
        callback := identity(read_user)
        callback() + 22
`;
  assert.equal(await runMain(source), 42);
});

test("suspending generic callable results retain their requirement substitutions", async () => {
  const source = `${repositories}
fn identity![T](callback: fn() -> i32 $ Repo[T]) -> (fn() -> i32 $ Repo[T]):
    callback

fn read_user() -> i32 $ Repo[User]:
    $.use(Repo[User]).number()

fn main!() -> i32:
    $.with(Repo[User]=UserRepo {}):
        callback := identity!(read_user)
        callback() + 22
`;
  assert.equal(await runMain(source), 42);
});

test("stored suspensions carry each call site's provider permutation", async () => {
  const source = `${repositories}
fn identity![A, B](callback: fn() -> i32 $ Repo[A] + Repo[B]) -> (fn() -> i32 $ Repo[A] + Repo[B]):
    callback

fn read_both() -> i32 $ Repo[User] + Repo[Post]:
    $.use(Repo[User]).number() * 10 + $.use(Repo[Post]).number()

fn choose!(reverse: bool) -> (fn() -> i32 $ Repo[User] + Repo[Post]):
    let pending: mut Suspend[fn() -> i32 $ Repo[User] + Repo[Post]] = if reverse:
        identity::[Post, User](read_both)
    else:
        identity::[User, Post](read_both)
    pending!()

fn main!() -> i32:
    $.with(Repo[User]=UserRepo {}, Repo[Post]=PostRepo {}):
        forward := choose!(false)
        reverse := choose!(true)
        forward() + reverse()
`;
  assert.equal(await runMain(source), 402);
});

test("dynamic trait suspensions carry concrete provider-key substitutions", async () => {
  const source = `${repositories}
trait Factory[A, B]:
    fn keep!(self, callback: fn() -> i32 $ Repo[A] + Repo[B]) -> (fn() -> i32 $ Repo[A] + Repo[B])

data Forward: pass
data Reverse: pass

impl Factory[User, Post] for Forward:
    fn keep!(self, callback: fn() -> i32 $ Repo[User] + Repo[Post]) -> (fn() -> i32 $ Repo[User] + Repo[Post]):
        callback

impl Factory[Post, User] for Reverse:
    fn keep!(self, callback: fn() -> i32 $ Repo[Post] + Repo[User]) -> (fn() -> i32 $ Repo[Post] + Repo[User]):
        callback

fn read_both() -> i32 $ Repo[User] + Repo[Post]:
    $.use(Repo[User]).number() * 10 + $.use(Repo[Post]).number()

fn main!() -> i32:
    $.with(Repo[User]=UserRepo {}, Repo[Post]=PostRepo {}):
        let forward: Factory[User, Post] = Forward {}
        let reverse: Factory[Post, User] = Reverse {}
        first := forward.keep!(read_both)
        let pending: mut Suspend[fn() -> i32 $ Repo[User] + Repo[Post]] = reverse.keep(read_both)
        second := pending!()
        first() + second()
`;
  assert.equal(await runMain(source), 402);
});

test("dynamic trait calls adapt generic callable parameters and results", async () => {
  const source = `${repositories}
trait Factory[A, B]:
    fn keep(self, callback: fn() -> i32 $ Repo[A] + Repo[B]) -> (fn() -> i32 $ Repo[A] + Repo[B])

data Reverse: pass

impl Factory[Post, User] for Reverse:
    fn keep(self, callback: fn() -> i32 $ Repo[Post] + Repo[User]) -> (fn() -> i32 $ Repo[Post] + Repo[User]):
        callback

fn read_both() -> i32 $ Repo[User] + Repo[Post]:
    $.use(Repo[User]).number() * 10 + $.use(Repo[Post]).number()

fn main() -> i32:
    $.with(Repo[User]=UserRepo {}, Repo[Post]=PostRepo {}):
        let factory: Factory[Post, User] = Reverse {}
        callback := factory.keep(read_both)
        callback() - 159
`;
  assert.equal(await runMain(source), 42);
});

test("provider adaptation follows binder identity across reordered rows", async () => {
  const source = `${repositories}
data Job[A, B]:
    callback: fn() -> i32 $ Repo[A] + Repo[B]

fn read_both() -> i32 $ Repo[User] + Repo[Post]:
    $.use(Repo[User]).number() + $.use(Repo[Post]).number()

fn main() -> i32:
    $.with(Repo[User]=UserRepo {}, Repo[Post]=PostRepo {}):
        let forward: Job[User, Post] = Job { callback: read_both }
        let reverse: Job[Post, User] = Job { callback: read_both }
        (forward.callback)() + (reverse.callback)()
`;
  assert.equal(await runMain(source), 42);
});

test("two generic keys may collapse to one concrete row during adaptation", async () => {
  const source = `${repositories}
data Job[A, B]:
    callback: fn() -> i32 $ Repo[A] + Repo[B]

fn read_user() -> i32 $ Repo[User]:
    $.use(Repo[User]).number()

fn main() -> i32:
    $.with(Repo[User]=UserRepo {}):
        let job: Job[User, User] = Job { callback: read_user }
        (job.callback)() + 22
`;
  assert.equal(await runMain(source), 42);
});

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
    ["cannot-infer-type"],
  );
});

test("enum payload extraction retains generic callable provider substitutions", async () => {
  const source = `${repositories}
enum Task[T]:
    Run(callback: fn() -> i32 $ Repo[T])

fn read_user() -> i32 $ Repo[User]:
    $.use(Repo[User]).number()

fn main() -> i32:
    $.with(Repo[User]=UserRepo {}):
        let task: Task[User] = Task.Run(read_user)
        match task:
            .Run(callback) => callback() + 22
`;
  assert.equal(await runMain(source), 42);
});
