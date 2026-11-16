"""Placeholder inheritance evidence (run with `python3 -I`).

Usage: python3 -I placeholders.py <pptx files/dirs...>

For each slide placeholder (p:sp//p:nvPr/p:ph) it resolves the layout part via
the slide's slideLayout relationship and the master via the layout's
slideMaster relationship, then reports:
  - how the slide ph matches a layout ph: by idx, by type only, or not at all;
  - how the layout ph matches a master ph by type (with the usual mapping
    ctrTitle->title, subTitle/obj/other->body);
  - whether the slide ph carries its own a:xfrm (position) or inherits it;
  - how many a:r runs in slide placeholders carry an explicit @sz (font size)
    vs. inherit it.
Nothing from the files is executed.
"""
import sys, os, zipfile, posixpath, collections, re
import xml.etree.ElementTree as ET

P = '{http://schemas.openxmlformats.org/presentationml/2006/main}'
A = '{http://schemas.openxmlformats.org/drawingml/2006/main}'
R = '{http://schemas.openxmlformats.org/package/2006/relationships}'
RT_LAYOUT = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout'
RT_MASTER = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster'

def rels(z, part):
    d, b = posixpath.split(part)
    rp = posixpath.join(d, '_rels', b + '.rels')
    if rp not in z.namelist():
        return []
    out = []
    for r in ET.fromstring(z.read(rp)):
        if r.get('TargetMode') == 'External':
            continue
        out.append((r.get('Type'), posixpath.normpath(posixpath.join(d, r.get('Target')))))
    return out

def phs(root):
    out = []
    for sp in root.iter():
        if sp.tag not in (P + 'sp', P + 'pic', P + 'graphicFrame'):
            continue
        ph = sp.find('.//' + P + 'nvPr/' + P + 'ph')
        if ph is None:
            continue
        out.append((sp, ph.get('type', 'obj'), ph.get('idx', '0')))
    return out

MASTER_TYPE = {'ctrTitle': 'title', 'subTitle': 'body', 'obj': 'body', 'body': 'body',
               'title': 'title', 'dt': 'dt', 'ftr': 'ftr', 'sldNum': 'sldNum'}

def main(argv):
    files = []
    for a in argv:
        files += [os.path.join(a, n) for n in sorted(os.listdir(a)) if n.endswith('.pptx')] if os.path.isdir(a) else [a]
    c = collections.Counter()
    for p in files:
        try:
            z = zipfile.ZipFile(p); names = set(z.namelist())
        except Exception:
            continue
        for s in sorted(n for n in names if re.match(r'ppt/slides/slide\d+\.xml$', n)):
            try:
                sroot = ET.fromstring(z.read(s))
            except Exception:
                continue
            lay = [t for ty, t in rels(z, s) if ty == RT_LAYOUT]
            if len(lay) != 1 or lay[0] not in names:
                c['slide_without_single_layout'] += 1; continue
            try:
                lroot = ET.fromstring(z.read(lay[0]))
                mst = [t for ty, t in rels(z, lay[0]) if ty == RT_MASTER]
                mroot = ET.fromstring(z.read(mst[0])) if mst and mst[0] in names else None
            except Exception:
                c['unreadable_layout_or_master'] += 1; continue
            lph = phs(lroot); mph = phs(mroot) if mroot is not None else []
            for sp, ty, idx in phs(sroot):
                c['slide_ph'] += 1
                if sp.find('.//' + A + 'xfrm') is not None:
                    c['slide_ph_has_xfrm'] += 1
                by_idx = [x for x in lph if x[2] == idx]
                by_type = [x for x in lph if x[1] == ty]
                if by_idx:
                    c['match_layout_by_idx'] += 1
                    if by_idx[0][1] != ty:
                        c['  idx_match_but_type_differs'] += 1
                    lsp, lty = by_idx[0][0], by_idx[0][1]
                elif by_type:
                    c['match_layout_by_type_only'] += 1; lsp, lty = by_type[0][0], ty
                else:
                    c['no_layout_match'] += 1; continue
                mt = MASTER_TYPE.get(lty, lty)
                if any(x[1] == mt for x in mph):
                    c['layout_ph_matches_master_type'] += 1
                else:
                    c['layout_ph_no_master_match'] += 1
                for r in sp.iter(A + 'r'):
                    rpr = r.find(A + 'rPr')
                    c['ph_runs'] += 1
                    if rpr is not None and rpr.get('sz'):
                        c['ph_runs_explicit_sz'] += 1
    for k in sorted(c):
        print('%-34s %d' % (k, c[k]))

if __name__ == '__main__':
    main(sys.argv[1:])
