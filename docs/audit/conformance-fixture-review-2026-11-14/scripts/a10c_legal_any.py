# A10c: an accept fixture whose `# test:` title contains words that describe
# a rejection ("is an error", "is rejected", "errors", "is one error",
# "reports", "cannot", "is invalid", "is misplaced", "is nonlocal",
# "is unknown"), whatever the fixture's name.
import os, re
os.chdir(os.path.join(os.path.dirname(os.path.abspath(__file__)), '../../../../spec/conformance'))
bad = re.compile(r'\b(is an error|is rejected|errors?\b|is one error|reports?|cannot|is invalid|is misplaced|is nonlocal|is unknown|is not allowed)', re.I)
for l in list(open('cases.tsv'))[1:]:
    p, ph, e, s = l.rstrip('\n').split('\t')
    if e != 'accept':
        continue
    t = open(p).readline().strip()
    if bad.search(t): print(f'{p}\tA10c\t{t[:90]}')
