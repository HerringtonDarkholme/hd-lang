# commit bd985d7 | command: hd check <probe> for each error-recovery probe | date 2026-09-25

## audit/probes/compiler/errors-types.hd
```
     1	fn a() -> i32:
     2	    true
     3	
     4	fn b() -> string:
     5	    1
     6	
     7	fn c() -> i32:
     8	    x := "s"
     9	    x + 1
    10	
    11	fn d() -> i32:
    12	    unknown_fn()
    13	
    14	fn e() -> i32:
    15	    let y: i32 = "no"
    16	    let z: bool = 3
    17	    0
    18	
    19	fn main() -> i32:
    20	    0
```
hd check output:
```
audit/probes/compiler/errors-types.hd:2:5: type-mismatch: expected i32, found bool
audit/probes/compiler/errors-types.hd:5:5: type-mismatch: expected string, found i32
audit/probes/compiler/errors-types.hd:9:5: type-mismatch: operator operands have types string and i32
audit/probes/compiler/errors-types.hd:12:5: unknown-name: unknown function 'unknown_fn'
audit/probes/compiler/errors-types.hd:15:18: type-mismatch: expected i32, found string
exit=1
```

## audit/probes/compiler/errors-parse.hd
```
     1	fn a() -> i32:
     2	    1 +
     3	
     4	fn b() -> i32:
     5	    (2
     6	
     7	fn c( -> i32:
     8	    3
     9	
    10	fn main() -> i32:
    11	    0
```
hd check output:
```
audit/probes/compiler/errors-parse.hd:5:5: unclosed-delimiter: unclosed '(' delimiter
audit/probes/compiler/errors-parse.hd:7:5: unclosed-delimiter: unclosed '(' delimiter
exit=1
```

## audit/probes/compiler/errors-parse2.hd
```
     1	fn a() -> i32:
     2	    1 +
     3	
     4	fn b() -> i32:
     5	    let = 2
     6	
     7	fn c() -> i32:
     8	    3 3
     9	
    10	fn main() -> i32:
    11	    0
```
hd check output:
```
audit/probes/compiler/errors-parse2.hd:2:8: expected-expression: expected an expression, found '
'
exit=1
```

## audit/probes/compiler/errors-mixed.hd
```
     1	fn a() -> i32:
     2	    1 +
     3	
     4	fn b() -> i32:
     5	    true
```
hd check output:
```
audit/probes/compiler/errors-mixed.hd:2:8: expected-expression: expected an expression, found '
'
exit=1
```

## audit/probes/compiler/errors-signature.hd
```
     1	fn a(x: Missing) -> i32:
     2	    0
     3	
     4	fn b() -> i32:
     5	    true
     6	
     7	fn main() -> i32:
     8	    0
```
hd check output:
```
audit/probes/compiler/errors-signature.hd:1:9: unknown-type: unknown or unsupported type 'Missing'
exit=1
```
