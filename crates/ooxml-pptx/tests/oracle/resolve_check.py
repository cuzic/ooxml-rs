"""ADR-0008 §7 O1 and O3, the independent half: python-pptx 1.0.2 reads
every fixture and its

  O1  inherited placeholder geometry (left/top/width/height; python-pptx
      resolves slide -> layout by idx, layout -> master by type) must equal
      ooxml-pptx's resolved geometry, except the committed, hand-checked
      differences in geometry_known_differences.json (ADR-0008 §1.2 decides
      them differently on purpose);
  O3  shape click actions (action, URL address, target slide id) and run
      hyperlink addresses must equal ooxml-pptx's links (runs whose
      hyperlink python-pptx cannot read, an action with an empty r:id, are
      skipped);
  §2  each notes slide's text (python-pptx notes_text_frame.text, None
      without a body placeholder) must equal ooxml-pptx's NotesRef text.

The known differences are keyed by fixture path, so on another corpus
they are reported as "no longer differs" (#56 m6); CI runs it on the
fixtures only.

    python3 -I resolve_check.py [--site DIR] DUMP.json FIXTURES_DIR
"""
import json
import os
import sys


def main():
    args = sys.argv[1:]
    site = None
    if args[:1] == ['--site']:
        site, args = args[1], args[2:]
    if site:
        sys.path.insert(0, site)
    from pptx import Presentation
    from pptx.enum.action import PP_ACTION
    from pptx.shapes.group import GroupShape
    dump_path, fixtures = args
    dump = json.load(open(dump_path, encoding='utf-8'))
    here = os.path.dirname(os.path.abspath(__file__))
    known = {(k['fixture'], k['slide_id'], k['shape_id']): k for k in json.load(open(os.path.join(here, 'geometry_known_differences.json'), encoding='utf-8'))}
    jump = {'firstslide': PP_ACTION.FIRST_SLIDE, 'lastslide': PP_ACTION.LAST_SLIDE, 'nextslide': PP_ACTION.NEXT_SLIDE,
            'previousslide': PP_ACTION.PREVIOUS_SLIDE, 'endshow': PP_ACTION.END_SHOW, 'lastslideviewed': PP_ACTION.LAST_SLIDE_VIEWED}
    verb = {'hlinkpres': PP_ACTION.PLAY, 'hlinkfile': PP_ACTION.OPEN_FILE, 'customshow': PP_ACTION.NAMED_SLIDE_SHOW,
            'ole': PP_ACTION.OLE_VERB, 'macro': PP_ACTION.RUN_MACRO, 'program': PP_ACTION.RUN_PROGRAM}
    failures, geo_n, link_n, used_known, total_notes = 0, 0, 0, set(), 0
    for fixture, d in dump.items():
        prs = Presentation(os.path.join(fixtures, fixture))
        by_id = {}
        for sldId in prs.part._element.sldIdLst.sldId_lst:
            by_id[sldId.id] = None
        slides = {s.slide_id: s for s in prs.slides}

        def shapes_of(sid, notes=False):
            out = {}

            def walk(shapes):
                for sh in shapes:
                    out[sh.shape_id] = sh
                    if isinstance(sh, GroupShape):
                        walk(sh.shapes)
            if notes:
                walk(slides[sid].notes_slide.shapes)
            else:
                walk(slides[sid].shapes)
            return out
        for g in d['geometry']:
            sh = shapes_of(g['slide_id'], g.get('notes', False)).get(g['shape_id'])
            if sh is None:
                continue
            theirs = [sh.left, sh.top, sh.width, sh.height]
            theirs = None if all(v is None for v in theirs) else theirs
            geo_n += 1
            key = (fixture, g['slide_id'], g['shape_id']) if not g.get('notes') else (fixture, g['slide_id'], g['shape_id'], 'notes')
            if theirs != g['xywh']:
                if key in known:
                    used_known.add(key)
                    continue
                failures += 1
                print(f'FAIL O1 {fixture} slide {g["slide_id"]} shape {g["shape_id"]}: ooxml {g["xywh"]} ({g["source"]}) vs python-pptx {theirs}')
        for ln in d['links']:
            sh = shapes_of(ln['slide_id']).get(ln['shape_id'])
            if sh is None:
                continue
            t = ln['target']
            link_n += 1
            if ln['run'] is None:
                ca = sh.click_action
                a = ca.action
                if t['kind'] == 'url':
                    ok = a == PP_ACTION.HYPERLINK and ca.hyperlink.address == t['address']
                elif t['kind'] == 'slide':
                    ok = a == PP_ACTION.NAMED_SLIDE and ca.target_slide.slide_id == t['slide_id']
                elif t['kind'] == 'jump':
                    ok = a == jump[t['jump']]
                elif t['kind'] == 'verb':
                    ok = a == verb.get(t['verb'], PP_ACTION.NONE)
                else:
                    ok = True
            else:
                runs = [r for p in sh.text_frame.paragraphs for r in p.runs]
                if ln.get('action'):
                    continue  # python-pptx reports a relative part name or raises
                try:
                    addr = runs[ln['run']].hyperlink.address
                except KeyError:
                    continue
                ok = t['kind'] == 'url' and addr == t['address']
            if not ok:
                failures += 1
                print(f'FAIL O3 {fixture} slide {ln["slide_id"]} shape {ln["shape_id"]} run {ln["run"]}: {t}')
        notes_n = 0
        for nt in d.get('notes', []):
            sl = slides[nt['slide_id']]
            tf = sl.notes_slide.notes_text_frame if sl.has_notes_slide else None
            theirs = None if tf is None else tf.text
            notes_n += 1
            if theirs != nt['text']:
                failures += 1
                print(f'FAIL notes {fixture} slide {nt["slide_id"]}: ooxml {nt["text"]!r} vs python-pptx {theirs!r}')
        total_notes += notes_n
    stale = set(known) - used_known
    for k in sorted(stale):
        failures += 1
        print(f'FAIL known difference no longer differs (remove it): {k}')
    print(f'O1 geometry: {geo_n} placeholders ({len(used_known)} known differences); O3 links: {link_n}; notes text: {total_notes}; {failures} failures')
    sys.exit(1 if failures else 0)


if __name__ == '__main__':
    main()
