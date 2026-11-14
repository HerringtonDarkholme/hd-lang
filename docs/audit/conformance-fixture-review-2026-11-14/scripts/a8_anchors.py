# A8: every `specification` anchor in cases.tsv must exist: a heading slug
# (GitHub style) or an `r-ID` whose `r[ID]` appears in the cited file.
import re, os, collections, sys
root = 'spec'
def slug(h):
    h = re.sub(r'`', '', h.strip().lower())
    h = re.sub(r'[^\w\- ]', '', h)
    return h.replace(' ', '-')
anchors = {}
def load(f):
    if f not in anchors:
        s = set()
        p = os.path.join(root, f)
        if os.path.exists(p):
            seen = collections.Counter()
            for line in open(p):
                m = re.match(r'#+\s+(.*)', line)
                if m:
                    b = slug(m.group(1)); n = seen[b]; seen[b] += 1
                    s.add(b if n == 0 else f'{b}-{n}')
                for rid in re.findall(r'r\[([^\]]+)\]', line): s.add('r-' + rid)
        anchors[f] = s if os.path.exists(p) else None
    return anchors[f]
bad = []
for l in open(os.path.join(root, 'conformance/cases.tsv')):
    r = l.rstrip('\n').split('\t')
    if r[0] == 'path': continue
    f, _, a = r[3].partition('#')
    s = load(f)
    if s is None: bad.append((r[0], 'A8', f'cited file {f} does not exist'))
    elif a and a not in s: bad.append((r[0], 'A8', f'anchor #{a} not found in {f}'))
for b in bad: print('\t'.join(b))
print(len(bad), 'missing', file=sys.stderr)
