"""Corpus counts quoted by ADR-0005 revision 2 (run with `python3 -I`).

Usage: python3 -I probe.py <dirs/files…>

Slide parts only (ppt/slides/slideN.xml), plus modern comment parts for
`ac:txMk`. Prints one `key value` line per count. Unreadable zips are
skipped (and counted).
"""
import os, re, sys, zipfile
import xml.etree.ElementTree as ET

P = '{http://schemas.openxmlformats.org/presentationml/2006/main}'
A = '{http://schemas.openxmlformats.org/drawingml/2006/main}'
MC = '{http://schemas.openxmlformats.org/markup-compatibility/2006}'
XML = '{http://www.w3.org/XML/1998/namespace}'
AC = '{http://schemas.microsoft.com/office/drawing/2013/main/command}'
PROOF = {'lang', 'altLang', 'dirty', 'err', 'noProof', 'smtClean', 'smtId', 'bmk'}

def files(args):
    for a in args:
        if os.path.isdir(a):
            for r, _, fs in os.walk(a):
                for f in sorted(fs):
                    if f.lower().endswith(('.pptx', '.pptm', '.ppsx', '.potx')):
                        yield os.path.join(r, f)
        else:
            yield a

c = {}
def inc(k, n=1):
    c[k] = c.get(k, 0) + n

def para(p):
    kids = list(p)
    runs = [k for k in kids if k.tag == A + 'r']
    flds = [k for k in kids if k.tag == A + 'fld']
    end = p.find(A + 'endParaRPr')
    ppr = p.find(A + 'pPr')
    inc('paragraphs')
    if ppr is not None and len(ppr):
        inc('pPr_with_children')
    if flds:
        inc('paragraphs_with_fld')
    if not runs and not flds:
        inc('paragraphs_without_runs')
        if end is not None and len(end):
            inc('empty_paragraphs_endParaRPr_with_children')
        if end is not None and (len(end) or set(end.attrib) - PROOF):
            inc('empty_paragraphs_endParaRPr_visible')
    if runs:
        rpr = runs[0].find(A + 'rPr')
        inc('paragraphs_with_runs')
        if rpr is not None and len(rpr):
            inc('first_rPr_with_children')
    for k in kids:
        if k.tag == MC + 'AlternateContent':
            inc('ac_under_a_p')
        if k.tag not in (A + 'pPr', A + 'r', A + 'br', A + 'fld', A + 'endParaRPr'):
            inc('other_children_of_a_p')

def body(b, key):
    inc(key)
    ps = b.findall(A + 'p')
    if len(ps) == 1 and not ps[0].findall(A + 'r') and not ps[0].findall(A + 'fld'):
        inc(key + '_single_empty_paragraph')
    if ps and ps[0].findall(A + 'fld') and not ps[0].findall(A + 'r'):
        inc(key + '_first_paragraph_field_only')
    for k in b:
        if k.tag == MC + 'AlternateContent':
            inc('ac_under_txBody')
    for p in ps:
        para(p)

bad = 0
for f in files(sys.argv[1:]):
    try:
        z = zipfile.ZipFile(f)
    except Exception:
        bad += 1
        continue
    inc('files')
    for n in z.namelist():
        if re.match(r'ppt/comments/modernComment.*\.xml$', n):
            inc('ac_txMk', len(ET.fromstring(z.read(n)).findall('.//' + AC + 'txMk')))
        if not re.match(r'ppt/slides/slide\d+\.xml$', n):
            continue
        inc('slides')
        root = ET.fromstring(z.read(n))
        for b in root.iter(P + 'txBody'):
            body(b, 'p_txBody')
        for b in root.iter(A + 'txBody'):
            body(b, 'a_txBody')
        for t in root.iter(A + 't'):
            inc('a_t')
            if t.get(XML + 'space') is not None:
                inc('a_t_xml_space')
            s = t.text or ''
            if s and s != s.strip(' \t\r\n'):
                inc('a_t_edge_ws')
            if s and not s.strip(' \t\r\n'):
                inc('a_t_ws_only')
        for r in root.iter(A + 'fld'):
            inc('a_fld')
        for na in root.iter(A + 'normAutofit'):
            fs, ls = na.get('fontScale'), na.get('lnSpcReduction')
            inc('normAutofit_fontScale', fs is not None)
            inc('normAutofit_lnSpcReduction', ls is not None)
            inc('normAutofit_either', fs is not None or ls is not None)
        inc('spAutoFit', len(list(root.iter(A + 'spAutoFit'))))
        for b in root.iter(P + 'bldP'):
            inc('bldP_not_whole', b.get('build', 'whole') != 'whole')
        inc('pRg', len(list(root.iter(P + 'pRg'))))
        inc('charRg', len(list(root.iter(P + 'charRg'))))
c['unreadable'] = bad
for k in sorted(c):
    print(k, int(c[k]))
