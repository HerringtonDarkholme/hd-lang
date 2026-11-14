# A16: a header-style comment that looks like an expectation but is not a
# directive the runner reads (# prints:, # output:, # expected:, # stdout:,
# # expect-output:, ...). Its expectation is never checked.
import os, re, collections
os.chdir('/Users/hd/code/test/hd-lang/spec/conformance')
REAL = {'test','expect','expect-stdout','expect-empty-stdout','fixture-runtime-profile','fixture-runtime-scenario',
        'fixture-runtime-pending-function','fixture-package-role','fixture-test-layout','fixture-package-tree'}
pat = re.compile(r'(?:^|\s)#\s*(prints?|output|outputs|expected|expects?|stdout|expect-[a-z-]+|result|returns)\s*:', re.I)
for l in list(open('cases.tsv'))[1:]:
    p = l.split('\t')[0]
    for i, ln in enumerate(open(p), 1):
        m = pat.search(ln)
        if m and m.group(1).lower() not in REAL:
            print(f'{p}\tA16\tline {i}: {ln.strip()[:70]}'); break
