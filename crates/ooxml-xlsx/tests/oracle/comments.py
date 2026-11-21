"""Openpyxl 3.1.5 read-back for the P5c manual-check workbooks."""
import re
import sys
from pathlib import Path

from openpyxl import load_workbook

out = Path(sys.argv[1])
with (out / "40-p5c-note.xlsx").open("rb") as source:
    wb = load_workbook(source)
    note = wb["Sheet1"]["B2"].comment
    assert note.text == "Quarterly budget", note.text
    assert note.author == "Ana", note.author
    wb.close()
with (out / "41-p5c-thread.xlsx").open("rb") as source:
    wb = load_workbook(source)
    note = wb["Sheet1"]["D3"].comment
    assert note.text == "[Threaded comment]\n\nLooks wrong\n\nFixed", note.text
    assert re.fullmatch(
        r"tc=\{[0-9A-F]{8}-[0-9A-F]{4}-4[0-9A-F]{3}-[89AB][0-9A-F]{3}-[0-9A-F]{12}\}",
        note.author,
    ), note.author
    wb.close()
print("openpyxl read the P5c note and threaded fallback")
