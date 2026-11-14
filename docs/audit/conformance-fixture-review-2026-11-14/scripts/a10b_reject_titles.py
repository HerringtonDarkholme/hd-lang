# A10b: an accept fixture whose `# test:` title is identical to a reject
# fixture's title (beyond the -legal naming of A10).
import os
os.chdir('spec/conformance')
rows=[l.rstrip('\n').split('\t') for l in list(open('cases.tsv'))[1:]]
title=lambda p: open(p).readline().strip()
rej={}
for p,ph,e,s in rows:
    if e.startswith('reject'): rej.setdefault(title(p),p)
for p,ph,e,s in rows:
    if e=='accept' and not p.endswith('-legal.hd') and title(p).startswith('# test:') and title(p) in rej:
        print(f'{p}\tA10b\tsame title as reject {rej[title(p)]}')
