"""Check ADR-0005's proposed Phase0 PML/DML allowlist against the SDK data
(run with `python3 -I`).

Usage: python3 -I allowlist_check.py <Open-XML-SDK data dir> <ECMA XSD dir> <path to spikes/adr-0002/py/slottable.py>

For every proposed SDK Name: it exists, which schema file (feature group) it
is in, its attributes after merging the same-file abstract base entry, its
ADR-0002 §4.1 slot lowering, and which children are themselves allowlisted
(=> generated `impl ChildOf`), flagging cross-feature pairs. Then it lists
allowlisted attributes whose XSD type is a DrawingML percentage union or
whose XSD default does not fit the SDK type (=> set_type / DefaultMismatch).
"""
import sys, os, json, re
import xml.etree.ElementTree as ET

sdk, xsd, slottable = sys.argv[1], sys.argv[2], sys.argv[3]
src = open(slottable).read().split('# ---- ECMA side')[0]
src = src.replace('sdk, xsd = sys.argv[1], sys.argv[2]', '')
ns = {}
exec(src, ns)
lower, NSx = ns['lower'], ns['NS']

ALLOW = {
    # name: (mode, feature)
    'p:CT_Presentation/p:presentation': 'read',
    'p:CT_SlideIdList/p:sldIdLst': 'read',
    'p:CT_SlideIdListEntry/p:sldId': 'read',
    'p:CT_Slide/p:sld': 'read',
    'p:CT_CommonSlideData/p:cSld': 'read',
    'p:CT_GroupShape/p:spTree': 'read',
    'p:CT_GroupShape/p:grpSp': 'read',
    'p:CT_GroupShapeNonVisual/p:nvGrpSpPr': 'read',
    'p:CT_Shape/p:sp': 'read',
    'p:CT_ShapeNonVisual/p:nvSpPr': 'read',
    'a:CT_NonVisualDrawingProps/p:cNvPr': 'read',
    'p:CT_ApplicationNonVisualDrawingProps/p:nvPr': 'read',
    'p:CT_Placeholder/p:ph': 'read',
    'p:CT_GraphicalObjectFrame/p:graphicFrame': 'read',
    'p:CT_GraphicalObjectFrameNonVisual/p:nvGraphicFramePr': 'read',
    'a:CT_GraphicalObject/a:graphic': 'read',
    'a:CT_GraphicalObjectData/a:graphicData': 'read',
    'a:CT_Table/a:tbl': 'read',
    'a:CT_TableRow/a:tr': 'read',
    'a:CT_TableCell/a:tc': 'read',
    'a:CT_TextBody/p:txBody': 'write',
    'a:CT_TextBody/a:txBody': 'write',
    'a:CT_TextParagraph/a:p': 'write',
    'a:CT_RegularTextRun/a:r': 'write',
    'xsd:string/a:t': 'write',
    'a:CT_TextLineBreak/a:br': 'write',
    'a:CT_TextField/a:fld': 'read',
    'p:CT_SlideTiming/p:timing': 'read',
}
PCT = {'ST_Percentage', 'ST_PositivePercentage', 'ST_FixedPercentage', 'ST_PositiveFixedPercentage',
       'ST_TextSpacingPercentOrPercentString', 'ST_TextFontScalePercentOrPercentString',
       'ST_TextBulletSizePercent'}

types = {}
for f in sorted(os.listdir(os.path.join(sdk, 'schemas'))):
    for t in json.load(open(os.path.join(sdk, 'schemas', f))).get('Types', []):
        types.setdefault(t['Name'], (f, t))
byclass = {}
for n, (f, t) in types.items():
    if n.endswith('/'):
        byclass[(f, t.get('ClassName'))] = t

# XSD attribute types/defaults by (prefix-of-type, CT, attr)
XS = '{http://www.w3.org/2001/XMLSchema}'
xattr = {}
for fn, p in (('dml-main.xsd', 'a'), ('pml.xsd', 'p')):
    root = ET.parse(os.path.join(xsd, fn)).getroot()
    for ct in root.findall(XS + 'complexType'):
        for a in ct.iter(XS + 'attribute'):
            if a.get('name'):
                xattr[(p, ct.get('name'), a.get('name'))] = (a.get('type'), a.get('default'))

def feature(f):
    return {'schemas_openxmlformats_org_presentationml_2006_main.json': 'pml',
            'schemas_openxmlformats_org_drawingml_2006_main.json': 'dml'}.get(f, f)

for n, mode in ALLOW.items():
    if n not in types:
        print('MISSING', n); continue
    f, t = types[n]
    base = byclass.get((f, t.get('BaseClass')))
    attrs = list(t.get('Attributes', [])) + (list(base.get('Attributes', [])) if base else [])
    try:
        slots = lower(t['Particle']) if 'Particle' in t else []
        sl = ' | '.join('[' + ','.join(sorted(s)) + ']' for s in slots)
    except NSx as e:
        sl = 'UNSUPPORTED (%s)' % e
    kids = sorted({c['Name'] for c in t.get('Children', [])} & set(ALLOW))
    cross = [k for k in kids if feature(types[k][0]) != feature(f)]
    print('%-46s %-5s %-4s attrs=%d' % (n, mode, feature(f), len(attrs)))
    print('     slots:', sl[:300])
    if kids:
        print('     ChildOf from:', kids, ('CROSS-FEATURE: %s' % cross) if cross else '')
    tp, _, ctn = n.split('/')[0].partition(':')
    for a in attrs:
        an = a['QName'].lstrip(':')
        xt = xattr.get((tp, ctn, an))
        if not xt:
            continue
        xtype, xdef = xt
        flag = []
        if (xtype or '').split(':')[-1] in PCT:
            flag.append('PERCENT-UNION')
        if xdef is not None and a['Type'] in ('Int32Value', 'UInt32Value', 'Int64Value') and not re.fullmatch(r'-?\d+', xdef):
            flag.append('DEFAULT-MISMATCH(%s)' % xdef)
        if flag:
            print('     attr %s: xsd %s default=%s sdk %s -> %s' % (an, xtype, xdef, a['Type'], ','.join(flag)))
