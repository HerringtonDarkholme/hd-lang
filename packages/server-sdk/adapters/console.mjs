// Console calls capture a record synchronously, as a memory console does.
// Observer effects run later in the lease owner, never another guest turn.
export function createRequestConsole(lease, { id = lease.id, policy, write }) {
  lease.assertLive();
  if (!policy || typeof policy.allows !== "function")
    throw new Error("server Console requires the compiler Console policy service");
  const identity = Object.freeze({
    generation: String(id.generation),
    sequence: String(id.sequence),
  });
  const capture = (stream, text) => {
    lease.assertLive();
    if (typeof text !== "string") throw new TypeError("a Console record needs text");
    if (!policy.allows())
      return {
        ok: false,
        error: {
          tag: "NotGranted",
          fields: { subject: stream === "out" ? "stdout" : "stderr" },
        },
      };
    const record = Object.freeze({ version: 1, lease: identity, stream, text });
    lease.effect(() => {
      try {
        write(record);
      } catch {
        // A private logging observer cannot prevent response/cleanup release.
      }
    });
    return { ok: true, value: undefined };
  };
  return Object.freeze({
    write: (text) => capture("out", text),
    write_error: (text) => capture("err", text),
  });
}
