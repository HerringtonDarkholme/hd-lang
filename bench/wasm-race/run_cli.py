#!/usr/bin/env python3
"""Egoist's #1291 workloads on the plain wasmtime CLI: every language the same binary and flags.
One discarded round, then 15 interleaved rounds rotating which language starts; median of each."""
import os, re, statistics, subprocess, sys

F = os.path.dirname(os.path.abspath(__file__))
LANGS = [a.split('=') for a in sys.argv[1:]]  # name=path
WT = ['wasmtime', 'run', '-O', 'opt-level=2', '-W', 'max-wasm-stack=4194304']
W = [('matmul', '256'), ('sieve', '10000000'), ('sort', '1000000'), ('find', '8388608 1024'),
     ('graph-dfs', '200000'), ('graph-bfs', '200000'), ('hashmap', '500000'), ('json', '10000'),
     ('btree', '15'), ('alloc', '16 20'), ('fib', '37'), ('btree-stretch', '18'), ('utf8', '100000'), ('alloc-arena', '16 20'), ('btree-arena', '15')]
times = {(l, w): [] for l, _ in LANGS for w, _ in W}
checks = {}
for r in range(16):
    order = LANGS[r % len(LANGS):] + LANGS[:r % len(LANGS)]
    for w, a in W:
        for l, path in order:
            out = subprocess.run(WT + [path, w, *a.split()], cwd=F, capture_output=True, text=True, timeout=300)
            m = re.search(r'RESULT .*ns=(\d+) checksum=(\S+)', out.stdout + out.stderr)
            if not m:
                pass
                continue
            checks.setdefault(w, {}).setdefault(l, set()).add(m.group(2))
            if r > 0:
                times[(l, w)].append(int(m.group(1)) / 1e6)
    print('round', r, 'load', os.getloadavg()[0], flush=True)
print('| workload | ' + ' | '.join(l for l, _ in LANGS) + ' | checksums |')
for w, a in W:
    cells = []
    for l, _ in LANGS:
        t = times[(l, w)]
        cells.append(f'{statistics.median(t):.0f} ({min(t):.0f}-{max(t):.0f})' if t else 'n/a')
    agree = 'same' if len({v for s in checks.get(w, {}).values() for v in s}) == 1 else str(checks.get(w))
    print(f'| {w} {a} | ' + ' | '.join(cells) + f' | {agree} |', flush=True)
