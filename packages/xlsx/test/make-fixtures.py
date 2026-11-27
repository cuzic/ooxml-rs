"""Build local synthetic XLSX fixtures for the browser compatibility tests."""
from pathlib import Path
import sys
from shutil import copyfile
from zipfile import ZIP_DEFLATED, ZipFile


BASE = Path(__file__).resolve().parents[3] / "crates/ooxml-xlsx/template/blank.xlsx"
NS = "http://schemas.openxmlformats.org/spreadsheetml/2006/main"
REL = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"


def worksheet(dimension, rows, tail=""):
    extent = f'<dimension ref="{dimension}"/>' if dimension is not None else ""
    return f'<worksheet xmlns="{NS}">{extent}<sheetData>{rows}</sheetData>{tail}</worksheet>'


def make(output):
    with ZipFile(BASE) as source:
        parts = {name: source.read(name).decode("utf-8") for name in source.namelist()}
    parts["xl/workbook.xml"] = parts["xl/workbook.xml"].replace(
        "</sheets>", '<sheet name="Other" sheetId="2" r:id="rId5"/></sheets>'
    )
    parts["xl/_rels/workbook.xml.rels"] = parts["xl/_rels/workbook.xml.rels"].replace(
        "</Relationships>", f'<Relationship Id="rId5" Type="{REL}/worksheet" '
        'Target="worksheets/sheet2.xml"/></Relationships>'
    )
    parts["[Content_Types].xml"] = parts["[Content_Types].xml"].replace(
        "</Types>", '<Override PartName="/xl/worksheets/sheet2.xml" ContentType='
        '"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>'
    )
    parts["xl/sharedStrings.xml"] = (
        f'<sst xmlns="{NS}" count="2" uniqueCount="2">'
        '<si><t>plain</t></si><si><r><t>rich</t></r></si></sst>'
    )
    parts["xl/styles.xml"] = parts["xl/styles.xml"].replace(
        '<cellXfs count="1">', '<cellXfs count="2">'
    ).replace(
        '</cellXfs>', '<xf numFmtId="14" fontId="0" fillId="0" borderId="0" xfId="0"/></cellXfs>'
    )
    rows = (
        '<row r="1"><c r="A1"><v>1</v></c><c r="B1" t="b"><v>0</v></c>'
        '<c r="C1" t="s"><v>0</v></c><c r="D1" t="s"><v>1</v></c>'
        '<c r="E1"><f>A1+1</f><v>2</v></c>'
        '<c r="F1"><f t="array" ref="F1:F3">ROW(F1:F3)</f><v>1</v></c>'
        '<c r="H1" t="d"><v>2020-01-01</v></c><c r="I1" t="e"><v>#N/A</v></c><c r="J1" s="1"><v>43831</v></c></row>'
        '<row r="2"><c r="F2"><v>2</v></c><c r="I2" t="inlineStr"><is><r><t>inline rich</t></r></is></c></row>'
    )
    merges = '<mergeCells count="1"><mergeCell ref="G1:G2"/></mergeCells>'
    extension = '<extLst><ext uri="urn:test:compat"><p:data xmlns:p="urn:test:compat">keep</p:data></ext></extLst>'
    other = '<row r="1"><c r="A1"><v>3</v></c></row><row r="3"><c r="C3"><v>9</v></c></row>'
    sparse_rows = "".join(
        f'<row r="{row}"><c r="A{row}"><v>{row}</v></c><c r="AL{row}"><v>1</v></c></row>'
        for row in range(1, 40002, 40)
    )
    cases = {
        "iso-formula": (worksheet("A1:A1",
            '<row r="1"><c r="A1" s="1" t="d"><f>DATE(2024,2,3)</f>'
            '<v>2024-02-03T04:05:06.789Z</v></c></row>'), worksheet("A1:C3", other)),
        "sparse": (worksheet("A1:AL40001", sparse_rows), worksheet("A1:AL40001", sparse_rows)),
        "sheet-ids": (worksheet("A1:J8", rows, merges), worksheet("A1:C3", other)),
        "scalar": (worksheet("A1:J8", rows, merges), worksheet("A1:C3", other, extension)),
        "undersized": (worksheet("A1:J8", rows, merges), worksheet("A1:A1", other)),
        "absent": (worksheet("A1:J8", rows, merges), worksheet(None, other)),
        "empty": (worksheet("A1:J8", rows, merges), worksheet("A1:C3", "")),
        "incomplete": (worksheet("A1:J8", rows, merges), worksheet("A1:C3", '<row><c r="A1"><v>1</v></c></row>')),
    }
    output.mkdir(parents=True, exist_ok=True)
    dynamic = BASE.parents[3] / "reference/fixtures/xlsx/synthetic/o1_arrays.xlsx"
    copyfile(dynamic, output / "dynamic.xlsx")
    for name, (first, second) in cases.items():
        parts["xl/worksheets/sheet1.xml"] = first
        parts["xl/worksheets/sheet2.xml"] = second
        with ZipFile(output / f"{name}.xlsx", "w", compression=ZIP_DEFLATED) as target:
            for part, xml in parts.items():
                if name == "sheet-ids" and part == "xl/workbook.xml":
                    xml = xml.replace('sheetId="1"', 'sheetId="9"').replace('sheetId="2"', 'sheetId="4"')
                target.writestr(part, xml)


make(Path(sys.argv[1]))
