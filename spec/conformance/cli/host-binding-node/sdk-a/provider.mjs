export default {
  read(text) {
    return "A " + text;
  },
  async wait() {
    await Promise.resolve();
    return "A pending";
  },
};
