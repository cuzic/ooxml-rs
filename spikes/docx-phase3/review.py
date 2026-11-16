"""ADR-0015 evidence: review features in real Word files.

Usage: python3 -I review.py [--json out.json] [--examples N] <dir or file>...

Every package that opens (no encryption, DTD, Strict, nesting deeper than 256; see
p3common.Pkg) is measured. Inputs are untrusted: read with zipfile + ElementTree only,
nothing is resolved or executed. Sections:

 comments    comments.xml and its companions (commentsExtended, commentsIds,
             commentsExtensible, people): counts, replies (w15:paraIdParent), done flags,
             how commentEx/commentId/commentExtensible link to comments (paraId of the
             comment's last paragraph), durable ids, anchoring in the stories (range start,
             end, reference), dates, authors vs people.
 paraid      w14:paraId / w14:textId per story kind: coverage, placeholder textId,
             values >= 0x80000000, duplicates across the package.
 revisions   tracked changes by element; author/date presence and date formats; id order and
             collisions with other annotation ids; paragraph-mark revisions; w16du:dateUtc;
             trackRevisions in settings.
 notes       footnotes/endnotes parts: separator kinds and ids, normal-note id ranges and
             gaps, references from the stories, settings footnotePr/endnotePr, reference run
             styles, footnoteRef/endnoteRef runs, note paragraph styles.
 sdt         content controls: level, kind, w:id, tag/alias, lock, data binding and whether
             its storeItemID names a custom XML part, placeholders, w14/w15 extensions.
 fields      field codes by kind (complex vs fldSimple), dirty and fldLock flags, fields
             without a cached result, nesting, settings updateFields.
 formatting  inputs of effective-formatting resolution: docDefaults, style types and
             defaults, basedOn chains (depth, dangling, cycles), linked styles, references to
             missing styles, numbering levels with rPr/pPr, theme font references, toggle
             properties, table conditional formatting, paragraph-mark rPr.
"""
import collections
import json
import os
import re
import sys
import xml.etree.ElementTree as ET

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import p3common as C  # noqa: E402

w, local = C.w, C.local
W14 = 'http://schemas.microsoft.com/office/word/2010/wordml'
W15 = 'http://schemas.microsoft.com/office/word/2012/wordml'
# Office 2013 pre-release namespace for the same w15 elements (review round 1, B1):
# Word 2013 betas wrote commentsExtended/people with it, and many SDK files keep it.
W15_PRE = 'http://schemas.microsoft.com/office/word/2010/11/wordml'
W15S = (W15, W15_PRE)
W16CID = 'http://schemas.microsoft.com/office/word/2016/wordml/cid'
W16CEX = 'http://schemas.microsoft.com/office/word/2018/wordml/cex'
W16DU = 'http://schemas.microsoft.com/office/word/2023/wordml/word16du'
DS = 'http://schemas.openxmlformats.org/officeDocument/2006/customXml'
A = 'http://schemas.openxmlformats.org/drawingml/2006/main'
REL_EXT = {
    'commentsExtended': 'http://schemas.microsoft.com/office/2011/relationships/commentsExtended',
    'commentsIds': 'http://schemas.microsoft.com/office/2016/09/relationships/commentsIds',
    'commentsExtensible': 'http://schemas.microsoft.com/office/2018/08/relationships/commentsExtensible',
    'people': 'http://schemas.microsoft.com/office/2011/relationships/people',
}
REVISIONS = ['ins', 'del', 'moveFrom', 'moveTo', 'rPrChange', 'pPrChange', 'sectPrChange', 'tblPrChange',
             'trPrChange', 'tcPrChange', 'tblGridChange', 'tblPrExChange', 'numberingChange', 'cellIns',
             'cellDel', 'cellMerge', 'moveFromRangeStart', 'moveToRangeStart']
SDT_KINDS = ['richText', 'text', 'comboBox', 'dropDownList', 'date', 'picture', 'docPartObj', 'docPartList',
             'group', 'citation', 'bibliography', 'equation']
TOGGLES = ['b', 'bCs', 'i', 'iCs', 'caps', 'smallCaps', 'strike', 'dstrike', 'outline', 'shadow', 'emboss',
           'imprint', 'vanish']
DATE_SHAPES = [('Z, no fraction', re.compile(r'^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ$')),
               ('Z, fraction', re.compile(r'^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d+Z$')),
               ('no zone', re.compile(r'^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(\.\d+)?$')),
               ('offset', re.compile(r'^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(\.\d+)?[+-]\d\d:\d\d$'))]


def date_shape(s):
    if s is None:
        return 'absent'
    for name, rx in DATE_SHAPES:
        if rx.match(s):
            return name
    return 'other'


def q(nsuri, l):
    return '{%s}%s' % (nsuri, l)


def rel_target(pkg, rtype):
    for _, t, tgt in pkg.rels:
        if t == rtype and tgt in pkg.names:
            return tgt
    return None


def xml_or_none(pkg, name):
    if not name:
        return None
    try:
        b = pkg.z.read(name)
        if C.has_dtd(b):
            return None
        root = ET.fromstring(b)
        return root if C.max_depth(root) <= 256 else None
    except (ET.ParseError, KeyError):
        return None


def paragraphs(el):
    return [p for p in el.iter(w('p'))]


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
    S = collections.defaultdict(collections.Counter)
    files = collections.defaultdict(set)
    ex = collections.defaultdict(list)

    def note(section, key, f, example=None, n=1):
        S[section][key] += n
        if n:
            files[section + ': ' + key].add(f)
        if example is not None and len(ex[section + ': ' + key]) < nex:
            ex[section + ': ' + key].append(example)

    for path in C.corpus_files(args):
        pkg = C.Pkg(path)
        if pkg.err:
            S['files']['not opened: ' + pkg.err.split(':')[0]] += 1
            continue
        stories = pkg.stories()
        if pkg.err:
            S['files']['not opened: ' + pkg.err] += 1
            continue
        S['files']['opened'] += 1
        f = pkg.name
        settings = xml_or_none(pkg, pkg.part('settings'))

        # ------------------------------------------------------------- annotation ids
        other_ids = set()  # bookmark + permission ids (ADR-0011 union, minus comments/revisions)
        for _, _, root in stories:
            for el in root.iter():
                if local(el.tag) in ('bookmarkStart', 'permStart') and el.get(w('id')) is not None:
                    other_ids.add(el.get(w('id')))

        # ------------------------------------------------------------------- comments
        cpart = pkg.part('comments')
        croot = xml_or_none(pkg, cpart)
        comments = {}
        if croot is not None:
            note('comments', 'files with a comments part', f)
            for c in croot.iter(w('comment')):
                cid = c.get(w('id'))
                ps = paragraphs(c)
                comments[cid] = c
                note('comments', 'comments', f)
                note('comments', 'comment paragraphs', f, n=len(ps))
                if len(ps) > 1:
                    note('comments', 'comments with more than one paragraph', f)
                if c.find('.//' + w('tbl')) is not None:
                    note('comments', 'comments containing a table', f)
                if ps and ps[-1].get(q(W14, 'paraId')):
                    note('comments', 'comments whose last paragraph has w14:paraId', f)
                note('comments', 'date: ' + date_shape(c.get(w('date'))), f, c.get(w('date')))
                if c.get(w('initials')) is None:
                    note('comments', 'comments without w:initials', f)
                if not (c.get(w('author')) or '').strip():
                    note('comments', 'comments with an empty author', f)
                if c.find('.//' + w('annotationRef')) is None:
                    note('comments', 'comments without w:annotationRef', f)
                if cid in other_ids:
                    note('comments', 'comment id equal to a bookmark/permission id', f, cid)
            ids = [int(i) for i in comments if i and i.lstrip('-').isdigit()]
            if ids:
                S['comments']['max comment id (largest over files)'] = max(S['comments']['max comment id (largest over files)'], max(ids))
                if sorted(ids) != list(range(min(ids), min(ids) + len(ids))):
                    note('comments', 'files whose comment ids have gaps', f)
                if min(ids) != 0:
                    note('comments', 'files whose comment ids do not start at 0', f, min(ids))
        # anchoring in the stories
        anchors = collections.defaultdict(lambda: collections.Counter())
        for kind, _, root in stories:
            for el in root.iter():
                ln = local(el.tag)
                if ln in ('commentRangeStart', 'commentRangeEnd', 'commentReference'):
                    anchors[el.get(w('id'))][ln] += 1
        for cid in comments:
            a = anchors.get(cid, collections.Counter())
            shape = '+'.join(k for k in ('commentRangeStart', 'commentRangeEnd', 'commentReference') if a[k])
            note('comments', 'anchoring: ' + (shape or 'none'), f, cid)
            if any(v > 1 for v in a.values()):
                note('comments', 'anchors repeated for one id', f, cid)
        for cid in anchors:
            if cid not in comments:
                note('comments', 'anchors naming a missing comment', f, cid)
        # companions
        last_para = {}
        for cid, c in comments.items():
            ps = paragraphs(c)
            if ps and ps[-1].get(q(W14, 'paraId')):
                last_para[ps[-1].get(q(W14, 'paraId'))] = cid
        any_para = {p.get(q(W14, 'paraId')): cid for cid, c in comments.items() for p in paragraphs(c) if p.get(q(W14, 'paraId'))}
        for kind, rtype in REL_EXT.items():
            tgt = rel_target(pkg, rtype)
            if tgt:
                note('comments', 'files with ' + kind + ' (' + os.path.basename(tgt) + ')', f)
        ext = xml_or_none(pkg, rel_target(pkg, REL_EXT['commentsExtended']))
        if ext is not None:
            n15 = C.ns(ext.tag)
            note('comments', 'commentsExtended namespace: ' + {W15: '2012 (w15)', W15_PRE: '2010/11 (pre-release)'}.get(n15, n15), f)
            entries = list(ext.iter(q(n15, 'commentEx')))
            by_para = {e.get(q(n15, 'paraId')): e for e in entries}
            parent_of = {}
            for e in entries:
                pid = e.get(q(n15, 'paraId'))
                note('comments', 'commentEx entries', f)
                if pid in last_para:
                    note('comments', 'commentEx paraId = last paragraph of a comment', f)
                elif pid in any_para:
                    note('comments', 'commentEx paraId = a non-last paragraph of a comment', f, pid)
                else:
                    note('comments', 'commentEx paraId matching no comment paragraph', f, pid)
                if e.get(q(n15, 'done')) == '1':
                    note('comments', 'commentEx done="1"', f)
                parent = e.get(q(n15, 'paraIdParent'))
                if parent:
                    note('comments', 'replies (commentEx with paraIdParent)', f)
                    if parent not in last_para:
                        note('comments', 'paraIdParent matching no comment last paragraph', f, parent)
                    pe = by_para.get(parent)
                    if pe is not None and pe.get(q(n15, 'paraIdParent')):
                        note('comments', 'reply to a reply (depth > 1)', f)
                    rc, pc = last_para.get(pid), last_para.get(parent)
                    if rc is not None and pc is not None:
                        parent_of[rc] = pc
                        a_ = anchors.get(rc, collections.Counter())
                        shape = '+'.join(k for k in ('commentRangeStart', 'commentRangeEnd', 'commentReference') if a_[k])
                        note('comments', 'reply anchoring: ' + (shape or 'none'), f)
                        pd = (pe.get(q(n15, 'done')) if pe is not None else None) or '0'
                        note('comments', 'done: parent %s, reply %s' % (pd, e.get(q(n15, 'done')) or '0'), f)
            # where replies' anchors sit relative to their parent's (main story order)
            order = [(local(el.tag), el.get(w('id'))) for kind, _, root in stories if kind == 'main' for el in root.iter()
                     if local(el.tag) in ('commentRangeStart', 'commentRangeEnd', 'commentReference')]
            pos = {k: i for i, k in enumerate(order)}
            for rc, pc in parent_of.items():
                rs, ps_ = pos.get(('commentRangeStart', rc)), pos.get(('commentRangeStart', pc))
                re_, pr_ = pos.get(('commentRangeEnd', rc)), pos.get(('commentReference', pc))
                if rs is not None and ps_ is not None:
                    between = {order[i][0] for i in range(ps_ + 1, rs)}
                    note('comments', 'reply start: after the parent start with only starts between' if rs > ps_ and between <= {'commentRangeStart'} else 'reply start: elsewhere', f)
                if re_ is not None and pr_ is not None:
                    note('comments', 'reply end: after the parent reference' if re_ > pr_ else 'reply end: before the parent reference', f)
            missing = [cid for cid in comments if not any(pp in by_para for pp in [p.get(q(W14, 'paraId')) for p in paragraphs(comments[cid])[-1:]])]
            if missing:
                note('comments', 'comments with no commentEx entry (file has commentsExtended)', f, n=len(missing))
        cids = xml_or_none(pkg, rel_target(pkg, REL_EXT['commentsIds']))
        if cids is not None:
            durable = []
            for e in cids.iter(q(W16CID, 'commentId')):
                note('comments', 'commentId entries', f)
                d = e.get(q(W16CID, 'durableId'))
                durable.append(d)
                if d and int(d, 16) >= 0x80000000:
                    note('comments', 'durableId >= 0x80000000', f, d)
                if e.get(q(W16CID, 'paraId')) not in last_para:
                    note('comments', 'commentId paraId matching no comment last paragraph', f)
            if len(durable) != len(set(durable)):
                note('comments', 'duplicate durableId', f)
        cex = xml_or_none(pkg, rel_target(pkg, REL_EXT['commentsExtensible']))
        if cex is not None:
            ids_durable = {e.get(q(W16CID, 'durableId')) for e in (cids.iter(q(W16CID, 'commentId')) if cids is not None else [])}
            for e in cex.iter(q(W16CEX, 'commentExtensible')):
                note('comments', 'commentExtensible entries', f)
                if e.get(q(W16CEX, 'durableId')) not in ids_durable:
                    note('comments', 'commentExtensible durableId not in commentsIds', f)
                note('comments', 'commentExtensible dateUtc: ' + date_shape(e.get(q(W16CEX, 'dateUtc'))), f)
        people = xml_or_none(pkg, rel_target(pkg, REL_EXT['people']))
        if people is not None:
            n15 = C.ns(people.tag)
            note('comments', 'people.xml namespace: ' + {W15: '2012 (w15)', W15_PRE: '2010/11 (pre-release)'}.get(n15, n15), f)
            names = set()
            for p in people.iter(q(n15, 'person')):
                names.add(p.get(q(n15, 'author')))
                pi = p.find(q(n15, 'presenceInfo'))
                note('comments', 'people: presenceInfo providerId=' + (pi.get(q(n15, 'providerId')) if pi is not None else '(none)'), f)
            for c in comments.values():
                note('comments', 'comment author found in people.xml' if c.get(w('author')) in names else 'comment author missing from people.xml', f, None if c.get(w('author')) in names else c.get(w('author')))

        # ---------------------------------------------------------------------- paraId
        seen = collections.Counter()
        for kind, part, root in stories:
            for p in root.iter(w('p')):
                note('paraid', kind + ': paragraphs', f)
                pid = p.get(q(W14, 'paraId'))
                if pid:
                    note('paraid', kind + ': with w14:paraId', f)
                    seen[pid] += 1
                    if int(pid, 16) >= 0x80000000:
                        note('paraid', 'paraId >= 0x80000000', f, pid)
                if p.get(q(W14, 'textId')) == '77777777':
                    note('paraid', kind + ': textId 77777777', f)
        if any(v > 1 for v in seen.values()):
            note('paraid', 'files with duplicate paraId across stories', f)

        # ------------------------------------------------------------------- revisions
        rev_ids = []
        for kind, part, root in stories:
            parent = {c: p for p in root.iter() for c in p}
            for el in root.iter():
                ln = local(el.tag)
                if C.ns(el.tag) != C.W or ln not in REVISIONS:
                    continue
                note('revisions', ln, f)
                rid = el.get(w('id'))
                if rid is None:
                    note('revisions', 'without w:id', f, ln)
                else:
                    rev_ids.append(rid)
                if el.get(w('author')) is None:
                    note('revisions', 'without w:author', f, ln)
                note('revisions', 'date: ' + date_shape(el.get(w('date'))), f, el.get(w('date')))
                if el.get(q(W16DU, 'dateUtc')) is not None:
                    note('revisions', 'with w16du:dateUtc', f)
                par = parent.get(el)
                if ln in ('ins', 'del') and par is not None and local(par.tag) == 'rPr':
                    gp = parent.get(par)
                    note('revisions', ln + ' as paragraph-mark revision (pPr/rPr)' if gp is not None and local(gp.tag) == 'pPr' else ln + ' in rPr (other)', f)
                if ln in ('ins', 'del') and par is not None and local(par.tag) == 'trPr':
                    note('revisions', ln + ' of a table row (trPr)', f)
                if ln == 'del':
                    if el.find('.//' + w('t')) is not None:
                        note('revisions', 'w:del containing w:t (not delText)', f)
                if ln == 'ins' and el.find('.//' + w('delText')) is not None:
                    note('revisions', 'w:ins containing w:delText (ins+del of the same text)', f)
        # paragraph-mark revisions: on the last paragraph of a container (review M1);
        # tracked row deletions/insertions: what their cell content carries (M2);
        # comment references inside revisions (m3)
        for kind, part, root in stories:
            parent = {c: p for p in root.iter() for c in p}
            for cont in root.iter():
                if local(cont.tag) not in ('body', 'tc', 'footnote', 'endnote', 'comment', 'hdr', 'ftr', 'txbxContent'):
                    continue
                ps = [c for c in cont if c.tag == w('p')]
                for i, p_ in enumerate(ps):
                    mk = p_.find(w('pPr') + '/' + w('rPr'))
                    if mk is None:
                        continue
                    for t in ('ins', 'del'):
                        if mk.find(w(t)) is None:
                            continue
                        in_row = False
                        a_ = parent.get(cont)
                        if local(cont.tag) == 'tc' and a_ is not None and a_.find(w('trPr') + '/' + w(t)) is not None:
                            in_row = True
                        where = 'the last paragraph of its container' if i == len(ps) - 1 else 'a paragraph that is not the last'
                        note('revisions', 'paragraph-mark %s on %s%s' % (t, where, ' (row revision)' if in_row else ''), f)
            for tr in root.iter(w('tr')):
                for t in ('ins', 'del'):
                    if tr.find(w('trPr') + '/' + w(t)) is None:
                        continue
                    runs = [r for r in tr.iter(w('r')) if r.find(w('t')) is not None or r.find(w('delText')) is not None]
                    wrapped = sum(1 for r in runs if parent.get(r) is not None and local(parent[r].tag) == t)
                    marks = [p_ for p_ in tr.iter(w('p'))]
                    mk = sum(1 for p_ in marks if p_.find(w('pPr') + '/' + w('rPr') + '/' + w(t)) is not None)
                    note('revisions', 'tracked row %s: runs with text %d, inside w:%s %d; paragraphs %d, marks %s %d' % (t, len(runs), t, wrapped, len(marks), t, mk), f)
            for el in root.iter():
                if local(el.tag) == 'commentReference':
                    a_ = parent.get(parent.get(el)) if parent.get(el) is not None else None
                    if a_ is not None and local(a_.tag) in ('ins', 'del'):
                        note('comments', 'commentReference run inside w:' + local(a_.tag), f)
        if rev_ids:
            note('revisions', 'files with revisions', f)
            ints = [int(i) for i in rev_ids if i.lstrip('-').isdigit()]
            if len(ints) != len(rev_ids):
                note('revisions', 'non-numeric revision id', f)
            if len(set(rev_ids)) != len(rev_ids):
                note('revisions', 'files with duplicate revision ids', f)
            if ints and ints != sorted(ints):
                note('revisions', 'files whose revision ids are not ascending in document order', f)
            if set(rev_ids) & other_ids:
                note('revisions', 'files where a revision id equals a bookmark/permission id', f)
            if set(rev_ids) & set(comments):
                note('revisions', 'files where a revision id equals a comment id', f)
            S['revisions']['largest revision id'] = max(S['revisions']['largest revision id'], max(ints, default=0))
        if settings is not None and C.onoff_on(settings.find(w('trackRevisions'))):
            note('revisions', 'settings trackRevisions on', f)

        # ----------------------------------------------------------------------- notes
        refs = collections.Counter()
        ref_styles = collections.Counter()
        for kind, part, root in stories:
            for el in root.iter():
                if local(el.tag) in ('footnoteReference', 'endnoteReference') and C.ns(el.tag) == C.W:
                    refs[(local(el.tag), el.get(w('id')))] += 1
        for kind, part, root in stories:
            for r in root.iter(w('r')):
                if r.find(w('footnoteReference')) is not None or r.find(w('endnoteReference')) is not None:
                    rs = r.find(w('rPr') + '/' + w('rStyle'))
                    va = r.find(w('rPr') + '/' + w('vertAlign'))
                    ref_styles['rStyle=' + (rs.get(w('val')) if rs is not None else '(none)') + (', vertAlign=' + va.get(w('val')) if va is not None else '')] += 1
        for k, v in ref_styles.items():
            note('notes', 'reference run: ' + k, f, n=v)
        for kind, tag in (('footnotes', 'footnote'), ('endnotes', 'endnote')):
            root = xml_or_none(pkg, pkg.part(kind))
            if root is None:
                continue
            note('notes', 'files with ' + kind, f)
            normal = []
            for n in root.iter(w(tag)):
                t = n.get(w('type')) or 'normal'
                nid = n.get(w('id'))
                if t != 'normal':
                    note('notes', '%s %s id=%s' % (tag, t, nid), f)
                else:
                    normal.append(int(nid) if nid and nid.lstrip('-').isdigit() else None)
                    ps = paragraphs(n)
                    st = ps[0].find(w('pPr') + '/' + w('pStyle')) if ps else None
                    note('notes', '%s first paragraph style %s' % (tag, st.get(w('val')) if st is not None else '(none)'), f)
                    if n.find('.//' + w(tag + 'Ref')) is None:
                        note('notes', '%s without w:%sRef' % (tag, tag), f)
                    if refs[(tag + 'Reference', nid)] == 0:
                        note('notes', '%s not referenced from any story' % tag, f, nid)
                    elif refs[(tag + 'Reference', nid)] > 1:
                        note('notes', '%s referenced more than once' % tag, f, nid)
            normal = [x for x in normal if x is not None]
            if normal:
                note('notes', '%s normal notes' % tag, f, n=len(normal))
                if sorted(normal) != list(range(min(normal), min(normal) + len(normal))):
                    note('notes', '%s ids with gaps' % tag, f)
                note('notes', '%s smallest normal id = %d' % (tag, min(normal)), f)
            if settings is not None:
                pr = settings.find(w(tag + 'Pr'))
                if pr is not None:
                    note('notes', 'settings %sPr children: %s' % (tag, ','.join(sorted({local(c.tag) + ('=' + (c.get(w('id')) or '') if c.get(w('id')) is not None else '') for c in pr}))), f)
        fn_ids = {i for (k, i) in refs if k == 'footnoteReference'}
        en_ids = {i for (k, i) in refs if k == 'endnoteReference'}
        if fn_ids & en_ids:
            note('notes', 'files where a footnote id equals an endnote id', f)

        # ------------------------------------------------------------------------- sdt
        store_ids = set()
        for n in pkg.names:
            if re.match(r'customXml/itemProps\d*\.xml$', n):
                r = xml_or_none(pkg, n)
                if r is not None and r.get(q(DS, 'itemID')):
                    store_ids.add(r.get(q(DS, 'itemID')).upper())
        sdt_ids = collections.Counter()
        for kind, part, root in stories:
            parent = {c: p for p in root.iter() for c in p}
            for sdt in root.iter(w('sdt')):
                pr = sdt.find(w('sdtPr'))
                par = parent.get(sdt)
                pl = local(par.tag) if par is not None else '?'
                level = {'p': 'inline', 'tbl': 'row', 'tr': 'cell'}.get(pl, 'block' if pl in ('body', 'tc', 'sdtContent', 'hdr', 'ftr', 'footnote', 'endnote', 'comment', 'customXml', 'txbxContent') else 'inline (' + pl + ')')
                if pl == 'sdtContent':
                    gp = parent.get(par)
                    level = 'nested in ' + ('inline' if gp is not None and parent.get(gp) is not None and local(parent.get(gp).tag) == 'p' else 'block') + ' sdt'
                note('sdt', 'level: ' + level, f)
                kinds = [local(c.tag) for c in (pr if pr is not None else []) if local(c.tag) in SDT_KINDS and C.ns(c.tag) == C.W]
                ext = [C.ns(c.tag).rsplit('/', 1)[-1] + ':' + local(c.tag) for c in (pr if pr is not None else []) if C.ns(c.tag) in (W14, W15, W15_PRE) and local(c.tag) in ('checkbox', 'repeatingSection', 'repeatingSectionItem', 'webExtensionLinked', 'webExtensionCreated')]
                note('sdt', 'kind: ' + ('+'.join(kinds + ext) or '(rich text, no kind element)'), f)
                if pr is None:
                    continue
                sid = pr.find(w('id'))
                if sid is not None:
                    sdt_ids[sid.get(w('val'))] += 1
                    v = sid.get(w('val'))
                    if v and v.lstrip('-').isdigit() and int(v) < 0:
                        note('sdt', 'negative w:id', f)
                else:
                    note('sdt', 'without w:id', f)
                for k in ('tag', 'alias', 'lock', 'showingPlcHdr', 'temporary', 'placeholder', 'dataBinding', 'rPr'):
                    if pr.find(w(k)) is not None:
                        note('sdt', 'has ' + k, f)
                lk = pr.find(w('lock'))
                if lk is not None:
                    note('sdt', 'lock=' + (lk.get(w('val')) or ''), f)
                db = pr.find(w('dataBinding'))
                if db is not None:
                    sid2 = (db.get(w('storeItemID')) or '').upper()
                    kind_ = {'{6C3C8BC8-F283-45AE-878A-BAB7291924A1}': 'core properties (Office built-in)',
                             '{6668398D-A668-4E3E-A5EB-62B293D839F1}': 'extended properties (Office built-in)',
                             '{55AF091B-3C7A-41E3-B477-F2FDAA23CFDA}': 'cover page properties'}.get(sid2, 'other store')
                    note('sdt', 'dataBinding store: %s, %s' % (kind_, 'a custom XML part with this itemID exists' if sid2 in store_ids else 'no custom XML part with this itemID'), f, sid2)
                    if db.get(w('prefixMappings')):
                        note('sdt', 'dataBinding with prefixMappings', f)
                if any(pr.find(q(n, 'dataBinding')) is not None for n in W15S):
                    note('sdt', 'w15:dataBinding (rich-text binding)', f)
                for n in W15S:
                    ap = pr.find(q(n, 'appearance'))
                    if ap is not None:
                        note('sdt', 'w15:appearance=' + (ap.get(q(n, 'val')) or '') + (' (pre-release ns)' if n == W15_PRE else ''), f)
        if any(v > 1 for v in sdt_ids.values()):
            note('sdt', 'files with duplicate sdt w:id', f)
        if store_ids:
            note('sdt', 'files with custom XML parts', f)

        # ---------------------------------------------------------------------- fields
        for kind, part, root in stories:
            fl = C.scan_fields(root, kind)
            for fld in fl:
                note('fields', 'fields (all levels)', f)
                code = (fld.key or '').split(' ')[0] or '(empty)'
                note('fields', 'code ' + code, f)
            for el in root.iter():
                if el.tag == w('fldSimple'):
                    note('fields', 'fldSimple', f)
                    if C.onoff_on(el) if el.get(w('dirty')) is not None and el.get(w('dirty')) not in C.OFF else False:
                        note('fields', 'fldSimple w:dirty on', f)
                    if el.get(w('fldLock')) not in (None, 'false', '0', 'off'):
                        note('fields', 'fldSimple w:fldLock on', f)
                    if len(el.findall(w('r'))) == 0:
                        note('fields', 'fldSimple without a result run', f)
                elif el.tag == w('fldChar') and el.get(w('fldCharType')) == 'begin':
                    note('fields', 'complex (fldChar begin)', f)
                    if el.get(w('dirty')) not in (None, 'false', '0', 'off'):
                        note('fields', 'fldChar w:dirty on', f)
                    if el.get(w('fldLock')) not in (None, 'false', '0', 'off'):
                        note('fields', 'fldChar w:fldLock on', f)
                    if el.find(w('ffData')) is not None:
                        note('fields', 'form field (ffData)', f)
            # complex fields without separate
            depth_sep = []
            for el in root.iter(w('fldChar')):
                t = el.get(w('fldCharType'))
                if t == 'begin':
                    depth_sep.append(False)
                elif t == 'separate' and depth_sep:
                    depth_sep[-1] = True
                elif t == 'end' and depth_sep:
                    if not depth_sep.pop():
                        note('fields', 'complex field without separate (no cached result)', f)
        if settings is not None and settings.find(w('updateFields')) is not None:
            note('fields', 'settings updateFields present', f)

        # ------------------------------------------------------------------ formatting
        st = xml_or_none(pkg, pkg.part('styles'))
        if st is None:
            note('formatting', 'no styles part', f)
        else:
            dd = st.find(w('docDefaults'))
            note('formatting', 'docDefaults: ' + ('absent' if dd is None else '+'.join(local(c.tag) for c in dd) or 'empty'), f)
            styles = {}
            for s in st.iter(w('style')):
                styles[s.get(w('styleId'))] = s
                t = s.get(w('type')) or '(none)'
                note('formatting', 'style type ' + t, f)
                if s.get(w('default')) in ('1', 'true', 'on'):
                    note('formatting', 'default style of type ' + t, f)
                if s.find(w('link')) is not None:
                    note('formatting', 'linked styles (w:link)', f)
                if s.find(w('tblStylePr')) is not None:
                    note('formatting', 'table styles with tblStylePr', f)
                    for tp in s.iter(w('tblStylePr')):
                        note('formatting', 'tblStylePr type ' + (tp.get(w('type')) or ''), f)
                rpr = s.find(w('rPr'))
                if rpr is not None:
                    for tg in TOGGLES:
                        if rpr.find(w(tg)) is not None:
                            note('formatting', 'toggle property in a style: ' + tg, f)
                            break
            depths = []
            for sid, s in styles.items():
                d, cur, seen_ids = 0, s, {sid}
                while True:
                    b = cur.find(w('basedOn'))
                    if b is None:
                        break
                    nxt = b.get(w('val'))
                    if nxt in seen_ids:
                        note('formatting', 'basedOn cycle', f, sid)
                        break
                    if nxt not in styles:
                        note('formatting', 'basedOn names a missing style', f, nxt)
                        break
                    seen_ids.add(nxt)
                    cur = styles[nxt]
                    d += 1
                depths.append(d)
                b = s.find(w('basedOn'))
                if b is not None and b.get(w('val')) in styles and styles[b.get(w('val'))].get(w('type')) != s.get(w('type')):
                    note('formatting', 'basedOn a style of another type', f, sid)
            if depths:
                S['formatting']['deepest basedOn chain'] = max(S['formatting']['deepest basedOn chain'], max(depths))
            for kind, part, root in stories:
                for tag in ('pStyle', 'rStyle', 'tblStyle'):
                    for el in root.iter(w(tag)):
                        v = el.get(w('val'))
                        note('formatting', 'references: ' + tag, f)
                        if v not in styles:
                            note('formatting', 'references to a missing style: ' + tag, f, v)
                for el in root.iter(w('rFonts')):
                    if any(el.get(w(a)) for a in ('asciiTheme', 'hAnsiTheme', 'eastAsiaTheme', 'cstheme')):
                        note('formatting', 'rFonts with theme fonts in stories', f)
                        break
                for el in root.iter(w('pPr')):
                    if el.find(w('rPr')) is not None:
                        note('formatting', 'paragraph-mark rPr (pPr/rPr)', f)
                        break
                for el in root.iter(w('tblLook')):
                    has_attrs = any(el.get(w(a)) is not None for a in ('firstRow', 'lastRow', 'firstColumn', 'lastColumn', 'noHBand', 'noVBand'))
                    note('formatting', 'tblLook with ' + ('explicit attributes' if has_attrs else 'only the hex w:val'), f)
                for el in root.iter(w('color')):
                    if el.get(w('themeColor')):
                        note('formatting', 'w:color with themeColor' + (', cached RGB present' if el.get(w('val')) not in (None, 'auto') else ', no cached RGB') + (', tint/shade' if el.get(w('themeTint')) or el.get(w('themeShade')) else ''), f)
                for el in root.iter(w('cnfStyle')):
                    note('formatting', 'cnfStyle (cached table conditional formatting)', f)
                    break
                for r in root.iter(w('rPr')):
                    if any(r.find(w(tg)) is not None for tg in TOGGLES):
                        note('formatting', 'toggle property in direct rPr', f)
                        break
            for el in st.iter(w('rFonts')):
                if any(el.get(w(a)) for a in ('asciiTheme', 'hAnsiTheme', 'eastAsiaTheme', 'cstheme')):
                    note('formatting', 'rFonts with theme fonts in styles.xml', f)
                    break
        num = xml_or_none(pkg, pkg.part('numbering'))
        if num is not None:
            for lvl in num.iter(w('lvl')):
                if lvl.find(w('rPr')) is not None:
                    note('formatting', 'numbering levels with rPr', f)
                if lvl.find(w('pPr')) is not None:
                    note('formatting', 'numbering levels with pPr', f)
                if lvl.find(w('pStyle')) is not None:
                    note('formatting', 'numbering levels linked to a style (lvl/pStyle)', f)
        if pkg.part('theme') is None:
            note('formatting', 'no theme part', f)

    report = {sec: {k: [v, len(files.get(sec + ': ' + k, ()))] for k, v in sorted(cnt.items())} for sec, cnt in S.items()}
    for sec, d in report.items():
        print('\n## ' + sec)
        for k, (v, nf) in d.items():
            e = ex.get(sec + ': ' + k)
            print('  %-75s %8s  files=%-4d %s' % (k, v, nf, ('e.g. ' + repr(e[:nex])) if e else ''))
    if out_json:
        with open(out_json, 'w') as fh:
            json.dump({'report': report, 'examples': ex}, fh, indent=1, default=str)


if __name__ == '__main__':
    main(sys.argv[1:])
