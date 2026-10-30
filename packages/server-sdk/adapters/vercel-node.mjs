import { responseHeaders } from "./http.mjs";

// The physical response closes an unread connection after its rejection flushes.
// Observe native stream completion without making guest cleanup await the client.
function discardIncoming(incoming) {
  if (incoming.readableEnded || incoming.destroyed) {
    return;
  }
  const done = () => {
    incoming.off("end", done);
    incoming.off("close", done);
    incoming.off("error", done);
  };
  incoming.once("end", done);
  incoming.once("close", done);
  incoming.once("error", done);
  incoming.resume();
}

// Preserve stream backpressure while the shared Fetch adapter counts bytes.
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
        discardIncoming(incoming);
      },
    },
    { highWaterMark: 0 },
  );
}

function validatedIncoming(incoming) {
  const headers = [];
  for (let i = 0; i < incoming.rawHeaders.length; i += 2) {
    headers.push([incoming.rawHeaders[i], incoming.rawHeaders[i + 1]]);
  }
  const host = incoming.headers.host;
  if (typeof host !== "string" || !host || /[\s\\/#?@]/.test(host)) {
    throw new Error("invalid request authority");
  }
  if (
    typeof incoming.url !== "string" ||
    !incoming.url.startsWith("/") ||
    Array.from(incoming.url).some(
      (character) =>
        character.charCodeAt(0) <= 32 || character.charCodeAt(0) === 127 || character === "\\",
    )
  ) {
    throw new Error("invalid request target");
  }
  return {
    url: new URL(`https://${host}${incoming.url}`).href,
    headers: new Headers(headers),
  };
}

async function sendResponse(incoming, outgoing, response, body) {
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
  if (body && !incoming.readableEnded) {
    body.cancel().catch(() => {});
    outgoing.setHeader("connection", "close");
  }
  outgoing.writeHead(response.status);
  outgoing.end(bytes);
}

function sendFailure(incoming, outgoing, body, ingressValidated) {
  if (outgoing.destroyed) {
    return;
  }
  if (outgoing.headersSent) {
    outgoing.destroy();
    return;
  }
  body?.cancel().catch(() => {});
  const status = ingressValidated ? 500 : 400;
  outgoing.writeHead(status, {
    "content-type": "text/plain; charset=utf-8",
    connection: "close",
  });
  outgoing.end(ingressValidated ? "internal server error" : "bad request");
  if (!body) {
    discardIncoming(incoming);
  }
}

export function createVercelNodeHandler(
  fetcher,
  { isReady = () => true, retryAfter = "1", log = () => {} } = {},
) {
  return async (incoming, outgoing) => {
    const cancelled = new AbortController();
    const disconnect = () => {
      if (!outgoing.writableFinished) {
        cancelled.abort("Disconnected");
      }
    };
    outgoing.once("close", disconnect);
    incoming.once("aborted", disconnect);
    let body;
    let ingressValidated = false;
    try {
      if (!isReady()) {
        outgoing.writeHead(503, {
          "retry-after": retryAfter,
          connection: "close",
          "content-type": "text/plain; charset=utf-8",
        });
        outgoing.end("server unavailable");
        discardIncoming(incoming);
        return;
      }
      const validated = validatedIncoming(incoming);
      ingressValidated = true;
      body = incomingBody(incoming);
      // Node permits GET/HEAD bodies; Fetch's Request constructor rejects
      // them. The shared mapper deliberately accepts this Request-like view.
      const response = await fetcher.fetch({
        ...validated,
        method: incoming.method,
        body,
        bodyUsed: false,
        signal: cancelled.signal,
      });
      await sendResponse(incoming, outgoing, response, body);
    } catch {
      try {
        log({ kind: "adapter-failure" });
      } catch {}
      sendFailure(incoming, outgoing, body, ingressValidated);
    } finally {
      incoming.off("aborted", disconnect);
      outgoing.off("close", disconnect);
    }
  };
}
