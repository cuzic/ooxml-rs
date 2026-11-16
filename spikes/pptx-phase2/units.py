"""Spike for ADR-0012 (PowerPoint Phase 2: slides and shapes as units).

Measures, over untrusted .pptx files (zipfile + xml.etree only; nothing is
executed), what adding/removing/reordering slides and adding shapes must
respect: slide ids, presentation rIds, part names, layout/notes/media
sharing, sections, custom shows, slide-jump links, shape ids, content
types, app.xml.

    python3 -I units.py <dirs/files...> [--json out.json]

Files are de-duplicated by SHA-256; unreadable files are counted and skipped.
"""
import collections
import hashlib
import json
import os
import posixpath
import re
import sys
import zipfile
import xml.etree.ElementTree as ET

NS = {
    'p': 'http://schemas.openxmlformats.org/presentationml/2006/main',
    'a': 'http://schemas.openxmlformats.org/drawingml/2006/main',
    'r': 'http://schemas.openxmlformats.org/officeDocument/2006/relationships',
    'rel': 'http://schemas.openxmlformats.org/package/2006/relationships',
    'ct': 'http://schemas.openxmlformats.org/package/2006/content-types',
    'p14': 'http://schemas.microsoft.com/office/powerpoint/2010/main',
    'mc': 'http://schemas.openxmlformats.org/markup-compatibility/2006',
    'ep': 'http://schemas.openxmlformats.org/officeDocument/2006/extended-properties',
    'vt': 'http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes',
}
R = NS['r']
RT = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships/'
RT_SLIDE = RT + 'slide'
RT_LAYOUT = RT + 'slideLayout'
RT_NOTES = RT + 'notesSlide'
RT_IMAGE = RT + 'image'
RT_MEDIA = ('http://schemas.microsoft.com/office/2007/relationships/media', RT + 'audio', RT + 'video')
RT_OFFICEDOC = RT + 'officeDocument'
RT_EXT = RT + 'extended-properties'


def q(t):
    p, l = t.split(':')
    return '{%s}%s' % (NS[p], l)


def files(args):
    for a in args:
        if os.path.isdir(a):
            for root, _, names in os.walk(a):
                for n in sorted(names):
                    if n.lower().endswith(('.pptx', '.pptm', '.potx', '.ppsx')):
                        yield os.path.join(root, n)
        else:
            yield a


def rels_name(part):
    d, b = posixpath.split(part)
    return posixpath.join(d, '_rels', b + '.rels')


def read_rels(z, names, part):
    rn = rels_name(part)
    if rn not in names:
        return []
    out = []
    root = ET.fromstring(z.read(rn))
    base = posixpath.dirname(part)
    for r in root.findall('rel:Relationship', NS):
        ext = r.get('TargetMode') == 'External'
        tgt = r.get('Target', '')
        if not ext:
            tgt = posixpath.normpath(posixpath.join('/' + base, tgt)).lstrip('/') if not tgt.startswith('/') else tgt.lstrip('/')
        out.append((r.get('Id'), r.get('Type'), tgt, ext))
    return out


DELETE_CASCADE = {'slideLayout', 'notesSlide', 'comments', 'image', 'media', 'audio', 'video', 'hdphoto',
                  'hyperlink', 'slide', 'tags', 'vmlDrawing', 'chart', 'oleObject', 'package', 'themeOverride',
                  'diagramData', 'diagramLayout', 'diagramQuickStyle', 'diagramColors', 'diagramDrawing'}
DUPLICATE_OK = {'slideLayout', 'notesSlide', 'image', 'media', 'audio', 'video', 'hdphoto', 'hyperlink', 'slide', 'tags'}


def sx_hidden(z, t):
    try:
        return ET.fromstring(z.read(t)).get('show') in ('0', 'false')
    except Exception:
        return False


def main():
    args = sys.argv[1:]
    jout = None
    if '--json' in args:
        i = args.index('--json')
        jout = args[i + 1]
        args = args[:i] + args[i + 2:]
    c = collections.Counter()
    hist = collections.defaultdict(collections.Counter)
    examples = collections.defaultdict(list)
    seen = set()
    for f in files(args):
        data = open(f, 'rb').read()
        h = hashlib.sha256(data).hexdigest()
        if h in seen:
            c['duplicate files skipped'] += 1
            continue
        seen.add(h)
        name = os.path.basename(f)
        try:
            z = zipfile.ZipFile(f)
            names = set(z.namelist())
            pkg = read_rels(z, names, '')
            main = [t for _, ty, t, e in pkg if ty == RT_OFFICEDOC and not e]
            if not main or main[0] not in names:
                c['no presentation part'] += 1
                continue
            pres_name = main[0]
            pres = ET.fromstring(z.read(pres_name))
            if pres.tag != q('p:presentation'):
                c['main part not p:presentation'] += 1
                continue
        except Exception:
            c['unreadable'] += 1
            continue
        c['files'] += 1
        prels = read_rels(z, names, pres_name)
        rid_target = {i: (ty, t) for i, ty, t, e in prels}
        # --- presentation rIds
        rids = [i for i, _, _, _ in prels]
        if all(re.fullmatch(r'rId\d+', i or '') for i in rids):
            c['pres rIds all rIdN'] += 1
            nums = sorted(int(i[3:]) for i in rids)
            if nums == list(range(1, len(nums) + 1)):
                c['pres rIds contiguous 1..n'] += 1
            else:
                c['pres rIds with gaps'] += 1
        else:
            c['pres rIds not all rIdN'] += 1
            examples['non rIdN ids'].append((name, [i for i in rids if not re.fullmatch(r'rId\d+', i or '')][:3]))
        # --- slides
        sld = pres.find('p:sldIdLst', NS)
        entries = [] if sld is None else [(int(e.get('id')), e.get(q('r:id'))) for e in sld.findall('p:sldId', NS)]
        c['slides'] += len(entries)
        hist['slides per file'][min(len(entries), 50) // 10 * 10] += 1
        if not entries:
            c['files without slides'] += 1
        ids = [i for i, _ in entries]
        if ids:
            if min(ids) < 256:
                c['sldId < 256'] += 1
            if ids != sorted(ids):
                c['files with sldIds not ascending in order'] += 1
            if ids != list(range(ids[0], ids[0] + len(ids))) and sorted(ids) != list(range(min(ids), min(ids) + len(ids))):
                c['files with sldId gaps'] += 1
            if min(ids) != 256:
                c['files whose lowest sldId != 256'] += 1
            hist['max sldId'][('256..299' if max(ids) < 300 else '300..999' if max(ids) < 1000 else '1000..2^31')] += 1
        # master / layout ids (shared id space >= 2^31)
        mids = [int(e.get('id')) for e in pres.findall('p:sldMasterIdLst/p:sldMasterId', NS) if e.get('id')]
        lids = []
        slide_parts = []
        for sid, rid in entries:
            ty, t = rid_target.get(rid, (None, None))
            if ty != RT_SLIDE or t not in names:
                c['dangling sldId'] += 1
                continue
            slide_parts.append(t)
        for t in slide_parts:
            m = re.fullmatch(r'ppt/slides/slide(\d+)\.xml', t)
            if not m:
                c['slide part with non-standard name'] += 1
                examples['non-standard slide names'].append((name, t))
        nums = [int(re.fullmatch(r'ppt/slides/slide(\d+)\.xml', t).group(1)) for t in slide_parts if re.fullmatch(r'ppt/slides/slide(\d+)\.xml', t)]
        if nums:
            if nums != list(range(1, len(nums) + 1)):
                c['files whose slide part numbers != 1..n in order'] += 1
                examples['slide numbering'].append((name, nums[:12]))
            if sorted(nums) != list(range(1, len(nums) + 1)):
                c['files with gaps in slide part numbers'] += 1
        # parts in ppt/slides not referenced by sldIdLst
        all_slide_parts = [n for n in names if re.fullmatch(r'ppt/slides/[^/]+\.xml', n)]
        orphans = set(all_slide_parts) - set(slide_parts)
        if orphans:
            c['files with slide parts not in sldIdLst'] += 1
            c['slide parts not in sldIdLst'] += len(orphans)
        # --- per slide
        layout_use = collections.Counter()
        notes_of = collections.defaultdict(list)
        media_use = collections.defaultdict(set)
        jump_links = 0
        rels_types = collections.Counter()
        for t in slide_parts:
            srels = read_rels(z, names, t)
            lay = [x for _, ty, x, e in srels if ty == RT_LAYOUT]
            kinds = {ty.rsplit('/', 1)[-1] for _, ty, _, e in srels}
            if kinds <= DELETE_CASCADE:
                c['slides deletable by the ADR-0012 cascade'] += 1
            else:
                c['slides with a relationship type outside the delete cascade'] += 1
                hist['blocking types (delete)'][','.join(sorted(kinds - DELETE_CASCADE))] += 1
            if kinds <= DUPLICATE_OK:
                c['slides duplicable (ADR-0012 types)'] += 1
            # incoming slide-jump links to this slide from other slides
            if sx_hidden(z, t):
                c['hidden slides (show=0)'] += 1
            if len(lay) != 1:
                c['slides without exactly one layout rel'] += 1
            for l in lay:
                layout_use[l] += 1
            for i, ty, x, e in srels:
                rels_types[ty.rsplit('/', 1)[-1]] += 1
                if ty == RT_NOTES:
                    notes_of[x].append(t)
                if (ty == RT_IMAGE or ty in RT_MEDIA) and not e:
                    media_use[x].add(t)
                if ty == RT_SLIDE:
                    jump_links += 1
            try:
                sx = ET.fromstring(z.read(t))
            except Exception:
                c['slide parse errors'] += 1
                continue
            # shape ids (p:cNvPr under spTree, excluding inside mc:AlternateContent and graphicData)
            tree = sx.find('p:cSld/p:spTree', NS)
            if tree is None:
                continue
            shape_ids = []

            def walk(n, in_ac=False):
                for ch in n:
                    if ch.tag == q('mc:AlternateContent'):
                        walk_ac(ch)
                        continue
                    nv = next((x for x in ch if x.tag.endswith('}nvSpPr') or x.tag.endswith('}nvGrpSpPr') or x.tag.endswith('}nvPicPr') or x.tag.endswith('}nvCxnSpPr') or x.tag.endswith('}nvGraphicFramePr') or x.tag.endswith('}nvContentPartPr')), None)
                    if nv is not None:
                        cnv = nv.find('p:cNvPr', NS)
                        if cnv is not None and (cnv.get('id') or '').isdigit():
                            shape_ids.append(int(cnv.get('id')))
                    if ch.tag == q('p:grpSp'):
                        walk(ch)

            def walk_ac(ac):
                c['AlternateContent in shape trees'] += 1

            nvt = tree.find('p:nvGrpSpPr/p:cNvPr', NS)
            if nvt is not None and nvt.get('id') != '1':
                c['spTree cNvPr id != 1'] += 1
            walk(tree)
            c['shapes (outside AC)'] += len(shape_ids)
            dup = [k for k, v in collections.Counter(shape_ids).items() if v > 1]
            if dup:
                c['slides with duplicate shape ids (outside AC)'] += 1
                examples['duplicate shape ids'].append((name, t, dup[:5]))
            if shape_ids:
                mx = max(shape_ids)
                hist['max shape id per slide'][('<=100' if mx <= 100 else '<=10^4' if mx <= 10**4 else '<=2^31' if mx < 2**31 else '>=2^31')] += 1
                if mx > len(shape_ids) * 4 + 10:
                    c['slides with sparse shape ids (max > 4*count+10)'] += 1
            # all @id anywhere (python-pptx's max_shape_id reads //@id)
            all_ids = [int(e.get('id')) for e in sx.iter() if (e.get('id') or '').isdigit()]
            if all_ids and shape_ids and max(all_ids) > max(shape_ids):
                c['slides where some non-shape @id exceeds max shape id'] += 1
            if sx.find('.//p14:creationId', NS) is not None:
                c['slides with p14:creationId'] += 1
        for l, n in layout_use.items():
            hist['slides per used layout'][min(n, 10)] += 1
        # layouts of all masters
        all_layouts = [n for n in names if re.fullmatch(r'ppt/slideLayouts/[^/]+\.xml', n)]
        c['layout parts'] += len(all_layouts)
        c['layout parts used by no slide'] += len(set(all_layouts) - set(layout_use))
        c['masters'] += len(mids)
        if len(mids) > 1:
            c['files with >1 master'] += 1
        # notes
        c['slides with notes'] += sum(len(v) for v in notes_of.values())
        for nt, srcs in notes_of.items():
            mn = re.fullmatch(r'ppt/notesSlides/notesSlide(\d+)\.xml', nt)
            ms = re.fullmatch(r'ppt/slides/slide(\d+)\.xml', srcs[0])
            if not (mn and ms and mn.group(1) == ms.group(1)):
                c['notes part number != its slide part number'] += 1
            if len(srcs) > 1:
                c['notes parts shared by >1 slide'] += 1
            nr = read_rels(z, names, nt)
            back = [x for _, ty, x, e in nr if ty == RT_SLIDE]
            if back != srcs[:1]:
                c['notes whose back-rel != its slide'] += 1
        if pres.find('p:notesMasterIdLst', NS) is not None:
            c['files with a notes master'] += 1
        # media sharing across slides
        for m, users in media_use.items():
            c['media parts used by slides'] += 1
            if not re.fullmatch(r'ppt/media/(image|media)\d+\.[A-Za-z0-9]+', m):
                c['media parts with non-standard names'] += 1
                examples['media names'].append((name, m))
            if len(users) > 1:
                c['media parts shared by >1 slide'] += 1
        # media also used by layouts/masters?
        lm_media = set()
        for lp in all_layouts + [n for n in names if re.fullmatch(r'ppt/slideMasters/[^/]+\.xml', n)]:
            for _, ty, x, e in read_rels(z, names, lp):
                if (ty == RT_IMAGE or ty in RT_MEDIA) and not e:
                    lm_media.add(x)
        c['slide media parts also used by a layout/master'] += len(set(media_use) & lm_media)
        c['slide-to-slide relationships (jump links)'] += jump_links
        for k, v in rels_types.items():
            hist['slide relationship types'][k] += v
        # sections / custom shows
        sects = pres.findall('.//p14:sectionLst/p14:section', NS)
        if sects:
            c['files with p14:sectionLst'] += 1
            ins = [int(e.get('id')) for s in sects for e in s.findall('p14:sldIdLst/p14:sldId', NS)]
            c['sections'] += len(sects)
            c['empty sections'] += sum(1 for s in sects if not s.findall('p14:sldIdLst/p14:sldId', NS))
            missing = set(ids) - set(ins)
            if missing:
                c['files with slides outside every section'] += 1
            if [i for i in ins if i in ids] != [i for i in ids if i in ins]:
                c['files whose section order != sldIdLst order'] += 1
            if set(ins) - set(ids):
                c['files with section entries for unknown sldIds'] += 1
            ext = pres.find('p:extLst', NS)
            if ext is not None:
                for e in ext.findall('p:ext', NS):
                    if e.find('p14:sectionLst', NS) is not None:
                        hist['sectionLst ext uri'][e.get('uri')] += 1
        if pres.find('p:custShowLst', NS) is not None:
            c['files with p:custShowLst'] += 1
        # slide-jump links: which slides are targets of another slide's relationship
        targets = collections.Counter()
        for t in slide_parts:
            for _, ty, x, e in read_rels(z, names, t):
                if ty == RT_SLIDE and not e and x != t:
                    targets[x] += 1
        c['slides that are jump-link targets'] += len(targets)
        # presProps: slide show range by position
        for _, ty, t, e in prels:
            if ty.endswith('/presProps') and t in names:
                pp = ET.fromstring(z.read(t))
                if pp.find('.//p:showPr/p:sldRg', NS) is not None:
                    c['presProps with showPr/sldRg (positional)'] += 1
                if pp.find('.//p:showPr/p:custShow', NS) is not None:
                    c['presProps with showPr/custShow'] += 1
        # content types
        ct = ET.fromstring(z.read('[Content_Types].xml'))
        defaults = {d.get('Extension', '').lower(): d.get('ContentType') for d in ct.findall('ct:Default', NS)}
        overrides = {o.get('PartName', '').lstrip('/'): o.get('ContentType') for o in ct.findall('ct:Override', NS)}
        for t in slide_parts:
            if t in overrides:
                c['slides typed by Override'] += 1
            elif t.rsplit('.', 1)[-1].lower() in defaults:
                c['slides typed by Default only'] += 1
        for m in media_use:
            ext = m.rsplit('.', 1)[-1].lower()
            if m in overrides:
                c['slide media typed by Override'] += 1
            elif ext in defaults:
                c['slide media typed by Default'] += 1
            else:
                c['slide media without content type'] += 1
            hist['media extensions'][ext] += 1
        # app.xml
        ext = [t for _, ty, t, e in pkg if ty == RT_EXT and not e]
        if ext and ext[0] in names:
            try:
                ap = ET.fromstring(z.read(ext[0]))
                s = ap.find('ep:Slides', NS)
                if s is None:
                    c['app.xml without Slides'] += 1
                elif int(s.text or -1) == len(entries):
                    c['app.xml Slides == count'] += 1
                else:
                    c['app.xml Slides != count'] += 1
                    examples['app.xml Slides mismatch'].append((name, s.text, len(entries)))
                n = ap.find('ep:Notes', NS)
                if n is not None and int(n.text or -1) != sum(len(v) for v in notes_of.values()):
                    c['app.xml Notes != notes count'] += 1
                if ap.find('ep:TitlesOfParts', NS) is not None:
                    c['app.xml with TitlesOfParts'] += 1
                    hp = ap.find('ep:HeadingPairs/vt:vector', NS)
                    if hp is not None:
                        vs = list(hp)
                        for k in range(0, len(vs) - 1, 2):
                            label = ''.join(vs[k].itertext())
                            cnt = ''.join(vs[k + 1].itertext())
                            if label.strip().lower() in ('slide titles', 'folientitel', 'titres des diapositives'):
                                c['app.xml with a slide-titles heading'] += 1
                                if cnt.strip().isdigit() and int(cnt) == len(entries):
                                    c['slide-titles heading count == slides'] += 1
                            hist['app.xml heading labels'][label.strip()] += 1
                if ap.find('ep:HiddenSlides', NS) is not None:
                    c['app.xml with HiddenSlides'] += 1
            except Exception:
                c['app.xml unreadable'] += 1
        else:
            c['no app.xml'] += 1
        # comments / tags at presentation level
        for _, ty, t, e in prels:
            hist['presentation relationship types'][ty.rsplit('/', 1)[-1]] += 1
    out = {'counts': dict(c), 'histograms': {k: dict(v) for k, v in hist.items()}, 'examples': {k: v[:8] for k, v in examples.items()}}
    for k in sorted(c):
        print(f'{k}: {c[k]}')
    for k, v in sorted(hist.items()):
        print(f'{k}: ' + ', '.join(f'{a}={b}' for a, b in sorted(v.items(), key=lambda x: str(x[0]))))
    for k, v in sorted(examples.items()):
        print(f'example {k}: {v[:6]}')
    if jout:
        json.dump(out, open(jout, 'w'), indent=1, default=str)


if __name__ == '__main__':
    main()
