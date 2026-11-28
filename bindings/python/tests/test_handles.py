"""Handles: open/save round-trips and thread basics (§11.1)."""

import threading
from pathlib import Path

import ooxml_rs as R

FIXTURES = Path(__file__).resolve().parent.parent.parent.parent / "reference" / "fixtures"


def test_workbook_new_round_trip(tmp_path):
    wb = R.Workbook.new()
    assert wb.sheet_names() == ["Sheet1"]
    blob = wb.to_bytes()
    assert R.Workbook.from_bytes(blob).sheet_names() == ["Sheet1"]
    out = tmp_path / "new.xlsx"
    wb.save(out)
    assert R.Workbook.open(out).to_bytes() == blob
    assert R.Workbook.open(str(out)).sheet_names() == ["Sheet1"]


def test_workbook_open_fixture():
    path = FIXTURES / "xlsx" / "poi" / "61605.xlsx"
    wb = R.Workbook.open(path)
    assert wb.sheet_names()
    assert R.Workbook.from_bytes(path.read_bytes()).sheet_names() == wb.sheet_names()


def test_document_round_trip(tmp_path):
    path = FIXTURES / "docx" / "poi-55733.docx"
    blob = path.read_bytes()
    doc = R.Document.from_bytes(blob)
    assert doc.to_bytes()
    out = tmp_path / "doc.docx"
    doc.save(out)
    assert R.Document.open(out).to_bytes() == doc.to_bytes()
    assert len(blob) > 0


def test_presentation_round_trip(tmp_path):
    candidates = sorted((FIXTURES / "pptx" / "poi").glob("*.pptx"))
    assert candidates, "need one pptx fixture"
    blob = candidates[0].read_bytes()
    deck = R.Presentation.from_bytes(blob)
    assert deck.to_bytes()
    out = tmp_path / "deck.pptx"
    deck.save(out)
    assert R.Presentation.open(out).to_bytes() == deck.to_bytes()


def test_handles_are_usable_across_threads():
    # No unsendable: a handle created on one thread works on another, and
    # calls on different handles run in parallel.
    wb = R.Workbook.new()
    seen: list[list[str]] = []

    def touch(handle):
        seen.append(handle.sheet_names())

    threads = [threading.Thread(target=touch, args=(wb,)) for _ in range(8)]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()
    assert seen == [["Sheet1"]] * 8

    errors: list[BaseException] = []

    def worker(_: int):
        try:
            R.Workbook.new().to_bytes()
        except BaseException as error:  # noqa: BLE001 - collected, then raised
            errors.append(error)

    threads = [threading.Thread(target=worker, args=(i,)) for i in range(8)]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()
    assert not errors
