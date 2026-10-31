export default {
  read(text: string) {
    return "A " + text;
  },
  async wait() {
    await Promise.resolve();
    return "A pending";
  },
};
