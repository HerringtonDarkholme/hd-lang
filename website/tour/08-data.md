---
title: Data and copy-with-changes
---

# A record says which fields are required, and `...` copies one with changes

A `data` type lists its fields. A field with a default may be left out, and a
field without one must be given, so a config can't be built half-filled. To make
a variant, `...` copies every field of an existing value and you name only what
changes; the original stays as it was. No builder class, no setter that edits a
config other code is already using.

```hd
data HttpConfig:
    host: string  # ← no default: required
    timeout_ms: i32 = 5000
    retries: i32 = 3

pub fn main() -> void $ Console:
    production := HttpConfig { host: "api.shop.example" }
    payments := HttpConfig { ...production, retries: 0 }  # ← copy, change one field
    for config in [production, payments]:
        println("${config.host}: ${config.timeout_ms} ms, ${config.retries} retries")

# Delete `host: "api.shop.example"` so the first literal
# reads `HttpConfig {}`, and Run:
#     missing-required-field: missing required field 'host'
```

```edit
replace:     production := HttpConfig { host: "api.shop.example" }
with:     production := HttpConfig {}
error: missing-required-field: missing required field 'host'
```

```output
api.shop.example: 5000 ms, 3 retries
api.shop.example: 5000 ms, 0 retries
```
