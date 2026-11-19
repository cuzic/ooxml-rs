"""Read the generated Phase2 part 3 manual case using python-docx 1.2.0.

Run with python -I and pass the manual-check output directory.
"""

from pathlib import Path
from sys import argv
from zipfile import ZipFile

from docx import Document
from docx.oxml.ns import qn
from lxml import etree


def check(directory):
    path = Path(directory) / "29-bookmark-list-style.docx"
    # Preflight ZIP XML without resolving entities, and never hand a part
    # with a DTD to the library under test.
    parser = etree.XMLParser(resolve_entities=False, load_dtd=False, no_network=True)
    with ZipFile(path) as package:
        for name in package.namelist():
            if name.endswith((".xml", ".rels")):
                data = package.read(name)
                if b"<!DOCTYPE" in data.replace(b"\x00", b""):
                    raise ValueError(f"DTD in {name}")
                etree.fromstring(data, parser)

    doc = Document(path)
    first, restart, plain = doc.paragraphs
    assert [p.text for p in doc.paragraphs] == [
        "First item, bookmarked", "Restart at five", "Last line, unnumbered"
    ]
    start, = doc.element.xpath(".//w:bookmarkStart")
    end, = doc.element.xpath(".//w:bookmarkEnd")
    assert start.get(qn("w:name")) == "ManualBookmark"
    assert start.get(qn("w:id")) == end.get(qn("w:id"))
    assert start.getnext().xpath("w:t/text()") == ["bookmarked"]
    assert start.getnext().getnext() is end

    first_num = first._p.pPr.numPr
    restart_num = restart._p.pPr.numPr
    assert first_num.ilvl.val == restart_num.ilvl.val == 0
    assert first_num.numId.val != restart_num.numId.val
    assert plain._p.pPr.numPr is None
    numbering = doc.part.numbering_part.element
    base = numbering.num_having_numId(first_num.numId.val)
    restarted = numbering.num_having_numId(restart_num.numId.val)
    assert base.abstractNumId.val == restarted.abstractNumId.val
    override, = restarted.lvlOverride_lst
    assert override.ilvl == 0 and override.startOverride.val == 5

    assert plain.style.style_id == "ManualCustom"
    assert plain.style.name == "Manual Custom" and plain.style.font.bold is True
    assert plain.style.element.get(qn("w:customStyle")) == "1"
    assert plain.style.element.xpath("w:qFormat")
    print("python-docx bookmark, numbering and style read-back passed")


if __name__ == "__main__":
    check(argv[1])
