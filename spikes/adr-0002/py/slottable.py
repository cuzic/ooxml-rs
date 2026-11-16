"""Count content models that slot-based insertion can handle.
Usage: python3 -I slottable.py <Open-XML-SDK>/data/schemas
Heuristic: a particle is 'slottable' if it flattens into an ordered list of
bands (sets of element QNames), where a Choice (any occurrence) is one band,
and no multi-band Sequence/Group repeats; every QName appears in one band only."""
import json, os, sys, collections
d = sys.argv[1]
def maxocc(x):
    o = x.get('Occurs')
    if not o: return 1
    m = [e.get('Max') for e in o]
    return None if any(v is None for v in m) else max(m)
class NS(Exception): pass
def bands(x, top=True):
    k = x.get('Kind', 'Element')
    if k == 'Element': return [{x['Name'].split('/')[-1]}]
    if k == 'Any': return [{'*ANY*'}]
    items = x.get('Items', [])
    if k == 'All':
        s = set()
        for i in items: s |= set().union(*bands(i, False))
        return [s]
    if k in ('Sequence', 'Group'):
        out = []
        for i in items: out += bands(i, False)
        r = maxocc(x)
        if not top and (r is None or r > 1) and len(out) > 1: raise NS()
        return out
    if k == 'Choice':
        s = set()
        for i in items:
            b = bands(i, False)
            if len(b) > 1: raise NS()
            s |= b[0]
        return [s]
    raise NS()
tot = flat = slot = 0
for f in sorted(os.listdir(d)):
    for t in json.load(open(os.path.join(d, f))).get('Types', []):
        P = t.get('Particle')
        if not P: continue
        tot += 1
        if P.get('Kind') == 'Sequence' and all(i.get('Kind', 'Element') == 'Element' for i in P.get('Items', [])):
            flat += 1; continue
        try:
            names = [n for s in bands(P) for n in s if n != '*ANY*']
            if len(names) != len(set(names)): raise NS()
            slot += 1
        except NS: pass
print(f'total {tot}  flat {flat}  slottable-non-flat {slot}  not-slottable {tot-flat-slot}')
