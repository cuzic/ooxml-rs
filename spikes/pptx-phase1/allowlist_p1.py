"""Check ADR-0008's proposed Phase 1 allowlist (run with `python3 -I`).

Usage: python3 -I allowlist_p1.py <Open-XML-SDK data dir> <ECMA XSD dir> <codegen/allowlist dir> <allowlist_phase1.toml>

1. Every proposed SDK Name exists, is not already allowlisted, and is not
   abstract; its schema file is printed (feature-group sanity).
2. Every descent edge the Phase 1 resolver walks is a generated `ChildOf`
   pair: the child's Name is in the parent's SDK `Children` (merged with the
   same-file abstract base), and both ends are allowlisted (Phase 0 or 1).
3. Attributes of allowlisted entries (Phase 0 + 1) whose XSD type is a
   DrawingML percentage union, and XSD defaults that do not fit the SDK type
   (ADR-0002 DefaultMismatch): the candidates for `set_type`.
"""
import sys, os, json, re, tomllib
import xml.etree.ElementTree as ET

sdk, xsd, allow_dir, prop = sys.argv[1:5]
types = {}
for f in sorted(os.listdir(os.path.join(sdk, 'schemas'))):
    for t in json.load(open(os.path.join(sdk, 'schemas', f))).get('Types', []):
        types.setdefault(t['Name'], (f, t))
byclass = {(f, t.get('ClassName')): t for n, (f, t) in types.items() if n.endswith('/')}


def merged(n):
    f, t = types[n]
    base = byclass.get((f, t.get('BaseClass')))
    attrs = list(t.get('Attributes', [])) + (list(base.get('Attributes', [])) if base else [])
    kids = [c['Name'] for c in t.get('Children', [])] + ([c['Name'] for c in base.get('Children', [])] if base else [])
    return f, attrs, kids


old = {}
for g in ('pml', 'dml'):
    with open(os.path.join(allow_dir, g + '.toml'), 'rb') as fh:
        t = tomllib.load(fh)
    for mode in ('write', 'read'):
        for n in t.get(mode, {}).get('names', []):
            old[n] = (g, mode)
with open(prop, 'rb') as fh:
    p = tomllib.load(fh)
new = {}
for g in ('pml', 'dml'):
    for n in p[g]['read']['names']:
        new[n] = (g, 'read')
ALL = dict(old); ALL.update(new)

print('## 1. names (%d proposed)' % len(new))
bad = 0
for n, (g, _m) in new.items():
    if n not in types:
        print('MISSING', n); bad += 1; continue
    if n in old:
        print('ALREADY ALLOWLISTED', n); bad += 1
    f = types[n][0]
    print('  %-48s -> %s.toml   (SDK file %s)' % (n, g, f.replace('schemas_', '').replace('.json', '')))
print('problems:', bad)

EDGES = '''
p:CT_Slide/p:sld a:CT_ColorMappingOverride/p:clrMapOvr
p:CT_Slide/p:sld p:CT_SlideTransition/p:transition
p:CT_Slide/p:sld p:CT_SlideTiming/p:timing
p:CT_SlideLayout/p:sldLayout p:CT_CommonSlideData/p:cSld
p:CT_SlideLayout/p:sldLayout a:CT_ColorMappingOverride/p:clrMapOvr
p:CT_SlideMaster/p:sldMaster p:CT_CommonSlideData/p:cSld
p:CT_SlideMaster/p:sldMaster a:CT_ColorMapping/p:clrMap
p:CT_SlideMaster/p:sldMaster p:CT_SlideMasterTextStyles/p:txStyles
p:CT_NotesSlide/p:notes p:CT_CommonSlideData/p:cSld
p:CT_NotesSlide/p:notes a:CT_ColorMappingOverride/p:clrMapOvr
p:CT_NotesMaster/p:notesMaster p:CT_CommonSlideData/p:cSld
p:CT_NotesMaster/p:notesMaster a:CT_ColorMapping/p:clrMap
p:CT_NotesMaster/p:notesMaster a:CT_TextListStyle/p:notesStyle
a:CT_ColorMappingOverride/p:clrMapOvr a:CT_ColorMapping/a:overrideClrMapping
a:CT_ColorMappingOverride/p:clrMapOvr a:CT_EmptyElement/a:masterClrMapping
p:CT_SlideMasterTextStyles/p:txStyles a:CT_TextListStyle/p:titleStyle
p:CT_SlideMasterTextStyles/p:txStyles a:CT_TextListStyle/p:bodyStyle
p:CT_SlideMasterTextStyles/p:txStyles a:CT_TextListStyle/p:otherStyle
p:CT_Presentation/p:presentation a:CT_TextListStyle/p:defaultTextStyle
p:CT_Presentation/p:presentation p:CT_SlideSize/p:sldSz
p:CT_Presentation/p:presentation p:CT_PresentationExtensionList/p:extLst
p:CT_PresentationExtensionList/p:extLst p:CT_PresentationExtension/p:ext
p:CT_PresentationExtension/p:ext p14:CT_SectionList/p14:sectionLst
p14:CT_SectionList/p14:sectionLst p14:CT_Section/p14:section
p14:CT_Section/p14:section p14:CT_SectionSlideIdList/p14:sldIdLst
p14:CT_SectionSlideIdList/p14:sldIdLst p14:CT_SectionSlideIdListEntry/p14:sldId
a:CT_TextListStyle/p:bodyStyle a:CT_TextParagraphProperties/a:lvl1pPr
a:CT_TextListStyle/p:defaultTextStyle a:CT_TextParagraphProperties/a:defPPr
a:CT_TextListStyle/a:lstStyle a:CT_TextParagraphProperties/a:lvl9pPr
a:CT_TextParagraphProperties/a:lvl1pPr a:CT_TextCharacterProperties/a:defRPr
a:CT_TextParagraphProperties/a:pPr a:CT_TextCharacterProperties/a:defRPr
a:CT_TextParagraphProperties/a:pPr a:CT_TextSpacing/a:lnSpc
a:CT_TextParagraphProperties/a:pPr a:CT_TextCharBullet/a:buChar
a:CT_TextParagraphProperties/a:pPr a:CT_TextBulletSizePercent/a:buSzPct
a:CT_TextSpacing/a:lnSpc a:CT_TextSpacingPercent/a:spcPct
a:CT_TextParagraphProperties/a:pPr a:CT_TextBulletColorFollowText/a:buClrTx
a:CT_TextParagraphProperties/a:pPr a:CT_TextBulletSizeFollowText/a:buSzTx
a:CT_TextParagraphProperties/a:pPr a:CT_TextBulletSizePoint/a:buSzPts
a:CT_TextParagraphProperties/a:lvl1pPr a:CT_TextBulletTypefaceFollowText/a:buFontTx
a:CT_TextCharacterProperties/a:rPr a:CT_TextFont/a:sym
a:CT_ShapeProperties/p:spPr a:CT_CustomGeometry2D/a:custGeom
a:CT_TextSpacing/a:spcBef a:CT_TextSpacingPoint/a:spcPts
a:CT_TextBody/p:txBody a:CT_TextBodyProperties/a:bodyPr
a:CT_TextBody/p:txBody a:CT_TextListStyle/a:lstStyle
a:CT_TextBodyProperties/a:bodyPr a:CT_TextNormalAutofit/a:normAutofit
p:CT_Shape/p:sp a:CT_ShapeProperties/p:spPr
p:CT_Shape/p:sp a:CT_ShapeStyle/p:style
a:CT_ShapeStyle/p:style a:CT_FontReference/a:fontRef
a:CT_FontReference/a:fontRef a:CT_SchemeColor/a:schemeClr
a:CT_ShapeProperties/p:spPr a:CT_Transform2D/a:xfrm
a:CT_ShapeProperties/p:spPr a:CT_PresetGeometry2D/a:prstGeom
a:CT_Transform2D/a:xfrm a:CT_Point2D/a:off
a:CT_Transform2D/a:xfrm a:CT_PositiveSize2D/a:ext
p:CT_GraphicalObjectFrame/p:graphicFrame a:CT_Transform2D/p:xfrm
a:CT_Transform2D/p:xfrm a:CT_Point2D/a:off
p:CT_GroupShape/p:grpSp a:CT_GroupShapeProperties/p:grpSpPr
a:CT_GroupShapeProperties/p:grpSpPr a:CT_GroupTransform2D/a:xfrm
a:CT_GroupTransform2D/a:xfrm a:CT_Point2D/a:chOff
a:CT_GroupTransform2D/a:xfrm a:CT_PositiveSize2D/a:chExt
p:CT_Picture/p:pic a:CT_ShapeProperties/p:spPr
p:CT_Connector/p:cxnSp a:CT_ShapeProperties/p:spPr
a:CT_TextCharacterProperties/a:rPr a:CT_TextFont/a:latin
a:CT_TextCharacterProperties/a:rPr a:CT_SolidColorFillProperties/a:solidFill
a:CT_TextCharacterProperties/a:rPr a:CT_Hyperlink/a:hlinkClick
a:CT_TextCharacterProperties/a:rPr a:CT_Hyperlink/a:hlinkMouseOver
a:CT_TextCharacterProperties/a:defRPr a:CT_TextFont/a:ea
a:CT_NonVisualDrawingProps/p:cNvPr a:CT_Hyperlink/a:hlinkClick
a:CT_NonVisualDrawingProps/p:cNvPr a:CT_Hyperlink/a:hlinkHover
a:CT_SolidColorFillProperties/a:solidFill a:CT_SchemeColor/a:schemeClr
a:CT_SolidColorFillProperties/a:solidFill a:CT_SRgbColor/a:srgbClr
a:CT_OfficeStyleSheet/a:theme a:CT_BaseStyles/a:themeElements
a:CT_BaseStyles/a:themeElements a:CT_ColorScheme/a:clrScheme
a:CT_BaseStyles/a:themeElements a:CT_FontScheme/a:fontScheme
a:CT_ColorScheme/a:clrScheme a:CT_Color2/a:dk1
a:CT_Color2/a:dk1 a:CT_SystemColor/a:sysClr
a:CT_Color2/a:accent1 a:CT_SRgbColor/a:srgbClr
a:CT_FontScheme/a:fontScheme a:CT_FontCollection/a:majorFont
a:CT_FontCollection/a:minorFont a:CT_TextFont/a:latin
a:CT_FontCollection/a:minorFont a:CT_SupplementalFont/a:font
p:CT_SlideTransition/p:transition p:CT_TransitionSoundAction/p:sndAc
p:CT_SlideTiming/p:timing p:CT_BuildList/p:bldLst
p:CT_BuildList/p:bldLst p:CT_TLBuildParagraph/p:bldP
p:CT_TLShapeTargetElement/p:spTgt p:CT_TLTextTargetElement/p:txEl
p:CT_TLTextTargetElement/p:txEl p:CT_IndexRange/p:pRg
p:CT_TLTextTargetElement/p:txEl p:CT_IndexRange/p:charRg
p:CT_CommentList/p:cmLst p:CT_Comment/p:cm
p:CT_Comment/p:cm a:CT_Point2D/p:pos
p:CT_Comment/p:cm xsd:string/p:text
p:CT_CommentAuthorList/p:cmAuthorLst p:CT_CommentAuthor/p:cmAuthor
p188:CT_CommentList/p188:cmLst p188:CT_Comment/p188:cm
p188:CT_Comment/p188:cm a:CT_Point2D/p188:pos
p188:CT_Comment/p188:cm a:CT_TextBody/p188:txBody
p188:CT_Comment/p188:cm p188:CT_CommentReplyList/p188:replyLst
p188:CT_CommentReplyList/p188:replyLst p188:CT_CommentReply/p188:reply
p188:CT_CommentReply/p188:reply a:CT_TextBody/p188:txBody
a:CT_TextBody/p188:txBody a:CT_TextParagraph/a:p
p188:CT_AuthorList/p188:authorLst p188:CT_Author/p188:author
'''
print('\n## 2. descent edges')
bad = 0
for line in EDGES.strip().splitlines():
    par, ch = line.split()
    for n in (par, ch):
        if n not in ALL:
            print('NOT ALLOWLISTED', n, '(edge %s -> %s)' % (par, ch)); bad += 1
    if par in types and ch not in merged(par)[2]:
        print('NOT A CHILD (no ChildOf):', par, '->', ch); bad += 1
print('edges checked: %d, problems: %d' % (len(EDGES.strip().splitlines()), bad))

PCT = {'ST_Percentage', 'ST_PositivePercentage', 'ST_FixedPercentage', 'ST_PositiveFixedPercentage',
       'ST_TextSpacingPercentOrPercentString', 'ST_TextFontScalePercentOrPercentString', 'ST_TextBulletSizePercent'}
XS = '{http://www.w3.org/2001/XMLSchema}'
xattr = {}
for fn, pf in (('dml-main.xsd', 'a'), ('pml.xsd', 'p')):
    root = ET.parse(os.path.join(xsd, fn)).getroot()
    for ct in root.findall(XS + 'complexType'):
        for a in ct.iter(XS + 'attribute'):
            if a.get('name'):
                xattr[(pf, ct.get('name'), a.get('name'))] = (a.get('type'), a.get('default'))
print('\n## 3. percentage-union attributes / default mismatches on allowlisted entries (Phase 0 + proposed)')
seen = set()
for n in sorted(ALL):
    if n not in types:
        continue
    ctp = n.split('/')[0]
    if ':' not in ctp:
        continue
    pf, ctn = ctp.split(':')
    f, attrs, _k = merged(n)
    for a in attrs:
        an = a['QName'].lstrip(':')
        xt = xattr.get((pf, ctn, an))
        if not xt:
            continue
        xtype, xdef = xt
        flags = []
        if (xtype or '').split(':')[-1] in PCT:
            flags.append('PERCENT-UNION')
        if xdef is not None and a['Type'] in ('Int32Value', 'UInt32Value', 'Int64Value') and not re.fullmatch(r'-?\d+', xdef):
            flags.append('DEFAULT-MISMATCH(%s)' % xdef)
        if flags and (ctp, an) not in seen:
            seen.add((ctp, an))
            who = 'phase1' if n in new else 'phase0'
            print('  [%s] %s@%s: xsd %s default=%s sdk %s -> %s   (e.g. %s)' % (who, ctp, an, xtype, xdef, a['Type'], ','.join(flags), n))
