#!/usr/bin/env python3
"""Verify the editable OOXML outputs of cargo xtask manual-check."""
import argparse
from collections import Counter
import csv
from decimal import Decimal
import json
import math
import os
from pathlib import Path
import posixpath
import re
import shutil
import subprocess
import tempfile
from xml.parsers import expat
import xml.etree.ElementTree as ET
import zipfile

from docx import Document
from pptx import Presentation

SML = "{http://schemas.openxmlformats.org/spreadsheetml/2006/main}"
RID = "{http://schemas.openxmlformats.org/officeDocument/2006/relationships}id"
FILTERS = {
    ".xlsx": "Calc MS Excel 2007 XML",
    ".xlsm": "Calc MS Excel 2007 VBA XML",
    ".docx": "Office Open XML Text",
    ".pptx": "Impress MS PowerPoint 2007 XML",
}
SPREADSHEETS = {".xlsx", ".xlsm"}
CHECKS = {
    "a": "PDF export",
    "b": "Expected content",
    "c": "Forced recalculation",
    "d": "Independent read-back",
}
# The default Writer PDF export crashes on the SDK footnote/endnote fixture
# in LibreOffice 25.2.7.2. This check only needs a plain PDF.
WRITER_PDF = 'pdf:writer_pdf_Export:{"UseTaggedPDF":{"type":"boolean","value":"false"}}'
# Export every Calc sheet (option 12 = -1). Calc still formats dates and
# percentages with option 9 (save as shown) turned off.
CSV_FILTER = "csv:Text - txt - csv (StarCalc):44,34,76,1,,0,false,false,false,false,false,-1"
PROFILE = """<?xml version="1.0" encoding="UTF-8"?>
<oor:items xmlns:oor="http://openoffice.org/2001/registry">
  <item oor:path="/org.openoffice.Office.Calc/Formula/Load">
    <prop oor:name="OOXMLRecalcMode" oor:op="fuse"><value>0</value></prop>
  </item>
  <item oor:path="/org.openoffice.Office.Common/Security/Scripting">
    <prop oor:name="MacroSecurityLevel" oor:op="fuse"><value>3</value></prop>
  </item>
</oor:items>
"""
LOAD_ERROR = re.compile(
    r"^\s*error:|source file could not be loaded|could not load|failed to load", re.I | re.M
)


def require(condition, message):
    if not condition:
        raise ValueError(message)


def reject_doctype(*_):
    raise ValueError("DTD in OOXML part")


def preflight(path):
    # Probe the XML with Expat so the DTD check also works for UTF-16 parts.
    # Leave syntax diagnostics to the application/oracle under test.
    with zipfile.ZipFile(path) as archive:
        for name in archive.namelist():
            if name.lower().endswith((".xml", ".rels")):
                parser = expat.ParserCreate()
                parser.StartDoctypeDeclHandler = reject_doctype
                with archive.open(name) as part:
                    try:
                        parser.ParseFile(part)
                    except expat.ExpatError:
                        pass


def run(command, *, conversion=False):
    result = subprocess.run(
        command, capture_output=True, text=True, errors="replace", timeout=90
    )
    output = (result.stdout + result.stderr).strip()
    require(result.returncode == 0 and not (conversion and LOAD_ERROR.search(output)),
            output or f"{command[0]} exited {result.returncode}")
    return output if conversion else result.stdout


def convert(profile, source, target, format_spec):
    target.mkdir(exist_ok=True)
    output = run([
        "soffice", f"-env:UserInstallation={profile.as_uri()}", "--headless",
        "--convert-to", format_spec, "--outdir", target, source,
    ], conversion=True)
    suffix = format_spec.split(":", 1)[0]
    path = target / source.with_suffix(f".{suffix}").name
    require(path.is_file() and path.stat().st_size, f"missing or empty {path.name}: {output}")
    return path


def text_nodes(node):
    return "".join(t.text or "" for t in node.iter(SML + "t"))


def workbook(path):
    """Use workbook relationships, including for reordered and hidden sheets."""
    with zipfile.ZipFile(path) as archive:
        book = ET.fromstring(archive.read("xl/workbook.xml"))
        rels = ET.fromstring(archive.read("xl/_rels/workbook.xml.rels"))
        targets = {rel.attrib["Id"]: rel.attrib["Target"] for rel in rels}
        strings = []
        if "xl/sharedStrings.xml" in archive.namelist():
            strings = [
                text_nodes(si) for si in ET.fromstring(archive.read("xl/sharedStrings.xml"))
            ]
        sheets = {}
        for sheet in book.iter(SML + "sheet"):
            target = targets[sheet.attrib[RID]]
            part = target.lstrip("/") if target.startswith("/") else posixpath.join("xl", target)
            root = ET.fromstring(archive.read(posixpath.normpath(part)))
            cells = {}
            for cell in root.iter(SML + "c"):
                kind = cell.get("t", "n")
                value = cell.findtext(SML + "v")
                if kind == "s" and value is not None:
                    kind, value = "str", strings[int(value)]
                elif kind == "inlineStr":
                    kind, value = "str", text_nodes(cell)
                formula = cell.find(SML + "f") is not None
                cells[cell.attrib["r"]] = kind, value, formula
            sheets[sheet.attrib["name"]] = cells
        return sheets


def same_value(kind, expected, actual):
    if expected == actual:
        return True
    if kind == "n" and actual not in (None, "") and expected not in (None, ""):
        return math.isclose(Decimal(expected), Decimal(actual), rel_tol=1e-12)
    return False


def csv_cell(rows, address):
    column, row = re.fullmatch(r"([A-Z]+)([1-9][0-9]*)", address).groups()
    index = 0
    for letter in column:
        index = index * 26 + ord(letter) - ord("A") + 1
    row_index = int(row) - 1
    return rows[row_index][index - 1] if row_index < len(rows) and index <= len(rows[row_index]) else ""


def sheet_csv(profile, source, outdir, names):
    outdir.mkdir()
    output = run([
        "soffice", f"-env:UserInstallation={profile.as_uri()}", "--headless",
        "--convert-to", CSV_FILTER, "--outdir", outdir, source,
    ], conversion=True)
    for name in names:
        require((outdir / f"{source.stem}-{name}.csv").is_file(),
                f"missing CSV for sheet {name!r}: {output}")


def check_content(source, expected, profile, scratch):
    if source.suffix in SPREADSHEETS:
        sheets = workbook(source)
        require(list(sheets) == list(expected["sheets"]), f"sheet names/order: {list(sheets)!r}")
        outdir = scratch / "csv"
        sheet_csv(profile, source, outdir, sheets)
        for name, cells in expected["sheets"].items():
            with (outdir / f"{source.stem}-{name}.csv").open(encoding="utf-8-sig", newline="") as file:
                rows = list(csv.reader(file))
            for address, value in cells.items():
                actual = csv_cell(rows, address)
                kind = "str" if isinstance(value, str) else "n"
                require(same_value(kind, str(value), actual),
                        f"{name}!{address}: expected {value!r}, got {actual!r}")
        return
    if source.suffix == ".docx":
        path = convert(profile, source, scratch / "txt", "txt:Text (encoded):UTF8")
        require_text(path.read_text(encoding="utf-8-sig"), expected["text"])
    if source.suffix == ".pptx" or "pdf_text" in expected:
        pdf = scratch / "pdf" / source.with_suffix(".pdf").name
        text = run(["pdftotext", "-enc", "UTF-8", pdf, "-"])
        if "pages" in expected:
            require(text.count("\f") == expected["pages"], f"expected {expected['pages']} pages")
            if not expected["text"]:
                require(not text.strip(), "expected a text-free deck")
        require_text(text, expected.get("pdf_text", expected["text"]))


def require_text(text, expected):
    normalized = " ".join(text.split())
    missing = [needle for needle in expected if " ".join(needle.split()) not in normalized]
    require(not missing, f"missing text: {missing!r}")


def check_recalculation(source, resaved, expected):
    original = workbook(source)
    preflight(resaved)
    actual = workbook(resaved)
    overrides = expected.get("formulas", {})
    checked = 0
    errors = []
    seen = set()
    for name, cells in original.items():
        for address, (kind, cached, formula) in cells.items():
            if not formula:
                continue
            key = f"{name}!{address}"
            seen.add(key)
            if cached is None:
                if key not in overrides:
                    errors.append(f"{key}: no cache or manifest formula expectation")
                    continue
                kind, cached = overrides[key]
            checked += 1
            got_kind, value, still_formula = actual.get(name, {}).get(address, (None, None, False))
            if not (still_formula and got_kind == kind and same_value(kind, cached, value)):
                errors.append(f"{key}: expected {(kind, cached)!r}, got {(got_kind, value)!r}"
                              f" (formula={still_formula})")
    errors.extend(f"{key}: expected formula missing from input" for key in sorted(overrides.keys() - seen))
    require(not errors, "\n".join(errors))
    return checked


def read_back(path, calamine):
    preflight(path)
    if path.suffix in SPREADSHEETS:
        run([calamine, path])
    elif path.suffix == ".docx":
        list(Document(path).iter_inner_content())
    else:
        for slide in Presentation(path).slides:
            list(slide.shapes)


def verify(source, expected, calamine, scratch):
    profile = scratch / "profile"
    (profile / "user").mkdir(parents=True)
    (profile / "user" / "registrymodifications.xcu").write_text(PROFILE, encoding="utf-8")
    results = {}
    checked = 0
    try:
        preflight(source)
    except (OSError, ValueError, zipfile.BadZipFile) as error:
        return {key: str(error) for key in CHECKS if key != "c" or source.suffix in SPREADSHEETS}, 0

    def attempt(key, action):
        try:
            result = action()
        except Exception as error:
            results[key] = f"{type(error).__name__}: {error}"
            return None
        results[key] = None
        return result

    pdf_filter = WRITER_PDF if source.suffix == ".docx" else "pdf"
    attempt("a", lambda: convert(profile, source, scratch / "pdf", pdf_filter))
    attempt("b", lambda: check_content(source, expected, profile, scratch))
    resaved = attempt("d", lambda: convert(
        profile, source, scratch / "resaved", f"{source.suffix[1:]}:{FILTERS[source.suffix]}"
    ))
    if resaved:
        attempt("d", lambda: read_back(resaved, calamine))
    if source.suffix in SPREADSHEETS:
        if resaved:
            checked = attempt("c", lambda: check_recalculation(source, resaved, expected)) or 0
        else:
            results["c"] = "recalculated workbook unavailable"
    return results, checked


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("bundle", type=Path)
    parser.add_argument("calamine", type=Path)
    parser.add_argument("failures", type=Path)
    args = parser.parse_args()
    bundle = args.bundle.resolve()
    manifest = json.loads((bundle / "expected.json").read_text(encoding="utf-8"))
    files = {
        path.relative_to(bundle).as_posix(): path
        for suffix in FILTERS
        for path in (bundle / ("xlsx" if suffix in SPREADSHEETS else suffix[1:])).glob(f"*{suffix}")
    }
    require(files and files.keys() == manifest.keys(),
            f"manifest coverage: missing={sorted(files.keys() - manifest.keys())}, "
            f"extra={sorted(manifest.keys() - files.keys())}")

    totals = Counter()
    failed_files = 0
    formulas = 0
    for name, source in sorted(files.items()):
        with tempfile.TemporaryDirectory(prefix="lo-check-") as tmp:
            scratch = Path(tmp)
            results, checked = verify(source, manifest[name], args.calamine.resolve(), scratch)
            formulas += checked
            errors = []
            for key, error in results.items():
                totals[key, error is None] += 1
                if error is not None:
                    errors.append(f"({key}) {error}")
            if errors:
                failed_files += 1
                report = "\n".join(errors)
                print(f"{name}:\n{report}", flush=True)
                dest = args.failures / name
                dest.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source, dest)
                dest.with_suffix(dest.suffix + ".log").write_text(report, encoding="utf-8")

    summary = [
        "## LibreOffice manual-check",
        "",
        f"{len(files)} files, {failed_files} failed. {formulas} formula values matched.",
        "",
        "| Check | Passed | Failed | N/A |",
        "|---|---:|---:|---:|",
    ]
    for key, label in CHECKS.items():
        passed, failed = totals[key, True], totals[key, False]
        summary.append(f"| ({key}) {label} | {passed} | {failed} | {len(files) - passed - failed} |")
    report = "\n".join(summary) + "\n"
    print(report)
    if path := os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(path, "a", encoding="utf-8") as file:
            file.write(report)
    return int(failed_files != 0)


if __name__ == "__main__":
    raise SystemExit(main())
