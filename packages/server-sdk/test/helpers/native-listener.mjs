import http from "node:http";
import { once } from "node:events";
import { responseHeaders } from "../../adapters/http.mjs";

// Cancelling a rejected upload must not destroy the duplex socket before
// its HTTP response is written. Drain without retaining chunks; the mapper
// still bounds the cancellation acknowledgement with its watchdog.
function incomingBody(incoming) {
  let controller;
  const detach = () => {
    incoming.off("data", data);
    incoming.off("end", end);
    incoming.off("error", error);
  };
  const data = (chunk) => {
    incoming.pause();
    controller.enqueue(new Uint8Array(chunk));
  };
  const end = () => {
    detach();
    controller.close();
  };
  const error = (reason) => {
    detach();
    controller.error(reason);
  };
  return new ReadableStream(
    {
      start(value) {
        controller = value;
        incoming.pause();
        incoming.on("data", data);
        incoming.once("end", end);
        incoming.once("error", error);
      },
      pull() {
        incoming.resume();
      },
      cancel() {
        detach();
        if (incoming.readableEnded || incoming.destroyed) {
          return;
        }
        return new Promise((resolve) => {
          const done = () => {
            incoming.off("end", done);
            incoming.off("close", done);
            incoming.off("error", done);
            resolve();
          };
          incoming.once("end", done);
          incoming.once("close", done);
          incoming.once("error", done);
          incoming.resume();
        });
      },
    },
    { highWaterMark: 0 },
  );
}

// Owned test scaffold. Production deployment/native mapping is a separate slice.
export async function nativeListener(fetcher) {
  const acknowledgements = [],
    failures = [];
  const server = http.createServer(async (incoming, outgoing) => {
    const signal = new AbortController();
    outgoing.once("close", () => {
      if (!outgoing.writableFinished) {
        signal.abort("Disconnected");
      }
    });
    incoming.once("aborted", () => signal.abort("Disconnected"));
    const headers = [];
    for (let i = 0; i < incoming.rawHeaders.length; i += 2) {
      headers.push([incoming.rawHeaders[i], incoming.rawHeaders[i + 1]]);
    }
    const body = ["GET", "HEAD"].includes(incoming.method) ? undefined : incomingBody(incoming);
    try {
      const request = new Request(`http://127.0.0.1${incoming.url}`, {
        method: incoming.method,
        headers,
        body,
        signal: signal.signal,
        ...(body ? { duplex: "half" } : {}),
      });
      const response = await fetcher.fetch(request);
      const bytes = new Uint8Array(await response.arrayBuffer());
      if (outgoing.destroyed) {
        return;
      }
      const cookies = [];
      for (const [name, value] of responseHeaders(response.headers)) {
        if (name.toLowerCase() === "set-cookie") {
          cookies.push(value);
        } else {
          outgoing.setHeader(name, value);
        }
      }
      if (cookies.length) {
        outgoing.setHeader("set-cookie", cookies);
      }
      // Rejection can leave an upload unread. Flush its response before
      // closing, instead of reusing a socket whose parser is still paused.
      if (body && !incoming.readableEnded) {
        outgoing.setHeader("connection", "close");
      }
      outgoing.writeHead(response.status);
      outgoing.end(bytes, () =>
        acknowledgements.push({ path: incoming.url, status: response.status }),
      );
    } catch (error) {
      failures.push({ path: incoming.url, name: error.name });
      if (!outgoing.destroyed) {
        outgoing.destroy();
      }
    }
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  return {
    url: `http://127.0.0.1:${server.address().port}`,
    acknowledgements,
    failures,
    async close() {
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
    },
  };
}
