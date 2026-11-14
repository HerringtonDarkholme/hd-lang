# A7: fixtures whose layout the canonical-format commit changed.
# A layout change = the number of non-blank code lines changed, or a final
# line ending was added or removed. For lexical and grammar fixtures this can
# erase the feature the fixture tests.
import subprocess, sys, re, collections
COMMIT = sys.argv[1] if len(sys.argv) > 1 else 'edea876ae'
rows = {r[0]: r for r in (l.rstrip('\n').split('\t') for l in open('spec/conformance/cases.tsv')) if r[0] != 'path'}
names = subprocess.run(['git','show',COMMIT,'--name-only','--format=','--','spec/conformance'],capture_output=True,text=True).stdout.split()
def blob(rev, p):
    r = subprocess.run(['git','show',f'{rev}:{p}'],capture_output=True)
    return r.stdout.decode('utf-8','replace') if r.returncode == 0 else None
def code_lines(t):
    return [l for l in t.split('\n') if l.strip() and not l.lstrip().startswith('#')]
out = []
for p in names:
    rel = p.removeprefix('spec/conformance/')
    if rel not in rows: continue
    old, new = blob(COMMIT+'^', p), blob(COMMIT, p)
    if old is None or new is None: continue
    dl = len(code_lines(old)) - len(code_lines(new))
    eol = old.endswith('\n') != new.endswith('\n')
    if dl or eol:
        r = rows[rel]
        out.append((rel, r[2], r[3], dl, 'final-newline-changed' if eol else ''))
for o in sorted(out, key=lambda o: (o[2], o[0])):
    print('\t'.join(map(str, o)))
