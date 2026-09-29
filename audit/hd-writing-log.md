# hd Writing Log

Mistakes that agents made while writing hd code with the cheapest model.
The rule is in [AGENTS.md](../AGENTS.md#writing-hd-code-cheapest-model-and-a-feedback-log).
This log is used to audit compiler diagnostics and documentation.

Append one row per mistake. The columns are:
- **Kind:** `syntax`, `type`, `api`, or `semantic`.
- **Helped?:** `yes`, `partly`, or `no`. Did the message alone lead to the
  fix?
- **Wrote** and **Fix:** keep them short; a single line of code where
  possible.

| Date | Task | Kind | Wrote | Compiler said (verbatim) | Helped? | Fix | Model |
| --- | --- | --- | --- | --- | --- | --- | --- |
