# Conformance Fixture Review, 2026-11-14

A review of every `spec/conformance/cases.tsv` fixture for fixtures that test
nothing, test the wrong thing, or repeat another fixture. Owner request,
2026-11-14. Nothing here has been applied yet; each finding waits for the
owner's approval.

## Summary (Review Complete)

All 7,331 `cases.tsv` fixtures were reviewed: the pilot (`lang/14`, 334), then
6,997 in 170 batches, with 18 script checks over the whole suite.
Out of scope: the 354 `cli/` cases. Reviewers wrote 855 rows; the main agent
checked the high-value kinds by hand, with `diff` or with `hd check`. Nothing in
the repository has been changed yet.

**Systemic problems (fix the cause, then the fixtures):**

1. **The canonical formatter erased what layout and token fixtures test.**
   `edea876ae` joined multi-line forms, split same-line suites, removed trailing
   commas and redundant parentheses, added a final newline and stripped a BOM.
   The targeted pass found 21 fixtures erased and 23 weakened, including one
   reject case. Fix: restore them from `edea876ae^`, keep fixtures out of the
   formatter (or have it keep these forms), and add `-text` git attributes for
   the CRLF and BOM fixtures (`crlf-line-endings.hd` lost its CRs in `d5a28f88e`).
2. **13 runtime fixtures run nothing (A11).** They have a private `main` and no
   tests, and `crates/hd_driver/tests/conformance.rs` (~1811) passes
   `missing-entry-point` as success, which contradicts the README.
3. **Reject cases that pass for the wrong reason.** A18 lists 15 failing on a
   missing `&`, about 6 to 9 of them not about references. Hand-found: a
   wrong-`Eq`/`Hash` derive, a missing `std` module, an orphan `impl` that also
   duplicates std's, invariance standing in for the row rule, and others. In
   these cases the rule under test is never reached.
4. **Accept cases titled as rejections.** 40 `-legal` twins (A10), 22 by the wider
   A10c, and 87 more by title match (A10b, not yet triaged).
5. **Duplicates.** 9 identical programs (A2), 17 renamed copies (A14), the 7
   identical trees (A1), plus about 30 more confirmed or plausible by review.
   The numbered `checker-*`/`declaration-checks-*`/`rejected-pattern-binds-*`
   series often repeat a named fixture.
6. **Fixtures that aren't portable.** About 20 assume a 32-bit `usize`, about 15
   depend on map or set iteration order (forbidden by the README and
   `types.map.order.unspecified`), and 3 compare floats exactly where the spec
   allows 1 ULP.
7. **Checks that cannot fail.** For example `x == x || x != x`,
   `assert(true)` in match arms, `total >= 0`, commutative provider sums,
   discarded provider values, and expected output in an unchecked `# prints:`
   comment (A16). About 45 confirmed or plausible-strong.
8. **Wrong citations.** About 290 B7 rows, and A9 has 1,269 leads. 148 generated
   pointer-loan fixtures cite a std type declaration, forced by the tier rule
   (owner question).
9. **`hd check` was 70× slower than necessary.** An over-cap 11 GB shared cache
   costs 5 s and 270 MB per run (a fresh cache takes 0.08 s). A fix prompt was
   handed off.

**Owner questions** (details in the batch sections): panic-marker line
convention (the raising line or the entry call); a target directive for
`usize`-width cases; the tier-forced pointer-loan citations; spec
gaps in nesting limits for prefix `*` and `**` chains, the compound-store code
through `Apply`, covariant list joins against `types.lct.no-combine`, how
bound depth is counted (64 against 65 or 66); the 21 accept cases in `typing/warnings/`; and the
message-only fixtures (hints and fix-its) that could move to `cli/` cases.

**Rubric row counts (Haiku, unverified unless stated):** B7 286, B1 236, C1 118,
B2 87, B3 43, B8 32, B5 20, B4 19, B6 13. The precision seen in hand checks
was about 3 in 4 for B1, B2 and B3, and lower for C1 and B6. The B7 and B1
rows are mostly plausible and need a human pass before editing.

## Verification (orchestrator, 2026-11-14, main 9ba8fa972)

What has been rechecked, and what is still unverified. Every action item says
which kind of row it rests on.

- **Script checks: verified.** Rerun on main, A8–A17 give the recorded counts.
  A10 40, A10b 87, A10c 22, A11 13, A14 17, A15 9, A16 3, A17 25 and A8 0 are
  unchanged. A12 (13 against 12), A13 (12 against 11) and A9 (1,281 against
  1,269) grew by fixtures added since.
- **P0 item 1: confirmed.** `crates/hd_driver/tests/conformance.rs:1812`
  returns `Verdict::Pass` on `missing-entry-point`. `runtime/valid/result-ok-unit.hd`
  has a private `main` and no tests, so its body never runs.
- **P0 item 2: confirmed.** `lexical/physical-and-logical-lines/crlf-line-endings.hd` had 18 CRLF
  line endings when 8f8f62c46 added it and 0 after edea876ae. The format commit
  folded the nested suite inside the header of
  `lexical/physical-and-logical-lines/header-resumes-after-nested-suite.hd`, the feature it tests.
  `.gitattributes` has no line for either fixture.
- **Hand findings, sampled:**
  - B2 `typing/invalid/derived-hash-field-not-hash.hd`: confirmed. The error
    names `Eq`, not `Hash`.
  - B4 `runtime/valid/pointer-loan-extra-deep-shared-loans-kept.hd`: **wrong**.
    Its `if n > 1000` never runs on purpose. The write that exists statically
    is what keeps the loans, so its verdict line says "loan needed" against the
    plain twin's "elided".
  - The hidden-second-error rejects (`orphan-impl`, `let-else-falls-through`,
    `module-qualified-without-use`, `template-outside-trait-module`) each report
    exactly their one marked code. The hidden-error claim still needs the
    remove-the-violation test.
- **Spec questions:** each was checked against the spec text before the owner
  decided it (#1954–#1962). b040's question was already answered by the spec:
  `expr.assign.compound.meaning` with `expr.call.apply.not-callable` gives
  `not-callable` (#1956). b008's `**` is a binary operator that
  `grammar.limit.chain` already covers (#1955).
- **Unverified:** the rubric rows (B7 286, B1 236, C1 118, B2 87, B3 43, B8 32,
  B5 20, B4 19, B6 13). They are Haiku output. The issues built on them
  (#1943–#1947, #1963, #1964) require showing each defect before fixing it.

## Action Items

Ordered by priority. **[decide]** marks an item that waits for an owner decision;
everything else can start now. "Lists" refers to the sections below, the
`checks/` outputs and `review-rows.tsv`. Each item fits one commit or one PR
unless it says otherwise.

### P0: the suite passes things it shouldn't

1. **Make the harness fail a runtime case that runs nothing, then fix the 13
   fixtures.**
   - Where: `crates/hd_driver/tests/conformance.rs` (~1811) returns `Verdict::Pass`
     for `missing-entry-point`. Change it to fail a runtime `accept` case
     with neither an entry point nor a registered test, as the README
     already says.
   - Fixtures: the 13 in `checks/a11.tsv`. Make `main` `pub`, or move the body
     into `tests:`.
   - Done when: `scripts/a11_private_main.py` prints nothing; a harness unit
     test covers the new verdict; the 13 pass under `HD_CONFORMANCE_ONLY`.
2. **Undo the formatter damage, and stop it from happening again.**
   - Restore the 21 ERASED fixtures (`formatter-review.tsv`) from `edea876ae^`,
     and `crlf-line-endings.hd` from `8f8f62c46`. Hand-check the 23 WEAKENED
     ones and restore only the lost forms.
   - Add `spec/conformance/lexical/physical-and-logical-lines/crlf-line-endings.hd -text` and the same
     for `lexical/processing-model/utf8-bom.hd` to `.gitattributes`.
   - Exclude layout and lexical fixtures (at least those citing `lang/01` and
     `lang/02`) from `hd fmt` runs over `spec/conformance`, or mark them
     format-exempt.
   - Done when: `scripts/a7b_format_tokens.py` against the fix commit shows no
     lost tokens, and a format run over the suite changes none of these 44 files.
3. **Land the `hd check` cache fix** (prompt handed off earlier). Done when:
   with the default cache, a 2-line file checks in under 0.2 s while other `hd`
   processes run.

### P1: fixtures that test the wrong thing (mechanical; agents can do these)

4. **Isolate the reject cases that pass for the wrong reason.**
   - The likely ones in `checks/a18.tsv` (missing `&`; the log lists which are
     not about references). The hand-found ones in batches b015, b030, b069,
     b073, b079, b085, b091, b095, b102 and b104. The hidden-second-error ones:
     `template-outside-trait-module`, `template-reexported-…`, `-126`,
     `tests-as-identifier`, `let-else-falls-through`, `module-qualified-without-use`,
     `orphan-impl`.
   - Fix so that only the rule under test is broken.
   - Done when: each fixed file reports exactly the marked code with
     `hd check --tests`, and after removing the intended violation by hand the
     file checks clean or gives a different code.
5. **Remove duplicates.**
   - The 9 A2 pairs and the 6 new A14 groups: delete one copy and keep the
     clearer citation. The 7 A1 tree pairs: point both fixtures at one tree.
   - Then the confirmed C1 pairs in the batch sections.
   - Update `cases.tsv` and `examples.tsv` together.
   - Done when: `scripts/a14_renamed.py` and `parta_all.py` (A1, A2) list only the
     legitimate directive-only variants.
6. **Fix the titles.**
   - Retitle the 62 accept twins in `checks/a10.tsv` and `checks/a10c.tsv`.
     Triage the 87 in `checks/a10b.tsv`.
   - Fix the confirmed B1 titles from split series (`int-widths-*`,
     `tuple-rest-a-spread-pattern-*`) and stale terms ("dynamically safe",
     `all!`, row lists in parentheses, partial eq).
   - Done when: A10 and A10c are empty, and A10b is triaged.
7. **Make the checks able to fail.**
   - Turn the 3 `# prints:` comments (`checks/a16.tsv`) into `# expect-stdout:`.
   - Fix the confirmed B3 and B4 cases, for example: `x == x || x != x`,
     `.None => assert(true)`, `total >= 0`, the commutative and discarded
     provider tests (b106), `channel-full-blocks-sender`, dead branches in
     `pointer-loan-extra-deep-shared-loans-*`, and uncalled functions in
     `reference-field-stores-preserve-rhs-alias-writes` and `match-reference-field-loan`.
   - Done when: `scripts/a16*.py` is empty and the A17 real leads are gone. For
     each B3 fix, a deliberately wrong implementation (edit the expected value
     or skip the effect) makes the case fail.
8. **Remove dependence on map and set order (about 15).**
   - Sort keys before asserting, or compare as sets, in the B8 map and set
     fixtures (b055, b059, b097, b112, b144–b148, b155, b158, b163).
   - Done when: a grep finds no expected output that lists map or set entries
     in insertion order.

### P2: decisions for the owner (recommendations in brackets)

9. **[decide] Panic-marker line.** Should the marker sit on the raising line or
   on the entry call? About 15 fixtures mark the caller (b018, b041, b046,
   b106, b118, b131). [The raising line, as the README says. Then add a check
   that the marker isn't a top-level call to a same-file function.]
10. **[decide] Target-dependent cases (about 20 `usize`-width fixtures, plus
    `heap-exhausted` by memory size).** [Add a `# fixture-target: wasm32` header
    directive, a fixture-format change rather than a language change, so other
    targets skip these cases, or rewrite them to use `u32`.]
11. **[decide] Exact float results (3 fixtures) against `expr.power.float.within-ulp`.**
    [Compare within 1 ULP, or use only IEEE-exact inputs.]
12. **[decide] Citations forced by the tier rule (148 pointer-loan fixtures).**
    [Add an optional `rule` column to `cases.tsv` for the rule tested, and keep
    `specification` for the tier. Or build the matrix on language-tier types.]
13. **[decide] The 21 accept cases in `typing/warnings/` (A6) and the lint
    expectations.** [Move them to `typing/valid/` citing the rule that makes the
    code legal; cover the lints with `cli/lint-*` cases.]
14. **[decide] Fixtures that differ only in message text** (hints, fix-its, panic
    report text; b007, b061, b112, b136). [Move them to `cli/` cases, which compare text.]
15. **[decide] Spec questions. File one `owner-decision` issue each (use the
    `write-issue` skill):**
    - nesting and chain limits for prefix `*` and `**` (b008);
    - the error code for a compound store through `Apply` (b040);
    - covariant list joins against `types.lct.no-combine` (b029);
    - how bound depth is counted, 64 against 65 or 66 (b084);
    - `unknown-name` against `invalid-facts-of-target` precedence (pilot);
    - the `*mut`-to-`&mut` loan claim (b120);
    - the empty-row spelling `$()` against `$ ()`.

### P3: citations and the remaining plausible rows

16. **Fix the citations.** About 290 B7 rows, plus A9's 158 heading-level leads.
    Haiku's B7 precision was moderate, so verify with a Sonnet pass per chapter
    before editing. Use `pnpm run spec refs ID` to find the rule. Done when: A9's
    heading-level list shrinks to cases the owner accepts.
17. **Triage the remaining plausible rows** in `review-rows.tsv` (B1, C1, B2) on a
    stronger model, one chapter at a time, and record the verdicts in this log.

### P4: prevent it happening again

18. **Add the cheap script checks to `spec/check.sh`** as guards: A1, A2 and A14
    (duplicates), A6, A10 and A10c, A11, A16, the literal part of A17, and A18
    (it needs `hd`; maybe CI only). Each runs in seconds, and its source is in `scripts/`.
19. **Fix the generators and splitters.** Give the "local" column of
    `generate-pointer-loan-matrix.ts` an honest title ("an alias cannot reach
    the local"). Make the example splitter that produces the numbered
    `checker-*`, `declaration-checks-*`, `rejected-pattern-binds-*` and `-legal`
    series write a title per case and skip cases that already exist as named
    fixtures.
20. **State the fixture rules in the README.** A reject case breaks exactly one
    rule; an accept twin's title says what is accepted; expected values must
    not depend on map order, target width or exact floats; a runtime case
    must run something.

## Files

Everything the review produced sits next to this log, in
[`conformance-fixture-review-2026-11-14/`](conformance-fixture-review-2026-11-14/):

| File | Contents |
|---|---|
| `review-rows.tsv` | All 855 Haiku rows: `batch, path, verdict, rubric, reason` (unverified unless this log says otherwise) |
| `pilot/review-rows.tsv`, `pilot/verified.md`, `pilot/brief.md` | The pilot's 70 rows, their hand verdicts, and the v1 brief |
| `brief.md` | The final reviewer brief (v2.2) |
| `batch-index.tsv` | Batch number, size and chapters (batches are `cases.tsv` rows grouped by spec anchor, 45 at most) |
| `formatter-brief.md`, `formatter-review.tsv` | The targeted formatter-damage pass (136 fixtures, ERASED/WEAKENED/HARMLESS) |
| `scripts/` | The script checks A1–A18 (`parta_all.py` covers A1–A6); run from the repository root unless the script says otherwise |
| `checks/` | Each script's output on 2026-11-14 (`a1-a6.tsv`, `a7.tsv` … `a18.tsv`) |

Paths in this log written as scratchpad `full/X` now live in `checks/X`
(script output), `scripts/X` (scripts) or the files above.

## Method

- **Part A, script checks** (whole suite, no model): identical package trees
  (A1), identical fixture code once comments and directives are stripped
  (A2), runtime accept cases with no check (A4), and a fixture whose
  directory disagrees with its expectation (A6).
- **Part B, Haiku review**: one Haiku 5.5 agent per batch of about 45
  fixtures, grouped by spec anchor, at most 3 at a time. Rubric: B1 tests its
  claim, B7 right citation, B2 reject for the right reason, B3 meaningful
  checks, B4 reached, B6 broken elsewhere, B5 minimal, C1 redundant sibling.
  Brief:
  scratchpad `full/brief.md` (v2, after the pilot).
- **Verification**: the main agent checks the high-value flags (B1, B2, B3,
  B6, B7, C1) against the source, the spec and `hd check` before they count.
  B5 dead-code flags are listed unverified.
- Out of scope this pass: `cli-cases.tsv` (354 CLI cases).

Verdicts: **CONFIRMED** (checked, real), **PLAUSIBLE** (likely, owner call),
**REJECTED** (Haiku was wrong; kept for calibration), **UNVERIFIED**.

## Progress

| Part | Scope | State |
|---|---|---|
| Pilot | `lang/14-annotations.md`, 334 cases, 8 batches | done, verified |
| A | whole suite | done; A2 triaged; A4 judged per batch |
| B | 6,997 remaining cases, 170 batches (`full/index.tsv`) | done (all 170), plus the formatter pass |

Batches done: all 170.

## Script Checks (Part A)

The checks need no model. Run them from `spec/conformance/`:
`python3 -I parta.py . out.tsv`. Each output row is `path<TAB>check<TAB>detail`. It
takes about 2 seconds on the whole suite.

| Check | What it flags | Why it matters |
|---|---|---|
| A1 | Two directories under `trees/` with byte-identical files | The same package tree is kept twice. Fixtures can share one tree. |
| A2 | Two fixtures whose code is identical once `#` comments, directives and blank lines are stripped | One program is indexed twice, often citing two different rules, so one copy tests nothing new. Directive-only differences (a package tree path, a runtime scenario) and doc-comment differences are legitimate and are triaged by hand. |
| A3 | Two fixtures with the same anchor and expectation whose stripped code differs by at most 2 lines | Candidate redundant siblings. **Dropped from v2.** It gave 2,130 pairs, and every pilot pair was a deliberate minimal pair. |
| A4 | A `runtime` `accept` case with no `assert`, `expect-stdout`, `expect-empty-stdout` or `fixture-runtime-scenario` | Possibly a runtime case that checks nothing. Each is judged under B3 by its batch's reviewer, since "runs without panicking" can be the point. |
| A6 | A fixture whose directory (`valid`, `invalid`, `warnings`, `panic`) disagrees with its `cases.tsv` expectation | The README Layout section ties each directory to one expectation kind. |

A pilot-only check, A5 (an `it(...)` body with no assert), was dropped. It
flagged tests whose asserts sit in a helper function, and panic tests,
which need no assert.

```python
import sys, re, os, difflib, itertools, collections, hashlib
root, out = sys.argv[1], sys.argv[2]
rows=[l.rstrip('\n').split('\t') for l in open(os.path.join(root,'cases.tsv'))][1:]
def norm(p):
    return [l for l in (re.sub(r'#.*$','',x).rstrip() for x in open(os.path.join(root,p))) if l]
code={r[0]:norm(r[0]) for r in rows}
res=[]
# A1 identical trees
th=collections.defaultdict(list)
for d in sorted(os.listdir(os.path.join(root,'trees'))):
    h=hashlib.sha1()
    base=os.path.join(root,'trees',d)
    for dp,_,fs in sorted(os.walk(base)):
        for f in sorted(fs):
            p=os.path.join(dp,f); h.update(os.path.relpath(p,base).encode()); h.update(open(p,'rb').read())
    th[h.hexdigest()].append(d)
for v in th.values():
    if len(v)>1: res.append(('trees/'+v[0],'A1','identical to '+', '.join('trees/'+x for x in v[1:])))
# A2 identical code
g=collections.defaultdict(list)
for r in rows: g['\n'.join(code[r[0]])].append(r)
seen=set()
for k,v in g.items():
    if len(v)>1:
        res.append((v[0][0],'A2','same code (comments/directives stripped) as '+', '.join(x[0]+' ['+x[2]+']' for x in v[1:])+' ['+v[0][2]+']'))
# A3 near-dup, same anchor & expectation
by=collections.defaultdict(list)
for r in rows: by[(r[3],r[2])].append(r)
for k,v in by.items():
    if len(v)>60: continue
    for a,b in itertools.combinations(v,2):
        ca,cb=code[a[0]],code[b[0]]
        if ca==cb: continue
        if abs(len(ca)-len(cb))>2: continue
        d=[l for l in difflib.unified_diff(ca,cb,lineterm='',n=0) if l[:1] in '+-' and not l.startswith(('+++','---'))]
        if len(d)<=4: res.append((a[0],'A3',b[0]))
# A6 dir vs expectation
for p,ph,e,s in rows:
    bad=(('/valid/' in p and e!='accept') or ('/invalid/' in p and not e.startswith('reject')) or ('/warnings/' in p and not e.startswith('warn')) or ('/panic/' in p and not e.startswith('panic')))
    if bad: res.append((p,'A6',f'directory says otherwise; expectation {e}, cites {s}'))
# A4 runtime accept without any check
for p,ph,e,s in rows:
    if ph=='runtime' and e=='accept':
        t=open(os.path.join(root,p)).read()
        if not re.search(r'assert|expect-stdout|expect-empty-stdout|fixture-runtime-scenario',t): res.append((p,'A4','runtime accept with no assert, stdout check, or scenario'))
with open(out,'w') as f:
    for x in res: f.write('\t'.join(x)+'\n')
print(collections.Counter(x[1] for x in res))
```

### Later checks (A7 to A10)

Run each from the repository root with `python3 -I SCRIPT > out.tsv`.

| Check | What it flags | Result |
|---|---|---|
| A7 | Fixtures whose code line count or final newline the canonical-format commit `edea876ae` changed | 1,679 (122 citing chapters 01 or 02); the source of the "formatter erased the feature" findings |
| A8 | `specification` anchors that resolve to no heading or `r[...]` rule | 0, so reviewer "missing heading" flags are always wrong |
| A9 | Reject, warn or panic cases whose expected code is absent from the cited section or rule | 1,269 (158 heading-level); leads for B7 |
| A10 | `NAME-legal.hd` accept fixtures with their reject twin's `# test:` title | 40 of 40 |

```python
# a7_format_drift.py
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
```

```python
# a8_anchors.py
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
```

```python
# a9_cited_code.py
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
```

```python
# a10_legal_titles.py
# A10: an accept fixture named NAME-legal.hd that keeps the `# test:` title of
# its reject twin NAME.hd, so its title describes a rejection it never makes.
import os, glob
os.chdir('spec/conformance')
rows = [l.split('\t') for l in open('cases.tsv')][1:]
paths = {r[0] for r in rows}
by_base = {os.path.basename(p): p for p in paths}
for p in sorted(paths):
    if not p.endswith('-legal.hd'): continue
    twin = by_base.get(os.path.basename(p)[:-len('-legal.hd')] + '.hd')
    if twin and open(p).readline() == open(twin).readline():
        print(f'{p}\tA10\tsame title as {twin}: {open(p).readline().strip()}')
```

## Script Findings (Part A)

### A1. Identical package trees (7 pairs)

Each pair should become one tree used by both fixtures.

- `trees/use-forms-a-group-use-from-an-unknown-module-reports-one-error` = `trees/use-forms-a-single-use-of-an-unknown-module-reports-one-error`
- `trees/impl-modules-a-trait-impl-outside-the-module-of-the-trait-or-target-is-nonlocal` = `trees/impl-modules-an-inherent-impl-outside-the-module-of-its-type-is-nonlocal`
- `trees/dyn-inherent-nonlocal` = `trees/impl-modules-a-dyn-inherent-impl-outside-the-module-of-its-trait-is-nonlocal`
- `trees/misplaced-derivation-a-trait-block-outside-its-targets-module-is-misplaced` = `trees/misplaced-derivation-a-trait-less-block-outside-its-targets-module-is-misplaced`
- `trees/row-aliases-a-nested-alias-flattens-into-the-keys-of-its-caller` = `trees/row-aliases-a-caller-missing-a-key-of-a-nested-alias-is-missing-it`
- `trees/row-aliases-a-context-type-naming-a-row-alias-matches-its-expanded-row` = `trees/row-aliases-a-context-type-with-fewer-keys-than-the-expanded-alias-is-a-mismatch`
- `trees/tuple-template-bounds-a-user-tuple-template-shows-nested-integer-members` = `trees/tuple-template-bounds-a-user-tuple-template-rejects-a-member-its-walker-cannot-show`

### A2. Identical code once comments and directives are stripped (13 groups)

Triaged by hand. **CONFIRMED duplicates (9 pairs)**: the code is the same and
the expectation is the same, so one fixture of each pair adds nothing. Most
cite two different rules for one program. The fix is to keep one fixture
and either cite the primary rule or write a second program that isolates the
other rule.

- `parse/invalid/mutable-field-modifier.hd` (cites 08 `#fields`) = `grammar/declarations/grammar-mutable-field-modifier.hd` (cites `grammar.data.no-mut-modifier`; it is also a `parse` case kept under `typing/`)
- `typing/invalid/nonfinal-spread.hd` (07 `#varargs`) = `grammar/expressions/positional-after-spread.hd` (02 `#calls-and-arguments`)
- `names/module-scope/forward-module-binding.hd` = `names/module-scope/exec-binding-read-before-declared.hd`
- `typing/invalid/tuple-rest-differs-from-plain.hd` (`types.tuple.rest.same`) = `typing/invalid/tuple-rest-assign.hd` (`types.tuple.rest.assign`)
- `requirements/provider-scopes/generic-requirement-key-collision.hd` = `requirements/requirement-polymorphism/generic-provider-key-collision-nested.hd`
- `requirements/requirement-rows/row-subsumption/row-union-omitted-result.hd` (`req.row.union.sites.type`) = `requirements/requirement-rows/row-subsumption/row-union-private-result.hd` (`req.row.union.sites`)
- `requirements/requirement-rows/requirement-keys/requirement-key-unknown-binding.hd` = `requirements/requirement-rows/requirement-keys/requirement-key-unknown-binding-rule.hd` (same rule, too)
- `requirements/requirement-rows/row-parameters/row-parameter-unmarked.hd` (`req.row.param.unmarked`) = `requirements/requirement-rows/row-parameters/row-parameter-kind-not-inferred.hd` (`req.row.param.no-inference`)
- `typing/valid/a-copied-box-keeps-a-dependent-reference-independent-of-its-list.hd` is the `runtime/valid/a-pointer-loan-keeps-its-independent-owner-owned-copy.hd` program without its `expect-stdout`. The runtime case covers it.

**REJECTED (legitimate, 4 groups)**: they differ only by a directive or a
doc comment, and that difference is the point of the case.
`competing-suspension-drivers` / `reentrant-suspension-poll` (runtime
scenario), `root-file-super` / `root-file-lib-super` (`main.hd` vs `lib.hd`),
`module-named-pkg` / `root-mod-file` / `module-path-not-identifier` (tree
path), `module-documentation` / `module-documentation-second-block` (doc
comments).

### A4. Runtime accept cases with no assert, stdout check or scenario (41)

Leads only: some exist to show the program runs without panicking. The Haiku
reviewer of each batch judges them under B3. List: scratchpad `full/parta.tsv`.

### A6. Directory disagrees with expectation (21)

All 21 are `accept` cases under `typing/warnings/`, which the README reserves
for `warn` cases. They cite lint rules that a type-check case cannot observe.
Fix: move them to `typing/valid/` and cite the rule that makes the code
legal, or make them `warn` cases if the lint is in scope.

## Confirmed Findings

### Pilot, `lang/14-annotations.md`

**A second error hides behind the marked one (B2)**
- `annotations/typed-derivation/templates/template-outside-trait-module.hd` and
  `annotations/typed-derivation/templates/template-reexported-from-other-package.hd`: the template writes
  `fn debug(self) -> string`; std `Debug` is
  `debug(&self, out: *mut DebugWriter) -> void`. `hd` stops at
  `misplaced-derivation`, so CI never sees the mismatch.
- `annotations/typed-derivation/misplaced-derivation-a-template-outside-its-traits-module-is-misplaced.hd`:
  the template body is `"x"` with no walk, describe or build call, so
  `annot.template.body.error` (`marker-template`) also applies.

**Wrong citation (B7)**
- `annotations/two-annotate-forms/facts-of-the-result-of-a-function-declaration-is-its-facts.hd`: an accept case cites
  `annot.facts-of.target.error`, an error-only rule.
- `annotations/two-annotate-forms/facts-of-a-missing-argument-follows-the-call-rules.hd`
  and `…-an-extra-argument-follows-the-call-rules-31.hd`: they expect
  `argument-count`, but cite `target.error`, whose code is
  `invalid-facts-of-target`.
- `annotations/typed-derivation/handles/tuple-handle-instances.hd`: cites `annot.handle.shared-target`,
  but sharing is never observed.

**Title doesn't match the code (B1)**
- `annotations/two-annotate-forms/values-are-found-by-type-in-source-order.hd`: each fact type
  appears once, so source order is never tested.
- `annotations/typed-derivation/derivation-blocks/trait-less-derivation-block.hd`: "replaces decorator metadata",
  but the replaced member has no decorator.
- `annotations/typed-derivation/the-structure-trait/structure-calls-are-pure.hd`: the title names walk, describe and
  build, but only describe is called.
- `annotations/typed-derivation/walkers-describers-and-sources/walker-member-through-a-generic-structure-parameter-is-callable.hd`:
  an accept case titled "errors"; the walker is concrete.
- `annotations/typed-derivation/misplaced-derivation-a-structure-block-without-lines-for-a-struct-is-valid.hd`:
  kept its reject twin's title.

**Checks that cannot fail (B3)**
- `annotations/typed-derivation/handles/structure-a-generic-owner-has-one-handle-per-instantiation.hd`:
  the printed equalities are true whether handles are shared or per
  instantiation.

**Panic marker on the wrong line (B2)**
- `annotations/typed-derivation/facts/fact-evaluation-reads-itself.hd` and
  `annotations/typed-derivation/facts/fact-evaluation-cycle.hd`: `annot.fact.eval.reentrant` puts the
  panic at the second read (inside `own_path`), but the marker is on `main`'s read.

**Accept cases in `typing/warnings/`**: covered by A6 above.

**Dead code (B5, minor)**
- `annotations/two-annotate-forms/different-facts-are-evaluated-independently.hd`: `facts_of`
  import and `no_prefix` unused.
- `annotations/two-annotate-forms/member-metadata-a-derivation-may-omit-a-member-with-a-default.hd`:
  `struct Audit` unused.
- `annotations/typed-derivation/derivation-blocks/trait-less-block-declares-no-method.hd`: `Note` and `note()`
  unused.

## Plausible (Owner Call)

- `annotations/error-derivation/error-form-other-rule.hd` ~ `annotations/error-derivation/error-argument-number.hd`:
  both are invalid `@error` arguments (an identifier and a number). Merge?
- `annotations/typed-derivation/walkers-describers-and-sources/generic-data-member-call.hd` ~ `annotations/typed-derivation/walkers-describers-and-sources/generic-member-call.hd`:
  the only difference is the hook name. One case per hook, or one in total?
- `annotations/typed-derivation/typed-derivation.hd` ~ `annotations/typed-derivation/walk-describe-and-build/typed-derivation-walk.hd`:
  the same declarations; the runtime one adds asserts.
- `annotations/error-derivation/error-generated-bounds.hd`: the title names the transparent
  bound, but no generic use needs it.
- `annotations/typed-derivation/derivation-blocks/member-line-list-expression.hd`: "any list-typed
  expression", but only named globals are tested.
- `annotations/typed-derivation/handles/facts-and-handles-build-once.hd`: tests fact laziness, but
  cites `handle.constants`.
- `annotations/two-annotate-forms/typed-fact-function.hd`: the title says the parameters are
  written as bounds, but `wrap[T]` is unbounded.

## Spec Questions

- `annotations/two-annotate-forms/facts-of-an-unknown-name-is-unknown.hd`:
  `annot.facts-of.target.error` says any other argument gives
  `invalid-facts-of-target`, but the fixture expects `unknown-name`. The spec
  doesn't say which one wins.

## Rejected Flags (Calibration)

Pilot: 8 of the 42 non-C2 flags were wrong. Haiku missed `types.option.wrap`
(a `T` is accepted where `T?` is expected). It flagged an `&mut`/`*mut` trait
signature that `hd check` accepts. It counted an unknown name as two errors.
It called a renamed type parameter "redundant". C2 (missing contrast) gave 28
reports, mostly noise, and is dropped from v2.

## Part B Findings (Full Run)

Each batch adds one ledger line and its verified findings below. The raw
Haiku rows are in scratchpad `full/bNNN-out.tsv`.

Each batch block starts with a ledger line:
`bNNN (chapters, fixtures): Haiku rows R; confirmed C, plausible P, rejected X, unverified U`.

#### b001 to b004 (`README.md#diagnostics`, `lang/01-lexical-structure.md`)

Ledger: b001 (44): 32 rows; b002 (44): 12 rows; b003 (42): 5 rows; b004 (44): 4 rows.
Confirmed 16, rejected 35 (32 of them poison rows, below).

**Systemic: the canonical formatter erased what lexical fixtures test.**
Commit `edea876ae` ("Apply the canonical hd format", 2026-10-27) reformatted
3,528 conformance fixtures. Its message says it kept "syntax-invalid fixtures
byte-exact", but it did reformat valid lexical and layout fixtures. The
following fixtures have lost the feature their title names, and their
pre-format version (`git show edea876ae^:PATH`) still has it:
- `lexical/physical-and-logical-lines/continuation-line-opens-with-a-bracket.hd`: the multi-line
  bracket lists were joined onto one line.
- `lexical/physical-and-logical-lines/leading-dot-deeper-continues.hd`: no leading-dot line is left.
- `lexical/physical-and-logical-lines/contextual-variant-after-same-line-if.hd`: the same-line `if`
  became a block.
- `lexical/whitespace-and-indentation/no-final-line-ending.hd`: the formatter added the final newline
  the fixture must lack (`lex.indent.eof-newline`).
- `lexical/physical-and-logical-lines/nested-suite-body-deeper-than-statement.hd`,
  `lexical/physical-and-logical-lines/header-resumes-after-nested-suite.hd`,
  `lexical/physical-and-logical-lines/operator-and-member-lines-continue-in-brackets.hd`,
  `lexical/physical-and-logical-lines/leading-pipe-lines-join-the-chain.hd`,
  `lexical/physical-and-logical-lines/colons-in-brackets-and-trailing-blocks.hd`: their continuation
  and suite layouts were flattened.
Fix: restore each from `edea876ae^`, and exclude layout-sensitive fixtures
from the formatter (the ones that cite `lex.*` rules, at least). **Script check
A7** (`full/a7.tsv`, script `a7_format_drift.py`) lists every fixture whose
code line count or final newline that commit changed: 1,679 fixtures, 122 of
them citing chapters 01 or 02. Those 122 need a targeted review against their
pre-format version (planned as extra batches at the end).

**Title doesn't match the code (B1), not the formatter**
- `lexical/processing-model/bom-inside-comment.hd`: the title says a U+FEFF "in a comment or
  a string literal" is content. Both BOMs are in comments, and that was true
  before the format commit too.
- `lexical/operators-and-delimiters/every-operator-and-delimiter-token.hd`: `$` and `::` never appear.
- `lexical/literals/reserved-word-interpolation.hd`: the title says a `$` before a
  reserved word is a bare `$`, but the case expects `syntax-error`
  (`lex.interp.reserved-dollar`).

**Wrong citation (B7)**
- `lexical/literals/semicolon-after-binding.hd`: cites `#literals`; the rule is
  `lex.punct.semicolon-reserved` under Operators And Delimiters.
- `lexical/processing-model/mid-file-bom.hd`: cites `#processing-model`; the rule is
  `lex.encoding.stray-bom` under Source Encoding.
- `lexical/literals/unary-plus-i32-literal-range.hd`: cites 01 `#integer-literals`;
  the `integer-literal-range` rule is `types.literal.int-range` in chapter 04.

**Duplicates (C1)**
- `lexical/literals/repeated-numeric-separator.hd` = `lexical/literals/double-numeric-separator.hd`
  (both are `1__0`, the same rule).
- `lexical/literals/semicolon-after-binding.hd` ~ `lexical/operators-and-delimiters/semicolon.hd` (only
  the literal before the `;` differs).
- `lexical/literals/stray-dollar-in-string.hd` ~ `lexical/literals/stray-dollar.hd` (both put
  a `$` before the closing quote).

**Rejected**: three `parse/valid` B6 flags (`closure-body-ends-at-comma-line`,
`leading-dot-continuation`, `same-line-loop-else`). A parse case only parses,
so type errors there don't matter.

**Rejected, poison "reports once" fixtures (32 rows).** With the stale
binary, 29 of 32 `typing/invalid/poison-*` fixtures reported a second error.
On a current build (2026-11-14 15:36), all 31 report exactly one. Three more
flags argued from the source that a poisoned arm or pattern must also cause
`nonexhaustive-match` or `duplicate-binding`. But suppressing that
follow-on error is exactly what these fixtures test, and the current build
does suppress it.

**Process note.** `target/release/hd` was from 2026-11-13, 114 compiler
commits behind HEAD. Every `hd check` result from b001 to b007 used it. A
current release build replaced it at 15:36. From b008 on, reviewers use the
current build. Flags from b001 to b007 that rest on `hd check` are re-checked
on it before they count.

#### b005 (`lang/01`, `lang/02`)

Ledger: b005 (44): 8 rows. Confirmed 3, plausible 2, rejected 3.

- **Formatter (systemic, see above)**: `lexical/physical-and-logical-lines/next-line-else-after-same-line-if.hd`.
  The `else:` on the next physical line was joined onto the `if` line, so
  `lex.suite-end.else-next-line` is untested.
- **Duplicates (C1)**: `lexical/whitespace-and-indentation/tab-indentation.hd` = `lexical/whitespace-and-indentation/tab-in-function-body.hd`
  (a tab before a body line); `grammar/expressions/positional-after-named.hd` =
  `grammar/expressions/positional-after-named-argument.hd` (`resize(width=640, 480)`).
- Plausible: `grammar/control-flow-expressions/while-else.hd` (the title says the loop produces a
  value; none is used). `lexical/identifiers/raw-identifiers-name-fields-members-labels-parameters-and-bindings.hd` ~
  `lexical/identifiers/raw-identifiers-are-ordinary-names-at-run-time.hd`.
- Rejected: two B7 flags claimed `#control-flow-expressions` is missing; the
  heading "Control-Flow Expressions" exists. One C1 flag: `brace-escape`
  (`\{`) and `invalid-escape` (`\x`) are different escapes.

**Script check A8** (`a8_anchors.py`): every `specification` anchor in
`cases.tsv` resolves to a heading or an `r[...]` rule in its file. 0 missing,
so B7 flags of the "anchor does not exist" kind are always wrong.

#### b006 (`lang/02-grammar.md`)

Ledger: b006 (45): 8 rows. Confirmed 7, plausible 1.

- **Formatter**: `grammar/expressions/embedded-copy-syntax.hd`. Its same-line suite
  `if post.id == "p": post.Timestamps = stamps` was split over two lines.
- **Wrong citation (B7)**: each cites a section that does not hold the rule.
  - `grammar/declarations/trailing-callback-statement.hd` cites `#declarations`; the rule is `grammar.call.trailing-block` (Trailing Blocks).
  - `grammar/declarations/copy-update-spread.hd` cites `#data-types`; the rule is `grammar.primary.update-spread`.
  - `grammar/declarations/copy-update-spread-not-first.hd` cites `#data-types`; the rule is `grammar.primary.prefix-elsewhere`.
  - `grammar/declarations/explicit-generic-data-arguments.hd` cites `#data-types`; the rule is `grammar.expr.type-arguments.type-name`.
  - `grammar/declarations/explicit-generic-arguments-with-placeholder.hd` cites `#declarations`; the rule is `grammar.primary.function-type-argument`.
- **Duplicate (C1)**: `grammar/expressions/comparison-chaining-with-names.hd` =
  `grammar/expressions/chained-comparison.hd` (`0 < x < 10`; only the names differ).
- Plausible: `grammar/expressions/reserved-receiver-names.hd` (no reserved word appears).

**Script check A9** (`a9_cited_code.py`, output `full/a9.tsv`): a reject,
warn or panic case whose expected code does not appear in the text it cites.
1,269 hits: 158 cite a heading, 1,111 cite an `r[...]` rule. Not triaged
yet. Many rule-level hits are expected, because a sub-rule often inherits
its parent's `Error:` line. The 158 heading hits are the stronger B7 leads.

#### b007 (`lang/02-grammar.md`)

Ledger: b007 (44): 6 rows. Confirmed 2, plausible 4. Reviewed on the stale
binary, but no row rests on `hd check`.

- **Code under test never runs (B4)**:
  - `grammar/statements/header-and-bracket-expression-positions.hd`: the guard
    closure on line 14 is never evaluated, since `classify(.Some(+12))` takes
    the first arm.
  - `grammar/control-flow-expressions/nested-control-flow-as-expressions.hd`: the middle `else if`
    branch (line 38) is never taken.
- Plausible, **fixtures that differ only in hint text**:
  `parse/invalid/a-colon-in-parentheses-{after-a-long-operand,before-a-comma,before-no-type}-gives-the-schematic-hint.hd`
  match `…-before-a-bracketed-type-suggests-its-call.hd` in code, line and
  rule. The runner never judges message text, so as conformance cases they
  add nothing. They are used only in `cases.tsv`. To keep the hint coverage,
  move them to `cli/` cases, which compare text.
- Plausible: `grammar/expressions/expression-type-arguments.hd` is a strict subset of
  `grammar/expressions/type-arguments-in-expressions.hd`.

#### b008 (`lang/02-grammar.md`)

Ledger: b008 (44): 9 rows. Confirmed 4, plausible 2, spec question 2, covered by A6 1.

- **Formatter**: `grammar/statements/same-line-for-list.hd`,
  `grammar/statements/same-line-suite-body-forms.hd` (their same-line suites were
  split), and `grammar/generic-parameters-and-bounds/trailing-commas-everywhere.hd` (its trailing commas
  in declarations and patterns were removed).
- **Wrong citation (B7)**: `grammar/generic-parameters-and-bounds/trailing-commas-everywhere.hd` cites
  `grammar.generic.default.type-row`; the fixture has no type default.
- **Spec question, limits**: `grammar/source-files-and-suites/nesting-too-deep-in-a-dereference.hd`
  (170 prefix `*`) and `grammar/source-files-and-suites/nesting-too-deep-in-a-power.hd` (169 `**`)
  expect `nesting-too-deep`. But `grammar.limit.nesting` covers only brackets,
  suites, closures and interpolations. `grammar.limit.chain` covers binary and
  postfix operators, with a limit of 10,001 in `hd`. Read literally, the spec
  sets no limit on prefix `*` and must accept 169 `**`. Either the spec's
  limits should cover prefix operators and right-nested chains, or these
  fixtures test `hd`'s recursion limit, not the language.
- Plausible: `grammar/generic-parameters-and-bounds/trait-value-bindings.hd` (the title says a named type
  takes bindings, which the rule forbids). `grammar/generic-parameters-and-bounds/a-parameter-bound-by-two-instantiations-of-one-trait-meets-each.hd`
  (only one instantiation is required).
- `grammar/statements/same-line-let-list-unused.hd`: an accept case in
  `warnings/`, already in A6.

#### b009 (`lang/02-grammar.md`)

Ledger: b009 (43): 6 rows. Confirmed 2, plausible 4.

- **Formatter**: `grammar/statements/let-else-same-line.hd` and
  `grammar/statements/let-after-same-line-if-keeps-else.hd`. Their same-line
  `let … else:` and `if` suites were split, so `grammar.stmt.let-else.same-line`
  and `.not-inline` are untested.
- Plausible, B7: `typing/invalid/a-chain-may-not-redeclare-a-name-{1,2}.hd`
  and `typing/invalid/a-chained-name-is-not-reassignable-{1,2}.hd` cite
  `grammar.stmt.chain`, which states no error. The codes come from
  `names.bind.no-redeclare` and `names.local.short` in chapter 03. Citing the
  chain rule may be the deliberate choice of "primary section". A9 flags this
  pattern too.

#### b010 (`lang/02-grammar.md`)

Ledger: b010 (43): 4 rows. Confirmed 1, plausible 1, rejected 2.

- **A second error behind the marked one (B2)**: `grammar/test-blocks/tests-as-identifier.hd`.
  Line 5 `tests := 3` is the marked `syntax-error`, but line 6 `tests` uses the
  reserved word again. A parser that recovers reports line 6 too, and fails
  the "no other located error" rule. Fix: make line 6 `0`.
- Plausible: `grammar/types/optional-function-type-after-requirement-row.hd`.
  Its helper trait method `fn line(self)` has no result type. `hd check`
  reports `missing-result-type`, but the case is `parse` phase, so this
  matters only if the missing result type is a parse error.
- Rejected: `grammar/statements/binding-list.hd` (B7 wanted several citations; the
  index allows one). `grammar/types/counted-reference-types.hd` (B6 type error in
  a parse-only case).

#### b011 (`lang/02-grammar.md`)

Ledger: b011 (50): 15 rows. Confirmed 2, plausible 6, rejected 7.

- **Title out of date after a spec change (B1)**: `grammar/types/requirement-row-lists.hd`
  ("rows are comma lists … parenthesized inside types") and
  `grammar/types/bare-row-list-in-type.hd` ("several keys inside a type must be
  parenthesized"). Today `+` joins keys, and `grammar.type.row.no-parentheses`
  forbids parentheses around a row. The code follows the new rule; the titles
  describe the old one.
- Plausible: `grammar/use-declarations/use-declaration-forms.hd` (the title promises a group
  with a trailing comma, but none has one; the format commit touched it).
  `grammar/use-declarations/single-and-grouped-use.hd` ~ `use-declaration-forms.hd` (merge?).
  `grammar/statements/same-line-let-comma.hd` (the title blames a multi-name `let`,
  but a single same-line `let` is also rejected there).
  `grammar/types/explicit-empty-row.hd` (the title says "normalization", which
  is never tested). `grammar/types/mutable-list-parameter-index-assignment.hd` and
  `…/mutable-parameter-field-assignment.hd` (cite `#types`; the rule is
  `grammar.stmt.assign-place`).
- Rejected: 7 B2 and B6 flags for check-only errors inside parse-phase cases.
  From b016 on, the brief tells reviewers that parse cases are judged on
  parse errors only.

#### b013 (`lang/03-names-and-scopes.md`)

Ledger: b013 (46): 3 rows. Plausible 2, unverified 1. No high-value finding.

- Plausible: `names/name-categories/redeclare-core-type.hd` cites `#name-categories`; the
  error rule is `module.prelude.no-shadow`. `names/member-resolution/part-trait-method-ignored-by-lookup.hd`
  repeats the last assert of `names/member-resolution/embedded-trait-method-via-part.hd`.
- Unverified B5: `names/member-resolution/stored-fn-call-a-parenthesized-field-call-calls-the-stored-function.hd`
  (unused `double`).

#### b012 (`lang/03-names-and-scopes.md`)

Ledger: b012 (38): 9 rows. Confirmed 9.

- **Wrong citation (B7), 8 fixtures**: the `declaration-checks-…-type-parameter-{323..330}.hd`
  set (4 reject, 4 accept) cites `#function-and-closure-scopes`. The rule
  they test, `names.type-param.no-redeclare`, is under `### Type Parameters`.
- **Title doesn't match the code (B1)**: `names/control-flow-binding-scopes/branch-scopes-shadow.hd`.
  There is no outer `value`, so nothing is shadowed. The branches only declare
  independent locals.

#### b015 (`lang/03-names-and-scopes.md`)

Ledger: b015 (44): 2 rows. Confirmed 1, unverified 1.

- **Rejected for the wrong reason (B2)**: `names/member-resolution/local-impl-trait-value-before-declaration.hd`.
  Line 11 `let labeled: &dyn Labeled = invoice` fails because the `&` is missing
  ("a value never becomes a trait value reference by itself"), not because the
  local `impl` comes later. A compiler that ignored `names.member.known-impl`
  would still pass. Fix: write `&invoice`, so that only the impl's position
  is wrong.
- Unverified: `names/member-resolution/local-impl-known-after-declaration.hd` (`hd check`
  did not finish under machine load).

**Kind to watch: rejected for the wrong reason.** A reject case whose marked
line breaks a second, unrelated rule passes even when the rule under test is
not implemented. This is the most valuable B2 kind.

#### b014 (`lang/03-names-and-scopes.md`)

Ledger: b014 (45): 4 rows. Plausible 4. Little was checked with `hd`
because of machine load.

- Plausible merges: `names/member-resolution/private-own-field-beside-promoted.hd` ~ half
  of `names/member-resolution/private-own-field-needs-another-name.hd`.
  `names/use-declarations/duration-unknown-unit.hd` ~ `names/use-declarations/unknown-literal-suffix.hd`
  (a duration unit is arguably its own form, so they are likely both needed).
- Plausible B7: `names/local-bindings/let-else-scope.hd` cites `names.let-else.not-in-else`
  but never reads the name in `else`.
- Plausible B2: `names/module-scope/use-names-exec-binding.hd` (tree case). Line 8
  also reads a name the failed import leaves unbound.

#### b016 (`lang/03-names-and-scopes.md`)

Ledger: b016 (45): 2 rows. Plausible 2.

- Plausible B7: `names/module-scope/a-function-name-in-a-local-annotation-is-an-unknown-type.hd`
  cites `names.module.other-module.type`, but `helper` is in the same module.
  `names.category.syntax` fits better.
- Plausible C1: `names/module-scope/sibling-module-private-function-bare.hd` ~
  `names/module-scope/sibling-module-function-not-imported.hd` (they differ only in
  `pub`, which the cited rule does not mention).
- Tooling note: some `hd check` runs printed their result and then did not
  exit within 60 seconds under heavy machine load. One file that hung did not
  hang when run alone. Worth a check later: does `hd check` hang after its
  output under contention?

#### b018 (`lang/04-type-system.md`)

Ledger: b018 (24): 4 rows. Confirmed 3, plausible 1.

- **Panic marker on the caller's line (B2, minor)**: `runtime/panic/i32-remainder-by-zero.hd`
  and `runtime/panic/i64-remainder-by-zero.hd` mark `_ := remainder(1, 0)` (line 7),
  but `a % b` (line 5) raises the panic. The runner does not judge panic
  lines, and the README says the marker names the line that raises the panic.
  This pattern (the marker on the top-level call) is probably widespread.
  **Owner question:** should a panic marker sit on the raising line or on the
  entry call? Then fix all of them at once.
- **A latent second error (B2, minor)**: `names/use-declarations/module-qualified-without-use.hd`.
  `fn timeout(…) -> void: time.s(seconds)` returns a `Duration` from a `void`
  function. With the `use` added, it is still a `type-mismatch`. Fix:
  `_ := time.s(seconds)`.
- Plausible B7: `typing/valid/generics-recursive-instantiation-at-run-time-reports-no-depth-error.hd`
  cites `#generics`; it tests `types.generic.call-cycle.fixed-callee`.

#### b017 (`lang/03`, `lang/04`)

Ledger: b017 (44): 4 rows. Plausible 2, rejected 2.

- Plausible B7: `names/name-categories/a-struct-called-with-an-unknown-argument.hd` cites
  `names.type-as-value` (whose code is `type-used-as-value`) but expects
  `unknown-name`. `names/name-categories/duplicate-method-generic-sibling.hd` cites
  `names.type-param.no-redeclare`; a repeat inside one list is `names.type-param.unique`.
- Rejected: `tests-block-use-leak.hd`. The `use` repeats a test-prelude name,
  but line 5 is non-test code, so a leak of either the `use` or the test
  prelude would still be caught. A non-prelude name would be cleaner, not
  required. Also rejected: "hd stops at the first error, so the no-leak
  claims are unobservable". The runner's "no other located error" rule does
  observe them.

#### b021 (`lang/04-type-system.md`)

Ledger: b021 (45): 4 rows. Confirmed 3, covered by A6 1.

- **Title copied across a generated series (B1)**: the
  `runtime/panic/int-widths-i8-overflow-panics-at-127-plus-1-0{1..4}.hd` series
  all share one title, but `-03` uses `u8` subtraction and `-04` uses `u16`
  multiplication. `runtime/panic/int-widths-a-u64-product-past-the-maximum-panics.hd`
  multiplies `u64`. These numbered series come from one spec example split
  into cases, and they keep the example's title. Each case should get its
  own title.
- `typing/warnings/unused-nested-optional-binding.hd`: an accept case under
  `warnings/`, already in A6.

**Brief v2.1 (from b025 on):** reviewers read all of a batch's fixtures in
one or two tool calls, and run `hd check` only to confirm a suspected flag,
batched in one call. Why: a batch took 6 to 14 minutes, mostly in 15 to 55
sequential model steps, not in the compiler.

#### b019 (`lang/04-type-system.md`)

Ledger: b019 (45): 8 rows. Confirmed 4 (+ the A10 class), plausible 3, rejected 1.

- **Systemic, script check A10** (`a10_legal_titles.py`, output `full/a10.tsv`):
  **all 40** `*-legal.hd` accept fixtures carry the `# test:` title of their
  reject twin word for word. An example is
  `typing/valid/literal-types-a-large-negative-float-literal-within-f64-is-accepted.hd`,
  "…is rejected", which is an accept case. The pilot's `…-130-legal.hd` is one
  of them. Fix: give each `-legal` fixture a title that states what is
  accepted.
- **Title promises evaluation in a type-check case (B1)**:
  `typing/valid/literals-a-signed-literal-under-power-checks-and-evaluates-{87,88,89}.hd`.
  A `type` case never runs, so "`-2 ** 2` evaluates to -4" is untested. Fix:
  make them `runtime` cases with an `assert_equal`.
- Plausible: `typing/valid/literal-types-float-literals-multiply-as-floats.hd`
  (uses only float literals). `runtime/valid/map-grow-and-remove.hd` (the header
  names order, replace, reindex and repair rules; the code exercises none).
  `typing/valid/literals-a-negated-bare-literal-is-signed-through-parentheses-{83..86}.hd`
  (no spec rule says this).
- Rejected: `typing/invalid/literal-types-a-negated-literal-cannot-become-unsigned.hd`.
  The marker is on line 5, which is where `hd` reports; the reviewer
  miscounted.

#### Systemic: runtime fixtures that run nothing (script check A11)

**Found from b020; confirmed by hand.** A runtime case whose `main` is not
`pub` and that has no `tests:` block is a pass without running anything:
- `module.entry.private-main`: a private `main` is not an entry point.
- With no `tests:` block, the runner skips `test FILE` and only does the
  entry run (README "Runtime Execution").
- `hd FILE` then fails with `missing-entry-point` (exit 101). But the Rust
  harness turns that into a pass: `crates/hd_driver/tests/conformance.rs`
  ~1811 says "no entry, nothing to run" and returns `Verdict::Pass`. This
  contradicts the README, where the entry run must exit 0.
- It started with commit `ef8d31f5a` (2026-10-09, "Entry selection requires
  pub main; warn on a private main"). Fixtures written with `fn main` before
  that became vacuous silently.

Script `a11_private_main.py` (output `full/a11.tsv`) lists them:
`closure-result-keeps-requirement-row`, `generic-inherent-box`,
`i64-widening-checked`, `index-then-call-element`, `ord-supertrait-dispatch`,
`reference-enum-replace-build`, `result-ok-unit`,
`shared-enum-data-computed-once`, `trait-default-method-instantiates-params`,
`u8-checked-add`, `validation-rules`, `validation-rule-order`,
`validation-nesting-option-cap` (all `runtime/valid/`). Several hold asserts
that never execute: `validation-nesting-option-cap` has 10, `validation-rules` has 5.
Fix: make `main` `pub` (or move the body into `tests:`). Then make the
harness fail a runtime accept case that has neither an entry point nor a
test, as the README already says.

```python
# a11_private_main.py
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
```

#### b020 (`lang/04-type-system.md`)

Ledger: b020 (34): 6 rows. Confirmed 3, plausible 3.

- `runtime/valid/u8-checked-add.hd`, `runtime/valid/i64-widening-checked.hd`: the A11 class above.
- `runtime/panic/u8-add-overflow.hd`: the panic marker is on the caller's line
  (the owner question in b018).
- Plausible: `runtime/valid/f32-to-integer-saturates.hd` and `…/f64-…`. Their
  `usize` column expects 32-bit maxima, which looks target-dependent.
  `runtime/valid/impl-trait-value-copy.hd`: the expected copy-on-bind is not
  stated in the cited section.

#### b023 (`lang/04-type-system.md`)

Ledger: b023 (45): 7 rows. Confirmed 2, plausible 4, unverified 1.

- **Duplicate (C1)**: `runtime/valid/value-argument-reference-change.hd` ~
  `runtime/valid/value-argument-legacy-alias.hd`. The same program, with a
  renamed helper and reworded titles.
- **Checks that cannot fail (B3)**: `runtime/valid/generic-forward-bound-explicit-first-slot.hd`.
  The default `= string` equals the bound, so the test passes even if bound
  inference breaks (not hand-checked; plausible).
- Plausible: `value-argument-legacy-alias.hd` (`view_len_after` is never called;
  test 2 repeats test 1). `map-of-lists-nested-push.hd` (the title claims linear
  time). `loop-list-cache-physical-cow.hd` (a subset of a sibling).
  `a-list-mapped-through-a-reference-keeps-its-elements-across-the-function.hd`
  (no write through a reference).
- Unverified B5: `value-argument-sibling-change.hd`.

#### b022 (`lang/04-type-system.md`)

Ledger: b022 (46): 9 rows. Confirmed 2, plausible 5, covered by A6 2.

- **Wrong citation (B7)**: `typing/invalid/a-cast-with-no-argument-is-an-argument-count-error.hd`
  and `…-two-arguments-…` cite `types.cast.syntax`, which has no error. The
  `argument-count` rule is the next one, `types.cast.argument`.
- Plausible: `typing/invalid/a-tuple-spread-of-a-diverging-value-is-never.hd`
  (no rule says a tuple spread of `never` is `never`; `types.never.not-structural`
  points the other way, which may be a spec question).
  `typing/invalid/let-pattern-mut-a-plain-field-in-the-same-pattern-stays-readonly.hd`
  (no `mut` field in the pattern). `typing/valid/collection-value-fix-…-267.hd`
  (title). `runtime/valid/a-push-through-a-map-value-with-a-computed-key-keeps-every-element.hd`
  (the key isn't computed by a call). `runtime/valid/indexed-replacement-list-and-map.hd`
  (cites the list-only rule).
- `typing/warnings/let-mut-primitive-unused.hd`, `…-annotation-unused.hd`: accept
  cases under `warnings/`, already in A6.

#### b028 (`lang/04-type-system.md`)

Ledger: b028 (45): 4 rows. Confirmed 3, plausible 1. **First batch on brief
v2.1 with a private cache: 104 s and 10 tool calls (earlier batches took
340 to 860 s and 14 to 56 calls).**

- **Duplicates (C1)**: `typing/invalid/own-type-arity-too-many-{struct,enum,newtype}-parameter.hd`
  are the same programs as `typing/invalid/type-too-long-a-data-type-written-too-long-reports-once-{01,05,06}.hd`.
  The numbered ones add an unused 5-line preamble (`type Both`, `trait Show`,
  `trait Conv`) inherited from the spec example they were split from.
- Plausible B7: `typing/valid/generics-a-written-type-with-its-exact-arguments-is-valid.hd`
  (an accept case citing the error-only `types.generic.too-long`).

**Script check A12** (`a12_subset.py`, output `full/a12.tsv`): fixture A's
code lines all appear in a sibling B with the same anchor and expectation.
12 leads. They are not all redundant, because the extra lines in B can change
the meaning (for example a re-export, in `template-outside-trait-module` /
`template-reexported-from-other-package`). Each needs a look.

```python
# a12_subset.py
# A12: fixture A is redundant when another fixture B with the same
# expectation and the same cited anchor contains every code line of A
# (comments and blank lines stripped), including A's marked line.
# Then B tests everything A does; A adds nothing.
import os, re, collections
os.chdir('spec/conformance')
rows = [l.rstrip('\n').split('\t') for l in list(open('cases.tsv'))[1:]]
def code(p):
    out = []
    for l in open(p):
        m = re.search(r'#\s*(diagnostic|warning|panic):\s*[a-z0-9-]+\s*$', l)
        c = re.sub(r'\s+#.*$', '', l.rstrip()) if not l.lstrip().startswith('#') else ''
        if c.strip(): out.append(c + (' <MARK>' if m else ''))
    return out
groups = collections.defaultdict(list)
for p, ph, e, s in rows:
    if '-legal' in p: pass
    groups[(s, e, ph)].append(p)
for (s, e, ph), ps in groups.items():
    if len(ps) < 2 or len(ps) > 200: continue
    cs = {p: code(p) for p in ps}
    for a in ps:
        ca = collections.Counter(cs[a])
        if not ca: continue
        for b in ps:
            if a == b or len(cs[b]) <= len(cs[a]): continue
            if not (ca - collections.Counter(cs[b])):
                print(f'{a}\tA12\tevery code line is also in {b} ({e}, {s})')
                break
```

#### b027 (`lang/04-type-system.md`)

Ledger: b027 (36): 4 rows. Covered by A10 1, plausible 3. 279 s, 11 tool calls.

- The `…-38-legal.hd` title says "errors" on an accept case: the A10 class.
- Plausible: `…-36` single-argument (B7). `…-37` optional vs string literal (a
  possible tension between the join rule and `types.option.wrap`; spec
  question?). `parse/…/placeholder-in-ordinary-type` ~ `placeholder-in-a-declaration` (C1).

#### b030 (`lang/04-type-system.md`)

Ledger: b030 (31): 3 rows. Confirmed 1, plausible 2. 79 s, 8 tool calls.

- **Rejected for the wrong reason (B2)**: `typing/invalid/least-common-type-functions-with-different-results-and-rows-have-no-common-type.hd`.
  `[orders, count]` mixes `fn() -> string $ Db` with `fn() -> i32 $ Clock`. The
  results already differ, so `no-common-type` holds whatever the rows do. The
  unused `health() -> string $ Clock` is the sibling that would test the row
  join. The fixture almost certainly meant `[orders, health]`.
- Plausible: `…-generic-arguments-join-without-trait-value-conversions-06.hd`
  (cites `types.lct.sites`, whose code is `no-common-type`; expects
  `type-mismatch`). `…-02` is the `-01` case with the arguments swapped (C1).

#### b026, b029 (`lang/04-type-system.md`)

Ledger: b026 (45): 2 rows, plausible 2. b029 (22): 4 rows, confirmed 2, plausible 2 (94 s, 8 calls).

- **Wrong citation (B7)**: `typing/valid/row-inference-explicit-type-arguments-disambiguate-two-keys.hd`
  and `typing/valid/row-inference-a-unique-key-match-solves-the-argument.hd`:
  accept cases that cite the error-only `types.infer.ambiguous.code`.
- **Spec question (B7)**: `typing/valid/least-common-type-weakens-a-covariant-list-element.hd`
  joins `List[*mut Cell]` with `List[*Cell]` by weakening the element type.
  `types.lct.no-combine` seems to forbid combining conversions that way.
  Either the spec or the fixture is wrong.
- Plausible: `runtime/valid/lct-optional-injection.hd` adds no site beyond the
  two `optional-join-*` fixtures (C1).
  `runtime/valid/type-name-forms-a-default-method-names-its-traits-own-parameter.hd`
  (cites `default.written`). `typing/valid/generic-join-fresh-list-elements.hd`
  (the title says the list literal solves `T` at the call, but it is bound
  to a variable first).

#### b025 (`lang/04-type-system.md`)

Ledger: b025 (34): 1 row, plausible 1.

- Plausible B7: `typing/invalid/a-const-parameter-as-a-body-type.hd` cites
  `types.generic.const.type-position`; the error comes from `types.generic.const.parameter`.

#### b024 (`lang/04-type-system.md`, const generics)

Ledger: b024 (43): 7 rows. Confirmed 1, plausible 6.

- **A check that cannot fail (B3)**: `typing/invalid/const-argument-or-of-named-constants-is-its-value.hd`.
  It expects `Fixed[LOW | MID]` (`1 | 4 = 5`) not to match `Fixed[6]`. Any
  wrong evaluation other than 6 also passes, including `+` (also 5). A reject
  case can only show a value is *not* 6. Fix: an accept case with overlapping
  bits, `Fixed[1 | 3]` matching `Fixed[3]` (not `Fixed[4]`).
- Plausible B1: `runtime/valid/union-type-arguments-preserve-named-const-pipes.hd`
  (no union sits in a type slot). `typing/invalid/const-generic-symbolic-arithmetic.hd`
  (the "a parameter alone is accepted" clause is never used).
  `typing/valid/const-generic-i16-bitwise-not-plus-and-comparison.hd`
  ("wraps", but `~0` is `-1`).
- Plausible B7: `typing/invalid/a-row-in-a-const-slot.hd`,
  `a-row-parameter-in-a-const-slot.hd`, `a-const-expression-in-a-body-row-slot.hd`
  (the cited `argument.kind` rule names only type and trait names in const
  slots, not rows; a spec gap?).

#### b031 (`lang/04-type-system.md`)

Ledger: b031 (36): 4 rows. Plausible 4.

- Plausible C1: `typing/valid/literal-types-a-duration-literal-is-a-duration-beside-a-function-named-ms.hd`
  ~ `typing/valid/duration-literals.hd` (the same `500ms` literal and rule).
- Plausible B3: `runtime/valid/an-interpolated-open-literal-writes-at-its-settled-width.hd`
  ("-7 7" is the same for every signed default width).
- Plausible B4: `typing/valid/literal-one-fit-bound.hd` (the `let n: i64`
  annotation may fix `T` before the one-fit path runs).
- Plausible B1: `typing/valid/generic-parameters-at-the-limit.hd` (the title
  claims lifetimes plus other parameters in one declaration; the method has
  only lifetimes).

#### b033 (`lang/04-type-system.md`, literal inference)

Ledger: b033 (49): 3 rows. Plausible 3 (all one kind).

- **Plausible, accept cases that would pass under the default anyway (B3)**:
  `typing/valid/literal-first-use-closure.hd`, `typing/valid/literal-first-use-top-level.hd`,
  `typing/valid/literal-var-for-range-index.hd`. The literal's default type
  (`usize` for an unsigned `0`) equals the type the "first use" is meant to
  fix. So the case accepts even if the first-use rule is ignored. Fix: use a
  literal whose default differs from the type the use fixes (a signed `+0`
  where the use needs `usize`, or the reverse).

#### b032, b034, b035 (`lang/04-type-system.md`)

Ledger: b032 (42): 12 rows. b034 (44): 9 rows. b035 (46): 4 rows. Confirmed 1
kind, plausible ~20, spec question 1.

- **Same kind as b033, now 10 fixtures (B3, plausible-strong)**: literal-inference
  accept cases that would pass under the default type anyway, or where a
  later annotation fixes the type instead of the use the title names.
  `typing/valid/literal-first-use-{generic-wait,method,while-len,generic-join}.hd`,
  `typing/valid/literal-var-{method-fallback,range-bound,flow-types}.hd`,
  `typing/valid/literal-one-fit-receiver.hd` (plus b033's three). Fix all of them
  together: pick literals whose default differs from the type the rule
  should infer, and remove the annotations that fix the type early.
- **Spec question, target-dependent fixtures**: `runtime/panic/usize-width-u32.hd`
  expects `usize(u32::MAX) + 1` to overflow, which holds only on a 32-bit
  target. `types.num.usize-width` ties `usize` to the target. No fixture
  directive names a target, so a 64-bit implementation fails this case. b020's
  `f32/f64-to-integer-saturates.hd` (a 32-bit `usize` column) is the same
  kind. **Owner question:** add a `# fixture-target:` directive, or keep
  target-specific cases out of the suite?
- Plausible B7: 7 `runtime/panic/never-*.hd` cite `types.never.assignable` (a
  typing rule) but test evaluation order (`expr.order.applies`).
  `typing/invalid/impl-trait-borrow-two-sources-local.hd` and
  `typing/invalid/impl-trait-borrow-box.hd` (the code comes from
  `types.opaque.borrows.body`). `typing/invalid/literal-var-method-missing.hd`,
  `typing/invalid/literal-trait-value-no-one-fit.hd`,
  `runtime/valid/literal-erased-fallback.hd`.
- Plausible C1: `typing/valid/literal-first-use-fallback.hd` ~ `sign-fallback-unsigned.hd`.
  `typing/invalid/option-invariant.hd` ~ `typing/invalid/optional-payload-weakening.hd`.
- Plausible B1: `typing/invalid/readonly-edge-mut-self-call.hd` (the title says an
  `&` field; the field is `*Child`). `runtime/valid/optional-closure-arguments.hd`
  (passes a named function, not a closure).

#### b038 (`lang/04-type-system.md`)

Ledger: b038 (44): 2 rows. Plausible 1, rejected 1. 108 s.

- Plausible B3: `runtime/valid/fn-reference-variance.hd`. Test 2's
  `create().name == "Ada"` also holds if the reference were copied.
- Rejected: `…/row-variance-an-invariant-struct-argument-keeps-its-row.hd`.
  With an invariant `Holder[T]`, two types that differ only in their row
  match only if the row is erased, so the rejection does show the row is
  kept.

#### b037 (`lang/04-type-system.md`)

Ledger: b037 (50): 4 rows. Plausible 2, spec question 2 (the target class).

- **Target-dependent fixtures (owner question from b032–b035)**:
  `runtime/valid/usize-width-wasm32.hd` and `runtime/panic/usize-max-overflow-wasm32.hd`
  also assume a 32-bit `usize`. A grep for "wasm32" or "32-bit" in the
  fixtures finds 6 files: these two, `runtime/panic/usize-width-u32.hd`,
  `runtime/valid/sized-integer-arithmetic.hd`, `runtime/valid/num-parse-unsigned.hd`
  and `std/hash/default-hasher/hasher-write-methods.hd`. The README names no target, and
  no directive selects one.
- Plausible B3: `runtime/valid/union-members-name-self-and-hold-a-reference.hd`
  (`first()` never reads `x`, so the `&i32` payload is unused).
- Plausible C1: `typing/invalid/let-readonly-fresh-literal.hd` ~
  `typing/invalid/struct-field-write-plain-let.hd`.

#### b039 (`lang/04-type-system.md`)

Ledger: b039 (40): 6 rows. Plausible 6 (four are B7, which A9 already
suggests is common for heading-level citations).

- Plausible B7: `runtime/valid/string-concatenation-and-numeric-selectors.hd`
  (`_0` selectors, cites `#strings`). `typing/valid/explicit-discard-and-void-result.hd`
  (`_ :=`, cites `#result-types`). `typing/invalid/generic-enum-variant-without-context.hd`
  and `typing/invalid/result-constructor-without-context.hd` (`cannot-infer-type`
  comes from `types.generic.default.unsolved`).
- Plausible B1: `…/type-argument-defaults-a-struct-parameter-may-default-to-a-concrete-type.hd`
  (`Box[T = i32]` has no earlier parameter).
- Plausible C1: `typing/invalid/bare-err-is-unknown-name.hd` ~ `bare-ok-is-unknown-name.hd`.

#### b036 (`lang/04-type-system.md`)

Ledger: b036 (44): 7 rows. Covered by A2 1, plausible 6.

- `typing/invalid/tuple-rest-differs-from-plain.hd` = `tuple-rest-assign.hd`: already in A2.
- Plausible B1: `typing/valid/anyref-accepts-references.hd` (no `*mut T` value).
  `runtime/valid/mut-self-primitive-arithmetic.hd` (`self + 1` gives 42 for any
  integer `self`, so it shows nothing about `Self`).
  `runtime/valid/string-scalar-boundaries.hd` (offset 2, inside a scalar, is
  never checked). `runtime/panic/string-slice-at-continuation-byte.hd` (the title
  says `text[index]`; the code calls `slice`).
- Plausible B7: `typing/invalid/a-union-pattern-naming-a-variant-reports-once-per-match.hd`,
  `runtime/valid/impl-head-binds-only-in-a-tuple-rest.hd`.

#### b040 (`lang/04`, `lang/05`)

Ledger: b040 (44): 4 rows. Covered by A10 1, plausible 2, spec question 1.

- **Spec question**: `typing/invalid/a-compound-store-reads-through-apply-compound.hd`
  expects `type-mismatch` for `sink() += 1`, and `hd` agrees. But the cited
  `#callable-values` section never states that code for a compound store; A9
  flags it too. The spec should say what a compound store through `Apply`
  reports.
- `…-72-legal.hd`: the A10 class (the identity return touches nothing the
  title names).
- Plausible: `typing/invalid/only-apply-and-update-make-a-value-callable-store.hd`
  (B7). `typing/valid/a-store-and-a-compound-store-check.hd` ~ `typing/valid/callable-values.hd` (C1).

#### b042 (`lang/05-expressions.md`)

Ledger: b042 (40): 1 row. Plausible 1. A clean batch.

- Plausible B7: `runtime/valid/write-line-suspending-call-argument.hd` cites
  `#literals`; the suspending call inside the interpolation is `req.suspend.bang`.

#### b041, b043 (`lang/05-expressions.md`)

Ledger: b041 (28): 3 rows. b043 (45): 4 rows. Confirmed 3 (title class), plausible 4.

- **An accept case titled as a rejection (B1)**: `typing/valid/checker-a-try-error-that-does-not-convert-is-invalid-propagation-{96,97}.hd`
  and `typing/valid/propagation-an-ordinary-function-may-propagate-with-a-query.hd`.
  The same kind as A10, without the `-legal` suffix. See A10b below.
- Plausible: `runtime/valid/compound-assign-index-once.hd` (the title promises
  two things it never exercises). `runtime/valid/power-and-remainder-edges.hd`
  (exact float asserts where `expr.power.float.within-ulp` allows 1 ULP; a
  portability risk). `runtime/valid/short-circuit-and-conditional-evaluation.hd`
  (cites `!` rules but never uses `!`). `runtime/panic/counted-index-guards-keep-out-of-range-panics.hd`
  (the marker is on the caller's line; the b018 owner question).
A10b: accept fixtures whose title equals a reject fixture's title:       87. Script inline in the log section above (a10b).

#### b044 (`lang/05-expressions.md`)

Ledger: b044 (36): 6 rows. Covered by A10 1, plausible 4, unverified 1.

- `typing/valid/call-positional-and-named-arguments-for-distinct-parameters-are-valid.hd`: the A10 class,
  and it also cites the error-only `expr.call.duplicate`.
- Plausible: `runtime/valid/callable-values-run.hd` ("a child reads a copy" is
  untested). `typing/invalid/vararg-by-name-and-positional.hd` (B7).
  `typing/invalid/positional-spread-duplicates-vararg.hd` ~
  `separate-values-then-a-spread-duplicate-a-vararg.hd` (C1).

#### b045 (`lang/05-expressions.md`)

Ledger: b045 (45): 3 rows. Plausible 3.

- Plausible B1: `typing/valid/reference-equality-equality-of-a-reference-with-a-value-is-no-reference-equality.hd`
  writes `*a == b`, which dereferences first, so a reference compared with a
  value is never tested.
- Plausible B7: `runtime/panic/list-index-underflow.hd` (the panic is
  `integer-overflow` from `i - 1`, yet it cites `expr.index.list.unsigned`).
- Plausible C1: `typing/valid/float-remainder.hd` is covered by `runtime/valid/float-remainder-run.hd`.

#### b046 (`lang/05-expressions.md`, indexing)

Ledger: b046 (46): 4 rows. Plausible 4 (the panic-marker owner question 1, C1 2, B3 1).

- `runtime/panic/index-trait-map-missing.hd`: the panic marker is on the caller's
  line (the b018 owner question).
- Plausible C1: `typing/valid/index-trait-literal-key-default.hd` and
  `typing/valid/index-trait-tuple-key.hd` are covered by runtime cases with
  the same program plus value checks. See A13.
- Plausible B3: `runtime/valid/index-builtin-receivers-direct.hd`.

**Script check A13** (`a13_type_in_runtime.py`, output `full/a13.tsv`): a
`type` accept case whose code lines all appear in one `runtime` accept case,
which type-checks the same program and also runs it. 11 leads, for example
`typing/valid/local-functions.hd` in `runtime/valid/named-local-functions.hd`.
Leads only: an extra line can change the meaning.

```python
# A13: a `type` accept fixture whose every code line also appears in one
# `runtime` accept fixture. The runtime case type-checks the same program
# and also runs it, so the type case adds nothing (unless the extra lines
# change the meaning). Index on each fixture's rarest line to stay near-linear.
import os, re, collections
os.chdir('spec/conformance')
rows = [l.rstrip('\n').split('\t') for l in list(open('cases.tsv'))[1:]]
def code(p):
    return [re.sub(r'\s+#.*$', '', l.rstrip()) for l in open(p)
            if l.strip() and not l.lstrip().startswith('#')]
run = {p: collections.Counter(code(p)) for p, ph, e, s in rows if ph == 'runtime' and e == 'accept'}
freq = collections.Counter(l for c in run.values() for l in c)
index = collections.defaultdict(list)
for p, c in run.items():
    for l in c: index[l].append(p)
for p, ph, e, s in rows:
    if ph != 'type' or e != 'accept': continue
    c = collections.Counter(code(p))
    if len(c) < 3: continue
    rare = min(c, key=lambda l: freq.get(l, 0))
    for q in index.get(rare, []):
        if not (c - run[q]):
            print(f'{p}\tA13\tevery code line is in runtime case {q}'); break
```

#### Systemic: the formatter also deleted tokens (script check A7b)

`a7b_format_tokens.py` compares each fixture before and after `edea876ae`
with whitespace and comments removed. 660 fixtures changed. Most of them
only *gained* trailing commas, which is harmless. **21 lost tokens**
(`full/a7b-lost.tsv`), and for these the formatter rewrote the very form
under test:
- `lexical/processing-model/utf8-bom.hd`: **its leading U+FEFF was stripped**, the one thing it tests.
- `parse/valid/use-group-trailing-comma.hd`, `grammar/generic-parameters-and-bounds/trailing-commas-everywhere.hd`
  (17 commas), `grammar/use-declarations/use-declaration-forms.hd`, `grammar/use-declarations/single-and-grouped-use.hd`,
  `grammar/test-blocks/test-block.hd`, `grammar/patterns/spread-pattern-syntax.hd`, `lexical/whitespace-and-indentation/layout.hd`,
  `parse/valid/data-embedding.hd`, `parse/valid/functions-and-closures.hd`,
  `grammar/expressions/literal-field-comparison-value.hd`, `lexical/physical-and-logical-lines/implicit-continuation-in-delimiters.hd`,
  `std/json/parsing/json-number-grammar.hd`, `runtime/valid/module-initialization-before-main.hd`,
  `typing/valid/composite-ordering.hd`, `typing/valid/data-types.hd`,
  `typing/valid/construction-a-literal-giving-each-field-once-is-valid.hd`,
  `typing/valid/generic-data-embedding.hd`: trailing commas removed.
- `runtime/valid/module-qualified-function-value-parenthesized.hd` (b048: `(time.s)`
  became `time.s`, so it is now a duplicate of `module-qualified-function-value.hd`),
  `lexical/physical-and-logical-lines/operator-and-member-lines-continue-in-brackets.hd`, and
  **`grammar/statements/grammar-assignment-nonplace.hd`**, a reject case: parentheses removed.

Fix: restore all 21 from `edea876ae^` together with the A7 layout cases.
Then exclude fixtures from the formatter, or have the formatter keep
redundant parentheses, trailing commas and a leading BOM in fixtures.

```python
# A7b: fixtures whose tokens (not just whitespace or line breaks) the
# canonical-format commit changed: removed parentheses, trailing commas,
# reordered or rewritten forms. Comments are ignored.
import subprocess, re, sys, collections
C = 'edea876ae'
rows = {r[0]: r for r in (l.rstrip('\n').split('\t') for l in open('spec/conformance/cases.tsv')) if r[0] != 'path'}
names = subprocess.run(['git','show',C,'--name-only','--format=','--','spec/conformance'],capture_output=True,text=True).stdout.split()
def blob(rev, p):
    r = subprocess.run(['git','show',f'{rev}:{p}'],capture_output=True); return r.stdout.decode('utf-8','replace') if r.returncode==0 else None
def toks(t):
    t = '\n'.join(re.sub(r'(^|\s)#.*$', '', l) for l in t.split('\n'))
    return re.sub(r'\s+', '', t)
for p in names:
    rel = p.removeprefix('spec/conformance/')
    if rel not in rows: continue
    a, b = blob(C+'^', p), blob(C, p)
    if a is None or b is None: continue
    ta, tb = toks(a), toks(b)
    if ta != tb:
        # summarize: which characters were dropped or added
        ca, cb = collections.Counter(ta), collections.Counter(tb)
        lost = ''.join(sorted((ca-cb).elements()))[:20]; gained = ''.join(sorted((cb-ca).elements()))[:20]
        print(f'{rel}\tA7b\tlost {lost!r} gained {gained!r}\t{rows[rel][3]}')
```

#### b047 to b053 (`lang/05-expressions.md`)

Ledger: b047 (44): 2. b048 (40): 1. b049 (45): 8. b050 (43): 9. b051 (37): 3.
b052 (38): 5. b053 (49): 8. Highlights below; the rest are plausible B1/B7
rows in the TSVs.

- **Formatter**: `runtime/valid/module-qualified-function-value-parenthesized.hd` (A7b).
- **A check that cannot fail (B3)**: `runtime/valid/identity-ignores-permissions.hd`.
  It only asserts `!same(...)`, so an `is` that is always false passes. It
  needs a positive case.
- **Rejected for the wrong reason (B2), plausible**: `typing/invalid/identity-operands-a-literal-operand-has-no-identity.hd`
  (the error names `a: User`, a value, not the literal; use `a: &User`).
- **Title doesn't match the code (B1), plausible**: `…-an-unbounded-type-parameter-has-no-identity-115.hd`
  (`T < Named` is bounded). `…-a-trait-value-needs-the-other-type-to-convert-122.hd`
  (no trait value). `runtime/valid/logical-operators.hd`. `typing/valid/tuple-element-push.hd`
  (a type case claiming a runtime effect). `typing/invalid/tuple-rest-…-not-assignable.hd`
  (only enum `_0` is assigned).
- **A panic that cannot fire (B1), plausible**: `runtime/panic/power-…-panics-04.hd`.
  Base `2` defaults to `usize`, so `2 ** 32` does not overflow on a 64-bit
  `usize`. This is another target-dependent case.
- **Warnings not checked**: `typing/warnings/unsigned-comparison-countdown.hd` and
  `-explicit.hd` are accept cases (A6). The spec example says they warn
  `unsigned-comparison-always`, and `hd check` gives 0 warnings, so the lint
  may be unimplemented.
- **Exact float asserts against a 1-ULP rule (B7), plausible**:
  `runtime/valid/floating-power.hd`, `runtime/valid/power-float-powers-follow-ieee-pow.hd`
  (b041's `power-and-remainder-edges.hd` too). `expr.power.float.within-ulp`
  allows 1 ULP, so exact `==` on non-special inputs ties the fixtures to one
  libm.
- Plausible C1: `runtime/panic/power-…-panics-06.hd` ~ `-05` (the same u8 overflow
  to 256). Two `typing/valid/deferred-propagation-*` cases are subsets of the
  runtime `deferred-propagation-lowers-result-and-option-once.hd`.

#### b055 (`lang/05-expressions.md`)

Ledger: b055 (24): 2 rows. Confirmed 1, plausible 1.

- **Depends on map order (portability)**: `runtime/valid/map-loop-reference-control-flow.hd`
  asserts that a `for` loop over a map runs in the order a, b, c.
  `types.map.order.unspecified` promises only determinism, and the README
  bans depending on map order. **Brief v2.2 (from b060 on) adds B8,
  "portable"**: no dependence on map or set order, `usize` width, or exact
  floats the spec allows to vary.
- Plausible: `runtime/valid/user-iterable-for-loop.hd` (lines 27 and 28 are the same assert).

#### b054 (`lang/05`, `lang/06`)

Ledger: b054 (45): 6 rows. Covered by A6 2, plausible 4.

- `typing/warnings/unused-local-binding.hd`: an accept case under `warnings/` (A6),
  and it cites `#blocks-and-completion` rather than `flow.unused.lint`. **About the
  A6 class:** several reviewers report that `hd check` emits none of these
  lints, which run under `hd lint`. So these 21 accept cases may be meant to
  show "check stays silent", but in a directory that promises a warning, and
  citing the lint rule. The owner should decide whether they move to
  `typing/valid/` or become `cli/` lint cases.
- Plausible C1: `typing/invalid/signed-exponent.hd` ~ `signed-integer-exponent.hd`.
- Plausible B1: `typing/valid/operators-arithmetic-and-bitwise-operators-apply-to-standard-integer-types.hd`
  (no impl targets). `typing/invalid/identity-operands-an-integer-against-another-struct-is-one-error.hd`
  (both operands are values; b053 found the same in `-103` and `-104`, so this
  series probably shares one bug). `typing/valid/control-flow.hd` (a type case
  claiming laziness).

#### Script check A14: copies that differ only by renaming

`a14_renamed.py` (output `full/a14.tsv`) finds fixtures with the same phase and
expectation whose code is identical after renaming the names each file
declares (functions, bindings, parameters, fields, declared types) and
dropping a `pub fn main` that only prints. Directives (`# fixture-*`,
`# expect-stdout`) are kept, so tree-path and scenario variants stay
distinct. **17 groups.** 11 were already found (A2 and earlier batches). The
6 new ones are all confirmed by `diff`:
- `typing/invalid/match-a-variant-call-pattern-without-a-dot-is-a-bare-variant-pattern.hd` = `typing/invalid/bare-ok-call-pattern.hd` (b056)
- `typing/invalid/match-a-field-named-twice-in-a-struct-pattern-is-a-duplicate.hd` = `typing/invalid/duplicate-data-pattern-field.hd`
- `typing/invalid/match-a-named-variant-pattern-field-the-variant-lacks-is-unknown.hd` = `typing/invalid/variant-pattern-unknown-field.hd` (b057)
- `typing/valid/call-positional-and-named-arguments-for-distinct-parameters-are-valid.hd` = `names/function-and-closure-scopes/parameter-names-two-function-parameters-with-distinct-names-are-valid.hd`
- `typing/valid/field-shorthand-a-shorthand-field-counts-as-given.hd` = `…-an-unknown-shorthand-suppresses-the-missing-field-error-298.hd`
- `typing/invalid/propagation-without-target.hd` = `typing/invalid/propagation-nearest-function-target.hd`
- `parse/invalid/placeholder-in-ordinary-type.hd` = `parse/invalid/placeholder-in-a-declaration.hd` (b027)
- `typing/valid/shared-reference-disjoint-field-write.hd` = `typing/valid/dependent-result-borrow-field-instead.hd`

A pattern: the numbered `rejected-pattern-binds-*`, `checker-*` and
`declaration-checks-*` series were split from spec examples, and several
repeat a named fixture that already existed. An early version of A14 that
renamed every identifier wrongly paired 22 `pointer-loan-*-ref-*` panics with
their `-mut-` twins (`get_ref` vs `get_mut`). Renaming only declared names
fixed that.

```python
# A14: fixtures with the same expectation whose code is identical after
# alpha-renaming local identifiers (each lowercase-initial name not in a
# keyword/std allowlist becomes v1, v2, ... in first-seen order) and dropping
# a trivial `pub fn main` that only prints. Catches renamed copies A2 misses.
import os, re, collections
os.chdir('spec/conformance')
KEEP = set('''fn pub let mut match if else for in while loop break continue return struct enum trait impl
type use tests it assert assert_equal true false self Self and or not as is pass defer by where
println print main string bool i8 i16 i32 i64 u8 u16 u32 u64 usize f32 f64 void never List Map
Result Option Some None Ok Err dyn const'''.split())
def norm(p):
    lines = [l.rstrip() if re.match(r'#\s*(fixture-|expect-stdout|expect-empty)', l) else re.sub(r'\s+#.*$', '', l.rstrip())
             for l in open(p) if l.strip() and (not l.lstrip().startswith('#') or re.match(r'#\s*(fixture-|expect-stdout|expect-empty)', l))]
    t = '\n'.join(lines)
    t = re.sub(r'\npub fn main\(\) -> void( \$ Console)?:\n    println\([^\n]*\)\s*$', '', t)
    declared = set(re.findall(r'\bfn\s+(\w+)', t)) | set(re.findall(r'\b(\w+)\s*:=', t)) \
        | set(re.findall(r'\blet\s+(?:mut\s+)?(\w+)', t)) | set(re.findall(r'\bfor\s+(\w+)\s+in', t)) \
        | set(re.findall(r'[(,]\s*(?:mut\s+)?(\w+)\s*:', t)) | set(re.findall(r'^\s+(\w+):\s', t, re.M))
    names = {}
    def sub(m):
        w = m.group(0)
        if w in KEEP or w[0].isupper() or w not in declared: return w
        return names.setdefault(w, f'v{len(names)+1}')
    t = re.sub(r'\b[A-Za-z_][A-Za-z0-9_]*\b', sub, t)
    # also canonicalize type names declared in the file
    tn = {}
    for m in re.finditer(r'\b(?:struct|enum|trait|type)\s+([A-Z]\w*)', t): tn.setdefault(m.group(1), f'T{len(tn)+1}')
    return re.sub(r'\b[A-Z]\w*\b', lambda m: tn.get(m.group(0), m.group(0)), t)
rows = [l.rstrip('\n').split('\t') for l in list(open('cases.tsv'))[1:]]
g = collections.defaultdict(list)
for p, ph, e, s in rows:
    if p.endswith('-legal.hd'): pass
    g[(ph, e, norm(p))].append(p)
for (ph, e, _), ps in g.items():
    if len(ps) > 1: print(f'{ps[0]}\tA14\t{e}: same code after renaming as ' + ', '.join(ps[1:]))
```

#### b056, b057, b058 (`lang/06-control-flow.md`)

Ledger: b056 (45): 7 rows. b057 (46): 14 rows (10 are the A10 class). b058 (42): 2 rows.

- **Code under test never runs (B4)**: `runtime/valid/literal-patterns.hd`;
  `integer(-1)` is never called, so the `-1 => 10` arm never runs.
- **Duplicates**: 171, 173 and 174 above (A14). Also `typing/invalid/bool-match-missing-false.hd`
  ~ `nonexhaustive-bool-match.hd` (C1, plausible).
- Plausible B7: `runtime/valid/owned-match-payloads-across-suspension.hd` (cites a
  guard rule; it has no guard). `runtime/valid/negative-float-arms-split-by-value.hd`
  (cites the error-only `flow.match.unreachable`). `typing/valid/for-over-mut-iterator.hd`.
  `runtime/valid/iterator-single-pass.hd`.
- Plausible B2: `typing/invalid/bare-variant-pattern-in-match.hd` (the payload-arity
  rule may also apply to the marked line).
  `…/match-an-unsolved-scrutinee-binds-nested-names-without-a-second-error.hd`.
- `typing/warnings/variant-binding-name-mismatch.hd`: A6.

#### b059 (`lang/06-control-flow.md`)

Ledger: b059 (45): 6 rows. Confirmed 4, plausible 2.

- **A second error behind the marked one (B2)**: `typing/invalid/let-else-falls-through.hd`.
  `falls_through` has no `$ Console` row but calls `println` (std
  `println[T < Display](value: T) -> void $ Console`), which is a second
  error. **Script check A15** (below) lists 9 reject cases with this shape.
  Some are deliberate (the "pure callback" and "omitted-result row" cases test
  the missing row), so each needs a look.
- **Depends on map order (B8)**: `runtime/valid/for-map-snapshot-insert.hd`
  (`seen == ["a","b"]`), `runtime/valid/for-map-snapshot-rebuild.hd`
  (`["a","b","c","d"]`), `runtime/valid/iterator-shape-versus-value-changes.hd`.
- Plausible B1: `…/a-list-parameter-loop-…-shrinks-the-source.hd` (3 of 4 subtests
  use index ranges). `…/branch-joins-keep-their-values-and-cleanups.hd` (no
  release is observed).

```python
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
```

#### b060 to b063 (`lang/06`, `lang/07`)

Ledger: b060 (46): 2. b061 (?): 2. b062 (42): 5. b063 (41): 3. Plausible 12.

- **Claims about message text the runner never judges** (plausible; the
  same kind as b007's hint fixtures): `runtime/panic/sign-fallback-balance-underflow.hd`
  cites `flow.panic.report.fallback`, but the runner checks only the panic
  category (README "Judging a Case").
- **Titles from a split series that don't fit the case (B1)**: three
  `typing/invalid/tuple-rest-a-spread-pattern-*` fixtures. "Needs a rest
  element", but the pattern has one; "covers every list", but the case shows
  non-exhaustiveness. The same kind as b021's `int-widths-*`.
- **Type case claiming runtime behavior (B1)**: `typing/valid/using-transfer.hd`
  (LIFO release order). Also b050's `tuple-element-push.hd`, b054's
  `control-flow.hd`, and b019's `checks-and-evaluates-87..89`. A recurring kind.
- **A check that cannot fail (B3)**: `runtime/valid/a-suspending-reference-copies-its-cell-and-not-a-fresh-capture.hd`
  (`copied()` returns 0 whether the value is copied or shared).
  `runtime/valid/defaults-reference-earlier-parameters.hd` (the "function values
  do not inherit defaults" part never tries a one-argument call).
- Plausible C1: `typing/invalid/parameter-default-later-parameter.hd` ~
  `default-later-parameter-earlier-twin.hd`. `runtime/valid/unit-pattern-void-success.hd` ~
  `match-unit-pattern-ok.hd`. `typing/warnings/match-payload-binding-name-swap.hd` ~
  `variant-binding-name-mismatch.hd` (both A6 as well).
- Plausible B7: `typing/valid/default-values-defaults-before-a-rest-a-trailing-closure-or-at-the-end-are-valid.hd`
  (cases 115 and 116 are type-parameter defaults).

#### b064 (`lang/07-functions.md`)

Ledger: b064 (46): 8 rows. Plausible 8 (C1 4, B7 2, B1 2).

- Plausible C1, **numbered series repeating named fixtures again**: `…-function-value-argument-is-reported-once-60.hd`
  ~ `generic-function-value-argument-unsolved.hd`. `…-placeholder-is-reported-once-58.hd`
  ~ `cannot-infer-type.hd` (both `make::[_]()`). `…-named-nowhere-is-reported-once-57.hd`
  ~ `unresolved-generic-return-placeholder.hd`. `runtime/valid/generic-suspending-associated-function-qualified-call.hd`
  ~ line 33 of `an-associated-function-takes-type-arguments-through-its-type.hd`.
- `typing/valid/…-57-legal-type-arguments.hd`: an accept case titled "is reported
  once". It is the A10 kind under a different suffix (`-legal-type-arguments`),
  so A10 misses it.
- Plausible B7: `runtime/valid/generic-function-value-instantiation.hd`,
  `runtime/valid/a-mutated-copy-keeps-its-callers-value.hd` (both cite
  `#function-types-and-values`; the rules are under other headings).
- Plausible B1: `runtime/valid/function-field-returns-mutable-data.hd`.

#### b068 (`lang/07-functions.md`)

Ledger: b068 (41): 2 rows. Plausible 2.

- Plausible B1: `runtime/valid/call-keeps-argument-usable.hd` passes `&mut items`, so
  passing by value is never tested.
- Plausible B3: `runtime/valid/unowned-parameter-cleanup-survives-every-return.hd`
  (the output matches even if the cleanup is skipped).

#### b067 (`lang/07-functions.md`)

Ledger: b067 (45): 6 rows. Plausible 6.

- Plausible B3: `runtime/valid/temporary-method-receivers.hd` asserts only the
  first character, which a dropped cursor change would still return.
  `runtime/valid/generic-local-function-constructs-an-unused-struct.hd` (the
  built struct is never observed).
- Plausible B7: `typing/invalid/a-trait-with-type-arguments-is-not-callable.hd`
  (the code comes from `expr.call.apply.not-callable`).
  `typing/invalid/a-function-value-takes-no-type-arguments.hd`
  (`fn.generic.explicit.too-long-count`).
- Plausible B1: `typing/invalid/row-slot-bare-explicit-argument.hd` (the argument
  is a type, not a key). `…-stay-clean-22-legal-rows.hd` (two forms; the spec's
  `$ AppRow` form is missing).

#### b066 (`lang/07-functions.md`)

Ledger: b066 (44): 3 rows. Plausible 2, rejected 1.

- Plausible C1: `typing/valid/omitted-result-cycle-one-annotated.hd` is covered by
  `runtime/valid/inferred-results-recursion-through-an-annotated-function-is-valid.hd`
  (the A13 kind). Plausible B7: `runtime/valid/mono-default-boolean-and-scalar-decisions.hd`.
- Rejected C1: `…/suspending-closures-a-closure-that-only-creates-another-still-captures-for-it.hd`
  vs `nested-mutable-captures.hd`. `make!` (suspending) against `make` is the point
  of the case.

#### b065 (`lang/07-functions.md`)

Ledger: b065 (44): 6 rows. Confirmed 2, plausible 4.

- **Formatter**: `runtime/valid/same-line-function-body.hd`. The same-line
  body was moved to the next line (it is in A7), so `fn.body.same-line` is untested.
- **Expected output never checked (B3)**, **script check A16** (below): the
  expected output sits in a trailing `# prints: …` comment, which is not a
  directive, so the runner never compares it.
  `runtime/valid/known-closure-consumers-preserve-captures.hd` and
  `runtime/valid/known-consumer-mixed-loop-contexts.hd` (both are also in A4:
  no assert and no `expect-stdout`). `runtime/valid/zero-iteration-callback-preserves-dependent-read.hd`
  (has `expect-stdout` too; check that it covers this line). Fix: turn each
  into an `# expect-stdout:` header.
- Plausible: `runtime/valid/track-caller-temporary-arguments.hd` (B3: string
  lengths don't show freeing). `typing/invalid/void-final-method-named-debug-is-not-prelude-debug.hd`
  (B1). `typing/valid/function-parameter-stored-in-a-local-through-a-reference.hd`
  (B7: an accept case citing the error-only `fn.capture.local-store`).
  `runtime/valid/a-closure-changing-a-captured-collection-shares-it-with-its-scope.hd`
  ~ `capture-variable-cell.hd` (C1).

```python
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
```

#### b069 (`lang/07-functions.md`, bound references)

Ledger: b069 (46): 5 rows. Plausible 5.

- Plausible B2, **two problems on the marked line**: `typing/invalid/bound-reference-readonly-receiver.hd`.
  When the marked `not-writable` problem is fixed (a `mut counter`), `hd`
  reports `local-reference-escapes` on the same return line. An
  implementation that checks escapes first gives a different code on the
  marked line and fails. Fix: take `counter: &Counter`, so only the
  readonly write is wrong (the reviewer tried this in a scratch copy).
- Plausible B7: `typing/invalid/a-returned-bound-reference-to-a-local-escapes.hd`,
  `typing/invalid/bound-reference-to-an-interior-place-is-local.hd` (cite
  `fn.ref.bound.address`, which has no error).
- Plausible B1: `typing/valid/trailing-bound-takes-the-bound-row.hd` and
  `typing/valid/bound-closure-takes-the-bound-row.hd` (the bodies use no row key,
  so acceptance doesn't show the row is adopted; the runtime twin covers it).

#### b070, b071 (`lang/07-functions.md`)

Ledger: b070 (44): 2 rows. b071 (46): 0 rows.

- **Expected output never checked (A16)**: `runtime/valid/known-consumer-mixed-loop-contexts.hd`
  (also found by script).
- Plausible B4: `runtime/valid/inlined-function-result-variance.hd` (no `@inline`,
  so inlining is the optimizer's choice and the title's claim is not forced).

#### b072 (`lang/07-functions.md`)

Ledger: b072 (41): 5 rows. Plausible 5.

- Plausible C1: `typing/invalid/a-tuple-vararg-given-fewer-values-than-its-arity.hd` ~ `tuple-vararg-arity.hd`.
- Plausible B7: `typing/invalid/local-reference-escapes-function-type-result-param-type-parameter-tuple.hd`
  (the cited rule's code is `type-mismatch`; the case expects `local-reference-escapes`).
  `runtime/valid/literal-suffix-default-parameter.hd`.
- Plausible B1: `runtime/valid/tuple-bound-vararg-call.hd`, `typing/invalid/function-result-representation-change.hd`.

#### b073 (`lang/07`, `lang/08`)

Ledger: b073 (38): 5 rows. Plausible 4, rejected 1.

- Plausible B3: `runtime/valid/copy-update-evaluation-order.hd` (both bang calls are
  pure, so the output can't show the order).
- Plausible B1: `typing/invalid/suspending-fn-weaken-the-weakening-keeps-the-row-and-the-result-nested.hd`
  (rejected by `List` invariance, not by the weakening rule; the wrong-reason kind).
- Already known: `nonfinal-spread.hd` (A2 duplicate; also a B7 lead).
  `nonfinal-vararg-declaration.hd` ~ `nonfinal-vararg-then-parameter.hd` (C1).
- Rejected B2: `typing/invalid/data-literal-unknown-field.hd` (`Point { y: 1 }` also
  omits `x`). An unknown field suppresses the missing-field error by design;
  see `…-an-unknown-shorthand-suppresses-the-missing-field-error-298.hd`.

#### b076 (`lang/08-data-and-enums.md`)

Ledger: b076 (46): 3 rows. Confirmed 1, plausible 2.

- **A check that cannot fail (B3)**: `runtime/valid/count-tests-skip-no-object-in-a-garbage-cycle.hd`
  asserts `total >= 0` on an unsigned count, which is always true.
- Plausible B3: `runtime/valid/shared-enum-generic-one-copy.hd` (the "item" label
  holds however many copies are made; it needs a side-effect counter).
- Plausible C1: `runtime/valid/reference-cycles-are-ordinary-data.hd` ~ the first test of
  `dropped-cycles-are-reclaimed.hd`.

**Script check A17** (`a17_tautologies.py`, output `full/a17.tsv`): `assert(true)`,
`assert_equal(x, x)` with the same literal on both sides, and `>= 0` on a count.
23 hits. Most are legitimate: the `assert-equal-*` and `test-prelude-*`
fixtures test the harness itself, so `assert_equal(1, 1)` is the point. Real
leads: `count-tests-skip-no-object-in-a-garbage-cycle.hd:43` (confirmed),
`encoding-base64-vectors.hd:97` (`Base64.Url == Base64.Url`), and
`multibyte-scalar-strings.hd:14` (`"\u{E9}" == "\u{E9}"`, which may have meant a
literal `é` against the escape).

```python
# A17: asserts that cannot fail: assert(true), assert_equal(X, X) with the
# same expression on both sides, or a `>= 0` comparison (always true for an
# unsigned count such as len() or a usize/u* total).
import re, os
os.chdir('/Users/hd/code/test/hd-lang/spec/conformance')
for l in list(open('cases.tsv'))[1:]:
    p = l.split('\t')[0]
    if not p.startswith('runtime/'): continue
    for i, ln in enumerate(open(p), 1):
        s = ln.strip()
        why = None
        if re.match(r'assert\(\s*true\s*[,)]', s): why = 'assert(true)'
        m = re.match(r'assert_equal\(\s*([^,]+?)\s*,\s*([^,]+?)\s*[,)]', s)
        if m and m.group(1) == m.group(2) and not re.search(r'[(!]', m.group(1)): why = 'assert_equal(x, x)'
        if re.search(r'\b(len\(\)|count|total|size)\s*>=\s*0\b', s): why = '>= 0 on a count'
        if why: print(f'{p}\tA17\tline {i}: {why}: {s[:70]}')
```

#### b074, b075 (`lang/08-data-and-enums.md`)

Ledger: b074 (43): 6 rows. b075 (30): 8 rows. Plausible ~12.

- **Numbered `declaration-checks-*` series again**, titles that promise a case
  the code lacks (B1): `…-an-unknown-shorthand-suppresses-the-missing-field-error-298.hd`
  (`Point { x, y }` has neither an unknown shorthand nor a missing field; A14
  also pairs it with `-300`). `…-an-unknown-field-suppresses-…-296.hd`.
  `…-a-cycle-is-reported-once-…-354.hd` (no cycle; also ~ `-352`).
  `…-cycle-through-a-generic-type-is-a-cycle-356.hd` (no cycle).
  `…-does-not-embed-itself-352.hd` (an accept case citing an error-only rule).
- Plausible C1: `…-a-type-embeds-at-most-three-fields-346.hd` ~ `three-embedded-fields.hd`.
  `…-an-embedding-chain-is-at-most-three-deep-348.hd` ~ `embedding-depth-three.hd`.
  `runtime/valid/enum-payload-mut-shallow.hd` ~ `enum-value-fixed-at-construction` test 2.
- Plausible B3: `runtime/valid/struct-derive-eq-debug.hd` (line 19 writes the value
  already held). Plausible B1: `runtime/valid/enum-niche-partial-moves.hd`.
- Plausible B2: `typing/invalid/variant-duplicate-argument.hd`,
  `typing/invalid/variant-unknown-payload-field.hd` (the required `second` is never
  given; it may be suppressed by design, as for data literals).
- Plausible B7: `typing/invalid/promoted-mut-method-on-readonly-receiver.hd` (the rule
  is `fn.method.auto-address.place-writable` in chapter 07).

#### b077 (`lang/08-data-and-enums.md`)

Ledger: b077 (43): 3 rows. Plausible 3 (all UNSURE): `typing/invalid/copy-update-private-field-other-module.hd`
(B2), `runtime/valid/a-generic-struct-update-keeps-its-inherited-counts.hd` (B1),
`typing/invalid/struct-embedded-field.hd` ~ `embedded-non-data.hd` (C1).

#### b078 (`lang/08`, `lang/09`)

Ledger: b078 (45): 4 rows. Confirmed 2 (A11), plausible 2.

- The A11 class, found independently by the reviewer: `runtime/valid/shared-enum-data-computed-once.hd`
  and `runtime/valid/ord-supertrait-dispatch.hd` (private `main`, so nothing runs).
- Plausible B1: `runtime/valid/derive-members-of-data-and-enums.hd` (`_1` is never
  emitted). `typing/invalid/variant-constructors-a-generic-variant-value-without-an-expected-type-cannot-be-inferred.hd`
  (`Either.Left` *is* a function value; the real error is an unsolved generic,
  so this is the wrong-reason kind).

#### Systemic: reject cases that fail on a missing `&` (script check A18)

b015 and b079 each found a reject case rejected because a value was passed
where a trait-value *reference* was expected (`hd`: "a value never becomes a
trait value reference by itself, so write `&`"), not for the rule the case
names. `a18_missing_amp.sh` runs `hd check` (private cache, about 25 s) on all
377 `reject:type-mismatch` cases and lists those whose first error is that
hint: **15** (`full/a18.tsv`). When the cited rule is about references (anchors
ending `.reference-values`, `.reference-fix`, `ref-value`), the missing `&` is
the point. **The likely wrong-reason ones** (the rule is about something else):
- `names/member-resolution/local-impl-trait-value-before-declaration.hd` (b015, confirmed)
- `typing/invalid/concrete-reference-bound-associated-type-mismatch.hd` (b079, confirmed)
- `typing/invalid/closure-not-inspectable.hd`, `typing/invalid/spelled-function-type-not-inspectable.hd`
  (Inspectable rules)
- `typing/invalid/trait-generic-instantiations-unrelated.hd` (invariance)
- `typing/invalid/inspectable-child-trait-without-impl.hd` (`trait.erase.child-impl`)
- `typing/invalid/concrete-value-binding-mismatch.hd`, `typing/invalid/counted-trait-value-from-value.hd`,
  `requirements/providers-behind-references/provider-ref-value.hd` (need a look)
Fix pattern: add the `&` so that only the rule under test is broken, then
check that the expected code still fires.

```bash
#!/bin/bash
# A18: a `type-mismatch` reject case whose actual error is hd's
# "a value never becomes a trait value reference by itself, so write `&`"
# hint, so the case fails for a missing `&`, not for the rule it names.
# Leads only: a few fixtures test exactly that rule (trait.dyn.ref...).
cd /Users/hd/code/test/hd-lang/spec/conformance
HD=/Users/hd/code/test/hd-lang/target/release/hd
awk -F'\t' '$2=="type" && $3=="reject:type-mismatch" && $1 !~ /fixture-package-tree/ {print $1"\t"$4}' cases.tsv |
while IFS=$'\t' read -r f spec; do
  grep -q '^# fixture-package' "$f" && continue
  out=$(HD_CACHE="$1" timeout 30 "$HD" check "$f" 2>&1 | grep -m1 '^error')
  case "$out" in *"never becomes a trait value reference by itself"*) printf '%s\tA18\t%s\n' "$f" "$spec";; esac
done
```

#### b079, b080, b081 (`lang/09-traits.md`)

Ledger: b079 (43): 5. b080 (31): 8. b081 (28): 3.

- **Wrong reason (B2)**: `concrete-reference-bound-associated-type-mismatch.hd` (A18 above).
  `typing/invalid/impl-targets-a-bare-parameter-target-with-a-delegation-is-rejected.hd`
  (line 8 also fails with `invalid-delegation`: `by inner` names no field; the
  reviewer saw this in a scratch copy).
- **Duplicates**: `typing/valid/impl-targets-a-trait-impl-for-a-list-of-trait-values-is-well-formed.hd` ~
  `trait-value-as-target-argument.hd` (also in A12). Plausible: `…-ownership-only-in-its-own-module-67.hd`
  ~ `-53` (only `pub` differs). `…-in-the-module-of-the-trait-or-target-is-valid-66.hd` ~ `-65`.
  `typing/valid/outer-constructor-permits-impl.hd` ~ `trait-argument-owner-impl.hd`.
- Plausible B2: `…-nonlocal-64/-69/-71.hd` (the body's `missing_name` may be a second
  error). Plausible B7: `typing/invalid/erase-readonly-to-mut-inspectable.hd`.
- Plausible B1: `runtime/valid/embedded-field-satisfies-trait.hd`,
  `runtime/valid/primitive-display-bound-and-trait-values.hd` ("every primitive" but 4 are tested),
  `…-tuple-template-bounds-…-17.hd`, `…-62.hd`, `…-71.hd`, `runtime/valid/tuple-impl-target.hd`.

#### b082 (`lang/09-traits.md`)

Ledger: b082 (41): 3 rows. Plausible 3.

- Plausible, missing-`&` kind (not in A18 because the expected code differs):
  `typing/invalid/any-bare-none.hd` omits the `&` that the rule's example
  (`&.None`) has, so it doesn't test reference erasure.
- Plausible B7: `typing/invalid/a-call-through-a-parameter-whose-bounds-declare-no-such-method.hd`.
  Plausible B1: `…-an-alias-of-a-non-enum-has-no-variants-35.hd` (no alias is declared).

#### b083 (`lang/09-traits.md`)

Ledger: b083 (38): 2 rows. Plausible 2 (B7; both also in A9's pattern):
`typing/invalid/a-body-projection-under-an-unknown-bound.hd` (cites a rule whose
code is `unknown-associated-type`; expects `unknown-trait`) and
`typing/valid/projection-binding-chain-normalizes-40-deep.hd` (an accept case citing
the error-only depth rule).

#### b087 (`lang/09-traits.md`, trait value references)

Ledger: b087 (27): 5 rows. Plausible 5.

- Plausible C1: `typing/invalid/readonly-trait-value-mutating-method.hd` ~ `dyn-ref-mut-method-through-shared.hd`.
- Plausible B1: `runtime/valid/dyn-ref-receiver-forms.hd` (no `mut self` call through
  `&mut dyn`). `typing/invalid/dyn-ref-mutable-upgrade.hd` (the header says `&dyn` to
  `&mut dyn`; the code converts the counted `*dyn` to `*mut dyn`).
- Plausible B7: `typing/invalid/dyn-ref-value-list-literal.hd` (A18 listed it; the fix
  is `box(...)`, rule `trait.dyn.ref.counted.new`). `runtime/valid/assign-dyn-child-to-supertrait.hd`
  (its header comment names the wrong chapter).

#### b084 (`lang/09-traits.md`, trait resolution depth)

Ledger: b084 (45): 5 rows. Covered by A18 1, plausible 2, spec question 2.

- `typing/invalid/trait-generic-instantiations-unrelated.hd`: A18 (missing `&`).
- **Spec question, how bound depth is counted**: `typing/valid/trait-resolution-depth-limit.hd`
  (titled "exactly 64 nested bounds") is accepted by `hd`, but a literal reading of
  `trait.bound.depth` counts 66. `typing/invalid/trait-resolution-depth-pending.hd`
  rejects 65 `Wrap` layers, while the limit fixture accepts 65 `Box` layers. The
  spec should state the counting with one worked example, and the boundary
  fixtures should sit at exactly N and N+1 under it.
- Plausible B1: `typing/valid/bounds-a-bound-two-levels-deep-proves.hd` (the
  body is `Box[Box[i32]]`, not 32 levels). Plausible B2: `…/trait-resolution-depth-in-instantiation-trial.hd`.

#### b085 (`lang/09-traits.md`, derive)

Ledger: b085 (41): 4 rows. Covered by A18 1, plausible 3.

- `typing/invalid/concrete-value-binding-mismatch.hd`: A18 (fails on the missing `&`).
- **Wrong reason (B2), plausible-strong**: `typing/invalid/derived-hash-field-not-hash.hd`.
  The field type `Opaque` lacks `Eq` as well as `Hash`, so the error names `Eq`.
  Fix: give `Opaque` an `Eq` implementation so only `Hash` is missing.
- Plausible C1: `runtime/valid/derived-newtype.hd` is covered by
  `newtype-derive-eq-hash-and-ordering-follow-the-base-type.hd`. Plausible B7:
  `…/newtype-derive-handwritten-methods-on-newtypes-keep-their-bodies.hd`.

#### b086 (`lang/09-traits.md`, trait values)

Ledger: b086 (46): 4 rows. Plausible 4.

- Plausible B2: `typing/invalid/dyn-conversion-without-impl.hd`. The name says the
  implementation is missing, but the error comes from the bare value (missing `&`;
  in A18, whose anchor `trait.dyn.convert.reference-values` made it look legitimate).
  An impl-less `*Square` would isolate the missing implementation.
- **Titles out of date after a spec change (B1)**: `typing/valid/generic-method-dynamically-safe.hd`
  and `typing/valid/nested-dynamically-safe-trait-values.hd` talk about "dynamically
  safe" traits. Today `trait.dyn.except-fn` lets every trait be a `dyn` type. The
  same kind as b011's row-list titles.
- Plausible B7: `typing/invalid/trait-reference-impl-target.hd` (the rule is `trait.target.trait-value.error`).

#### b088 (`lang/09-traits.md`) and script check A10c

Ledger: b088 (46): 6 rows. Covered by A18 2, confirmed 1, covered by A10c 1, plausible 2.

- A18 (missing `&`): `typing/invalid/inspectable-child-trait-without-impl.hd`,
  `typing/invalid/generic-erasure-without-bound.hd`. The reviewer independently
  found that both fail for the bare value, not the missing impl or bound.
- **A check that cannot fail (B3)**: `runtime/valid/erased-error-result.hd`. Its
  `.None` arm only does `assert(true)`.
- Plausible: `typing/invalid/a-method-with-its-own-type-parameter-through-a-dyn-value.hd`
  (an unused `trait Sink`; cites the hint rule).

**A10c** (`a10c_legal_any.py`): A10 matched only names ending in `-legal.hd`.
Some accept twins are named `…-legal-<suffix>.hd`. A10c matches `-legal`
anywhere in the name plus a title that describes a rejection. 22 hits,
including 4 that A10 missed: `…-is-one-error-legal-body-positions.hd`,
`…-is-a-syntax-error-legal-types-and-receivers.hd`,
`…-adds-no-second-error-169-legal-bounds.hd` and `…-is-reported-once-57-legal-type-arguments.hd`.

```python
# A10c: an accept fixture whose name contains `-legal` (anywhere) and whose
# `# test:` title contains words that describe a rejection
# ("is an error", "is rejected", "errors", "is one error", "reports", "cannot",
# "is invalid", "is misplaced", "is nonlocal", "is unknown").
import os, re
os.chdir('spec/conformance')
bad = re.compile(r'\b(is an error|is rejected|errors?\b|is one error|reports?|cannot|is invalid|is misplaced|is nonlocal|is unknown|is not allowed)', re.I)
for l in list(open('cases.tsv'))[1:]:
    p, ph, e, s = l.rstrip('\n').split('\t')
    if e != 'accept' or '-legal' not in os.path.basename(p): continue
    t = open(p).readline().strip()
    if bad.search(t): print(f'{p}\tA10c\t{t[:90]}')
```

#### b089, b090 (`lang/09-traits.md`)

Ledger: b089 (42): 4 rows. b090 (46): 9 rows.

- **A check that cannot fail (B3)**: `runtime/valid/typeid-same-name-modules.hd:15` and
  `runtime/valid/erased-error-result.hd:55`, where a `.None => assert(true, …)` arm
  checks nothing. A17 now also matches `assert(true)` inside match arms (25 hits).
- Covered by A18: `typing/invalid/spelled-function-type-not-inspectable.hd` (fails on
  the missing `box`, not the function type).
- Plausible C1: `…/inherent-impls-a-member-name-may-repeat-across-targets-that-do-not-unify.hd`
  (also in A13; its title says "unify" though the targets are disjoint).
  `typing/invalid/a-concrete-row-impl-overlaps-a-later-generic-row-impl.hd` and
  `row-parameter-data-impl-overlap.hd` ~ `a-generic-row-impl-overlaps-a-later-concrete-row-impl.hd`.
- Plausible B1: `runtime/valid/from-propagation-and-panic.hd` (the panic branch is
  never reached). `runtime/valid/inspectable-primitives-collections-options.hd`.
  `typing/invalid/num-sealed-user-number.hd`. Plausible B7: `typing/valid/impl-trait-nested-trait-method.hd`,
  `typing/valid/impl-trait-trait-method.hd`. Plausible panic/diagnostic line:
  `typing/invalid/impl-unknown-trait-cross-module.hd` (the error is raised in the tree file).
- Rejected: `typing/invalid/tests-block-impl-overlap.hd`. Plain `hd check` accepts
  it, but the runner always passes `--tests`, and `hd check --tests` reports the
  expected `overlapping-impl`. The brief now tells reviewers to use `--tests`.

#### b091 (`lang/09-traits.md`, resolution)

Ledger: b091 (45): 9 rows. Plausible 6, rejected 2, unverified 1.

- Plausible B2: `typing/invalid/nonlocal-impl.hd` (line 10 reads a non-`pub` field
  from another module, a second private-member error).
- Plausible B1: `runtime/valid/explicit-type-args-associated.hd` and
  `runtime/valid/explicit-type-args-method.hd` (they type-check unchanged without
  `::[i32]`, so the explicit argument selects nothing). `runtime/valid/failed-trial-no-leak.hd`
  (no capture). `runtime/valid/generic-method-candidate-local.hd`. Plausible B7:
  `typing/invalid/fn-trait-user-impl.hd`.
- Rejected C1: `candidate-closure-selection-reversed.hd` and
  `candidate-scope-isolation-reversed.hd`. The reversed implementation order
  checks that resolution doesn't depend on declaration order, which is a real
  property even though the rule doesn't name it.

#### b092 (`lang/09-traits.md`, downcast)

Ledger: b092 (41): 4 rows. Plausible 4.

- Plausible C1: `typing/invalid/downcast-value-type-target.hd` ~ `downcast-target-not-inspectable.hd`
  (the same `downcast_ref::[fn() -> i32]` line). `…/downcast-downcast-ref-with-a-type-argument-on-dyn-inspectable.hd`
  (the A13 kind).
- Plausible B1: `…/downcast-downcast-mut-and-downcast-ref-take-a-type-argument.hd`
  (no `downcast_mut` on a bounded parameter). `typing/valid/tuple-derived-traits.hd` (never uses `Ord`).

#### b094 (`lang/09-traits.md`, delegation)

Ledger: b094 (28): 1 row. Plausible 1: `typing/valid/trait-delegation.hd` is covered by
`runtime/valid/trait-delegation-forwards.hd` (the A13 kind). The rest is clean.

#### b093 (`lang/09-traits.md`, sealed traits)

Ledger: b093 (36): 10 rows. Plausible 7, unverified B5 3.

- Plausible B2, **one implementation line breaking two rules**: `…-151`, `-152`, `-153`, `-154`
  (sealed-user-impls series). The spec says the same `impl` line also breaks
  `structure-outside-template` or `trait.super.missing`. An implementation that
  reports both, or reports the other one first, fails these cases.
- Plausible B7: `-169`, `-170` (cite `#sealed-traits` for `unknown-trait`).
- Plausible C1: `-150` ~ `typing/invalid/inspectable-user-implementation.hd`.

#### b095 (`lang/09-traits.md`, implementations)

Ledger: b095 (31): 7 rows. Confirmed 4, plausible 3.

- The A11 class: `runtime/valid/generic-inherent-box.hd` and
  `runtime/valid/trait-default-method-instantiates-params.hd` (private `main`; the
  reviewer found them independently).
- **Wrong reason / two problems (B2)**: `typing/invalid/orphan-impl.hd` is
  `impl Display for i32` with a `to_string(&self)` method. std already has
  `impl Display for i32` (`lib/std/format.hd:478`), so the line also breaks
  `trait.impl.unique`. The method also doesn't match std `Display`'s
  signature. Fix: use a std trait and std type pair that std doesn't
  implement, with the right method.
- **Duplicate (C1)**: `typing/invalid/trait-impl-missing-method.hd` = `missing-trait-method.hd`
  (only `struct User: pass` against a one-field struct). Plausible:
  `trait-method-result-type-mismatch.hd` ~ `trait-method-signature.hd`.
- Plausible B1: `typing/valid/trait-implementations-an-impl-may-omit-a-method-with-a-default-body.hd`
  (an accept case titled "must write every"). `typing/valid/closable-handle-closed.hd`
  (a type case claiming disposal).

#### b096, b097 (`lang/10-modules.md`)

Ledger: b096 (39): 1 row. b097 (45): 1 row. A pair took 132 s.

- **Depends on map order (B8)**: `runtime/valid/boundary-map-round-trip.hd` asserts
  that a `Map` iterates in insertion order.
- Plausible B1: `typing/valid/module-initialization-a-function-reads-an-earlier-binding.hd`
  (nothing reads `count` and `read()` is never called).

#### b098, b099 (`lang/10-modules.md`)

Ledger: b098 (43): 1 row. b099 (45): 3 rows. Plausible 4.

- Plausible B1: `runtime/valid/entry-exit-code-map.hd` (`main` takes only the `Ok`
  path; the error-to-`ExitCode(2)` mapping is never run through `main`).
  `runtime/valid/init-constants-only-module-reads-its-constants.hd`.
  `typing/invalid/optional-none-comparison-still-checks-eq-initialization.hd` (a type
  case that claims an optimizer property).
- Plausible C1: `typing/invalid/host-on-a-private-trait.hd` ~ `host-declaration-private.hd`.

#### b100, b101 (`lang/10-modules.md`)

Ledger: b100 (45): 1 row. b101 (45): 0 rows. Clean apart from one plausible
diagnostic-line question: `typing/invalid/module-path-case-fold-collision.hd` marks
the body line, while sibling duplicate-module fixtures mark the declaration line.

#### b104, b105 (`lang/10-modules.md`)

Ledger: b104 (25): 1 row. b105 (35): 1 row.

- **Formatter (A7b)**: `parse/valid/use-group-trailing-comma.hd`. The reviewer
  independently found the missing trailing comma that the formatter removed.
- **Stale term in the title (B1)**: `typing/invalid/assert-equal-fieldless-data-without-partial-eq.hd`.
  hd has no partial-equality trait (`trait.cmp.one-eq`).

#### b102, b103 (`lang/10-modules.md`)

Ledger: b102 (45): 4 rows. b103 (44): 1 row. Plausible 4, rejected 1.

- **Wrong reason (B4), plausible-strong**: `typing/invalid/relative-path-into-std.hd`.
  The `user-modules` tree has no `std` module, so `super.std.text` fails as a
  missing module and the "no relative path into std" rule is never reached.
  `typing/invalid/integration-super-above-test-root.hd` (`tests/checkout.hd` is a
  root file, so `root-file.super` rejects first).
- Plausible: `runtime/valid/slicing-every-range-form-slices-a-string.hd` (no range
  syntax at all). `typing/invalid/debug-missing-derive.hd` (B7).
- Rejected C1: `root-file-super.hd` ~ `root-file-lib-super.hd` (the `main.hd`/`lib.hd`
  difference is deliberate; see A2 triage).

#### b106, b107 (`lang/10`, `lang/11` requirements and providers)

Ledger: b106 (43): 11 rows. b107 (45): 6 rows. Plausible ~15.

- **Checks that cannot fail (B3), plausible-strong; a provider-test pattern**:
  - `requirements/provider-access/multi-provider-use-order.hd`: `left.value() + right.value()` is
    commutative, so swapping the use order still gives 42. Use `left*100 + right`.
  - `requirements/construction-time-requirement-binding/closure-captures-provider-scope.hd` and
    `requirements/provider-scopes/nested-provider-scope.hd`: the provider value is discarded
    (`_ := clock`) and the function returns a constant 42, so a broken capture or
    the wrong nearest provider still passes. Call a method of the provider.
  - `requirements/cancellation/cold-suspension-cancel-idempotent.hd`: `ready!` has no visible
    effect, so running the cold body by mistake would go unnoticed.
- **Stale title (B1)**: `requirements/cancellation/await-cancelled-job-cancels-all.hd` names an
  `all!` combinator that doesn't exist (it is `join!`).
- Plausible B2: `requirements/cancellation/abandon-active-ancestor.hd` (the panic marker is on
  the caller's line; the b018 owner question).
- Plausible B7: `requirements/cancellation/suspending-call-with-defer.hd` and `…-in-scoped-defer.hd`
  (cite cancellation; nothing is cancelled). `runtime/valid/result-pattern-nested-enum.hd`.
  `typing/invalid/unknown-std-module.hd` and `unknown-dep-module.hd` (the cited rules
  state no error; `unknown-module` is defined only in the README diagnostics list).
- Plausible: `…/context-values-a-provider-that-does-not-implement-its-key-is-a-type-mismatch.hd` (B1). `requirements/provider-scopes/nested-provider-restoration.hd`
  ~ `…-a-later-entry-wins-over-a-spread-269.hd` (C1). `parse/invalid/direct-variant-use.hd`
  (`hd` reports it at use resolution; is it really a parse error?).

#### b114, b115 (`lang/11-requirements-and-suspension.md`)

Ledger: b114 (45): 2 rows. b115 (39): 2 rows.

- Duplicate: `requirements/requirement-rows/row-subsumption/row-union-omitted-result.hd` = `row-union-private-result.hd` (already in A2).
- Plausible B3: `requirements/suspending-functions/pending-first-poll-return-wide-result.hd` (returns 42,
  which fits in `i32`, so a narrow result type still passes).
- Plausible: `requirements/requirement-polymorphism/row-extension-provider-restoration.hd` (B1, restoration
  is never observed). `requirements/provider-scopes/row-forced-overlap-nearest.hd` ~
  `dynamic-row-parameter-nearest.hd` (C1).

#### b110, b111 (`lang/11-requirements-and-suspension.md`)

Ledger: b110 (45): 2 rows. b111 (45): 0 rows.

- Plausible B3: `requirements/providers-behind-references/provider-install-box.hd` (`count()` prints 10 whether
  the value is shared or copied, so "a value provider is a copy" is untested).
- Plausible C1: `requirements/mutable-providers/provider-share-shared-for-mutable.hd` ~ `provider-ref-shared-for-mutable.hd`.

#### b108, b109, b112, b113 (`lang/11-requirements-and-suspension.md`)

Ledger: b108 (42): 0. b109 (44): 6. b112 (43): 5. b113 (40): 3.

- Already known duplicates: `row-parameter-unmarked.hd` (A2), `requirement-key-unknown-binding-rule.hd` (A2).
- **Depends on map order (B8)**: `requirements/requirement-rows/row-subsumption/row-union-list-runs.hd` (`from_map` iterates
  a map literal and asserts `health=noon;orders=db;`).
- Plausible C1: `requirements/requirement-rows/row-parameters/a-local-row-fixit-skips-an-enclosing-const-parameter.hd` and
  `a-local-row-fixit-uses-the-active-declaration.hd`. They match siblings in code
  and line and differ only in the fix-it, which the runner doesn't judge (the
  b007 kind of message-only difference).
- Plausible B3: `…/key-assoc-function-…-only-has-an-empty-vtable.hd` (the output is
  the same whatever the vtable holds).
- Plausible B1/B4: `checker-…-107` (the title says `println` is forbidden in direct
  contexts; nothing tests it). `checker-…-102` (`block_on` is never reached).
  `defer-indirect-block-on-checks.hd`. `requirement-key-any-trait.hd`.
  `requirements/requirement-rows/row-slots/row-slot-bare-alias-argument.hd`.
  `requirements/requirement-rows/row-sets/a-delayed-function-trait-value-keeps-the-callback-requirement.hd`
  (nothing is delayed).
- Plausible B7: `…/inferred-rows-a-growing-key-in-a-recursive-row-is-too-deep.hd`
  (the error is `types.generic.polymorphic-recursion`).
  `requirements/requirement-rows/row-subsumption/row-union-a-call-through-the-union-needs-every-key.hd` (an `if`
  union, not a list literal).

#### b116, b117 (`lang/11-requirements-and-suspension.md`)

Ledger: b116 (31): 5 rows. b117 (35): 3 rows.

- `requirements/requirement-rows/closure-result-keeps-requirement-row.hd`: the A11 class (private `main`, nothing checked).
- Accept cases titled as rejections (the A10b kind): `requirements/requirement-rows/requirement-rows-main-declaring-console-may-print.hd`,
  `…-generic-parameter-used-in-the-other-kind-is-a-kind-mismatch-229.hd`.
- Plausible C1: `requirements/requirement-rows/call-missing-requirement.hd` ~ `missing-requirement.hd`.
  `bang-call-in-plain-function.hd` ~ `bang-call-outside-suspension.hd`.
  `bang-call-in-defer.hd` ~ `defer-suspends.hd`.
- Plausible: `requirements/cancellation/a-cancelled-join-cleans-its-completed-text-result.hd` (B3,
  cleanup not observed). `requirements/suspending-functions/mutable-suspension.hd` (B1, nothing is `mut`).

**Ticket size (owner direction):** from b128 on, each Haiku reviewer takes four
old batches (about 180 fixtures), not one or two. The 43 remaining old batches
(128 to 170) become 11 reviewers. Output files stay one per old batch.

#### b118, b119 (`lang/11`, `lang/13`)

Ledger: b118 (11): 3 rows. b119 (45): 0 rows (the generated pointer-loan matrix is clean).
- Panic marker on the caller's line (the b018 owner question): `requirements/suspendt-protocol/reentrant-cancel.hd`,
  `requirements/suspendt-protocol/dispose-cancels-active-ancestor.hd`.
- Plausible C1: `requirements/suspendt-protocol/second-suspension-drive.hd` ~ `block-on-completed-suspension.hd`.

#### b120, b121 (`lang/13-values-and-references.md`, alias conflicts)

Ledger: b120 (45): 1 row. b121 (45): 0 rows. Clean except a plausible spec
question: `runtime/panic/pointer-loan-extra-pointer-lent-as-mut-argument-panics.hd`
claims that a bare `*mut` argument converted to `&mut` takes a loan, but rule 7
covers only paths that dereference a `*mut`, and `ref.convert.lend` doesn't say
a loan is taken.

#### b122, b123 (`lang/13-values-and-references.md`, pointer loans)

Ledger: b122 (45): 0 rows. b123 (45): 4 rows.

- **Code under test never runs (B4), plausible-strong**: `runtime/valid/pointer-loan-extra-deep-shared-loans-kept.hd`
  (`if n > 1000` with `n <= 70`, so the `&mut` that should keep the loans never
  runs) and `…-deep-shared-loans-many-objects.hd` (`if k > 100000` with `k <= 70`).
- Plausible C1: `…-deep-shared-loans.hd` ~ `-kept` (depths 40 and 70 are both under
  the 31-count limit, so they exercise the same path).
- Plausible B1: `…-elide-read-only-visitor.hd` (`see` takes `&mut self`).

#### b126, b127 (`lang/13-values-and-references.md`, dependent results)

Ledger: b126 (37): 1 row. b127 (44): 4 rows.

- Duplicate, already in A2: `typing/valid/a-copied-box-keeps-a-dependent-reference-independent-of-its-list.hd`.
- Plausible B7: `typing/invalid/a-pointer-read-from-a-place-keeps-its-root-nested-field.hd`
  (cites a call rule; there is no call). `runtime/valid/dependent-result-capture-static.hd`
  (an accept case citing the error-only `ref.dep.capture-elide.ambiguous`).
- Plausible B1: `typing/valid/a-copied-local-reference-in-a-list-keeps-its-own-source.hd`.
  Plausible C1: `move-while-borrowed-nested-100-levels.hd` ~ the 33-level twin
  (both are beyond any limit, so they likely take the same path).

#### b124, b125 (`lang/13-values-and-references.md`)

Ledger: b124 (42): 8 rows. b125 (41): 13 rows.

- **Generated title misleading, test probably intended (B1, plausible)**: the 8
  `runtime/valid/pointer-loan-{loop,ret,stmt,struct}-{mut,ref}-local-write.hd`. The
  title says "an alias writes the place", but for a *local* struct no `*mut`
  alias can reach the place, so the alias writes a different box. That seems
  to be the point of the matrix's "local" column (no loan needed, runs). The
  generator's title template (`spec/tools/generate-pointer-loan-matrix.ts`)
  should say so, for example "an alias cannot reach the local's place".
- **Code under test never runs (B4)**: `runtime/valid/match-reference-field-loan.hd`
  (`across_call`, `across_store` and `across_guard` are never called). Plausible:
  `runtime/valid/a-plain-entry-borrows-its-cell-reference.hd` (`helper!` is never awaited).
- **A check that cannot fail (B3)**: `runtime/valid/provider-local-replaced-through-a-reference-keeps-its-counts.hd`
  (the replacement is the same provider, so skipping the write gives the same 2).
- Plausible B7: the `provider-local-*-keeps-its-counts` trio (cites the Copy
  section; the rule is `ref.type.local-uncounted`). `typing/invalid/box-local-reference-alias-write.hd`
  and `box-local-reference-tuple.hd` (`ref.box.local` names no error).
  `typing/invalid/reference-container-upgrade.hd`.
- Plausible B1: `runtime/valid/list-local-shared-before-a-change-keeps-its-copy.hd`,
  `typing/invalid/reference-shared-to-mut-result.hd` (`&T` in the title; the code uses `*i32`),
  `runtime/valid/constant-size-reference-allocation.hd`.

#### b128 to b131 (`lang/13-values-and-references.md`); first 4-batch reviewer, 340 s for 137 fixtures

Ledger: b128 (44): 2. b129 (45): 2. b130 (14): 1. b131 (34): 8.

- Plausible B2: `typing/invalid/dependent-result-function-type-position.hd` (line 8
  `&u.name` returns a reference to the value parameter `u`, a second error
  behind line 7).
- **Owner question, the panic-marker line again (b018)**: in the `runtime/panic/element-reference-*`
  and alias-change family, the clear, extend, insert, pop, push, remove-at, map
  insert and get_or_insert cases mark a line inside the callee, while swap,
  sort, store, replace and field-store mark the call line. One convention
  should cover all of them.
- Plausible B1: `typing/valid/suspension-holds-arguments.hd` (no `'static` argument).
  `runtime/valid/reference-arguments-keep-their-values.hd` (no tuple or `*T`).
  `typing/invalid/element-reference-shared-and-mut-argument.hd` (says "at any index";
  both use `xs[1]`). Several `runtime/panic/*-aliases-panics.hd` titles describe a
  completed run while the case panics.
- Plausible C1: `typing/valid/element-reference-binding.hd` (the A13 kind).
  `runtime/panic/mut-parameter-then-closure-pushes-through-an-alias-panics.hd`.

#### b136 to b139 (`lang/13-values-and-references.md`, lifetimes)

Ledger: b136 (45): 3. b137 (45): 1. b138 (41): 0. b139 (45): 0. Plausible 2, rejected 2.

- **Message-only claim (plausible)**: `typing/invalid/reference-equality-wraps-a-compound-operand-in-its-value-fix.hd`.
  The title is about the fix-it's parentheses, but the runner checks only the
  code. The b007 kind: a candidate for a `cli/` case.
- Plausible B1: `typing/invalid/lifetime-enum-box.hd` ("a local", but it is a parameter).
- Rejected B6: `typing/valid/fn-bound-nested-captures.hd` and `runtime/valid/fn-bound-lifetime-field.hd`
  ("a plain `let` is reassigned"). `types.value.write.let` covers field writes;
  `types.value.reassign-binding` allows reassigning a plain `let` as a whole.

#### b132 to b135 (`lang/13-values-and-references.md`, loans)

Ledger: b132 (41): 4. b133 (41): 4. b134 (45): 1. b135 (43): 1. Confirmed 2, plausible 8.

- **Duplicates (C1), confirmed by `diff`**: `runtime/panic/list-element-place-push-panics.hd`
  = `element-binding-box-alias-push-panics.hd` (only the binding names and the
  `it` label differ; A14 missed it because the `it` strings differ).
  `runtime/panic/nonescaping-closure-keeps-captured-root-loan.hd` =
  `dependent-result-captured-list-parent-replacement-panics.hd` (only `main`
  differs). Plausible: `a-pointer-loan-rejects-an-aliased-root-write-closure-write.hd`
  ~ the same file (the closure runs from `main` against a helper).
- Plausible B1: `runtime/panic/inlined-capture-of-an-alias-keeps-other-loans.hd` (the title
  says the opposite of the outcome). The `inlined-scalar-writer-*` pair (no
  `@inline`; the "inlined" path isn't forced). `runtime/panic/replace-pointer-on-live-loan-path-panics.hd`
  (only one of the two slots is replaced). `runtime/valid/dyn-reference-source-replaced-during-its-call.hd`.
  `typing/invalid/implicit-lend-cancellation-actors.hd` (the title describes a runtime outcome).
- Plausible B7: `runtime/valid/deep-recursive-release.hd`.

#### b140 to b143 (`lang/13-values-and-references.md`, references)

Ledger: b140 (40): 1. b141 (45): 3. b142 (44): 3. b143 (42): 3.

- **Code under test never runs (B4)**: `runtime/valid/reference-field-stores-preserve-rhs-alias-writes.hd`
  (`Pair::assign`, `Pair::compound` and `changed_both` are never called).
- `runtime/valid/reference-enum-replace-build.hd`: the A11 class (private `main`), and a
  subset of `reference-enum-replace.hd`.
- Plausible C1: `typing/valid/reference-tuple-element-comparator.hd` and `…-place.hd` are
  covered by `runtime/valid/reference-tuple-element-run.hd` (the A13 kind).
  `runtime/valid/a-reader-of-a-non-copyable-value-beside-a-field-move-in-a-loop.hd` ~
  `field-move-after-local-borrow.hd` (also A2: identical to the runtime box fixture).
- Plausible B1: `runtime/valid/mutable-data-paths-share-identity.hd` (copies a `*mut`, not
  `&mut`). `runtime/valid/reference-aliases-see-writes.hd` (`r` and `s` are never live
  together). Plausible B7: `typing/invalid/lifetime-trait-result-outlives.hd`.
- Unverified B5: two `associated-payload-writes-*` (an unused `Box2`).

#### b144 to b147 (`lang/13`, then `std/` chapters: collections, cmp, cli)

Ledger: b144 (45): 0. b145 (45): 2. b146 (45): 3. b147 (45): 4.

- **Depends on map or set order (B8)**: `std/collections/deque/collection-structs-snapshot-iteration.hd`
  (set order), `std/collections/map-methods/map-drain.hd`, `std/collections/map-methods/map-extend.hd` (map order).
- **Target width (B8)**: `runtime/valid/list-drain-inclusive-overflow.hd` (end 4294967295
  overflows only with a 32-bit `usize`; the target owner question).
- **A check that cannot fail (B3)**: `std/cmp/min-and-max/cmp-min-max.hd` (the tie check
  uses values that can't be told apart, so `min` or `max` picking either passes).
- Plausible C1: `readonly-mutation.hd` ~ `reference-write-through-shared-field.hd`.
  Plausible: `initialization-…-01.hd` (B7), `slicing-a-range-value-is-a-slice-key.hd` (B1).

#### b148 to b151 (`std/collections.md` and the generated pointer-loan matrix)

Ledger: b148 (45): 6. b149 (1): 0. b150 (45): 45. b151 (36): 26. Most of the 71
rows in b150 and b151 are one kind:

- **Owner question, a citation forced by the tier rule**: 148 generated `pointer-loan-*`
  fixtures cite `std/collections.md#r-std-collections.uniq-list.type` (50),
  `…uniq-map.type` (49 and more) or `std/time.md#r-std-time.helper.sleep` (36),
  never the alias-conflict rule they test (`ref.excl.alias.*`). This is
  deliberate: `sectionOf` in `spec/tools/generate-pointer-loan-matrix.ts` picks a
  std anchor when the fixture imports a std item, because the README tier rule
  says a fixture using a stdlib-tier item must cite a stdlib chapter. The
  result is that the spec column no longer says what the case tests. Options:
  allow a secondary citation, add a stdlib rule about the alias semantics of
  `UniqList`/`UniqMap` to cite, or build these fixtures on language-tier types only.
- **Depends on map order (B8)**: `std/collections/map-methods/map-entries-most-common.hd`, `std/collections/map-methods/map-retain.hd`.
- **Depends on machine memory (B8)**: `uniq-list-reserve-overflow.hd` expects
  `heap-exhausted` for 4294967295 elements, which may fit on a 64-bit machine.
- Plausible B3: `span-index-of-span-skip.hd` (cites the cost rule but counts no
  comparisons, so a naive search gives the same output).
- Plausible B1: `uniq-list-assigned-through-pointer-in-bang-function.hd` (no suspension),
  `uniq-map-borrowed-payload.hd`.
- Rejected: "the panic category `borrow-conflict` appears in no spec file".
  `flow.panic.known-categories` (chapter 06) lists it.

#### b152 to b155 (`std/collections`, `std/encoding`, `std/fs`, `std/hash`)

Ledger: b152 (45): 47 rows (45 are the tier-forced pointer-loan citation above).
b153 (45): 17 (mostly the same). b154 (42): 3. b155 (45): 9.

- **Checks that cannot fail (B3)**:
  - `std/hash/default-hasher/map-many-string-keys.hd` compares `keys_of(values)` with itself.
  - `runtime/valid/fs-glob*.hd`: the pattern `"c[a-]*.md"` expects `[]`, so it can't
    tell whether the dash in a character class works.
  - `std/encoding/base64/encoding-decode-errors.hd`: the Display check only asserts
    `len > 0`, though `lib/std/encoding.hd` defines the exact text.
- **Depends on map order (B8)**: `runtime/valid/map-index-groups.hd` pins a literal
  digest of `Map`/`UniqMap` iteration order.
- Plausible B8: several `fnv1a`/hasher fixtures assume a `usize` hashes as 4 bytes,
  while `std-hash.bytes.integer` says "its own width" (target-dependent).
- Plausible B7: `union-hash` cites an unrelated hash rule. Plausible B1: the `list-view-run` and
  `list-span` titles promise "no copy", which nothing observes. Plausible C1:
  `encoding-properties` ~ `encoding-decode-errors`. Unverified B5: unused `Digest` and
  `ParseNumberError` imports.

#### b156 to b159 (`std/http`, `std/json`, `std/num`, `std/iter`)

Ledger: b156 (45): 4. b157 (45): 0. b158 (45): 6. b159 (44): 4.

- **Depends on map or set order (B8)**: `std/json/typed-json/json-typed-round-trip.hd` (`Map`
  output in insertion order), `json-model-round-trips.hd` and `json-adversarial-inputs.hd`
  (`Set` output in first-occurrence order; `std-collections.set.iter` doesn't promise it).
- **Depends on `usize` width (B8, the target owner question)**: `runtime/valid/num-intrinsic-boundaries.hd`,
  `num-bit-count-widths.hd`, `num-bit-counts.hd`, `num-checked-wrapping.hd` all assume a
  32-bit `usize`. Together with b032–b037, b147 and b155, the target-width class is now
  about 15 fixtures. A `# fixture-target:` directive or a rule that hd's `usize`
  is 32-bit is needed.
- Plausible B3: `union-serde.hd` (round-trip only). Plausible B7: `initialization-implicit-a-for-loop-reaches-a-binding-its-iter-reads-before-it-is-set/-02.hd`
  (cite `std-iter.from-fn`; the behavior is `module.init.definite`).
  `std/http/read-once-bodies/http-body-serialize-removed.hd`. Unverified B5: three `http-form-*`
  import `Serialize` without using it.

#### b168 to b170 (`std/task`, `std/time`, `std/validate`)

Ledger: b168 (36): 2. b169 (45): 5. b170 (18): 2.

- **Checks that cannot fail (B3)**: `runtime/valid/time-serde-forms.hd` (the only
  `Timestamp` written has zero milliseconds, so a writer that drops milliseconds
  still passes). `runtime/valid/a-point-cancelled-after-an-earlier-point-releases-none-of-its-temporaries.hd`
  (output `11` either way).
- **Title doesn't match the code (B1)**: `typing/valid/validation-phantom-derive-no-bound.hd`
  (names std's `Length` fact; tests a local `Phantom`). `runtime/valid/a-moved-cell-releases-storage-after-transferring-its-payload.hd`
  (no `# test:` title; the `same()` results are discarded).
- The tier-forced citation (b148): 36 `pointer-loan-*` cite `std/time.md#r-std-time.helper.sleep`.
  `runtime/valid/provider-keys-a-suspension-binds-both-providers-at-construction.hd` cites the same anchor
  for a requirement-row rule.
- The `pointer-loan-bang-{ref,mut}-local-write.hd` title (the b124 kind).
- Plausible B2: `runtime/panic/pointer-loan-extra-other-task-field-write-panics.hd` (writes
  `alias.other` while holding `&p.value`, which looks disjoint; its accept sibling runs
  disjoint writes; check the panic claim).

#### b160 to b163 (`std/num`, `std/random`, `std/serde`, `std/task`)

Ledger: b160 (44): 6. b161 (45): 0. b162 (45): 1. b163 (45): 5.

- **Depends on `usize` width (B8)**: `runtime/valid/number-text-properties.hd`,
  `num-parse-byte-reader-matches-scalar-reader.hd`, `num-parse-unsigned.hd`,
  `num-wrapping-mul.hd`. The target class is now about 20 fixtures.
- **Depends on map order (B8)**: `runtime/valid/serde-writer-first-error.hd`, `serde-std-reads.hd`.
- Plausible B2: `typing/invalid/serde-map-key-payload-derive.hd`, `serde-map-key-struct-member-derive.hd`.
- Plausible: `…-derived-default-enums-mark-one-variant-182.hd` (no derive or `@default`;
  also A14 paired with `-181`). `…-363.hd` ~ `derived-default-declared-no-bound` (C1).
  `rng-int-generic` ~ `rng-int-range` (C1). `serde-tagging-lookalike-not-static` (C1).
- Reviewer errors noted: it reported b162 as 92 fixtures (it has 45), and claimed
  `types.map.order.unspecified` was renamed (it exists, chapter 04 line 2233).

#### b164 to b167 (`std/serde`, `std/server-sdk`, `std/task`, `std/testing`, `std/text`)

Ledger: b164 (45): 6. b165 (44): 2. b166 (45): 5. b167 (44): 1.

- **Checks that cannot fail (B3), confirmed**:
  - `runtime/valid/property-generators-scalars.hd:58` asserts `x == x || x != x`,
    which holds for every `f64`, NaN included. A17 missed it because it
    isn't a literal tautology.
  - `runtime/valid/channel-full-blocks-sender.hd`: the order loop checks
    `got(i-1)` against `sent(i+1)`. At `i = 3`, `"sent 4"` never exists
    (`position` returns 1000), so the last comparison always passes and only
    two of three orderings are really checked.
  - Plausible: `runtime/valid/derived-arbitrary-with.hd` (asserts `price >= 0` with an
    example price of 0).
- **Code under test never runs (B4)**: `runtime/valid/a-module-function-takes-type-arguments-through-its-module.hd`
  (`echo`, `make`, `say`, `wrap` and `convert!` are never called; it also cites only
  `std/task.md`, with no rule).
- Plausible C1: `runtime/valid/with-timeout-cleanup-zero.hd` ~ `timeout-cleanup-zero.hd`.
  `task-all-list-empty.hd` ~ a test in `task-map-all.hd`. `string-from-bytes-checks-utf8.hd`
  ~ `utf8-valid-text.hd` and `utf8-truncated.hd`. `property-generators-scalars.hd`'s
  assume test ~ `property-assume-discards.hd`.
- Plausible B7: `typing/invalid/trait-values-a-trait-value-does-not-meet-a-bound-its-trait-does-not-extend.hd`
  and `runtime/valid/a-bound-call-at-a-trait-value-dispatches-through-its-vtable.hd` cite
  `std/text.md#more-string-methods` for trait-value rules (probably the tier rule
  again, as in b148). `runtime/valid/server-sdk-release-order.hd`.
- Plausible B1: `runtime/valid/serde-reference-round-trip.hd` (`&Node?` against `*Cell?`).
  `runtime/valid/property-assume-discards.hd` (nothing counts the cases).

#### Targeted pass: formatter damage in chapters 01 and 02 (A7 + A7b, 136 fixtures)

One reviewer read each fixture's diff in `edea876ae` (`full/fmt-diffs.txt`, brief
`full/fmt-brief.md`, output `full/fmt-out.tsv`). **21 ERASED, 23 WEAKENED, 92 harmless.**
ERASED means the form the fixture tests is gone, and restoring the pre-format
version fixes it:

- `lexical/physical-and-logical-lines/header-resumes-after-nested-suite.hd`: Closure body in the if header is now same-line, so no nested suite inside the header remains.
- `lexical/physical-and-logical-lines/nested-suite-body-deeper-than-statement.hd`: Nested suite inside run( joined to one line; the deeper-body form is gone.
- `lexical/physical-and-logical-lines/next-line-else-after-same-line-if.hd`: Else on its own line after a same-line if joined onto the if line.
- `lexical/physical-and-logical-lines/operator-and-member-lines-continue-in-brackets.hd`: Bracketed expression collapsed to one line; no line starts with a dot or operator.
- `grammar/statements/same-line-for-list.hd`: Same-line for suites and same-line bodies became indented; no same-line for remains.
- `lexical/physical-and-logical-lines/same-line-loop-else.hd`: Same-line for and while suites with else became indented blocks; no same-line loop else remains.
- `grammar/statements/same-line-suite-let-list.hd`: Both same-line if suites holding a let list became indented; no same-line let-list suite remains.
- `parse/valid/use-group-trailing-comma.hd`: All three grouped-use trailing commas removed.
- `lexical/processing-model/utf8-bom.hd`: Leading BOM bytes stripped.
- `lexical/physical-and-logical-lines/contextual-variant-after-same-line-if.hd`: Same-line if body became indented, so the .Variant line no longer follows a same-line if.
- `lexical/physical-and-logical-lines/continuation-line-opens-with-a-bracket.hd`: Every list whose lines open with a bracket or bang collapsed to one line.
- `lexical/physical-and-logical-lines/crlf-line-endings.hd`: Current file has zero CR bytes; the CRLF line endings the title requires are gone.
- `lexical/physical-and-logical-lines/leading-dot-deeper-continues.hd`: Deeper leading .grow() and .size lines joined onto the step name; no leading-dot continuation remains.
- `lexical/physical-and-logical-lines/leading-dot-statement-indent-tail.hd`: The := chain with a leading .size line is now one line; no such chain remains.
- `grammar/statements/let-after-same-line-if-keeps-else.hd`: Same-line if ... let ... else forms became indented; no same-line if with let remains.
- `runtime/valid/module-qualified-function-value-parenthesized.hd`: Parenthesized (time.s) became time.s; the parenthesized function value is gone.
- `lexical/whitespace-and-indentation/no-final-line-ending.hd`: A final line ending was added; the file no longer ends without one.
- `lexical/operators-and-delimiters/operators-longest-match.hd`: Glued operators such as f!=g, shifted<<=1, a-=b and m*=2 spaced out throughout.
- `grammar/statements/same-line-suite-body-forms.hd`: Every same-line suite became indented, including the else-if continuation, loop, let and parenthesized-if bodies.
- `grammar/statements/grammar-assignment-nonplace.hd`: Parens removed from (1 + 2) = 3; a binary left side is a syntax error, not invalid-assignment-target.
- `grammar/statements/same-line-let-list-unused.hd`: The only same-line suite binding was indented, so the same-line case the title names is gone.

WEAKENED (some of the title's forms are gone):

- `lexical/physical-and-logical-lines/bracketed-suite-header-line.hd`: Header on the bracket line erased for the if and closure cases; the for header moved to its own line.
- `lexical/physical-and-logical-lines/closing-delimiter-ends-nested-suite.hd`: Multi-line if suite closed by a paren on its last line joined; the match case remains.
- `lexical/physical-and-logical-lines/closure-body-ends-at-comma-line.hd`: Closure bodies ending at a comma line or a closing paren line joined; only a formatter-added comma line remains.
- `lexical/operators-and-delimiters/compound-assignment-tokens.hd`: Glued x-=1 spaced to x -= 1, so the glued longest-match case is gone.
- `grammar/expressions/embedded-copy-syntax.hd`: Copy marker in a same-line suite became a multi-line suite; the field-label and assignment forms remain.
- `parse/valid/functions-and-closures.hd`: Multi-line parameter list joined; parenthesized multi-line closure bodies became same-line; single-line closures remain.
- `grammar/inline-conditions-and-headers.hd`: The negative-literal line after else joined to one line; the match arm and headers remain.
- `lexical/whitespace-and-indentation/layout.hd`: Same-line fn bodies one_line and test became indented; same-line if remains.
- `lexical/physical-and-logical-lines/leading-dot-continuation.hd`: Leading .len and .slice lines under a head or match arm joined; one deeper .slice line remains.
- `grammar/statements/let-else-same-line.hd`: Same-line let-else body became an indented body; the indented form remains.
- `lexical/physical-and-logical-lines/nested-layout.hd`: Line break after the opening paren before the closure joined; the nesting remains.
- `grammar/use-declarations/use-declaration-forms.hd`: Trailing comma in the grouped use with an alias removed; the title names that form.
- `grammar/expressions/bang-call-and-prefix-not.hd`: Glued left!=right spaced to left != right; the != one-token case is gone, bang calls remain.
- `grammar/statements/header-and-bracket-expression-positions.hd`: Multi-line if inside a list ending in an indented suite joined; the match inside parens remains.
- `lexical/physical-and-logical-lines/implicit-continuation-in-delimiters.hd`: Call, nested-list, map and index-bracket continuations joined; the struct literal and roles list remain.
- `lexical/whitespace-and-indentation/indentation-levels.hd`: Comment lines at off-block indentation re-indented; nested if and else flattened to one line.
- `lexical/physical-and-logical-lines/leading-dot-chain.hd`: Most leading-dot method-chain lines joined; one .double() after a comment line remains.
- `lexical/physical-and-logical-lines/leading-dot-lines-join-the-chain.hd`: Most leading-dot and match-arm continuations joined; .double() and .read() after blank and comment lines remain.
- `lexical/physical-and-logical-lines/leading-pipe-lines-join-the-chain.hd`: Pipe lines after a raw head or a leading .len joined; pipes after comment and blank lines remain.
- `lexical/operators-and-delimiters/operators-without-spaces.hd`: Glued operators in statements such as x:=-b, y=-b and z%=3 spaced; glued ones in interpolations remain.
- `lexical/physical-and-logical-lines/same-line-suite-boundaries.hd`: Same-line for and while suites, including one before else, became indented blocks.
- `grammar/statements/suite-statement-right-sides.hd`: Right-side if and closure suites joined to one line; right-side match suites remain.
- `grammar/generic-parameters-and-bounds/trailing-commas-everywhere.hd`: Most trailing commas removed; those in an interpolated call and a closure header remain.

Notes on the pass:
- **`lexical/physical-and-logical-lines/crlf-line-endings.hd` was not erased by the formatter.** Its first
  version (`8f8f62c46`) had 18 CR bytes. They were lost in `d5a28f88e` ("Bridge
  HTTP messages through internal wire providers", 2026-10-26), an unrelated
  commit, and the fixture has had no CRLF since. It needs a `.gitattributes`
  entry (`-text`) so git can't normalize it again, and so does `utf8-bom.hd`.
- `grammar/statements/grammar-assignment-nonplace.hd`: with its parentheses removed,
  `1 + 2 = 3` becomes a parse error under `grammar.assignment_target`, not the
  expected `invalid-assignment-target`. The case may now pass for the wrong
  reason, or fail.
- The formatter writes `$ ()` for the empty row, but the spec text writes `$()`
  (`grammar.type.row.empty`). The tokens are the same, but the spec and the
  formatter disagree on spelling.
