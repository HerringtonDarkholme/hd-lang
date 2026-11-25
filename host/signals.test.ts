import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { test } from "node:test";

const moduleUrl = new URL("./signals.ts", import.meta.url).href;
const start = (body: string) => {
  const child = spawn(process.execPath, ["--input-type=module", "-e", body], {
    stdio: ["ignore", "ignore", "pipe", "ipc"],
  });
  let errors = "";
  child.stderr!.on("data", (data) => {
    errors += data;
  });
  return {
    child,
    errors: () => errors,
    close: () => {
      child.kill("SIGKILL");
    },
  };
};
for (const [kind, signal] of [
  ["Interrupt", "SIGINT"],
  ["Terminate", "SIGTERM"],
  ["Hangup", "SIGHUP"],
] as const) {
  test(
    `shared source delivers ${kind} once to every waiter, then restores default`,
    {
      skip: process.platform === "win32",
      timeout: 5_000,
    },
    async () => {
      const run = start(`
      import { signalSource, cancelSignals } from ${JSON.stringify(moduleUrl)};
      let woke = 0, cancelled = 0;
      const releaseCancel = cancelSignals(() => { cancelled++; });
      const source = signalSource();
      for (let i=0; i<1000; i++) source.wait(${JSON.stringify(kind)}, () => {
        if (++woke === 1000) {
          releaseCancel();
          process.send({woke, cancelled, listeners: process.listenerCount(${JSON.stringify(signal)})});
        }
      });
      process.on('message', () => {});
      process.send('armed');
    `);
      try {
        assert.deepEqual(await once(run.child, "message"), ["armed", undefined]);
        const received = once(run.child, "message");
        run.child.kill(signal);
        const [result] = await received;
        assert.deepEqual(result, { woke: 1000, cancelled: 0, listeners: 0 });
        const exited = once(run.child, "exit");
        run.child.kill(signal);
        const [code, stopped] = await exited;
        assert.equal(code, null, run.errors());
        assert.equal(stopped, signal);
      } finally {
        run.close();
      }
    },
  );
}

test(
  "cancelled waits cannot wake; the same source resumes entry cancellation",
  {
    skip: process.platform === "win32",
    timeout: 5_000,
  },
  async () => {
    const run = start(`
    import { signalSource, cancelSignals } from ${JSON.stringify(moduleUrl)};
    let woke = 0;
    const releaseWait = signalSource().wait('Interrupt', () => { woke++; });
    releaseWait();
    const releaseCancel = cancelSignals(() => {
      releaseCancel();
      process.send({woke, listeners: process.listenerCount('SIGINT')});
    });
    process.on('message', () => {});
    process.send('armed');
  `);
    try {
      await once(run.child, "message");
      const received = once(run.child, "message");
      run.child.kill("SIGINT");
      assert.deepEqual((await received)[0], { woke: 0, listeners: 0 });
      const exited = once(run.child, "exit");
      run.child.kill("SIGINT");
      assert.deepEqual(await exited, [null, "SIGINT"]);
    } finally {
      run.close();
    }
  },
);

test(
  "a supervised worker receives the original kind from the main process source",
  {
    skip: process.platform === "win32",
    timeout: 5_000,
  },
  async () => {
    const worker = `
    import {workerData, parentPort} from 'node:worker_threads';
    parentPort.on('message', () => {});
    import(${JSON.stringify(moduleUrl)}).then(({connectSignals, signalSource}) => {
      const close = connectSignals(workerData.port, workerData.flag);
      signalSource().wait('Terminate', () => {
        parentPort.postMessage({woke:1, direct:process.listenerCount('SIGTERM')});
        close();
      });
      parentPort.postMessage('armed');
    });
  `;
    const run = start(`
    import {serveSignals, cancelSignals} from ${JSON.stringify(moduleUrl)};
    import {MessageChannel, Worker} from 'node:worker_threads';
    const {port1, port2} = new MessageChannel();
    const flag = new Int32Array(new SharedArrayBuffer(4));
    const close = serveSignals(port1, flag);
    const releaseCancel = cancelSignals(() => { throw Error('handled signal cancelled entry'); });
    const worker = new Worker(${JSON.stringify(worker)}, {eval:true, workerData:{port:port2,flag}, transferList:[port2]});
    worker.on('message', message => {
      if (message === 'armed') process.send('armed');
      else { close(); releaseCancel(); process.send({...message, listeners:process.listenerCount('SIGTERM')}); }
    });
    process.on('message', () => {});
  `);
    try {
      await once(run.child, "message");
      const received = once(run.child, "message");
      run.child.kill("SIGTERM");
      assert.deepEqual((await received)[0], { woke: 1, direct: 0, listeners: 0 });
      const exited = once(run.child, "exit");
      run.child.kill("SIGTERM");
      assert.deepEqual(await exited, [null, "SIGTERM"]);
    } finally {
      run.close();
    }
  },
);

// No OS notification is needed to prove refusal: it must happen before a
// pending operation is registered, in both a browser and a denied Node host.
test("a host without signals and an explicit deny report a grant error", async () => {
  const { createCoreHost } = await import("./core.ts");
  const { createHost } = await import("./node.ts");
  for (const host of [
    createCoreHost({ out() {}, err() {} }),
    createHost({ out() {}, err() {} }, { grants: { Signals: false }, stdin: null }),
  ]) {
    const memory = new WebAssembly.Memory({ initial: 1 });
    host.prime({ exports: { "hd.x": memory } });
    try {
      assert.throws(
        () => host.imports["hd:Signals"]["wait.start"](4),
        /capability not granted: Signals/,
      );
      assert.equal(host.core.st.ops.size, 0);
    } finally {
      host.close();
    }
  }
});
