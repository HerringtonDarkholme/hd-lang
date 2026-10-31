import fs from "node:fs";
import path from "node:path";
fs.mkdirSync("granted", { recursive: true });
fs.mkdirSync("outside/deep", { recursive: true });
fs.writeFileSync("outside/deep/x", "outside");
fs.writeFileSync("plain.txt", "plain");
// A directory junction on Windows needs no symlink privilege; on Unix
// Node creates the equivalent directory symlink.
fs.symlinkSync(path.resolve("outside/deep"), "granted/link", "junction");
// Unix follows the link before the following ..; Windows first removes
// .. lexically. Both requests below address outside/deep/x on their OS.
fs.writeFileSync(
  "granted/request",
  process.platform === "win32" ? "granted/link/x" : "granted/link/../deep/x",
);
