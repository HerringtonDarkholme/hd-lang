# commit bd985d7 | command: hd check audit/probes/compiler/row-omit.hd; hd check/explain-requirements row-local.hd | date 2026-09-25
```
     1	trait Logger:
     2	    fn write(self, message: string) -> void
     3	
     4	fn log_message(message: string) -> void $ Logger:
     5	    logger := $.use(Logger)
     6	    logger.write(message)
     7	
     8	fn middle(message: string) -> void:
     9	    log_message(message)
    10	
    11	fn main() -> i32:
    12	    0
audit/probes/compiler/row-omit.hd:9:5: missing-requirement: call to 'log_message' requires Logger
exit=1
```
Named functions (top-level and local) do not infer rows: omitting `$ Logger` is an error, not an inferred row. Only closures infer (see row-local.hd: `fn inner` needed an explicit clause; `cb := fn(m: string): log_message(m)` inferred Logger).

```
$ hd check audit/probes/compiler/row-local-omit.hd   (local fn inner without a row clause)
audit/probes/compiler/row-local-omit.hd:15:9: missing-requirement: call to 'log_message' requires Logger
```
