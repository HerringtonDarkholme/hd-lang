---
title: Element references
---

# `for p in &mut players` changes the elements in place, and the list holds still while you do

A plain `for p in players` binds each element as a read-only copy, so a loop that
writes `p.score` would change nothing, and the compiler rejects it. To change
the elements, loop over `&mut players`: each step binds a writable reference to
the element's slot. While such a reference lives, the list may not grow, shrink,
or move its elements, because the reference would point at moved memory. A
`push` in the loop's body panics with `collection-pinned`, and replacing or
sorting elements is still fine. `&mut players[0]` passed to a call holds the
list still for that call only, and no name may keep it: write
`players[0].score += 1` instead. `bonus` takes `&mut List[Player]` so that it
changes the caller's list, not a copy.

```hd
fn bonus(players: &mut List[Player], points: i32) -> void:
    for p in &mut players:  # ← a writable reference per element; the list is pinned
        p.score += points

fn leader(players: List[Player]) -> Player:
    let mut best = players[0]
    for p in players:  # ← a snapshot of the list: read-only copies
        if p.score > best.score:
            best = p
    best

pub fn main() -> void $ Console:
    let mut players = [Player { name: "ada", score: 10 }, Player { name: "grace", score: 12 }]
    bonus(&mut players, 5)
    players[0].score += 1  # ← the element is a place
    best := leader(players)
    println("${best.name}: ${best.score}")
    println(players[0].score)

# Change the loop in `bonus` to `for p in players:` and Run:
#     not-writable: `p` is not `let mut`, so it may only be reassigned as a whole; bind it with `let mut` to change a field

# ── plumbing ──
struct Player:
    name: string
    score: i32
```

```edit
replace:     for p in &mut players:  # ← a writable reference per element; the list is pinned
with:     for p in players:
error: not-writable: `p` is not `let mut`, so it may only be reassigned as a whole; bind it with `let mut` to change a field
```

```output
grace: 17
16
```
