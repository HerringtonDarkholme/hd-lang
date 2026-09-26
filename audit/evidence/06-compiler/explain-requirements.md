# commit bd985d7 | command: hd explain-requirements <probe> | date 2026-09-25

## audit/probes/compiler/explain-paths.hd
```
     1	trait Logger:
     2	    fn write(self, message: string) -> void
     3	
     4	fn log_message(message: string) -> i32 $ Logger:
     5	    logger := $.use(Logger)
     6	    logger.write(message)
     7	    1
     8	
     9	fn direct() -> i32 $ Logger:
    10	    log_message("a")
    11	
    12	fn in_for(items: list[string]) -> void $ Logger:
    13	    for item in items:
    14	        _ := log_message(item)
    15	
    16	fn in_tuple() -> (i32, i32) $ Logger:
    17	    (log_message("a"), 2)
    18	
    19	fn in_interp() -> string $ Logger:
    20	    "n=${log_message("a")}"
    21	
    22	fn in_comprehension(items: list[string]) -> list[i32] $ Logger:
    23	    [for item in items => log_message(item)]
    24	
    25	fn main() -> i32:
    26	    0
```
```
log_message: $ Logger
  Logger: log_message -> $.use(Logger)
direct: $ Logger
  Logger: direct -> log_message -> $.use(Logger)
in_for: $ Logger
  Logger: in_for -> declared
in_tuple: $ Logger
  Logger: in_tuple -> declared
in_interp: $ Logger
  Logger: in_interp -> declared
in_comprehension: $ Logger
  Logger: in_comprehension -> declared
main: $()
```

## audit/probes/compiler/row-local.hd
```
     1	trait Logger:
     2	    fn write(self, message: string) -> void
     3	
     4	data ConsoleLogger: pass
     5	
     6	impl Logger for ConsoleLogger:
     7	    fn write(self, message: string) -> void: pass
     8	
     9	fn log_message(message: string) -> void $ Logger:
    10	    logger := $.use(Logger)
    11	    logger.write(message)
    12	
    13	fn outer() -> void $ Logger:
    14	    fn inner(m: string) -> void $ Logger:
    15	        log_message(m)
    16	    cb := fn(m: string): log_message(m)
    17	    inner("a")
    18	    cb("b")
    19	
    20	fn main() -> i32:
    21	    $.with(Logger=ConsoleLogger {}):
    22	        outer()
    23	    0
```
```
log_message: $ Logger
  Logger: log_message -> $.use(Logger)
outer: $ Logger
  Logger: outer -> $.use(Logger)
  Logger: outer -> $.use(Logger)
main: $()
$impl0.write: $()
```

## HIR expression kinds with no case in src/requirements.ts visitExpression (silently skipped)
23 of 80: string-build, display, string-transform, string-split, console-print, permission-weaken, binding-expression, list-comprehension, tuple, map-comprehension, suspension-wrap, suspension-drive, suspension-cancel, trait-upcast, field-set, list-set, list-append, map-set, string-starts-with, tuple-index, map-entry-key, map-entry-value, for

## path enumeration size on a doubling call chain (f_i calls f_{i+1} twice, all $ Logger)
depth 8: 521 lines; depth 12: 8205 lines; depth 16: 131089 lines (hd explain-requirements | wc -l)
