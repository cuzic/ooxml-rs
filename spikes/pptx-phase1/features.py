"""Corpus counts for ADR-0008 §2-§3 (run with `python3 -I`).

Usage: python3 -I features.py <pptx files/dirs...>

Notes slides (shape of the part, back-relationship, body placeholder,
content Phase 0's text rules would refuse), hyperlinks/actions (where they
sit, `action` kinds, what `r:id` resolves to), sections (p14:sectionLst
consistency with p:sldIdLst), comments (legacy p:cmLst + commentAuthors,
modern p188:cmLst + authors: anchors, replies, unresolved authors),
transitions (direct vs mc:AlternateContent, Requires, element kinds,
attributes) and timing (node types, preset classes, build lists, targets).
"""
import sys, os, re, collections
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import (Pkg, q, qn, iter_files, walk_shapes, ph_of, sptree, NS, RT_NOTES, RT_SLIDE,
                    RT_NOTES_MASTER, RT_COMMENTS, RT_COMMENT_AUTHORS, RT_MODERN_COMMENTS, RT_AUTHORS)

C = collections.Counter()
EX = collections.defaultdict(list)
SECTIONS_URI = '{521415D9-36F7-43E2-AB2F-B90AF26B5E84}'


def ex(key, what=None, n=1):
    C[key] += n
    if what is not None and len(EX[key]) < 3:
        EX[key].append(what)


def action_kind(a):
    if not a:
        return '(none)'
    m = re.match(r'ppaction://([a-zA-Z]+)(\?jump=([a-zA-Z]+))?', a)
    if m:
        return m.group(1) + (('?jump=' + m.group(3)) if m.group(3) else '')
    return 'other:' + a.split(':')[0]


def scan_links(pk, part, root, fname, where):
    rels = {i: (ty, t, e) for i, ty, t, e in pk.rels(part)}
    for tag in ('a:hlinkClick', 'a:hlinkMouseOver', 'a:hlinkHover'):  # hlinkHover is p:cNvPr's name for mouse-over
        for parent in root.iter():
            for h in parent.findall(q(tag)):
                ctx = qn(parent.tag)
                ex('link.%s.%s.on_%s' % (where, tag, ctx))
                ak = action_kind(h.get('action'))
                rid = h.get(q('r:id'))
                if rid is None:
                    tgt = 'no r:id attribute'
                elif rid == '':
                    tgt = 'empty r:id'
                elif rid not in rels:
                    tgt = 'dangling r:id'
                else:
                    ty, t, e = rels[rid]
                    tgt = 'external(%s)' % ty.rsplit('/', 1)[-1] if e else 'internal(%s)' % ty.rsplit('/', 1)[-1]
                ex('link.%s.action=%s -> %s' % (where, ak, tgt), (fname, part))
                if h.get('tooltip') is not None:
                    C['link.tooltip'] += 1
                if h.find(q('a:snd')) is not None:
                    C['link.with_a:snd'] += 1
                for k in ('highlightClick', 'endSnd', 'history', 'invalidUrl', 'tgtFrame'):
                    if h.get(k) is not None:
                        C['link.attr.' + k] += 1


def main(argv):
    for path in iter_files(argv):
        fname = os.path.basename(path)
        try:
            pk = Pkg(path)
            pres = pk.xml(pk.pres) if pk.pres else None
        except Exception:
            continue
        if pres is None:
            continue
        C['file'] += 1
        slides = pk.slides()
        ids = [s for s, p in slides]
        # ---- sections
        secs = None
        for ext in pres.iter(q('p:ext')):
            if ext.get('uri') == SECTIONS_URI:
                secs = ext.find(q('p14:sectionLst'))
                if secs is None:
                    ex('sections.ext_without_p14_sectionLst', fname)
        if secs is not None:
            C['sections.files'] += 1
            seen = collections.Counter()
            for s in secs.findall(q('p14:section')):
                C['sections.section'] += 1
                if s.get('name') is None or s.get('id') is None:
                    ex('sections.section_missing_name_or_id', fname)
                lst = s.find(q('p14:sldIdLst'))
                ref = [x.get('id') for x in (lst.findall(q('p14:sldId')) if lst is not None else [])]
                if not ref:
                    ex('sections.empty_section', fname)
                for r in ref:
                    seen[r] += 1
                    if r not in ids:
                        ex('sections.unknown_sldId', (fname, r))
            for i in ids:
                if seen[i] == 0:
                    ex('sections.slide_in_no_section', (fname, i))
                elif seen[i] > 1:
                    ex('sections.slide_in_several_sections', (fname, i))
            order = [r for s in secs.findall(q('p14:section')) for x in s.iter(q('p14:sldId')) for r in [x.get('id')] if r in ids]
            if order != [i for i in ids if i in order]:
                ex('sections.order_differs_from_sldIdLst', fname)
            others = [qn(c.tag) for c in secs if c.tag != q('p14:section')]
            if others:
                ex('sections.other_children', (fname, others))
        for ext in pres.iter(q('p:ext')):
            C['presentation.ext uri=%s' % ext.get('uri')] += 1
        # ---- authors
        legacy_authors = {}
        for t in pk.rel_targets(pk.pres, RT_COMMENT_AUTHORS):
            r = pk.xml(t)
            if r is None:
                ex('comments.legacy.authors_unparsable', fname); continue
            for a in r.findall(q('p:cmAuthor')):
                legacy_authors[a.get('id')] = a
        modern_authors = {}
        for t in pk.rel_targets(pk.pres, RT_AUTHORS):
            r = pk.xml(t)
            if r is None:
                ex('comments.modern.authors_unparsable', fname); continue
            for a in r.findall(q('p188:author')):
                modern_authors[a.get('id')] = a
        notes_owner = collections.Counter()
        has_legacy = has_modern = False
        for sid, part in slides:
            if part is None:
                continue
            root = pk.xml(part)
            if root is None:
                continue
            C['slide'] += 1
            scan_links(pk, part, root, fname, 'slide')
            # ---- comments
            for t in pk.rel_targets(part, RT_COMMENTS):
                r = pk.xml(t)
                has_legacy = True
                if r is None:
                    ex('comments.legacy.part_unparsable', fname); continue
                for cm in r.findall(q('p:cm')):
                    C['comments.legacy.cm'] += 1
                    if cm.get('authorId') not in legacy_authors:
                        ex('comments.legacy.author_unresolved', fname)
                    for k in ('dt', 'idx'):
                        if cm.get(k) is None:
                            ex('comments.legacy.missing_' + k, fname)
                    if cm.find(q('p:pos')) is None:
                        ex('comments.legacy.no_pos', fname)
                    if cm.find(q('p:text')) is None:
                        ex('comments.legacy.no_text', fname)
                    if cm.find('.//' + q('p15:threadingInfo')) is not None:
                        ex('comments.legacy.p15_threadingInfo', fname)
                    if cm.find('.//' + q('p15:parentCm')) is not None:
                        ex('comments.legacy.p15_parentCm(reply)', fname)
            for t in pk.rel_targets(part, RT_MODERN_COMMENTS):
                r = pk.xml(t)
                has_modern = True
                if r is None:
                    ex('comments.modern.part_unparsable', fname); continue
                for cm in r.findall(q('p188:cm')):
                    C['comments.modern.cm'] += 1
                    if cm.get('authorId') not in modern_authors:
                        ex('comments.modern.author_unresolved', fname)
                    for c in cm:
                        if c.tag not in (q('p188:pos'), q('p188:replyLst'), q('p188:txBody'), q('p188:extLst')):
                            ex('comments.modern.anchor=%s' % qn(c.tag), fname)
                            for m in c:
                                C['comments.modern.anchor_moniker=%s' % qn(m.tag)] += 1
                    rl = cm.find(q('p188:replyLst'))
                    if rl is not None:
                        C['comments.modern.reply'] += len(rl.findall(q('p188:reply')))
                    if cm.get('status') is not None:
                        ex('comments.modern.status=%s' % cm.get('status'), fname)
                    if cm.find(q('p188:txBody')) is None:
                        ex('comments.modern.no_txBody', fname)
            # ---- transitions
            direct = root.findall(q('p:transition'))
            for tr in direct:
                ex('transition.direct', None)
                for c in tr:
                    C['transition.direct.kind=%s' % qn(c.tag)] += 1
            for ac in root.findall(q('mc:AlternateContent')):
                kids = {qn(x.tag) for b in ac for x in b}
                ex('ac_under_sld.content=%s' % sorted(kids), fname)
                for ch in ac.findall(q('mc:Choice')):
                    C['transition.ac.choice.Requires=%s' % ch.get('Requires')] += 1
                    for tr in ch.findall(q('p:transition')):
                        for c in tr:
                            C['transition.ac.choice.kind=%s' % qn(c.tag)] += 1
                        for k, v in tr.attrib.items():
                            C['transition.ac.choice.attr=%s' % qn(k)] += 1
                fb = ac.find(q('mc:Fallback'))
                if fb is None:
                    ex('transition.ac.no_fallback', fname)
                else:
                    for tr in fb.findall(q('p:transition')):
                        for c in tr:
                            C['transition.ac.fallback.kind=%s' % qn(c.tag)] += 1
            for tr in direct:
                for k in tr.attrib:
                    C['transition.direct.attr=%s' % qn(k)] += 1
            if direct and root.findall(q('mc:AlternateContent')):
                ex('transition.both_direct_and_ac', fname)
            # ---- timing
            for tm in root.iter(q('p:timing')):
                C['timing'] += 1
                for ctn in tm.iter(q('p:cTn')):
                    if ctn.get('presetClass'):
                        C['timing.effect.presetClass=%s' % ctn.get('presetClass')] += 1
                    if ctn.get('nodeType'):
                        C['timing.cTn.nodeType=%s' % ctn.get('nodeType')] += 1
                for tgt in tm.iter(q('p:spTgt')):
                    sub = [qn(c.tag) for c in tgt]
                    C['timing.spTgt.child=%s' % (sub[0] if sub else '(none)')] += 1
                for bl in tm.iter(q('p:bldLst')):
                    for b in bl:
                        C['timing.build=%s' % qn(b.tag)] += 1
                for other in ('p:sndTgt', 'p:inkTgt', 'p:sldTgt', 'p:audio', 'p:video', 'p:cmd'):
                    n = len(list(tm.iter(q(other))))
                    if n:
                        C['timing.' + other] += n
                if tm.find('.//' + q('mc:AlternateContent')) is not None:
                    ex('timing.contains_AC', fname)
            # ---- notes
            notes = pk.rel_targets(part, RT_NOTES)
            if len(notes) > 1:
                ex('notes.slide_with_several_notes_rels', fname)
            if not notes:
                C['notes.slide_without_notes'] += 1
                continue
            n = notes[0]
            notes_owner[n] += 1
            nroot = pk.xml(n)
            if nroot is None:
                ex('notes.missing_or_unparsable', fname); continue
            C['notes.part'] += 1
            back = pk.rel_targets(n, RT_SLIDE)
            if back != [part]:
                ex('notes.back_rel_mismatch', (fname, n, back, part))
            if not pk.rel_targets(n, RT_NOTES_MASTER):
                ex('notes.no_notesMaster_rel', fname)
            scan_links(pk, n, nroot, fname, 'notes')
            bodies = []
            for s, _d, g in walk_shapes(sptree(nroot)):
                ph = ph_of(s)
                if ph is not None and ph.get('type', 'obj') == 'body':
                    bodies.append(s)
                if ph is not None:
                    C['notes.ph.type=%s' % ph.get('type', '(absent=obj)')] += 1
            ex('notes.body_placeholders=%d' % len(bodies), fname if len(bodies) != 1 else None)
            if sptree(nroot) is not None and sptree(nroot).find(q('mc:AlternateContent')) is not None:
                ex('notes.AC_in_spTree', fname)
            if nroot.find('.//' + q('p:timing')) is not None:
                ex('notes.timing', fname)
            for b in bodies:
                tb = b.find(q('p:txBody'))
                if tb is None:
                    ex('notes.body_without_txBody', fname); continue
                ps = tb.findall(q('a:p'))
                text = ''.join(t.text or '' for t in tb.iter(q('a:t')))
                C['notes.body_nonempty' if text.strip() else 'notes.body_empty'] += 1
                if tb.find('.//' + q('a:fld')) is not None:
                    ex('notes.body_with_field', fname)
                if tb.find('.//' + q('a:br')) is not None:
                    C['notes.body_with_br'] += 1
                for p in ps:
                    for c in p:
                        if c.tag not in (q('a:pPr'), q('a:r'), q('a:br'), q('a:fld'), q('a:endParaRPr')):
                            ex('notes.para_unsupported_child=%s' % qn(c.tag), fname)
                for c in tb:
                    if c.tag == q('mc:AlternateContent'):
                        ex('notes.AC_in_txBody', fname)
        for n, k in notes_owner.items():
            if k > 1:
                ex('notes.shared_by_several_slides', (fname, n, k))
        if has_legacy:
            C['comments.files_legacy'] += 1
        if has_modern:
            C['comments.files_modern'] += 1
        if has_legacy and has_modern:
            ex('comments.files_both_formats', fname)
        # layouts / masters: links and transitions there
        for nm in sorted(pk.names):
            if re.match(r'ppt/(slideLayouts|slideMasters)/[^/]+\.xml$', nm):
                r = pk.xml(nm)
                if r is None:
                    continue
                kind = 'layout' if 'Layout' in nm else 'master'
                scan_links(pk, nm, r, fname, kind)
                if r.find(q('p:transition')) is not None or any(
                        ch.find(q('p:transition')) is not None for ac in r.findall(q('mc:AlternateContent')) for ch in ac):
                    ex('transition.on_%s' % kind, fname)
    for k in sorted(C):
        print('%-80s %d' % (k, C[k]))
        for e in EX.get(k, []):
            print('      e.g. %s' % (e,))


if __name__ == '__main__':
    main(sys.argv[1:])
