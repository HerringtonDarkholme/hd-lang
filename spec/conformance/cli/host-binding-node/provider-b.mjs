import { defineProvider } from "./build/debug/host_app.host.mjs";
export default defineProvider("dep.b.Server", {
  read(text) {
    return "B " + text;
  },
  async wait() {
    await Promise.resolve();
    return "B pending";
  },
});
