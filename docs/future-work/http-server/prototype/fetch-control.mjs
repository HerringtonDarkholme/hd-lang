// Current-ABI control. Args/Console are an experiment bridge, not a handler ABI.
import { createCoreHost, httpProvider, pendingOperation } from "../../../../host/core.mjs";

export async function invoke(module, request, options = {}) {
  const body = Array.from(new Uint8Array(await request.arrayBuffer()));
  const input = JSON.stringify({
    method: request.method,
    url: request.url,
    headers: [...request.headers],
    body,
  });
  let stdout = "",
    stderr = "",
    blockCalls = 0,
    started = 0,
    aborted = 0;
  const host = createCoreHost(
    {
      out: (s) => {
        stdout += s;
      },
      err: (s) => {
        stderr += s;
      },
    },
    { args: [input] },
    {
      providers: () => ({
        traits: {
          "hd:Http": httpProvider((spec) => {
            // This control grants only the loopback fixture, never arbitrary destinations.
            const url = new URL(spec.url);
            if (url.hostname !== "127.0.0.1") return { error: "NotGranted", text: url.hostname };
            const controller = new AbortController();
            started++;
            let answer;
            fetch(spec.url, {
              method: spec.method,
              headers: spec.headers,
              body: spec.method === "GET" || spec.method === "HEAD" ? undefined : spec.body,
              redirect: "manual",
              signal: controller.signal,
            })
              .then(async (r) => {
                if (r.status >= 300 && r.status < 400)
                  throw new Error("redirect fixtures are outside this control");
                answer = {
                  status: r.status,
                  headers: [...r.headers],
                  body: new Uint8Array(await r.arrayBuffer()),
                  url: r.url,
                };
              })
              .catch((e) => {
                answer = { error: "Other", text: String(e) };
              });
            return pendingOperation(
              () => answer !== undefined,
              () => answer,
              () => {
                aborted++;
                controller.abort();
              },
            );
          }),
        },
      }),
    },
  );
  // Ready-only block: mirror core.complete without its blocking sleep. This
  // proves whether a call actually needs to await the JS event loop.
  host.imports["hd:rt"].block = () => {
    blockCalls++;
    const now = performance.now();
    const due = [...host.core.st.ops].filter(
      ([, o]) => !o.completed && (o.poll ? o.poll() : o.at <= now),
    );
    if (!due.length)
      throw new Error("pending synchronous block_on requires JSPI or another driver");
    const bytes = new Uint8Array(due.length * 4),
      view = new DataView(bytes.buffer);
    due.forEach(([h, o], i) => {
      o.value = o.done?.();
      o.completed = true;
      view.setUint32(i * 4, h, true);
    });
    host.core.put(bytes);
    return due.length;
  };
  const t0 = performance.now();
  const instance = new WebAssembly.Instance(module, host.imports);
  const instantiated = performance.now();
  let requests = 0;
  const signal = request.signal;
  const ask = () => {
    requests++;
  };
  signal.addEventListener("abort", ask, { once: true });
  if (signal.aborted) ask();
  try {
    const result = await host.run(instance, "hd.init", "hd.poll", null, {
      requests: () => requests,
      boundMs: 5,
      wait: (ms) => new Promise((r) => setTimeout(r, ms)),
    });
    const ended = performance.now();
    const memories = Object.entries(instance.exports)
      .filter(([, v]) => v instanceof WebAssembly.Memory)
      .map(([name, v]) => [name, v.buffer.byteLength]);
    if (result.status !== 0)
      return {
        result,
        stderr,
        blockCalls,
        started,
        aborted,
        memories,
        instantiateMs: instantiated - t0,
        totalMs: ended - t0,
      };
    const answer = JSON.parse(stdout.trim());
    const headers = new Headers();
    for (const [k, v] of answer.headers) headers.append(k, v);
    const response = new Response(
      request.method === "HEAD" || [204, 205, 304].includes(answer.status)
        ? null
        : new Uint8Array(answer.body),
      { status: answer.status, headers },
    );
    return {
      response,
      result,
      stderr,
      blockCalls,
      started,
      aborted,
      memories,
      instantiateMs: instantiated - t0,
      totalMs: ended - t0,
    };
  } finally {
    signal.removeEventListener("abort", ask);
    host.close();
  }
}
