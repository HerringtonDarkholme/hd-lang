export default {
  wait(kind: string) {
    if (kind === "ready") {
      return 7;
    }
    if (kind === "promise") {
      return Promise.resolve(7);
    }
    return new Promise<number>((resolve) => setTimeout(() => resolve(7), 1));
  },
};
