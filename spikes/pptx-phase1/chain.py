"""Inheritance-chain evidence for ADR-0008 (run with `python3 -I`).

Usage: python3 -I chain.py <pptx files/dirs...> [--geometry out.json]

This is a *model* of ADR-0008 §1's resolver written in Python, used to
count how often each rule matters. For every slide it follows
slide -> slideLayout -> slideMaster -> theme and reports:

  * placeholder matching slide->layout (idx, then type; duplicates; the
    cases where python-pptx (idx only) and POI (idx only when @idx is
    present, then type) would pick differently) and layout->master (mapped
    type);
  * which part supplies geometry (a:xfrm), body properties (a:bodyPr
    @anchor), run properties (sz, b, latin typeface, fill) and paragraph
    properties (algn, marL, bullet kind), for placeholder and
    non-placeholder text;
  * how often the contested steps decide a value: master otherStyle vs
    presentation defaultTextStyle for non-placeholder text (Q6), the
    txStyles category of dt/ftr/sldNum, a:pPr/a:defRPr on a slide paragraph,
    p:style/a:fontRef, lstStyle a:defPPr, level fall-back in txStyles;
  * theme references: +mj/+mn fonts, schemeClr through clrMap/clrMapOvr.

With --geometry it writes the resolved off/ext of every slide placeholder
({file, part, id, x, y, cx, cy, source}) for ppy_geometry.py to compare
with python-pptx.
"""
import sys, os, json, collections
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import Pkg, q, qn, iter_files, walk_shapes, ph_of, ph_key, sptree, NS

MASTER_TYPE = {'title': 'title', 'ctrTitle': 'title', 'dt': 'dt', 'ftr': 'ftr',
               'sldNum': 'sldNum', 'hdr': 'hdr'}  # everything else -> body
TITLE = {'title', 'ctrTitle'}
OTHER_POI = {'dt', 'ftr', 'sldNum'}            # POI getDefaultMasterStyle: otherStyle
FILLS = [q('a:' + t) for t in ('noFill', 'solidFill', 'gradFill', 'blipFill', 'pattFill', 'grpFill')]
BULLETS = [q('a:' + t) for t in ('buNone', 'buAutoNum', 'buChar', 'buBlip')]
PROOF = {'lang', 'altLang', 'err', 'dirty', 'smtClean', 'smtId', 'noProof', 'bmk'}

C = collections.Counter()
EX = collections.defaultdict(list)   # examples per counter key


def ex(key, what):
    C[key] += 1
    if len(EX[key]) < 3:
        EX[key].append(what)


def master_type(t):
    return MASTER_TYPE.get(t, 'body')


class Ph:
    def __init__(self, shape, in_group):
        self.shape = shape
        ph = ph_of(shape)
        self.ph = ph
        self.type, self.idx = ph_key(ph)
        self.has_idx = ph.get('idx') is not None
        self.has_type = ph.get('type') is not None
        self.in_group = in_group


def phs(root):
    return [Ph(s, g) for s, _d, g in walk_shapes(sptree(root)) if ph_of(s) is not None]


def match_layout(sp, lph):
    """ADR-0008 §1.2 rule. Effective idx = @idx or 0.
    1. layout placeholders with the same effective idx: exact type, then
       same master family (title / body / dt / ftr / sldNum / hdr); if none
       is family-compatible, take the first only when the slide's @idx is
       explicit (reported as a type mismatch), else go on to 2.
    2. exact type (canonical: idx 0 preferred), then same family.
    3. orphan.  Returns (Ph or None, how)."""
    fam = lambda t: master_type(t)
    by_idx = [x for x in lph if x.idx == sp.idx]
    if len(by_idx) > 1:
        ex('match.layout.idx_duplicate(idx=%d)' % sp.idx, (sp.type, [x.type for x in by_idx]))
    same = [x for x in by_idx if x.type == sp.type]
    if same:
        return same[0], 'idx' if len(by_idx) == 1 else 'idx+type'
    famc = [x for x in by_idx if fam(x.type) == fam(sp.type)]
    if famc:
        return famc[0], 'idx(family)' if len(by_idx) == 1 else 'idx+family'
    if by_idx and sp.has_idx:
        ex('match.layout.explicit_idx_family_mismatch', (sp.type, by_idx[0].type))
        return by_idx[0], 'idx(type mismatch)'
    if by_idx:
        ex('match.layout.implicit_idx_family_mismatch->type', (sp.type, [x.type for x in by_idx]))
    by_type = [x for x in lph if x.type == sp.type]
    if by_type:
        canon = [x for x in by_type if x.idx == 0] or by_type
        return canon[0], 'type'
    by_fam = [x for x in lph if fam(x.type) == fam(sp.type)]
    if by_fam:
        canon = [x for x in by_fam if x.idx == 0] or by_fam
        return canon[0], 'family'
    return None, 'orphan'


def poi_match(sp, lph):
    if sp.has_idx:
        x = [l for l in lph if l.has_idx and l.idx == sp.idx]
        if x:
            return x[-1]          # HashMap put: last one wins
    if sp.has_type:
        by = [l for l in lph if l.has_type and l.type == sp.type]
        canon = [l for l in by if not l.has_idx or l.idx == 0]
        return (canon or by or [None])[0]
    return None


def ppy_match(sp, lph):
    top = [l for l in lph if not l.in_group]
    x = [l for l in top if l.idx == sp.idx]
    return x[0] if x else None


def master_match(t, mph):
    mt = master_type(t)
    x = [m for m in mph if m.type == mt]
    if len(x) > 1:
        ex('match.master.type_duplicate', mt)
    return x[0] if x else None


def xfrm(shape):
    if shape is None:
        return None
    if shape.tag == q('p:graphicFrame'):
        return shape.find(q('p:xfrm'))
    sppr = shape.find(q('p:spPr'))
    return sppr.find(q('a:xfrm')) if sppr is not None else None


def lst_lvl(lst, lvl):
    return lst.find(q('a:lvl%dpPr' % (lvl + 1))) if lst is not None else None


def body_of(shape):
    return shape.find(q('p:txBody')) if shape is not None else None


def lst_of(shape):
    b = body_of(shape)
    return b.find(q('a:lstStyle')) if b is not None else None


def defrpr(ppr):
    return ppr.find(q('a:defRPr')) if ppr is not None else None


def rget(e, prop):
    """Value of a run property on an rPr-like element, or None."""
    if e is None:
        return None
    if prop in ('sz', 'b'):
        return e.get(prop)
    if prop == 'latin':
        l = e.find(q('a:latin'))
        return l.get('typeface') if l is not None else None
    if prop == 'fill':
        for c in e:
            if c.tag in FILLS:
                if c.tag == q('a:solidFill') and len(c):
                    k = c[0]
                    return 'solid:%s:%s' % (qn(k.tag), k.get('val') or k.get('lastClr') or '')
                return qn(c.tag)
        return None


def pget(e, prop):
    if e is None:
        return None
    if prop in ('algn', 'marL'):
        return e.get(prop)
    if prop == 'bullet':
        for c in e:
            if c.tag in BULLETS:
                return qn(c.tag)
        return None


def first(chain, getter, prop):
    for name, e in chain:
        v = getter(e, prop)
        if v is not None:
            return v, name
    return None, None


def clr_map(sroot, lroot, mroot):
    """Effective clrMap for a slide: slide overrideClrMapping, else layout
    overrideClrMapping, else master clrMap (counted separately)."""
    for who, root in (('slide', sroot), ('layout', lroot)):
        if root is None:
            continue
        o = root.find(q('p:clrMapOvr'))
        if o is not None:
            ov = o.find(q('a:overrideClrMapping'))
            if ov is not None:
                return dict(ov.attrib), who
    m = mroot.find(q('p:clrMap')) if mroot is not None else None
    return (dict(m.attrib) if m is not None else {}), 'master'


def theme_color(theme, name):
    if theme is None:
        return None
    cs = theme.find('.//' + q('a:clrScheme'))
    if cs is None:
        return None
    e = cs.find(q('a:' + name))
    if e is None or not len(e):
        return None
    c = e[0]
    return c.get('val') if c.tag == q('a:srgbClr') else c.get('lastClr') if c.tag == q('a:sysClr') else qn(c.tag)


def theme_font(theme, ref):
    # +mj-lt / +mn-ea ...
    if theme is None:
        return None
    grp = 'majorFont' if ref.startswith('+mj') else 'minorFont'
    script = {'lt': 'latin', 'ea': 'ea', 'cs': 'cs'}.get(ref[-2:])
    f = theme.find('.//' + q('a:fontScheme') + '/' + q('a:' + grp))
    e = f.find(q('a:' + script)) if f is not None and script else None
    return e.get('typeface') if e is not None else None


def main(argv):
    geo_out = None
    if '--geometry' in argv:
        i = argv.index('--geometry'); geo_out = argv[i + 1]; del argv[i:i + 2]
    geo = []
    for path in iter_files(argv):
        try:
            pk = Pkg(path)
        except Exception:
            C['file.unreadable'] += 1; continue
        if pk.pres is None or pk.xml(pk.pres) is None:
            C['file.no_presentation'] += 1; continue
        C['file.ok'] += 1
        pres = pk.xml(pk.pres)
        dts = pres.find(q('p:defaultTextStyle'))
        fname = os.path.basename(path)
        for lst in pres.iter(q('a:defPPr')):
            if any(k not in PROOF for k in lst.attrib) or any(
                    c.tag != q('a:defRPr') or any(k not in PROOF for k in c.attrib) or len(c) for c in lst):
                ex('lststyle.defPPr_with_formatting(presentation)', fname)
        seen_masters = set()
        for sid, spart in pk.slides():
            if spart is None:
                C['slide.dangling'] += 1; continue
            C['slide'] += 1
            sroot = pk.xml(spart)
            lay, mst, thm = pk.chain(spart)
            lroot, mroot, troot = (pk.xml(x) if x else None for x in (lay, mst, thm))
            if lroot is None:
                ex('chain.no_layout', (fname, spart)); continue
            if mroot is None:
                ex('chain.no_master', (fname, lay)); continue
            if troot is None:
                ex('chain.no_theme', (fname, mst))
            C['chain.complete'] += 1
            if mst not in seen_masters:
                seen_masters.add(mst)
                C['master'] += 1
                txs = mroot.find(q('p:txStyles'))
                for st in ('titleStyle', 'bodyStyle', 'otherStyle'):
                    if txs is None or txs.find(q('p:' + st)) is None:
                        ex('master.missing_' + st, fname)
                for lst in mroot.iter(q('a:defPPr')):
                    if any(k not in PROOF for k in lst.attrib):
                        ex('lststyle.defPPr_with_formatting(master)', fname)
            cmap, cmap_src = clr_map(sroot, lroot, mroot)
            C['clrmap.from_' + cmap_src] += 1
            mo = sroot.find(q('p:clrMapOvr'))
            C['clrmap.slide_has_clrMapOvr' if mo is not None else 'clrmap.slide_without_clrMapOvr'] += 1
            lo = lroot.find(q('p:clrMapOvr'))
            if lo is not None and lo.find(q('a:overrideClrMapping')) is not None:
                ex('clrmap.layout_override', (fname, lay))
            lph, mph = phs(lroot), phs(mroot)
            for l in lph:
                if l.in_group:
                    ex('ph.layout_in_group', (fname, lay))
            txs = mroot.find(q('p:txStyles'))
            for shape, depth, in_group in walk_shapes(sptree(sroot)):
                ph = ph_of(shape)
                body = body_of(shape)
                if ph is not None:
                    sp = Ph(shape, in_group)
                    C['ph.slide'] += 1
                    if in_group:
                        ex('ph.slide_in_group', (fname, spart))
                    C['ph.slide.on_' + qn(shape.tag)] += 1
                    lp, how = match_layout(sp, lph)
                    C['match.layout.' + how] += 1
                    if how.startswith('idx') and lp.type != sp.type:
                        ex('match.layout.idx_type_differs:%s->%s' % (sp.type, lp.type), fname)
                    pm, ym = poi_match(sp, lph), ppy_match(sp, lph)
                    desc = lambda x: None if x is None else '%s/%s' % (x.type, x.idx)
                    if pm is not lp:
                        ex('match.differs_from_poi[ours=%s poi=%s slide has idx=%s type=%s]' % (
                            how, 'none' if pm is None else 'other', sp.has_idx, sp.has_type), (fname, spart, desc(sp), desc(lp), desc(pm)))
                    if ym is not lp:
                        ex('match.differs_from_python_pptx[ours=%s ppy=%s]' % (how, 'none' if ym is None else 'other'),
                           (fname, spart, desc(sp), desc(lp), desc(ym)))
                    mt_src = lp.type if lp is not None else sp.type
                    mp = master_match(mt_src, mph)
                    if mp is None:
                        ex('match.master.none:%s' % master_type(mt_src), fname)
                    else:
                        C['match.master.by_type'] += 1
                    # geometry
                    src = None
                    for who, s in (('slide', shape), ('layout', lp.shape if lp else None), ('master', mp.shape if mp else None)):
                        x = xfrm(s)
                        if x is not None:
                            off, ext = x.find(q('a:off')), x.find(q('a:ext'))
                            if off is None or ext is None:
                                ex('geometry.partial_xfrm(%s)' % who, (fname, spart))
                                continue
                            src = who
                            geo.append({'file': fname, 'part': spart, 'sldId': sid, 'id': shape.find('.//' + q('p:cNvPr')).get('id'),
                                        'x': int(off.get('x')), 'y': int(off.get('y')), 'cx': int(ext.get('cx')),
                                        'cy': int(ext.get('cy')), 'source': who})
                            break
                    C['geometry.from_%s' % src] += 1
                    if src is None:
                        geo.append({'file': fname, 'part': spart, 'sldId': sid, 'id': shape.find('.//' + q('p:cNvPr')).get('id'), 'source': None})
                    if body is None:
                        continue
                    # body properties
                    chainb = [('slide', body.find(q('a:bodyPr')))]
                    for who, s in (('layout', lp.shape if lp else None), ('master', mp.shape if mp else None)):
                        b = body_of(s)
                        chainb.append((who, b.find(q('a:bodyPr')) if b is not None else None))
                    v, w = first(chainb, lambda e, p: e.get(p) if e is not None else None, 'anchor')
                    C['bodyPr.anchor.from_%s' % w] += 1
                    cat = 'titleStyle' if sp.type in TITLE else 'otherStyle' if sp.type in OTHER_POI else 'bodyStyle'
                    if sp.type in ('hdr', 'sldImg'):
                        cat = 'otherStyle'
                    level_sources = lambda lvl: [
                        ('shape.lstStyle', lst_lvl(lst_of(shape), lvl)),
                        ('layout.ph.lstStyle', lst_lvl(lst_of(lp.shape), lvl) if lp else None),
                        ('master.ph.lstStyle', lst_lvl(lst_of(mp.shape), lvl) if mp else None),
                        ('master.txStyles.' + cat, lst_lvl(txs.find(q('p:' + cat)), lvl) if txs is not None else None),
                        ('presentation.defaultTextStyle', lst_lvl(dts, lvl)),
                    ]
                    alt = None
                    if sp.type in OTHER_POI:  # LibreOffice: no txStyles for dt/ftr/sldNum, uses defaultTextStyle
                        alt = lambda lvl: [x for x in level_sources(lvl) if not x[0].startswith('master.txStyles')]
                    kind = 'ph'
                else:
                    sp = None
                    style = shape.find(q('p:style'))
                    fref = style.find(q('a:fontRef')) if style is not None else None
                    if body is None:
                        continue
                    C['nonph.text_shape'] += 1
                    if fref is not None:
                        C['nonph.has_fontRef'] += 1

                    def level_sources(lvl, fref=fref, shape=shape):
                        out = [('shape.lstStyle', lst_lvl(lst_of(shape), lvl))]
                        if fref is not None:
                            out.append(('shape.style.fontRef', fref))
                        out += [('presentation.defaultTextStyle', lst_lvl(dts, lvl)),
                                ('master.txStyles.otherStyle', lst_lvl(txs.find(q('p:otherStyle')), lvl) if txs is not None else None)]
                        return out
                    alt = None
                    kind = 'nonph'
                for p in body.findall(q('a:p')):
                    ppr = p.find(q('a:pPr'))
                    lvl = int(ppr.get('lvl', '0')) if ppr is not None else 0
                    if lvl:
                        C['para.level>0'] += 1
                    levs = level_sources(lvl)
                    pchain = [('paragraph', ppr)] + [(n, e if n != 'shape.style.fontRef' else None) for n, e in levs]
                    for prop in ('algn', 'marL', 'bullet'):
                        v, w = first(pchain, pget, prop)
                        C['%s.para.%s.from_%s' % (kind, prop, w)] += 1
                    if ppr is not None and defrpr(ppr) is not None and any(k not in PROOF for k in defrpr(ppr).attrib):
                        ex('para.pPr_defRPr_with_formatting', fname)
                    # txStyles level fall-back (POI walks lvl, lvl-1, ...)
                    if kind == 'ph' and txs is not None and lvl:
                        st = txs.find(q('p:' + cat))
                        if st is not None and lst_lvl(st, lvl) is None and any(lst_lvl(st, k) is not None for k in range(lvl)):
                            ex('txStyles.level_missing_lower_present', fname)
                    for r in [c for c in p if c.tag in (q('a:r'), q('a:fld'))]:
                        C[kind + '.runs'] += 1
                        rpr = r.find(q('a:rPr'))

                        def mk(levs):
                            ch = [('run', rpr), ('paragraph.defRPr', defrpr(ppr))]
                            for n, e in levs:
                                ch.append((n, e if n == 'shape.style.fontRef' else defrpr(e)))
                            return ch
                        ch = mk(levs)
                        for prop in ('sz', 'b', 'latin', 'fill'):
                            getter = rget
                            if prop in ('sz', 'b'):
                                ch2 = [(n, None if n == 'shape.style.fontRef' else e) for n, e in ch]
                            elif prop == 'latin':
                                ch2 = [(n, e) for n, e in ch]
                                getter = lambda e, p: (('+mj-lt' if e.get('idx') == 'major' else '+mn-lt' if e.get('idx') == 'minor' else None)
                                                       if e is not None and e.tag == q('a:fontRef') else rget(e, p))
                            else:
                                getter = lambda e, p: (('solid:%s:%s' % (qn(e[0].tag), e[0].get('val') or '') if len(e) else None)
                                                       if e is not None and e.tag == q('a:fontRef') else rget(e, p))
                                ch2 = ch
                            v, w = first(ch2, getter, prop)
                            C['%s.run.%s.from_%s' % (kind, prop, w)] += 1
                            # contested steps: would the value change without them?
                            if w == 'paragraph.defRPr':
                                v2, _ = first([x for x in ch2 if x[0] != 'paragraph.defRPr'], getter, prop)
                                if v2 != v:
                                    ex('decides.paragraph_defRPr.%s' % prop, fname)
                            if w == 'shape.style.fontRef':
                                v2, _ = first([x for x in ch2 if x[0] != 'shape.style.fontRef'], getter, prop)
                                if v2 != v:
                                    ex('decides.fontRef.%s' % prop, fname)
                            if kind == 'nonph' and w in ('presentation.defaultTextStyle', 'master.txStyles.otherStyle', None):
                                # Q6: POI = dts then otherStyle; LO = dts only; ECMA note = bodyStyle
                                v_lo, _ = first([x for x in ch2 if x[0] != 'master.txStyles.otherStyle'], getter, prop)
                                v_other_first, _ = first([x for x in ch2 if x[0] != 'presentation.defaultTextStyle'] +
                                                         [x for x in ch2 if x[0] == 'presentation.defaultTextStyle'], getter, prop)
                                body_st = txs.find(q('p:bodyStyle')) if txs is not None else None
                                v_body, _ = first([x for x in ch2 if not x[0].startswith(('presentation', 'master'))] +
                                                  [('bodyStyle', defrpr(lst_lvl(body_st, lvl)))], getter, prop)
                                C['q6.%s.reached' % prop] += 1
                                if v_lo != v:
                                    ex('q6.%s.poi_vs_libreoffice_differ' % prop, (fname, v, v_lo))
                                if v_other_first != v:
                                    ex('q6.%s.order_dts_vs_other_differ' % prop, (fname, v, v_other_first))
                                if v_body != v:
                                    ex('q6.%s.poi_vs_ecma_note_bodyStyle_differ' % prop, (fname, v, v_body))
                            if kind == 'ph' and alt is not None and w and w.startswith('master.txStyles'):
                                v_lo, _ = first(mk(alt(lvl)), getter, prop)
                                C['dtftrnum.%s.reached_txStyles' % prop] += 1
                                if v_lo != v:
                                    ex('dtftrnum.%s.poi_vs_libreoffice_differ' % prop, (fname, v, v_lo))
                            if v is None:
                                continue
                            if prop == 'latin' and v.startswith('+'):
                                C['theme.font_ref'] += 1
                                if not theme_font(troot, v):
                                    ex('theme.font_ref_unresolved', (fname, v))
                            if prop == 'fill' and v.startswith('solid:a:schemeClr:'):
                                name = v.split(':')[-1]
                                C['color.schemeClr.%s' % ('mapped(bg/tx)' if name in ('bg1', 'bg2', 'tx1', 'tx2') else 'phClr' if name == 'phClr' else 'direct')] += 1
                                if name == 'phClr':
                                    continue
                                tname = cmap.get(name, name) if name in ('bg1', 'bg2', 'tx1', 'tx2', 'accent1', 'accent2', 'accent3', 'accent4', 'accent5', 'accent6', 'hlink', 'folHlink') else name
                                if theme_color(troot, tname) is None:
                                    ex('color.unresolved', (fname, name, tname))
                                else:
                                    C['color.resolved'] += 1
        # notes slides
        for sid, spart in pk.slides():
            if spart is None:
                continue
            notes = pk.rel_targets(spart, 'http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide')
            if not notes:
                continue
            nroot = pk.xml(notes[0])
            nm = pk.one(notes[0], 'http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesMaster')
            nmroot = pk.xml(nm) if nm else None
            C['notes.slide'] += 1
            if nmroot is None:
                ex('notes.no_notes_master', fname); continue
            nmph = phs(nmroot)
            ns = nmroot.find(q('p:notesStyle'))
            for s, _d, g in walk_shapes(sptree(nroot)):
                ph = ph_of(s)
                if ph is None or ph.get('type', 'obj') != 'body':
                    continue
                C['notes.body_ph'] += 1
                m = [x for x in nmph if x.type == 'body']
                if not m:
                    ex('notes.body_ph_without_master_body', fname); continue
                body = body_of(s)
                for p in (body.findall(q('a:p')) if body is not None else []):
                    ppr = p.find(q('a:pPr'))
                    lvl = int(ppr.get('lvl', '0')) if ppr is not None else 0
                    for r in p.findall(q('a:r')):
                        ch = [('run', r.find(q('a:rPr'))), ('paragraph.defRPr', defrpr(ppr)),
                              ('shape.lstStyle', defrpr(lst_lvl(lst_of(s), lvl))),
                              ('notesMaster.ph.lstStyle', defrpr(lst_lvl(lst_of(m[0].shape), lvl))),
                              ('notesMaster.notesStyle', defrpr(lst_lvl(ns, lvl))),
                              ('presentation.defaultTextStyle', defrpr(lst_lvl(dts, lvl)))]
                        v, w = first(ch, rget, 'sz')
                        C['notes.run.sz.from_%s' % w] += 1
    for k in sorted(C):
        print('%-62s %d' % (k, C[k]))
        for e in EX.get(k, []):
            print('      e.g. %s' % (e,))
    if geo_out:
        json.dump(geo, open(geo_out, 'w'), indent=0)


if __name__ == '__main__':
    main(sys.argv[1:])
