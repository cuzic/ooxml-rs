"""ADR-0005 T3, independent half: for every case written by
`cargo run -p ooxml-pptx --example oracle_cases -- OUTDIR`,

  (a) python-pptx(output) == python-pptx(edit(input))  -- expected model
  (b) python-pptx(output) == ooxml-pptx(output)        -- read agreement
  (c) python-pptx(input)  == ooxml-pptx(input)         -- read agreement

Find/replace cases (ADR-0008 §7 O5, review #48 M1) carry deck-wide
`replace_all` calls: `edit` finds every match itself on python-pptx's
paragraphs, checks that ooxml-pptx's accepted matches are exactly its own
matches minus the refused ones, decides the paragraph-local refusals
(rules 1, 5, 6) itself and compares them with ooxml-pptx's, kind included
(review #48 r2-m1; body-level locks are taken from ooxml-pptx), and applies
the ADR-0008 §4 rules 2–5 to the lxml tree independently; then (a)–(c).

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
    if e.get('notes'):
        ph = part.slide.notes_slide.notes_placeholder
        assert ph is not None, 'notes body'
        return ph._element.find(qn('p:txBody'))

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


# ---------------------------------------------------------------------------
# Find/replace (ADR-0008 §4) on lxml elements

CONTENT = (A + 'r', A + 'br', A + 'fld')


def item_text(el):
    if el.tag == A + 'br':
        return '\v'
    t = el.find(A + 't')
    return (t.text or '') if t is not None else ''


def set_run_text(r, text):
    t = r.find(A + 't')
    if t is None:
        t = make(r, 't')
        r.append(t)
    t.text = text


def replace_one(p, s, e, segments, touched):
    """Rules 2–5 for one accepted match [s, e) (character offsets in the
    paragraph's current text)."""
    items = [c for c in p if c.tag in CONTENT]
    spans, off = [], 0
    for it in items:
        n = len(item_text(it))
        spans.append((off, off + n))
        off += n
    cov = [i for i, (a, b) in enumerate(spans) if a < e and b > s]
    first, last = cov[0], cov[-1]
    start = items[first]
    assert start.tag == A + 'r', 'an accepted match starts in a run'
    tail = item_text(items[last])[e - spans[last][0]:]
    suffix = tail if first == last else ''
    prefix = item_text(start)[:s - spans[first][0]]
    if len(segments) == 1:
        set_run_text(start, prefix + segments[0] + suffix)
    else:
        set_run_text(start, prefix + segments[0])
    touched.add(start)
    if first != last:
        for i in cov[1:-1]:
            p.remove(items[i])
        if tail == '':
            p.remove(items[last])
        else:
            set_run_text(items[last], tail)
            touched.add(items[last])
    anchor = start
    for k, seg in enumerate(segments[1:], 1):
        br = make(p, 'br')
        anchor.addnext(br)
        anchor = br
        text = seg + (suffix if k == len(segments) - 1 else '')
        if text:
            r = new_run(p, text, start.find(A + 'rPr'))
            anchor.addnext(r)
            anchor = r


def finish_paragraph(p, touched):
    """Rule 4: touched runs left empty are removed unless no run remains."""
    runs = [c for c in p if c.tag == A + 'r']
    empty = [r for r in runs if r in touched and item_text(r) == '']
    if len(empty) < len(runs):
        drop = empty
    else:
        drop = empty[1:]
    for r in drop:
        p.remove(r)


def paragraphs_of(prs, site, notes=False):
    """(slide index, in notes, shape id, cell, paragraph index, a:p) for every
    paragraph python-pptx sees, in the order of readback.dump_prs."""
    _, qn, GroupShape, _ = readback.import_pptx(site)
    out = []

    def bodies(shapes):
        for sh in shapes:
            if isinstance(sh, GroupShape):
                yield from bodies(sh.shapes)
            elif getattr(sh, 'has_table', False) and sh.has_table:
                for ri, row in enumerate(sh.table.rows):
                    for ci, cell in enumerate(row.cells):
                        yield sh.shape_id, [ri, ci], cell._tc.find(qn('a:txBody'))
            elif sh._element.tag == qn('p:sp'):
                yield sh.shape_id, None, sh._element.find(qn('p:txBody'))

    for si, (_, _, part) in enumerate(readback.slide_parts(prs)):
        for sid, cell, body in bodies(part.slide.shapes):
            if body is None:
                continue
            for k, p in enumerate(kids(body, 'p')):
                out.append((si, False, sid, cell, k, p))
        # the notes body (ADR-0008 §2), searched with notes=True
        if notes and part.slide.has_notes_slide:
            ph = part.slide.notes_slide.notes_placeholder
            body = ph._element.find(qn('p:txBody')) if ph is not None else None
            if body is not None:
                for k, p in enumerate(kids(body, 'p')):
                    out.append((si, True, ph.shape_id, None, k, p))
    return out


PROOFING = {'lang', 'altLang', 'err', 'dirty', 'smtClean', 'smtId', 'noProof', 'bmk'}
# refusals decided outside the paragraph (ADR-0005 §6 locks): taken from
# ooxml-pptx, but each must be one of python's own matches
BODY_REFUSALS = {'AnimatedCharacters', 'CommentAnchoredText', 'InsideAlternateContent', 'MergedCell', 'UnsupportedContent'}


def canon(el, top=True):
    """Rule 6's format of an a:rPr: attributes sorted (top-level proofing
    attributes dropped), child elements in order, whitespace-only text and
    comments ignored; a missing a:rPr equals an empty one."""
    if el is None:
        return ''

    def name(k):
        return k if k.startswith('{') else '{}' + k
    attrs = sorted(f'{name(k)}={v!r}' for k, v in el.attrib.items() if not (top and not k.startswith('{') and k in PROOFING))
    body = ' '.join(attrs)
    texts = [el.text] + [c.tail for c in el]
    for c in el:
        if isinstance(c.tag, str):
            body += canon(c, False)
    body += ''.join(repr(t) for t in texts if t and t.strip())
    return body if top else f'<{name(el.tag)} {body}>'


def own_refusal(items, s, e, segments, refuse_mixed):
    """Rules 1, 5 and 6 (ADR-0008 §4) for a match [s, e) of a paragraph,
    decided from the lxml tree alone (review #48 r2-m1)."""
    spans, off = [], 0
    for it in items:
        n = len(item_text(it))
        spans.append((off, off + n))
        off += n
    cov = [i for i, (a, b) in enumerate(spans) if a < e and b > s]
    if any(items[i].tag == A + 'fld' for i in cov):
        return 'FieldInMatch'
    if any(items[i].tag == A + 'br' for i in cov):
        return 'BreakInMatch'
    rprs = [canon(items[i].find(A + 'rPr')) for i in cov]
    if refuse_mixed and any(a != b for a, b in zip(rprs, rprs[1:])):
        return 'MixedFormatInMatch'
    start_rpr = items[cov[0]].find(A + 'rPr')
    linked = start_rpr is not None and any(c.tag in LINKS for c in start_rpr)
    if len(segments) > 1 and linked and e < spans[cov[0]][1]:
        return 'LinkSplitByBreak'
    return None


def apply_replace(prs, r, site):
    """Finds every match itself, decides the paragraph-local refusals
    (rules 1, 5, 6) itself and compares them, kind included, with
    ooxml-pptx's; body-level locks are taken from ooxml-pptx. Then applies
    the accepted matches by rules 2–5 and returns the mismatches."""
    key = lambda d: (d['slide'], d.get('notes', False), d['shape_id'], tuple(d['cell']) if d['cell'] else None,  # noqa: E731
                     d['paragraph'], d['start'], d['end'])
    refused = {key(d): d['error'].split(' ')[0].split('{')[0].split('(')[0] for d in r['refused']}
    rust = {key(d) for d in r['replaced']}
    pattern, replacement = r['pattern'], r['replacement']
    segments = replacement.split('\v')
    ours, seen, problems = set(), set(), []
    for si, nt, sid, cell, k, p in paragraphs_of(prs, site, r.get('notes', False)):
        items = [c for c in p if c.tag in CONTENT]
        text = ''.join(item_text(c) for c in items)
        found, i = [], 0
        while True:
            j = text.find(pattern, i)
            if j < 0:
                break
            found.append((j, j + len(pattern)))
            i = j + len(pattern)
        b = lambda n: len(text[:n].encode('utf-8'))  # noqa: E731
        accepted = []
        for s, e in found:
            kk = (si, nt, sid, tuple(cell) if cell else None, k, b(s), b(e))
            seen.add(kk)
            theirs = refused.get(kk)
            if theirs in BODY_REFUSALS:
                continue
            mine = own_refusal(items, s, e, segments, r['refuse_mixed'])
            if mine != theirs:
                problems.append(f'refusal differs at {kk}: python {mine}, ooxml {theirs}')
            if mine is None:
                accepted.append((s, e))
                ours.add(kk)
        touched = set()
        for s, e in reversed(accepted):
            replace_one(p, s, e, segments, touched)
        if accepted:
            finish_paragraph(p, touched)
        if found and len(accepted) == len(found):
            # ADR-0008 §7 O5 as worded: the paragraph text is str.replace's
            after = ''.join(item_text(c) for c in p if c.tag in CONTENT)
            if after != text.replace(pattern, replacement):
                problems.append(f'paragraph text at {(si, nt, sid, cell, k)} is not str.replace: {after!r}')
    if ours != rust:
        problems.append(f'accepted matches differ: only python {sorted(ours - rust)[:3]}, only ooxml {sorted(rust - ours)[:3]}')
    if set(refused) - seen:
        problems.append(f'refused matches python does not find: {sorted(set(refused) - seen)[:3]}')
    return problems


def expected(path, edits, site, replaces=(), problems=None):
    Presentation = readback.import_pptx(site)[0]
    prs = Presentation(path)
    for e in edits:
        edit(find_body(prs, site, e), e['op'], e['args'])
    for r in replaces:
        found = apply_replace(prs, r, site)
        if problems is not None:
            problems.extend(found)
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


def check_shape_properties(outdir):
    from pptx import Presentation
    from pptx.dml.color import RGBColor
    from pptx.enum.dml import MSO_FILL, MSO_LINE_DASH_STYLE
    from pptx.enum.shapes import MSO_SHAPE

    prs = Presentation(os.path.join(outdir, 'p3b-shape-properties.pptx'))
    group = prs.slides[2].shapes[0]
    arrow, oval, right_arrow = group.shapes
    rect = lambda shape: (shape.left, shape.top, shape.width, shape.height)
    assert rect(group) == (800000, 1100000, 4000600, 2204032)
    assert rect(arrow) == (800000, 1100000, 2000000, 850000)
    assert arrow.rotation == 30.0
    assert arrow.auto_shape_type == MSO_SHAPE.UP_ARROW
    assert abs(arrow.adjustments[0] - 0.35) < 1e-9
    assert arrow.fill.type == MSO_FILL.SOLID
    assert arrow.fill.fore_color.rgb == RGBColor(18, 52, 86)
    assert arrow.line.width == 38100
    assert arrow.line.color.rgb == RGBColor(192, 80, 40)
    assert arrow.line.dash_style == MSO_LINE_DASH_STYLE.DASH
    assert rect(oval) == (3886200, 1905000, 914400, 914400)
    assert oval.fill.type == MSO_FILL.GRADIENT
    assert [(stop.position, stop.color.rgb) for stop in oval.fill.gradient_stops] == [
        (0.0, RGBColor(255, 255, 255)), (1.0, RGBColor(0, 128, 128)),
    ]
    assert rect(right_arrow) == (1828800, 2819400, 978408, 484632)
    print('P3b python-pptx: shape properties and explicit group fit read back')


def check_text_formatting(outdir):
    from pptx import Presentation
    from pptx.dml.color import RGBColor
    from pptx.enum.dml import MSO_THEME_COLOR
    from pptx.enum.text import MSO_AUTO_SIZE, PP_ALIGN
    from pptx.util import Pt

    prs = Presentation(os.path.join(outdir, 'text-formatting.pptx'))
    body = next(shape.text_frame for shape in prs.slides[0].shapes if shape.has_text_frame)
    para = body.paragraphs[0]
    first, second = para.runs
    assert body.auto_size == MSO_AUTO_SIZE.SHAPE_TO_FIT_TEXT
    assert para.text == 'Lorem ipsum dolor sit amet'
    assert first.text == 'Lorem'
    assert para.alignment == PP_ALIGN.CENTER
    assert para.level == 1
    assert para.line_spacing == 1.2
    assert para.space_before == Pt(6)
    assert para.space_after == Pt(3)
    assert first.font.name == 'Arial'
    assert first.font.size == Pt(28)
    assert first.font.bold is True
    assert first.font.italic is True
    assert first.font.underline is True
    assert first.font.color.rgb == RGBColor(0x12, 0x34, 0x56)
    assert second.font.color.theme_color == MSO_THEME_COLOR.ACCENT_1
    assert second.font.color.brightness == 0.2
    print('P3a python-pptx: direct text formatting read back')


def check_links(outdir):
    from pptx import Presentation
    from pptx.enum.action import PP_ACTION

    prs = Presentation(os.path.join(outdir, 'p3c-links.pptx'))
    title, body, *_ = [shape for shape in prs.slides[0].shapes if shape.has_text_frame]
    run = title.text_frame.paragraphs[0].runs[0]
    assert run.hyperlink.address == 'https://example.com/slides?x=1&y=2'
    assert title.click_action.action == PP_ACTION.NAMED_SLIDE
    assert title.click_action.target_slide.slide_id == prs.slides[1].slide_id
    assert body.click_action.action == PP_ACTION.LAST_SLIDE
    print('P3c python-pptx: URL, named slide and show jump read back')


def check_notes_creation(outdir):
    from pptx import Presentation
    from pptx.enum.shapes import PP_PLACEHOLDER

    path = os.path.join(outdir, '33-p3d-create-notes-a4.pptx')
    prs = Presentation(path)
    slide, next_slide = prs.slides
    assert slide.has_notes_slide
    assert not next_slide.has_notes_slide
    notes = slide.notes_slide
    assert notes.notes_text_frame.text == (
        'Speaker notes for an A4 page.\nCentred 16:10 slide image.'
    )
    assert prs.notes_master is notes.part.notes_master
    master_image = next(ph for ph in prs.notes_master.placeholders
                        if ph.placeholder_format.type == PP_PLACEHOLDER.SLIDE_IMAGE)
    assert (master_image.left, master_image.top, master_image.width, master_image.height) == (
        893160, 1336500, 5773680, 3608550
    )
    print('P3d python-pptx: notes text and scaled notes master read back')


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
        problems = []
        want = norm(expected(fixture, c['edits'], site, c.get('replace', []), problems))
        for pr in problems:
            failures += 1
            print(f'FAIL {c["name"]}: {pr}')
        rust_in = json.load(open(os.path.join(outdir, c['name'] + '.input.ooxml.json'), encoding='utf-8'))
        rust_out = json.load(open(os.path.join(outdir, c['name'] + '.ooxml.json'), encoding='utf-8'))
        edits += len(c['edits'])
        for label, x, y in (('expected model', out, want), ('agreement (output)', out, rust_out),
                            ('agreement (input)', src, rust_in)):
            d = first_diff(x, y)
            if d:
                failures += 1
                print(f'FAIL {c["name"]}: {label}: {d}')
    replaces = sum(len(r['replaced']) for c in cases for r in c.get('replace', []))
    print(f'T3 python-pptx: {len(cases)} cases, {edits} edits, {replaces} replaced matches, {failures} failures')
    check_text_formatting(outdir)
    check_shape_properties(outdir)
    check_links(outdir)
    check_notes_creation(outdir)
    sys.exit(1 if failures else 0)


if __name__ == '__main__':
    main()
