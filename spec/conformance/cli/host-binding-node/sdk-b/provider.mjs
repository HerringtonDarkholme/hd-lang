export default {
  read(text) {
    return "B " + text;
  },
  async wait() {
    await Promise.resolve();
    return "B pending";
  },
};
