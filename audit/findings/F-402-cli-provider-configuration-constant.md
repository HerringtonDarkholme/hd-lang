# F-402: `hd replay` accepts a history recorded under a different runtime profile
Severity: minor
Area: runtime
Evidence: audit/evidence/04-runtime/provider-config.log
Effect: A copy of fixture 32 is recorded with `--profile ready-gate`, where the gate completes at once. `hd replay --profile pending-gate` on it exits 0. A live run under `pending-gate` never finishes. Every CLI event carries `providerConfigurationId: "cli-default"`, whatever the profile. Through the Node API, `prod-v1` against `prod-v2` is rejected correctly.
Recommendation: implementation change: derive the CLI configuration identity from the profile name, for example `profile:ready-gate`.
