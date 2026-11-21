"""Read the generated Phase2 part 3 manual case using python-docx 1.2.0.

Run with python -I and pass the manual-check output directory.
"""

from pathlib import Path
import re
from sys import argv
from zipfile import ZipFile

from docx import Document
from docx.oxml.ns import qn
from lxml import etree


def check(directory):
    path = Path(directory) / "29-bookmark-list-style.docx"
    doc = read(path)
    check_registries(doc)
    check_notes(read(Path(directory) / "30-insert-footnote-endnote.docx"))
    check_comments(read(Path(directory) / "31-comment-thread.docx"))
    doc = read(Path(directory) / "32-comment-local-ignorable.docx")
    assert [comment.text for comment in doc.comments] == [
        "Existing comment.", "D3b paragraph-local ignorable comment."
    ]
    print("python-docx paragraph-local mc:Ignorable read-back passed")

    check_controls(Path(directory))
    check_fields(Path(directory))


def check_controls(directory):
    cases = {
        "37-sdt-plain.docx": ["Plain text edited"],
        "38-sdt-date.docx": ["November 21, 2026"],
        "39-sdt-checkbox.docx": ["☒"],
        "40-sdt-dropdown.docx": ["Ocean blue"],
        "41-sdt-bound.docx": ["Shared value", "Shared value"],
        "42-sdt-wrapped.docx": ["Wrapped paragraph"],
    }
    for name, values in cases.items():
        doc = read(directory / name)
        paragraphs = doc.element.xpath(".//w:p[ancestor::w:sdt or w:sdt]")
        assert ["".join(p.xpath(".//w:t/text()")) for p in paragraphs] == values, name
    print("python-docx content control read-back passed")


def check_fields(directory):
    cases = {
        "43-field-page.docx": r"Page: 1",
        "44-field-numpages.docx": r"Pages: 1",
        "45-field-sectionpages.docx": r"Section pages: 1",
        "46-field-date.docx": r"\(\d{4}-\d{2}-\d{2}\)",
        "47-field-time.docx": r"\(\d{2}:\d{2}\)",
        "48-field-docproperty.docx": r"Field property value",
        "49-field-ref.docx": r"Reference: Reference target",
        "50-field-pageref.docx": r"Reference page: \?",
        "51-field-seq.docx": r"Sequence: 1",
        "52-field-toc.docx": r"Table of contents — update this field\.",
    }
    for name, text in cases.items():
        doc = read(directory / name)
        assert re.fullmatch(text, doc.paragraphs[-1].text), name
    print("python-docx complex field cache read-back passed")


def read(path):
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

    return Document(path)


def check_registries(doc):
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


def check_notes(doc):
    for kind in ("footnote", "endnote"):
        reference, = doc.paragraphs[0]._p.xpath(f".//w:{kind}Reference")
        note_id = reference.get(qn("w:id"))
        notes = doc.part.part_related_by(
            f"http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}s"
        )
        root = etree.fromstring(
            notes.blob, etree.XMLParser(resolve_entities=False, load_dtd=False, no_network=True)
        )
        definitions = {note.get(qn("w:id")): note for note in root}
        assert len(definitions) == len(root)
        note = definitions[note_id]
        assert note.find(f".//{qn(f'w:{kind}Ref')}") is not None
        assert "".join(node.text for node in note.iter(qn("w:t"))) == f" D3a inserted {kind}."
        assert definitions["-1"].get(qn("w:type")) == "separator"
        assert definitions["0"].get(qn("w:type")) == "continuationSeparator"
        props, = doc.settings.element.xpath(f"w:{kind}Pr")
        assert [node.get(qn("w:id")) for node in props.findall(qn(f"w:{kind}"))] == ["-1", "0"]
    print("python-docx footnote and endnote read-back passed")


def check_comments(doc):
    parent, reply = list(doc.comments)
    assert (parent.author, parent.text) == ("Ada Lovelace", "D3b review comment.")
    assert (reply.author, reply.text) == ("Grace Hopper", "D3b review reply.")
    for comment in (parent, reply):
        ident = str(comment.comment_id)
        start, = doc.element.xpath(f".//w:commentRangeStart[@w:id='{ident}']")
        end, = doc.element.xpath(f".//w:commentRangeEnd[@w:id='{ident}']")
        node = start.getnext()
        texts = []
        while node is not end:
            assert node is not None
            texts.extend(t.text for t in node.findall(qn("w:t")))
            node = node.getnext()
        assert "".join(texts) == "this phrase"
    print("python-docx comment authors, bodies and ranges read-back passed")


if __name__ == "__main__":
    check(argv[1])
