"""Check slot lowering and MC placement against real .docx/.pptx files.
Usage: python3 -I corpus_check.py <slottable.py> <sdk data dir> <dir of files>"""
import sys, os, json, zipfile, collections, re
import xml.etree.ElementTree as ET
slot_src, sdk, fdir = sys.argv[1], sys.argv[2], sys.argv[3]
g = {}; exec(open(slot_src).read().split("# ---- ECMA side")[0].replace("sdk, xsd = sys.argv[1], sys.argv[2]", "sdk = None"), g)
lower, NS = g['lower'], g['NS']
pfx = {e['Prefix']: e['Uri'] for e in json.load(open(os.path.join(sdk, 'namespaces.json')))}
T = {}
for f in os.listdir(os.path.join(sdk, 'schemas')):
    for t in json.load(open(os.path.join(sdk, 'schemas', f))).get('Types', []): T[t['Name']] = t
def clark(q):  # 'w:p' -> '{uri}p'
    p, l = q.split(':'); return '{%s}%s' % (pfx[p], l)
TARGETS = {  # parent element (clark) -> SDK entry
 'w:body': 'w:CT_Body/w:body', 'w:p': 'w:CT_P/w:p', 'w:r': 'w:CT_R/w:r', 'w:tbl': 'w:CT_Tbl/w:tbl',
 'w:tr': 'w:CT_Row/w:tr', 'w:tc': 'w:CT_Tc/w:tc', 'w:pPr': 'w:CT_PPr/w:pPr', 'w:rPr': 'w:CT_RPr/w:rPr',
 'w:sectPr': 'w:CT_SectPr/w:sectPr',
 'p:sld': 'p:CT_Slide/p:sld', 'p:cSld': 'p:CT_CommonSlideData/p:cSld', 'p:spTree': 'p:CT_GroupShape/p:spTree',
 'p:sp': 'p:CT_Shape/p:sp', 'p:pic': 'p:CT_Picture/p:pic', 'p:txBody': 'a:CT_TextBody/p:txBody',
 'a:p': 'a:CT_TextParagraph/a:p', 'a:r': 'a:CT_RegularTextRun/a:r', 'p:presentation': 'p:CT_Presentation/p:presentation'}
rank = {}   # parent -> {child: sorted list of slot indices}; greedy left-to-right for names in 2 slots
for k, n in TARGETS.items():
    sl = lower(T[n]['Particle'])
    m = collections.defaultdict(list)
    for i, s in enumerate(sl):
        for q in s:
            if q != '*ANY*' and ':' in q: m[clark(q)].append(i)
    rank[clark(k)] = m
MC = '{http://schemas.openxmlformats.org/markup-compatibility/2006}AlternateContent'
ac_parent = collections.Counter(); ac_files = collections.defaultdict(set)
viol = collections.Counter(); viol_ex = {}; unlisted = collections.Counter(); seen = collections.Counter()
runs_per_p = []; files = 0; tspace = collections.Counter()
def parts(z):
    for n in z.namelist():
        if re.match(r'word/(document|header\d*|footer\d*)\.xml$', n) or re.match(r'ppt/(slides/slide\d+|presentation)\.xml$', n): yield n
for fn in sorted(os.listdir(fdir)):
    try: z = zipfile.ZipFile(os.path.join(fdir, fn))
    except Exception: continue
    files += 1
    for pn in parts(z):
        try: root = ET.fromstring(z.read(pn))
        except Exception: continue
        for el in root.iter():
            ch = [c for c in el if isinstance(c.tag, str)]
            if any(c.tag == MC for c in ch):
                ac_parent[el.tag] += 1; ac_files[el.tag].add(fn)
            if el.tag in rank:
                seen[el.tag] += 1
                r = rank[el.tag]; last = -1
                for c in ch:
                    if c.tag == MC: continue
                    if c.tag not in r: unlisted[(el.tag, c.tag)] += 1; continue
                    ok = [i for i in r[c.tag] if i >= last]   # greedy: smallest slot not before current
                    if not ok:
                        viol[el.tag] += 1; viol_ex.setdefault(el.tag, (fn, pn, c.tag)); break
                    last = ok[0]
            if el.tag == clark('w:p'):
                runs_per_p.append(sum(1 for c in ch if c.tag == clark('w:r') and c.find(clark('w:t')) is not None))
            if el.tag == clark('w:t'):
                tspace['preserve' if el.get('{http://www.w3.org/XML/1998/namespace}space') == 'preserve' else 'none'] += 1
def short(c):
    for p, u in pfx.items():
        if c.startswith('{%s}' % u) and p: return p + ':' + c.split('}')[1]
    return c
print('files', files)
print('order violations vs slots (parent: count / instances checked):')
for k in rank: print(f'  {short(k):16} {viol[k]:5} / {seen[k]:6}', viol_ex.get(k, ''))
print('mc:AlternateContent parents (instances, files):')
for k, v in ac_parent.most_common(15): print(f'  {short(k):24} {v:5} {len(ac_files[k]):4}')
print('unlisted children of targets (top):')
for (p, c), v in unlisted.most_common(12): print(f'  {short(p)} > {short(c)}: {v}')
nz = [x for x in runs_per_p if x]
print('w:p with text: count', len(nz), 'runs-with-text per p: mean %.2f' % (sum(nz) / max(1, len(nz))), 'share >1 run: %.0f%%' % (100 * sum(1 for x in nz if x > 1) / max(1, len(nz))))
print('w:t xml:space', dict(tspace))
