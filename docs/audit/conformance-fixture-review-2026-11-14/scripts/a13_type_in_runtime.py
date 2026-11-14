# A13: a `type` accept fixture whose every code line also appears in one
# `runtime` accept fixture. The runtime case type-checks the same program
# and also runs it, so the type case adds nothing (unless the extra lines
# change the meaning). Index on each fixture's rarest line to stay near-linear.
import os, re, collections
os.chdir('spec/conformance')
rows = [l.rstrip('\n').split('\t') for l in list(open('cases.tsv'))[1:]]
def code(p):
    return [re.sub(r'\s+#.*$', '', l.rstrip()) for l in open(p)
            if l.strip() and not l.lstrip().startswith('#')]
run = {p: collections.Counter(code(p)) for p, ph, e, s in rows if ph == 'runtime' and e == 'accept'}
freq = collections.Counter(l for c in run.values() for l in c)
index = collections.defaultdict(list)
for p, c in run.items():
    for l in c: index[l].append(p)
for p, ph, e, s in rows:
    if ph != 'type' or e != 'accept': continue
    c = collections.Counter(code(p))
    if len(c) < 3: continue
    rare = min(c, key=lambda l: freq.get(l, 0))
    for q in index.get(rare, []):
        if not (c - run[q]):
            print(f'{p}\tA13\tevery code line is in runtime case {q}'); break
