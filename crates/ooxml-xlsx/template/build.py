"""Builds blank.xlsx, the template Workbook::new() opens (ADR-0001 "From-scratch path").

Run from anywhere: python3 -I crates/ooxml-xlsx/template/build.py
The output is deterministic (fixed entry order and timestamps). The test
`template_matches_sources` checks that blank.xlsx and blank/ agree.
"""
import os
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(HERE, "blank")
OUT = os.path.join(HERE, "blank.xlsx")
ORDER = [
    "[Content_Types].xml",
    "_rels/.rels",
    "docProps/app.xml",
    "docProps/core.xml",
    "xl/workbook.xml",
    "xl/_rels/workbook.xml.rels",
    "xl/styles.xml",
    "xl/sharedStrings.xml",
    "xl/theme/theme1.xml",
    "xl/worksheets/sheet1.xml",
]

found = sorted(
    os.path.relpath(os.path.join(d, f), SRC).replace(os.sep, "/")
    for d, _, fs in os.walk(SRC)
    for f in fs
)
assert found == sorted(ORDER), f"template parts differ from ORDER: {found}"
with zipfile.ZipFile(OUT, "w") as z:
    for name in ORDER:
        info = zipfile.ZipInfo(name, date_time=(2026, 11, 16, 0, 0, 0))
        info.compress_type = zipfile.ZIP_DEFLATED
        info.external_attr = 0o644 << 16
        with open(os.path.join(SRC, name), "rb") as f:
            z.writestr(info, f.read(), compresslevel=9)
print(OUT, os.path.getsize(OUT))
