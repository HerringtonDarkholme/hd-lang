// Return a loopback port without keeping the parent's output pipe open.
// The one-request server exits after its delayed response, or after 10 s.
import { fork } from "node:child_process";
import { createServer } from "node:http";
import { fileURLToPath } from "node:url";

if (process.argv[2] === "serve") {
  const server = createServer((_request, response) => {
    setTimeout(() => {
      response.writeHead(200, { "Content-Length": "0" });
      response.end();
      server.close();
    }, 1000);
  });
  server.listen(0, "127.0.0.1", () => {
    const address = server.address();
    if (address === null || typeof address === "string" || process.send === undefined) {
      throw new Error("server did not publish a loopback port");
    }
    process.send(address.port);
  });
  setTimeout(() => server.close(), 10_000).unref();
} else {
  const child = fork(fileURLToPath(import.meta.url), ["serve"], {
    // Windows libuv otherwise puts this child in a kill-on-parent-exit Job.
    // unref/disconnect close handles; detached gives the server its own lifetime.
    detached: true,
    stdio: ["ignore", "ignore", "ignore", "ipc"],
  });
  child.once("message", (port) => {
    process.stdout.write(String(port));
    child.disconnect();
    child.unref();
  });
}
