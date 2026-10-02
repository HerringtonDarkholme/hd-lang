#!/usr/bin/env node
// Cache compiled modules (including stripped TypeScript) across runs: the
// test suites start one process per case, and compilation was a third of
// each start-up.
import { enableCompileCache } from "node:module";

enableCompileCache?.();
const { main } = await import("../src/cli.ts");

process.exitCode = await main();
