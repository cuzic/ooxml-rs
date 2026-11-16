#!/usr/bin/env python3
"""ADR-0014 spike: range features in existing workbooks (parse only).

Reads every .xlsx/.xlsm under the given directories with `zipfile` and
`xml.etree.ElementTree` (untrusted input: nothing is executed or extracted),
and prints one JSON line per package plus a summary table on stderr-free
stdout with `--summary`. Run with `python3 -I` from the repository root:

    python3 -I spikes/xlsx-phase4/range_survey.py --summary DIR...
"""
import json
import os
import re
import sys
import zipfile
import xml.etree.ElementTree as ET

S = "{http://schemas.openxmlformats.org/spreadsheetml/2006/main}"
R = "{http://schemas.openxmlformats.org/officeDocument/2006/relationships}"
PR = "{http://schemas.openxmlformats.org/package/2006/relationships}"
X14 = "{http://schemas.microsoft.com/office/spreadsheetml/2009/9/main}"
XM = "{http://schemas.microsoft.com/office/excel/2006/main}"
REF = re.compile(r"^\$?([A-Z]{1,3})\$?(\d+)$")


def col_num(s):
    n = 0
    for ch in s:
        n = n * 26 + ord(ch) - 64
    return n


def rect(ref):
    a, _, b = ref.partition(":")
    b = b or a
    ma, mb = REF.match(a), REF.match(b)
    if not (ma and mb):
        return None
    c1, r1, c2, r2 = col_num(ma[1]), int(ma[2]), col_num(mb[1]), int(mb[2])
    return (min(r1, r2), min(c1, c2), max(r1, r2), max(c1, c2))


def overlaps(a, b):
    return a[0] <= b[2] and b[0] <= a[2] and a[1] <= b[3] and b[1] <= a[3]


def sheets_of(z):
    wb = ET.fromstring(z.read("xl/workbook.xml"))
    rels = ET.fromstring(z.read("xl/_rels/workbook.xml.rels"))
    t = {r.get("Id"): r.get("Target") for r in rels.findall(PR + "Relationship")}
    out = []
    for s in wb.find(S + "sheets").findall(S + "sheet"):
        target = (t.get(s.get(R + "id")) or "").lstrip("/")
        part = target if target.startswith("xl/") else "xl/" + target
        out.append((s.get("name"), part))
    return wb, out


def survey(path):
    z = zipfile.ZipFile(path)
    names = set(z.namelist())
    wb, sheets = sheets_of(z)
    f = {"file": os.path.basename(path)}
    c = lambda k, n=1: f.__setitem__(k, f.get(k, 0) + n)
    dn = wb.find(S + "definedNames")
    if dn is not None:
        for d in dn.findall(S + "definedName"):
            n = d.get("name", "")
            if n.startswith("_xlnm."):
                c("name:" + n)
    if wb.find(S + "workbookProtection") is not None:
        wp = wb.find(S + "workbookProtection")
        c("wbprot:" + ("sha" if wp.get("workbookAlgorithmName") else "legacy" if wp.get("workbookPassword") else "flags"))
    for sname, part in sheets:
        if part not in names:
            continue
        try:
            ws = ET.fromstring(z.read(part))
        except ET.ParseError:
            c("parse_error")
            continue
        if not ws.tag.endswith("worksheet"):
            continue
        anchors = {}
        sd = ws.find(S + "sheetData")
        values = {}
        if sd is not None:
            for row in sd.findall(S + "row"):
                if row.get("ht") is not None:
                    c("row:ht")
                if row.get("hidden") in ("1", "true"):
                    c("row:hidden")
                if row.get("outlineLevel"):
                    c("row:outline")
                for cell in row.findall(S + "c"):
                    if cell.find(S + "v") is not None or cell.find(S + "is") is not None or cell.find(S + "f") is not None:
                        m = REF.match(cell.get("r") or "")
                        if m:
                            values[(int(m[2]), col_num(m[1]))] = True
        mc = ws.find(S + "mergeCells")
        if mc is not None:
            rs = [rect(m.get("ref", "")) for m in mc.findall(S + "mergeCell")]
            rs = [r for r in rs if r]
            c("merge", len(rs))
            c("sheets_with_merge")
            if any(overlaps(a, b) for i, a in enumerate(rs) for b in rs[i + 1:]):
                c("merge_overlap")
            for r in rs:
                for (row, col) in values:
                    if r[0] <= row <= r[2] and r[1] <= col <= r[3] and (row, col) != (r[0], r[1]):
                        c("merge_nonanchor_value")
        hl = ws.find(S + "hyperlinks")
        if hl is not None:
            for h in hl.findall(S + "hyperlink"):
                c("hyperlink:external" if h.get(R + "id") else "hyperlink:internal" if h.get("location") is not None else "hyperlink:other")
                if h.get(R + "id") and h.get("location") is not None:
                    c("hyperlink:both")
        dv = ws.find(S + "dataValidations")
        if dv is not None:
            for d in dv.findall(S + "dataValidation"):
                c("dv:" + (d.get("type") or "none"))
                f1 = d.find(S + "formula1")
                if f1 is not None and f1.text and "!" in f1.text:
                    c("dv:list_other_sheet")
                if f1 is not None and f1.text and f1.text.startswith('"') and len(f1.text) > 257:
                    c("dv:inline_over_255")
        cf_prios = []
        cf_ids = set()
        for cf in ws.findall(S + "conditionalFormatting"):
            c("cf")
            for rule in cf.findall(S + "cfRule"):
                c("cf:" + (rule.get("type") or "?"))
                if rule.get("priority"):
                    cf_prios.append(int(rule.get("priority")))
                if rule.get("dxfId") is not None:
                    c("cf:dxf")
                for idn in rule.iter(X14 + "id"):
                    cf_ids.add((idn.text or "").strip())
        if len(cf_prios) != len(set(cf_prios)):
            c("cf:dup_priority")
        x14_ids = set()
        ext = ws.find(S + "extLst")
        if ext is not None:
            for e in ext.findall(S + "ext"):
                uri = (e.get("uri") or "").upper()
                c("ext:" + uri)
            for r in ext.iter(X14 + "cfRule"):
                c("x14:cfRule")
                if r.get("id"):
                    x14_ids.add(r.get("id"))
            for d in ext.iter(X14 + "dataValidation"):
                c("x14:dataValidation")
                fx = d.find(X14 + "formula1")
                if fx is not None:
                    t = "".join(fx.itertext())
                    if "!" in t:
                        c("x14:dv_other_sheet")
        if cf_ids:
            c("cf:x14_linked")
            if not cf_ids <= x14_ids:
                c("cf:x14_dangling_id")
        if ws.find(S + "autoFilter") is not None:
            c("autofilter")
            if ws.find(S + "autoFilter").find(S + "filterColumn") is not None:
                c("autofilter:filterColumn")
        if ws.find(S + "sortState") is not None or (ws.find(S + "autoFilter") is not None and ws.find(S + "autoFilter").find(S + "sortState") is not None):
            c("sortState")
        svs = ws.find(S + "sheetViews")
        if svs is not None:
            views = svs.findall(S + "sheetView")
            if len(views) > 1:
                c("sheetViews>1")
            for v in views:
                p = v.find(S + "pane")
                if p is not None:
                    c("pane:" + (p.get("state") or "split"))
        cols = ws.find(S + "cols")
        if cols is not None:
            for col in cols.findall(S + "col"):
                c("col")
                if col.get("min") != col.get("max"):
                    c("col:range")
                if col.get("hidden") in ("1", "true"):
                    c("col:hidden")
                if col.get("outlineLevel"):
                    c("col:outline")
        for tag in ("pageMargins", "pageSetup", "printOptions", "headerFooter", "rowBreaks", "colBreaks"):
            if ws.find(S + tag) is not None:
                c(tag)
        sp = ws.find(S + "sheetProtection")
        if sp is not None:
            c("sheetprot:" + ("sha" if sp.get("algorithmName") else "legacy" if sp.get("password") else "flags"))
            if sp.get("algorithmName"):
                c("sheetprot_alg:" + sp.get("algorithmName"))
        for el in ws:
            if el.tag.endswith("}AlternateContent"):
                c("ws_alternate_content")
    return f


def main():
    args = sys.argv[1:]
    summary = "--summary" in args
    dirs = [a for a in args if a != "--summary"]
    files = []
    for d in dirs:
        for root, _, fs in os.walk(d):
            files += [os.path.join(root, x) for x in fs if x.lower().endswith((".xlsx", ".xlsm"))]
    totals, nfiles, ok = {}, {}, 0
    for p in sorted(files):
        try:
            f = survey(p)
        except Exception as e:  # untrusted input: count, don't crash
            print(json.dumps({"file": os.path.basename(p), "error": type(e).__name__}))
            continue
        ok += 1
        print(json.dumps(f, ensure_ascii=False))
        for k, v in f.items():
            if k != "file":
                totals[k] = totals.get(k, 0) + v
                nfiles[k] = nfiles.get(k, 0) + 1
    if summary:
        print(f"# {ok} readable packages of {len(files)}")
        print("# feature\tfiles\toccurrences")
        for k in sorted(totals):
            print(f"# {k}\t{nfiles[k]}\t{totals[k]}")


if __name__ == "__main__":
    main()
