export default {
  read(text: string) {
    return "B " + text;
  },
  async wait() {
    await Promise.resolve();
    return "B pending";
  },
};
