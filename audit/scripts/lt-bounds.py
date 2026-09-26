"""Rewrite bound syntax `T: Bound` to `T < Bound` (decision S1).

Covers generic parameter lists after fn/data/enum/trait/type/impl, where
clauses, and inline-code prose mentions such as `T: PartialEq`.
"""
import re, sys, pathlib

HEAD = re.compile(r'(\bfn [A-Za-z_][\w]*!?|\bdata [A-Z]\w*|\benum [A-Z]\w*|\btrait [A-Z]\w*|\btype [A-Z]\w*|\bimpl)\[')
PARAM = re.compile(r'^(\s*(?:reified\s+)?[+-]?[A-Z][A-Za-z0-9_]*(?:\.\.\.)?): ')
WHERE = re.compile(r'\bwhere\b')
BOUND = re.compile(r'([+-]?[A-Z][A-Za-z0-9_]*(?:\[[^\]]*\])?(?:\.\.\.)?): (?=(?:mut )?[A-Z])')
INLINE = re.compile(r'`((?:reified )?[+-]?[A-Z][A-Za-z0-9_]*(?:\.\.\.)?): ((?:mut )?[A-Z][^`]*)`')

def fix_params(line):
    out, i = [], 0
    for m in HEAD.finditer(line):
        start = m.end()
        if start < i:
            continue
        depth, j = 1, start
        while j < len(line) and depth:
            depth += {'[': 1, ']': -1}.get(line[j], 0)
            j += 1
        inner = line[start:j - 1]
        parts, d, buf = [], 0, ''
        for ch in inner:
            if ch == ',' and d == 0:
                parts.append(buf); buf = ''
                continue
            d += {'[': 1, ']': -1}.get(ch, 0)
            buf += ch
        parts.append(buf)
        new = ','.join(PARAM.sub(r'\1 < ', p) for p in parts)
        out.append(line[i:start] + new)
        i = j - 1
    out.append(line[i:])
    return ''.join(out)

def fix_where(line):
    m = WHERE.search(line)
    if not m:
        return line
    head, tail = line[:m.end()], line[m.end():]
    return head + BOUND.sub(r'\1 < ', tail)

def fix(text, prose):
    lines = []
    for line in text.split('\n'):
        line = fix_where(fix_params(line))
        if prose:
            line = INLINE.sub(r'`\1 < \2`', line)
        lines.append(line)
    return '\n'.join(lines)

changed = 0
for name in sys.argv[1:]:
    path = pathlib.Path(name)
    old = path.read_text()
    new = fix(old, path.suffix == '.md')
    if new != old:
        path.write_text(new)
        changed += 1
print(f'rewrote {changed} files')
