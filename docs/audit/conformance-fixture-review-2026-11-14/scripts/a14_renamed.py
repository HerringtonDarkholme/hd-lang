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
