# A10: an accept fixture whose `# test:` title is the title of a reject
# fixture in the same folder, its twin: the accept case keeps a title that
# describes a rejection it never makes. Twins pair by folder and title, one
# pass over cases.tsv with a (folder, title) index.
import os
os.chdir(os.path.join(os.path.dirname(os.path.abspath(__file__)), '../../../../spec/conformance'))
rows = [l.rstrip('\n').split('\t') for l in list(open('cases.tsv'))[1:]]
title = lambda p: open(p).readline().strip()
rejects = {}
for p, ph, e, s in rows:
    if e.startswith('reject'):
        rejects.setdefault((os.path.dirname(p), title(p)), p)
for p, ph, e, s in sorted(rows):
    if e != 'accept':
        continue
    twin = rejects.get((os.path.dirname(p), title(p)))
    if twin:
        print(f'{p}\tA10\tsame title as {twin}: {title(p)}')
