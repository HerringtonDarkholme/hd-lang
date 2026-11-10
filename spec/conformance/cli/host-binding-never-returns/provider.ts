// A provider that returns from a method that never returns breaks its contract.
export default {
  stop(): never {
    return undefined as never;
  },
};
