// The host never answers `stop`: it ends the process.
export default {
  stop(): never {
    console.log("stopping");
    process.exit(0);
  },
};
