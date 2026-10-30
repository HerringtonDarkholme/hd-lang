// The shared guest may poll in any request. Native I/O starts and disposal
// belong to the request coroutine which calls lease.run(), never that poll.
let nextGeneration = 0n;

export function createLeaseDispatcher({ drive, failed = () => {} }) {
  const generation = ++nextGeneration;
  const jobs = new Map();
  let sequence = 0n;
  let failure = null;

  const lookup = (id) => {
    const job = id.generation === generation ? jobs.get(id.sequence) : null;
    if (!job || job.finished) {
      throw new Error("expired request lease");
    }
    return job;
  };
  const signal = (job) => job.wake?.();
  const enqueue = (job, task) => {
    job.tasks.push(task);
    signal(job);
  };
  const turn = () => {
    if (failure) {
      return;
    }
    try {
      drive();
    } catch (error) {
      poison(error);
    }
  };
  const dispose = (job, operation) => {
    if (!operation.live) {
      return;
    }
    operation.live = false;
    job.operations.delete(operation);
    enqueue(job, () => operation.cleanup?.());
  };
  const finish = (job, abandoned) => {
    if (job.finished) {
      return;
    }
    job.finished = true;
    job.abandoned = abandoned;
    for (const operation of job.operations) {
      dispose(job, operation);
    }
    signal(job);
  };
  function poison(error) {
    if (failure) {
      return;
    }
    failure = error instanceof Error ? error : new Error(String(error));
    for (const job of jobs.values()) {
      finish(job, true);
    }
    // Cleanup is queued before a user callback can close the core host.
    try {
      failed(failure);
    } catch {
      // A failing shutdown observer must not bypass native disposal in
      // the request owners. Their result still reports the original fault.
    }
  }

  function open(request) {
    if (failure) {
      throw new Error("server instance failed");
    }
    const job = {
      id: Object.freeze({ generation, sequence: ++sequence }),
      request,
      tasks: [],
      operations: new Set(),
      wake: null,
      running: false,
      finished: false,
      abandoned: false,
      cancelled: null,
      response: null,
    };
    jobs.set(job.id.sequence, job);
    const check = () => lookup(job.id);
    return Object.freeze({
      id: job.id,
      request,
      assertLive() {
        check();
      },
      effect(task) {
        check();
        if (typeof task !== "function") {
          throw new TypeError("a lease effect must be a function");
        }
        enqueue(job, task);
      },
      operation(context, start) {
        check();
        const operation = {
          live: true,
          ready: false,
          value: undefined,
          error: null,
          cleanup: null,
        };
        job.operations.add(operation);
        const settle = (value, error = null) => {
          if (!operation.live || job.finished || operation.ready) {
            return;
          }
          operation.value = value;
          operation.error = error;
          operation.ready = true;
          turn();
        };
        enqueue(job, () => {
          if (!operation.live) {
            return;
          }
          try {
            operation.cleanup = start(
              (value) => settle(value),
              (error) =>
                settle(undefined, error instanceof Error ? error : new Error(String(error))),
            );
            if (operation.cleanup != null && typeof operation.cleanup !== "function") {
              throw new TypeError("an operation start must return a cleanup function or void");
            }
          } catch (error) {
            settle(undefined, error);
          }
        });
        return context.pending(
          () => operation.ready,
          () => {
            if (operation.error) {
              throw operation.error;
            }
            return operation.value;
          },
          () => dispose(job, operation),
          () => {
            dispose(job, operation);
          },
        );
      },
      cancelled(context) {
        check();
        return context.pending(
          () => job.cancelled !== null,
          () => ({ tag: job.cancelled, fields: {} }),
        );
      },
      cancel(reason) {
        if (job.finished || job.cancelled !== null) {
          return;
        }
        job.cancelled = reason;
        turn();
        signal(job);
      },
      respond(response) {
        check();
        if (job.cancelled) {
          return false;
        }
        if (job.response) {
          throw new Error("duplicate response");
        }
        // Copy data out before releasing the guest's lease. Native Response
        // construction belongs to the platform caller after run() settles.
        job.response = {
          status: response.status,
          headers: response.headers.map(([key, value]) => [key, value]),
          body: new Uint8Array(response.body),
          url: response.url,
        };
        return true;
      },
      finish(abandoned = false) {
        finish(check(), abandoned);
      },
      async run() {
        if (job.running) {
          throw new Error("request lease already has an owner");
        }
        job.running = true;
        try {
          turn();
          for (;;) {
            // Disposal can enqueue more disposal, so drain before leaving.
            while (job.tasks.length) {
              const tasks = job.tasks.splice(0);
              for (const task of tasks) {
                try {
                  task();
                } catch (error) {
                  poison(error);
                }
              }
            }
            if (job.finished) {
              break;
            }
            await new Promise((resolve) => {
              job.wake = resolve;
            });
            job.wake = null;
          }
          if (failure) {
            throw failure;
          }
          return {
            response: job.response,
            cancelled: job.cancelled,
            abandoned: job.abandoned,
          };
        } finally {
          job.wake = null;
          jobs.delete(job.id.sequence);
        }
      },
    });
  }
  return Object.freeze({ open, fail: poison, active: () => jobs.size });
}
