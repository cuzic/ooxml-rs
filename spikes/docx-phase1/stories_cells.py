"""ADR-0007 §2 and §3 evidence: story parts other than the main document, and table cells.

Usage: python3 -I stories_cells.py [--json out.json] <dir or file>...

Counts over files that pass the Phase0 document gates (p1common.gate):
 stories  per kind (header, footer, footnotes, endnotes, comments): parts, paragraphs with
          text, text runs, settable runs under the Phase1 run rule, refusal reasons; header and
          footer parts shared by several sections; note kinds (separator, continuationSeparator,
          continuationNotice, normal) and where the footnoteRef/annotationRef mark sits.
 cells    per w:tc in any story: number of paragraphs and nested tables, vMerge/gridSpan/hMerge,
          cells inside a row- or cell-level content control, and the outcome of the proposed
          set_cell_text rule (§3); for empty paragraphs, what their mark run properties hold.
"""
import collections
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import p1common as C  # noqa: E402

w, local = C.w, C.local


def cell_outcome(tc, paras, ancestors):
    """Proposed set_cell_text outcome for one cell (ADR-0007 §3)."""
    if any(a.tag in (w('sdt'), w('customXml')) for a in ancestors):
        return 'UnsupportedContext (row/cell-level sdt or customXml)'
    tcpr = tc.find(w('tcPr'))
    if tcpr is not None:
        vm = tcpr.find(w('vMerge'))
        if vm is not None and vm.get(w('val')) in (None, 'continue'):
            return 'MergedContinuation'
        hm = tcpr.find(w('hMerge'))
        if hm is not None and hm.get(w('val')) in (None, 'continue'):
            return 'MergedContinuation'
    blocks = [c for c in tc if c.tag not in (w('tcPr'),) and c.tag not in C.ANCHORS]
    if any(c.tag == w('tbl') for c in blocks):
        return 'CellNotSimple (nested table)'
    if any(c.tag not in (w('p'),) for c in blocks):
        return 'CellNotSimple (%s)' % ','.join(sorted({local(c.tag) for c in blocks if c.tag != w('p')}))
    ps = [c for c in blocks if c.tag == w('p')]
    if len(ps) != 1:
        return 'CellNotSimple (%d paragraphs)' % len(ps) if ps else 'CellNotSimple (no paragraph)'
    para = paras[0] if paras else None
    return paragraph_outcome(para)


REVISION_MARK = {w('ins'), w('del'), w('moveFrom'), w('moveTo'), w('rPrChange'),
                 '{http://schemas.microsoft.com/office/word/2010/wordml}conflictIns',
                 '{http://schemas.microsoft.com/office/word/2010/wordml}conflictDel'}


def mark_copy(p):
    """Outcome of creating a run in an empty paragraph: the run gets a copy of the paragraph
    mark's run properties (w:pPr/w:rPr) minus the CT_ParaRPr-only children (ADR-0007 §3.2)."""
    ppr = p.find(w('pPr'))
    rpr = ppr.find(w('rPr')) if ppr is not None else None
    if rpr is None:
        return 'Ok (empty paragraph: new run, no mark rPr)'
    kids = list(rpr)
    if any(k.tag in REVISION_MARK for k in kids):
        return 'InsideRevision (paragraph mark)'
    for k in kids:
        if k.tag in (w('vanish'), w('specVanish'), w('webHidden')) and C.onoff_on(k):
            return 'MarkHidden'
        if k.tag == w('eastAsianLayout'):
            return 'UnsupportedMarkProperties (eastAsianLayout id)'
        if C.ns(k.tag) != C.W:
            return 'UnsupportedMarkProperties (%s)' % C.ns(k.tag).rsplit('/', 2)[-2]
    return 'Ok (empty paragraph: new run, mark rPr copied)'


def paragraph_outcome(para):
    """Proposed set_paragraph_text outcome (ADR-0007 §3.1): replace of the whole text under
    the §1 rules, or a run created in an empty paragraph."""
    from replace_sim import Stream
    runs = para.runs()
    if not runs:
        if any(t[0] == 'hard' or t[0] == 'enter' for t in para.tokens):
            return 'CellNotSimple (non-run content)'
        return mark_copy(para.el)
    st = Stream(para)
    if not st.text:
        refs = [r.refusal(False) for r in runs if r.refusal(False)]
        if refs:
            return refs[0].split(' ', 1)[1].split(':')[0]
        if any(t[0] in ('hard', 'enter') for t in para.tokens):
            return 'CellNotSimple (non-run content)'
        return 'Ok (runs without text: first run gets the text)'
    o = st.classify(0, len(st.text))
    # anything outside the matched text (a hard element, a refused run without text) also refuses
    if o.startswith('Ok'):
        extra = [t for t in para.tokens if t[0] == 'hard']
        if extra:
            return 'CellNotSimple (non-run content)'
        bad = [r.refusal(False) for r in runs if r.refusal(False)]
        if bad:
            return bad[0].split(' ', 1)[1].split(':')[0]
    return o.split(':')[0]


def main(argv):
    out_json, args = None, []
    it = iter(argv)
    for a in it:
        if a == '--json':
            out_json = next(it)
        else:
            args.append(a)
    st = collections.Counter()
    refusals = collections.Counter()
    notes = collections.Counter()
    hf = collections.Counter()
    cells = collections.Counter()
    empty_mark = collections.Counter()
    files_with = collections.Counter()
    fields_by_story = collections.Counter()
    for f in C.corpus_files(args):
        pkg = C.Pkg(f)
        if C.gate(pkg)[0]:
            continue
        st['files'] += 1
        kinds = set()
        # header/footer sharing: references from every sectPr in the main part
        refs = collections.Counter()
        rid2part = {rid: tgt for rid, _, tgt in pkg.rels}
        for sp in pkg.doc.iter(w('sectPr')):
            for ref in list(sp.findall(w('headerReference'))) + list(sp.findall(w('footerReference'))):
                refs[rid2part.get(ref.get('{%s}id' % C.R))] += 1
                hf['reference type ' + (ref.get(w('type')) or '<absent>')] += 1
        for sw in C.iter_stories(pkg):
            kinds.add(sw.kind)
            k = sw.kind
            st[k, 'parts'] += 1
            if k in ('header', 'footer'):
                n = refs.get(sw.part, 0)
                hf['%s parts referenced by %s sectPr refs' % (k, '0' if n == 0 else '1' if n == 1 else '2+')] += 1
                if any(e.tag in (w('drawing'), w('pict'), '{%s}AlternateContent' % C.MC) for e in sw.root.iter()):
                    hf['%s parts with drawings/VML/AC (watermarks, logos)' % k] += 1
            for fl in C.scan_fields(sw.root, k):
                fields_by_story[k, fl.key] += 1
            if k in ('footnotes', 'endnotes'):
                for n in sw.root:
                    if local(n.tag) in ('footnote', 'endnote'):
                        notes[k, n.get(w('type')) or 'normal'] += 1
                        if (n.get(w('type')) or 'normal') == 'normal':
                            mark = [r for r in n.iter(w('r')) if r.find(w(local(n.tag) + 'Ref')) is not None]
                            if mark:
                                kids = [local(x.tag) for x in mark[0] if x.tag != w('rPr')]
                                notes[k, 'mark run content', '+'.join(kids)] += 1
            if k == 'comments':
                for c in sw.root:
                    if local(c.tag) == 'comment':
                        notes[k, 'comment'] += 1
                        mark = [r for r in c.iter(w('r')) if r.find(w('annotationRef')) is not None]
                        if mark:
                            notes[k, 'annotationRef run content', '+'.join(local(x.tag) for x in mark[0] if x.tag != w('rPr'))] += 1
            for para in sw.paras:
                runs = para.runs()
                textruns = [r for r in runs if r.has_text()]
                if textruns:
                    st[k, 'paragraphs with text'] += 1
                for r in textruns:
                    st[k, 'text runs'] += 1
                    ref = r.refusal(False)
                    note_type = para.note.get(w('type')) if para.note is not None else None
                    if note_type and note_type != 'normal':
                        ref = 'R3 UnsupportedContext:separator note'
                    if ref is None:
                        st[k, 'settable text runs'] += 1
                    else:
                        refusals[k, ref.split(':')[0] + (':' + ref.split(':')[1] if ':' in ref else '')] += 1
                    if ref and r.refusal(True) is None:
                        st[k, 'settable only with a smartTag rule (not proposed)'] += 1
            # cells: need ancestors (sdt at row/cell level)
            parent = {c: p for p in sw.root.iter() for c in p}
            for tc, paras in sw.cells:
                anc, a = [], parent.get(tc)
                while a is not None and a.tag != w('tbl'):
                    anc.append(a); a = parent.get(a)
                own = [p for p in paras if parent.get(p.el) is tc]
                o = cell_outcome(tc, own, anc)
                cells[k, o] += 1
                cells['all', o.split(' (')[0] if not o.startswith('Ok') else o] += 1
            for para in sw.paras:
                if para.runs() or para.el.find(w('r')) is not None:
                    continue
                o = mark_copy(para.el)
                cells['empty paragraphs (any story)', o] += 1
            for para in sw.paras:
                if para.runs():
                    continue
                ppr = para.el.find(w('pPr'))
                rpr = ppr.find(w('rPr')) if ppr is not None else None
                where = 'cell' if para.cell is not None else 'other'
                if rpr is None:
                    empty_mark[where, 'no mark rPr'] += 1
                else:
                    kids = {local(x.tag) for x in rpr}
                    rev = kids & {'ins', 'del', 'moveFrom', 'moveTo', 'rPrChange'}
                    empty_mark[where, 'mark rPr with revision children' if rev else 'mark rPr (formatting only)'] += 1
                    for kk in kids:
                        empty_mark['child', kk] += 1
        for kd in kinds:
            files_with[kd] += 1
    rep = {
        'files': st['files'],
        'files_with_story': dict(files_with),
        'stories': {'%s/%s' % k: v for k, v in sorted((k, v) for k, v in st.items() if isinstance(k, tuple))},
        'refusals': {'%s/%s' % k: v for k, v in sorted(refusals.items())},
        'notes_and_comments': {'/'.join(k): v for k, v in sorted(notes.items())},
        'headers_footers': dict(sorted(hf.items())),
        'fields_by_story': {'%s/%s' % k: v for k, v in sorted(fields_by_story.items()) if k[0] != 'main'},
        'cells': {'%s/%s' % k: v for k, v in sorted(cells.items())},
        'empty_paragraph_mark': {'%s/%s' % k: v for k, v in sorted(empty_mark.items(), key=lambda x: (-x[1]))[:40]},
    }
    print(json.dumps(rep, indent=1, ensure_ascii=False))
    if out_json:
        with open(out_json, 'w') as fh:
            json.dump(rep, fh, indent=1, ensure_ascii=False)


if __name__ == '__main__':
    main(sys.argv[1:])
