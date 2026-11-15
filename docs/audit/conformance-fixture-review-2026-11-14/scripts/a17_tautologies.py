# A17: asserts that cannot fail: assert(true), assert_equal(X, X) with the
# same expression on both sides, or a `>= 0` comparison (always true for an
# unsigned count such as len() or a usize/u* total).
import re, os
os.chdir(os.path.join(os.path.dirname(os.path.abspath(__file__)), '../../../../spec/conformance'))
for l in list(open('cases.tsv'))[1:]:
    p, phase = l.split('\t')[:2]
    if phase != 'runtime': continue
    for i, ln in enumerate(open(p), 1):
        s = ln.strip()
        why = None
        if re.search(r'(^|=>\s*)assert\(\s*true\s*[,)]', s): why = 'assert(true)'
        m = re.match(r'assert_equal\(\s*([^,]+?)\s*,\s*([^,]+?)\s*[,)]', s)
        if m and m.group(1) == m.group(2) and not re.search(r'[(!]', m.group(1)): why = 'assert_equal(x, x)'
        if re.search(r'\b(len\(\)|count|total|size)\s*>=\s*0\b', s): why = '>= 0 on a count'
        if why: print(f'{p}\tA17\tline {i}: {why}: {s[:70]}')
