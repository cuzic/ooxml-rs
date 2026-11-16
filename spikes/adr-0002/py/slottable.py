"""Slot lowering per ADR-0002 §4.1 (revision 3) and an SDK-vs-ECMA order check.
Usage: python3 -I slottable.py <Open-XML-SDK>/data <ECMA Transitional XSD dir>
Heuristic reading of SDK `Occurs` ([] / missing Max = unbounded when Occurs is present)."""
import json, os, sys, collections, xml.etree.ElementTree as ET
sdk, xsd = sys.argv[1], sys.argv[2]
def maxocc(x):
    o = x.get('Occurs')
    if not o: return 1
    m = [e.get('Max') for e in o]
    return None if any(v is None for v in m) else max(m)
class NS(Exception): pass
def optional(x):
    """Can this particle match nothing? (missing Occurs = 1..1; Occurs without Min = min 0)"""
    o = x.get('Occurs')
    if o and all(e.get('Min', 0) == 0 for e in o): return True
    k = x.get('Kind', 'Element'); items = x.get('Items', [])
    if k == 'Choice': return any(optional(i) for i in items)
    if k in ('Sequence', 'Group', 'All'): return all(optional(i) for i in items)
    return False
def mul(a, b): return None if a is None or b is None else a * b
def lower(x, top=True, outer=False):
    """outer: an enclosing particle that collapses into the same slot repeats."""
    """-> list of slots; slot = dict qname->max_each (None=unbounded). Repetition multiplies."""
    k = x.get('Kind', 'Element')
    if k == 'Element': return [{x['Name'].split('/')[-1]: maxocc(x)}]
    if k == 'Any': return [{'*ANY*': maxocc(x)}]
    items = x.get('Items', [])
    if k == 'All':
        s = {}
        for i in items:
            for sl in lower(i, False, outer): s.update({q: 1 for q in sl})
        return [s]
    if k in ('Sequence', 'Group'):
        r = maxocc(x)
        out = []
        for i in items: out += lower(i, False, outer or r != 1)
        if len(out) > 1 and (outer or r != 1):
            # (a? b? ...)* == (a | b | ...)*: a repeated sequence of optional parts is a free slot
            if all(optional(i) for i in items):
                u = {}
                for sl in out: u.update({q: None for q in sl})
                return [u]
            raise NS('repeating multi-slot group')
        if r != 1:   # single-slot repeating group: lift cardinality by multiplication
            out = [{q: mul(m, r) for q, m in sl.items()} for sl in out]
        return out
    if k == 'Choice':
        s = {}; rep = maxocc(x)
        for i in items:
            b = lower(i, False, outer or rep != 1)
            if len(b) > 1: raise NS('multi-slot alternative')
            s.update(b[0])
        if rep == 1 and not outer and len(s) > 1 and any(v is None or v > 1 for v in s.values()):
            raise NS('exclusive choice of repeating alternatives')
        if rep != 1: s = {q: mul(m, rep) for q, m in s.items()}
        return [s]
    raise NS(k)
# ---- ECMA side: ordered slot ranks by local name ----
XS = '{http://www.w3.org/2001/XMLSchema}'
cts, groups = {}, {}
for f in sorted(os.listdir(xsd)):
    if not f.endswith('.xsd'): continue
    root = ET.parse(os.path.join(xsd, f)).getroot(); tns = root.get('targetNamespace')
    for g in root.findall(XS + 'group'): groups[(tns, g.get('name'))] = (g, tns)
    for c in root.findall(XS + 'complexType'): cts[(tns, c.get('name'))] = (c, tns)
def xlow(n, tns):
    t = n.tag
    if t == XS + 'element': return [{(n.get('name') or n.get('ref')).split(':')[-1]}]
    if t == XS + 'any': return [{'*ANY*'}]
    if t == XS + 'group':
        g = groups.get((tns, n.get('ref', '').split(':')[-1]))
        return [s for c in g[0] for s in xlow(c, g[1])] if g else []
    if t in (XS + 'sequence', XS + 'all'):
        out = [s for c in n for s in xlow(c, tns)]
        return [set().union(*out)] if t == XS + 'all' and out else out
    if t == XS + 'choice':
        s = set()
        for c in n:
            for sl in xlow(c, tns): s |= sl
        return [s]
    if t in (XS + 'complexContent', XS + 'extension', XS + 'restriction'):
        out = []
        if t == XS + 'extension' and n.get('base'):
            b = cts.get((tns, n.get('base').split(':')[-1]))
            if b: out += [s for c in b[0] for s in xlow(c, b[1])]
        return out + [s for c in n for s in xlow(c, tns)]
    return []
def ranks(slots):
    r = {}
    for i, s in enumerate(slots):
        for q in s: r.setdefault(q.split(':')[-1], i)
    return r
pfx = {e['Prefix']: e['Uri'] for e in json.load(open(os.path.join(sdk, 'namespaces.json')))}
tot = flat = slot = ambiguous = 0; why = collections.Counter(); mismatch = []
for f in sorted(os.listdir(os.path.join(sdk, 'schemas'))):
    for t in json.load(open(os.path.join(sdk, 'schemas', f))).get('Types', []):
        P = t.get('Particle')
        if not P: continue
        tot += 1
        try:
            sl = lower(P)
        except NS as e:
            why[str(e)] += 1; continue
        names = [q for s in sl for q in s if q != '*ANY*']
        if len(names) != len(set(names)): ambiguous += 1   # supported; those QNames need an explicit slot
        if all(len(s) == 1 for s in sl): flat += 1
        else: slot += 1
        typ = t['Name'].split('/')[0]; p, _, ctn = typ.partition(':')
        x = cts.get((pfx.get(p), ctn))
        if not x: continue
        xsl = [s for c in x[0] for s in xlow(c, x[1])]
        xr = ranks(xsl)
        sr = {q.split(':')[-1]: i for i, s in enumerate(sl) for q in s if q.split(':')[0] == p}
        # names that occur in more than one slot (either side) need an explicit slot; not compared
        dup = {q for q, c in collections.Counter(q.split(':')[-1] for s in sl for q in s).items() if c > 1}
        dup |= {q for q, c in collections.Counter(q.split(':')[-1] for s in xsl for q in s).items() if c > 1}
        common = [q for q in sr if q in xr and q != '*ANY*' and q not in dup]
        for a in common:
            for b in common:
                # unsafe only if SDK allows an order ECMA forbids: SDK same slot but ECMA ordered,
                # or SDK order inverted relative to ECMA
                if a != b and (sr[a] == sr[b] and xr[a] != xr[b] or (sr[a] - sr[b]) * (xr[a] - xr[b]) < 0):
                    mismatch.append((t['Name'], a, b)); break
            else: continue
            break
print(f'total {tot}  single-QName slots only {flat}  multi-QName slots {slot}  unsupported {tot-flat-slot} {dict(why)}  (of supported: {ambiguous} with a QName in two slots)')
print(f'SDK slots permit an order the ECMA XSD forbids: {len(mismatch)}')
for m in mismatch[:15]: print('  ', m)
