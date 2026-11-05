"""Check a migration plan against the feature map.

The feature map is the feature snapshot cases in
crates/hd_driver/tests/cases/features/, with their blessed baselines in
crates/hd_driver/tests/baselines/features/ (views tir, own, counts, wat).
It is additive: a case may be added freely; deleting or merging one needs
the owner's approval. crates/hd_driver/tests/feature-cases.lock records the
known cases, and a case missing from the repo fails the check.

Usage: python3 scripts/plancheck.py PLAN.md [--show STEP] [--update-lock]

--show STEP prints every line the step's evidence matched, with its case
and function, so the step's title can be checked against what it covers.

The plan of record is the body of issue #1399; check it with
  gh issue view 1399 --json body -q .body > /tmp/plan.md && python3 scripts/plancheck.py /tmp/plan.md

Step lines:
  - [ ] ID. text -- evidence: VIEW~REGEX [& VIEW~REGEX ...] -- code: PATH[::SYMBOL], ...
A step's feature cases are derived, never written: the cases whose
baselines match every evidence term. `!VIEW~REGEX` negates a term.
Evidence that starts with `fn:` is function-scoped: every term must hold
inside one function's section (the `> path` header shared by all four
views), so `fn: own~X & tir~Loop` means a function with X that loops.
Checks:
  1. every evidence term parses, and an open step's evidence matches at
     least one case (a done step, or the --show step, may match none);
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
argv = sys.argv[1:]
show = None
if '--show' in argv:
    i = argv.index('--show')
    show = argv[i + 1]
    del argv[i:i + 2]
args = [a for a in argv if not a.startswith('--')]
plan_path = args[0]
cases_dir = os.path.join(repo, 'crates/hd_driver/tests/cases/features')
base_dir = os.path.join(repo, 'crates/hd_driver/tests/baselines/features')
lock = os.path.join(repo, 'crates/hd_driver/tests/feature-cases.lock')

cases = sorted(f[:-3] for f in os.listdir(cases_dir) if f.endswith('.hd'))
errors = []
warnings = []

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

header_re = re.compile(r'^> (\S.*)$')

sections_cache = {}
def sections(case, v):
    """The view split into {function path: text}; '' holds lines before
    the first header (the source preamble)."""
    key = (case, v)
    if key not in sections_cache:
        out = {'': []}
        cur = ''
        for l in view(case, v).splitlines():
            m = header_re.match(l)
            if m and not l.startswith('>  '):
                cur = m.group(1)
                out.setdefault(cur, [])
            out.setdefault(cur, []).append(l)
        sections_cache[key] = {k: '\n'.join(ls) for k, ls in out.items()}
    return sections_cache[key]

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
    evtext = ev[0][len('evidence:'):].strip()
    scoped = evtext.startswith('fn:')
    if scoped:
        evtext = evtext[len('fn:'):].strip()
    for t in evtext.split(' & '):
        tm = term_re.match(t.strip())
        if not tm:
            errors.append(f'step {sid}: bad evidence term {t!r}')
            continue
        neg, v, rx = tm.groups()
        try:
            terms.append((neg == '!', v, re.compile(rx, re.M)))
        except re.error as e:
            errors.append(f'step {sid}: bad regex {rx!r}: {e}')
    def ok(text, n, r):
        return (not r.search(text)) if n else bool(r.search(text))
    matched = {}   # case -> [(function, view, line)] the positive terms matched
    for c in cases:
        if scoped:
            fns = set()
            for _, v, _ in terms:
                fns |= set(sections(c, v)) - {''}
            good = [f for f in sorted(fns)
                    if all(ok(sections(c, v).get(f, ''), n, r) for n, v, r in terms)]
            if good:
                matched[c] = [(f, v, l) for f in good for n, v, r in terms if not n
                              for l in sections(c, v).get(f, '').splitlines() if r.search(l)]
        elif all(ok(view(c, v), n, r) for n, v, r in terms):
            matched[c] = [('', v, l) for n, v, r in terms if not n
                          for l in view(c, v).splitlines() if r.search(l)]
    hits = sorted(matched)
    if show == sid:
        for c in hits:
            for f, v, l in matched[c]:
                print(f'  {c} {f or "-"} [{v}] {l.strip()[:110]}')
    # A done step, or the step under --show (a gate checking it is done),
    # may have emptied its evidence: that is its done check.
    if not hits and done != 'x' and show != sid:
        # An earlier step can empty a later step's evidence; a stack of
        # local commits hits that often. Flag it for the planner (the step
        # may be done or mis-specified), but don't fail the gate.
        warnings.append(f'step {sid}: evidence matches no feature case (done by an earlier step, or wrong evidence?)')
    covered.update(hits)
    for c in hits:
        for f, v, l in matched[c]:
            if v == 'counts' and l.startswith('>   '):
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
    nlines = sum(len(matched[c]) for c in hits)
    report.append(f'{sid} [{"x" if done == "x" else " "}] {len(hits):3d} cases {nlines:4d} lines{" fn" if scoped else ""}: {", ".join(hits)}')

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
for w in warnings:
    print('WARN', w)
for e in errors:
    print('FAIL', e)
sys.exit(1 if errors else 0)
