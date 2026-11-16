"""ADR-0011 revision 2 evidence: the facts behind the round-1 review findings, measured in this
spike rather than taken from the reviewer's probe.

Usage: python3 -I review1.py [--json out.json] <dir or file>...

All packages that open are measured; "files" = packages with at least one occurrence.
  M1  sectPr carrying restart settings that a section break would duplicate:
      pgNumType/@w:start, lnNumType/@w:restart, footnotePr|endnotePr/w:numRestart.
  M2  a:graphicFrameLocks: declares xmlns:a itself / relies on an ancestor.
  M3  references to ids that are not defined: pStyle/rStyle/tblStyle/basedOn/next/link to a
      styleId absent from styles.xml; numPr/numId absent from numbering.xml; r:* attribute
      values absent from the source part's relationships; internal relationship targets
      that are not parts (dangling).
  M4  style names whose ASCII-alphanumeric stripping is empty or starts with a digit; style
      ids of Word's localized form a / aN (a + digits / a + letters a-z).
  M5  table cells with gridSpan > 1 / vMerge / hMerge (the attributes a column copy would carry).
  M6  table rows containing w:footnoteReference/w:endnoteReference; rows containing revision
      elements (run-level ins/del/moveFrom/moveTo, mark ins/del, *PrChange).
  m1  drawing ids other than wp:docPr: every */cNvPr/@id in story parts by namespace.
  m2  w:sdtPr/w:id (random, can be negative); non-numeric permStart ids.
  m3  numIdMacAtCleanup >= max numId.
  m7  w:trPr/w:tblHeader.
"""
import collections
import json
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import p2common as C  # noqa: E402
from registries import rels_of  # noqa: E402

w, local = C.w, C.local
A = 'http://schemas.openxmlformats.org/drawingml/2006/main'
WP = 'http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing'
R = C.R
REV = {'ins', 'del', 'moveFrom', 'moveTo', 'rPrChange', 'pPrChange', 'tcPrChange', 'trPrChange', 'cellIns',
       'cellDel', 'cellMerge'}
STYLE_REFS = ('pStyle', 'rStyle', 'tblStyle', 'basedOn', 'next', 'link')


def declares(raw, tag_prefix, ns_uri):
    """Count start tags `<a:graphicFrameLocks` that declare xmlns:a themselves (text scan of
    the part; ElementTree drops namespace declarations)."""
    yes = no = 0
    for m in re.finditer(r'<%s\b[^>]*>' % re.escape(tag_prefix), raw):
        if ('xmlns:a="%s"' % ns_uri) in m.group(0):
            yes += 1
        else:
            no += 1
    return yes, no


def main(argv):
    out_json, args = None, []
    it = iter(argv)
    for a in it:
        if a == '--json':
            out_json = next(it)
        else:
            args.append(a)
    S = collections.Counter()
    F = collections.defaultdict(set)

    def note(k, f, n=1):
        S[k] += n
        F[k].add(f)

    for f in C.corpus_files(args):
        pkg = C.Pkg(f)
        if pkg.err:
            continue
        stories = pkg.stories()
        if pkg.err:
            continue
        fn = pkg.name
        S['files opened'] += 1
        # M1
        for sp in pkg.doc.iter(w('sectPr')):
            pg = sp.find(w('pgNumType'))
            if pg is not None and pg.get(w('start')) is not None:
                note('M1 sectPr with pgNumType/@start', fn)
            ln = sp.find(w('lnNumType'))
            if ln is not None and ln.get(w('restart')) not in (None, 'continuous'):
                note('M1 sectPr with lnNumType restart', fn)
            for k in ('footnotePr', 'endnotePr'):
                e = sp.find(w(k))
                if e is not None and e.find(w('numRestart')) is not None:
                    note('M1 sectPr with %s/numRestart' % k, fn)
        # M2
        for kind, part, root in stories:
            raw = pkg.z.read(part).decode('utf-8', 'replace')
            y, n = declares(raw, 'a:graphicFrameLocks', A)
            if y:
                note('M2 a:graphicFrameLocks declaring xmlns:a', fn, y)
            if n:
                note('M2 a:graphicFrameLocks without its own xmlns:a', fn, n)
        # M3 styles
        sp = pkg.part('styles')
        sids = set()
        if sp is not None:
            sr = pkg.xml(sp)
            sids = {s.get(w('styleId')) for s in sr.findall(w('style'))}
            for s in sr.findall(w('style')):
                sid = s.get(w('styleId')) or ''
                if re.fullmatch(r'a[0-9a-z]?\d*', sid):
                    note('M4 styleId of form a / aN', fn)
                nm = s.find(w('name'))
                name = nm.get(w('val')) if nm is not None else ''
                st = re.sub(r'[^A-Za-z0-9]', '', name or '')
                if name and not st:
                    note('M4 w:name strips to empty (ASCII alphanumerics)', fn)
                elif st[:1].isdigit():
                    note('M4 w:name strips to a leading digit', fn)
        refparts = [(k, p, r) for k, p, r in stories]
        if sp is not None:
            refparts.append(('styles', sp, pkg.xml(sp)))
        nump = pkg.part('numbering')
        nids = set()
        if nump is not None:
            nr = pkg.xml(nump)
            nids = {n.get(w('numId')) for n in nr.findall(w('num'))}
            refparts.append(('numbering', nump, nr))
            mac = nr.find(w('numIdMacAtCleanup'))
            if mac is not None:
                ints = [int(x) for x in nids if x and x.isdigit()]
                v = mac.get(w('val'))
                if ints and v and v.isdigit() and int(v) >= max(ints):
                    note('m3 numIdMacAtCleanup >= max numId', fn)
        for k, p, r in refparts:
            for tag in STYLE_REFS:
                for e in r.iter(w(tag)):
                    v = e.get(w('val'))
                    if sp is not None and v not in sids:
                        note('M3 %s to an undefined styleId' % tag, fn)
            for e in r.iter(w('numId')):
                v = e.get(w('val'))
                if v not in (None, '0') and v not in nids:
                    note('M3 numId to an undefined num', fn)
        for kind, part, root in stories:
            ids = {i for i, _, _ in rels_of(pkg, part, external=True)}
            for el in root.iter():
                for at, v in el.attrib.items():
                    if at.startswith('{%s}' % R):
                        S['M3 r:* attributes'] += 1
                        if v not in ids:
                            note('M3 r:* attribute to an undefined relationship', fn)
        for rp in [n for n in pkg.names if n.endswith('.rels')]:
            src = rp.replace('_rels/', '')[:-5] if rp != '_rels/.rels' else None
            for _, t, tgt in rels_of(pkg, src):
                if tgt not in pkg.names:
                    note('M3 dangling internal relationship target', fn)
        # M5, M6, m7
        for kind, part, root in stories:
            for tc in root.iter(w('tc')):
                tp = tc.find(w('tcPr'))
                if tp is None:
                    continue
                g = tp.find(w('gridSpan'))
                if g is not None and g.get(w('val'), '1') != '1':
                    note('M5 cells with gridSpan > 1', fn)
                if tp.find(w('vMerge')) is not None:
                    note('M5 cells with vMerge', fn)
                if tp.find(w('hMerge')) is not None:
                    note('M5 cells with hMerge', fn)
            for tr in root.iter(w('tr')):
                inner = [e for e in tr.iter() if e is not tr]
                if any(e.tag in (w('footnoteReference'), w('endnoteReference')) for e in inner):
                    note('M6 rows containing a footnote/endnote reference', fn)
                if any(isinstance(e.tag, str) and C.ns(e.tag) == C.W and local(e.tag) in REV for e in inner):
                    note('M6 rows containing revision elements', fn)
                trpr = tr.find(w('trPr'))
                if trpr is not None and trpr.find(w('tblHeader')) is not None:
                    note('m7 rows with trPr/tblHeader', fn)
        # m1, m2
        for kind, part, root in stories:
            for el in root.iter():
                if not isinstance(el.tag, str):
                    continue
                if local(el.tag) == 'cNvPr' and el.get('id') is not None:
                    ns = C.ns(el.tag).rsplit('/', 1)[-1]
                    note('m1 cNvPr/@id in namespace .../%s' % ns, fn)
            for sdtpr in root.iter(w('sdtPr')):
                e = sdtpr.find(w('id'))
                if e is not None:
                    S['m2 sdtPr/w:id'] += 1
                    v = e.get(w('val')) or ''
                    if v.startswith('-'):
                        note('m2 negative sdtPr/w:id', fn)
            for ps in root.iter(w('permStart')):
                if not (ps.get(w('id')) or '').lstrip('-').isdigit():
                    note('m2 non-numeric permStart id', fn)
    rep = {'counts': dict(sorted(S.items())), 'files': {k: len(v) for k, v in sorted(F.items())}}
    print(json.dumps(rep, indent=1, ensure_ascii=False))
    if out_json:
        with open(out_json, 'w') as fh:
            json.dump(rep, fh, indent=1, ensure_ascii=False)


if __name__ == '__main__':
    main(sys.argv[1:])
