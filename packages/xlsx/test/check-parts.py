"""Check the original OOXML bytes and cache locality for browser test outputs."""
import re
from pathlib import Path
import sys
from zipfile import ZipFile


def parts(path):
    with ZipFile(path) as archive:
        return {name: archive.read(name) for name in archive.namelist()}


def cell(xml, address):
    match = re.search(rb'<c r="' + address + rb'"(?: [^>]+)?>(?:(?!</c>).)*</c>', xml, re.DOTALL)
    assert match is not None, address
    return match.group()


def without(xml, *addresses):
    for address in addresses:
        xml = xml.replace(cell(xml, address), b"", 1)
    return xml


source, sheet_file, excel_file = (parts(Path(path)) for path in sys.argv[1:])
sheet1 = "xl/worksheets/sheet1.xml"
sheet2 = "xl/worksheets/sheet2.xml"
shared = "xl/sharedStrings.xml"
workbook = "xl/workbook.xml"

for output, changed in [
    (sheet_file, {sheet1, sheet2, shared, workbook}),
    (excel_file, {sheet1, workbook}),
]:
    assert output.keys() == source.keys()
    assert {name for name in source if source[name] != output[name]} == changed
    assert without(source[sheet1], b"A1", b"E1") == without(output[sheet1], b"A1", b"E1")
    assert cell(source[sheet1], b"E1").replace(b"<v>2</v>", b"") == cell(output[sheet1], b"E1")
    assert b'fullCalcOnLoad="1"' in output[workbook]
assert source[sheet2] == without(sheet_file[sheet2], b"B2").replace(b'<row r="2"></row>', b"")
print("PASS: edited ZIP parts, worksheet locality and dependent formula cache")
