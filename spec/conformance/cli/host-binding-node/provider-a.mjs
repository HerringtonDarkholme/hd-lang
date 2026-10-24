import { defineProvider } from "./build/debug/host_app.host.mjs";
export default defineProvider("dep.a.Server", {
  read(text) {
    return "A " + text;
  },
  async wait() {
    await Promise.resolve();
    return "A pending";
  },
});
