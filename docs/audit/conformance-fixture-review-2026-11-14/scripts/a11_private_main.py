# A11: a runtime case with a non-public top-level `main` and no `tests:` block
# or doc test registers no test and has no entry point
# (module.entry.private-main), so nothing in it runs.
import os, re
os.chdir('spec/conformance')
for l in list(open('cases.tsv'))[1:]:
    p, ph, e, s = l.rstrip('\n').split('\t')
    if ph != 'runtime': continue
    t = open(p).read()
    if re.search(r'^fn main!?\(', t, re.M) and not re.search(r'^pub fn main!?\(', t, re.M) \
       and not re.search(r'^tests:', t, re.M) and not re.search(r'^## ', t, re.M):
        print(f'{p}\tA11\t{e}; private main, no tests: nothing runs')
