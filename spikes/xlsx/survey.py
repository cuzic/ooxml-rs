"""Survey .xlsx/.xlsm files for the shapes ADR-0001's fixture does not cover.

Read-only inspection of untrusted files: zipfile + xml.etree iterparse only,
never executes or extracts anything. Run with `python3 -I`:

    python3 -I spikes/xlsx/survey.py DIR [DIR...] > survey.jsonl

One JSON object per file on stdout. Files that are not zip packages
(e.g. CFB-encrypted) are reported with "error".
"""
import io
import json
import posixpath
import re
import sys
import zipfile
import xml.etree.ElementTree as ET
from pathlib import Path

SML = "http://schemas.openxmlformats.org/spreadsheetml/2006/main"
SML_STRICT = "http://purl.oclc.org/ooxml/spreadsheetml/main"
MC = "http://schemas.openxmlformats.org/markup-compatibility/2006"
XML = "http://www.w3.org/XML/1998/namespace"
REL = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
PKGREL = "http://schemas.openxmlformats.org/package/2006/relationships"
APP = "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"

PART_FEATURES = {
    "calcChain": re.compile(r"(^|/)calcChain\.xml$"),
    "sharedStrings": re.compile(r"(^|/)sharedStrings\.xml$"),
    "table": re.compile(r"^xl/tables/"),
    "pivotTable": re.compile(r"^xl/pivotTables/"),
    "pivotCache": re.compile(r"^xl/pivotCache/"),
    "chart": re.compile(r"^xl/charts/chart"),
    "chartEx": re.compile(r"^xl/charts/chartEx"),
    "drawing": re.compile(r"^xl/drawings/drawing"),
    "vmlDrawing": re.compile(r"\.vml$"),
    "comments": re.compile(r"^xl/comments"),
    "threadedComments": re.compile(r"^xl/threadedComments/"),
    "persons": re.compile(r"^xl/persons/"),
    "vba": re.compile(r"vbaProject\.bin$"),
    "externalLink": re.compile(r"^xl/externalLinks/"),
    "richData": re.compile(r"^xl/richData/"),
    "metadata": re.compile(r"^xl/metadata\.xml$"),
    "slicer": re.compile(r"^xl/slicers?(Caches)?/"),
    "timeline": re.compile(r"^xl/timeline"),
    "media": re.compile(r"^xl/media/"),
    "customXml": re.compile(r"^customXml/"),
    "chartsheet": re.compile(r"^xl/chartsheets/"),
    "macrosheet": re.compile(r"^xl/macrosheets/"),
    "dialogsheet": re.compile(r"^xl/dialogsheets/"),
    "queryTable": re.compile(r"^xl/queryTables/"),
    "connections": re.compile(r"^xl/connections\.xml$"),
    "customProps": re.compile(r"^docProps/custom\.xml$"),
    "printerSettings": re.compile(r"^xl/printerSettings/"),
    "activeX": re.compile(r"^xl/activeX/"),
    "ctrlProps": re.compile(r"^xl/ctrlProps/"),
    "embeddings": re.compile(r"^xl/embeddings/"),
}


def local(tag):
    return tag.rsplit("}", 1)[-1]


def ns(tag):
    return tag[1:].split("}", 1)[0] if tag.startswith("{") else ""


def rels_of(z, part):
    d, n = posixpath.split(part)
    rp = posixpath.join(d, "_rels", n + ".rels")
    out = {}
    if rp not in z.namelist():
        return out
    for e in ET.fromstring(z.read(rp)).iter(f"{{{PKGREL}}}Relationship"):
        tgt = e.get("Target", "")
        if e.get("TargetMode") != "External":
            tgt = posixpath.normpath(posixpath.join(d, tgt)) if not tgt.startswith("/") else tgt[1:]
        out[e.get("Id")] = (e.get("Type", "").rsplit("/", 1)[-1], tgt)
    return out


def scan_sheet(data, f):
    head = data[:4096].decode("utf-8", "replace")
    m = re.search(r"<(?:\w+:)?worksheet\b[^>]*>", head)
    if m and "Ignorable=" in m.group(0):
        f["mc_ignorable_root"] = True
    sheetdata_rows = 0
    for ev, el in ET.iterparse(io.BytesIO(data), events=("start", "end")):
        ln = local(el.tag)
        if ev == "start":
            continue
        # end events
        if ns(el.tag) == SML_STRICT:
            f["strict"] = True
        if ns(el.tag) not in (SML, SML_STRICT):
            # DrawingML anchors inside the sheet have their own <xdr:row>;
            # only SpreadsheetML elements count below, except MC.
            if ln == "AlternateContent":
                f["mc_AlternateContent"] += 1
            elif ln in ("conditionalFormatting", "dataValidation", "sparklineGroup", "slicerList"):
                f["x14_" + ln] = f.get("x14_" + ln, 0) + 1
            if el.get(f"{{{XML}}}space") == "preserve":
                f["xml_space_preserve"] += 1
            continue
        if ln == "f":
            f["formula"] += 1
            t = el.get("t")
            if t == "shared":
                f["f_shared"] += 1
            elif t == "array":
                f["f_array"] += 1
            elif t == "dataTable":
                f["f_dataTable"] += 1
            if el.text and "_xlfn." in el.text:
                f["f_xlfn"] += 1
        elif ln == "c":
            f["cells"] += 1
            if el.get("r") is None:
                f["c_no_r"] += 1
            t = el.get("t")
            if t == "inlineStr":
                f["inlineStr"] += 1
                if el.find(f"{{{SML}}}is/{{{SML}}}r") is not None:
                    f["inlineStr_rich"] += 1
            elif t == "e":
                f["c_error"] += 1
            elif t == "str":
                f["c_str"] += 1
            elif t == "d":
                f["c_date_iso"] += 1
            if el.get("cm") is not None:
                f["c_cm"] += 1
            if el.get("vm") is not None:
                f["c_vm"] += 1
            if len(el) == 0 and el.text is None:
                f["c_empty"] += 1
            el.clear()
        elif ln == "row":
            sheetdata_rows += 1
            if el.get("r") is None:
                f["row_no_r"] += 1
            if el.get("spans") is not None:
                f["row_spans"] += 1
            el.clear()
        elif ln == "sheetData":
            if sheetdata_rows == 0:
                f["empty_sheetData"] += 1
            sheetdata_rows = 0
        elif ln == "mergeCell":
            f["mergeCell"] += 1
        elif ln == "extLst":
            f["extLst"] += 1
        elif ln == "dimension":
            f["dimension"] += 1
        elif ln in ("conditionalFormatting", "dataValidation", "hyperlink",
                    "sparklineGroup", "autoFilter", "pane", "sheetProtection",
                    "pageSetup", "headerFooter", "legacyDrawing", "drawing",
                    "tablePart", "colBreaks", "rowBreaks", "cols", "picture",
                    "oleObjects", "controls", "printOptions", "sortState",
                    "phoneticPr", "rPh", "ignoredErrors"):
            f["el_" + ln] = f.get("el_" + ln, 0) + 1
        if el.get(f"{{{XML}}}space") == "preserve":
            f["xml_space_preserve"] += 1


def survey(path):
    f = {k: 0 for k in (
        "formula f_shared f_array f_dataTable f_xlfn cells c_no_r inlineStr "
        "inlineStr_rich c_error c_str c_date_iso c_cm c_vm c_empty row_no_r "
        "row_spans empty_sheetData mergeCell mc_AlternateContent extLst "
        "dimension xml_space_preserve sst_preserve sst_rich").split()}
    f["file"] = path.name
    f["bytes"] = path.stat().st_size
    try:
        z = zipfile.ZipFile(path)
    except Exception as e:  # noqa: BLE001 - untrusted input, report and move on
        f["error"] = f"not a zip: {type(e).__name__}"
        return f
    try:
        names = z.namelist()
        infos = z.infolist()
        f["parts"] = len(names)
        f["data_descriptor"] = sum(1 for i in infos if i.flag_bits & 0x08)
        f["stored"] = sum(1 for i in infos if i.compress_type == zipfile.ZIP_STORED)
        f["features"] = sorted(k for k, rx in PART_FEATURES.items() if any(rx.search(n) for n in names))
        if "docProps/app.xml" in names:
            app = ET.fromstring(z.read("docProps/app.xml"))
            a = app.find(f"{{{APP}}}Application")
            v = app.find(f"{{{APP}}}AppVersion")
            f["app"] = (a.text if a is not None else None, v.text if v is not None else None)
        root_rels = rels_of(z, "")
        wb = next((t for (ty, t) in root_rels.values() if ty == "officeDocument"), None)
        f["workbook_part"] = wb
        if wb is None or wb not in names:
            f["error"] = "no workbook part"
            return f
        wbx = ET.fromstring(z.read(wb))
        f["strict"] = ns(wbx.tag) == SML_STRICT
        wns = ns(wbx.tag)
        pr = wbx.find(f"{{{wns}}}workbookPr")
        f["date1904"] = pr is not None and pr.get("date1904") in ("1", "true")
        cp = wbx.find(f"{{{wns}}}calcPr")
        f["fullCalcOnLoad"] = cp is not None and cp.get("fullCalcOnLoad") in ("1", "true")
        f["definedNames"] = len(wbx.findall(f"{{{wns}}}definedNames/{{{wns}}}definedName"))
        wrels = rels_of(z, wb)
        sheets = []
        for s in wbx.findall(f"{{{wns}}}sheets/{{{wns}}}sheet"):
            rid = s.get(f"{{{REL}}}id") or s.get("{http://purl.oclc.org/ooxml/officeDocument/relationships}id")
            ty, tgt = wrels.get(rid, ("?", None))
            sheets.append((s.get("name"), ty, tgt, s.get("state")))
        f["sheets"] = [(n, ty, t) for n, ty, t, _ in sheets]
        f["hidden_sheets"] = sum(1 for *_, st in sheets if st in ("hidden", "veryHidden"))
        ws = [t for _, ty, t, _ in sheets if ty == "worksheet"]
        f["n_worksheets"] = len(ws)
        nums = [int(m.group(1)) for t in ws if t and (m := re.search(r"sheet(\d+)\.xml$", t))]
        f["sheets_reordered"] = len(nums) == len(ws) and nums != sorted(nums)
        for t in ws:
            if t in names:
                scan_sheet(z.read(t), f)
        sst = next((t for (ty, t) in wrels.values() if ty == "sharedStrings"), None)
        if sst and sst in names:
            data = z.read(sst)
            f["sst_preserve"] = data.count(b'xml:space="preserve"')
            f["sst_rich"] = len(re.findall(rb"<(?:\w+:)?si>\s*<(?:\w+:)?r>", data))
    except Exception as e:  # noqa: BLE001
        f["error"] = f"{type(e).__name__}: {e}"[:200]
    return f


def main(argv):
    for d in argv[1:]:
        for p in sorted(Path(d).iterdir()):
            if p.suffix.lower() in (".xlsx", ".xlsm"):
                print(json.dumps(survey(p), ensure_ascii=False))


if __name__ == "__main__":
    main(sys.argv)
