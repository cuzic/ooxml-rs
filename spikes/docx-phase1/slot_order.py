"""ADR-0007 rev 2 §4 (review M2): parents that Phase1 inserts into, whose existing children are
not in the generated slot order or include mc:AlternateContent. Core's insert refuses such a
parent (UnresolvedSibling); Phase1 must find this before its first write.

Usage: python3 -I slot_order.py <SDK schema json> <dir or file>...

Rank = position of the child's SDK Name in the parent's particle tree (the parents checked
here lower to single-QName slots only: w:rPr 51, w:pPr 36, paragraph-mark w:rPr 57,
w:settings 103; a QName that occurs twice in a particle takes its first rank). Children that
are not in the particle (unlisted, e.g. vendor extensions) are skipped, as in ADR-0002 §4.3.
Files: those that pass the Phase0 gates (p1common.gate).
"""
import collections
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import p1common as C  # noqa: E402

w = C.w
PARENTS = {'w:CT_RPr/w:rPr': 'run rPr', 'w:CT_PPr/w:pPr': 'pPr', 'w:CT_ParaRPr/w:rPr': 'mark rPr',
           'w:CT_Settings/w:settings': 'settings'}


def ranks(schema, name):
    t = {x['Name']: x for x in schema['Types']}[name]
    out = {}

    def walk(p):
        if isinstance(p, dict):
            n = p.get('Name', '')
            if '/' in n and n != name:
                q = n.split('/', 1)[1]
                out.setdefault(q, len(out))
            for v in p.values():
                walk(v)
        elif isinstance(p, list):
            for v in p:
                walk(v)
    walk(t.get('Particle'))
    return out


def qname(tag):
    nsmap = {C.W: 'w', 'http://schemas.microsoft.com/office/word/2010/wordml': 'w14',
             'http://schemas.microsoft.com/office/word/2012/wordml': 'w15', C.MC: 'mc'}
    ns = C.ns(tag)
    return '%s:%s' % (nsmap.get(ns, ns), C.local(tag))


def check(el, rk):
    """'ac' if an mc:AlternateContent child, 'order' if listed children are out of rank order,
    'dup' if a listed QName occurs twice, else None."""
    seen, last = set(), -1
    for k in el:
        if not isinstance(k.tag, str):
            continue
        q = qname(k.tag)
        if q == 'mc:AlternateContent':
            return 'ac'
        r = rk.get(q)
        if r is None:
            continue
        if q in seen:
            return 'dup'
        seen.add(q)
        if r < last:
            return 'order'
        last = r
    return None


def main(argv):
    schema = json.load(open(argv[0]))
    rk = {k: ranks(schema, k) for k in PARENTS}
    res = collections.Counter()
    files = collections.defaultdict(set)
    ex = collections.defaultdict(list)
    for f in C.corpus_files(argv[1:]):
        pkg = C.Pkg(f)
        if C.gate(pkg)[0]:
            continue
        targets = []
        for kind, part, root in pkg.stories():
            parent = {c: p for p in root.iter() for c in p}
            for el in root.iter(w('rPr')):
                pa = parent.get(el)
                if pa is not None and pa.tag == w('r'):
                    targets.append(('w:CT_RPr/w:rPr', part, el))
                elif pa is not None and pa.tag == w('pPr'):
                    targets.append(('w:CT_ParaRPr/w:rPr', part, el))
            for el in root.iter(w('pPr')):
                if parent.get(el) is not None and parent[el].tag == w('p'):
                    targets.append(('w:CT_PPr/w:pPr', part, el))
        for sp in pkg.parts('settings'):
            targets.append(('w:CT_Settings/w:settings', sp, pkg.xml(sp)))
        for name, part, el in targets:
            res[PARENTS[name], 'checked'] += 1
            v = check(el, rk[name])
            if v:
                res[PARENTS[name], v] += 1
                files[PARENTS[name], v].add(pkg.name)
                if len(ex[PARENTS[name], v]) < 4:
                    ex[PARENTS[name], v].append('%s %s %s' % (pkg.name, part, [qname(k.tag) for k in el][:8]))
    print(json.dumps({'counts': {'%s/%s' % k: v for k, v in sorted(res.items())},
                      'files': {'%s/%s' % k: len(v) for k, v in sorted(files.items())},
                      'examples': {'%s/%s' % k: v for k, v in ex.items()}}, indent=1, ensure_ascii=False))


if __name__ == '__main__':
    main(sys.argv[1:])
