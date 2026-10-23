if (!process.argv[2]) throw new Error("pass a scratch evidence directory");
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { writeFileSync } from "node:fs";
let active = 0,
  peak = 0;
const upstream = createServer((req, res) => {
  active++;
  peak = Math.max(peak, active);
  let ended = false;
  const end = () => {
    if (!ended) {
      ended = true;
      active--;
    }
  };
  const t = setTimeout(() => {
    res.end("upstream");
    end();
  }, 30);
  res.on("close", () => {
    clearTimeout(t);
    end();
  });
});
await new Promise((r) => upstream.listen(18768, "127.0.0.1", r));
const rows = [];
try {
  for (const [path, status, expected] of [
    ["json", 200, '{"greeting":"Hello Ada"}'],
    ["timeout", 504, "deadline"],
    ["fetch", 200, "upstream"],
  ]) {
    const r = await fetch(`http://127.0.0.1:18767/${path}`, {
      method: path === "timeout" ? "GET" : "POST",
      body:
        path === "fetch"
          ? "http://127.0.0.1:18768"
          : path === "json"
            ? '{"name":"Ada"}'
            : undefined,
    });
    const body = await r.text();
    rows.push({ path, status: r.status, body });
    assert.equal(r.status, status);
    assert.equal(body, expected);
  }
  const replies = await Promise.all(
    Array.from({ length: 8 }, () =>
      fetch("http://127.0.0.1:18767/fetch", {
        method: "POST",
        body: "http://127.0.0.1:18768",
      }).then(async (r) => ({ status: r.status, body: await r.text() })),
    ),
  );
  for (const r of replies) {
    assert.equal(r.status, 200);
    assert.equal(r.body, "upstream");
  }
  assert.equal(peak, 8);
  rows.push({ path: "concurrent", peak, replies });
  writeFileSync(`${process.argv[2]}/workerd-control.json`, JSON.stringify(rows, null, 2));
  console.log(JSON.stringify(rows));
} finally {
  upstream.closeAllConnections();
  await new Promise((r) => upstream.close(r));
}
