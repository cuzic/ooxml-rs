"""Document content API (ADR-0023 §12, U9)."""

import io
import zipfile

import pytest

import ooxml_rs as R

W_NS = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"


def package(body: str) -> bytes:
    """A minimal docx with `body` as the body's inner XML."""
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as zf:
        zf.writestr(
            "[Content_Types].xml",
            '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
            '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
            '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
            '<Default Extension="xml" ContentType="application/xml"/>'
            '<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>'
            "</Types>",
        )
        zf.writestr(
            "_rels/.rels",
            '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
            '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
            '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>'
            "</Relationships>",
        )
        zf.writestr(
            "word/_rels/document.xml.rels",
            '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
            '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"></Relationships>',
        )
        zf.writestr(
            "word/document.xml",
            '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
            f'<w:document xmlns:w="{W_NS}" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">'
            f"<w:body>{body}</w:body></w:document>",
        )
    return buf.getvalue()


def para(text: str) -> str:
    return f'<w:p><w:r><w:t xml:space="preserve">{text}</w:t></w:r></w:p>'


FIELD = (
    '<w:r><w:fldChar w:fldCharType="begin"/></w:r>'
    "<w:r><w:instrText> DATE </w:instrText></w:r>"
    '<w:r><w:fldChar w:fldCharType="separate"/></w:r>'
    "<w:r><w:t>{{d}}</w:t></w:r>"
    '<w:r><w:fldChar w:fldCharType="end"/></w:r>'
)


def test_paragraph_texts_and_append(tmp_path):
    doc = R.Document.from_bytes(package(para("one") + para("two")))
    assert doc.paragraph_texts() == ["one", "two"]
    assert doc.append_paragraph("three") == 2
    assert doc.paragraph_texts() == ["one", "two", "three"]
    out = tmp_path / "a.docx"
    doc.save(out)
    assert R.Document.open(out).paragraph_texts() == ["one", "two", "three"]


def test_replace_counts_and_is_atomic(tmp_path):
    doc = R.Document.from_bytes(package(para("Dear {{name}}, hi {{name}}")))
    assert doc.replace("{{name}}", "Ada") == 2
    assert doc.paragraph_texts() == ["Dear Ada, hi Ada"]
    assert doc.replace("missing", "X") == 0
    out = tmp_path / "r.docx"
    doc.save(out)
    assert R.Document.open(out).paragraph_texts() == ["Dear Ada, hi Ada"]


def test_replace_many_applies_in_order():
    doc = R.Document.from_bytes(package(para("a {{x}} b")))
    assert doc.replace_many([("{{x}}", "{{y}}"), ("{{y}}", "done")]) == 2
    assert doc.paragraph_texts() == ["a done b"]


def test_refuse_mode_raises_and_writes_nothing():
    before = package(f"<w:p>{FIELD}</w:p>")
    doc = R.Document.from_bytes(before)
    with pytest.raises(R.RefusedError) as caught:
        doc.replace("{{d}}", "X")
    assert (caught.value.crate, caught.value.kind) == ("docx", "ReplaceRefused")
    assert doc.to_bytes() == before


def test_skip_mode_returns_dicts_and_writes_the_rest():
    doc = R.Document.from_bytes(package(f"<w:p>{FIELD}</w:p>" + para("plain {{d}}")))
    count, skipped = doc.replace("{{d}}", "X", on_refused="skip")
    assert count == 1
    assert skipped == [
        {"story": 0, "paragraph": 0, "start": 0, "end": 5, "reason": "InsideField"}
    ]
    assert doc.paragraph_texts()[1] == "plain X"


def test_skip_mode_many_concatenates():
    doc = R.Document.from_bytes(package(f"<w:p>{FIELD}</w:p>"))
    count, skipped = doc.replace_many([("{{d}}", "X"), ("{{d}}", "Y")], on_refused="skip")
    assert count == 0
    assert len(skipped) == 2
    assert all(entry["reason"] == "InsideField" for entry in skipped)


def test_replace_rejects_bad_mode():
    doc = R.Document.from_bytes(package(para("x")))
    with pytest.raises(ValueError, match="on_refused"):
        doc.replace("x", "y", on_refused="bogus")
    with pytest.raises(ValueError, match="on_refused"):
        doc.replace_many([("x", "y")], on_refused="bogus")
