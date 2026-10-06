import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { Worker } from "node:worker_threads";

import { UsageError } from "../src/cli-args.ts";
import { capabilityFlags, coversHost, type Grant } from "../src/commands/capabilities.ts";
import { hd } from "./hd-in-process.ts";

// The default profile's `Http` provider (spec/std/http.md#sending) and the
// `--cap Http=` grant (spec/cli/command-line.md#grant-scopes). The provider
// blocks the main thread while it waits, so the test's server runs on its
// own worker thread, listens on 127.0.0.1 only, and is stopped by its handle.
// No test contacts another host.

const SERVER = `
const { parentPort } = require("node:worker_threads");
const { createServer } = require("node:http");
const server = createServer((request, response) => {
  const chunks = [];
  request.on("data", (chunk) => chunks.push(chunk));
  request.on("end", () => {
    if (request.url === "/hello") {
      response.setHeader("Content-Type", "text/plain");
      response.setHeader("Set-Cookie", ["a=1", "b=2"]);
      response.end("hi there");
    } else if (request.url === "/echo") {
      response.end(request.method + " " + request.headers["x-tag"] + " " + Buffer.concat(chunks));
    } else if (request.url === "/redirect") {
      response.writeHead(302, { Location: "/hello" }).end();
    } else if (request.url === "/loop") {
      response.writeHead(302, { Location: "/loop" }).end();
    } else if (request.url === "/away") {
      response.writeHead(307, { Location: "http://localhost:" + server.address().port + "/hello" }).end();
    } else if (request.url === "/slow") {
      setTimeout(() => response.end("late"), 2000);
    } else {
      response.writeHead(404).end("no");
    }
  });
});
server.listen(0, "127.0.0.1", () => parentPort.postMessage(server.address().port));
`;

async function withServer(run: (base: string) => Promise<void>): Promise<void> {
  const server = new Worker(SERVER, { eval: true });
  try {
    const port = await new Promise<number>((resolve, reject) => {
      server.once("message", resolve);
      server.once("error", reject);
    });
    await run(`http://127.0.0.1:${port}`);
  } finally {
    await server.terminate();
  }
}

const PROGRAM = `use std.http.{Http, Method, Request, get, send}
use std.host.{Args, args}
use std.time.Duration

fn show!(label: string, url: string) -> void $ Console + Http:
    match get!(url):
        .Ok(response) => println("\${label} \${response.status} \${response.text()}")
        .Err(error) => println("\${label} error: \${error}")

pub fn main!() -> void $ Console + Http + Args:
    base := args()[0]
    match get!("\${base}/hello"):
        .Ok(response) =>
            println("hello \${response.status} \${response.header("content-type").unwrap_or("none")} \${response.text()}")
            println("first cookie \${response.header("SET-COOKIE").unwrap_or("none")}")
            for (name, value) in response.headers:
                if name == "Set-Cookie":
                    println("cookie \${value}")
        .Err(error) => println("hello error: \${error}")
    posted := Request { method: Method.Post, url: "\${base}/echo", headers: [("X-Tag", "a"), ("X-Tag", "b")], body: "ping".to_utf8() }
    match send!(posted):
        .Ok(response) => println("echo \${response.status} \${response.text()}")
        .Err(error) => println("echo error: \${error}")
    show!("redirect", "\${base}/redirect")
    show!("missing", "\${base}/missing")
    show!("loop", "\${base}/loop")
    show!("away", "\${base}/away")
    show!("invalid", "ftp://127.0.0.1/")
    match send!(Request { url: "\${base}/slow", timeout: .Some(Duration::milliseconds(100)) }):
        .Ok(_) => println("slow answered")
        .Err(error) => println("slow error: \${error}")
`;

async function runProgram(flags: readonly string[]): Promise<string> {
  const directory = await mkdtemp(join(tmpdir(), "hd-http-host-"));
  try {
    await writeFile(join(directory, "fetch.hd"), PROGRAM);
    let output = "";
    await withServer(async (base) => {
      const { stdout } = await hd(["fetch.hd", ...flags, "--", base], { cwd: directory });
      output = stdout.replaceAll(base.slice("http://127.0.0.1:".length), "PORT");
    });
    return output;
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

test("hd FILE sends requests from the host, follows redirects, and reports errors", async () => {
  assert.equal(
    await runProgram([]),
    [
      "hello 200 text/plain hi there",
      "first cookie a=1",
      "cookie a=1",
      "cookie b=2",
      "echo 200 POST a, b ping",
      "redirect 200 hi there",
      "missing 404 no",
      "loop error: too many redirects; the last went to http://127.0.0.1:PORT/loop",
      "away 200 hi there",
      "invalid error: invalid URL: ftp://127.0.0.1/",
      "slow error: the request timed out",
      "",
    ].join("\n"),
  );
});

test("--cap Http= refuses a host outside the grant, on a request or a redirect", async () => {
  assert.equal(
    await runProgram(["--cap", "Http=127.0.0.1"]),
    [
      "hello 200 text/plain hi there",
      "first cookie a=1",
      "cookie a=1",
      "cookie b=2",
      "echo 200 POST a, b ping",
      "redirect 200 hi there",
      "missing 404 no",
      "loop error: too many redirects; the last went to http://127.0.0.1:PORT/loop",
      "away error: http access to localhost is not granted; run with --cap Http=localhost",
      "invalid error: invalid URL: ftp://127.0.0.1/",
      "slow error: the request timed out",
      "",
    ].join("\n"),
  );
  const denied = await runProgram(["--cap", "Http=false"]);
  assert.match(denied, /^hello error: http access to 127\.0\.0\.1 is not granted/);
});

const grant = (...values: string[]): Grant => capabilityFlags(values, "hd FILE").get("Http")!;
const covers = (value: Grant, url: string): boolean => coversHost(value, new URL(url));

test("an Http entry matches a host, a *. wildcard, an address, and a port", () => {
  const hosts = grant("Http=api.example.com,*.example.org,localhost:8080,10.0.0.1,[::1]:443");
  assert.equal(covers(hosts, "https://api.example.com/x"), true);
  assert.equal(covers(hosts, "https://API.Example.com:9/x"), true);
  assert.equal(covers(hosts, "https://example.com/"), false);
  assert.equal(covers(hosts, "https://a.b.example.org/"), true);
  assert.equal(covers(hosts, "https://example.org/"), false);
  assert.equal(covers(hosts, "http://localhost:8080/"), true);
  assert.equal(covers(hosts, "http://localhost/"), false);
  assert.equal(covers(hosts, "http://10.0.0.1:5/"), true);
  assert.equal(covers(hosts, "https://[0:0::1]/"), true);
  assert.equal(covers(hosts, "http://[::1]/"), false);
  assert.equal(covers(grant("Http=true"), "http://anything.test/"), true);
  assert.equal(covers(grant("Http=a.test", "Http=b.test"), "http://b.test/"), true);
  // `false` wins over every other flag (cli.cap.order.deny).
  assert.equal(grant("Http=true", "Http=false").kind, "deny");
  assert.equal(grant("Http=a.test", "Http=true").kind, "all");
});

test("--cap rejects an unknown name, a malformed entry, and a list for an unscoped trait", () => {
  assert.throws(() => capabilityFlags(["Htp=x"], "hd FILE"), UsageError);
  assert.throws(() => capabilityFlags(["Http"], "hd FILE"), UsageError);
  assert.throws(() => capabilityFlags(["Http=http://x"], "hd FILE"), UsageError);
  assert.throws(() => capabilityFlags(["Http=x:"], "hd FILE"), UsageError);
  assert.throws(() => capabilityFlags(["Http=*.[::1]"], "hd FILE"), UsageError);
  assert.throws(
    () => capabilityFlags(["Console=stdout"], "hd FILE"),
    /takes only true or false, since Console has no scope entries/,
  );
});
