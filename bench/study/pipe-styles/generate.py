"""Generate equivalent direct, curried and partial-application benchmark packages."""
from pathlib import Path
import subprocess,json,os
import sys
root=Path(__file__).resolve().parents[3]
work=Path(sys.argv[1]).resolve() if len(sys.argv)>1 else root/'.cache-muse-822-perf'
work.mkdir(parents=True,exist_ok=True)
common=(root/'bench/study/pipe-styles/common.hd').read_text()
for dataset in ['numeric','string']:
 for style in ['direct','curried','partial']:
  d=work/(dataset+'-'+style);(d/'src').mkdir(parents=True,exist_ok=True)
  (d/'hd.toml').write_text('[package]\nname = "pipe_probe"\n[capabilities]\nArgs = true\nConsole = true\n')
  (d/'src/common.hd').write_text(common)
  pre='''use pkg.common.{select, transform, select_step, transform_step, join_text, join_step, number_text, number_step, clean_text, clean_step}
use std.host.Args
use std.text.Parse

pub fn main() -> void $ Console + Args:
    args := $.use(Args).list()
    n := args[0].parse::[usize]().expect("input length")
'''
  if dataset=='numeric':
   pre+='''    let mut items: List[i64] = []
    for i in 0..n:
        items.push(i64(i))
    keep := fn(value: i64) -> bool: value % 3 == 1
'''
   direct='    kept := select(items, keep)\n    mapped := transform(kept, fn(value: i64) -> string: number_text(value, 7i64))\n    joined := join_text(mapped, ",")\n'
   curried='    kept := select_step(keep)(items)\n    mapped := transform_step(number_step(7i64))(kept)\n    joined := join_step(",")(mapped)\n'
   partial='    kept := select(items, keep)\n    change := fn(value: i64) -> string: number_text(value, 7i64)\n    mapped := transform(kept, change)\n    joined := join_text(mapped, ",")\n'
  else:
   pre+='''    let mut items: List[string] = []
    for i in 0..n:
        items.push("  row${i % 17}  ")
    keep := fn(value: string) -> bool: value.contains("row1")
'''
   direct='    kept := select(items, keep)\n    mapped := transform(kept, fn(value: string) -> string: clean_text(value, "!"))\n    joined := join_text(mapped, ",")\n'
   curried='    kept := select_step(keep)(items)\n    mapped := transform_step(clean_step("!"))(kept)\n    joined := join_step(",")(mapped)\n'
   partial='    kept := select(items, keep)\n    change := fn(value: string) -> string: clean_text(value, "!")\n    mapped := transform(kept, change)\n    joined := join_text(mapped, ",")\n'
  src=pre+{'direct':direct,'curried':curried,'partial':partial}[style]+'''    let checksum: u64 = 0
    for byte in joined.bytes():
        checksum = (checksum * 131u64 + u64(byte)) % 1_000_000_007u64
    println("${kept.len()} ${mapped.len()} ${joined.len()} $checksum")
'''
  (d/'src/main.hd').write_text(src)
  print(d)
