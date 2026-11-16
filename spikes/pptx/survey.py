"""Feature inventory of .pptx files (untrusted data: run with `python3 -I`).

Usage: python3 -I survey.py <dir-or-files...> [--json out.json]

Reads every part with zipfile + xml.etree (no code from the files is executed,
no external entities are resolved) and counts PowerPoint features per file.
"""
import sys, zipfile, json, re, os, collections
import xml.etree.ElementTree as ET

NS = {
    'p': 'http://schemas.openxmlformats.org/presentationml/2006/main',
    'a': 'http://schemas.openxmlformats.org/drawingml/2006/main',
    'r': 'http://schemas.openxmlformats.org/officeDocument/2006/relationships',
    'mc': 'http://schemas.openxmlformats.org/markup-compatibility/2006',
    'p14': 'http://schemas.microsoft.com/office/powerpoint/2010/main',
    'p15': 'http://schemas.microsoft.com/office/powerpoint/2012/main',
    'p188': 'http://schemas.microsoft.com/office/powerpoint/2018/8/main',
    'c': 'http://schemas.openxmlformats.org/drawingml/2006/chart',
    'dgm': 'http://schemas.openxmlformats.org/drawingml/2006/diagram',
}
def q(t):
    p, l = t.split(':')
    return '{%s}%s' % (NS[p], l)

SLIDE_RE = re.compile(r'^ppt/slides/slide\d+\.xml$')

def survey(path):
    f = collections.Counter()
    notes = []
    try:
        z = zipfile.ZipFile(path)
    except Exception as e:
        return {'file': os.path.basename(path), 'error': 'zip: %s' % e}
    names = z.namelist()
    f['parts'] = len(names)
    def parse(n):
        try:
            return ET.fromstring(z.read(n))
        except Exception as e:
            notes.append('parse %s: %s' % (n, type(e).__name__))
            return None
    f['slides'] = sum(1 for n in names if SLIDE_RE.match(n))
    f['layouts'] = sum(1 for n in names if re.match(r'^ppt/slideLayouts/slideLayout\d+\.xml$', n))
    f['masters'] = sum(1 for n in names if re.match(r'^ppt/slideMasters/slideMaster\d+\.xml$', n))
    f['notesSlides'] = sum(1 for n in names if re.match(r'^ppt/notesSlides/.*\.xml$', n))
    f['notesMaster'] = sum(1 for n in names if re.match(r'^ppt/notesMasters/.*\.xml$', n))
    f['handoutMaster'] = sum(1 for n in names if re.match(r'^ppt/handoutMasters/.*\.xml$', n))
    f['legacyComments'] = sum(1 for n in names if re.match(r'^ppt/comments/comment\d+\.xml$', n))
    f['modernComments'] = sum(1 for n in names if re.match(r'^ppt/comments/modernComment.*\.xml$', n))
    f['commentAuthors'] = int('ppt/commentAuthors.xml' in names)
    f['authors(p188)'] = int('ppt/authors.xml' in names)
    f['charts'] = sum(1 for n in names if re.match(r'^ppt/charts/chart\d+\.xml$', n))
    f['embeddings'] = sum(1 for n in names if n.startswith('ppt/embeddings/'))
    f['diagramData'] = sum(1 for n in names if re.match(r'^ppt/diagrams/data\d+\.xml$', n))
    f['media'] = sum(1 for n in names if n.startswith('ppt/media/'))
    f['fonts'] = sum(1 for n in names if n.startswith('ppt/fonts/'))
    f['vba'] = int(any(n.endswith('vbaProject.bin') for n in names))
    f['customXml'] = sum(1 for n in names if n.startswith('customXml/') and n.endswith('.xml') and '/_rels/' not in n and 'itemProps' not in n)
    app = 'docProps/app.xml'
    if app in names:
        a = parse(app)
        if a is not None:
            ep = '{http://schemas.openxmlformats.org/officeDocument/2006/extended-properties}'
            notes.append('app=%s %s' % (a.findtext(ep + 'Application') or '?', a.findtext(ep + 'AppVersion') or ''))
    pres = parse('ppt/presentation.xml') if 'ppt/presentation.xml' in names else None
    if pres is not None:
        ids = [int(s.get('id')) for s in pres.iter(q('p:sldId')) if s.get('id', '').isdigit()]
        f['sldId_min'] = min(ids) if ids else 0
        f['sldId_max'] = max(ids) if ids else 0
        f['sldId_gaps'] = int(bool(ids) and (max(ids) - min(ids) + 1 != len(ids)))
        f['sections(p14)'] = sum(1 for _ in pres.iter(q('p14:section')))
        f['custShow'] = sum(1 for _ in pres.iter(q('p:custShow')))
        f['embeddedFont'] = sum(1 for _ in pres.iter(q('p:embeddedFont')))
        f['pres_AC'] = sum(1 for _ in pres.iter(q('mc:AlternateContent')))
    for n in names:
        if not (SLIDE_RE.match(n) or n.startswith('ppt/slideLayouts/slideLayout') or n.startswith('ppt/slideMasters/slideMaster')) or not n.endswith('.xml'):
            continue
        root = parse(n)
        if root is None:
            continue
        kind = 'sld' if SLIDE_RE.match(n) else ('lay' if 'Layout' in n else 'mst')
        for ch in root:
            if ch.tag == q('mc:AlternateContent'):
                f[kind + '_AC_child'] += 1
                inner = {c.tag.split('}')[1] for br in ch for c in br}
                for t in inner:
                    f[kind + '_AC_wraps_' + t] += 1
            if ch.tag == q('p:transition'):
                f[kind + '_transition_direct'] += 1
            if ch.tag == q('p:timing'):
                f[kind + '_timing_direct'] += 1
        if kind != 'sld':
            continue
        ids = []
        for el in root.iter():
            t = el.tag
            if t == q('p:cNvPr'):
                if el.get('id', '').isdigit():
                    ids.append(int(el.get('id')))
            elif t == q('p:sp'): f['sp'] += 1
            elif t == q('p:pic'): f['pic'] += 1
            elif t == q('p:grpSp'): f['grpSp'] += 1
            elif t == q('p:cxnSp'): f['cxnSp'] += 1
            elif t == q('p:graphicFrame'): f['graphicFrame'] += 1
            elif t == q('a:tbl'): f['tbl'] += 1
            elif t == q('p:ph'): f['ph'] += 1
            elif t == q('a:prstGeom'): f['prstGeom'] += 1
            elif t == q('a:custGeom'): f['custGeom'] += 1
            elif t == q('a:fld'): f['fld'] += 1
            elif t == q('a:hlinkClick'): f['hlinkClick'] += 1
            elif t == q('a:hlinkMouseOver'): f['hlinkMouseOver'] += 1
            elif t == q('a:br'): f['br'] += 1
            elif t == q('mc:AlternateContent'): f['AC_any_depth'] += 1
            elif t == q('p:oleObj'): f['oleObj'] += 1
            elif t in (q('a:videoFile'), q('a:audioFile'), q('p14:media')): f['media_ref'] += 1
            elif t == q('a:graphicData'):
                u = el.get('uri', '')
                if u.endswith('/chart'): f['gf_chart'] += 1
                elif u.endswith('/diagram'): f['gf_smartart'] += 1
                elif u.endswith('/table'): f['gf_table'] += 1
                elif u.endswith('/ole'): f['gf_ole'] += 1
                else: f['gf_other'] += 1
            elif t == q('a:p'):
                runs = [c for c in el if c.tag in (q('a:r'), q('a:fld'))]
                if runs:
                    f['para_text'] += 1
                    if len(runs) > 1: f['para_multirun'] += 1
            elif t == q('a:t'):
                txt = el.text or ''
                if txt != txt.strip(): f['t_edge_ws'] += 1
            elif t == q('p:txBody'):
                f['txBody'] += 1
        if len(ids) != len(set(ids)):
            f['dup_shape_ids'] += 1
        if ids:
            f['max_shape_id'] = max(f['max_shape_id'], max(ids))
    return {'file': os.path.basename(path), 'size': os.path.getsize(path), 'f': dict(f), 'notes': notes}

def main(argv):
    out = None
    if '--json' in argv:
        i = argv.index('--json'); out = argv[i + 1]; argv = argv[:i] + argv[i + 2:]
    files = []
    for a in argv:
        if os.path.isdir(a):
            files += sorted(os.path.join(a, n) for n in os.listdir(a) if n.endswith('.pptx'))
        else:
            files.append(a)
    res = [survey(p) for p in files]
    if out:
        with open(out, 'w') as fh:
            json.dump(res, fh, indent=1)
    tot = collections.Counter(); has = collections.Counter()
    for r in res:
        for k, v in r.get('f', {}).items():
            if k.startswith('sldId') or k == 'max_shape_id':
                continue
            tot[k] += v; has[k] += bool(v)
    print('files', len(res), 'errors', sum('error' in r for r in res))
    for k in sorted(tot):
        print('%-28s total=%-7d files=%d' % (k, tot[k], has[k]))

if __name__ == '__main__':
    main(sys.argv[1:])
