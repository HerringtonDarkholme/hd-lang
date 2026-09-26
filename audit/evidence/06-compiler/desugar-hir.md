# commit bd985d7 | command: hd dump-hir <probe> > json; node --experimental-strip-types audit/scripts/compiler/hir-shape.ts json | date 2026-09-25

## audit/probes/compiler/desugar.hd
```
use std.testing.assert_equal


data Point:
    x: i32
    y: i32 = 7

fn add(a: i32, b: i32 = 10) -> i32: a + b

fn total(values: i32...) -> i32:
    let sum: i32 = 0
    for v in values:
        sum = sum + v
    sum

fn apply(callback: fn() -> i32) -> i32: callback()

fn main() -> i32:
    p := Point { x: 1 }
    q := Point { ...p, x: 3 }
    doubled := [for v in [1, 2, 3] if v > 1 => v * 2]
    _ := "q=${q.x} n=${add(1)}"
    t := total(1, 2, 3)
    r := apply:
        5
    q.y + t + r + doubled.len()

test "adds":
    assert_equal(add(1, 2), 3, reason="sum")
```
### hd run: 20
```
== add synthetic=false requirements=[]
  expression 
    binary : i32
      local : i32
      local : i32

== total synthetic=false requirements=[]
  binding 
    integer value=0 : i32
  expression 
    for iteratorKind=list : void
      local : list[i32]
      assignment 
        binary : i32
          local : i32
          local : i32
  expression 
    local : i32

== apply synthetic=false requirements=[]
  expression 
    closure-call : i32
      local : fn()->i32

== main synthetic=false requirements=[]
  binding 
    data : Point
      integer value=1 : i32
      call functionName=$default.Point.y : i32
  binding 
    data : Point
      local : Point
      integer value=3 : i32
  binding 
    permission-weaken : list[i32]
      list-comprehension : mut:list[i32]
        for iteratorKind=list
          list : mut:list[i32]
            integer value=1 : i32
            integer value=2 : i32
            integer value=3 : i32
        if 
          value-ordering : bool
            local : i32
            integer value=1 : i32
            builtin 
        binary : i32
          local : i32
          integer value=2 : i32
  discard 
    string-build : string
      string : string
      display : string
        member : i32
          local : Point
      string : string
      display : string
        call functionName=add : i32 defaults=[{"parameterIndex":1,"functionIndex":5}]
          integer value=1 : i32
  binding 
    call functionName=total : i32
      list : list[i32]
        integer value=1 : i32
        integer value=2 : i32
        integer value=3 : i32
  binding 
    call functionName=apply : i32
      closure : fn()->i32
  expression 
    binary : i32
      binary : i32
        binary : i32
          member : i32
            local : Point
          local : i32
        local : i32
      list-length : i32
        local : list[i32]

== $test.0 synthetic=true requirements=[]
  expression 
    assert-equal : void
      call functionName=add : i32
        integer value=1 : i32
        integer value=2 : i32
      integer value=3 : i32
      string : string
      builtin 

== $parameter-default.add.b synthetic=true requirements=[]
  expression 
    integer value=10 : i32

== $default.Point.y synthetic=true requirements=[]
  expression 
    integer value=7 : i32

== $closure0 synthetic=true requirements=[]
  expression 
    integer value=5 : i32

```
named-argument calls with argumentParameterIndices (reordering left to emitter):
   data  {"spread":"local"}
   call add {"defaultArguments":[{"parameterIndex":1,"functionIndex":5}]}
distinct HIR type strings: i32 , list[i32] , void , fn()->i32 , Point , mut:list[i32] , bool , string

## audit/probes/compiler/varargs-named.hd
```
# test: homogeneous varargs lower through the existing list ABI
# expect-result: main = 34

fn count(values: i32...) -> i32: values.len()
fn first_or(values: i32...) -> i32:
    if values.len() == 0:
        9
    else:
        values[0]
fn generic_first[T](values: T...) -> T: values[0]
fn tagged(tag: i32, values: i32...) -> i32: tag * 10 + values[0]
fn main() -> i32:
    items := [4, 5, 6]
    count() + count(1, 2, 3) + first_or() + first_or(items...) + generic_first(6) + tagged([2]..., tag=1)
```
### hd run: 34
```
== count synthetic=false requirements=[]
  expression 
    list-length : i32
      local : list[i32]

== first_or synthetic=false requirements=[]
  expression 
    if : i32
      value-equality : bool
        list-length : i32
          local : list[i32]
        integer value=0 : i32
        builtin 
      expression 
        integer value=9 : i32
      expression 
        list-index : i32
          local : list[i32]
          integer value=0 : i32

== generic_first synthetic=false requirements=[]
  expression 
    list-index : generic:T
      local : list[generic:T]
      integer value=0 : i32

== tagged synthetic=false requirements=[]
  expression 
    binary : i32
      binary : i32
        local : i32
        integer value=10 : i32
      list-index : i32
        local : list[i32]
        integer value=0 : i32

== main synthetic=false requirements=[]
  binding 
    list : list[i32]
      integer value=4 : i32
      integer value=5 : i32
      integer value=6 : i32
  expression 
    binary : i32
      binary : i32
        binary : i32
          binary : i32
            binary : i32
              call functionName=count : i32
                list : list[i32]
              call functionName=count : i32
                list : list[i32]
                  integer value=1 : i32
                  integer value=2 : i32
                  integer value=3 : i32
            call functionName=first_or : i32
              list : list[i32]
          call functionName=first_or : i32
            local : list[i32]
        call functionName=generic_first : i32
          list : list[i32]
            integer value=6 : i32
      call functionName=tagged : i32
        list : list[i32]
          integer value=2 : i32
        integer value=1 : i32

```
named-argument calls with argumentParameterIndices (reordering left to emitter):
   call tagged {"argumentParameterIndices":[1,0]}
distinct HIR type strings: list[i32] , i32 , bool , list[generic:T] , generic:T
