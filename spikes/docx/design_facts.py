"""Corpus facts behind the design questions of ADR-0004 / docs/roadmap/docx.md.

Usage: python3 -I design_facts.py <dir of .docx> [<dir> ...]

Read-only: zipfile + ElementTree; input is untrusted data and is never executed.
Parts with a DOCTYPE are skipped (counted), never parsed.

Reports, over all story parts (document, headers, footers, footnotes, endnotes,
comments):
  Q1 text across runs: where text-bearing w:r live (parent element), how many
     adjacent text-run pairs in a paragraph have identical w:rPr (an rsid-only
     split: mergeable without changing formatting) vs different w:rPr, and what
     sits between them (proofErr, bookmark, ...).
  Q2 fields: complex fields with/without a cached result, nesting, instruction
     split over several instrText runs, fields spanning paragraphs, fldSimple,
     settings/updateFields, fldChar/@dirty.
  Q3 tracked changes: text runs inside w:ins / w:del / moveFrom / moveTo,
     paragraph-mark revisions (w:rPr/w:ins|w:del under w:pPr).
  Q4 ids: duplicates of bookmark ids per part and across parts, ins/del ids that
     collide with bookmark ids, comment ids vs commentsExtended paraIds, w14:paraId
     duplicates.
  Q5 CT_OnOff lexical forms of w:val across all w-namespace on/off-shaped elements
     under rPr/pPr (b, i, caps, keepNext, ...).
  Q6 sectPr placement.
"""
import sys, os, re, zipfile, collections
import xml.etree.ElementTree as ET

W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main'
W14 = 'http://schemas.microsoft.com/office/word/2010/wordml'
W15 = 'http://schemas.microsoft.com/office/word/2012/wordml'
W15_PRE = 'http://schemas.microsoft.com/office/word/2010/11/wordml'  # Office 2013 pre-release URI, seen in real files
MC = 'http://schemas.openxmlformats.org/markup-compatibility/2006'


def w(l): return '{%s}%s' % (W, l)


STORY = re.compile(r'word/(document|header\d*|footer\d*|footnotes|endnotes|comments)\.xml$')
ONOFF = {w(x) for x in ('b', 'bCs', 'i', 'iCs', 'caps', 'smallCaps', 'strike', 'dstrike', 'outline', 'shadow',
                        'emboss', 'imprint', 'noProof', 'snapToGrid', 'vanish', 'webHidden', 'specVanish',
                        'keepNext', 'keepLines', 'pageBreakBefore', 'widowControl', 'suppressLineNumbers',
                        'suppressAutoHyphens', 'contextualSpacing', 'bidi', 'rtl', 'cs', 'titlePg',
                        'mirrorIndents', 'adjustRightInd', 'autoSpaceDE', 'autoSpaceDN', 'kinsoku',
                        'wordWrap', 'overflowPunct', 'topLinePunct', 'snapToGrid')}

C = collections.Counter()
between = collections.Counter()
run_parent = collections.Counter()
onoff_val = collections.Counter()
examples = collections.defaultdict(list)


def rpr_key(r):
    rp = r.find(w('rPr'))
    if rp is None: return ''
    return ET.tostring(rp)


def has_text(r):
    return any(c.tag in (w('t'), w('tab')) for c in r)


def scan_part(fname, pn, root, ids):
    parent = {c: p for p in root.iter() for c in p}
    # Q1 / Q3 run parents
    for r in root.iter(w('r')):
        if not (has_text(r) or r.find(w('delText')) is not None): continue
        p = parent.get(r)
        run_parent[p.tag.split('}')[1] if p is not None else '?'] += 1
        if p is not None and p.tag in (w('ins'), w('del'), w('moveFrom'), w('moveTo')):
            C['Q3 text runs inside ' + p.tag.split('}')[1]] += 1
    for p in root.iter(w('p')):
        kids = list(p)
        # Q1: adjacent text runs (direct children only, as a Phase0 API would see them)
        last = None; gap = []
        for k in kids:
            if k.tag == w('r') and has_text(k):
                if last is not None:
                    same = rpr_key(last) == rpr_key(k)
                    C['Q1 adjacent text-run pairs'] += 1
                    C['Q1   ... identical rPr (rsid-only split)' if same else 'Q1   ... different rPr'] += 1
                    for g in gap: between[g] += 1
                    if not gap: between['(nothing)'] += 1
                last = k; gap = []
            elif k.tag == w('pPr'):
                continue
            elif k.tag == w('r'):
                gap.append('r without text (%s)' % ','.join(sorted({c.tag.split('}')[1] for c in k if c.tag != w('rPr')})) or 'empty')
            else:
                gap.append(k.tag.split('}')[1])
        # Q3 paragraph-mark revision
        ppr = p.find(w('pPr'))
        if ppr is not None:
            rpr = ppr.find(w('rPr'))
            if rpr is not None and (rpr.find(w('ins')) is not None or rpr.find(w('del')) is not None):
                C['Q3 paragraph-mark revisions (pPr/rPr/ins|del)'] += 1
            if ppr.find(w('sectPr')) is not None: C['Q6 sectPr in pPr (section break paragraph)'] += 1
        pid = p.get('{%s}paraId' % W14)
        if pid: ids['paraId'][pid] += 1
    # Q2 fields: walk fldChar in document order across the part
    stack = []; para_of = {}
    for p in root.iter(w('p')):
        for d in p.iter(): para_of[d] = p
    instr_runs = 0
    for el in root.iter():
        if el.tag == w('fldChar'):
            t = el.get(w('fldCharType'))
            if el.get(w('dirty')) in ('1', 'true', 'on'): C['Q2 fldChar dirty=true'] += 1
            if t == 'begin':
                if stack: C['Q2 nested complex fields'] += 1
                stack.append({'p': para_of.get(el), 'sep': False, 'instr': 0, 'code': ''})
            elif t == 'separate' and stack:
                stack[-1]['sep'] = True
            elif t == 'end' and stack:
                f = stack.pop()
                C['Q2 complex fields'] += 1
                C['Q2   ... with cached result (separate present)' if f['sep'] else 'Q2   ... without separate'] += 1
                if f['p'] is not para_of.get(el):
                    C['Q2   ... spanning paragraphs'] += 1
                    examples['field spanning paragraphs'].append((fname, f['code'].strip()[:30]))
                if f['instr'] > 1: C['Q2   ... instruction split over >1 instrText'] += 1
                code = f['code'].strip().split(' ')[0].upper() if f['code'].strip() else '?'
                C['Q2   code ' + code] += 1
        elif el.tag == w('instrText') and stack:
            stack[-1]['instr'] += 1; stack[-1]['code'] += el.text or ''
        elif el.tag == w('fldSimple'):
            C['Q2 fldSimple'] += 1
    if stack: C['Q2 unterminated complex field at end of part'] += len(stack)
    # Q4 ids
    for el in root.iter():
        if el.tag == w('bookmarkStart'):
            ids['bm:' + pn][el.get(w('id'))] += 1
            ids['bm_all'][el.get(w('id'))] += 1
            ids['bm_name'][el.get(w('name'))] += 1
        elif el.tag in (w('ins'), w('del'), w('moveFrom'), w('moveTo'), w('rPrChange'), w('pPrChange')):
            ids['rev'][el.get(w('id'))] += 1
        elif el.tag == w('comment'):
            ids['comment'][el.get(w('id'))] += 1
            ps = el.findall(w('p'))
            if ps and ps[-1].get('{%s}paraId' % W14): ids['comment_last_paraId'][ps[-1].get('{%s}paraId' % W14)] += 1
        elif el.tag in ONOFF and parent.get(el) is not None and parent[el].tag in (w('rPr'), w('pPr'), w('sectPr')):
            v = el.get(w('val'))
            onoff_val['(absent)' if v is None else v] += 1
    # Q6 body
    body = root.find(w('body'))
    if body is not None:
        kids = list(body)
        if kids and kids[-1].tag == w('sectPr'): C['Q6 body: sectPr is last child'] += 1
        elif any(k.tag == w('sectPr') for k in kids): C['Q6 body: sectPr present but not last'] += 1
        else: C['Q6 body: no sectPr'] += 1
        if any(k.tag == '{%s}AlternateContent' % MC for k in kids): C['Q6 body: has mc:AlternateContent child'] += 1


def scan_file(path):
    fname = os.path.basename(path)
    try: z = zipfile.ZipFile(path)
    except Exception: C['files: not a zip (e.g. encrypted CFB)'] += 1; return
    C['files'] += 1
    ids = collections.defaultdict(collections.Counter)
    names = z.namelist()
    for pn in names:
        if not STORY.match(pn): continue
        raw = z.read(pn)
        if b'<!DOCTYPE' in raw[:4096]: C['parts with DOCTYPE (skipped)'] += 1; continue
        try: root = ET.fromstring(raw)
        except Exception: C['parts not well-formed'] += 1; continue
        scan_part(fname, pn, root, ids)
    if 'word/settings.xml' in names:
        s = z.read('word/settings.xml')
        if b'updateFields' in s: C['Q2 settings with updateFields'] += 1
    if 'word/commentsExtended.xml' in names:
        try:
            ce = ET.fromstring(z.read('word/commentsExtended.xml'))
            ext = {e.get('{%s}paraId' % ns) for ns in (W15, W15_PRE) for e in ce.iter('{%s}commentEx' % ns)}
            if any(True for _ in ce.iter('{%s}commentEx' % W15_PRE)): C['Q4 files using the pre-release w15 URI (2010/11/wordml)'] += 1
            last = set(ids['comment_last_paraId'])
            C['Q4 commentEx entries'] += len(ext)
            C['Q4 commentEx paraId matching a comment\'s last paragraph'] += len(ext & last)
        except Exception: pass
    # per-file ID facts
    per_part_dup = any(v > 1 for k, cnt in ids.items() if k.startswith('bm:') for v in cnt.values())
    if per_part_dup: C['Q4 files with duplicate bookmark id within one part'] += 1
    if any(v > 1 for v in ids['bm_all'].values()) and not per_part_dup:
        C['Q4 files where bookmark ids repeat only across parts'] += 1
    if ids['bm_all'] and ids['rev'] and set(ids['bm_all']) & set(ids['rev']):
        C['Q4 files where a revision id equals a bookmark id'] += 1
    if ids['rev'] and any(v > 1 for v in ids['rev'].values()): C['Q4 files with duplicate revision ids'] += 1
    if any(v > 1 for v in ids['paraId'].values()): C['Q4 files with duplicate w14:paraId'] += 1
    if ids['paraId']: C['Q4 files with w14:paraId'] += 1
    if any(v > 1 for k, v in ids['bm_name'].items() if k): C['Q4 files with duplicate bookmark name'] += 1


def main():
    for d in sys.argv[1:]:
        for fn in sorted(os.listdir(d)):
            if fn.lower().endswith(('.docx', '.docm', '.dotx')): scan_file(os.path.join(d, fn))
    for k in sorted(C): print('%-70s %8d' % (k, C[k]))
    print('\nQ1 what separates adjacent text runs (top 15):')
    for k, v in between.most_common(15): print('  %-60s %6d' % (k, v))
    print('\nQ1/Q3 parent of text-bearing runs:')
    for k, v in run_parent.most_common(): print('  %-30s %8d' % (k, v))
    print('\nQ5 CT_OnOff w:val lexical forms:')
    for k, v in onoff_val.most_common(): print('  %-12s %8d' % (k, v))
    for k, v in examples.items(): print('\nexamples:', k, v[:5])


if __name__ == '__main__':
    main()
