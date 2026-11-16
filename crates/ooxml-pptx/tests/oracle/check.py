"""ADR-0005 T3, independent half: for every case written by
`cargo run -p ooxml-pptx --example oracle_cases -- OUTDIR`,

  (a) python-pptx(output) == python-pptx(edit(input))  -- expected model
  (b) python-pptx(output) == ooxml-pptx(output)        -- read agreement
  (c) python-pptx(input)  == ooxml-pptx(input)         -- read agreement

`edit` (review R2-3) does not mirror the Rust oracle's model arithmetic:
it opens the *input* with python-pptx and performs each edit on
python-pptx's own lxml tree, following the edit rules of ADR-0005 §3
(locate the body by shape id / cell, then move, copy and remove
elements), and the expected model is read back from that tree with the
same reader as (a). Exit status 1 on any mismatch.

    python3 -I check.py [--site DIR] OUTDIR FIXTURES_DIR
"""
import copy
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import readback  # noqa: E402

A = '{http://schemas.openxmlformats.org/drawingml/2006/main}'


# ---------------------------------------------------------------------------
# Locating the edited text body in python-pptx's tree

def find_body(prs, site, e):
    _, qn, GroupShape, _ = readback.import_pptx(site)
    _, _, part = readback.slide_parts(prs)[e['slide']]

    def walk(shapes):
        for sh in shapes:
            if isinstance(sh, GroupShape):
                yield from walk(sh.shapes)
            yield sh

    hits = [sh for sh in walk(part.slide.shapes) if sh.shape_id == e['shape_id']]
    assert len(hits) == 1, f'shape id {e["shape_id"]}: {len(hits)} shapes'
    sh = hits[0]
    if e['cell'] is None:
        body = sh._element.find(qn('p:txBody'))
    else:
        r, c = e['cell']
        body = sh.table.rows[r].cells[c]._tc.find(qn('a:txBody'))
    assert body is not None
    return body


# ---------------------------------------------------------------------------
# The edit rules (ADR-0005 §3) on lxml elements

def kids(el, local):
    return [c for c in el if c.tag == A + local]


def make(parent, local):
    return parent.makeelement(A + local, {})


def put(p, el):
    """Paragraph content goes before a:endParaRPr (else at the end)."""
    end = p.find(A + 'endParaRPr')
    if end is not None:
        end.addprevious(el)
    else:
        p.append(el)


def run_source(p):
    """Format source of a new run: the first run's a:rPr, else a:endParaRPr."""
    runs = kids(p, 'r')
    return runs[0].find(A + 'rPr') if runs else p.find(A + 'endParaRPr')


LINKS = (A + 'hlinkClick', A + 'hlinkMouseOver')


def copy_as(src, local):
    """A copy of a run format source under another name, without its link
    children (Phase 0.1: a link stays on the run that had it); None when
    nothing is left (no attributes, no child elements)."""
    if src is None:
        return None
    c = copy.deepcopy(src)
    for link in [k for k in c if k.tag in LINKS]:
        c.remove(link)
    if not c.attrib and not any(isinstance(k.tag, str) for k in c):
        return None
    c.tag = A + local
    c.tail = None
    return c


def new_run(p, text, src):
    r = make(p, 'r')
    fmt = copy_as(src, 'rPr')
    if fmt is not None:
        r.append(fmt)
    t = make(r, 't')
    t.text = text
    r.append(t)
    return r


def fill(p, segments, first, src):
    if first is not None:
        t = first.find(A + 't')
        if t is None:
            t = make(first, 't')
            first.append(t)
        t.text = segments[0]
    elif segments[0]:
        put(p, new_run(p, segments[0], src))
    for s in segments[1:]:
        put(p, make(p, 'br'))
        if s:
            put(p, new_run(p, s, src))


def set_paragraph(p, text):
    src = copy.deepcopy(run_source(p))
    runs = kids(p, 'r')
    for el in runs[1:] + kids(p, 'br'):
        p.remove(el)
    fill(p, text.split('\v'), runs[0] if runs else None, src)


def new_paragraph(body, text, ppr, src, end_src):
    p = make(body, 'p')
    kids(body, 'p')[-1].addnext(p)
    if ppr is not None:
        p.append(copy.deepcopy(ppr))
    fill(p, text.split('\v'), None, src)
    end = copy_as(end_src, 'endParaRPr')
    if end is not None:
        p.append(end)


def edit(body, op, a):
    paras = kids(body, 'p')
    if op == 'run_set_text':
        kids(paras[a['p']], 'r')[a['r']].find(A + 't').text = a['text']
    elif op == 'para_set_text':
        set_paragraph(paras[a['p']], a['text'])
    elif op == 'add_run':
        put(paras[a['p']], new_run(paras[a['p']], a['text'], None))
    elif op == 'add_break':
        put(paras[a['p']], make(paras[a['p']], 'br'))
    elif op == 'remove_run':
        paras[a['p']].remove(kids(paras[a['p']], 'r')[a['r']])
    elif op == 'remove_field':
        paras[a['p']].remove(kids(paras[a['p']], 'fld')[a['f']])
    elif op == 'add_paragraph':
        new_paragraph(body, a['text'], None, None, None)
    elif op == 'remove_paragraph':
        body.remove(paras[a['p']])
    elif op == 'body_set_text':
        lines = a['text'].replace('\r\n', '\n').replace('\r', '\n').split('\n')
        last = paras[-1]
        ppr, src = copy.deepcopy(last.find(A + 'pPr')), copy.deepcopy(run_source(last))
        # the end-of-paragraph format carries over (N3-1): the last
        # paragraph's own a:endParaRPr when it has a format to copy
        own_end = last.find(A + 'endParaRPr')
        end_src = copy.deepcopy(own_end) if copy_as(own_end, 'endParaRPr') is not None else src
        for p, line in zip(paras, lines):
            set_paragraph(p, line)
        for line in lines[len(paras):]:
            new_paragraph(body, line, ppr, src, end_src)
        for p in paras[len(lines):]:
            body.remove(p)
    else:
        raise ValueError(op)


def expected(path, edits, site):
    Presentation = readback.import_pptx(site)[0]
    prs = Presentation(path)
    for e in edits:
        edit(find_body(prs, site, e), e['op'], e['args'])
    return readback.dump_prs(prs, site)


# ---------------------------------------------------------------------------

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
        fixture = os.path.join(fixtures, c['fixture'])
        src = norm(readback.dump(fixture, site))
        out = norm(readback.dump(os.path.join(outdir, c['output']), site))
        want = norm(expected(fixture, c['edits'], site))
        rust_in = json.load(open(os.path.join(outdir, c['name'] + '.input.ooxml.json'), encoding='utf-8'))
        rust_out = json.load(open(os.path.join(outdir, c['name'] + '.ooxml.json'), encoding='utf-8'))
        edits += len(c['edits'])
        for label, x, y in (('expected model', out, want), ('agreement (output)', out, rust_out),
                            ('agreement (input)', src, rust_in)):
            d = first_diff(x, y)
            if d:
                failures += 1
                print(f'FAIL {c["name"]}: {label}: {d}')
    print(f'T3 python-pptx: {len(cases)} cases, {edits} edits, {failures} failures')
    sys.exit(1 if failures else 0)


if __name__ == '__main__':
    main()
