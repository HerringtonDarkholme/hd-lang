# commit bd985d7 | command: hd run + hd build --wat audit/probes/compiler/frame-liveness.hd | grep struct.set frame-local stores | date 2026-09-25
```
     1	fn step!(value: i32) -> i32: value + 1
     2	
     3	fn main!() -> i32:
     4	    a := 1
     5	    b := a + 1
     6	    c := b + 1
     7	    d := c + 1
     8	    first := step!(d)
     9	    second := step!(first)
    10	    second
```
hd run: 6
```
   5 struct.set $s1 $s1l0
   5 struct.set $s1 $s1l1
   5 struct.set $s1 $s1l2
   5 struct.set $s1 $s1l3
   5 struct.set $s1 $s1l4
   5 struct.set $s1 $s1l5
```
main! has 6 non-parameter locals (a, b, c, d, first, second); every one of l0..l5 is stored at all 5 store points, although a, b, c are dead before the first suspension.
