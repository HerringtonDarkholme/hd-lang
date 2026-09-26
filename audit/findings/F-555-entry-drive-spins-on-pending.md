# F-555: the `main` entry export busy-polls forever when a host provider stays pending
Severity: minor
Area: runtime
Evidence: audit/probes/arch/suspension/pending-spin.hd; `timeout 5 node --experimental-strip-types bin/hd.js run --profile pending-gate audit/probes/arch/suspension/pending-spin.hd` exits 124 (killed after 5 s), recorded in audit/evidence/05-requirements/pending-spin.md
Effect: `hd run` hangs with 100% CPU on any program whose host operation returns Pending. The host never regains control, so it cannot complete the operation or wake the task.
Recommendation: implementation change. On Pending, the entry export should return to the host (as `__hd_poll` does) instead of looping in `$driveN`. Decided rule (spec/11-requirements-and-suspension.md, Suspending Functions): the entry driver returns control to the host while a host provider is pending, and a busy-polling driver is not conforming.
