"""Spike for ADR-0016 (PowerPoint Phase 3: formatting writes and links).

Parses untrusted .pptx files with zipfile + xml.etree only (nothing is
executed) and counts the formatting, shape-property, hyperlink, notes,
table-style and theme constructs that Phase 3 writes must produce or
preserve.

    python3 -I formatting.py <dirs/files...>

De-duplicated by SHA-256; unreadable files counted and skipped.
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
}
A = '{%s}' % NS['a']
P = '{%s}' % NS['p']
RT = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships/'
COLOR = ('srgbClr', 'schemeClr', 'sysClr', 'prstClr', 'hslClr', 'scrgbClr')
FILL = ('noFill', 'solidFill', 'gradFill', 'blipFill', 'pattFill', 'grpFill')


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
        ext = r.get('TargetMode') == 'External'
        if not ext:
            t = t.lstrip('/') if t.startswith('/') else posixpath.normpath(posixpath.join(d, t))
        out[r.get('Id')] = (r.get('Type', '').rsplit('/', 1)[-1], t, ext)
    return out


def local(t):
    return t.rsplit('}', 1)[-1]


def main():
    c = collections.Counter()
    h = collections.defaultdict(collections.Counter)
    seen = set()
    for f in files(sys.argv[1:]):
        data = open(f, 'rb').read()
        dig = hashlib.sha256(data).hexdigest()
        if dig in seen:
            c['duplicate files skipped'] += 1
            continue
        seen.add(dig)
        try:
            z = zipfile.ZipFile(f)
            names = set(z.namelist())
            ET.fromstring(z.read('ppt/presentation.xml'))
        except Exception:
            c['unreadable'] += 1
            continue
        c['files'] += 1
        slides = sorted(n for n in names if re.fullmatch(r'ppt/slides/slide\d+\.xml', n))
        masters = sorted(n for n in names if re.fullmatch(r'ppt/slideMasters/[^/]+\.xml', n))
        themes = sorted(n for n in names if re.fullmatch(r'ppt/theme/[^/]+\.xml', n))
        c['slides'] += len(slides)
        # --- notes
        notes = [n for n in names if re.fullmatch(r'ppt/notesSlides/[^/]+\.xml', n)]
        nm = [n for n in names if re.fullmatch(r'ppt/notesMasters/[^/]+\.xml', n)]
        c['notes slides'] += len(notes)
        c['files with a notes master'] += bool(nm)
        if slides and not nm:
            c['files with slides but no notes master'] += 1
        # --- themes
        for t in themes:
            try:
                tx = ET.fromstring(z.read(t))
            except Exception:
                continue
            cs = tx.find('a:themeElements/a:clrScheme', NS)
            if cs is not None:
                h['clrScheme name'][cs.get('name')] += 1
                for slot in cs:
                    kind = local(slot[0].tag) if len(slot) else '-'
                    h['theme slot colour kind'][kind] += 1
            fs = tx.find('a:themeElements/a:fontScheme', NS)
            if fs is not None:
                for which in ('majorFont', 'minorFont'):
                    lat = fs.find(f'a:{which}/a:latin', NS)
                    if lat is not None:
                        h[f'theme {which} latin'][lat.get('typeface')] += 1
                    c[f'theme {which} script fonts'] += len(fs.findall(f'a:{which}/a:font', NS))
        # theme sharing by masters
        mt = collections.Counter()
        for m in masters:
            for _, (ty, t, e) in rels(z, names, m).items():
                if ty == 'theme':
                    mt[t] += 1
            try:
                mx = ET.fromstring(z.read(m))
                cm = mx.find('p:clrMap', NS)
                if cm is not None:
                    std = cm.get('bg1') == 'lt1' and cm.get('tx1') == 'dk1' and cm.get('bg2') == 'lt2' and cm.get('tx2') == 'dk2'
                    c['masters with the standard bg/tx clrMap' if std else 'masters with a non-standard clrMap'] += 1
            except Exception:
                pass
        c['themes shared by >1 master'] += sum(1 for v in mt.values() if v > 1)
        # --- table styles
        if 'ppt/tableStyles.xml' in names:
            try:
                ts = ET.fromstring(z.read('ppt/tableStyles.xml'))
                n = len(ts.findall('a:tblStyle', NS))
                h['tblStyle definitions per file'][min(n, 5)] += 1
                for s in ts.findall('a:tblStyle', NS):
                    parts = sorted(local(ch.tag) for ch in s)
                    h['tblStyle parts'].update(parts)
            except Exception:
                pass
        # --- slides
        for sp in slides:
            try:
                sx = ET.fromstring(z.read(sp))
            except Exception:
                continue
            srels = rels(z, names, sp)
            # clrMapOvr
            ov = sx.find('p:clrMapOvr/a:overrideClrMapping', NS)
            if ov is not None:
                c['slides with overrideClrMapping'] += 1
            for el in sx.iter():
                t = el.tag
                if t == A + 'rPr' or t == A + 'endParaRPr' or t == A + 'defRPr':
                    which = local(t)
                    for k in ('sz', 'b', 'i', 'u', 'strike', 'baseline', 'cap', 'spc', 'kern', 'lang'):
                        if el.get(k) is not None:
                            h[f'{which} attributes'][k] += 1
                    if el.get('u') is not None:
                        h['u values'][el.get('u')] += 1
                    for ch in el:
                        n = local(ch.tag)
                        h[f'{which} children'][n] += 1
                        if n == 'solidFill' and len(ch):
                            h['run solidFill colour kind'][local(ch[0].tag)] += 1
                            for tr in ch[0]:
                                h['run colour transforms'][local(tr.tag)] += 1
                        if n in ('latin', 'ea', 'cs'):
                            tf = ch.get('typeface') or ''
                            h['run typeface kind'][tf if tf.startswith('+') else 'explicit' if tf else 'empty'] += 1
                elif t == A + 'pPr':
                    for k in ('algn', 'marL', 'indent', 'lvl', 'rtl', 'fontAlgn'):
                        if el.get(k) is not None:
                            h['pPr attributes'][k] += 1
                    if el.get('algn'):
                        h['algn values'][el.get('algn')] += 1
                    for ch in el:
                        n = local(ch.tag)
                        h['pPr children'][n] += 1
                        if n in ('lnSpc', 'spcBef', 'spcAft') and len(ch):
                            h['spacing unit'][f'{n}/{local(ch[0].tag)}'] += 1
                        if n == 'buAutoNum':
                            h['buAutoNum type'][ch.get('type')] += 1
                elif t == A + 'bodyPr':
                    kids = [local(x.tag) for x in el]
                    fit = next((k for k in kids if k in ('noAutofit', 'normAutofit', 'spAutoFit')), 'none')
                    h['bodyPr autofit (slides)'][fit] += 1
                    if fit == 'normAutofit':
                        na = el.find('a:normAutofit', NS)
                        if na.get('fontScale') or na.get('lnSpcReduction'):
                            c['normAutofit with fontScale/lnSpcReduction'] += 1
                elif t == P + 'spPr' or t == P + 'grpSpPr':
                    kids = [local(x.tag) for x in el]
                    fill = next((k for k in kids if k in FILL), 'inherited')
                    h[f'{local(t)} fill'][fill] += 1
                    if 'ln' in kids:
                        c[f'{local(t)} with a:ln'] += 1
                    geom = el.find('a:prstGeom', NS)
                    if geom is not None:
                        h['prstGeom'][geom.get('prst')] += 1
                        if geom.find('a:avLst/a:gd', NS) is not None:
                            c['prstGeom with adjustment values'] += 1
                    if el.find('a:custGeom', NS) is not None:
                        c['custGeom'] += 1
                    xf = el.find('a:xfrm', NS)
                    if xf is not None:
                        if xf.get('rot') not in (None, '0'):
                            c[f'{local(t)} rotated'] += 1
                        if xf.get('flipH') in ('1', 'true') or xf.get('flipV') in ('1', 'true'):
                            c[f'{local(t)} flipped'] += 1
                        if t == P + 'grpSpPr':
                            off, ext = xf.find('a:off', NS), xf.find('a:ext', NS)
                            choff, chext = xf.find('a:chOff', NS), xf.find('a:chExt', NS)
                            if None not in (off, ext, choff, chext):
                                same = (off.attrib == choff.attrib) and (ext.attrib == chext.attrib)
                                c['groups with child space == own space' if same else 'groups with a scaled/offset child space'] += 1
                    else:
                        c[f'{local(t)} without xfrm (inherits position)'] += 1
                elif t == A + 'hlinkClick' or t == A + 'hlinkMouseOver' or t == A + 'hlinkHover':
                    parent_kind = 'shape (cNvPr)'
                    # crude: hlink under rPr/endParaRPr counts as text
                    c[f'{local(t)}'] += 1
                    act = el.get('action') or ''
                    h['hlink action'][re.sub(r'\?.*', '?…', act) if act else '(none)'] += 1
                    rid = el.get('{%s}id' % NS['r'])
                    if rid:
                        ty, tgt, ext = srels.get(rid, ('missing', '', False))
                        if ext:
                            scheme = tgt.split(':', 1)[0].lower() if ':' in tgt else 'relative'
                            h['external link target scheme'][scheme] += 1
                        h['hlink relationship type'][ty] += 1
                    elif rid == '':
                        c['hlink with empty r:id'] += 1
                    for ext_el in el.iter():
                        if local(ext_el.tag) == 'hlinkClr':
                            c['ahyp:hlinkClr'] += 1
                            h['hlinkClr val'][ext_el.get('val')] += 1
                    for k in ('tooltip', 'highlightClick', 'history', 'endSnd', 'tgtFrame', 'invalidUrl'):
                        if el.get(k) is not None:
                            h['hlink attributes'][k] += 1
            # where hlinkClick sits
            for parent in sx.iter():
                for ch in parent:
                    if ch.tag == A + 'hlinkClick':
                        h['hlinkClick parent'][local(parent.tag)] += 1
    for k in sorted(c):
        print(f'{k}: {c[k]}')
    for k, v in sorted(h.items()):
        items = sorted(v.items(), key=lambda x: -x[1])[:20]
        print(f'{k}: ' + ', '.join(f'{a}={b}' for a, b in items))


if __name__ == '__main__':
    main()
