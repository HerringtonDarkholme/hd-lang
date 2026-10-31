// The conformance fixtures' runtime profiles (spec/conformance/README.md,
// "Runtime Profiles"): the host providers of the traits that a fixture
// declares itself. The conformance runner selects one with a run
// configuration's `profile` (run.ts); `hd` never ships this file, and no
// `hd` command names a profile.
//
// `io` is the host's view of the exchange buffer: `put(enc)` writes an
// encoded result and returns its length, `Enc` encodes, `pending(at,
// value)` registers an operation that completes at `at`, and `finish(h)`
// drops it (core.ts).
// Each trait's methods are imported from `hd:<Trait>` by name; every
// argument and result crosses in the buffer.

type Dynamic = ReturnType<typeof JSON.parse>;
type DynamicRecord = Record<string, Dynamic>;

export function providers(name: string, io: DynamicRecord): DynamicRecord {
  switch (name) {
    // `trait Gauge: fn level(self) -> u8`, broken: every call returns 300,
    // which no `u8` holds (module.profile.host-result.contract-panic).
    case "misbehaving-host":
      return { "hd:Gauge": { level: () => io.put(new io.Enc().leb(300)) } };
    // `trait Sensor: fn reading(mut self) -> f64`: a NaN, then positive
    // infinity, then `-0.0` on every later call, each as raw IEEE 754
    // bits (module.profile.host-result.float, module.profile.host-float.special).
    case "special-float-host": {
      const readings = [NaN, Infinity];
      let calls = 0;
      return {
        "hd:Sensor": {
          reading: () => io.put(new io.Enc().f64(calls < readings.length ? readings[calls++] : -0)),
        },
      };
    }
    // `trait Vault: fn keep(self, token: Token) -> Token`: returns its
    // argument's boundary value unchanged, the bytes already at offset 0.
    case "serde-vault":
      return { "hd:Vault": { keep: (len: number) => len } };
    // `trait Files: fn open!(self) -> mut FileHandle` and the handle's
    // `trait FileHandle`: `read!(mut self)` and `close(mut self)`, each
    // returning `Result[_, E]`, where `E` is the fixture's error enum with
    // `Closed` as its second variant. A handle crosses as its number, which
    // every handle method takes first. `open!` and `read!` complete at once
    // and decide their result at `.finish`; after its first `.Ok(())`
    // close, every operation on the handle is `.Err(E.Closed)`
    // (module.resource.closed.after).
    case "disposed-file": {
      // Whether each handle number is still open; numbers are never reused.
      const files = new Map<number, boolean>();
      // The handle number each pending `read!` reads.
      const reads = new Map<number, number>();
      // `.Err(E.Closed)`: `Err` is variant 1, `Closed` variant 1.
      const disposed = () => new io.Enc().leb(1).leb(1);
      const known = (file: number): boolean => {
        if (!files.has(file)) {
          throw new Error(`no disposed-file handle numbered ${file}`);
        }
        return files.get(file)!;
      };
      return {
        "hd:Files": {
          "open.start": () => io.pending({ at: performance.now(), done: null }),
          "open.finish": (h: number) => {
            io.finish(h);
            const file = files.size + 1;
            files.set(file, true);
            return io.put(new io.Enc().leb(file));
          },
        },
        "hd:FileHandle": {
          "read.start": (file: number) => {
            const [status, h] = io.pending({ at: performance.now(), done: null });
            reads.set(h, file);
            return [status, h];
          },
          "read.finish": (h: number) => {
            io.finish(h);
            const file = reads.get(h)!;
            reads.delete(h);
            return io.put(known(file) ? new io.Enc().leb(0).str("") : disposed());
          },
          close: (file: number) => {
            if (!known(file)) {
              return io.put(disposed());
            }
            files.set(file, false);
            return io.put(new io.Enc().leb(0));
          },
        },
      };
    }
    // `trait Tick: fn tick!(self) -> void`: under `tick` every call
    // completes at its start; under `pending-tick` it is pending on its
    // first poll and completes on its second.
    case "tick":
      return { "hd:Tick": { "tick.start": () => [0, 0], "tick.finish": () => 0 } };
    case "pending-tick":
      return {
        "hd:Tick": {
          "tick.start": () => io.pending({ at: performance.now(), done: null }),
          "tick.finish": (h: number) => {
            io.finish(h);
            return 0;
          },
        },
      };
    // `trait Gate: fn wait!(self) -> void`, never resolving: every poll
    // stays pending, dropped when the suspension is cancelled.
    case "pending-gate":
      return {
        "hd:Gate": {
          "wait.start": () => io.pending({ at: Infinity, done: null }),
          "wait.finish": (h: number) => {
            io.finish(h);
            return 0;
          },
        },
      };
    default:
      throw new Error(`no runtime profile named ${name}`);
  }
}
