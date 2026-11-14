# A9: a reject, warn or panic case's expected CODE should appear in the spec
# text it cites: the cited heading's section (with its subsections), or the
# cited r[ID] rule (with the rest of its numbered item). A miss suggests the
# fixture cites the wrong place.
import re, os, collections, sys
def slug(h):
    h = re.sub(r'`', '', h.strip().lower()); h = re.sub(r'[^\w\- ]', '', h)
    return h.replace(' ', '-')
cache = {}
def sections(f):
    if f in cache: return cache[f]
    lines = open(os.path.join('spec', f)).read().split('\n')
    heads = []  # (index, level, slug)
    seen = collections.Counter()
    for i, l in enumerate(lines):
        m = re.match(r'(#+)\s+(.*)', l)
        if m:
            b = slug(m.group(2)); n = seen[b]; seen[b] += 1
            heads.append((i, len(m.group(1)), b if n == 0 else f'{b}-{n}'))
    sec = {}
    for k, (i, lv, s) in enumerate(heads):
        end = next((j for j, l2, _ in heads[k+1:] if l2 <= lv), len(lines))
        sec[s] = '\n'.join(lines[i:end])
    rules = {}
    for i, l in enumerate(lines):
        for rid in re.findall(r'r\[([^\]]+)\]', l):
            body = [l]
            for l2 in lines[i+1:]:
                if re.match(r'\s*\d+\.\s', l2) or l2.startswith('#') or not l2.strip(): break
                body.append(l2)
            rules['r-' + rid] = '\n'.join(body)
    cache[f] = (sec, rules)
    return cache[f]
out = []
for l in open('spec/conformance/cases.tsv'):
    p, ph, e, s = l.rstrip('\n').split('\t')
    if p == 'path' or ':' not in e: continue
    code = e.split(':', 1)[1]
    f, _, a = s.partition('#')
    sec, rules = sections(f)
    text = rules.get(a) if a.startswith('r-') else sec.get(a, '')
    if text is None: text = ''
    if code not in text:
        out.append((p, 'A9', f'{e} not mentioned in cited {s}'))
for o in out: print('\t'.join(o))
print(len(out), file=sys.stderr)
