# A15: a reject or warn case where a top-level or nested `fn` with no `$`
# requirement row calls println/print, which need `$ Console`. That is a
# second error hidden behind the marked one (hd stops at the first).
# Closures and tests: blocks are skipped (their rows are inferred).
import os, re
os.chdir('spec/conformance')
for l in list(open('cases.tsv'))[1:]:
    p, ph, e, s = l.rstrip('\n').split('\t')
    if ph != 'type' or not (e.startswith('reject') or e.startswith('warn')): continue
    lines = open(p).read().split('\n')
    for i, ln in enumerate(lines):
        m = re.match(r'^(\s*)(pub\s+)?fn\s+\w+.*:\s*$', ln)
        if not m or '$' in ln: continue
        ind = len(m.group(1))
        for j in range(i + 1, len(lines)):
            b = lines[j]
            if b.strip() and len(b) - len(b.lstrip()) <= ind: break
            if re.search(r'\bprint(ln)?\(', b) and 'diagnostic:' not in b:
                print(f'{p}\tA15\tline {j+1} calls print in fn at line {i+1} with no $ row'); break
