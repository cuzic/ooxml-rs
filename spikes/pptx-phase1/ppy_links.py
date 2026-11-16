"""Differential preview of ADR-0008 §3.1 (hyperlinks/actions) and §2
(notes text) against python-pptx 1.0.2 (run with `python3 -I`).

Usage: python3 -I ppy_links.py <site-dir with python-pptx 1.0.2> <pptx files/dirs...>

Model (this script, from the XML): for every shape on a slide (groups
recursed, AC skipped) the p:cNvPr/a:hlinkClick -> (kind, target) where kind
follows ADR-0008 §3.1 (`action` absent -> Hyperlink to the r:id target;
hlinksldjump -> Slide(sldId of the target part); hlinkshowjump?jump=X ->
Jump(X); other verbs -> Other(verb)); for every a:r the a:rPr/a:hlinkClick
external address. python-pptx: shape.click_action.action / .hyperlink.address
/ .target_slide.slide_id and run.hyperlink.address. Also compares notes
text (python-pptx notes_slide.notes_text_frame.text vs the model's body
placeholder text with a:br as "\\v").
"""
import sys, os, re, collections
site = sys.argv[1]
sys.path.insert(0, site)
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import Pkg, q, iter_files, walk_shapes, sptree, ph_of, RT_NOTES
from pptx import Presentation
from pptx.enum.action import PP_ACTION

C = collections.Counter(); D = []
JUMP = {'firstslide': PP_ACTION.FIRST_SLIDE, 'lastslide': PP_ACTION.LAST_SLIDE, 'nextslide': PP_ACTION.NEXT_SLIDE,
        'previousslide': PP_ACTION.PREVIOUS_SLIDE, 'endshow': PP_ACTION.END_SHOW, 'lastslideviewed': PP_ACTION.LAST_SLIDE_VIEWED}
VERB = {'hlinkpres': PP_ACTION.PLAY, 'hlinkfile': PP_ACTION.OPEN_FILE, 'customshow': PP_ACTION.NAMED_SLIDE_SHOW,
        'ole': PP_ACTION.OLE_VERB, 'macro': PP_ACTION.RUN_MACRO, 'program': PP_ACTION.RUN_PROGRAM}


def model_link(pk, part, h, slide_ids):
    rels = {i: (ty, t, e) for i, ty, t, e in pk.rels(part)}
    a = h.get('action')
    rid = h.get(q('r:id'))
    tgt = rels.get(rid)
    if not a:
        return ('Hyperlink', tgt[1] if tgt and tgt[2] else None)
    m = re.match(r'ppaction://([a-zA-Z]+)(?:\?(.*))?$', a)
    verb = m.group(1) if m else None
    if verb == 'hlinksldjump':
        return ('Slide', slide_ids.get(tgt[1]) if tgt and not tgt[2] else None)
    if verb == 'hlinkshowjump':
        return ('Jump', dict(x.split('=', 1) for x in (m.group(2) or '').split('&') if '=' in x).get('jump'))
    return ('Other', verb)


def main():
    for path in iter_files(sys.argv[2:]):
        f = os.path.basename(path)
        try:
            pk = Pkg(path); prs = Presentation(path); pslides = list(prs.slides)
        except Exception:
            C['skipped (unreadable by either)'] += 1; continue
        slides = pk.slides()
        slide_ids = {p: int(s) for s, p in slides if p}
        by_id = {str(s.slide_id): s for s in pslides}
        for sid, part in slides:
            root = pk.xml(part) if part else None
            ps = by_id.get(sid)
            if root is None or ps is None:
                continue
            pshapes = {}

            def rec(shapes):
                for sh in shapes:
                    pshapes[str(sh.shape_id)] = sh
                    if sh.shape_type is not None and hasattr(sh, 'shapes'):
                        rec(sh.shapes)
            rec(ps.shapes)
            for s, _d, _g in walk_shapes(sptree(root)):
                cnv = s.find('.//' + q('p:cNvPr'))
                h = cnv.find(q('a:hlinkClick')) if cnv is not None else None
                sh = pshapes.get(cnv.get('id')) if cnv is not None else None
                if sh is None:
                    continue
                try:
                    ca = sh.click_action
                    pa = ca.action
                except Exception as e:
                    C['shape: python-pptx raised %s' % type(e).__name__] += 1; continue
                if h is None:
                    C['shape: no link, agree' if pa == PP_ACTION.NONE else 'shape: no link, DIFFER'] += 1
                    continue
                kind, val = model_link(pk, part, h, slide_ids)
                if kind == 'Hyperlink':
                    ok = pa == PP_ACTION.HYPERLINK and (ca.hyperlink.address == val)
                elif kind == 'Slide':
                    ts = ca.target_slide
                    ok = pa == PP_ACTION.NAMED_SLIDE and ts is not None and ts.slide_id == val
                elif kind == 'Jump':
                    ok = pa == JUMP.get(val)
                else:
                    ok = pa == VERB.get(val, PP_ACTION.NONE)
                C['shape link %s: %s' % (kind, 'agree' if ok else 'DIFFER')] += 1
                if not ok:
                    D.append((f, sid, cnv.get('id'), kind, val, pa))
            # runs (top-level p:sp only, as python-pptx's text frames)
            for s in root.iter(q('p:sp')):
                cnv = s.find('.//' + q('p:cNvPr'))
                sh = pshapes.get(cnv.get('id')) if cnv is not None else None
                if sh is None or not sh.has_text_frame:
                    continue
                xr = [r for p in s.iter(q('a:p')) for r in p.findall(q('a:r'))]
                pr = [r for p in sh.text_frame.paragraphs for r in p.runs]
                if len(xr) != len(pr):
                    C['runs: count differs (skipped)'] += 1; continue
                for x, r in zip(xr, pr):
                    rpr = x.find(q('a:rPr'))
                    h = rpr.find(q('a:hlinkClick')) if rpr is not None else None
                    mine = model_link(pk, part, h, slide_ids)[1] if h is not None and not h.get('action') else None
                    try:
                        theirs = r.hyperlink.address
                    except KeyError:
                        C['run link: python-pptx raised KeyError (action with empty r:id)'] += 1
                        continue
                    if h is not None and h.get('action'):
                        C['run link: action %s (python-pptx reports address %r)' % (h.get('action').split('?')[0], theirs)] += 1
                        continue
                    if h is None and theirs is None:
                        continue
                    C['run link: %s' % ('agree' if mine == theirs else 'DIFFER')] += 1
                    if mine != theirs:
                        D.append((f, sid, 'run', mine, theirs))
            # notes
            notes = pk.rel_targets(part, RT_NOTES)
            has = ps.has_notes_slide
            if bool(notes) != has:
                C['notes: presence DIFFER'] += 1; continue
            if not notes:
                continue
            nroot = pk.xml(notes[0])
            body = None
            for s, _d, _g in walk_shapes(sptree(nroot)):
                ph = ph_of(s)
                if ph is not None and ph.get('type') == 'body':
                    body = s; break
            tf = ps.notes_slide.notes_text_frame
            if body is None or tf is None:
                C['notes: no body placeholder, agree' if (body is None) == (tf is None) else 'notes: body DIFFER'] += 1
                continue
            paras = []
            for p in body.find(q('p:txBody')).findall(q('a:p')):
                t = ''
                for c in p:
                    if c.tag in (q('a:r'), q('a:fld')):
                        e = c.find(q('a:t')); t += (e.text or '') if e is not None else ''
                    elif c.tag == q('a:br'):
                        t += '\v'
                paras.append(t)
            mine = '\n'.join(paras)
            C['notes text: %s' % ('agree' if mine == tf.text else 'DIFFER')] += 1
            if mine != tf.text:
                D.append((f, sid, 'notes', mine[:40], tf.text[:40]))
    for k in sorted(C):
        print('%-48s %d' % (k, C[k]))
    for d in D[:40]:
        print('  ', d)


if __name__ == '__main__':
    main()
