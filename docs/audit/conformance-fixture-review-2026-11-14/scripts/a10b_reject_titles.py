# A10b: an accept fixture whose `# test:` title is identical to a reject
# fixture's title in another folder (A10 covers the same folder).
import os
os.chdir(os.path.join(os.path.dirname(os.path.abspath(__file__)), '../../../../spec/conformance'))
rows = [l.rstrip('\n').split('\t') for l in list(open('cases.tsv'))[1:]]
title = lambda p: open(p).readline().strip()
rej = {}
for p, ph, e, s in rows:
    if e.startswith('reject'):
        rej.setdefault(title(p), []).append(p)
for p, ph, e, s in rows:
    if e != 'accept' or not title(p).startswith('# test:'):
        continue
    others = [r for r in rej.get(title(p), []) if os.path.dirname(r) != os.path.dirname(p)]
    if others:
        print(f'{p}\tA10b\tsame title as reject {others[0]}')
