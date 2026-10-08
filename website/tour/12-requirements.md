---
title: Requirements and providers
---

# A signature lists what a function touches, so a test can hand it a fake

A function that prints, sends mail, or reads the clock usually looks like any
other function: you learn what it touches by reading its body, and testing it
means patching globals or adding a mocking framework. In hd a public signature
lists what the function needs after `$`. `$ Console` says that `remind_unpaid`
writes to the console, and nothing else. The caller provides the console:
`main` passes on the real one, and the test uses `$.with` to put a recording
`BufferConsole` in its place for one block, then reads what was written. A test
gets no real console at all, so a test that forgets the fake does not compile.
Your own traits work the same way: a `Mailer`, a `Clock`, a `PaymentGateway`.

```hd
pub fn remind_unpaid(invoices: List[Invoice]) -> void $ Console:  # ← its whole reach
    for invoice in invoices:
        if !invoice.paid:
            println("Reminder: invoice ${invoice.id} is due")

tests:
    use std.testing.assert_equal

    it("reminds only the unpaid invoices"):
        let console: mut BufferConsole = BufferConsole::new()
        $.with(Console=console):  # ← a recording console, for this block only
            remind_unpaid(sample())
        assert_equal(console.output(), ["Reminder: invoice INV-7 is due"], reason="one reminder")

# Press Test to run the test. Then delete ` $ Console` from the
# first line and Run. A public function can't hide what it touches:
#     missing-requirement: call to 'println' requires Console

# ── plumbing ──
use std.console.BufferConsole
pub data Invoice:
    id: string
    paid: bool

fn sample() -> List[Invoice]:
    [Invoice { id: "INV-7", paid: false }, Invoice { id: "INV-8", paid: true }]

pub fn main() -> void $ Console:
    remind_unpaid(sample())
```

```edit
replace: pub fn remind_unpaid(invoices: List[Invoice]) -> void $ Console:  # ← its whole reach
with: pub fn remind_unpaid(invoices: List[Invoice]) -> void:
error: missing-requirement: call to 'println' requires Console
```

```output
Reminder: invoice INV-7 is due
```

```tests
1 test passed
```
