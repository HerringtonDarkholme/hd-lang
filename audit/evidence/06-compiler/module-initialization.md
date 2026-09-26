# commit bd985d7 | command: hd check / hd run on init probes | date 2026-09-25

## init-direct.hd
```
     1	first := read_later()
     2	later := 5
     3	
     4	fn read_later() -> i32:
     5	    later
     6	
     7	fn main() -> i32:
     8	    first
```
```
audit/probes/compiler/init-direct.hd:1:1: top-level-read-before-initialization: top-level statement may read module binding 'later' before initialization
check exit=1
hd run output:
audit/probes/compiler/init-direct.hd:1:1: top-level-read-before-initialization: top-level statement may read module binding 'later' before initialization
```

## init-dispatch.hd
```
     1	data Box:
     2	    value: i32
     3	
     4	trait Reader:
     5	    fn read(self) -> i32
     6	
     7	fn via_bound[T: Reader](value: T) -> i32:
     8	    value.read()
     9	
    10	direct := Box { value: 1 }.read()
    11	text := "${Box { value: 1 }}"
    12	bounded := via_bound(Box { value: 2 })
    13	let dynamic_value: Reader = Box { value: 3 }
    14	dynamic := dynamic_value.read()
    15	later := 5
    16	
    17	impl Display for Box:
    18	    fn to_string(self) -> string:
    19	        "box ${later}"
    20	
    21	impl Reader for Box:
    22	    fn read(self) -> i32:
    23	        later
    24	
    25	fn main() -> i32:
    26	    direct + bounded + dynamic + text.len()
```
```
audit/probes/compiler/init-dispatch.hd:10:1: top-level-read-before-initialization: top-level statement may read module binding 'later' before initialization
audit/probes/compiler/init-dispatch.hd:11:1: top-level-read-before-initialization: top-level statement may read module binding 'later' before initialization
check exit=1
hd run output:
audit/probes/compiler/init-dispatch.hd:10:1: top-level-read-before-initialization: top-level statement may read module binding 'later' before initialization
audit/probes/compiler/init-dispatch.hd:11:1: top-level-read-before-initialization: top-level statement may read module binding 'later' before initialization
```

## init-dispatch-miss.hd
```
     1	data Box:
     2	    value: i32
     3	
     4	trait Reader:
     5	    fn read(self) -> i32
     6	
     7	fn via_bound[T: Reader](value: T) -> i32:
     8	    value.read()
     9	
    10	bounded := via_bound(Box { value: 2 })
    11	let dynamic_value: Reader = Box { value: 3 }
    12	dynamic := dynamic_value.read()
    13	later := 5
    14	
    15	impl Reader for Box:
    16	    fn read(self) -> i32:
    17	        later
    18	
    19	fn main() -> i32:
    20	    bounded * 100 + dynamic * 10 + later
```
```
audit/probes/compiler/init-dispatch-miss.hd: ok
check exit=0
hd run output:
5
```
