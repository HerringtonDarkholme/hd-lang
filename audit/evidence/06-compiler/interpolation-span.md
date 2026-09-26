# commit bd985d7 | command: hd check <probe> | date 2026-09-25

## interp-span.hd
```
     1	fn main() -> i32:
     2	    x := 1
     3	    _ := "value ${missing_name}"
     4	    x
```
```
audit/probes/compiler/interp-span.hd:1:1: unknown-name: unknown name 'missing_name'
```

## interp-span2.hd
```
     1	fn main() -> i32:
     2	    x := 1
     3	    _ := "value ${x + true}"
     4	    x
```
```
audit/probes/compiler/interp-span2.hd:1:1: type-mismatch: operator operands have types i32 and bool
```
