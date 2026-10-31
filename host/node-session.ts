// Node-only REPL session glue. Kept separate from node.ts so `hd run`
// does not parse or initialize the journal and session implementation.
import { createOutput } from "./output.ts";
import { createSession } from "./session.ts";
import { lineReader, nodeLayer } from "./node.ts";
import type { HostEnv, LineReader, Sink } from "./node.ts";

type Dynamic = ReturnType<typeof JSON.parse>;

const { readFileSync } = process.getBuiltinModule("node:fs");

function endOf(live: Dynamic): string {
  if (live === null) {
    return "-";
  }
  return live.end.kind === "exit" ? `exit${live.end.status}` : live.end.kind;
}

export const createNodeSession = (
  sink: Sink,
  env: HostEnv = {},
  options: { journalCap?: number } = {},
) => createSession(sink, env, nodeLayer, options);

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
  const frame = (kind: string, ...fields: Dynamic[]) => `${[kind, ...fields].join("\t")}\n`;
  const send = (kind: string, ...fields: Dynamic[]): void => output.out(frame(kind, ...fields));
  const input = (all: boolean): Dynamic => {
    send(all ? "all" : "input");
    return receive()?.value ?? null;
  };
  const readLine = (() => input(false)) as LineReader;
  readLine.all = () => input(true);
  let session: ReturnType<typeof createNodeSession> | undefined;
  let recording = true;
  try {
    for (let request; (request = receive()) !== null;) {
      try {
        switch (request.op) {
          case "open":
            session = createNodeSession(
              {
                write: (fd: number, text: string) =>
                  output.write(1, frame(fd === 1 ? "out" : "err", JSON.stringify(text))),
                out: (text: string) => send("out", JSON.stringify(text)),
                err: (text: string) => send("err", JSON.stringify(text)),
              },
              { args: [], program: "", readLine },
              { journalCap: request.cap },
            );
            send("ready");
            break;
          case "run": {
            const ran = await session!.run(
              readFileSync(request.wasm),
              request.sources,
              request.live,
            );
            const notice = session!.takeJournalNotice();
            recording &&= !notice;
            const live = ran.live;
            const end = endOf(live);
            send(
              "ran",
              ran.kept.join(","),
              ran.skipped.join(","),
              ran.dropped.join(","),
              JSON.stringify(ran.reason),
              live === null ? "-" : String(live.status),
              end,
              JSON.stringify(live?.error ?? null),
              Number(recording),
              Number(notice),
            );
            break;
          }
          case "clear":
            session!.clear();
            recording = true;
            send("cleared");
            break;
          case "close":
            return;
          default:
            throw new Error("unknown session control request");
        }
      } catch (error) {
        send("error", JSON.stringify(String(error instanceof Error ? error.message : error)));
      }
    }
  } finally {
    session?.close();
    await output.close();
  }
}
