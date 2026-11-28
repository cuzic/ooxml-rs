"""Presentation content API (ADR-0023 §12, U9)."""

import io
import zipfile
from pathlib import Path

import pytest

import ooxml_rs as R

FIXTURES = Path(__file__).resolve().parent.parent.parent.parent / "reference" / "fixtures"
SAMPLE = FIXTURES / "pptx" / "poi" / "sample.pptx"

SUBTITLE = (
    '<a:r><a:rPr lang="en-US">'
    '<a:sym typeface="Wingdings" pitchFamily="2" charset="2"/>'
    "</a:rPr>"
    "<a:t>Nunc at risus vel erat tempus posuere. Aenean non ante.</a:t></a:r>"
)


def run(text: str) -> str:
    return f'<a:r><a:rPr lang="en-US"/><a:t>{text}</a:t></a:r>'


def with_subtitle(runs: str) -> bytes:
    """sample.pptx with slide 1's subtitle paragraph replaced (in memory)."""
    with zipfile.ZipFile(SAMPLE) as zf:
        names = zf.namelist()
        slide = zf.read("ppt/slides/slide1.xml").decode()
    assert SUBTITLE in slide
    slide = slide.replace(SUBTITLE, runs, 1)
    buf = io.BytesIO()
    with zipfile.ZipFile(SAMPLE) as zf, zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as out:
        for name in names:
            out.writestr(name, slide.encode() if name == "ppt/slides/slide1.xml" else zf.read(name))
    return buf.getvalue()


FIELD_RUNS = (
    run("one")
    + "<a:br/>"
    + run("two ")
    + '<a:fld id="{B6F15528-21DE-4FAA-801E-634DDDAF4B2B}" type="slidenum">'
    + '<a:rPr lang="en-US"/><a:t>7</a:t></a:fld>'
    + run(" three")
)


def test_slide_texts_lists_every_slide():
    deck = R.Presentation.open(SAMPLE)
    texts = deck.slide_texts()
    assert len(texts) == 2
    assert "Lorem ipsum" in texts[0]
    assert all(isinstance(text, str) for text in texts)


def test_replace_all_counts_and_round_trips(tmp_path):
    deck = R.Presentation.open(SAMPLE)
    count = deck.replace_all("Lorem", "LOREM")
    assert count >= 1
    assert "LOREM" in deck.slide_texts()[0]
    assert deck.replace_all("no-such-pattern-zz", "X") == 0
    out = tmp_path / "r.pptx"
    deck.save(out)
    assert "LOREM" in R.Presentation.open(out).slide_texts()[0]


def test_replace_all_aborts_on_a_refused_match():
    before = with_subtitle(FIELD_RUNS)
    deck = R.Presentation.from_bytes(before)
    with pytest.raises(R.RefusedError) as caught:
        deck.replace_all("two 7", "X")
    assert (caught.value.crate, caught.value.kind) == ("binding", "ReplaceAborted")
    assert deck.to_bytes() == before


def test_slide_operations(tmp_path):
    deck = R.Presentation.open(SAMPLE)
    assert len(deck.slide_texts()) == 2
    assert deck.add_slide(0) == 2
    assert len(deck.slide_texts()) == 3
    first = deck.slide_texts()[0]
    assert deck.duplicate_slide(0) == 1
    assert deck.slide_texts()[1] == first
    assert len(deck.slide_texts()) == 4
    deck.move_slide(0, 3)
    assert deck.slide_texts()[3] == first
    deck.delete_slide(0)
    assert len(deck.slide_texts()) == 3
    out = tmp_path / "s.pptx"
    deck.save(out)
    assert len(R.Presentation.open(out).slide_texts()) == 3


def test_slide_operations_refuse_bad_indexes():
    deck = R.Presentation.open(SAMPLE)
    with pytest.raises(IndexError, match="layout index"):
        deck.add_slide(999)
    with pytest.raises(R.InvalidInputError):
        deck.delete_slide(999)
    with pytest.raises(R.InvalidInputError):
        deck.move_slide(0, 999)
    with pytest.raises(R.InvalidInputError):
        deck.duplicate_slide(999)
