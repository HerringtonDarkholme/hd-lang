# commit bd985d7 | command: hd record audit/probes/compiler/replay/base.hd; copy base.hd.replay.json to each variant; hd replay <variant> | date 2026-09-25

## variant outside: diff vs base
```
0a1,2
> # a comment above
> 
```
```
42
exit=0
```

## variant inside: diff vs base
```
2a3
>     # note
```
```
file:///Users/hd/code/test/hd-lang/src/compiler.ts:264
        throw new Error(`replay event ${replayIndex} does not match ${reason}`);
              ^
exit=1
```

## variant spacing: diff vs base
```
3c3
<     value + 2
---
>     value  +  2
```
```
file:///Users/hd/code/test/hd-lang/src/compiler.ts:264
        throw new Error(`replay event ${replayIndex} does not match ${reason}`);
              ^
exit=1
```

## error lines
inside: Error: replay event 1 does not match function code identity '13843c85b1440aae'
spacing: Error: replay event 1 does not match function code identity '3c499b6733a06e3d'

Identity source: src/compiler.ts:215-226 (sha256 of source.slice(span.start.offset, span.end.offset), first 16 hex chars).
