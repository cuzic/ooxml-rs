"""Build the hand-made mixed-deck input of ADR-0019 P4a-0 (F6).

python3 -I make_mixed_input.py reference/fixtures/pptx/poi/45545_Comment.pptx OUT.pptx

Adds to slide 3 (sldId 542 = 0x21E) of the POI deck: a p14:creationId
(1234567890 = 0x499602D2), one modern comment part in the F1 (UI) form, the
p188:commentRel, ppt/authors.xml and the content types. The author id is the
one PowerPoint derives for the dummy identity (probe-p1.pptx). Every other
entry is copied unchanged.
"""
import zipfile, sys
src, dst = sys.argv[1], sys.argv[2]
P188 = 'http://schemas.microsoft.com/office/powerpoint/2018/8/main'
NS = 'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p188="%s"' % P188
CID = 1234567890  # 0x499602D2
part = 'ppt/comments/modernComment_21E_499602D2.xml'
AUTH = '{9D15D246-2D30-1376-E7B2-A74C58067C45}'
cm = ('<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\r\n<p188:cmLst %s><p188:cm id="{3F2504E0-4F89-41D3-9A0C-0305E82C3301}" authorId="%s" created="2026-11-21T21:30:00.000">'
      '<pc:sldMkLst xmlns:pc="http://schemas.microsoft.com/office/powerpoint/2013/main/command"><pc:docMk/><pc:sldMk cId="%d" sldId="542"/></pc:sldMkLst>'
      '<p188:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang="en-US"/><a:t>Hand-made modern comment in a deck with legacy comments</a:t></a:r></a:p></p188:txBody></p188:cm></p188:cmLst>') % (NS, AUTH, CID)
au = ('<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\r\n<p188:authorLst %s><p188:author id="%s" name="Probe User" initials="PU" userId="S::probe.user@example.com::0f3c2a1b-9d8e-4f7a-b6c5-d4e3f2a1b0c9" providerId="AD"/></p188:authorLst>') % (NS, AUTH)
zi = zipfile.ZipFile(src); zo = zipfile.ZipFile(dst, 'w', zipfile.ZIP_DEFLATED)
for it in zi.infolist():
    b = zi.read(it.filename)
    if it.filename == 'ppt/slides/slide3.xml':
        s = b.decode()
        s = s.replace('</p:spTree></p:cSld>', '</p:spTree><p:extLst><p:ext uri="{BB962C8B-B14F-4D97-AF65-F5344CB8AC3E}"><p14:creationId xmlns:p14="http://schemas.microsoft.com/office/powerpoint/2010/main" val="%d"/></p:ext></p:extLst></p:cSld>' % CID, 1)
        s = s.replace('</p:sld>', '<p:extLst><p:ext uri="{6950BFC3-D8DA-4A85-94F7-54DA5524770B}"><p188:commentRel xmlns:p188="%s" r:id="rId9"/></p:ext></p:extLst></p:sld>' % P188)
        b = s.encode()
    elif it.filename == 'ppt/slides/_rels/slide3.xml.rels':
        b = b.decode().replace('</Relationships>', '<Relationship Id="rId9" Type="http://schemas.microsoft.com/office/2018/10/relationships/comments" Target="../comments/modernComment_21E_499602D2.xml"/></Relationships>').encode()
    elif it.filename == 'ppt/_rels/presentation.xml.rels':
        b = b.decode().replace('</Relationships>', '<Relationship Id="rId99" Type="http://schemas.microsoft.com/office/2018/10/relationships/authors" Target="authors.xml"/></Relationships>').encode()
    elif it.filename == '[Content_Types].xml':
        b = b.decode().replace('</Types>', '<Override PartName="/%s" ContentType="application/vnd.ms-powerpoint.comments+xml"/><Override PartName="/ppt/authors.xml" ContentType="application/vnd.ms-powerpoint.authors+xml"/></Types>' % part).encode()
    zo.writestr(it, b)
zo.writestr(part, cm); zo.writestr('ppt/authors.xml', au); zo.close()
