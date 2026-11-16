"""ST_Percentage-family attributes: XSD union type vs. Open XML SDK type, and
what real files actually contain (run with `python3 -I`).

Usage: python3 -I percent.py <ECMA Transitional XSD dir> <Open-XML-SDK data dir> [pptx files/dirs...]

1. Lists every attribute in dml-main.xsd / pml.xsd / dml-chart.xsd whose XSD
   type is a DrawingML percentage union (int in 1/1000 % OR "NN%" string), with
   the SDK's .NET type for the same (complex type, attribute).
2. Scans the given .pptx files and counts values of those attributes in the
   "NN%" lexical form (which an SDK Int32Value would reject).
"""
import sys, os, json, re, zipfile, collections
import xml.etree.ElementTree as ET

XS = '{http://www.w3.org/2001/XMLSchema}'
UNIONS = {'ST_Percentage', 'ST_PositivePercentage', 'ST_FixedPercentage',
          'ST_PositiveFixedPercentage', 'ST_TextSpacingPercentOrPercentString',
          'ST_TextFontScalePercentOrPercentString', 'ST_TextBulletSizePercent',
          'ST_HPercentWithSymbol', 'ST_GapAmount', 'ST_Overlap', 'ST_BubbleScale',
          'ST_HoleSize', 'ST_SecondPieSize', 'ST_DepthPercentWithSymbol',
          'ST_LblOffsetPercent'}

def xsd_attrs(xsd_dir):
    out = []  # (prefix, complexType, attr, type)
    for fn, pfx in (('dml-main.xsd', 'a'), ('pml.xsd', 'p'), ('dml-chart.xsd', 'c')):
        root = ET.parse(os.path.join(xsd_dir, fn)).getroot()
        # also resolve local simpleTypes that are unions including s:ST_Percentage
        local_union = set()
        for st in root.iter(XS + 'simpleType'):
            n = st.get('name')
            u = st.find(XS + 'union')
            if n and u is not None and 'Percentage' in (u.get('memberTypes') or ''):
                local_union.add(n)
            r = st.find(XS + 'restriction')
            if n and r is not None and (r.get('base') or '').split(':')[-1] in ('ST_Percentage',):
                local_union.add(n)
        for ct in root.iter(XS + 'complexType'):
            for a in ct.iter(XS + 'attribute'):
                t = (a.get('type') or '').split(':')[-1]
                if t in UNIONS or t in local_union:
                    out.append((pfx, ct.get('name'), a.get('name'), a.get('type'), a.get('default')))
    return out

def sdk_types(sdk_dir):
    m = {}
    for f in os.listdir(os.path.join(sdk_dir, 'schemas')):
        for t in json.load(open(os.path.join(sdk_dir, 'schemas', f))).get('Types', []):
            typ = t['Name'].split('/')[0]
            for a in t.get('Attributes', []):
                m.setdefault((typ, a['QName'].lstrip(':')), set()).add(a['Type'])
    return m

def main(argv):
    xsd_dir, sdk_dir, rest = argv[0], argv[1], argv[2:]
    rows = xsd_attrs(xsd_dir)
    st = sdk_types(sdk_dir)
    attrnames = collections.defaultdict(set)
    print('XSD percentage-union attributes (prefix, CT, attr, xsd type, default) -> SDK type')
    for pfx, ct, an, ty, d in rows:
        sdk = st.get(('%s:%s' % (pfx, ct), an), {'(not in SDK under this CT)'})
        print('  %s:%s/@%s %s default=%s -> %s' % (pfx, ct, an, ty, d, ','.join(sorted(sdk))))
        attrnames[an].add(pfx)
    print('total', len(rows))
    files = []
    for a in rest:
        if os.path.isdir(a):
            files += [os.path.join(a, n) for n in sorted(os.listdir(a)) if n.endswith('.pptx')]
        else:
            files.append(a)
    pct = collections.Counter(); ints = collections.Counter(); fhit = collections.Counter()
    for p in files:
        try:
            z = zipfile.ZipFile(p)
        except Exception:
            continue
        hit = False
        for n in z.namelist():
            if not n.endswith('.xml') or not n.startswith('ppt/'):
                continue
            try:
                root = ET.fromstring(z.read(n))
            except Exception:
                continue
            for el in root.iter():
                if not el.tag.startswith('{http://schemas.openxmlformats.org/drawingml/2006/main}') and \
                   not el.tag.startswith('{http://schemas.openxmlformats.org/presentationml/2006/main}') and \
                   not el.tag.startswith('{http://schemas.openxmlformats.org/drawingml/2006/chart}'):
                    continue
                for k, v in el.attrib.items():
                    if '}' in k:
                        continue
                    if k in attrnames or (el.tag.endswith('}spcPct') or el.tag.endswith('}buSzPct')):
                        key = el.tag.split('}')[1] + '/@' + k
                        if v.endswith('%'):
                            pct[key] += 1; hit = True
                        elif re.fullmatch(r'-?\d+', v):
                            ints[key] += 1
        if hit:
            fhit[os.path.basename(p)] += 1
    if files:
        print('\nfiles scanned', len(files))
        print('values in "NN%%" form: %d (files: %s)' % (sum(pct.values()), sorted(fhit)))
        for k, v in pct.most_common(20):
            print('   %-28s %d' % (k, v))
        print('values in integer form (top 15):')
        for k, v in ints.most_common(15):
            print('   %-28s %d' % (k, v))

if __name__ == '__main__':
    main(sys.argv[1:])
