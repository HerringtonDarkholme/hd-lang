"""Check a migration plan against the feature map.

The feature map is the feature snapshot cases in
crates/hd_driver/tests/cases/features/, with their blessed baselines in
crates/hd_driver/tests/baselines/features/ (views tir, own, counts, wat).
It is additive: a case may be added freely; deleting or merging one needs
the owner's approval. crates/hd_driver/tests/feature-cases.lock records the
known cases, and a case missing from the repo fails the check.

Usage: python3 scripts/plancheck.py PLAN.md [--update-lock]

The plan of record is the body of issue #1399; check it with
  gh issue view 1399 --json body -q .body > /tmp/plan.md && python3 scripts/plancheck.py /tmp/plan.md

Step lines:
  - [ ] ID. text -- evidence: VIEW~REGEX [& VIEW~REGEX ...] -- code: PATH[::SYMBOL], ...
A step's feature cases are derived, never written: the cases whose
baselines match every evidence term. `!VIEW~REGEX` negates a term.
Checks:
  1. every evidence term parses and matches at least one case;
  2. every code path exists, and every ::SYMBOL occurs in its file;
  3. every case with work to do (a non-empty counts baseline, or a
     `value xN` join in own) matches at least one step;
  4. no locked case has disappeared.
Prints each step's derived cases. Exit status 1 on any failure.
"""
import os
import re
import sys

here = os.path.dirname(os.path.abspath(__file__))
repo = os.path.dirname(here)
args = [a for a in sys.argv[1:] if not a.startswith('--')]
plan_path = args[0]
cases_dir = os.path.join(repo, 'crates/hd_driver/tests/cases/features')
base_dir = os.path.join(repo, 'crates/hd_driver/tests/baselines/features')
lock = os.path.join(repo, 'crates/hd_driver/tests/feature-cases.lock')

cases = sorted(f[:-3] for f in os.listdir(cases_dir) if f.endswith('.hd'))
errors = []

if '--update-lock' in sys.argv or not os.path.exists(lock):
    known = set(open(lock).read().split()) if os.path.exists(lock) else set()
    gone = known - set(cases)
    if gone:
        errors.append(f'feature cases removed (needs owner approval): {sorted(gone)}')
    else:
        open(lock, 'w').write('\n'.join(cases) + '\n')
else:
    gone = set(open(lock).read().split()) - set(cases)
    if gone:
        errors.append(f'feature cases removed (needs owner approval): {sorted(gone)}')

views = {}
def view(case, v):
    key = (case, v)
    if key not in views:
        p = os.path.join(base_dir, f'{case}.{v}')
        views[key] = open(p, errors='replace').read() if os.path.exists(p) else ''
    return views[key]

def counts_ops(case):
    # An operation line is `>   [(no span) ]ACTION TYPE (REASON)`; a
    # function header is `> path`, and source lines carry no `>`.
    return [l for l in view(case, 'counts').splitlines() if l.startswith('>   ')]

def needs_work(case):
    return bool(counts_ops(case)) or re.search(r'value ×[0-9]', view(case, 'own'))

term_re = re.compile(r'^(!?)(tir|own|counts|wat)~(.+)$')
step_re = re.compile(r'^- \[([ x])\] ([0-9]+[a-z]*)\. (.*)$')
looks_like_step = re.compile(r'^- \[[ x]\] ')
covered = set()
op_covered = set()   # (case, line) pairs a step's counts evidence matches
report = []

for line in open(plan_path):
    m = step_re.match(line.rstrip('\n'))
    if not m:
        if looks_like_step.match(line):
            errors.append(f'unparsed step line: {line.strip()[:60]}')
        continue
    done, sid, body = m.groups()
    parts = [p.strip() for p in body.split(' -- ')]
    ev = [p for p in parts if p.startswith('evidence:')]
    code = [p for p in parts if p.startswith('code:')]
    if not ev:
        errors.append(f'step {sid}: no evidence')
        continue
    terms = []
    for t in ev[0][len('evidence:'):].split(' & '):
        tm = term_re.match(t.strip())
        if not tm:
            errors.append(f'step {sid}: bad evidence term {t!r}')
            continue
        neg, v, rx = tm.groups()
        try:
            terms.append((neg == '!', v, re.compile(rx, re.M)))
        except re.error as e:
            errors.append(f'step {sid}: bad regex {rx!r}: {e}')
    hits = [c for c in cases if all((not r.search(view(c, v))) if n else r.search(view(c, v)) for n, v, r in terms)]
    if not hits:
        errors.append(f'step {sid}: evidence matches no feature case')
    covered.update(hits)
    for c in hits:
        for n, v, r in terms:
            if v == 'counts' and not n and r.pattern not in ('.', '.*'):
                for l in counts_ops(c):
                    if r.search(l):
                        op_covered.add((c, l))
    for c in (code[0][len('code:'):].split(',') if code else []):
        c = c.strip()
        if not c:
            continue
        path, _, sym = c.partition('::')
        full = os.path.join(repo, path)
        if not os.path.exists(full):
            errors.append(f'step {sid}: missing path {path}')
        elif sym and sym not in open(full, errors='replace').read():
            errors.append(f'step {sid}: missing symbol {sym} in {path}')
    report.append(f'{sid} [{"x" if done == "x" else " "}] {len(hits):3d} cases: {", ".join(hits)}')

work = [c for c in cases if needs_work(c)]
for c in work:
    if c not in covered:
        errors.append(f'case with work not covered by any step: {c}')
# Every count operation (except the by-design ones) is matched by a step
# whose counts evidence names it.
uncovered = {}
for c in cases:
    for l in counts_ops(c):
        if '(by design)' in l or (c, l) in op_covered:
            continue
        key = re.sub(r'^\s*(\(no span\) )?', '', l)
        uncovered.setdefault(key, []).append(c)
for k, cs in sorted(uncovered.items()):
    errors.append(f'count op not covered by any step: {k!r} in {len(cs)} cases ({cs[0]}...)')

print(f'{len(cases)} feature cases, {len(work)} with work, {len(covered & set(work))} covered')
for r in report:
    print(r)
idle = [c for c in cases if not needs_work(c)]
print(f'no work (empty counts, no joins): {", ".join(idle) or "none"}')
for e in errors:
    print('FAIL', e)
sys.exit(1 if errors else 0)
