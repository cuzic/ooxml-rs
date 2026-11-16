#!/usr/bin/env python3
"""Builds the synthetic O1 workbooks of ADR-0003 §7.1 (stdlib only).

Run from anywhere: `python3 -I reference/fixtures/xlsx/synthetic/build.py`.
The outputs are committed; no Python runs at test time. Raw XML is written
directly (openpyxl cannot write cached formula values). Zip entries use a
fixed timestamp so the output is reproducible byte for byte.

Cell specs:
  number / str            a value cell (str -> inline string)
  F(text, cached, ...)    a formula cell with cached value `cached`
                          (number, str -> t="str", E("#N/A") -> t="e",
                          None -> no v)
"""
import os
import zipfile
from xml.sax.saxutils import escape, quoteattr

HERE = os.path.dirname(os.path.abspath(__file__))
SML = "http://schemas.openxmlformats.org/spreadsheetml/2006/main"
R = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
DATE = (2026, 11, 16, 0, 0, 0)


class E:
    def __init__(self, v):
        self.v = v


class F:
    def __init__(self, text, cached=None, f_attrs=None, c_attrs=None):
        self.text, self.cached = text, cached
        self.f_attrs, self.c_attrs = f_attrs or {}, c_attrs or {}


def col_row(ref):
    i = 0
    while ref[i].isalpha():
        i += 1
    col = 0
    for ch in ref[:i]:
        col = col * 26 + ord(ch.upper()) - 64
    return col, int(ref[i:])


def attrs(d):
    return "".join(f" {k}={quoteattr(str(v))}" for k, v in d.items())


def cell_xml(ref, spec):
    if isinstance(spec, F):
        # Every formula cell carries a style and every row height/spans, so
        # that an oracle that ignored them would miss a corrupting edit
        # (review R3-M2).
        c_attrs = {"r": ref, "s": "0"}
        v = ""
        if isinstance(spec.cached, E):
            c_attrs["t"] = "e"
            v = f"<v>{escape(spec.cached.v)}</v>"
        elif isinstance(spec.cached, str):
            c_attrs["t"] = "str"
            v = f"<v>{escape(spec.cached)}</v>"
        elif spec.cached is not None:
            v = f"<v>{spec.cached!r}</v>" if isinstance(spec.cached, float) else f"<v>{spec.cached}</v>"
        c_attrs.update(spec.c_attrs)
        f = f"<f{attrs(spec.f_attrs)}>{escape(spec.text)}</f>" if spec.text else f"<f{attrs(spec.f_attrs)}/>"
        return f"<c{attrs(c_attrs)}>{f}{v}</c>"
    if isinstance(spec, str):
        return f'<c r="{ref}" t="inlineStr"><is><t>{escape(spec)}</t></is></c>'
    if isinstance(spec, tuple) and spec[0] == "result":
        return f'<c r="{ref}"><v>{spec[1]}</v></c>'
    return f'<c r="{ref}"><v>{spec}</v></c>'


def sheet_xml(cells, root="worksheet", root_ns=SML, prefix=""):
    rows = {}
    for ref, spec in cells.items():
        c, r = col_row(ref)
        rows.setdefault(r, []).append((c, ref, spec))
    out = []
    for r in sorted(rows):
        cs = "".join(cell_xml(ref, spec) for _, ref, spec in sorted(rows[r]))
        out.append(f'<row r="{r}" spans="1:11" ht="15">{cs}</row>')
    if root == "worksheet":
        head = f'<worksheet xmlns="{SML}" xmlns:r="{R}">'
        tail = "</worksheet>"
    else:
        head = f'<xm:macrosheet xmlns:xm="http://schemas.microsoft.com/office/excel/2006/main" xmlns="{SML}" xmlns:r="{R}">'
        tail = "</xm:macrosheet>"
    return f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n{head}<sheetData>{"".join(out)}</sheetData>{tail}'


STYLES = (
    f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<styleSheet xmlns="{SML}">'
    '<fonts count="1"><font><sz val="11"/><name val="Calibri"/></font></fonts>'
    '<fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills>'
    '<borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders>'
    '<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>'
    '<cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/></cellXfs>'
    '<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles>'
    "</styleSheet>"
)

METADATA_XLDAPR = (
    f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<metadata xmlns="{SML}" '
    'xmlns:xda="http://schemas.microsoft.com/office/spreadsheetml/2017/dynamicarray">'
    '<metadataTypes count="1"><metadataType name="XLDAPR" minSupportedVersion="120000" copy="1" pasteAll="1" '
    'pasteValues="1" merge="1" splitFirst="1" rowColShift="1" clearFormats="1" clearComments="1" assign="1" '
    'coerce="1" cellMeta="1"/></metadataTypes><futureMetadata name="XLDAPR" count="1"><bk><extLst>'
    '<ext uri="{bdbb8cdc-fa1e-496e-a857-3c3f30c029c3}"><xda:dynamicArrayProperties fDynamic="1" fCollapsed="0"/>'
    "</ext></extLst></bk></futureMetadata><cellMetadata count=\"1\"><bk><rc t=\"1\" v=\"0\"/></bk></cellMetadata></metadata>"
)

METADATA_OTHER = (
    f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<metadata xmlns="{SML}">'
    '<metadataTypes count="1"><metadataType name="XLSOMETHING" minSupportedVersion="120000"/></metadataTypes>'
    '<cellMetadata count="1"><bk><rc t="1" v="0"/></bk></cellMetadata></metadata>'
)


def build(name, sheets, names=(), calc_pr='calcId="191029"', calc_chain=True, metadata=None):
    """sheets: list of (name, cells, kind) with kind 'ws' or 'macro'."""
    files = {}
    ct = [
        '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>',
        '<Default Extension="xml" ContentType="application/xml"/>',
        '<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>',
        '<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>',
    ]
    rels = []
    sheet_els = []
    chain = []
    for i, (sname, cells, kind) in enumerate(sheets, start=1):
        if kind == "macro":
            path = f"macrosheets/sheet{i}.xml"
            files[f"xl/{path}"] = sheet_xml(cells, root="macrosheet")
            ct.append(f'<Override PartName="/xl/{path}" ContentType="application/vnd.ms-excel.macrosheet+xml"/>')
            rels.append(f'<Relationship Id="rId{i}" Type="http://schemas.microsoft.com/office/2006/relationships/xlMacrosheet" Target="{path}"/>')
        else:
            path = f"worksheets/sheet{i}.xml"
            files[f"xl/{path}"] = sheet_xml(cells)
            ct.append(f'<Override PartName="/xl/{path}" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>')
            rels.append(f'<Relationship Id="rId{i}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="{path}"/>')
        sheet_els.append(f'<sheet name={quoteattr(sname)} sheetId="{i}" r:id="rId{i}"/>')
        for ref, spec in sorted(cells.items(), key=lambda kv: col_row(kv[0])[::-1]):
            if isinstance(spec, F):
                chain.append(f'<c r="{ref}" i="{i}"/>')
    n = len(sheets)
    rels.append(f'<Relationship Id="rId{n + 1}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>')
    files["xl/styles.xml"] = STYLES
    if calc_chain and chain:
        files["xl/calcChain.xml"] = f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<calcChain xmlns="{SML}">{"".join(chain)}</calcChain>'
        ct.append('<Override PartName="/xl/calcChain.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.calcChain+xml"/>')
        rels.append(f'<Relationship Id="rId{n + 2}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/calcChain" Target="calcChain.xml"/>')
    if metadata:
        files["xl/metadata.xml"] = metadata
        ct.append('<Override PartName="/xl/metadata.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheetMetadata+xml"/>')
        rels.append(f'<Relationship Id="rId{n + 3}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sheetMetadata" Target="metadata.xml"/>')
    dn = ""
    if names:
        items = []
        for nm, text, local in names:
            la = f' localSheetId="{local}"' if local is not None else ""
            items.append(f"<definedName name={quoteattr(nm)}{la}>{escape(text)}</definedName>")
        dn = f"<definedNames>{''.join(items)}</definedNames>"
    calc = f"<calcPr {calc_pr}/>" if calc_pr is not None else ""
    files["xl/workbook.xml"] = (
        f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<workbook xmlns="{SML}" xmlns:r="{R}">'
        f"<sheets>{''.join(sheet_els)}</sheets>{dn}{calc}</workbook>"
    )
    files["xl/_rels/workbook.xml.rels"] = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
        f'<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{"".join(rels)}</Relationships>'
    )
    files["_rels/.rels"] = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
        '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>'
        "</Relationships>"
    )
    files["[Content_Types].xml"] = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
        f'<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">{"".join(ct)}</Types>'
    )
    order = ["[Content_Types].xml", "_rels/.rels", "xl/workbook.xml", "xl/_rels/workbook.xml.rels"]
    order += sorted(k for k in files if k not in order)
    with zipfile.ZipFile(os.path.join(HERE, name), "w", zipfile.ZIP_DEFLATED) as z:
        for k in order:
            info = zipfile.ZipInfo(k, DATE)
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, files[k])


# --- o1_precise: cross-sheet, names (local/global/sheet-qualified), 3-D, A:A, 1:1,
#     lower-case sheet name, transitive chain, SUMIF resizing, independents.
build(
    "o1_precise.xlsx",
    [
        ("Sheet1", {
            "A1": 5, "A2": 1, "A3": 2,
            "B1": F("A1*3", 15),
            "C1": F("7*2", 14),
            "D1": F("Sheet2!A1+1", 11),
            "E1": F("SUM(A2:A3)", 3),
        }, "ws"),
        ("Sheet2", {
            "A1": 10,
            "B1": F("Sheet1!A1*2", 10),
            "C1": F("B1+1", 11),
            "D1": F("Rate*2", 20),
        }, "ws"),
        ("Sheet3", {
            "A1": F("Sheet2!C1*2", 22),
            "B1": F("'sheet2'!A1", 10),
            "C1": F("SUM(Sheet1:Sheet2!A1)", 15),
            "D1": F("SUM(Sheet2:Sheet1!A2)", 1),
            "E1": F("SUM(Sheet1!A:A)", 8),
            "F1": F("SUM(Sheet1!3:3)", 2),
            "G1": F("Rate", 2),
            "H1": F("Defines!NR", 1),
            "I1": F("NR", 10),
            "J1": F('SUMIF(Sheet1!A1:A3,">0",Sheet1!B1)', 15),
            "K1": F("LEN(\"A1\")+ROUND(PI(),0)", 5),
        }, "ws"),
        ("Defines", {"A1": 1}, "ws"),
    ],
    names=[
        ("Rate", "Sheet1!$A$3", None),
        ("Rate", "Sheet2!$A$1", 1),
        ("NR", "Sheet1!$A$2", 3),
        ("NR", "Sheet2!$A$1", None),
    ],
)

# --- o1_everything_<tag>: one construct each that must depend on everything.
#     Sheet1: A1..A3 values, B1 = construct, C1 = independent, D1 = B1+1.
EVERYTHING = {
    "indirect": (F('INDIRECT("A1")', 1), {}),
    "offset": (F("OFFSET(A1,0,0)", 1), {}),
    "lookup": (F("LOOKUP(2,A1:A3,E1)", 0), {}),
    "sumif_computed": (F('SUMIF(A1:A3,">0",INDEX(E1:E3,1))', 0), {}),
    "range_computed": (F("SUM(A1:INDEX(E1:E3,2))", 6), {}),
    "udf": (F("myFunc(A1)", 1), {}),
    "external": (F("[1]Sheet1!A1", 0), {}),
    "unknown_sheet": (F("Nope!A1", E("#REF!")), {}),
    "structured": (F("SUM(Table1[Col])", 0), {}),
    "relative_name": (F("Rel", 1), {"names": [("Rel", "Sheet1!A1", None)]}),
    "name_collision": (F("TAX2024", 1), {"names": [("TAX2024", "Sheet1!$A$1", None)]}),
    "volatile_ca": (F("A1+0", 1, f_attrs={"ca": "1"}), {}),
    "spill_error": (F("_xlfn.SEQUENCE(3)", E("#SPILL!")), {}),
    "data_table": (F("", 1, f_attrs={"t": "dataTable", "ref": "B1", "dt2D": "0", "dtr": "0", "r1": "A1"}), {}),
    "bx_name": (F("Total+0", 6), {"names": [("Total", "Sheet1!$A$1:$A$3", None)], "bx": True}),
}
for tag, (b1, opts) in EVERYTHING.items():
    cells = {"A1": 1, "A2": 2, "A3": 3, "B1": b1, "C1": F("5*2", 10), "D1": F("B1+1", 2)}
    if opts.get("bx"):
        cells["F1"] = F("1", 1, f_attrs={"bx": "1"})
    build(f"o1_everything_{tag}.xlsx", [("Sheet1", cells, "ws")], names=opts.get("names", ()))

# --- o1_arrays: CSE array + result reader, dynamic array + reader below the spill.
build(
    "o1_arrays.xlsx",
    [("Sheet1", {
        "A1": 1, "A2": 2, "A3": 3, "A9": 4,
        "B1": F("A1:A3*2", 2, f_attrs={"t": "array", "ref": "B1:B3"}),
        "B2": ("result", 4), "B3": ("result", 6),
        "C1": F("B2+1", 5),
        "D1": F("A1:A3+0", 1, f_attrs={"t": "array", "ref": "D1:D3"}, c_attrs={"cm": "1"}),
        "D2": ("result", 2), "D3": ("result", 3),
        "E1": F("D5*2", 0),
        "F1": F("A9*2", 8),
        "G1": F("1+1", 2),
    }, "ws")],
    metadata=METADATA_XLDAPR,
)

# --- o1_shared: shared groups (1-D, master not top-left).
build(
    "o1_shared.xlsx",
    [
        ("Sheet2", {
            "A2": 1, "A3": 2, "A4": 3, "A5": 4,
            "B2": F("A2*2", 2, f_attrs={"t": "shared", "ref": "B2:B5", "si": "0"}),
            "B3": F("", 4, f_attrs={"t": "shared", "si": "0"}),
            "B4": F("", 6, f_attrs={"t": "shared", "si": "0"}),
            "B5": F("", 8, f_attrs={"t": "shared", "si": "0"}),
            "C1": 10, "D1": 20, "E1": 30,
            "D2": F("$A2+D$1", 21, f_attrs={"t": "shared", "ref": "C2:E3", "si": "1"}),
            "C2": F("", 11, f_attrs={"t": "shared", "si": "1"}),
            "E2": F("", 31, f_attrs={"t": "shared", "si": "1"}),
            "C3": F("", 12, f_attrs={"t": "shared", "si": "1"}),
            "D3": F("", 22, f_attrs={"t": "shared", "si": "1"}),
            "E3": F("", 32, f_attrs={"t": "shared", "si": "1"}),
            "H9": F("SUM(B2:B5)", 20),
        }, "ws"),
    ],
)

# --- o1_shared_oob: quoted sheet name, a member whose translation leaves the sheet
#     (its precedents are Everything; overwriting the master must fail).
build(
    "o1_shared_oob.xlsx",
    [
        ("Q1 2024", {"A1": 3, "A2": 4}, "ws"),
        ("Sheet2", {
            "G3": F("'Q1 2024'!A1+G2", 3, f_attrs={"t": "shared", "ref": "G2:G3", "si": "2"}),
            "G2": F("", 3, f_attrs={"t": "shared", "si": "2"}),
        }, "ws"),
    ],
)

# --- o1_name_scope: an unqualified reference inside a local name, used from
#     another sheet (review R3-M1 a). Excel reads it on the using sheet, so
#     S1!C1 = S1!A1 = 1 (cached), not S2!A1 = 7. Depends on everything.
build(
    "o1_name_scope.xlsx",
    [
        ("S1", {"A1": 1, "C1": F("S2!Loc", 1), "D1": F("2*3", 6)}, "ws"),
        ("S2", {"A1": 7}, "ws"),
    ],
    names=[("Loc", "$A$1", 1)],
)

# --- o3_canary: a positive control for O3 (PR #21 review M2). B1's cached
#     value is deliberately wrong (3; A1+1 is 2). If LibreOffice really
#     recalculates, its output says 2; spikes/xlsx/o3_recalc.py and
#     tests/phase1_o3.rs both fail otherwise. Not an O1 workbook.
build(
    "o3_canary.xlsx",
    [("Sheet1", {"A1": 1, "B1": F("A1+1", 3), "C1": F("A1*10", 10)}, "ws")],
)

# --- calculation modes and workbook-wide refusals.
build("o1_manual.xlsx", [("Sheet1", {"A1": 1, "B1": F("A1*2", 2), "C1": F("3", 3)}, "ws")], calc_pr='calcId="191029" calcMode="manual"')
build("o1_iterate.xlsx", [("Sheet1", {"A1": F("A1+B1", 1), "B1": 1, "C1": 5}, "ws")], calc_pr='calcId="191029" iterate="1"')
build(
    "o1_macro.xlsx",
    [("Sheet1", {"A1": 1, "B1": F("A1*2", 2)}, "ws"), ("Macro1", {"A1": F("Sheet1!A1+1", 2)}, "macro")],
)
build("o1_unknown_cm.xlsx", [("Sheet1", {"A1": 1, "B1": F("A1*2", 2, c_attrs={"cm": "1"})}, "ws")], metadata=METADATA_OTHER)
build("o1_no_calcpr.xlsx", [("Sheet1", {"A1": 1, "B1": F("A1*2", 2)}, "ws")], calc_pr=None, calc_chain=False)
