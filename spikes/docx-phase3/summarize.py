"""Merge review.py JSON outputs into one Markdown table per section.

Usage: python3 -I summarize.py name=out.json ... > tables.md
Cells are "occurrences (files)"; single-number keys (largest id, deepest chain) show the value.
"""
import json
import sys

cols, data = [], {}
for a in sys.argv[1:]:
    name, path = a.split('=', 1)
    cols.append(name)
    with open(path) as fh:
        data[name] = json.load(fh)['report']
sections = []
for d in data.values():
    for s in d:
        if s not in sections:
            sections.append(s)
for s in sections:
    keys = sorted({k for d in data.values() for k in d.get(s, {})})
    print('\n### ' + s + '\n')
    print('| ' + ' | '.join(['measure'] + cols) + ' |')
    print('|' + '---|' * (len(cols) + 1))
    for k in keys:
        cells = []
        for c in cols:
            v = data[c].get(s, {}).get(k)
            if v is None:
                cells.append('0')
            elif s == 'files' or k.startswith(('largest', 'deepest', 'max ')):
                cells.append(str(v[0]))
            else:
                cells.append('%d (%d)' % (v[0], v[1]))
        print('| ' + ' | '.join([k.replace('|', '/')] + cells) + ' |')
