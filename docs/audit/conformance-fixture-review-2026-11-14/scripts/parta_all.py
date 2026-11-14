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
