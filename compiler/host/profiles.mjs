// The conformance fixtures' runtime profiles (spec/conformance/README.md,
// "Runtime Profiles"): the host providers of the traits that a fixture
// declares itself. The conformance runner selects one with a run
// configuration's `profile` (run.mjs); `hd` never ships this file, and no
// `hd` command names a profile.
//
// `io` is the host's view of the exchange buffer: `put(enc)` writes an
// encoded result and returns its length, and `Enc` encodes (core.mjs).
// Each trait's methods are imported from `hd:<Trait>` by name; every
// argument and result crosses in the buffer.

export function providers(name, io) {
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
      return { "hd:Vault": { keep: (len) => len } };
    // `pending-write` is the host's own `Console`, whose `write_line!` is
    // pending on its first poll and writes the line on its second.
    case "pending-write":
      return {};
    default:
      throw new Error(`no runtime profile named ${name}`);
  }
}
