# A7b: fixtures whose tokens (not just whitespace or line breaks) the
# canonical-format commit changed: removed parentheses, trailing commas,
# reordered or rewritten forms. Comments are ignored.
import subprocess, re, sys, collections
C = 'edea876ae'
rows = {r[0]: r for r in (l.rstrip('\n').split('\t') for l in open('spec/conformance/cases.tsv')) if r[0] != 'path'}
names = subprocess.run(['git','show',C,'--name-only','--format=','--','spec/conformance'],capture_output=True,text=True).stdout.split()
def blob(rev, p):
    r = subprocess.run(['git','show',f'{rev}:{p}'],capture_output=True); return r.stdout.decode('utf-8','replace') if r.returncode==0 else None
def toks(t):
    t = '\n'.join(re.sub(r'(^|\s)#.*$', '', l) for l in t.split('\n'))
    return re.sub(r'\s+', '', t)
for p in names:
    rel = p.removeprefix('spec/conformance/')
    if rel not in rows: continue
    a, b = blob(C+'^', p), blob(C, p)
    if a is None or b is None: continue
    ta, tb = toks(a), toks(b)
    if ta != tb:
        # summarize: which characters were dropped or added
        ca, cb = collections.Counter(ta), collections.Counter(tb)
        lost = ''.join(sorted((ca-cb).elements()))[:20]; gained = ''.join(sorted((cb-ca).elements()))[:20]
        print(f'{rel}\tA7b\tlost {lost!r} gained {gained!r}\t{rows[rel][3]}')
