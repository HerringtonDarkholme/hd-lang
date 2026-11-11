export default {
  parent(label: string, values: string[]) {
    if (!label || !Array.isArray(values) || !values.includes("one")) {
      throw new Error("parent arguments were lost");
    }
    return 35;
  },
  async child(label: string, values: string[]) {
    await Promise.resolve();
    if (label !== "pending35" || values.join(",") !== "one") {
      throw new Error("pending arguments changed");
    }
    return 7;
  },
};
