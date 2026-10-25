// #819: the first action in the leader starts a descendant, before markers.
import { spawn } from "node:child_process";
import { writeFileSync } from "node:fs";
import net from "node:net";

const [mode, root, port] = process.argv.slice(2);
if (mode !== "descendant") {
  const child = spawn(process.execPath, [import.meta.filename, "descendant", root, port], {
    stdio: mode === "detached" ? "ignore" : ["ignore", "inherit", "inherit"],
  });
  child.unref();
  writeFileSync(root + ".parent", String(process.pid));
  if (mode === "running") setTimeout(() => process.exit(99), 30_000);
} else {
  // A socket is a physical liveness witness even when PID reuse or an
  // unreaped Unix zombie makes process.kill(pid, 0) ambiguous.
  const socket = net.connect(Number(port), "127.0.0.1", () => {
    writeFileSync(root + ".awake", String(process.pid));
  });
  socket.on("error", () => process.exit(2));
  socket.on("data", () => {
    writeFileSync(root + ".released", "alive");
    socket.end();
  });
  socket.on("close", () => process.exit(0));
  setTimeout(() => process.exit(3), 30_000).unref();
}
