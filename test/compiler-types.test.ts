import assert from "node:assert/strict";
import test from "node:test";

import { analyze, compile, instantiate } from "../src/compiler.ts";

test("trailing callback blocks lower as contextually typed closures", async () => {
  const source = `fn apply(callback: fn() -> i32) -> i32: callback()
fn add(base: i32, callback: fn() -> i32) -> i32: base + callback()
fn main() -> i32:
    left := apply:
        40
    add(left):
        2
`;
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("readonly data fields can call closures that return mutable access", async () => {
  const source = `data Child:
    value: i32
data Holder:
    get: fn() -> mut Child
fn make() -> mut Child: Child { value: 42 }
fn main() -> i32:
    holder := Holder { get: make }
    let child: mut Child = holder.get()
    child.value
`;
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("named functions reify as monomorphic function values", async () => {
  const source = `fn increment(value: i32) -> i32: value + 1

fn apply(value: i32, transform: fn(i32) -> i32) -> i32:
    transform(value)

fn main() -> i32: apply(41, increment)
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /func \$fv0 \(type \$sig/);
  assert.match(compilation.wat, /ref\.func \$fv0/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("erased generic functions infer type arguments and box primitive values", async () => {
  const source = `data Box:
    value: i32

fn identity[T](value: T) -> T: value

fn choose[T](left: T, right: T) -> T: left

fn float_value() -> f64: identity(3.5)

fn main() -> i32:
    number := identity(40)
    text := identity("hd")
    box := identity(Box { value: 2 })
    choose(number + box.value, 42) + text.len() - 2
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /param \$l0 anyref/);
  assert.match(compilation.wat, /struct\.new \$hd\.box-i32/);
  assert.match(compilation.wat, /ref\.cast \(ref \$hd\.box-f64\)/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.equal((instance.exports.float_value as CallableFunction)(), 3.5);

  assert.equal(
    analyze("fn choose[T](left: T, right: T) -> T: left\nfn main() -> i32: choose(1, true)\n")
      .diagnostics[0]?.code,
    "generic-type-mismatch",
  );
  assert.equal(
    analyze("fn missing[T]() -> i32: 1\nfn main() -> i32: missing()\n").diagnostics[0]?.code,
    "unresolved-generic-placeholder",
  );
  assert.equal(
    analyze("fn identity[T](value: T) -> T: value\nfn main() -> void:\n    value := identity\n")
      .diagnostics[0]?.code,
    "generic-function-value-needs-arguments",
  );
});

test("generic inference traverses optional and Result types", async () => {
  const source = `data Failure:
    code: i32

fn keep_optional[T](value: T?) -> T?: value

fn keep_result[T, E](value: Result[T, E]) -> Result[T, E]: value

fn inspect_optional(value: i32?) -> i32:
    match value:
        actual? => actual
        nil => 0

fn inspect_result(value: Result[i32, Failure]) -> i32:
    match value:
        Ok(actual) => actual
        Err(error) => -error.code

fn main() -> i32:
    let optional: i32? = 40
    let result: Result[i32, Failure] = Ok(2)
    inspect_optional(keep_optional(optional)) + inspect_result(keep_result(result))
`;
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("higher-order erased generics adapt concrete callable ABIs", async () => {
  const source = `fn apply[T](value: T, transform: fn(T) -> T) -> T:
    transform(value)

fn increment(value: i32) -> i32: value + 1

fn main() -> i32:
    apply(40, increment) + apply(0, fn(value): value + 1)
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /func \$adapt0/);
  assert.match(compilation.wat, /ref\.cast \(ref \$closure/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("generic requirement rows infer callback providers and pack them for Wasm GC", async () => {
  const source = `fn invoke[r](callback: fn() -> i32 $ r) -> i32 $ r:
    callback()

fn read() -> i32 $ Clock + Logger:
    _ := $.use(Clock)
    _ := $.use(Logger)
    42

fn main() -> i32 $ Clock + Logger:
    invoke(read)
`;
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.hir.functions[0]?.rowParameters, ["r"]);
  assert.equal(compilation.hir.functions[0]?.parameters[0]?.type, "fn()->i32$row:r");
  assert.match(compilation.wat, /type \$hd\.providers \(struct/);
  assert.match(compilation.wat, /struct\.new \$hd\.providers/);
  assert.match(compilation.wat, /call \$hd\.provider_get/);
  assert.equal((instance.exports.main as CallableFunction)({ clock: true }, { logger: true }), 42);
});

test("generic requirement rows infer empty rows and diagnose unavailable or conflicting rows", async () => {
  const empty = `fn invoke[r](callback: fn() -> i32 $ r) -> i32 $ r: callback()
fn pure() -> i32: 42
fn main() -> i32: invoke(pure)
`;
  const { instance, compilation } = await instantiate(empty);
  assert.match(compilation.wat, /ref\.null \$hd\.providers/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);

  const missing = `fn invoke[r](callback: fn() -> i32 $ r) -> i32 $ r: callback()
fn read() -> i32 $ Clock + Logger: 42
fn main() -> i32 $ Clock: invoke(read)
`;
  assert.equal(analyze(missing).diagnostics[0]?.code, "missing-requirement");

  const conflicting = `fn combine[r](left: fn() -> i32 $ r, right: fn() -> i32 $ r) -> i32 $ r:
    left() + right()
fn clock() -> i32 $ Clock: 20
fn logger() -> i32 $ Logger: 22
fn main() -> i32 $ Clock + Logger: combine(clock, logger)
`;
  assert.equal(analyze(conflicting).diagnostics[0]?.code, "generic-type-mismatch");
});

test("generic row subtraction restores a locally supplied provider", async () => {
  const source = `fn provide_logger[r](callback: fn() -> i32 $ r) -> i32 $ (r - Logger) + Backup:
    $.with(Logger=$.use(Backup)):
        callback()

fn read() -> i32 $ Clock + Logger:
    _ := $.use(Clock)
    _ := $.use(Logger)
    42

fn main() -> i32 $ Clock + Backup:
    provide_logger(read)
`;
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.hir.functions[0]?.requirements, ["Backup", "row:r\\Logger"]);
  assert.match(compilation.wat, /struct\.new \$hd\.providers/);
  assert.equal((instance.exports.main as CallableFunction)({ backup: true }, { clock: true }), 42);

  const unsound = `fn drop_logger[r](callback: fn() -> i32 $ r) -> i32 $ (r - Logger):
    callback()
fn read() -> i32 $ Logger: 42
fn main() -> i32: drop_logger(read)
`;
  assert.equal(analyze(unsound).diagnostics[0]?.code, "missing-requirement");

  const redundant = `fn redundant[r](callback: fn() -> i32 $ r) -> i32 $ (r - Logger) + Backup:
    $.with(Logger=$.use(Backup)):
        callback()
fn tick() -> i32 $ Clock: 42
fn main() -> i32 $ Clock + Backup: redundant(tick)
`;
  const warning = analyze(redundant);
  assert.ok(warning.hir);
  assert.deepEqual(
    warning.diagnostics.map(({ code, severity }) => ({ code, severity })),
    [{ code: "requirement-subtract-absent", severity: "warning" }],
  );
});

test("generic row union inference chooses the least row solution", async () => {
  const source = `fn invoke[r](callback: fn() -> i32 $ r + Logger) -> i32 $ r + Logger:
    callback()
fn logged() -> i32 $ Logger: 42
fn main() -> i32 $ Logger: invoke(logged)
`;
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)({ logger: true }), 42);
});

test("generic row forwarding composes symbolic and concrete provider packs", async () => {
  const source = `fn invoke[r](callback: fn() -> i32 $ r) -> i32 $ r:
    callback()

fn forward[s](callback: fn() -> i32 $ s + Clock) -> i32 $ s + Clock:
    invoke(callback)

fn read() -> i32 $ Clock + Logger:
    _ := $.use(Clock)
    _ := $.use(Logger)
    42

fn main() -> i32 $ Clock + Logger:
    forward(read)
`;
  const { instance, compilation } = await instantiate(source);
  const forwarded = compilation.hir.functions[1]?.body[0];
  assert.equal(forwarded?.kind, "expression");
  assert.equal(forwarded?.kind === "expression" && forwarded.expression.kind, "call");
  const pack =
    forwarded?.kind === "expression" && forwarded.expression.kind === "call"
      ? forwarded.expression.providers[0]
      : undefined;
  assert.equal(pack?.kind, "provider-pack");
  assert.deepEqual(pack?.kind === "provider-pack" && pack.keys, ["Clock"]);
  assert.equal(pack?.kind === "provider-pack" && pack.bases.length, 1);
  assert.match(compilation.wat, /call \$hd\.provider_concat/);
  assert.equal((instance.exports.main as CallableFunction)({ clock: true }, { logger: true }), 42);
});

test("generic row forwarding unions multiple symbolic provider packs", async () => {
  const source = `fn invoke[r](callback: fn() -> i32 $ r) -> i32 $ r:
    callback()

fn forward[s, t](left: fn() -> i32 $ s, right: fn() -> i32 $ t, callback: fn() -> i32 $ s + t) -> i32 $ s + t:
    a := left()
    b := right()
    a + b + invoke(callback)

fn tick() -> i32 $ Clock: 1
fn log() -> i32 $ Logger: 2
fn read() -> i32 $ Clock + Logger: 42

fn main() -> i32 $ Clock + Logger:
    forward(tick, log, read)
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /func \$hd\.provider_concat/);
  assert.equal((instance.exports.main as CallableFunction)({ clock: true }, { logger: true }), 45);
});

test("generic provider keys reject possible collisions before erasure", async () => {
  const collision = `trait Repo[T]
fn choose[T, U](first: Repo[T], second: Repo[U]) -> void:
    $.with(Repo[T]=first):
        $.with(Repo[U]=second):
            pass
`;
  assert.equal(analyze(collision).diagnostics[0]?.code, "generic-requirement-key-collision");

  const spreadCollision = `trait Repo[T]
fn choose[T, U](first: Repo[T], second: Repo[U]) -> void:
    base := $.context(Repo[T]=first)
    _ := $.context(...base, Repo[U]=second)
`;
  assert.equal(analyze(spreadCollision).diagnostics[0]?.code, "generic-requirement-key-collision");

  const consistentSubstitution = `trait Pair[A, B]
fn safe[T](same: Pair[T, T], mixed: Pair[i32, string]) -> void:
    $.with(Pair[T, T]=same, Pair[i32, string]=mixed):
        pass
`;
  assert.deepEqual(analyze(consistentSubstitution).diagnostics, []);
  const occursCheck = `trait Repo[T]
fn safe[T](plain: Repo[T], nested: Repo[T?]) -> void:
    $.with(Repo[T]=plain, Repo[T?]=nested):
        pass
`;
  assert.deepEqual(analyze(occursCheck).diagnostics, []);

  const distinct = `trait Repo[T]
data User: pass
data Post: pass
fn choose(first: Repo[User], second: Repo[Post]) -> void:
    $.with(Repo[User]=first, Repo[Post]=second):
        pass
fn consume[T](repo: Repo[T]) -> i32 $ Repo[T]:
    _ := $.use(Repo[T])
    1
fn run(repo: Repo[User]) -> i32:
    $.with(Repo[User]=repo):
        consume(repo)
`;
  const compilation = compile(distinct);
  assert.deepEqual(compilation.diagnostics, []);
  assert.ok(WebAssembly.validate(compilation.bytes));
});

test("trait implementations support static and Wasm GC dynamic dispatch", async () => {
  const source = `trait Describe:
    fn describe(self) -> i32

data User:
    value: i32

impl Describe for User:
    fn describe(self) -> i32: self.value

fn dynamic_show(value: Describe) -> i32:
    value.describe()

fn main() -> i32:
    user := User { value: 21 }
    user.describe() + dynamic_show(user)
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /type \$trait0 \(struct/);
  assert.match(compilation.wat, /func \$tadapt0_0/);
  assert.match(compilation.wat, /call_ref \$tsig0_0/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("mutable trait receivers preserve permission through every dispatch path", async () => {
  const source = `trait Add:
    fn add(mut self, value: i32) -> void
    fn twice(mut self, value: i32) -> void:
        self.add(value)
        self.add(value)

data Counter:
    value: i32

impl Add for Counter:
    fn add(mut self, value: i32) -> void:
        self.value = self.value + value

fn bounded[T: Add](value: mut T, amount: i32) -> void:
    value.add(amount)

fn main() -> i32:
    let counter: mut Counter = Counter { value: 0 }
    counter.add(10)
    let dynamic: mut Add = counter
    dynamic.twice(5)
    bounded(counter, 22)
    counter.value
`;
  const { instance, compilation } = await instantiate(source);
  assert.equal(compilation.hir.traits[0]?.methods[0]?.receiverMutable, true);
  assert.equal(compilation.hir.traits[0]?.methods[1]?.receiverMutable, true);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("inherent methods lower as direct functions with mutable and suspending receivers", async () => {
  const source = `data Counter:
    value: i32

impl Counter:
    fn add(mut self, amount: i32) -> void:
        self.value = self.value + amount
    fn load!(self, amount: i32) -> i32:
        self.value + amount

fn main!() -> i32:
    let counter: mut Counter = Counter { value: 10 }
    counter.add(amount=12)
    counter.load!(20)
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /func \$f\d+/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("embedded fields promote inherent methods and can fill readonly trait requirements", async () => {
  const source = `trait Describe:
    fn describe(self) -> i32

data Label:
    value: i32

impl Label:
    fn describe(self) -> i32: self.value

data Page:
    Label

impl Describe for Page

fn show(value: Describe) -> i32: value.describe()

fn main() -> i32:
    page := Page { Label: Label { value: 21 } }
    page.describe() + show(page)
`;
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("mutable trait receivers reject readonly calls and signature weakening", () => {
  const readonlyStatic = `trait Add:
    fn add(mut self) -> void
data Counter:
    value: i32
impl Add for Counter:
    fn add(mut self) -> void: pass
fn invalid(value: Counter) -> void: value.add()
`;
  assert.equal(analyze(readonlyStatic).diagnostics[0]?.code, "mutable-receiver-required");

  const readonlyDynamic = readonlyStatic.replace(
    "fn invalid(value: Counter) -> void: value.add()",
    "fn invalid(value: Add) -> void: value.add()",
  );
  assert.equal(analyze(readonlyDynamic).diagnostics[0]?.code, "mutable-receiver-required");

  const mismatched = readonlyStatic
    .replace("fn add(mut self) -> void: pass", "fn add(self) -> void: pass")
    .replace("fn invalid(value: Counter) -> void: value.add()\n", "");
  assert.equal(analyze(mismatched).diagnostics[0]?.code, "trait-method-signature");
});

test("trait checking diagnoses missing, mismatched, and ambiguous methods", () => {
  const missing = `trait Named:
    fn name(self) -> string
data User:
    value: i32
impl Named for User
`;
  assert.equal(analyze(missing).diagnostics[0]?.code, "missing-trait-method");

  const mismatched = `trait Named:
    fn name(self) -> string
data User:
    value: i32
impl Named for User:
    fn name(self) -> i32: 0
`;
  assert.equal(analyze(mismatched).diagnostics[0]?.code, "trait-method-signature");

  const ambiguous = `trait Left:
    fn label(self) -> i32
trait Right:
    fn label(self) -> i32
data User:
    value: i32
impl Left for User:
    fn label(self) -> i32: self.value
impl Right for User:
    fn label(self) -> i32: self.value
fn main() -> i32: User { value: 42 }.label()
`;
  assert.equal(analyze(ambiguous).diagnostics[0]?.code, "ambiguous-method");
});

test("default trait methods participate in static and dynamic dispatch", async () => {
  const source = `trait Named:
    fn name(self) -> i32
    fn label(self) -> i32:
        self.name() + 2

data Number:
    value: i32

impl Named for Number:
    fn name(self) -> i32: self.value

fn show(value: Named) -> i32:
    value.label()

fn main() -> i32:
    show(Number { value: 40 })
`;
  const { instance, compilation } = await instantiate(source);
  assert.equal(compilation.hir.implementations[0]?.methodFunctions.length, 2);
  assert.match(compilation.wat, /func \$tadapt0_1/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);

  const override = source.replace(
    "fn name(self) -> i32: self.value",
    "fn name(self) -> i32: self.value\n    fn label(self) -> i32: 41",
  );
  const overridden = await instantiate(override);
  assert.equal((overridden.instance.exports.main as CallableFunction)(), 41);
});

test("suspending trait methods use concrete and dynamic Wasm GC frames", async () => {
  const source = `trait Read:
    fn read!(self, delta: i32) -> i32

data Counter:
    value: i32

impl Read for Counter:
    fn read!(self, delta: i32) -> i32: self.value + delta

fn bounded![T: Read](value: T) -> i32:
    value.read!(1)

fn main!() -> i32:
    value := Counter { value: 10 }
    static := value.read!(0)
    let dynamic: Read = value
    let pending: mut Suspend[i32] = dynamic.read(1)
    stored := pending!()
    direct := dynamic.read!(0)
    generic := bounded!(value)
    static + stored + direct + generic
`;
  const { instance, compilation } = await instantiate(source, {
    pending: (functionIndex, pollCount) => functionIndex === 2 && pollCount === 1,
  });
  assert.match(compilation.wat, /type \$ts0_0 \(struct/);
  assert.match(compilation.wat, /ref\.func \$tspolladapt0_0/);
  assert.match(compilation.wat, /call_ref \$tspollsig0_0/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("cancelling a dynamic suspending trait method reaches child cleanup", async () => {
  const source = `trait Job:
    fn run!(self) -> void

data Worker:
    value: i32

fn wait!() -> void: pass

impl Job for Worker:
    fn run!(self) -> void:
        defer:
            _ := self.value
        wait!()

fn main!() -> void:
    let job: Job = Worker { value: 42 }
    job.run!()
`;
  const events: Array<[number, number]> = [];
  const { instance } = await instantiate(source, {
    trace: (functionIndex, event) => events.push([functionIndex, event]),
    pending: (functionIndex, pollCount) => functionIndex === 0 && pollCount === 1,
  });
  (instance.exports.__hd_start as CallableFunction)();
  assert.equal((instance.exports.__hd_poll as CallableFunction)(), 0);
  (instance.exports.__hd_cancel as CallableFunction)();
  assert.ok(events.some(([functionIndex, event]) => functionIndex === 2 && event === 7));
});

test("default suspending trait methods lower for each implementation", async () => {
  const source = `trait Named:
    fn value(self) -> i32
    fn load!(self) -> i32:
        self.value()

data Number:
    value: i32

impl Named for Number:
    fn value(self) -> i32: self.value

fn main!() -> i32:
    let named: Named = Number { value: 42 }
    named.load!()
`;
  const { instance, compilation } = await instantiate(source);
  assert.equal(compilation.hir.implementations[0]?.methodFunctions.length, 2);
  assert.match(compilation.wat, /func \$tadapt0_1/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("trait values act as lexical providers and survive generic row subtraction", async () => {
  const source = `trait Logger:
    fn adjust(self, value: i32) -> i32

data OffsetLogger:
    offset: i32

impl Logger for OffsetLogger:
    fn adjust(self, value: i32) -> i32: value + self.offset

fn read() -> i32 $ Logger:
    $.use(Logger).adjust(40)

fn provide_logger[r](callback: fn() -> i32 $ r, logger: Logger) -> i32 $ (r - Logger):
    $.with(Logger=logger):
        callback()

fn main() -> i32:
    provide_logger(read, OffsetLogger { offset: 2 })
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /field \$hd\.provider-value anyref/);
  assert.match(compilation.wat, /ref\.cast \(ref null \$trait0\)/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("erased generic trait bounds dispatch methods and unwrap returned values", async () => {
  const source = `trait Describe:
    fn describe(self) -> i32

data Number:
    value: i32

impl Describe for Number:
    fn describe(self) -> i32: self.value

fn show[T: Describe](value: T) -> i32:
    value.describe()

fn identity[T: Describe](value: T) -> T:
    value

fn main() -> i32:
    returned := identity(Number { value: 22 })
    show(Number { value: 20 }) + returned.value
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /param \$bound0 \(ref null \$trait0\)/);
  assert.match(compilation.wat, /struct\.new \$trait0 \(ref\.null any\)/);
  assert.match(compilation.wat, /struct\.get \$trait0 \$trait0value/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);

  const missing = source.replace("show(Number { value: 20 })", "show(42)");
  assert.equal(analyze(missing).diagnostics[0]?.code, "missing-trait-implementation");
});

test("multiple trait bounds pass independent dictionaries and forward them", async () => {
  const source = `trait Describe:
    fn describe(self) -> i32

trait Named:
    fn name(self) -> i32

data Number:
    value: i32
    extra: i32

impl Describe for Number:
    fn describe(self) -> i32: self.value

impl Named for Number:
    fn name(self) -> i32: self.extra

fn describe_one[T: Describe](value: T) -> i32:
    value.describe()

fn inspect[T: Describe + Named](value: T) -> i32:
    describe_one(value) + value.name()

fn inspect_first[T: Describe](values: list[T]) -> i32:
    values[0].describe()

fn identity[T: Describe + Named](value: T) -> T:
    value

fn main() -> i32:
    number := identity(Number { value: 20, extra: 2 })
    inspect(number) + inspect_first([Number { value: 20, extra: 0 }])
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(
    compilation.wat,
    /param \$bound0 \(ref null \$trait0\).*param \$bound1 \(ref null \$trait1\)/s,
  );
  assert.match(compilation.wat, /local\.get \$bound0/);
  assert.match(compilation.wat, /local\.get \$bound1/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);

  const duplicate = source.replace("T: Describe + Named", "T: Describe + Describe");
  assert.equal(analyze(duplicate).diagnostics[0]?.code, "duplicate-trait-bound");
});

test("generic data uses one erased GC layout with precise instantiated member types", async () => {
  const source = `data Box[T]:
    value: T

data Pair[A, B]:
    first: A
    second: B

fn keep[T](box: Box[T]) -> Box[T]: box

fn main() -> i32:
    box := Box { value: 40 }
    pair := Pair { first: keep(box), second: "hd" }
    pair.first.value + pair.second.len()
`;
  const { instance, compilation } = await instantiate(source);
  assert.equal(compilation.hir.functions.at(-1)?.locals[0]?.type, "Box[i32]");
  assert.match(compilation.wat, /field \$d0f0 \(mut anyref\)/);
  assert.match(compilation.wat, /struct\.new \$hd\.box-i32/);
  assert.match(compilation.wat, /ref\.cast \(ref null \$d0\)/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);

  const mismatch = `data Box[T]:\n    value: T\nfn main() -> void:\n    let box: Box[i32] = Box { value: true }\n`;
  assert.equal(analyze(mismatch).diagnostics[0]?.code, "generic-type-mismatch");
});

test("generic enums erase exact payloads and recover instantiated match bindings", async () => {
  const source = `enum Maybe[T]:
    Some(value: T)
    None

enum Tree[T]:
    Leaf(value: T)
    Branch(left: Tree[T], right: Tree[T])

fn keep[T](value: Maybe[T]) -> Maybe[T]: value

fn unwrap(value: Maybe[i32]) -> i32:
    match value:
        Maybe.Some(actual) => actual
        Maybe.None => 0

fn sum(tree: Tree[i32]) -> i32:
    match tree:
        Tree.Leaf(value) => value
        Tree.Branch(left, right) => sum(left) + sum(right)

fn main() -> i32:
    some := Maybe.Some(40)
    let none: Maybe[i32] = Maybe.None
    tree := Tree.Branch(Tree.Leaf(1), Tree.Leaf(1))
    unwrap(keep(some)) + unwrap(none) + sum(tree)
`;
  const { instance, compilation } = await instantiate(source);
  assert.equal(compilation.hir.functions.at(-1)?.locals[0]?.type, "Maybe[i32]");
  assert.match(compilation.wat, /field \$e0f0 \(mut anyref\)/);
  assert.match(compilation.wat, /struct\.new \$hd\.box-i32/);
  assert.match(compilation.wat, /ref\.cast \(ref \$hd\.box-i32\)/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);

  assert.equal(
    analyze(
      "enum Maybe[T]:\n    Some(value: T)\n    None\nfn main() -> void:\n    value := Maybe.None\n",
    ).diagnostics[0]?.code,
    "generic-enum-needs-context",
  );
  assert.equal(
    analyze(
      "enum Maybe[T]:\n    Some(value: T)\n    None\nfn consume(value: Maybe) -> void:\n    pass\n",
    ).diagnostics[0]?.code,
    "unknown-type",
  );
});

test("generic lists lower to growable GC vectors with erased element storage", async () => {
  const source = `fn first[T](items: list[T]) -> T:
    items[0]

fn main() -> i32:
    numbers := [40, 2]
    first(numbers) + numbers.len()
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /type \$hd\.vector \(struct/);
  assert.match(compilation.wat, /type \$hd\.list \(array \(mut anyref\)\)/);
  assert.match(compilation.wat, /struct\.new \$hd\.vector/);
  assert.match(compilation.wat, /call \$hd\.vector_get/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);

  assert.equal(
    analyze("fn main() -> void:\n    values := []\n").diagnostics[0]?.code,
    "empty-list-needs-context",
  );
  assert.equal(
    analyze("fn main() -> void:\n    values := [1, true]\n").diagnostics[0]?.code,
    "no-common-type",
  );
  assert.equal(
    analyze("fn consume(values: list[void]) -> void:\n    pass\n").diagnostics[0]?.code,
    "unknown-type",
  );
});

test("contextual list elements convert to dynamic trait values", async () => {
  const source = `trait Value:
    fn get(self) -> i32

data Number:
    value: i32

impl Value for Number:
    fn get(self) -> i32: self.value

fn main() -> i32:
    let values: list[Value] = [Number { value: 20 }, Number { value: 22 }]
    values[0].get() + values[1].get()
`;
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("generic maps lower to GC storage with optional lookup and replacement", async () => {
  const source = `fn lookup[T](values: map[string, T], key: string) -> T?:
    values[key]

fn unwrap(value: i32?) -> i32:
    match value:
        actual? => actual
        nil => 0

fn main() -> i32:
    scores := {"answer": 20, "other": 2, "answer": 40}
    unwrap(lookup(scores, "answer")) + scores.len()

fn scalar_keys() -> i32:
    values := {1: 20, 2: 2, 1: 40}
    unwrap(values[1]) + values.len()

fn empty() -> i32:
    let values: map[string, i32] = {}
    values.len()
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /type \$hd\.map \(struct/);
  assert.match(compilation.wat, /call \$hd\.map_insert/);
  assert.match(compilation.wat, /call \$hd\.map_get/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.equal((instance.exports.scalar_keys as CallableFunction)(), 42);
  assert.equal((instance.exports.empty as CallableFunction)(), 0);

  assert.equal(
    analyze("fn main() -> void:\n    values := {}\n").diagnostics[0]?.code,
    "empty-map-needs-context",
  );
  assert.equal(
    analyze("fn main() -> void:\n    values := {1: 1, 2: true}\n").diagnostics[0]?.code,
    "type-mismatch",
  );
  assert.equal(
    analyze("fn main() -> void:\n    values := {1.5: 1}\n").diagnostics[0]?.code,
    "unsupported-map-key",
  );
});

test("capturing closures store outer locals in GC environments", async () => {
  const source = `fn main() -> i32:
    base := 40
    add := fn(value: i32) -> i32: base + value
    add(2)
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /\(type \$env0 \(struct/);
  assert.match(compilation.wat, /struct\.new \$closure0/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("reference identity uses Wasm GC identity and enforces Reference bounds", async () => {
  const source = `data Box:
    value: i32

trait Value:
    fn get(self) -> i32

impl Value for Box:
    fn get(self) -> i32: self.value

enum Status:
    Ready
    Code(value: i32)

fn same[T: Reference](left: T, right: T) -> bool: left is right
fn as_value(value: Value) -> Value: value
fn pending!() -> i32: 1

fn main() -> i32:
    box := Box { value: 1 }
    alias := box
    other := Box { value: 1 }
    callback := fn() -> i32: 1
    callback_alias := callback
    other_callback := fn() -> i32: 1
    values := [1]
    values_alias := values
    first_trait := as_value(box)
    second_trait := as_value(box)
    suspension := pending()
    suspension_alias := suspension
    if same(box, alias) and not (box is other) and callback is callback_alias and not (callback is other_callback) and values is values_alias and first_trait is second_trait and suspension is suspension_alias and Status.Ready is Status.Ready and not (Status.Code(1) is Status.Code(1)):
        42
    else:
        0
`;
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.match(compilation.wat, /ref\.eq/);
  assert.match(compilation.wat, /global \$e0v0/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);

  assert.equal(
    analyze("fn same[T](left: T, right: T) -> bool: left is right\n").diagnostics[0]?.code,
    "identity-needs-reference-bound",
  );
  assert.equal(
    analyze(
      "fn same[T: Reference](left: T, right: T) -> bool: left is right\nfn main() -> bool: same(1, 1)\n",
    ).diagnostics[0]?.code,
    "missing-trait-implementation",
  );
});

test("heterogeneous tuples retain static element types through Wasm GC storage", async () => {
  const source = `fn load!() -> i32: 36
fn pair[T](left: T, right: i32) -> (T, i32): (left, right)

fn main!() -> i32:
    loaded, text := (load!(), "界")
    nested := ((1, 2),)
    single := (1,)
    generic := pair("value", 1)
    let first, second: (i32, i32) = (1, 2)
    first = 2
    loaded + text.len() + nested.0.0 + single.0 + generic.1 + first + second - second
`;
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.match(compilation.wat, /array\.new_fixed \$hd\.list/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.equal(
    analyze("fn same(left: (i32, i32), right: (i32, i32)) -> bool: left is right\n").diagnostics[0]
      ?.code,
    "identity-requires-references",
  );
  assert.equal(
    analyze("fn bad(value: (i32,)) -> i32: value.1\n").diagnostics[0]?.code,
    "tuple-index-range",
  );
});

test("nested closures propagate grandparent captures through GC environments", async () => {
  const source = `fn main() -> i32:
    base := 40
    make := fn(delta: i32) -> fn(i32) -> i32:
        fn(value: i32) -> i32: base + delta + value
    add := make(1)
    add(1)
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /\(type \$env0 \(struct/);
  assert.match(compilation.wat, /\(type \$env1 \(struct/);
  assert.match(compilation.wat, /struct\.get \$env0 \$env0f0/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("closures capture lexical provider overrides when they escape $.with", async () => {
  const source = `fn make_reader() -> (fn() -> i32) $ Backup:
    $.with(Clock=$.use(Backup)):
        fn() -> i32:
            _ := $.use(Clock)
            42

fn main() -> i32 $ Backup:
    reader := make_reader()
    reader()
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /\(type \$env0 \(struct\n\s*\(field \$env0f0 externref\)/);
  assert.equal((instance.exports.main as CallableFunction)({ backup: true }), 42);
});

test("requirement-bearing closure types pass providers at invocation", async () => {
  const source = `fn invoke(callback: fn() -> i32 $ Clock) -> i32 $ Clock:
    callback()

fn main() -> i32 $ Clock:
    reader := fn() -> i32 $ Clock:
        _ := $.use(Clock)
        42
    invoke(reader)
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /type \$sig0 \(func \(param anyref\) \(param externref\)/);
  assert.match(compilation.wat, /call_ref \$sig0/);
  assert.equal((instance.exports.main as CallableFunction)({ clock: true }), 42);
});

test("requirement-bearing named function values receive call-site providers", async () => {
  const source = `fn read() -> i32 $ Clock:
    _ := $.use(Clock)
    42

fn invoke(callback: fn() -> i32 $ Clock) -> i32 $ Clock:
    callback()

fn main() -> i32 $ Clock: invoke(read)
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /func \$fv0[^]*param \$provider0 externref/);
  assert.equal((instance.exports.main as CallableFunction)({ clock: true }), 42);
});

test("closures infer unsatisfied requirements but capture lexical providers", async () => {
  const source = `fn main() -> i32 $ Clock:
    reader := fn() -> i32:
        _ := $.use(Clock)
        42
    reader()
`;
  const analysis = analyze(source);
  assert.deepEqual(analysis.diagnostics, []);
  assert.equal(analysis.hir?.functions[0]?.locals[0]?.type, "fn()->i32$Clock");
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)({ clock: true }), 42);

  const missing = source.replace("fn main() -> i32 $ Clock:", "fn main() -> i32:");
  assert.equal(analyze(missing).diagnostics[0]?.code, "missing-requirement");
});

test("concrete requirement rows thread hidden externref providers", async () => {
  const source = `fn read() -> i32 $ Clock: 40
fn middle() -> i32 $ Clock: read() + 1
fn main() -> i32 $ Clock: middle() + 1
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /param \$provider0 externref/);
  assert.match(compilation.wat, /call \$f0 \(local\.get \$provider0\)/);
  assert.equal((instance.exports.main as CallableFunction)({ now: 0 }), 42);
});

test("calls cannot acquire undeclared requirements", () => {
  const source = "fn read() -> i32 $ Clock: 1\nfn main() -> i32: read()\n";
  assert.equal(analyze(source).diagnostics[0]?.code, "missing-requirement");
});

test("$.use resolves hidden providers and $.with overrides them lexically", async () => {
  const source = `fn main() -> i32 $ Clock + Backup:
    _ := $.use(Clock)
    $.with(Clock=$.use(Backup)):
        _ := $.use(Clock)
        42
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /local\.set \$l0 \(local\.get \$provider0\)/);
  assert.equal((instance.exports.main as CallableFunction)({ backup: true }, { clock: true }), 42);
});

test("provider access and scope errors are diagnosed statically", () => {
  assert.equal(
    analyze("fn main() -> void: _ := $.use(Clock)\n").diagnostics[0]?.code,
    "missing-requirement",
  );
  assert.equal(
    analyze("fn main() -> void:\n    $.with(Clock=1):\n        pass\n").diagnostics[0]?.code,
    "provider-type-mismatch",
  );
});

test("concrete requirement rows normalize union and subtraction as sets", () => {
  const result = analyze("fn main() -> void $ Logger + Clock + Logger - Logger: pass\n");
  assert.deepEqual(result.diagnostics, []);
  assert.deepEqual(result.hir?.functions[0]?.requirements, ["Clock"]);

  const grouped = analyze("fn main() -> void $ (Logger + Clock) - Logger: pass\n");
  assert.deepEqual(grouped.diagnostics, []);
  assert.deepEqual(grouped.hir?.functions[0]?.requirements, ["Clock"]);

  const empty = analyze(
    "fn invoke(callback: fn() -> void $()) -> void: callback()\nfn main() -> void:\n    callback := fn() -> void $(): pass\n    invoke(callback)\n",
  );
  assert.deepEqual(empty.diagnostics, []);
});

test("provider contexts use GC structs and spread into lexical call arguments", async () => {
  const source = `fn make_context() -> $.Context[Clock + Backup] $ Clock + Backup:
    $.context(Clock=$.use(Clock), Backup=$.use(Backup))

fn read() -> i32 $ Clock: 42

fn main() -> i32 $ Clock + Backup:
    context := make_context()
    $.with(Clock=$.use(Backup), ...context):
        read()
`;
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /\(type \$context0 \(struct/);
  assert.match(compilation.wat, /struct\.new \$context0/);
  assert.match(compilation.wat, /struct\.get \$context0 \$context0f0/);
  assert.match(compilation.wat, /call \$f1 \(local\.get \$l/);
  assert.equal((instance.exports.main as CallableFunction)({ backup: true }, { clock: true }), 42);
});

test("context creation accepts spreads and normalizes exact replacement keys", () => {
  const source = `fn make() -> $.Context[Clock + Backup] $ Clock + Backup:
    base := $.context(Clock=$.use(Clock))
    $.context(...base, Backup=$.use(Backup), Clock=$.use(Backup))
`;
  const result = analyze(source);
  assert.deepEqual(result.diagnostics, []);
  const final = result.hir?.functions[0]?.body.at(-1);
  assert.equal(final?.kind, "expression");
  if (final?.kind === "expression") assert.equal(final.expression.type, "context:Backup+Clock");
});

test("context spreads and annotations are checked statically", () => {
  assert.equal(
    analyze("fn main() -> void:\n    $.with(...1):\n        pass\n").diagnostics[0]?.code,
    "context-spread-type",
  );
  assert.equal(
    analyze("fn main() -> $.Context[Clock] $ Clock + Backup: $.context(Backup=$.use(Backup))\n")
      .diagnostics[0]?.code,
    "type-mismatch",
  );
});
