# A10: an accept fixture named NAME-legal.hd that keeps the `# test:` title of
# its reject twin NAME.hd, so its title describes a rejection it never makes.
import os, glob
os.chdir('spec/conformance')
rows = [l.split('\t') for l in open('cases.tsv')][1:]
paths = {r[0] for r in rows}
by_base = {os.path.basename(p): p for p in paths}
for p in sorted(paths):
    if not p.endswith('-legal.hd'): continue
    twin = by_base.get(os.path.basename(p)[:-len('-legal.hd')] + '.hd')
    if twin and open(p).readline() == open(twin).readline():
        print(f'{p}\tA10\tsame title as {twin}: {open(p).readline().strip()}')
