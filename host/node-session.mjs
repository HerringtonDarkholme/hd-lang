// Node-only REPL session glue. Kept separate from node.mjs so `hd run`
// does not parse or initialize the journal and session implementation.
import { createOutput } from "./output.mts";
import { createSession } from "./session.mts";
import { lineReader, nodeLayer } from "./node.mjs";

const { readFileSync } = process.getBuiltinModule("node:fs");

export const createNodeSession = (sink, env = {}, options = {}) =>
  createSession(sink, env, nodeLayer, options);

// The CLI owns the user's streams. stdin/stdout of this child carry only
// framed control messages; even a guest read goes through its own request,
// so it cannot consume the next session command. Output frames precede the
// completion frame, which lets the CLI flush them before its next prompt.
export async function driveNodeSession() {
  const control = lineReader(0);
  const receive = () => {
    const line = control();
    if (line === null) {
      return null;
    }
    if (line === false) {
      throw new Error("cannot read the session control stream");
    }
    return JSON.parse(line);
  };
  const output = createOutput();
  const frame = (kind, ...fields) => `${[kind, ...fields].join("\t")}\n`;
  const send = (kind, ...fields) => output.out(frame(kind, ...fields));
  const input = (all) => {
    send(all ? "all" : "input");
    return receive()?.value ?? null;
  };
  const readLine = () => input(false);
  readLine.all = () => input(true);
  let session;
  let recording = true;
  try {
    for (let request; (request = receive()) !== null;) {
      try {
        switch (request.op) {
          case "open":
            session = createNodeSession(
              {
                write: (fd, text) =>
                  output.write(1, frame(fd === 1 ? "out" : "err", JSON.stringify(text))),
                out: (text) => send("out", JSON.stringify(text)),
                err: (text) => send("err", JSON.stringify(text)),
              },
              { args: [], program: "", readLine },
              { journalCap: request.cap },
            );
            send("ready");
            break;
          case "run": {
            const ran = await session.run(
              readFileSync(request.wasm),
              request.sources,
              request.live,
            );
            const notice = session.takeJournalNotice();
            if (notice) {
              recording = false;
            }
            const live = ran.live;
            send(
              "ran",
              ran.kept.join(","),
              ran.skipped.join(","),
              ran.dropped.join(","),
              JSON.stringify(ran.reason),
              live === null ? "-" : String(live.status),
              live === null
                ? "-"
                : live.end.kind === "exit"
                  ? `exit${live.end.status}`
                  : live.end.kind,
              JSON.stringify(live?.error ?? null),
              Number(recording),
              Number(notice),
            );
            break;
          }
          case "clear":
            session.clear();
            recording = true;
            send("cleared");
            break;
          case "close":
            return;
          default:
            throw new Error("unknown session control request");
        }
      } catch (error) {
        send("error", JSON.stringify(String(error?.message ?? error)));
      }
    }
  } finally {
    session?.close();
    await output.close();
  }
}
