import { createApplication } from "./application.ts";
import { responseHeaders } from "./http.ts";

const BODY_LIMIT = 1024 * 1024;
const HEADER_LIMIT = 64 * 1024;
const ACTIVE_LIMIT = 8;
const DEADLINE_MS = 20000;
const CLEANUP_BACKSTOP_MS = 200;
const encoder = new TextEncoder();
const token = /^[!#$%&'*+.^_`|~0-9A-Za-z-]+$/;
const standardMethods = new Set(["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"]);
const hostHeaders = new Set([
  "connection",
  "keep-alive",
  "proxy-authenticate",
  "proxy-authorization",
  "te",
  "trailer",
  "transfer-encoding",
  "upgrade",
  "content-length",
]);

class Rejected extends Error {
  constructor(status, message) {
    super(message);
    this.status = status;
  }
}
const errorMessage = (status) => {
  switch (status) {
    case 413:
      return "request body too large";
    case 431:
      return "request headers too large";
    case 400:
      return "bad request";
    case 503:
      return "server unavailable";
    case 504:
      return "request timed out";
    default:
      return "internal server error";
  }
};
const errorResponse = (status) =>
  new Response(errorMessage(status), {
    status,
    headers: status === 503 ? { "retry-after": "1" } : undefined,
  });
const abortError = (reason) =>
  reason instanceof Error ? reason : new DOMException("request aborted", "AbortError");

function headerBytes(pairs) {
  let total = 0;
  for (const pair of pairs) {
    if (
      !Array.isArray(pair) ||
      pair.length !== 2 ||
      typeof pair[0] !== "string" ||
      typeof pair[1] !== "string"
    ) {
      throw new Error("invalid response header pair");
    }
    total += encoder.encode(pair[0]).length + encoder.encode(pair[1]).length + 4;
    if (total > HEADER_LIMIT) {
      return total;
    }
  }
  return total;
}

// Connection names its own hop-by-hop fields. Only this grammar's comma
// list is split; ordinary combined headers, including cookie values, are not.
function publicHeaders(pairs) {
  const removed = new Set(hostHeaders);
  for (const [name, value] of pairs) {
    if (name.toLowerCase() === "connection") {
      for (const field of value.split(",")) {
        const name = field.trim().toLowerCase();
        if (name) {
          removed.add(name);
        }
      }
    }
  }
  return pairs.filter(([name]) => !removed.has(name.toLowerCase()));
}

function validateContentLength(request) {
  const length = request.headers.get("content-length");
  if (length === null) {
    return;
  }
  if (!/^\d+$/.test(length)) {
    throw new Rejected(400, "invalid content length");
  }
  if (BigInt(length) > BigInt(BODY_LIMIT)) {
    throw new Rejected(413, "declared request body too large");
  }
}

async function readChunks(reader, job) {
  const chunks = [];
  let size = 0;
  for (;;) {
    // Cancellation only wakes this owner. Another request's failure/stop
    // must never cancel this request's native reader in its own coroutine.
    const interrupted = new Promise((resolve) => {
      job.wakeRead = resolve;
    });
    let next;
    try {
      next = await Promise.race([reader.read(), interrupted]);
    } finally {
      job.wakeRead = null;
    }
    if (job.controller.signal.aborted) {
      throw abortError(job.controller.signal.reason);
    }
    if (next.done) {
      job.bodyRead = true;
      return { chunks, size };
    }
    if (!(next.value instanceof Uint8Array)) {
      throw new Rejected(400, "request body is not bytes");
    }
    size += next.value.byteLength;
    if (size > BODY_LIMIT) {
      throw new Rejected(413, "streamed request body too large");
    }
    chunks.push(new Uint8Array(next.value));
  }
}

function combinedBody(chunks, size) {
  const body = new Uint8Array(size);
  let offset = 0;
  for (const chunk of chunks) {
    body.set(chunk, offset);
    offset += chunk.length;
  }
  return body;
}

async function readBody(request, job) {
  if (request.bodyUsed) {
    throw new Rejected(400, "request body is already consumed");
  }
  let reader;
  try {
    reader = request.body?.getReader();
  } catch {
    throw new Rejected(400, "request body is already locked");
  }
  job.reader = reader;
  let body;
  try {
    validateContentLength(request);
    if (!reader) {
      job.bodyRead = true;
      return new Uint8Array(0);
    }
    const read = await readChunks(reader, job);
    body = combinedBody(read.chunks, read.size);
  } catch (error) {
    if (!job.controller.signal.aborted) {
      job.rejectBody();
    }
    job.cancelBodyOwned();
    if (error instanceof Rejected || job.reason !== "Rejected") {
      throw error;
    }
    throw new Rejected(400, "request body stream failed");
  } finally {
    job.reader = null;
    try {
      reader?.releaseLock();
    } catch {
      /* A cancellation can have released it. */
    }
  }
  return body;
}

function platformResponse(value, method) {
  if (!value || !Number.isInteger(value.status) || value.status < 200 || value.status > 599) {
    throw new Error("invalid final response status");
  }
  if (!Array.isArray(value.headers) || headerBytes(value.headers) > HEADER_LIMIT) {
    throw new Error("response headers exceed limit");
  }
  const bytes = value.body;
  if (!(bytes instanceof Uint8Array) && !Array.isArray(bytes)) {
    throw new Error("response body is not bytes");
  }
  if (bytes.length > BODY_LIMIT) {
    throw new Error("response body exceeds limit");
  }
  if (
    Array.isArray(bytes) &&
    bytes.some((byte) => !Number.isInteger(byte) || byte < 0 || byte > 255)
  ) {
    throw new Error("response body is not bytes");
  }
  const headers = new Headers();
  for (const [name, content] of publicHeaders(value.headers)) {
    headers.append(name, content);
  }
  const bodyless = method === "HEAD" || [204, 205, 304].includes(value.status);
  return new Response(bodyless ? null : new Uint8Array(bytes), { status: value.status, headers });
}

// The platform entry supplies its matched public module/bindings/runtime and
// explicit production grants. This module never locates a compiler cache.
export function createFetchAdapter(options) {
  return createFetchHandler(createApplication(options), { sink: options.sink });
}

// This smaller mapping seam also permits platform controls with a fake
// application. timers is a host dependency for deterministic policy tests;
// it does not change the fixed admission/deadline/cleanup policy.
export function createFetchHandler(
  application,
  {
    sink,
    timers = {
      set: globalThis.setTimeout.bind(globalThis),
      clear: globalThis.clearTimeout.bind(globalThis),
    },
  } = {},
) {
  const jobs = new Set();
  let stopping = false,
    failed = false;
  const event = (name) => {
    try {
      sink?.err(JSON.stringify({ event: name }) + "\n");
    } catch {
      /* Logs cannot bypass cleanup. */
    }
  };
  const fail = (error, current) => {
    if (failed) {
      return;
    }
    failed = true;
    for (const job of jobs) {
      if (job !== current) {
        cancel(job, "Failed");
      }
    }
    try {
      application.fail(error);
    } catch {
      event("http.shutdown-failure");
    }
  };
  const armCleanup = (job) => {
    if (job.cleanupTimer !== null) {
      return;
    }
    job.cleanupTimer = timers.set(() => {
      event("http.cleanup-timeout");
      fail(new Error("request cleanup exceeded its bound"), job);
      job.escalated();
    }, CLEANUP_BACKSTOP_MS);
  };
  const cancelReader = (job, reason) => {
    if (!job.reader || job.readerCancelled) {
      return;
    }
    try {
      job.readerCancelled = Promise.resolve(job.reader.cancel(reason)).catch(() => {});
    } catch {
      job.readerCancelled = Promise.resolve();
    }
    armCleanup(job);
  };
  const cancel = (job, reason) => {
    if (job.controller.signal.aborted) {
      return;
    }
    job.reason = reason;
    job.controller.abort(reason);
    job.wakeRead?.(null);
    armCleanup(job);
  };
  const dispatchRequest = async (request, job, admittedAt) => {
    if (job.controller.signal.aborted || failed) {
      return null;
    }
    if (typeof request.method !== "string" || !token.test(request.method)) {
      throw new Rejected(400, "invalid request method");
    }
    const pairs = responseHeaders(request.headers);
    if (headerBytes(pairs) > HEADER_LIMIT) {
      throw new Rejected(431, "request headers exceed limit");
    }
    const body = await readBody(request, job);
    if (job.controller.signal.aborted || failed) {
      return null;
    }
    const method = standardMethods.has(request.method)
      ? { tag: request.method[0] + request.method.slice(1).toLowerCase(), fields: {} }
      : { tag: "Other", fields: { name: request.method } };
    return application.dispatch(
      {
        method,
        url: request.url,
        headers: publicHeaders(pairs),
        body,
        redirects: 10,
      },
      job.controller.signal,
      { admittedAt },
    );
  };
  const interruptionResponse = (job, request) => {
    if (job.reason === "Deadline") {
      return errorResponse(504);
    }
    if (job.reason === "Disconnected") {
      throw abortError(request.signal?.reason);
    }
    return job.reason === "Stopping" ? errorResponse(503) : null;
  };
  const finish = async ({ job, request, deadline, disconnected, escalation }) => {
    timers.clear(deadline);
    request.signal?.removeEventListener("abort", disconnected);
    if (!job.bodyRead && !job.readerCancelled && request.body) {
      try {
        job.reader = request.body.getReader();
      } catch {
        /* Already locked/errored. */
      }
      cancelReader(job, "request ended");
      try {
        job.reader?.releaseLock();
      } catch {
        /* Read may still be settling. */
      }
    }
    if (job.readerCancelled) {
      await Promise.race([job.readerCancelled, escalation]);
    }
    if (job.cleanupTimer !== null) {
      timers.clear(job.cleanupTimer);
    }
    jobs.delete(job);
    if (stopping && jobs.size === 0 && !failed) {
      application.stop();
    }
  };
  return Object.freeze({
    async fetch(request) {
      if (stopping || failed) {
        return errorResponse(503);
      }
      if (jobs.size === ACTIVE_LIMIT) {
        event("http.overload");
        return errorResponse(503);
      }
      let escalate;
      const escalation = new Promise((resolve) => {
        escalate = resolve;
      });
      const job = {
        controller: new AbortController(),
        reader: null,
        readerCancelled: null,
        bodyRead: false,
        wakeRead: null,
        reason: null,
        cleanupTimer: null,
        escalated: escalate,
      };
      job.rejectBody = () => cancel(job, "Rejected");
      job.cancelBodyOwned = () => cancelReader(job, job.reason);
      const admittedAt = performance.now();
      jobs.add(job); // Reservation includes header checks and incoming body reads.
      const deadline = timers.set(() => {
        event("http.deadline");
        cancel(job, "Deadline");
      }, DEADLINE_MS);
      const disconnected = () => cancel(job, "Disconnected");
      request.signal?.addEventListener("abort", disconnected, { once: true });
      if (request.signal?.aborted) {
        disconnected();
      }
      try {
        const work = dispatchRequest(request, job, admittedAt);
        // The rejection observer is installed immediately, including after
        // escalation. A late guest failure cannot become an unhandled Promise.
        const result = await Promise.race([work, escalation.then(() => null)]);
        const interrupted = interruptionResponse(job, request);
        if (interrupted) {
          return interrupted;
        }
        if (!result || result.cancelled) {
          return errorResponse(500);
        }
        try {
          return platformResponse(result.response, request.method);
        } catch {
          event("http.response-invalid");
          return errorResponse(500);
        }
      } catch (error) {
        const interrupted = interruptionResponse(job, request);
        if (interrupted) {
          return interrupted;
        }
        if (error instanceof Rejected) {
          return errorResponse(error.status);
        }
        event("http.failure");
        fail(error, job);
        return errorResponse(500);
      } finally {
        await finish({ job, request, deadline, disconnected, escalation });
      }
    },
    stop() {
      if (stopping) {
        for (const job of jobs) {
          cancel(job, "Stopping");
        }
      }
      stopping = true;
      if (jobs.size === 0) {
        application.stop();
      }
    },
    active: () => jobs.size,
  });
}
