"""ADR-0005 T3, independent half: for every case written by
`cargo run -p ooxml-pptx --example oracle_cases -- OUTDIR`,

  (a) python-pptx(output) == apply(edits, python-pptx(input))  -- expected model
  (b) python-pptx(output) == ooxml-pptx(output)                -- read agreement
  (c) python-pptx(input)  == ooxml-pptx(input)                 -- read agreement

`apply` below is written from the ADR's edit semantics, independently of
the Rust code. Exit status 1 on any mismatch.

    python3 -I check.py [--site DIR] OUTDIR FIXTURES_DIR
"""
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import readback  # noqa: E402


def segs(text):
    return text.split('\v')


def build(segments, first, fmt):
    items = []
    if first is not None:
        items.append(['r', segments[0], first[2]])
    elif segments[0]:
        items.append(['r', segments[0], fmt])
    for s in segments[1:]:
        items.append(['br'])
        if s:
            items.append(['r', s, fmt])
    return items


def runs(p):
    return [i for i in p['items'] if i[0] == 'r']


def run_fmt(p):
    r = runs(p)
    return r[0][2] if r else p['end']


def para_set(p, text):
    r = runs(p)
    p['items'] = build(segs(text), r[0] if r else None, run_fmt(p))


def nth(items, kind, n):
    return [k for k, i in enumerate(items) if i[0] == kind][n]


def apply(model, e):
    slide = model['slides'][e['slide']]
    target = next(s for s in slide['shapes'] if s['shape_id'] == e['shape_id'] and s['cell'] == e['cell'])
    paras = target['paragraphs']
    a, op = e['args'], e['op']
    if op == 'run_set_text':
        items = paras[a['p']]['items']
        items[nth(items, 'r', a['r'])][1] = a['text']
    elif op == 'para_set_text':
        para_set(paras[a['p']], a['text'])
    elif op == 'add_run':
        paras[a['p']]['items'].append(['r', a['text'], None])
    elif op == 'add_break':
        paras[a['p']]['items'].append(['br'])
    elif op in ('remove_run', 'remove_field'):
        kind, n = ('r', a['r']) if op == 'remove_run' else ('fld', a['f'])
        items = paras[a['p']]['items']
        del items[nth(items, kind, n)]
    elif op == 'add_paragraph':
        paras.append({'ppr': None, 'end': None, 'items': build(segs(a['text']), None, None)})
    elif op == 'remove_paragraph':
        del paras[a['p']]
    elif op == 'body_set_text':
        lines = a['text'].replace('\r\n', '\n').replace('\r', '\n').split('\n')
        last = json.loads(json.dumps(paras[-1]))
        n = len(paras)
        for i, line in enumerate(lines):
            if i < n:
                para_set(paras[i], line)
            else:
                paras.append({'ppr': last['ppr'], 'end': None, 'items': build(segs(line), None, run_fmt(last))})
        del paras[len(lines):]
    else:
        raise ValueError(op)


def first_diff(a, b, path='$'):
    if type(a) is not type(b):
        return f'{path}: {a!r} != {b!r}'
    if isinstance(a, dict):
        for k in sorted(set(a) | set(b)):
            if k not in a or k not in b:
                return f'{path}.{k}: missing on one side'
            d = first_diff(a[k], b[k], f'{path}.{k}')
            if d:
                return d
        return None
    if isinstance(a, list):
        if len(a) != len(b):
            return f'{path}: length {len(a)} != {len(b)}'
        for i, (x, y) in enumerate(zip(a, b)):
            d = first_diff(x, y, f'{path}[{i}]')
            if d:
                return d
        return None
    return None if a == b else f'{path}: {a!r} != {b!r}'


def main():
    args = sys.argv[1:]
    site = None
    if args[:1] == ['--site']:
        site, args = args[1], args[2:]
    outdir, fixtures = args
    cases = json.load(open(os.path.join(outdir, 'cases.json'), encoding='utf-8'))
    norm = lambda m: json.loads(json.dumps(m))  # noqa: E731
    failures, edits = 0, 0
    for c in cases:
        src = norm(readback.dump(os.path.join(fixtures, c['fixture']), site))
        out = norm(readback.dump(os.path.join(outdir, c['output']), site))
        rust_in = json.load(open(os.path.join(outdir, c['name'] + '.input.ooxml.json'), encoding='utf-8'))
        rust_out = json.load(open(os.path.join(outdir, c['name'] + '.ooxml.json'), encoding='utf-8'))
        expected = norm(src)
        for e in c['edits']:
            apply(expected, e)
        edits += len(c['edits'])
        for label, x, y in (('expected model', out, expected), ('agreement (output)', out, rust_out),
                            ('agreement (input)', src, rust_in)):
            d = first_diff(x, y)
            if d:
                failures += 1
                print(f'FAIL {c["name"]}: {label}: {d}')
    print(f'T3 python-pptx: {len(cases)} cases, {edits} edits, {failures} failures')
    sys.exit(1 if failures else 0)


if __name__ == '__main__':
    main()
