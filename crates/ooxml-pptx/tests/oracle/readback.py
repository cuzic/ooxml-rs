"""Independent read-back for ADR-0005 T3: the text model of a .pptx read
with python-pptx (no code shared with ooxml-rs). Same JSON as
tests/oracle/model.rs.

Run with `python3 -I`; python-pptx (pinned in requirements.txt) is found on
sys.path or in the directory given by --site.

    python3 -I readback.py [--site DIR] file.pptx
"""
import json
import sys


def import_pptx(site=None):
    if site:
        sys.path.insert(0, site)
    import pptx  # noqa: F401
    from pptx import Presentation
    from pptx.oxml.ns import qn
    from pptx.shapes.group import GroupShape
    from pptx.text.text import TextFrame
    return Presentation, qn, GroupShape, TextFrame


def attrs(el):
    """Attributes of an element (Clark keys) or None if absent/empty."""
    if el is None:
        return None
    d = {k: v for k, v in el.attrib.items()}
    return d or None


def dump(path, site=None):
    Presentation, qn, GroupShape, TextFrame = import_pptx(site)

    def body(txBody):
        if txBody is None:
            return []
        out = []
        for p in TextFrame(txBody, None).paragraphs:
            items, joined = [], []
            for el in p._p.content_children:
                tag = el.tag
                if tag == qn('a:r'):
                    items.append(['r', el.text, attrs(el.rPr)])
                    joined.append(el.text)
                elif tag == qn('a:br'):
                    items.append(['br'])
                    joined.append('\v')
                elif tag == qn('a:fld'):
                    t = el.find(qn('a:t'))
                    text = (t.text or '') if t is not None else ''
                    items.append(['fld', text])
                    joined.append(text)
            # python-pptx's own paragraph text must agree with the items
            assert p.text == ''.join(joined), (p.text, joined)
            out.append({'ppr': attrs(p._p.pPr), 'end': attrs(p._p.find(qn('a:endParaRPr'))), 'items': items})
        return out

    def walk(shapes, out):
        for sh in shapes:
            if isinstance(sh, GroupShape):
                walk(sh.shapes, out)
            elif getattr(sh, 'has_table', False) and sh.has_table:
                for ri, row in enumerate(sh.table.rows):
                    for ci, cell in enumerate(row.cells):
                        out.append({'shape_id': sh.shape_id, 'name': sh.name, 'cell': [ri, ci], 'ph': None,
                                    'paragraphs': body(cell._tc.find(qn('a:txBody')))})
            elif sh._element.tag == qn('p:sp'):
                txBody = sh._element.find(qn('p:txBody'))
                if txBody is None:
                    continue
                ph = sh._element.ph
                out.append({'shape_id': sh.shape_id, 'name': sh.name, 'cell': None,
                            'ph': None if ph is None else [ph.get('type'), ph.get('idx')],
                            'paragraphs': body(txBody)})

    prs = Presentation(path)
    # Read part names from sldIdLst *before* touching prs.slides: python-pptx
    # renames slide parts to slide1..N when the slides collection is built.
    entries = []
    for sldId in prs.part._element.sldIdLst.sldId_lst:
        part = prs.part.related_part(sldId.rId)
        entries.append((sldId.id, str(part.partname), part))
    slides = []
    for sid, partname, part in entries:
        shapes = []
        walk(part.slide.shapes, shapes)
        slides.append({'slide_id': sid, 'partname': partname, 'shapes': shapes})
    return {'slides': slides}


if __name__ == '__main__':
    args = sys.argv[1:]
    site = None
    if args[:1] == ['--site']:
        site, args = args[1], args[2:]
    json.dump(dump(args[0], site), sys.stdout, ensure_ascii=False, indent=1)
