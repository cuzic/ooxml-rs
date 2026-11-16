"""Differential check of resolved placeholder geometry against python-pptx
(ADR-0008 §7 oracle O1 preview; run with `python3 -I`).

Usage: python3 -I ppy_geometry.py <site-dir with python-pptx 1.0.2> <geometry.json from chain.py> <pptx files/dirs...>

python-pptx resolves left/top/width/height of a slide placeholder through
the layout placeholder with the same idx (top-level shapes only), and a
layout placeholder through the master placeholder of the mapped type
(`shapes/placeholder.py`, `_InheritsDimensions`). Shapes are keyed by
(file, sldId, cNvPr id) because python-pptx renames slide parts.
Reports agreement, and every disagreement with both values.
"""
import sys, os, json, collections
site, geo_path = sys.argv[1], sys.argv[2]
sys.path.insert(0, site)
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import iter_files
from pptx import Presentation

ours = {}
for g in json.load(open(geo_path)):
    ours[(g['file'], g['sldId'], g['id'])] = g
C = collections.Counter(); diffs = []
for path in iter_files(sys.argv[3:]):
    f = os.path.basename(path)
    try:
        prs = Presentation(path)
        slides = list(prs.slides)
    except Exception as e:
        C['python-pptx cannot open'] += 1; continue
    for s in slides:
        for sh in s.shapes:
            if not sh.is_placeholder:
                continue
            key = (f, str(s.slide_id), str(sh.shape_id))
            o = ours.get(key)
            if o is None:
                C['not in ours (python-pptx-only placeholder)'] += 1; continue
            try:
                theirs = (sh.left, sh.top, sh.width, sh.height)
            except Exception as e:
                C['python-pptx raised: %s' % type(e).__name__] += 1; continue
            mine = (o.get('x'), o.get('y'), o.get('cx'), o.get('cy'))
            C['compared'] += 1
            if theirs == mine:
                C['agree (source %s)' % o['source']] += 1
            elif all(v is None for v in theirs) and o['source'] is not None:
                C['python-pptx None, ours resolved (source %s)' % o['source']] += 1; diffs.append((key, mine, theirs))
            else:
                C['DIFFER (source %s)' % o['source']] += 1; diffs.append((key, mine, theirs))
for k in sorted(C):
    print('%-50s %d' % (k, C[k]))
for d in diffs[:80]:
    print('  ', d)
