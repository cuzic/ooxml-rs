"""ADR-0011 evidence: ID spaces, registries, part naming and structure in real Word files.

Usage: python3 -I registries.py [--json out.json] [--examples N] <dir or file>...

Every package that opens (no encryption, DTD, Strict, depth > 256) is measured; the
counters "files with ..." are per package. Sections of the output:

 docpr       wp:docPr/@id (drawing object ids) over all story parts: duplicates within a part,
             across parts, and the ones explained by the two branches of one mc:AlternateContent;
             value range; relation to pic:cNvPr/@id.
 bookmarks   ids and names: duplicates, case-insensitive name duplicates, names over 40
             characters, hidden ('_') names.
 annotation  the union of w:id over bookmarks, comments (range, reference, comments part),
             revisions (ins/del/move*, *PrChange, numberingChange, ...) and permissions:
             cross-category collisions; which elements carry w:id at all.
 paraid      w14:paraId / w14:textId: duplicates, values >= 0x80000000, the most common textId.
 numbering   numbering.xml: num/abstractNum counts, duplicate ids, gaps, dangling num->abstractNum
             and numPr->num references, numId 0, nsid duplicates, order violations, overrides.
 rels        relationship ids: rIdN shape, duplicates, gaps, max; external targets by type;
             relationships whose Id no attribute of the source part mentions.
 ctypes      [Content_Types].xml: Defaults by extension, how media is registered, parts covered
             by neither, Overrides naming missing parts.
 media       image relationships: part names, sharing of one media part, linked images, svgBlip.
 parts       naming of headers/footers/media (nameN.ext), numbers vs counts, orphan parts.
 sections    sectPr count and types, headerReference inheritance, titlePg, evenAndOddHeaders.
 tables      grid consistency (gridCol count vs gridBefore + gridSpan + gridAfter per row),
             vMerge structure, hMerge, row-level revisions, w14:paraId on rows.
 hyperlinks  r:id vs anchor, relationship reuse, duplicate external targets, HYPERLINK fields.
 styles      stylesWithEffects part, style counts, latentStyles count attribute.
"""
import collections
import json
import os
import posixpath
import re
import sys
import xml.etree.ElementTree as ET

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import p2common as C  # noqa: E402

w, local = C.w, C.local
WP = 'http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing'
PIC = 'http://schemas.openxmlformats.org/drawingml/2006/picture'
A = 'http://schemas.openxmlformats.org/drawingml/2006/main'
W14 = 'http://schemas.microsoft.com/office/word/2010/wordml'
MC = C.MC
R = C.R
ASVG = 'http://schemas.microsoft.com/office/drawing/2016/SVG/main'
CT_NS = 'http://schemas.openxmlformats.org/package/2006/content-types'
REL_IMAGE = C.RT + 'image'
REL_HYPERLINK = C.RT + 'hyperlink'
REVISION_ID_ELEMENTS = {'ins', 'del', 'moveFrom', 'moveTo', 'rPrChange', 'pPrChange', 'sectPrChange',
                        'tblPrChange', 'trPrChange', 'tcPrChange', 'tblGridChange', 'numberingChange',
                        'cellIns', 'cellDel', 'cellMerge', 'moveFromRangeStart', 'moveFromRangeEnd',
                        'moveToRangeStart', 'moveToRangeEnd', 'customXmlInsRangeStart', 'customXmlInsRangeEnd',
                        'customXmlDelRangeStart', 'customXmlDelRangeEnd', 'customXmlMoveFromRangeStart',
                        'customXmlMoveFromRangeEnd', 'customXmlMoveToRangeStart', 'customXmlMoveToRangeEnd'}
BOOKMARK = {'bookmarkStart', 'bookmarkEnd'}
COMMENT = {'commentRangeStart', 'commentRangeEnd', 'commentReference'}
PERM = {'permStart', 'permEnd'}
NAMED = re.compile(r'^(.*?)(\d+)\.([A-Za-z0-9]+)$')


def parent_map(root):
    return {c: p for p in root.iter() for c in p}


def ac_branch(el, parent):
    """(id of the innermost mc:AlternateContent ancestor, branch index) or None."""
    a, child = parent.get(el), el
    while a is not None:
        if a.tag == '{%s}AlternateContent' % MC:
            return (id(a), list(a).index(child))
        child, a = a, parent.get(a)
    return None


def all_xml_parts(pkg):
    for n in sorted(pkg.names):
        if n.endswith('.xml') and not n.startswith('[') and '/_rels/' not in n:
            try:
                yield n, pkg.xml(n)
            except ET.ParseError:
                continue


def main(argv):
    out_json, nex, args = None, 3, []
    it = iter(argv)
    for a in it:
        if a == '--json':
            out_json = next(it)
        elif a == '--examples':
            nex = int(next(it))
        else:
            args.append(a)
    S = collections.defaultdict(collections.Counter)  # section -> counter
    files = collections.defaultdict(set)              # "files with X" -> set of file names
    ex = collections.defaultdict(list)

    def note(section, key, f, example=None, n=1):
        S[section][key] += n
        files[section + ': ' + key].add(f)
        if example is not None and len(ex[section + ': ' + key]) < nex:
            ex[section + ': ' + key].append(example)

    for f in C.corpus_files(args):
        pkg = C.Pkg(f)
        if pkg.err:
            S['files']['not opened: ' + pkg.err.split(':')[0]] += 1
            continue
        stories = pkg.stories()
        if pkg.err:
            S['files']['not opened: ' + pkg.err] += 1
            continue
        S['files']['opened'] += 1
        g, _ = C.gate(pkg)
        S['files']['writable (Phase0 gates pass)' if not g else 'gated: ' + g] += 1
        fn = pkg.name

        # ------------------------------------------------------------ docPr and cNvPr
        docpr = collections.defaultdict(list)   # id -> [(part, ac)]
        maxid = 0
        for kind, part, root in stories:
            par = parent_map(root)
            for el in root.iter('{%s}docPr' % WP):
                v = el.get('id')
                note('docpr', 'docPr elements', fn)
                if v is None or not v.lstrip('-').isdigit():
                    note('docpr', 'docPr id missing or not an integer', fn, (part, v))
                    continue
                iv = int(v)
                maxid = max(maxid, iv)
                if iv == 0:
                    note('docpr', 'docPr id 0', fn)
                if iv >= 2 ** 31:
                    note('docpr', 'docPr id >= 2^31', fn, (part, v))
                docpr[iv].append((part, ac_branch(el, par)))
            for el in root.iter('{%s}cNvPr' % PIC):
                v = el.get('id')
                note('docpr', 'pic:cNvPr elements', fn)
                if v == '0':
                    note('docpr', 'pic:cNvPr id 0', fn)
                gp = par.get(el)
                # pic:cNvPr id equal to the enclosing docPr id?
                a = par.get(el)
                while a is not None and a.tag not in ('{%s}inline' % WP, '{%s}anchor' % WP):
                    a = par.get(a)
                if a is not None:
                    d = a.find('{%s}docPr' % WP)
                    if d is not None:
                        note('docpr', 'pic:cNvPr id == docPr id' if d.get('id') == v else 'pic:cNvPr id != docPr id', fn)
        if maxid:
            S['docpr_max']['max <= 100' if maxid <= 100 else 'max <= 10^4' if maxid <= 10 ** 4 else
                           'max <= 2^31' if maxid < 2 ** 31 else 'max >= 2^31'] += 1
        for iv, uses in docpr.items():
            if len(uses) < 2:
                continue
            parts = {p for p, _ in uses}
            branches = {ac for _, ac in uses}
            if len(parts) == 1 and len(uses) == 2 and all(b is not None for b in branches) and \
                    len({b[0] for b in branches}) == 1 and len(branches) == 2:
                note('docpr', 'duplicate explained by the two branches of one AC', fn)
            elif len(parts) == 1:
                note('docpr', 'duplicate within one part (not AC branches)', fn, (parts.pop(), iv, len(uses)))
            else:
                note('docpr', 'duplicate across parts', fn, (sorted(parts), iv))

        # ------------------------------------------------------------ annotation ids, bookmarks
        cat_ids = collections.defaultdict(set)   # category -> ids
        bm_ids = collections.defaultdict(list)
        bm_names = collections.defaultdict(list)
        wid_carriers = collections.Counter()
        for kind, part, root in stories:
            for el in root.iter():
                if not isinstance(el.tag, str) or C.ns(el.tag) != C.W:
                    continue
                v = el.get(w('id'))
                if v is None:
                    continue
                ln = local(el.tag)
                wid_carriers[ln] += 1
                if ln in BOOKMARK:
                    cat_ids['bookmark'].add(v)
                    if ln == 'bookmarkStart':
                        bm_ids[v].append(part)
                        nm = el.get(w('name')) or ''
                        bm_names[nm].append(part)
                elif ln in COMMENT or (kind == 'comments' and ln == 'comment'):
                    cat_ids['comment'].add(v)
                elif ln in REVISION_ID_ELEMENTS:
                    cat_ids['revision'].add(v)
                elif ln in PERM:
                    cat_ids['perm'].add(v)
                elif ln in ('footnote', 'endnote', 'footnoteReference', 'endnoteReference'):
                    cat_ids['note:' + ln.replace('Reference', '')].add(v)
                else:
                    cat_ids['other:' + ln].add(v)
        for k, v in wid_carriers.items():
            S['w:id carriers'][k] += v
        cats = sorted(c for c in cat_ids if not c.startswith('note:'))
        for i, a in enumerate(cats):
            for b in cats[i + 1:]:
                if cat_ids[a] & cat_ids[b]:
                    note('annotation', 'id collision %s/%s' % (a, b), fn, sorted(cat_ids[a] & cat_ids[b])[:3])
        for c, ids in cat_ids.items():
            nums = [int(x) for x in ids if x.lstrip('-').isdigit()]
            if len(nums) != len(ids):
                note('annotation', 'non-integer w:id in ' + c, fn)
            if any(n < 0 for n in nums):
                note('annotation', 'negative w:id in ' + c, fn, sorted(n for n in nums if n < 0)[:3])
            if nums and max(nums) >= 2 ** 31:
                note('annotation', 'w:id >= 2^31 in ' + c, fn)
        for v, parts in bm_ids.items():
            if len(parts) > 1:
                note('bookmarks', 'duplicate bookmarkStart id within part' if len(set(parts)) == 1 else
                     'duplicate bookmarkStart id across parts', fn, (v, parts[:3]))
        lower = collections.Counter(n.lower() for n in bm_names)
        for nm, parts in bm_names.items():
            S['bookmarks']['bookmarkStart'] += len(parts)
            if len(parts) > 1:
                note('bookmarks', 'duplicate name (exact)', fn, nm)
            if lower[nm.lower()] > 1 and sum(1 for n in bm_names if n.lower() == nm.lower()) > 1:
                note('bookmarks', 'names equal ignoring case only', fn, nm)
            if len(nm) > 40:
                note('bookmarks', 'name longer than 40', fn, nm[:50])
            if nm.startswith('_'):
                S['bookmarks']['hidden (_) names'] += len(parts)
            if nm and not re.match(r'^[^\W\d]\w*$', nm):
                note('bookmarks', 'name not letter + word characters', fn, nm[:40])
            if nm == '':
                note('bookmarks', 'empty name', fn)
        # bookmark ids relative to max (is max+1 safe? are ids dense?)
        nums = sorted(int(x) for x in cat_ids['bookmark'] if x.lstrip('-').isdigit())
        if nums:
            S['bookmarks']['files with bookmarks'] += 1
            if nums[-1] >= 10 ** 6:
                note('bookmarks', 'max bookmark id >= 10^6', fn, nums[-1])

        # ------------------------------------------------------------ paraId / textId
        para = collections.defaultdict(list)
        textids = collections.Counter()
        for kind, part, root in stories:
            par = parent_map(root)
            for el in root.iter():
                if not isinstance(el.tag, str):
                    continue
                v = el.get('{%s}paraId' % W14)
                if v is not None:
                    note('paraid', 'paraId attributes', fn)
                    try:
                        if int(v, 16) >= 0x80000000:
                            note('paraid', 'paraId >= 0x80000000', fn, v)
                    except ValueError:
                        note('paraid', 'paraId not hex', fn, v)
                    para[v.upper()].append((part, ac_branch(el, par), local(el.tag)))
                t = el.get('{%s}textId' % W14)
                if t is not None:
                    textids[t.upper()] += 1
                    try:
                        if int(t, 16) >= 0x80000000:
                            note('paraid', 'textId >= 0x80000000', fn, t)
                    except ValueError:
                        note('paraid', 'textId not hex', fn, t)
        dup_files = False
        for v, uses in para.items():
            if len(uses) > 1:
                acs = {u[1] for u in uses}
                if None not in acs and len({a[0] for a in acs}) == 1 and len(acs) == len(uses):
                    note('paraid', 'duplicate paraId explained by AC branches', fn)
                else:
                    note('paraid', 'duplicate paraId (other)', fn, (v, [(u[0], u[2]) for u in uses][:3]))
                dup_files = True
        for t, n in textids.most_common(1):
            if n > 1:
                note('paraid', 'textId repeated (most common value)', fn, (t, n))
        if '77777777' in textids:
            note('paraid', 'textId 77777777 present', fn, None, textids['77777777'])
        # comments extended paraIds in other parts
        for n in pkg.names:
            if n.endswith('commentsExtended.xml'):
                note('paraid', 'commentsExtended parts', fn)

        # ------------------------------------------------------------ numbering
        nump = pkg.part('numbering')
        num_ids, abs_ids = set(), set()
        if nump is not None:
            S['numbering']['files with numbering part'] += 1
            nr = pkg.xml(nump)
            nums = [c for c in nr if c.tag == w('num')]
            abss = [c for c in nr if c.tag == w('abstractNum')]
            S['numbering']['num'] += len(nums)
            S['numbering']['abstractNum'] += len(abss)
            seen_num = False
            for c in nr:
                if c.tag == w('num'):
                    seen_num = True
                elif c.tag == w('abstractNum') and seen_num:
                    note('numbering', 'abstractNum after a num (schema order violated)', fn)
                    break
            nid = [c.get(w('numId')) for c in nums]
            aid = [c.get(w('abstractNumId')) for c in abss]
            num_ids, abs_ids = set(nid), set(aid)
            if len(nid) != len(num_ids):
                note('numbering', 'duplicate numId', fn)
            if len(aid) != len(abs_ids):
                note('numbering', 'duplicate abstractNumId', fn)
            ni = sorted(int(x) for x in num_ids if x and x.isdigit())
            ai = sorted(int(x) for x in abs_ids if x and x.isdigit())
            if ni and ni != list(range(ni[0], ni[0] + len(ni))):
                note('numbering', 'numId set has gaps', fn, (ni[:5], ni[-1]))
            if ni and ni[0] == 0:
                note('numbering', 'num with numId 0', fn)
            if ai and ai != list(range(ai[0], ai[0] + len(ai))):
                note('numbering', 'abstractNumId set has gaps', fn)
            if ni and ni[-1] > 10 ** 4:
                note('numbering', 'numId > 10^4', fn, ni[-1])
            used_abs = set()
            for c in nums:
                a = c.find(w('abstractNumId'))
                v = a.get(w('val')) if a is not None else None
                used_abs.add(v)
                if v not in abs_ids:
                    note('numbering', 'num -> undefined abstractNum', fn, (c.get(w('numId')), v))
                lo = c.findall(w('lvlOverride'))
                if lo:
                    note('numbering', 'num with lvlOverride', fn)
                    if any(x.find(w('startOverride')) is not None for x in lo):
                        note('numbering', 'num with startOverride (list restart)', fn)
            unref = abs_ids - used_abs
            if unref:
                note('numbering', 'abstractNum referenced by no num', fn, None, len(unref))
            nsids = [c.find(w('nsid')).get(w('val')) for c in abss if c.find(w('nsid')) is not None]
            S['numbering']['abstractNum with nsid'] += len(nsids)
            if len(nsids) != len(set(nsids)):
                note('numbering', 'duplicate nsid', fn)
            for c in abss:
                if c.find(w('numStyleLink')) is not None:
                    note('numbering', 'abstractNum with numStyleLink', fn)
                if c.find(w('styleLink')) is not None:
                    note('numbering', 'abstractNum with styleLink', fn)
                mlt = c.find(w('multiLevelType'))
                S['numbering', 'multiLevelType'][mlt.get(w('val')) if mlt is not None else '<absent>'] += 1
            if nr.find(w('numIdMacAtCleanup')) is not None:
                note('numbering', 'numIdMacAtCleanup present', fn)
            for c in nr:
                if c.tag not in (w('num'), w('abstractNum'), w('numIdMacAtCleanup')):
                    note('numbering', 'other child: ' + local(c.tag), fn)
        refparts = [(k, p, r) for k, p, r in stories]
        sp = pkg.part('styles')
        if sp is not None:
            refparts.append(('styles', sp, pkg.xml(sp)))
        for k, p, r in refparts:
            for npr in r.iter(w('numPr')):
                e = npr.find(w('numId'))
                if e is None:
                    continue
                v = e.get(w('val'))
                if v == '0':
                    note('numbering', 'numPr numId 0 (removes numbering) in ' + k, fn)
                elif nump is None:
                    note('numbering', 'numPr numId without numbering part', fn)
                elif v not in num_ids:
                    note('numbering', 'numPr -> undefined numId in ' + k, fn, v)
                else:
                    S['numbering']['numPr references (defined) in ' + k] += 1

        # ------------------------------------------------------------ rels
        rels_parts = [n for n in pkg.names if n.endswith('.rels')]
        for rp in rels_parts:
            try:
                rr = pkg.xml(rp)
            except ET.ParseError:
                note('rels', 'unparsable rels part', fn)
                continue
            ids = [r.get('Id') or '' for r in rr]
            S['rels']['relationships'] += len(ids)
            shaped = [i for i in ids if re.fullmatch(r'rId\d+', i)]
            S['rels']['Id of shape rIdN'] += len(shaped)
            for i in ids:
                if not re.fullmatch(r'rId\d+', i):
                    note('rels', 'Id not rIdN', fn, i[:30])
            if len({i.lower() for i in ids}) != len(ids):
                note('rels', 'duplicate Id (case-insensitive)', fn, rp)
            nums = sorted(int(i[3:]) for i in shaped)
            if nums and nums != list(range(1, len(nums) + 1)):
                note('rels', 'rIdN set is not 1..n', fn, (rp, nums[:6], nums[-1]))
            for r in rr:
                if r.get('TargetMode') == 'External':
                    note('rels', 'external: ' + (r.get('Type') or '').rsplit('/', 1)[-1], fn)
            # unreferenced relationships of a story part
            src = rp.replace('_rels/', '')[:-5]
            if src in {p for _, p, _ in stories}:
                try:
                    raw = pkg.z.read(src).decode('utf-8', 'replace')
                except KeyError:
                    continue
                for r in rr:
                    t = (r.get('Type') or '').rsplit('/', 1)[-1]
                    if t in ('image', 'hyperlink', 'header', 'footer', 'oleObject', 'chart', 'package') and \
                            ('"%s"' % r.get('Id')) not in raw:
                        note('rels', 'relationship of type %s not mentioned by its source part' % t, fn)

        # ------------------------------------------------------------ content types
        ctb = pkg.z.read('[Content_Types].xml')
        ct = ET.fromstring(ctb)
        defaults = {d.get('Extension'): d.get('ContentType') for d in ct.findall('{%s}Default' % CT_NS)}
        overrides = {o.get('PartName'): o.get('ContentType') for o in ct.findall('{%s}Override' % CT_NS)}
        for e in defaults:
            S['ctypes', 'Default extension'][e] += 1
            if e != e.lower():
                note('ctypes', 'Default extension not lower case', fn, e)
        lower_def = {}
        for e in defaults:
            lower_def.setdefault(e.lower(), []).append(e)
        for e, vs in lower_def.items():
            if len(vs) > 1:
                note('ctypes', 'Default extensions equal ignoring case', fn, vs)
        names_lower = {('/' + n).lower() for n in pkg.names}
        for pn in overrides:
            if pn.lower() not in names_lower:
                note('ctypes', 'Override for a missing part', fn, pn)
        for n in pkg.names:
            if n.endswith('/') or n == '[Content_Types].xml':
                continue
            ext = n.rsplit('.', 1)[-1] if '.' in n.rsplit('/', 1)[-1] else ''
            has_o = ('/' + n).lower() in {k.lower() for k in overrides}
            has_d = ext.lower() in {k.lower() for k in defaults}
            if n.startswith('word/media/'):
                note('ctypes', 'media registered by ' + ('Override' if has_o else 'Default' if has_d else 'NOTHING'), fn)
            if not has_o and not has_d:
                note('ctypes', 'part with neither Default nor Override', fn, n)
            if has_o and n.endswith('.xml') and defaults.get('xml') == overrides.get('/' + n):
                note('ctypes', 'Override equal to the xml Default', fn)

        # ------------------------------------------------------------ media and images
        media_targets = collections.Counter()
        for _, t, tgt in [(i, ty, tg) for src in [pkg.main] + [p for _, p, _ in stories[1:]]
                          for (i, ty, tg) in rels_of(pkg, src)]:
            if t == REL_IMAGE:
                media_targets[tgt] += 1
                m = NAMED.match(posixpath.basename(tgt))
                note('media', 'image target named nameN.ext' if m and posixpath.dirname(tgt) == 'word/media' else 'image target named otherwise', fn,
                     None if m else tgt)
        for tgt, n in media_targets.items():
            if n > 1:
                note('media', 'media part targeted by several relationships', fn, (tgt, n))
        img_ext = 0
        for src in [p for _, p, _ in stories]:
            for i, ty, tg in rels_of(pkg, src, external=True):
                if ty == REL_IMAGE and tg is None:
                    img_ext += 1
        if img_ext:
            note('media', 'external (linked) image relationships', fn, None, img_ext)
        for kind, part, root in stories:
            blips = list(root.iter('{%s}blip' % A))
            for b in blips:
                if b.get('{%s}link' % R) is not None:
                    note('media', 'a:blip with r:link', fn)
                if b.get('{%s}embed' % R) is None and b.get('{%s}link' % R) is None:
                    note('media', 'a:blip without embed or link', fn)
            if any(True for _ in root.iter('{%s}svgBlip' % ASVG)):
                note('media', 'asvg:svgBlip (SVG + PNG fallback, two image rels)', fn)
            embeds = collections.Counter(b.get('{%s}embed' % R) for b in blips if b.get('{%s}embed' % R))
            for rid, n in embeds.items():
                if n > 1:
                    note('media', 'one image rId used by several drawings in a part', fn)
            if list(root.iter('{%s}inline' % WP)):
                note('media', 'parts with wp:inline', fn, None, sum(1 for _ in root.iter('{%s}inline' % WP)))
            if list(root.iter('{%s}anchor' % WP)):
                note('media', 'parts with wp:anchor', fn, None, sum(1 for _ in root.iter('{%s}anchor' % WP)))
            if list(root.iter('{urn:schemas-microsoft-com:vml}imagedata')):
                note('media', 'VML v:imagedata', fn)

        # ------------------------------------------------------------ part naming
        for kind in ('header', 'footer'):
            ps = sorted(p for k, p, _ in stories if k == kind)
            nums = []
            for p in ps:
                m = re.fullmatch(r'word/%s(\d+)\.xml' % kind, p)
                if m:
                    nums.append(int(m.group(1)))
                else:
                    note('parts', '%s part not named word/%sN.xml' % (kind, kind), fn, p)
            if nums and sorted(nums) != list(range(1, len(nums) + 1)):
                note('parts', '%s numbers not 1..n' % kind, fn, sorted(nums))
        targeted = set()
        for rp in rels_parts:
            src = rp.replace('_rels/', '')[:-5]
            for _, _, tgt in rels_of(pkg, src if rp != '_rels/.rels' else None):
                targeted.add(tgt.lower())
        for n in pkg.names:
            if n.endswith('/') or n.endswith('.rels') or n == '[Content_Types].xml':
                continue
            if n.lower() not in targeted:
                note('parts', 'part targeted by no relationship', fn, n)

        # ------------------------------------------------------------ sections
        body = pkg.doc.find(w('body'))
        sects = list(pkg.doc.iter(w('sectPr')))
        S['sections', 'sectPr per file'][min(len(sects), 10)] += 1
        for s in sects:
            t = s.find(w('type'))
            S['sections', 'type'][t.get(w('val')) if t is not None else '<absent> (nextPage)'] += 1
            refs = s.findall(w('headerReference')) + s.findall(w('footerReference'))
            if not refs:
                note('sections', 'sectPr without header/footer references (inherits)', fn)
            if s.find(w('titlePg')) is not None and C.onoff_on(s.find(w('titlePg'))):
                note('sections', 'titlePg on', fn)
            if s.find(w('pgSz')) is None:
                note('sections', 'sectPr without pgSz', fn)
        f_ = C.settings_flags(pkg)
        sp_ = pkg.part('settings')
        if sp_ is not None:
            st = pkg.xml(sp_)
            if st.find(w('evenAndOddHeaders')) is not None and C.onoff_on(st.find(w('evenAndOddHeaders'))):
                note('sections', 'settings evenAndOddHeaders on', fn)
            for k in ('rsids', 'docVars', 'footnotePr', 'endnotePr', 'attachedTemplate', 'mailMerge'):
                if st.find(w(k)) is not None:
                    note('settings', k, fn)

        # ------------------------------------------------------------ tables
        for kind, part, root in stories:
            for tbl in root.iter(w('tbl')):
                S['tables']['tables'] += 1
                grid = tbl.find(w('tblGrid'))
                ncol = len(grid.findall(w('gridCol'))) if grid is not None else None
                if grid is None:
                    note('tables', 'table without tblGrid', fn)
                rows = [r for r in tbl if r.tag == w('tr')]
                if any(c.tag != w('tr') and c.tag not in (w('tblPr'), w('tblGrid')) and local(c.tag) not in ('bookmarkStart', 'bookmarkEnd', 'commentRangeStart', 'commentRangeEnd', 'proofErr', 'permStart', 'permEnd') for c in tbl):
                    note('tables', 'table with non-row children (sdt/customXml/AC/...)', fn,
                         sorted({local(c.tag) for c in tbl if c.tag not in (w('tr'), w('tblPr'), w('tblGrid'))})[:4])
                widths = set()
                prev_vm = None
                for r in rows:
                    S['tables']['rows'] += 1
                    if r.get('{%s}paraId' % W14) is not None:
                        S['tables']['rows with w14:paraId'] += 1
                    trpr = r.find(w('trPr'))
                    if trpr is not None and (trpr.find(w('ins')) is not None or trpr.find(w('del')) is not None):
                        note('tables', 'row-level tracked insert/delete', fn)
                    before = after = 0
                    if trpr is not None:
                        gb, ga = trpr.find(w('gridBefore')), trpr.find(w('gridAfter'))
                        before = int(gb.get(w('val'), '0')) if gb is not None else 0
                        after = int(ga.get(w('val'), '0')) if ga is not None else 0
                    cells = row_cells(r)
                    if any(c.tag != w('tc') and c.tag != w('trPr') and c.tag != w('tblPrEx') and local(c.tag) not in ('bookmarkStart', 'bookmarkEnd', 'proofErr', 'commentRangeStart', 'commentRangeEnd', 'permStart', 'permEnd') for c in r):
                        note('tables', 'row with non-cell children (sdt/customXml/...)', fn)
                    span = before + after
                    col = before
                    vm_here = {}
                    for c in cells:
                        S['tables']['cells'] += 1
                        tcpr = c.find(w('tcPr'))
                        gs = 1
                        if tcpr is not None:
                            g = tcpr.find(w('gridSpan'))
                            if g is not None:
                                gs = int(g.get(w('val'), '1'))
                                if gs > 1:
                                    S['tables']['cells with gridSpan > 1'] += 1
                            vm = tcpr.find(w('vMerge'))
                            if vm is not None:
                                st_ = vm.get(w('val')) or 'continue'
                                S['tables']['vMerge ' + st_] += 1
                                vm_here[col] = (st_, gs)
                                if st_ == 'continue' and (prev_vm is None or col not in prev_vm):
                                    note('tables', 'vMerge continue with no vMerge cell above at that grid column', fn)
                                elif st_ == 'continue' and prev_vm[col][1] != gs:
                                    note('tables', 'vMerge continue with a different gridSpan than above', fn)
                            if tcpr.find(w('hMerge')) is not None:
                                note('tables', 'legacy hMerge', fn)
                        col += gs
                        span += gs
                    prev_vm = vm_here
                    widths.add(span)
                    if ncol is not None and span != ncol:
                        note('tables', 'row grid width != number of gridCol', fn, (part, span, ncol))
                if len(widths) > 1:
                    note('tables', 'rows of different grid widths in one table', fn)
                if tbl.find('.//' + w('tbl')) is not None:
                    note('tables', 'tables containing nested tables', fn)

        # ------------------------------------------------------------ hyperlinks
        for kind, part, root in stories:
            rels = {i: (t, tg) for i, t, tg in rels_of(pkg, part, external=True)}
            used = collections.Counter()
            for h in root.iter(w('hyperlink')):
                rid = h.get('{%s}id' % R)
                if rid:
                    used[rid] += 1
                    note('hyperlinks', 'w:hyperlink with r:id', fn)
                    if rid not in rels:
                        note('hyperlinks', 'r:id with no relationship', fn)
                    elif rels[rid][1] is not None:
                        note('hyperlinks', 'r:id to an internal target', fn)
                if h.get(w('anchor')):
                    note('hyperlinks', 'w:hyperlink with w:anchor', fn)
                    if rid:
                        note('hyperlinks', 'both r:id and w:anchor', fn)
            for rid, n in used.items():
                if n > 1:
                    note('hyperlinks', 'one relationship used by several w:hyperlink', fn)
            ext_targets = collections.Counter(pkg_ext_target(pkg, part, i) for i, (t, tg) in rels.items()
                                              if t == REL_HYPERLINK and tg is None)
            for tg, n in ext_targets.items():
                if n > 1:
                    note('hyperlinks', 'several hyperlink relationships with one target', fn)
            for fl in C.scan_fields(root, kind):
                if fl.key == 'HYPERLINK':
                    note('hyperlinks', 'HYPERLINK field', fn)

        # ------------------------------------------------------------ styles
        if any(t == C.RT.replace('officeDocument/2006/relationships/', 'office/2007/relationships/') + 'stylesWithEffects'
               for _, t, _ in pkg.rels):
            note('styles', 'stylesWithEffects part', fn)
        if sp is not None:
            sr = pkg.xml(sp)
            ls = sr.find(w('latentStyles'))
            if ls is not None:
                cnt = ls.get(w('count'))
                n = len(ls.findall(w('lsdException')))
                note('styles', 'latentStyles count attribute ' + ('== entries' if cnt == str(n) else '!= entries'), fn)

    rep = {}
    for sec, ctr in sorted(S.items(), key=lambda x: str(x[0])):
        key = sec if isinstance(sec, str) else '/'.join(sec)
        rep[key] = {str(k): v for k, v in sorted(ctr.items(), key=lambda x: str(x[0]))}
    rep['files_with'] = {k: len(v) for k, v in sorted(files.items())}
    rep['examples'] = {k: [str(x) for x in v] for k, v in ex.items()}
    print(json.dumps(rep, indent=1, ensure_ascii=False))
    if out_json:
        with open(out_json, 'w') as fh:
            json.dump(rep, fh, indent=1, ensure_ascii=False)


def row_cells(r):
    """w:tc of a row in order, including cells wrapped in cell-level w:sdt/w:customXml."""
    out = []
    for c in r:
        if c.tag == w('tc'):
            out.append(c)
        elif c.tag in (w('sdt'), w('sdtContent'), w('customXml')):
            out.extend(row_cells(c))
    return out


def rels_of(pkg, src, external=False):
    """[(Id, Type, target part or None for external)] of a source part (None = package)."""
    if src is None:
        rp = '_rels/.rels'
        d = ''
    else:
        d, b = posixpath.split(src)
        rp = posixpath.join(d, '_rels', b + '.rels')
    if rp not in pkg.names:
        return []
    out = []
    try:
        rr = pkg.xml(rp)
    except ET.ParseError:
        return []
    for r in rr:
        t = r.get('Target') or ''
        if r.get('TargetMode') == 'External':
            if external:
                out.append((r.get('Id'), r.get('Type'), None))
            continue
        tgt = t.lstrip('/') if t.startswith('/') else posixpath.normpath(posixpath.join(d, t))
        out.append((r.get('Id'), r.get('Type'), tgt))
    return out


def pkg_ext_target(pkg, src, rid):
    d, b = posixpath.split(src)
    rp = posixpath.join(d, '_rels', b + '.rels')
    for r in pkg.xml(rp):
        if r.get('Id') == rid:
            return r.get('Target')
    return None


if __name__ == '__main__':
    main(sys.argv[1:])
