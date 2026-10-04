---
title: Option, not null
---

# There is no null: a value that may be missing says so in its type

In a language with null, any value might be missing, and you find out when the
program crashes. In hd, `string?` (short for `Option[string]`) is either
`.Some(text)` or `.None`, and a plain `string` is always there. You can't call
string methods on a `string?` until you decide what a missing value means: a
fallback with `unwrap_or`, a `match`, or `?` to pass the `.None` up. Returning a
plain value where an optional is expected needs no wrapping, as
`return user.email` shows.

```hd
fn email_of(users: List[User], name: string) -> string?:
    for user in users:
        if user.name == name:
            return user.email
    .None

pub fn main() -> void $ Console:
    for name in ["Grace", "Linus"]:
        email := email_of(team(), name)
        println(email.unwrap_or("no email on file"))  # ← you decide what "none" means

# Change the last line to `println(email.lower())` and Run:
#     unknown-method: type 'string?' has no supported method 'lower'

# ── plumbing ──
data User:
    name: string
    email: string

fn team() -> List[User]:
    [
        User { name: "Ada", email: "ada@example.com" },
        User { name: "Grace", email: "grace@example.com" },
    ]
```

```edit
replace:         println(email.unwrap_or("no email on file"))  # ← you decide what "none" means
with:         println(email.lower())
error: unknown-method: type 'string?' has no supported method 'lower'
```

```output
grace@example.com
no email on file
```
