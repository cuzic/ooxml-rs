"""Spike for ADR-0013 (PowerPoint Phase 1b: table structure and SmartArt text).

Parses untrusted .pptx files with zipfile + xml.etree only (nothing is
executed) and counts what structural table edits and SmartArt text edits
must respect.

    python3 -I tables_smartart.py <dirs/files...>

Files are de-duplicated by SHA-256; unreadable files are counted and skipped.
"""
import collections
import hashlib
import os
import posixpath
import re
import sys
import zipfile
import xml.etree.ElementTree as ET

NS = {
    'p': 'http://schemas.openxmlformats.org/presentationml/2006/main',
    'a': 'http://schemas.openxmlformats.org/drawingml/2006/main',
    'r': 'http://schemas.openxmlformats.org/officeDocument/2006/relationships',
    'rel': 'http://schemas.openxmlformats.org/package/2006/relationships',
    'dgm': 'http://schemas.openxmlformats.org/drawingml/2006/diagram',
    'dsp': 'http://schemas.microsoft.com/office/drawing/2008/diagram',
    'a16': 'http://schemas.microsoft.com/office/drawing/2014/main',
    'mc': 'http://schemas.openxmlformats.org/markup-compatibility/2006',
}
TABLE_URI = 'http://schemas.openxmlformats.org/drawingml/2006/table'
DIAGRAM_URI = 'http://schemas.openxmlformats.org/drawingml/2006/diagram'
RT = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships/'
DRAWING_EXT_URI = 'http://schemas.microsoft.com/office/drawing/2008/diagram'


def q(t):
    p, l = t.split(':')
    return '{%s}%s' % (NS[p], l)


def files(args):
    for a in args:
        if os.path.isdir(a):
            for root, _, names in os.walk(a):
                for n in sorted(names):
                    if n.lower().endswith(('.pptx', '.pptm', '.potx', '.ppsx')):
                        yield os.path.join(root, n)
        else:
            yield a


def rels(z, names, part):
    d, b = posixpath.split(part)
    rn = posixpath.join(d, '_rels', b + '.rels')
    out = {}
    if rn not in names:
        return out
    for r in ET.fromstring(z.read(rn)).findall('rel:Relationship', NS):
        t = r.get('Target', '')
        if r.get('TargetMode') != 'External':
            t = t.lstrip('/') if t.startswith('/') else posixpath.normpath(posixpath.join(d, t))
        out[r.get('Id')] = (r.get('Type'), t)
    return out


def text_of(el):
    """Paragraph texts of a txBody-like element (a:p children)."""
    if el is None:
        return []
    paras = []
    for p in el.findall('a:p', NS):
        s = []
        for c in p:
            if c.tag == q('a:r') or c.tag == q('a:fld'):
                t = c.find('a:t', NS)
                s.append(t.text or '' if t is not None else '')
            elif c.tag == q('a:br'):
                s.append('\v')
        paras.append(''.join(s))
    return paras


def bool_attr(e, name):
    return e.get(name) in ('1', 'true')


def main():
    c = collections.Counter()
    hist = collections.defaultdict(collections.Counter)
    ex = collections.defaultdict(list)
    seen = set()
    for f in files(sys.argv[1:]):
        data = open(f, 'rb').read()
        h = hashlib.sha256(data).hexdigest()
        if h in seen:
            c['duplicate files skipped'] += 1
            continue
        seen.add(h)
        name = os.path.basename(f)
        try:
            z = zipfile.ZipFile(f)
            names = set(z.namelist())
            slides = sorted(n for n in names if re.fullmatch(r'ppt/slides/slide\d+\.xml', n))
            c['files'] += 1
        except Exception:
            c['unreadable'] += 1
            continue
        file_has_table = file_has_dgm = False
        ts_ids = set()
        if 'ppt/tableStyles.xml' in names:
            try:
                ts = ET.fromstring(z.read('ppt/tableStyles.xml'))
                ts_ids = {s.get('styleId') for s in ts.findall('a:tblStyle', NS)}
                c['tableStyles.xml with tblStyle entries'] += bool(ts_ids)
                hist['tableStyles.xml def'][ts.get('def')] += 1
            except Exception:
                c['tableStyles.xml unreadable'] += 1
        for sp in slides:
            try:
                sx = ET.fromstring(z.read(sp))
            except Exception:
                c['slide parse errors'] += 1
                continue
            srels = rels(z, names, sp)
            for gd in sx.iter(q('a:graphicData')):
                uri = gd.get('uri')
                if uri == TABLE_URI:
                    file_has_table = True
                    table(c, hist, ex, name, gd, ts_ids, sx)
                elif uri == DIAGRAM_URI:
                    file_has_dgm = True
                    diagram(c, hist, ex, name, z, names, gd, srels)
        c['files with tables'] += file_has_table
        c['files with SmartArt'] += file_has_dgm
    for k in sorted(c):
        print(f'{k}: {c[k]}')
    for k, v in sorted(hist.items()):
        items = sorted(v.items(), key=lambda x: -x[1])[:25]
        print(f'{k}: ' + ', '.join(f'{a}={b}' for a, b in items))
    for k, v in sorted(ex.items()):
        print(f'example {k}: {v[:5]}')


def table(c, hist, ex, name, gd, ts_ids, sx):
    tbl = gd.find('a:tbl', NS)
    if tbl is None:
        c['table graphicData without a:tbl'] += 1
        return
    c['tables'] += 1
    grid = tbl.findall('a:tblGrid/a:gridCol', NS)
    rows = tbl.findall('a:tr', NS)
    ncol, nrow = len(grid), len(rows)
    hist['table rows'][min(nrow, 20)] += 1
    hist['table columns'][min(ncol, 12)] += 1
    # frame size vs grid/rows
    frame = None
    for fr in sx.iter(q('p:graphicFrame')):
        if gd in list(fr.iter(q('a:graphicData'))):
            frame = fr
    if frame is not None:
        ext = frame.find('p:xfrm/a:ext', NS)
        if ext is not None:
            w = sum(int(g.get('w', 0)) for g in grid)
            hsum = sum(int(r.get('h', 0)) for r in rows)
            if int(ext.get('cx', 0)) == w:
                c['frame cx == sum(gridCol@w)'] += 1
            else:
                c['frame cx != sum(gridCol@w)'] += 1
            if int(ext.get('cy', 0)) == hsum:
                c['frame cy == sum(tr@h)'] += 1
            else:
                c['frame cy != sum(tr@h) (rows grow with text)'] += 1
    tblpr = tbl.find('a:tblPr', NS)
    if tblpr is not None:
        sid = tblpr.find('a:tableStyleId', NS)
        if sid is not None:
            c['tables with tableStyleId'] += 1
            if sid.text in ts_ids:
                c['tableStyleId defined in tableStyles.xml'] += 1
            else:
                c['tableStyleId not in tableStyles.xml (built-in by GUID)'] += 1
                hist['undefined tableStyleId'][sid.text] += 1
        for flag in ('firstRow', 'bandRow', 'firstCol', 'lastRow', 'lastCol', 'bandCol', 'rtl'):
            if bool_attr(tblpr, flag):
                hist['tblPr flags'][flag] += 1
        if tblpr.find('a:tableStyle', NS) is not None:
            c['tables with an inline a:tableStyle'] += 1
    # a16 ids
    if any(g.find('a:extLst/a:ext/a16:colId', NS) is not None for g in grid):
        c['tables with a16:colId'] += 1
    if any(r.find('a:extLst/a:ext/a16:rowId', NS) is not None for r in rows):
        c['tables with a16:rowId'] += 1
    # cells
    bad_count = False
    merged = False
    occ = [[None] * ncol for _ in range(nrow)]
    for ri, r in enumerate(rows):
        tcs = r.findall('a:tc', NS)
        if len(tcs) != ncol:
            bad_count = True
        for ci, tc in enumerate(tcs):
            c['cells'] += 1
            gs, rs = int(tc.get('gridSpan', 1)), int(tc.get('rowSpan', 1))
            hm, vm = bool_attr(tc, 'hMerge'), bool_attr(tc, 'vMerge')
            if gs > 1:
                c['cells gridSpan>1'] += 1
            if rs > 1:
                c['cells rowSpan>1'] += 1
            if hm:
                c['cells hMerge'] += 1
            if vm:
                c['cells vMerge'] += 1
            if gs > 1 or rs > 1 or hm or vm:
                merged = True
            if vm and gs > 1:
                c['vMerge cells that also carry gridSpan (python-pptx/LibreOffice edge form)'] += 1
            if hm and rs > 1:
                c['hMerge cells that also carry rowSpan (edge form)'] += 1
            if (hm or vm) and any(t for t in text_of(tc.find('a:txBody', NS))):
                c['covered cells with non-empty text'] += 1
            pr = tc.find('a:tcPr', NS)
            if pr is not None:
                for k, v in pr.attrib.items():
                    hist['tcPr attributes'][k] += 1
                for ch in pr:
                    hist['tcPr children'][ch.tag.split('}')[1]] += 1
            if ci < ncol and ri < nrow:
                occ[ri][ci] = (gs, rs, hm, vm)
    if bad_count:
        c['tables with a row whose a:tc count != gridCol count'] += 1
    if merged:
        c['tables with merges'] += 1
        # consistency: every origin's covered rectangle carries the flags
        ok = True
        for ri in range(nrow):
            for ci in range(ncol):
                cell = occ[ri][ci]
                if cell is None:
                    continue
                gs, rs, hm, vm = cell
                if hm or vm:
                    continue
                for dr in range(rs):
                    for dc in range(gs):
                        if dr == dc == 0:
                            continue
                        rr, cc = ri + dr, ci + dc
                        if rr >= nrow or cc >= ncol or occ[rr][cc] is None:
                            ok = False
                            continue
                        _, _, h2, v2 = occ[rr][cc]
                        if dc > 0 and not h2:
                            ok = False
                        if dr > 0 and not v2:
                            ok = False
        if ok:
            c['merged tables consistent (origin span = covered flags)'] += 1
        else:
            c['merged tables with inconsistent span/flags'] += 1
            ex['inconsistent merges'].append(name)


def diagram(c, hist, ex, name, z, names, gd, srels):
    c['SmartArt frames'] += 1
    ri = gd.find('dgm:relIds', NS)
    if ri is None:
        c['SmartArt without dgm:relIds'] += 1
        return
    dm = srels.get(ri.get(q('r:dm')))
    lo = srels.get(ri.get(q('r:lo')))
    if not dm or dm[1] not in names:
        c['SmartArt with missing data part'] += 1
        return
    data = ET.fromstring(z.read(dm[1]))
    if lo and lo[1] in names:
        try:
            lay = ET.fromstring(z.read(lo[1]))
            hist['layout uniqueId'][(lay.get('uniqueId') or '').rsplit('/', 1)[-1]] += 1
        except Exception:
            pass
    pts = data.findall('dgm:ptLst/dgm:pt', NS)
    cxns = data.findall('dgm:cxnLst/dgm:cxn', NS)
    hist['pt types'].update(pt.get('type', 'node') for pt in pts)
    hist['cxn types'].update(cx.get('type', 'parOf') for cx in cxns)
    nodes = {pt.get('modelId'): pt for pt in pts if pt.get('type', 'node') in ('node', 'asst')}
    texts = {mid: text_of(pt.find('dgm:t', NS)) for mid, pt in nodes.items()}
    with_text = {m for m, t in texts.items() if any(x for x in t)}
    c['data nodes (node/asst)'] += len(nodes)
    for mid, pt in nodes.items():
        ps = pt.find('dgm:prSet', NS)
        if ps is not None and ps.get('phldr') in ('1', 'true'):
            if any(x for x in text_of(pt.find('dgm:t', NS))):
                c['nodes phldr=1 with text'] += 1
            else:
                c['nodes phldr=1 without text (placeholder prompt)'] += 1
    c['data nodes with text'] += len(with_text)
    # data text formatting
    for pt in nodes.values():
        for r in pt.iterfind('dgm:t/a:p/a:r', NS):
            rp = r.find('a:rPr', NS)
            if rp is not None and len(rp):
                c['data runs whose rPr has children'] += 1
            c['data runs'] += 1
        for p in pt.iterfind('dgm:t/a:p', NS):
            if len(p.findall('a:r', NS)) > 1:
                c['data paragraphs with >1 run'] += 1
            if p.find('a:fld', NS) is not None:
                c['data paragraphs with a field'] += 1
    # the drawing shapes that show a node's text: the pres points its
    # `presOf` connections point to (srcId = data node, destId = pres point)
    pres_of = collections.defaultdict(list)
    for cx in cxns:
        if cx.get('type') == 'presOf':
            pres_of[cx.get('srcId')].append(cx.get('destId'))
    # drawing part
    ext = data.find('dgm:extLst/a:ext/dsp:dataModelExt', NS)
    if ext is None:
        c['SmartArt without dataModelExt (no drawing cache)'] += 1
        return
    drel = srels.get(ext.get('relId'))
    if not drel or drel[1] not in names:
        c['SmartArt dataModelExt relId unresolved from the slide'] += 1
        return
    c['SmartArt with a drawing cache'] += 1
    if drel[0] != 'http://schemas.microsoft.com/office/2007/relationships/diagramDrawing':
        hist['drawing rel type'][drel[0]] += 1
    dr = ET.fromstring(z.read(drel[1]))
    dsp = {s.get('modelId'): s for s in dr.iter(q('dsp:sp'))}
    if not dsp:
        # LibreOffice ignores such a drawing (DiagramShapeCounter) and lays out itself
        c['drawing cache present but without shapes'] += 1
        return
    c['SmartArt with a non-empty drawing cache'] += 1
    c['drawing shapes'] += len(dsp)
    c['drawing shapes with text'] += sum(1 for s in dsp.values() if any(t for t in text_of(s.find('dsp:txBody', NS))))
    # per drawing shape: do its paragraphs equal the concatenation of the
    # paragraphs of the data nodes whose presOf points at it (by destOrd)?
    srcs = collections.defaultdict(list)
    for cx in cxns:
        if cx.get('type') == 'presOf':
            srcs[cx.get('destId')].append((int(cx.get('destOrd', 0)), int(cx.get('srcOrd', 0)), cx.get('srcId')))
    for mid, shp in dsp.items():
        shown = text_of(shp.find('dsp:txBody', NS))
        if not any(shown):
            continue
        order = [sid for _, _, sid in sorted(srcs.get(mid, []))]
        concat = [x for sid in order for x in texts.get(sid, [])]
        if not order:
            c['text shapes with no presOf source'] += 1
        elif concat == shown:
            c['text shapes == concat of presOf sources (destOrd)'] += 1
            if len(order) > 1:
                c['  of which aggregate >1 node'] += 1
        else:
            c['text shapes != concat of presOf sources'] += 1
            ex['shape != concat'].append((name, shown[:3], concat[:3]))
    for mid in with_text:
        shapes = [dsp[p] for p in pres_of.get(mid, []) if p in dsp]
        shaped_text = [text_of(s.find('dsp:txBody', NS)) for s in shapes]
        shaped_text = [t for t in shaped_text if any(x for x in t)]
        mine = [x for x in texts[mid]]
        if not shaped_text:
            # text of this node may be shown in another node's shape (aggregation)
            found = [m for m, s in dsp.items() if all(x in text_of(s.find('dsp:txBody', NS)) for x in mine if x)]
            if found:
                c['node text shown only inside another node\'s shape (aggregated)'] += 1
            else:
                c['node text not in the drawing (stale or hidden)'] += 1
                ex['node text not in drawing'].append((name, mine[:2]))
        elif len(shaped_text) == 1 and shaped_text[0] == mine:
            c['node text == its one drawing shape text (1:1)'] += 1
        elif any(all(x in t for x in mine if x) for t in shaped_text):
            c['node text is part of its drawing shape (shape aggregates children)'] += 1
        else:
            c['node text != its drawing shape text (stale cache)'] += 1
            ex['stale drawing text'].append((name, mine[:2], shaped_text[0][:2]))


if __name__ == '__main__':
    main()
