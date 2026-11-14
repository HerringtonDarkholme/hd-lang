# A12: fixture A is redundant when another fixture B with the same
# expectation and the same cited anchor contains every code line of A
# (comments and blank lines stripped), including A's marked line.
# Then B tests everything A does; A adds nothing.
import os, re, collections
os.chdir('spec/conformance')
rows = [l.rstrip('\n').split('\t') for l in list(open('cases.tsv'))[1:]]
def code(p):
    out = []
    for l in open(p):
        m = re.search(r'#\s*(diagnostic|warning|panic):\s*[a-z0-9-]+\s*$', l)
        c = re.sub(r'\s+#.*$', '', l.rstrip()) if not l.lstrip().startswith('#') else ''
        if c.strip(): out.append(c + (' <MARK>' if m else ''))
    return out
groups = collections.defaultdict(list)
for p, ph, e, s in rows:
    if '-legal' in p: pass
    groups[(s, e, ph)].append(p)
for (s, e, ph), ps in groups.items():
    if len(ps) < 2 or len(ps) > 200: continue
    cs = {p: code(p) for p in ps}
    for a in ps:
        ca = collections.Counter(cs[a])
        if not ca: continue
        for b in ps:
            if a == b or len(cs[b]) <= len(cs[a]): continue
            if not (ca - collections.Counter(cs[b])):
                print(f'{a}\tA12\tevery code line is also in {b} ({e}, {s})')
                break
