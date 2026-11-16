"""ADR-0011 evidence: what Phase2's additions would have to create in each writable file, and a
prototype of the allocation rules (the registries of ADR-0011 §2) checked for freshness.

Usage: python3 -I needs.py [--json out.json] <dir or file>...

For every file that passes the Phase0 gates:
  parts     would a first list need a new numbering part, a first style a new styles part, a
            new header a new header part (always), an image a new content-type Default (png,
            jpeg), updateFields a settings part?
  alloc     prototype allocations, then a check that each is absent from every place it must be
            absent from:
              rId        rId{n} with n = 1 + max numeric suffix of ids of shape rIdN (any case),
                         skipping any Id equal ignoring case (ids like R44c8… are kept apart)
              docPr      1 + max wp:docPr/@id over all story parts, both AC branches included
              annotation 1 + max w:id over bookmarks, comments, revisions, permissions in all
                         story parts and comments.xml
              numId      1 + max numId; abstractNumId 1 + max abstractNumId
              paraId     random < 0x80000000 not present in any part (incl. commentsExtended)
              part name  word/media/image{n}.{ext}, word/header{n}.xml: smallest n >= 1 whose
                         name is free, compared ignoring ASCII case
            and the headroom: how far the allocated values are from 2^31.
"""
import collections
import json
import os
import random
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import p2common as C  # noqa: E402
from registries import rels_of, WP, W14, REVISION_ID_ELEMENTS, BOOKMARK, COMMENT, PERM, CT_NS  # noqa: E402
import xml.etree.ElementTree as ET  # noqa: E402

w, local = C.w, C.local


def main(argv):
    out_json, args = None, []
    it = iter(argv)
    for a in it:
        if a == '--json':
            out_json = next(it)
        else:
            args.append(a)
    S = collections.Counter()
    hr = collections.Counter()
    rnd = random.Random(7)
    for f in C.corpus_files(args):
        pkg = C.Pkg(f)
        if C.gate(pkg)[0]:
            continue
        S['writable files'] += 1
        stories = pkg.stories()
        # ---- parts that would be needed
        S['has numbering part'] += pkg.part('numbering') is not None
        S['has styles part'] += pkg.part('styles') is not None
        S['has settings part'] += pkg.part('settings') is not None
        S['has footnotes part'] += pkg.part('footnotes') is not None
        S['has a header part'] += any(k == 'header' for k, _, _ in stories)
        ct = ET.fromstring(pkg.z.read('[Content_Types].xml'))
        dflt = {(d.get('Extension') or '').lower() for d in ct.findall('{%s}Default' % CT_NS)}
        S['has png Default'] += 'png' in dflt
        S['has jpeg or jpg Default'] += bool({'jpeg', 'jpg'} & dflt)
        S['has xml Default'] += 'xml' in dflt
        overrides = {o.get('PartName').lower() for o in ct.findall('{%s}Override' % CT_NS)}
        names_lower = {n.lower() for n in pkg.names}
        # ---- rId on the main part
        ids = [i for i, _, _ in rels_of(pkg, pkg.main, external=True)]
        nums = [int(i[3:]) for i in ids if i and re.fullmatch(r'(?i)rid\d+', i)]
        n = (max(nums) if nums else 0) + 1
        while ('rid%d' % n) in {i.lower() for i in ids}:
            n += 1
        if ('rId%d' % n).lower() in {i.lower() for i in ids}:
            S['FAIL rId not fresh'] += 1
        hr['rId n'] = max(hr['rId n'], n)
        if len(nums) != len(ids):
            S['main rels with non-rIdN ids'] += 1
        # ---- docPr
        dp = [int(e.get('id')) for _, _, r in stories for e in r.iter('{%s}docPr' % WP)
              if (e.get('id') or '').isdigit()]
        new_dp = (max(dp) if dp else 0) + 1
        if new_dp in dp:
            S['FAIL docPr not fresh'] += 1
        hr['docPr'] = max(hr['docPr'], new_dp)
        # ---- annotation ids (union)
        ann = []
        for kind, _, r in stories:
            for el in r.iter():
                if not isinstance(el.tag, str) or C.ns(el.tag) != C.W:
                    continue
                v = el.get(w('id'))
                ln = local(el.tag)
                if v is not None and v.lstrip('-').isdigit() and (
                        ln in BOOKMARK | COMMENT | PERM | REVISION_ID_ELEMENTS or (kind == 'comments' and ln == 'comment')):
                    ann.append(int(v))
        new_ann = max(ann + [-1]) + 1
        hr['annotation'] = max(hr['annotation'], new_ann)
        if new_ann in ann:
            S['FAIL annotation not fresh'] += 1
        # ---- numbering
        nump = pkg.part('numbering')
        if nump is not None:
            nr = pkg.xml(nump)
            ni = [int(c.get(w('numId'))) for c in nr if c.tag == w('num') and (c.get(w('numId')) or '').isdigit()]
            ai = [int(c.get(w('abstractNumId'))) for c in nr if c.tag == w('abstractNum') and (c.get(w('abstractNumId')) or '').isdigit()]
            hr['numId'] = max(hr['numId'], max(ni + [0]) + 1)
            hr['abstractNumId'] = max(hr['abstractNumId'], max(ai + [-1]) + 1)
        # ---- paraId: random values absent everywhere (all XML parts, not only stories)
        seen = set()
        for nme in pkg.names:
            if nme.endswith('.xml'):
                try:
                    root = pkg.xml(nme)
                except ET.ParseError:
                    continue
                for el in root.iter():
                    for at in ('{%s}paraId' % W14, '{%s}textId' % W14,
                               '{http://schemas.microsoft.com/office/word/2012/wordml}paraId',
                               '{http://schemas.microsoft.com/office/word/2012/wordml}paraIdParent'):
                        v = el.get(at)
                        if v:
                            seen.add(v.upper())
        tries = 0
        while True:
            tries += 1
            v = '%08X' % rnd.randrange(1, 0x80000000)
            if v not in seen:
                break
        hr['paraId tries (max)'] = max(hr['paraId tries (max)'], tries)
        S['paraId values present (sum)'] += len(seen)
        # ---- part names
        for base, ext in (('word/media/image', 'png'), ('word/header', 'xml')):
            k = 1
            while ('%s%d.%s' % (base, k, ext)).lower() in names_lower:
                k += 1
            if ('/%s%d.%s' % (base, k, ext)).lower() in overrides:
                S['FAIL part name has a dangling Override (%s)' % base] += 1
            hr['next %s n' % base.rsplit('/', 1)[1]] = max(hr['next %s n' % base.rsplit('/', 1)[1]], k)
    rep = {'counts': dict(S), 'max allocated (headroom vs 2^31)': dict(hr)}
    print(json.dumps(rep, indent=1))
    if out_json:
        with open(out_json, 'w') as fh:
            json.dump(rep, fh, indent=1)


if __name__ == '__main__':
    main(sys.argv[1:])
