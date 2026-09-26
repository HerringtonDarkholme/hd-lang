<!-- commit bd985d7; command: timeout 5 node --experimental-strip-types bin/hd.js run --profile pending-gate audit/probes/arch/suspension/pending-spin.hd; date: 2026-09-25T20:49:43Z -->

```text
exit=124 after 5 s
```

For contrast, the dev driver returns Pending to the host:

```text
spec/conformance/runtime/valid/cancellation-runs-defer.hd: 1 passed
exit=0
```
