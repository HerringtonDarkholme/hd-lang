import { defineProvider } from "./build/debug/surface.host.mjs";
export default defineProvider("pkg.main.Surface", {
  async render(commands) {
    return commands.length;
  },
  async events() {
    let closed = false;
    return {
      async next() {
        return closed
          ? { ok: false, error: { tag: "Closed", fields: {} } }
          : { ok: true, value: { tag: "Click", fields: { tag: 7 } } };
      },
      async dispose() {
        closed = true;
      },
    };
  },
});
